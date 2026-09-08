//! The producer-contract half of BP6: an unknown dataset selector must not be
//! publishable as a complete but empty field.
//!
//! `examples/llm_field_eval.rs`, `examples/evidence_sweep.rs` and
//! `examples/risk_managed_eval.rs` each accepted support that was never
//! evaluated: a misspelled selector skipped every dataset, and a failed dataset
//! load skipped one, both reaching the ordinary final filename and a zero exit.
//! The checks now live in `examples/support/declared_support.rs`, which the
//! three producers call before they open an output and again before they
//! publish it. This is the same file the examples compile, included by path, so
//! a producer cannot drift from what is asserted here without the shared module
//! changing under it.
//!
//! Structural support only: these assertions establish that the declared cells
//! were evaluated, never that the scores in them are right.

#[path = "../examples/support/declared_support.rs"]
mod declared_support;

use declared_support::{
    require_declared_member, require_evaluated, require_grid, resolve_numeric_selector,
    resolve_selector, SupportError,
};

/// The nine frozen datasets of `evidence_sweep` and `risk_managed_eval`.
const DATASETS: &[&str] = &[
    "us-indices-1d",
    "us-indices-1w",
    "crypto-majors-1h",
    "crypto-majors-4h",
    "crypto-majors-1d",
    "crypto-majors-1w",
    "fx-majors-1d",
    "commodities-1d",
    "rates-1d",
];
/// The two datasets `llm_field_eval` scopes its paid field to.
const LLM_DATASETS: &[&str] = &["us-indices-1d", "crypto-majors-1d"];
const DSR_BARS: &[f64] = &[0.80, 0.90, 0.95, 0.99];

fn owned(names: &[&str]) -> Vec<String> {
    names.iter().map(|n| (*n).to_string()).collect()
}

/// The finding's case: a misspelled selector used to skip every dataset and
/// reach the publication path with zero records.
#[test]
fn unknown_dataset_selector_is_refused_before_any_output() {
    for declared in [DATASETS, LLM_DATASETS] {
        let err = resolve_selector("dataset", declared, Some("us-indicies-1d"))
            .expect_err("a misspelled selector must not resolve to an empty plan");
        assert_eq!(
            err,
            SupportError::UnknownSelector {
                kind: "dataset",
                requested: "us-indicies-1d".to_string(),
                declared: owned(declared),
            }
        );
    }
}

/// A refusal has to name the unknown value and list the valid ones, or the
/// operator learns nothing an empty file did not already tell them.
#[test]
fn unknown_selector_refusal_names_the_value_and_the_declared_set() {
    let message = resolve_selector("dataset", LLM_DATASETS, Some("crypto-majors-1h"))
        .expect_err("not one of the two LLM-field datasets")
        .to_string();
    assert!(message.contains("\"crypto-majors-1h\""), "{message}");
    for name in LLM_DATASETS {
        assert!(message.contains(name), "{message}");
    }
    // The nine-dataset table is a different declared set; naming a member of it
    // must not be enough to make the message look like a match.
    assert!(!message.contains("us-indices-1w"), "{message}");
}

#[test]
fn a_declared_selector_plans_exactly_itself_and_no_selector_plans_everything() {
    assert_eq!(
        resolve_selector("dataset", DATASETS, Some("rates-1d")).unwrap(),
        vec!["rates-1d".to_string()]
    );
    assert_eq!(
        resolve_selector("dataset", DATASETS, None).unwrap(),
        owned(DATASETS)
    );
}

/// `evidence_sweep`'s third argument shards the grid by DSR bar. A value off
/// the declared axis matched no cell and produced an empty shard.
#[test]
fn unknown_dsr_bar_selector_is_refused() {
    let err = resolve_numeric_selector("dsr_bar", DSR_BARS, Some(0.85))
        .expect_err("0.85 is not one of the four declared bars");
    let message = err.to_string();
    assert!(message.contains("0.85"), "{message}");
    assert!(message.contains("0.99"), "{message}");
    assert!(matches!(err, SupportError::UnknownSelector { .. }));

    assert_eq!(
        resolve_numeric_selector("dsr_bar", DSR_BARS, Some(0.95)).unwrap(),
        vec![0.95]
    );
    assert_eq!(
        resolve_numeric_selector("dsr_bar", DSR_BARS, None).unwrap(),
        DSR_BARS.to_vec()
    );
}

/// The bar selector is compared with the same tolerance the sweep filters
/// cells with, so a resolvable selector always selects a cell.
#[test]
fn dsr_bar_selector_resolves_within_the_sweep_filter_tolerance() {
    let resolved = resolve_numeric_selector("dsr_bar", DSR_BARS, Some(0.8)).unwrap();
    assert_eq!(resolved, vec![0.80]);
    assert!(DSR_BARS.iter().any(|bar| (bar - resolved[0]).abs() <= 1e-9));
}

/// `risk_managed_eval` names its N-sensitivity anchor and its perturbation
/// dataset as constants. A value outside the table became an absent section.
#[test]
fn a_named_support_constant_must_be_a_declared_dataset() {
    assert!(require_declared_member("perturbation dataset", DATASETS, "us-indices-1w").is_ok());
    let err = require_declared_member("perturbation dataset", DATASETS, "us-indices-1m")
        .expect_err("a dataset outside the table is missing support, not a section to skip");
    let message = err.to_string();
    assert!(message.contains("perturbation dataset"), "{message}");
    assert!(message.contains("us-indices-1m"), "{message}");
}

/// The publication check: every planned dataset must have contributed records.
/// A dataset whose CSV failed to load used to warn and leave the rest of the
/// file to be published under a name claiming the full sweep.
#[test]
fn planned_support_that_produced_no_records_is_refused() {
    let planned = owned(DATASETS);
    let evaluated: Vec<String> = planned
        .iter()
        .filter(|d| *d != "commodities-1d" && *d != "rates-1d")
        .cloned()
        .collect();
    let err = require_evaluated("dataset", &planned, &evaluated)
        .expect_err("two planned datasets contributed nothing");
    assert_eq!(
        err,
        SupportError::MissingSupport {
            kind: "dataset",
            missing: vec!["commodities-1d".to_string(), "rates-1d".to_string()],
        }
    );
    let message = err.to_string();
    assert!(message.contains("commodities-1d"), "{message}");
    assert!(message.contains("rates-1d"), "{message}");
}

#[test]
fn fully_evaluated_planned_support_publishes() {
    let planned = owned(DATASETS);
    assert!(require_evaluated("dataset", &planned, &planned).is_ok());
    // The empty plan is unreachable from a resolved selector, and an empty plan
    // is exactly the state the finding describes, so it must not read as OK
    // support for a non-empty one.
    assert!(require_evaluated("dataset", &planned, &[]).is_err());
}

/// The count check is against the declared grid, not against zero: 9 datasets
/// times 4 bars times 4 trial counts times 4 dispersion priors times the
/// 8-agent field is the 4608-record sweep, 512 per dataset.
#[test]
fn a_short_grid_is_refused_and_the_declared_grid_is_accepted() {
    let expected = DATASETS.len() * DSR_BARS.len() * 4 * 4 * 8;
    assert_eq!(expected, 4608);
    assert!(require_grid(expected, expected).is_ok());
    let err =
        require_grid(expected, expected - 512).expect_err("one dataset short of the declared grid");
    assert_eq!(
        err,
        SupportError::IncompleteGrid {
            expected,
            produced: expected - 512,
        }
    );
    assert!(require_grid(expected, 0).is_err());
}
