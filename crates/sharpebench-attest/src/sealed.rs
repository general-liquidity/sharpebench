//! Sealed held-out evaluation sets — training-set contamination defense.
//!
//! The forward bars an agent is scored on must be *frozen and secret* until the
//! window unlocks: if the plaintext leaks, an agent can train on it and the score
//! is contaminated. But the host also wants to prove, publicly and in advance,
//! that the set wasn't swapped after results came in. These pull in opposite
//! directions — publish vs. conceal — and a [`DatasetCommitment`] resolves both:
//!
//!  - **Commit** to the frozen bytes by content hash (publishable now, reveals
//!    nothing) plus a random **canary GUID** embedded in (and bound to) the set.
//!    The canary is a tripwire: if it ever surfaces in an agent's training data
//!    or output, the held-out set leaked.
//!  - **Seal** the plaintext under a secret key into ciphertext that can sit in a
//!    public artifact; **open** it with the key once the window unlocks.
//!  - **Verify** any candidate plaintext against the published hash, so anyone can
//!    confirm the opened bytes are exactly what was committed.
//!
//! The cipher is a deterministic HMAC-SHA256 keystream (CTR-style) XORed over the
//! plaintext — built only on the crate's existing `hmac` + `sha2` deps. This is a
//! confidentiality wrapper for a public artifact whose integrity is *separately*
//! guaranteed by the content-hash commitment and the signed result chain; it is
//! deliberately NOT a `ring`-based AEAD (forbidden by `deny.toml`). Tamper
//! detection is the hash check in [`verify_dataset`], not the cipher.

use hmac::{Hmac, Mac};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::to_hex;

type HmacSha256 = Hmac<Sha256>;

/// A public commitment to a frozen held-out dataset.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct DatasetCommitment {
    /// Hex SHA-256 of the plaintext dataset bytes.
    pub content_hash: String,
    /// A random canary GUID bound into the commitment. Surfacing this token in
    /// training data or agent output is proof the held-out set leaked.
    pub canary: String,
    /// Length of the committed plaintext in bytes (lets a verifier reject a
    /// truncated/padded candidate before hashing).
    pub len: usize,
}

/// Hex SHA-256 of `bytes`.
pub fn content_hash(bytes: &[u8]) -> String {
    let mut h = Sha256::new();
    h.update(bytes);
    to_hex(&h.finalize())
}

/// Build a dataset commitment over `bytes` with a caller-supplied `canary` GUID.
/// The canary is opaque (any unique string — a UUID, a random hex token); the
/// host generates it out of band so this stays pure/deterministic.
pub fn commit_dataset(bytes: &[u8], canary: &str) -> DatasetCommitment {
    DatasetCommitment {
        content_hash: content_hash(bytes),
        canary: canary.to_string(),
        len: bytes.len(),
    }
}

/// Verify a candidate plaintext against a published commitment: the length and
/// content hash must both match. Constant-time hash comparison is unnecessary —
/// the hash is public — but the length pre-check avoids hashing obviously-wrong
/// candidates.
pub fn verify_dataset(bytes: &[u8], committed: &DatasetCommitment) -> bool {
    bytes.len() == committed.len && content_hash(bytes) == committed.content_hash
}

/// Sealed (encrypted) dataset bytes, safe to place in a public artifact.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct SealedDataset {
    /// Hex ciphertext (HMAC keystream XORed over the plaintext).
    pub ciphertext: String,
    /// The public commitment to the *plaintext*, so an opener can verify what it
    /// decrypted is exactly what was committed.
    pub commitment: DatasetCommitment,
}

/// Derive the keystream block for counter `ctr`: `HMAC-SHA256(key, "sb-seal" | ctr_le)`.
fn keystream_block(key: &[u8], ctr: u64) -> [u8; 32] {
    let mut mac = HmacSha256::new_from_slice(key).expect("HMAC accepts any key length");
    mac.update(b"sb-seal");
    mac.update(&ctr.to_le_bytes());
    mac.finalize().into_bytes().into()
}

