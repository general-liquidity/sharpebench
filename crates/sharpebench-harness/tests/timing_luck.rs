//! The timing-luck floor: the reference field under a declared cadence, run
//! once per schedule phase over the full declared windows, never an entrant.

use std::collections::BTreeMap;

use sharpebench_core::composite::pooled_returns;
use sharpebench_core::deflated_sharpe::{
    deflated_sharpe_ratio_against_null, expected_max_sharpe, observed_sharpe_ratio,
};
use sharpebench_core::{rank, AgentSubmission, CompositeScore, ScoreConfig, TrialsSrStdSource};
use sharpebench_harness::timing_luck::{
    cadenced_reference_field, timing_luck, Cadenced, ReferenceRole, ScopeSpread, TimingLuckReport,
    TimingLuckRow, TimingLuckSpec, TimingLuckUnavailable, ALL_WINDOWS_SCOPE, MIN_WINDOW_BARS,
    TIMING_LUCK_SCHEMA_VERSION,
};
use sharpebench_harness::{luck_floor, run_agent};
use sharpebench_sim::{
    run_backtest_capture, Agent, BuyAndHold, CostModel, Dataset, Momentum, RandomAgent, Window,
};

const HOLD: &str = "pipeline-hold";
const LUCK_FLOOR_AGENTS: usize = 3;

fn spec(cadence: usize) -> TimingLuckSpec<'static> {
    TimingLuckSpec {
        cadence,
        luck_floor_agents: LUCK_FLOOR_AGENTS,
        hold_control_id: HOLD,
    }
}

fn cfg(seeds: &[u64]) -> ScoreConfig {
    let mut cfg = ScoreConfig::for_periods_per_year(252.0);
    cfg.execution_seeds_per_window = seeds.len();
    cfg
}

fn zero_costs() -> CostModel {
    CostModel {
        fee_bps: 0.0,
        slippage_bps: 0.0,
        impact_bps: 0.0,
        financing_bps: 0.0,
        ..CostModel::default()
    }
}

/// A dataset built from one close function per symbol.
fn dataset(n: usize, closes: &[fn(usize) -> f64]) -> Dataset {
    Dataset {
        dates: (0..n).map(|t| format!("t{t:04}")).collect(),
        closes: closes
            .iter()
            .enumerate()
            .map(|(i, close)| (format!("S{i}"), (0..n).map(close).collect()))
            .collect(),
        dividends: BTreeMap::new(),
    }
}

fn run_geometry() -> (Dataset, Vec<Window>) {
    (
        Dataset::synthetic(8, 180, 20_260_621),
        vec![
            Window {
                start: 20,
                end: 100,
            },
            Window {
                start: 100,
                end: 180,
            },
        ],
    )
}

fn row<'a>(report: &'a TimingLuckReport, id: &str) -> &'a TimingLuckRow {
    report
        .rows
        .iter()
        .find(|row| row.agent_id == id)
        .unwrap_or_else(|| panic!("no row for {id}"))
}

fn scopes(row: &TimingLuckRow) -> impl Iterator<Item = &ScopeSpread> {
    std::iter::once(&row.all_windows).chain(&row.per_window)
}

/// `field` restricted to scope `scope`: every window (`None`) or one.
fn scoped(field: &[AgentSubmission], scope: Option<usize>, seeds: usize) -> Vec<AgentSubmission> {
    match scope {
        None => field.to_vec(),
        Some(w) => field
            .iter()
            .map(|sub| AgentSubmission {
                runs: sub.runs[w * seeds..(w + 1) * seeds].to_vec(),
                ..sub.clone()
            })
            .collect(),
    }
}

/// The pooled track of `id` in `field` and its score when `field` is ranked.
fn scored(
    field: &[AgentSubmission],
    id: &str,
    scope: Option<usize>,
    seeds: usize,
    cfg: &ScoreConfig,
) -> (Vec<f64>, CompositeScore) {
    let field = scoped(field, scope, seeds);
    let board = rank(&field, cfg);
    let sub = field.iter().find(|s| s.agent_id == id).unwrap();
    let score = board.iter().find(|s| s.agent_id == id).unwrap().clone();
    (pooled_returns(sub, seeds), score)
}

