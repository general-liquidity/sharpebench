//! The study-protocol document: what a study declares before it runs.
//!
//! Every type here is a wire type. The contract is closed
//! (`#[serde(deny_unknown_fields)]`): a document carrying a key this module
//! does not define is refused at load, not read past. The published JSON
//! Schema under `schema/study-protocol.schema.json` is the language-agnostic
//! statement of the same shape, and `tests/schema_drift.rs` fails the build if
//! the two disagree in either direction.
//!
//! No type here can carry a *result*. There is no field for an achieved
//! interval, an observed rate or a half width that was reached: those exist
//! only in [`crate::precision::PrecisionClaim`], which is constructed from the
//! run count actually executed. See the module docs of [`crate::precision`].

use serde::{Deserialize, Serialize};

/// Identifier of the contract implemented by this module. A field addition,
/// removal or meaning change mints a new value here rather than editing this
/// one in place.
pub const STUDY_CONTRACT_VERSION: &str = "sharpebench/study-protocol/v1";

/// A study-protocol version. Ordering is lexicographic on (major, minor,
/// patch); an amendment to a frozen protocol must raise it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProtocolVersion {
    pub major: u32,
    pub minor: u32,
    pub patch: u32,
}

impl std::fmt::Display for ProtocolVersion {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)
    }
}

/// Which of the three run tiers a protocol belongs to. The tier is not a
/// label: it bounds what the run may claim, and the validator enforces that
/// bound (see [`crate::validate::validate`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunTier {
    /// Small deterministic fixtures and known-answer or control cases, with
    /// pinned outputs and tolerances. Fast behaviour check. It cannot
    /// establish a rare false-positive rate or population power, so it may not
    /// carry an error-rate or power estimand.
    CiRegression,
    /// Exploration and debugging of candidate methods on designated
    /// development seeds and scenarios. Tuning is permitted and recorded; the
    /// outputs are exploratory and are not a claim.
    DevelopmentCalibration,
    /// A fixed method and protocol evaluated on reserved seeds and scenarios.
    /// No tuning on results. The protocol must be marked frozen, and an
    /// amendment to it must raise its version.
    FrozenValidation,
}

impl RunTier {
    pub fn tag(self) -> &'static str {
        match self {
            Self::CiRegression => "ci_regression",
            Self::DevelopmentCalibration => "development_calibration",
            Self::FrozenValidation => "frozen_validation",
        }
    }

    /// Every tag the schema must offer. A new variant is added here and to the
    /// schema together; `tests/schema_drift.rs` compares the two sets.
    pub const ALL_TAGS: &'static [&'static str] = &[
        "ci_regression",
        "development_calibration",
        "frozen_validation",
    ];
}

/// Which method, at which version, under which configuration.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MethodIdentity {
    /// The method under test, named as the repository names it.
    pub method: String,
    /// The method's released version. Unknown is not an acceptable value here:
    /// a study that cannot name the version it evaluated has no identity.
    pub method_version: String,
    /// Operator-facing name of the effective configuration.
    pub configuration_id: String,
    /// Digest over the effective configuration document, lowercase hex.
    pub configuration_digest: String,
    /// Digest of the scoring specification the configuration resolves against.
    pub scoring_spec_hash: String,
}

/// Raw units, or units net of the benchmark. Mixing the two inside one
/// reported comparison is a refusal, not a rounding difference: an arithmetic
/// return difference and a relative-wealth return are different quantities.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EffectUnits {
    Raw,
    Active,
}

impl EffectUnits {
    pub fn tag(self) -> &'static str {
        match self {
            Self::Raw => "raw",
            Self::Active => "active",
        }
    }

    pub const ALL_TAGS: &'static [&'static str] = &["raw", "active"];
}

/// What a named quantity in this study is. Per-entry and whole-field false
/// positives are separate variants because they are separate quantities: a
/// study may target one, the other, or both, but never conflate them.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum EstimandKind {
    /// Probability that one entry in the field is falsely declared eligible.
    PerEntryFalsePositive,
    /// Probability that *any* entry in the field is falsely declared eligible.
    WholeFieldAnyFalseEligibility,
    /// Probability of declaring eligibility at a named, sized effect.
    PowerAtEffect {
        effect_name: String,
        effect_size: f64,
    },
}

