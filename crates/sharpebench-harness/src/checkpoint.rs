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
    failing_sentinel_run, run_with_retries, AttemptLedger, Backoff, BackoffSchedule, FailureKind,
    FailureLog, FailureRecord, RunOutcome, Sleeper, ThreadSleeper,
};
use crate::ResilientSubmission;

/// Versioned identity of the conditions a resumable sweep binds: dataset, cost
/// model, score configuration, runner artifact, entrant artifact and
/// invocation. A checkpoint is reusable only when this record matches exactly.
///
/// The digests bind semantic inputs without copying a dataset, secrets, or a
/// binary into the checkpoint. Exact windows and seeds stay visible because
/// they are useful diagnostics rather than opaque implementation details.
///
/// What it deliberately does not bind: credential values. The CLI folds the
/// effective *non-secret* environment handed to a `--cmd` entrant into
/// `invocation_sha256` (see `sharpebench_sim::agent_env_identity`), so changing
/// a policy variable invalidates the checkpoint, while rotating a token does
/// not and never reaches the checkpoint file. Anything the harness cannot
/// observe, such as state inside a remote endpoint behind `--http`, is outside
/// this record too.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SweepIdentity {
    pub dataset_sha256: String,
    pub cost_model_sha256: String,
    pub score_config_sha256: String,
    pub runner_artifact_sha256: String,
    pub entrant_sha256: String,
    /// SHA-256 of the exact launch descriptor: transport, endpoint or command,
    /// arguments, and the names of explicitly passed environment variables.
    /// This is separate from `entrant_sha256`, which identifies the immutable
    /// code or model artifact rather than how the harness invokes it.
    pub invocation_sha256: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SweepContract {
    pub schema_version: u32,
    pub dataset_sha256: String,
    pub cost_model_sha256: String,
    pub score_config_sha256: String,
    pub runner_artifact_sha256: String,
    pub entrant_sha256: String,
    pub invocation_sha256: String,
    pub windows: Vec<(usize, usize)>,
    pub seeds: Vec<u64>,
    pub max_retries: u32,
}

impl SweepContract {
    /// Bumped to 3 when the per-task attempt ledger landed: a version-2
    /// checkpoint carries no failed-attempt evidence, and resuming into it would
    /// report everything its attempts already spent as zero. It is refused
    /// rather than read that way.
    ///
    /// Bumped to 4 for `TaskRecord::attempts_in_round` for exactly the same
    /// reason one level down. A version-3 checkpoint has no per-round spend, so
    /// its unfinished cells deserialize at the `#[serde(default)]` zero and are
    /// granted a fresh `max_retries + 1` attempts on top of whatever the writing
    /// binary already spent in that round. The load-time budget pre-check reads
    /// the same zero and cannot see the discontinuity, so the version is what
    /// refuses it.
    pub const SCHEMA_VERSION: u32 = 4;

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
            invocation_sha256: identity.invocation_sha256,
            windows: windows.iter().map(|w| (w.start, w.end)).collect(),
            seeds: seeds.to_vec(),
            max_retries,
        }
    }

    /// Everything checkable without caller-supplied execution parameters: the
    /// schema this binary understands, well-formed identity digests, and the
    /// window matrix. Callers that take the seeds and the retry budget from the
    /// contract itself have nothing further to compare against.
    fn matches_windows(&self, windows: &[Window]) -> bool {
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
                &self.invocation_sha256,
            ]
            .into_iter()
            .all(|digest| valid_digest(digest))
            && self.windows == windows.iter().map(|w| (w.start, w.end)).collect::<Vec<_>>()
    }

    /// The full check, for callers that supply seeds and a retry budget of their
    /// own: those two legs only mean something against values the contract did
    /// not provide.
    fn matches_execution(&self, windows: &[Window], seeds: &[u64], max_retries: u32) -> bool {
        self.matches_windows(windows) && self.seeds == seeds && self.max_retries == max_retries
    }
}

/// The one bound identity a comparison's arms are allowed to differ on.
///
/// [`SweepContract`] binds six identities, but binding alone does not say which
/// of them the experiment is varying on purpose. Two arms that differ in the
/// entrant are a model comparison; two that differ in the dataset are two
/// different experiments wearing one table. The axis is declared, so a reader
/// checks comparability instead of assuming it.
///
/// Dataset, cost model, runner artifact and the execution matrix (windows,
/// seeds, retry budget) are deliberately not declarable: an arm that moves one
/// of them is measuring something else.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TreatmentAxis {
    /// A different entrant artifact: another model, or another build of one.
    Entrant,
    /// The same entrant launched differently: transport, endpoint or command,
    /// arguments, and the non-secret environment policy folded into
    /// `invocation_sha256`. Rotating a credential is not a launch difference and
    /// never reaches that digest.
    Invocation,
    /// The same recorded runs read by a different scorer.
    ScoreConfig,
}

