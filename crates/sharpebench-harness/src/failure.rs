//! Retry-vs-runtime-failure taxonomy.
//!
//! A money-agent benchmark must not let infrastructure flakiness masquerade as
//! agent skill — in either direction. There are two *kinds* of failure and they
//! must be accounted for differently:
//!
//! - An **agent failure** is the agent's own fault: it produced a run that didn't
//!   clear the bar (a losing strategy, a process violation). This is a genuine
//!   pass^k failure and *must* count against the agent.
//! - A **runtime / harness error** is the harness's fault: a container crashed,
//!   stdout closed, the endpoint timed out. Silently scoring this as an agent
//!   failure would punish an agent for the operator's flaky infrastructure, and
//!   silently scoring it as a *pass* would let a crash-on-loss agent game pass^k.
//!   Neither is acceptable: a runtime error is **retried** up to a bound, and only
//!   if it never recovers is it logged as `Exhausted` — still excluded from the
//!   pass^k pool, but surfaced in the [`FailureLog`] so the operator sees it.
//!
//! The rule the rest of the harness relies on: **pass^k accounting only ever sees
//! genuine agent pass/fail outcomes** ([`RunOutcome::Completed`]). Runtime errors
//! are diverted into the log, never into the score.

use serde::{Deserialize, Serialize};
use sharpebench_core::Run;

/// Why a single run attempt failed to produce a scorable [`Run`].
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FailureKind {
    /// The agent process/endpoint could not be created (e.g. container failed to
    /// spawn). A harness/runtime error — retryable.
    SpawnError,
    /// Transport broke mid-run (stdout closed, connection reset, I/O error). A
    /// harness/runtime error — retryable.
    TransportError,
    /// The agent ran but exceeded the wall-clock budget. A harness/runtime error
    /// — retryable.
    Timeout,
    /// The agent produced output the harness could not parse into a decision. This
    /// is the *agent's* fault, not the harness's — **not** retried; it is a real
    /// agent failure and is surfaced as such.
    AgentProtocolViolation,
    /// The agent exceeded a published resource budget: the kernel OOM-killed its
    /// sandbox container (the `--memory` limit). The exit status alone cannot
    /// show this — an OOM kill exits 137 like any other SIGKILL — so the sandbox
    /// reads the container's `State.OOMKilled` after the run and the driver folds
    /// it in via [`apply_oom_verdict`]. Breaching a published budget is the
    /// agent's own fault: **not** retried (the same agent against the same budget
    /// reproduces it) and counted as a genuine failure for pass^k.
    ResourceLimitExceeded,
}

impl FailureKind {
    /// Whether this is a runtime/harness error (retryable) rather than an agent
    /// fault. Only runtime errors are retried; an agent-fault failure is final.
    pub fn is_runtime(&self) -> bool {
        matches!(
            self,
            FailureKind::SpawnError | FailureKind::TransportError | FailureKind::Timeout
        )
    }
}

/// Fold a sandboxed run's post-exit resource verdict into its result.
///
/// `oom_killed` is what the sandbox read from the exited container's
/// `State.OOMKilled` (`None` only when there was no container to inspect, e.g.
/// an explicitly opted-in unsandboxed local-dev run). The sandbox finalizer
/// returns an error rather than `None` when a container verdict is
/// indeterminate. A kernel OOM kill overrides *everything*, the transport-level
/// classification and even a clean run: exceeding the published budget is a
/// scoring-relevant fact in its own right, and the dead pipe an OOM-killed agent
/// leaves behind would otherwise be misfiled as a retryable transport blip — the
/// harness would then respawn an agent that is guaranteed to blow the same
/// budget again.
pub fn apply_oom_verdict(
    result: Result<Run, FailureKind>,
    oom_killed: Option<bool>,
) -> Result<Run, FailureKind> {
    if oom_killed == Some(true) {
        Err(FailureKind::ResourceLimitExceeded)
    } else {
        result
    }
}

/// How long one attempt took, and which clock saw it.
///
/// A failed attempt spends real wall-clock time and real money. Dropping it
/// before accounting, or folding an unmeasured attempt in as a zero, makes a
/// slow and error-prone agent look cheaper and faster than it was. An attempt
/// nobody timed is therefore *typed* as unavailable rather than summed as zero.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "source")]
pub enum AttemptDuration {
    /// Measured end to end on the driver's monotonic host clock.
    HostClock { nanos: u64 },
    /// No clock observed this attempt.
    Unavailable,
}