/// Phase 0: the row's figures are its board's, and its deflation inputs are
/// the ones that board recorded.
fn assert_phase_zero(scope: &ScopeSpread, (pooled, score): &(Vec<f64>, CompositeScore)) {
    assert!(
        score.deflation_error.is_none(),
        "{:?}",
        score.deflation_error
    );
    let sharpe = observed_sharpe_ratio(pooled).unwrap();
    assert_eq!(
        scope.sharpe.by_phase[0].map(f64::to_bits),
        Some(sharpe.to_bits())
    );
    assert_eq!(
        scope.deflated_sharpe.by_phase[0].map(f64::to_bits),
        Some(score.deflated_sharpe.to_bits())
    );
    let bar = &scope.deflation;
    assert_eq!(bar.trials_sr_std.to_bits(), score.trials_sr_std.to_bits());
    assert_eq!(bar.trials_sr_std_source, score.trials_sr_std_source);
    assert_eq!(bar.effective_n_trials, score.effective_n_trials);
    assert_eq!(
        bar.null_mean_per_period.to_bits(),
        score.deflation_null_mean_per_period.to_bits()
    );
    assert_eq!(
        bar.deflation_bar_per_period.map(f64::to_bits),
        Some(score.deflation_bar_per_period.to_bits())
    );
}

/// Phase `p`: the row's Sharpe is its own track's, its deflated Sharpe uses the
/// phase-0 bar, and the dispersion the phase's own field measured is recorded.
fn assert_phase(scope: &ScopeSpread, phase: usize, (pooled, score): &(Vec<f64>, CompositeScore)) {
    let sharpe = observed_sharpe_ratio(pooled).unwrap();
    assert_eq!(
        scope.sharpe.by_phase[phase].map(f64::to_bits),
        Some(sharpe.to_bits())
    );
    let bar = &scope.deflation;
    let deflated = deflated_sharpe_ratio_against_null(
        pooled,
        bar.effective_n_trials,
        bar.null_mean_per_period,
        bar.trials_sr_std,
    )
    .unwrap();
    assert_eq!(
        scope.deflated_sharpe.by_phase[phase].map(f64::to_bits),
        Some(deflated.to_bits())
    );
    let field = &scope.field_dispersion_by_phase[phase];
    assert_eq!(field.trials_sr_std.to_bits(), score.trials_sr_std.to_bits());
    assert_eq!(field.trials_sr_std_source, score.trials_sr_std_source);
}

/// Cadence 1 has one phase, a decision on every bar: the board's reference
/// field, bit for bit, assembled here the way `run` assembles it.
#[test]
fn cadence_one_reproduces_the_board_exactly() {
    let (data, windows) = run_geometry();
    let seeds: Vec<u64> = (0..4).collect();
    let costs = CostModel::default();
    let cfg = cfg(&seeds);
    let report = timing_luck(&data, &windows, &seeds, costs, &cfg, spec(1)).expect("measurable");

    let mut field = vec![
        run_agent("buy-and-hold", &data, &windows, &seeds, costs, || {
            Box::new(BuyAndHold) as Box<dyn Agent>
        }),
        run_agent("momentum", &data, &windows, &seeds, costs, || {
            Box::new(Momentum::default()) as Box<dyn Agent>
        }),
    ];
    field.extend(luck_floor(
        &data,
        &windows,
        &seeds,
        costs,
        LUCK_FLOOR_AGENTS,
    ));

    let ids: Vec<&str> = report.rows.iter().map(|r| r.agent_id.as_str()).collect();
    let run_order: Vec<&str> = field
        .iter()
        .map(|s| s.agent_id.as_str())
        .chain([HOLD])
        .collect();
    assert_eq!(ids, run_order);
    assert_eq!(report.cadence, 1);
    assert_eq!(report.windows[0].window, "20-100");
    assert_eq!(report.windows[0].bars, 80);
    assert_eq!(report.windows[0].decisions_by_phase, vec![80]);

    for sub in &field {
        let got = row(&report, &sub.agent_id);
        assert_eq!(got.role, ReferenceRole::RankedReference);
        assert_eq!(got.all_windows.scope, ALL_WINDOWS_SCOPE);
        assert_eq!(got.all_windows.windows, 2);
        assert_eq!(got.all_windows.phases, 1);
        let all = scored(&field, &sub.agent_id, None, seeds.len(), &cfg);
        assert_phase_zero(&got.all_windows, &all);
        assert_phase(&got.all_windows, 0, &all);
        for (w, scope) in got.per_window.iter().enumerate() {
            assert_eq!(scope.scope, report.windows[w].window);
            assert_eq!(scope.windows, 1);
            let one = scored(&field, &sub.agent_id, Some(w), seeds.len(), &cfg);
            assert_phase_zero(scope, &one);
            assert_phase(scope, 0, &one);
        }
        // One phase: measured once, no spread.
        assert_eq!(got.all_windows.sharpe.phases_measured, 1);
        assert_eq!(got.all_windows.sharpe.range, Some(0.0));
        assert_eq!(got.all_windows.sharpe.std_dev, Some(0.0));
    }

    // The hold control never trades: its track is constant, which has no
    // Sharpe ratio and no deflated Sharpe, so both are absent, not zero. Its
    // bar is the configured prior at the configured trial count.
    let hold = row(&report, HOLD);
    assert_eq!(hold.role, ReferenceRole::SuiteControl);
    for scope in scopes(hold) {
        assert_eq!(scope.sharpe.by_phase, vec![None]);
        assert_eq!(scope.deflated_sharpe.by_phase, vec![None]);
        assert_eq!(scope.sharpe.phases_measured, 0);
        assert_eq!(scope.sharpe.range, None);
        assert_eq!(scope.sharpe.std_dev, None);
        assert_eq!(
            scope.deflation.trials_sr_std_source,
            TrialsSrStdSource::Configured
        );
        assert_eq!(scope.deflation.effective_n_trials, cfg.n_trials);
        let bar = expected_max_sharpe(scope.deflation.trials_sr_std, cfg.n_trials).unwrap();
        assert_eq!(scope.deflation.deflation_bar_per_period, Some(bar));
    }
}

