//! Treatment-activation receipts and a placebo-controlled retrieval comparison.
//!
//! "Memory enabled" is a configuration flag, not evidence that retrieved content
//! reached a trading decision. A retrieval arm's lift can come from the retrieved
//! content, from the extra prompt bytes it adds, or from extra compute. This leg
//! separates those three readings without replacing the oracle ceiling of
//! [`crate::ablation_report`]:
//!
//! 1. **Activation established.** An [`ActivationReceipt`] records, for one
//!    decision where memory was offered, the SHA-256 of the exact offered bytes,
//!    when those bytes became available, when the decision was taken, and the
//!    digest of every framed segment of the decision-boundary input. The content
//!    counts as exposed only when one whole segment has the same digest. That is
//!    structured evidence from the host that assembled the input, not a substring
//!    search over a log. A retrieval arm is activated only when at least one
//!    receipt shows exposure; a worker that writes memory which never reaches a
//!    decision boundary is not an activated treatment.
//! 2. **Placebo-controlled lift.** A [`TreatmentKind::Placebo`] arm exposes
//!    content of matched byte length that is meant to carry no task information,
//!    at the same decision boundaries, under the same model, task and budget
//!    identities. [`placebo_controlled_report`] refuses, with a typed cause, any
//!    comparison whose identities differ, and reports `retrieval - placebo`.
//! 3. **Causal trading improvement.** Not established by this module. The report
//!    says so in [`PlaceboControlledReport::not_established`].
//!
//! Point-in-time semantics follow [`crate::pit`]: content available after the
//! decision instant has leaked the future. Here the receipt constructors refuse it
//! outright as [`ActivationError::PointInTimeViolation`], because a receipt is
//! per-decision evidence and a lookahead receipt cannot be scored as activation.
//! Availability equal to the decision time is allowed.
//!
//! An opaque agent (for example one reached only over HTTP) that cannot produce
//! receipts supplies [`ActivationEvidence::Unavailable`]. That stays diagnostic:
//! the comparison is still computed, the arm is never treated as activated, and
//! the lift is labeled a proxy.
//!
//! Pure and deterministic like the rest of the crate: timestamps are
//! caller-supplied integers in any consistent unit, no clock is read, and the
//! significance test reuses the crate's fixed stationary-bootstrap parameters.
//!
//! ```
//! use sharpebench_memory::activation::{
//!     placebo_controlled_report, ActivationEvidence, ActivationReceipt, DecisionBoundary,
//!     DecisionBudget, LiftEvidence, OfferedMemory, TreatmentArm, TreatmentIdentity,
//!     TreatmentKind,
//! };
//!
//! let identity = TreatmentIdentity::new(
//!     "model-a",
//!     vec!["t1".into(), "t2".into(), "t3".into()],
//!     DecisionBudget { max_tokens: 4096, max_latency: 2000 },
//! )
//! .unwrap();
//!
//! let retrieved = OfferedMemory { content: b"BTC funding flipped".to_vec(), available_at: 90 };
//! let placebo = OfferedMemory { content: b"lorem ipsum dolor s".to_vec(), available_at: 90 };
//! let seen = |memory: &OfferedMemory| DecisionBoundary {
//!     decided_at: 100,
//!     segments: vec![b"system prompt".to_vec(), memory.content.clone()],
//! };
//!
//! let retrieval_arm = TreatmentArm::new(
//!     TreatmentKind::Retrieval,
//!     identity.clone(),
//!     vec![0.8, 0.7, 0.9],
//!     ActivationEvidence::Receipts(vec![
//!         ActivationReceipt::observe("d1", &retrieved, &seen(&retrieved)).unwrap(),
//!     ]),
//! );
//! let placebo_arm = TreatmentArm::new(
//!     TreatmentKind::Placebo,
//!     identity,
//!     vec![0.4, 0.5, 0.4],
//!     ActivationEvidence::Receipts(vec![
//!         ActivationReceipt::observe("d1", &placebo, &seen(&placebo)).unwrap(),
//!     ]),
//! );
//!
//! let report = placebo_controlled_report(&retrieval_arm, &placebo_arm, 0.05).unwrap();
//! assert!(report.activation_established);
//! assert_eq!(report.lift_evidence, LiftEvidence::PlaceboControlled);
//! assert!(report.placebo_controlled_lift > 0.0);
//! assert!(!report.not_established.is_empty()); // causal improvement is never claimed
//! ```

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::{self, Write as _};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use sharpebench_stats::{significance::bootstrap_pvalue, stats::mean};

