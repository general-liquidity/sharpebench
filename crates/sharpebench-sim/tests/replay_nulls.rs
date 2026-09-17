//! The two rank-neutral replay diagnostics on planted panels: the
//! exposure-matched random-timing reference and the lagged replay.
//!
//! The panels are i.i.d. price paths, so nothing but the entrant's knowledge of
//! the next bar can make its timing pay, and nothing a static tilt could
//! exploit changes with the lag.

use std::collections::BTreeMap;

use sharpebench_protocol::{Action, AgentTrajectory, Decision, DecisionStep, Order, RunTrajectory};
use sharpebench_sim::costs::Rng;
use sharpebench_sim::replay_nulls::{
    lagged_replay, lagged_trajectory, timing_null, ReplayNullRefusal, RunTimingNull,
    TimingNullAggregate, TimingNullConfig, TimingNullReport, TimingNullUnavailable, VALID_WHEN,
};
use sharpebench_sim::{
    replay_run, run_backtest_capture, BuyAndHold, CostModel, CostProfile, Dataset, HoldAgent,
    Momentum, TradingEnv, Window,
};

/// `symbols` independent price paths with uniform per-bar returns of mean
/// `drift` and half-width `width`.
fn iid_panel(symbols: usize, bars: usize, seed: u64, drift: f64, width: f64) -> Dataset {
    let mut rng = Rng::new(seed);
    let closes = (0..symbols)
        .map(|symbol| {
            let mut price = 100.0;
            let series = (0..bars)
                .map(|_| {
                    price *= 1.0 + drift + width * rng.signed_unit();
                    price
                })
                .collect();
            (format!("S{symbol}"), series)
        })
        .collect();
    Dataset {
        dates: (0..bars).map(|bar| format!("b{bar:05}")).collect(),
        closes,
        dividends: BTreeMap::new(),
    }
}

fn frictionless() -> CostModel {
    CostProfile::None.resolve().costs
}

/// One run on `S0` whose decision at each bar is `target(bar)`: a target
/// weight, or `None` to hold.
fn planted(
    data: &Dataset,
    window: (usize, usize),
    target: impl Fn(usize) -> Option<f64>,
) -> RunTrajectory {
    let steps = (window.0..window.1)
        .enumerate()
        .map(|(step, bar)| DecisionStep {
            step,
            observation_id: data.dates[bar].clone(),
            decision: Decision {
                orders: target(bar)
                    .map(|weight| Order {
                        symbol: "S0".to_string(),
                        action: if weight > 0.0 {
                            Action::Buy
                        } else {
                            Action::Close
                        },
                        target_weight: weight,
                        confidence: None,
                        rationale: String::new(),
                    })
                    .into_iter()
                    .collect(),
                reasoning: String::new(),
                cost: None,
            },
        })
        .collect();
    RunTrajectory {
        window_start: window.0,
        window_end: window.1,
        seed: 3,
        steps,
    }
}

fn trajectory(runs: Vec<RunTrajectory>) -> AgentTrajectory {
    AgentTrajectory {
        agent_id: "planted".to_string(),
        contract: None,
        in_sample_trials: 0,
        declared_mandate: None,
        runs,
    }
}

fn next_return(data: &Dataset, bar: usize) -> f64 {
    let closes = &data.closes["S0"];
    closes[bar + 1] / closes[bar] - 1.0
}

/// Invested at 0.5 of NAV on the 30% of bars whose next return is largest,
/// flat otherwise: perfect knowledge of the next bar at a fixed exposure.
fn next_bar_informed(data: &Dataset, window: (usize, usize)) -> RunTrajectory {
    let mut next: Vec<f64> = (window.0..window.1 - 1)
        .map(|bar| next_return(data, bar))
        .collect();
    next.sort_by(f64::total_cmp);
    let cut = next[next.len() * 7 / 10];
    planted(data, window, |bar| {
        Some(if bar + 1 < window.1 && next_return(data, bar) >= cut {
            0.5
        } else {
            0.0
        })
    })
}

/// Enters at 0.5 of NAV and exits on a two-state Markov schedule that ignores
/// prices: invested about 30% of the time, holding about four bars at a time.
fn random_timing(data: &Dataset, window: (usize, usize), seed: u64) -> RunTrajectory {
    let mut rng = Rng::new(seed);
    let mut state = false;
    let schedule: Vec<bool> = (window.0..window.1)
        .map(|_| {
            let draw = rng.unit();
            state = if state { draw >= 0.23 } else { draw < 0.1 };
            state
        })
        .collect();
    planted(data, window, |bar| {
        let step = bar - window.0;
        match (step > 0 && schedule[step - 1], schedule[step]) {
            (false, true) => Some(0.5),
            (true, false) => Some(0.0),
            _ => None,
        }
    })
}

