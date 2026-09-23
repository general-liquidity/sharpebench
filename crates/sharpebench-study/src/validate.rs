//! The validator. It refuses a protocol that parses but does not describe a
//! study that could be run and reported honestly.

use std::collections::BTreeSet;

use crate::precision::required_simulation_runs;
use crate::protocol::{
    Accounting, Budget, DecisionRule, Design, Estimand, EstimandKind, EstimandTarget,
    FailurePolicy, Inference, MultiplicityMethod, Replication, ReplicationUnit, RunTier,
    SimulationPlan, StoppingRule, StudyProtocol, STUDY_CONTRACT_VERSION,
};
use crate::refusal::ProtocolRefusal;

/// Relative agreement required between the stated compute estimate and the one
/// the run count and per-run runtime imply, with a floor of a hundredth of an
/// hour so that a rounded small estimate is not refused.
const RUNTIME_ESTIMATE_TOLERANCE: f64 = 0.01;

/// Validate a parsed protocol. The first refusal is returned; the checks are
/// ordered so that the earliest one names the most structural problem.
pub fn validate(protocol: &StudyProtocol) -> Result<(), ProtocolRefusal> {
    if protocol.contract_version != STUDY_CONTRACT_VERSION {
        return Err(ProtocolRefusal::UnsupportedContractVersion {
            found: protocol.contract_version.clone(),
            supported: STUDY_CONTRACT_VERSION,
        });
    }
    check_tier(protocol)?;
    check_estimands(&protocol.estimands, &protocol.design, protocol.tier)?;
    check_claims(protocol)?;
    check_replication(&protocol.replication, &protocol.design)?;
    check_inference(&protocol.inference)?;
    check_accounting(&protocol.accounting)?;
    check_budget(&protocol.budget)?;
    check_simulation(&protocol.simulation, &protocol.inference)?;
    check_budget_feasibility(&protocol.simulation, &protocol.budget)?;
    check_stopping_rule(&protocol.stopping_rule, &protocol.simulation)?;
    Ok(())
}

fn check_tier(protocol: &StudyProtocol) -> Result<(), ProtocolRefusal> {
    let frozen_expected = protocol.tier == RunTier::FrozenValidation;
    if protocol.frozen != frozen_expected {
        return Err(ProtocolRefusal::FrozenFlagTierMismatch {
            tier: protocol.tier,
            frozen: protocol.frozen,
        });
    }
    if protocol.tier == RunTier::FrozenValidation && protocol.design.tuning_allowed {
        return Err(ProtocolRefusal::TuningOnFrozenTier);
    }
    Ok(())
}

/// The CI regression tier declares no rate or power estimand, because it
/// cannot measure one. Every other tier must name at least one false-positive
/// quantity.
fn check_estimands(
    estimands: &[Estimand],
    design: &Design,
    tier: RunTier,
) -> Result<(), ProtocolRefusal> {
    let mut seen = BTreeSet::new();
    for estimand in estimands {
        if !seen.insert(estimand.name.as_str()) {
            return Err(ProtocolRefusal::DuplicateEstimandName {
                name: estimand.name.clone(),
            });
        }
        if estimand.effect_units != design.effect_units {
            return Err(ProtocolRefusal::IncompatibleEffectUnits {
                estimand: estimand.name.clone(),
                protocol_units: design.effect_units,
                estimand_units: estimand.effect_units,
            });
        }
        match (&estimand.kind, estimand.target) {
            (
                EstimandKind::PerEntryFalsePositive | EstimandKind::WholeFieldAnyFalseEligibility,
                EstimandTarget::UpperErrorLimit { limit },
            ) => probability(limit, "estimand target limit")?,
            (EstimandKind::PowerAtEffect { .. }, EstimandTarget::LowerPowerBound { bound }) => {
                probability(bound, "estimand power bound")?;
            }
            (kind, target) => {
                return Err(ProtocolRefusal::EstimandTargetMismatch {
                    estimand: estimand.name.clone(),
                    kind_tag: kind.tag(),
                    target_tag: target.tag(),
                })
            }
        }
        if let EstimandKind::PowerAtEffect {
            effect_name,
            effect_size,
        } = &estimand.kind
        {
            if effect_name.trim().is_empty() || !effect_size.is_finite() {
                return Err(ProtocolRefusal::PowerTargetWithoutNamedEffect {
                    estimand: estimand.name.clone(),
                });
            }
        }
    }

    let has_false_positive_estimand = estimands.iter().any(|estimand| {
        matches!(
            estimand.kind,
            EstimandKind::PerEntryFalsePositive | EstimandKind::WholeFieldAnyFalseEligibility
        )
    });
    if !has_false_positive_estimand && tier != RunTier::CiRegression {
        return Err(ProtocolRefusal::MissingFalsePositiveEstimand);
    }
    Ok(())
}

