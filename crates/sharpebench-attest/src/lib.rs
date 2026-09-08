//! sb-attest — forward-attestation.
//!
//! Byte-binding and signed-chain primitives for SharpeBench:
//!
//! 1. **Commitment** — before the operator-declared commitment deadline, an
//!    agent publishes a SHA-256 [`Commitment`] binding its identity, target
//!    window, frozen-artifact digest, and salt. Revealing the pre-image later
//!    proves only that those bytes match the earlier commitment. This crate
//!    does not observe wall time, data availability, or what either party saw.
//! 2. **Signed result chain** — each scored result is signed over the previous
//!    signature, forming a tamper-evident chain (the same pattern Gordon uses
//!    for its audit log). Two schemes share one link shape ([`SignedResult`]):
//!    - **HMAC-SHA256** ([`sign_result`] / [`verify_chain`]): symmetric. Anyone
//!      holding the key can verify the chain, and anyone holding the key can
//!      forge it. Integrity for key-holders; the host's own audit trail.
//!    - **Ed25519** ([`public`]): asymmetric. The host signs with a private key
//!      and publishes the verifying key inside the board. Anyone with the
//!      document can check internal consistency under that key; authenticating
//!      the key as the operator's identity remains an out-of-band task.
//!
//! Together these make committed bytes and the signed chain independently
//! checkable. They do not witness chronology. A forward interpretation trusts
//! the operator-declared deadline, epoch advancement, custody of held-out data
//! and signing keys, prior non-observation by entrants, and neutral host
//! operation.
#![forbid(unsafe_code)]

pub mod canary;
pub mod framing;
pub mod public;
pub mod registry;
pub mod sealed;

pub use canary::{detect_leak, embed_canary, make_canary, verify_canary, Canary};
pub use framing::{framed_preimage, FRAMING_VERSION};
pub use public::{
    publish_public_chain, sign_chain_receipt_public, sign_result_public, verify_chain_public,
    verify_chain_public_anchored, verify_chain_receipt_public, verify_public_chain,
    verify_public_chain_with, PublicChain, SigningKey, VerifyingKey, ED25519_SCHEME,
};
pub use sealed::{
    commit_dataset, content_hash, open_dataset, seal_dataset, seal_dataset_with_nonce,
    verify_dataset, DatasetCommitment, SealError, SealedDataset, SEALED_DATASET_SCHEME,
};

use std::fmt::Write as _;

use hmac::{Hmac, Mac};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

type HmacSha256 = Hmac<Sha256>;

/// A commitment intended for publication before the operator-declared deadline.
///
/// The value contains no wall-time or data-availability witness. Its forward
/// meaning therefore depends on an external chronology and custody process.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct Commitment {
    pub agent_id: String,
    pub target_window: String,
    /// Hex SHA-256 of [`framed_preimage`] over [`COMMITMENT_DOMAIN`] and
    /// `agent_id`, `target_window`, `artifact_digest`, `salt`, in that order.
    pub commit_hash: String,
}

/// Domain and framing version of a [`Commitment`] pre-image.
///
/// `v1` pasted the four fields between literal `|` separators, so an agent
/// could move a separator from one field into the next and commit to the same
/// hash under a different identity: `("a|b", "c", ..)` and `("a", "b|c", ..)`
/// produced one pre-image. `v2` frames the fields by length, which is not a
/// reinterpretation of the old digest but a different one; a commitment
/// computed under `v1` does not verify here, and must be recomputed.
pub const COMMITMENT_DOMAIN: &str = "sharpebench-attest/commitment/v2";

/// Build a commitment. `artifact_digest` is a hash of the agent's frozen
/// binary/config; `salt` is a private nonce revealed only at reveal time.
pub fn make_commitment(
    agent_id: &str,
    target_window: &str,
    artifact_digest: &str,
    salt: &str,
) -> Commitment {
    let mut h = Sha256::new();
    h.update(framed_preimage(
        COMMITMENT_DOMAIN,
        &[agent_id, target_window, artifact_digest, salt],
    ));
    Commitment {
        agent_id: agent_id.to_string(),
        target_window: target_window.to_string(),
        commit_hash: to_hex(&h.finalize()),
    }
}

/// Verify a revealed pre-image against a previously published commitment.
pub fn verify_commitment(
    c: &Commitment,
    agent_id: &str,
    target_window: &str,
    artifact_digest: &str,
    salt: &str,
) -> bool {
    make_commitment(agent_id, target_window, artifact_digest, salt) == *c
}

/// SHA-256 hex digest of arbitrary content — used to bind a leaderboard entry to
/// the exact frozen dataset (and any other run inputs) it was scored on, so the
/// published blob is self-re-derivable. Same primitive as [`make_commitment`],
/// exposed for the run-spec.
pub fn content_digest(bytes: &[u8]) -> String {
    let mut h = Sha256::new();
    h.update(bytes);
    to_hex(&h.finalize())
}

