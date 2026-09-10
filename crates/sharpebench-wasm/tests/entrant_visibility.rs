//! The WASM board entry points (and so the npm `score` / `scoreAgent` calls and
//! the MCP `score` / `score_agent` tools, which forward to them) emit only
//! fields the entrant-visibility allowlist declares, and emit them unchanged.

use sharpebench_core::{
    parse_declared_field, rank_declared, score_agent, AgentSubmission, ScoreConfig, Visibility,
    COMPOSITE_SCORE_VISIBILITY,
};
use sharpebench_wasm::{score_agent_json, score_json};

fn field() -> String {
    let track = |mean: f64| {
        (0..60)
            .map(|i| mean + 0.001 * (i as f64 * 0.7).sin())
            .collect::<Vec<_>>()
    };
    serde_json::json!([
        {"agent_id":"candidate", "runs":[{"returns":track(0.01)}],
         "declared_mandate":{"kind":"relative_to", "benchmark_id":"reference"}},
        {"agent_id":"reference", "runs":[{"returns":track(0.02)}]}
    ])
    .to_string()
}

/// Every key a row carries must be declared, and a declared key must not be
/// one the allowlist withholds.
fn assert_declared(row: &serde_json::Map<String, serde_json::Value>) {
    for key in row.keys() {
        match COMPOSITE_SCORE_VISIBILITY.visibility(key) {
            Some(Visibility::Visible) | Some(Visibility::Nested(_)) => {}
            other => panic!("`{key}` reached the WASM surface as {other:?}"),
        }
    }
}

#[test]
fn the_board_surface_emits_only_declared_fields_and_unchanged_bytes() {
    let raw = field();
    let emitted = score_json(&raw, "").unwrap();
    let (subs, declarations) = parse_declared_field(&raw).unwrap();
    let native = rank_declared(&subs, &declarations, &ScoreConfig::default());
    assert_eq!(emitted, serde_json::to_string(&native).unwrap());
    let rows: Vec<serde_json::Value> = serde_json::from_str(&emitted).unwrap();
    assert!(rows.iter().any(|row| row.get("declared_mandate").is_some()));
    for row in &rows {
        assert_declared(row.as_object().unwrap());
    }
}

#[test]
fn the_single_row_surface_emits_only_declared_fields_and_unchanged_bytes() {
    let raw = field();
    let first: serde_json::Value =
        serde_json::from_str::<serde_json::Value>(&raw).unwrap()[1].clone();
    let emitted = score_agent_json(&first.to_string(), "").unwrap();
    let submission: AgentSubmission = serde_json::from_value(first).unwrap();
    assert_eq!(
        emitted,
        serde_json::to_string(&score_agent(&submission, &ScoreConfig::default())).unwrap()
    );
    let row: serde_json::Value = serde_json::from_str(&emitted).unwrap();
    assert_declared(row.as_object().unwrap());
}
