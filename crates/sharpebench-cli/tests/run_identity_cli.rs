//! Keyed run identity through the CSV import and the ranking path.
//!
//! `score` reads `runs[i]` for every agent when it restricts to shared support
//! or compares against a benchmark. `--require-run-keys` makes that alignment
//! an explicit, validated claim: the import carries the run identity the CSV
//! declared, and the scorer refuses an unkeyed, partial or duplicated field
//! instead of aligning by position.

use std::path::PathBuf;
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};

struct Fixture(PathBuf);

impl Fixture {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        loop {
            let path = std::env::temp_dir().join(format!(
                "sharpebench-run-identity-{}-{}",
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

    fn read(&self, name: &str) -> String {
        std::fs::read_to_string(self.0.join(name)).unwrap()
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

fn refused(output: &Output, message: &str) {
    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty(), "no partial board on failure");
    assert!(
        String::from_utf8_lossy(&output.stderr).contains(message),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

/// A long-format field: one row per (agent, run, seed, period).
fn long_csv(cells: &[(&str, &str, u64)]) -> String {
    let mut csv = String::from("agent,run,seed,period,return\n");
    for (agent, window, seed) in cells {
        for i in 0..40 {
            let r = 0.001 + 0.0004 * ((i as f64) * 0.9 + *seed as f64).sin();
            csv.push_str(&format!("{agent},{window},{seed},t{i:02},{r}\n"));
        }
    }
    csv
}

#[test]
fn import_carries_run_window_seed_and_period_identity_into_the_scored_field() {
    let fixture = Fixture::new();
    // `mo` lists its cells in the opposite order to `bh`. Under positional
    // alignment index 0 would be a different window for each agent.
    fixture.write(
        "field.csv",
        &long_csv(&[
            ("mo", "w1", 1),
            ("mo", "w1", 0),
            ("mo", "w0", 1),
            ("mo", "w0", 0),
            ("bh", "w0", 0),
            ("bh", "w0", 1),
            ("bh", "w1", 0),
            ("bh", "w1", 1),
        ]),
    );
    let import = fixture.cli(&["import", "csv", "field.csv", "--out", "subs.json"]);
    assert_eq!(import.status.code(), Some(0));

    let doc: serde_json::Value = serde_json::from_str(&fixture.read("subs.json")).unwrap();
    let keys = doc[0]["run_keys"].as_array().expect("run keys emitted");
    assert_eq!(keys.len(), 4);
    assert_eq!(keys[0]["window"], "w1");
    assert_eq!(keys[0]["seed"], 1);
    assert_eq!(keys[0]["periods"][0], "t00");
    assert_eq!(keys[0]["periods"].as_array().unwrap().len(), 40);

    // The scorer accepts it and reorders both agents onto one cell order.
    let scored = fixture.cli(&["score", "subs.json", "--require-run-keys"]);
    assert_eq!(
        scored.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&scored.stderr)
    );
    let board: serde_json::Value = serde_json::from_slice(&scored.stdout).unwrap();
    assert_eq!(board.as_array().map(Vec::len), Some(2));
    for row in board.as_array().unwrap() {
        // Two 40-period windows, not four independent 40-period runs.
        assert_eq!(row["pooled_observations"], 80);
    }
    let explicit = fixture.cli(&[
        "score",
        "subs.json",
        "--require-run-keys",
        "--execution-seeds-per-window",
        "2",
    ]);
    assert!(explicit.status.success());
    assert_eq!(scored.stdout, explicit.stdout);

    for width in ["1", "3"] {
        let output = fixture.cli(&[
            "score",
            "subs.json",
            "--require-run-keys",
            "--execution-seeds-per-window",
            width,
        ]);
        assert_eq!(output.status.code(), Some(2));
        assert!(output.stdout.is_empty());
        assert!(String::from_utf8_lossy(&output.stderr).contains("validated run keys require 2"));
    }
}

#[test]
fn missing_wide_returns_keep_their_dates_and_cannot_pair_different_periods() {
    let fixture = Fixture::new();
    std::fs::create_dir(fixture.0.join("inputs")).unwrap();
    fixture.write("inputs/alpha.csv", "date,w0\nd1,0.01\nd2,\nd3,0.03\n");
    fixture.write("inputs/beta.csv", "date,w0\nd1,0.01\nd2,0.02\nd3,\n");
    let imported = fixture.cli(&["import", "csv", "inputs", "--out", "subs.json"]);
    assert!(
        imported.status.success(),
        "{}",
        String::from_utf8_lossy(&imported.stderr)
    );
    let doc: serde_json::Value = serde_json::from_str(&fixture.read("subs.json")).unwrap();
    assert_eq!(
        doc[0]["run_keys"][0]["periods"],
        serde_json::json!(["d1", "d3"])
    );
    assert_eq!(
        doc[1]["run_keys"][0]["periods"],
        serde_json::json!(["d1", "d2"])
    );
    let scored = fixture.cli(&["score", "subs.json", "--require-run-keys"]);
    assert_eq!(scored.status.code(), Some(1));
    assert!(scored.stdout.is_empty());
    assert!(String::from_utf8_lossy(&scored.stderr).contains("period"));

    // The same missing dates are still valid shared support, explicitly named.
    fixture.write("inputs/beta.csv", "date,w0\nd1,0.02\nd2,\nd3,0.04\n");
    assert!(fixture
        .cli(&["import", "csv", "inputs", "--out", "subs.json"])
        .status
        .success());
    assert!(fixture
        .cli(&["score", "subs.json", "--require-run-keys"])
        .status
        .success());
}

#[test]
fn an_unkeyed_import_is_refused_by_the_keyed_scorer_and_never_aligned_by_position() {
    let fixture = Fixture::new();
    // No header row, so the wide format declares no run identity.
    let mut wide = String::new();
    for i in 0..40 {
        wide.push_str(&format!("{},{}\n", 0.001 + 0.0001 * i as f64, 0.002));
    }
    fixture.write("alpha.csv", &wide);
    let import = fixture.cli(&["import", "csv", "alpha.csv", "--out", "subs.json"]);
    assert_eq!(import.status.code(), Some(0));
    assert!(String::from_utf8_lossy(&import.stderr).contains("declared no run identity"));
    assert!(!fixture.read("subs.json").contains("run_keys"));

    // Unkeyed still scores on the legacy path, and is refused on the keyed one.
    assert_eq!(fixture.cli(&["score", "subs.json"]).status.code(), Some(0));
    refused(
        &fixture.cli(&["score", "subs.json", "--require-run-keys"]),
        "submitted an unkeyed run array",
    );
}

#[test]
fn a_partial_grid_is_refused_rather_than_scored_on_shared_support() {
    let fixture = Fixture::new();
    // `bh` never ran (w1, 1). Restricting to shared support would silently
    // drop that cell from every other entrant's evidence.
    fixture.write(
        "field.csv",
        &long_csv(&[
            ("mo", "w0", 0),
            ("mo", "w0", 1),
            ("mo", "w1", 0),
            ("mo", "w1", 1),
            ("bh", "w0", 0),
            ("bh", "w0", 1),
            ("bh", "w1", 0),
        ]),
    );
    assert_eq!(
        fixture
            .cli(&["import", "csv", "field.csv", "--out", "subs.json"])
            .status
            .code(),
        Some(0)
    );
    refused(
        &fixture.cli(&["score", "subs.json", "--require-run-keys"]),
        "is missing (window `w1`, seed 1)",
    );
}

#[test]
fn an_incomplete_seed_by_window_product_is_refused_even_when_agents_agree() {
    let fixture = Fixture::new();
    fixture.write(
        "field.csv",
        &long_csv(&[
            ("mo", "w0", 0),
            ("mo", "w0", 1),
            ("mo", "w1", 0),
            ("bh", "w0", 0),
            ("bh", "w0", 1),
            ("bh", "w1", 0),
        ]),
    );
    assert_eq!(
        fixture
            .cli(&["import", "csv", "field.csv", "--out", "subs.json"])
            .status
            .code(),
        Some(0)
    );
    refused(
        &fixture.cli(&["score", "subs.json", "--require-run-keys"]),
        "incomplete grid",
    );
}
