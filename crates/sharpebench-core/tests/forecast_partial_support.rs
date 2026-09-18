//! Regressions for partial forecast support (C3, 2026-09-16 reading).
//!
//! Before `sharpebench.forecast-quality.v2` every pair was compared on the contracts
//! resolved by the whole field. One document that abstained, or left settlements
//! pending, on contracts it would lose removed those contracts from every pair,
//! including pairs of two complete agents, and charged the exclusion to the complete
//! agents. A pair is now differenced on the contracts both agents resolved and
//! receives inference only when both resolved the same contracts; each agent's gaps
//! are charged to that agent.

use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use sharpebench_core::forecast::{
    analyze_forecast_quality_against_plan, parse_forecast_contract_plan, AgentUnresolvedSupport,
    ForecastContractPlan, PairwiseForecastComparison, SettlementStatusDisagreement, SupportGap,
};
use sharpebench_core::{
    analyze_forecast_quality, parse_forecast_evidence, ForecastAnalysisConfig, ForecastEvidence,
    ForecastQualityReport,
};
use sharpebench_protocol::canonical::versioned_preimage;

const CONFIG: ForecastAnalysisConfig = ForecastAnalysisConfig {
    bootstrap_seed: 20_260_916,
    bootstrap_samples: 2_000,
    confidence: 0.95,
    familywise_alpha: 0.05,
    calibration_bins: 5,
};

#[derive(Clone, Copy)]
enum Status {
    Resolved,
    Pending,
    Cancelled,
    Rejected,
}

/// One claim on contract `index`: the contract, its outcome and its settlement block
/// depend on `index` alone, so the same index is the same digest in every document.
#[derive(Clone, Copy)]
struct Claim {
    index: u64,
    probability: f64,
    status: Status,
}

fn opens_at(index: u64) -> u64 {
    10 + index * 2
}

/// Two contracts per settlement block.
fn resolves_at(index: u64) -> u64 {
    (index / 2 + 1) * 100
}

fn outcome(index: u64) -> f64 {
    if index.is_multiple_of(2) {
        1.0
    } else {
        0.0
    }
}