/// What one attempt produced. A failure keeps its kind, so the ledger shows
/// *what* was paid for as well as how much.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "outcome")]
pub enum AttemptOutcome {
    Completed,
    Failed { kind: FailureKind },
}

/// One attempt: its outcome and what it spent.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AttemptRecord {
    pub outcome: AttemptOutcome,
    /// The attempt itself, never including a backoff wait before or after it.
    pub duration: AttemptDuration,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub usage: Option<crate::accounting::AttemptUsage>,
    /// The wait the retry driver scheduled after this failed attempt and before
    /// the next one. Absent when retries are immediate (the default), so every
    /// ledger written without a schedule keeps its bytes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub backoff_after: Option<Backoff>,
    /// Faults a frozen plan injected into this attempt. Absent, and absent
    /// from the serialized record, whenever no plan is configured.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub injected_faults: Option<crate::fault_plan::InjectedFaults>,
}

impl AttemptRecord {
    pub fn completed(duration: AttemptDuration) -> Self {
        Self {
            outcome: AttemptOutcome::Completed,
            duration,
            usage: None,
            backoff_after: None,
            injected_faults: None,
        }
    }

    pub fn failed(kind: FailureKind, duration: AttemptDuration) -> Self {
        Self {
            outcome: AttemptOutcome::Failed { kind },
            duration,
            usage: None,
            backoff_after: None,
            injected_faults: None,
        }
    }

    pub fn is_failure(&self) -> bool {
        matches!(self.outcome, AttemptOutcome::Failed { .. })
    }
}

/// An append-only record of every attempt spent on one cell.
///
/// Scoring keeps only the terminal outcome of a cell. If operational accounting
/// reads that terminal record alone, a completion that resumed a failed attempt
/// erases everything the failed attempt spent. The ledger is the other half:
/// rank-neutral, never scored, and never rewritten in place.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AttemptLedger {
    pub attempts: Vec<AttemptRecord>,
}

impl AttemptLedger {
    pub fn monetary_summary(&self) -> crate::accounting::MonetarySummary {
        crate::accounting::summarize_usage(self.attempts.iter().map(|record| record.usage.as_ref()))
    }

    pub fn push(&mut self, record: AttemptRecord) {
        self.attempts.push(record);
    }

    pub fn len(&self) -> usize {
        self.attempts.len()
    }

    pub fn is_empty(&self) -> bool {
        self.attempts.is_empty()
    }

    /// Append `other`, except when it exactly repeats the records already at the
    /// tail. A resumed completion must never replace the failed attempt it
    /// superseded, and re-recording the same batch onto a reloaded checkpoint
    /// must never inflate the cost. The cost of that guarantee: two genuinely
    /// identical consecutive batches collapse into one.
    pub fn append(&mut self, other: &AttemptLedger) {
        if other.is_empty() {
            return;
        }
        let tail = self.attempts.len().checked_sub(other.attempts.len());
        if let Some(start) = tail {
            if self.attempts[start..] == other.attempts[..] {
                return;
            }
        }
        self.extend(other);
    }

    /// Concatenate `other` unconditionally. This is the right operation for
    /// gathering distinct cells into one sweep total: two cells that happen to
    /// have identical records are two attempts, not one, and deduplicating them
    /// would delete real spend. Only a replay of the *same* cell is a duplicate,
    /// which is what `append` is for.
    pub fn extend(&mut self, other: &AttemptLedger) {
        self.attempts.extend(other.attempts.iter().cloned());
    }

    /// Rank-neutral totals over every attempt, failed ones included.
    pub fn summary(&self) -> AttemptSummary {
        let mut duration_ns_total: u64 = 0;
        let mut backoff_ns_total: u64 = 0;
        let mut timed = 0usize;
        for record in &self.attempts {
            if let AttemptDuration::HostClock { nanos } = record.duration {
                duration_ns_total = duration_ns_total.saturating_add(nanos);
                timed += 1;
            }
            if let Some(backoff) = record.backoff_after {
                backoff_ns_total = backoff_ns_total.saturating_add(backoff.delay_ns);
            }
        }
        AttemptSummary {
            attempts: self.attempts.len(),
            failed: self.attempts.iter().filter(|a| a.is_failure()).count(),
            completed: self.attempts.iter().filter(|a| !a.is_failure()).count(),
            duration_ns_total,
            backoff_ns_total,
            duration_source: if timed == 0 {
                DurationSource::Unavailable
            } else if timed == self.attempts.len() {
                DurationSource::HostClock
            } else {
                DurationSource::Mixed
            },
        }
    }
}

