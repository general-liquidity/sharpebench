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
        assert!(run["reference"]["monte_carlo_standard_error"].is_f64());
        assert!(run["distinct_placements"].as_u64().unwrap() > 12);
        let exposure = &run["exposure"];
        assert_eq!(exposure["bars"], 90);
        assert!(exposure["invested_bars"].as_u64().unwrap() < 90);
    }
    // The eight seed copies of a window draw the same placements: with the
    // same decisions, their reference means differ only by slippage.
    let reference_mean = |index: usize| {
        runs[index]["reference"]["reference_mean_sharpe"]
            .as_f64()
            .unwrap()
    };
    assert!((reference_mean(0) - reference_mean(7)).abs() < 0.01);
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
    assert!(lagged["end_effect"]
        .as_str()
        .unwrap()
        .contains("never executes the run's last k recorded decisions"));
    let lagged_runs = lagged["runs"].as_array().unwrap();
    assert_eq!(lagged_runs.len(), 16);
    for run in lagged_runs {
        assert_eq!(run["status"], "available", "{run}");
        let skipped = run["skipped_leading_bars"].as_u64().unwrap();
        assert!(skipped >= 3, "{run}");
        assert_eq!(run["compared_bars"].as_u64().unwrap(), 90 - skipped);
    }
    let aggregate = &lagged["aggregate"];
    assert_eq!(aggregate["status"], "available");
    assert_eq!(aggregate["runs"], 16);
    assert_eq!(aggregate["undelayed"]["lag"], 0);
    let rows = aggregate["lagged"].as_array().unwrap();
    assert_eq!(rows.len(), 2);
    // Its edge decays as its decisions arrive later.
    let sharpe = |row: &Value| row["mean_sharpe"].as_f64().unwrap();
    assert!(sharpe(&rows[1]) < sharpe(&rows[0]));
    assert!(sharpe(&rows[0]) < sharpe(&aggregate["undelayed"]));

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
    assert!(block.contains("MC s.e."));
    assert!(block.contains("distinct placements"));
    assert!(block.contains("Lagged replay"));
    assert!(block.contains("bars compared after skipping"));
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
            "--null-draws",
            "100001",
        ],
        vec![
            "verify-trajectory",
            "missing.json",
            "--timing-null",
            "--null-draws",
            "18446744073709551615",
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
        vec![
            "verify-trajectory",
            "missing.json",
            "--lagged-replay",
            "1",
            "--diagnostics",
            "sizing-response",
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

    // A lag the windows cannot hold is a usage error once the trajectory is
    // read, including lags whose bar arithmetic would overflow.
    for lag in ["88", "89", "18446744073709551614", "18446744073709551615"] {
        let out = fx.cli(&[
            "verify-trajectory",
            "momentum.json",
            "--data",
            "wave.csv",
            "--lagged-replay",
            &format!("1,{lag}"),
        ]);
        assert_eq!(out.status.code(), Some(2), "{lag}: {out:?}");
        assert!(out.stdout.is_empty(), "{lag}");
        let stderr = String::from_utf8(out.stderr).unwrap();
        assert!(
            stderr.contains(&format!(
                "lagged replay: lag {lag} leaves fewer than two comparable bars"
            )),
            "{lag}: {stderr}"
        );
    }
    // The longest lag a 90-bar run holds.
    let out = fx.cli(&[
        "verify-trajectory",
        "momentum.json",
        "--data",
        "wave.csv",
        "--lagged-replay",
        "87",
    ]);
    assert_eq!(out.status.code(), Some(0), "{out:?}");

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

/// The diagnostics replay under the cost model the trajectory is bound to,
/// short borrow included: a short book's figures move with the bound rate.
#[test]
fn the_diagnostics_replay_under_the_bound_borrow_rate() {
    let fx = Fixture::new();
    let shorted = |name: &str, extra: &[&str]| {
        let mut args = vec!["capture", "momentum", name, "--data", "wave.csv"];
        args.extend_from_slice(extra);
        let captured = fx.cli(&args);
        assert!(captured.status.success(), "{captured:?}");
        // The contract binds data, costs, windows, seeds and runner, not the
        // decisions: turn every long target into a short of the same size.
        let path = fx.0.join(name);
        let mut traj: Value =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        for run in traj["runs"].as_array_mut().unwrap() {
            for step in run["steps"].as_array_mut().unwrap() {
                for order in step["decision"]["orders"].as_array_mut().unwrap() {
                    let weight = order["target_weight"].as_f64().unwrap();
                    order["target_weight"] = serde_json::json!(-weight);
                }
            }
        }
        std::fs::write(&path, serde_json::to_string(&traj).unwrap()).unwrap();
    };
    shorted("free.json", &[]);
    shorted("borrowed.json", &["--short-borrow-bps", "25"]);
    let report = |name: &str, extra: &[&str]| {
        let mut args = vec![
            "verify-trajectory",
            name,
            "--data",
            "wave.csv",
            "--timing-null",
            "--null-draws",
            "6",
            "--lagged-replay",
            "1",
            "--json",
        ];
        args.extend_from_slice(extra);
        fx.json(&args)["replay_diagnostics"].clone()
    };
    let free = report("free.json", &[]);
    let borrowed = report("borrowed.json", &["--short-borrow-bps", "25"]);
    let mean_return = |diagnostics: &Value| {
        diagnostics["lagged_replay"]["aggregate"]["undelayed"]["mean_return"]
            .as_f64()
            .unwrap()
    };
    // 25 bps per bar on shorts of up to the whole book.
    assert!(
        mean_return(&borrowed) < mean_return(&free) - 1e-4,
        "{} against {}",
        mean_return(&borrowed),
        mean_return(&free)
    );
    let entrant = |diagnostics: &Value| {
        diagnostics["timing_null"]["runs"][0]["entrant_sharpe"]
            .as_f64()
            .unwrap()
    };
    assert!(entrant(&borrowed) < entrant(&free));
}
