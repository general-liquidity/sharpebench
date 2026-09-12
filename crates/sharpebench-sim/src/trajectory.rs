//! Trajectory capture + replay-recompute — the Tier 1 verification boundary.
//!
//! The persisted artifact is the agent's *raw decisions*, never its returns or any
//! self-reported metric (those are recomputed). [`run_backtest_capture`] records the
//! [`Decision`] the agent emits at every point-in-time step while it drives the
//! identical [`crate::engine::run_backtest`] path; [`replay_run`] feeds those frozen
//! decisions back through the **same** engine to regenerate the [`Run`]
//! byte-for-byte. Because capture and replay share one engine code path and the
//! engine's only other input (the execution seed) is stored in the trajectory, the
//! round trip is exact by construction: replaying a captured trajectory reproduces
//! the original `Run` exactly, so a score recomputed from the artifact is provably
//! the score the agent's decisions actually earned.

use serde::{Deserialize, Serialize};
use sharpebench_core::Run;
use sharpebench_protocol::{
    AgentTrajectory, Decision, DecisionStep, MarketObservation, RunTrajectory,
};

use crate::agent::Agent;
use crate::costs::CostModel;
use crate::data::Dataset;
use crate::engine::{run_backtest, Window};

/// Wraps an [`Agent`], recording every [`Decision`] it makes (tagged with the
/// observation's date and step) while passing the call straight through. The
/// recorded steps become a [`RunTrajectory`] — the raw, replayable artifact.
struct CapturingAgent<'a> {
    inner: &'a mut dyn Agent,
    window_start: usize,
    steps: Vec<DecisionStep>,
}

impl Agent for CapturingAgent<'_> {
    fn decide(&mut self, obs: &MarketObservation) -> Decision {
        let decision = self.inner.decide(obs);
        self.steps.push(DecisionStep {
            step: self.steps.len(),
            observation_id: obs.date.clone(),
            decision: decision.clone(),
        });
        // The engine sees the byte-identical decision the inner agent returned.
        decision
    }
}

/// Replays a frozen sequence of decisions: returns each recorded [`Decision`] in
/// step order, ignoring the live observation. Past the end (which cannot happen on
/// a faithful replay of the same window) it holds. Driving this through the engine
/// is what makes replay deterministic and engine-identical.
struct ReplayAgent {
    steps: std::vec::IntoIter<DecisionStep>,
}

impl Agent for ReplayAgent {
    fn decide(&mut self, _obs: &MarketObservation) -> Decision {
        self.steps
            .next()
            .map(|s| s.decision)
            .unwrap_or_else(|| Decision {
                orders: Vec::new(),
                reasoning: "replay exhausted → hold".to_string(),
                cost: None,
            })
    }
}

/// Run a backtest while capturing the agent's raw decisions. Returns the same
/// [`Run`] [`run_backtest`] would produce, plus the [`RunTrajectory`] artifact (the
/// per-step decisions + the window/seed coordinates needed to replay it).
pub fn run_backtest_capture(
    data: &Dataset,
    agent: &mut dyn Agent,
    window: Window,
    seed: u64,
    costs: CostModel,
) -> (Run, RunTrajectory) {
    let mut cap = CapturingAgent {
        inner: agent,
        window_start: window.start,
        steps: Vec::new(),
    };
    let run = run_backtest(data, &mut cap, window, seed, costs);
    let traj = RunTrajectory {
        window_start: cap.window_start,
        window_end: window.end,
        seed,
        steps: cap.steps,
    };
    (run, traj)
}

/// Replay one captured run's raw decisions through the identical point-in-time
/// engine to regenerate its [`Run`]. The frozen `data` and `costs` must match those
/// the trajectory was captured against; the window and seed come from the artifact.
///
/// Round-trip invariant: `replay_run(data, &traj, costs)` is byte-identical to the
/// `Run` returned alongside `traj` by [`run_backtest_capture`].
pub fn replay_run(data: &Dataset, traj: &RunTrajectory, costs: CostModel) -> Run {
    let mut agent = ReplayAgent {
        steps: traj.steps.clone().into_iter(),
    };
    run_backtest(
        data,
        &mut agent,
        Window {
            start: traj.window_start,
            end: traj.window_end,
        },
        traj.seed,
        costs,
    )
}

