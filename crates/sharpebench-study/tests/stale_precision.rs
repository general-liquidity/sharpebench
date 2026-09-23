//! The stale-precision guarantee, asserted rather than promised.
//!
//! The failure this rules out: a protocol plans N runs, the study executes
//! fewer, and the write-up keeps the precision the planned N would have
//! bought. Three properties together make that unrepresentable, and each is
//! checked here.
//!
//! 1. The configuration cannot carry a precision claim. No property of a
//!    serialized protocol holds an achieved interval, an observed rate or a
//!    reached half width, at any depth, and the contract is closed, so one
//!    cannot be added by a document either.
//! 2. A precision claim is a function of the count it was built from. For
//!    every realized count, the reported half width equals the interval at
//!    that count, never the interval at the planned count.
//! 3. A realized count that misses the requirement produces no report. The
//!    refusal names both counts and both half widths.

mod common;

use common::valid_protocol;
use sharpebench_study::precision::PrecisionClaim;
use sharpebench_study::protocol::ConfidenceLevel;
use sharpebench_study::refusal::ReportRefusal;
use sharpebench_study::{wilson_interval, StudyReport};

/// Names a result-bearing field would plausibly take. A protocol that gains
/// one of these has gained a place to keep a stale claim.
const RESULT_BEARING_NAMES: &[&str] = &[
    "achieved_half_width",
    "achieved",
    "observed_rate",
    "observed_events",
    "realized_runs",
    "precision",
    "precision_claim",
    "interval",
    "lower",
    "upper",
    "result",
    "measured_rate",
];

fn collect_keys(value: &serde_json::Value, into: &mut Vec<String>) {
    match value {
        serde_json::Value::Object(map) => {
            for (key, child) in map {
                into.push(key.clone());
                collect_keys(child, into);
            }
        }
        serde_json::Value::Array(items) => {
            for item in items {
                collect_keys(item, into);
            }
        }
        _ => {}
    }
}

/// Property 1. The configuration has nowhere to put a result.
#[test]
fn a_protocol_has_no_result_bearing_field_at_any_depth() {
    let value = serde_json::to_value(valid_protocol()).expect("serializes");
    let mut keys = Vec::new();
    collect_keys(&value, &mut keys);

    for name in RESULT_BEARING_NAMES {
        assert!(
            !keys.iter().any(|key| key == name),
            "a study protocol must not carry a result: found the key {name}"
        );
    }
    assert!(
        keys.iter().any(|key| key == "required_half_width"),
        "the protocol should still state the precision it requires"
    );
}

/// Property 2. Reducing the count widens the reported interval, run for run.
/// If the claim ever carried the planned count's precision, one of these
/// comparisons would fail.
#[test]
fn a_precision_claim_always_reports_the_interval_of_its_own_count() {
    let level = ConfidenceLevel::NinetyFive;
    let planned = 500u64;
    let events = 25u64;

    let planned_claim = PrecisionClaim::from_realized(events, planned, level).expect("computable");

    for realized in [events, 30, 60, 120, 250, 499, planned] {
        let claim = PrecisionClaim::from_realized(events, realized, level).expect("computable");
        let expected = wilson_interval(events, realized, level).expect("computable");

        assert_eq!(claim.realized_runs(), realized);
        assert_eq!(claim.interval(), expected);
        assert_eq!(claim.observed_rate(), events as f64 / realized as f64);
        if realized < planned {
            assert!(
                claim.half_width() > planned_claim.half_width(),
                "{realized} runs must report a wider interval than {planned}: {} vs {}",
                claim.half_width(),
                planned_claim.half_width()
            );
        }
    }
}

/// Property 3. A short run produces a refusal, not a report carrying the
/// planned precision.
#[test]
fn a_short_run_produces_no_report_at_all() {
    let protocol = valid_protocol();
    let planned = protocol.simulation.planned_runs;
    let realized = planned / 5;
    let events = realized / 20;

    let refusal = StudyReport::from_realized_runs(&protocol, "per_entry", events, realized)
        .expect_err("a run this short cannot meet the required precision");

    match refusal {
        ReportRefusal::PrecisionTargetUnmet {
            required_half_width,
            achieved_half_width,
            planned_runs,
            realized_runs,
        } => {
            assert_eq!(required_half_width, protocol.inference.required_half_width);
            assert_eq!(planned_runs, planned);
            assert_eq!(realized_runs, realized);
            assert!(achieved_half_width > required_half_width);
            let expected = wilson_interval(events, realized, protocol.inference.confidence_level)
                .expect("computable")
                .half_width();
            assert_eq!(achieved_half_width, expected);
        }
        other => panic!("expected a precision-target refusal, got {other}"),
    }
}

#[test]
fn a_report_on_the_full_run_carries_the_realized_count_and_its_own_interval() {
    let protocol = valid_protocol();
    let planned = protocol.simulation.planned_runs;

    let report = StudyReport::from_realized_runs(&protocol, "per_entry", 20, planned)
        .expect("the planned count meets its own requirement");

    assert_eq!(report.precision.realized_runs(), planned);
    assert_eq!(report.planned_runs, planned);
    assert_eq!(
        report.precision.interval(),
        wilson_interval(20, planned, protocol.inference.confidence_level).expect("computable")
    );
    assert!(report.precision.half_width() <= protocol.inference.required_half_width);
}

/// The serialized report states the count its interval came from, so a reader
/// of the artifact alone can check the two against each other.
#[test]
fn a_serialized_report_states_the_count_behind_its_interval() {
    let protocol = valid_protocol();
    let planned = protocol.simulation.planned_runs;
    let report = StudyReport::from_realized_runs(&protocol, "per_entry", 20, planned)
        .expect("the planned count meets its own requirement");

    let value = serde_json::to_value(&report).expect("serializes");
    assert_eq!(
        value["precision"]["realized_runs"],
        serde_json::json!(planned)
    );
    assert_eq!(value["precision"]["observed_events"], serde_json::json!(20));
}

#[test]
fn a_report_on_an_undeclared_estimand_is_refused() {
    let protocol = valid_protocol();
    assert_eq!(
        StudyReport::from_realized_runs(&protocol, "not_declared", 0, 500),
        Err(ReportRefusal::UndeclaredEstimand {
            estimand: "not_declared".to_string()
        })
    );
}
