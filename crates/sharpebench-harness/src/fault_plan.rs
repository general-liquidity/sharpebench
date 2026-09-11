//! Frozen fault plans for seeded fault injection at the entrant boundary.
//!
//! Hyper-Tau port reconciliation rows 32 and 33
//! (`docs/audits/2026-09-09/PORT-RECONCILIATION.md`), recorded in
//! `docs/audits/2026-09-09/FAULT-INJECTION.md`.
//!
//! A [`FaultPlan`] is the immutable, content-addressed input to a faulted
//! sweep. It is validated once, has no mutating methods and private fields, and
//! its [`FaultPlan::digest`] is folded into the checkpoint invocation identity
//! by [`bind_invocation`], so a resume under a different plan is refused by the
//! existing contract comparison. Everything that varies while a cell runs lives
//! in [`modes::TrialFaultState`], which is built from the plan and never writes
//! back to it.
//!
//! Which cells receive which fault is a cohort draw over the plan digest
//! ([`cohort_draw`]): a SHA-256 of the digest, the draw scope, the plan seed and
//! the cell, with no separate random stream to record. Faults sharing a group
//! occupy disjoint sub-ranges of one shared draw, so they are mutually exclusive
//! by construction. Cohorting means not every cell sees every fault, so any
//! reported per-fault result carries its own denominator ([`FaultDenominator`]).
//!
//! Nothing here reads a clock or ambient randomness. With no plan configured the
//! harness path is the unchanged one: [`modes::run_faulted_backtest_observed`]
//! delegates to [`crate::run_external_backtest_observed`] and records no fault
//! evidence, so every existing artifact stays byte-identical.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use sharpebench_sim::Window;

use crate::failure::{AttemptDuration, AttemptLedger, AttemptObservation};

#[path = "fault_modes.rs"]
pub mod modes;

/// Schema string for version 1 of the plan encoding. The digest is over the
/// validated record, so this string versions the digest too.
pub const FAULT_PLAN_VERSION: &str = "sharpebench.fault-plan.v1";

/// Upper bound on a plan read from bytes, matching the rate-card bound.
pub const MAX_FAULT_PLAN_BYTES: usize = 64 * 1024;

/// Cohort rates are integer parts per million, so the digest never depends on
/// how a float was printed and group sums are exact.
pub const COHORT_SCALE: u32 = 1_000_000;

/// Upper bound on faults in one plan.
pub const MAX_FAULTS: usize = 64;

/// Upper bound on every per-mode step or presentation parameter. A rate limit
/// re-presents one observation at most this many times, so a plan cannot turn
/// one decision step into an unbounded number of entrant calls.
pub const MAX_MODE_PARAMETER: u32 = 64;

const MAX_NAME_BYTES: usize = 64;

/// The consistency or presentation guarantee a fault mode relaxes. A plan must
/// declare exactly the relaxations its armed faults use: row 27 requires the
/// relaxed consistency to be declared before a stale read is injected, or the
/// fault is a trap rather than a test.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContractRelaxation {
    /// A read after a write may not reflect the write for a bounded number of
    /// decision steps. The write itself is authoritative and executes at once.
    ReadYourWrites,
    /// The sign of a reported position quantity may not follow the documented
    /// convention (negative is short) for a bounded number of decision steps.
    PositionSignConvention,
    /// An order-bearing decision may be rejected under a rate limit. Rejection
    /// is signalled by re-presenting the identical observation.
    SubmissionAcceptance,
    /// A paged read may return a window taken before its ordering. Declared for
    /// completeness of the row 29 record; no plan can arm it today.
    CompleteResults,
}

impl ContractRelaxation {
    /// The declaration text an operator publishes to entrants with the plan.
    pub fn declaration(self) -> &'static str {
        match self {
            ContractRelaxation::ReadYourWrites => {
                "read-your-writes is relaxed: after an order executes, `cash` and \
                 `portfolio` may show the pre-execution state for a bounded number \
                 of decision steps; the order is executed and the book is \
                 authoritative"
            }
            ContractRelaxation::PositionSignConvention => {
                "the position sign convention is relaxed: a reported nonzero \
                 `portfolio[].shares` may carry the opposite sign for a bounded \
                 number of decision steps; the book is unchanged"
            }
            ContractRelaxation::SubmissionAcceptance => {
                "submission acceptance is relaxed: an order-bearing decision may be \
                 rejected under a rate limit, signalled by re-presenting the \
                 identical observation until a bounded deadline; only the decision \
                 answering the first presentation after the deadline executes"
            }
            ContractRelaxation::CompleteResults => {
                "complete results are relaxed: a paged read may truncate before \
                 ordering"
            }
        }
    }
}

