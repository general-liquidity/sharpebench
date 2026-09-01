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

use std::io::Write as _;
use std::path::Path;

use serde::{Deserialize, Serialize};
use sharpebench_core::{AgentSubmission, Run};
use sharpebench_sim::Window;

use crate::failure::{
    failing_sentinel_run, run_with_retries, FailureKind, FailureLog, FailureRecord, RunOutcome,
};
use crate::ResilientSubmission;

/// Versioned identity of every condition that can change a resumable sweep's
/// result. A checkpoint is reusable only when this record matches exactly.
///
/// The digests bind semantic inputs without copying a dataset, secrets, or a
/// binary into the checkpoint. Exact windows and seeds stay visible because
/// they are useful diagnostics rather than opaque implementation details.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SweepIdentity {
    pub dataset_sha256: String,
    pub cost_model_sha256: String,
    pub score_config_sha256: String,
    pub runner_artifact_sha256: String,
    pub entrant_sha256: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SweepContract {
    pub schema_version: u32,
    pub dataset_sha256: String,
    pub cost_model_sha256: String,
    pub score_config_sha256: String,
    pub runner_artifact_sha256: String,
    pub entrant_sha256: String,
    pub windows: Vec<(usize, usize)>,
    pub seeds: Vec<u64>,
    pub max_retries: u32,
}

impl SweepContract {
    pub const SCHEMA_VERSION: u32 = 1;

    /// Build the contract from already-computed SHA-256 identities.
    pub fn new(
        identity: SweepIdentity,
        windows: &[Window],
        seeds: &[u64],
        max_retries: u32,
    ) -> Self {
        Self {
            schema_version: Self::SCHEMA_VERSION,
            dataset_sha256: identity.dataset_sha256,
            cost_model_sha256: identity.cost_model_sha256,
            score_config_sha256: identity.score_config_sha256,
            runner_artifact_sha256: identity.runner_artifact_sha256,
            entrant_sha256: identity.entrant_sha256,
            windows: windows.iter().map(|w| (w.start, w.end)).collect(),
            seeds: seeds.to_vec(),
            max_retries,
        }
    }

    fn matches_execution(&self, windows: &[Window], seeds: &[u64], max_retries: u32) -> bool {
        let valid_digest = |digest: &str| {
            digest.len() == 64
                && digest
                    .bytes()
                    .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        };
        self.schema_version == Self::SCHEMA_VERSION
            && [
                &self.dataset_sha256,
                &self.cost_model_sha256,
                &self.score_config_sha256,
                &self.runner_artifact_sha256,
                &self.entrant_sha256,
            ]
            .into_iter()
            .all(|digest| valid_digest(digest))
            && self.windows == windows.iter().map(|w| (w.start, w.end)).collect::<Vec<_>>()
            && self.seeds == seeds
            && self.max_retries == max_retries
    }
}

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
    /// Absent only in legacy checkpoints created through the compatibility API.
    /// The CLI uses the bound API and refuses an absent or different contract.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub contract: Option<SweepContract>,
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
            contract: None,
            tasks,
        }
    }

    /// A fresh checkpoint bound to the full execution contract.
    pub fn new_bound(agent_id: &str, contract: SweepContract) -> Self {
        let mut checkpoint = Self::new(agent_id, contract.windows.len(), &contract.seeds);
        checkpoint.contract = Some(contract);
        checkpoint
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

    /// Whether this checkpoint belongs to exactly this experiment. The legacy
    /// agent/matrix match is necessary but not sufficient: the same matrix can
    /// be run over different prices, costs, scorer settings, binaries, or
    /// entrant artifacts.
    pub fn matches_bound(&self, agent_id: &str, contract: &SweepContract) -> bool {
        self.contract.as_ref() == Some(contract)
            && self.matches(agent_id, contract.windows.len(), &contract.seeds)
    }

    /// Load a checkpoint from `path`. A serde error is surfaced as an I/O error.
    pub fn load(path: &Path) -> std::io::Result<Self> {
        let raw = std::fs::read_to_string(path)?;
        serde_json::from_str(&raw).map_err(std::io::Error::other)
    }

    /// Persist through a sibling temporary file, sync its bytes, rename it, and
    /// sync the containing directory on Unix. A successful return therefore
    /// means the checkpoint is durable, not merely present in a page cache.
    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        let payload = serde_json::to_string_pretty(self).map_err(std::io::Error::other)?;
        let tmp = path.with_extension("json.tmp");
        let result = (|| {
            let mut file = std::fs::File::create(&tmp)?;
            file.write_all(payload.as_bytes())?;
            file.sync_all()?;
            drop(file);
            std::fs::rename(&tmp, path)?;
            #[cfg(unix)]
            if let Some(parent) = path.parent() {
                std::fs::File::open(parent)?.sync_all()?;
            }
            Ok(())
        })();
        if result.is_err() {
            let _ = std::fs::remove_file(&tmp);
        }
        result
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

    fn validate_terminal(&self, windows: &[Window]) -> std::io::Result<()> {
        for (index, task) in self.tasks.iter().enumerate() {
            let expected_len = windows
                .get(task.window)
                .map(|window| window.end.saturating_sub(window.start))
                .ok_or_else(|| {
                    std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        format!(
                            "checkpoint task {index} names absent window {}",
                            task.window
                        ),
                    )
                })?;
            match (&task.state, &task.run) {
                (TaskState::Done, Some(run)) if run.returns.len() == expected_len => {}
                (TaskState::AgentFailed { .. }, Some(run))
                    if run.returns.len() == expected_len.max(1) => {}
                (TaskState::Done | TaskState::AgentFailed { .. }, Some(run)) => {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        format!(
                            "checkpoint task {index} has run length {}, expected {}",
                            run.returns.len(),
                            expected_len
                        ),
                    ));
                }
                (TaskState::Done | TaskState::AgentFailed { .. }, None) => {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        format!("checkpoint task {index} is scorable but carries no run"),
                    ));
                }
                (TaskState::RuntimeFailed { attempts, .. }, None) => {
                    if *attempts == 0 {
                        return Err(std::io::Error::new(
                            std::io::ErrorKind::InvalidData,
                            format!("checkpoint task {index} records zero runtime attempts"),
                        ));
                    }
                }
                (TaskState::RuntimeFailed { .. }, Some(_)) => {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        format!("checkpoint task {index} is a runtime failure but carries a run"),
                    ));
                }
                (TaskState::Pending | TaskState::Claimed { .. }, _) => {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        format!("checkpoint task {index} is not terminal"),
                    ));
                }
            }
        }
        Ok(())
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

    // Single-worker driver: claim the next pending task, run it under the retry
    // taxonomy, record the outcome, and persist before moving on.
    while let Some((w, seed)) = cp.claim_next(0, 0) {
        let (outcome, _) = run_with_retries(max_retries, || attempt(w, seed));
        match outcome {
            RunOutcome::Completed(run) => cp.complete(w, seed, run),
            RunOutcome::Exhausted { last, attempts } => cp.fail_runtime(w, seed, last, attempts),
            RunOutcome::AgentFault(kind) => {
                let expected_len = windows
                    .get(w)
                    .map(|window| window.end.saturating_sub(window.start))
                    .unwrap_or(0);
                cp.fail_agent(w, seed, kind, failing_sentinel_run(expected_len))
            }
        }
        cp.save(path)?;
    }

    cp.validate_terminal(windows)?;
    Ok(cp.assemble())
}

