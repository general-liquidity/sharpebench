//! Each refusal case fails for its own reason. Every test starts from the
//! valid fixture and breaks exactly one thing, so a test that passes for the
//! wrong reason would have to pass the equality assertion on the refusal it
//! names.

mod common;

use common::valid_protocol;
use sharpebench_study::precision::required_simulation_runs;
use sharpebench_study::protocol::*;
use sharpebench_study::refusal::ProtocolRefusal;
use sharpebench_study::validate;

fn refusal_for(protocol: &StudyProtocol) -> ProtocolRefusal {
    validate(protocol).expect_err("this protocol must be refused")
}

#[test]
fn protocol_without_a_false_positive_estimand_is_refused() {
    let mut protocol = valid_protocol();
    protocol
        .estimands
        .retain(|estimand| matches!(estimand.kind, EstimandKind::PowerAtEffect { .. }));
    protocol.claims.retain(|claim| claim.estimand == "power");

    assert_eq!(
        refusal_for(&protocol),
        ProtocolRefusal::MissingFalsePositiveEstimand
    );
}

/// The per-entry and whole-field quantities are separate estimands, so a
/// document cannot name one and decide the other on it. Reusing a name is
/// refused before it can make them look like one quantity.
#[test]
fn two_estimands_sharing_a_name_are_refused() {
    let mut protocol = valid_protocol();
    protocol.estimands[1].name = "per_entry".to_string();

    assert_eq!(
        refusal_for(&protocol),
        ProtocolRefusal::DuplicateEstimandName {
            name: "per_entry".to_string()
        }
    );
}

#[test]
fn estimand_in_other_units_than_the_protocol_is_refused() {
    let mut protocol = valid_protocol();
    protocol.estimands[0].effect_units = EffectUnits::Active;

    assert_eq!(
        refusal_for(&protocol),
        ProtocolRefusal::IncompatibleEffectUnits {
            estimand: "per_entry".to_string(),
            protocol_units: EffectUnits::Raw,
            estimand_units: EffectUnits::Active,
        }
    );
}

#[test]
fn error_rate_estimand_carrying_a_power_bound_is_refused() {
    let mut protocol = valid_protocol();
    protocol.estimands[0].target = EstimandTarget::LowerPowerBound { bound: 0.8 };

    assert_eq!(
        refusal_for(&protocol),
        ProtocolRefusal::EstimandTargetMismatch {
            estimand: "per_entry".to_string(),
            kind_tag: "per_entry_false_positive",
            target_tag: "lower_power_bound",
        }
    );
}

#[test]
fn power_estimand_without_a_named_effect_is_refused() {
    let mut protocol = valid_protocol();
    protocol.estimands[2].kind = EstimandKind::PowerAtEffect {
        effect_name: "   ".to_string(),
        effect_size: 0.5,
    };

    assert_eq!(
        refusal_for(&protocol),
        ProtocolRefusal::PowerTargetWithoutNamedEffect {
            estimand: "power".to_string()
        }
    );
}

#[test]
fn claim_on_an_undeclared_estimand_is_refused() {
    let mut protocol = valid_protocol();
    protocol.claims[0].estimand = "not_declared".to_string();

    assert_eq!(
        refusal_for(&protocol),
        ProtocolRefusal::UndeclaredEstimand {
            claim: "C1".to_string(),
            estimand: "not_declared".to_string(),
        }
    );
}

#[test]
fn claim_without_a_decision_rule_is_refused() {
    let mut protocol = valid_protocol();
    protocol.claims[0].decision_rule = DecisionRule::Unspecified;

    assert_eq!(
        refusal_for(&protocol),
        ProtocolRefusal::MissingDecisionRule {
            claim: "C1".to_string()
        }
    );
}

/// An upper-bound rule cannot decide a power estimand: it would declare
/// support exactly when power is low.
#[test]
fn decision_rule_that_cannot_decide_its_estimand_is_refused() {
    let mut protocol = valid_protocol();
    protocol.claims[1].decision_rule = DecisionRule::IntervalUpperBoundAtMost { limit: 0.5 };

    assert_eq!(
        refusal_for(&protocol),
        ProtocolRefusal::DecisionRuleEstimandMismatch {
            claim: "C2".to_string(),
            estimand: "power".to_string(),
            rule_tag: "interval_upper_bound_at_most",
            estimand_tag: "power_at_effect",
        }
    );
}