/// One fault mode with its bounds. Every draw within a bound comes from the
/// plan digest; the plan fixes only the bound.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "mode", rename_all = "snake_case", deny_unknown_fields)]
pub enum FaultMode {
    /// Row 27: after a write, the projection of the book (`cash`, `portfolio`)
    /// stays at its pre-write state for a seeded `1..=max_lag_steps` steps.
    ProjectionLag { max_lag_steps: u32 },
    /// Row 30: nonzero reported position quantities carry the opposite sign
    /// for a seeded `1..=max_span_steps` steps.
    AmountSign { max_span_steps: u32 },
    /// Row 31: an order-bearing decision is rejected, and the observation
    /// re-presented, until a seeded deadline of `1..=max_rejected_presentations`
    /// rejected presentations has passed.
    RateLimit { max_rejected_presentations: u32 },
    /// Row 29: `limit_before_sort` pagination. Recorded, never armable: the row
    /// admits it only once a paged read exists on the Bench data surface.
    LimitBeforeSort { page_size: u32 },
}

impl FaultMode {
    /// The contract relaxation this mode requires.
    pub fn relaxation(&self) -> ContractRelaxation {
        match self {
            FaultMode::ProjectionLag { .. } => ContractRelaxation::ReadYourWrites,
            FaultMode::AmountSign { .. } => ContractRelaxation::PositionSignConvention,
            FaultMode::RateLimit { .. } => ContractRelaxation::SubmissionAcceptance,
            FaultMode::LimitBeforeSort { .. } => ContractRelaxation::CompleteResults,
        }
    }

    fn name(&self) -> &'static str {
        match self {
            FaultMode::ProjectionLag { .. } => "projection_lag",
            FaultMode::AmountSign { .. } => "amount_sign",
            FaultMode::RateLimit { .. } => "rate_limit",
            FaultMode::LimitBeforeSort { .. } => "limit_before_sort",
        }
    }
}

/// One fault in the plan: a stable id, an optional mutual-exclusion group, the
/// cohort rate and the mode.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FaultSpec {
    pub id: String,
    /// Faults sharing a group are mutually exclusive within a cell: they occupy
    /// disjoint sub-ranges of one shared draw.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub group: Option<String>,
    /// Share of cells assigned this fault, in parts per [`COHORT_SCALE`].
    pub cohort_ppm: u32,
    pub fault: FaultMode,
}

/// Why a plan was refused.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FaultPlanError {
    /// The plan is malformed or out of bounds.
    Invalid(String),
    /// The mode is recorded in the reconciliation but its admission condition
    /// does not hold in this tree.
    ConditionUnmet {
        mode: &'static str,
        reason: &'static str,
    },
}

impl fmt::Display for FaultPlanError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FaultPlanError::Invalid(message) => write!(f, "invalid fault plan: {message}"),
            FaultPlanError::ConditionUnmet { mode, reason } => {
                write!(f, "fault mode {mode} cannot be armed: {reason}")
            }
        }
    }
}

impl std::error::Error for FaultPlanError {}

/// The reason row 29 is not armable, quoted in the refusal.
pub const LIMIT_BEFORE_SORT_CONDITION: &str =
    "PORT-RECONCILIATION row 29 admits limit_before_sort only once the Bench data \
     surface has a paged read, and the observation contract has none; pagination is \
     not added for the fault's sake";

/// The frozen, content-addressed fault plan (row 32). Private validated fields
/// and no mutating methods keep it identical for the whole sweep.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "FaultPlanWire")]
pub struct FaultPlan {
    schema_version: String,
    seed: u64,
    declared_relaxations: Vec<ContractRelaxation>,
    faults: Vec<FaultSpec>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct FaultPlanWire {
    schema_version: String,
    seed: u64,
    declared_relaxations: Vec<ContractRelaxation>,
    faults: Vec<FaultSpec>,
}

impl TryFrom<FaultPlanWire> for FaultPlan {
    type Error = FaultPlanError;

    fn try_from(wire: FaultPlanWire) -> Result<Self, Self::Error> {
        if wire.schema_version != FAULT_PLAN_VERSION {
            return Err(FaultPlanError::Invalid(format!(
                "schema_version must be {FAULT_PLAN_VERSION}"
            )));
        }
        FaultPlan::new(wire.seed, wire.declared_relaxations, wire.faults)
    }
}

fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= MAX_NAME_BYTES
        && name
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_' || b == b'-')
}

