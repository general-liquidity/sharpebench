//! A period belongs to one window.
//!
//! The pooled track concatenates a field's windows, so a period declared in two
//! windows would enter the mean, the variance, the probabilistic Sharpe sample
//! length and the bootstrap twice. Execution seeds of one window replicate the
//! same periods and are averaged before pooling, so they may share them. The
//! import refuses a period in two windows, and `score --require-run-keys`
//! refuses a hand-built field that carries one.

use std::path::PathBuf;
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};

struct Fixture(PathBuf);

impl Fixture {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        loop {
            let path = std::env::temp_dir().join(format!(
                "sharpebench-overlapping-periods-{}-{}",
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

    fn exists(&self, name: &str) -> bool {
        self.0.join(name).exists()
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

fn refused(output: &Output, needles: &[&str]) {
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(output.status.code(), Some(1), "{stderr}");
    assert!(output.stdout.is_empty(), "no partial output on failure");
    for needle in needles {
        assert!(stderr.contains(needle), "missing `{needle}` in: {stderr}");
    }
}

fn day(i: usize) -> String {
    format!("2025-{:03}", i + 1)
}

fn ret(agent: &str, i: usize) -> f64 {
    let drift = if agent == "a" { 0.0008 } else { 0.0002 };
    drift + 0.01 * ((i as f64) * 0.77 + agent.len() as f64).sin()
}

/// A long CSV for agents `a` and `b` at seed 0, one run per named window over
/// the given period indices. The return of a period is the same whichever
/// window carries it, as it is when one window is imported twice.
fn long_csv(windows: &[(&str, std::ops::Range<usize>)]) -> String {
    let mut csv = String::from("agent,run,seed,period,return\n");
    for agent in ["a", "b"] {
        for (window, range) in windows {
            for i in range.clone() {
                csv.push_str(&format!(
                    "{agent},{window},0,{},{}\n",
                    day(i),
                    ret(agent, i)
                ));
            }
        }
    }
    csv
}

#[test]
fn import_refuses_a_window_imported_twice_and_writes_nothing() {
    let fixture = Fixture::new();
    // Header is row 1; a's w0 occupies rows 2..=61, so a's w1 starts at 62.
    fixture.write("dup.csv", &long_csv(&[("w0", 0..60), ("w1", 0..60)]));
    refused(
        &fixture.cli(&["import", "csv", "dup.csv", "--out", "subs.json"]),
        &[
            "row 62: period `2025-001` is in run `w1` for agent `a`, and row 2 put it in run `w0`",
            "a period belongs to one window",
        ],
    );
    assert!(!fixture.exists("subs.json"));

    // Overlapping, not identical: w1 restates w0's second half.
    fixture.write("overlap.csv", &long_csv(&[("w0", 0..60), ("w1", 30..90)]));
    refused(
        &fixture.cli(&["import", "csv", "overlap.csv", "--out", "subs.json"]),
        &["row 62: period `2025-031` is in run `w1` for agent `a`, and row 32 put it in run `w0`"],
    );
    assert!(!fixture.exists("subs.json"));
}

#[test]
fn disjoint_windows_import_and_score_on_every_period_once() {
    let fixture = Fixture::new();
    fixture.write("disjoint.csv", &long_csv(&[("w0", 0..60), ("w1", 60..120)]));
    let import = fixture.cli(&["import", "csv", "disjoint.csv", "--out", "subs.json"]);
    assert!(
        import.status.success(),
        "{}",
        String::from_utf8_lossy(&import.stderr)
    );
    let scored = fixture.cli(&["score", "subs.json", "--require-run-keys"]);
    assert!(
        scored.status.success(),
        "{}",
        String::from_utf8_lossy(&scored.stderr)
    );
    let board: serde_json::Value = serde_json::from_slice(&scored.stdout).unwrap();
    for row in board.as_array().unwrap() {
        assert_eq!(row["pooled_observations"], 120);
    }
}

#[test]
fn import_refuses_a_wide_date_with_returns_in_two_columns() {
    let fixture = Fixture::new();
    std::fs::create_dir(fixture.0.join("inputs")).unwrap();
    let mut wide = String::from("date,w0,w1\n");
    for i in 0..40 {
        wide.push_str(&format!("{},{},{}\n", day(i), ret("a", i), ret("a", i)));
    }
    fixture.write("inputs/alpha.csv", &wide);
    refused(
        &fixture.cli(&["import", "csv", "inputs", "--out", "subs.json"]),
        &[
            "alpha.csv",
            "row 2: period `2025-001` has a return in column `w0` and in column `w1`",
            "`seed` column",
        ],
    );
    assert!(!fixture.exists("subs.json"));
}

/// A keyed field that did not come through the import: two agents, two
/// windows, one seed, every cell declaring `periods`. Both windows carry the
/// same returns, so only the declared periods differ between the two calls.
fn keyed_field(w0: &[String], w1: &[String]) -> String {
    let docs: Vec<serde_json::Value> = ["a", "b"]
        .iter()
        .map(|agent| {
            let run = |periods: &[String]| {
                serde_json::json!({
                    "returns": (0..periods.len()).map(|i| ret(agent, i)).collect::<Vec<_>>(),
                })
            };
            serde_json::json!({
                "agent_id": agent,
                "runs": [run(w0), run(w1)],
                "run_keys": [
                    {"window": "w0", "seed": 0, "periods": w0},
                    {"window": "w1", "seed": 0, "periods": w1},
                ],
            })
        })
        .collect();
    serde_json::to_string(&docs).unwrap()
}

#[test]
fn keyed_score_refuses_a_period_declared_in_two_windows() {
    let fixture = Fixture::new();
    let w0: Vec<String> = (0..60).map(day).collect();
    fixture.write("subs.json", &keyed_field(&w0, &w0));
    refused(
        &fixture.cli(&["score", "subs.json", "--require-run-keys"]),
        &[
            "period `2025-001` is declared in (window `w0`, seed 0) by agent `a` and in \
             (window `w1`, seed 0) by agent `a`",
            "the pooled track would count its return twice",
        ],
    );

    // The unkeyed scorer has no coordinates to check and still scores it: the
    // refusal belongs to the keyed path.
    assert!(fixture.cli(&["score", "subs.json"]).status.success());

    // Disjoint periods are the control.
    let w1: Vec<String> = (60..120).map(day).collect();
    fixture.write("subs.json", &keyed_field(&w0, &w1));
    let scored = fixture.cli(&["score", "subs.json", "--require-run-keys"]);
    assert!(
        scored.status.success(),
        "{}",
        String::from_utf8_lossy(&scored.stderr)
    );
}
