//! Bidirectional drift guard between the published JSON Schemas under
//! `schema/` and the Rust types they claim to describe.
//!
//! The schemas set `additionalProperties: false` to mirror the types'
//! `#[serde(deny_unknown_fields)]`. That makes any disagreement an
//! interoperability break rather than a documentation gap, in both directions:
//!
//! - a field on the Rust type but absent from the schema means a conforming
//!   non-Rust implementer validating a SharpeBench-emitted message rejects it;
//! - a property in the schema but absent from the Rust type means an entrant
//!   that follows the published contract is rejected at the transport boundary
//!   and scored as an agent protocol fault.
//!
//! Both are therefore asserted separately, with the offending names printed, for
//! every one of the six wire types. Optional fields are populated so the
//! comparison sees the full surface: `Decision::cost` is
//! `skip_serializing_if = "Option::is_none"`, so a `None` would hide it.

use std::collections::BTreeSet;
use std::fs;

use sharpebench_protocol::{
    Action, Decision, DecisionCost, MarketObservation, Order, PositionState, SymbolSnapshot,
};

/// Every key a fully-populated value serializes to, at the top level.
fn serialized_keys<T: serde::Serialize>(value: &T) -> BTreeSet<String> {
    match serde_json::to_value(value).expect("value serializes") {
        serde_json::Value::Object(map) => map.keys().cloned().collect(),
        other => panic!("expected a JSON object, got {other}"),
    }
}

/// The property names a schema object declares at `pointer`.
fn schema_properties(schema: &serde_json::Value, pointer: &str) -> BTreeSet<String> {
    schema
        .pointer(pointer)
        .unwrap_or_else(|| panic!("schema has no {pointer}"))
        .as_object()
        .unwrap_or_else(|| panic!("{pointer} is not an object"))
        .keys()
        .cloned()
        .collect()
}

fn read_schema(name: &str) -> serde_json::Value {
    let path = format!("schema/{name}");
    serde_json::from_str(
        &fs::read_to_string(&path).unwrap_or_else(|error| panic!("read {path}: {error}")),
    )
    .unwrap_or_else(|error| panic!("{path} is not valid JSON: {error}"))
}

/// Assert the two descriptions of one type agree, naming what each side is
/// missing rather than dumping two opaque sets.
fn assert_no_drift(type_name: &str, rust: &BTreeSet<String>, schema: &BTreeSet<String>) {
    let missing_from_schema = rust.difference(schema).cloned().collect::<Vec<_>>();
    assert!(
        missing_from_schema.is_empty(),
        "{type_name}: fields on the Rust type are absent from the published schema, so a \
         conforming validator would reject a SharpeBench-emitted message: {missing_from_schema:?}",
    );
    let missing_from_rust = schema.difference(rust).cloned().collect::<Vec<_>>();
    assert!(
        missing_from_rust.is_empty(),
        "{type_name}: properties in the published schema are absent from the Rust type, so an \
         entrant following the contract would be rejected by deny_unknown_fields: \
         {missing_from_rust:?}",
    );
}

/// Every object in a closed contract must say so, or `deny_unknown_fields` and
/// the schema disagree about what an unknown key means.
fn assert_closed(schema: &serde_json::Value, pointer: &str) {
    let node = schema
        .pointer(pointer)
        .unwrap_or_else(|| panic!("schema has no {pointer}"));
    assert_eq!(
        node.get("additionalProperties"),
        Some(&serde_json::Value::Bool(false)),
        "{pointer} must set additionalProperties: false to mirror deny_unknown_fields",
    );
}

fn populated_order() -> Order {
    Order {
        symbol: "SPX".to_string(),
        action: Action::Sell,
        target_weight: -0.3,
        confidence: 0.7,
        rationale: "downtrend".to_string(),
    }
}

fn populated_cost() -> DecisionCost {
    DecisionCost {
        cost_usd: 0.014,
        tokens_in: 2048,
        tokens_out: 256,
        reasoning_tokens: 64,
    }
}

#[test]
fn published_decision_schema_matches_the_protocol_types() {
    let schema = read_schema("decision.schema.json");
    assert_closed(&schema, "");
    assert_closed(&schema, "/$defs/Order");
    assert_closed(&schema, "/$defs/DecisionCost");

    let order = populated_order();
    let decision = Decision {
        orders: vec![order.clone()],
        reasoning: "tilt short".to_string(),
        cost: Some(populated_cost()),
    };

    assert_no_drift(
        "Decision",
        &serialized_keys(&decision),
        &schema_properties(&schema, "/properties"),
    );
    assert_no_drift(
        "Order",
        &serialized_keys(&order),
        &schema_properties(&schema, "/$defs/Order/properties"),
    );
    assert_no_drift(
        "DecisionCost",
        &serialized_keys(&decision.cost.expect("cost was populated above")),
        &schema_properties(&schema, "/$defs/DecisionCost/properties"),
    );
}

#[test]
fn published_observation_schema_matches_the_protocol_types() {
    let schema = read_schema("observation.schema.json");
    assert_closed(&schema, "");
    assert_closed(&schema, "/$defs/SymbolSnapshot");
    assert_closed(&schema, "/$defs/PositionState");

    let snapshot = SymbolSnapshot {
        symbol: "SPX".to_string(),
        close_history: vec![240.0, 235.5],
        fundamentals: [("pe".to_string(), 61.4)].into_iter().collect(),
        news: vec!["deliveries beat".to_string()],
    };
    let position = PositionState {
        symbol: "SPX".to_string(),
        shares: 0.5,
        avg_price: 250.0,
    };
    let observation = MarketObservation {
        date: "2026-05-05".to_string(),
        cash: 1.0,
        symbols: vec![snapshot.clone()],
        portfolio: vec![position.clone()],
    };

    assert_no_drift(
        "MarketObservation",
        &serialized_keys(&observation),
        &schema_properties(&schema, "/properties"),
    );
    assert_no_drift(
        "SymbolSnapshot",
        &serialized_keys(&snapshot),
        &schema_properties(&schema, "/$defs/SymbolSnapshot/properties"),
    );
    assert_no_drift(
        "PositionState",
        &serialized_keys(&position),
        &schema_properties(&schema, "/$defs/PositionState/properties"),
    );
}

/// The schemas are only authoritative if what they declare `required` actually
/// deserializes, and what they omit actually round-trips. A message carrying
/// exactly the schema's required keys must parse; adding a key the schema
/// forbids must not.
#[test]
fn schema_required_keys_deserialize_and_forbidden_keys_do_not() {
    let minimal_decision = r#"{"orders":[{"symbol":"A","action":"buy","target_weight":0.5}]}"#;
    let parsed: Decision = serde_json::from_str(minimal_decision).expect("required keys suffice");
    assert!((parsed.orders[0].confidence - 0.5).abs() < 1e-12);

    let extra = r#"{"orders":[],"latency_ms":12}"#;
    assert!(
        serde_json::from_str::<Decision>(extra).is_err(),
        "additionalProperties: false must be enforced by the type, not only documented",
    );

    let minimal_observation = r#"{"date":"2026-01-01","cash":1.0,"symbols":[{"symbol":"A","close_history":[1.0]}],"portfolio":[]}"#;
    let observation: MarketObservation =
        serde_json::from_str(minimal_observation).expect("required keys suffice");
    assert!(observation.symbols[0].news.is_empty());
}
