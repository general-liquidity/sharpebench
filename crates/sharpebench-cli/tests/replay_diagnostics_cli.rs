//! `sharpebench verify-trajectory --timing-null` and `--lagged-replay`:
//! rank-neutral replay diagnostics beside a strictly verified trajectory.
//!
//! Absent, the verification output is what it always was. Present, the
//! verification is unchanged and the diagnostics follow it: under `--json` as
//! a `replay_diagnostics` member beside the sealed verification fields, in the
//! human output as a block after the verification. A malformed or
//! contradictory flag is refused before anything is read.

use std::path::PathBuf;
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};

use serde_json::Value;

struct Fixture(PathBuf);

impl Fixture {
    /// A directory holding `wave.csv`, one symbol whose price swings on a
    /// 40-bar cycle, `momentum.json` captured on it (flat on the falling legs,
    /// so it has timing to test) and `buy-and-hold.json` captured on the
    /// synthetic dataset (invested on every bar), both by this binary.
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let fixture = loop {
            let path = std::env::temp_dir().join(format!(
                "sharpebench-replay-diagnostics-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            match std::fs::create_dir(&path) {
                Ok(()) => break Self(path),
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(e) => panic!("create fixture: {e}"),
            }
        };
        let mut csv = String::from(
            "date,symbol,close
",
        );
        for bar in 0..200 {
            let phase = bar as f64 * std::f64::consts::TAU / 40.0;
            let close = 100.0 * (1.0 + 0.001 * bar as f64) + 10.0 * phase.sin();
            csv.push_str(&format!(
                "d{bar:03},WAVE,{close}
"
            ));
        }
        std::fs::write(fixture.0.join("wave.csv"), csv).unwrap();
        for args in [
            ["capture", "momentum", "momentum.json", "--data", "wave.csv"].as_slice(),
            ["capture", "buy-and-hold", "buy-and-hold.json"].as_slice(),
        ] {
            let captured = fixture.cli(args);
            assert!(captured.status.success(), "{captured:?}");
        }
        fixture
    }

    fn cli(&self, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_sharpebench"))
            .current_dir(&self.0)
            .args(args)
            .output()
            .unwrap()
    }

    fn json(&self, args: &[&str]) -> Value {
        let out = self.cli(args);
        assert!(out.status.success(), "{args:?}: {out:?}");
        serde_json::from_slice(&out.stdout).unwrap()
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.0).expect("remove exclusively owned fixture");
    }
}

#[test]
fn the_diagnostics_add_a_member_and_change_nothing_else() {
    let fx = Fixture::new();
    let plain = fx.cli(&[
        "verify-trajectory",
        "momentum.json",
        "--data",
        "wave.csv",
        "--json",
    ]);
    assert!(plain.status.success(), "{plain:?}");
    let plain_json: Value = serde_json::from_slice(&plain.stdout).unwrap();
    assert!(plain_json.get("replay_diagnostics").is_none());

    let flagged = fx.json(&[
        "verify-trajectory",
        "momentum.json",
        "--data",
        "wave.csv",
        "--timing-null",
        "--null-draws",
        "12",
        "--null-seed",
        "3",
        "--lagged-replay",
        "1,2",
        "--json",
    ]);
    let mut without = flagged.clone();
    let diagnostics = without
        .as_object_mut()
        .unwrap()
        .remove("replay_diagnostics")
        .expect("the diagnostics member");
    assert_eq!(without, plain_json, "the verification must not move");
    assert_eq!(diagnostics["rank_neutral"], true);
    assert!(diagnostics["valid_when"]
        .as_str()
        .unwrap()
        .contains("do not move the price"));

    let timing = &diagnostics["timing_null"];
    assert_eq!(timing["draws"], 12);
    assert_eq!(timing["seed"], 3);
    let runs = timing["runs"].as_array().unwrap();
    assert_eq!(runs.len(), 16, "two windows by eight seeds");
    for (index, run) in runs.iter().enumerate() {
        assert_eq!(run["run"], index);
        assert_eq!(run["status"], "available", "{run}");
        assert_eq!(run["reference"]["draws"], 12);
        let percentile = run["reference"]["percentile"].as_f64().unwrap();
        assert!((0.0..=1.0).contains(&percentile));
        let exposure = &run["exposure"];
        assert_eq!(exposure["bars"], 90);
        assert!(exposure["invested_bars"].as_u64().unwrap() < 90);
    }
    // Momentum rides a smooth cycle: its timing beats random timing.
    assert_eq!(timing["aggregate"]["status"], "available");
    assert_eq!(timing["aggregate"]["runs"], 16);
    assert!(
        timing["aggregate"]["reference"]["percentile"]
            .as_f64()
            .unwrap()
            >= 0.9
    );

    let lagged = &diagnostics["lagged_replay"];
    assert_eq!(lagged["lags"], serde_json::json!([1, 2]));
    assert_eq!(lagged["skipped_leading_bars"], 3);
    assert_eq!(lagged["runs"], 16);
    assert_eq!(lagged["undelayed"]["lag"], 0);
    let rows = lagged["lagged"].as_array().unwrap();
    assert_eq!(rows.len(), 2);
    // Its edge decays as its decisions arrive later.
    let sharpe = |row: &Value| row["mean_sharpe"].as_f64().unwrap();
    assert!(sharpe(&rows[1]) < sharpe(&rows[0]));
    assert!(sharpe(&rows[0]) < sharpe(&lagged["undelayed"]));

    // The human output is the verification, then the block.
    let plain_text = fx.cli(&["verify-trajectory", "momentum.json", "--data", "wave.csv"]);
    let flagged_text = fx.cli(&[
        "verify-trajectory",
        "momentum.json",
        "--data",
        "wave.csv",
        "--lagged-replay",
        "1",
        "--timing-null",
        "--null-draws",
        "5",
    ]);
    assert!(flagged_text.status.success(), "{flagged_text:?}");
    let (plain_text, flagged_text) = (
        String::from_utf8(plain_text.stdout).unwrap(),
        String::from_utf8(flagged_text.stdout).unwrap(),
    );
    assert!(flagged_text.starts_with(&plain_text), "{flagged_text}");
    let block = &flagged_text[plain_text.len()..];
    assert!(
        block.contains("Replay diagnostics (rank-neutral"),
        "{block}"
    );
    assert!(block.contains("Exposure-matched random timing: 5 draws, seed 0"));
    assert!(block.contains("Lagged replay"));
    assert!(block.contains("undelayed"));
}

