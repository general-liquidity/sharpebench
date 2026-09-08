//! Regressions for the two forecast-comparison defects in the 2026-09-07 audit.
//!
//! R06: the paired comparison checked contract identity, resolution time, instrument
//! and rule, but not agreement on the realized outcome, so flipping one document's
//! outcome under an identical contract digest was accepted and moved the reported
//! comparison. Bench takes documents from any producer, so it has to require
//! settlement agreement itself.
//!
//! R05: two valid one-contract documents produced a single settlement block, a
//! zero-width interval and `familywise_significant = true` at p = 1/2001, and raising
//! the replication count made that unsupported result look more significant.

use serde_json::Value;
use sharpebench_core::forecast::{
    ForecastContract, ForecastEvidence, ForecastProducer, ForecastResolution, ForecastRevision,
    ForecastRunIdentity, InformationExposure,
};
use sharpebench_core::{analyze_forecast_quality, parse_forecast_evidence, ForecastAnalysisConfig};

/// One binary-Brier document per agent. Contract `index` resolves on clock
/// `(index / 2 + 1) * 10`, so `n` contracts occupy `n.div_ceil(2)` settlement blocks.
fn document(agent: &str, probabilities: &[f64], outcomes: &[f64]) -> ForecastEvidence {
    assert_eq!(probabilities.len(), outcomes.len());
    let contracts: Vec<ForecastContract> = (0..probabilities.len())
        .map(|index| ForecastContract {
            schema_version: "sharpearena.forecast-contract.v1".to_string(),
            contract_id: format!("c{index}"),
            question: format!("question {index}"),
            instrument: if index % 2 == 0 { "ES" } else { "NQ" }.to_string(),
            target: "close_up".to_string(),
            kind: "probability".to_string(),
            opens_at: index as u64 * 2,
            deadline: index as u64 * 2 + 1,
            resolves_at: (index as u64 / 2 + 1) * 10,
            observation_source: "fixture:v1".to_string(),
            open_definition: "close at opens_at".to_string(),
            close_definition: "close at resolves_at".to_string(),
            unit: "binary".to_string(),
            scoring_rule: "binary_brier".to_string(),
            neutral_threshold: 0.001,
            boundary_ownership: "threshold is false".to_string(),
            missing_data_policy: "cancel".to_string(),
            fallback_policy: "cancel".to_string(),
            categories: vec![],
            interval_alpha: None,
        })
        .collect();
    let payload = serde_json::to_string(&serde_json::json!({
        "schema_version": "sharpe.forecast-evidence.v1",
        "producer": ForecastProducer {
            name: "sharpearena".to_string(),
            contract: "native".to_string(),
        },
        "generated_at": 1_000,
        "identity": ForecastRunIdentity {
            agent_id: agent.to_string(),
            model_id: format!("model-{agent}"),
            model_sha256: "a".repeat(64),
            scaffold_id: "scaffold".to_string(),
            scaffold_sha256: "b".repeat(64),
            prompt_sha256: "c".repeat(64),
            operator_id: "operator".to_string(),
            config_sha256: "d".repeat(64),
        },
        "contracts": contracts,
        "revisions": probabilities
            .iter()
            .enumerate()
            .map(|(index, probability)| ForecastRevision {
                revision_id: format!("claim-{index}:r0"),
                claim_id: format!("claim-{index}"),
                ordinal: 0,
                supersedes: None,
                contract_sha256: String::new(),
                prediction: vec![*probability],
                confidence: *probability,
                rationale: "evidence".to_string(),
                submitted_at: index as u64 * 2,
                status: "eligible".to_string(),
                reason: None,
                trigger_event_id: None,
                revision_reason: None,
                exposure: InformationExposure {
                    observed_at: index as u64 * 2,
                    market_snapshot_sha256: Some("e".repeat(64)),
                    consensus_visible: false,
                    consensus_snapshot_sha256: None,
                    source_ids: vec!["market".to_string()],
                },
                idempotency_key: format!("request-{index}"),
            })
            .collect::<Vec<_>>(),
        "resolutions": outcomes
            .iter()
            .enumerate()
            .map(|(index, outcome)| ForecastResolution {
                claim_id: format!("claim-{index}"),
                status: "resolved".to_string(),
                outcome: Some(Value::from(*outcome)),
                available_at: Some(contracts[index].resolves_at),
                recorded_at: 1_000,
                reason: None,
            })
            .collect::<Vec<_>>(),
    }))
    .expect("fixture serializes");

    // The revision digests are content addresses of the contracts above, so they are
    // filled in from the parser's own view rather than restated by hand.
    let mut value: Value = serde_json::from_str(&payload).expect("fixture is JSON");
    let digests = contract_digests(&value);
    for (index, digest) in digests.iter().enumerate() {
        value["revisions"][index]["contract_sha256"] = Value::String(digest.clone());
    }
    parse_forecast_evidence(&value.to_string()).expect("fixture is valid forecast evidence")
}

