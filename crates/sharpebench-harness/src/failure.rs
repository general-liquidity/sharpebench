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

/// Drive one (window, seed) run with bounded retries on runtime errors.
///
/// `attempt` produces either a scorable [`Run`] (`Ok`) or a typed [`FailureKind`]
/// (`Err`). A runtime error ([`FailureKind::is_runtime`]) is retried up to
/// `max_retries` additional times; an agent-fault is returned immediately. The
/// returned [`RunOutcome`] is what the submission-assembler maps to pass^k.
pub fn run_with_retries<F>(max_retries: u32, mut attempt: F) -> (RunOutcome, Option<FailureKind>)
where
    F: FnMut() -> Result<Run, FailureKind>,
{
    let mut tries: u32 = 0;
    loop {
        tries += 1;
        match attempt() {
            Ok(run) => return (RunOutcome::Completed(run), None),
            Err(kind) if kind.is_runtime() => {
                if tries > max_retries {
                    return (
                        RunOutcome::Exhausted {
                            last: kind.clone(),
                            attempts: tries,
                        },
                        Some(kind),
                    );
                }
                // else: loop and retry
            }
            Err(kind) => return (RunOutcome::AgentFault(kind.clone()), Some(kind)),
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
        let (outcome, _) = run_with_retries(3, || {
            calls += 1;
            if calls < 3 {
                Err(FailureKind::TransportError)
            } else {
                Ok(failing_sentinel_run(5))
            }
        });
        assert!(matches!(outcome, RunOutcome::Completed(_)));
        assert_eq!(calls, 3, "should retry until it recovers");
    }

    #[test]
    fn runtime_error_exhausts_after_bounded_retries() {
        let mut calls = 0;
        let (outcome, last) = run_with_retries(2, || {
            calls += 1;
            Err(FailureKind::SpawnError)
        });
        // 1 initial + 2 retries = 3 attempts.
        assert_eq!(calls, 3);
        match outcome {
            RunOutcome::Exhausted { last, attempts } => {
                assert_eq!(last, FailureKind::SpawnError);
                assert_eq!(attempts, 3);
            }
            other => panic!("expected Exhausted, got {other:?}"),
        }
        assert_eq!(last, Some(FailureKind::SpawnError));
    }

    #[test]
    fn agent_fault_is_not_retried() {
        let mut calls = 0;
        let (outcome, _) = run_with_retries(5, || {
            calls += 1;
            Err(FailureKind::AgentProtocolViolation)
        });
        assert_eq!(calls, 1, "an agent fault must not be retried");
        assert!(matches!(outcome, RunOutcome::AgentFault(_)));
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