/// Phase `p` at cadence `m` is the reference field with every agent wrapped in
/// `Cadenced(m, p)`, over the full declared windows. The report's Sharpe at
/// phase `p` is that field's, its deflated Sharpe uses the phase-0 field's bar,
/// and the dispersion phase `p`'s own field measured is recorded beside it.
#[test]
fn each_phase_is_the_cadenced_field_over_the_full_windows() {
    let (data, windows) = run_geometry();
    let seeds: Vec<u64> = (0..3).collect();
    let costs = CostModel::default();
    let cfg = cfg(&seeds);
    let report = timing_luck(&data, &windows, &seeds, costs, &cfg, spec(3)).expect("measurable");
    assert_eq!(report.windows[1].window, "100-180");
    assert_eq!(report.windows[1].decisions_by_phase, vec![27, 28, 27]);

    let mut dispersions = std::collections::BTreeSet::new();
    for phase in 0..3 {
        // Built by hand for buy-and-hold and momentum; the luck floor through
        // the public builder.
        let mut field = vec![
            run_agent("buy-and-hold", &data, &windows, &seeds, costs, || {
                Box::new(Cadenced::new(Box::new(BuyAndHold), 3, phase).unwrap()) as Box<dyn Agent>
            }),
            run_agent("momentum", &data, &windows, &seeds, costs, || {
                Box::new(Cadenced::new(Box::new(Momentum::default()), 3, phase).unwrap())
                    as Box<dyn Agent>
            }),
        ];
        field.extend(
            cadenced_reference_field(&data, &windows, &seeds, costs, LUCK_FLOOR_AGENTS, 3, phase)
                .into_iter()
                .skip(2),
        );
        for sub in &field {
            // Every run covers the whole declared window.
            assert_eq!(sub.runs[0].returns.len(), 80);
            let got = row(&report, &sub.agent_id);
            assert_eq!(got.all_windows.phases, 3);
            let all = scored(&field, &sub.agent_id, None, seeds.len(), &cfg);
            if phase == 0 {
                assert_phase_zero(&got.all_windows, &all);
            }
            assert_phase(&got.all_windows, phase, &all);
            dispersions.insert(all.1.trials_sr_std.to_bits());
            for (w, scope) in got.per_window.iter().enumerate() {
                let one = scored(&field, &sub.agent_id, Some(w), seeds.len(), &cfg);
                if phase == 0 {
                    assert_phase_zero(scope, &one);
                }
                assert_phase(scope, phase, &one);
            }
        }
    }
    // The fields measure different dispersions at different phases, which is
    // why the bar is held at phase 0's.
    assert!(dispersions.len() > 1, "{dispersions:?}");
}