/// Which clocks stand behind an aggregated duration.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DurationSource {
    /// Every attempt in the total was timed on the host clock.
    HostClock,
    /// Some attempts were timed and some were not: the total covers only the
    /// timed ones and understates the real spend.
    Mixed,
    /// No attempt carried an observed duration. The total is not a measurement.
    #[default]
    Unavailable,
}

/// Rank-neutral totals published beside the scored pool. Never an input to a
/// score, a rank, or a pass^k pool.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AttemptSummary {
    pub attempts: usize,
    pub completed: usize,
    pub failed: usize,
    pub duration_ns_total: u64,
    pub duration_source: DurationSource,
    /// Scheduled backoff between attempts, kept apart from `duration_ns_total`
    /// so waiting is never read as work. Omitted while it is zero, which is
    /// every sweep that retries immediately.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub backoff_ns_total: u64,
}

fn is_zero(value: &u64) -> bool {
    *value == 0
}

/// The outcome of attempting one (window, seed) run, after any retries.
#[derive(Clone, Debug)]
pub enum RunOutcome {
    /// The agent produced a scorable run — feeds pass^k as a genuine pass/fail.
    Completed(Run),
    /// Every retry of a runtime error was exhausted. **Not** a pass^k failure (it
    /// is the harness's fault, not the agent's); recorded in the [`FailureLog`].
    Exhausted { last: FailureKind, attempts: u32 },
    /// A non-retryable agent fault (e.g. malformed output). Counts as a genuine
    /// agent failure for pass^k accounting — represented as a sentinel failing run.
    AgentFault(FailureKind),
}

/// One logged failure event: which run, what kind, how many attempts were spent.
#[derive(Clone, Debug)]
pub struct FailureRecord {
    pub window_index: usize,
    pub seed: u64,
    pub kind: FailureKind,
    pub attempts: u32,
    /// Whether this was a retryable runtime error (vs. a final agent fault).
    pub runtime: bool,
}

/// The harness-side failure log accumulated across a submission's runs. A clear,
/// inspectable type — not a side-channel of `eprintln!`s.
#[derive(Clone, Debug, Default)]
pub struct FailureLog {
    pub records: Vec<FailureRecord>,
}

impl FailureLog {
    pub fn push(&mut self, record: FailureRecord) {
        self.records.push(record);
    }

    /// Runtime (harness) errors that exhausted their retries — *not* agent faults.
    pub fn runtime_failures(&self) -> usize {
        self.records.iter().filter(|r| r.runtime).count()
    }

    /// Genuine agent faults (excluded from the retry path, counted against pass^k).
    pub fn agent_faults(&self) -> usize {
        self.records.iter().filter(|r| !r.runtime).count()
    }

    pub fn is_empty(&self) -> bool {
        self.records.is_empty()
    }
}

/// One driven cell: what it ended as, and everything it spent getting there.
#[derive(Clone, Debug)]
pub struct AttemptedRun {
    /// The terminal outcome the submission-assembler maps to pass^k.
    pub outcome: RunOutcome,
    /// The failure that ended the cell, if it ended in one.
    pub last_failure: Option<FailureKind>,
    /// Every attempt, in order, including the failed ones a later completion
    /// supersedes. Rank-neutral: it never reaches a score.
    pub ledger: AttemptLedger,
}

/// Optional usage accompanies both successful and failed attempt outcomes.
pub struct AttemptObservation {
    pub result: Result<Run, FailureKind>,
    pub usage: Option<crate::accounting::AttemptUsage>,
}

impl From<Result<Run, FailureKind>> for AttemptObservation {
    fn from(result: Result<Run, FailureKind>) -> Self {
        Self {
            result,
            usage: None,
        }
    }
}

/// Drive one (window, seed) run with bounded retries on runtime errors.
///
/// `attempt` produces either a scorable [`Run`] (`Ok`) or a typed [`FailureKind`]
/// (`Err`). A runtime error ([`FailureKind::is_runtime`]) is retried up to
/// `max_retries` additional times; an agent-fault is returned immediately.
///
/// Every attempt is timed on the host clock and recorded, whether it succeeded
/// or failed, so a cell that failed twice before completing reports the time
/// those two failures actually spent instead of reporting only the completion.
pub fn run_with_retries<F>(max_retries: u32, mut attempt: F) -> AttemptedRun
where
    F: FnMut() -> Result<Run, FailureKind>,
{
    run_with_observed_retries(max_retries, || attempt().into())
}

