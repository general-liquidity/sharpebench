//! Resumable, crash-tolerant checkpointing for the **external-agent** sweep.
//!
//! [`run_agent`](crate::run_agent) builds the whole window × seed matrix in one
//! in-memory loop: a crash mid-sweep loses every completed run. For a reference
//! in-process agent that is cheap (just re-run), but an external LLM agent is
//! expensive and slow - losing a half-finished sweep is real money and wall-clock.
//!
//! This module persists per-task status (`pending | claimed | done | failed`) to a
//! JSON checkpoint file after every task, so an interrupted sweep resumes and runs
//! **only** the tasks that did not finish. A completed checkpoint is a no-op. The
//! claim / reset-stale primitives ([`SweepCheckpoint::claim_next`] /
//! [`SweepCheckpoint::reset_stale`]) also support an optional multi-worker pool: a
//! worker claims the next pending task (stamped with a caller-supplied monotonic
//! `epoch`, not a wall clock - the kernel stays deterministic), and a stale claim
//! left by a dead worker is reset back to pending.
//!
//! Determinism + attestation: runs are seeded by (window, seed), so the assembled
//! submission from a resumed sweep is byte-identical to an uninterrupted one - the
//! checkpoint changes *when* work happens, never *what* it computes.

use std::path::Path;

use serde::{Deserialize, Serialize};
use sharpebench_core::{AgentSubmission, Run};
use sharpebench_sim::Window;

use crate::failure::{
    failing_sentinel_run, run_with_retries, FailureKind, FailureLog, FailureRecord, RunOutcome,
};
use crate::ResilientSubmission;

/// The lifecycle state of one (window, seed) task in the sweep.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "state")]
pub enum TaskState {
    /// Not yet run.
    Pending,
    /// Claimed by a worker at a monotonic `epoch`; in flight.
    Claimed { worker: u64, epoch: u64 },
    /// Completed with a scorable run (stored in [`TaskRecord::run`]).
    Done,
    /// A runtime/harness error exhausted its retries - excluded from the pass^k pool
    /// (the harness's fault, not the agent's), but recorded.
    RuntimeFailed { kind: FailureKind, attempts: u32 },
    /// A non-retryable agent fault - a failing sentinel run (in [`TaskRecord::run`])
    /// counts against pass^k.
    AgentFailed { kind: FailureKind },
}

/// One task in the sweep matrix: its (window index, seed) coordinates, its lifecycle
/// state, and - once terminal - the run it produced (a real run for `Done`, a failing
/// sentinel for `AgentFailed`).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TaskRecord {
    /// 0-based index into the sweep's `windows` slice.
    pub window: usize,
    /// Execution seed.
    pub seed: u64,
    pub state: TaskState,
    /// The scorable run, present for `Done` (real) and `AgentFailed` (sentinel).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub run: Option<Run>,
}

impl TaskRecord {
    fn is_terminal(&self) -> bool {
        matches!(
            self.state,
            TaskState::Done | TaskState::RuntimeFailed { .. } | TaskState::AgentFailed { .. }
        )
    }
}

/// A persisted, resumable view of an external-agent sweep: the ordered task matrix
/// (window-major, matching [`run_agent`](crate::run_agent)'s layout) plus the agent
/// id it belongs to.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SweepCheckpoint {
    pub agent_id: String,
    pub tasks: Vec<TaskRecord>,
}

impl SweepCheckpoint {
    /// A fresh checkpoint with every (window, seed) task pending, laid out
    /// window-major (all seeds of window 0, then window 1, …) - the identical order
    /// [`run_agent`](crate::run_agent) produces, so the assembled submission lines up.
    pub fn new(agent_id: &str, n_windows: usize, seeds: &[u64]) -> Self {
        let mut tasks = Vec::with_capacity(n_windows * seeds.len());
        for w in 0..n_windows {
            for &seed in seeds {
                tasks.push(TaskRecord {
                    window: w,
                    seed,
                    state: TaskState::Pending,
                    run: None,
                });
            }
        }
        Self {
            agent_id: agent_id.to_string(),
            tasks,
        }
    }

    /// Does this checkpoint describe the given agent + (n_windows × seeds) matrix, in
    /// order? A mismatch means the file belongs to a different sweep and must not be
    /// resumed against this one.
    pub fn matches(&self, agent_id: &str, n_windows: usize, seeds: &[u64]) -> bool {
        if self.agent_id != agent_id || self.tasks.len() != n_windows * seeds.len() {
            return false;
        }
        let mut idx = 0;
        for w in 0..n_windows {
            for &seed in seeds {
                let t = &self.tasks[idx];
                if t.window != w || t.seed != seed {
                    return false;
                }
                idx += 1;
            }
        }
        true
    }

