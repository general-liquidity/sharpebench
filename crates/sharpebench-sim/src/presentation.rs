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
//! [`presentation_report`] runs those two pairs. [`presentation_suite`] runs
//! them with the full mask split into its handles: unmasked against each
//! [`BlindingLevel`] (tickers only, dates only, levels only, full), the
//! reordered pair against the full mask, and, when an offset is declared,
//! unmasked against the same bars under a shifted calendar
//! ([`Dataset::date_shifted`]). Its arms run through a callback, so a host
//! process, a sandboxed image or a gateway-attached entrant is driven the same
//! way, and a failed arm refuses the suite instead of being dropped.
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

use serde::Serialize;
use sharpebench_protocol::Decision;

use crate::agent::Agent;
use crate::blinding::{BlindingLevel, DateShiftError};
use crate::contamination::window_sharpe;
use crate::costs::CostModel;
use crate::data::{Dataset, MaskError, MaskedView};
use crate::engine::{run_backtest, Window};

/// Weights closer than this are the same decision.
const WEIGHT_TOLERANCE: f64 = 1e-12;

/// One paired comparison.
#[derive(Clone, Debug, PartialEq, serde::Serialize)]
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

/// One arm's raw outcome: the decisions in the presentation's own symbol
/// names, and the per-bar returns.
#[derive(Clone, Debug, PartialEq)]
pub struct ArmRun {
    pub decisions: Vec<Decision>,
    pub returns: Vec<f64>,
}

/// Run `agent` on one presentation and record what it decided.
pub fn record_run(
    data: &Dataset,
    agent: &mut dyn Agent,
    window: Window,
    seed: u64,
    costs: CostModel,
) -> ArmRun {
    let mut rec = Recording {
        inner: agent,
        log: Vec::new(),
    };
    let run = run_backtest(data, &mut rec, window, seed, costs);
    ArmRun {
        decisions: rec.log,
        returns: run.returns,
    }
}

/// Decisions as `source symbol -> weight` maps, and the returns.
type MappedArm = (Vec<BTreeMap<String, f64>>, Vec<f64>);

/// Map an arm's decisions back to the source symbols. A symbol the map does
/// not know keeps its own name with an `unmapped:` prefix, so it can never
/// coincide with a source symbol.
fn map_arm(arm: ArmRun, to_source: &BTreeMap<String, String>) -> MappedArm {
    let mapped = arm
        .decisions
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
    (mapped, arm.returns)
}

fn run_arm(
    data: &Dataset,
    to_source: &BTreeMap<String, String>,
    agent: &mut dyn Agent,
    window: Window,
    seed: u64,
    costs: CostModel,
) -> MappedArm {
    map_arm(record_run(data, agent, window, seed, costs), to_source)
}

fn compare(base: &MappedArm, variant: &MappedArm) -> PresentationComparison {
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

/// The masked view with its symbols listed in reverse: the k-th masked symbol
/// takes the id the (n-1-k)-th had, so a sorted map walks them backwards. Only
/// the order of the symbols in the observation changes.
fn reversed(view: &MaskedView) -> (Dataset, BTreeMap<String, String>) {
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
    (reordered, reordered_map)
}

fn source_map(view: &MaskedView) -> BTreeMap<String, String> {
    view.symbols
        .iter()
        .map(|m| (m.opaque.clone(), m.source.clone()))
        .collect()
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
    let masked_map = source_map(&view);
    let (reordered, reordered_map) = reversed(&view);

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

/// One way of showing the window to the agent.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Presentation {
    /// The source dataset as it is.
    Unmasked,
    /// A [`Dataset::blinded_view`].
    Blinded { level: BlindingLevel },
    /// The full mask with the symbols listed in reverse order.
    Reordered,
    /// The source dataset under a calendar moved by `offset_days`.
    DateShifted { offset_days: i64 },
}

/// What the suite asks one arm to run.
pub struct ArmRequest<'a> {
    pub presentation: Presentation,
    pub data: &'a Dataset,
    pub window: Window,
    pub seed: u64,
    pub costs: CostModel,
}

/// The declared presentation suite. Every blinding level is compared against
/// the unmasked arm, and the reordered arm against the full mask. A date-shift
/// pair is added when an offset is declared.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SuiteSpec {
    pub date_shift_days: Option<i64>,
}

/// One compared pair, with the decisions mapped back to the source symbols.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct PresentationPair {
    pub baseline: Presentation,
    pub variant: Presentation,
    pub comparison: PresentationComparison,
}

