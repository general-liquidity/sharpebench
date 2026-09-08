//! Versioned canonical JSON for digest pre-images.
//!
//! # Why this exists
//!
//! A contract digest is only an identity if every producer, in every language,
//! serializes the same document to the same bytes. Language defaults do not:
//! for `1e-5` Python's `repr` emits `1e-05` while Rust's shortest-roundtrip
//! `Display` emits `0.00001`, so a contract hashed by a Python producer is
//! rejected by a Rust consumer as an unknown digest. The difference is
//! fixed-point versus exponential formatting, not exponent-digit padding, so
//! zero-padding the exponent cannot reconcile the two.
//!
//! # The specified form (`sharpebench/canonical-json/v1`)
//!
//! 1. **Numbers** use the ECMAScript `Number::toString` form, the same numeric
//!    form RFC 8785 (JSON Canonicalization Scheme) section 3.2.2.3 adopts:
//!    shortest round-tripping decimal digits, fixed-point while the decimal
//!    exponent `n` satisfies `-6 < n <= 21`, exponential outside that range
//!    with an unpadded exponent carrying an explicit sign. `-0.0`, `0.0` and
//!    integer-valued floats render as integers (`0`, `1`, `100`). Non-finite
//!    values are rejected: JSON has no NaN or infinity.
//! 2. **Strings** use RFC 8785 escaping, which is what `serde_json` already
//!    emits: escape only `"`, `\` and the C0 controls, prefer the two-character
//!    escapes `\b \t \n \f \r`, `\u00xx` for the rest, and pass every other
//!    scalar through as UTF-8. `/` and non-ASCII are not escaped.
//! 3. **Object members** are ordered by Unicode **code point**, which is byte
//!    order for UTF-8 and matches Python's `json.dumps(sort_keys=True)` and
//!    Rust's `str` ordering. This is a deliberate, documented divergence from
//!    RFC 8785, which orders by UTF-16 code units; the two disagree whenever an
//!    astral key (U+10000 and above) is compared against a key in U+E000
//!    through U+FFFF. Code-point order is the one both of this project's
//!    producer languages reach by default.
//! 4. **Arrays** keep their given order; `null`, `true` and `false` are literal.
//!
//! # Versioning
//!
//! [`versioned_preimage`] frames the canonical text under
//! [`CANONICAL_JSON_VERSION`] and a byte length, so a digest taken under one
//! version can never be silently equal to a digest of the same document under
//! another. Changing the rules above means minting a new version string, not
//! editing this one in place.

use std::fmt;

use serde_json::{Number, Value};

/// Identifier of the canonical form implemented by this module. A new numeric,
/// string or ordering rule requires a new value here.
pub const CANONICAL_JSON_VERSION: &str = "sharpebench/canonical-json/v1";

/// A document that has no canonical form under [`CANONICAL_JSON_VERSION`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CanonicalError {
    /// JSON cannot represent NaN or an infinity, so neither can a digest of one.
    NonFinite(String),
}

impl fmt::Display for CanonicalError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CanonicalError::NonFinite(rendered) => {
                write!(
                    f,
                    "canonical JSON has no form for the non-finite {rendered}"
                )
            }
        }
    }
}

impl std::error::Error for CanonicalError {}

/// The canonical text of one number.
///
/// ```
/// use sharpebench_protocol::canonical::canonical_number;
/// // The R07 case: Python's repr says `1e-05`, Rust's Display says `0.00001`.
/// assert_eq!(canonical_number(1e-5).unwrap(), "0.00001");
/// assert_eq!(canonical_number(1e-7).unwrap(), "1e-7");
/// assert_eq!(canonical_number(-0.0).unwrap(), "0");
/// assert_eq!(canonical_number(2.0).unwrap(), "2");
/// ```
pub fn canonical_number(value: f64) -> Result<String, CanonicalError> {
    if !value.is_finite() {
        return Err(CanonicalError::NonFinite(format!("{value}")));
    }
    // Covers -0.0, which compares equal to 0.0 and renders as `0`.
    if value == 0.0 {
        return Ok("0".to_string());
    }

    // Rust's `Display` for f64 is shortest-round-tripping and never
    // exponential, so it is exactly the digit string the ECMAScript algorithm
    // is defined over, already laid out positionally.
    let magnitude = format!("{}", value.abs());
    let (integer_part, fraction_part) = match magnitude.split_once('.') {
        Some((integer, fraction)) => (integer, fraction),
        None => (magnitude.as_str(), ""),
    };
    let all = format!("{integer_part}{fraction_part}");
    let leading_zeros = all.bytes().take_while(|byte| *byte == b'0').count();
    // `n` is the decimal exponent: the value is `0.<digits> * 10^n`.
    let n = integer_part.len() as i64 - leading_zeros as i64;
    let digits = all[leading_zeros..].trim_end_matches('0');
    let k = digits.len() as i64;

    let body = if k <= n && n <= 21 {
        format!("{digits}{}", "0".repeat((n - k) as usize))
    } else if 0 < n && n <= 21 {
        let split = n as usize;
        format!("{}.{}", &digits[..split], &digits[split..])
    } else if -6 < n && n <= 0 {
        format!("0.{}{digits}", "0".repeat((-n) as usize))
    } else if k == 1 {
        format!("{digits}e{}", signed_exponent(n - 1))
    } else {
        format!(
            "{}.{}e{}",
            &digits[..1],
            &digits[1..],
            signed_exponent(n - 1)
        )
    };

    Ok(if value < 0.0 {
        format!("-{body}")
    } else {
        body
    })
}

