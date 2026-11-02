//! Offline validation of the presentation-stability report with planted
//! agents. No model is called.

use sharpebench_protocol::{Action, Decision, MarketObservation, Order};
use sharpebench_sim::{
    presentation_report, presentation_suite, record_run, Agent, ArmRequest, BlindingLevel,
    BuyAndHold, CostProfile, Dataset, DateShiftError, Momentum, Presentation, PresentationSuite,
    SuiteError, SuiteSpec, Window,
};

fn frozen(name: &str) -> Dataset {
    let path = format!("{}/../../data/{name}", env!("CARGO_MANIFEST_DIR"));
    Dataset::from_csv_file(&path).unwrap()
}

fn decide_by(obs: &MarketObservation, pick: impl Fn(usize, &str, f64) -> f64) -> Decision {
    Decision {
        orders: obs
            .symbols
            .iter()
            .enumerate()
            .map(|(i, s)| {
                let w = pick(i, &s.symbol, *s.close_history.last().unwrap());
                Order {
                    symbol: s.symbol.clone(),
                    action: if w > 0.0 { Action::Buy } else { Action::Close },
                    target_weight: w,
                    confidence: None,
                    rationale: String::new(),
                }
            })
            .collect(),
        reasoning: String::new(),
        cost: None,
    }
}

/// Position bias: always buys whatever is listed first.
struct FirstListed;
impl Agent for FirstListed {
    fn decide(&mut self, obs: &MarketObservation) -> Decision {
        decide_by(obs, |i, _, _| if i == 0 { 1.0 } else { 0.0 })
    }
}

/// Recalls a ticker: buys SPX when it can see the name, else holds evenly.
struct TickerRecall;
impl Agent for TickerRecall {
    fn decide(&mut self, obs: &MarketObservation) -> Decision {
        let named = obs.symbols.iter().any(|s| s.symbol == "SPX");
        let n = obs.symbols.len() as f64;
        decide_by(obs, |_, sym, _| match (named, sym == "SPX") {
            (true, true) => 1.0,
            (true, false) => 0.0,
            (false, _) => 1.0 / n,
        })
    }
}

/// Reads the price level: buys anything quoted above 1000.
struct LevelReader;
impl Agent for LevelReader {
    fn decide(&mut self, obs: &MarketObservation) -> Decision {
        decide_by(obs, |_, _, px| if px > 1000.0 { 0.3 } else { 0.0 })
    }
}

fn report(make: &mut dyn FnMut() -> Box<dyn Agent>) -> sharpebench_sim::PresentationReport {
    let data = frozen("us-indices-1d.csv");
    let costs = CostProfile::None.resolve().costs;
    presentation_report(
        &data,
        Window {
            start: 30,
            end: 230,
        },
        0,
        costs,
        make,
    )
    .unwrap()
}

#[test]
fn observation_only_agents_decide_identically_in_every_presentation() {
    for (name, mut make) in [
        (
            "momentum",
            Box::new(|| Box::new(Momentum::default()) as Box<dyn Agent>)
                as Box<dyn FnMut() -> Box<dyn Agent>>,
        ),
        (
            "buy-and-hold",
            Box::new(|| Box::new(BuyAndHold) as Box<dyn Agent>),
        ),
    ] {
        let r = report(&mut make);
        for c in [&r.unmasked_vs_masked, &r.canonical_vs_reordered] {
            assert_eq!(c.steps, 200, "{name}");
            assert_eq!(c.disagreeing_steps, 0, "{name} {c:?}");
        }
        // Masking is exact. Reordering sums the book in another order, so the
        // score may move in the last bits and no further.
        let m = &r.unmasked_vs_masked;
        assert_eq!(m.baseline_sharpe, m.variant_sharpe, "{name}");
        let o = &r.canonical_vs_reordered;
        let (a, b) = (o.baseline_sharpe.unwrap(), o.variant_sharpe.unwrap());
        assert!((a - b).abs() <= 1e-12 * a.abs().max(1.0), "{name} {a} {b}");
    }
}