/// Every pair of the suite, in a fixed order: tickers-only, dates-only,
/// levels-only and full blinding against unmasked, then reordered against
/// full, then the date shift against unmasked when one is declared.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct PresentationSuite {
    pub pairs: Vec<PresentationPair>,
}

impl PresentationSuite {
    /// The comparison of `variant` against its baseline, if the suite ran it.
    pub fn pair(&self, variant: Presentation) -> Option<&PresentationComparison> {
        self.pairs
            .iter()
            .find(|p| p.variant == variant)
            .map(|p| &p.comparison)
    }
}

/// Why the suite has no result. An arm that fails refuses the whole suite: a
/// pair with a missing arm would otherwise read as agreement.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(tag = "reason", rename_all = "snake_case")]
pub enum SuiteError<E> {
    Mask {
        error: String,
    },
    DateShift {
        error: DateShiftError,
    },
    Arm {
        presentation: Presentation,
        error: E,
    },
}

/// The blinding levels in suite order. The full mask is last.
pub const BLINDING_LEVELS: [BlindingLevel; 4] = [
    BlindingLevel::TickersOnly,
    BlindingLevel::DatesOnly,
    BlindingLevel::LevelsOnly,
    BlindingLevel::Full,
];

/// Run the presentation suite. `run_arm` runs a fresh agent on the presentation
/// it is handed and returns its decisions in that presentation's own symbol
/// names; the suite maps them back to the source symbols and compares
/// decisions exactly. External entrants, sandboxed images and gateway-attached
/// entrants run through the same callback, and report a failed arm as `E`.
pub fn presentation_suite<E>(
    data: &Dataset,
    window: Window,
    seed: u64,
    costs: CostModel,
    spec: SuiteSpec,
    run_arm: &mut dyn FnMut(&ArmRequest<'_>) -> Result<ArmRun, E>,
) -> Result<PresentationSuite, SuiteError<E>> {
    let mask = |e: MaskError| SuiteError::Mask {
        error: e.to_string(),
    };
    // Build every presentation before any arm runs, so a refusal costs no run.
    let mut views = Vec::with_capacity(BLINDING_LEVELS.len());
    for level in BLINDING_LEVELS {
        views.push((level, data.blinded_view(level).map_err(mask)?));
    }
    let (reordered, reordered_map) = reversed(&views[BLINDING_LEVELS.len() - 1].1);
    let shifted = match spec.date_shift_days {
        Some(offset) => Some((
            offset,
            data.date_shifted(offset)
                .map_err(|error| SuiteError::DateShift { error })?,
        )),
        None => None,
    };

    let identity: BTreeMap<String, String> =
        data.symbols().into_iter().map(|s| (s.clone(), s)).collect();
    let mut arm = |presentation: Presentation,
                   shown: &Dataset,
                   map: &BTreeMap<String, String>|
     -> Result<MappedArm, SuiteError<E>> {
        let request = ArmRequest {
            presentation,
            data: shown,
            window,
            seed,
            costs,
        };
        run_arm(&request)
            .map(|run| map_arm(run, map))
            .map_err(|error| SuiteError::Arm {
                presentation,
                error,
            })
    };

    let plain = arm(Presentation::Unmasked, data, &identity)?;
    let mut pairs = Vec::new();
    let mut full_arm = None;
    for (level, view) in &views {
        let variant = Presentation::Blinded { level: *level };
        let run = arm(variant, &view.dataset, &source_map(view))?;
        pairs.push(PresentationPair {
            baseline: Presentation::Unmasked,
            variant,
            comparison: compare(&plain, &run),
        });
        if *level == BlindingLevel::Full {
            full_arm = Some(run);
        }
    }
    let full_arm = full_arm.expect("the full level is one of the blinding levels run");
    let reversed_arm = arm(Presentation::Reordered, &reordered, &reordered_map)?;
    pairs.push(PresentationPair {
        baseline: Presentation::Blinded {
            level: BlindingLevel::Full,
        },
        variant: Presentation::Reordered,
        comparison: compare(&full_arm, &reversed_arm),
    });
    if let Some((offset_days, shifted)) = &shifted {
        let variant = Presentation::DateShifted {
            offset_days: *offset_days,
        };
        let run = arm(variant, shifted, &identity)?;
        pairs.push(PresentationPair {
            baseline: Presentation::Unmasked,
            variant,
            comparison: compare(&plain, &run),
        });
    }
    Ok(PresentationSuite { pairs })
}