impl TreatmentAxis {
    /// The contract field this axis permits to differ between arms.
    pub fn field(self) -> &'static str {
        match self {
            Self::Entrant => "entrant_sha256",
            Self::Invocation => "invocation_sha256",
            Self::ScoreConfig => "score_config_sha256",
        }
    }
}

/// Why two arms cannot be compared, naming the field that decided it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ComparabilityRefusal {
    /// A bound identity the declared axis does not cover differs.
    OffAxisIdentity {
        field: &'static str,
        baseline: String,
        treatment: String,
    },
    /// The arms did not execute the same matrix: windows, seeds, the
    /// failed-attempt budget, or the checkpoint schema those are recorded under.
    ExecutionMatrix { field: &'static str },
    /// An arm carries no contract, so it binds nothing to compare against.
    UnboundArm { agent_id: String },
}

impl std::fmt::Display for ComparabilityRefusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::OffAxisIdentity {
                field,
                baseline,
                treatment,
            } => write!(
                f,
                "`{field}` differs off the declared treatment axis: baseline {baseline}, treatment {treatment}"
            ),
            Self::ExecutionMatrix { field } => {
                write!(f, "the arms did not execute the same `{field}`")
            }
            Self::UnboundArm { agent_id } => {
                write!(f, "arm `{agent_id}` carries no sweep contract")
            }
        }
    }
}

impl std::error::Error for ComparabilityRefusal {}

/// A declaration that two sweep arms are comparable, and on which axis.
///
/// The receipt states the treatment axis, the digests the arms carry on it, and
/// every identity checked equal to get there. A reader who disagrees with the
/// declared axis can see exactly what was held fixed rather than trusting that
/// anything was.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ComparisonReceipt {
    pub schema_version: u32,
    pub axis: TreatmentAxis,
    pub baseline_agent_id: String,
    pub treatment_agent_id: String,
    /// The contract field the axis permits to differ.
    pub axis_field: String,
    pub baseline_axis_sha256: String,
    pub treatment_axis_sha256: String,
    /// The identities checked equal across both arms, in the order checked.
    pub held_fixed: Vec<String>,
}

impl ComparisonReceipt {
    pub const SCHEMA_VERSION: u32 = 1;