use crate::{validate_alpha, BOOTSTRAP_BLOCK_PROB, BOOTSTRAP_SAMPLES, BOOTSTRAP_SEED};

/// Lowercase hex SHA-256 of `bytes`.
pub fn sha256_hex(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(64);
    for byte in Sha256::digest(bytes) {
        let _ = write!(out, "{byte:02x}");
    }
    out
}

fn is_sha256_hex(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

/// Which treatment an arm is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TreatmentKind {
    /// The retrieval layer under test, exposing retrieved content.
    Retrieval,
    /// Length-matched content meant to carry no task information, exposed at the
    /// same boundaries. Controls for extra prompt bytes and the compute they cost.
    Placebo,
}

impl TreatmentKind {
    /// Stable lowercase label.
    pub fn as_str(&self) -> &'static str {
        match self {
            TreatmentKind::Retrieval => "retrieval",
            TreatmentKind::Placebo => "placebo",
        }
    }
}

/// Memory content offered to one decision, with the instant it became available.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OfferedMemory {
    /// The exact bytes offered.
    pub content: Vec<u8>,
    /// When the content became available, on the caller's clock.
    pub available_at: i64,
}

/// The decision-boundary input as the host framed it: the segments the decision
/// was actually taken on, and when.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DecisionBoundary {
    /// When the decision was taken, on the same clock as
    /// [`OfferedMemory::available_at`].
    pub decided_at: i64,
    /// The framed input segments, one entry per segment. Memory counts as exposed
    /// only when it occupies a whole segment byte for byte.
    pub segments: Vec<Vec<u8>>,
}

/// Evidence that memory offered to one decision was, or was not, present in the
/// decision-boundary input. Every constructor, including deserialization, refuses
/// a malformed digest, an empty decision id, and content available after the
/// decision time.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ActivationReceipt {
    decision_id: String,
    content_sha256: String,
    content_len: u64,
    available_at: i64,
    decided_at: i64,
    boundary_segment_sha256: Vec<String>,
}

/// The wire shape of a receipt, and the only way one is read back, so a parsed
/// receipt passes the same checks as one built by [`ActivationReceipt::from_digests`].
#[derive(Deserialize)]
#[serde(rename = "ActivationReceipt", deny_unknown_fields)]
struct ReceiptWire {
    decision_id: String,
    content_sha256: String,
    content_len: u64,
    available_at: i64,
    decided_at: i64,
    boundary_segment_sha256: Vec<String>,
}

impl<'de> Deserialize<'de> for ActivationReceipt {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let wire = ReceiptWire::deserialize(deserializer)?;
        Self::from_digests(
            &wire.decision_id,
            &wire.content_sha256,
            wire.content_len,
            wire.available_at,
            wire.decided_at,
            wire.boundary_segment_sha256,
        )
        .map_err(serde::de::Error::custom)
    }
}

impl ActivationReceipt {
    /// Build a receipt by hashing the offered content and every boundary segment.
    pub fn observe(
        decision_id: &str,
        offered: &OfferedMemory,
        boundary: &DecisionBoundary,
    ) -> Result<Self, ActivationError> {
        Self::from_digests(
            decision_id,
            &sha256_hex(&offered.content),
            offered.content.len() as u64,
            offered.available_at,
            boundary.decided_at,
            boundary.segments.iter().map(|s| sha256_hex(s)).collect(),
        )
    }

    /// Build a receipt from digests a host recorded, for a host that retains
    /// digests rather than the raw bytes.
    pub fn from_digests(
        decision_id: &str,
        content_sha256: &str,
        content_len: u64,
        available_at: i64,
        decided_at: i64,
        boundary_segment_sha256: Vec<String>,
    ) -> Result<Self, ActivationError> {
        if decision_id.is_empty() {
            return Err(ActivationError::EmptyDecisionId);
        }
        if !is_sha256_hex(content_sha256) {
            return Err(ActivationError::MalformedDigest {
                decision_id: decision_id.to_string(),
                field: "content_sha256",
            });
        }
        if boundary_segment_sha256.iter().any(|d| !is_sha256_hex(d)) {
            return Err(ActivationError::MalformedDigest {
                decision_id: decision_id.to_string(),
                field: "boundary_segment_sha256",
            });
        }
        if available_at > decided_at {
            return Err(ActivationError::PointInTimeViolation {
                decision_id: decision_id.to_string(),
                available_at,
                decided_at,
            });
        }
        Ok(Self {
            decision_id: decision_id.to_string(),
            content_sha256: content_sha256.to_string(),
            content_len,
            available_at,
            decided_at,
            boundary_segment_sha256,
        })
    }

