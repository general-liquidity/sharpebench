//! BR2: an append-only attempt ledger that preserves failed and retried cost.
//!
//! The expanded audit ledger claimed that "append-only or checkpointed retries
//! preserve prior evidence". They did not. `run_with_retries` returned the
//! completed run and dropped every attempt that preceded it, the checkpoint
//! stored only that terminal outcome, and an eventual agent fault was recorded
//! as a single attempt however many transport failures came first. A slow,
//! error-prone agent therefore looked cheaper and faster than it was.
//!
//! These regressions pin the other half of the accounting: every attempt is
//! timed and kept, a resumed completion is appended after the failed attempt it
//! supersedes rather than over it, an unobserved duration is typed rather than
//! summed as zero, and a replayed batch cannot inflate the total.

use std::time::Duration;

use sharpebench_harness::{
    failing_sentinel_run, run_agent_resilient, AttemptDuration, AttemptLedger, AttemptRecord,
    DurationSource, FailureKind, SweepCheckpoint,
};

/// Long enough that any host clock resolves it, short enough to stay cheap.
const SLOW_FAILURE: Duration = Duration::from_millis(3);

fn timed(nanos: u64) -> AttemptDuration {
    AttemptDuration::HostClock { nanos }
}

fn one_failed(nanos: u64) -> AttemptLedger {
    let mut ledger = AttemptLedger::default();
    ledger.push(AttemptRecord::failed(
        FailureKind::TransportError,
        timed(nanos),
    ));
    ledger
}

fn one_completed(nanos: u64) -> AttemptLedger {
    let mut ledger = AttemptLedger::default();
    ledger.push(AttemptRecord::completed(timed(nanos)));
    ledger
}

/// Two transport failures then a success: the completion must not be the only
/// thing the accounting sees, and the time the failures burned must be in the
/// total rather than deleted with them.
#[test]
fn a_completion_after_transport_failures_keeps_what_the_failures_spent() {
    let mut calls = 0;
    let result = run_agent_resilient("slow-but-eventually-fine", 1, &[0], 3, 40, |_w, _seed| {
        calls += 1;
        if calls < 3 {
            std::thread::sleep(SLOW_FAILURE);
            Err(FailureKind::TransportError)
        } else {
            Ok(failing_sentinel_run(40))
        }
    });

    assert_eq!(result.submission.runs.len(), 1, "one scorable cell");
    let ledger = result.attempts;
    assert_eq!(ledger.attempts, 3, "three attempts were paid for");
    assert_eq!(ledger.failed, 2, "both failures stay in the ledger");
    assert_eq!(ledger.completed, 1);
    assert_eq!(ledger.duration_source, DurationSource::HostClock);
    assert!(
        ledger.duration_ns_total >= 2 * u64::try_from(SLOW_FAILURE.as_nanos()).unwrap(),
        "the failed attempts' own elapsed time must be in the total, got {} ns",
        ledger.duration_ns_total
    );
}

/// Two transport failures then an agent fault is three attempts, not one. The
/// failure log used to hardcode a single attempt for an eventual agent fault.
#[test]
fn an_agent_fault_after_transport_retries_reports_every_attempt() {
    let mut calls = 0;
    let result = run_agent_resilient("flaky-then-malformed", 1, &[0], 3, 40, |_w, _seed| {
        calls += 1;
        if calls < 3 {
            Err(FailureKind::TransportError)
        } else {
            Err(FailureKind::AgentProtocolViolation)
        }
    });

    assert_eq!(result.failures.records.len(), 1);
    let record = &result.failures.records[0];
    assert_eq!(record.kind, FailureKind::AgentProtocolViolation);
    assert!(!record.runtime, "the fault itself is the agent's");
    assert_eq!(
        record.attempts, 3,
        "the retries that preceded the fault were attempts too"
    );
    assert_eq!(result.attempts.attempts, 3);
    assert_eq!(result.attempts.failed, 3);
}

/// The concrete failure BR2 names: a completion that resumes a failed attempt
/// must be appended after it, never over it.
#[test]
fn a_resumed_completion_does_not_erase_the_failed_attempt_it_superseded() {
    let mut checkpoint = SweepCheckpoint::new("resumed", 1, &[7]);

    // A worker claims the cell, its attempt fails, and it persists that cost
    // before dying mid-cell.
    let (window, seed) = checkpoint.claim_next(0, 0).unwrap();
    checkpoint.record_attempts(window, seed, &one_failed(10_000));
    checkpoint.requeue_claimed();

    // A later worker resumes the same cell and completes it.
    let (window, seed) = checkpoint.claim_next(0, 0).unwrap();
    checkpoint.complete(
        window,
        seed,
        failing_sentinel_run(40),
        &one_completed(5_000),
    );

    let ledger = checkpoint.attempt_ledger().summary();
    assert_eq!(ledger.attempts, 2, "the failed attempt survives the resume");
    assert_eq!(ledger.failed, 1);
    assert_eq!(ledger.completed, 1);
    assert_eq!(
        ledger.duration_ns_total, 15_000,
        "the resumed completion adds to the failed attempt's cost, it does not replace it"
    );
    assert_eq!(ledger.duration_source, DurationSource::HostClock);
}