/// Restate the frozen contract encoding on the fixture side. `parse_forecast_evidence`
/// rejects the document if these digests disagree with the kernel's own.
fn contract_digests(value: &Value) -> Vec<String> {
    use sha2::{Digest, Sha256};

    value["contracts"]
        .as_array()
        .expect("contracts array")
        .iter()
        .map(|contract| {
            let mut preimage = String::new();
            canonical(contract, &mut preimage);
            format!("{:x}", Sha256::digest(preimage.as_bytes()))
        })
        .collect()
}

/// The canonical encoding the evidence contract fixes: sorted object keys, no
/// whitespace, Python exponent padding.
fn canonical(value: &Value, output: &mut String) {
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
                canonical(value, output);
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
                canonical(value, output);
            }
            output.push('}');
        }
    }
}

/// Eight contracts over four settlement blocks: enough support for the comparison to
/// be reported, so the numbers below are the reference for "a valid input is unchanged".
fn supported_field() -> [ForecastEvidence; 2] {
    [
        document(
            "left",
            &[0.81, 0.22, 0.73, 0.34, 0.66, 0.15, 0.59, 0.27],
            &[1.0, 0.0, 1.0, 0.0, 0.0, 1.0, 1.0, 0.0],
        ),
        document(
            "right",
            &[0.61, 0.42, 0.53, 0.54, 0.46, 0.35, 0.39, 0.47],
            &[1.0, 0.0, 1.0, 0.0, 0.0, 1.0, 1.0, 0.0],
        ),
    ]
}

const SUPPORTED_CONFIG: ForecastAnalysisConfig = ForecastAnalysisConfig {
    bootstrap_seed: 260_904,
    bootstrap_samples: 500,
    confidence: 0.95,
    familywise_alpha: 0.05,
    calibration_bins: 5,
};

#[test]
fn r06_flipped_outcome_under_an_identical_contract_is_refused() {
    let left = document("left", &[0.9, 0.1, 0.8, 0.2], &[1.0, 0.0, 1.0, 0.0]);
    let mut right = document("right", &[0.6, 0.4, 0.55, 0.45], &[1.0, 0.0, 1.0, 0.0]);

    // Same contract digest, opposite realized outcome. The contract is untouched.
    right.resolutions[0].outcome = Some(Value::from(0.0));

    let error = analyze_forecast_quality(&[left, right], ForecastAnalysisConfig::default())
        .expect_err("a disputed settlement cannot be differenced");
    assert!(
        error.0.contains("unequal realized outcomes"),
        "unexpected message: {}",
        error.0
    );
}

#[test]
fn r06_disagreeing_outcome_availability_is_refused() {
    let left = document("left", &[0.9, 0.1, 0.8, 0.2], &[1.0, 0.0, 1.0, 0.0]);
    let mut right = document("right", &[0.6, 0.4, 0.55, 0.45], &[1.0, 0.0, 1.0, 0.0]);

    // Same outcome, later claimed availability: a different settlement record.
    let available_at = right.resolutions[0].available_at.expect("resolved record");
    right.resolutions[0].available_at = Some(available_at + 1);

    let error = analyze_forecast_quality(&[left, right], ForecastAnalysisConfig::default())
        .expect_err("a disputed availability time cannot be differenced");
    assert!(
        error.0.contains("unequal outcome availability times"),
        "unexpected message: {}",
        error.0
    );
}

#[test]
fn r06_agreeing_settlement_still_produces_the_comparison() {
    let [left, right] = supported_field();
    let report = analyze_forecast_quality(&[left, right], SUPPORTED_CONFIG)
        .expect("agreeing settlement is comparable");
    assert_eq!(report.comparisons.len(), 1);
    assert_eq!(report.comparisons[0].n_contracts, 8);
    assert!(report.comparisons[0].inference_error.is_none());
}

#[test]
fn r05_one_settlement_block_withholds_the_interval_and_the_p_value() {
    let left = document("left", &[0.9], &[1.0]);
    let right = document("right", &[0.6], &[1.0]);
    let report = analyze_forecast_quality(&[left, right], ForecastAnalysisConfig::default())
        .expect("the field is well formed; only the inference is unsupported");
    let comparison = &report.comparisons[0];

    assert_eq!(comparison.n_settlement_blocks, 1);
    assert_eq!(comparison.confidence_lower, None);
    assert_eq!(comparison.confidence_upper, None);
    assert_eq!(comparison.raw_p_value, None);
    assert_eq!(comparison.holm_adjusted_p_value, None);
    assert!(!comparison.familywise_significant);
    let reason = comparison
        .inference_error
        .as_deref()
        .expect("the unavailability is stated, not coerced to a favourable number");
    assert!(reason.contains("1-block"), "unexpected message: {reason}");

    // The descriptive difference survives: 0.01 - 0.16 = -0.15 is still reported.
    assert!((comparison.mean_loss_difference - -0.15).abs() < 1e-12);
}

