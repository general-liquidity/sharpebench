//! Treatment activation and placebo control.
//!
//! Every refusal test changes exactly one input relative to the passing control
//! fixture and asserts the specific refusal cause, because several causes refuse
//! a comparison and an assertion on refusal alone would not say which one fired.

use sharpebench_memory::activation::{
    activation_status, placebo_controlled_report, sha256_hex, ActivationError, ActivationEvidence,
    ActivationReceipt, ActivationStatus, DecisionBoundary, DecisionBudget, LiftEvidence,
    OfferedMemory, PlaceboMatch, TreatmentArm, TreatmentIdentity, TreatmentKind,
};

const DECIDED_AT: i64 = 1_000;
const RETRIEVED: &[u8] = b"ETH basis inverted at 14:00; last three inversions mean-reverted";
const PLACEBO: &[u8] = b"the quick brown fox jumps over the lazy dog, then sits and waits";
const RETRIEVAL_SCORES: [f64; 6] = [0.71, 0.78, 0.74, 0.80, 0.69, 0.77];
const PLACEBO_SCORES: [f64; 6] = [0.41, 0.44, 0.39, 0.46, 0.40, 0.43];

fn budget() -> DecisionBudget {
    DecisionBudget {
        max_tokens: 8192,
        max_latency: 3000,
    }
}

fn tasks() -> Vec<String> {
    (1..=6).map(|i| format!("task-{i}")).collect()
}

fn identity(model: &str, tasks: Vec<String>, budget: DecisionBudget) -> TreatmentIdentity {
    TreatmentIdentity::new(model, tasks, budget).unwrap()
}

fn offered(content: &[u8], available_at: i64) -> OfferedMemory {
    OfferedMemory {
        content: content.to_vec(),
        available_at,
    }
}

/// A decision input framed by the host, with the memory as its own segment when
/// `memory` is given.
fn boundary(memory: Option<&[u8]>) -> DecisionBoundary {
    let mut segments = vec![
        b"system: you are a trading agent".to_vec(),
        b"observation: ETH 3120.5".to_vec(),
    ];
    if let Some(bytes) = memory {
        segments.push(bytes.to_vec());
    }
    DecisionBoundary {
        decided_at: DECIDED_AT,
        segments,
    }
}

fn exposed_receipt(decision: &str, content: &[u8]) -> ActivationReceipt {
    ActivationReceipt::observe(decision, &offered(content, 900), &boundary(Some(content))).unwrap()
}

fn unexposed_receipt(decision: &str, content: &[u8]) -> ActivationReceipt {
    ActivationReceipt::observe(decision, &offered(content, 900), &boundary(None)).unwrap()
}

fn retrieval_arm(identity: TreatmentIdentity, evidence: ActivationEvidence) -> TreatmentArm {
    TreatmentArm::new(
        TreatmentKind::Retrieval,
        identity,
        RETRIEVAL_SCORES.to_vec(),
        evidence,
    )
}

fn placebo_arm(identity: TreatmentIdentity, evidence: ActivationEvidence) -> TreatmentArm {
    TreatmentArm::new(
        TreatmentKind::Placebo,
        identity,
        PLACEBO_SCORES.to_vec(),
        evidence,
    )
}

fn matched_pair() -> (TreatmentArm, TreatmentArm) {
    let id = identity("model-a", tasks(), budget());
    (
        retrieval_arm(
            id.clone(),
            ActivationEvidence::Receipts(vec![
                exposed_receipt("d1", RETRIEVED),
                exposed_receipt("d2", RETRIEVED),
            ]),
        ),
        placebo_arm(
            id,
            ActivationEvidence::Receipts(vec![
                exposed_receipt("d1", PLACEBO),
                exposed_receipt("d2", PLACEBO),
            ]),
        ),
    )
}

#[test]
fn fixtures_are_length_matched() {
    assert_eq!(RETRIEVED.len(), PLACEBO.len());
}