    /// The decision this receipt covers.
    pub fn decision_id(&self) -> &str {
        &self.decision_id
    }

    /// SHA-256 of the exact bytes offered.
    pub fn content_sha256(&self) -> &str {
        &self.content_sha256
    }

    /// Byte length of the offered content.
    pub fn content_len(&self) -> u64 {
        self.content_len
    }

    /// When the offered content became available.
    pub fn available_at(&self) -> i64 {
        self.available_at
    }

    /// When the decision was taken.
    pub fn decided_at(&self) -> i64 {
        self.decided_at
    }

    /// Digests of the framed decision-boundary segments.
    pub fn boundary_segment_sha256(&self) -> &[String] {
        &self.boundary_segment_sha256
    }

    /// Whether the offered bytes occupied a whole segment of the decision input.
    /// Derived from the digests, never carried as a separate flag.
    pub fn exposed(&self) -> bool {
        self.boundary_segment_sha256.contains(&self.content_sha256)
    }
}

/// What an arm can show about memory reaching its decisions.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActivationEvidence {
    /// One receipt per decision where memory was offered. Decision ids must be
    /// unique within an arm.
    Receipts(Vec<ActivationReceipt>),
    /// The agent is opaque to the host and produced no receipts. Diagnostic, not
    /// invalidating: the arm is never treated as activated.
    Unavailable {
        /// Why no receipts exist, for the report.
        reason: String,
    },
}

/// The declared compute budget a treatment ran under. Compared for equality only.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DecisionBudget {
    /// Declared token cap.
    pub max_tokens: u64,
    /// Declared latency cap, in any consistent unit.
    pub max_latency: u64,
}

/// The model, task and budget a treatment ran under. The placebo and retrieval
/// arms must share it exactly.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TreatmentIdentity {
    model_id: String,
    task_ids: Vec<String>,
    budget: DecisionBudget,
}

#[derive(Deserialize)]
#[serde(rename = "TreatmentIdentity", deny_unknown_fields)]
struct IdentityWire {
    model_id: String,
    task_ids: Vec<String>,
    budget: DecisionBudget,
}

impl<'de> Deserialize<'de> for TreatmentIdentity {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let wire = IdentityWire::deserialize(deserializer)?;
        Self::new(wire.model_id, wire.task_ids, wire.budget).map_err(serde::de::Error::custom)
    }
}

impl TreatmentIdentity {
    /// Declare an identity. Refuses an empty model id, an empty task list, an
    /// empty task id and a repeated task id. Task order is significant: scores
    /// pair with `task_ids` positionally.
    pub fn new(
        model_id: impl Into<String>,
        task_ids: Vec<String>,
        budget: DecisionBudget,
    ) -> Result<Self, ActivationError> {
        let model_id = model_id.into();
        if model_id.is_empty() {
            return Err(ActivationError::EmptyModelId);
        }
        if task_ids.is_empty() {
            return Err(ActivationError::EmptyTaskSet);
        }
        let mut seen = BTreeSet::new();
        for (index, task) in task_ids.iter().enumerate() {
            if task.is_empty() {
                return Err(ActivationError::EmptyTaskId { index });
            }
            if !seen.insert(task.as_str()) {
                return Err(ActivationError::DuplicateTaskId {
                    task_id: task.clone(),
                });
            }
        }
        Ok(Self {
            model_id,
            task_ids,
            budget,
        })
    }

    /// The model identity.
    pub fn model_id(&self) -> &str {
        &self.model_id
    }

    /// The task identities, in scoring order.
    pub fn task_ids(&self) -> &[String] {
        &self.task_ids
    }

    /// The declared budget.
    pub fn budget(&self) -> DecisionBudget {
        self.budget
    }
}

/// One treatment arm: its identity, per-task scores and activation evidence.
#[derive(Debug, Clone, PartialEq)]
pub struct TreatmentArm {
    /// Which treatment this is.
    pub kind: TreatmentKind,
    /// Model, tasks and budget. Scores pair with `identity.task_ids()`.
    pub identity: TreatmentIdentity,
    /// One outcome score per task.
    pub scores: Vec<f64>,
    /// Receipts, or an explicit statement that none exist.
    pub evidence: ActivationEvidence,
}

