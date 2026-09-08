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
    pub duration: AttemptDuration,
}

impl AttemptRecord {
    pub fn completed(duration: AttemptDuration) -> Self {
        Self {
            outcome: AttemptOutcome::Completed,
            duration,
        }
    }

    pub fn failed(kind: FailureKind, duration: AttemptDuration) -> Self {
        Self {
            outcome: AttemptOutcome::Failed { kind },
            duration,
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
        let mut timed = 0usize;
        for record in &self.attempts {
            if let AttemptDuration::HostClock { nanos } = record.duration {
                duration_ns_total = duration_ns_total.saturating_add(nanos);
                timed += 1;
            }
        }
        AttemptSummary {
            attempts: self.attempts.len(),
            failed: self.attempts.iter().filter(|a| a.is_failure()).count(),
            completed: self.attempts.iter().filter(|a| !a.is_failure()).count(),
            duration_ns_total,
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
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct AttemptSummary {
    pub attempts: usize,
    pub completed: usize,
    pub failed: usize,
    pub duration_ns_total: u64,
    pub duration_source: DurationSource,
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
    let mut tries: u32 = 0;
    let mut ledger = AttemptLedger::default();
    loop {
        tries += 1;
        let started = std::time::Instant::now();
        let result = attempt();
        let duration = AttemptDuration::HostClock {
            nanos: u64::try_from(started.elapsed().as_nanos()).unwrap_or(u64::MAX),
        };
        match result {
            Ok(run) => {
                ledger.push(AttemptRecord::completed(duration));
                return AttemptedRun {
                    outcome: RunOutcome::Completed(run),
                    last_failure: None,
                    ledger,
                };
            }
            Err(kind) => {
                ledger.push(AttemptRecord::failed(kind.clone(), duration));
                if !kind.is_runtime() {
                    return AttemptedRun {
                        outcome: RunOutcome::AgentFault(kind.clone()),
                        last_failure: Some(kind),
                        ledger,
                    };
                }
                if tries > max_retries {
                    return AttemptedRun {
                        outcome: RunOutcome::Exhausted {
                            last: kind.clone(),
                            attempts: tries,
                        },
                        last_failure: Some(kind),
                        ledger,
                    };
                }
                // else: loop and retry
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
