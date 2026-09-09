//! Regressions for the contract-digest migration (R07, Bench side).
//!
//! A contract carrying `neutral_threshold = 1e-5` hashed by a Python producer was
//! refused as an unknown digest because the two languages render that number
//! differently. `sharpebench/canonical-json/v1` fixes one numeric form and frames
//! the pre-image with its version. The migration is dual-accept, not replacement:
//! `paper/evidence/prospective-forecast-field/` pins 24 digests under the
//! pre-migration encoding, and none of them recomputes under v1 (`0.0` renders
//! `0.0` there and `0` under v1). A legacy digest is accepted only when it matches
//! the legacy recomputation, the report says which encoding each scored digest
//! verified under, and a digest matching neither is refused.

use serde_json::Value;
use sha2::{Digest, Sha256};
use sharpebench_core::{
    analyze_forecast_quality, parse_forecast_evidence, ContractDigestVersion,
    ForecastAnalysisConfig,
};
use sharpebench_protocol::canonical::{canonical_json, versioned_preimage};

/// A one-contract binary-Brier document whose revision digest is `digest(contract)`.
fn document(neutral_threshold: f64, digest: impl Fn(&Value) -> String) -> Value {
    let mut value = serde_json::json!({
        "schema_version": "sharpe.forecast-evidence.v1",
        "producer": {"name": "sharpearena", "contract": "native"},
        "generated_at": 1_000,
        "identity": {
            "agent_id": "agent",
            "model_id": "model",
            "model_sha256": "a".repeat(64),
            "scaffold_id": "scaffold",
            "scaffold_sha256": "b".repeat(64),
            "prompt_sha256": "c".repeat(64),
            "operator_id": "operator",
            "config_sha256": "d".repeat(64),
        },
        "contracts": [{
            "schema_version": "sharpearena.forecast-contract.v1",
            "contract_id": "c0",
            "question": "question 0",
            "instrument": "ES",
            "target": "close_up",
            "kind": "probability",
            "opens_at": 0,
            "deadline": 1,
            "resolves_at": 10,
            "observation_source": "fixture:v1",
            "open_definition": "close at opens_at",
            "close_definition": "close at resolves_at",
            "unit": "binary",
            "scoring_rule": "binary_brier",
            "neutral_threshold": neutral_threshold,
            "boundary_ownership": "threshold is false",
            "missing_data_policy": "cancel",
            "fallback_policy": "cancel",
            "categories": [],
            "interval_alpha": null,
        }],
        "revisions": [{
            "revision_id": "claim-0:r0",
            "claim_id": "claim-0",
            "ordinal": 0,
            "supersedes": null,
            "contract_sha256": "",
            "prediction": [0.7],
            "confidence": 0.7,
            "rationale": "evidence",
            "submitted_at": 0,
            "status": "eligible",
            "reason": null,
            "trigger_event_id": null,
            "revision_reason": null,
            "exposure": {
                "observed_at": 0,
                "market_snapshot_sha256": "e".repeat(64),
                "consensus_visible": false,
                "consensus_snapshot_sha256": null,
                "source_ids": ["market"],
            },
            "idempotency_key": "request-0",
        }],
        "resolutions": [{
            "claim_id": "claim-0",
            "status": "resolved",
            "outcome": 1.0,
            "available_at": 10,
            "recorded_at": 1_000,
            "reason": null,
        }],
    });
    let contract_digest = digest(&value["contracts"][0]);
    value["revisions"][0]["contract_sha256"] = Value::String(contract_digest);
    value
}