/// Replay a whole [`AgentTrajectory`] back into the [`sharpebench_core::AgentSubmission`]
/// the scoring kernel consumes — recomputing every `Run` from raw decisions alone.
/// This is the engine half of the separate-verifier path: the resulting submission
/// is derived only from the persisted decisions + the frozen dataset, never from any
/// metric the agent reported.
pub fn replay_submission(
    data: &Dataset,
    traj: &AgentTrajectory,
    costs: CostModel,
) -> sharpebench_core::AgentSubmission {
    let runs = traj
        .runs
        .iter()
        .map(|rt| replay_run(data, rt, costs))
        .collect();
    sharpebench_core::AgentSubmission {
        agent_id: traj.agent_id.clone(),
        runs,
        in_sample_trials: traj.in_sample_trials,
        candidates: Vec::new(),
    }
}

/// Who graded a frozen artifact, and under what resource limits.
///
/// A regrade changes the evaluator, never the run. The limits are part of the
/// identity because a grade produced under a different wall-clock or memory
/// ceiling is a different evaluation of the same decisions, and a reader
/// comparing an original grade with its replacement has to see that.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct EvaluatorIdentity {
    pub score_config_sha256: String,
    pub verifier_artifact_sha256: String,
    pub wall_clock_limit_secs: u64,
    pub memory_limit_bytes: u64,
}

impl EvaluatorIdentity {
    /// The inputs that differ from `original`, named in a fixed order.
    pub fn changed_from(&self, original: &Self) -> Vec<String> {
        let mut changed = Vec::new();
        if self.score_config_sha256 != original.score_config_sha256 {
            changed.push("score_config_sha256".to_string());
        }
        if self.verifier_artifact_sha256 != original.verifier_artifact_sha256 {
            changed.push("verifier_artifact_sha256".to_string());
        }
        if self.wall_clock_limit_secs != original.wall_clock_limit_secs {
            changed.push("wall_clock_limit_secs".to_string());
        }
        if self.memory_limit_bytes != original.memory_limit_bytes {
            changed.push("memory_limit_bytes".to_string());
        }
        changed
    }
}

/// What a superseding grade is allowed to do to the record it supersedes.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "disposition")]
pub enum RegradeDisposition {
    /// The source artifact is not frozen published evidence: the superseding
    /// grade may stand in for the grade it replaces wherever that is held.
    ReplacesSource,
    /// The source is frozen published evidence. The superseding grade is
    /// reportable and nothing more: the published record keeps its own number.
    OperationalOnly { frozen_record: String },
}

/// Refusals a regrade returns rather than producing a grade it cannot justify.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RegradeRefusal {
    /// A run's artifact does not carry a decision for every step of its window.
    /// Replaying it would manufacture holds the agent never made, which is the
    /// evaluator deciding, so the regrade is refused instead.
    FabricatedDecisions {
        run: usize,
        recorded: usize,
        required: usize,
    },
    /// A regrade with no stated reason is not auditable.
    MissingReason,
    /// The source artifact identity is not a SHA-256 digest.
    MalformedSourceDigest,
    /// The source is frozen published evidence and cannot be written over.
    FrozenPublishedRecord { record: String },
}

impl std::fmt::Display for RegradeRefusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::FabricatedDecisions {
                run,
                recorded,
                required,
            } => write!(
                f,
                "run {run} records {recorded} of {required} decisions: a regrade would have to invent the rest"
            ),
            Self::MissingReason => write!(f, "a regrade must state its reason"),
            Self::MalformedSourceDigest => {
                write!(f, "the source artifact identity is not a SHA-256 digest")
            }
            Self::FrozenPublishedRecord { record } => write!(
                f,
                "{record} is frozen published evidence: the superseding grade is reportable, never a replacement"
            ),
        }
    }
}

impl std::error::Error for RegradeRefusal {}

/// What a caller asks for when regrading a frozen artifact.
pub struct RegradeRequest<'a> {
    /// SHA-256 of the artifact bytes being regraded. The receipt names the
    /// source by identity; the regrade never rewrites it.
    pub source_artifact_sha256: &'a str,
    pub original_evaluator: &'a EvaluatorIdentity,
    pub replacement_evaluator: &'a EvaluatorIdentity,
    /// Why the original grade is being replaced.
    pub reason: &'a str,
    /// Digests of artifacts whose grades are frozen published evidence.
    pub frozen_published: &'a [String],
}