/// Decisions land on the first bar and on steps `j % m == p`, nowhere else.
#[test]
fn decisions_follow_the_declared_schedule() {
    let data = Dataset::synthetic(3, 40, 11);
    let window = Window { start: 5, end: 16 };
    for phase in 0..4 {
        let mut agent = Cadenced::new(Box::new(BuyAndHold), 4, phase).unwrap();
        let (_, trajectory) = run_backtest_capture(&data, &mut agent, window, 0, zero_costs());
        let decided: Vec<usize> = trajectory
            .steps
            .iter()
            .filter(|step| !step.decision.orders.is_empty())
            .map(|step| step.step)
            .collect();
        let schedule: Vec<usize> = (0..11).filter(|j| *j == 0 || j % 4 == phase).collect();
        assert_eq!(decided, schedule, "phase {phase}");
        assert_eq!(Cadenced::decisions_in(4, phase, 11), schedule.len());
    }
    assert_eq!(Cadenced::decisions_in(4, 0, 11), 3);
    assert_eq!(Cadenced::decisions_in(4, 1, 11), 4);
    assert_eq!(Cadenced::decisions_in(4, 3, 11), 3);
    assert!(Cadenced::new(Box::new(BuyAndHold), 4, 4).is_none());
    assert!(Cadenced::new(Box::new(BuyAndHold), 0, 0).is_none());
}

/// A luck-floor row asks its random agent for a decision only on scheduled
/// bars, so the `i`-th decision draws the same weights at every phase.
#[test]
fn every_phase_draws_the_same_random_weights_per_decision() {
    let data = Dataset::synthetic(4, 60, 3);
    let window = Window { start: 20, end: 50 };
    let draws = |phase: usize| -> Vec<Vec<u64>> {
        let mut agent = Cadenced::new(Box::new(RandomAgent::new(77)), 5, phase).unwrap();
        let (_, trajectory) = run_backtest_capture(&data, &mut agent, window, 2, zero_costs());
        trajectory
            .steps
            .iter()
            .filter(|step| !step.decision.orders.is_empty())
            .map(|step| {
                step.decision
                    .orders
                    .iter()
                    .map(|o| o.target_weight.to_bits())
                    .collect()
            })
            .collect()
    };
    let base = draws(0);
    assert_eq!(base.len(), 6);
    for phase in 1..5 {
        let shifted = draws(phase);
        // A phase above 0 decides on the first bar and then on its own cadence.
        assert_eq!(shifted.len(), 7, "phase {phase}");
        assert_eq!(shifted[..6], base[..], "phase {phase}");
    }
}

/// Every symbol doubles every bar, and there are no costs. Buy-and-hold and
/// momentum hold weights of 1/4, which are exact in binary and never drift, so
/// no scheduled rebalance trades and every phase produces the same track. With
/// the bar held fixed, both spreads are exactly zero.
#[test]
fn spread_is_zero_when_no_rebalance_trades() {
    let data = dataset(
        100,
        &[
            |t| 2f64.powi(t as i32),
            |t| 2f64.powi(t as i32 + 1),
            |t| 2f64.powi(t as i32 - 1),
            |t| 2f64.powi(t as i32 + 2),
        ],
    );
    let windows = [Window { start: 25, end: 60 }, Window { start: 60, end: 95 }];
    let seeds = [0, 1, 2];
    let report = timing_luck(&data, &windows, &seeds, zero_costs(), &cfg(&seeds), spec(4))
        .expect("measurable");
    for id in ["buy-and-hold", "momentum"] {
        for scope in scopes(row(&report, id)) {
            for spread in [&scope.sharpe, &scope.deflated_sharpe] {
                assert_eq!(spread.phases_measured, 4, "{id} {}", scope.scope);
                assert_eq!(spread.range, Some(0.0), "{id} {}", scope.scope);
                assert_eq!(spread.std_dev, Some(0.0), "{id} {}", scope.scope);
            }
        }
    }
}