fn sha256_hex(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

/// The current digest: SHA-256 over the versioned `canonical-json/v1` frame.
fn v1_digest(contract: &Value) -> String {
    sha256_hex(&versioned_preimage(contract).expect("finite contract"))
}

/// The pre-migration digest, restated on the fixture side: sorted keys, no
/// whitespace, `serde_json` number text with a two-digit padded exponent.
fn legacy_digest(contract: &Value) -> String {
    let mut text = String::new();
    legacy_canonical(contract, &mut text);
    sha256_hex(text.as_bytes())
}

fn legacy_canonical(value: &Value, output: &mut String) {
    match value {
        Value::Null => output.push_str("null"),
        Value::Bool(value) => output.push_str(if *value { "true" } else { "false" }),
        Value::Number(number) => {
            let rendered = number.to_string();
            match rendered.split_once('e') {
                None => output.push_str(&rendered),
                Some((mantissa, exponent)) => {
                    let (sign, digits) = match exponent.strip_prefix('-') {
                        Some(rest) => ("-", rest),
                        None => ("+", exponent.strip_prefix('+').unwrap_or(exponent)),
                    };
                    output.push_str(&format!("{mantissa}e{sign}{digits:0>2}"));
                }
            }
        }
        Value::String(value) => {
            output.push_str(&serde_json::to_string(value).expect("string encodes"))
        }
        Value::Array(values) => {
            output.push('[');
            for (index, value) in values.iter().enumerate() {
                if index > 0 {
                    output.push(',');
                }
                legacy_canonical(value, output);
            }
            output.push(']');
        }
        Value::Object(values) => {
            output.push('{');
            let mut fields: Vec<_> = values.iter().collect();
            fields.sort_by(|left, right| left.0.cmp(right.0));
            for (index, (key, value)) in fields.into_iter().enumerate() {
                if index > 0 {
                    output.push(',');
                }
                output.push_str(&serde_json::to_string(key).expect("key encodes"));
                output.push(':');
                legacy_canonical(value, output);
            }
            output.push('}');
        }
    }
}

fn digest_versions(document: Value) -> Vec<(String, ContractDigestVersion)> {
    let evidence = parse_forecast_evidence(&document.to_string()).expect("valid evidence");
    analyze_forecast_quality(&[evidence], ForecastAnalysisConfig::default())
        .expect("one agent scores")
        .contract_digest_versions
        .into_iter()
        .collect()
}

#[test]
fn r07_small_threshold_contract_verifies_under_canonical_json_v1() {
    // The reproduced defect: 1e-5 is `1e-05` to Python and `0.00001` to Rust.
    // Under v1 there is one form, and the framed digest of it is accepted.
    let document = document(1e-5, v1_digest);
    let text = canonical_json(&document["contracts"][0]).expect("finite contract");
    assert!(
        text.contains("\"neutral_threshold\":0.00001,"),
        "v1 renders the threshold in fixed point: {text}"
    );
    let versions = digest_versions(document.clone());
    assert_eq!(
        versions,
        vec![(
            v1_digest(&document["contracts"][0]),
            ContractDigestVersion::CanonicalJsonV1
        )]
    );
    assert_eq!(
        serde_json::to_value(ContractDigestVersion::CanonicalJsonV1).unwrap(),
        Value::String("sharpebench/canonical-json/v1".to_string())
    );
}

#[test]
fn a_legacy_digest_verifies_and_is_labelled_legacy() {
    let document = document(0.001, legacy_digest);
    let legacy = legacy_digest(&document["contracts"][0]);
    assert_ne!(
        legacy,
        v1_digest(&document["contracts"][0]),
        "the two encodings never agree on a contract, so the label is decisive"
    );
    let versions = digest_versions(document);
    assert_eq!(versions, vec![(legacy, ContractDigestVersion::Legacy)]);
    assert_eq!(
        serde_json::to_value(ContractDigestVersion::Legacy).unwrap(),
        Value::String("legacy".to_string())
    );
}

#[test]
fn a_digest_matching_neither_encoding_is_refused() {
    // The digest a Python producer takes with `json.dumps(sort_keys=True,
    // separators=(",", ":"))`: the same members, with the threshold rendered
    // `1e-05`. That is the R07 input, and it recomputes under neither encoding.
    let python_convention = |contract: &Value| {
        let v1 = canonical_json(contract).expect("finite contract");
        assert!(v1.contains("\"neutral_threshold\":0.00001,"));
        sha256_hex(
            v1.replace(
                "\"neutral_threshold\":0.00001,",
                "\"neutral_threshold\":1e-05,",
            )
            .as_bytes(),
        )
    };
    let document = document(1e-5, python_convention);
    let error = parse_forecast_evidence(&document.to_string())
        .expect_err("a Python-convention digest is neither encoding");
    assert!(
        error.0.contains("unknown contract digest")
            && error.0.contains("sharpebench/canonical-json/v1")
            && error.0.contains("legacy"),
        "unexpected message: {}",
        error.0
    );
}

#[test]
fn one_contract_under_both_digests_is_still_one_contract_per_agent() {
    // Dual acceptance must not open a second effective claim on the same
    // contract by naming it under its other digest.
    let mut document = document(0.001, legacy_digest);
    let mut second = document["revisions"][0].clone();
    second["revision_id"] = Value::String("claim-1:r0".to_string());
    second["claim_id"] = Value::String("claim-1".to_string());
    second["idempotency_key"] = Value::String("request-1".to_string());
    second["contract_sha256"] = Value::String(v1_digest(&document["contracts"][0]));
    document["revisions"]
        .as_array_mut()
        .expect("revisions")
        .push(second);
    let mut resolution = document["resolutions"][0].clone();
    resolution["claim_id"] = Value::String("claim-1".to_string());
    document["resolutions"]
        .as_array_mut()
        .expect("resolutions")
        .push(resolution);

    let evidence = parse_forecast_evidence(&document.to_string()).expect("both digests parse");
    let error = analyze_forecast_quality(&[evidence], ForecastAnalysisConfig::default())
        .expect_err("two effective claims on one contract");
    assert!(
        error
            .0
            .contains("multiple effective claims for the same contract"),
        "unexpected message: {}",
        error.0
    );
}

/// The same document as `document`, as a `sharpe.forecast-evidence.v2` envelope
/// declaring `encoding` on its one revision.
fn v2_document(neutral_threshold: f64, digest: impl Fn(&Value) -> String, encoding: &str) -> Value {
    let mut value = document(neutral_threshold, digest);
    value["schema_version"] = Value::String("sharpe.forecast-evidence.v2".to_string());
    value["revisions"][0]["contract_digest_encoding"] = Value::String(encoding.to_string());
    value
}

#[test]
fn v2_with_a_correct_declaration_verifies_under_the_declared_encoding() {
    let under_v1 = v2_document(1e-5, v1_digest, "sharpebench/canonical-json/v1");
    assert_eq!(
        digest_versions(under_v1.clone()),
        vec![(
            v1_digest(&under_v1["contracts"][0]),
            ContractDigestVersion::CanonicalJsonV1
        )]
    );
    let under_legacy = v2_document(0.001, legacy_digest, "legacy");
    assert_eq!(
        digest_versions(under_legacy.clone()),
        vec![(
            legacy_digest(&under_legacy["contracts"][0]),
            ContractDigestVersion::Legacy
        )]
    );
}

#[test]
fn v2_with_the_wrong_declaration_is_refused_naming_both_encodings() {
    // A legacy digest labelled v1: v1 documents would accept it by inference,
    // v2 verifies under the declaration only and says what the digest is.
    let document = v2_document(0.001, legacy_digest, "sharpebench/canonical-json/v1");
    let legacy = legacy_digest(&document["contracts"][0]);
    let error = parse_forecast_evidence(&document.to_string())
        .expect_err("a mislabelled digest is refused");
    assert_eq!(
        error.0,
        format!(
            "revision claim-0:r0 declares contract digest {legacy} under \
             sharpebench/canonical-json/v1, but it recomputes under legacy"
        )
    );

    let document = v2_document(1e-5, v1_digest, "legacy");
    let v1 = v1_digest(&document["contracts"][0]);
    let error = parse_forecast_evidence(&document.to_string())
        .expect_err("a mislabelled digest is refused");
    assert_eq!(
        error.0,
        format!(
            "revision claim-0:r0 declares contract digest {v1} under legacy, \
             but it recomputes under sharpebench/canonical-json/v1"
        )
    );
}

#[test]
fn v2_with_a_digest_matching_no_contract_names_neither_encoding() {
    let document = v2_document(0.001, |_| "f".repeat(64), "legacy");
    let error =
        parse_forecast_evidence(&document.to_string()).expect_err("an unknown digest is refused");
    assert_eq!(
        error.0,
        format!(
            "revision claim-0:r0 declares contract digest {} under legacy, but it \
             recomputes under neither sharpebench/canonical-json/v1 nor the legacy \
             encoding of any contract",
            "f".repeat(64)
        )
    );
}

#[test]
fn v2_with_an_unknown_encoding_label_is_refused() {
    let document = v2_document(0.001, legacy_digest, "sharpebench/canonical-json/v2");
    let error = parse_forecast_evidence(&document.to_string())
        .expect_err("an unknown encoding label is refused");
    assert_eq!(
        error.0,
        "unknown contract_digest_encoding \"sharpebench/canonical-json/v2\"; expected \
         sharpebench/canonical-json/v1 or legacy"
    );
}

#[test]
fn v2_without_the_declaration_is_refused() {
    let mut document = document(0.001, legacy_digest);
    document["schema_version"] = Value::String("sharpe.forecast-evidence.v2".to_string());
    let error =
        parse_forecast_evidence(&document.to_string()).expect_err("v2 must declare the encoding");
    assert_eq!(
        error.0,
        "sharpe.forecast-evidence.v2 requires contract_digest_encoding on every revision"
    );
}

#[test]
fn v1_with_the_declaration_present_is_refused() {
    // The v1 field set is exact: the declaration exists only under the v2 envelope,
    // even when it is correct.
    let mut document = document(0.001, legacy_digest);
    document["revisions"][0]["contract_digest_encoding"] = Value::String("legacy".to_string());
    let error = parse_forecast_evidence(&document.to_string())
        .expect_err("v1 does not carry the declaration");
    assert_eq!(
        error.0,
        "contract_digest_encoding is not a field of sharpe.forecast-evidence.v1; \
         a document that declares it must be sharpe.forecast-evidence.v2"
    );
}

/// The frozen field keeps verifying: all 24 pinned digests are accepted, every one
/// of them under the legacy encoding, and they are exactly the published support.
#[test]
fn the_frozen_prospective_field_verifies_with_24_legacy_digests() {
    let documents = [
        include_str!("../../../paper/evidence/prospective-forecast-field/resolved/phi-4.json"),
        include_str!("../../../paper/evidence/prospective-forecast-field/resolved/qwen-0.5b.json"),
        include_str!("../../../paper/evidence/prospective-forecast-field/resolved/qwen-7b.json"),
    ]
    .map(|payload| parse_forecast_evidence(payload).expect("frozen ledger still parses"));
    let published: Value = serde_json::from_str(include_str!(
        "../../../paper/evidence/prospective-forecast-field/report.json"
    ))
    .expect("frozen report is JSON");

    let report = analyze_forecast_quality(
        &documents,
        ForecastAnalysisConfig {
            bootstrap_seed: 260_904,
            bootstrap_samples: 2_000,
            confidence: 0.95,
            familywise_alpha: 0.05,
            calibration_bins: 5,
        },
    )
    .expect("frozen field scores");

    assert_eq!(report.contract_digest_versions.len(), 24);
    assert!(report
        .contract_digest_versions
        .values()
        .all(|version| *version == ContractDigestVersion::Legacy));
    let pinned: Vec<Value> = report
        .contract_digest_versions
        .keys()
        .map(|digest| Value::String(digest.clone()))
        .collect();
    assert_eq!(
        published["common_support"]["contract_sha256"],
        Value::Array(pinned)
    );
    assert_eq!(report.common_support.n_contracts, 24);
}