#[test]
fn r05_more_bootstrap_replications_do_not_manufacture_significance() {
    let field = || {
        [
            document("left", &[0.9], &[1.0]),
            document("right", &[0.6], &[1.0]),
        ]
    };
    let mut reasons = Vec::new();
    for bootstrap_samples in [2_000, 20_000, 200_000] {
        let report = analyze_forecast_quality(
            &field(),
            ForecastAnalysisConfig {
                bootstrap_samples,
                ..ForecastAnalysisConfig::default()
            },
        )
        .expect("the field is well formed");
        let comparison = &report.comparisons[0];
        assert_eq!(comparison.raw_p_value, None);
        assert!(!comparison.familywise_significant);
        reasons.push(comparison.inference_error.clone().expect("stated reason"));
    }
    // Replications refine the same fixed resampling law; the verdict cannot move.
    assert_eq!(reasons[0], reasons[1]);
    assert_eq!(reasons[1], reasons[2]);
}

#[test]
fn r05_three_settlement_blocks_are_below_the_declared_requirement() {
    // Six contracts, three blocks: single-block resamples still hold 1/9 of the
    // resampling law's mass, which is coarser than the 0.05 level being claimed.
    let left = document(
        "left",
        &[0.9, 0.1, 0.85, 0.15, 0.8, 0.2],
        &[1.0, 0.0, 1.0, 0.0, 1.0, 0.0],
    );
    let right = document(
        "right",
        &[0.6, 0.4, 0.55, 0.45, 0.5, 0.5],
        &[1.0, 0.0, 1.0, 0.0, 1.0, 0.0],
    );
    let comparison = analyze_forecast_quality(&[left, right], ForecastAnalysisConfig::default())
        .expect("the field is well formed")
        .comparisons
        .remove(0);
    assert_eq!(comparison.n_settlement_blocks, 3);
    assert_eq!(comparison.raw_p_value, None);
    assert!(!comparison.familywise_significant);
    assert!(comparison
        .inference_error
        .expect("stated reason")
        .contains("3-block"));
}

/// The published numbers for a supported comparison must not move. These are the
/// exact bit patterns the pre-repair kernel produced for this field.
#[test]
fn valid_supported_comparison_is_bit_for_bit_unchanged() {
    let [left, right] = supported_field();
    let comparison = analyze_forecast_quality(&[left, right], SUPPORTED_CONFIG)
        .expect("supported field")
        .comparisons
        .remove(0);

    assert_eq!(comparison.agent_a, "left");
    assert_eq!(comparison.agent_b, "right");
    assert_eq!(comparison.n_contracts, 8);
    assert_eq!(comparison.n_settlement_blocks, 4);
    assert_eq!(comparison.mean_loss_difference, -0.049499999999999995);
    assert_eq!(comparison.confidence_lower, Some(-0.16899999999999996));
    assert_eq!(comparison.confidence_upper, Some(0.15599999999999997));
    assert_eq!(comparison.raw_p_value, Some(0.5828343313373253));
    assert_eq!(comparison.holm_adjusted_p_value, Some(0.5828343313373253));
    assert!(!comparison.familywise_significant);
    assert_eq!(comparison.inference_error, None);
}

/// A supported comparison serializes to the same bytes it did before the repair: the
/// four inference fields carry their values and `inference_error` is omitted. This is
/// the string the pre-repair kernel emitted for this field.
#[test]
fn supported_comparison_json_is_byte_identical() {
    let [left, right] = supported_field();
    let report = analyze_forecast_quality(&[left, right], SUPPORTED_CONFIG).expect("supported");
    let encoded = serde_json::to_string(&report.comparisons[0]).expect("comparison serializes");
    assert_eq!(
        encoded,
        r#"{"agent_a":"left","agent_b":"right","n_contracts":8,"n_settlement_blocks":4,"mean_loss_difference":-0.049499999999999995,"confidence_lower":-0.16899999999999996,"confidence_upper":0.15599999999999997,"raw_p_value":0.5828343313373253,"holm_adjusted_p_value":0.5828343313373253,"familywise_significant":false}"#
    );
}

/// Single-agent scoring never reaches the comparison path, so its published summary
/// is untouched by either repair.
#[test]
fn single_agent_summary_is_unchanged() {
    let document = document("a", &[0.8, 0.2, 0.7, 0.1], &[1.0, 0.0, 1.0, 0.0]);
    let report = analyze_forecast_quality(&[document], ForecastAnalysisConfig::default())
        .expect("single agent");
    let summary = &report.agents[0];
    assert_eq!(summary.metrics[0].mean_loss, 0.045000000000000005);
    let calibration = summary.binary_calibration.as_ref().expect("calibration");
    assert_eq!(calibration.brier, 0.045000000000000005);
    assert_eq!(calibration.reliability, 0.045000000000000005);
    assert_eq!(calibration.resolution, 0.25);
    assert_eq!(calibration.brier_skill, Some(0.82));
    assert!(report.comparisons.is_empty());
}
