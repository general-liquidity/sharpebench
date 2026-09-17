//! Overlapping or unordered evaluation windows are refused at the evidence
//! boundary: strict trajectory replay, sweep-contract construction and every
//! bound sweep runner. The pooled track concatenates runs in window order and
//! PSR, the Deflated Sharpe and the bootstrap read it as successive market
//! observations, so a bar two windows share would be counted twice.

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use sharpebench_core::composite::pooled_returns;
use sharpebench_core::{Run, ScoreConfig};
use sharpebench_harness::{
    run_agent_capture, run_resumable_sweep_bound, run_resumable_sweep_observed, verify_trajectory,
    verify_trajectory_strict, AttemptObservation, FailureKind, ResumePolicy, SweepContract,
    SweepIdentity,
};
use sharpebench_protocol::AgentTrajectory;
use sharpebench_sim::trajectory::{IndexedWindow, WindowOrderError};
use sharpebench_sim::{walk_forward, Agent, BuyAndHold, CostModel, Dataset, Window};

fn capture(data: &Dataset, windows: &[Window]) -> AgentTrajectory {
    let (_, trajectory) = run_agent_capture(
        "buy-and-hold",
        data,
        windows,
        &[1],
        CostModel::default(),
        || Box::new(BuyAndHold) as Box<dyn Agent>,
    );
    trajectory
}

fn strict(data: &Dataset, trajectory: &AgentTrajectory) -> Result<usize, String> {
    verify_trajectory_strict(
        data,
        trajectory,
        CostModel::default(),
        &ScoreConfig::default(),
        None,
    )
    .map(|verified| verified.decisions_replayed)
}

fn at(index: usize, start: usize, end: usize) -> IndexedWindow {
    IndexedWindow { index, start, end }
}

fn identity() -> SweepIdentity {
    SweepIdentity {
        dataset_sha256: "11".repeat(32),
        cost_model_sha256: "22".repeat(32),
        score_config_sha256: "33".repeat(32),
        runner_artifact_sha256: "44".repeat(32),
        entrant_sha256: "55".repeat(32),
        invocation_sha256: "66".repeat(32),
    }
}

struct CheckpointFile(PathBuf);

impl CheckpointFile {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        Self(std::env::temp_dir().join(format!(
            "sharpe-overlapping-windows-{}-{}.json",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        )))
    }
}

impl Drop for CheckpointFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

fn run_of(window: &Window) -> Run {
    Run {
        returns: vec![0.001; window.end - window.start],
        ..Run::default()
    }
}

#[test]
fn strict_replay_refuses_a_capture_over_overlapping_windows() {
    let data = Dataset::synthetic(3, 100, 20_260_916);
    let windows = [Window { start: 20, end: 60 }, Window { start: 40, end: 80 }];
    let trajectory = capture(&data, &windows);

    // What the unchecked path makes of it: 80 pooled observations drawn from
    // the 60 distinct bars [20, 80), with [40, 60) counted twice.
    let diagnostic = verify_trajectory(
        &data,
        &trajectory,
        CostModel::default(),
        &ScoreConfig::default(),
    );
    assert_eq!(diagnostic.decisions_replayed, 80);
    let pooled = pooled_returns(
        &sharpebench_sim::replay_submission(&data, &trajectory, CostModel::default()),
        1,
    );
    assert_eq!(pooled.len(), 80);

    let refusal = strict(&data, &trajectory).expect_err("overlapping windows are not evidence");
    let expected = WindowOrderError::Overlapping {
        earlier: at(0, 20, 60),
        later: at(1, 40, 80),
        shared_start: 40,
        shared_end: 60,
    };
    assert_eq!(refusal, format!("trajectory contract: {expected}"));
    assert!(refusal.contains("[20, 60)"), "{refusal}");
    assert!(refusal.contains("[40, 80)"), "{refusal}");
    assert!(refusal.contains("overlap on bars [40, 60)"), "{refusal}");
}

#[test]
fn strict_replay_accepts_adjacent_windows() {
    let data = Dataset::synthetic(3, 100, 20_260_916);
    let windows = [
        Window { start: 20, end: 60 },
        Window {
            start: 60,
            end: 100,
        },
    ];
    let trajectory = capture(&data, &windows);
    assert_eq!(strict(&data, &trajectory), Ok(80));
}

#[test]
fn strict_replay_refuses_windows_out_of_time_order() {
    let data = Dataset::synthetic(3, 100, 20_260_916);
    let windows = [
        Window {
            start: 60,
            end: 100,
        },
        Window { start: 20, end: 60 },
    ];
    let trajectory = capture(&data, &windows);
    let refusal = strict(&data, &trajectory).expect_err("the pooled track must run forward");
    let expected = WindowOrderError::Unordered {
        earlier: at(0, 60, 100),
        later: at(1, 20, 60),
    };
    assert_eq!(refusal, format!("trajectory contract: {expected}"));
}