#[test]
fn protocol_with_no_claims_is_refused_outside_the_ci_tier() {
    let mut protocol = valid_protocol();
    protocol.claims.clear();

    assert_eq!(refusal_for(&protocol), ProtocolRefusal::NoClaims);
}

#[test]
fn unspecified_refusal_policy_is_refused() {
    let mut protocol = valid_protocol();
    protocol.accounting.refusal = FailurePolicy::Unspecified;

    assert_eq!(
        refusal_for(&protocol),
        ProtocolRefusal::MissingFailurePolicy { outcome: "refusal" }
    );
}

#[test]
fn unspecified_unavailability_policy_is_refused() {
    let mut protocol = valid_protocol();
    protocol.accounting.unavailability = FailurePolicy::Unspecified;

    assert_eq!(
        refusal_for(&protocol),
        ProtocolRefusal::MissingFailurePolicy {
            outcome: "unavailability"
        }
    );
}

#[test]
fn unspecified_infrastructure_failure_policy_is_refused() {
    let mut protocol = valid_protocol();
    protocol.accounting.infrastructure_failure = FailurePolicy::Unspecified;

    assert_eq!(
        refusal_for(&protocol),
        ProtocolRefusal::MissingFailurePolicy {
            outcome: "infrastructure failure"
        }
    );
}

/// More available results than completed runs is a denominator that grew
/// without a source.
#[test]
fn expected_counts_that_do_not_nest_are_refused() {
    let mut protocol = valid_protocol();
    protocol.accounting.expected_available = protocol.accounting.expected_completed + 1;

    assert_eq!(
        refusal_for(&protocol),
        ProtocolRefusal::InconsistentCounts {
            expected_attempted: 500,
            expected_completed: 490,
            expected_available: 491,
        }
    );
}

#[test]
fn unapproved_budget_is_refused() {
    let mut protocol = valid_protocol();
    protocol.budget.approved = false;

    assert_eq!(
        refusal_for(&protocol),
        ProtocolRefusal::UnapprovedBudget {
            detail: "approved is false"
        }
    );
}

#[test]
fn approved_budget_with_no_named_approver_is_refused() {
    let mut protocol = valid_protocol();
    protocol.budget.approver = "  ".to_string();

    assert_eq!(
        refusal_for(&protocol),
        ProtocolRefusal::UnapprovedBudget {
            detail: "no approver is named"
        }
    );
}

#[test]
fn budget_without_a_spend_ceiling_is_refused() {
    let mut protocol = valid_protocol();
    protocol.budget.spend_cap_usd = 0.0;

    assert_eq!(
        refusal_for(&protocol),
        ProtocolRefusal::UnapprovedBudget {
            detail: "spend_cap_usd is not a positive finite number"
        }
    );
}

#[test]
fn estimated_spend_above_its_cap_is_refused() {
    let mut protocol = valid_protocol();
    protocol.budget.estimated_spend_usd = protocol.budget.spend_cap_usd + 1.0;

    assert_eq!(
        refusal_for(&protocol),
        ProtocolRefusal::BudgetCeilingExceeded {
            quantity: "spend_usd",
            estimated: 51.0,
            cap: 50.0,
        }
    );
}

/// The planned count is checked against the count the precision requirement
/// implies, and the refusal names both, so the fix is a decision rather than a
/// guess.
#[test]
fn simulation_count_below_the_precision_requirement_is_refused() {
    let protocol = valid_protocol();
    let required = required_simulation_runs(
        protocol.simulation.anticipated_rate,
        protocol.inference.required_half_width,
        protocol.inference.confidence_level,
    )
    .expect("the requirement is reachable");

    let mut too_small = protocol;
    too_small.simulation.planned_runs = required - 1;
    too_small.stopping_rule = StoppingRule::FixedRuns { runs: required - 1 };

    assert_eq!(
        refusal_for(&too_small),
        ProtocolRefusal::SimulationCountBelowPrecision {
            planned: required - 1,
            required,
        }
    );
}

