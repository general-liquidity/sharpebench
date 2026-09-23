//! Intervals, the run count a precision requirement implies, and the only
//! place a precision *claim* can come from.
//!
//! # Why a reduced run count cannot leave a stale precision claim
//!
//! The mechanism is structural, not a review convention:
//!
//! 1. [`crate::protocol::StudyProtocol`] has no field that can hold an
//!    achieved interval, an observed rate or a reached half width.
//!    `Inference::required_half_width` is a requirement on the design.
//!    `tests/stale_precision.rs` walks the serialized protocol and asserts no
//!    result-bearing key exists at any depth, so a claim cannot be smuggled in
//!    through the configuration.
//! 2. [`PrecisionClaim`] has private fields, no `Deserialize`, and exactly one
//!    constructor, [`PrecisionClaim::from_realized`], whose interval is a
//!    function of the trial count passed to it. There is no setter and no
//!    parse path, so no claim can be carried over from an earlier, larger run.
//! 3. [`StudyReport::from_realized_runs`] recomputes the claim from the
//!    realized count on every call and refuses with
//!    [`crate::refusal::ReportRefusal::PrecisionTargetUnmet`] when that count
//!    leaves the interval wider than the protocol required. Reducing the count
//!    therefore either widens the reported interval or produces no report.

use serde::Serialize;

use crate::protocol::{ConfidenceLevel, StudyProtocol};
use crate::refusal::{PrecisionError, ReportRefusal};

/// Largest trial count [`required_simulation_runs`] will search to before
/// declaring a requirement unreachable. A requirement that needs more runs
/// than this is a claim to narrow, not a budget to raise quietly.
pub const MAX_SEARCHED_RUNS: u64 = 1_000_000_000;

/// A two-sided interval for a proportion.
#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub struct Interval {
    pub lower: f64,
    pub upper: f64,
}

impl Interval {
    pub fn half_width(&self) -> f64 {
        (self.upper - self.lower) / 2.0
    }

    pub fn center(&self) -> f64 {
        (self.upper + self.lower) / 2.0
    }
}

/// Wilson score interval for `events` successes in `trials` binomial trials.
///
/// The Wilson interval is used rather than the Wald interval because the
/// quantities this crate reports are rare-event rates, where the Wald interval
/// collapses to zero width at zero observed events and would report a false
/// certainty. At zero events the Wilson upper bound stays positive, which is
/// the honest statement: observing no failures is not proof of a zero rate.
///
/// Both bounds are clamped into `[0, 1]`; the interval is never wider than the
/// parameter space.
pub fn wilson_interval(
    events: u64,
    trials: u64,
    level: ConfidenceLevel,
) -> Result<Interval, PrecisionError> {
    if trials == 0 {
        return Err(PrecisionError::NoTrials);
    }
    if events > trials {
        return Err(PrecisionError::EventsExceedTrials { events, trials });
    }
    let n = trials as f64;
    let x = events as f64;
    let z = level.two_sided_z();
    let z2 = z * z;
    let denominator = n + z2;
    let center = (x + z2 / 2.0) / denominator;
    let spread = z / denominator * (x * (n - x) / n + z2 / 4.0).sqrt();
    Ok(Interval {
        lower: (center - spread).max(0.0),
        upper: (center + spread).min(1.0),
    })
}

/// The smallest trial count whose interval at `anticipated_rate` has a half
/// width no wider than `required_half_width`.
///
/// The count is derived from the interval construction the study will actually
/// report under, not from a separate normal approximation, so the plan and the
/// report cannot disagree about what a given count buys. The search is a
/// doubling scan followed by a bisection; the Wilson half width is monotone
/// decreasing in the trial count at a fixed rate, which is what makes the
/// bisection sound.
pub fn required_simulation_runs(
    anticipated_rate: f64,
    required_half_width: f64,
    level: ConfidenceLevel,
) -> Result<u64, PrecisionError> {
    if !(anticipated_rate.is_finite() && (0.0..=1.0).contains(&anticipated_rate)) {
        return Err(PrecisionError::InvalidParameter {
            name: "anticipated_rate",
            requirement: "must be finite and in [0, 1]",
        });
    }
    if !(required_half_width.is_finite() && required_half_width > 0.0 && required_half_width < 1.0)
    {
        return Err(PrecisionError::InvalidParameter {
            name: "required_half_width",
            requirement: "must be finite and in (0, 1)",
        });
    }

    let half_width_at = |n: u64| -> Result<f64, PrecisionError> {
        let events = (anticipated_rate * n as f64).round() as u64;
        Ok(wilson_interval(events.min(n), n, level)?.half_width())
    };

    let mut high = 1u64;
    while half_width_at(high)? > required_half_width {
        if high >= MAX_SEARCHED_RUNS {
            return Err(PrecisionError::RequirementUnreachable {
                required_half_width_millionths: (required_half_width * 1e6).round() as u64,
                searched_up_to: MAX_SEARCHED_RUNS,
            });
        }
        high = (high * 2).min(MAX_SEARCHED_RUNS);
    }

    let mut low = high / 2;
    while low + 1 < high {
        let middle = low + (high - low) / 2;
        if half_width_at(middle)? <= required_half_width {
            high = middle;
        } else {
            low = middle;
        }
    }
    Ok(high)
}