/// Symbol A alternates 1, 2, 1, 2 by bar parity and B stays at 1; no costs.
/// Equal-weight buy-and-hold at cadence 2 rebalances on A's low bars at phase
/// 0 and on its high bars at phase 1. Hand values, derived twice in exact
/// rationals with sympy (an accounting simulation and the closed-form return
/// pattern), with sample-standard-deviation Sharpe ratios:
/// - phase 0, one 40-bar window: returns 0, then +1/2 on odd steps and -1/3 on
///   even ones; Sharpe 11 sqrt(105690) / 16260;
/// - phase 1: 0, +1/2, then -1/4 on even steps and +1/3 on odd ones; Sharpe
///   5 sqrt(34346) / 5284;
/// - both windows pooled: 11 sqrt(107045) / 16260 and 5 sqrt(313077) / 15852.
#[test]
fn planted_phase_effect_matches_the_hand_computed_spread() {
    let data = dataset(110, &[|t| if t % 2 == 0 { 1.0 } else { 2.0 }, |_| 1.0]);
    let windows = [
        Window { start: 20, end: 60 },
        Window {
            start: 60,
            end: 100,
        },
    ];
    let seeds = [0, 1];
    let report = timing_luck(&data, &windows, &seeds, zero_costs(), &cfg(&seeds), spec(2))
        .expect("measurable");
    assert_eq!(report.windows[0].decisions_by_phase, vec![20, 21]);

    let window = [
        11.0 * 105_690f64.sqrt() / 16_260.0,
        5.0 * 34_346f64.sqrt() / 5_284.0,
    ];
    let pooled = [
        11.0 * 107_045f64.sqrt() / 16_260.0,
        5.0 * 313_077f64.sqrt() / 15_852.0,
    ];
    let close = |got: Option<f64>, want: f64| {
        let got = got.expect("measured");
        assert!((got - want).abs() < 1e-12, "{got} vs {want}");
    };
    let bh = row(&report, "buy-and-hold");
    for scope in &bh.per_window {
        close(scope.sharpe.by_phase[0], window[0]);
        close(scope.sharpe.by_phase[1], window[1]);
        close(scope.sharpe.range, window[0] - window[1]);
        close(scope.sharpe.std_dev, (window[0] - window[1]) / 2.0);
        assert_eq!(scope.sharpe.max, scope.sharpe.by_phase[0]);
    }
    close(bh.all_windows.sharpe.by_phase[0], pooled[0]);
    close(bh.all_windows.sharpe.by_phase[1], pooled[1]);
    close(bh.all_windows.sharpe.range, pooled[0] - pooled[1]);
    assert!(bh.all_windows.sharpe.range.unwrap() > 0.04);
}

/// A window shorter than the cadence is refused with the input that decided
/// it; a window exactly as long as the cadence is measured.
#[test]
fn too_short_dataset_is_typed_unavailable() {
    let data = Dataset::synthetic(3, 30, 7);
    let seeds = [0];
    let cfg = cfg(&seeds);
    let window = [Window { start: 10, end: 30 }];
    let measure = |windows: &[Window], seeds: &[u64], cadence: usize| {
        timing_luck(
            &data,
            windows,
            seeds,
            CostModel::default(),
            &cfg,
            spec(cadence),
        )
    };

    assert_eq!(
        measure(&window, &seeds, 21).unwrap_err(),
        TimingLuckUnavailable::WindowShorterThanCadence {
            window: "10-30".to_string(),
            window_len: 20,
            cadence: 21,
            min_window_bars: 21,
        }
    );
    let report = measure(&window, &seeds, 20).expect("a window as long as the cadence");
    assert_eq!(report.windows[0].decisions_by_phase[0], 1);
    assert_eq!(report.windows[0].decisions_by_phase[19], 2);
    assert_eq!(report.rows[0].all_windows.sharpe.by_phase.len(), 20);

    // One bar has no Sharpe ratio, whatever the cadence.
    let one_bar = [Window { start: 10, end: 11 }];
    assert_eq!(
        measure(&one_bar, &seeds, 1).unwrap_err(),
        TimingLuckUnavailable::WindowShorterThanCadence {
            window: "10-11".to_string(),
            window_len: 1,
            cadence: 1,
            min_window_bars: MIN_WINDOW_BARS,
        }
    );
    measure(&[Window { start: 10, end: 12 }], &seeds, 1).expect("two bars");

    assert_eq!(
        measure(&[Window { start: 10, end: 31 }], &seeds, 1).unwrap_err(),
        TimingLuckUnavailable::WindowPastDatasetEnd {
            window: "10-31".to_string(),
            dataset_len: 30,
        }
    );
    let overlapping = [Window { start: 5, end: 15 }, Window { start: 14, end: 24 }];
    assert!(matches!(
        measure(&overlapping, &seeds, 1).unwrap_err(),
        TimingLuckUnavailable::WindowOrder { detail } if detail.contains("overlap")
    ));
    assert_eq!(
        measure(&window, &seeds, 0).unwrap_err(),
        TimingLuckUnavailable::NoCadence
    );
    assert_eq!(
        measure(&[], &seeds, 1).unwrap_err(),
        TimingLuckUnavailable::NoWindows
    );
    assert_eq!(
        measure(&window, &[], 1).unwrap_err(),
        TimingLuckUnavailable::NoSeeds
    );
    for bad in [0.0, -1.0, f64::NAN, f64::INFINITY] {
        let mut cfg = cfg.clone();
        cfg.periods_per_year = bad;
        assert_eq!(
            timing_luck(&data, &window, &seeds, CostModel::default(), &cfg, spec(1)).unwrap_err(),
            TimingLuckUnavailable::InvalidPeriodsPerYear
        );
    }
}

