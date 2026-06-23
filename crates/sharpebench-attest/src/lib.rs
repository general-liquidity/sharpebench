//! sb-attest — forward-attestation.
//!
//! The un-gameable spine of SharpeBench. Two primitives:
//!
//! 1. **Commitment** — before an evaluation window's data exists, an agent
//!    publishes a SHA-256 [`Commitment`] binding (its identity, the target
//!    window, a digest of its frozen artifact, a salt). There is nothing to
//!    overfit because the data isn't out yet; revealing the pre-image later
//!    proves the agent wasn't tuned to the window.
//! 2. **Signed result chain** — each scored result is HMAC-signed over the
//!    previous signature, forming a tamper-evident chain (the same pattern
//!    Gordon uses for its audit log). Anyone holding the key can verify that a
//!    published leaderboard wasn't altered after the fact.
//!
//! Together these let a *host-operated* benchmark be *independently trusted*:
//! the numbers are reproducible from a pre-registered artifact and the published
//! chain is tamper-evident.
#![forbid(unsafe_code)]

pub mod registry;
pub mod sealed;

pub use sealed::{
    commit_dataset, content_hash, open_dataset, seal_dataset, verify_dataset, DatasetCommitment,
    SealedDataset,
};

use std::fmt::Write as _;

use hmac::{Hmac, Mac};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

type HmacSha256 = Hmac<Sha256>;

/// A pre-registration commitment, published before the target window's data exists.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct Commitment {
    pub agent_id: String,
    pub target_window: String,
    /// Hex SHA-256 of `agent_id | target_window | artifact_digest | salt`.
    pub commit_hash: String,
}

/// Build a commitment. `artifact_digest` is a hash of the agent's frozen
/// binary/config; `salt` is a private nonce revealed only at reveal time.
pub fn make_commitment(
    agent_id: &str,
    target_window: &str,
    artifact_digest: &str,
    salt: &str,
) -> Commitment {
    let mut h = Sha256::new();
    for part in [agent_id, target_window, artifact_digest, salt] {
        h.update(part.as_bytes());
        h.update(b"|");
    }
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

/// A signed link in the result chain.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SignedResult {
    /// Canonical serialization of the scored result (e.g. a `CompositeScore` JSON).
    pub payload: String,
    /// Signature of the previous link, or `"genesis"` for the first.
    pub prev_signature: String,
    /// Hex HMAC-SHA256 over `prev_signature | payload`.
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
fn from_hex(s: &str) -> Option<Vec<u8>> {
    if !s.len().is_multiple_of(2) {
        return None;
    }
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).ok())
        .collect()
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