/// A precision statement about runs that were executed.
///
/// The fields are private and there is no `Deserialize`: the only way to hold
/// one is [`PrecisionClaim::from_realized`], which computes the interval from
/// the trial count it is given. A claim made at one count cannot be reattached
/// to another.
#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub struct PrecisionClaim {
    realized_runs: u64,
    observed_events: u64,
    confidence_level: ConfidenceLevel,
    interval: Interval,
}

impl PrecisionClaim {
    /// Derive the claim from the runs actually executed.
    pub fn from_realized(
        observed_events: u64,
        realized_runs: u64,
        confidence_level: ConfidenceLevel,
    ) -> Result<Self, PrecisionError> {
        let interval = wilson_interval(observed_events, realized_runs, confidence_level)?;
        Ok(Self {
            realized_runs,
            observed_events,
            confidence_level,
            interval,
        })
    }

    pub fn realized_runs(&self) -> u64 {
        self.realized_runs
    }

    pub fn observed_events(&self) -> u64 {
        self.observed_events
    }

    pub fn confidence_level(&self) -> ConfidenceLevel {
        self.confidence_level
    }

    pub fn interval(&self) -> Interval {
        self.interval
    }

    pub fn half_width(&self) -> f64 {
        self.interval.half_width()
    }

    /// The observed rate over the realized denominator. Never over the planned
    /// one.
    pub fn observed_rate(&self) -> f64 {
        self.observed_events as f64 / self.realized_runs as f64
    }
}

/// Whether a claim's decision rule was met by the interval that was reached.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ClaimStatus {
    Supported,
    NotSupported,
}

/// What one executed study leg reports for one estimand.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct StudyReport {
    pub protocol_id: String,
    pub protocol_version: crate::protocol::ProtocolVersion,
    pub tier: crate::protocol::RunTier,
    pub estimand: String,
    /// Planned runs, carried for the reader's comparison only. The interval
    /// below is never computed from it.
    pub planned_runs: u64,
    pub precision: PrecisionClaim,
    pub claims: Vec<ClaimOutcome>,
}

/// One claim's prespecified decision applied to the interval that was reached.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ClaimOutcome {
    pub claim: String,
    pub status: ClaimStatus,
}

impl StudyReport {
    /// Build the report for one estimand from the runs that were executed.
    ///
    /// Refuses when the realized count leaves the interval wider than
    /// `inference.required_half_width`. There is no path that emits the
    /// planned count's precision alongside a smaller realized count.
    pub fn from_realized_runs(
        protocol: &StudyProtocol,
        estimand: &str,
        observed_events: u64,
        realized_runs: u64,
    ) -> Result<Self, ReportRefusal> {
        if protocol.estimand(estimand).is_none() {
            return Err(ReportRefusal::UndeclaredEstimand {
                estimand: estimand.to_string(),
            });
        }
        let precision = PrecisionClaim::from_realized(
            observed_events,
            realized_runs,
            protocol.inference.confidence_level,
        )?;
        let achieved = precision.half_width();
        if achieved > protocol.inference.required_half_width {
            return Err(ReportRefusal::PrecisionTargetUnmet {
                required_half_width: protocol.inference.required_half_width,
                achieved_half_width: achieved,
                planned_runs: protocol.simulation.planned_runs,
                realized_runs,
            });
        }
        let claims = protocol
            .claims
            .iter()
            .filter(|claim| claim.estimand == estimand)
            .map(|claim| ClaimOutcome {
                claim: claim.id.clone(),
                status: decide(claim.decision_rule, precision.interval()),
            })
            .collect();
        Ok(Self {
            protocol_id: protocol.protocol_id.clone(),
            protocol_version: protocol.version,
            tier: protocol.tier,
            estimand: estimand.to_string(),
            planned_runs: protocol.simulation.planned_runs,
            precision,
            claims,
        })
    }
}

fn decide(rule: crate::protocol::DecisionRule, interval: Interval) -> ClaimStatus {
    use crate::protocol::DecisionRule;
    let supported = match rule {
        DecisionRule::IntervalUpperBoundAtMost { limit } => interval.upper <= limit,
        DecisionRule::IntervalLowerBoundAtLeast { bound } => interval.lower >= bound,
        // A validated protocol has no unspecified rule; an unvalidated one
        // decides nothing rather than defaulting to support.
        DecisionRule::Unspecified => false,
    };
    if supported {
        ClaimStatus::Supported
    } else {
        ClaimStatus::NotSupported
    }
}