/// Retry while preserving usage from every observation, including failures.
/// Failed attempts never establish complete billing, even if some decisions
/// carried counts: the failed request itself may have consumed unobserved work.
pub fn run_with_observed_retries<F>(max_retries: u32, attempt: F) -> AttemptedRun
where
    F: FnMut() -> AttemptObservation,
{
    run_with_backoff(
        max_retries,
        &BackoffSchedule::immediate(),
        &mut ThreadSleeper,
        attempt,
    )
}

/// One scheduled wait between a failed attempt and its retry, as recorded in
/// the ledger.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Backoff {
    /// Which retry this wait preceded, counting from 1.
    pub retry: u32,
    /// The scheduled wait in nanoseconds. This is what the schedule asked for;
    /// a real sleeper waits at least this long.
    pub delay_ns: u64,
}

/// An explicit, deterministic wait schedule for runtime-failure retries.
///
/// Entry `i` is the wait before retry `i + 1`; retries past the end reuse the
/// last entry. The empty schedule is immediate retry, the behaviour before
/// schedules existed, and it records nothing. Nothing here reads a clock: the
/// schedule is data, and the only thing that waits is the injected [`Sleeper`].
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct BackoffSchedule {
    pub delays_ns: Vec<u64>,
}

impl BackoffSchedule {
    /// Retry immediately. Records no backoff and changes no identity.
    pub fn immediate() -> Self {
        Self::default()
    }

    /// Wait `delays[i]` before retry `i + 1`, holding the last delay after.
    pub fn from_delays(delays: &[std::time::Duration]) -> Self {
        Self {
            delays_ns: delays
                .iter()
                .map(|delay| u64::try_from(delay.as_nanos()).unwrap_or(u64::MAX))
                .collect(),
        }
    }

    /// Whether this schedule never waits.
    pub fn is_immediate(&self) -> bool {
        self.delays_ns.iter().all(|&delay| delay == 0)
    }

    /// The wait before retry number `retry` (1-based), or `None` for none.
    pub fn delay_before(&self, retry: u32) -> Option<std::time::Duration> {
        let index = usize::try_from(retry.saturating_sub(1)).unwrap_or(usize::MAX);
        let delay = self
            .delays_ns
            .get(index)
            .or(self.delays_ns.last())
            .copied()?;
        (delay > 0).then(|| std::time::Duration::from_nanos(delay))
    }

    /// Fold this schedule into a sweep's `invocation_sha256`.
    ///
    /// A wait can change results: against a transiently degraded endpoint it
    /// decides whether a retry lands after recovery, and so which cells
    /// complete and which exhaust. A checkpoint resumed under a different
    /// schedule would mix two retry policies in one pool, so the schedule is
    /// bound into the invocation identity. The immediate schedule returns the
    /// digest unchanged, so every existing checkpoint contract stays valid.
    pub fn bind_invocation(&self, invocation_sha256: &str) -> String {
        if self.is_immediate() {
            return invocation_sha256.to_string();
        }
        let delays = self
            .delays_ns
            .iter()
            .map(u64::to_string)
            .collect::<Vec<_>>()
            .join(",");
        sharpebench_attest::content_digest(
            format!("sharpebench-retry-backoff-v1|{invocation_sha256}|{delays}").as_bytes(),
        )
    }
}

/// Where a retry driver waits. Injected so tests run a schedule without real
/// time passing and so the scoring kernel never sees a clock.
pub trait Sleeper {
    fn sleep(&mut self, delay: std::time::Duration);
}

/// Waits on the calling thread.
#[derive(Clone, Copy, Debug, Default)]
pub struct ThreadSleeper;

impl Sleeper for ThreadSleeper {
    fn sleep(&mut self, delay: std::time::Duration) {
        std::thread::sleep(delay);
    }
}

