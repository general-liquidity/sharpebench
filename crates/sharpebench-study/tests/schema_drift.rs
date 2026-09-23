//! Bidirectional drift guard between `schema/study-protocol.schema.json` and
//! the Rust types it claims to describe.
//!
//! Both sides are closed: the schema sets `additionalProperties: false` and
//! the types carry `#[serde(deny_unknown_fields)]`. A disagreement is then an
//! interoperability break in one direction or the other:
//!
//! - a field on the Rust type but absent from the schema means a non-Rust
//!   implementer validating a SharpeBench-emitted protocol rejects it;
//! - a property in the schema but absent from the type means an author who
//!   follows the published contract has their document refused at load.
//!
//! Both are asserted separately, with the offending names printed. The
//! comparison covers the property names of every object type and the tag
//! values of every enum. It does not compare types, ranges or descriptions:
//! those are checked by the validator's own tests, not here.

mod common;

use std::collections::BTreeSet;
use std::fs;

use serde_json::Value;
use sharpebench_study::protocol::*;

fn schema() -> Value {
    let path = "schema/study-protocol.schema.json";
    serde_json::from_str(
        &fs::read_to_string(path).unwrap_or_else(|error| panic!("read {path}: {error}")),
    )
    .unwrap_or_else(|error| panic!("{path} is not valid JSON: {error}"))
}

fn schema_properties(schema: &Value, pointer: &str) -> BTreeSet<String> {
    schema
        .pointer(pointer)
        .unwrap_or_else(|| panic!("schema has no {pointer}"))
        .as_object()
        .unwrap_or_else(|| panic!("{pointer} is not an object"))
        .keys()
        .cloned()
        .collect()
}

/// The `kind` values a tagged-enum definition offers across its `oneOf`.
fn schema_tags(schema: &Value, pointer: &str) -> BTreeSet<String> {
    schema
        .pointer(pointer)
        .unwrap_or_else(|| panic!("schema has no {pointer}"))
        .as_array()
        .unwrap_or_else(|| panic!("{pointer} is not an array"))
        .iter()
        .map(|variant| {
            variant
                .pointer("/properties/kind/const")
                .and_then(Value::as_str)
                .unwrap_or_else(|| panic!("a variant under {pointer} has no kind const"))
                .to_string()
        })
        .collect()
}

/// The values a plain string enum definition offers.
fn schema_enum(schema: &Value, pointer: &str) -> BTreeSet<String> {
    schema
        .pointer(pointer)
        .unwrap_or_else(|| panic!("schema has no {pointer}"))
        .as_array()
        .unwrap_or_else(|| panic!("{pointer} is not an array"))
        .iter()
        .map(|value| {
            value
                .as_str()
                .unwrap_or_else(|| panic!("a value under {pointer} is not a string"))
                .to_string()
        })
        .collect()
}

fn serialized_keys(value: &Value) -> BTreeSet<String> {
    match value {
        Value::Object(map) => map.keys().cloned().collect(),
        other => panic!("expected a JSON object, got {other}"),
    }
}

fn assert_no_drift(type_name: &str, rust: &BTreeSet<String>, schema: &BTreeSet<String>) {
    let missing_from_schema: Vec<_> = rust.difference(schema).cloned().collect();
    assert!(
        missing_from_schema.is_empty(),
        "{type_name}: present on the Rust type but absent from the published schema, so a \
         conforming implementer would reject a SharpeBench-emitted protocol: \
         {missing_from_schema:?}"
    );
    let missing_from_rust: Vec<_> = schema.difference(rust).cloned().collect();
    assert!(
        missing_from_rust.is_empty(),
        "{type_name}: published in the schema but absent from the Rust type, so an author \
         following the contract would have the document refused at load: {missing_from_rust:?}"
    );
}

#[test]
fn object_property_names_agree() {
    let schema = schema();
    let protocol = serde_json::to_value(common::valid_protocol()).expect("serializes");

    let cases: &[(&str, &Value, &str)] = &[
        ("StudyProtocol", &protocol, "/properties"),
        (
            "MethodIdentity",
            &protocol["identity"],
            "/$defs/MethodIdentity/properties",
        ),
        ("Claim", &protocol["claims"][0], "/$defs/Claim/properties"),
        (
            "Estimand",
            &protocol["estimands"][0],
            "/$defs/Estimand/properties",
        ),
        ("Design", &protocol["design"], "/$defs/Design/properties"),
        (
            "FieldComposition",
            &protocol["design"]["field_composition"],
            "/$defs/FieldComposition/properties",
        ),
        (
            "DependenceStructure",
            &protocol["design"]["dependence"],
            "/$defs/DependenceStructure/properties",
        ),
        (
            "WindowGeometry",
            &protocol["design"]["window_geometry"],
            "/$defs/WindowGeometry/properties",
        ),
        (
            "SearchAssumptions",
            &protocol["design"]["search_assumptions"],
            "/$defs/SearchAssumptions/properties",
        ),
        (
            "Replication",
            &protocol["replication"],
            "/$defs/Replication/properties",
        ),
        (
            "Inference",
            &protocol["inference"],
            "/$defs/Inference/properties",
        ),
        (
            "Multiplicity",
            &protocol["inference"]["multiplicity"],
            "/$defs/Multiplicity/properties",
        ),
        (
            "Accounting",
            &protocol["accounting"],
            "/$defs/Accounting/properties",
        ),
        (
            "SimulationPlan",
            &protocol["simulation"],
            "/$defs/SimulationPlan/properties",
        ),
        ("Budget", &protocol["budget"], "/$defs/Budget/properties"),
        (
            "ProtocolVersion",
            &protocol["version"],
            "/$defs/ProtocolVersion/properties",
        ),
    ];

    for (type_name, value, pointer) in cases {
        assert_no_drift(
            type_name,
            &serialized_keys(value),
            &schema_properties(&schema, pointer),
        );
    }
}

