//! Presentation stability: the same agent on the same window, shown two ways,
//! with every decision mapped back to the source symbols and compared.
//!
//! Two pairs are run:
//!
//! 1. Unmasked against masked ([`Dataset::masked_view`]): names, dates and
//!    price level change, returns do not. An agent that reads only the
//!    observation's returns decides identically. A gap is evidence that the
//!    agent uses a ticker, a date or a level, which is how recall of price
//!    history enters.
//! 2. Masked in canonical symbol order against masked in reversed order: only
//!    the order of the symbols in the observation changes. A gap is position
//!    bias.
//!
//! The scores of the reordered pair can differ in the last bits for an agent
//! that decides identically, because the book is summed in another symbol
//! order, and under execution noise or seeded slippage the per-symbol draws are
//! assigned by position. Decisions are compared exactly; read score deltas of
//! the reordered pair with that in mind.
//!
//! This is separate from `sharpebench-harness`'s decision stability, which
//! compares replicates that saw byte-equal observations; a masked or reordered
//! presentation never meets its original there. The report is rank-neutral.

use std::collections::BTreeMap;

use sharpebench_protocol::Decision;

use crate::agent::Agent;
use crate::contamination::window_sharpe;
use crate::costs::CostModel;
use crate::data::{Dataset, MaskError};
use crate::engine::{run_backtest, Window};

/// Weights closer than this are the same decision.
const WEIGHT_TOLERANCE: f64 = 1e-12;

/// One paired comparison.
#[derive(Clone, Debug, PartialEq)]
pub struct PresentationComparison {
    /// Decisions compared (the window's steps).
    pub steps: usize,
    /// Steps whose mapped target-weight orders differ.
    pub disagreeing_steps: usize,
    /// First disagreeing step, if any. Later steps can differ only because the
    /// books diverged there.
    pub first_disagreement: Option<usize>,
    /// Mean over steps of the largest absolute target-weight gap on any symbol.
    pub mean_max_weight_gap: f64,
    /// Per-bar Sharpe of each arm; `None` when it cannot be computed.
    pub baseline_sharpe: Option<f64>,
    pub variant_sharpe: Option<f64>,
}

/// Both comparisons for one window.
#[derive(Clone, Debug, PartialEq)]
pub struct PresentationReport {
    pub unmasked_vs_masked: PresentationComparison,
    pub canonical_vs_reordered: PresentationComparison,
}

/// Records each decision the wrapped agent makes.
struct Recording<'a> {
    inner: &'a mut dyn Agent,
    log: Vec<Decision>,
}

impl Agent for Recording<'_> {
    fn decide(&mut self, obs: &sharpebench_protocol::MarketObservation) -> Decision {
        let d = self.inner.decide(obs);
        self.log.push(d.clone());
        d
    }
}

/// Run one arm and return its decisions (as `source symbol -> weight` maps)
/// and its returns. A symbol the map does not know keeps its own name with an
/// `unmapped:` prefix, so it can never coincide with a source symbol.
fn run_arm(
    data: &Dataset,
    to_source: &BTreeMap<String, String>,
    agent: &mut dyn Agent,
    window: Window,
    seed: u64,
    costs: CostModel,
) -> (Vec<BTreeMap<String, f64>>, Vec<f64>) {
    let mut rec = Recording {
        inner: agent,
        log: Vec::new(),
    };
    let run = run_backtest(data, &mut rec, window, seed, costs);
    let mapped = rec
        .log
        .iter()
        .map(|d| {
            d.orders
                .iter()
                .map(|o| {
                    let name = to_source
                        .get(&o.symbol)
                        .cloned()
                        .unwrap_or_else(|| format!("unmapped:{}", o.symbol));
                    (name, o.target_weight)
                })
                .collect()
        })
        .collect();
    (mapped, run.returns)
}