fn aggregate_percentile(report: &TimingNullReport) -> f64 {
    match &report.aggregate {
        TimingNullAggregate::Available { reference, .. } => reference.percentile,
        other => panic!("expected a percentile, got {other:?}"),
    }
}

fn config(draws: usize, seed: u64) -> TimingNullConfig {
    TimingNullConfig { draws, seed }
}

const WINDOW: (usize, usize) = (20, 320);

#[test]
fn the_timing_percentile_is_a_pure_function_of_the_declared_seed_and_draws() {
    let data = iid_panel(1, WINDOW.1, 1, 0.0005, 0.02);
    let traj = trajectory(vec![
        random_timing(&data, WINDOW, 11),
        random_timing(&data, WINDOW, 12),
    ]);
    let costs = CostModel::default();
    let first = timing_null(&data, &traj, costs, config(60, 7)).unwrap();
    let again = timing_null(&data, &traj, costs, config(60, 7)).unwrap();
    assert_eq!(first, again);
    assert_eq!(
        serde_json::to_string(&first).unwrap(),
        serde_json::to_string(&again).unwrap()
    );
    assert_eq!((first.draws, first.seed), (60, 7));

    let reseeded = timing_null(&data, &traj, costs, config(60, 8)).unwrap();
    assert_eq!(reseeded.seed, 8);
    assert_ne!(
        first.aggregate, reseeded.aggregate,
        "another seed must draw other placements"
    );
    let fewer = timing_null(&data, &traj, costs, config(30, 7)).unwrap();
    match &fewer.aggregate {
        TimingNullAggregate::Available {
            reference, runs, ..
        } => {
            assert_eq!(reference.draws, 30);
            assert_eq!(*runs, 2);
        }
        other => panic!("{other:?}"),
    }
    // The two runs draw from separate streams: identical decisions in both
    // would otherwise give identical reference distributions.
    let twins = trajectory(vec![
        random_timing(&data, WINDOW, 11),
        random_timing(&data, WINDOW, 11),
    ]);
    let twins = timing_null(&data, &twins, costs, config(60, 7)).unwrap();
    let references: Vec<_> = twins
        .runs
        .iter()
        .map(|run| match run {
            RunTimingNull::Available { reference, .. } => reference.clone(),
            other => panic!("{other:?}"),
        })
        .collect();
    assert_ne!(references[0], references[1]);
}

#[test]
fn genuine_timing_skill_ranks_above_every_random_placement() {
    for panel in [1, 2, 3] {
        let data = iid_panel(1, WINDOW.1, panel, 0.0005, 0.02);
        let traj = trajectory(vec![next_bar_informed(&data, WINDOW)]);
        let report = timing_null(&data, &traj, CostModel::default(), config(200, 9)).unwrap();
        assert!(
            aggregate_percentile(&report) >= 0.99,
            "panel {panel}: {report:?}"
        );
        let RunTimingNull::Available {
            exposure,
            reference,
            reference_mean_invested_bars,
            entrant_sharpe,
            ..
        } = &report.runs[0]
        else {
            panic!("panel {panel}: {report:?}");
        };
        assert_eq!(reference.below, 200, "panel {panel}");
        assert!(*entrant_sharpe > reference.reference_mean_sharpe);
        // Without a liquidity cap or execution noise every draw holds for
        // exactly as many bars as the entrant did.
        assert_eq!(*reference_mean_invested_bars, exposure.invested_bars as f64);
        assert!(
            (0.25..=0.35).contains(&(exposure.invested_bars as f64 / exposure.bars as f64)),
            "{exposure:?}"
        );
        assert!((exposure.mean_gross_when_invested - 0.5).abs() < 0.05);
    }
}