#[test]
fn planted_biases_show_up_in_the_pair_that_exposes_them() {
    let position = report(&mut || Box::new(FirstListed));
    assert_eq!(position.unmasked_vs_masked.disagreeing_steps, 0);
    assert_eq!(position.canonical_vs_reordered.disagreeing_steps, 200);
    assert_eq!(position.canonical_vs_reordered.first_disagreement, Some(0));

    let ticker = report(&mut || Box::new(TickerRecall));
    assert_eq!(ticker.unmasked_vs_masked.disagreeing_steps, 200);
    assert_eq!(ticker.canonical_vs_reordered.disagreeing_steps, 0);
    assert_ne!(
        ticker.unmasked_vs_masked.baseline_sharpe,
        ticker.unmasked_vs_masked.variant_sharpe
    );

    let level = report(&mut || Box::new(LevelReader));
    assert_eq!(level.unmasked_vs_masked.disagreeing_steps, 200);
    assert_eq!(level.canonical_vs_reordered.disagreeing_steps, 0);
    assert!(level.unmasked_vs_masked.mean_max_weight_gap > 0.29);
}

/// Recalls calendar dates: for every date it remembers, it knows which listed
/// position rises most by the next close, and buys that position. On a date it
/// does not remember it holds evenly. It never reads a symbol name.
struct DateRecall {
    memory: std::collections::BTreeMap<String, usize>,
}

impl DateRecall {
    fn remembering(data: &Dataset) -> Self {
        let series: Vec<&Vec<f64>> = data.closes.values().collect();
        let memory = (0..data.len() - 1)
            .map(|t| {
                let best = (0..series.len())
                    .max_by(|&a, &b| {
                        (series[a][t + 1] / series[a][t])
                            .total_cmp(&(series[b][t + 1] / series[b][t]))
                    })
                    .unwrap();
                (data.dates[t].clone(), best)
            })
            .collect();
        DateRecall { memory }
    }
}

impl Agent for DateRecall {
    fn decide(&mut self, obs: &MarketObservation) -> Decision {
        let n = obs.symbols.len() as f64;
        match self.memory.get(&obs.date).copied() {
            Some(best) => decide_by(obs, |i, _, _| if i == best { 1.0 } else { 0.0 }),
            None => decide_by(obs, |_, _, _| 1.0 / n),
        }
    }
}

const SHIFT_DAYS: i64 = 364;

fn suite(make: &mut dyn FnMut() -> Box<dyn Agent>) -> PresentationSuite {
    let data = frozen("us-indices-1d.csv");
    let costs = CostProfile::None.resolve().costs;
    let window = Window {
        start: 30,
        end: 230,
    };
    presentation_suite::<()>(
        &data,
        window,
        0,
        costs,
        SuiteSpec {
            date_shift_days: Some(SHIFT_DAYS),
        },
        &mut |request: &ArmRequest<'_>| {
            Ok(record_run(
                request.data,
                make().as_mut(),
                request.window,
                request.seed,
                request.costs,
            ))
        },
    )
    .unwrap()
}

/// Disagreeing steps per pair, in suite order: tickers-only, dates-only,
/// levels-only, full, reordered, date-shifted.
fn disagreements(suite: &PresentationSuite) -> Vec<usize> {
    suite
        .pairs
        .iter()
        .map(|p| p.comparison.disagreeing_steps)
        .collect()
}