impl EstimandKind {
    pub fn tag(&self) -> &'static str {
        match self {
            Self::PerEntryFalsePositive => "per_entry_false_positive",
            Self::WholeFieldAnyFalseEligibility => "whole_field_any_false_eligibility",
            Self::PowerAtEffect { .. } => "power_at_effect",
        }
    }

    pub const ALL_TAGS: &'static [&'static str] = &[
        "per_entry_false_positive",
        "whole_field_any_false_eligibility",
        "power_at_effect",
    ];
}

/// The target attached to an estimand. An error-rate estimand takes an upper
/// limit; a power estimand takes a lower bound. The validator refuses the
/// crossed pairings.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum EstimandTarget {
    /// The study supports its claim only if the rate stays at or below this.
    UpperErrorLimit { limit: f64 },
    /// The study supports its claim only if power reaches at least this.
    LowerPowerBound { bound: f64 },
}

impl EstimandTarget {
    pub fn tag(self) -> &'static str {
        match self {
            Self::UpperErrorLimit { .. } => "upper_error_limit",
            Self::LowerPowerBound { .. } => "lower_power_bound",
        }
    }

    pub const ALL_TAGS: &'static [&'static str] = &["upper_error_limit", "lower_power_bound"];
}

/// One named quantity the study estimates.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Estimand {
    /// Unique within the protocol. Claims reference an estimand by this name,
    /// which is why per-entry and whole-field quantities cannot share one.
    pub name: String,
    pub kind: EstimandKind,
    /// The units this quantity's effects are expressed in.
    pub effect_units: EffectUnits,
    pub target: EstimandTarget,
}

/// The rule that decides, before the data exists, what would count as support
/// for a claim. These are criteria for supporting a claim, not a requirement
/// that the experiment produce a favourable result.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum DecisionRule {
    /// Supported when the estimand's interval upper bound is at or below the
    /// limit. The pairing for an error-rate estimand.
    IntervalUpperBoundAtMost { limit: f64 },
    /// Supported when the estimand's interval lower bound is at or above the
    /// bound. The pairing for a power estimand.
    IntervalLowerBoundAtLeast { bound: f64 },
    /// Explicitly no rule. Present so that omitting a rule is a stated,
    /// refusable condition rather than a missing key that a reader has to
    /// notice.
    Unspecified,
}

impl DecisionRule {
    pub fn tag(self) -> &'static str {
        match self {
            Self::IntervalUpperBoundAtMost { .. } => "interval_upper_bound_at_most",
            Self::IntervalLowerBoundAtLeast { .. } => "interval_lower_bound_at_least",
            Self::Unspecified => "unspecified",
        }
    }

    pub const ALL_TAGS: &'static [&'static str] = &[
        "interval_upper_bound_at_most",
        "interval_lower_bound_at_least",
        "unspecified",
    ];
}

/// One claim the study is run to test.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Claim {
    pub id: String,
    /// What is being asserted, in words, bounded to the method version and
    /// configuration named in [`MethodIdentity`].
    pub statement: String,
    /// The name of the estimand this claim is decided on.
    pub estimand: String,
    pub decision_rule: DecisionRule,
}

/// Who is in the field.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FieldComposition {
    /// Ordinary entrants drawn from the null family.
    pub null_entrants: u32,
    /// Entrants carrying a declared planted effect.
    pub planted_skill_entrants: u32,
    /// Invalid-evidence and refusal controls. These test refusal behaviour;
    /// they are not a substitute for a valid zero-skill null.
    pub control_entrants: u32,
}

/// How observations depend on each other. A study that reports independent
/// replications while entrants share a market history is reporting the wrong
/// denominator, so the validator reads these fields together with
/// [`Replication`].
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DependenceStructure {
    /// Serial dependence model assumed within one return series.
    pub serial: String,
    /// Dependence assumed across entrants within one field.
    pub cross_entrant: String,
    /// True when entrants, windows or seeds are drawn from one market history.
    pub shared_market_history: bool,
}