fn compare(
    base: &(Vec<BTreeMap<String, f64>>, Vec<f64>),
    variant: &(Vec<BTreeMap<String, f64>>, Vec<f64>),
) -> PresentationComparison {
    let steps = base.0.len().max(variant.0.len());
    let empty = BTreeMap::new();
    let mut disagreeing = 0;
    let mut first = None;
    let mut gap_sum = 0.0;
    // inv: disagreeing, first and gap_sum cover steps [0, t).
    for t in 0..steps {
        let a = base.0.get(t).unwrap_or(&empty);
        let b = variant.0.get(t).unwrap_or(&empty);
        let keys: std::collections::BTreeSet<&String> = a.keys().chain(b.keys()).collect();
        let mut gap: f64 = 0.0;
        let mut differs = false;
        for k in keys {
            match (a.get(k), b.get(k)) {
                (Some(x), Some(y)) => gap = gap.max((x - y).abs()),
                // An order on one side and none on the other is a different
                // decision even at the same weight: "no order" keeps the book.
                (Some(x), None) | (None, Some(x)) => {
                    differs = true;
                    gap = gap.max(x.abs());
                }
                (None, None) => {}
            }
        }
        if differs || gap > WEIGHT_TOLERANCE {
            disagreeing += 1;
            first.get_or_insert(t);
        }
        gap_sum += gap;
    }
    PresentationComparison {
        steps,
        disagreeing_steps: disagreeing,
        first_disagreement: first,
        mean_max_weight_gap: if steps == 0 {
            0.0
        } else {
            gap_sum / steps as f64
        },
        baseline_sharpe: window_sharpe(&base.1),
        variant_sharpe: window_sharpe(&variant.1),
    }
}

/// Run `make_agent()` (a fresh agent per arm) on the window in the four
/// presentations and compare the two pairs.
pub fn presentation_report(
    data: &Dataset,
    window: Window,
    seed: u64,
    costs: CostModel,
    make_agent: &mut dyn FnMut() -> Box<dyn Agent>,
) -> Result<PresentationReport, MaskError> {
    let identity: BTreeMap<String, String> =
        data.symbols().into_iter().map(|s| (s.clone(), s)).collect();
    let view = data.masked_view()?;
    let masked_map: BTreeMap<String, String> = view
        .symbols
        .iter()
        .map(|m| (m.opaque.clone(), m.source.clone()))
        .collect();

    // Reverse the observation order by renaming: the k-th masked symbol takes
    // the id the (n-1-k)-th had, so a sorted map walks them backwards.
    let n = view.symbols.len();
    let mut reordered = view.dataset.clone();
    let mut reordered_map = BTreeMap::new();
    let rename = |k: usize| format!("ASSET_{:03}", n - 1 - k);
    reordered.closes = view
        .symbols
        .iter()
        .enumerate()
        .map(|(k, m)| (rename(k), view.dataset.closes[&m.opaque].clone()))
        .collect();
    reordered.dividends = view
        .symbols
        .iter()
        .enumerate()
        .filter_map(|(k, m)| {
            view.dataset
                .dividends
                .get(&m.opaque)
                .map(|d| (rename(k), d.clone()))
        })
        .collect();
    if let Some(sheet) = reordered.marks.as_mut() {
        sheet.symbols = view
            .symbols
            .iter()
            .enumerate()
            .filter_map(|(k, m)| {
                view.dataset
                    .marks
                    .as_ref()
                    .and_then(|s| s.symbols.get(&m.opaque))
                    .map(|mark| (rename(k), mark.clone()))
            })
            .collect();
    }
    for (k, m) in view.symbols.iter().enumerate() {
        reordered_map.insert(rename(k), m.source.clone());
    }

    let plain = run_arm(data, &identity, make_agent().as_mut(), window, seed, costs);
    let masked = run_arm(
        &view.dataset,
        &masked_map,
        make_agent().as_mut(),
        window,
        seed,
        costs,
    );
    let reversed = run_arm(
        &reordered,
        &reordered_map,
        make_agent().as_mut(),
        window,
        seed,
        costs,
    );
    Ok(PresentationReport {
        unmasked_vs_masked: compare(&plain, &masked),
        canonical_vs_reordered: compare(&masked, &reversed),
    })
}