fn check_claims(protocol: &StudyProtocol) -> Result<(), ProtocolRefusal> {
    if protocol.claims.is_empty() {
        // A CI regression protocol is a pinned-output behaviour check, so it
        // carries no claim; every other tier exists to test one.
        return if protocol.tier == RunTier::CiRegression {
            Ok(())
        } else {
            Err(ProtocolRefusal::NoClaims)
        };
    }
    for claim in &protocol.claims {
        let estimand = protocol.estimand(&claim.estimand).ok_or_else(|| {
            ProtocolRefusal::UndeclaredEstimand {
                claim: claim.id.clone(),
                estimand: claim.estimand.clone(),
            }
        })?;
        match (claim.decision_rule, &estimand.kind) {
            (DecisionRule::Unspecified, _) => {
                return Err(ProtocolRefusal::MissingDecisionRule {
                    claim: claim.id.clone(),
                })
            }
            (
                DecisionRule::IntervalUpperBoundAtMost { .. },
                EstimandKind::PerEntryFalsePositive | EstimandKind::WholeFieldAnyFalseEligibility,
            )
            | (
                DecisionRule::IntervalLowerBoundAtLeast { .. },
                EstimandKind::PowerAtEffect { .. },
            ) => {}
            (rule, kind) => {
                return Err(ProtocolRefusal::DecisionRuleEstimandMismatch {
                    claim: claim.id.clone(),
                    estimand: estimand.name.clone(),
                    rule_tag: rule.tag(),
                    estimand_tag: kind.tag(),
                })
            }
        }
        if protocol.tier == RunTier::CiRegression {
            return Err(ProtocolRefusal::TierClaimBoundary {
                tier: protocol.tier,
                claim: claim.id.clone(),
                estimand_tag: estimand.kind.tag(),
            });
        }
    }
    Ok(())
}

fn check_replication(replication: &Replication, design: &Design) -> Result<(), ProtocolRefusal> {
    if replication
        .nested_observations
        .contains(&replication.independent_unit)
    {
        return Err(ProtocolRefusal::PseudoreplicationDeclared {
            detail: "the declared independent unit is also declared nested inside itself",
        });
    }
    if replication.independent_units_planned == 0 {
        return Err(ProtocolRefusal::InvalidParameter {
            name: "independent_units_planned",
            requirement: "must be at least one",
        });
    }
    if design.dependence.shared_market_history
        && matches!(
            replication.independent_unit,
            ReplicationUnit::Entrant | ReplicationUnit::Window | ReplicationUnit::Seed
        )
    {
        return Err(ProtocolRefusal::PseudoreplicationDeclared {
            detail: "entrants, windows and seeds inside one shared market history are repeated observations, not independent market replications",
        });
    }
    Ok(())
}

fn check_inference(inference: &Inference) -> Result<(), ProtocolRefusal> {
    if !(inference.required_half_width.is_finite()
        && inference.required_half_width > 0.0
        && inference.required_half_width < 1.0)
    {
        return Err(ProtocolRefusal::InvalidParameter {
            name: "required_half_width",
            requirement: "must be finite and in (0, 1)",
        });
    }
    let conditions = &inference.multiplicity.reported_conditions;
    if conditions.is_empty() {
        return Err(ProtocolRefusal::MultiplicityNotPredeclared {
            detail: "no reported conditions are listed",
        });
    }
    if conditions.len() > 1 && inference.multiplicity.method == MultiplicityMethod::None {
        return Err(ProtocolRefusal::MultiplicityNotPredeclared {
            detail: "more than one reported condition with no multiplicity method",
        });
    }
    Ok(())
}

fn check_accounting(accounting: &Accounting) -> Result<(), ProtocolRefusal> {
    for (outcome, policy) in [
        ("refusal", &accounting.refusal),
        ("unavailability", &accounting.unavailability),
        ("infrastructure failure", &accounting.infrastructure_failure),
    ] {
        if !policy.is_specified() {
            return Err(ProtocolRefusal::MissingFailurePolicy { outcome });
        }
        if let FailurePolicy::RetryThenExclude { max_attempts, .. } = policy {
            if *max_attempts == 0 {
                return Err(ProtocolRefusal::InvalidParameter {
                    name: "max_attempts",
                    requirement: "must be at least one",
                });
            }
        }
    }
    if accounting.expected_attempted == 0 {
        return Err(ProtocolRefusal::InvalidParameter {
            name: "expected_attempted",
            requirement: "must be at least one",
        });
    }
    if accounting.expected_completed > accounting.expected_attempted
        || accounting.expected_available > accounting.expected_completed
    {
        return Err(ProtocolRefusal::InconsistentCounts {
            expected_attempted: accounting.expected_attempted,
            expected_completed: accounting.expected_completed,
            expected_available: accounting.expected_available,
        });
    }
    Ok(())
}