impl TreatmentArm {
    /// Construct a treatment arm.
    pub fn new(
        kind: TreatmentKind,
        identity: TreatmentIdentity,
        scores: Vec<f64>,
        evidence: ActivationEvidence,
    ) -> Self {
        Self {
            kind,
            identity,
            scores,
            evidence,
        }
    }
}

/// Whether an arm's evidence establishes that memory reached a decision.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ActivationStatus {
    /// At least one receipt shows the offered bytes in the decision input.
    Activated {
        /// Decisions with a receipt.
        offered_decisions: usize,
        /// Decisions whose receipt shows exposure.
        exposed_decisions: usize,
    },
    /// Receipts exist and none shows exposure: memory was written or offered but
    /// never reached a decision boundary.
    NotActivated {
        /// Decisions with a receipt.
        offered_decisions: usize,
    },
    /// No receipts could be produced.
    Unavailable {
        /// The reason the caller gave.
        reason: String,
    },
}

/// Classify an arm's activation evidence.
///
/// # Errors
///
/// [`ActivationError::DuplicateDecision`] when two receipts cover the same
/// decision id.
pub fn activation_status(
    kind: TreatmentKind,
    evidence: &ActivationEvidence,
) -> Result<ActivationStatus, ActivationError> {
    let receipts = match evidence {
        ActivationEvidence::Unavailable { reason } => {
            return Ok(ActivationStatus::Unavailable {
                reason: reason.clone(),
            })
        }
        ActivationEvidence::Receipts(receipts) => receipts,
    };
    let mut seen = BTreeSet::new();
    for receipt in receipts {
        if !seen.insert(receipt.decision_id()) {
            return Err(ActivationError::DuplicateDecision {
                kind,
                decision_id: receipt.decision_id().to_string(),
            });
        }
    }
    let offered_decisions = receipts.len();
    let exposed_decisions = receipts.iter().filter(|r| r.exposed()).count();
    if exposed_decisions > 0 {
        Ok(ActivationStatus::Activated {
            offered_decisions,
            exposed_decisions,
        })
    } else {
        Ok(ActivationStatus::NotActivated { offered_decisions })
    }
}

/// Whether the placebo was shown to match the retrieval arm at its boundaries.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlaceboMatch {
    /// At every decision where retrieval content was exposed, placebo content of
    /// the same byte length and a different digest was exposed at the same
    /// decision time, and nowhere else.
    Matched {
        /// Decisions matched.
        decisions: usize,
    },
    /// The match could not be checked.
    NotAssessed {
        /// Why.
        reason: String,
    },
}

/// What the placebo-controlled lift is evidence of.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LiftEvidence {
    /// Retrieval activated and the placebo matched at its boundaries.
    PlaceboControlled,
    /// Proxy: the retrieval arm has no activation receipts.
    ProxyActivationUnavailable,
    /// Proxy: the retrieval arm's receipts show no exposure.
    ProxyRetrievalNotActivated,
    /// Proxy: retrieval activated but the placebo match was not assessable.
    ProxyPlaceboUnverified,
}

impl LiftEvidence {
    /// `"placebo-controlled"` or `"proxy"`.
    pub fn label(&self) -> &'static str {
        match self {
            LiftEvidence::PlaceboControlled => "placebo-controlled",
            _ => "proxy",
        }
    }
}

/// The scored placebo comparison. Three claims are reported separately and never
/// merged: activation, placebo-controlled lift, and (never) causal improvement.
#[derive(Debug, Clone, PartialEq)]
pub struct PlaceboControlledReport {
    /// Activation of the retrieval arm.
    pub retrieval_activation: ActivationStatus,
    /// Exposure of the placebo arm, classified with the same predicate.
    pub placebo_activation: ActivationStatus,
    /// Claim 1: the retrieval arm's receipts show memory reached a decision.
    pub activation_established: bool,
    /// Whether the placebo matched the retrieval boundaries.
    pub placebo_match: PlaceboMatch,
    /// Mean retrieval outcome.
    pub retrieval_mean: f64,
    /// Mean placebo outcome.
    pub placebo_mean: f64,
    /// Claim 2: `retrieval_mean - placebo_mean`. Read with
    /// [`PlaceboControlledReport::lift_evidence`].
    pub placebo_controlled_lift: f64,
    /// Stationary-bootstrap p-value that the paired per-task lift is > 0, with the
    /// same fixed parameters as [`crate::ablation_report`].
    pub lift_pvalue: f64,
    /// `lift_pvalue < alpha`.
    pub significant: bool,
    /// The threshold used.
    pub alpha: f64,
    /// Whether the lift is placebo-controlled or a proxy, and why.
    pub lift_evidence: LiftEvidence,
    /// Claims the evidence supports.
    pub verified: Vec<String>,
    /// Claims the evidence does not support. Always names causal trading
    /// improvement (claim 3).
    pub not_established: Vec<String>,
}

