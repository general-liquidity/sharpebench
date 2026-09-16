//! The timing-luck floor: offsets of a window's start, measured on the
//! reference field and the hold control, never on an entrant.

use std::collections::BTreeMap;

use sharpebench_core::composite::pooled_returns;
use sharpebench_core::deflated_sharpe::observed_sharpe_ratio;
use sharpebench_core::{rank, AgentSubmission, ScoreConfig};
use sharpebench_harness::timing_luck::{
    timing_luck, ReferenceRole, ShiftGeometry, TimingLuckReport, TimingLuckSpec,
    TimingLuckUnavailable, ALL_WINDOWS_SCOPE, MIN_INSTANCE_BARS, TIMING_LUCK_SCHEMA_VERSION,
};
use sharpebench_harness::{luck_floor, run_agent};
use sharpebench_sim::{Agent, BuyAndHold, CostModel, Dataset, Momentum, Window};

const HOLD: &str = "pipeline-hold";
const LUCK_FLOOR_AGENTS: usize = 3;

fn spec(offsets: usize) -> TimingLuckSpec<'static> {
    TimingLuckSpec {
        offsets,
        luck_floor_agents: LUCK_FLOOR_AGENTS,
        hold_control_id: HOLD,
    }
}

fn cfg(seeds: &[u64]) -> ScoreConfig {
    let mut cfg = ScoreConfig::for_periods_per_year(252.0);
    cfg.execution_seeds_per_window = seeds.len();
    cfg
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

fn row<'a>(
    report: &'a TimingLuckReport,
    id: &str,
) -> &'a sharpebench_harness::timing_luck::TimingLuckRow {
    report
        .rows
        .iter()
        .find(|row| row.agent_id == id)
        .unwrap_or_else(|| panic!("no row for {id}"))
}

fn restrict(sub: &AgentSubmission, window: usize, seeds: usize) -> AgentSubmission {
    AgentSubmission {
        runs: sub.runs[window * seeds..(window + 1) * seeds].to_vec(),
        ..sub.clone()
    }
}

/// With one offset the report evaluates the declared windows and nothing else,
/// so every figure is the unshifted one, bit for bit: the Sharpe of the pooled
/// track, and the deflated Sharpe the board publishes for the field assembled
/// the way `run` assembles it.
#[test]
fn one_offset_reproduces_the_unshifted_figures_exactly() {
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
    let board = rank(&field, &cfg);
    let per_window: Vec<_> = (0..windows.len())
        .map(|w| {
            let scoped: Vec<AgentSubmission> = field
                .iter()
                .map(|sub| restrict(sub, w, seeds.len()))
                .collect();
            let board = rank(&scoped, &cfg);
            (scoped, board)
        })
        .collect();

    let ids: Vec<&str> = report.rows.iter().map(|r| r.agent_id.as_str()).collect();
    let expected: Vec<&str> = field
        .iter()
        .map(|s| s.agent_id.as_str())
        .chain([HOLD])
        .collect();
    assert_eq!(
        ids, expected,
        "the reference field in run order, then the control"
    );

    for sub in &field {
        let got = row(&report, &sub.agent_id);
        assert_eq!(got.role, ReferenceRole::RankedReference);
        let score = board.iter().find(|s| s.agent_id == sub.agent_id).unwrap();
        assert!(
            score.deflation_error.is_none(),
            "{:?}",
            score.deflation_error
        );
        assert_eq!(got.all_windows.scope, ALL_WINDOWS_SCOPE);
        assert_eq!(got.all_windows.windows, 2);
        assert_eq!(got.all_windows.offsets, 1);
        assert_eq!(
            got.all_windows.deflated_sharpe.by_offset,
            vec![Some(score.deflated_sharpe)]
        );
        let sharpe = observed_sharpe_ratio(&pooled_returns(sub, seeds.len())).unwrap();
        assert_eq!(
            got.all_windows.sharpe.by_offset[0].map(f64::to_bits),
            Some(sharpe.to_bits())
        );
        assert_eq!(
            got.all_windows.deflated_sharpe.by_offset[0].map(f64::to_bits),
            Some(score.deflated_sharpe.to_bits())
        );
        // One offset: measured once, no spread.
        assert_eq!(got.all_windows.sharpe.offsets_measured, 1);
        assert_eq!(got.all_windows.sharpe.range, Some(0.0));

        for (w, (scoped, board)) in per_window.iter().enumerate() {
            let window = &got.per_window[w];
            assert_eq!(
                window.scope,
                format!("{}-{}", windows[w].start, windows[w].end)
            );
            assert_eq!(window.windows, 1);
            let scoped_sub = scoped.iter().find(|s| s.agent_id == sub.agent_id).unwrap();
            let sharpe = observed_sharpe_ratio(&pooled_returns(scoped_sub, seeds.len())).unwrap();
            assert_eq!(
                window.sharpe.by_offset[0].map(f64::to_bits),
                Some(sharpe.to_bits())
            );
            let score = board.iter().find(|s| s.agent_id == sub.agent_id).unwrap();
            assert_eq!(
                window.deflated_sharpe.by_offset[0].map(f64::to_bits),
                Some(score.deflated_sharpe.to_bits())
            );
        }
    }

    // The hold control never trades: its track is constant, which has no
    // Sharpe ratio and no deflated Sharpe, so both are absent, not zero.
    let hold = row(&report, HOLD);
    assert_eq!(hold.role, ReferenceRole::SuiteControl);
    for scope in std::iter::once(&hold.all_windows).chain(&hold.per_window) {
        assert_eq!(scope.sharpe.by_offset, vec![None]);
        assert_eq!(scope.deflated_sharpe.by_offset, vec![None]);
        assert_eq!(scope.sharpe.offsets_measured, 0);
        assert_eq!(scope.sharpe.range, None);
        assert_eq!(scope.deflated_sharpe.range, None);
    }
}

