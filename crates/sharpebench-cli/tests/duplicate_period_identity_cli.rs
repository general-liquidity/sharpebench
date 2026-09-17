//! A period identity names one observation of one run.
//!
//! A repeated period within a run would put one period's return into the
//! mean, the variance, the probabilistic Sharpe sample length and the
//! bootstrap twice. The import refuses it at the row that repeats, and
//! `score --require-run-keys` refuses a hand-built field that carries it, even
//! when every agent repeats the same period or only one agent declares periods.

use std::path::PathBuf;
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};

struct Fixture(PathBuf);

impl Fixture {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        loop {
            let path = std::env::temp_dir().join(format!(
                "sharpebench-duplicate-period-{}-{}",
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

/// 40 distinct periods per cell (t00 to t39 in `w0`, t40 to t79 in `w1`), with
/// one extra row that repeats `t07` for agent `mo` in cell (w0, 0). The header
/// is row 1, so that row is 322.
fn long_csv_with_one_repeat() -> String {
    let mut csv = String::from("agent,run,seed,period,return\n");
    for agent in ["mo", "bh"] {
        for (window, seed) in [("w0", 0), ("w0", 1), ("w1", 0), ("w1", 1)] {
            let first = if window == "w0" { 0 } else { 40 };
            for i in 0..40 {
                let r = 0.001 + 0.0004 * ((i as f64) * 0.9 + seed as f64).sin();
                let period = first + i;
                csv.push_str(&format!("{agent},{window},{seed},t{period:02},{r}\n"));
            }
        }
    }
    csv.push_str("mo,w0,0,t07,0.002\n");
    csv
}

#[test]
fn import_refuses_a_repeated_long_row_and_writes_nothing() {
    let fixture = Fixture::new();
    fixture.write("field.csv", &long_csv_with_one_repeat());
    let import = fixture.cli(&["import", "csv", "field.csv", "--out", "subs.json"]);
    refused(
        &import,
        &[
            "row 322: period `t07` repeats row 9",
            "agent `mo`, run `w0`, seed 0",
        ],
    );
    assert!(!fixture.exists("subs.json"));
}

#[test]
fn import_refuses_a_repeated_wide_date_and_names_the_file() {
    let fixture = Fixture::new();
    std::fs::create_dir(fixture.0.join("inputs")).unwrap();
    fixture.write("inputs/alpha.csv", "date,w0\nd1,0.01\nd2,0.02\nd3,0.03\n");
    // An hourly export labelled by date only.
    fixture.write(
        "inputs/beta.csv",
        "date,w0\nd1,0.01\nd1,0.02\nd2,0.03\nd3,0.04\n",
    );
    let import = fixture.cli(&["import", "csv", "inputs", "--out", "subs.json"]);
    refused(
        &import,
        &["beta.csv", "row 3: period `d1` repeats row 2", "timestamp"],
    );
    assert!(!fixture.exists("subs.json"));
}

/// A hand-built keyed field: one cell per agent, with the declared periods.
fn keyed_field(entries: &[(&str, Option<&[&str]>)]) -> String {
    let docs: Vec<serde_json::Value> = entries
        .iter()
        .map(|(agent_id, periods)| {
            let returns: Vec<f64> = (0..40)
                .map(|i| 0.001 + 0.0004 * ((i as f64) * 0.7 + agent_id.len() as f64).sin())
                .collect();
            let mut key = serde_json::json!({"window": "w0", "seed": 0});
            if let Some(periods) = periods {
                key["periods"] = serde_json::json!(periods);
            }
            serde_json::json!({
                "agent_id": agent_id,
                "runs": [{"returns": returns}],
                "run_keys": [key],
            })
        })
        .collect();
    serde_json::to_string(&docs).unwrap()
}

fn periods_with_repeat() -> Vec<String> {
    // 40 labels for 40 returns, `d05` declared twice (indices 5 and 6).
    let mut periods: Vec<String> = (0..40).map(|i| format!("d{i:02}")).collect();
    periods[6] = "d05".to_string();
    periods
}

#[test]
fn keyed_score_refuses_a_period_every_agent_repeats() {
    let fixture = Fixture::new();
    let owned = periods_with_repeat();
    let periods: Vec<&str> = owned.iter().map(String::as_str).collect();
    fixture.write(
        "subs.json",
        &keyed_field(&[("mo", Some(&periods)), ("bh", Some(&periods))]),
    );
    refused(
        &fixture.cli(&["score", "subs.json", "--require-run-keys"]),
        &[
            "agent `mo` (window `w0`, seed 0) declares period `d05` at index 5 and again at index 6",
        ],
    );

    // Distinct periods are the control: the same field scores.
    let distinct: Vec<String> = (0..40).map(|i| format!("d{i:02}")).collect();
    let distinct: Vec<&str> = distinct.iter().map(String::as_str).collect();
    fixture.write(
        "subs.json",
        &keyed_field(&[("mo", Some(&distinct)), ("bh", Some(&distinct))]),
    );
    let scored = fixture.cli(&["score", "subs.json", "--require-run-keys"]);
    assert!(
        scored.status.success(),
        "{}",
        String::from_utf8_lossy(&scored.stderr)
    );
}

#[test]
fn keyed_score_refuses_a_repeat_when_only_one_agent_declares_periods() {
    let fixture = Fixture::new();
    let owned = periods_with_repeat();
    let periods: Vec<&str> = owned.iter().map(String::as_str).collect();
    fixture.write(
        "subs.json",
        &keyed_field(&[("mo", None), ("bh", Some(&periods))]),
    );
    refused(
        &fixture.cli(&["score", "subs.json", "--require-run-keys"]),
        &["agent `bh` (window `w0`, seed 0) declares period `d05`"],
    );
}