impl fmt::Display for PlaceboControlledReport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let activation = match &self.retrieval_activation {
            ActivationStatus::Activated {
                offered_decisions,
                exposed_decisions,
            } => format!("established ({exposed_decisions} of {offered_decisions} receipted decisions exposed)"),
            ActivationStatus::NotActivated { offered_decisions } => {
                format!("not activated (0 of {offered_decisions} receipted decisions exposed)")
            }
            ActivationStatus::Unavailable { reason } => {
                format!("evidence unavailable ({reason})")
            }
        };
        writeln!(f, "activation: {activation}")?;
        writeln!(
            f,
            "lift over placebo: {:.6} (p = {:.4}, {})",
            self.placebo_controlled_lift,
            self.lift_pvalue,
            self.lift_evidence.label()
        )?;
        if self.lift_evidence != LiftEvidence::PlaceboControlled {
            writeln!(
                f,
                "lift without activation evidence and a matched placebo is a proxy"
            )?;
        }
        writeln!(f, "causal trading improvement: not established")?;
        for line in &self.not_established {
            writeln!(f, "not established: {line}")?;
        }
        Ok(())
    }
}

/// Typed refusals. Several causes refuse a comparison, so each is its own variant.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ActivationError {
    /// A receipt with no decision id.
    EmptyDecisionId,
    /// A digest that is not 64 lowercase hex characters.
    MalformedDigest {
        /// The receipt's decision.
        decision_id: String,
        /// Which field.
        field: &'static str,
    },
    /// Content offered to a decision became available after the decision time.
    PointInTimeViolation {
        /// The decision.
        decision_id: String,
        /// When the content became available.
        available_at: i64,
        /// When the decision was taken.
        decided_at: i64,
    },
    /// Two receipts in one arm cover the same decision.
    DuplicateDecision {
        /// The arm.
        kind: TreatmentKind,
        /// The repeated decision.
        decision_id: String,
    },
    /// An identity with an empty model id.
    EmptyModelId,
    /// An identity with no tasks.
    EmptyTaskSet,
    /// An empty task id.
    EmptyTaskId {
        /// Position in the task list.
        index: usize,
    },
    /// A repeated task id.
    DuplicateTaskId {
        /// The repeated id.
        task_id: String,
    },
    /// An arm passed in the other arm's slot.
    MistaggedArm {
        /// The slot.
        expected: TreatmentKind,
        /// The arm's tag.
        found: TreatmentKind,
    },
    /// The arms ran different models.
    ModelMismatch {
        /// Retrieval model.
        retrieval: String,
        /// Placebo model.
        placebo: String,
    },
    /// The arms ran different task lists (identity or order).
    TaskMismatch {
        /// First position at which the lists differ (the shorter length when one
        /// is a prefix of the other).
        first_difference: usize,
    },
    /// The arms ran under different declared budgets.
    BudgetMismatch {
        /// Retrieval budget.
        retrieval: DecisionBudget,
        /// Placebo budget.
        placebo: DecisionBudget,
    },
    /// Score count differs from the identity's task count.
    ScoreCountMismatch {
        /// The arm.
        kind: TreatmentKind,
        /// Declared tasks.
        tasks: usize,
        /// Scores supplied.
        scores: usize,
    },
    /// A nonfinite outcome score.
    NonFiniteScore {
        /// The arm.
        kind: TreatmentKind,
        /// Task position.
        task_index: usize,
    },
    /// `alpha` not finite or not in `(0, 1)`.
    InvalidAlpha,
    /// Retrieval content was exposed at a decision where the placebo was not.
    PlaceboNotExposedAtBoundary {
        /// The decision.
        decision_id: String,
    },
    /// Placebo content was exposed at a decision where retrieval content was not.
    PlaceboExposedOutsideRetrievalBoundary {
        /// The decision.
        decision_id: String,
    },
    /// The placebo receipt for a decision records a different decision time.
    PlaceboDecisionTimeMismatch {
        /// The decision.
        decision_id: String,
    },
    /// The placebo bytes are not the same length as the retrieval bytes.
    PlaceboLengthMismatch {
        /// The decision.
        decision_id: String,
        /// Retrieval byte length.
        retrieval_len: u64,
        /// Placebo byte length.
        placebo_len: u64,
    },
    /// The placebo bytes are the retrieval bytes.
    PlaceboContentIdentical {
        /// The decision.
        decision_id: String,
    },
    /// The shared significance routine refused its input.
    Statistics(String),
}