/// Strict resumable sweep used by the CLI. Unlike the compatibility function,
/// an existing malformed, legacy, or differently-bound checkpoint is an error.
/// It is never overwritten and never mixed into the current experiment.
pub fn run_resumable_sweep_bound<F>(
    path: &Path,
    agent_id: &str,
    contract: &SweepContract,
    windows: &[Window],
    seeds: &[u64],
    max_retries: u32,
    mut attempt: F,
) -> std::io::Result<ResilientSubmission>
where
    F: FnMut(usize, u64) -> Result<Run, FailureKind>,
{
    if !contract.matches_execution(windows, seeds, max_retries) {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "sweep contract does not describe the supplied windows, seeds, and retry policy",
        ));
    }

    let mut cp =
        match SweepCheckpoint::load(path) {
            Ok(existing) if existing.matches_bound(agent_id, contract) => {
                let mut existing = existing;
                existing.requeue_claimed();
                existing
            }
            Ok(_) => return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "checkpoint contract differs from this experiment; choose a new checkpoint path",
            )),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                SweepCheckpoint::new_bound(agent_id, contract.clone())
            }
            Err(error) => return Err(error),
        };

    while let Some((w, seed)) = cp.claim_next(0, 0) {
        let (outcome, _) = run_with_retries(max_retries, || attempt(w, seed));
        match outcome {
            RunOutcome::Completed(run) => cp.complete(w, seed, run),
            RunOutcome::Exhausted { last, attempts } => cp.fail_runtime(w, seed, last, attempts),
            RunOutcome::AgentFault(kind) => {
                let expected_len = windows
                    .get(w)
                    .map(|window| window.end.saturating_sub(window.start))
                    .unwrap_or(0);
                cp.fail_agent(w, seed, kind, failing_sentinel_run(expected_len))
            }
        }
        cp.save(path)?;
    }
    cp.validate_terminal(windows)?;
    Ok(cp.assemble())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn contract(windows: &[Window], seeds: &[u64], max_retries: u32) -> SweepContract {
        SweepContract::new(
            SweepIdentity {
                dataset_sha256: "11".repeat(32),
                cost_model_sha256: "22".repeat(32),
                score_config_sha256: "33".repeat(32),
                runner_artifact_sha256: "44".repeat(32),
                entrant_sha256: "55".repeat(32),
            },
            windows,
            seeds,
            max_retries,
        )
    }

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

    #[test]
    fn agent_fault_sentinels_follow_each_windows_length() {
        let path = tmp_path("unequal-window-sentinels");
        let windows = [
            Window { start: 20, end: 60 },
            Window {
                start: 60,
                end: 120,
            },
        ];
        let seeds = [0u64];
        let pool = run_resumable_sweep(&path, "ext", &windows, &seeds, 0, |_w, _seed| {
            Err(FailureKind::AgentProtocolViolation)
        })
        .unwrap();
        let lengths: Vec<usize> = pool
            .submission
            .runs
            .iter()
            .map(|run| run.returns.len())
            .collect();
        assert_eq!(lengths, vec![40, 60]);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn bound_resume_refuses_a_same_shape_different_experiment() {
        let path = tmp_path("bound-mismatch");
        let windows = [Window { start: 20, end: 60 }];
        let seeds = [0u64, 1];
        let first = contract(&windows, &seeds, 1);
        let attempt = |_w: usize, seed: u64| Ok(skilled_run(seed));
        run_resumable_sweep_bound(&path, "ext", &first, &windows, &seeds, 1, attempt)
            .expect("first experiment writes its checkpoint");

        let mut changed_dataset = first.clone();
        changed_dataset.dataset_sha256 = "aa".repeat(32);
        let error = match run_resumable_sweep_bound(
            &path,
            "ext",
            &changed_dataset,
            &windows,
            &seeds,
            1,
            attempt,
        ) {
            Ok(_) => panic!("same matrix over different data must not resume"),
            Err(error) => error,
        };
        assert_eq!(error.kind(), std::io::ErrorKind::InvalidData);
        assert!(error.to_string().contains("contract differs"));
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn bound_resume_refuses_a_legacy_unbound_checkpoint() {
        let path = tmp_path("bound-legacy");
        let windows = [Window { start: 20, end: 60 }];
        let seeds = [0u64];
        SweepCheckpoint::new("ext", windows.len(), &seeds)
            .save(&path)
            .unwrap();
        let error = match run_resumable_sweep_bound(
            &path,
            "ext",
            &contract(&windows, &seeds, 1),
            &windows,
            &seeds,
            1,
            |_w, seed| Ok(skilled_run(seed)),
        ) {
            Ok(_) => panic!("an unbound checkpoint cannot prove experiment identity"),
            Err(error) => error,
        };
        assert_eq!(error.kind(), std::io::ErrorKind::InvalidData);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn bound_resume_refuses_a_terminal_task_without_its_required_run() {
        let path = tmp_path("bound-terminal-shape");
        let windows = [Window { start: 20, end: 60 }];
        let seeds = [0u64];
        let contract = contract(&windows, &seeds, 1);
        let mut checkpoint = SweepCheckpoint::new_bound("ext", contract.clone());
        checkpoint.tasks[0].state = TaskState::Done;
        checkpoint.tasks[0].run = None;
        checkpoint.save(&path).unwrap();

        let error = match run_resumable_sweep_bound(
            &path,
            "ext",
            &contract,
            &windows,
            &seeds,
            1,
            |_window, _seed| Ok(skilled_run(0)),
        ) {
            Ok(_) => panic!("a done label without evidence must not disappear at assembly"),
            Err(error) => error,
        };
        assert_eq!(error.kind(), std::io::ErrorKind::InvalidData);
        assert!(error.to_string().contains("scorable but carries no run"));
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn bound_resume_validates_the_contract_against_call_arguments() {
        let path = tmp_path("bound-arguments");
        let windows = [Window { start: 20, end: 60 }];
        let different = [Window { start: 21, end: 61 }];
        let seeds = [0u64];
        let error = match run_resumable_sweep_bound(
            &path,
            "ext",
            &contract(&windows, &seeds, 1),
            &different,
            &seeds,
            1,
            |_w, seed| Ok(skilled_run(seed)),
        ) {
            Ok(_) => panic!("a caller cannot lie about the contract it supplies"),
            Err(error) => error,
        };
        assert_eq!(error.kind(), std::io::ErrorKind::InvalidInput);
        assert!(!path.exists(), "an invalid contract must write nothing");

        let mut malformed = contract(&windows, &seeds, 1);
        malformed.entrant_sha256 = "endpoint-label".to_string();
        let error = match run_resumable_sweep_bound(
            &path,
            "ext",
            &malformed,
            &windows,
            &seeds,
            1,
            |_w, seed| Ok(skilled_run(seed)),
        ) {
            Ok(_) => panic!("a label is not an entrant artifact identity"),
            Err(error) => error,
        };
        assert_eq!(error.kind(), std::io::ErrorKind::InvalidInput);
        assert!(!path.exists());
    }
}
