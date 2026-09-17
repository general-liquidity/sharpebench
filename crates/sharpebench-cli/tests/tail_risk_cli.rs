//! `sharpebench score --diagnostics expected-shortfall`: the historical
//! expected shortfall, its tail count and the loss frequency, reported beside
//! an unchanged board.

use std::path::PathBuf;
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};

use sharpebench_core::{AgentSubmission, Run};

struct Fixture(PathBuf);

impl Fixture {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        loop {
            let path = std::env::temp_dir().join(format!(
                "sharpebench-tail-risk-{}-{}",
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

    fn write(&self, name: &str, field: &[AgentSubmission]) {
        std::fs::write(self.0.join(name), serde_json::to_string(field).unwrap()).unwrap();
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

fn sub(id: &str, returns: Vec<f64>) -> AgentSubmission {
    AgentSubmission {
        agent_id: id.to_string(),
        runs: vec![Run {
            returns,
            ..Run::default()
        }],
        in_sample_trials: 0,
        candidates: Vec::new(),
    }
}

fn repeat(pattern: &[f64], times: usize) -> Vec<f64> {
    pattern
        .iter()
        .copied()
        .cycle()
        .take(pattern.len() * times)
        .collect()
}

/// Frequent small losses, rare large ones, and a flat track, 240 bars each
/// (a 5% tail of 12); and, in a second file, a 100-bar track (a tail of 5).
fn fixture() -> Fixture {
    let fx = Fixture::new();
    fx.write(
        "field.json",
        &[
            sub("often", repeat(&[-0.02, 0.02], 120)),
            sub(
                "rarely",
                repeat(&[-0.04, 0.04, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0], 30),
            ),
            sub("flat", vec![0.0; 240]),
        ],
    );
    fx.write(
        "short.json",
        &[sub("short", repeat(&[-0.03, 0.01, 0.02, -0.01], 25))],
    );
    fx
}

fn json(out: Output) -> serde_json::Value {
    assert_eq!(
        out.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    serde_json::from_slice(&out.stdout).unwrap()
}

#[test]
fn expected_shortfall_is_reported_beside_an_unchanged_board() {
    let fx = fixture();
    let plain: serde_json::Value =
        serde_json::from_slice(&fx.cli(&["score", "field.json", "--json"]).stdout).unwrap();
    let doc = json(fx.cli(&[
        "score",
        "field.json",
        "--diagnostics",
        "expected-shortfall",
        "--json",
    ]));
    assert_eq!(doc["board"], plain);
    let diags = doc["sharpe_diagnostics"].as_array().unwrap();
    assert_eq!(diags.len(), 3);
    let by_id = |id: &str| {
        &diags
            .iter()
            .find(|d| d["agent_id"] == id)
            .unwrap_or_else(|| panic!("{id}"))["expected_shortfall"]
    };
    for d in diags {
        assert_eq!(d["used_by_gate"], false);
        assert!(d.get("mppm").is_none());
        let t = &d["expected_shortfall"];
        assert_eq!(t["level"], 0.05);
        assert_eq!(t["min_tail_observations"], 10);
        assert_eq!(t["tail_size"], 12.0);
        assert_eq!(t["tail_observations"], 12);
        assert!(t.get("error").is_none());
    }
    let (often, rarely) = (by_id("often"), by_id("rarely"));
    assert!((often["tail_mean_return"].as_f64().unwrap() + 0.02).abs() <= 1e-15);
    assert!((rarely["tail_mean_return"].as_f64().unwrap() + 0.04).abs() <= 1e-15);
    assert_eq!(often["losses"], 120);
    assert_eq!(often["loss_frequency"], 0.5);
    assert_eq!(rarely["losses"], 30);
    assert_eq!(rarely["loss_frequency"], 0.125);
    // The board's downside deviation does not separate the two.
    let dd = |id: &str| {
        plain
            .as_array()
            .unwrap()
            .iter()
            .find(|r| r["agent_id"] == id)
            .unwrap()["downside_deviation"]
            .as_f64()
            .unwrap()
    };
    assert!((dd("often") - dd("rarely")).abs() <= 1e-15);
    assert_eq!(by_id("flat")["tail_mean_return"], 0.0);
    assert_eq!(by_id("flat")["loss_frequency"], 0.0);
}

#[test]
fn a_short_tail_prints_no_number_and_says_why() {
    let fx = fixture();
    let doc = json(fx.cli(&[
        "score",
        "short.json",
        "--diagnostics",
        "expected-shortfall",
        "--json",
    ]));
    let t = &doc["sharpe_diagnostics"][0]["expected_shortfall"];
    assert!(t.get("tail_mean_return").is_none());
    assert_eq!(t["tail_observations"], 5);
    assert_eq!(t["loss_frequency"], 0.5);
    assert!(t["error"]
        .as_str()
        .unwrap()
        .contains("at least 10 observations are required, got 5"));

    let plain = String::from_utf8(fx.cli(&["score", "short.json"]).stdout).unwrap();
    let out = fx.cli(&["score", "short.json", "--diagnostics", "expected-shortfall"]);
    assert_eq!(out.status.code(), Some(0));
    let text = String::from_utf8(out.stdout).unwrap();
    let rest = text
        .strip_prefix(plain.as_str())
        .expect("the board prints first, unchanged");
    assert!(rest.starts_with("\nOpt-in Sharpe diagnostics. Not used by the gate"));
    for word in ["ES(5%)", "tail", "loss_fq", "n/a", "0.5000"] {
        assert!(rest.contains(word), "{word}\n{rest}");
    }
}

#[test]
fn the_identifier_is_exact_and_combines_with_the_others() {
    let fx = fixture();
    for bad in ["expected_shortfall", "es", "cvar", "Expected-Shortfall"] {
        let out = fx.cli(&["score", "field.json", "--diagnostics", bad, "--json"]);
        assert_eq!(out.status.code(), Some(2), "{bad}");
        assert!(out.stdout.is_empty());
    }
    let doc = json(fx.cli(&[
        "score",
        "field.json",
        "--diagnostics",
        "expected-shortfall,mppm",
        "--json",
    ]));
    for d in doc["sharpe_diagnostics"].as_array().unwrap() {
        assert!(d.get("mppm").is_some());
        assert!(d.get("expected_shortfall").is_some());
        assert!(d.get("autocorrelated_psr").is_none());
    }
    let help = fx.cli(&["--help"]);
    let usage = format!(
        "{}{}",
        String::from_utf8_lossy(&help.stdout),
        String::from_utf8_lossy(&help.stderr)
    );
    assert!(usage.contains("mppm,expected-shortfall"), "{usage}");
}