#[test]
fn random_timing_ranks_near_the_middle_across_seeds() {
    for panel in [1, 2, 3] {
        let data = iid_panel(1, WINDOW.1, panel, 0.0005, 0.02);
        let percentiles: Vec<f64> = std::thread::scope(|scope| {
            let workers: Vec<_> = (0..40u64)
                .map(|entrant| {
                    let data = &data;
                    scope.spawn(move || {
                        let traj = trajectory(vec![random_timing(data, WINDOW, 1_000 + entrant)]);
                        let report = timing_null(
                            data,
                            &traj,
                            CostModel::default(),
                            config(100, 50 + entrant),
                        )
                        .unwrap();
                        if let RunTimingNull::Available {
                            exposure,
                            reference_mean_invested_bars,
                            ..
                        } = &report.runs[0]
                        {
                            assert_eq!(
                                *reference_mean_invested_bars,
                                exposure.invested_bars as f64
                            );
                        }
                        aggregate_percentile(&report)
                    })
                })
                .collect();
            workers
                .into_iter()
                .map(|worker| worker.join().unwrap())
                .collect()
        });
        let mean = percentiles.iter().sum::<f64>() / percentiles.len() as f64;
        let central = percentiles
            .iter()
            .filter(|p| (0.1..=0.9).contains(*p))
            .count();
        // A uniform percentile has mean 0.5 with a standard error of 0.046
        // over 40 entrants, and lands in the central 80% about 32 times.
        assert!(
            (0.35..=0.65).contains(&mean),
            "panel {panel}: {percentiles:?}"
        );
        assert!(central >= 24, "panel {panel}: {percentiles:?}");
        // A reference that merely echoed the entrant would put every one at 0.5.
        assert!(
            percentiles.iter().any(|&p| p < 0.25) && percentiles.iter().any(|&p| p > 0.75),
            "panel {panel}: {percentiles:?}"
        );
    }
}

#[test]
fn a_run_with_no_timing_freedom_is_typed_unavailable() {
    let data = Dataset::synthetic(3, 120, 5);
    let window = Window {
        start: 20,
        end: 120,
    };
    let costs = CostModel::default();
    let capture = |agent: &mut dyn sharpebench_sim::Agent| {
        run_backtest_capture(&data, agent, window, 2, costs).1
    };
    let never = capture(&mut HoldAgent);
    let always = capture(&mut BuyAndHold);

    let report = timing_null(
        &data,
        &trajectory(vec![never.clone()]),
        costs,
        config(10, 0),
    )
    .unwrap();
    let RunTimingNull::Unavailable {
        run,
        reason,
        exposure,
    } = &report.runs[0]
    else {
        panic!("{report:?}");
    };
    assert_eq!((*run, *reason), (0, TimingNullUnavailable::NeverInvested));
    assert_eq!((exposure.invested_bars, exposure.holding_periods), (0, 0));
    assert_eq!(exposure.mean_gross_when_invested, 0.0);
    assert_eq!(
        report.aggregate,
        TimingNullAggregate::Unavailable {
            reason: TimingNullUnavailable::NeverInvested
        }
    );
    let json = serde_json::to_value(&report).unwrap();
    assert_eq!(json["runs"][0]["status"], "unavailable");
    assert_eq!(json["runs"][0]["reason"], "never_invested");
    assert_eq!(json["aggregate"]["status"], "unavailable");
    assert_eq!(json["valid_when"], VALID_WHEN);

    let report = timing_null(
        &data,
        &trajectory(vec![always.clone()]),
        costs,
        config(10, 0),
    )
    .unwrap();
    assert!(matches!(
        report.runs[0],
        RunTimingNull::Unavailable {
            reason: TimingNullUnavailable::AlwaysInvested,
            ..
        }
    ));
    assert_eq!(
        report.aggregate,
        TimingNullAggregate::Unavailable {
            reason: TimingNullUnavailable::AlwaysInvested
        }
    );

    let mixed = timing_null(
        &data,
        &trajectory(vec![never.clone(), always.clone()]),
        costs,
        config(10, 0),
    )
    .unwrap();
    assert_eq!(
        mixed.aggregate,
        TimingNullAggregate::Unavailable {
            reason: TimingNullUnavailable::NoRunWithTimingFreedom
        }
    );

    // A run with timing freedom beside them is the aggregate on its own, and
    // its Sharpe is the one its plain replay earns.
    let free = capture(&mut Momentum::default());
    let replayed =
        sharpebench_core::deflated_sharpe::sharpe_ratio(&replay_run(&data, &free, costs).returns);
    let beside = timing_null(
        &data,
        &trajectory(vec![never, free, always]),
        costs,
        config(10, 4),
    )
    .unwrap();
    let TimingNullAggregate::Available {
        runs,
        entrant_mean_sharpe,
        ..
    } = &beside.aggregate
    else {
        panic!("{beside:?}");
    };
    assert_eq!(*runs, 1);
    let RunTimingNull::Available { entrant_sharpe, .. } = &beside.runs[1] else {
        panic!("{beside:?}");
    };
    assert_eq!(*entrant_sharpe, replayed);
    assert_eq!(*entrant_mean_sharpe, replayed);
    assert_eq!(
        serde_json::to_value(&beside).unwrap()["runs"][1]["status"],
        "available"
    );
}