#[test]
fn passing_control_is_activated_placebo_controlled_and_reports_a_lift() {
    let (retrieval, placebo) = matched_pair();
    let report = placebo_controlled_report(&retrieval, &placebo, 0.05).unwrap();

    assert!(report.activation_established);
    assert_eq!(
        report.retrieval_activation,
        ActivationStatus::Activated {
            offered_decisions: 2,
            exposed_decisions: 2
        }
    );
    assert_eq!(report.placebo_match, PlaceboMatch::Matched { decisions: 2 });
    assert_eq!(report.lift_evidence, LiftEvidence::PlaceboControlled);
    assert_eq!(report.lift_evidence.label(), "placebo-controlled");

    let expected_lift =
        RETRIEVAL_SCORES.iter().sum::<f64>() / 6.0 - PLACEBO_SCORES.iter().sum::<f64>() / 6.0;
    assert!((report.placebo_controlled_lift - expected_lift).abs() < 1e-12);
    assert!(report.placebo_controlled_lift > 0.0);
    assert!(report.significant, "pvalue {}", report.lift_pvalue);

    // Claim 3 is never claimed, even by the passing control.
    assert!(report
        .not_established
        .iter()
        .any(|line| line.starts_with("causal trading improvement")));
    let printed = report.to_string();
    assert!(printed.contains("causal trading improvement: not established"));
    assert!(!printed.contains("is a proxy"));
}

#[test]
fn memory_written_but_never_exposed_is_not_activated() {
    // The worker offered memory at two decisions; the host-framed decision input
    // never carried it.
    let evidence = ActivationEvidence::Receipts(vec![
        unexposed_receipt("d1", RETRIEVED),
        unexposed_receipt("d2", RETRIEVED),
    ]);
    assert_eq!(
        activation_status(TreatmentKind::Retrieval, &evidence).unwrap(),
        ActivationStatus::NotActivated {
            offered_decisions: 2
        }
    );

    let id = identity("model-a", tasks(), budget());
    let retrieval = retrieval_arm(id.clone(), evidence);
    let placebo = placebo_arm(
        id,
        ActivationEvidence::Receipts(vec![
            unexposed_receipt("d1", PLACEBO),
            unexposed_receipt("d2", PLACEBO),
        ]),
    );
    let report = placebo_controlled_report(&retrieval, &placebo, 0.05).unwrap();
    assert!(!report.activation_established);
    assert_eq!(
        report.lift_evidence,
        LiftEvidence::ProxyRetrievalNotActivated
    );
    assert_eq!(report.lift_evidence.label(), "proxy");
    assert!(matches!(
        report.placebo_match,
        PlaceboMatch::NotAssessed { .. }
    ));
}

#[test]
fn memory_embedded_inside_a_larger_segment_is_not_exposure() {
    // Exposure is whole-segment digest equality over the framed input, not a
    // substring search: bytes buried in another segment are not receipted.
    let mut buried = b"notes: ".to_vec();
    buried.extend_from_slice(RETRIEVED);
    let receipt = ActivationReceipt::observe(
        "d1",
        &offered(RETRIEVED, 900),
        &DecisionBoundary {
            decided_at: DECIDED_AT,
            segments: vec![buried],
        },
    )
    .unwrap();
    assert!(!receipt.exposed());
}

#[test]
fn content_available_after_the_decision_is_a_point_in_time_refusal() {
    let err = ActivationReceipt::observe(
        "d1",
        &offered(RETRIEVED, DECIDED_AT + 1),
        &boundary(Some(RETRIEVED)),
    )
    .unwrap_err();
    assert_eq!(
        err,
        ActivationError::PointInTimeViolation {
            decision_id: "d1".to_string(),
            available_at: DECIDED_AT + 1,
            decided_at: DECIDED_AT,
        }
    );
}

#[test]
fn content_available_exactly_at_the_decision_is_allowed() {
    let receipt = ActivationReceipt::observe(
        "d1",
        &offered(RETRIEVED, DECIDED_AT),
        &boundary(Some(RETRIEVED)),
    )
    .unwrap();
    assert!(receipt.exposed());
}

