//! Unambiguous digest framing.
//!
//! A pre-image built by pasting field values between literal separators is only
//! injective while no value can contain a separator. Where the values are
//! agent-supplied identifiers, salts or free text, nothing enforces that, so
//! two different field assignments can serialize to the same bytes and
//! therefore to the same digest: `("a|b", "c")` and `("a", "b|c")` are one
//! string once the separators are written. Escaping trades one such rule for
//! another; length prefixes remove the question, because the reader knows how
//! far each field runs before it reads any of it.
//!
//! [`framed_preimage`] is the shape every digest in this crate that hashes more
//! than one field should be built from. It carries a domain string as well, so
//! a pre-image for one purpose can never be read as a pre-image for another
//! under the same key, the way the chain receipt domain separates a chain
//! receipt from a chain link.

/// Version of the framing layout below. A change to the layout mints a new
/// value here and, with it, a new domain string for every caller: framing is
/// part of what a digest commits to.
pub const FRAMING_VERSION: &str = "v1";

/// Injective bytes for `domain` and `fields`.
///
/// Layout: the domain string, a `0x00`, the field count as a big-endian `u64`,
/// then each field as a big-endian `u64` byte length followed by its bytes.
/// Distinct `(domain, fields)` inputs always produce distinct output, whatever
/// the fields contain, including the separators and lengths of other frames.
pub fn framed_preimage(domain: &str, fields: &[&str]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(
        domain.len() + 9 + fields.iter().map(|field| field.len() + 8).sum::<usize>(),
    );
    bytes.extend_from_slice(domain.as_bytes());
    bytes.push(0x00);
    bytes.extend_from_slice(&(fields.len() as u64).to_be_bytes());
    for field in fields {
        bytes.extend_from_slice(&(field.len() as u64).to_be_bytes());
        bytes.extend_from_slice(field.as_bytes());
    }
    bytes
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_value_cannot_borrow_a_neighbour() {
        assert_ne!(
            framed_preimage("d", &["a|b", "c"]),
            framed_preimage("d", &["a", "b|c"])
        );
        assert_ne!(
            framed_preimage("d", &["ab", "c"]),
            framed_preimage("d", &["a", "bc"])
        );
        assert_ne!(
            framed_preimage("d", &["", "ab"]),
            framed_preimage("d", &["ab", ""])
        );
    }

    #[test]
    fn a_domain_cannot_borrow_a_value() {
        assert_ne!(framed_preimage("ab", &["c"]), framed_preimage("a", &["bc"]));
        assert_ne!(framed_preimage("a", &["b"]), framed_preimage("b", &["b"]));
    }

    #[test]
    fn arity_is_committed() {
        assert_ne!(
            framed_preimage("d", &["a"]),
            framed_preimage("d", &["a", ""])
        );
    }
}
