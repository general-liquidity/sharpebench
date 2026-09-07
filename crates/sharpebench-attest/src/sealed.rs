//! Versioned authenticated seals for held-out dataset bytes.
//!
//! V2 uses RustCrypto AES-256-GCM-SIV (RFC 8452), with a fresh 96-bit OS-random
//! nonce for each [`seal_dataset`] call. The algorithm resists catastrophic
//! nonce-reuse failure; fresh nonces are still required, and key/message usage
//! limits in RFC 8452 section 9 still apply. This integration is not independently
//! security-audited. Authentication binds the format and the entire commitment
//! metadata, including the canary. A failed open never returns plaintext.
//!
//! The public SHA-256/length commitment permits offline guesses of predictable
//! plaintext and reveals equality and length. It is not a hiding commitment.
//! A public canary can be copied without access to the dataset, so its appearance
//! alone does not prove training contamination. The caller embeds any private
//! leak-detection marker in the plaintext; this module does not insert markers.
//!
//! Unversioned V1 seals reused a keystream under the same key. They are refused,
//! not silently decrypted or reinterpreted. Recover a trusted original privately
//! and reseal with a fresh key; resealing cannot undo any prior exposure. Public
//! chronology, operator identity and earlier commitment custody remain external
//! requirements even for a valid authenticated seal.

use aes_gcm_siv::{
    aead::{Aead, KeyInit, Payload},
    Aes256GcmSiv, Nonce,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::to_hex;

/// Authenticated format identifier. Unknown and legacy formats are refused.
pub const SEALED_DATASET_SCHEME: &str = "sharpebench.sealed.aes-256-gcm-siv.v2";

/// A sealing failure. No weak-key derivation or entropy fallback is performed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SealError {
    InvalidKeyLength,
    EntropyUnavailable,
    InputTooLarge,
}

impl std::fmt::Display for SealError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::InvalidKeyLength => "dataset sealing requires a 32-byte key",
            Self::EntropyUnavailable => {
                "dataset sealing requires an available secure entropy source"
            }
            Self::InputTooLarge => {
                "dataset or metadata exceeds the authenticated cipher's size limits"
            }
        })
    }
}

impl std::error::Error for SealError {}

/// A public commitment to a frozen held-out dataset.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct DatasetCommitment {
    /// Hex SHA-256 of the plaintext dataset bytes.
    pub content_hash: String,
    /// Public caller-supplied canary metadata, authenticated by a V2 seal but not
    /// included in `content_hash`. Its appearance alone is not evidence of a leak.
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

/// Authenticated ciphertext and public metadata. See module-level leakage limits.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct SealedDataset {
    /// Missing scheme identifies a legacy object, which `open_dataset` refuses.
    #[serde(default)]
    pub scheme: String,
    /// Hex 96-bit nonce. Missing or malformed nonces are refused.
    #[serde(default)]
    pub nonce: String,
    /// Hex ciphertext followed by the 16-byte authentication tag.
    pub ciphertext: String,
    /// The public commitment to the *plaintext*, so an opener can verify what it
    /// decrypted is exactly what was committed.
    pub commitment: DatasetCommitment,
}

// Fixed domain followed by length-framed UTF-8 hash/canary and a u64 byte count.
// This is independent of JSON whitespace, object ordering and platform usize width.
fn associated_data(commitment: &DatasetCommitment) -> Result<Vec<u8>, SealError> {
    let mut aad = SEALED_DATASET_SCHEME.as_bytes().to_vec();
    for field in [&commitment.content_hash, &commitment.canary] {
        let len = u64::try_from(field.len()).map_err(|_| SealError::InputTooLarge)?;
        aad.extend_from_slice(&len.to_le_bytes());
        aad.extend_from_slice(field.as_bytes());
    }
    let len = u64::try_from(commitment.len).map_err(|_| SealError::InputTooLarge)?;
    aad.extend_from_slice(&len.to_le_bytes());
    Ok(aad)
}

/// Seal using a fresh OS-random nonce. `key` must contain 32 cryptographically
/// random bytes, not a password. Returns an error if entropy is unavailable
/// (including wasm32, which must use `seal_dataset_with_nonce` with host entropy).
pub fn seal_dataset(
    plaintext: &[u8],
    key: &[u8],
    canary: &str,
) -> Result<SealedDataset, SealError> {
    if key.len() != 32 {
        return Err(SealError::InvalidKeyLength);
    }
    let nonce = fresh_nonce()?;
    seal_dataset_with_nonce(plaintext, key, canary, nonce)
}

#[cfg(not(target_arch = "wasm32"))]
fn fresh_nonce() -> Result<[u8; 12], SealError> {
    let mut nonce = [0; 12];
    getrandom::fill(&mut nonce).map_err(|_| SealError::EntropyUnavailable)?;
    Ok(nonce)
}

#[cfg(target_arch = "wasm32")]
fn fresh_nonce() -> Result<[u8; 12], SealError> {
    Err(SealError::EntropyUnavailable)
}

