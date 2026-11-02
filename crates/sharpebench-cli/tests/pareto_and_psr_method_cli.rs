//! `sharpebench score --psr-standard-error`, `score --pareto` and
//! `sharpebench pareto`: opt-in, and without them the board is unchanged.

use std::path::PathBuf;
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};

struct Fixture(PathBuf);

impl Fixture {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        loop {
            let path = std::env::temp_dir().join(format!(
                "sharpebench-pareto-{}-{}",
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

fn field() -> Fixture {
    use sharpebench_core::{AgentSubmission, Run};
    let fx = Fixture::new();
    let good: Vec<f64> = (0..400)
        .map(|t| 0.001 + 0.01 * (1.3 * t as f64).sin())
        .collect();
    let bad: Vec<f64> = good.iter().map(|x| 2.0 * x - 0.02).collect();
    let sub = |id: &str, returns: Vec<f64>| AgentSubmission {
        agent_id: id.to_string(),
        runs: vec![Run {
            returns,
            ..Run::default()
        }],
        in_sample_trials: 0,
        candidates: Vec::new(),
    };
    let field = vec![sub("good", good), sub("bad", bad)];
    std::fs::write(
        fx.0.join("field.json"),
        serde_json::to_string(&field).unwrap(),
    )
    .unwrap();
    fx
}

fn stdout(out: &Output) -> String {
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout.clone()).unwrap()
}

#[test]
fn the_null_standard_error_is_opt_in_and_recorded_on_every_row() {
    let fx = field();
    let plain = stdout(&fx.cli(&["score", "field.json", "--json"]));
    let observed = stdout(&fx.cli(&[
        "score",
        "field.json",
        "--psr-standard-error",
        "observed",
        "--json",
    ]));
    assert_eq!(plain, observed);
    assert!(!plain.contains("psr_standard_error"));
    let null = stdout(&fx.cli(&[
        "score",
        "field.json",
        "--psr-standard-error",
        "benchmark",
        "--json",
    ]));
    let rows: serde_json::Value = serde_json::from_str(&null).unwrap();
    let rows = rows
        .as_array()
        .expect("the board-only output is the sealed row array");
    assert_eq!(rows.len(), 2);
    assert!(rows.iter().all(|r| r["psr_standard_error"] == "benchmark"));

    for bad in [
        vec!["--psr-standard-error", "null"],
        vec!["--psr-standard-error"],
    ] {
        let mut args = vec!["score", "field.json"];
        args.extend(bad);
        let out = fx.cli(&args);
        assert_eq!(out.status.code(), Some(2), "{args:?}");
        assert!(out.stdout.is_empty());
    }
}

#[test]
fn the_pareto_report_sits_beside_an_unchanged_board() {
    let fx = field();
    let plain: serde_json::Value =
        serde_json::from_str(&stdout(&fx.cli(&["score", "field.json", "--json"]))).unwrap();
    let with: serde_json::Value = serde_json::from_str(&stdout(&fx.cli(&[
        "score",
        "field.json",
        "--pareto",
        "--json",
    ])))
    .unwrap();
    assert_eq!(with["board"], plain);
    let rows = with["pareto"]["rows"].as_array().unwrap();
    assert_eq!(rows.len(), 2);
    let bad = rows.iter().find(|r| r["agent_id"] == "bad").unwrap();
    assert_eq!(bad["on_front"], false);
    assert_eq!(bad["dominated_by"], serde_json::json!(["good"]));
    assert_eq!(with["pareto"]["used_by_rank"], false);

    let report: serde_json::Value =
        serde_json::from_str(&stdout(&fx.cli(&["pareto", "field.json", "--json"]))).unwrap();
    assert_eq!(report["pareto"], with["pareto"]);

    let table = stdout(&fx.cli(&["score", "field.json", "--pareto"]));
    // Line 0 is the evaluation tier; the column header follows it.
    assert!(table.lines().nth(1).unwrap().ends_with("front elig_front"));
    assert!(table.contains("never a rank input"));
    let human = stdout(&fx.cli(&["pareto", "field.json"]));
    assert!(human.contains("dominated by"));
    assert!(human
        .lines()
        .any(|l| l.starts_with("bad ") && l.trim_end().ends_with("good")));
    assert_eq!(fx.cli(&["pareto"]).status.code(), Some(2));
}