#[test]
fn model_mismatch_is_refused_as_a_model_mismatch() {
    let (retrieval, mut placebo) = matched_pair();
    placebo.identity = identity("model-b", tasks(), budget());
    assert_eq!(
        placebo_controlled_report(&retrieval, &placebo, 0.05).unwrap_err(),
        ActivationError::ModelMismatch {
            retrieval: "model-a".to_string(),
            placebo: "model-b".to_string(),
        }
    );
}

#[test]
fn task_mismatch_is_refused_as_a_task_mismatch() {
    let (retrieval, mut placebo) = matched_pair();
    let mut other_tasks = tasks();
    other_tasks[3] = "task-other".to_string();
    placebo.identity = identity("model-a", other_tasks, budget());
    assert_eq!(
        placebo_controlled_report(&retrieval, &placebo, 0.05).unwrap_err(),
        ActivationError::TaskMismatch {
            first_difference: 3
        }
    );
}

#[test]
fn task_order_mismatch_is_refused_as_a_task_mismatch() {
    let (retrieval, mut placebo) = matched_pair();
    let mut reordered = tasks();
    reordered.swap(0, 1);
    placebo.identity = identity("model-a", reordered, budget());
    assert_eq!(
        placebo_controlled_report(&retrieval, &placebo, 0.05).unwrap_err(),
        ActivationError::TaskMismatch {
            first_difference: 0
        }
    );
}

#[test]
fn budget_mismatch_is_refused_as_a_budget_mismatch() {
    let (retrieval, mut placebo) = matched_pair();
    let bigger = DecisionBudget {
        max_tokens: 16384,
        ..budget()
    };
    placebo.identity = identity("model-a", tasks(), bigger);
    assert_eq!(
        placebo_controlled_report(&retrieval, &placebo, 0.05).unwrap_err(),
        ActivationError::BudgetMismatch {
            retrieval: budget(),
            placebo: bigger,
        }
    );
}

#[test]
fn placebo_of_different_byte_length_is_refused_as_a_length_mismatch() {
    let (retrieval, mut placebo) = matched_pair();
    placebo.evidence = ActivationEvidence::Receipts(vec![
        exposed_receipt("d1", b"short placebo"),
        exposed_receipt("d2", PLACEBO),
    ]);
    assert_eq!(
        placebo_controlled_report(&retrieval, &placebo, 0.05).unwrap_err(),
        ActivationError::PlaceboLengthMismatch {
            decision_id: "d1".to_string(),
            retrieval_len: RETRIEVED.len() as u64,
            placebo_len: 13,
        }
    );
}

#[test]
fn placebo_missing_at_a_retrieval_boundary_is_refused_as_not_exposed() {
    let (retrieval, mut placebo) = matched_pair();
    placebo.evidence = ActivationEvidence::Receipts(vec![
        exposed_receipt("d1", PLACEBO),
        unexposed_receipt("d2", PLACEBO),
    ]);
    assert_eq!(
        placebo_controlled_report(&retrieval, &placebo, 0.05).unwrap_err(),
        ActivationError::PlaceboNotExposedAtBoundary {
            decision_id: "d2".to_string(),
        }
    );
}

#[test]
fn placebo_that_is_the_retrieved_content_is_refused() {
    let (retrieval, mut placebo) = matched_pair();
    placebo.evidence = ActivationEvidence::Receipts(vec![
        exposed_receipt("d1", RETRIEVED),
        exposed_receipt("d2", PLACEBO),
    ]);
    assert_eq!(
        placebo_controlled_report(&retrieval, &placebo, 0.05).unwrap_err(),
        ActivationError::PlaceboContentIdentical {
            decision_id: "d1".to_string(),
        }
    );
}