/// Host-entropy variant for wasm or callers managing nonce generation. Supply a
/// fresh random 96-bit nonce for every seal under a key. Deterministic only when
/// all inputs are identical; not an invitation to deliberately reuse nonces.
pub fn seal_dataset_with_nonce(
    plaintext: &[u8],
    key: &[u8],
    canary: &str,
    nonce: [u8; 12],
) -> Result<SealedDataset, SealError> {
    let cipher = Aes256GcmSiv::new_from_slice(key).map_err(|_| SealError::InvalidKeyLength)?;
    let commitment = commit_dataset(plaintext, canary);
    let aad = associated_data(&commitment)?;
    let ciphertext = cipher
        .encrypt(
            &Nonce::from(nonce),
            Payload {
                msg: plaintext,
                aad: &aad,
            },
        )
        .map_err(|_| SealError::InputTooLarge)?;
    Ok(SealedDataset {
        scheme: SEALED_DATASET_SCHEME.into(),
        nonce: to_hex(&nonce),
        ciphertext: to_hex(&ciphertext),
        commitment,
    })
}

/// Open a sealed dataset with `key`, returning the recovered plaintext only if it
/// authenticates and verifies against the embedded commitment. `None` on an
/// unknown/legacy format, malformed input, wrong key or authentication failure.
/// The caller must separately compare the commitment with its trusted earlier copy.
pub fn open_dataset(sealed: &SealedDataset, key: &[u8]) -> Option<Vec<u8>> {
    if sealed.scheme != SEALED_DATASET_SCHEME {
        return None;
    }
    let nonce: [u8; 12] = crate::from_hex(&sealed.nonce)?.try_into().ok()?;
    let bytes = crate::from_hex(&sealed.ciphertext)?;
    let cipher = Aes256GcmSiv::new_from_slice(key).ok()?;
    let aad = associated_data(&sealed.commitment).ok()?;
    let plaintext = cipher
        .decrypt(
            &Nonce::from(nonce),
            Payload {
                msg: &bytes,
                aad: &aad,
            },
        )
        .ok()?;
    if verify_dataset(&plaintext, &sealed.commitment) {
        Some(plaintext)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const KEY: &[u8] = &[0x42; 32];

    // Multiple cipher blocks and an empty-input companion exercise both payload
    // encryption and authentication-only sealing.
    fn bars() -> Vec<u8> {
        (0..200u32).flat_map(|i| i.to_le_bytes()).collect()
    }

    #[test]
    fn opening_one_dataset_does_not_reveal_another_sealed_under_the_same_key() {
        let known = vec![b'A'; 96];
        let held_out = vec![b'B'; 96];
        // Even accidental nonce reuse must not recreate V1's two-time pad.
        let first = seal_dataset_with_nonce(&known, KEY, "first", [1; 12]).unwrap();
        let second = seal_dataset_with_nonce(&held_out, KEY, "second", [1; 12]).unwrap();
        let c1 = crate::from_hex(&first.ciphertext).unwrap();
        let c2 = crate::from_hex(&second.ciphertext).unwrap();
        let recovered: Vec<u8> = known
            .iter()
            .zip(c1)
            .zip(c2)
            .map(|((&p1, c1), c2)| p1 ^ c1 ^ c2)
            .collect();
        assert_ne!(
            recovered, held_out,
            "reused keystream exposed the held-out bytes"
        );
    }

    #[test]
    fn the_seal_authenticates_its_canary_metadata() {
        let mut sealed = seal_dataset_with_nonce(&bars(), KEY, "original-canary", [1; 12]).unwrap();
        sealed.commitment.canary = "replacement-canary".into();
        assert!(open_dataset(&sealed, KEY).is_none());
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
        let sealed = seal_dataset_with_nonce(&data, KEY, "canary-guid-002", [2; 12]).unwrap();
        assert_eq!(sealed.ciphertext.len(), 2 * (data.len() + 16));
        let opened = open_dataset(&sealed, KEY).expect("opens with the right key");
        assert_eq!(opened, data);
        // The embedded commitment carries the canary forward.
        assert_eq!(sealed.commitment.canary, "canary-guid-002");
    }

    #[test]
    fn wrong_key_fails_to_open() {
        let data = bars();
        let sealed = seal_dataset_with_nonce(&data, KEY, "c", [3; 12]).unwrap();
        assert!(
            open_dataset(&sealed, &[0x43; 32]).is_none(),
            "wrong key must not yield a verifying plaintext"
        );
    }

    #[test]
    fn malformed_ciphertext_returns_none() {
        let data = bars();
        let mut sealed = seal_dataset_with_nonce(&data, KEY, "c", [4; 12]).unwrap();
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
        let sealed = seal_dataset_with_nonce(&[], KEY, "c", [5; 12]).unwrap();
        assert_eq!(sealed.ciphertext.len(), 32); // authentication tag, even when empty
        assert_eq!(open_dataset(&sealed, KEY), Some(Vec::new()));
    }

    #[test]
    #[cfg(not(target_arch = "wasm32"))]
    fn os_nonce_seals_repeat_plaintext_differently_and_both_open() {
        let a = seal_dataset(&bars(), KEY, "c").unwrap();
        let b = seal_dataset(&bars(), KEY, "c").unwrap();
        assert_eq!(a.scheme, SEALED_DATASET_SCHEME);
        assert_eq!(a.nonce.len(), 24);
        assert_ne!(a.nonce, b.nonce);
        assert_ne!(a.ciphertext, b.ciphertext);
        assert_eq!(open_dataset(&a, KEY), Some(bars()));
        assert_eq!(open_dataset(&b, KEY), Some(bars()));
    }

    #[test]
    fn every_authenticated_field_and_tag_refuses_tampering() {
        let original = seal_dataset_with_nonce(&bars(), KEY, "c", [5; 12]).unwrap();
        for field in [
            "scheme",
            "nonce",
            "ciphertext",
            "tag",
            "hash",
            "canary",
            "len",
        ] {
            let mut bad = original.clone();
            match field {
                "scheme" => bad.scheme.push('!'),
                "nonce" => bad.nonce = to_hex(&[6; 12]),
                "ciphertext" => {
                    let mut bytes = crate::from_hex(&bad.ciphertext).unwrap();
                    bytes[0] ^= 1;
                    bad.ciphertext = to_hex(&bytes);
                }
                "tag" => {
                    let mut bytes = crate::from_hex(&bad.ciphertext).unwrap();
                    *bytes.last_mut().unwrap() ^= 1;
                    bad.ciphertext = to_hex(&bytes);
                }
                "hash" => bad.commitment.content_hash = "0".repeat(64),
                "canary" => bad.commitment.canary.push('!'),
                "len" => bad.commitment.len += 1,
                _ => unreachable!(),
            }
            assert!(
                open_dataset(&bad, KEY).is_none(),
                "accepted altered {field}"
            );
        }
        assert_eq!(open_dataset(&original, KEY), Some(bars()));
    }

    #[test]
    fn invalid_lengths_and_legacy_formats_are_explicit_refusals() {
        for key in [&[][..], &[0; 31][..], &[0; 33][..]] {
            assert_eq!(
                seal_dataset_with_nonce(b"x", key, "c", [1; 12]),
                Err(SealError::InvalidKeyLength)
            );
            assert_eq!(
                seal_dataset(b"x", key, "c"),
                Err(SealError::InvalidKeyLength)
            );
        }
        let good = seal_dataset_with_nonce(&bars(), KEY, "c", [5; 12]).unwrap();
        for nonce in ["", "00", "€€", "00000000000000000000000000"] {
            let mut bad = good.clone();
            bad.nonce = nonce.into();
            assert!(open_dataset(&bad, KEY).is_none());
        }
        let mut wire = serde_json::to_value(&good).unwrap();
        wire.as_object_mut().unwrap().remove("scheme");
        wire.as_object_mut().unwrap().remove("nonce");
        let legacy: SealedDataset = serde_json::from_value(wire).unwrap();
        assert!(legacy.scheme.is_empty());
        assert!(open_dataset(&legacy, KEY).is_none());
        let restored: SealedDataset =
            serde_json::from_str(&serde_json::to_string(&good).unwrap()).unwrap();
        assert_eq!(open_dataset(&restored, KEY), Some(bars()));
    }

    #[test]
    fn associated_metadata_is_length_framed() {
        let a = DatasetCommitment {
            content_hash: "a|b".into(),
            canary: "c".into(),
            len: 3,
        };
        let b = DatasetCommitment {
            content_hash: "a".into(),
            canary: "b|c".into(),
            len: 3,
        };
        assert_ne!(associated_data(&a).unwrap(), associated_data(&b).unwrap());
        let mut expected = SEALED_DATASET_SCHEME.as_bytes().to_vec();
        expected.extend_from_slice(&3u64.to_le_bytes());
        expected.extend_from_slice(b"a|b");
        expected.extend_from_slice(&1u64.to_le_bytes());
        expected.extend_from_slice(b"c");
        expected.extend_from_slice(&3u64.to_le_bytes());
        assert_eq!(associated_data(&a).unwrap(), expected);
    }

    #[test]
    fn selected_aead_matches_rfc8452_c2_known_answers() {
        // Primitive conformance, not a substitute for the wrapper tampering tests.
        // https://www.rfc-editor.org/rfc/rfc8452.html#appendix-C.2
        let mut key = [0; 32];
        key[0] = 1;
        let mut nonce = [0; 12];
        nonce[0] = 3;
        let cipher = Aes256GcmSiv::new_from_slice(&key).unwrap();
        for (plain, expected) in [
            (&[][..], "07f5f4169bbf55a8400cd47ea6fd400f"),
            (
                &[1, 0, 0, 0, 0, 0, 0, 0][..],
                "c2ef328e5c71c83b843122130f7364b761e0b97427e3df28",
            ),
        ] {
            let bytes = cipher.encrypt(&Nonce::from(nonce), plain).unwrap();
            assert_eq!(to_hex(&bytes), expected);
            assert_eq!(
                cipher
                    .decrypt(&Nonce::from(nonce), bytes.as_slice())
                    .unwrap(),
                plain
            );
        }
    }
}
