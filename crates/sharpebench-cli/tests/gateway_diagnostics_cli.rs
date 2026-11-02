//! `sharpebench gateway-diagnostics` through the real binary, on a journal
//! written by the harness's own journal API.

use std::process::Command;

use serde_json::Value;
use sharpebench_harness::accounting::RateCard;
use sharpebench_harness::gateway_journal::{
    DecisionEntry, DecisionLog, GatewayBudget, GatewayJournal, JournalIdentity, ReleaseReason,
    Settlement,
};

fn cli(args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_sharpebench"))
        .args(args)
        .output()
        .expect("the CLI runs")
}

fn card() -> RateCard {
    RateCard::from_json(
        br#"{"schema_version":"sharpebench.token-rate-card.v1","provider":"fake","model":"fake-1","revision":"2026-01-01","input_usd_nanos_per_token":1,"output_usd_nanos_per_token":1}"#,
    )
    .expect("a valid test rate card")
}

/// One decision: a 429 retried and priced at (7, 3) tokens, and a second
/// request refused by the per-decision limit.
fn write_journal(dir: &std::path::Path) -> std::path::PathBuf {
    let mut journal = GatewayJournal::new(
        JournalIdentity::new(
            "r".repeat(64),
            GatewayBudget {
                max_usd_nanos: 1_000_000,
                max_calls: 10,
                per_cell: None,
            },
        )
        .for_sweep("s".repeat(64)),
    );
    let refused = journal.reserve("fake.v1", &card(), 100);
    journal.settle(
        refused,
        Settlement::Released {
            reason: ReleaseReason::ProviderRefusedBeforeWork,
        },
    );
    let priced = journal.reserve("fake.v1", &card(), 100);
    journal.settle(
        priced,
        Settlement::Priced {
            input_tokens: 7,
            output_tokens: 3,
            usd_nanos: "10".into(),
            cache_read_input_tokens: None,
            cache_write_input_tokens: None,
        },
    );
    journal.log_decisions(DecisionLog {
        window_index: 0,
        seed: 0,
        attempt: 0,
        decisions: vec![DecisionEntry {
            requests: 2,
            served: 1,
            first_ordinal: 0,
            calls: 2,
        }],
    });
    let path = dir.join("journal.json");
    journal.save(&path).expect("the journal saves");
    path
}

fn scratch(tag: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("sb-gwdiag-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("a scratch dir");
    dir
}

#[test]
fn the_report_counts_one_decisions_retry_limit_refusal_and_tokens() {
    let dir = scratch("json");
    let path = write_journal(&dir);
    let output = cli(&[
        "gateway-diagnostics",
        "--journal",
        path.to_str().unwrap(),
        "--json",
    ]);
    assert_eq!(
        output.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let d: Value = serde_json::from_slice(&output.stdout).expect("JSON");
    assert_eq!(d["schema_version"], "sharpebench.gateway-diagnostics.v1");
    assert_eq!(d["sweep_sha256"], "s".repeat(64));
    assert_eq!(d["rank_neutral"], true);
    assert_eq!(
        (d["decisions"].as_u64(), d["calls"].as_u64()),
        (Some(1), Some(2))
    );
    assert_eq!(d["refused_by_limit"], 1);
    assert_eq!(d["faults"]["provider_refused_before_work"], 1);
    assert_eq!(
        (
            d["retries_at_least"].as_u64(),
            d["retries_at_most"].as_u64()
        ),
        (Some(1), Some(1))
    );
    assert_eq!(d["per_decision"][0]["input_tokens"], 7);
    assert_eq!(d["tokens_per_decision"]["output_mean"], 3.0);
    assert_eq!(d["requests_per_decision"]["2"], 1);
    assert_eq!(d["by_alias"][0]["alias"], "fake.v1");

    let text = cli(&["gateway-diagnostics", "--journal", path.to_str().unwrap()]);
    assert_eq!(text.status.code(), Some(0));
    let text = String::from_utf8_lossy(&text.stdout);
    assert!(text.contains("rank-neutral"), "{text}");
    assert!(text.contains("retries 1..1"), "{text}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_missing_flag_is_a_usage_error_and_an_unreadable_journal_fails() {
    assert_eq!(cli(&["gateway-diagnostics"]).status.code(), Some(2));
    assert_eq!(
        cli(&["gateway-diagnostics", "--journal", "--json"])
            .status
            .code(),
        Some(2)
    );
    let dir = scratch("bad");
    let bad = dir.join("journal.json");
    std::fs::write(&bad, b"{\"not\":\"a journal\"}").unwrap();
    let output = cli(&["gateway-diagnostics", "--journal", bad.to_str().unwrap()]);
    assert_eq!(output.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&output.stderr).contains("cannot read gateway journal"));
    let missing = dir.join("absent.json");
    assert_eq!(
        cli(&[
            "gateway-diagnostics",
            "--journal",
            missing.to_str().unwrap()
        ])
        .status
        .code(),
        Some(1)
    );
    let _ = std::fs::remove_dir_all(&dir);
}
