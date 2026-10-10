mod common;

use common::valid_protocol;
use sharpebench_study::protocol::{DecisionRule, RunTier};
use sharpebench_study::{validate, ReportRefusal, StudyProtocol, StudyReport};

fn assert_report_refuses(protocol: StudyProtocol) {
    let refusal = validate(&protocol).expect_err("fixture violates the existing contract");
    let report = StudyReport::from_realized_runs(&protocol, "per_entry", 20, 500);
    assert_eq!(report, Err(ReportRefusal::Protocol(Box::new(refusal))));
}

#[test]
fn invalid_precision_targets_cannot_produce_reports() {
    for target in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, 0.0, -1.0, 1.0] {
        let mut protocol = valid_protocol();
        protocol.inference.required_half_width = target;
        assert_report_refuses(protocol);
    }
}

#[test]
fn regression_tier_cannot_report_population_claims() {
    let mut protocol = valid_protocol();
    protocol.tier = RunTier::CiRegression;
    assert_report_refuses(protocol);
}

#[test]
fn report_rechecks_previously_validated_protocols() {
    let mut protocol = valid_protocol();
    validate(&protocol).unwrap();
    protocol.budget.approved = false;
    assert_report_refuses(protocol);
}

#[test]
fn unspecified_claim_rule_cannot_produce_an_ordinary_report() {
    let mut protocol = valid_protocol();
    protocol.claims[0].decision_rule = DecisionRule::Unspecified;
    assert_report_refuses(protocol);
}

#[test]
fn report_refusal_preserves_the_validator_error_source() {
    use std::error::Error;
    let mut protocol = valid_protocol();
    protocol.budget.approved = false;
    let original = validate(&protocol).unwrap_err();
    let error = StudyReport::from_realized_runs(&protocol, "per_entry", 20, 500).unwrap_err();
    assert_eq!(error.source().unwrap().to_string(), original.to_string());
    assert_eq!(
        error.to_string(),
        format!("invalid study protocol: {original}")
    );
}