#[test]
fn the_diagnostics_are_deterministic_under_the_declared_seed() {
    let fx = Fixture::new();
    let args = [
        "verify-trajectory",
        "momentum.json",
        "--data",
        "wave.csv",
        "--timing-null",
        "--null-draws",
        "8",
        "--null-seed",
        "21",
        "--json",
    ];
    let first = fx.cli(&args);
    let again = fx.cli(&args);
    assert!(first.status.success(), "{first:?}");
    assert_eq!(first.stdout, again.stdout);

    let reseeded = fx.json(&[
        "verify-trajectory",
        "momentum.json",
        "--data",
        "wave.csv",
        "--timing-null",
        "--null-draws",
        "8",
        "--null-seed",
        "22",
        "--json",
    ]);
    let first: Value = serde_json::from_slice(&first.stdout).unwrap();
    assert_ne!(
        first["replay_diagnostics"]["timing_null"]["runs"],
        reseeded["replay_diagnostics"]["timing_null"]["runs"]
    );
    // The default declaration is 200 draws from seed 0.
    let defaulted = fx.json(&[
        "verify-trajectory",
        "momentum.json",
        "--data",
        "wave.csv",
        "--timing-null",
        "--json",
    ]);
    assert_eq!(defaulted["replay_diagnostics"]["timing_null"]["draws"], 200);
    assert_eq!(defaulted["replay_diagnostics"]["timing_null"]["seed"], 0);
}

#[test]
fn a_run_without_timing_freedom_is_reported_unavailable() {
    let fx = Fixture::new();
    let report = fx.json(&[
        "verify-trajectory",
        "buy-and-hold.json",
        "--timing-null",
        "--null-draws",
        "4",
        "--json",
    ]);
    let timing = &report["replay_diagnostics"]["timing_null"];
    for run in timing["runs"].as_array().unwrap() {
        assert_eq!(run["status"], "unavailable", "{run}");
        assert_eq!(run["reason"], "always_invested", "{run}");
    }
    assert_eq!(timing["aggregate"]["status"], "unavailable");
    assert_eq!(timing["aggregate"]["reason"], "always_invested");

    let text = fx.cli(&[
        "verify-trajectory",
        "buy-and-hold.json",
        "--timing-null",
        "--null-draws",
        "4",
    ]);
    let text = String::from_utf8(text.stdout).unwrap();
    assert!(
        text.contains("unavailable: invested on every bar"),
        "{text}"
    );
    assert!(text.contains("across runs: unavailable"), "{text}");
}

#[test]
fn malformed_or_contradictory_flags_are_refused_before_reading() {
    let fx = Fixture::new();
    for args in [
        vec!["verify-trajectory", "missing.json", "--null-draws", "5"],
        vec!["verify-trajectory", "missing.json", "--null-seed", "5"],
        vec![
            "verify-trajectory",
            "missing.json",
            "--timing-null",
            "--null-draws",
            "0",
        ],
        vec![
            "verify-trajectory",
            "missing.json",
            "--timing-null",
            "--null-draws",
            "x",
        ],
        vec![
            "verify-trajectory",
            "missing.json",
            "--timing-null",
            "--null-seed",
        ],
        vec!["verify-trajectory", "missing.json", "--lagged-replay"],
        vec![
            "verify-trajectory",
            "missing.json",
            "--lagged-replay",
            "1,x",
        ],
        vec!["verify-trajectory", "missing.json", "--lagged-replay", "-1"],
        vec![
            "verify-trajectory",
            "missing.json",
            "--lagged-replay",
            "1",
            "--allow-unbound-trajectory",
        ],
        vec![
            "verify-trajectory",
            "missing.json",
            "--timing-null",
            "--reexecute",
        ],
    ] {
        let out = fx.cli(&args);
        assert_eq!(out.status.code(), Some(2), "{args:?}: {out:?}");
        assert!(out.stdout.is_empty(), "{args:?}");
        let stderr = String::from_utf8(out.stderr).unwrap();
        assert!(
            stderr.starts_with("error: ") && !stderr.contains("cannot read"),
            "{args:?}: {stderr}"
        );
    }

    // A lag the windows cannot hold is the library's refusal, after reading.
    let out = fx.cli(&[
        "verify-trajectory",
        "momentum.json",
        "--data",
        "wave.csv",
        "--lagged-replay",
        "89",
    ]);
    assert_eq!(out.status.code(), Some(1), "{out:?}");
    assert!(out.stdout.is_empty());
    let stderr = String::from_utf8(out.stderr).unwrap();
    assert!(
        stderr.contains("lagged replay: lag 89 leaves fewer than two comparable bars"),
        "{stderr}"
    );

    // Without the diagnostics, the unbound override still works as before.
    let unbound = fx.cli(&[
        "verify-trajectory",
        "momentum.json",
        "--data",
        "wave.csv",
        "--allow-unbound-trajectory",
    ]);
    assert!(unbound.status.success(), "{unbound:?}");
}