impl fmt::Display for ActivationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyDecisionId => write!(f, "activation receipt: empty decision id"),
            Self::MalformedDigest { decision_id, field } => write!(
                f,
                "activation receipt {decision_id}: {field} is not a lowercase hex SHA-256"
            ),
            Self::PointInTimeViolation {
                decision_id,
                available_at,
                decided_at,
            } => write!(
                f,
                "activation receipt {decision_id}: content available at {available_at} is after the decision at {decided_at} (point-in-time violation)"
            ),
            Self::DuplicateDecision { kind, decision_id } => write!(
                f,
                "{} arm: decision {decision_id} has more than one receipt",
                kind.as_str()
            ),
            Self::EmptyModelId => write!(f, "treatment identity: empty model id"),
            Self::EmptyTaskSet => write!(f, "treatment identity: no tasks"),
            Self::EmptyTaskId { index } => write!(f, "treatment identity: task {index} is empty"),
            Self::DuplicateTaskId { task_id } => {
                write!(f, "treatment identity: task {task_id} is declared twice")
            }
            Self::MistaggedArm { expected, found } => write!(
                f,
                "{} slot received a {} arm",
                expected.as_str(),
                found.as_str()
            ),
            Self::ModelMismatch { retrieval, placebo } => write!(
                f,
                "placebo comparison refused: model differs (retrieval {retrieval}, placebo {placebo})"
            ),
            Self::TaskMismatch { first_difference } => write!(
                f,
                "placebo comparison refused: task identities differ from position {first_difference}"
            ),
            Self::BudgetMismatch { retrieval, placebo } => write!(
                f,
                "placebo comparison refused: budget differs (retrieval {retrieval:?}, placebo {placebo:?})"
            ),
            Self::ScoreCountMismatch {
                kind,
                tasks,
                scores,
            } => write!(
                f,
                "{} arm: {scores} scores for {tasks} declared tasks",
                kind.as_str()
            ),
            Self::NonFiniteScore { kind, task_index } => write!(
                f,
                "{} arm task {task_index}: outcome scores must be finite",
                kind.as_str()
            ),
            Self::InvalidAlpha => write!(f, "alpha must be finite and in (0, 1)"),
            Self::PlaceboNotExposedAtBoundary { decision_id } => write!(
                f,
                "placebo comparison refused: retrieval content was exposed at {decision_id} and placebo content was not"
            ),
            Self::PlaceboExposedOutsideRetrievalBoundary { decision_id } => write!(
                f,
                "placebo comparison refused: placebo content was exposed at {decision_id} and retrieval content was not"
            ),
            Self::PlaceboDecisionTimeMismatch { decision_id } => write!(
                f,
                "placebo comparison refused: decision {decision_id} has different decision times in the two arms"
            ),
            Self::PlaceboLengthMismatch {
                decision_id,
                retrieval_len,
                placebo_len,
            } => write!(
                f,
                "placebo comparison refused: at {decision_id} retrieval exposed {retrieval_len} bytes and placebo {placebo_len}"
            ),
            Self::PlaceboContentIdentical { decision_id } => write!(
                f,
                "placebo comparison refused: at {decision_id} the placebo bytes are the retrieval bytes"
            ),
            Self::Statistics(message) => write!(f, "significance: {message}"),
        }
    }
}

impl std::error::Error for ActivationError {}

fn validate_arm(arm: &TreatmentArm, expected: TreatmentKind) -> Result<(), ActivationError> {
    if arm.kind != expected {
        return Err(ActivationError::MistaggedArm {
            expected,
            found: arm.kind,
        });
    }
    let tasks = arm.identity.task_ids().len();
    if arm.scores.len() != tasks {
        return Err(ActivationError::ScoreCountMismatch {
            kind: arm.kind,
            tasks,
            scores: arm.scores.len(),
        });
    }
    if let Some(task_index) = arm.scores.iter().position(|s| !s.is_finite()) {
        return Err(ActivationError::NonFiniteScore {
            kind: arm.kind,
            task_index,
        });
    }
    Ok(())
}

