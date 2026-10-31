//! A ledger SharpeArena wrote, checked by the independent duplicate rule.
//!
//! `fixtures/sharpearena-duplicate-ledger.json` comes from Arena's
//! `parse_generated_pool` over three proposals. Row 2 repeats row 0's strategy
//! in different words: the window is omitted, the constant and `gross_target`
//! are integers, and `short_when` is null. Arena's validator marks it a
//! duplicate, so a verifier comparing raw JSON would refuse real evidence.

use sharpebench_core::candidate_lineage::{
    verify_candidate_lineage, CandidateLineageLedger, CandidateLineageScore, NEWEST_LEDGER_SCHEMA,
};

fn arena_ledger() -> CandidateLineageLedger {
    serde_json::from_str(include_str!("fixtures/sharpearena-duplicate-ledger.json"))
        .expect("the fixture is an Arena ledger")
}

fn scores() -> Vec<CandidateLineageScore> {
    ["base", "other"]
        .map(|id| CandidateLineageScore {
            candidate_id: id.to_owned(),
            median_deflated_sharpe: 0.1,
        })
        .to_vec()
}

#[test]
fn an_arena_duplicate_written_differently_verifies() {
    let ledger = arena_ledger();
    assert_eq!(ledger.summary.schema_version, NEWEST_LEDGER_SCHEMA);
    assert_eq!(ledger.records[2].duplicate_of_ordinal, Some(0));
    assert_ne!(
        ledger.records[0].raw_candidate["long_when"], ledger.records[2].raw_candidate["long_when"],
        "the fixture must exercise the normal form, not byte equality"
    );

    let report = verify_candidate_lineage(&ledger, &scores()).expect("Arena's ledger verifies");
    assert_eq!(report.duplicate_trials, 1);
    assert_eq!(report.scored_candidates, 2);
}

#[test]
fn an_arena_row_pointed_at_a_different_candidate_is_refused() {
    let mut ledger = arena_ledger();
    ledger.records[2].duplicate_of_ordinal = Some(1);
    let error = verify_candidate_lineage(&ledger, &scores()).expect_err("row 1 differs");
    assert_eq!(error.path, "records[2].duplicate_of_ordinal");
}