/// Window count, length and overlap. Overlapping windows are repeated
/// observations of one history, not independent replications.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WindowGeometry {
    pub windows: u32,
    pub window_length_steps: u32,
    pub stride_steps: u32,
    pub overlapping: bool,
}

/// What the study assumes about the search that produced the entries it
/// scores. An understated trial count understates the deflation.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SearchAssumptions {
    pub trials_searched: u32,
    /// How the reported entry was selected out of those trials.
    pub selection_rule: String,
    /// Whether those trials are treated as independent. Declaring
    /// independence that the design does not have inflates the effective
    /// trial count's protection.
    pub independent_trials_assumed: bool,
}

/// Whether the dispersion used by the deflation is a configured constant or
/// measured from the field. The two give different answers and different
/// failure modes, so the protocol states which one the run uses.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum DispersionPolicy {
    Configured { value: f64 },
    Measured { estimator: String },
}

impl DispersionPolicy {
    pub fn tag(&self) -> &'static str {
        match self {
            Self::Configured { .. } => "configured",
            Self::Measured { .. } => "measured",
        }
    }

    pub const ALL_TAGS: &'static [&'static str] = &["configured", "measured"];
}

/// The structural part of the design: families, units, field, dependence,
/// geometry, search and dispersion.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Design {
    /// The zero-skill generating process the error rates are measured against.
    pub null_family: String,
    /// The generating process the power targets are measured against,
    /// including alternatives below and above the deflation benchmark.
    pub alternative_family: String,
    /// The units every effect in this protocol is expressed in. Each estimand
    /// restates its own units and the validator refuses a disagreement.
    pub effect_units: EffectUnits,
    pub field_composition: FieldComposition,
    pub dependence: DependenceStructure,
    pub window_geometry: WindowGeometry,
    pub search_assumptions: SearchAssumptions,
    pub dispersion_policy: DispersionPolicy,
    /// Whether thresholds may be tuned against this tier's outputs. Frozen
    /// validation refuses it.
    pub tuning_allowed: bool,
}

/// The level at which observations are independent.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReplicationUnit {
    /// One independently simulated field. The only unit that is independent
    /// when a market history is shared.
    SimulatedField,
    /// One independently generated market history.
    MarketHistory,
    /// One entrant inside a field.
    Entrant,
    /// One window inside a history.
    Window,
    /// One seed inside a history.
    Seed,
}

impl ReplicationUnit {
    pub fn tag(self) -> &'static str {
        match self {
            Self::SimulatedField => "simulated_field",
            Self::MarketHistory => "market_history",
            Self::Entrant => "entrant",
            Self::Window => "window",
            Self::Seed => "seed",
        }
    }

    pub const ALL_TAGS: &'static [&'static str] = &[
        "simulated_field",
        "market_history",
        "entrant",
        "window",
        "seed",
    ];
}

/// The independent unit and what is nested inside it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Replication {
    pub independent_unit: ReplicationUnit,
    /// Units nested inside the independent one. These are repeated or
    /// dependent observations and are not counted in the denominator of an
    /// independent-replication rate.
    pub nested_observations: Vec<ReplicationUnit>,
    /// How many independent units the study plans to observe.
    pub independent_units_planned: u64,
}

/// Confidence levels with pinned two-sided normal quantiles. Levels are
/// enumerated rather than free-form so that every interval in the project uses
/// the same constant, checked against published values in
/// `tests/precision_known_answers.rs`. A new level is added by pinning its
/// quantile here, not by computing one at runtime.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConfidenceLevel {
    Ninety,
    NinetyFive,
    NinetyNine,
}

impl ConfidenceLevel {
    /// The two-sided standard normal quantile for this level.
    pub fn two_sided_z(self) -> f64 {
        match self {
            Self::Ninety => 1.644_853_626_951_472_2,
            Self::NinetyFive => 1.959_963_984_540_054,
            Self::NinetyNine => 2.575_829_303_548_900_4,
        }
    }

    pub fn tag(self) -> &'static str {
        match self {
            Self::Ninety => "ninety",
            Self::NinetyFive => "ninety_five",
            Self::NinetyNine => "ninety_nine",
        }
    }

    pub const ALL_TAGS: &'static [&'static str] = &["ninety", "ninety_five", "ninety_nine"];
}