    /// Load a checkpoint from `path`. A serde error is surfaced as an I/O error.
    pub fn load(path: &Path) -> std::io::Result<Self> {
        let raw = std::fs::read_to_string(path)?;
        serde_json::from_str(&raw).map_err(std::io::Error::other)
    }

    /// Atomically-enough persist the checkpoint to `path` (write to a sibling temp
    /// file, then rename), so a crash mid-write can't corrupt the resume point.
    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        let payload = serde_json::to_string_pretty(self).map_err(std::io::Error::other)?;
        let tmp = path.with_extension("json.tmp");
        std::fs::write(&tmp, payload)?;
        std::fs::rename(&tmp, path)
    }

    /// Number of non-terminal (pending or claimed) tasks left.
    pub fn remaining(&self) -> usize {
        self.tasks.iter().filter(|t| !t.is_terminal()).count()
    }

    /// Whether every task has reached a terminal state.
    pub fn is_complete(&self) -> bool {
        self.remaining() == 0
    }

    /// Revert every in-flight claim back to pending - used on resume, when any
    /// `Claimed` task was left behind by an interrupted run.
    pub fn requeue_claimed(&mut self) {
        for t in &mut self.tasks {
            if matches!(t.state, TaskState::Claimed { .. }) {
                t.state = TaskState::Pending;
            }
        }
    }

    /// Claim the first pending task for `worker` at monotonic `epoch`, returning its
    /// (window index, seed). `None` when nothing is pending. The claim is what lets a
    /// multi-worker pool divide the sweep without double-running a task.
    pub fn claim_next(&mut self, worker: u64, epoch: u64) -> Option<(usize, u64)> {
        let t = self
            .tasks
            .iter_mut()
            .find(|t| matches!(t.state, TaskState::Pending))?;
        t.state = TaskState::Claimed { worker, epoch };
        Some((t.window, t.seed))
    }

    /// Reset any claim older than `ttl` epochs (i.e. `now - epoch > ttl`) back to
    /// pending, so a task a dead worker never finished is reclaimable. Returns how
    /// many were reset. Deterministic: staleness is measured in the caller's
    /// monotonic epoch units, never a wall clock.
    pub fn reset_stale(&mut self, now: u64, ttl: u64) -> usize {
        let mut n = 0;
        for t in &mut self.tasks {
            if let TaskState::Claimed { epoch, .. } = t.state {
                if now.saturating_sub(epoch) > ttl {
                    t.state = TaskState::Pending;
                    n += 1;
                }
            }
        }
        n
    }

    fn task_mut(&mut self, window: usize, seed: u64) -> Option<&mut TaskRecord> {
        self.tasks
            .iter_mut()
            .find(|t| t.window == window && t.seed == seed)
    }

    /// Mark a task done with its scorable run.
    pub fn complete(&mut self, window: usize, seed: u64, run: Run) {
        if let Some(t) = self.task_mut(window, seed) {
            t.state = TaskState::Done;
            t.run = Some(run);
        }
    }

    /// Mark a task as an exhausted runtime failure (excluded from the score).
    pub fn fail_runtime(&mut self, window: usize, seed: u64, kind: FailureKind, attempts: u32) {
        if let Some(t) = self.task_mut(window, seed) {
            t.state = TaskState::RuntimeFailed { kind, attempts };
            t.run = None;
        }
    }

    /// Mark a task as an agent fault, storing the failing sentinel run that counts
    /// against pass^k.
    pub fn fail_agent(&mut self, window: usize, seed: u64, kind: FailureKind, sentinel: Run) {
        if let Some(t) = self.task_mut(window, seed) {
            t.state = TaskState::AgentFailed { kind };
            t.run = Some(sentinel);
        }
    }

    /// Assemble the terminal tasks into the submission + failure log the scorer
    /// consumes - the identical pool [`run_agent_resilient`](crate::run_agent_resilient)
    /// produces for the same outcomes: `Done` and `AgentFailed` (sentinel) runs feed
    /// pass^k in window-major order; runtime failures are logged but never scored.
    pub fn assemble(&self) -> ResilientSubmission {
        let mut runs = Vec::new();
        let mut failures = FailureLog::default();
        for t in &self.tasks {
            match &t.state {
                TaskState::Done => {
                    if let Some(r) = &t.run {
                        runs.push(r.clone());
                    }
                }
                TaskState::AgentFailed { kind } => {
                    if let Some(r) = &t.run {
                        runs.push(r.clone());
                    }
                    failures.push(FailureRecord {
                        window_index: t.window,
                        seed: t.seed,
                        kind: kind.clone(),
                        attempts: 1,
                        runtime: false,
                    });
                }
                TaskState::RuntimeFailed { kind, attempts } => {
                    failures.push(FailureRecord {
                        window_index: t.window,
                        seed: t.seed,
                        kind: kind.clone(),
                        attempts: *attempts,
                        runtime: true,
                    });
                }
                TaskState::Pending | TaskState::Claimed { .. } => {}
            }
        }
        ResilientSubmission {
            submission: AgentSubmission {
                agent_id: self.agent_id.clone(),
                runs,
                in_sample_trials: 0,
                candidates: Vec::new(),
            },
            failures,
        }
    }
}