impl FaultPlan {
    /// Validate and freeze a plan. The declared relaxations are stored sorted
    /// and deduplicated, so the digest does not depend on how they were listed.
    pub fn new(
        seed: u64,
        declared_relaxations: Vec<ContractRelaxation>,
        faults: Vec<FaultSpec>,
    ) -> Result<Self, FaultPlanError> {
        let invalid = |message: String| Err(FaultPlanError::Invalid(message));
        if faults.is_empty() || faults.len() > MAX_FAULTS {
            return invalid(format!("a plan carries 1..={MAX_FAULTS} faults"));
        }
        let mut ids = BTreeSet::new();
        let mut group_sums: BTreeMap<&str, u64> = BTreeMap::new();
        for spec in &faults {
            if !valid_name(&spec.id) {
                return invalid(format!(
                    "fault id {:?} must be 1..={MAX_NAME_BYTES} bytes of [a-z0-9_-]",
                    spec.id
                ));
            }
            if !ids.insert(spec.id.as_str()) {
                return invalid(format!("duplicate fault id {:?}", spec.id));
            }
            if let Some(group) = &spec.group {
                if !valid_name(group) {
                    return invalid(format!(
                        "group {group:?} must be 1..={MAX_NAME_BYTES} bytes of [a-z0-9_-]"
                    ));
                }
            }
            if spec.cohort_ppm == 0 || spec.cohort_ppm > COHORT_SCALE {
                return invalid(format!(
                    "fault {:?} cohort_ppm must be in 1..={COHORT_SCALE}",
                    spec.id
                ));
            }
            let bound = match spec.fault {
                FaultMode::ProjectionLag { max_lag_steps } => max_lag_steps,
                FaultMode::AmountSign { max_span_steps } => max_span_steps,
                FaultMode::RateLimit {
                    max_rejected_presentations,
                } => max_rejected_presentations,
                FaultMode::LimitBeforeSort { .. } => {
                    return Err(FaultPlanError::ConditionUnmet {
                        mode: spec.fault.name(),
                        reason: LIMIT_BEFORE_SORT_CONDITION,
                    })
                }
            };
            if bound == 0 || bound > MAX_MODE_PARAMETER {
                return invalid(format!(
                    "fault {:?} bound must be in 1..={MAX_MODE_PARAMETER}",
                    spec.id
                ));
            }
            if let Some(group) = &spec.group {
                let sum = group_sums.entry(group.as_str()).or_default();
                *sum += u64::from(spec.cohort_ppm);
                if *sum > u64::from(COHORT_SCALE) {
                    return invalid(format!(
                        "group {group:?} cohort rates sum past {COHORT_SCALE} ppm; mutually \
                         exclusive faults must share one draw of at most one"
                    ));
                }
            }
        }
        let armed: BTreeSet<ContractRelaxation> =
            faults.iter().map(|spec| spec.fault.relaxation()).collect();
        let declared: BTreeSet<ContractRelaxation> = declared_relaxations.into_iter().collect();
        if armed != declared {
            return invalid(format!(
                "declared_relaxations must be exactly the relaxations the armed faults \
                 use: declared {declared:?}, armed {armed:?}"
            ));
        }
        Ok(Self {
            schema_version: FAULT_PLAN_VERSION.to_string(),
            seed,
            declared_relaxations: declared.into_iter().collect(),
            faults,
        })
    }

    /// Parse and validate a plan from JSON bytes.
    pub fn from_json(bytes: &[u8]) -> Result<Self, String> {
        if bytes.len() > MAX_FAULT_PLAN_BYTES {
            return Err("fault plan exceeds the 64 KiB input limit".into());
        }
        serde_json::from_slice(bytes).map_err(|error| format!("invalid fault plan: {error}"))
    }

    /// SHA-256 of the validated, fixed-order record, independent of input JSON
    /// formatting.
    pub fn digest(&self) -> String {
        sharpebench_attest::content_digest(
            &serde_json::to_vec(self).expect("a validated fault plan serializes"),
        )
    }

    pub fn seed(&self) -> u64 {
        self.seed
    }

    pub fn faults(&self) -> &[FaultSpec] {
        &self.faults
    }

    pub fn declared_relaxations(&self) -> &[ContractRelaxation] {
        &self.declared_relaxations
    }