#[test]
fn a_diagnostic_that_would_invent_decisions_is_refused() {
    let data = iid_panel(1, 120, 1, 0.0, 0.01);
    let costs = CostModel::default();
    let whole = planted(&data, (20, 120), |_| None);
    let mut short = whole.clone();
    short.steps.pop();
    let refusal = ReplayNullRefusal::IncompleteRun {
        run: 1,
        recorded: 99,
        required: 100,
    };
    let two = trajectory(vec![whole.clone(), short]);
    assert_eq!(
        timing_null(&data, &two, costs, config(5, 0)).unwrap_err(),
        refusal
    );
    assert_eq!(
        lagged_replay(&data, &two, costs, &[1]).unwrap_err(),
        refusal
    );

    let empty = trajectory(Vec::new());
    assert_eq!(
        timing_null(&data, &empty, costs, config(5, 0)).unwrap_err(),
        ReplayNullRefusal::EmptyTrajectory
    );
    assert_eq!(
        lagged_replay(&data, &empty, costs, &[1]).unwrap_err(),
        ReplayNullRefusal::EmptyTrajectory
    );

    let past = RunTrajectory {
        window_end: 121,
        ..whole.clone()
    };
    assert_eq!(
        lagged_replay(&data, &trajectory(vec![past]), costs, &[1]).unwrap_err(),
        ReplayNullRefusal::WindowOutsideData {
            run: 0,
            window_end: 121,
            dataset_len: 120,
        }
    );

    let one = trajectory(vec![whole]);
    assert_eq!(
        timing_null(&data, &one, costs, config(0, 0)).unwrap_err(),
        ReplayNullRefusal::NoDraws
    );
    assert_eq!(
        lagged_replay(&data, &one, costs, &[]).unwrap_err(),
        ReplayNullRefusal::EmptyLagSet
    );
    // 100 bars: lag 97 leaves bars 98 and 99, lag 98 leaves one.
    assert!(lagged_replay(&data, &one, costs, &[97]).is_ok());
    assert_eq!(
        lagged_replay(&data, &one, costs, &[1, 98]).unwrap_err(),
        ReplayNullRefusal::LagTooLong {
            lag: 98,
            run: 0,
            steps: 100,
        }
    );
    for refusal in [
        ReplayNullRefusal::NoDraws,
        ReplayNullRefusal::EmptyLagSet,
        ReplayNullRefusal::LagTooLong {
            lag: 98,
            run: 0,
            steps: 100,
        },
    ] {
        assert!(!refusal.to_string().is_empty());
    }
}

#[test]
fn a_zero_lag_reproduces_the_undelayed_replay_exactly() {
    let data = Dataset::synthetic(4, 160, 11);
    let costs = CostProfile::Realistic.resolve().costs;
    let (captured, run) = run_backtest_capture(
        &data,
        &mut Momentum::default(),
        Window {
            start: 20,
            end: 160,
        },
        5,
        costs,
    );
    let on_time = replay_run(&data, &lagged_trajectory(&run, 0), costs);
    assert_eq!(
        serde_json::to_string(&on_time).unwrap(),
        serde_json::to_string(&captured).unwrap()
    );
    let report = lagged_replay(&data, &trajectory(vec![run.clone()]), costs, &[0, 2]).unwrap();
    assert_eq!(report.lagged[0].lag, 0);
    assert_eq!(
        report.lagged[0].sharpe_by_run,
        report.undelayed.sharpe_by_run
    );
    assert_eq!(report.lagged[0].mean_return, report.undelayed.mean_return);
    assert_eq!(report.undelayed.lag, 0);

    // A lag shifts the decisions and holds before them; nothing else moves.
    let late = lagged_trajectory(&run, 2);
    assert_eq!(late.steps.len(), run.steps.len());
    assert!(late.steps[..2]
        .iter()
        .all(|step| step.decision.orders.is_empty()));
    for (late, early) in late.steps[2..].iter().zip(&run.steps) {
        assert_eq!(
            serde_json::to_string(&late.decision).unwrap(),
            serde_json::to_string(&early.decision).unwrap()
        );
    }
    assert_eq!(late.steps[5].observation_id, run.steps[5].observation_id);
    assert_eq!(
        (late.window_start, late.window_end, late.seed),
        (run.window_start, run.window_end, run.seed)
    );
}

