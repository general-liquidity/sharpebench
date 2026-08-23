//! Ed25519 public verifiability for the result chain.
//!
//! The HMAC chain in the crate root ([`crate::sign_result`] / [`crate::verify_chain`])
//! is *symmetric*: whoever can verify it holds the key that signs it, so it is
//! tamper-evident only to key-holders, and any key-holder can forge. That is the
//! right tool for a host checking its own audit log. It is the wrong tool for the
//! claim SharpeBench actually makes, "verify the board, don't trust the host",
//! because a third party cannot check an HMAC chain without being handed the
//! power to rewrite it.
//!
//! This module adds an *asymmetric* chain with the same shape: every link is an
//! Ed25519 signature over `prev_signature | payload`, genesis-anchored, so an
//! altered payload, a dropped row, or a reordered pair breaks the chain exactly
//! as it does under HMAC. The difference is who can check it. The host publishes
//! its [`VerifyingKey`] inside the board ([`PublicChain`]); anyone with the
//! document can run [`verify_public_chain`] with no secret at all, and nobody
//! without the [`SigningKey`] can produce a chain that passes.
//!
//! What each scheme guarantees, precisely:
//!
//! - **HMAC-SHA256** (crate root): integrity for key-holders. The set of parties
//!   who can verify equals the set who can forge.
//! - **Ed25519** (this module): public verifiability. Anyone holding the published
//!   verifying key can check the chain; only the holder of the signing key can
//!   forge it.
//!
//! Ed25519 is chosen over other signature schemes because RFC 8032 signing is
//! **deterministic**: the nonce is derived from the key and the message, never
//! from a random source, so the same key over the same chain yields the same
//! signature bytes on every run. The SharpeBench kernel is deterministic end to
//! end and the attestation layer must not be the place that stops being so.
//!
//! Self-verification proves *consistency* (the document was signed by the key it
//! carries), not *identity*. A host could re-sign a doctored board under a fresh
//! key and embed that. Pin the host's advertised key out of band via
//! [`verify_public_chain_with`] when identity matters; the embedded key exists so
//! a document can be checked by someone who has nothing else.

use ed25519_dalek::{Signature, Signer};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::{from_hex, to_hex, SignedResult, GENESIS};

/// Scheme tag written into a [`PublicChain`] so a reader can tell at a glance
/// which verifier applies.
pub const ED25519_SCHEME: &str = "ed25519";

/// Domain-separation prefix for [`SigningKey::derive`]. Keeps a seed derived
/// here distinct from any other SHA-256 use of the same secret bytes (the HMAC
/// chain key, the dataset seal key).
const DERIVE_DOMAIN: &[u8] = b"sharpebench-attest/ed25519-seed/v1";

/// An Ed25519 signing (private) key. Never serialized by this crate; export the
/// seed explicitly with [`SigningKey::seed_hex`] if it must be stored.
#[derive(Clone)]
pub struct SigningKey(ed25519_dalek::SigningKey);

/// An Ed25519 verifying (public) key. Safe to publish; hex-encoded on the wire.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VerifyingKey(ed25519_dalek::VerifyingKey);

impl std::fmt::Debug for SigningKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Never print the seed, even in a debug dump.
        f.debug_tuple("SigningKey")
            .field(&self.verifying_key().to_hex())
            .finish()
    }
}

impl SigningKey {
    /// Build a key from a raw 32-byte RFC 8032 seed. Deterministic.
    pub fn from_seed(seed: &[u8; 32]) -> Self {
        Self(ed25519_dalek::SigningKey::from_bytes(seed))
    }

    /// Derive a key from arbitrary secret bytes (a passphrase, the contents of a
    /// key file, an env var). The seed is the domain-separated SHA-256 of the
    /// secret, so the same secret always yields the same keypair. This is the
    /// path the CLI's `env:NAME` / `file:PATH` convention flows through, and the
    /// path tests use for fixed keys.
    pub fn derive(secret: &[u8]) -> Self {
        let mut h = Sha256::new();
        h.update(DERIVE_DOMAIN);
        h.update(secret);
        Self::from_seed(&h.finalize().into())
    }

    /// Generate a fresh key from OS entropy. Not available on wasm32, where the
    /// kernel only verifies or signs with seed-derived keys.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn generate() -> Self {
        let mut seed = [0u8; 32];
        getrandom::fill(&mut seed).expect("OS entropy source is available");
        Self::from_seed(&seed)
    }

    /// Parse a 64-hex-char seed as produced by [`SigningKey::seed_hex`].
    pub fn from_seed_hex(hex: &str) -> Option<Self> {
        let seed: [u8; 32] = from_hex(hex)?.try_into().ok()?;
        Some(Self::from_seed(&seed))
    }

    /// The raw seed as hex, for storing a generated key. Treat as a secret.
    pub fn seed_hex(&self) -> String {
        to_hex(&self.0.to_bytes())
    }

    pub fn verifying_key(&self) -> VerifyingKey {
        VerifyingKey(self.0.verifying_key())
    }
}

