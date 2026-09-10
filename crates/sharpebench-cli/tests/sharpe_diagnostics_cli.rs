//! `sharpebench score --diagnostics <list>`: opt-in Sharpe diagnostics the
//! gate does not use.
//!
//! Absent, the output is the board and nothing else, byte for byte. Present,
//! the board is unchanged and the diagnostics sit beside it: under `--json` as
//! a `sharpe_diagnostics` member next to a `board` member equal to the
//! board-only output, in the human table as a separate block after the board.
//! An unknown identifier or a missing value is refused before anything is
//! printed.

use std::path::PathBuf;
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};

struct Fixture(PathBuf);

impl Fixture {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        loop {
            let path = std::env::temp_dir().join(format!(
                "sharpebench-diagnostics-{}-{}",
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
}

impl Drop for Fixture {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.0).expect("remove exclusively owned fixture");
    }
}

/// An autocorrelated entrant, a steady one and a flat one, serialized from the
/// kernel's own types.
fn field() -> Fixture {
    use sharpebench_core::{AgentSubmission, Run};
    let fx = Fixture::new();
    let wave: Vec<f64> = (0..200)
        .map(|t| {
            let t = t as f64;
            0.001 + 0.01 * (1.3 * t).sin() + 0.004 * (0.21 * t).cos()
        })
        .collect();
    let steady: Vec<f64> = (0..200)
        .map(|i| 0.001 + 0.0002 * ((i % 7) as f64 - 3.0))
        .collect();
    let sub = |id: &str, returns: Vec<f64>| AgentSubmission {
        agent_id: id.to_string(),
        runs: vec![Run {
            returns,
            ..Run::default()
        }],
        in_sample_trials: 0,
        candidates: Vec::new(),
    };
    let field = vec![
        sub("wave", wave),
        sub("steady", steady),
        sub("flat", vec![0.0; 200]),
    ];
    std::fs::write(
        fx.0.join("field.json"),
        serde_json::to_string(&field).unwrap(),
    )
    .unwrap();
    fx
}

#[test]
fn absent_flag_prints_the_board_only() {
    let fx = field();
    for json in [true, false] {
        let mut args = vec!["score", "field.json"];
        if json {
            args.push("--json");
        }
        let out = fx.cli(&args);
        assert_eq!(out.status.code(), Some(0));
        let text = String::from_utf8(out.stdout).unwrap();
        for word in [
            "sharpe_diagnostics",
            "Opt-in Sharpe diagnostics",
            "mppm",
            "MPPM",
            "autocorrelated_psr",
            "null_se_psr",
        ] {
            assert!(!text.contains(word), "{word} in default output");
        }
    }
}

#[test]
fn json_diagnostics_sit_beside_a_byte_identical_board() {
    let fx = field();
    let plain = fx.cli(&["score", "field.json", "--json"]);
    let with = fx.cli(&[
        "score",
        "field.json",
        "--diagnostics",
        "mppm,autocorrelated-psr,null-se-psr",
        "--json",
    ]);
    assert_eq!(with.status.code(), Some(0));
    let plain_text = String::from_utf8(plain.stdout).unwrap();
    let doc: serde_json::Value = serde_json::from_slice(&with.stdout).unwrap();
    let keys: Vec<&str> = doc
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    assert_eq!(keys, ["board", "sharpe_diagnostics"]);
    // The `board` member is the board-only output, value for value.
    let plain_board: serde_json::Value = serde_json::from_str(&plain_text).unwrap();
    assert_eq!(doc["board"], plain_board);

    let diags = doc["sharpe_diagnostics"].as_array().unwrap();
    let board = doc["board"].as_array().unwrap();
    assert_eq!(diags.len(), board.len());
    for (row, d) in board.iter().zip(diags) {
        assert_eq!(d["agent_id"], row["agent_id"]);
        assert_eq!(d["used_by_gate"], false);
        assert_eq!(d["pooled_observations"], row["pooled_observations"]);
        assert_eq!(d["mppm"]["risk_aversion"], 3.0);
        assert_eq!(d["mppm"]["periods_per_year"], 252.0);
        assert_eq!(d["null_se_psr"]["rho"], 0.0);
    }
    let by_id = |id: &str| diags.iter().find(|d| d["agent_id"] == id).unwrap();
    let row_of = |id: &str| board.iter().find(|r| r["agent_id"] == id).unwrap();
    let wave = by_id("wave");
    assert!(wave["autocorrelated_psr"]["rho"].as_f64().unwrap() > 0.3);
    assert!(
        wave["autocorrelated_psr"]["psr"].as_f64().unwrap()
            < row_of("wave")["psr"].as_f64().unwrap()
    );
    let flat = by_id("flat");
    assert!(flat["autocorrelated_psr"]["error"]
        .as_str()
        .unwrap()
        .contains("constant"));
    assert!(flat["autocorrelated_psr"].get("psr").is_none());
    assert_eq!(flat["mppm"]["annualized"], 0.0);
}

#[test]
fn only_the_requested_diagnostics_are_reported() {
    let fx = field();
    let out = fx.cli(&["score", "field.json", "--diagnostics", "mppm", "--json"]);
    assert_eq!(out.status.code(), Some(0));
    let doc: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    for d in doc["sharpe_diagnostics"].as_array().unwrap() {
        assert!(d.get("mppm").is_some());
        assert!(d.get("autocorrelated_psr").is_none());
        assert!(d.get("null_se_psr").is_none());
    }
}

#[test]
fn human_output_appends_a_labelled_block_after_the_unchanged_board() {
    let fx = field();
    let plain = String::from_utf8(fx.cli(&["score", "field.json"]).stdout).unwrap();
    let with = fx.cli(&[
        "score",
        "field.json",
        "--diagnostics",
        "autocorrelated-psr,null-se-psr,mppm",
    ]);
    assert_eq!(with.status.code(), Some(0));
    let text = String::from_utf8(with.stdout).unwrap();
    let rest = text
        .strip_prefix(plain.as_str())
        .expect("the board prints first, unchanged");
    assert!(rest.starts_with("\nOpt-in Sharpe diagnostics. Not used by the gate"));
    for header in [
        "rho",
        "ac_PSR",
        "ac_DSR",
        "null_PSR",
        "null_DSR",
        "MPPM(3)/yr",
    ] {
        assert!(rest.contains(header), "{header}");
    }
    assert!(rest.contains("n/a"), "the flat track's rho is unavailable");
}

#[test]
fn unknown_or_missing_diagnostics_are_refused_before_output() {
    let fx = field();
    for list in ["psr", "mppm,", "MPPM", ""] {
        let out = fx.cli(&["score", "field.json", "--diagnostics", list, "--json"]);
        assert_eq!(out.status.code(), Some(2), "{list}");
        assert!(out.stdout.is_empty(), "no partial board on failure");
        assert!(String::from_utf8_lossy(&out.stderr).contains("unknown diagnostic"));
    }
    for args in [
        &["score", "field.json", "--diagnostics"][..],
        &["score", "field.json", "--diagnostics", "--json"][..],
    ] {
        let out = fx.cli(args);
        assert_eq!(out.status.code(), Some(2));
        assert!(out.stdout.is_empty());
        assert!(String::from_utf8_lossy(&out.stderr).contains("--diagnostics requires a value"));
    }
}