/// Every symbol doubles every bar. Doubling is exact in binary floating point,
/// so every quantity the engine computes at a later start is the same one
/// scaled by a power of two, and every return is identical at every offset:
/// the spread is exactly zero for every row that has the figure.
#[test]
fn spread_is_zero_on_a_constant_return_series() {
    let data = dataset(
        100,
        &[
            |t| 2f64.powi(t as i32),
            |t| 2f64.powi(t as i32 + 1),
            |t| 2f64.powi(t as i32 - 1),
        ],
    );
    let windows = [Window { start: 25, end: 60 }, Window { start: 60, end: 95 }];
    let seeds = [0, 1, 2];
    let report = timing_luck(
        &data,
        &windows,
        &seeds,
        CostModel::default(),
        &cfg(&seeds),
        spec(5),
    )
    .expect("measurable");

    let ranked: Vec<_> = report
        .rows
        .iter()
        .filter(|row| row.role == ReferenceRole::RankedReference)
        .collect();
    assert_eq!(ranked.len(), 2 + LUCK_FLOOR_AGENTS);
    for row in ranked {
        for scope in std::iter::once(&row.all_windows).chain(&row.per_window) {
            for spread in [&scope.sharpe, &scope.deflated_sharpe] {
                assert_eq!(
                    spread.offsets_measured, 5,
                    "{} {}",
                    row.agent_id, scope.scope
                );
                assert_eq!(spread.range, Some(0.0), "{} {}", row.agent_id, scope.scope);
                assert_eq!(spread.min, spread.max);
            }
        }
    }
}

/// Flat prices with one jump just after the first window starts. Offset 0
/// holds through the jump; every later offset starts after it. The first
/// window's spread is positive, and the second window, which never sees the
/// jump, has none.
#[test]
fn spread_is_positive_with_a_planted_start_date_effect() {
    const JUMP: usize = 21;
    let data = dataset(
        120,
        &[
            |t| if t >= JUMP { 2.0 } else { 1.0 },
            |t| if t >= JUMP { 3.0 } else { 1.5 },
        ],
    );
    let windows = [
        Window { start: 20, end: 60 },
        Window {
            start: 60,
            end: 100,
        },
    ];
    let seeds = [0, 1];
    let report = timing_luck(
        &data,
        &windows,
        &seeds,
        CostModel::default(),
        &cfg(&seeds),
        spec(4),
    )
    .expect("measurable");

    let bh = row(&report, "buy-and-hold");
    let first = &bh.per_window[0];
    assert!(first.sharpe.range.unwrap() > 0.0, "{first:?}");
    assert!(first.deflated_sharpe.range.unwrap() > 0.0, "{first:?}");
    assert!(bh.all_windows.sharpe.range.unwrap() > 0.0);
    // Only offset 0 holds through the jump.
    let by_offset = &first.sharpe.by_offset;
    assert_eq!(first.sharpe.max, by_offset[0]);
    assert!(by_offset[1..]
        .iter()
        .all(|s| s.unwrap() < by_offset[0].unwrap()));

    let second = &bh.per_window[1];
    assert_eq!(second.sharpe.offsets_measured, 4);
    assert_eq!(second.sharpe.range, Some(0.0), "{second:?}");
}

