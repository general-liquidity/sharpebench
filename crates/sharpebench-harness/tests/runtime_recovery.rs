use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use sharpebench_harness::fault_plan::FaultedObservation;
use sharpebench_harness::{
    failing_sentinel_run, run_resumable_sweep_bound_with_policy, run_resumable_sweep_with_backoff,
    AttemptObservation, BackoffSchedule, FailureKind, ResumePolicy, Sleeper, SweepCheckpoint,
    SweepContract, SweepIdentity, TaskState, ThreadSleeper, MAX_RUNTIME_RECOVERY_ROUNDS,
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

/// Records every wait it is asked for and checks, at the moment of each wait,
/// that the checkpoint on disk already carries it.
struct CheckingSleeper<'a> {
    path: &'a std::path::Path,
    waits: Vec<std::time::Duration>,
}

impl Sleeper for CheckingSleeper<'_> {
    fn sleep(&mut self, delay: std::time::Duration) {
        let saved = SweepCheckpoint::load(self.path).unwrap();
        let recorded = saved
            .tasks
            .iter()
            .filter_map(|task| task.attempts.attempts.last())
            .filter_map(|record| record.backoff_after)
            .any(|backoff| u128::from(backoff.delay_ns) == delay.as_nanos());
        assert!(recorded, "a wait must be saved before the driver sleeps");
        self.waits.push(delay);
    }
}

fn transport_failure() -> FaultedObservation {
    FaultedObservation::from(AttemptObservation::from(Err(FailureKind::TransportError)))
}

fn secs(values: &[u64]) -> Vec<std::time::Duration> {
    values
        .iter()
        .map(|&s| std::time::Duration::from_secs(s))
        .collect()
}

#[test]
fn a_checkpointed_backoff_is_saved_before_each_wait_and_restarts_per_round() {
    let file = CheckpointFile::new();
    let windows = [Window { start: 0, end: 4 }];
    let contract = contract(&windows, &[7, 8], 2);
    let schedule = BackoffSchedule::from_delays(&secs(&[5, 15]));
    let mut sleeper = CheckingSleeper {
        path: &file.0,
        waits: Vec::new(),
    };
    let mut seed_eight_calls = 0;
    let first = run_resumable_sweep_with_backoff(
        &file.0,
        "entrant",
        &contract,
        &windows,
        ResumePolicy::UnfinishedOnly,
        &schedule,
        &mut sleeper,
        |_, seed| {
            if seed == 8 {
                seed_eight_calls += 1;
                if seed_eight_calls == 2 {
                    return AttemptObservation::from(Ok(failing_sentinel_run(4))).into();
                }
            }
            transport_failure()
        },
    )
    .unwrap();
    // Seed 7 exhausts three attempts with two waits; seed 8 recovers after one.
    assert_eq!(sleeper.waits, secs(&[5, 15, 5]));
    assert_eq!(first.attempts.attempts, 5);
    assert_eq!(first.attempts.backoff_ns_total, 25_000_000_000);
    let saved = SweepCheckpoint::load(&file.0).unwrap();
    let waits_of = |task: usize| {
        saved.tasks[task]
            .attempts
            .attempts
            .iter()
            .map(|record| record.backoff_after.map(|b| (b.retry, b.delay_ns)))
            .collect::<Vec<_>>()
    };
    assert_eq!(
        waits_of(0),
        vec![Some((1, 5_000_000_000)), Some((2, 15_000_000_000)), None]
    );
    assert_eq!(waits_of(1), vec![Some((1, 5_000_000_000)), None]);

    // A recovery round is a new round: its retries restart the schedule.
    let mut sleeper = CheckingSleeper {
        path: &file.0,
        waits: Vec::new(),
    };
    let recovered = run_resumable_sweep_with_backoff(
        &file.0,
        "entrant",
        &contract,
        &windows,
        ResumePolicy::RetryRuntimeFailures,
        &schedule,
        &mut sleeper,
        |_, _| transport_failure(),
    )
    .unwrap();
    assert_eq!(sleeper.waits, secs(&[5, 15]));
    assert_eq!(recovered.attempts.backoff_ns_total, 45_000_000_000);
}

#[test]
fn an_immediate_checkpoint_schedule_writes_no_backoff() {
    let file = CheckpointFile::new();
    let windows = [Window { start: 0, end: 4 }];
    let contract = contract(&windows, &[7], 2);
    let result = run_resumable_sweep_with_backoff(
        &file.0,
        "entrant",
        &contract,
        &windows,
        ResumePolicy::UnfinishedOnly,
        &BackoffSchedule::immediate(),
        &mut ThreadSleeper,
        |_, _| transport_failure(),
    )
    .unwrap();
    assert_eq!(result.attempts.attempts, 3);
    assert_eq!(result.attempts.backoff_ns_total, 0);
    let text = std::fs::read_to_string(&file.0).unwrap();
    assert!(!text.contains("backoff"), "{text}");
}