/// The runtime estimate and the compute estimate are one decision. A budget
/// checked against a compute number that the run count and per-run runtime do
/// not produce is not a feasibility decision.
#[test]
fn compute_estimate_that_the_runtime_does_not_produce_is_refused() {
    let mut protocol = valid_protocol();
    protocol.budget.estimated_core_hours = 0.1;

    assert_eq!(
        refusal_for(&protocol),
        ProtocolRefusal::RuntimeEstimateInconsistent {
            stated_core_hours: 0.1,
            derived_core_hours: protocol.simulation.runtime_estimate_hours(),
        }
    );
}

#[test]
fn fixed_stopping_rule_disagreeing_with_the_plan_is_refused() {
    let mut protocol = valid_protocol();
    protocol.stopping_rule = StoppingRule::FixedRuns { runs: 499 };

    assert_eq!(
        refusal_for(&protocol),
        ProtocolRefusal::StoppingRuleCountMismatch {
            stopping_runs: 499,
            planned: common::FIXTURE_PLANNED_RUNS,
        }
    );
}

#[test]
fn adaptive_stopping_without_error_control_is_refused() {
    let mut protocol = valid_protocol();
    protocol.stopping_rule = StoppingRule::Adaptive {
        rule: "Stop early once the interval excludes the limit.".to_string(),
        error_control_analysis: String::new(),
    };

    assert_eq!(
        refusal_for(&protocol),
        ProtocolRefusal::AdaptiveStoppingWithoutErrorControl
    );
}

#[test]
fn adaptive_stopping_with_error_control_is_accepted() {
    let mut protocol = valid_protocol();
    protocol.stopping_rule = StoppingRule::Adaptive {
        rule: "Two interim looks at a third and two thirds of the planned runs.".to_string(),
        error_control_analysis: "O'Brien-Fleming spending over two looks, recomputed for this \
            design and reported with the protocol."
            .to_string(),
    };

    assert_eq!(validate(&protocol), Ok(()));
}

#[test]
fn independent_unit_nested_inside_itself_is_refused() {
    let mut protocol = valid_protocol();
    protocol
        .replication
        .nested_observations
        .push(ReplicationUnit::SimulatedField);

    assert_eq!(
        refusal_for(&protocol),
        ProtocolRefusal::PseudoreplicationDeclared {
            detail: "the declared independent unit is also declared nested inside itself",
        }
    );
}

/// Entrants inside one market history are dependent observations. Counting
/// them as independent replications is the denominator error the plan calls
/// out, so it is a refusal rather than a note.
#[test]
fn entrant_as_independent_unit_under_a_shared_history_is_refused() {
    let mut protocol = valid_protocol();
    protocol.replication.independent_unit = ReplicationUnit::Entrant;
    protocol
        .replication
        .nested_observations
        .retain(|unit| *unit != ReplicationUnit::Entrant);

    assert!(matches!(
        refusal_for(&protocol),
        ProtocolRefusal::PseudoreplicationDeclared { .. }
    ));
}

#[test]
fn several_reported_conditions_with_no_multiplicity_method_are_refused() {
    let mut protocol = valid_protocol();
    protocol.inference.multiplicity.method = MultiplicityMethod::None;

    assert_eq!(
        refusal_for(&protocol),
        ProtocolRefusal::MultiplicityNotPredeclared {
            detail: "more than one reported condition with no multiplicity method",
        }
    );
}

#[test]
fn no_predeclared_reported_conditions_are_refused() {
    let mut protocol = valid_protocol();
    protocol.inference.multiplicity.reported_conditions.clear();

    assert_eq!(
        refusal_for(&protocol),
        ProtocolRefusal::MultiplicityNotPredeclared {
            detail: "no reported conditions are listed",
        }
    );
}

#[test]
fn unsupported_contract_version_is_refused() {
    let mut protocol = valid_protocol();
    protocol.contract_version = "sharpebench/study-protocol/v99".to_string();

    assert_eq!(
        refusal_for(&protocol),
        ProtocolRefusal::UnsupportedContractVersion {
            found: "sharpebench/study-protocol/v99".to_string(),
            supported: STUDY_CONTRACT_VERSION,
        }
    );
}