/// Run (or resume) an external-agent sweep with a JSON checkpoint at `path`.
///
/// Scoped to the **external-agent path**: each task's `attempt` is expected to spawn
/// and drive a fresh external agent (the reference in-process agents are cheap and
/// need no checkpoint). If `path` holds a checkpoint for the same agent + matrix, the
/// sweep resumes - completed tasks are skipped, any interrupted claim is requeued,
/// and only the remaining tasks run. Progress is persisted after every task, so a
/// crash loses at most one task. Returns the assembled submission + failure log.
pub fn run_resumable_sweep<F>(
    path: &Path,
    agent_id: &str,
    windows: &[Window],
    seeds: &[u64],
    max_retries: u32,
    mut attempt: F,
) -> std::io::Result<ResilientSubmission>
where
    F: FnMut(usize, u64) -> Result<Run, FailureKind>,
{
    let mut cp = match SweepCheckpoint::load(path) {
        Ok(existing) if existing.matches(agent_id, windows.len(), seeds) => {
            let mut cp = existing;
            cp.requeue_claimed();
            cp
        }
        _ => SweepCheckpoint::new(agent_id, windows.len(), seeds),
    };

    let expected_len = windows
        .first()
        .map(|w| w.end.saturating_sub(w.start))
        .unwrap_or(0);

    // Single-worker driver: claim the next pending task, run it under the retry
    // taxonomy, record the outcome, and persist before moving on.
    while let Some((w, seed)) = cp.claim_next(0, 0) {
        let (outcome, _) = run_with_retries(max_retries, || attempt(w, seed));
        match outcome {
            RunOutcome::Completed(run) => cp.complete(w, seed, run),
            RunOutcome::Exhausted { last, attempts } => cp.fail_runtime(w, seed, last, attempts),
            RunOutcome::AgentFault(kind) => {
                cp.fail_agent(w, seed, kind, failing_sentinel_run(expected_len))
            }
        }
        cp.save(path)?;
    }

    Ok(cp.assemble())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp_path(tag: &str) -> std::path::PathBuf {
        use std::sync::atomic::{AtomicU64, Ordering};
        static N: AtomicU64 = AtomicU64::new(0);
        let n = N.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir().join(format!(
            "sharpebench-ckpt-{}-{}-{tag}.json",
            std::process::id(),
            n
        ))
    }

    fn skilled_run(seed: u64) -> Run {
        Run {
            returns: (0..40)
                .map(|i| 0.002 + 0.0005 * ((i + seed as usize) as f64 * 0.7).sin())
                .collect(),
            trace: Default::default(),
            confidences: Vec::new(),
            outcomes: Vec::new(),
            cost: 0.0,
        }
    }

    #[test]
    fn checkpoint_roundtrips_and_reports_progress() {
        let seeds = [0u64, 1, 2];
        let mut cp = SweepCheckpoint::new("agent", 2, &seeds); // 6 tasks
        assert_eq!(cp.tasks.len(), 6);
        assert_eq!(cp.remaining(), 6);
        cp.complete(0, 0, skilled_run(0));
        cp.complete(0, 1, skilled_run(1));
        assert_eq!(cp.remaining(), 4);

        // Round-trips through JSON with progress intact.
        let json = serde_json::to_string(&cp).unwrap();
        let back: SweepCheckpoint = serde_json::from_str(&json).unwrap();
        assert_eq!(back.remaining(), 4);
        assert!(back.matches("agent", 2, &seeds));
        assert!(!back.matches("other", 2, &seeds));
    }

    #[test]
    fn claim_and_reset_stale_support_a_worker_pool() {
        let mut cp = SweepCheckpoint::new("a", 1, &[0, 1]);
        // Two workers each claim a task at epoch 0.
        let t0 = cp.claim_next(1, 0).unwrap();
        let t1 = cp.claim_next(2, 0).unwrap();
        assert_ne!(t0, t1, "distinct tasks handed out");
        assert!(cp.claim_next(3, 0).is_none(), "nothing left to claim");

        // Worker 2 dies; at epoch 10 with ttl 5 its claim is stale and reclaimable.
        assert_eq!(cp.reset_stale(10, 5), 2, "both stale claims reset");
        assert!(cp.claim_next(4, 11).is_some(), "reclaimed after reset");
    }

    #[test]
    fn interrupted_sweep_resumes_only_the_remaining_tasks() {
        let path = tmp_path("resume");
        let windows = [Window { start: 20, end: 60 }];
        let seeds = [0u64, 1, 2, 3];
        let attempt = |_w: usize, seed: u64| Ok(skilled_run(seed));

        // Simulate a crash after 2 of 4 tasks: build a checkpoint, complete two,
        // persist it (as an interrupted run would have).
        let mut cp = SweepCheckpoint::new("ext", windows.len(), &seeds);
        for _ in 0..2 {
            let (w, seed) = cp.claim_next(0, 0).unwrap();
            cp.complete(w, seed, skilled_run(seed));
        }
        cp.save(&path).unwrap();
        assert_eq!(cp.remaining(), 2);

        // Resume: only the remaining 2 tasks run.
        let mut ran = 0u32;
        let pool = run_resumable_sweep(&path, "ext", &windows, &seeds, 2, |w, seed| {
            ran += 1;
            attempt(w, seed)
        })
        .unwrap();
        assert_eq!(ran, 2, "resume runs only the 2 unfinished tasks");
        assert_eq!(pool.submission.runs.len(), 4, "all 4 runs assembled");

        // A completed checkpoint is a no-op.
        let mut ran2 = 0u32;
        let pool2 = run_resumable_sweep(&path, "ext", &windows, &seeds, 2, |w, seed| {
            ran2 += 1;
            attempt(w, seed)
        })
        .unwrap();
        assert_eq!(ran2, 0, "a completed checkpoint reruns nothing");
        assert_eq!(pool2.submission.runs.len(), 4);

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn resumed_sweep_is_byte_identical_to_an_uninterrupted_one() {
        let windows = [Window { start: 20, end: 60 }];
        let seeds = [0u64, 1, 2, 3];
        let attempt = |_w: usize, seed: u64| Ok(skilled_run(seed));

        // Uninterrupted run.
        let full_path = tmp_path("full");
        let full = run_resumable_sweep(&full_path, "ext", &windows, &seeds, 2, attempt).unwrap();

        // Interrupted-then-resumed run, over a separate file.
        let part_path = tmp_path("part");
        let mut cp = SweepCheckpoint::new("ext", windows.len(), &seeds);
        for _ in 0..3 {
            let (w, seed) = cp.claim_next(0, 0).unwrap();
            cp.complete(w, seed, skilled_run(seed));
        }
        cp.save(&part_path).unwrap();
        let resumed = run_resumable_sweep(&part_path, "ext", &windows, &seeds, 2, attempt).unwrap();

        assert_eq!(
            serde_json::to_string(&full.submission).unwrap(),
            serde_json::to_string(&resumed.submission).unwrap(),
            "a resumed sweep must assemble byte-identically to an uninterrupted one"
        );

        let _ = std::fs::remove_file(&full_path);
        let _ = std::fs::remove_file(&part_path);
    }

    #[test]
    fn agent_and_runtime_failures_flow_into_the_assembled_pool() {
        let path = tmp_path("fail");
        let windows = [Window { start: 20, end: 60 }];
        let seeds = [0u64, 1, 2];
        let pool = run_resumable_sweep(&path, "ext", &windows, &seeds, 1, |_w, seed| match seed {
            0 => Ok(skilled_run(0)),
            1 => Err(FailureKind::AgentProtocolViolation), // agent fault → sentinel
            _ => Err(FailureKind::TransportError),         // runtime → exhausted
        })
        .unwrap();
        // Done + AgentFailed contribute runs; the exhausted runtime failure does not.
        assert_eq!(pool.submission.runs.len(), 2);
        assert_eq!(pool.failures.agent_faults(), 1);
        assert_eq!(pool.failures.runtime_failures(), 1);
        let _ = std::fs::remove_file(&path);
    }
}