    /// The declaration an operator publishes to entrants before the sweep.
    pub fn entrant_declaration(&self) -> String {
        self.declared_relaxations
            .iter()
            .map(|relaxation| format!("- {}", relaxation.declaration()))
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// The faults assigned to `cell`, in plan order.
    pub fn assigned(&self, cell: CellId) -> Vec<&FaultSpec> {
        assign(&self.digest(), self.seed, &self.faults, cell)
    }

    /// Per-fault denominators over `cells`: how many cells the sweep ran and
    /// how many of them each fault was assigned to.
    pub fn denominators(&self, cells: &[CellId]) -> Vec<FaultDenominator> {
        let digest = self.digest();
        let mut assigned: BTreeMap<&str, u64> = BTreeMap::new();
        for &cell in cells {
            for spec in assign(&digest, self.seed, &self.faults, cell) {
                *assigned.entry(spec.id.as_str()).or_default() += 1;
            }
        }
        self.faults
            .iter()
            .map(|spec| FaultDenominator {
                fault_id: spec.id.clone(),
                cells: cells.len() as u64,
                assigned: assigned.get(spec.id.as_str()).copied().unwrap_or(0),
                fired: 0,
            })
            .collect()
    }

    /// [`FaultPlan::denominators`] with `fired` filled from recorded evidence:
    /// the distinct cells whose attempts under this plan show at least one
    /// event for the fault. Evidence from another plan is ignored.
    pub fn denominators_with_evidence(
        &self,
        cells: &[CellId],
        ledger: &AttemptLedger,
    ) -> Vec<FaultDenominator> {
        let digest = self.digest();
        let mut fired: BTreeMap<String, BTreeSet<CellId>> = BTreeMap::new();
        for evidence in ledger
            .attempts
            .iter()
            .filter_map(|record| record.injected_faults.as_ref())
            .filter(|evidence| evidence.plan_sha256 == digest)
        {
            for event in &evidence.events {
                fired
                    .entry(event.fault_id().to_string())
                    .or_default()
                    .insert(evidence.cell);
            }
        }
        let mut out = self.denominators(cells);
        for row in &mut out {
            row.fired = fired.get(&row.fault_id).map_or(0, |set| set.len() as u64);
        }
        out
    }
}

/// Fold a fault plan into the checkpoint invocation digest. Without a plan the
/// digest is returned unchanged, so an unfaulted sweep keeps its identity; with
/// one, any change to the plan changes the bound digest, and the existing
/// contract comparison refuses to resume the checkpoint under it.
pub fn bind_invocation(invocation_sha256: &str, plan: Option<&FaultPlan>) -> String {
    match plan {
        None => invocation_sha256.to_string(),
        Some(plan) => {
            let mut bytes = Vec::new();
            push_field(&mut bytes, b"sharpebench.fault-plan.invocation.v1");
            push_field(&mut bytes, invocation_sha256.as_bytes());
            push_field(&mut bytes, plan.digest().as_bytes());
            sharpebench_attest::content_digest(&bytes)
        }
    }
}

/// The coordinates of one sweep cell. Window bounds rather than an index, so a
/// draw names the same cell whatever order the windows were listed in.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct CellId {
    pub window_start: usize,
    pub window_end: usize,
    pub seed: u64,
}

impl CellId {
    pub fn new(window: Window, seed: u64) -> Self {
        Self {
            window_start: window.start,
            window_end: window.end,
            seed,
        }
    }

    /// Decision steps in the cell's window.
    pub fn steps(&self) -> u32 {
        u32::try_from(self.window_end.saturating_sub(self.window_start)).unwrap_or(u32::MAX)
    }
}

/// What a cohort draw is taken over: a mutual-exclusion group, or one
/// independent fault.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DrawScope<'a> {
    Group(&'a str),
    Independent(&'a str),
}

fn push_field(bytes: &mut Vec<u8>, field: &[u8]) {
    bytes.extend_from_slice(&(field.len() as u64).to_le_bytes());
    bytes.extend_from_slice(field);
}

/// A 64-bit draw from SHA-256 over length-prefixed fields, so no two distinct
/// inputs share an encoding.
fn draw_u64(plan_sha256: &str, label: &str, key: &str, plan_seed: u64, cell: CellId) -> u64 {
    let mut bytes = Vec::new();
    push_field(&mut bytes, b"sharpebench.fault-draw.v1");
    push_field(&mut bytes, label.as_bytes());
    push_field(&mut bytes, key.as_bytes());
    push_field(&mut bytes, plan_sha256.as_bytes());
    bytes.extend_from_slice(&plan_seed.to_le_bytes());
    bytes.extend_from_slice(&(cell.window_start as u64).to_le_bytes());
    bytes.extend_from_slice(&(cell.window_end as u64).to_le_bytes());
    bytes.extend_from_slice(&cell.seed.to_le_bytes());
    let digest = Sha256::digest(&bytes);
    let mut first = [0u8; 8];
    first.copy_from_slice(&digest[..8]);
    u64::from_be_bytes(first)
}

/// Map a uniform 64-bit draw onto `0..bound` by its high bits.
fn scale(draw: u64, bound: u32) -> u32 {
    ((u128::from(draw) * u128::from(bound)) >> 64) as u32
}

/// The cohort draw (row 33): a value in `0..COHORT_SCALE`, a pure function of
/// the plan digest, the scope, the plan seed and the cell.
pub fn cohort_draw(plan_sha256: &str, scope: DrawScope<'_>, plan_seed: u64, cell: CellId) -> u32 {
    let (label, key) = match scope {
        DrawScope::Group(group) => ("group", group),
        DrawScope::Independent(id) => ("independent", id),
    };
    scale(
        draw_u64(plan_sha256, label, key, plan_seed, cell),
        COHORT_SCALE,
    )
}

/// A seeded parameter in `0..bound` for one fault in one cell, drawn from the
/// plan digest like the cohort itself. `bound` must be nonzero.
pub(crate) fn parameter_draw(
    plan_sha256: &str,
    fault_id: &str,
    parameter: &str,
    plan_seed: u64,
    cell: CellId,
    bound: u32,
) -> u32 {
    scale(
        draw_u64(
            plan_sha256,
            &format!("parameter:{parameter}"),
            fault_id,
            plan_seed,
            cell,
        ),
        bound,
    )
}

/// Assign faults to one cell. Grouped faults partition one shared draw into
/// consecutive sub-ranges in plan order; an ungrouped fault has its own draw.
pub fn assign<'p>(
    plan_sha256: &str,
    plan_seed: u64,
    faults: &'p [FaultSpec],
    cell: CellId,
) -> Vec<&'p FaultSpec> {
    let mut group_draws: BTreeMap<&str, (u32, u32)> = BTreeMap::new();
    let mut out = Vec::new();
    for spec in faults {
        let hit = match &spec.group {
            None => {
                cohort_draw(
                    plan_sha256,
                    DrawScope::Independent(&spec.id),
                    plan_seed,
                    cell,
                ) < spec.cohort_ppm
            }
            Some(group) => {
                let (draw, lower) = group_draws.entry(group.as_str()).or_insert_with(|| {
                    (
                        cohort_draw(plan_sha256, DrawScope::Group(group), plan_seed, cell),
                        0,
                    )
                });
                let upper = *lower + spec.cohort_ppm;
                let hit = *lower <= *draw && *draw < upper;
                *lower = upper;
                hit
            }
        };
        if hit {
            out.push(spec);
        }
    }
    out
}