fn contract(index: u64) -> Value {
    json!({
        "schema_version": "sharpearena.forecast-contract.v1",
        "contract_id": format!("c{index}"),
        "question": format!("question {index}"),
        "instrument": if index.is_multiple_of(2) { "ES" } else { "NQ" },
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

fn document(agent: &str, claims: &[Claim]) -> ForecastEvidence {
    let mut indices: Vec<u64> = claims.iter().map(|claim| claim.index).collect();
    indices.sort_unstable();
    indices.dedup();
    let mut revisions = Vec::new();
    let mut resolutions = Vec::new();
    for (ordinal, claim) in claims.iter().enumerate() {
        let claim_id = format!("claim-{ordinal}");
        let rejected = matches!(claim.status, Status::Rejected);
        let submitted_at = if rejected {
            opens_at(claim.index) - 1
        } else {
            opens_at(claim.index)
        };
        revisions.push(json!({
            "revision_id": format!("{claim_id}:r0"),
            "claim_id": claim_id,
            "ordinal": 0,
            "supersedes": null,
            "contract_sha256": digest(claim.index),
            "contract_digest_encoding": "sharpebench/canonical-json/v1",
            "prediction": [claim.probability],
            "confidence": claim.probability,
            "rationale": "evidence",
            "submitted_at": submitted_at,
            "status": if rejected { "rejected" } else { "eligible" },
            "reason": if rejected { json!("submitted before the contract opened") } else { Value::Null },
            "trigger_event_id": null,
            "revision_reason": null,
            "exposure": {
                "observed_at": submitted_at,
                "market_snapshot_sha256": "e".repeat(64),
                "consensus_visible": false,
                "consensus_snapshot_sha256": null,
                "source_ids": ["market"],
            },
            "idempotency_key": format!("request-{ordinal}"),
        }));
        let (status, settled, available_at, reason) = match claim.status {
            Status::Resolved => (
                "resolved",
                json!(outcome(claim.index)),
                json!(resolves_at(claim.index)),
                Value::Null,
            ),
            Status::Pending => ("pending", Value::Null, Value::Null, Value::Null),
            Status::Cancelled => (
                "cancelled",
                Value::Null,
                Value::Null,
                json!("observation source outage"),
            ),
            Status::Rejected => (
                "rejected",
                Value::Null,
                Value::Null,
                json!("claim has no eligible submission"),
            ),
        };
        resolutions.push(json!({
            "claim_id": claim_id,
            "status": status,
            "outcome": settled,
            "available_at": available_at,
            "recorded_at": 5_000,
            "reason": reason,
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
        "contracts": indices.iter().map(|index| contract(*index)).collect::<Vec<_>>(),
        "revisions": revisions,
        "resolutions": resolutions,
    });
    parse_forecast_evidence(&value.to_string()).expect("fixture is valid forecast evidence")
}

fn resolved(indices: impl IntoIterator<Item = u64>, sharp: bool) -> Vec<Claim> {
    indices
        .into_iter()
        .map(|index| {
            // A sharp agent leans the right way by 0.4, a vague one by 0.1 to 0.32, so
            // the settlement blocks differ and the bootstrap has variation to resample.
            let lean = if sharp {
                0.4
            } else {
                0.1 + 0.02 * index as f64
            };
            Claim {
                index,
                probability: if outcome(index) == 1.0 {
                    0.5 + lean
                } else {
                    0.5 - lean
                },
                status: Status::Resolved,
            }
        })
        .collect()
}

/// Ten contracts in five settlement blocks, the whole field's question set.
fn complete(agent: &str, sharp: bool) -> ForecastEvidence {
    document(agent, &resolved(0..10, sharp))
}

/// Resolved on the first six contracts, then one gap of each kind.
fn partial(agent: &str) -> ForecastEvidence {
    let mut claims = resolved(0..6, true);
    for (index, status) in [
        (6, Status::Rejected),
        (7, Status::Cancelled),
        (8, Status::Pending),
    ] {
        claims.push(Claim {
            index,
            probability: 0.5,
            status,
        });
    }
    document(agent, &claims)
}

fn pair<'a>(report: &'a ForecastQualityReport, a: &str, b: &str) -> &'a PairwiseForecastComparison {
    report
        .comparisons
        .iter()
        .find(|comparison| comparison.agent_a == a && comparison.agent_b == b)
        .expect("pair is reported")
}

fn assert_same_inference(left: &PairwiseForecastComparison, right: &PairwiseForecastComparison) {
    assert_eq!(left.n_contracts, right.n_contracts);
    assert_eq!(left.n_settlement_blocks, right.n_settlement_blocks);
    assert_eq!(left.mean_loss_difference, right.mean_loss_difference);
    assert_eq!(left.confidence_lower, right.confidence_lower);
    assert_eq!(left.confidence_upper, right.confidence_upper);
    assert_eq!(left.raw_p_value, right.raw_p_value);
    assert_eq!(left.familywise_significant, right.familywise_significant);
    assert_eq!(left.support_gap, right.support_gap);
    assert_eq!(left.inference_error, right.inference_error);
}

#[test]
fn a_partial_third_document_leaves_the_complete_pair_unchanged() {
    let two = analyze_forecast_quality(&[complete("a", true), complete("b", false)], CONFIG)
        .expect("complete field");
    let baseline = pair(&two, "a", "b");
    assert_eq!(baseline.n_contracts, 10);
    assert_eq!(baseline.n_settlement_blocks, 5);
    assert!(baseline.inference_error.is_none());
    assert!(baseline.familywise_significant);

    for field in [
        [complete("a", true), complete("b", false), partial("c")],
        [partial("c"), complete("a", true), complete("b", false)],
    ] {
        let three = analyze_forecast_quality(&field, CONFIG).expect("partial field is reported");
        let comparison = pair(&three, "a", "b");
        assert_same_inference(baseline, comparison);
        // The partial pairs stay in the family: the multiplier is three, not one.
        let raw = comparison.raw_p_value.expect("supported pair");
        assert_eq!(comparison.holm_adjusted_p_value, Some((3.0 * raw).min(1.0)));
        assert_eq!(three.comparisons.len(), 3);
    }
}

#[test]
fn pairs_with_the_partial_agent_are_withheld_and_charged_to_it() {
    let report = analyze_forecast_quality(
        &[complete("a", true), complete("b", false), partial("c")],
        CONFIG,
    )
    .expect("partial field is reported");

    for (a, b) in [("a", "c"), ("b", "c")] {
        let comparison = pair(&report, a, b);
        assert_eq!(comparison.n_contracts, 6);
        assert_eq!(comparison.n_settlement_blocks, 3);
        assert_eq!(
            comparison.support_gap,
            Some(SupportGap {
                agent_a_unresolved: 0,
                agent_b_unresolved: 4,
            })
        );
        assert_eq!(comparison.confidence_lower, None);
        assert_eq!(comparison.confidence_upper, None);
        assert_eq!(comparison.raw_p_value, None);
        assert_eq!(comparison.holm_adjusted_p_value, None);
        assert!(!comparison.familywise_significant);
        assert!(comparison
            .inference_error
            .as_deref()
            .expect("withheld with a reason")
            .starts_with(
                "unequal resolved support: agent_a did not resolve 0 contract(s) that agent_b \
                 resolved and agent_b did not resolve 4 that agent_a resolved"
            ));
    }

    let support = &report.common_support;
    assert_eq!(report.schema_version, "sharpebench.forecast-quality.v2");
    assert_eq!(support.n_contracts, 10);
    assert_eq!(support.contract_sha256, {
        let mut all: Vec<String> = (0..10).map(digest).collect();
        all.sort();
        all
    });
    assert_eq!(
        support.unresolved_by_agent["a"],
        AgentUnresolvedSupport::default()
    );
    assert_eq!(
        support.unresolved_by_agent["b"],
        AgentUnresolvedSupport::default()
    );
    assert_eq!(
        support.unresolved_by_agent["c"],
        AgentUnresolvedSupport {
            n_unresolved: 4,
            not_claimed: vec![digest(9)],
            pending: vec![digest(8)],
            cancelled: vec![digest(7)],
            rejected: vec![digest(6)],
        }
    );
}

#[test]
fn settlement_status_disagreement_is_recorded_not_differenced() {
    let report = analyze_forecast_quality(
        &[complete("b", false), partial("c"), complete("a", true)],
        CONFIG,
    )
    .expect("a pending or cancelled settlement is disclosed, not refused");
    let mut expected = vec![
        SettlementStatusDisagreement {
            contract_sha256: digest(7),
            resolved_by: vec!["a".to_string(), "b".to_string()],
            pending_by: vec![],
            cancelled_by: vec!["c".to_string()],
        },
        SettlementStatusDisagreement {
            contract_sha256: digest(8),
            resolved_by: vec!["a".to_string(), "b".to_string()],
            pending_by: vec!["c".to_string()],
            cancelled_by: vec![],
        },
    ];
    expected.sort_by(|left, right| left.contract_sha256.cmp(&right.contract_sha256));
    // A rejected claim is the agent's own timing failure and a claim never made is
    // not a settlement record, so neither is a status disagreement.
    assert_eq!(
        report.common_support.settlement_status_disagreements,
        expected
    );
}

#[test]
fn the_support_section_does_not_depend_on_input_order() {
    let encode = |field: &[ForecastEvidence]| {
        serde_json::to_string(
            &analyze_forecast_quality(field, CONFIG)
                .expect("field is reported")
                .common_support,
        )
        .expect("support serializes")
    };
    let pending_twice = |agent: &str| {
        let mut claims = resolved(0..9, false);
        claims.push(Claim {
            index: 9,
            probability: 0.5,
            status: Status::Pending,
        });
        document(agent, &claims)
    };
    let cancelled_twice = |agent: &str| {
        let mut claims = resolved(0..9, false);
        claims.push(Claim {
            index: 9,
            probability: 0.5,
            status: Status::Cancelled,
        });
        document(agent, &claims)
    };
    let forward = encode(&[
        complete("a", true),
        complete("b", false),
        pending_twice("p1"),
        pending_twice("p2"),
        cancelled_twice("x1"),
        cancelled_twice("x2"),
    ]);
    let reverse = encode(&[
        cancelled_twice("x2"),
        cancelled_twice("x1"),
        pending_twice("p2"),
        pending_twice("p1"),
        complete("b", false),
        complete("a", true),
    ]);
    assert_eq!(forward, reverse);
    assert!(forward.contains(
        r#""resolved_by":["a","b"],"pending_by":["p1","p2"],"cancelled_by":["x1","x2"]"#
    ));
}

/// A union of resolved contracts is not a question set: a document that resolved two
/// contracts nobody else did must not demote the rest of the field.
#[test]
fn a_document_with_extra_contracts_does_not_demote_the_complete_pair() {
    let two = analyze_forecast_quality(&[complete("a", true), complete("b", false)], CONFIG)
        .expect("complete field");
    let wider = document("d", &resolved(0..12, true));
    let three =
        analyze_forecast_quality(&[complete("a", true), complete("b", false), wider], CONFIG)
            .expect("wider field is reported");
    assert_same_inference(pair(&two, "a", "b"), pair(&three, "a", "b"));

    let comparison = pair(&three, "a", "d");
    assert_eq!(comparison.n_contracts, 10);
    assert_eq!(
        comparison.support_gap,
        Some(SupportGap {
            agent_a_unresolved: 2,
            agent_b_unresolved: 0,
        })
    );
    assert_eq!(comparison.raw_p_value, None);
    let mut extra = vec![digest(10), digest(11)];
    extra.sort();
    assert_eq!(
        three.common_support.unresolved_by_agent["a"],
        AgentUnresolvedSupport {
            n_unresolved: 2,
            not_claimed: extra,
            ..AgentUnresolvedSupport::default()
        }
    );
    assert_eq!(
        three.common_support.unresolved_by_agent["d"],
        AgentUnresolvedSupport::default()
    );
}

/// Two agents with the same gaps resolved the same contracts, so neither chose the
/// other's comparison set; they are compared on that shared support.
#[test]
fn agents_with_identical_gaps_are_compared_on_their_shared_support() {
    let report = analyze_forecast_quality(
        &[
            complete("a", true),
            document("c", &resolved(0..8, true)),
            document("e", &resolved(0..8, false)),
        ],
        CONFIG,
    )
    .expect("field is reported");
    let comparison = pair(&report, "c", "e");
    assert_eq!(comparison.n_contracts, 8);
    assert_eq!(comparison.n_settlement_blocks, 4);
    assert_eq!(comparison.support_gap, None);
    assert!(comparison.inference_error.is_none());
    assert!(comparison.raw_p_value.is_some());
    assert_eq!(
        report.common_support.unresolved_by_agent["c"].n_unresolved,
        2
    );
}

/// A claim's most recoverable status is the one the agent is charged under, and a
/// resolved claim on the same contract means the contract is not missing at all.
#[test]
fn several_claims_on_one_contract_are_charged_once_under_the_most_recoverable_status() {
    let with = |extra: &[(u64, Status)]| {
        let mut claims = resolved(0..7, false);
        claims.extend(extra.iter().map(|(index, status)| Claim {
            index: *index,
            probability: 0.5,
            status: *status,
        }));
        claims
    };
    let agent = document(
        "m",
        &with(&[
            (6, Status::Pending),
            (7, Status::Cancelled),
            (7, Status::Pending),
            (8, Status::Rejected),
            (8, Status::Cancelled),
            (9, Status::Rejected),
            (9, Status::Rejected),
        ]),
    );
    let report =
        analyze_forecast_quality(&[complete("a", true), agent], CONFIG).expect("field is reported");
    assert_eq!(
        report.common_support.unresolved_by_agent["m"],
        AgentUnresolvedSupport {
            n_unresolved: 3,
            not_claimed: vec![],
            pending: vec![digest(7)],
            cancelled: vec![digest(8)],
            rejected: vec![digest(9)],
        }
    );
    assert_eq!(
        pair(&report, "a", "m").support_gap,
        Some(SupportGap {
            agent_a_unresolved: 0,
            agent_b_unresolved: 3,
        })
    );
}

/// Settlement agreement is checked for every pair on every contract both resolved.
/// Before v2 a dispute on a contract the partial agent lacked was outside the field
/// intersection and passed silently.
#[test]
fn a_disputed_settlement_outside_the_partial_agents_support_is_refused() {
    let flipped = || {
        let mut document = complete("b", false);
        let resolution = document
            .resolutions
            .iter_mut()
            .find(|resolution| resolution.claim_id == "claim-9")
            .expect("contract 9 is claimed");
        resolution.outcome = Some(json!(1.0 - outcome(9)));
        document
    };
    let error = analyze_forecast_quality(
        &[
            complete("a", true),
            flipped(),
            document("c", &resolved(0..9, true)),
        ],
        CONFIG,
    )
    .expect_err("a disputed settlement cannot be hidden by a partial document");
    assert!(
        error.0.contains("unequal realized outcomes"),
        "unexpected message: {}",
        error.0
    );

    // The same dispute between a partial agent and a complete one is also refused.
    let error = analyze_forecast_quality(
        &[
            complete("a", true),
            document("c", &resolved(0..10, true)),
            {
                let mut partial = document("p", &resolved(0..10, true));
                partial.resolutions[0].outcome = Some(json!(1.0 - outcome(0)));
                partial
                    .resolutions
                    .retain(|resolution| resolution.claim_id != "claim-9");
                partial
                    .revisions
                    .retain(|revision| revision.claim_id != "claim-9");
                partial
                    .contracts
                    .retain(|contract| contract.contract_id != "c9");
                partial
            },
        ],
        CONFIG,
    )
    .expect_err("a dispute with a partial agent is still a dispute");
    assert!(error.0.contains("unequal realized outcomes"));
}

// A declared contract universe (CR-MAIN SB-5).
//
// Without a plan the field support is the union of resolved digests, so one
// document that resolves contracts nobody else resolved withholds inference on all
// of its own pairs and charges those contracts to every other agent. With a plan
// the support is the declared digest set: an extra contract is reported and never
// scored, and a gap is a planned contract the agent did not resolve.

fn plan(indices: impl IntoIterator<Item = u64>) -> ForecastContractPlan {
    let digests: Vec<String> = indices.into_iter().map(digest).collect();
    parse_forecast_contract_plan(
        &json!({
            "schema_version": "sharpebench.forecast-contract-plan.v1",
            "contract_sha256": digests,
        })
        .to_string(),
    )
    .expect("fixture plan is valid")
}

fn sorted_digests(indices: impl IntoIterator<Item = u64>) -> Vec<String> {
    let mut digests: Vec<String> = indices.into_iter().map(digest).collect();
    digests.sort();
    digests
}

#[test]
fn without_a_plan_extra_contracts_still_withhold_the_padded_pairs() {
    // The documented weakness of the planless report, pinned so a change to it is
    // deliberate: `d` resolves the ten field contracts plus two of its own.
    let report = analyze_forecast_quality(
        &[complete("a", true), document("d", &resolved(0..12, true))],
        CONFIG,
    )
    .expect("field is reported");
    assert_eq!(report.schema_version, "sharpebench.forecast-quality.v2");
    let comparison = pair(&report, "a", "d");
    assert_eq!(comparison.raw_p_value, None);
    assert_eq!(
        report.common_support.unresolved_by_agent["a"].not_claimed,
        sorted_digests(10..12)
    );
    let encoded = serde_json::to_string(&report).expect("report serializes");
    assert!(!encoded.contains("outside_plan_by_agent"));
}

#[test]
fn with_a_plan_extra_contracts_are_reported_and_charge_nobody() {
    let planned = plan(0..10);
    let baseline = analyze_forecast_quality_against_plan(
        &[complete("a", true), complete("b", false)],
        CONFIG,
        &planned,
    )
    .expect("complete planned field");
    let unplanned = analyze_forecast_quality(&[complete("a", true), complete("b", false)], CONFIG)
        .expect("complete field");
    // A complete field compares the same way with or without its plan.
    assert_same_inference(pair(&unplanned, "a", "b"), pair(&baseline, "a", "b"));

    let report = analyze_forecast_quality_against_plan(
        &[
            complete("a", true),
            complete("b", false),
            document("d", &resolved(0..12, true)),
        ],
        CONFIG,
        &planned,
    )
    .expect("padded field is reported");
    assert_eq!(report.schema_version, "sharpebench.forecast-quality.v3");
    assert!(report.common_support.rule.contains("declared plan"));
    let two = pair(&baseline, "a", "b");
    let three = pair(&report, "a", "b");
    assert_eq!(two.n_contracts, three.n_contracts);
    assert_eq!(two.mean_loss_difference, three.mean_loss_difference);
    assert_eq!(two.raw_p_value, three.raw_p_value);
    assert_eq!(two.confidence_lower, three.confidence_lower);

    for other in ["a", "b"] {
        let comparison = pair(&report, other, "d");
        assert_eq!(comparison.n_contracts, 10);
        assert_eq!(comparison.n_settlement_blocks, 5);
        assert_eq!(comparison.support_gap, None);
        assert!(comparison.inference_error.is_none(), "{other}");
        assert!(comparison.raw_p_value.is_some(), "{other}");
    }

    let support = &report.common_support;
    assert_eq!(support.n_contracts, 10);
    assert_eq!(support.contract_sha256, sorted_digests(0..10));
    for agent in ["a", "b", "d"] {
        assert_eq!(
            support.unresolved_by_agent[agent],
            AgentUnresolvedSupport::default(),
            "{agent}"
        );
    }
    let outside = support
        .outside_plan_by_agent
        .as_ref()
        .expect("a planned report lists digests outside the plan");
    assert_eq!(outside["a"], Vec::<String>::new());
    assert_eq!(outside["b"], Vec::<String>::new());
    assert_eq!(outside["d"], sorted_digests(10..12));

    // The padded agent is scored on the plan only, and its document still says
    // what it resolved.
    let d = report
        .agents
        .iter()
        .find(|agent| agent.agent_id == "d")
        .expect("d is summarized");
    assert_eq!(d.metrics[0].n, 10);
    assert_eq!(d.n_resolved, 12);
    assert_eq!(report.contract_digest_versions.len(), 10);
}

#[test]
fn with_a_plan_an_abstaining_agent_is_charged_its_planned_gaps() {
    // `c` leaves out contracts 8 and 9 and pads with 10 and 11. Without a plan the
    // padding charges `a` two gaps; with one, `c` carries its own two.
    let abstainer = || {
        let mut claims = resolved(0..8, true);
        claims.extend(resolved(10..12, true));
        document("c", &claims)
    };
    let unplanned = analyze_forecast_quality(&[complete("a", true), abstainer()], CONFIG)
        .expect("field is reported");
    assert_eq!(
        pair(&unplanned, "a", "c").support_gap,
        Some(SupportGap {
            agent_a_unresolved: 2,
            agent_b_unresolved: 2,
        })
    );
    assert_eq!(
        unplanned.common_support.unresolved_by_agent["a"].n_unresolved,
        2
    );

    let report = analyze_forecast_quality_against_plan(
        &[complete("a", true), abstainer()],
        CONFIG,
        &plan(0..10),
    )
    .expect("planned field is reported");
    let comparison = pair(&report, "a", "c");
    assert_eq!(comparison.n_contracts, 8);
    assert_eq!(
        comparison.support_gap,
        Some(SupportGap {
            agent_a_unresolved: 0,
            agent_b_unresolved: 2,
        })
    );
    assert_eq!(comparison.raw_p_value, None);
    assert!(!comparison.familywise_significant);
    let support = &report.common_support;
    assert_eq!(
        support.unresolved_by_agent["a"],
        AgentUnresolvedSupport::default()
    );
    assert_eq!(
        support.unresolved_by_agent["c"],
        AgentUnresolvedSupport {
            n_unresolved: 2,
            not_claimed: sorted_digests(8..10),
            ..AgentUnresolvedSupport::default()
        }
    );
    assert_eq!(
        support.outside_plan_by_agent.as_ref().expect("planned")["c"],
        sorted_digests(10..12)
    );
}

#[test]
fn a_planned_contract_nobody_resolved_is_charged_to_every_agent() {
    // Contract 10 is planned and nobody claimed it. Both agents resolved the same
    // planned digests, so their pair keeps its inference, and each carries the gap.
    let report = analyze_forecast_quality_against_plan(
        &[complete("a", true), complete("b", false)],
        CONFIG,
        &plan(0..11),
    )
    .expect("planned field is reported");
    let comparison = pair(&report, "a", "b");
    assert_eq!(comparison.n_contracts, 10);
    assert!(comparison.raw_p_value.is_some());
    assert_eq!(report.common_support.n_contracts, 11);
    for agent in ["a", "b"] {
        assert_eq!(
            report.common_support.unresolved_by_agent[agent],
            AgentUnresolvedSupport {
                n_unresolved: 1,
                not_claimed: vec![digest(10)],
                ..AgentUnresolvedSupport::default()
            },
            "{agent}"
        );
    }
}

#[test]
fn with_a_plan_a_settlement_dispute_outside_it_is_still_refused() {
    let with_extra = |agent: &str, flipped: bool| {
        let mut claims = resolved(0..10, true);
        claims.extend(resolved([10], true));
        let mut document = document(agent, &claims);
        if flipped {
            let last = document.resolutions.len() - 1;
            document.resolutions[last].outcome = Some(json!(1.0 - outcome(10)));
        }
        document
    };
    let error = analyze_forecast_quality_against_plan(
        &[with_extra("a", false), with_extra("b", true)],
        CONFIG,
        &plan(0..10),
    )
    .expect_err("an unscored contract still has one settlement");
    assert!(error.0.contains("unequal realized outcomes"), "{}", error.0);
}

#[test]
fn a_malformed_plan_is_refused() {
    let parse = |value: Value| parse_forecast_contract_plan(&value.to_string());
    let schema = "sharpebench.forecast-contract-plan.v1";
    let good = digest(0);
    assert!(parse(json!({"schema_version": schema, "contract_sha256": [good]})).is_ok());
    for (label, value) in [
        (
            "schema",
            json!({"schema_version": "sharpebench.forecast-contract-plan.v2", "contract_sha256": [good]}),
        ),
        (
            "empty",
            json!({"schema_version": schema, "contract_sha256": []}),
        ),
        (
            "uppercase",
            json!({"schema_version": schema, "contract_sha256": [good.to_uppercase()]}),
        ),
        (
            "short",
            json!({"schema_version": schema, "contract_sha256": [&good[1..]]}),
        ),
        (
            "duplicate",
            json!({"schema_version": schema, "contract_sha256": [good, good]}),
        ),
        (
            "unknown field",
            json!({"schema_version": schema, "contract_sha256": [good], "extra": 1}),
        ),
    ] {
        assert!(parse(value).is_err(), "{label}");
    }
}
