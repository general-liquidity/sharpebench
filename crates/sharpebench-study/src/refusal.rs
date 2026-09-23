//! Typed refusals. A protocol is either accepted or refused for a named
//! reason; there is no warning level and no partial acceptance.

use std::fmt;

use crate::protocol::{EffectUnits, ProtocolVersion, RunTier};

/// Why a study protocol was refused.
#[derive(Clone, Debug, PartialEq)]
pub enum ProtocolRefusal {
    /// The document did not parse against the closed contract: an unknown key,
    /// a missing required key, or a wrong type. Serde's message names the
    /// field.
    Malformed { detail: String },
    /// `contract_version` is not one this build implements.
    UnsupportedContractVersion {
        found: String,
        supported: &'static str,
    },
    /// The protocol declares no false-positive estimand. A study of an
    /// eligibility gate must name a per-entry rate, a whole-field rate, or
    /// both, as separate quantities.
    MissingFalsePositiveEstimand,
    /// Two estimands share a name, so a claim cannot say which it is decided
    /// on.
    DuplicateEstimandName { name: String },
    /// A claim references an estimand the protocol does not declare.
    UndeclaredEstimand { claim: String, estimand: String },
    /// A claim carries no decision rule.
    MissingDecisionRule { claim: String },
    /// The decision rule cannot decide the estimand it is attached to: an
    /// upper-bound rule on a power estimand, or a lower-bound rule on an
    /// error-rate estimand.
    DecisionRuleEstimandMismatch {
        claim: String,
        estimand: String,
        rule_tag: &'static str,
        estimand_tag: &'static str,
    },
    /// An error-rate estimand carries a power bound, or a power estimand
    /// carries an error limit.
    EstimandTargetMismatch {
        estimand: String,
        kind_tag: &'static str,
        target_tag: &'static str,
    },
    /// A power estimand does not name the effect it is powered at.
    PowerTargetWithoutNamedEffect { estimand: String },
    /// An estimand's effect units differ from the protocol's. Arithmetic
    /// return differences and relative-wealth returns are different
    /// quantities; one document reports one of them.
    IncompatibleEffectUnits {
        estimand: String,
        protocol_units: EffectUnits,
        estimand_units: EffectUnits,
    },
    /// One of refusal, unavailability or infrastructure failure has no stated
    /// handling.
    MissingFailurePolicy { outcome: &'static str },
    /// The expected counts do not nest: completed cannot exceed attempted, and
    /// available cannot exceed completed.
    InconsistentCounts {
        expected_attempted: u64,
        expected_completed: u64,
        expected_available: u64,
    },
    /// The budget was not approved, names no approver, or has a ceiling that
    /// is not a positive finite number.
    UnapprovedBudget { detail: &'static str },
    /// The planned work exceeds its own ceiling.
    BudgetCeilingExceeded {
        quantity: &'static str,
        estimated: f64,
        cap: f64,
    },
    /// The stated compute estimate and the one the run count and per-run
    /// runtime imply disagree, so the ceiling is being checked against a
    /// number nothing produced.
    RuntimeEstimateInconsistent {
        stated_core_hours: f64,
        derived_core_hours: f64,
    },
    /// The planned simulation count is smaller than the precision requirement
    /// implies. Running fewer widens the interval; this refuses the plan
    /// rather than letting the report inherit an unreachable half width.
    SimulationCountBelowPrecision { planned: u64, required: u64 },
    /// A quantity that must be a probability, a half width or a positive
    /// count is not.
    InvalidParameter {
        name: &'static str,
        requirement: &'static str,
    },
    /// A fixed stopping rule names a run count other than the planned
    /// simulation count.
    StoppingRuleCountMismatch { stopping_runs: u64, planned: u64 },
    /// An adaptive stopping rule with no error-control analysis behind it.
    AdaptiveStoppingWithoutErrorControl,
    /// The declared independent unit is also declared as nested inside
    /// itself, or is a unit that a shared market history makes dependent.
    PseudoreplicationDeclared { detail: &'static str },
    /// More than one reported condition with no multiplicity method, or no
    /// predeclared conditions at all.
    MultiplicityNotPredeclared { detail: &'static str },
    /// The tier cannot carry this claim. A CI regression run is a behaviour
    /// check with pinned outputs; it cannot establish a rare false-positive
    /// rate or population power.
    TierClaimBoundary {
        tier: RunTier,
        claim: String,
        estimand_tag: &'static str,
    },
    /// Tuning against results was left enabled on the frozen tier.
    TuningOnFrozenTier,
    /// The frozen flag and the tier disagree.
    FrozenFlagTierMismatch { tier: RunTier, frozen: bool },
    /// The protocol has no claims to test.
    NoClaims,
    /// A frozen protocol was edited without raising its version.
    FrozenProtocolModified {
        protocol_id: String,
        version: ProtocolVersion,
    },
    /// An amendment changed the protocol identity, so it is a different
    /// protocol rather than a new version of this one.
    ProtocolIdentityChanged { previous: String, next: String },
    /// An amendment lowered or held the version of a frozen protocol.
    VersionNotRaised {
        previous: ProtocolVersion,
        next: ProtocolVersion,
    },
}

impl fmt::Display for ProtocolRefusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Malformed { detail } => {
                write!(f, "document does not satisfy the closed contract: {detail}")
            }
            Self::UnsupportedContractVersion { found, supported } => write!(
                f,
                "contract_version {found} is not implemented by this build, which implements {supported}"
            ),
            Self::MissingFalsePositiveEstimand => write!(
                f,
                "no false-positive estimand declared: name a per-entry rate, a whole-field rate, or both"
            ),
            Self::DuplicateEstimandName { name } => {
                write!(f, "estimand name {name} is declared more than once")
            }
            Self::UndeclaredEstimand { claim, estimand } => write!(
                f,
                "claim {claim} is decided on estimand {estimand}, which is not declared"
            ),
            Self::MissingDecisionRule { claim } => {
                write!(f, "claim {claim} carries no decision rule")
            }
            Self::DecisionRuleEstimandMismatch {
                claim,
                estimand,
                rule_tag,
                estimand_tag,
            } => write!(
                f,
                "claim {claim} decides estimand {estimand} ({estimand_tag}) with a {rule_tag} rule, which cannot decide it"
            ),
            Self::EstimandTargetMismatch {
                estimand,
                kind_tag,
                target_tag,
            } => write!(
                f,
                "estimand {estimand} is a {kind_tag} but carries a {target_tag} target"
            ),
            Self::PowerTargetWithoutNamedEffect { estimand } => write!(
                f,
                "power estimand {estimand} does not name the effect it is powered at"
            ),
            Self::IncompatibleEffectUnits {
                estimand,
                protocol_units,
                estimand_units,
            } => write!(
                f,
                "estimand {estimand} is in {} units while the protocol declares {} units",
                estimand_units.tag(),
                protocol_units.tag()
            ),
            Self::MissingFailurePolicy { outcome } => {
                write!(f, "no policy stated for {outcome}")
            }
            Self::InconsistentCounts {
                expected_attempted,
                expected_completed,
                expected_available,
            } => write!(
                f,
                "expected counts do not nest: attempted {expected_attempted}, completed {expected_completed}, available {expected_available}"
            ),
            Self::UnapprovedBudget { detail } => write!(f, "budget is not approved: {detail}"),
            Self::BudgetCeilingExceeded {
                quantity,
                estimated,
                cap,
            } => write!(
                f,
                "estimated {quantity} {estimated} exceeds its cap {cap}"
            ),
            Self::RuntimeEstimateInconsistent {
                stated_core_hours,
                derived_core_hours,
            } => write!(
                f,
                "the stated estimate of {stated_core_hours} core-hours disagrees with the {derived_core_hours} the run count and per-run runtime imply"
            ),
            Self::SimulationCountBelowPrecision { planned, required } => write!(
                f,
                "planned {planned} runs cannot reach the required precision, which needs {required}"
            ),
            Self::InvalidParameter { name, requirement } => write!(f, "{name} {requirement}"),
            Self::StoppingRuleCountMismatch {
                stopping_runs,
                planned,
            } => write!(
                f,
                "fixed stopping rule names {stopping_runs} runs but the plan is {planned}"
            ),
            Self::AdaptiveStoppingWithoutErrorControl => write!(
                f,
                "adaptive stopping rule states no error-control analysis"
            ),
            Self::PseudoreplicationDeclared { detail } => {
                write!(f, "independent unit is not independent: {detail}")
            }
            Self::MultiplicityNotPredeclared { detail } => {
                write!(f, "multiplicity is not predeclared: {detail}")
            }
            Self::TierClaimBoundary {
                tier,
                claim,
                estimand_tag,
            } => write!(
                f,
                "tier {} cannot carry claim {claim}, which is decided on a {estimand_tag} estimand",
                tier.tag()
            ),
            Self::TuningOnFrozenTier => write!(
                f,
                "frozen validation refuses tuning against its own results"
            ),
            Self::FrozenFlagTierMismatch { tier, frozen } => write!(
                f,
                "tier {} with frozen={frozen} is contradictory",
                tier.tag()
            ),
            Self::NoClaims => write!(f, "protocol declares no claim to test"),
            Self::FrozenProtocolModified {
                protocol_id,
                version,
            } => write!(
                f,
                "frozen protocol {protocol_id} {version} was edited without raising its version"
            ),
            Self::ProtocolIdentityChanged { previous, next } => write!(
                f,
                "amendment changes protocol_id from {previous} to {next}, so it is a different protocol"
            ),
            Self::VersionNotRaised { previous, next } => {
                write!(f, "version {next} does not raise {previous}")
            }
        }
    }
}

impl std::error::Error for ProtocolRefusal {}

/// Why a precision computation could not be performed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PrecisionError {
    /// No runs, so no interval.
    NoTrials,
    /// More events than trials.
    EventsExceedTrials { events: u64, trials: u64 },
    /// A rate outside [0, 1], or a non-finite or non-positive half width.
    InvalidParameter {
        name: &'static str,
        requirement: &'static str,
    },
    /// The required half width cannot be reached inside the search ceiling.
    /// Narrow the claim rather than promising a precision no affordable run
    /// delivers.
    RequirementUnreachable {
        required_half_width_millionths: u64,
        searched_up_to: u64,
    },
}