#[test]
fn strict_replay_refuses_rolling_walk_forward_windows() {
    let data = Dataset::synthetic(2, 365, 20_260_916);
    let windows = walk_forward(365, 30, 45, 20);
    let trajectory = capture(&data, &windows);
    let refusal = strict(&data, &trajectory).expect_err("step < test overlaps");
    assert!(
        refusal.contains("windows 0 [30, 75) and 1 [50, 95) overlap on bars [50, 75)"),
        "{refusal}"
    );

    let disjoint = walk_forward(365, 30, 45, 45);
    assert_eq!(strict(&data, &capture(&data, &disjoint)), Ok(45 * 7));
}

#[test]
fn sweep_contract_construction_refuses_overlapping_windows() {
    let overlapping = [Window { start: 20, end: 60 }, Window { start: 40, end: 80 }];
    assert_eq!(
        SweepContract::try_new(identity(), &overlapping, &[1], 0),
        Err(WindowOrderError::Overlapping {
            earlier: at(0, 20, 60),
            later: at(1, 40, 80),
            shared_start: 40,
            shared_end: 60,
        })
    );

    let unordered = [
        Window {
            start: 60,
            end: 100,
        },
        Window { start: 20, end: 60 },
    ];
    assert_eq!(
        SweepContract::try_new(identity(), &unordered, &[1], 0),
        Err(WindowOrderError::Unordered {
            earlier: at(0, 60, 100),
            later: at(1, 20, 60),
        })
    );

    let adjacent = [
        Window { start: 20, end: 60 },
        Window {
            start: 60,
            end: 100,
        },
    ];
    let checked = SweepContract::try_new(identity(), &adjacent, &[1, 2], 3)
        .expect("adjacent windows share no bar");
    assert_eq!(
        checked,
        SweepContract::new(identity(), &adjacent, &[1, 2], 3)
    );
    assert_eq!(checked.check_window_order(), Ok(()));
}

#[test]
fn a_bound_sweep_refuses_an_overlapping_contract_before_touching_the_checkpoint() {
    let file = CheckpointFile::new();
    let windows = [Window { start: 20, end: 60 }, Window { start: 40, end: 80 }];
    // `new` does not check; the runner does, before it loads or writes anything.
    let contract = SweepContract::new(identity(), &windows, &[1], 0);
    let Err(error) =
        run_resumable_sweep_bound(&file.0, "entrant", &contract, &windows, &[1], 0, |_, _| {
            panic!("no cell of an overlapping sweep may run")
        })
    else {
        panic!("an overlapping sweep is not evidence");
    };
    assert_eq!(error.kind(), std::io::ErrorKind::InvalidInput);
    assert_eq!(
        error
            .get_ref()
            .and_then(|inner| inner.downcast_ref::<WindowOrderError>()),
        Some(&WindowOrderError::Overlapping {
            earlier: at(0, 20, 60),
            later: at(1, 40, 80),
            shared_start: 40,
            shared_end: 60,
        })
    );
    assert!(!file.0.exists(), "a refused sweep writes no checkpoint");
}

#[test]
fn a_deserialized_contract_with_unordered_windows_is_refused_too() {
    let file = CheckpointFile::new();
    let windows = [
        Window {
            start: 60,
            end: 100,
        },
        Window { start: 20, end: 60 },
    ];
    let mut json = serde_json::to_value(SweepContract::new(
        identity(),
        &[Window { start: 20, end: 60 }],
        &[1],
        0,
    ))
    .unwrap();
    json["windows"] = serde_json::json!([[60, 100], [20, 60]]);
    let contract: SweepContract = serde_json::from_value(json).unwrap();
    let Err(error) = run_resumable_sweep_observed(
        &file.0,
        "entrant",
        &contract,
        &windows,
        ResumePolicy::UnfinishedOnly,
        |_, _| -> AttemptObservation { panic!("no cell of an unordered sweep may run") },
    ) else {
        panic!("an unordered sweep is not evidence");
    };
    assert_eq!(error.kind(), std::io::ErrorKind::InvalidInput);
    assert!(
        error
            .to_string()
            .contains("window 1 [20, 60) starts before window 0 [60, 100)"),
        "{error}"
    );
    assert!(!file.0.exists());
}

#[test]
fn a_bound_sweep_over_adjacent_windows_still_runs() {
    let file = CheckpointFile::new();
    let windows = [
        Window { start: 20, end: 60 },
        Window {
            start: 60,
            end: 100,
        },
    ];
    let contract = SweepContract::try_new(identity(), &windows, &[1], 0).unwrap();
    let result = run_resumable_sweep_bound(
        &file.0,
        "entrant",
        &contract,
        &windows,
        &[1],
        0,
        |window, _| -> Result<Run, FailureKind> { Ok(run_of(&windows[window])) },
    )
    .expect("adjacent windows are a valid sweep");
    assert_eq!(result.submission.runs.len(), 2);
    assert_eq!(pooled_returns(&result.submission, 1).len(), 80);
}