fn signed_exponent(exponent: i64) -> String {
    if exponent < 0 {
        format!("-{}", -exponent)
    } else {
        format!("+{exponent}")
    }
}

/// The canonical text of one JSON number as it arrived on the wire. Integer
/// literals keep their exact value rather than passing through `f64`.
pub fn canonical_json_number(number: &Number) -> Result<String, CanonicalError> {
    if let Some(value) = number.as_i64() {
        return Ok(value.to_string());
    }
    if let Some(value) = number.as_u64() {
        return Ok(value.to_string());
    }
    let value = number
        .as_f64()
        .ok_or_else(|| CanonicalError::NonFinite(number.to_string()))?;
    canonical_number(value)
}

/// The canonical text of a whole document under [`CANONICAL_JSON_VERSION`].
pub fn canonical_json(value: &Value) -> Result<String, CanonicalError> {
    let mut output = String::new();
    write_canonical(value, &mut output)?;
    Ok(output)
}

fn write_canonical(value: &Value, output: &mut String) -> Result<(), CanonicalError> {
    match value {
        Value::Null => output.push_str("null"),
        Value::Bool(true) => output.push_str("true"),
        Value::Bool(false) => output.push_str("false"),
        Value::Number(number) => output.push_str(&canonical_json_number(number)?),
        Value::String(text) => write_canonical_string(text, output),
        Value::Array(values) => {
            output.push('[');
            for (index, value) in values.iter().enumerate() {
                if index > 0 {
                    output.push(',');
                }
                write_canonical(value, output)?;
            }
            output.push(']');
        }
        Value::Object(members) => {
            output.push('{');
            // `serde_json::Map` is a BTreeMap unless `preserve_order` is on, so
            // sort explicitly rather than depending on a feature flag chosen by
            // whatever else is in the dependency graph.
            let mut fields: Vec<_> = members.iter().collect();
            fields.sort_by(|left, right| left.0.cmp(right.0));
            for (index, (key, value)) in fields.into_iter().enumerate() {
                if index > 0 {
                    output.push(',');
                }
                write_canonical_string(key, output);
                output.push(':');
                write_canonical(value, output)?;
            }
            output.push('}');
        }
    }
    Ok(())
}

/// RFC 8785 string escaping. Written out rather than delegated to
/// `serde_json::to_string` so the canonical rules are readable at the one place
/// the format is specified, and so a `serde_json` escaping change cannot move a
/// digest without a version bump.
fn write_canonical_string(text: &str, output: &mut String) {
    output.push('"');
    for character in text.chars() {
        match character {
            '"' => output.push_str("\\\""),
            '\\' => output.push_str("\\\\"),
            '\u{08}' => output.push_str("\\b"),
            '\u{09}' => output.push_str("\\t"),
            '\u{0a}' => output.push_str("\\n"),
            '\u{0c}' => output.push_str("\\f"),
            '\u{0d}' => output.push_str("\\r"),
            control if control < '\u{20}' => {
                output.push_str(&format!("\\u{:04x}", control as u32));
            }
            other => output.push(other),
        }
    }
    output.push('"');
}

/// The bytes to hash for `value`.
///
/// The frame is `version | 0x00 | big-endian u64 body length | body`. The
/// version tag makes a digest taken under a future canonical form incomparable
/// to this one, and the length prefix makes the frame injective: no document,
/// however it is worded, can imitate the header of another.
pub fn versioned_preimage(value: &Value) -> Result<Vec<u8>, CanonicalError> {
    Ok(frame(CANONICAL_JSON_VERSION, &canonical_json(value)?))
}

fn frame(version: &str, body: &str) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(version.len() + 9 + body.len());
    bytes.extend_from_slice(version.as_bytes());
    bytes.push(0x00);
    bytes.extend_from_slice(&(body.len() as u64).to_be_bytes());
    bytes.extend_from_slice(body.as_bytes());
    bytes
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn a_future_version_cannot_share_a_preimage() {
        let value = json!({"neutral_threshold": 1e-5});
        let body = canonical_json(&value).unwrap();
        assert_eq!(
            versioned_preimage(&value).unwrap(),
            frame(CANONICAL_JSON_VERSION, &body)
        );
        assert_ne!(
            versioned_preimage(&value).unwrap(),
            frame("sharpebench/canonical-json/v2", &body)
        );
    }

    #[test]
    fn the_frame_is_injective_across_body_boundaries() {
        // Without the length prefix, a body that begins with the tail of the
        // header of another frame could be read two ways. With it, the reader
        // knows where the body ends before it reads a byte of it.
        let short = frame(CANONICAL_JSON_VERSION, "\"ab\"");
        let long = frame(CANONICAL_JSON_VERSION, "\"abc\"");
        assert_ne!(short, long);
        assert!(!long.starts_with(&short));
    }
}
