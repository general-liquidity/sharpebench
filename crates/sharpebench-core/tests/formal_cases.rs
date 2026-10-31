//! The shared conformance cases in `formal/cases/forecast.json`, checked against the
//! production forecast report.
//!
//! The Lean model in `formal/SharpeBenchFormal` evaluates the same file against its
//! `pairSupport` and `admittedDecision` while it builds (`SharpeBenchFormal/Cases.lean`).
//! This test runs each case through `analyze_forecast_quality` or
//! `analyze_forecast_quality_against_plan`: the differenced contract count must be the
//! length of the model's pair support, and inference must be refused for unequal
//! support exactly when the model's gate refuses it. One table, two implementations,
//! so a change to either that the other does not share fails a build.

use std::path::PathBuf;

use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use sharpebench_core::forecast::{
    analyze_forecast_quality_against_plan, parse_forecast_contract_plan,
};
use sharpebench_core::{
    analyze_forecast_quality, parse_forecast_evidence, ForecastAnalysisConfig, ForecastEvidence,
};
use sharpebench_protocol::canonical::versioned_preimage;

const CONFIG: ForecastAnalysisConfig = ForecastAnalysisConfig {
    bootstrap_seed: 20_261_031,
    bootstrap_samples: 200,
    confidence: 0.95,
    familywise_alpha: 0.05,
    calibration_bins: 5,
};

fn cases() -> Value {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../formal/cases/forecast.json");
    let text = std::fs::read_to_string(&path).expect("formal/cases/forecast.json is readable");
    serde_json::from_str(&text).expect("formal/cases/forecast.json is JSON")
}

fn indices(value: &Value) -> Vec<u64> {
    value
        .as_array()
        .expect("a list of contract indices")
        .iter()
        .map(|index| index.as_u64().expect("a contract index"))
        .collect()
}

fn opens_at(index: u64) -> u64 {
    10 + index * 2
}

/// One contract per settlement block, so no case depends on how contracts group.
fn resolves_at(index: u64) -> u64 {
    (index + 1) * 100
}

fn contract(index: u64) -> Value {
    json!({
        "schema_version": "sharpearena.forecast-contract.v1",
        "contract_id": format!("c{index}"),
        "question": format!("question {index}"),
        "instrument": "ES",
        "target": "close_up",
        "kind": "probability",
        "opens_at": opens_at(index),
        "deadline": opens_at(index) + 1,
        "resolves_at": resolves_at(index),
        "observation_source": "fixture:v1",
        "open_definition": "close at opens_at",
        "close_definition": "close at resolves_at",
        "unit": "binary",
        "scoring_rule": "binary_brier",
        "neutral_threshold": 0.001,
        "boundary_ownership": "threshold is false",
        "missing_data_policy": "cancel",
        "fallback_policy": "cancel",
        "categories": [],
        "interval_alpha": null,
    })
}

fn digest(index: u64) -> String {
    Sha256::digest(versioned_preimage(&contract(index)).expect("finite contract"))
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// A document in which `agent` resolved exactly the contracts `resolved`.
fn document(agent: &str, resolved: &[u64], lean: f64) -> ForecastEvidence {
    let mut revisions = Vec::new();
    let mut resolutions = Vec::new();
    for (ordinal, index) in resolved.iter().enumerate() {
        let claim_id = format!("claim-{ordinal}");
        let outcome = if index % 2 == 0 { 1.0 } else { 0.0 };
        let probability = if outcome == 1.0 {
            0.5 + lean
        } else {
            0.5 - lean
        };
        revisions.push(json!({
            "revision_id": format!("{claim_id}:r0"),
            "claim_id": claim_id,
            "ordinal": 0,
            "supersedes": null,
            "contract_sha256": digest(*index),
            "contract_digest_encoding": "sharpebench/canonical-json/v1",
            "prediction": [probability],
            "confidence": probability,
            "rationale": "evidence",
            "submitted_at": opens_at(*index),
            "status": "eligible",
            "reason": null,
            "trigger_event_id": null,
            "revision_reason": null,
            "exposure": {
                "observed_at": opens_at(*index),
                "market_snapshot_sha256": "e".repeat(64),
                "consensus_visible": false,
                "consensus_snapshot_sha256": null,
                "source_ids": ["market"],
            },
            "idempotency_key": format!("request-{ordinal}"),
        }));
        resolutions.push(json!({
            "claim_id": claim_id,
            "status": "resolved",
            "outcome": outcome,
            "available_at": resolves_at(*index),
            "recorded_at": 5_000,
            "reason": null,
        }));
    }
    let value = json!({
        "schema_version": "sharpe.forecast-evidence.v2",
        "producer": {"name": "sharpearena", "contract": "native"},
        "generated_at": 5_000,
        "identity": {
            "agent_id": agent,
            "model_id": format!("model-{agent}"),
            "model_sha256": "a".repeat(64),
            "scaffold_id": "scaffold",
            "scaffold_sha256": "b".repeat(64),
            "prompt_sha256": "c".repeat(64),
            "operator_id": "operator",
            "config_sha256": "d".repeat(64),
        },
        "contracts": resolved.iter().map(|index| contract(*index)).collect::<Vec<_>>(),
        "revisions": revisions,
        "resolutions": resolutions,
    });
    parse_forecast_evidence(&value.to_string()).expect("case document is valid forecast evidence")
}

#[test]
fn production_pair_support_and_gate_match_every_shared_case() {
    let cases = cases();
    assert_eq!(cases["schema_version"], "sharpe.formal-cases.v1");
    let support = cases["pair_support"]
        .as_array()
        .expect("pair_support cases");
    assert!(
        !support.is_empty(),
        "the shared case table must not be empty"
    );
    for case in support {
        let name = case["name"].as_str().expect("case name");
        let left = indices(&case["left"]);
        let right = indices(&case["right"]);
        let expected = indices(&case["pair_support"]);
        let admitted = case["admitted"].as_bool().expect("admitted flag");
        let field = [document("a", &left, 0.4), document("b", &right, 0.1)];
        let report = if case["plan"].is_null() {
            analyze_forecast_quality(&field, CONFIG)
        } else {
            let plan = parse_forecast_contract_plan(
                &json!({
                    "schema_version": "sharpebench.forecast-contract-plan.v1",
                    "contract_sha256": indices(&case["plan"]).into_iter().map(digest).collect::<Vec<_>>(),
                })
                .to_string(),
            )
            .expect("case plan is valid");
            analyze_forecast_quality_against_plan(&field, CONFIG, &plan)
        }
        .unwrap_or_else(|error| panic!("{name}: the report refused the case: {error:?}"));
        let pair = report
            .comparisons
            .iter()
            .find(|comparison| comparison.agent_a == "a" && comparison.agent_b == "b")
            .unwrap_or_else(|| panic!("{name}: the pair is reported"));
        assert_eq!(
            pair.n_contracts,
            expected.len(),
            "{name}: differenced contracts disagree with the model's pairSupport"
        );
        assert_eq!(
            pair.support_gap.is_none(),
            admitted,
            "{name}: the unequal-support refusal disagrees with the model's admittedDecision"
        );
    }
}