#[test]
fn every_lag_is_compared_on_the_same_bars() {
    let data = Dataset::synthetic(3, 120, 7);
    let costs = CostModel::default();
    let runs: Vec<RunTrajectory> = [1, 2]
        .iter()
        .map(|&seed| {
            run_backtest_capture(
                &data,
                &mut Momentum::default(),
                Window {
                    start: 20,
                    end: 120,
                },
                seed,
                costs,
            )
            .1
        })
        .collect();
    let traj = trajectory(runs.clone());
    let report = lagged_replay(&data, &traj, costs, &[5, 1, 5, 2]).unwrap();
    assert_eq!(report.lags, vec![1, 2, 5]);
    assert_eq!(
        report.lagged.iter().map(|row| row.lag).collect::<Vec<_>>(),
        vec![1, 2, 5]
    );
    assert_eq!(report.skipped_leading_bars, 6);
    assert_eq!(report.compared_bars, 2 * 94);
    assert_eq!(report.runs, 2);
    assert_eq!(report.valid_when, VALID_WHEN);

    // Recompute one row by hand from the lagged replays' bars after the sixth.
    let bars: Vec<Vec<f64>> = runs
        .iter()
        .map(|run| replay_run(&data, &lagged_trajectory(run, 2), costs).returns[6..].to_vec())
        .collect();
    let row = &report.lagged[1];
    let pooled: f64 = bars.iter().flatten().sum::<f64>() / 188.0;
    assert_eq!(row.mean_return, pooled);
    for (index, returns) in bars.iter().enumerate() {
        assert_eq!(
            row.sharpe_by_run[index],
            sharpebench_core::deflated_sharpe::sharpe_ratio(returns)
        );
        assert_eq!(
            row.mean_return_by_run[index],
            returns.iter().sum::<f64>() / returns.len() as f64
        );
    }
    assert_eq!(
        row.mean_sharpe,
        (row.sharpe_by_run[0] + row.sharpe_by_run[1]) / 2.0
    );
    let undelayed: Vec<f64> = replay_run(&data, &runs[0], costs).returns[6..].to_vec();
    assert_eq!(
        report.undelayed.sharpe_by_run[0],
        sharpebench_core::deflated_sharpe::sharpe_ratio(&undelayed)
    );
}

#[test]
fn a_static_tilt_earns_the_same_at_every_lag() {
    let data = iid_panel(2, 420, 4, 0.0004, 0.02);
    let window = Window {
        start: 20,
        end: 420,
    };
    for costs in [frictionless(), CostModel::default()] {
        let traj = trajectory(
            (0..3)
                .map(|seed| run_backtest_capture(&data, &mut BuyAndHold, window, seed, costs).1)
                .collect(),
        );
        let report = lagged_replay(&data, &traj, costs, &[1, 2, 5]).unwrap();
        let exact = costs.fee_bps == 0.0;
        for row in &report.lagged {
            let (return_gap, sharpe_gap) = (
                (row.mean_return - report.undelayed.mean_return).abs(),
                (row.mean_sharpe - report.undelayed.mean_sharpe).abs(),
            );
            if exact {
                assert!(return_gap < 1e-15 && sharpe_gap < 1e-12, "{row:?}");
            } else {
                // Only the order of the seeded slippage draws moves.
                assert!(return_gap < 2e-7 && sharpe_gap < 1e-4, "{row:?}");
            }
        }
        assert!(report.undelayed.mean_return > 0.0);
    }
}

#[test]
fn a_next_bar_informed_agent_loses_its_edge_at_one_bar() {
    for panel in [1, 2, 3] {
        let data = iid_panel(1, 2_020, panel, 0.0004, 0.02);
        let window = (20, 2_020);
        let informed = planted(&data, window, |bar| {
            Some(if bar + 1 < window.1 && next_return(&data, bar) > 0.0 {
                1.0
            } else {
                0.0
            })
        });
        let report =
            lagged_replay(&data, &trajectory(vec![informed]), frictionless(), &[1, 2]).unwrap();
        let undelayed = &report.undelayed;
        assert!(undelayed.mean_sharpe > 0.5, "panel {panel}: {undelayed:?}");
        for row in &report.lagged {
            assert!(
                row.mean_sharpe.abs() < 0.2 && row.mean_return.abs() < undelayed.mean_return / 5.0,
                "panel {panel}: {row:?} against {undelayed:?}"
            );
        }
    }
}