/// [`run_with_observed_retries`] under an explicit backoff schedule. Before each
/// retry of a runtime error the driver records the scheduled wait on the failed
/// attempt's ledger entry, then waits through `sleeper`. The wait is never
/// inside an attempt's `duration`. Agent faults are still final and never
/// wait.
pub fn run_with_backoff<F>(
    max_retries: u32,
    schedule: &BackoffSchedule,
    sleeper: &mut dyn Sleeper,
    mut attempt: F,
) -> AttemptedRun
where
    F: FnMut() -> AttemptObservation,
{
    run_with_faulted_backoff(max_retries, schedule, sleeper, || attempt().into())
}

/// [`run_with_observed_retries`] carrying each attempt's injected-fault
/// evidence onto its ledger record.
pub fn run_with_faulted_retries<F>(max_retries: u32, attempt: F) -> AttemptedRun
where
    F: FnMut() -> crate::fault_plan::FaultedObservation,
{
    run_with_faulted_backoff(
        max_retries,
        &BackoffSchedule::immediate(),
        &mut ThreadSleeper,
        attempt,
    )
}

/// [`run_with_backoff`] carrying each attempt's injected-fault evidence onto
/// its ledger record. Both retry drivers reduce to this one loop, so a faulted
/// sweep and a backed-off sweep record identically.
pub fn run_with_faulted_backoff<F>(
    max_retries: u32,
    schedule: &BackoffSchedule,
    sleeper: &mut dyn Sleeper,
    mut attempt: F,
) -> AttemptedRun
where
    F: FnMut() -> crate::fault_plan::FaultedObservation,
{
    let mut tries: u32 = 0;
    let mut ledger = AttemptLedger::default();
    loop {
        tries += 1;
        let started = std::time::Instant::now();
        let crate::fault_plan::FaultedObservation {
            observation: AttemptObservation { result, mut usage },
            injected_faults,
        } = attempt();
        if result.is_err() {
            if let Some(usage) = &mut usage {
                usage.complete = false;
            }
        }
        let duration = AttemptDuration::HostClock {
            nanos: u64::try_from(started.elapsed().as_nanos()).unwrap_or(u64::MAX),
        };
        match result {
            Ok(run) => {
                let mut record = AttemptRecord::completed(duration);
                record.usage = usage;
                record.injected_faults = injected_faults;
                ledger.push(record);
                return AttemptedRun {
                    outcome: RunOutcome::Completed(run),
                    last_failure: None,
                    ledger,
                };
            }
            Err(kind) => {
                let mut record = AttemptRecord::failed(kind.clone(), duration);
                record.usage = usage;
                record.injected_faults = injected_faults;
                if !kind.is_runtime() {
                    ledger.push(record);
                    return AttemptedRun {
                        outcome: RunOutcome::AgentFault(kind.clone()),
                        last_failure: Some(kind),
                        ledger,
                    };
                }
                if tries > max_retries || tries == u32::MAX {
                    ledger.push(record);
                    return AttemptedRun {
                        outcome: RunOutcome::Exhausted {
                            last: kind.clone(),
                            attempts: tries,
                        },
                        last_failure: Some(kind),
                        ledger,
                    };
                }
                let delay = schedule.delay_before(tries);
                record.backoff_after = delay.map(|delay| Backoff {
                    retry: tries,
                    delay_ns: u64::try_from(delay.as_nanos()).unwrap_or(u64::MAX),
                });
                ledger.push(record);
                if let Some(delay) = delay {
                    sleeper.sleep(delay);
                }
            }
        }
    }
}