/// A regrade: the link from an original artifact to the grade that supersedes
/// it, the evaluator identity on both sides, and the reason.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RegradeReceipt {
    pub schema_version: u32,
    pub source_agent_id: String,
    pub source_artifact_sha256: String,
    pub original_evaluator: EvaluatorIdentity,
    pub replacement_evaluator: EvaluatorIdentity,
    /// Every evaluator input that differs between the two, named.
    pub changed_evaluator_inputs: Vec<String>,
    pub reason: String,
    /// Decisions read back out of the artifact. Every graded step comes from
    /// there.
    pub decisions_replayed: usize,
    /// Always zero. A regrade consumes recorded decisions only: one it cannot
    /// read is refused, never bought from an agent.
    pub agent_invocations: u32,
    pub disposition: RegradeDisposition,
}

impl RegradeReceipt {
    pub const SCHEMA_VERSION: u32 = 1;

    /// Whether the superseding grade may replace the record it grades.
    pub fn may_replace_published(&self) -> bool {
        matches!(self.disposition, RegradeDisposition::ReplacesSource)
    }

    /// Take the receipt as a replacement for the record it grades, or refuse
    /// naming the frozen record that stays as published.
    pub fn into_published_replacement(self) -> Result<Self, RegradeRefusal> {
        match &self.disposition {
            RegradeDisposition::ReplacesSource => Ok(self),
            RegradeDisposition::OperationalOnly { frozen_record } => {
                Err(RegradeRefusal::FrozenPublishedRecord {
                    record: frozen_record.clone(),
                })
            }
        }
    }
}