/// With no bootstrap resamples the score's interval fails and the score
/// reports that through `deflation_error`, but the deflation itself succeeded.
/// The report keeps the deflated Sharpe, identical to the one computed with an
/// interval.
#[test]
fn deflated_sharpe_survives_a_failed_interval() {
    let (data, windows) = run_geometry();
    let seeds = [0, 1];
    let with_interval = cfg(&seeds);
    let mut without = with_interval.clone();
    without.n_boot = 0;

    let field = cadenced_reference_field(
        &data,
        &windows,
        &seeds,
        CostModel::default(),
        LUCK_FLOOR_AGENTS,
        2,
        0,
    );
    let board = rank(&field, &without);
    assert!(board
        .iter()
        .all(|s| s.deflation_error.is_some() && s.dsr_ci_low.is_none()));

    let measure = |cfg: &ScoreConfig| {
        timing_luck(&data, &windows, &seeds, CostModel::default(), cfg, spec(2))
            .expect("measurable")
    };
    let kept = measure(&without);
    let reference = measure(&with_interval);
    for (a, b) in kept.rows.iter().zip(&reference.rows) {
        if a.role == ReferenceRole::SuiteControl {
            continue;
        }
        for (x, y) in scopes(a).zip(scopes(b)) {
            assert_eq!(x.deflated_sharpe.phases_measured, 2, "{}", a.agent_id);
            assert_eq!(x.deflated_sharpe, y.deflated_sharpe, "{}", a.agent_id);
            assert_eq!(x.deflation, y.deflation, "{}", a.agent_id);
            assert!(x.deflation.deflation_bar_per_period.is_some());
        }
    }
}

#[test]
fn the_report_is_deterministic() {
    let (data, windows) = run_geometry();
    let seeds = [0, 1, 2];
    let once = || {
        let report = timing_luck(
            &data,
            &windows,
            &seeds,
            CostModel::default(),
            &cfg(&seeds),
            spec(3),
        )
        .expect("measurable");
        serde_json::to_vec(&report).expect("the report serializes")
    };
    let first = once();
    assert_eq!(first, once());

    let doc: serde_json::Value = serde_json::from_slice(&first).unwrap();
    assert_eq!(doc["schema_version"], TIMING_LUCK_SCHEMA_VERSION);
    assert_eq!(doc["rank_input"], false);
    assert_eq!(doc["cadence"], 3);
    assert_eq!(doc["seeds"], 3);
    assert_eq!(doc["dataset_len"], 180);
    // Real synthetic prices: the phase of the schedule moves the Sharpe ratio.
    let moved = doc["rows"].as_array().unwrap().iter().any(|row| {
        row["all_windows"]["sharpe"]["range"]
            .as_f64()
            .is_some_and(|r| r > 0.0)
    });
    assert!(moved, "{doc}");
    let scope = &doc["rows"][0]["all_windows"];
    assert!(scope["deflation"]["trials_sr_std_source"].is_string());
    assert_eq!(
        scope["field_dispersion_by_phase"].as_array().unwrap().len(),
        3
    );
}