/// A signed link in the result chain.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SignedResult {
    /// Canonical serialization of the scored result (e.g. a `CompositeScore` JSON).
    pub payload: String,
    /// Signature of the previous link, or `"genesis"` for the first.
    pub prev_signature: String,
    /// Hex signature over `prev_signature | payload`: HMAC-SHA256 (32 bytes) in
    /// a [`sign_result`] chain, Ed25519 (64 bytes) in a [`sign_result_public`]
    /// chain. Same wire shape, so the two chains are link-for-link comparable.
    pub signature: String,
}

pub const GENESIS: &str = "genesis";

/// Append a result to the chain, signing it against `prev_signature`.
pub fn sign_result(payload: &str, prev_signature: &str, key: &[u8]) -> SignedResult {
    let mut mac = HmacSha256::new_from_slice(key).expect("HMAC accepts any key length");
    mac.update(prev_signature.as_bytes());
    mac.update(b"|");
    mac.update(payload.as_bytes());
    SignedResult {
        payload: payload.to_string(),
        prev_signature: prev_signature.to_string(),
        signature: to_hex(&mac.finalize().into_bytes()),
    }
}

/// Decode a hex string to bytes (`None` if malformed). Used so the HMAC can be
/// compared in constant time via [`Mac::verify_slice`] instead of a variable-time
/// string `==`, which would leak a timing signal about the secret key.
pub(crate) fn from_hex(s: &str) -> Option<Vec<u8>> {
    if !s.len().is_multiple_of(2) {
        return None;
    }
    let nibble = |b: u8| match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    };
    s.as_bytes()
        .chunks_exact(2)
        .map(|pair| Some(nibble(pair[0])? * 16 + nibble(pair[1])?))
        .collect()
}

#[cfg(test)]
mod hex_boundary_tests {
    use super::*;

    #[test]
    fn arbitrary_utf8_is_refused_not_sliced_at_byte_offsets() {
        for invalid in ["€€", "a€", "🦀", "０１", "é", "00gg", "a"] {
            assert_eq!(from_hex(invalid), None, "{invalid}");
            assert!(VerifyingKey::from_hex(invalid).is_none());
            let mut sealed =
                seal_dataset_with_nonce(b"dataset", &[7; 32], "canary", [1; 12]).unwrap();
            sealed.ciphertext = invalid.into();
            assert!(open_dataset(&sealed, &[7; 32]).is_none());
        }
        assert_eq!(from_hex("00aAfF"), Some(vec![0, 170, 255]));
        assert_eq!(from_hex(""), Some(vec![]));
    }
}

/// Verify a full chain: every link's `prev_signature` must match the previous
/// link's signature, and every signature must recompute. Tamper-evident. The MAC
/// check is **constant-time** (`verify_slice`), so verification leaks no timing
/// signal about the key even under repeated adversarial probing.
pub fn verify_chain(results: &[SignedResult], key: &[u8]) -> bool {
    let mut prev = GENESIS.to_string();
    for r in results {
        if r.prev_signature != prev {
            return false;
        }
        let Some(expected) = from_hex(&r.signature) else {
            return false;
        };
        let mut mac = HmacSha256::new_from_slice(key).expect("HMAC accepts any key length");
        mac.update(r.prev_signature.as_bytes());
        mac.update(b"|");
        mac.update(r.payload.as_bytes());
        if mac.verify_slice(&expected).is_err() {
            return false;
        }
        prev = r.signature.clone();
    }
    true
}

/// Domain-separation prefix for a [`ChainReceipt`] message. Keeps a receipt
/// signature from ever being readable as a link signature under the same key.
pub(crate) const RECEIPT_DOMAIN: &str = "sharpebench-attest/chain-receipt/v1";

/// A signed statement of where a chain ends: how many links the signer
/// published and what the last one's signature was.
///
/// A genesis-anchored chain proves that the links a document contains are the
/// ones that were signed, in the order they were signed. It cannot prove that
/// they are *all* of them, because every prefix of a valid chain is itself a
/// valid chain: dropping the final records leaves a document that verifies.
/// Interior deletion breaks the `prev_signature` link, terminal deletion does
/// not. The receipt closes that asymmetry, since restating a count and a
/// terminal signature for a shortened chain requires the signing key.
///
/// The receipt binds completeness, not chronology. It says nothing about when
/// the chain was signed or whether the signer withheld records before signing.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ChainReceipt {
    /// Number of links the signer published.
    pub records: usize,
    /// Signature of the last link, or [`GENESIS`] for an empty chain.
    pub terminal_signature: String,
    /// Hex signature over the domain-separated `records | terminal_signature`:
    /// HMAC-SHA256 in a [`sign_chain_receipt`] receipt, Ed25519 in a
    /// [`public::sign_chain_receipt_public`] one, matching its chain's scheme.
    pub signature: String,
}