/// A per-fault denominator for reporting (row 33): results for a fault are
/// over `assigned` cells of `cells`, of which `fired` actually triggered it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FaultDenominator {
    pub fault_id: String,
    pub cells: u64,
    pub assigned: u64,
    pub fired: u64,
}

/// Evidence of the faults injected into one attempt, recorded on its
/// [`crate::AttemptRecord`] so a ledger reader can tell an injected fault from
/// a genuine one. Rank-neutral: never an input to a score, rank or pass^k pool.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InjectedFaults {
    pub plan_sha256: String,
    pub cell: CellId,
    /// Fault ids the cohort draw assigned to this cell, in plan order.
    pub assigned: Vec<String>,
    /// What fired, in order, with the process grade of the entrant's response.
    pub events: Vec<FaultEvent>,
    /// Host time spent on presentations whose decision a rate limit rejected,
    /// kept apart from the attempt total so waiting on a limit is not read as
    /// thinking.
    pub rate_limited: AttemptDuration,
}

/// The sign of a quantity as the evidence records it, so the record stays
/// exact and comparable.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Sign {
    Negative,
    Zero,
    Positive,
}

impl Sign {
    pub fn of(value: f64) -> Self {
        if value > 0.0 {
            Sign::Positive
        } else if value < 0.0 {
            Sign::Negative
        } else {
            Sign::Zero
        }
    }
}

/// Row 30 grade: was the entrant's decision on an inverted symbol consistent
/// with its own last stated target, or did it follow the inverted report?
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReadingGrade {
    /// No order on the symbol: the stated target stands.
    NoOrder,
    /// The new target has the sign of the entrant's own last stated target.
    ConsistentWithOwnStatement,
    /// The new target flipped to the inverted sign it was shown.
    FollowedPresentation,
    /// Any other change, such as a close to zero.
    Revised,
    /// The entrant never stated a target for the symbol.
    NoPriorStatement,
}

/// Row 27 grade: what the entrant did on a symbol whose write the stale read
/// hid from it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StaleReadGrade {
    NoOrder,
    /// It restated its own last target: the idempotent response.
    Reaffirmed,
    /// It pushed the target further in the direction of the hidden write: the
    /// resubmission failure the fault exists to detect.
    Escalated,
    /// It moved against the hidden write.
    Revised,
    NoPriorStatement,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SignResponse {
    pub symbol: String,
    /// The sign the entrant was shown (the canonical sign is its opposite).
    pub presented: Sign,
    pub grade: ReadingGrade,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct StaleResponse {
    pub symbol: String,
    pub grade: StaleReadGrade,
}

/// One fired fault. `step` is the 0-based decision step within the window.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum FaultEvent {
    /// A presentation whose `cash` and `portfolio` were the pre-write state.
    ProjectionStale {
        fault_id: String,
        step: u32,
        responses: Vec<StaleResponse>,
    },
    /// The first presentation observed to carry the canonical state again.
    /// Recorded when a read observes the new state, not when the lag timer
    /// runs out.
    ProjectionConverged {
        fault_id: String,
        step: u32,
        stale_presentations: u32,
    },
    /// The attempt ended before any read observed convergence.
    ProjectionUnconverged {
        fault_id: String,
        stale_presentations: u32,
    },
    /// A presentation with inverted position signs.
    SignInverted {
        fault_id: String,
        step: u32,
        responses: Vec<SignResponse>,
    },
    /// An order-bearing decision was rejected until the seeded deadline.
    RateLimited {
        fault_id: String,
        step: u32,
        /// Presentations under the limit, the rejected first submission included.
        rejected_presentations: u32,
        /// Order-bearing submissions after the first one, while still limited.
        resubmissions_under_limit: u32,
        /// Empty decisions while limited: the entrant waited.
        waits_under_limit: u32,
        /// The accepted decision after the deadline carried orders.
        resubmitted_after_deadline: bool,
    },
}