impl VerifyingKey {
    /// Parse a 64-hex-char public key. `None` if malformed or not a valid
    /// Ed25519 point.
    pub fn from_hex(hex: &str) -> Option<Self> {
        let arr: [u8; 32] = from_hex(hex)?.try_into().ok()?;
        ed25519_dalek::VerifyingKey::from_bytes(&arr).ok().map(Self)
    }

    pub fn to_hex(&self) -> String {
        to_hex(self.0.as_bytes())
    }

    fn verify(&self, msg: &[u8], sig_hex: &str) -> bool {
        let Some(bytes) = from_hex(sig_hex) else {
            return false;
        };
        let Ok(sig) = Signature::from_slice(&bytes) else {
            return false;
        };
        // `verify_strict` rejects the small-order / non-canonical points that
        // the permissive RFC check lets through; a benchmark wants the narrow
        // definition of "valid".
        self.0.verify_strict(msg, &sig).is_ok()
    }
}

/// The bytes both signer and verifier agree on: `prev_signature | payload`,
/// the same framing as the HMAC chain so the two chains are link-for-link
/// comparable.
fn link_message(prev_signature: &str, payload: &str) -> Vec<u8> {
    let mut m = Vec::with_capacity(prev_signature.len() + 1 + payload.len());
    m.extend_from_slice(prev_signature.as_bytes());
    m.push(b'|');
    m.extend_from_slice(payload.as_bytes());
    m
}

/// Append a result to a public chain, signing it against `prev_signature`. The
/// signature is 64 bytes, hex-encoded (128 chars), where the HMAC chain's is 32.
pub fn sign_result_public(payload: &str, prev_signature: &str, key: &SigningKey) -> SignedResult {
    let sig = key.0.sign(&link_message(prev_signature, payload));
    SignedResult {
        payload: payload.to_string(),
        prev_signature: prev_signature.to_string(),
        signature: to_hex(&sig.to_bytes()),
    }
}

/// Verify a public chain with **only** the verifying key: every link's
/// `prev_signature` must equal the previous link's signature (genesis-anchored)
/// and every signature must verify under `key`. No secret is involved, so this
/// is safe to run anywhere the document is.
pub fn verify_chain_public(results: &[SignedResult], key: &VerifyingKey) -> bool {
    let mut prev = GENESIS.to_string();
    for r in results {
        if r.prev_signature != prev {
            return false;
        }
        if !key.verify(&link_message(&r.prev_signature, &r.payload), &r.signature) {
            return false;
        }
        prev = r.signature.clone();
    }
    true
}

/// A publicly verifiable chain together with the key that verifies it. Embedding
/// the key makes the document self-contained: a reader needs this JSON and
/// nothing else to run [`verify_public_chain`].
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PublicChain {
    /// Always [`ED25519_SCHEME`]; present so a future scheme can coexist.
    pub scheme: String,
    /// Hex Ed25519 verifying key (32 bytes, 64 chars).
    pub verifying_key: String,
    pub chain: Vec<SignedResult>,
}

/// Sign an ordered sequence of canonical payloads into a [`PublicChain`].
pub fn publish_public_chain(payloads: &[String], key: &SigningKey) -> PublicChain {
    let mut chain = Vec::with_capacity(payloads.len());
    let mut prev = GENESIS.to_string();
    for p in payloads {
        let link = sign_result_public(p, &prev, key);
        prev = link.signature.clone();
        chain.push(link);
    }
    PublicChain {
        scheme: ED25519_SCHEME.to_string(),
        verifying_key: key.verifying_key().to_hex(),
        chain,
    }
}

/// Verify a [`PublicChain`] from the document alone, using the key it carries.
/// Proves the chain is internally consistent under that key; see the module
/// docs for why identity still needs an out-of-band pin.
pub fn verify_public_chain(board: &PublicChain) -> bool {
    if board.scheme != ED25519_SCHEME {
        return false;
    }
    match VerifyingKey::from_hex(&board.verifying_key) {
        Some(vk) => verify_chain_public(&board.chain, &vk),
        None => false,
    }
}