fn compare_identities(
    retrieval: &TreatmentIdentity,
    placebo: &TreatmentIdentity,
) -> Result<(), ActivationError> {
    if retrieval.model_id != placebo.model_id {
        return Err(ActivationError::ModelMismatch {
            retrieval: retrieval.model_id.clone(),
            placebo: placebo.model_id.clone(),
        });
    }
    if retrieval.task_ids != placebo.task_ids {
        let first_difference = retrieval
            .task_ids
            .iter()
            .zip(placebo.task_ids.iter())
            .position(|(r, p)| r != p)
            .unwrap_or_else(|| retrieval.task_ids.len().min(placebo.task_ids.len()));
        return Err(ActivationError::TaskMismatch { first_difference });
    }
    if retrieval.budget != placebo.budget {
        return Err(ActivationError::BudgetMismatch {
            retrieval: retrieval.budget,
            placebo: placebo.budget,
        });
    }
    Ok(())
}

/// Check placebo receipts against retrieval receipts decision by decision.
/// Returns the number of matched exposed decisions.
fn match_placebo(
    retrieval: &[ActivationReceipt],
    placebo: &[ActivationReceipt],
) -> Result<usize, ActivationError> {
    let placebo_by_decision: BTreeMap<&str, &ActivationReceipt> =
        placebo.iter().map(|r| (r.decision_id(), r)).collect();
    let mut retrieval_exposed = BTreeSet::new();
    for r in retrieval.iter().filter(|r| r.exposed()) {
        retrieval_exposed.insert(r.decision_id());
        let p = match placebo_by_decision.get(r.decision_id()) {
            Some(p) if p.exposed() => p,
            _ => {
                return Err(ActivationError::PlaceboNotExposedAtBoundary {
                    decision_id: r.decision_id().to_string(),
                })
            }
        };
        if p.decided_at() != r.decided_at() {
            return Err(ActivationError::PlaceboDecisionTimeMismatch {
                decision_id: r.decision_id().to_string(),
            });
        }
        if p.content_len() != r.content_len() {
            return Err(ActivationError::PlaceboLengthMismatch {
                decision_id: r.decision_id().to_string(),
                retrieval_len: r.content_len(),
                placebo_len: p.content_len(),
            });
        }
        if p.content_sha256() == r.content_sha256() {
            return Err(ActivationError::PlaceboContentIdentical {
                decision_id: r.decision_id().to_string(),
            });
        }
    }
    if let Some(extra) = placebo
        .iter()
        .find(|p| p.exposed() && !retrieval_exposed.contains(p.decision_id()))
    {
        return Err(ActivationError::PlaceboExposedOutsideRetrievalBoundary {
            decision_id: extra.decision_id().to_string(),
        });
    }
    Ok(retrieval_exposed.len())
}