impl FaultEvent {
    pub fn fault_id(&self) -> &str {
        match self {
            FaultEvent::ProjectionStale { fault_id, .. }
            | FaultEvent::ProjectionConverged { fault_id, .. }
            | FaultEvent::ProjectionUnconverged { fault_id, .. }
            | FaultEvent::SignInverted { fault_id, .. }
            | FaultEvent::RateLimited { fault_id, .. } => fault_id,
        }
    }
}

/// One attempt's observation with the fault evidence the attempt produced.
/// `None` evidence is an unfaulted attempt, recorded exactly as before.
pub struct FaultedObservation {
    pub observation: AttemptObservation,
    pub injected_faults: Option<InjectedFaults>,
}

impl From<AttemptObservation> for FaultedObservation {
    fn from(observation: AttemptObservation) -> Self {
        Self {
            observation,
            injected_faults: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec(id: &str, group: Option<&str>, ppm: u32, fault: FaultMode) -> FaultSpec {
        FaultSpec {
            id: id.to_string(),
            group: group.map(str::to_string),
            cohort_ppm: ppm,
            fault,
        }
    }

    fn sample_plan(seed: u64) -> FaultPlan {
        FaultPlan::new(
            seed,
            vec![
                ContractRelaxation::SubmissionAcceptance,
                ContractRelaxation::ReadYourWrites,
                ContractRelaxation::PositionSignConvention,
            ],
            vec![
                spec(
                    "lag",
                    Some("book"),
                    400_000,
                    FaultMode::ProjectionLag { max_lag_steps: 3 },
                ),
                spec(
                    "sign",
                    Some("book"),
                    300_000,
                    FaultMode::AmountSign { max_span_steps: 2 },
                ),
                spec(
                    "limit",
                    None,
                    500_000,
                    FaultMode::RateLimit {
                        max_rejected_presentations: 4,
                    },
                ),
            ],
        )
        .expect("the sample plan is valid")
    }

    fn cells() -> Vec<CellId> {
        (0..40)
            .flat_map(|w| {
                (0..25u64).map(move |seed| CellId {
                    window_start: w * 10,
                    window_end: w * 10 + 30,
                    seed,
                })
            })
            .collect()
    }

    #[test]
    fn digest_ignores_json_formatting_and_relaxation_order() {
        let plan = sample_plan(7);
        let compact = serde_json::to_vec(&plan).unwrap();
        let pretty = serde_json::to_vec_pretty(&plan).unwrap();
        let reparsed = FaultPlan::from_json(&pretty).unwrap();
        assert_eq!(FaultPlan::from_json(&compact).unwrap(), plan);
        assert_eq!(reparsed.digest(), plan.digest());
        let reordered = FaultPlan::new(
            7,
            vec![
                ContractRelaxation::PositionSignConvention,
                ContractRelaxation::ReadYourWrites,
                ContractRelaxation::SubmissionAcceptance,
                ContractRelaxation::ReadYourWrites,
            ],
            plan.faults().to_vec(),
        )
        .unwrap();
        assert_eq!(reordered.digest(), plan.digest());
    }

    /// Row 32: mutating any plan field changes the digest.
    #[test]
    fn every_plan_field_is_bound_into_the_digest() {
        let base = sample_plan(7);
        let digest = base.digest();
        let faults = base.faults().to_vec();
        let relaxations = base.declared_relaxations().to_vec();
        let mut variants = vec![FaultPlan::new(8, relaxations.clone(), faults.clone()).unwrap()];
        let mut edit = |change: &dyn Fn(&mut Vec<FaultSpec>)| {
            let mut changed = faults.clone();
            change(&mut changed);
            variants.push(FaultPlan::new(7, relaxations.clone(), changed).unwrap());
        };
        edit(&|f| f[0].id = "lag2".to_string());
        edit(&|f| f[0].group = Some("other".to_string()));
        edit(&|f| f[2].group = Some("solo".to_string()));
        edit(&|f| f[0].cohort_ppm = 400_001);
        edit(&|f| f[0].fault = FaultMode::ProjectionLag { max_lag_steps: 4 });
        edit(&|f| f[1].fault = FaultMode::AmountSign { max_span_steps: 3 });
        edit(&|f| {
            f[2].fault = FaultMode::RateLimit {
                max_rejected_presentations: 5,
            }
        });
        edit(&|f| f.swap(0, 1));
        let mut seen = BTreeSet::from([digest]);
        for variant in variants {
            assert!(
                seen.insert(variant.digest()),
                "a changed plan must have a new digest: {variant:?}"
            );
        }
    }

    #[test]
    fn plan_validation_refuses_malformed_plans() {
        let lag = |ppm| {
            spec(
                "a",
                None,
                ppm,
                FaultMode::ProjectionLag { max_lag_steps: 2 },
            )
        };
        let ryw = vec![ContractRelaxation::ReadYourWrites];
        // A plan with no faults arms no relaxation, so it declares none either:
        // otherwise the declared-versus-armed rule refuses it and the fault
        // count is never what is tested. The diagnostic is asserted for the
        // same reason.
        assert_eq!(
            FaultPlan::new(1, vec![], vec![]).expect_err("a plan carries at least one fault"),
            FaultPlanError::Invalid(format!("a plan carries 1..={MAX_FAULTS} faults"))
        );
        assert!(FaultPlan::new(1, ryw.clone(), vec![lag(0)]).is_err());
        assert!(FaultPlan::new(1, ryw.clone(), vec![lag(COHORT_SCALE + 1)]).is_err());
        assert!(FaultPlan::new(1, ryw.clone(), vec![lag(1), lag(1)]).is_err());
        assert!(
            FaultPlan::new(1, vec![], vec![lag(1)]).is_err(),
            "undeclared"
        );
        assert!(
            FaultPlan::new(
                1,
                vec![
                    ContractRelaxation::ReadYourWrites,
                    ContractRelaxation::SubmissionAcceptance
                ],
                vec![lag(1)]
            )
            .is_err(),
            "a declared relaxation nothing uses is not the plan's contract"
        );
        let mut bad_name = lag(1);
        bad_name.id = "Upper Case".to_string();
        assert!(FaultPlan::new(1, ryw.clone(), vec![bad_name]).is_err());
        let unbounded = spec(
            "a",
            None,
            1,
            FaultMode::ProjectionLag {
                max_lag_steps: MAX_MODE_PARAMETER + 1,
            },
        );
        assert!(FaultPlan::new(1, ryw.clone(), vec![unbounded]).is_err());
        let over = |id: &str| {
            spec(
                id,
                Some("g"),
                600_000,
                FaultMode::ProjectionLag { max_lag_steps: 1 },
            )
        };
        assert!(
            FaultPlan::new(1, ryw.clone(), vec![over("a"), over("b")]).is_err(),
            "a group's disjoint sub-ranges cannot exceed one draw"
        );
        let unknown = br#"{"schema_version":"sharpebench.fault-plan.v1","seed":1,
            "declared_relaxations":["read_your_writes"],
            "faults":[{"id":"a","cohort_ppm":1,"fault":{"mode":"projection_lag","max_lag_steps":1,"extra":1}}]}"#;
        assert!(FaultPlan::from_json(unknown).is_err());
        let wrong_version = br#"{"schema_version":"v0","seed":1,
            "declared_relaxations":["read_your_writes"],
            "faults":[{"id":"a","cohort_ppm":1,"fault":{"mode":"projection_lag","max_lag_steps":1}}]}"#;
        assert!(FaultPlan::from_json(wrong_version).is_err());
    }