/// The interval construction. Only one method is implemented; naming it in the
/// document means a protocol that assumed another one is refused rather than
/// silently reported under this one.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IntervalMethod {
    /// Wilson score interval for a binomial proportion.
    WilsonScore,
}

impl IntervalMethod {
    pub fn tag(self) -> &'static str {
        match self {
            Self::WilsonScore => "wilson_score",
        }
    }

    pub const ALL_TAGS: &'static [&'static str] = &["wilson_score"];
}

/// How the study controls multiplicity across everything it reports.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MultiplicityMethod {
    /// One reported condition only. Refused when more than one condition is
    /// predeclared.
    None,
    Bonferroni,
    HolmBonferroni,
    BenjaminiHochberg,
    RomanoWolf,
}

impl MultiplicityMethod {
    pub fn tag(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Bonferroni => "bonferroni",
            Self::HolmBonferroni => "holm_bonferroni",
            Self::BenjaminiHochberg => "benjamini_hochberg",
            Self::RomanoWolf => "romano_wolf",
        }
    }

    pub const ALL_TAGS: &'static [&'static str] = &[
        "none",
        "bonferroni",
        "holm_bonferroni",
        "benjamini_hochberg",
        "romano_wolf",
    ];
}

/// Multiplicity predeclared across the conditions the study will report. The
/// conditions are listed before the run so that a condition added afterwards
/// is visibly an addition.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Multiplicity {
    pub method: MultiplicityMethod,
    pub reported_conditions: Vec<String>,
}

/// Confidence level, interval construction, the precision the study requires,
/// and the multiplicity it predeclares.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Inference {
    pub confidence_level: ConfidenceLevel,
    pub interval_method: IntervalMethod,
    /// The half width the study requires of its reported interval. This is a
    /// requirement on the design, never a record of what was achieved.
    pub required_half_width: f64,
    pub multiplicity: Multiplicity,
}

/// Which denominator a reason-coded outcome is counted against. Reporting
/// conditional-on-availability results against the attempted denominator, or
/// the reverse, is the silent denominator change the accounting exists to
/// prevent.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Denominator {
    Attempted,
    Completed,
    Available,
}

impl Denominator {
    pub fn tag(self) -> &'static str {
        match self {
            Self::Attempted => "attempted",
            Self::Completed => "completed",
            Self::Available => "available",
        }
    }

    pub const ALL_TAGS: &'static [&'static str] = &["attempted", "completed", "available"];
}

/// What the study does with a run that refuses, is unavailable, or fails on
/// infrastructure.
///
/// There is deliberately no variant that substitutes a zero return, a zero
/// cost or a passing statistic for a missing result. An unavailable result is
/// not a measurement.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum FailurePolicy {
    /// No policy stated. Present so that omission is refusable rather than
    /// absent.
    Unspecified,
    /// Excluded from the analysis, recorded under a reason code, counted
    /// against the named denominator.
    ExcludeWithReasonCode {
        reason_code: String,
        denominator: Denominator,
    },
    /// Retried up to a bounded number of attempts, then excluded as above.
    RetryThenExclude {
        max_attempts: u32,
        reason_code: String,
        denominator: Denominator,
    },
    /// Counted as a failed outcome of the study, against the named
    /// denominator.
    CountAsFailure {
        reason_code: String,
        denominator: Denominator,
    },
}

impl FailurePolicy {
    pub fn tag(&self) -> &'static str {
        match self {
            Self::Unspecified => "unspecified",
            Self::ExcludeWithReasonCode { .. } => "exclude_with_reason_code",
            Self::RetryThenExclude { .. } => "retry_then_exclude",
            Self::CountAsFailure { .. } => "count_as_failure",
        }
    }

    pub const ALL_TAGS: &'static [&'static str] = &[
        "unspecified",
        "exclude_with_reason_code",
        "retry_then_exclude",
        "count_as_failure",
    ];

    pub fn is_specified(&self) -> bool {
        !matches!(self, Self::Unspecified)
    }
}

