//! `sharpebench lineage`: the single-record report and the `--census` mode.
//!
//! The two `sharpearena-lineage-v2.report.*` goldens were captured from the
//! binary built at 016d20c, before the census and source-dating work. A
//! single schema 2 record must still produce those bytes. The census journal
//! and price panel were written by the SharpeArena schema 3 producer.

use std::path::PathBuf;
use std::process::{Command, Output};

fn fixture(name: &str) -> String {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
        .to_str()
        .expect("fixture path is UTF-8")
        .to_owned()
}

fn lineage(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_sharpebench"))
        .arg("lineage")
        .args(args)
        .output()
        .expect("run sharpebench lineage")
}

fn golden(name: &str) -> Vec<u8> {
    // Normalize a checkout that converted the golden's line endings.
    std::fs::read_to_string(fixture(name))
        .expect("read golden")
        .replace("\r\n", "\n")
        .into_bytes()
}

#[test]
fn a_single_schema_2_record_report_is_byte_identical() {
    let evidence = fixture("sharpearena-lineage-v2.json");
    let text = lineage(&[&evidence]);
    assert!(text.status.success(), "{text:?}");
    assert_eq!(text.stdout, golden("sharpearena-lineage-v2.report.txt"));
    let json = lineage(&[&evidence, "--json"]);
    assert!(json.status.success(), "{json:?}");
    assert_eq!(json.stdout, golden("sharpearena-lineage-v2.report.json"));
}

#[test]
fn a_multi_record_journal_is_refused_without_census_and_counted_with_it() {
    let journal = fixture("sharpearena-census-journal.jsonl");
    let refused = lineage(&[&journal]);
    assert_eq!(refused.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&refused.stderr);
    assert!(stderr.contains("5 JSONL records"), "{stderr}");

    let census = lineage(&[&journal, "--census", "--json"]);
    assert!(census.status.success(), "{census:?}");
    let report: serde_json::Value = serde_json::from_slice(&census.stdout).unwrap();
    let mut splits: Vec<(u64, u64)> = report["splits"]
        .as_array()
        .unwrap()
        .iter()
        .map(|split| {
            (
                split["test_consultations"].as_u64().unwrap(),
                split["cumulative_observed_n_trials"].as_u64().unwrap(),
            )
        })
        .collect();
    splits.sort_unstable();
    assert_eq!(splits, [(1, 4), (3, 12)]);
    assert_eq!(report["declared_censuses_verified"], 4);
    // Without the dataset the test split's first date is unavailable, not clean.
    assert_eq!(
        report["journal"][2]["lineage"]["source_dating"]["splits"][1],
        serde_json::json!({"split": "test", "status": "unavailable", "reason": "dataset_not_supplied"})
    );

    let text = lineage(&[&journal, "--census"]);
    assert!(text.status.success(), "{text:?}");
    let stdout = String::from_utf8_lossy(&text.stdout);
    assert!(
        stdout.contains("consultations=3 cumulative_trials=12"),
        "{stdout}"
    );
    assert!(stdout.contains("other journal files"), "{stdout}");
}

#[test]
fn a_supplied_dataset_dates_the_cited_sources_against_the_test_split() {
    let journal = fixture("sharpearena-census-journal.jsonl");
    let prices = fixture("sharpearena-census-prices.csv");
    let census = lineage(&[&journal, "--census", "--dataset", &prices, "--json"]);
    assert!(census.status.success(), "{census:?}");
    let report: serde_json::Value = serde_json::from_slice(&census.stdout).unwrap();
    let dating = &report["journal"][2]["lineage"]["source_dating"];
    assert_eq!(dating["undated_sources"], 1);
    assert_eq!(
        dating["splits"][1],
        serde_json::json!({
            "split": "test",
            "status": "measured",
            "first_date": "2025-01-11",
            "sources_on_or_after_first_date": 1
        })
    );

    let single = std::env::temp_dir().join(format!(
        "sharpebench-lineage-single-{}.jsonl",
        std::process::id()
    ));
    let third = std::fs::read_to_string(&journal)
        .unwrap()
        .lines()
        .nth(2)
        .unwrap()
        .to_owned();
    std::fs::write(&single, format!("{third}\n")).unwrap();
    let text = lineage(&[single.to_str().unwrap(), "--dataset", &prices]);
    std::fs::remove_file(&single).unwrap();
    assert!(text.status.success(), "{text:?}");
    let stdout = String::from_utf8_lossy(&text.stdout);
    assert!(
        stdout.contains("test       first_date=2025-01-11 sources_on_or_after_first_date=1"),
        "{stdout}"
    );
    assert!(
        stdout.contains("prior_consultations=1 cumulative_trials=8"),
        "{stdout}"
    );
}