#[test]
fn each_planted_recall_is_caught_only_by_the_handle_it_reads() {
    let data = frozen("us-indices-1d.csv");
    let honest = suite(&mut || Box::new(Momentum::default()));
    assert_eq!(disagreements(&honest), [0, 0, 0, 0, 0, 0]);
    assert_eq!(
        honest.pairs.iter().map(|p| p.variant).collect::<Vec<_>>(),
        [
            Presentation::Blinded {
                level: BlindingLevel::TickersOnly
            },
            Presentation::Blinded {
                level: BlindingLevel::DatesOnly
            },
            Presentation::Blinded {
                level: BlindingLevel::LevelsOnly
            },
            Presentation::Blinded {
                level: BlindingLevel::Full
            },
            Presentation::Reordered,
            Presentation::DateShifted {
                offset_days: SHIFT_DAYS
            },
        ]
    );

    let ticker = suite(&mut || Box::new(TickerRecall));
    assert_eq!(disagreements(&ticker), [200, 0, 0, 200, 0, 0]);

    let level = suite(&mut || Box::new(LevelReader));
    assert_eq!(disagreements(&level), [0, 0, 200, 200, 0, 0]);

    // Date recall never reads a name, so only a presentation that changes the
    // calendar it sees separates its arms.
    let date = suite(&mut || Box::new(DateRecall::remembering(&data)));
    let d = disagreements(&date);
    assert_eq!(d[0], 0, "tickers-only keeps the calendar");
    assert_eq!(d[2], 0, "levels-only keeps the calendar");
    assert_eq!(d[1], 200, "dates-only hides it");
    assert_eq!(d[3], 200, "the full mask hides it");
    assert_eq!(
        d[4], 0,
        "the reordered pair is fully masked, so no date is seen"
    );
    // A 52-week shift lands most days on another remembered day, so the agent
    // recalls the wrong bar rather than nothing; it agrees only where the two
    // days happen to share a best position.
    assert!(
        d[5] >= 100,
        "the shifted calendar is not the real one: {d:?}"
    );
    let shifted = date
        .pair(Presentation::DateShifted {
            offset_days: SHIFT_DAYS,
        })
        .unwrap();
    assert!(
        shifted.baseline_sharpe.unwrap() > shifted.variant_sharpe.unwrap(),
        "recall pays only under the real calendar: {shifted:?}"
    );
}

#[test]
fn the_suite_full_level_matches_the_two_pair_report() {
    let report = report(&mut || Box::new(TickerRecall));
    let suite = suite(&mut || Box::new(TickerRecall));
    assert_eq!(
        suite.pair(Presentation::Blinded {
            level: BlindingLevel::Full
        }),
        Some(&report.unmasked_vs_masked)
    );
    assert_eq!(
        suite.pair(Presentation::Reordered),
        Some(&report.canonical_vs_reordered)
    );
}

#[test]
fn a_failed_arm_or_an_unshiftable_calendar_refuses_the_suite() {
    let data = frozen("us-indices-1d.csv");
    let costs = CostProfile::None.resolve().costs;
    let window = Window { start: 30, end: 60 };
    let mut calls = 0;
    let failed = presentation_suite(
        &data,
        window,
        0,
        costs,
        SuiteSpec::default(),
        &mut |request: &ArmRequest<'_>| {
            calls += 1;
            if request.presentation == Presentation::Reordered {
                Err("transport")
            } else {
                Ok(record_run(
                    request.data,
                    &mut BuyAndHold,
                    request.window,
                    request.seed,
                    request.costs,
                ))
            }
        },
    );
    assert_eq!(
        failed,
        Err(SuiteError::Arm {
            presentation: Presentation::Reordered,
            error: "transport"
        })
    );
    assert_eq!(
        calls, 6,
        "unmasked, four levels, then the failed reordered arm"
    );

    let undated = Dataset::synthetic(3, 80, 1);
    let mut ran = false;
    let refused = presentation_suite::<()>(
        &undated,
        window,
        0,
        costs,
        SuiteSpec {
            date_shift_days: Some(7),
        },
        &mut |_| {
            ran = true;
            unreachable!("no arm runs before every presentation is built")
        },
    );
    assert!(matches!(
        refused,
        Err(SuiteError::DateShift {
            error: DateShiftError::Unparseable { step: 0, .. }
        })
    ));
    assert!(!ran);
}
