//! BS6 regression: chain verification must detect deletion of terminal records.
//!
//! A genesis-anchored chain is prefix-closed. Interior deletion breaks a link,
//! terminal deletion does not, so "a dropped row breaks the chain" was true of
//! the middle of a board and false of its end. These cases pin the signed
//! terminal receipt that closes the gap, for both schemes.

use sharpebench_attest::{
    publish_public_chain, sign_chain_receipt, sign_chain_receipt_public, sign_result,
    sign_result_public, verify_chain, verify_chain_anchored, verify_chain_public,
    verify_chain_public_anchored, verify_chain_receipt, verify_chain_receipt_public,
    verify_public_chain, ChainReceipt, PublicChain, SignedResult, SigningKey, GENESIS,
};

const KEY: &[u8] = b"host-audit-key";

fn hmac_chain() -> Vec<SignedResult> {
    let payloads = [
        "{\"agent\":\"a\"}",
        "{\"agent\":\"b\"}",
        "{\"agent\":\"c\"}",
    ];
    let mut prev = GENESIS.to_string();
    let mut chain = Vec::new();
    for p in payloads {
        let link = sign_result(p, &prev, KEY);
        prev = link.signature.clone();
        chain.push(link);
    }
    chain
}

fn signing_key() -> SigningKey {
    SigningKey::derive(b"sharpebench-terminal-anchor-test")
}

