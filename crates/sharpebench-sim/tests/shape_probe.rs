//! Offline validation of the shape-level contamination probe. No model is
//! called. A planted agent that memorized the real paths must be convicted;
//! agents that read only texture, or nothing, must not be.

use sharpebench_protocol::{Action, Decision, MarketObservation, Order};
use sharpebench_sim::windows::walk_forward;
use sharpebench_sim::{
    build_twin, run_backtest, shape_probe, window_sharpe, Agent, BuyAndHold, CostModel,
    CostProfile, Dataset, Momentum, PairedScore, ProbeConfig, ShapeProbeReport, TwinPair, TwinSpec,
};

const WARMUP: usize = 20;

fn relatives(series: &[f64]) -> Vec<f64> {
    series.windows(2).map(|w| w[1] / w[0]).collect()
}

/// What the memorizer remembers of one real window: the bar relatives of the
/// history it will first be shown, and every symbol's relatives afterwards.
#[derive(Clone)]
struct Memory {
    first_history: Vec<f64>,
    next: Vec<Vec<f64>>,
}

/// Recognises a remembered window from the shape of its first observation,
/// then trades on what it remembers happens next. Masking does not stop it,
/// because masked relatives equal the source's bit for bit. When it does not
/// recognise the window it holds an equal-weight book.
struct Memorizer {
    library: Vec<Memory>,
    found: Option<usize>,
    step: usize,
}

impl Agent for Memorizer {
    fn decide(&mut self, obs: &MarketObservation) -> Decision {
        if self.step == 0 {
            let seen = relatives(&obs.symbols[0].close_history);
            self.found = self.library.iter().position(|m| m.first_history == seen);
        }
        let n = obs.symbols.len();
        let weights: Vec<f64> = match self.found {
            Some(w) => {
                // `next[s][k]` is symbol s's relative from this bar's close to
                // the next one: the move a fill at this close earns.
                let mem = &self.library[w];
                let best = (0..n)
                    .map(|s| (s, mem.next[s].get(self.step).copied().unwrap_or(1.0)))
                    .max_by(|a, b| a.1.total_cmp(&b.1))
                    .unwrap();
                (0..n)
                    .map(|s| {
                        if s == best.0 && best.1 > 1.0 {
                            1.0
                        } else {
                            0.0
                        }
                    })
                    .collect()
            }
            None => vec![1.0 / n as f64; n],
        };
        self.step += 1;
        Decision {
            orders: obs
                .symbols
                .iter()
                .zip(weights)
                .map(|(s, w)| Order {
                    symbol: s.symbol.clone(),
                    action: if w > 0.0 { Action::Buy } else { Action::Close },
                    target_weight: w,
                    confidence: None,
                    rationale: String::new(),
                })
                .collect(),
            reasoning: String::new(),
            cost: None,
        }
    }
}

fn pairs() -> Vec<TwinPair> {
    let source = Dataset::synthetic(3, WARMUP + 12 * 40, 2026);
    let spec = TwinSpec::default();
    walk_forward(source.len(), WARMUP, 40, 40)
        .into_iter()
        .enumerate()
        .map(|(i, w)| build_twin(&source, w, &spec, 1000 + i as u64).unwrap())
        .collect()
}

fn library(pairs: &[TwinPair]) -> Vec<Memory> {
    pairs
        .iter()
        .map(|p| {
            let first = p.window.start;
            let lookback_start = first + 1 - WARMUP.min(first + 1);
            let sym0 = p.real.closes.values().next().unwrap();
            Memory {
                first_history: relatives(&sym0[lookback_start..=first]),
                next: p
                    .real
                    .closes
                    .values()
                    .map(|v| relatives(&v[first..]))
                    .collect(),
            }
        })
        .collect()
}

/// Frictionless execution: the probe measures recall, and the planted
/// memorizer switches its whole book most bars, which the default impact
/// charge would turn into a loss unrelated to what it remembers.
fn probe(pairs: &[TwinPair], make: &dyn Fn() -> Box<dyn Agent>) -> ShapeProbeReport {
    let costs = CostProfile::None.resolve().costs;
    let scores: Vec<PairedScore> = pairs
        .iter()
        .map(|p| PairedScore {
            real: window_sharpe(
                &run_backtest(&p.real, make().as_mut(), p.window, 0, costs).returns,
            ),
            twin: window_sharpe(
                &run_backtest(&p.twin, make().as_mut(), p.window, 0, costs).returns,
            ),
        })
        .collect();
    shape_probe(&scores, ProbeConfig::default()).unwrap()
}

#[test]
fn a_memorizer_is_convicted_and_texture_readers_are_cleared() {
    let pairs = pairs();
    assert_eq!(pairs.len(), 12);
    let lib = library(&pairs);

    let memorizer = probe(&pairs, &|| {
        Box::new(Memorizer {
            library: lib.clone(),
            found: None,
            step: 0,
        })
    });
    let momentum = probe(&pairs, &|| Box::new(Momentum::default()));
    let hold = probe(&pairs, &|| Box::new(BuyAndHold));
    eprintln!("memorizer {memorizer:?}\nmomentum {momentum:?}\nbuy-and-hold {hold:?}");

    assert!(memorizer.memorization, "{memorizer:?}");
    assert!(
        memorizer.edges.iter().all(|e| *e > 0.0),
        "recall pays on every window"
    );
    assert!(!momentum.memorization, "{momentum:?}");
    assert!(!hold.memorization, "{hold:?}");
}

#[test]
fn the_memorizer_recognises_every_real_window_and_no_twin() {
    let pairs = pairs();
    let lib = library(&pairs);
    for (i, p) in pairs.iter().enumerate() {
        for (data, expect) in [(&p.real, Some(i)), (&p.twin, None)] {
            let mut agent = Memorizer {
                library: lib.clone(),
                found: None,
                step: 0,
            };
            run_backtest(data, &mut agent, p.window, 0, CostModel::default());
            assert_eq!(agent.found, expect, "window {i}");
        }
    }
}
