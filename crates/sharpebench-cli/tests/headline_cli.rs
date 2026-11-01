//! `sharpebench headline <field.json>` and `sharpebench score --headline`: one
//! verdict per row, reported beside the board and never written into it.
//!
//! Absent, `score` prints the board and nothing else, byte for byte. Present,
//! the sealed board is unchanged: under `--json` it is the `board` member next
//! to a `headlines` list, and in the human table the verdict is a last column.
//! The `disqualify` advisory evidence (`--known`, `--oos-windows`) is opt-in in
//! the same way.

use std::path::PathBuf;
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};

struct Fixture(PathBuf);

impl Fixture {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        loop {
            let path = std::env::temp_dir().join(format!(
                "sharpebench-headline-{}-{}",
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

    fn cli(&self, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_sharpebench"))
            .current_dir(&self.0)
            .args(args)
            .output()
            .unwrap()
    }

    fn write(&self, name: &str, contents: &str) {
        std::fs::write(self.0.join(name), contents).unwrap();
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.0).expect("remove exclusively owned fixture");
    }
}

fn skilled_returns() -> Vec<f64> {
    (0..60)
        .map(|i| 0.002 + 0.004 * (i as f64 * 0.7).sin())
        .collect()
}

/// A skilled entrant, a flat one (no Sharpe ratio) and a noisy one, serialized
/// from the kernel's own types.
fn field() -> Fixture {
    use sharpebench_core::{AgentSubmission, Run};
    let fx = Fixture::new();
    let sub = |id: &str, runs: Vec<Vec<f64>>| AgentSubmission {
        agent_id: id.to_string(),
        runs: runs
            .into_iter()
            .map(|returns| Run {
                returns,
                ..Run::default()
            })
            .collect(),
        in_sample_trials: 0,
        candidates: Vec::new(),
    };
    let noise: Vec<f64> = (0..60).map(|i| 0.004 * (i as f64 * 0.7).sin()).collect();
    let field = vec![
        sub("skilled", vec![skilled_returns(); 5]),
        sub("flat", vec![vec![0.0; 60]]),
        sub("noise", vec![noise]),
    ];
    fx.write("field.json", &serde_json::to_string(&field).unwrap());
    fx
}

fn stdout(out: Output) -> String {
    assert_eq!(out.status.code(), Some(0), "{out:?}");
    String::from_utf8(out.stdout).unwrap()
}

fn verdicts(headlines: &serde_json::Value) -> Vec<(String, String)> {
    headlines
        .as_array()
        .unwrap()
        .iter()
        .map(|h| {
            (
                h["agent_id"].as_str().unwrap().to_string(),
                h["verdict"].as_str().unwrap().to_string(),
            )
        })
        .collect()
}

fn expected() -> Vec<(String, String)> {
    [
        ("skilled", "skill_supported"),
        ("noise", "no_evidence_of_skill"),
        ("flat", "degenerate_track"),
    ]
    .iter()
    .map(|(a, v)| (a.to_string(), v.to_string()))
    .collect()
}

#[test]
fn json_headlines_sit_beside_a_byte_identical_board() {
    let fx = field();
    let plain: serde_json::Value =
        serde_json::from_str(&stdout(fx.cli(&["score", "field.json", "--json"]))).unwrap();
    let with: serde_json::Value = serde_json::from_str(&stdout(fx.cli(&[
        "score",
        "field.json",
        "--headline",
        "--json",
    ])))
    .unwrap();
    let keys: Vec<&str> = with
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    assert_eq!(keys, ["board", "headlines"]);
    assert_eq!(with["board"], plain);
    assert_eq!(verdicts(&with["headlines"]), expected());
    let flat = &with["headlines"][2];
    assert_eq!(
        flat["hard_reasons"],
        serde_json::json!([
            "failed_pass_k",
            "dsr_below_bar",
            "bootstrap_insignificant",
            "degenerate_track"
        ])
    );
    assert_eq!(
        flat["cause"],
        "returns must not be constant: a constant series has no Sharpe ratio"
    );
    assert_eq!(with["headlines"][0]["hard_reasons"], serde_json::json!([]));
}

#[test]
fn headlines_and_diagnostics_are_reported_together() {
    let fx = field();
    let doc: serde_json::Value = serde_json::from_str(&stdout(fx.cli(&[
        "score",
        "field.json",
        "--headline",
        "--diagnostics",
        "mppm",
        "--json",
    ])))
    .unwrap();
    let keys: Vec<&str> = doc
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    assert_eq!(keys, ["board", "headlines", "sharpe_diagnostics"]);
    assert_eq!(verdicts(&doc["headlines"]), expected());
    let human = stdout(fx.cli(&["score", "field.json", "--headline", "--diagnostics", "mppm"]));
    assert!(human.contains("Opt-in Sharpe diagnostics"));
    assert!(human.lines().next().unwrap().ends_with(" headline"));
}

#[test]
fn the_human_board_gains_a_last_verdict_column_only_when_asked() {
    let fx = field();
    let plain = stdout(fx.cli(&["score", "field.json"]));
    assert!(!plain.contains("headline"));
    let with = stdout(fx.cli(&["score", "field.json", "--headline"]));
    let (plain_lines, with_lines): (Vec<&str>, Vec<&str>) =
        (plain.lines().collect(), with.lines().collect());
    assert_eq!(plain_lines.len(), with_lines.len());
    assert_eq!(with_lines[0], format!("{} headline", plain_lines[0]));
    for (i, (agent, verdict)) in expected().iter().enumerate() {
        let row = with_lines[2 + i];
        assert!(row.contains(agent.as_str()), "{row}");
        assert_eq!(row, format!("{} {verdict}", plain_lines[2 + i]));
    }
    assert_eq!(&with_lines[2 + 3..], &plain_lines[2 + 3..]);
}

#[test]
fn the_headline_command_reports_the_same_verdicts() {
    let fx = field();
    let json: serde_json::Value =
        serde_json::from_str(&stdout(fx.cli(&["headline", "field.json", "--json"]))).unwrap();
    let from_score: serde_json::Value = serde_json::from_str(&stdout(fx.cli(&[
        "score",
        "field.json",
        "--headline",
        "--json",
    ])))
    .unwrap();
    assert_eq!(json, from_score["headlines"]);
    assert_eq!(json[0]["schema"], "sharpebench.headline.v1");
    assert_eq!(json[0]["qualifiers"]["evidence"]["status"], "unknown");
    assert_eq!(json[0]["qualifiers"]["stress"], "not_run");

    let human = stdout(fx.cli(&["headline", "field.json"]));
    assert!(human.contains("skilled: skill_supported\n"), "{human}");
    assert!(
        human.contains("noise: no_evidence_of_skill [FailedPassK, DsrBelowBar"),
        "{human}"
    );
    assert!(
        human.contains("flat: degenerate_track [FailedPassK, DsrBelowBar, BootstrapInsignificant, DegenerateTrack]"),
        "{human}"
    );
}

#[test]
fn the_headline_command_refuses_a_missing_path_or_bad_input() {
    let fx = field();
    assert_eq!(fx.cli(&["headline"]).status.code(), Some(2));
    assert_eq!(
        fx.cli(&["headline", "no-such-file.json"]).status.code(),
        Some(1)
    );
    fx.write("bad.json", "not json");
    assert_eq!(fx.cli(&["headline", "bad.json"]).status.code(), Some(1));
    assert_eq!(
        fx.cli(&["headline", "field.json", "--periods-per-year", "-1"])
            .status
            .code(),
        Some(2)
    );
}

fn reasons_of(doc: &serde_json::Value, agent: &str) -> Vec<String> {
    doc.as_array()
        .unwrap()
        .iter()
        .find(|row| row["agent_id"] == agent)
        .unwrap()["reasons"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r.as_str().unwrap().to_string())
        .collect()
}

/// `--known` screens each agent's pooled stream against a known library and
/// names a rediscovery; without the flag the reason is never emitted.
#[test]
fn disqualify_names_a_rediscovery_only_against_a_supplied_library() {
    let fx = field();
    let pooled: Vec<f64> = (0..5).flat_map(|_| skilled_returns()).collect();
    let csv: Vec<String> = pooled.iter().map(|x| (2.0 * x).to_string()).collect();
    fx.write("known.csv", &csv.join("\n"));
    let bare: serde_json::Value =
        serde_json::from_str(&stdout(fx.cli(&["disqualify", "field.json", "--json"]))).unwrap();
    assert!(!reasons_of(&bare, "skilled").contains(&"is_rediscovery".to_string()));
    let screened: serde_json::Value = serde_json::from_str(&stdout(fx.cli(&[
        "disqualify",
        "field.json",
        "--known",
        "known.csv",
        "--json",
    ])))
    .unwrap();
    assert_eq!(reasons_of(&screened, "skilled"), ["is_rediscovery"]);
    assert!(!reasons_of(&screened, "noise").contains(&"is_rediscovery".to_string()));
    // A threshold above every similarity flags nothing.
    let strict: serde_json::Value = serde_json::from_str(&stdout(fx.cli(&[
        "disqualify",
        "field.json",
        "--known",
        "known.csv",
        "--rediscovery-threshold",
        "1.01",
        "--json",
    ])))
    .unwrap();
    assert!(reasons_of(&strict, "skilled").is_empty());
}

/// `--oos-windows` splits each agent's runs window-major and names an edge
/// that decays after the first window.
#[test]
fn disqualify_names_out_of_sample_decay_only_when_a_split_is_given() {
    use sharpebench_core::{AgentSubmission, Run};
    let fx = Fixture::new();
    let wave = |level: f64| -> Vec<f64> {
        (0..60)
            .map(|i| level + 0.004 * (i as f64 * 0.7).sin())
            .collect()
    };
    let decaying = AgentSubmission {
        agent_id: "decaying".into(),
        runs: [0.004, 0.004, 0.0001, 0.0001]
            .iter()
            .map(|&level| Run {
                returns: wave(level),
                ..Run::default()
            })
            .collect(),
        in_sample_trials: 0,
        candidates: Vec::new(),
    };
    fx.write("field.json", &serde_json::to_string(&[decaying]).unwrap());
    let bare: serde_json::Value =
        serde_json::from_str(&stdout(fx.cli(&["disqualify", "field.json", "--json"]))).unwrap();
    assert!(!reasons_of(&bare, "decaying").contains(&"oos_decay".to_string()));
    let split: serde_json::Value = serde_json::from_str(&stdout(fx.cli(&[
        "disqualify",
        "field.json",
        "--oos-windows",
        "2",
        "--json",
    ])))
    .unwrap();
    assert!(reasons_of(&split, "decaying").contains(&"oos_decay".to_string()));
    // More windows than runs: no split is possible, so no decay is reported.
    let too_many: serde_json::Value = serde_json::from_str(&stdout(fx.cli(&[
        "disqualify",
        "field.json",
        "--oos-windows",
        "5",
        "--json",
    ])))
    .unwrap();
    assert!(!reasons_of(&too_many, "decaying").contains(&"oos_decay".to_string()));
}

#[test]
fn disqualify_refuses_malformed_advisory_flags() {
    let fx = field();
    for args in [
        vec!["disqualify", "field.json", "--oos-windows", "1"],
        vec!["disqualify", "field.json", "--oos-windows", "two"],
        vec!["disqualify", "field.json", "--oos-windows"],
        vec!["disqualify", "field.json", "--known"],
        vec!["disqualify", "field.json", "--known", "--json"],
        vec!["disqualify", "field.json", "--rediscovery-threshold", "x"],
    ] {
        assert_eq!(fx.cli(&args).status.code(), Some(2), "{args:?}");
    }
    assert_eq!(
        fx.cli(&["disqualify", "field.json", "--known", "missing.csv"])
            .status
            .code(),
        Some(1)
    );
}