/// Regrade a frozen trajectory: recompute its submission from the recorded
/// decisions alone and return it with the receipt that links source, evaluator
/// identity, reason and disposition.
///
/// No agent runs. [`replay_submission`] drives the engine off the artifact, so
/// the expensive half of an evaluation (buying the model's decisions) is not
/// repeated when only the evaluator failed. The one way an agent's judgement
/// could sneak back in is a short artifact, where the replay would fall back to
/// holds it was never told to make; that is refused rather than graded.
pub fn regrade_submission(
    data: &Dataset,
    traj: &AgentTrajectory,
    costs: CostModel,
    request: RegradeRequest<'_>,
) -> Result<(sharpebench_core::AgentSubmission, RegradeReceipt), RegradeRefusal> {
    if request.reason.trim().is_empty() {
        return Err(RegradeRefusal::MissingReason);
    }
    let digest = request.source_artifact_sha256;
    if digest.len() != 64
        || !digest
            .bytes()
            .all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'))
    {
        return Err(RegradeRefusal::MalformedSourceDigest);
    }

    let mut decisions_replayed = 0;
    for (index, run) in traj.runs.iter().enumerate() {
        let required = run.window_end.saturating_sub(run.window_start);
        if run.steps.len() != required {
            return Err(RegradeRefusal::FabricatedDecisions {
                run: index,
                recorded: run.steps.len(),
                required,
            });
        }
        decisions_replayed += run.steps.len();
    }

    let disposition = if request
        .frozen_published
        .iter()
        .any(|record| record == digest)
    {
        RegradeDisposition::OperationalOnly {
            frozen_record: digest.to_string(),
        }
    } else {
        RegradeDisposition::ReplacesSource
    };

    let receipt = RegradeReceipt {
        schema_version: RegradeReceipt::SCHEMA_VERSION,
        source_agent_id: traj.agent_id.clone(),
        source_artifact_sha256: digest.to_string(),
        original_evaluator: request.original_evaluator.clone(),
        replacement_evaluator: request.replacement_evaluator.clone(),
        changed_evaluator_inputs: request
            .replacement_evaluator
            .changed_from(request.original_evaluator),
        reason: request.reason.to_string(),
        decisions_replayed,
        agent_invocations: 0,
        disposition,
    };
    Ok((replay_submission(data, traj, costs), receipt))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::{BuyAndHold, Momentum};

    /// Counts every decision it is asked for: what a regrade must never spend.
    struct CountingAgent {
        inner: BuyAndHold,
        decisions: std::rc::Rc<std::cell::Cell<u32>>,
    }

    impl Agent for CountingAgent {
        fn decide(&mut self, obs: &MarketObservation) -> Decision {
            self.decisions.set(self.decisions.get() + 1);
            self.inner.decide(obs)
        }
    }

    fn source_digest() -> String {
        "cd".repeat(32)
    }

    fn evaluator(score_config: &str, wall_clock_limit_secs: u64) -> EvaluatorIdentity {
        EvaluatorIdentity {
            score_config_sha256: score_config.repeat(32),
            verifier_artifact_sha256: "ab".repeat(32),
            wall_clock_limit_secs,
            memory_limit_bytes: 4 << 30,
        }
    }

    /// One captured trajectory plus the decision count its capture cost.
    fn captured() -> (Dataset, AgentTrajectory, std::rc::Rc<std::cell::Cell<u32>>) {
        let data = Dataset::synthetic(3, 100, 7);
        let decisions = std::rc::Rc::new(std::cell::Cell::new(0));
        let mut agent = CountingAgent {
            inner: BuyAndHold,
            decisions: std::rc::Rc::clone(&decisions),
        };
        let (_run, run_traj) = run_backtest_capture(
            &data,
            &mut agent,
            Window {
                start: 20,
                end: 100,
            },
            4,
            CostModel::default(),
        );
        let traj = AgentTrajectory {
            agent_id: "entrant".to_string(),
            contract: None,
            in_sample_trials: 0,
            declared_mandate: None,
            runs: vec![run_traj],
        };
        (data, traj, decisions)
    }

    fn request<'a>(
        digest: &'a str,
        original: &'a EvaluatorIdentity,
        replacement: &'a EvaluatorIdentity,
        frozen: &'a [String],
    ) -> RegradeRequest<'a> {
        RegradeRequest {
            source_artifact_sha256: digest,
            original_evaluator: original,
            replacement_evaluator: replacement,
            reason: "the verifier ran out of memory and returned no grade",
            frozen_published: frozen,
        }
    }

    #[test]
    fn a_regrade_buys_no_decisions_and_leaves_the_source_artifact_alone() {
        let (data, traj, decisions) = captured();
        let captured_cost = decisions.get();
        let source_bytes = serde_json::to_string(&traj).unwrap();
        let (original, replacement) = (evaluator("11", 900), evaluator("11", 1800));

        let (submission, receipt) = regrade_submission(
            &data,
            &traj,
            CostModel::default(),
            request(&source_digest(), &original, &replacement, &[]),
        )
        .unwrap();

        assert_eq!(
            decisions.get(),
            captured_cost,
            "a regrade must not ask the agent for a single decision"
        );
        assert_eq!(receipt.agent_invocations, 0);
        assert_eq!(receipt.decisions_replayed, 80);
        assert_eq!(submission.runs.len(), 1);
        assert_eq!(
            serde_json::to_string(&traj).unwrap(),
            source_bytes,
            "the source artifact is graded, never rewritten"
        );
    }

    #[test]
    fn a_regrade_that_would_invent_a_decision_is_refused() {
        let (data, mut traj, _decisions) = captured();
        traj.runs[0].steps.pop();
        let (original, replacement) = (evaluator("11", 900), evaluator("11", 1800));
        let Err(refusal) = regrade_submission(
            &data,
            &traj,
            CostModel::default(),
            request(&source_digest(), &original, &replacement, &[]),
        ) else {
            panic!("a short artifact must not be graded");
        };
        assert_eq!(
            refusal,
            RegradeRefusal::FabricatedDecisions {
                run: 0,
                recorded: 79,
                required: 80,
            }
        );
    }

    #[test]
    fn a_regrade_records_every_changed_evaluator_input() {
        let (data, traj, _decisions) = captured();
        let (original, replacement) = (evaluator("11", 900), evaluator("22", 1800));
        let (_submission, receipt) = regrade_submission(
            &data,
            &traj,
            CostModel::default(),
            request(&source_digest(), &original, &replacement, &[]),
        )
        .unwrap();
        assert_eq!(
            receipt.changed_evaluator_inputs,
            vec!["score_config_sha256", "wall_clock_limit_secs"],
            "both the scorer and the resource limit that moved are named"
        );
        assert_eq!(receipt.original_evaluator, original);
        assert_eq!(receipt.replacement_evaluator, replacement);
        assert_eq!(receipt.source_agent_id, "entrant");
        assert_eq!(receipt.source_artifact_sha256, source_digest());
    }

    #[test]
    fn a_superseding_grade_never_replaces_frozen_published_evidence() {
        let (data, traj, _decisions) = captured();
        let (original, replacement) = (evaluator("11", 900), evaluator("22", 900));
        let frozen = vec![source_digest()];

        let (submission, receipt) = regrade_submission(
            &data,
            &traj,
            CostModel::default(),
            request(&source_digest(), &original, &replacement, &frozen),
        )
        .unwrap();

        // The grade exists and may be reported; it just cannot stand in for the
        // published record.
        assert_eq!(submission.runs.len(), 1);
        assert!(!receipt.may_replace_published());
        assert_eq!(
            receipt.disposition,
            RegradeDisposition::OperationalOnly {
                frozen_record: source_digest(),
            }
        );
        assert_eq!(
            receipt.into_published_replacement(),
            Err(RegradeRefusal::FrozenPublishedRecord {
                record: source_digest(),
            })
        );

        // The identical regrade of a source nobody published may replace it.
        let (_submission, loose) = regrade_submission(
            &data,
            &traj,
            CostModel::default(),
            request(&source_digest(), &original, &replacement, &[]),
        )
        .unwrap();
        assert!(loose.may_replace_published());
        assert!(loose.into_published_replacement().is_ok());
    }

    #[test]
    fn a_regrade_without_a_reason_or_a_source_identity_is_refused() {
        let (data, traj, _decisions) = captured();
        let (original, replacement) = (evaluator("11", 900), evaluator("22", 900));
        let digest = source_digest();
        let unreasoned = RegradeRequest {
            reason: "   ",
            ..request(&digest, &original, &replacement, &[])
        };
        let Err(unreasoned) = regrade_submission(&data, &traj, CostModel::default(), unreasoned)
        else {
            panic!("a reasonless regrade must not be graded");
        };
        assert_eq!(unreasoned, RegradeRefusal::MissingReason);

        let unidentified = RegradeRequest {
            source_artifact_sha256: "not-a-digest",
            ..request(&digest, &original, &replacement, &[])
        };
        let Err(unidentified) =
            regrade_submission(&data, &traj, CostModel::default(), unidentified)
        else {
            panic!("a regrade with no source identity must not be graded");
        };
        assert_eq!(unidentified, RegradeRefusal::MalformedSourceDigest);
    }

    #[test]
    fn capture_then_replay_is_byte_identical() {
        let data = Dataset::synthetic(4, 120, 11);
        let window = Window {
            start: 20,
            end: 120,
        };
        let costs = CostModel::default();
        let (direct, traj) =
            run_backtest_capture(&data, &mut Momentum::default(), window, 3, costs);
        let replayed = replay_run(&data, &traj, costs);
        // The whole Run must match field-for-field: returns, trace, conf, outcomes.
        assert_eq!(
            serde_json::to_string(&direct).unwrap(),
            serde_json::to_string(&replayed).unwrap(),
            "replay must reproduce the captured run byte-for-byte"
        );
    }

    #[test]
    fn capture_matches_a_plain_run() {
        let data = Dataset::synthetic(3, 100, 5);
        let window = Window {
            start: 20,
            end: 100,
        };
        let costs = CostModel::default();
        let plain = run_backtest(&data, &mut BuyAndHold, window, 1, costs);
        let (captured, _traj) = run_backtest_capture(&data, &mut BuyAndHold, window, 1, costs);
        assert_eq!(
            serde_json::to_string(&plain).unwrap(),
            serde_json::to_string(&captured).unwrap(),
            "capturing must not perturb the run"
        );
    }

    #[test]
    fn trajectory_records_one_step_per_window_day() {
        let data = Dataset::synthetic(2, 60, 9);
        let window = Window { start: 20, end: 60 };
        let (_run, traj) =
            run_backtest_capture(&data, &mut BuyAndHold, window, 0, CostModel::default());
        assert_eq!(traj.steps.len(), 40, "one decision per step in the window");
        assert_eq!(traj.steps[0].observation_id, data.dates[20]);
        assert_eq!(traj.steps[0].step, 0);
    }

    #[test]
    fn tampered_trajectory_yields_a_different_run() {
        let data = Dataset::synthetic(4, 120, 11);
        let window = Window {
            start: 20,
            end: 120,
        };
        let costs = CostModel::default();
        let (direct, mut traj) =
            run_backtest_capture(&data, &mut Momentum::default(), window, 3, costs);
        // Tamper: inflate every order's target weight. An honest replay through the
        // engine produces a *different* Run — the artifact can't lie about returns.
        for step in &mut traj.steps {
            for order in &mut step.decision.orders {
                order.target_weight *= 2.0;
            }
        }
        let replayed = replay_run(&data, &traj, costs);
        assert_ne!(
            direct.returns, replayed.returns,
            "a tampered trajectory must recompute to different returns"
        );
    }
}
