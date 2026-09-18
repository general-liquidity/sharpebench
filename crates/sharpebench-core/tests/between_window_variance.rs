//! The between-window share of pooled variance, an opt-in rank-neutral
//! diagnostic.
//!
//! A pooled track is the concatenation of its window segments, so its sum of
//! squares splits exactly into the part between windows and the part within
//! them, `SST = SSB + SSW`. The share `SSB / SST` is near 1 for a track whose
//! dispersion is mostly level differences between windows. That is the shape a
//! track whose every window is constant produces (the kernel refuses those), and
//! the shape of its near relative the refusal cannot see: one differing
//! observation inside a window is enough to clear exact value equality while
//! leaving the dispersion almost all between windows.
//!
//! These tests request the diagnostic by its command-line identifier and read
//! the report as JSON, so they compile against the tree that had no such
//! diagnostic and fail there on their assertions.

use serde_json::Value;
use sharpebench_core::{
    rank, sharpe_diagnostics, AgentSubmission, Run, ScoreConfig, SharpeDiagnostic,
};

const ID: &str = "between-window-variance";

fn agent(id: &str, windows: Vec<Vec<f64>>) -> AgentSubmission {
    AgentSubmission {
        agent_id: id.into(),
        runs: windows
            .into_iter()
            .map(|returns| Run {
                returns,
                ..Run::default()
            })
            .collect(),
        in_sample_trials: 0,
        candidates: Vec::new(),
    }
}

/// The diagnostic for every row, as the report serializes it.
fn report(field: &[AgentSubmission]) -> Vec<Value> {
    let cfg = ScoreConfig::default();
    let requested = SharpeDiagnostic::parse_list(ID).expect("the diagnostic is requestable by id");
    let board = rank(field, &cfg);
    let before = serde_json::to_string(&board).unwrap();
    let diags = sharpe_diagnostics(field, &board, &cfg, &requested);
    assert_eq!(
        serde_json::to_string(&board).unwrap(),
        before,
        "a diagnostic never writes the board"
    );
    diags
        .iter()
        .map(|d| serde_json::to_value(d).unwrap())
        .collect()
}

fn share(row: &Value) -> Option<f64> {
    row["between_window_variance"]["between_window_share"].as_f64()
}

#[test]
fn the_share_is_the_exact_one_way_split() {
    // Windows [0.01, 0.03] and [0.05, 0.07]: grand mean 0.04, window means 0.02
    // and 0.06. SSB = 2 * 0.02^2 + 2 * 0.02^2 = 0.0016 and SSW = 4 * 0.01^2 =
    // 0.0004, so SSB / SST = 0.0016 / 0.0020 = 0.8. Equal windows split nothing.
    let rows = report(&[
        agent("split", vec![vec![0.01, 0.03], vec![0.05, 0.07]]),
        agent("level", vec![vec![0.01, 0.03], vec![0.01, 0.03]]),
    ]);
    let by_id = |id: &str| rows.iter().find(|r| r["agent_id"] == id).unwrap().clone();
    let split = by_id("split");
    assert_eq!(split["used_by_gate"], false);
    assert_eq!(split["between_window_variance"]["windows"], 2);
    assert!((share(&split).unwrap() - 0.8).abs() < 1e-12, "{split}");
    assert!(share(&by_id("level")).unwrap().abs() < 1e-12);
}

/// The two shapes the diagnostic exists to show apart from honest trading.
#[test]
fn level_differences_between_windows_read_near_one() {
    let wave = |phase: f64| -> Vec<f64> {
        (0..60)
            .map(|t| 0.001 + 0.01 * (0.9 * f64::from(t) + phase).sin())
            .collect()
    };
    let mut nudged = vec![0.001; 30];
    nudged[29] = 0.001 + 1e-12;
    let rows = report(&[
        agent("window-constant", vec![vec![0.001; 30], vec![0.002; 30]]),
        agent("one-bar-nudged", vec![nudged, vec![0.002; 30]]),
        agent("honest", vec![wave(0.0), wave(1.7)]),
    ]);
    let by_id = |id: &str| share(rows.iter().find(|r| r["agent_id"] == id).unwrap());
    // Refused as having no Sharpe ratio, and still described: all between.
    assert!(by_id("window-constant").unwrap() > 1.0 - 1e-9);
    // Clears exact value equality with one bar, and reads the same.
    assert!(by_id("one-bar-nudged").unwrap() > 1.0 - 1e-9);
    // An honest traded track's dispersion is inside its windows.
    assert!(by_id("honest").unwrap() < 0.05, "{:?}", by_id("honest"));
}

#[test]
fn a_track_the_split_cannot_describe_says_why() {
    let rows = report(&[
        agent("one-window", vec![vec![0.01, -0.02, 0.03]]),
        agent("flat", vec![vec![0.0; 10], vec![0.0; 10]]),
    ]);
    for row in &rows {
        let d = &row["between_window_variance"];
        assert!(d["between_window_share"].is_null(), "{row}");
        assert!(d["error"].is_string(), "{row}");
    }
}

/// Not requested, not present: the record the other diagnostics already
/// produce does not change for a caller who does not ask for this one.
#[test]
fn an_unrequested_diagnostic_is_absent() {
    let field = [agent("a", vec![vec![0.01, 0.03], vec![0.05, 0.07]])];
    let cfg = ScoreConfig::default();
    let board = rank(&field, &cfg);
    let requested = SharpeDiagnostic::parse_list("expected-shortfall").unwrap();
    let row =
        serde_json::to_value(&sharpe_diagnostics(&field, &board, &cfg, &requested)[0]).unwrap();
    assert!(row.get("between_window_variance").is_none(), "{row}");
}

/// A board row with no submission behind it says so, rather than reporting a
/// track with too few windows that was never read.
#[test]
fn a_row_with_no_submission_says_so() {
    let field = [agent("a", vec![vec![0.01, 0.03], vec![0.05, 0.07]])];
    let cfg = ScoreConfig::default();
    let board = rank(&field, &cfg);
    let requested = SharpeDiagnostic::parse_list(ID).unwrap();
    let row = serde_json::to_value(&sharpe_diagnostics(&[], &board, &cfg, &requested)[0]).unwrap();
    let d = &row["between_window_variance"];
    assert!(d["between_window_share"].is_null(), "{row}");
    assert_eq!(d["error"], "no submission in the field has this agent_id");
}