impl fmt::Display for PrecisionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoTrials => write!(f, "an interval needs at least one trial"),
            Self::EventsExceedTrials { events, trials } => {
                write!(f, "{events} events in {trials} trials is impossible")
            }
            Self::InvalidParameter { name, requirement } => write!(f, "{name} {requirement}"),
            Self::RequirementUnreachable {
                required_half_width_millionths,
                searched_up_to,
            } => write!(
                f,
                "a half width of {required_half_width_millionths}e-6 is not reached by {searched_up_to} runs"
            ),
        }
    }
}

impl std::error::Error for PrecisionError {}

/// Why a study report could not be produced from the runs that were executed.
#[derive(Clone, Debug, PartialEq)]
pub enum ReportRefusal {
    /// The report names an estimand the protocol does not declare.
    UndeclaredEstimand { estimand: String },
    /// The interval could not be computed.
    Precision(PrecisionError),
    /// The runs actually executed do not reach the precision the protocol
    /// requires. The report is refused; it is never emitted carrying the
    /// precision the larger planned count would have bought.
    PrecisionTargetUnmet {
        required_half_width: f64,
        achieved_half_width: f64,
        planned_runs: u64,
        realized_runs: u64,
    },
}

impl fmt::Display for ReportRefusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UndeclaredEstimand { estimand } => {
                write!(f, "estimand {estimand} is not declared by this protocol")
            }
            Self::Precision(error) => write!(f, "{error}"),
            Self::PrecisionTargetUnmet {
                required_half_width,
                achieved_half_width,
                planned_runs,
                realized_runs,
            } => write!(
                f,
                "{realized_runs} of {planned_runs} planned runs reach a half width of {achieved_half_width}, wider than the required {required_half_width}"
            ),
        }
    }
}

impl std::error::Error for ReportRefusal {}

impl From<PrecisionError> for ReportRefusal {
    fn from(error: PrecisionError) -> Self {
        Self::Precision(error)
    }
}
