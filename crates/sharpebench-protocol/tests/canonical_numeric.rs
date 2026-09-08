//! Cross-language fixtures for `sharpebench/canonical-json/v1`.
//!
//! Each case below is one where two mainstream serializers disagree, so a
//! digest taken by a Python producer and checked by a Rust consumer would part
//! company. The expected column is the specified form, not either default.

use serde_json::json;
use sharpebench_protocol::canonical::{
    canonical_json, canonical_number, versioned_preimage, CanonicalError, CANONICAL_JSON_VERSION,
};

#[test]
fn exponent_boundaries_are_fixed_point_inside_the_window() {
    // The reproduced R07 defect: Python's repr renders 1e-05, Rust's Display
    // renders 0.00001, and the contract digest depends on which one was hashed.
    assert_eq!(canonical_number(1e-5).unwrap(), "0.00001");
    assert_eq!(canonical_number(1e-6).unwrap(), "0.000001");
    // n = -6 is outside the fixed-point window, so this is where the form flips.
    assert_eq!(canonical_number(1e-7).unwrap(), "1e-7");
    assert_eq!(canonical_number(1.5e-7).unwrap(), "1.5e-7");
    assert_eq!(canonical_number(1e-8).unwrap(), "1e-8");
}

#[test]
fn exponent_boundaries_are_fixed_point_up_to_1e21() {
    assert_eq!(
        canonical_number(1e20).unwrap(),
        "100000000000000000000",
        "n = 21 is the last fixed-point magnitude"
    );
    assert_eq!(canonical_number(1e21).unwrap(), "1e+21");
    assert_eq!(canonical_number(1.5e21).unwrap(), "1.5e+21");
    assert_eq!(canonical_number(123456.789).unwrap(), "123456.789");
}

#[test]
fn extreme_magnitudes_keep_shortest_round_tripping_digits() {
    assert_eq!(canonical_number(5e-324).unwrap(), "5e-324");
    assert_eq!(
        canonical_number(f64::MAX).unwrap(),
        "1.7976931348623157e+308"
    );
    assert_eq!(
        canonical_number(f64::MIN_POSITIVE).unwrap(),
        "2.2250738585072014e-308"
    );
}

#[test]
fn signed_zero_has_one_canonical_form() {
    assert_eq!(canonical_number(0.0).unwrap(), "0");
    assert_eq!(canonical_number(-0.0).unwrap(), "0");
    assert_eq!(
        canonical_json(&json!({"a": 0.0, "b": -0.0})).unwrap(),
        "{\"a\":0,\"b\":0}"
    );
}

#[test]
fn integer_looking_floats_render_as_integers() {
    // Rust's serde_json writes 1.0, Python writes 1.0, ECMAScript writes 1.
    // Whichever a producer holds in memory, the digest sees the same bytes.
    assert_eq!(canonical_number(1.0).unwrap(), "1");
    assert_eq!(canonical_number(-3.0).unwrap(), "-3");
    assert_eq!(canonical_number(100.0).unwrap(), "100");
    assert_eq!(
        canonical_json(&json!({"n": 1.0})).unwrap(),
        canonical_json(&json!({"n": 1})).unwrap()
    );
}

#[test]
fn negative_values_carry_one_leading_sign() {
    assert_eq!(canonical_number(-1e-5).unwrap(), "-0.00001");
    assert_eq!(canonical_number(-1e21).unwrap(), "-1e+21");
    assert_eq!(canonical_number(-0.5).unwrap(), "-0.5");
}

#[test]
fn non_finite_numbers_have_no_canonical_form() {
    assert!(matches!(
        canonical_number(f64::NAN),
        Err(CanonicalError::NonFinite(_))
    ));
    assert!(matches!(
        canonical_number(f64::INFINITY),
        Err(CanonicalError::NonFinite(_))
    ));
    assert!(matches!(
        canonical_number(f64::NEG_INFINITY),
        Err(CanonicalError::NonFinite(_))
    ));
}

#[test]
fn unicode_boundaries_escape_exactly_the_specified_set() {
    // Controls take the short escapes, everything else passes through as UTF-8:
    // no \u for non-ASCII, no escaped solidus, no escaped U+007F, and the two
    // line separators JavaScript sources care about stay literal in JSON.
    assert_eq!(
        canonical_json(&json!("a\"b\\c/d")).unwrap(),
        "\"a\\\"b\\\\c/d\""
    );
    assert_eq!(
        canonical_json(&json!("\u{08}\u{09}\u{0a}\u{0c}\u{0d}")).unwrap(),
        "\"\\b\\t\\n\\f\\r\""
    );
    assert_eq!(
        canonical_json(&json!("\u{00}\u{1f}")).unwrap(),
        "\"\\u0000\\u001f\""
    );
    assert_eq!(canonical_json(&json!("\u{7f}")).unwrap(), "\"\u{7f}\"");
    assert_eq!(
        canonical_json(&json!("é😀\u{2028}\u{2029}")).unwrap(),
        "\"é😀\u{2028}\u{2029}\""
    );
}

#[test]
fn member_order_is_by_code_point_not_utf16_code_unit() {
    // The one place this form diverges from RFC 8785 on purpose. An astral key
    // sorts after U+FF3A by code point and before it by UTF-16 code unit, which
    // is what a JavaScript producer would emit; both of this project's producer
    // languages sort by code point, so that is the specified rule.
    assert_eq!(
        canonical_json(&json!({"\u{1f600}": 1, "\u{ff3a}": 2})).unwrap(),
        "{\"\u{ff3a}\":2,\"\u{1f600}\":1}"
    );
    assert_eq!(
        canonical_json(&json!({"b": 1, "a": 2, "A": 3})).unwrap(),
        "{\"A\":3,\"a\":2,\"b\":1}"
    );
}

#[test]
fn nesting_and_arrays_keep_their_given_order() {
    assert_eq!(
        canonical_json(&json!({"z": [3, 1, 2], "a": {"n": null, "t": true}})).unwrap(),
        "{\"a\":{\"n\":null,\"t\":true},\"z\":[3,1,2]}"
    );
}

#[test]
fn a_preimage_names_the_version_it_was_taken_under() {
    let preimage = versioned_preimage(&json!({"neutral_threshold": 1e-5})).unwrap();
    assert!(
        preimage.starts_with(CANONICAL_JSON_VERSION.as_bytes()),
        "a digest under one canonical form must not be silently comparable to another"
    );
    let body_start = CANONICAL_JSON_VERSION.len() + 1 + 8;
    assert_eq!(preimage[CANONICAL_JSON_VERSION.len()], 0x00);
    assert_eq!(&preimage[body_start..], b"{\"neutral_threshold\":0.00001}");
    let declared = u64::from_be_bytes(
        preimage[CANONICAL_JSON_VERSION.len() + 1..body_start]
            .try_into()
            .unwrap(),
    );
    assert_eq!(declared as usize, preimage.len() - body_start);
}

#[test]
fn distinct_documents_cannot_share_a_preimage() {
    // Length framing, not separator hygiene, is what keeps a document that
    // contains the frame's own punctuation from imitating a different one.
    let left = versioned_preimage(&json!({"a": "b\u{00}", "c": "d"})).unwrap();
    let right = versioned_preimage(&json!({"a": "b", "c\u{00}": "d"})).unwrap();
    assert_ne!(left, right);
}