fn check_budget(budget: &Budget) -> Result<(), ProtocolRefusal> {
    if !budget.approved {
        return Err(ProtocolRefusal::UnapprovedBudget {
            detail: "approved is false",
        });
    }
    if budget.approver.trim().is_empty() {
        return Err(ProtocolRefusal::UnapprovedBudget {
            detail: "no approver is named",
        });
    }
    for (detail, cap) in [
        (
            "spend_cap_usd is not a positive finite number",
            budget.spend_cap_usd,
        ),
        (
            "compute_cap_core_hours is not a positive finite number",
            budget.compute_cap_core_hours,
        ),
    ] {
        if !(cap.is_finite() && cap > 0.0) {
            return Err(ProtocolRefusal::UnapprovedBudget { detail });
        }
    }
    for (quantity, estimated, cap) in [
        (
            "spend_usd",
            budget.estimated_spend_usd,
            budget.spend_cap_usd,
        ),
        (
            "core_hours",
            budget.estimated_core_hours,
            budget.compute_cap_core_hours,
        ),
    ] {
        if !(estimated.is_finite() && estimated >= 0.0) {
            return Err(ProtocolRefusal::UnapprovedBudget {
                detail: "an estimate is not a nonnegative finite number",
            });
        }
        if estimated > cap {
            return Err(ProtocolRefusal::BudgetCeilingExceeded {
                quantity,
                estimated,
                cap,
            });
        }
    }
    Ok(())
}

fn check_simulation(
    simulation: &SimulationPlan,
    inference: &Inference,
) -> Result<(), ProtocolRefusal> {
    if simulation.planned_runs == 0 {
        return Err(ProtocolRefusal::InvalidParameter {
            name: "planned_runs",
            requirement: "must be at least one",
        });
    }
    if !(simulation.runtime_estimate_seconds_per_run.is_finite()
        && simulation.runtime_estimate_seconds_per_run > 0.0)
    {
        return Err(ProtocolRefusal::InvalidParameter {
            name: "runtime_estimate_seconds_per_run",
            requirement: "must be finite and positive",
        });
    }
    probability(simulation.anticipated_rate, "anticipated_rate")?;
    let required = required_simulation_runs(
        simulation.anticipated_rate,
        inference.required_half_width,
        inference.confidence_level,
    )
    .map_err(|_| ProtocolRefusal::InvalidParameter {
        name: "required_half_width",
        requirement:
            "is not reachable at the anticipated rate within the search ceiling; narrow the claim",
    })?;
    if simulation.planned_runs < required {
        return Err(ProtocolRefusal::SimulationCountBelowPrecision {
            planned: simulation.planned_runs,
            required,
        });
    }
    Ok(())
}

/// The runtime estimate and the stated compute estimate are one decision, not
/// two numbers. Total core-hours are the run count times the per-run runtime
/// whatever the parallelism, so the two must agree; a disagreement means one of
/// them was written without the other and the budget ceiling is being checked
/// against a number nothing produced.
fn check_budget_feasibility(
    simulation: &SimulationPlan,
    budget: &Budget,
) -> Result<(), ProtocolRefusal> {
    let derived = simulation.runtime_estimate_hours();
    let tolerance = (derived * RUNTIME_ESTIMATE_TOLERANCE).max(0.01);
    if (budget.estimated_core_hours - derived).abs() > tolerance {
        return Err(ProtocolRefusal::RuntimeEstimateInconsistent {
            stated_core_hours: budget.estimated_core_hours,
            derived_core_hours: derived,
        });
    }
    Ok(())
}

fn check_stopping_rule(
    rule: &StoppingRule,
    simulation: &SimulationPlan,
) -> Result<(), ProtocolRefusal> {
    match rule {
        StoppingRule::FixedRuns { runs } => {
            if *runs != simulation.planned_runs {
                return Err(ProtocolRefusal::StoppingRuleCountMismatch {
                    stopping_runs: *runs,
                    planned: simulation.planned_runs,
                });
            }
            Ok(())
        }
        StoppingRule::Adaptive {
            error_control_analysis,
            ..
        } => {
            if error_control_analysis.trim().is_empty() {
                Err(ProtocolRefusal::AdaptiveStoppingWithoutErrorControl)
            } else {
                Ok(())
            }
        }
    }
}

fn probability(value: f64, name: &'static str) -> Result<(), ProtocolRefusal> {
    if value.is_finite() && (0.0..=1.0).contains(&value) {
        Ok(())
    } else {
        Err(ProtocolRefusal::InvalidParameter {
            name,
            requirement: "must be finite and in [0, 1]",
        })
    }
}