#[test]
fn opaque_agent_is_unavailable_and_its_lift_is_a_proxy() {
    let id = identity("model-a", tasks(), budget());
    let reason = "HTTP agent exposes no decision-boundary input".to_string();
    let retrieval = retrieval_arm(
        id.clone(),
        ActivationEvidence::Unavailable {
            reason: reason.clone(),
        },
    );
    let placebo = placebo_arm(
        id,
        ActivationEvidence::Unavailable {
            reason: reason.clone(),
        },
    );
    let report = placebo_controlled_report(&retrieval, &placebo, 0.05).unwrap();

    assert_eq!(
        report.retrieval_activation,
        ActivationStatus::Unavailable { reason }
    );
    assert!(!report.activation_established);
    assert_eq!(
        report.lift_evidence,
        LiftEvidence::ProxyActivationUnavailable
    );
    assert_eq!(report.lift_evidence.label(), "proxy");
    // The lift is still computed: unavailable evidence is diagnostic, not a refusal.
    assert!(report.placebo_controlled_lift > 0.0);
    assert!(report
        .not_established
        .iter()
        .any(|line| line.contains("lift without activation evidence is a proxy")));
    assert!(report
        .to_string()
        .contains("lift without activation evidence and a matched placebo is a proxy"));
}

#[test]
fn duplicate_receipts_for_one_decision_are_refused() {
    let evidence = ActivationEvidence::Receipts(vec![
        exposed_receipt("d1", RETRIEVED),
        unexposed_receipt("d1", RETRIEVED),
    ]);
    assert_eq!(
        activation_status(TreatmentKind::Retrieval, &evidence).unwrap_err(),
        ActivationError::DuplicateDecision {
            kind: TreatmentKind::Retrieval,
            decision_id: "d1".to_string(),
        }
    );
}

#[test]
fn receipt_and_evidence_round_trip_through_serde() {
    let receipt = exposed_receipt("d1", RETRIEVED);
    let json = serde_json::to_string(&receipt).unwrap();
    let back: ActivationReceipt = serde_json::from_str(&json).unwrap();
    assert_eq!(back, receipt);
    assert!(back.exposed());
    assert_eq!(back.content_sha256(), sha256_hex(RETRIEVED));

    let evidence = ActivationEvidence::Receipts(vec![receipt, unexposed_receipt("d2", RETRIEVED)]);
    let back: ActivationEvidence =
        serde_json::from_str(&serde_json::to_string(&evidence).unwrap()).unwrap();
    assert_eq!(back, evidence);

    let unavailable = ActivationEvidence::Unavailable {
        reason: "opaque".to_string(),
    };
    let back: ActivationEvidence =
        serde_json::from_str(&serde_json::to_string(&unavailable).unwrap()).unwrap();
    assert_eq!(back, unavailable);

    let id = identity("model-a", tasks(), budget());
    let back: TreatmentIdentity =
        serde_json::from_str(&serde_json::to_string(&id).unwrap()).unwrap();
    assert_eq!(back, id);
}

#[test]
fn a_deserialized_receipt_cannot_bypass_point_in_time_validation() {
    let mut value = serde_json::to_value(exposed_receipt("d1", RETRIEVED)).unwrap();
    value["available_at"] = serde_json::json!(DECIDED_AT + 5);
    let err = serde_json::from_value::<ActivationReceipt>(value).unwrap_err();
    assert!(err.to_string().contains("point-in-time violation"), "{err}");
}

#[test]
fn a_deserialized_receipt_cannot_forge_exposure_with_a_flag() {
    // Exposure is derived from digests; there is no field to set it.
    let mut value = serde_json::to_value(unexposed_receipt("d1", RETRIEVED)).unwrap();
    value["exposed"] = serde_json::json!(true);
    assert!(serde_json::from_value::<ActivationReceipt>(value).is_err());
}

#[test]
fn a_deserialized_receipt_with_a_malformed_digest_is_refused() {
    let mut value = serde_json::to_value(exposed_receipt("d1", RETRIEVED)).unwrap();
    value["content_sha256"] = serde_json::json!("not-a-digest");
    let err = serde_json::from_value::<ActivationReceipt>(value).unwrap_err();
    assert!(err.to_string().contains("content_sha256"), "{err}");
}

#[test]
fn a_deserialized_identity_cannot_bypass_task_validation() {
    let mut value = serde_json::to_value(identity("model-a", tasks(), budget())).unwrap();
    value["task_ids"] = serde_json::json!(["task-1", "task-1"]);
    let err = serde_json::from_value::<TreatmentIdentity>(value).unwrap_err();
    assert!(err.to_string().contains("declared twice"), "{err}");
}