/// XOR `data` against the HMAC-SHA256 CTR keystream under `key`. Symmetric: the
/// same call seals and opens (XOR is an involution over a fixed keystream).
fn xor_keystream(key: &[u8], data: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(data.len());
    let mut ctr = 0u64;
    let mut block = keystream_block(key, ctr);
    for (i, &byte) in data.iter().enumerate() {
        let j = i % 32;
        if i != 0 && j == 0 {
            ctr += 1;
            block = keystream_block(key, ctr);
        }
        out.push(byte ^ block[j]);
    }
    out
}

/// Seal a frozen dataset: encrypt `plaintext` under `key` and bind a public
/// commitment (content hash + `canary`) to it. The result can be published; the
/// plaintext cannot be recovered without `key`, yet the commitment proves which
/// dataset was frozen.
pub fn seal_dataset(plaintext: &[u8], key: &[u8], canary: &str) -> SealedDataset {
    SealedDataset {
        ciphertext: to_hex(&xor_keystream(key, plaintext)),
        commitment: commit_dataset(plaintext, canary),
    }
}

/// Open a sealed dataset with `key`, returning the recovered plaintext only if it
/// verifies against the embedded commitment. `None` on a malformed ciphertext or
/// a commitment mismatch (wrong key → wrong plaintext → hash fails).
pub fn open_dataset(sealed: &SealedDataset, key: &[u8]) -> Option<Vec<u8>> {
    let cipher = decode_hex(&sealed.ciphertext)?;
    let plaintext = xor_keystream(key, &cipher);
    if verify_dataset(&plaintext, &sealed.commitment) {
        Some(plaintext)
    } else {
        None
    }
}

/// Decode an even-length hex string to bytes (`None` if malformed).
fn decode_hex(s: &str) -> Option<Vec<u8>> {
    if !s.len().is_multiple_of(2) {
        return None;
    }
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).ok())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const KEY: &[u8] = b"sharpebench-heldout-seal-key";

    // A plaintext longer than one 32-byte keystream block, to exercise the CTR
    // counter advance.
    fn bars() -> Vec<u8> {
        (0..200u32).flat_map(|i| i.to_le_bytes()).collect()
    }

    #[test]
    fn verify_accepts_exact_and_rejects_tampered() {
        let data = bars();
        let c = commit_dataset(&data, "canary-guid-001");
        assert!(verify_dataset(&data, &c));
        let mut tampered = data.clone();
        tampered[0] ^= 0xFF;
        assert!(!verify_dataset(&tampered, &c), "a flipped bit must fail");
        // A truncation fails on the length pre-check.
        assert!(!verify_dataset(&data[..data.len() - 1], &c));
    }

    #[test]
    fn seal_open_roundtrip() {
        let data = bars();
        let sealed = seal_dataset(&data, KEY, "canary-guid-002");
        // Ciphertext is not the plaintext (confidentiality).
        assert_ne!(sealed.ciphertext, to_hex(&data));
        let opened = open_dataset(&sealed, KEY).expect("opens with the right key");
        assert_eq!(opened, data);
        // The embedded commitment carries the canary forward.
        assert_eq!(sealed.commitment.canary, "canary-guid-002");
    }

    #[test]
    fn wrong_key_fails_to_open() {
        let data = bars();
        let sealed = seal_dataset(&data, KEY, "c");
        assert!(
            open_dataset(&sealed, b"the-wrong-key").is_none(),
            "wrong key must not yield a verifying plaintext"
        );
    }

    #[test]
    fn malformed_ciphertext_returns_none() {
        let data = bars();
        let mut sealed = seal_dataset(&data, KEY, "c");
        sealed.ciphertext.push('z'); // odd-length, non-hex
        assert!(open_dataset(&sealed, KEY).is_none());
    }

    #[test]
    fn committed_hash_publishable_without_plaintext() {
        // The commitment reveals only hash + canary + len — not the bytes.
        let data = bars();
        let c = commit_dataset(&data, "canary");
        assert_eq!(c.len, data.len());
        assert_eq!(c.content_hash.len(), 64); // SHA-256 hex
                                              // Anyone can later verify the revealed plaintext against the public hash.
        assert!(verify_dataset(&data, &c));
    }

    #[test]
    fn empty_dataset_roundtrips() {
        let sealed = seal_dataset(&[], KEY, "c");
        assert_eq!(open_dataset(&sealed, KEY), Some(Vec::new()));
    }
}