/// A dataset too short for the declared offsets is refused with the input
/// that decided it; one offset fewer is measured.
#[test]
fn too_short_dataset_is_typed_unavailable() {
    let data = Dataset::synthetic(3, 30, 7);
    let seeds = [0];
    let cfg = cfg(&seeds);
    let window = [Window { start: 10, end: 30 }];
    let measure = |windows: &[Window], seeds: &[u64], offsets: usize| {
        timing_luck(
            &data,
            windows,
            seeds,
            CostModel::default(),
            &cfg,
            spec(offsets),
        )
    };

    // 20 bars and 20 offsets leave one-bar instances.
    assert_eq!(
        measure(&window, &seeds, 20).unwrap_err(),
        TimingLuckUnavailable::WindowTooShortForOffsets {
            window: "10-30".to_string(),
            window_len: 20,
            offsets: 20,
            min_instance_bars: MIN_INSTANCE_BARS,
        }
    );
    // 19 offsets leave exactly the minimum.
    let report = measure(&window, &seeds, 19).expect("two-bar instances are measurable");
    assert_eq!(report.geometry.windows[0].instance_len, MIN_INSTANCE_BARS);
    assert_eq!(report.geometry.windows[0].last_instance, "28-30");

    // A window the dataset does not reach.
    assert_eq!(
        measure(&[Window { start: 10, end: 31 }], &seeds, 1).unwrap_err(),
        TimingLuckUnavailable::WindowPastDatasetEnd {
            window: "10-31".to_string(),
            dataset_len: 30,
        }
    );
    assert_eq!(
        measure(&window, &seeds, 0).unwrap_err(),
        TimingLuckUnavailable::NoOffsets
    );
    assert_eq!(
        measure(&[], &seeds, 1).unwrap_err(),
        TimingLuckUnavailable::NoWindows
    );
    assert_eq!(
        measure(&window, &[], 1).unwrap_err(),
        TimingLuckUnavailable::NoSeeds
    );
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
    assert_eq!(doc["seeds"], 3);
    // Real synthetic prices: the phase of the schedule moves the Sharpe ratio.
    // (Every reference row sits far below the deflation bar on this panel, so
    // its deflated Sharpe is 0 at every offset.)
    let moved = doc["rows"].as_array().unwrap().iter().any(|row| {
        row["per_window"]
            .as_array()
            .unwrap()
            .iter()
            .any(|w| w["sharpe"]["range"].as_f64().unwrap_or(0.0) > 0.0)
    });
    assert!(moved, "{doc}");
}

/// Adjacent declared windows, as `run` declares them: every shifted instance
/// stays inside its own window, and the report says instances of one window
/// overlap across offsets.
#[test]
fn shifted_instances_stay_inside_their_declared_windows() {
    let (_, windows) = run_geometry();
    let geometry = ShiftGeometry::resolve(180, &windows, 8, 5).unwrap();
    assert!(geometry.declared_windows_disjoint);
    assert!(geometry.instances_of_distinct_windows_disjoint);
    assert!(geometry.instances_of_one_window_overlap);
    let first = &geometry.windows[0];
    assert_eq!(first.declared, "20-100");
    assert_eq!(first.instance_len, 76);
    assert_eq!(first.first_instance, "20-96");
    assert_eq!(first.last_instance, "24-100");
    assert_eq!(first.bars_shared_by_first_and_last_instance, 72);
    for offset in 0..5 {
        let shifted = geometry.instances(offset);
        for (instance, declared) in shifted.iter().zip(&windows) {
            assert_eq!(instance.start, declared.start + offset);
            assert_eq!(instance.end - instance.start, 76);
            assert!(instance.end <= declared.end);
        }
    }
    // Never reads a bar past the dataset end, at the last offset included.
    assert_eq!(geometry.instances(4)[1].end, 180);

    let single = ShiftGeometry::resolve(180, &windows, 8, 1).unwrap();
    assert!(!single.instances_of_one_window_overlap);
    assert_eq!(single.windows[1].first_instance, "100-180");
    assert_eq!(single.windows[1].last_instance, "100-180");

    // Overlapping declared windows are reported as such, not repaired.
    let overlapping = [
        Window {
            start: 20,
            end: 100,
        },
        Window {
            start: 99,
            end: 180,
        },
    ];
    let geometry = ShiftGeometry::resolve(180, &overlapping, 8, 2).unwrap();
    assert!(!geometry.declared_windows_disjoint);
    assert!(!geometry.instances_of_distinct_windows_disjoint);
    // Windows that merely touch are disjoint.
    let touching = ShiftGeometry::resolve(180, &windows, 8, 2).unwrap();
    assert!(touching.declared_windows_disjoint);
}
