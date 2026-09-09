use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use sharpebench_harness::{
    failing_sentinel_run, run_resumable_sweep_bound_with_policy, FailureKind, ResumePolicy,
    SweepCheckpoint, SweepContract, SweepIdentity, TaskState, MAX_RUNTIME_RECOVERY_ROUNDS,
};
use sharpebench_sim::Window;

struct CheckpointFile(PathBuf);

impl CheckpointFile {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        Self(std::env::temp_dir().join(format!(
            "sharpe-runtime-recovery-{}-{}.json",
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

fn contract(windows: &[Window], seeds: &[u64], retries: u32) -> SweepContract {
    SweepContract::new(
        SweepIdentity {
            dataset_sha256: "11".repeat(32),
            cost_model_sha256: "22".repeat(32),
            score_config_sha256: "33".repeat(32),
            runner_artifact_sha256: "44".repeat(32),
            entrant_sha256: "55".repeat(32),
            invocation_sha256: "66".repeat(32),
        },
        windows,
        seeds,
        retries,
    )
}

#[test]
fn recovery_only_retries_runtime_cells_and_keeps_all_recorded_history() {
    let file = CheckpointFile::new();
    let windows = [Window { start: 0, end: 4 }];
    let contract = contract(&windows, &[7, 8, 9], 1);
    let first = run_resumable_sweep_bound_with_policy(
        &file.0,
        "entrant",
        &contract,
        &windows,
        ResumePolicy::UnfinishedOnly,
        |_, seed| match seed {
            7 => Ok(failing_sentinel_run(4)),
            8 => Err(FailureKind::AgentProtocolViolation),
            _ => Err(FailureKind::TransportError),
        },
    )
    .unwrap();
    assert_eq!(first.attempts.attempts, 4);
    let old = SweepCheckpoint::load(&file.0).unwrap();
    let no_op = run_resumable_sweep_bound_with_policy(
        &file.0,
        "entrant",
        &contract,
        &windows,
        ResumePolicy::UnfinishedOnly,
        |_, _| panic!("default resume must not rerun terminal cells"),
    )
    .unwrap();
    assert_eq!(no_op.attempts, first.attempts);
    let recovered = run_resumable_sweep_bound_with_policy(
        &file.0,
        "entrant",
        &contract,
        &windows,
        ResumePolicy::RetryRuntimeFailures,
        |window, seed| {
            assert_eq!((window, seed), (0, 9));
            let saved = SweepCheckpoint::load(&file.0).unwrap();
            assert_eq!(saved.tasks[2].runtime_recovery_rounds, 1);
            assert!(matches!(saved.tasks[2].state, TaskState::Claimed { .. }));
            Ok(failing_sentinel_run(4))
        },
    )
    .unwrap();
    assert_eq!(recovered.attempts.attempts, 5);
    assert_eq!(recovered.attempts.failed, 3);
    assert_eq!(recovered.attempts.completed, 2);
    assert_eq!(recovered.submission.runs.len(), 3);
    assert_eq!(recovered.failures.runtime_failures(), 0);
    assert_eq!(recovered.failures.agent_faults(), 1);
    let saved = SweepCheckpoint::load(&file.0).unwrap();
    assert_eq!(saved.contract, Some(contract));
    for i in 0..2 {
        assert_eq!(
            serde_json::to_value(&saved.tasks[i]).unwrap(),
            serde_json::to_value(&old.tasks[i]).unwrap()
        );
    }
    assert_eq!(
        &saved.tasks[2].attempts.attempts[..2],
        &old.tasks[2].attempts.attempts
    );
}

#[test]
fn recovery_rounds_and_failure_counts_accumulate_across_process_resumes() {
    let file = CheckpointFile::new();
    let windows = [Window { start: 0, end: 4 }];
    let contract = contract(&windows, &[7], 1);
    for round in 0..=MAX_RUNTIME_RECOVERY_ROUNDS {
        let result = run_resumable_sweep_bound_with_policy(
            &file.0,
            "entrant",
            &contract,
            &windows,
            ResumePolicy::RetryRuntimeFailures,
            |_, _| Err(FailureKind::Timeout),
        )
        .unwrap();
        assert_eq!(result.attempts.attempts, 2 * (round as usize + 1));
        assert_eq!(result.failures.records[0].attempts, 2 * (round + 1));
        assert!(result.submission.runs.is_empty());
        assert_eq!(
            SweepCheckpoint::load(&file.0).unwrap().tasks[0].runtime_recovery_rounds,
            round
        );
    }
    let before = std::fs::read(&file.0).unwrap();
    let error = run_resumable_sweep_bound_with_policy(
        &file.0,
        "entrant",
        &contract,
        &windows,
        ResumePolicy::RetryRuntimeFailures,
        |_, _| panic!("exhausted recovery must refuse before execution"),
    )
    .err()
    .unwrap();
    assert_eq!(error.kind(), std::io::ErrorKind::InvalidInput);
    assert_eq!(std::fs::read(&file.0).unwrap(), before);
}

#[test]
fn each_attempt_is_persisted_before_the_next_attempt_can_start() {
    let file = CheckpointFile::new();
    let windows = [Window { start: 0, end: 4 }];
    let contract = contract(&windows, &[7], 2);
    let mut calls = 0;
    let interrupted = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        run_resumable_sweep_bound_with_policy(
            &file.0,
            "entrant",
            &contract,
            &windows,
            ResumePolicy::UnfinishedOnly,
            |_, _| {
                let saved = SweepCheckpoint::load(&file.0).unwrap();
                assert_eq!(saved.tasks[0].attempts.len(), calls);
                calls += 1;
                if calls == 2 {
                    panic!("simulated interruption in the second attempt");
                }
                Err(FailureKind::TransportError)
            },
        )
    }));
    assert!(interrupted.is_err());
    assert_eq!(
        calls, 2,
        "the intended interruption, not a failed precondition"
    );
    let saved = SweepCheckpoint::load(&file.0).unwrap();
    assert_eq!(saved.tasks[0].attempts.len(), 1);
    let result = run_resumable_sweep_bound_with_policy(
        &file.0,
        "entrant",
        &contract,
        &windows,
        ResumePolicy::UnfinishedOnly,
        |_, _| Err(FailureKind::TransportError),
    )
    .unwrap();
    assert_eq!(
        result.attempts.attempts, 3,
        "resume has two attempts left, not three"
    );
    assert_eq!(result.attempts.failed, 3);
    assert_eq!(result.attempts.completed, 0);
    assert_eq!(result.failures.records[0].attempts, 3);
}

#[test]
fn a_changed_contract_or_agent_fault_disguised_as_runtime_cannot_be_recovered() {
    let file = CheckpointFile::new();
    let windows = [Window { start: 0, end: 4 }];
    let contract = contract(&windows, &[7], 0);
    run_resumable_sweep_bound_with_policy(
        &file.0,
        "entrant",
        &contract,
        &windows,
        ResumePolicy::UnfinishedOnly,
        |_, _| Err(FailureKind::SpawnError),
    )
    .unwrap();
    let before = std::fs::read(&file.0).unwrap();
    let mut changed = contract.clone();
    changed.invocation_sha256 = "aa".repeat(32);
    assert!(run_resumable_sweep_bound_with_policy(
        &file.0,
        "entrant",
        &changed,
        &windows,
        ResumePolicy::RetryRuntimeFailures,
        |_, _| panic!("changed identity must not execute"),
    )
    .is_err());
    assert_eq!(std::fs::read(&file.0).unwrap(), before);
    let mut malformed = SweepCheckpoint::load(&file.0).unwrap();
    malformed.tasks[0].state = TaskState::RuntimeFailed {
        kind: FailureKind::ResourceLimitExceeded,
        attempts: 1,
    };
    malformed.save(&file.0).unwrap();
    assert!(run_resumable_sweep_bound_with_policy(
        &file.0,
        "entrant",
        &contract,
        &windows,
        ResumePolicy::RetryRuntimeFailures,
        |_, _| panic!("agent fault must not execute"),
    )
    .is_err());
}