    /// Row 29: the condition does not hold, so the mode is refused with the
    /// reason, never silently armed or ignored.
    #[test]
    fn limit_before_sort_is_refused_while_no_paged_read_exists() {
        let refused = FaultPlan::new(
            1,
            vec![ContractRelaxation::CompleteResults],
            vec![spec(
                "page",
                None,
                COHORT_SCALE,
                FaultMode::LimitBeforeSort { page_size: 10 },
            )],
        );
        assert_eq!(
            refused,
            Err(FaultPlanError::ConditionUnmet {
                mode: "limit_before_sort",
                reason: LIMIT_BEFORE_SORT_CONDITION,
            })
        );
        let wire = br#"{"schema_version":"sharpebench.fault-plan.v1","seed":1,
            "declared_relaxations":["complete_results"],
            "faults":[{"id":"page","cohort_ppm":1000000,"fault":{"mode":"limit_before_sort","page_size":10}}]}"#;
        let error = FaultPlan::from_json(wire).unwrap_err();
        assert!(error.contains("row 29"), "{error}");
    }

    /// The tripwire for row 29's condition: the day the observation contract
    /// grows a paged read, this fails and the fault becomes due.
    #[test]
    fn the_observation_contract_still_has_no_paged_read() {
        let schema = std::fs::read_to_string(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../sharpebench-protocol/schema/observation.schema.json"
        ))
        .expect("the observation schema is checked in");
        let lowered = schema.to_ascii_lowercase();
        for marker in ["cursor", "page", "offset", "next_token", "limit"] {
            assert!(
                !lowered.contains(marker),
                "the observation contract mentions {marker:?}: if a paged read now \
                 exists, row 29 requires limit_before_sort and cursor binding to be \
                 implemented"
            );
        }
    }

    /// Row 27 requires the relaxed consistency to be declared in the contract
    /// before a fault uses it. Every relaxation a plan can declare is named by
    /// its wire name in the protocol crate documentation, and every armable one
    /// also in the published schema text an entrant validates against.
    #[test]
    fn every_declarable_relaxation_is_stated_in_the_published_contract() {
        let read = |path: &str| {
            std::fs::read_to_string(format!(
                "{}/../sharpebench-protocol/{path}",
                env!("CARGO_MANIFEST_DIR")
            ))
            .unwrap_or_else(|error| panic!("{path} is checked in: {error}"))
        };
        let docs = read("src/lib.rs");
        let schemas = read("schema/observation.schema.json") + &read("schema/decision.schema.json");
        let all = [
            ContractRelaxation::ReadYourWrites,
            ContractRelaxation::PositionSignConvention,
            ContractRelaxation::SubmissionAcceptance,
            ContractRelaxation::CompleteResults,
        ];
        for relaxation in all {
            // Exhaustive on purpose: a new relaxation must be listed above.
            let armable = match relaxation {
                ContractRelaxation::ReadYourWrites
                | ContractRelaxation::PositionSignConvention
                | ContractRelaxation::SubmissionAcceptance => true,
                ContractRelaxation::CompleteResults => false,
            };
            let name = serde_json::to_value(relaxation).unwrap();
            let name = name.as_str().unwrap();
            assert!(
                docs.contains(&format!("//! - `{name}`: ")),
                "the protocol docs do not state the `{name}` relaxation"
            );
            if armable {
                assert!(
                    schemas.contains(&format!("declares {name}")),
                    "the published schema does not state the `{name}` relaxation"
                );
            }
        }
    }

    /// Row 33: the draw is a pure function of the digest, scope, seed and
    /// cell; two plans with one digest assign every cell identically.
    #[test]
    fn the_cohort_draw_is_a_pure_function_of_the_plan_digest() {
        let plan = sample_plan(7);
        let twin = FaultPlan::from_json(&serde_json::to_vec_pretty(&plan).unwrap()).unwrap();
        assert_eq!(plan.digest(), twin.digest());
        let digest = plan.digest();
        for cell in cells() {
            let ids = |p: &FaultPlan| {
                p.assigned(cell)
                    .iter()
                    .map(|s| s.id.clone())
                    .collect::<Vec<_>>()
            };
            assert_eq!(ids(&plan), ids(&twin));
            assert_eq!(
                assign(&digest, plan.seed(), plan.faults(), cell),
                plan.assigned(cell)
            );
            assert_eq!(
                cohort_draw(&digest, DrawScope::Group("book"), 7, cell),
                cohort_draw(&digest, DrawScope::Group("book"), 7, cell)
            );
        }
        // Only the digest moves the assignment: the same faults under a
        // different digest string assign some cell differently.
        let other = "0".repeat(64);
        assert!(cells()
            .into_iter()
            .any(|cell| assign(&digest, 7, plan.faults(), cell)
                != assign(&other, 7, plan.faults(), cell)));
    }

    /// Pinned encoding: a refactor that changes the draw bytes changes which
    /// cells a published plan faults, which must be a visible decision. Both
    /// values were reproduced by an independent Python `hashlib` rendering of
    /// the documented length-prefixed encoding.
    #[test]
    fn the_draw_encoding_is_pinned() {
        let cell = CellId {
            window_start: 3,
            window_end: 33,
            seed: 11,
        };
        let digest = "ab".repeat(32);
        assert_eq!(
            cohort_draw(&digest, DrawScope::Group("book"), 7, cell),
            PINNED_GROUP_DRAW
        );
        assert_eq!(
            cohort_draw(&digest, DrawScope::Independent("book"), 7, cell),
            PINNED_INDEPENDENT_DRAW
        );
    }

    const PINNED_GROUP_DRAW: u32 = 972_610;
    const PINNED_INDEPENDENT_DRAW: u32 = 158_468;

    #[test]
    fn grouped_faults_are_mutually_exclusive_and_rates_hold_roughly() {
        let plan = sample_plan(7);
        let cells = cells();
        for &cell in &cells {
            let assigned = plan.assigned(cell);
            let in_group = assigned
                .iter()
                .filter(|s| s.group.as_deref() == Some("book"))
                .count();
            assert!(in_group <= 1, "grouped faults share one draw");
        }
        let rows = plan.denominators(&cells);
        assert_eq!(rows.len(), 3);
        for row in &rows {
            assert_eq!(row.cells, cells.len() as u64);
            let expected = plan
                .faults()
                .iter()
                .find(|s| s.id == row.fault_id)
                .unwrap()
                .cohort_ppm as f64
                / f64::from(COHORT_SCALE);
            let observed = row.assigned as f64 / row.cells as f64;
            assert!(
                (observed - expected).abs() < 0.06,
                "{}: {observed} vs {expected}",
                row.fault_id
            );
        }
    }

    #[test]
    fn binding_is_the_identity_without_a_plan_and_plan_specific_with_one() {
        let invocation = "cd".repeat(32);
        assert_eq!(bind_invocation(&invocation, None), invocation);
        let a = bind_invocation(&invocation, Some(&sample_plan(7)));
        let b = bind_invocation(&invocation, Some(&sample_plan(8)));
        assert_ne!(a, invocation);
        assert_ne!(a, b);
        assert_eq!(a.len(), 64);
        assert_eq!(a, bind_invocation(&invocation, Some(&sample_plan(7))));
    }
}
