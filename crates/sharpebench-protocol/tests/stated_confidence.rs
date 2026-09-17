//! An order's confidence is scored only when the agent states it.
//!
//! The wire contract used to fill an omitted `confidence` with 0.5, so a
//! decision that stated nothing looked, to the simulator and to every captured
//! trajectory, exactly like one that stated 0.5. These tests pin the absent
//! representation: an omitted key stays absent through parsing, serialization
//! and a trajectory round trip, a stated value round-trips byte for byte, and a
//! present key must carry a number. They read and write JSON only, so they
//! state the wire behaviour independently of the Rust field type.

use sharpebench_protocol::{decision_from_wire, AgentTrajectory, TrajectoryContract};

#[test]
fn a_decision_that_omits_confidence_parses_and_stays_unstated() {
    let wire = r#"{"orders":[{"symbol":"A","action":"buy","target_weight":0.5}]}"#;
    let decision = decision_from_wire(wire).expect("confidence is optional");
    let reserialized = serde_json::to_string(&decision).unwrap();
    assert_eq!(
        reserialized,
        r#"{"orders":[{"symbol":"A","action":"buy","target_weight":0.5,"rationale":""}],"reasoning":""}"#,
        "an unstated confidence must not be written back as a value"
    );
}

#[test]
fn a_stated_confidence_round_trips_byte_identically() {
    for stated in ["0.0", "0.5", "0.7", "1.0", "0.30000000000000004"] {
        let wire = format!(
            r#"{{"orders":[{{"symbol":"A","action":"sell","target_weight":-0.25,"confidence":{stated},"rationale":"r"}}],"reasoning":"x"}}"#
        );
        let decision = decision_from_wire(&wire).expect("a stated confidence parses");
        let value = serde_json::to_value(&decision).unwrap();
        assert_eq!(
            value["orders"][0]["confidence"].as_f64(),
            Some(stated.parse::<f64>().unwrap()),
            "{stated}"
        );
        assert_eq!(serde_json::to_string(&decision).unwrap(), wire);
    }
}

#[test]
fn a_present_confidence_key_must_carry_a_number() {
    for invalid in ["null", r#""0.7""#, "true", "[0.7]"] {
        let wire = format!(
            r#"{{"orders":[{{"symbol":"A","action":"buy","target_weight":0.5,"confidence":{invalid}}}]}}"#
        );
        assert!(
            decision_from_wire(&wire).is_err(),
            "confidence {invalid} is not a stated number and must be refused"
        );
    }
}

#[test]
fn a_captured_trajectory_keeps_stated_and_unstated_confidences_apart() {
    let bytes = r#"{"agent_id":"a","in_sample_trials":0,"runs":[{"window_start":0,"window_end":1,"seed":0,"steps":[{"step":0,"observation_id":"2026-01-01","decision":{"orders":[{"symbol":"A","action":"buy","target_weight":0.25,"rationale":""},{"symbol":"B","action":"buy","target_weight":0.25,"confidence":0.5,"rationale":""}],"reasoning":""}}]}]}"#;
    let trajectory: AgentTrajectory = serde_json::from_str(bytes).unwrap();
    assert_eq!(
        serde_json::to_string(&trajectory).unwrap(),
        bytes,
        "the unstated order must stay distinguishable from the one that stated 0.5"
    );
}

#[test]
fn the_trajectory_contract_schema_marks_the_optional_confidence() {
    assert_eq!(
        TrajectoryContract::SCHEMA_VERSION,
        3,
        "schema-2 captures wrote a filled-in 0.5 for unstated confidences"
    );
}
