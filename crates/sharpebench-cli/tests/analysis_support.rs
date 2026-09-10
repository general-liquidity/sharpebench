use std::path::PathBuf;
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};

struct Fixture(PathBuf);

impl Fixture {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        loop {
            let path = std::env::temp_dir().join(format!(
                "sharpebench-analysis-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            match std::fs::create_dir(&path) {
                Ok(()) => return Self(path),
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(e) => panic!("create fixture: {e}"),
            }
        }
    }

    fn write(&self, name: &str, text: &str) {
        std::fs::write(self.0.join(name), text).unwrap();
    }

    fn cli(&self, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_sharpebench"))
            .current_dir(&self.0)
            .args(args)
            .arg("--json")
            .output()
            .unwrap()
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.0).expect("remove exclusively owned fixture");
    }
}

fn refused(output: Output, message: &str) {
    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty(), "no partial report on failure");
    assert!(
        String::from_utf8_lossy(&output.stderr).contains(message),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn regime_cli_requires_complete_support_and_independent_column_selection() {
    let fixture = Fixture::new();
    fixture.write("a.csv", "period,ret\nt0,0.1\nt1,0.2\n");
    fixture.write("b.csv", "period,ret\nt0,0.05\nt1,0.15\n");
    fixture.write("r.csv", "period,state\nt0,calm\nt1,stress\n");
    let args = [
        "regime",
        "a.csv",
        "b.csv",
        "r.csv",
        "--col",
        "ret",
        "--regime-col",
        "state",
        "--period-col",
        "period",
    ];
    let output = fixture.cli(&args);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert!((report["pooled_mean_gap"].as_f64().unwrap() - 0.05).abs() < 1e-12);
    fixture.write("r.csv", "period,state\nt1,stress\nt0,calm\n");
    refused(fixture.cli(&args), "period identities");
    fixture.write("r.csv", "period,state\nt0,calm\n");
    refused(fixture.cli(&args), "aligned support required");
    fixture.write("r.csv", "period,other\nt0,calm\nt1,stress\n");
    refused(fixture.cli(&args), "column `state` not found");
    for flag in ["--col", "--regime-col", "--period-col"] {
        let output = fixture.cli(&["regime", "a.csv", "b.csv", "r.csv", flag]);
        assert_eq!(output.status.code(), Some(2));
        assert!(output.stdout.is_empty());
        assert!(String::from_utf8_lossy(&output.stderr).contains("requires a column name"));
    }
}

#[test]
fn analysis_commands_reject_independently_missing_observations() {
    let fixture = Fixture::new();
    fixture.write("ragged.csv", "a,b\n1,2\n,3\n4,\n");
    refused(fixture.cli(&["select", "ragged.csv"]), "empty cell");
    fixture.write("blank.csv", "return\n0.1\n\n0.2\n");
    refused(
        fixture.cli(&["check", "blank.csv", "--trials", "1"]),
        "blank observation",
    );
    refused(
        fixture.cli(&["uncertainty", "blank.csv"]),
        "blank observation",
    );
}

/// F-A: `disqualify` prints its reasons under a header promising that the hard
/// gates mirror the scorer, so a reason the scorer does not gate on has to carry
/// the advisory marker. A refusing candidate set leaves the track itself
/// eligible, and an unmarked `SelectionUnavailable` claimed a disqualification
/// the board had not applied.
#[test]
fn a_refusing_candidate_set_is_marked_advisory_next_to_an_eligible_verdict() {
    let fixture = Fixture::new();
    let track: Vec<f64> = (0..60)
        .map(|i| 0.002 + 0.0005 * (i as f64 * 0.7).sin())
        .collect();
    let field = serde_json::json!([{
        "agent_id": "refusing-candidates",
        "runs": [{"returns": track}],
        "candidates": [[1e308, 1e308, 1e308]],
    }]);
    fixture.write("field.json", &field.to_string());

    let json = fixture.cli(&["disqualify", "field.json"]);
    assert_eq!(json.status.code(), Some(0));
    let report: Vec<serde_json::Value> = serde_json::from_slice(&json.stdout).unwrap();
    assert_eq!(report[0]["rank_eligible"], true);
    assert_eq!(
        report[0]["reasons"],
        serde_json::json!(["selection_unavailable"])
    );

    let text = Command::new(env!("CARGO_BIN_EXE_sharpebench"))
        .current_dir(&fixture.0)
        .args(["disqualify", "field.json"])
        .output()
        .unwrap();
    assert_eq!(text.status.code(), Some(0));
    let printed = String::from_utf8_lossy(&text.stdout).into_owned();
    assert!(
        printed.contains("SelectionUnavailable (advisory)"),
        "a reason the scorer does not gate on must be marked: {printed}"
    );
    assert!(printed.contains("rank-eligible=true"), "{printed}");
}

#[test]
fn score_and_disqualify_use_the_same_relative_field_and_cli_controls() {
    let fixture = Fixture::new();
    let track = |mean: f64| {
        (0..60)
            .map(|i| mean + 0.001 * (i as f64 * 0.7).sin())
            .collect::<Vec<_>>()
    };
    let field = serde_json::json!([
        {"agent_id":"candidate", "runs":[{"returns":track(0.01)}, {"returns":track(0.01)}], "declared_mandate":{"kind":"relative_to", "benchmark_id":"reference"}},
        {"agent_id":"reference", "runs":[{"returns":track(0.02)}, {"returns":track(0.02)}]}
    ]);
    fixture.write("field.json", &field.to_string());
    let flags = [
        "field.json",
        "--pass-mode",
        "relative-to-benchmark",
        "--benchmark-agent",
        "reference",
        "--execution-seeds-per-window",
        "2",
        "--periods-per-year",
        "365",
    ];
    let run = |command: &str| {
        let args = std::iter::once(command).chain(flags).collect::<Vec<_>>();
        let output = fixture.cli(&args);
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        serde_json::from_slice::<Vec<serde_json::Value>>(&output.stdout).unwrap()
    };
    let board = run("score");
    let explanations = run("disqualify");
    for (score, explanation) in board.iter().zip(&explanations) {
        assert_eq!(score["agent_id"], explanation["agent_id"]);
        assert_eq!(score["rank_eligible"], explanation["rank_eligible"]);
        assert_eq!(score["passed_k"], false);
        assert!(explanation["reasons"]
            .as_array()
            .unwrap()
            .contains(&serde_json::json!("failed_pass_k")));
    }
    let candidate = board.iter().find(|s| s["agent_id"] == "candidate").unwrap();
    assert_eq!(candidate["declared_passed_k"], false);
    // Old fieldless explanations tested raw positive returns and passed this
    // gate. An assertion about eligibility alone would not catch that mismatch.
    let absolute = fixture.cli(&["score", "field.json"]);
    let absolute: Vec<serde_json::Value> = serde_json::from_slice(&absolute.stdout).unwrap();
    assert!(absolute.iter().all(|s| s["passed_k"] == true));
    for command in ["score", "disqualify"] {
        for flag in [
            "--periods-per-year",
            "--execution-seeds-per-window",
            "--pass-mode",
            "--benchmark-agent",
        ] {
            let output = fixture.cli(&[command, "field.json", flag]);
            assert_eq!(output.status.code(), Some(2));
            assert!(output.stdout.is_empty());
            assert!(String::from_utf8_lossy(&output.stderr).contains("requires a value"));
        }
    }
}

#[test]
fn board_commands_refuse_ambiguous_agent_identity() {
    let fixture = Fixture::new();
    fixture.write(
        "field.json",
        r#"[{"agent_id":"a","runs":[]},{"agent_id":"a","runs":[]}]"#,
    );
    for command in ["score", "disqualify"] {
        refused(fixture.cli(&[command, "field.json"]), "nonempty and unique");
    }
}