    /// Declare a comparison between two checkpointed arms, or refuse it naming
    /// the field that made them incomparable.
    ///
    /// The axis field is allowed to differ and is recorded either way; every
    /// other bound identity must match exactly, and so must the execution
    /// matrix. What the contract deliberately does not bind stays unbound here
    /// too: a rotated credential leaves `invocation_sha256` untouched, so it
    /// neither breaks a comparison nor a resume.
    pub fn declare(
        axis: TreatmentAxis,
        baseline: &SweepCheckpoint,
        treatment: &SweepCheckpoint,
    ) -> Result<Self, ComparabilityRefusal> {
        let base = baseline
            .contract
            .as_ref()
            .ok_or_else(|| ComparabilityRefusal::UnboundArm {
                agent_id: baseline.agent_id.clone(),
            })?;
        let treat =
            treatment
                .contract
                .as_ref()
                .ok_or_else(|| ComparabilityRefusal::UnboundArm {
                    agent_id: treatment.agent_id.clone(),
                })?;

        let identities: [(&'static str, &String, &String); 6] = [
            (
                "dataset_sha256",
                &base.dataset_sha256,
                &treat.dataset_sha256,
            ),
            (
                "cost_model_sha256",
                &base.cost_model_sha256,
                &treat.cost_model_sha256,
            ),
            (
                "score_config_sha256",
                &base.score_config_sha256,
                &treat.score_config_sha256,
            ),
            (
                "runner_artifact_sha256",
                &base.runner_artifact_sha256,
                &treat.runner_artifact_sha256,
            ),
            (
                "entrant_sha256",
                &base.entrant_sha256,
                &treat.entrant_sha256,
            ),
            (
                "invocation_sha256",
                &base.invocation_sha256,
                &treat.invocation_sha256,
            ),
        ];

        let mut held_fixed = Vec::with_capacity(identities.len() - 1);
        let mut on_axis = None;
        for (field, baseline_digest, treatment_digest) in identities {
            if field == axis.field() {
                on_axis = Some((baseline_digest.clone(), treatment_digest.clone()));
                continue;
            }
            if baseline_digest != treatment_digest {
                return Err(ComparabilityRefusal::OffAxisIdentity {
                    field,
                    baseline: baseline_digest.clone(),
                    treatment: treatment_digest.clone(),
                });
            }
            held_fixed.push(field.to_string());
        }

        if base.schema_version != treat.schema_version {
            return Err(ComparabilityRefusal::ExecutionMatrix {
                field: "schema_version",
            });
        }
        if base.windows != treat.windows {
            return Err(ComparabilityRefusal::ExecutionMatrix { field: "windows" });
        }
        if base.seeds != treat.seeds {
            return Err(ComparabilityRefusal::ExecutionMatrix { field: "seeds" });
        }
        if base.max_retries != treat.max_retries {
            return Err(ComparabilityRefusal::ExecutionMatrix {
                field: "max_retries",
            });
        }

        // Unreachable by construction: every axis names one of the six.
        let (baseline_axis_sha256, treatment_axis_sha256) = on_axis.expect("axis names a field");
        Ok(Self {
            schema_version: Self::SCHEMA_VERSION,
            axis,
            baseline_agent_id: baseline.agent_id.clone(),
            treatment_agent_id: treatment.agent_id.clone(),
            axis_field: axis.field().to_string(),
            baseline_axis_sha256,
            treatment_axis_sha256,
            held_fixed,
        })
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

/// Explicit recovery of infrastructure failures, never a completed or agent-fault
/// result. Three additional rounds per cell is a lifetime checkpoint ceiling,
/// not a budget that resets each time the process starts.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ResumePolicy {
    #[default]
    UnfinishedOnly,
    RetryRuntimeFailures,
}

pub const MAX_RUNTIME_RECOVERY_ROUNDS: u32 = 3;

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
    /// Every attempt this cell has cost, in order, across resumes. Append-only:
    /// a completion that resumes a failed attempt is added after it, never over
    /// it. Rank-neutral, and never part of the assembled run pool.
    #[serde(default)]
    pub attempts: AttemptLedger,
    /// Recovery rounds already authorized for this cell, including a round
    /// interrupted after its claim was saved. Missing in older v3 checkpoints.
    #[serde(default)]
    pub runtime_recovery_rounds: u32,
    /// Completed attempt observations in the current round. Persisting this
    /// prevents an interrupted claim from resetting its per-round retry budget.
    #[serde(default)]
    pub attempts_in_round: u32,
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
                    attempts: AttemptLedger::default(),
                    runtime_recovery_rounds: 0,
                    attempts_in_round: 0,
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
    /// sync the containing directory on Unix. Durability assumes the filesystem
    /// honors those sync/rename operations; parent-directory sync is Unix-only.
    /// Concurrent saves own distinct temporary files (the final rename is last-writer-wins).
    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        let payload = serde_json::to_string_pretty(self).map_err(std::io::Error::other)?;
        use std::sync::atomic::{AtomicU64, Ordering};
        static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);
        let parent = path
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        let name = path.file_name().ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "checkpoint path needs a filename",
            )
        })?;
        let mut collisions = 0;
        let (tmp, mut file) = loop {
            let mut temp_name = name.to_os_string();
            temp_name.push(format!(
                ".{}.{}.tmp",
                std::process::id(),
                NEXT_TEMP.fetch_add(1, Ordering::Relaxed)
            ));
            let tmp = parent.join(temp_name);
            match std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&tmp)
            {
                Ok(file) => break (tmp, file),
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                    collisions += 1;
                    if collisions == 64 {
                        return Err(error);
                    }
                }
                Err(error) => return Err(error),
            }
        };
        let result = (|| {
            file.write_all(payload.as_bytes())?;
            file.sync_all()?;
            drop(file);
            std::fs::rename(&tmp, path)?;
            #[cfg(unix)]
            std::fs::File::open(parent)?.sync_all()?;
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

    fn append_executed_attempts(
        &mut self,
        window: usize,
        seed: u64,
        ledger: &AttemptLedger,
    ) -> u32 {
        let cell = self
            .task_mut(window, seed)
            .expect("the claimed task exists");
        // Distinct executions are not replayed ledger batches. Identical records
        // still count separately, unlike the legacy record_attempts merge API.
        cell.attempts.extend(ledger);
        u32::try_from(cell.attempts.len()).unwrap_or(u32::MAX)
    }

    /// Record what a cell has spent so far without settling its state. A worker
    /// that persists an attempt before retrying keeps that attempt's cost even
    /// if it dies before the cell reaches a terminal state.
    pub fn record_attempts(&mut self, window: usize, seed: u64, ledger: &AttemptLedger) {
        if let Some(t) = self.task_mut(window, seed) {
            t.attempts.append(ledger);
        }
    }

    /// Mark a task done with its scorable run, appending what the completion and
    /// the attempts it supersedes cost.
    pub fn complete(&mut self, window: usize, seed: u64, run: Run, ledger: &AttemptLedger) {
        if let Some(t) = self.task_mut(window, seed) {
            t.state = TaskState::Done;
            t.run = Some(run);
            t.attempts.append(ledger);
        }
    }

    /// Mark a task as an exhausted runtime failure (excluded from the score).
    pub fn fail_runtime(
        &mut self,
        window: usize,
        seed: u64,
        kind: FailureKind,
        attempts: u32,
        ledger: &AttemptLedger,
    ) {
        if let Some(t) = self.task_mut(window, seed) {
            t.state = TaskState::RuntimeFailed { kind, attempts };
            t.run = None;
            t.attempts.append(ledger);
        }
    }

    /// Mark a task as an agent fault, storing the failing sentinel run that counts
    /// against pass^k.
    pub fn fail_agent(
        &mut self,
        window: usize,
        seed: u64,
        kind: FailureKind,
        sentinel: Run,
        ledger: &AttemptLedger,
    ) {
        if let Some(t) = self.task_mut(window, seed) {
            t.state = TaskState::AgentFailed { kind };
            t.run = Some(sentinel);
            t.attempts.append(ledger);
        }
    }

    /// The whole sweep's attempts, cell by cell in matrix order. Rank-neutral.
    pub fn attempt_ledger(&self) -> AttemptLedger {
        let mut ledger = AttemptLedger::default();
        for t in &self.tasks {
            // Concatenation, never the replay merge: two cells with identical
            // records are two attempts.
            ledger.extend(&t.attempts);
        }
        ledger
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
                        // Every attempt this cell cost, not only the one that
                        // ended it: two transport failures then an agent fault
                        // is three attempts, not one.
                        attempts: u32::try_from(t.attempts.len()).unwrap_or(u32::MAX).max(1),
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
            attempts: self.attempt_ledger().summary(),
            monetary_cost: self.attempt_ledger().monetary_summary(),
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
            // A terminal cell cost at least the attempt that ended it. An empty
            // ledger is a checkpoint written before attempts were recorded, and
            // reading it would report that cost as zero.
            if task.attempts.is_empty() {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    format!("checkpoint task {index} is terminal but records no attempt"),
                ));
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
        let driven = run_with_retries(max_retries, || attempt(w, seed));
        let ledger = driven.ledger;
        match driven.outcome {
            RunOutcome::Completed(run) => cp.complete(w, seed, run, &ledger),
            RunOutcome::Exhausted { last, attempts } => {
                cp.fail_runtime(w, seed, last, attempts, &ledger)
            }
            RunOutcome::AgentFault(kind) => {
                let expected_len = windows
                    .get(w)
                    .map(|window| window.end.saturating_sub(window.start))
                    .unwrap_or(0);
                cp.fail_agent(w, seed, kind, failing_sentinel_run(expected_len), &ledger)
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
    attempt: F,
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

    run_resumable_sweep_bound_with_policy(
        path,
        agent_id,
        contract,
        windows,
        ResumePolicy::UnfinishedOnly,
        attempt,
    )
}

/// Bound sweep with opt-in recovery of exhausted runtime cells. Execution seeds
/// and per-round retries come from the unchanged contract. Every newly observed
/// attempt is appended and saved before another is made; completed and agent-
/// fault outcomes are saved with that attempt, not in a later checkpoint write.
/// A crash during an attempt can still leave its duration/outcome unobserved.
pub fn run_resumable_sweep_bound_with_policy<F>(
    path: &Path,
    agent_id: &str,
    contract: &SweepContract,
    windows: &[Window],
    policy: ResumePolicy,
    mut attempt: F,
) -> std::io::Result<ResilientSubmission>
where
    F: FnMut(usize, u64) -> Result<Run, FailureKind>,
{
    run_resumable_sweep_observed(path, agent_id, contract, windows, policy, |window, seed| {
        attempt(window, seed).into()
    })
}

/// Bound recovery with per-attempt usage persisted in the same atomic checkpoint
/// write as the outcome. Include the frozen rate-card identity in the invocation
/// digest; otherwise a caller could resume the same sweep under different rates.
pub fn run_resumable_sweep_observed<F>(
    path: &Path,
    agent_id: &str,
    contract: &SweepContract,
    windows: &[Window],
    policy: ResumePolicy,
    mut attempt: F,
) -> std::io::Result<ResilientSubmission>
where
    F: FnMut(usize, u64) -> crate::AttemptObservation,
{
    run_resumable_sweep_faulted(path, agent_id, contract, windows, policy, |window, seed| {
        attempt(window, seed).into()
    })
}

/// [`run_resumable_sweep_observed`] persisting each attempt's injected-fault
/// evidence on its ledger record. The plan must already be folded into
/// `contract.invocation_sha256` with [`crate::fault_plan::bind_invocation`], so
/// resuming under a different plan is refused as a different contract.
pub fn run_resumable_sweep_faulted<F>(
    path: &Path,
    agent_id: &str,
    contract: &SweepContract,
    windows: &[Window],
    policy: ResumePolicy,
    attempt: F,
) -> std::io::Result<ResilientSubmission>
where
    F: FnMut(usize, u64) -> crate::fault_plan::FaultedObservation,
{
    run_resumable_sweep_with_backoff(
        path,
        agent_id,
        contract,
        windows,
        policy,
        &BackoffSchedule::immediate(),
        &mut ThreadSleeper,
        attempt,
    )
}

/// [`run_resumable_sweep_faulted`] under an explicit backoff schedule between
/// runtime retries. This driver owns its retry loop, so that each observation
/// is durable before the next attempt starts, and applies the schedule itself
/// with the record [`crate::run_with_backoff`] makes: the scheduled wait is
/// written on the failed attempt's `backoff_after` and saved before the driver
/// sleeps, and it is never inside an attempt's duration. Retries are numbered
/// within the cell's current round, so a resumed round continues the schedule
/// where the interrupted process left it and a recovery round starts it again.
/// The schedule must already be folded into `contract.invocation_sha256` with
/// [`BackoffSchedule::bind_invocation`], so resuming under a different
/// schedule is refused as a different contract.
#[allow(clippy::too_many_arguments)]
pub fn run_resumable_sweep_with_backoff<F>(
    path: &Path,
    agent_id: &str,
    contract: &SweepContract,
    windows: &[Window],
    policy: ResumePolicy,
    schedule: &BackoffSchedule,
    sleeper: &mut dyn Sleeper,
    mut attempt: F,
) -> std::io::Result<ResilientSubmission>
where
    F: FnMut(usize, u64) -> crate::fault_plan::FaultedObservation,
{
    // Seeds and the retry budget come from the contract itself here, so
    // comparing them against the contract would compare them against
    // themselves. `run_resumable_sweep_bound` still makes the real comparison
    // against the values its caller supplied.
    if !contract.matches_windows(windows) {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "sweep contract does not describe the supplied windows",
        ));
    }

    let mut cp =
        match SweepCheckpoint::load(path) {
            Ok(existing)
                if existing.contract.as_ref().is_some_and(|written| {
                    written.schema_version != SweepContract::SCHEMA_VERSION
                }) =>
            {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "checkpoint was written under a different contract schema; its per-task attempt accounting cannot be read as this one",
                ))
            }
            Ok(existing) if existing.matches_bound(agent_id, contract) => {
                let mut existing = existing;
                // Validate every proposed recovery before modifying any cell.
                for task in &existing.tasks {
                    if matches!(task.state, TaskState::Pending | TaskState::Claimed { .. })
                        && task.attempts_in_round > contract.max_retries
                    {
                        return Err(std::io::Error::new(
                            std::io::ErrorKind::InvalidData,
                            "unfinished checkpoint cell has exhausted its per-round budget",
                        ));
                    }
                    if task.runtime_recovery_rounds > MAX_RUNTIME_RECOVERY_ROUNDS {
                        return Err(std::io::Error::new(
                            std::io::ErrorKind::InvalidData,
                            "checkpoint exceeds the runtime recovery ceiling",
                        ));
                    }
                    if let TaskState::RuntimeFailed { kind, attempts } = &task.state {
                        if !kind.is_runtime()
                            || *attempts == 0
                            || task.run.is_some()
                            || task.attempts.is_empty()
                        {
                            return Err(std::io::Error::new(
                                std::io::ErrorKind::InvalidData,
                                "checkpoint contains an invalid runtime failure",
                            ));
                        }
                        if policy == ResumePolicy::RetryRuntimeFailures
                            && task.runtime_recovery_rounds == MAX_RUNTIME_RECOVERY_ROUNDS
                        {
                            return Err(std::io::Error::new(
                                std::io::ErrorKind::InvalidInput,
                                "checkpoint runtime recovery budget is exhausted",
                            ));
                        }
                    }
                }
                if policy == ResumePolicy::RetryRuntimeFailures {
                    for task in &mut existing.tasks {
                        if matches!(task.state, TaskState::RuntimeFailed { .. }) {
                            task.runtime_recovery_rounds += 1;
                            task.attempts_in_round = 0;
                            task.state = TaskState::Pending;
                        }
                    }
                }
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
        cp.save(path)?;
        let mut tries = cp
            .task_mut(w, seed)
            .expect("the claimed task exists")
            .attempts_in_round;
        loop {
            // The checkpoint driver owns the retry loop so that each observation
            // is durable before a later attempt can start.
            let mut driven = crate::failure::run_with_faulted_retries(0, || attempt(w, seed));
            tries += 1;
            cp.task_mut(w, seed)
                .expect("the claimed task exists")
                .attempts_in_round = tries;
            let budget_spent = tries > contract.max_retries || tries == u32::MAX;
            let wait = match driven.outcome {
                RunOutcome::Exhausted { .. } if !budget_spent => schedule.delay_before(tries),
                _ => None,
            };
            if let (Some(delay), Some(record)) = (wait, driven.ledger.attempts.last_mut()) {
                record.backoff_after = Some(Backoff {
                    retry: tries,
                    delay_ns: u64::try_from(delay.as_nanos()).unwrap_or(u64::MAX),
                });
            }
            let total = cp.append_executed_attempts(w, seed, &driven.ledger);
            let recorded = AttemptLedger::default();
            let terminal = match driven.outcome {
                RunOutcome::Completed(run) => {
                    cp.complete(w, seed, run, &recorded);
                    true
                }
                RunOutcome::Exhausted { last, .. } => {
                    if budget_spent {
                        cp.fail_runtime(w, seed, last, total, &recorded);
                        true
                    } else {
                        false
                    }
                }
                RunOutcome::AgentFault(kind) => {
                    let expected_len = windows[w].end.saturating_sub(windows[w].start);
                    cp.fail_agent(w, seed, kind, failing_sentinel_run(expected_len), &recorded);
                    true
                }
            };
            cp.save(path)?;
            if terminal {
                break;
            }
            if let Some(delay) = wait {
                sleeper.sleep(delay);
            }
        }
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
                invocation_sha256: "66".repeat(32),
            },
            windows,
            seeds,
            max_retries,
        )
    }

    /// Two checkpointed arms of one experiment, identical but for `mutate`.
    fn two_arms(mutate: impl FnOnce(&mut SweepContract)) -> (SweepCheckpoint, SweepCheckpoint) {
        let windows = [Window { start: 20, end: 60 }];
        let seeds = [0u64, 1];
        let base = contract(&windows, &seeds, 2);
        let mut treatment = base.clone();
        mutate(&mut treatment);
        (
            SweepCheckpoint::new_bound("baseline", base),
            SweepCheckpoint::new_bound("treatment", treatment),
        )
    }

    #[test]
    fn comparison_receipt_declares_the_axis_the_arms_differ_on() {
        let (baseline, treatment) = two_arms(|c| c.entrant_sha256 = "77".repeat(32));
        let receipt =
            ComparisonReceipt::declare(TreatmentAxis::Entrant, &baseline, &treatment).unwrap();
        assert_eq!(receipt.axis_field, "entrant_sha256");
        assert_eq!(receipt.baseline_axis_sha256, "55".repeat(32));
        assert_eq!(receipt.treatment_axis_sha256, "77".repeat(32));
        assert_eq!(
            receipt.held_fixed,
            vec![
                "dataset_sha256",
                "cost_model_sha256",
                "score_config_sha256",
                "runner_artifact_sha256",
                "invocation_sha256",
            ],
            "every identity the axis does not cover is named as held fixed"
        );
    }

    #[test]
    fn treatment_content_off_the_declared_axis_refuses_the_comparison() {
        // The identical pair of arms: comparable on the entrant axis, refused
        // the moment the declaration says the treatment was the invocation.
        let (baseline, treatment) = two_arms(|c| c.entrant_sha256 = "77".repeat(32));
        assert!(ComparisonReceipt::declare(TreatmentAxis::Entrant, &baseline, &treatment).is_ok());
        assert_eq!(
            ComparisonReceipt::declare(TreatmentAxis::Invocation, &baseline, &treatment),
            Err(ComparabilityRefusal::OffAxisIdentity {
                field: "entrant_sha256",
                baseline: "55".repeat(32),
                treatment: "77".repeat(32),
            })
        );
    }

    #[test]
    fn changed_data_cost_or_runner_identity_refuses_the_comparison() {
        type ContractEdit = Box<dyn FnOnce(&mut SweepContract)>;
        let changed = "77".repeat(32);
        let cases: [(&str, &str, ContractEdit); 3] = [
            (
                "dataset_sha256",
                "11",
                Box::new(|c: &mut SweepContract| c.dataset_sha256 = "77".repeat(32)),
            ),
            (
                "cost_model_sha256",
                "22",
                Box::new(|c: &mut SweepContract| c.cost_model_sha256 = "77".repeat(32)),
            ),
            (
                "runner_artifact_sha256",
                "44",
                Box::new(|c: &mut SweepContract| c.runner_artifact_sha256 = "77".repeat(32)),
            ),
        ];
        for (field, original, mutate) in cases {
            let (baseline, treatment) = two_arms(mutate);
            // Declared on the widest axis there is: still refused, because none
            // of these three is ever declarable.
            let refusal = ComparisonReceipt::declare(TreatmentAxis::Entrant, &baseline, &treatment)
                .unwrap_err();
            assert_eq!(
                refusal,
                ComparabilityRefusal::OffAxisIdentity {
                    field,
                    baseline: original.repeat(32),
                    treatment: changed.clone(),
                }
            );
            assert!(refusal.to_string().contains(field), "the reason names it");
        }
    }

    #[test]
    fn a_different_failed_attempt_budget_refuses_the_comparison() {
        let (baseline, treatment) = two_arms(|c| c.max_retries += 1);
        assert_eq!(
            ComparisonReceipt::declare(TreatmentAxis::Entrant, &baseline, &treatment),
            Err(ComparabilityRefusal::ExecutionMatrix {
                field: "max_retries"
            })
        );
    }

    #[test]
    fn credential_rotation_does_not_invalidate_a_resume() {
        use sha2::{Digest, Sha256};

        // The same launch policy, before and after the token behind it rotates.
        let invocation = |token: &str| {
            let identity = sharpebench_sim::agent_env_identity(
                "OPENAI_API_KEY,SHARPEBENCH_AGENT_POLICY",
                "",
                |name| match name {
                    "OPENAI_API_KEY" => Some(token.to_string()),
                    "SHARPEBENCH_AGENT_POLICY" => Some("strict".to_string()),
                    _ => None,
                },
            );
            format!("{:x}", Sha256::digest(identity.as_bytes()))
        };

        let windows = [Window { start: 20, end: 60 }];
        let seeds = [0u64, 1];
        let bind = |token: &str| {
            let mut bound = contract(&windows, &seeds, 2);
            bound.invocation_sha256 = invocation(token);
            bound
        };

        let before = bind("sk-original");
        let path = tmp_path("credential-rotation");
        let mut cp = SweepCheckpoint::new_bound("ext", before.clone());
        let (w, seed) = cp.claim_next(0, 0).unwrap();
        cp.complete(w, seed, skilled_run(seed), &untimed_completion());
        cp.save(&path).unwrap();

        let after = bind("sk-rotated");
        let reloaded = SweepCheckpoint::load(&path).unwrap();
        assert!(
            reloaded.matches_bound("ext", &after),
            "rotating a credential must leave the bound invocation identity intact"
        );

        let mut ran = 0u32;
        let pool = run_resumable_sweep_bound(&path, "ext", &after, &windows, &seeds, 2, |_w, s| {
            ran += 1;
            Ok(skilled_run(s))
        })
        .unwrap();
        assert_eq!(ran, 1, "the finished task is not paid for a second time");
        assert_eq!(pool.submission.runs.len(), 2);

        let _ = std::fs::remove_file(&path);
    }

    /// One completed attempt that no clock observed: what a caller recording an
    /// outcome from outside the retry driver can honestly say.
    fn untimed_completion() -> AttemptLedger {
        let mut ledger = AttemptLedger::default();
        ledger.push(crate::failure::AttemptRecord::completed(
            crate::failure::AttemptDuration::Unavailable,
        ));
        ledger
    }

    #[test]
    fn distinct_executions_with_identical_records_are_not_deduplicated() {
        let mut checkpoint = SweepCheckpoint::new("entrant", 1, &[7]);
        let batch = untimed_completion();
        assert_eq!(checkpoint.append_executed_attempts(0, 7, &batch), 1);
        assert_eq!(checkpoint.append_executed_attempts(0, 7, &batch), 2);
        assert_eq!(checkpoint.attempt_ledger().summary().completed, 2);
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
    fn relative_path_save_is_durable() {
        const CHILD: &str = "SHARPEBENCH_TEST_RELATIVE_CHECKPOINT";
        if std::env::var_os(CHILD).is_some() {
            let checkpoint = SweepCheckpoint::new("relative", 1, &[7]);
            checkpoint.save(Path::new("checkpoint.json")).unwrap();
            assert!(SweepCheckpoint::load(Path::new("checkpoint.json"))
                .unwrap()
                .matches("relative", 1, &[7]));
            return;
        }
        // A child process owns its cwd: never race other tests with set_current_dir.
        let dir = tmp_path("relative-save");
        std::fs::create_dir(&dir).unwrap();
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "checkpoint::tests::relative_path_save_is_durable",
                "--nocapture",
            ])
            .env(CHILD, "1")
            .current_dir(&dir)
            .output()
            .unwrap();
        let published = dir.join("checkpoint.json").is_file();
        std::fs::remove_dir_all(&dir).unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(published, "the child must execute the save, not zero tests");
    }

    #[test]
    fn save_never_overwrites_a_preexisting_sibling_temp() {
        let path = tmp_path("owned-temp");
        let stale = path.with_extension("json.tmp");
        std::fs::write(&stale, "not owned by this save").unwrap();
        let checkpoint = SweepCheckpoint::new("owned", 1, &[7]);
        checkpoint.save(&path).unwrap();
        let unchanged = std::fs::read_to_string(&stale).ok();
        let _ = std::fs::remove_file(&stale);
        std::fs::remove_file(path).unwrap();
        assert_eq!(unchanged.as_deref(), Some("not owned by this save"));
    }

    #[test]
    fn checkpoint_roundtrips_and_reports_progress() {
        let seeds = [0u64, 1, 2];
        let mut cp = SweepCheckpoint::new("agent", 2, &seeds); // 6 tasks
        assert_eq!(cp.tasks.len(), 6);
        assert_eq!(cp.remaining(), 6);
        cp.complete(0, 0, skilled_run(0), &untimed_completion());
        cp.complete(0, 1, skilled_run(1), &untimed_completion());
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
            cp.complete(w, seed, skilled_run(seed), &untimed_completion());
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
            cp.complete(w, seed, skilled_run(seed), &untimed_completion());
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

        let mut changed_invocation = first.clone();
        changed_invocation.invocation_sha256 = "bb".repeat(32);
        let error = match run_resumable_sweep_bound(
            &path,
            "ext",
            &changed_invocation,
            &windows,
            &seeds,
            1,
            attempt,
        ) {
            Ok(_) => panic!("the same artifact under a different invocation must not resume"),
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

        let mut malformed = contract(&windows, &seeds, 1);
        malformed.invocation_sha256 = "cmd:agent --flag".to_string();
        let error = match run_resumable_sweep_bound(
            &path,
            "ext",
            &malformed,
            &windows,
            &seeds,
            1,
            |_w, seed| Ok(skilled_run(seed)),
        ) {
            Ok(_) => panic!("a launch label is not a canonical invocation digest"),
            Err(error) => error,
        };
        assert_eq!(error.kind(), std::io::ErrorKind::InvalidInput);
        assert!(!path.exists());
    }

    /// F-B: `attempts_in_round` became a persisted spend invariant while the
    /// contract schema stayed at 3, so a checkpoint written before it existed
    /// deserializes at the serde default. Its claimed cell is requeued with a
    /// per-round spend of zero and granted a fresh `max_retries + 1` attempts on
    /// top of everything the writing binary already spent, which is the exact
    /// "spend read as zero" case the schema-3 bump exists to refuse. The
    /// budget pre-check reads the same zero and cannot see it.
    #[test]
    fn bound_resume_refuses_a_checkpoint_written_before_the_per_round_budget() {
        let path = tmp_path("bound-pre-round-budget");
        let windows = [Window { start: 20, end: 60 }];
        let seeds = [0u64];
        let contract = contract(&windows, &seeds, 1);
        let legacy = format!(
            r#"{{
  "agent_id": "ext",
  "contract": {{
    "schema_version": 3,
    "dataset_sha256": "{d}",
    "cost_model_sha256": "{c}",
    "score_config_sha256": "{sc}",
    "runner_artifact_sha256": "{r}",
    "entrant_sha256": "{e}",
    "invocation_sha256": "{i}",
    "windows": [[20, 60]],
    "seeds": [0],
    "max_retries": 1
  }},
  "tasks": [
    {{
      "window": 0,
      "seed": 0,
      "state": {{ "state": "claimed", "worker": 0, "epoch": 0 }},
      "attempts": {{ "attempts": [] }}
    }}
  ]
}}"#,
            d = contract.dataset_sha256,
            c = contract.cost_model_sha256,
            sc = contract.score_config_sha256,
            r = contract.runner_artifact_sha256,
            e = contract.entrant_sha256,
            i = contract.invocation_sha256,
        );
        std::fs::write(&path, &legacy).unwrap();

        // The missing key really does read as an unspent round, which is why the
        // version and not the budget pre-check has to refuse it.
        let parsed = SweepCheckpoint::load(&path).unwrap();
        assert_eq!(parsed.tasks[0].attempts_in_round, 0);
        assert!(parsed.tasks[0].attempts.is_empty());

        let error = match run_resumable_sweep_bound(
            &path,
            "ext",
            &contract,
            &windows,
            &seeds,
            1,
            |_w, seed| Ok(skilled_run(seed)),
        ) {
            Ok(_) => panic!("a pre-budget checkpoint must not grant a fresh round"),
            Err(error) => error,
        };
        assert_eq!(error.kind(), std::io::ErrorKind::InvalidData);
        assert!(
            error.to_string().contains("different contract schema"),
            "{error}"
        );
        let _ = std::fs::remove_file(&path);
    }

    /// F-E: the seed and retry legs of `matches_execution` are real only against
    /// values the contract did not supply. `run_resumable_sweep_bound` is the
    /// caller that supplies them, so that is where the comparison has to bite.
    #[test]
    fn bound_resume_rejects_seeds_and_retries_the_contract_did_not_declare() {
        let path = tmp_path("bound-execution-legs");
        let windows = [Window { start: 20, end: 60 }];
        let seeds = [0u64];
        let contract = contract(&windows, &seeds, 1);

        for (other_seeds, retries) in [(vec![1u64], 1u32), (vec![0u64], 2u32)] {
            let error = match run_resumable_sweep_bound(
                &path,
                "ext",
                &contract,
                &windows,
                &other_seeds,
                retries,
                |_w, seed| Ok(skilled_run(seed)),
            ) {
                Ok(_) => panic!("seeds {other_seeds:?} / {retries} retries are not the contract"),
                Err(error) => error,
            };
            assert_eq!(error.kind(), std::io::ErrorKind::InvalidInput);
            assert!(!path.exists(), "an invalid contract must write nothing");
        }

        // The windows leg is the one `run_resumable_sweep_observed` can still
        // check, and it does.
        let different = [Window { start: 21, end: 61 }];
        let error = match run_resumable_sweep_bound_with_policy(
            &path,
            "ext",
            &contract,
            &different,
            ResumePolicy::UnfinishedOnly,
            |_w, seed| Ok(skilled_run(seed)),
        ) {
            Ok(_) => panic!("the window matrix must still be compared"),
            Err(error) => error,
        };
        assert_eq!(error.kind(), std::io::ErrorKind::InvalidInput);
        assert!(error.to_string().contains("supplied windows"), "{error}");
        assert!(!path.exists());
    }
}