fn public_chain() -> PublicChain {
    let payloads: Vec<String> = [
        "{\"agent\":\"a\"}",
        "{\"agent\":\"b\"}",
        "{\"agent\":\"c\"}",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();
    publish_public_chain(&payloads, &signing_key())
}

#[test]
fn hmac_receipt_rejects_terminal_deletion_that_the_chain_accepts() {
    let chain = hmac_chain();
    let receipt = sign_chain_receipt(&chain, KEY);
    assert!(verify_chain_anchored(&chain, &receipt, KEY));

    let mut truncated = chain.clone();
    truncated.pop();
    // The surviving prefix is still a perfectly valid chain: that is the defect.
    assert!(verify_chain(&truncated, KEY));
    // The receipt is what notices. It cannot be restated without the key.
    assert!(!verify_chain_receipt(&truncated, &receipt, KEY));
    assert!(!verify_chain_anchored(&truncated, &receipt, KEY));

    // Deleting every record is the same failure, not a vacuous pass.
    assert!(verify_chain(&[], KEY));
    assert!(!verify_chain_anchored(&[], &receipt, KEY));
}

#[test]
fn hmac_receipt_cannot_be_restated_for_a_shorter_chain_without_the_key() {
    let chain = hmac_chain();
    let mut truncated = chain.clone();
    truncated.pop();

    // Editing the claimed count, or the claimed terminal signature, or both, is
    // caught: the receipt signature no longer recomputes.
    let honest = sign_chain_receipt(&chain, KEY);
    for forged in [
        ChainReceipt {
            records: truncated.len(),
            ..honest.clone()
        },
        ChainReceipt {
            terminal_signature: truncated[1].signature.clone(),
            ..honest.clone()
        },
        ChainReceipt {
            records: truncated.len(),
            terminal_signature: truncated[1].signature.clone(),
            signature: honest.signature.clone(),
        },
    ] {
        assert!(!verify_chain_receipt(&truncated, &forged, KEY));
    }

    // A receipt signed under some other key is not an anchor for this one.
    let other = sign_chain_receipt(&chain, b"not-the-host-key");
    assert!(!verify_chain_receipt(&chain, &other, KEY));
}

#[test]
fn public_receipt_rejects_terminal_deletion_that_the_chain_accepts() {
    let vk = signing_key().verifying_key();
    let board = public_chain();
    let receipt = board
        .receipt
        .clone()
        .expect("published boards carry a receipt");
    assert!(verify_chain_public_anchored(&board.chain, &receipt, &vk));

    let mut truncated = board.chain.clone();
    truncated.pop();
    assert!(verify_chain_public(&truncated, &vk));
    assert!(!verify_chain_receipt_public(&truncated, &receipt, &vk));
    assert!(!verify_chain_public_anchored(&truncated, &receipt, &vk));
}

/// The public receipt's clauses, isolated one at a time. A truncation is
/// refused by all of them at once, so the case above says only that something
/// refused: the whole signature check could be gone and it would still pass,
/// which is how the published anchor could stop being unforgeable without any
/// test noticing. Each receipt here is wrong in one way.
///
/// The count clause is not isolated, and cannot be: the signature commits to
/// the pair, and a chain with the committed terminal signature has the
/// committed length, so no honestly signed receipt can state the right terminal
/// signature and the wrong count. It is defence in depth against a future
/// caller that builds a receipt by hand, and it is recorded as unfalsifiable
/// rather than covered by a case that would really be testing something else.
#[test]
fn a_public_receipt_is_refused_clause_by_clause() {
    let host = signing_key();
    let vk = host.verifying_key();
    let board = public_chain();
    let honest = board
        .receipt
        .clone()
        .expect("published boards carry a receipt");

    // Only the signature can refuse: the receipt is honestly signed for this
    // chain, but under a key the reader does not hold. Count and terminal
    // signature both agree with the chain supplied.
    let other_key = SigningKey::derive(b"not-the-host-key");
    let other = sign_chain_receipt_public(&board.chain, &other_key);
    assert_eq!(other.records, honest.records);
    assert_eq!(other.terminal_signature, honest.terminal_signature);
    assert!(!verify_chain_receipt_public(&board.chain, &other, &vk));
    // The same receipt verifies under its own key, so the fixture is sound and
    // the key is what that case turns on.
    assert!(verify_chain_receipt_public(
        &board.chain,
        &other,
        &other_key.verifying_key()
    ));

    // Only the signature again, this time a receipt whose stated values were
    // edited after signing to match the chain it is presented against. Every
    // other clause agrees; nothing but the signature knows.
    let mut truncated = board.chain.clone();
    truncated.pop();
    let restated = ChainReceipt {
        records: truncated.len(),
        terminal_signature: truncated[1].signature.clone(),
        signature: honest.signature.clone(),
    };
    assert_eq!(restated.records, truncated.len());
    assert_eq!(restated.terminal_signature, truncated[1].signature);
    assert!(!verify_chain_receipt_public(&truncated, &restated, &vk));

    // Only the terminal-signature clause can refuse: an honestly signed receipt
    // for a different chain of the same length, so the count agrees and the
    // signature verifies over exactly what the receipt states.
    let sibling = publish_public_chain(
        &[
            "{\"agent\":\"x\"}",
            "{\"agent\":\"y\"}",
            "{\"agent\":\"z\"}",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect::<Vec<_>>(),
        &host,
    );
    let for_sibling = sibling
        .receipt
        .clone()
        .expect("published boards carry a receipt");
    assert_eq!(for_sibling.records, board.chain.len());
    assert_ne!(for_sibling.terminal_signature, honest.terminal_signature);
    assert!(verify_chain_receipt_public(
        &sibling.chain,
        &for_sibling,
        &vk
    ));
    assert!(!verify_chain_receipt_public(
        &board.chain,
        &for_sibling,
        &vk
    ));
}

#[test]
fn a_published_document_with_its_last_record_removed_no_longer_verifies() {
    let board = public_chain();
    let json = serde_json::to_string(&board).unwrap();

    // Round-trip proves the anchor is part of the wire format, not in-memory state.
    let parsed: PublicChain = serde_json::from_str(&json).unwrap();
    assert!(verify_public_chain(&parsed));

    let mut doctored: PublicChain = serde_json::from_str(&json).unwrap();
    doctored.chain.pop();
    assert!(!verify_public_chain(&doctored));

    // Dropping the anchor as well is refused rather than silently unchecked.
    doctored.receipt = None;
    assert!(!verify_public_chain(&doctored));
}

#[test]
fn an_unanchored_document_is_refused_not_treated_as_complete() {
    let board = public_chain();
    let mut unanchored = board.clone();
    unanchored.receipt = None;
    // The links themselves are genuine, and the low-level check says so.
    assert!(verify_chain_public(
        &unanchored.chain,
        &signing_key().verifying_key()
    ));
    // The document-level check must not report a completeness pass it never ran.
    assert!(!verify_public_chain(&unanchored));

    // A board written before the anchor existed deserializes, and fails closed.
    let legacy = serde_json::json!({
        "scheme": "ed25519",
        "verifying_key": board.verifying_key,
        "chain": board.chain,
    });
    let parsed: PublicChain = serde_json::from_value(legacy).unwrap();
    assert!(parsed.receipt.is_none());
    assert!(!verify_public_chain(&parsed));
}

#[test]
fn an_empty_chain_still_has_an_unforgeable_anchor() {
    let vk = signing_key().verifying_key();
    let empty = sign_chain_receipt_public(&[], &signing_key());
    assert_eq!(empty.records, 0);
    assert_eq!(empty.terminal_signature, GENESIS);
    assert!(verify_chain_public_anchored(&[], &empty, &vk));

    // An empty receipt does not anchor a chain that has records, and a receipt
    // for records does not anchor an emptied chain.
    let one = vec![sign_result_public(
        "{\"agent\":\"a\"}",
        GENESIS,
        &signing_key(),
    )];
    assert!(!verify_chain_public_anchored(&one, &empty, &vk));
    let for_one = sign_chain_receipt_public(&one, &signing_key());
    assert!(!verify_chain_public_anchored(&[], &for_one, &vk));
}

#[test]
fn the_receipt_signature_is_deterministic_and_scheme_shaped() {
    let a = sign_chain_receipt_public(&public_chain().chain, &signing_key());
    let b = sign_chain_receipt_public(&public_chain().chain, &signing_key());
    assert_eq!(a, b);
    // Ed25519 is 64 bytes, HMAC-SHA256 is 32, hex-encoded, same as their links.
    assert_eq!(a.signature.len(), 128);
    assert_eq!(sign_chain_receipt(&hmac_chain(), KEY).signature.len(), 64);
}