/// The condition the diagnostics state: an entrant's own orders never move the
/// price it is marked at. Under every shipped profile, a held position's NAV
/// moves exactly with the frozen closes on every bar the book does not trade,
/// however large the trade that opened it and however steep the impact it paid.
#[test]
fn every_shipped_profile_marks_held_positions_at_the_frozen_close() {
    let data = iid_panel(2, 80, 6, 0.0, 0.03);
    let buy = Decision {
        orders: vec![
            Order {
                symbol: "S0".to_string(),
                action: Action::Buy,
                target_weight: 0.6,
                confidence: None,
                rationale: String::new(),
            },
            Order {
                symbol: "S1".to_string(),
                action: Action::Sell,
                target_weight: -0.3,
                confidence: None,
                rationale: String::new(),
            },
        ],
        reasoning: String::new(),
        cost: None,
    };
    let hold = Decision {
        orders: Vec::new(),
        reasoning: String::new(),
        cost: None,
    };
    for profile in [
        CostProfile::None,
        CostProfile::Typical,
        CostProfile::WorstCase,
        CostProfile::Realistic,
    ] {
        let mut env = TradingEnv::new(
            data.clone(),
            Window { start: 10, end: 80 },
            profile.resolve().costs,
            1,
        );
        env.reset();
        let mut previous: Option<(Vec<f64>, f64)> = None;
        let mut checked = 0;
        for bar in 10..80 {
            let decision = if bar == 10 { buy.clone() } else { hold.clone() };
            let stepped = env.step(decision);
            let shares: Vec<f64> = stepped
                .observation
                .portfolio
                .iter()
                .map(|position| position.shares)
                .collect();
            if let Some((held, nav)) = &previous {
                if *held == shares && held.iter().any(|&share| share != 0.0) {
                    let moved: f64 = ["S0", "S1"]
                        .iter()
                        .zip(held)
                        .map(|(symbol, share)| {
                            share
                                * (data.close_at(symbol, bar).unwrap()
                                    - data.close_at(symbol, bar - 1).unwrap())
                        })
                        .sum();
                    assert!(
                        (stepped.info.nav - nav - moved).abs() < 1e-12,
                        "{}: bar {bar}",
                        profile.name()
                    );
                    checked += 1;
                }
            }
            previous = Some((shares, stepped.info.nav));
        }
        assert!(checked > 50, "{}: {checked}", profile.name());
    }
    assert!(VALID_WHEN.contains("do not move the price"));
}

/// The book's route to the stressed profile's declared delay: its cost model
/// and `decision_delay_bars` passed to the lagged replay. The profile itself
/// still executes on the decision bar.
#[test]
fn the_stressed_profiles_declared_delay_is_measured_by_a_lagged_replay() {
    let stressed = CostProfile::WorstCase.resolve();
    assert_eq!(stressed.decision_delay_bars, 2);
    let data = Dataset::synthetic(4, 160, 3);
    let window = Window {
        start: 20,
        end: 160,
    };
    let (direct, run) =
        run_backtest_capture(&data, &mut Momentum::default(), window, 1, stressed.costs);
    let undelayed = replay_run(&data, &run, stressed.costs);
    assert_eq!(direct.returns, undelayed.returns);

    let report = lagged_replay(
        &data,
        &trajectory(vec![run.clone()]),
        stressed.costs,
        &[stressed.decision_delay_bars],
    )
    .unwrap();
    assert_eq!(report.lags, vec![2]);
    assert_eq!(report.skipped_leading_bars, 3);
    let delayed = replay_run(&data, &lagged_trajectory(&run, 2), stressed.costs);
    assert_ne!(delayed.returns, undelayed.returns);
    assert_eq!(
        report.lagged[0].sharpe_by_run[0],
        sharpebench_core::deflated_sharpe::sharpe_ratio(&delayed.returns[3..])
    );
    assert_eq!(
        report.undelayed.sharpe_by_run[0],
        sharpebench_core::deflated_sharpe::sharpe_ratio(&undelayed.returns[3..])
    );
}