#[test]
fn tagged_enum_kind_values_agree() {
    let schema = schema();

    let cases: &[(&str, &[&str], &str)] = &[
        (
            "EstimandKind",
            EstimandKind::ALL_TAGS,
            "/$defs/EstimandKind/oneOf",
        ),
        (
            "EstimandTarget",
            EstimandTarget::ALL_TAGS,
            "/$defs/EstimandTarget/oneOf",
        ),
        (
            "DecisionRule",
            DecisionRule::ALL_TAGS,
            "/$defs/DecisionRule/oneOf",
        ),
        (
            "DispersionPolicy",
            DispersionPolicy::ALL_TAGS,
            "/$defs/DispersionPolicy/oneOf",
        ),
        (
            "FailurePolicy",
            FailurePolicy::ALL_TAGS,
            "/$defs/FailurePolicy/oneOf",
        ),
        (
            "StoppingRule",
            StoppingRule::ALL_TAGS,
            "/$defs/StoppingRule/oneOf",
        ),
    ];

    for (type_name, tags, pointer) in cases {
        let rust: BTreeSet<String> = tags.iter().map(|tag| tag.to_string()).collect();
        assert_no_drift(type_name, &rust, &schema_tags(&schema, pointer));
    }
}

#[test]
fn plain_enum_values_agree() {
    let schema = schema();

    let cases: &[(&str, &[&str], &str)] = &[
        ("RunTier", RunTier::ALL_TAGS, "/$defs/RunTier/enum"),
        (
            "EffectUnits",
            EffectUnits::ALL_TAGS,
            "/$defs/EffectUnits/enum",
        ),
        (
            "ReplicationUnit",
            ReplicationUnit::ALL_TAGS,
            "/$defs/ReplicationUnit/enum",
        ),
        (
            "ConfidenceLevel",
            ConfidenceLevel::ALL_TAGS,
            "/$defs/ConfidenceLevel/enum",
        ),
        (
            "IntervalMethod",
            IntervalMethod::ALL_TAGS,
            "/$defs/IntervalMethod/enum",
        ),
        (
            "MultiplicityMethod",
            MultiplicityMethod::ALL_TAGS,
            "/$defs/MultiplicityMethod/enum",
        ),
        (
            "Denominator",
            Denominator::ALL_TAGS,
            "/$defs/Denominator/enum",
        ),
    ];

    for (type_name, tags, pointer) in cases {
        let rust: BTreeSet<String> = tags.iter().map(|tag| tag.to_string()).collect();
        assert_no_drift(type_name, &rust, &schema_enum(&schema, pointer));
    }
}

/// Each declared tag must name a variant that actually parses, so a tag list
/// that drifted ahead of the types is caught rather than believed.
#[test]
fn every_declared_tag_parses_into_its_type() {
    for tag in RunTier::ALL_TAGS {
        let parsed: RunTier =
            serde_json::from_value(Value::String(tag.to_string())).expect("a declared tag parses");
        assert_eq!(parsed.tag(), *tag);
    }
    for tag in ReplicationUnit::ALL_TAGS {
        let parsed: ReplicationUnit =
            serde_json::from_value(Value::String(tag.to_string())).expect("a declared tag parses");
        assert_eq!(parsed.tag(), *tag);
    }
    for tag in ConfidenceLevel::ALL_TAGS {
        let parsed: ConfidenceLevel =
            serde_json::from_value(Value::String(tag.to_string())).expect("a declared tag parses");
        assert_eq!(parsed.tag(), *tag);
    }
    for tag in MultiplicityMethod::ALL_TAGS {
        let parsed: MultiplicityMethod =
            serde_json::from_value(Value::String(tag.to_string())).expect("a declared tag parses");
        assert_eq!(parsed.tag(), *tag);
    }
    for tag in Denominator::ALL_TAGS {
        let parsed: Denominator =
            serde_json::from_value(Value::String(tag.to_string())).expect("a declared tag parses");
        assert_eq!(parsed.tag(), *tag);
    }
    for tag in EffectUnits::ALL_TAGS {
        let parsed: EffectUnits =
            serde_json::from_value(Value::String(tag.to_string())).expect("a declared tag parses");
        assert_eq!(parsed.tag(), *tag);
    }
    for tag in IntervalMethod::ALL_TAGS {
        let parsed: IntervalMethod =
            serde_json::from_value(Value::String(tag.to_string())).expect("a declared tag parses");
        assert_eq!(parsed.tag(), *tag);
    }
}

/// The schema must not acquire a place to keep a result either. Its closed
/// objects are what a non-Rust producer writes against, so a result-bearing
/// property there would reintroduce the stale-precision path from outside.
#[test]
fn the_schema_declares_no_result_bearing_property() {
    let schema = schema();
    let text = serde_json::to_string(&schema).expect("serializes");
    for forbidden in [
        "\"achieved_half_width\"",
        "\"observed_rate\"",
        "\"realized_runs\"",
        "\"precision_claim\"",
    ] {
        assert!(
            !text.contains(forbidden),
            "the protocol schema must not declare {forbidden}"
        );
    }
}