/// Verify a [`PublicChain`] against a key the caller trusts independently. The
/// embedded key must match `key` exactly, so a board re-signed under a swapped
/// key is rejected even though it would self-verify.
pub fn verify_public_chain_with(board: &PublicChain, key: &VerifyingKey) -> bool {
    board.verifying_key == key.to_hex() && verify_public_chain(board)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixed_key() -> SigningKey {
        SigningKey::derive(b"sharpebench-test-signing-secret")
    }

    fn three_link_chain(key: &SigningKey) -> Vec<SignedResult> {
        let r1 = sign_result_public("{\"agent\":\"a\",\"dsr\":0.97}", GENESIS, key);
        let r2 = sign_result_public("{\"agent\":\"b\",\"dsr\":0.40}", &r1.signature, key);
        let r3 = sign_result_public("{\"agent\":\"c\",\"dsr\":0.12}", &r2.signature, key);
        vec![r1, r2, r3]
    }

    #[test]
    fn public_chain_verifies_with_public_key_alone() {
        let sk = fixed_key();
        let chain = three_link_chain(&sk);
        // Only the verifying key crosses this line; the signing key is gone.
        let vk = sk.verifying_key();
        drop(sk);
        assert!(verify_chain_public(&chain, &vk));
        // An Ed25519 signature is 64 bytes (128 hex chars); a key is 32 (64).
        assert_eq!(chain[0].signature.len(), 128);
        assert_eq!(vk.to_hex().len(), 64);
    }

    #[test]
    fn altered_payload_breaks_public_chain() {
        let sk = fixed_key();
        let mut chain = three_link_chain(&sk);
        chain[1].payload = "{\"agent\":\"b\",\"dsr\":0.99}".to_string();
        assert!(!verify_chain_public(&chain, &sk.verifying_key()));
    }

    #[test]
    fn reordered_entries_break_public_chain() {
        let sk = fixed_key();
        let mut chain = three_link_chain(&sk);
        chain.swap(1, 2);
        assert!(!verify_chain_public(&chain, &sk.verifying_key()));
    }

    #[test]
    fn dropped_entry_breaks_public_chain() {
        let sk = fixed_key();
        let mut chain = three_link_chain(&sk);
        chain.remove(1);
        assert!(!verify_chain_public(&chain, &sk.verifying_key()));
    }

    #[test]
    fn different_public_key_rejects() {
        let chain = three_link_chain(&fixed_key());
        let other = SigningKey::derive(b"someone-else").verifying_key();
        assert!(!verify_chain_public(&chain, &other));
    }

    #[test]
    fn signing_is_deterministic() {
        let a = sign_result_public("payload", GENESIS, &fixed_key());
        let b = sign_result_public("payload", GENESIS, &fixed_key());
        assert_eq!(a.signature, b.signature);
        // Same secret, same keypair, on every derivation.
        assert_eq!(fixed_key().verifying_key(), fixed_key().verifying_key());
        assert_eq!(fixed_key().seed_hex(), fixed_key().seed_hex());
    }

    #[test]
    fn board_carries_its_own_key_and_verifies_from_document_alone() {
        let sk = fixed_key();
        let payloads: Vec<String> = ["{\"a\":1}", "{\"b\":2}"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        let board = publish_public_chain(&payloads, &sk);
        let json = serde_json::to_string(&board).unwrap();
        drop(sk);
        drop(board);
        // Round-trip through the wire format; nothing but the document remains.
        let parsed: PublicChain = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.scheme, ED25519_SCHEME);
        assert!(verify_public_chain(&parsed));
        // Pinning the right key passes; pinning a different key fails even
        // though the document self-verifies.
        let pinned = fixed_key().verifying_key();
        assert!(verify_public_chain_with(&parsed, &pinned));
        let wrong = SigningKey::derive(b"impostor").verifying_key();
        assert!(!verify_public_chain_with(&parsed, &wrong));
    }

    #[test]
    fn swapped_embedded_key_cannot_self_verify_old_chain() {
        let board = publish_public_chain(&["x".to_string()], &fixed_key());
        let mut tampered = board.clone();
        tampered.verifying_key = SigningKey::derive(b"impostor").verifying_key().to_hex();
        assert!(!verify_public_chain(&tampered));
    }

    #[test]
    fn key_hex_roundtrip_and_malformed_inputs() {
        let sk = fixed_key();
        let vk = sk.verifying_key();
        assert_eq!(VerifyingKey::from_hex(&vk.to_hex()), Some(vk.clone()));
        assert_eq!(
            SigningKey::from_seed_hex(&sk.seed_hex())
                .unwrap()
                .verifying_key(),
            vk
        );
        assert!(VerifyingKey::from_hex("zz").is_none());
        assert!(VerifyingKey::from_hex("ab").is_none());
        assert!(SigningKey::from_seed_hex("ab").is_none());
        // A garbage signature string must fail closed, not panic.
        let mut r = sign_result_public("x", GENESIS, &sk);
        r.signature = "not-hex".to_string();
        assert!(!verify_chain_public(&[r], &vk));
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn generated_keys_are_distinct_and_usable() {
        let a = SigningKey::generate();
        let b = SigningKey::generate();
        assert_ne!(a.verifying_key(), b.verifying_key());
        let chain = three_link_chain(&a);
        assert!(verify_chain_public(&chain, &a.verifying_key()));
        assert!(!verify_chain_public(&chain, &b.verifying_key()));
    }
}
