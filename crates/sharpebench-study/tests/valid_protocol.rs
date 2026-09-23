//! The accepting side of the contract: a coherent protocol validates, and the
//! shipped placeholder document is one.

mod common;

use sharpebench_study::protocol::RunTier;
use sharpebench_study::{validate, StudyProtocol};

#[test]
fn coherent_protocol_validates() {
    let protocol = common::valid_protocol();
    assert_eq!(validate(&protocol), Ok(()));
}

#[test]
fn shipped_placeholder_example_validates() {
    let text = std::fs::read_to_string("examples/placeholder-protocol.json")
        .expect("the example ships with the crate");
    let protocol = StudyProtocol::from_json(&text).expect("the example parses");
    assert_eq!(validate(&protocol), Ok(()));
}

/// The example is a stand-in awaiting decisions D3 and D8, and says so where a
/// reader will see it. A future edit that drops the marking, or that seals the
/// placeholder as frozen validation evidence, fails here.
#[test]
fn placeholder_example_is_marked_and_unsealed() {
    let text = std::fs::read_to_string("examples/placeholder-protocol.json")
        .expect("the example ships with the crate");
    let protocol = StudyProtocol::from_json(&text).expect("the example parses");
    assert!(
        protocol.notes.contains("PLACEHOLDER"),
        "the example's notes must mark it as a placeholder: {}",
        protocol.notes
    );
    assert!(protocol.protocol_id.contains("PLACEHOLDER"));
    assert_ne!(protocol.tier, RunTier::FrozenValidation);
    assert!(!protocol.frozen);
}

#[test]
fn protocol_round_trips_through_json() {
    let protocol = common::valid_protocol();
    let text = serde_json::to_string(&protocol).expect("serializes");
    assert_eq!(StudyProtocol::from_json(&text), Ok(protocol));
}

/// The contract is closed: a key it does not define is refused at load rather
/// than read past, and the refusal names the offending key.
#[test]
fn unknown_key_is_refused_at_load_naming_the_key() {
    let protocol = common::valid_protocol();
    let mut value = serde_json::to_value(&protocol).expect("serializes");
    value
        .as_object_mut()
        .expect("object")
        .insert("achieved_half_width".to_string(), serde_json::json!(0.001));
    let text = serde_json::to_string(&value).expect("serializes");

    let refusal = StudyProtocol::from_json(&text).expect_err("an unknown key is refused");
    assert!(
        refusal.to_string().contains("achieved_half_width"),
        "the refusal should name the offending key: {refusal}"
    );
}