/// The bytes a receipt signs. Signer and verifier must agree byte for byte.
pub(crate) fn receipt_message(records: usize, terminal_signature: &str) -> Vec<u8> {
    format!("{RECEIPT_DOMAIN}|{records}|{terminal_signature}").into_bytes()
}

/// The signature a complete chain ends on: the last link's, or [`GENESIS`] when
/// the chain is empty, so an empty chain still has an unforgeable receipt.
pub(crate) fn terminal_signature(results: &[SignedResult]) -> &str {
    results.last().map_or(GENESIS, |r| r.signature.as_str())
}

/// Sign a terminal receipt for an HMAC chain. Same key as [`sign_result`].
pub fn sign_chain_receipt(results: &[SignedResult], key: &[u8]) -> ChainReceipt {
    let terminal = terminal_signature(results);
    let mut mac = HmacSha256::new_from_slice(key).expect("HMAC accepts any key length");
    mac.update(&receipt_message(results.len(), terminal));
    ChainReceipt {
        records: results.len(),
        terminal_signature: terminal.to_string(),
        signature: to_hex(&mac.finalize().into_bytes()),
    }
}

/// Check a terminal receipt against the chain actually supplied: the signature
/// must recompute under `key`, and the count and terminal signature it commits
/// to must be the ones observed. A truncated chain fails on both counts.
pub fn verify_chain_receipt(results: &[SignedResult], receipt: &ChainReceipt, key: &[u8]) -> bool {
    let Some(expected) = from_hex(&receipt.signature) else {
        return false;
    };
    let mut mac = HmacSha256::new_from_slice(key).expect("HMAC accepts any key length");
    mac.update(&receipt_message(
        receipt.records,
        &receipt.terminal_signature,
    ));
    mac.verify_slice(&expected).is_ok()
        && receipt.records == results.len()
        && receipt.terminal_signature == terminal_signature(results)
}

/// Verify an HMAC chain **and** its terminal receipt: the complete check.
///
/// [`verify_chain`] alone accepts any valid prefix, so use this wherever the
/// question is "is this the whole chain" rather than "are these links genuine".
pub fn verify_chain_anchored(results: &[SignedResult], receipt: &ChainReceipt, key: &[u8]) -> bool {
    verify_chain(results, key) && verify_chain_receipt(results, receipt, key)
}

pub(crate) fn to_hex(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        let _ = write!(s, "{b:02x}");
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn commitment_roundtrip() {
        let c = make_commitment("gordon", "2025-Q4", "deadbeef", "secret-salt");
        assert!(verify_commitment(
            &c,
            "gordon",
            "2025-Q4",
            "deadbeef",
            "secret-salt"
        ));
        // Any changed field (e.g. a swapped artifact) fails to verify.
        assert!(!verify_commitment(
            &c,
            "gordon",
            "2025-Q4",
            "tampered!",
            "secret-salt"
        ));
    }

    #[test]
    fn content_digest_is_stable_and_sensitive() {
        assert_eq!(content_digest(b"abc"), content_digest(b"abc"));
        assert_ne!(content_digest(b"abc"), content_digest(b"abd"));
        // 32-byte SHA-256 → 64 hex chars.
        assert_eq!(content_digest(b"abc").len(), 64);
    }

    #[test]
    fn chain_signs_and_verifies() {
        let key = b"sharpebench-leaderboard-key";
        let r1 = sign_result("{\"agent\":\"a\",\"dsr\":0.97}", GENESIS, key);
        let r2 = sign_result("{\"agent\":\"b\",\"dsr\":0.40}", &r1.signature, key);
        assert!(verify_chain(&[r1.clone(), r2.clone()], key));
    }

    #[test]
    fn tampering_breaks_the_chain() {
        let key = b"sharpebench-leaderboard-key";
        let r1 = sign_result("{\"agent\":\"a\",\"dsr\":0.97}", GENESIS, key);
        let mut r2 = sign_result("{\"agent\":\"b\",\"dsr\":0.40}", &r1.signature, key);
        r2.payload = "{\"agent\":\"b\",\"dsr\":0.99}".to_string(); // forge a better score
        assert!(!verify_chain(&[r1, r2], key));
    }

    #[test]
    fn wrong_key_fails() {
        let r1 = sign_result("x", GENESIS, b"key-a");
        assert!(!verify_chain(&[r1], b"key-b"));
    }
}