/// Compare a retrieval arm with a placebo arm under identical model, task and
/// budget identities, and report activation, placebo-controlled lift and what is
/// not established as three separate claims.
///
/// Scores pair positionally with the shared `task_ids`. The lift p-value is the
/// crate's paired stationary bootstrap with fixed parameters.
///
/// # Errors
///
/// A typed [`ActivationError`] when an arm is mistagged; when model, task list or
/// budget differ (checked in that order, each its own variant); when a score count
/// differs from the task count or a score is nonfinite; when `alpha` is not finite
/// and inside `(0, 1)`; when an arm repeats a decision id; and, when both arms
/// carry receipts, when the placebo was not exposed at exactly the retrieval
/// arm's exposed decisions with the same decision time, the same byte length and
/// different bytes.
pub fn placebo_controlled_report(
    retrieval: &TreatmentArm,
    placebo: &TreatmentArm,
    alpha: f64,
) -> Result<PlaceboControlledReport, ActivationError> {
    validate_arm(retrieval, TreatmentKind::Retrieval)?;
    validate_arm(placebo, TreatmentKind::Placebo)?;
    compare_identities(&retrieval.identity, &placebo.identity)?;
    validate_alpha(alpha).map_err(|_| ActivationError::InvalidAlpha)?;

    let retrieval_activation = activation_status(retrieval.kind, &retrieval.evidence)?;
    let placebo_activation = activation_status(placebo.kind, &placebo.evidence)?;
    let activation_established = matches!(retrieval_activation, ActivationStatus::Activated { .. });

    let placebo_match = match (&retrieval.evidence, &placebo.evidence) {
        (ActivationEvidence::Receipts(r), ActivationEvidence::Receipts(p)) => {
            let decisions = match_placebo(r, p)?;
            if activation_established {
                PlaceboMatch::Matched { decisions }
            } else {
                PlaceboMatch::NotAssessed {
                    reason:
                        "the retrieval arm exposed no content, so there is no boundary to match"
                            .to_string(),
                }
            }
        }
        (ActivationEvidence::Unavailable { .. }, _) => PlaceboMatch::NotAssessed {
            reason: "the retrieval arm has no receipts to match against".to_string(),
        },
        (_, ActivationEvidence::Unavailable { reason }) => PlaceboMatch::NotAssessed {
            reason: format!("the placebo arm has no receipts ({reason})"),
        },
    };

    let lift_evidence = match (&retrieval_activation, &placebo_match) {
        (ActivationStatus::Unavailable { .. }, _) => LiftEvidence::ProxyActivationUnavailable,
        (ActivationStatus::NotActivated { .. }, _) => LiftEvidence::ProxyRetrievalNotActivated,
        (ActivationStatus::Activated { .. }, PlaceboMatch::Matched { .. }) => {
            LiftEvidence::PlaceboControlled
        }
        (ActivationStatus::Activated { .. }, PlaceboMatch::NotAssessed { .. }) => {
            LiftEvidence::ProxyPlaceboUnverified
        }
    };

    let retrieval_mean = mean(&retrieval.scores);
    let placebo_mean = mean(&placebo.scores);
    let placebo_controlled_lift = retrieval_mean - placebo_mean;
    let paired: Vec<f64> = retrieval
        .scores
        .iter()
        .zip(placebo.scores.iter())
        .map(|(r, p)| r - p)
        .collect();
    let lift_pvalue = bootstrap_pvalue(
        &paired,
        BOOTSTRAP_SEED,
        BOOTSTRAP_SAMPLES,
        BOOTSTRAP_BLOCK_PROB,
    )
    .map_err(|error| ActivationError::Statistics(error.to_string()))?;

    let mut verified = vec![format!(
        "both arms ran under model {}, the same {} task identities in the same order, and the same declared budget",
        retrieval.identity.model_id(),
        retrieval.identity.task_ids().len()
    )];
    let mut not_established = vec![
        "causal trading improvement: activation and a placebo-controlled lift show that retrieved content reached decisions and outscored length-matched placebo content on these tasks; they do not show that the content caused better trading outcomes elsewhere".to_string(),
        "that the exposed bytes were used: a receipt shows presence in the decision input, not attention or reliance".to_string(),
        "that the host framed and hashed the decision input honestly: receipts are checked for internal consistency and point-in-time order, not attested".to_string(),
        "that the placebo content carries no task information: byte length and distinct digest are checked, informational emptiness is the caller's contract".to_string(),
    ];
    match &retrieval_activation {
        ActivationStatus::Activated {
            offered_decisions,
            exposed_decisions,
        } => verified.push(format!(
            "activation: {exposed_decisions} of {offered_decisions} receipted retrieval decisions carried the exact offered bytes as a whole input segment, each available at or before its decision time"
        )),
        ActivationStatus::NotActivated { offered_decisions } => not_established.push(format!(
            "activation: memory was offered at {offered_decisions} retrieval decisions and exposed at none; memory that never reaches a decision boundary is not an activated treatment, so the lift is a proxy"
        )),
        ActivationStatus::Unavailable { reason } => not_established.push(format!(
            "activation: the retrieval arm produced no receipts ({reason}); lift without activation evidence is a proxy"
        )),
    }
    match &placebo_match {
        PlaceboMatch::Matched { decisions } => verified.push(format!(
            "placebo: at all {decisions} exposed retrieval decisions, distinct placebo bytes of the same length were exposed at the same decision time, and at no other decision"
        )),
        PlaceboMatch::NotAssessed { reason } => not_established.push(format!(
            "that the placebo was exposed at the same boundaries with matched byte length: {reason}"
        )),
    }

    Ok(PlaceboControlledReport {
        retrieval_activation,
        placebo_activation,
        activation_established,
        placebo_match,
        retrieval_mean,
        placebo_mean,
        placebo_controlled_lift,
        lift_pvalue,
        significant: lift_pvalue < alpha,
        alpha,
        lift_evidence,
        verified,
        not_established,
    })
}