/// A run that is guaranteed to *fail* the per-run pass^k bar — the scorable
/// stand-in for an `AgentFault`. Its returns are a flat negative drift so the
/// run's probabilistic Sharpe sits far below any sane bar, marking it a genuine
/// agent failure without inventing fake positive performance.
pub fn failing_sentinel_run(len: usize) -> Run {
    Run {
        returns: vec![-0.01; len.max(1)],
        trace: sharpebench_core::Trace::default(),
        confidences: Vec::new(),
        outcomes: Vec::new(),
        cost: 0.0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runtime_error_is_retried_then_recovers() {
        let mut calls = 0;
        let driven = run_with_retries(3, || {
            calls += 1;
            if calls < 3 {
                Err(FailureKind::TransportError)
            } else {
                Ok(failing_sentinel_run(5))
            }
        });
        assert!(matches!(driven.outcome, RunOutcome::Completed(_)));
        assert_eq!(calls, 3, "should retry until it recovers");
        assert_eq!(
            driven.ledger.len(),
            3,
            "the two failures stay in the ledger"
        );
    }

    #[test]
    fn runtime_error_exhausts_after_bounded_retries() {
        let mut calls = 0;
        let driven = run_with_retries(2, || {
            calls += 1;
            Err(FailureKind::SpawnError)
        });
        // 1 initial + 2 retries = 3 attempts.
        assert_eq!(calls, 3);
        match driven.outcome {
            RunOutcome::Exhausted { last, attempts } => {
                assert_eq!(last, FailureKind::SpawnError);
                assert_eq!(attempts, 3);
            }
            other => panic!("expected Exhausted, got {other:?}"),
        }
        assert_eq!(driven.last_failure, Some(FailureKind::SpawnError));
    }

    #[test]
    fn agent_fault_is_not_retried() {
        let mut calls = 0;
        let driven = run_with_retries(5, || {
            calls += 1;
            Err(FailureKind::AgentProtocolViolation)
        });
        assert_eq!(calls, 1, "an agent fault must not be retried");
        assert!(matches!(driven.outcome, RunOutcome::AgentFault(_)));
    }

    /// An OOM kill must override every other outcome: a transport-classified
    /// failure (the dead pipe the kill leaves behind) and even a clean run,
    /// because the budget breach is scoring-relevant regardless of what made it
    /// onto the wire before the kill.
    #[test]
    fn an_oom_kill_overrides_both_a_transport_failure_and_a_clean_run() {
        assert_eq!(
            apply_oom_verdict(Err(FailureKind::TransportError), Some(true)).unwrap_err(),
            FailureKind::ResourceLimitExceeded,
        );
        assert_eq!(
            apply_oom_verdict(Ok(failing_sentinel_run(5)), Some(true)).unwrap_err(),
            FailureKind::ResourceLimitExceeded,
        );
    }

    /// Without an OOM verdict the result must pass through untouched — both the
    /// no-container case (`None`) and an inspected container that was not
    /// OOM-killed (`Some(false)`).
    #[test]
    fn no_oom_verdict_leaves_the_result_untouched() {
        for verdict in [None, Some(false)] {
            assert!(apply_oom_verdict(Ok(failing_sentinel_run(5)), verdict).is_ok());
            assert_eq!(
                apply_oom_verdict(Err(FailureKind::Timeout), verdict).unwrap_err(),
                FailureKind::Timeout,
            );
        }
    }

    /// Rerunning an agent against the same published budget reproduces the same
    /// OOM, so a budget breach must be a final agent fault, never retried.
    #[test]
    fn a_resource_limit_breach_is_an_agent_fault_and_is_not_retried() {
        assert!(!FailureKind::ResourceLimitExceeded.is_runtime());
        let mut calls = 0;
        let driven = run_with_retries(5, || {
            calls += 1;
            Err(FailureKind::ResourceLimitExceeded)
        });
        assert_eq!(calls, 1, "a budget breach must not be retried");
        assert!(matches!(driven.outcome, RunOutcome::AgentFault(_)));
    }

    /// Records every requested wait and never waits.
    #[derive(Default)]
    struct FakeSleeper {
        waits: Vec<std::time::Duration>,
    }

    impl Sleeper for FakeSleeper {
        fn sleep(&mut self, delay: std::time::Duration) {
            self.waits.push(delay);
        }
    }

    fn secs(values: &[u64]) -> Vec<std::time::Duration> {
        values
            .iter()
            .map(|&s| std::time::Duration::from_secs(s))
            .collect()
    }

    /// The archive's schedule (5 s, then 15 s) over four retries: the sleeper
    /// is asked for exactly 5, 15, 15, 15 seconds in that order, each wait is
    /// recorded on the failure that preceded it, and the final exhausted
    /// attempt, which is followed by no retry, records none.
    #[test]
    fn a_backoff_schedule_is_followed_exactly_and_recorded() {
        let schedule = BackoffSchedule::from_delays(&secs(&[5, 15]));
        let mut sleeper = FakeSleeper::default();
        let mut calls = 0;
        let driven = run_with_backoff(4, &schedule, &mut sleeper, || {
            calls += 1;
            Err(FailureKind::TransportError).into()
        });
        assert_eq!(calls, 5);
        assert_eq!(sleeper.waits, secs(&[5, 15, 15, 15]));
        let recorded: Vec<Option<Backoff>> = driven
            .ledger
            .attempts
            .iter()
            .map(|record| record.backoff_after)
            .collect();
        let backoff = |retry, s: u64| {
            Some(Backoff {
                retry,
                delay_ns: s * 1_000_000_000,
            })
        };
        assert_eq!(
            recorded,
            vec![
                backoff(1, 5),
                backoff(2, 15),
                backoff(3, 15),
                backoff(4, 15),
                None
            ]
        );
        assert_eq!(driven.ledger.summary().backoff_ns_total, 50_000_000_000);
        // The wait is kept out of the attempt durations: four sleeps of real
        // time would be 50 s, and a fake sleeper spends none.
        assert!(driven.ledger.summary().duration_ns_total < 50_000_000_000);
    }

    #[test]
    fn a_recovery_stops_the_schedule_and_an_agent_fault_never_waits() {
        let schedule = BackoffSchedule::from_delays(&secs(&[1, 2, 3]));
        let mut sleeper = FakeSleeper::default();
        let mut calls = 0;
        let driven = run_with_backoff(5, &schedule, &mut sleeper, || {
            calls += 1;
            if calls < 3 {
                Err(FailureKind::Timeout).into()
            } else {
                Ok(failing_sentinel_run(3)).into()
            }
        });
        assert!(matches!(driven.outcome, RunOutcome::Completed(_)));
        assert_eq!(sleeper.waits, secs(&[1, 2]));
        assert_eq!(driven.ledger.attempts[2].backoff_after, None);

        let mut sleeper = FakeSleeper::default();
        let driven = run_with_backoff(5, &schedule, &mut sleeper, || {
            Err(FailureKind::AgentProtocolViolation).into()
        });
        assert!(matches!(driven.outcome, RunOutcome::AgentFault(_)));
        assert!(sleeper.waits.is_empty());
        assert_eq!(driven.ledger.attempts[0].backoff_after, None);
    }

    /// Without a schedule nothing waits, nothing is recorded, and the ledger
    /// serializes exactly as it did before schedules existed.
    #[test]
    fn the_immediate_schedule_records_nothing_and_keeps_ledger_bytes() {
        let mut sleeper = FakeSleeper::default();
        let mut calls = 0;
        let driven = run_with_backoff(2, &BackoffSchedule::immediate(), &mut sleeper, || {
            calls += 1;
            Err(FailureKind::SpawnError).into()
        });
        assert!(sleeper.waits.is_empty());
        assert!(driven
            .ledger
            .attempts
            .iter()
            .all(|r| r.backoff_after.is_none()));
        let text = serde_json::to_string(&driven.ledger.attempts[0]).unwrap();
        assert!(!text.contains("backoff"), "{text}");
        let summary = serde_json::to_string(&driven.ledger.summary()).unwrap();
        assert!(!summary.contains("backoff"), "{summary}");
        // A ledger written before the field existed still reads.
        let legacy = r#"{"outcome":{"outcome":"completed"},"duration":{"source":"unavailable"}}"#;
        let record: AttemptRecord = serde_json::from_str(legacy).unwrap();
        assert_eq!(
            record,
            AttemptRecord::completed(AttemptDuration::Unavailable)
        );
    }

    #[test]
    fn only_a_waiting_schedule_changes_the_invocation_identity() {
        let invocation = "a".repeat(64);
        assert_eq!(
            BackoffSchedule::immediate().bind_invocation(&invocation),
            invocation
        );
        assert_eq!(
            BackoffSchedule::from_delays(&secs(&[0])).bind_invocation(&invocation),
            invocation
        );
        let five = BackoffSchedule::from_delays(&secs(&[5, 15])).bind_invocation(&invocation);
        let other = BackoffSchedule::from_delays(&secs(&[5, 16])).bind_invocation(&invocation);
        assert_ne!(five, invocation);
        assert_ne!(five, other);
        assert_eq!(five.len(), 64);
    }

    #[test]
    fn failure_log_separates_runtime_from_agent_faults() {
        let mut log = FailureLog::default();
        log.push(FailureRecord {
            window_index: 0,
            seed: 1,
            kind: FailureKind::Timeout,
            attempts: 4,
            runtime: true,
        });
        log.push(FailureRecord {
            window_index: 0,
            seed: 2,
            kind: FailureKind::AgentProtocolViolation,
            attempts: 1,
            runtime: false,
        });
        assert_eq!(log.runtime_failures(), 1);
        assert_eq!(log.agent_faults(), 1);
    }
}