/// An attempt nobody timed is a typed unavailability. It must never be folded
/// into a total as a zero, which would read as a free attempt.
#[test]
fn an_unobserved_attempt_duration_is_typed_not_summed_as_zero() {
    let mut untimed = AttemptLedger::default();
    untimed.push(AttemptRecord::completed(AttemptDuration::Unavailable));
    let summary = untimed.summary();
    assert_eq!(summary.attempts, 1);
    assert_eq!(summary.duration_ns_total, 0);
    assert_eq!(
        summary.duration_source,
        DurationSource::Unavailable,
        "no clock saw this attempt, so the total is not a measurement"
    );

    untimed.push(AttemptRecord::failed(FailureKind::Timeout, timed(10_000)));
    let mixed = untimed.summary();
    assert_eq!(mixed.duration_ns_total, 10_000);
    assert_eq!(
        mixed.duration_source,
        DurationSource::Mixed,
        "a partly timed total understates the spend and must say so"
    );

    let empty = AttemptLedger::default().summary();
    assert_eq!(empty.attempts, 0);
    assert_eq!(empty.duration_source, DurationSource::Unavailable);
}

/// Append-only is not the same as append-blindly: replaying the identical batch
/// must not let a resumed or re-read record inflate the cost.
#[test]
fn replaying_the_same_attempt_batch_does_not_inflate_the_ledger() {
    let mut checkpoint = SweepCheckpoint::new("replayed", 1, &[7]);
    let batch = one_failed(10_000);
    checkpoint.record_attempts(0, 7, &batch);
    checkpoint.record_attempts(0, 7, &batch);
    let summary = checkpoint.attempt_ledger().summary();
    assert_eq!(summary.attempts, 1, "an exact replay is deduplicated");
    assert_eq!(summary.duration_ns_total, 10_000);

    // A genuinely different attempt still appends.
    checkpoint.record_attempts(0, 7, &one_failed(20_000));
    let summary = checkpoint.attempt_ledger().summary();
    assert_eq!(summary.attempts, 2);
    assert_eq!(summary.duration_ns_total, 30_000);
}

/// The replay guard is scoped to one cell. Two different cells that happen to
/// have identical records are two attempts, and folding them together would
/// delete real spend just as surely as dropping a failed attempt would.
#[test]
fn identical_records_from_different_cells_are_both_kept() {
    let mut checkpoint = SweepCheckpoint::new("two-cells", 1, &[7, 8]);
    checkpoint.record_attempts(0, 7, &one_failed(10_000));
    checkpoint.record_attempts(0, 8, &one_failed(10_000));
    let summary = checkpoint.attempt_ledger().summary();
    assert_eq!(
        summary.attempts, 2,
        "one attempt per cell, not one in total"
    );
    assert_eq!(summary.failed, 2);
    assert_eq!(summary.duration_ns_total, 20_000);

    // The same distinction at the ledger API: extend concatenates, append
    // suppresses only an exact replay of the tail.
    let mut gathered = AttemptLedger::default();
    gathered.extend(&one_failed(10_000));
    gathered.extend(&one_failed(10_000));
    assert_eq!(gathered.len(), 2);
    gathered.append(&one_failed(10_000));
    assert_eq!(gathered.len(), 2, "a replayed tail is still deduplicated");
}

/// The ledger is persisted state, not a per-process counter: an interrupted
/// sweep that reloads its checkpoint must reload what the earlier attempts cost.
#[test]
fn a_saved_checkpoint_reloads_every_recorded_attempt() {
    let path = std::env::temp_dir().join(format!(
        "sharpebench-br2-roundtrip-{}.json",
        std::process::id()
    ));
    let mut checkpoint = SweepCheckpoint::new("persisted", 1, &[7]);
    checkpoint.record_attempts(0, 7, &one_failed(10_000));
    checkpoint.complete(0, 7, failing_sentinel_run(40), &one_completed(5_000));
    checkpoint.save(&path).unwrap();

    let reloaded = SweepCheckpoint::load(&path).unwrap();
    let _ = std::fs::remove_file(&path);
    let summary = reloaded.attempt_ledger().summary();
    assert_eq!(summary.attempts, 2);
    assert_eq!(summary.failed, 1);
    assert_eq!(summary.duration_ns_total, 15_000);
    assert_eq!(summary.duration_source, DurationSource::HostClock);
}

/// A checkpoint written before the ledger existed carries no attempt evidence.
/// Resuming into it would report everything it already spent as zero, so it is
/// refused rather than read that way.
#[test]
fn a_terminal_task_without_any_recorded_attempt_is_refused() {
    let path = std::env::temp_dir().join(format!(
        "sharpebench-br2-legacy-{}.json",
        std::process::id()
    ));
    let mut checkpoint = SweepCheckpoint::new("legacy", 1, &[7]);
    checkpoint.complete(0, 7, failing_sentinel_run(40), &one_completed(5_000));
    let mut json: serde_json::Value =
        serde_json::from_str(&serde_json::to_string(&checkpoint).unwrap()).unwrap();
    json["tasks"][0]
        .as_object_mut()
        .unwrap()
        .remove("attempts")
        .expect("the checkpoint must have carried a ledger to strip");
    std::fs::write(&path, serde_json::to_string(&json).unwrap()).unwrap();

    let windows = [sharpebench_sim::Window { start: 0, end: 40 }];
    let error = sharpebench_harness::run_resumable_sweep(
        &path,
        "legacy",
        &windows,
        &[7],
        0,
        |_window, _seed| Ok(failing_sentinel_run(40)),
    );
    let _ = std::fs::remove_file(&path);
    let error = match error {
        Ok(_) => panic!("a terminal cell with no attempt evidence must not assemble"),
        Err(error) => error,
    };
    assert_eq!(error.kind(), std::io::ErrorKind::InvalidData);
    assert!(
        error.to_string().contains("records no attempt"),
        "unexpected error: {error}"
    );
}
