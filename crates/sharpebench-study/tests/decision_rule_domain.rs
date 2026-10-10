mod common;

use sharpebench_study::protocol::DecisionRule;
use sharpebench_study::{validate, ProtocolRefusal, ReportRefusal, StudyReport};

#[test]
fn decision_thresholds_must_be_finite_probabilities() {
    for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, -0.01, 1.01] {
        for (index, rule, name, estimand) in [
            (
                0,
                DecisionRule::IntervalUpperBoundAtMost { limit: value },
                "decision rule limit",
                "per_entry",
            ),
            (
                1,
                DecisionRule::IntervalLowerBoundAtLeast { bound: value },
                "decision rule bound",
                "power",
            ),
        ] {
            let mut protocol = common::valid_protocol();
            protocol.claims[index].decision_rule = rule;
            let expected = ProtocolRefusal::InvalidParameter {
                name,
                requirement: "must be finite and in [0, 1]",
            };
            assert_eq!(validate(&protocol), Err(expected.clone()), "rule {rule:?}");
            assert_eq!(
                StudyReport::from_realized_runs(&protocol, estimand, 20, 500),
                Err(ReportRefusal::Protocol(Box::new(expected))),
                "invalid threshold cannot become an ordinary claim outcome"
            );
        }
    }
}

#[test]
fn vacuous_rules_cannot_emit_supported_reports() {
    for (index, rule, name, estimand) in [
        (
            0,
            DecisionRule::IntervalUpperBoundAtMost {
                limit: f64::INFINITY,
            },
            "decision rule limit",
            "per_entry",
        ),
        (
            1,
            DecisionRule::IntervalLowerBoundAtLeast {
                bound: f64::NEG_INFINITY,
            },
            "decision rule bound",
            "power",
        ),
    ] {
        let mut protocol = common::valid_protocol();
        protocol.claims[index].decision_rule = rule;
        assert_eq!(
            StudyReport::from_realized_runs(&protocol, estimand, 20, 500),
            Err(ReportRefusal::Protocol(Box::new(
                ProtocolRefusal::InvalidParameter {
                    name,
                    requirement: "must be finite and in [0, 1]",
                }
            )))
        );
    }
}

#[test]
fn finite_out_of_range_json_rules_are_refused_after_loading() {
    for (index, field, value, name) in [
        (0, "limit", 1.01, "decision rule limit"),
        (1, "bound", -0.01, "decision rule bound"),
    ] {
        let mut document = serde_json::to_value(common::valid_protocol()).unwrap();
        document["claims"][index]["decision_rule"][field] = serde_json::json!(value);
        let protocol = sharpebench_study::StudyProtocol::from_json(&document.to_string()).unwrap();
        assert_eq!(
            validate(&protocol),
            Err(ProtocolRefusal::InvalidParameter {
                name,
                requirement: "must be finite and in [0, 1]",
            })
        );
    }
}

#[test]
fn both_probability_endpoints_and_interior_rules_remain_valid() {
    for value in [0.0, 0.5, 1.0] {
        let mut protocol = common::valid_protocol();
        protocol.claims[0].decision_rule = DecisionRule::IntervalUpperBoundAtMost { limit: value };
        protocol.claims[1].decision_rule = DecisionRule::IntervalLowerBoundAtLeast { bound: value };
        validate(&protocol).unwrap();
        StudyReport::from_realized_runs(&protocol, "per_entry", 20, 500).unwrap();
        StudyReport::from_realized_runs(&protocol, "power", 20, 500).unwrap();
    }
}

#[test]
fn published_decision_schema_has_the_same_probability_domain() {
    let schema: serde_json::Value =
        serde_json::from_str(include_str!("../schema/study-protocol.schema.json")).unwrap();
    for (variant, field) in [(0, "limit"), (1, "bound")] {
        let property = &schema["$defs"]["DecisionRule"]["oneOf"][variant]["properties"][field];
        assert_eq!(property["type"], "number");
        assert_eq!(property["minimum"], 0);
        assert_eq!(property["maximum"], 1);
    }
}