/// Expected counts and the handling of each way a run can fail to produce a
/// result.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Accounting {
    /// Runs the study expects to start.
    pub expected_attempted: u64,
    /// Of those, the runs expected to finish.
    pub expected_completed: u64,
    /// Of those, the runs expected to yield a usable result.
    pub expected_available: u64,
    pub refusal: FailurePolicy,
    pub unavailability: FailurePolicy,
    pub infrastructure_failure: FailurePolicy,
}

/// The simulation count and what it was sized for. The count is checked
/// against the precision requirement in [`crate::validate::validate`]; it is
/// not taken on trust.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SimulationPlan {
    pub planned_runs: u64,
    /// The rate the count is sized for. For a rare-event bound this is the
    /// target rate itself.
    pub anticipated_rate: f64,
    /// Development-stage runtime estimate per run.
    pub runtime_estimate_seconds_per_run: f64,
}

impl SimulationPlan {
    /// Estimated wall time for the planned count, in hours.
    pub fn runtime_estimate_hours(&self) -> f64 {
        self.planned_runs as f64 * self.runtime_estimate_seconds_per_run / 3600.0
    }
}

/// The resource and spend ceiling, and who approved it. A protocol whose
/// estimates exceed its ceiling, or whose ceiling nobody approved, is refused
/// before it can spend.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Budget {
    pub approved: bool,
    /// Who approved it. Required when `approved` is true.
    pub approver: String,
    pub spend_cap_usd: f64,
    pub compute_cap_core_hours: f64,
    pub estimated_spend_usd: f64,
    pub estimated_core_hours: f64,
}

/// When the study stops. Fixed by default; an adaptive rule must carry its own
/// error-control analysis.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum StoppingRule {
    /// Stop after exactly this many runs, whatever the interim numbers look
    /// like. Must equal the planned simulation count.
    FixedRuns { runs: u64 },
    /// An interim-analysis rule. Refused unless it states the error-control
    /// analysis that justifies it.
    Adaptive {
        rule: String,
        error_control_analysis: String,
    },
}

impl StoppingRule {
    pub fn tag(&self) -> &'static str {
        match self {
            Self::FixedRuns { .. } => "fixed_runs",
            Self::Adaptive { .. } => "adaptive",
        }
    }

    pub const ALL_TAGS: &'static [&'static str] = &["fixed_runs", "adaptive"];
}

/// One study protocol.
///
/// Load with [`StudyProtocol::from_json`], which refuses an unknown key, then
/// validate with [`crate::validate::validate`], which refuses a document that
/// parses but does not describe a runnable study.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StudyProtocol {
    /// Must equal [`STUDY_CONTRACT_VERSION`].
    pub contract_version: String,
    /// Stable identifier for this protocol across its versions.
    pub protocol_id: String,
    pub version: ProtocolVersion,
    pub tier: RunTier,
    /// True once the document is sealed. Required on the frozen tier, refused
    /// on the other two. An edit to a frozen protocol must raise `version`;
    /// see [`crate::amend::check_amendment`].
    pub frozen: bool,
    pub identity: MethodIdentity,
    pub claims: Vec<Claim>,
    pub estimands: Vec<Estimand>,
    pub design: Design,
    pub replication: Replication,
    pub inference: Inference,
    pub accounting: Accounting,
    pub simulation: SimulationPlan,
    pub budget: Budget,
    pub stopping_rule: StoppingRule,
    /// Free text for the reader: provenance, caveats, and the decisions a
    /// placeholder document is still waiting on. Carries no meaning for the
    /// validator and no result.
    pub notes: String,
}

impl StudyProtocol {
    /// Parse a protocol document. An unknown or misspelled key is refused
    /// here, with the offending field named by serde.
    pub fn from_json(text: &str) -> Result<Self, crate::refusal::ProtocolRefusal> {
        serde_json::from_str(text).map_err(|error| crate::refusal::ProtocolRefusal::Malformed {
            detail: error.to_string(),
        })
    }

    /// The estimand of the given name, if the protocol declares one.
    pub fn estimand(&self, name: &str) -> Option<&Estimand> {
        self.estimands.iter().find(|e| e.name == name)
    }
}
