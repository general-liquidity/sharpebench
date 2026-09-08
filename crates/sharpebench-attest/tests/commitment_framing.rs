//! A commitment must bind the four fields it was made from, not their pasted
//! concatenation. Under separator framing an entrant could shift a separator
//! from one field into the next, publish one hash, and reveal a different
//! identity or a different frozen artifact against it.

use sharpebench_attest::{framed_preimage, make_commitment, verify_commitment, COMMITMENT_DOMAIN};

#[test]
fn a_separator_cannot_move_between_the_identity_fields() {
    let shifted = make_commitment("alpha|2026-W01", "window", "digest", "salt");
    let original = make_commitment("alpha", "2026-W01|window", "digest", "salt");
    assert_ne!(
        shifted.commit_hash, original.commit_hash,
        "two different (agent, window) pairs must not commit to one hash"
    );
}

#[test]
fn a_separator_cannot_move_between_the_artifact_and_the_salt() {
    let shifted = make_commitment("alpha", "w1", "digest|salt", "extra");
    let original = make_commitment("alpha", "w1", "digest", "salt|extra");
    assert_ne!(
        shifted.commit_hash, original.commit_hash,
        "a frozen artifact must not be exchangeable for part of the salt"
    );
}

#[test]
fn a_shifted_reveal_does_not_verify_against_the_published_commitment() {
    // The artifact digest and the salt are the fields the reveal supplies and
    // the published commitment does not carry in the clear, so a shift between
    // those two is the exploitable one: nothing but the hash separates them.
    let published = make_commitment("alpha", "2026-W01", "digest|salt", "extra");
    assert!(verify_commitment(
        &published,
        "alpha",
        "2026-W01",
        "digest|salt",
        "extra"
    ));
    assert!(
        !verify_commitment(&published, "alpha", "2026-W01", "digest", "salt|extra"),
        "a reveal must not be able to swap the frozen artifact for part of the salt"
    );
}

#[test]
fn an_empty_field_is_still_a_field() {
    assert_ne!(
        make_commitment("alpha", "", "digest", "salt").commit_hash,
        make_commitment("alpha", "digest", "", "salt").commit_hash
    );
}

#[test]
fn the_commitment_domain_separates_the_pre_image() {
    let fields = ["alpha", "w1", "digest", "salt"];
    assert_ne!(
        framed_preimage(COMMITMENT_DOMAIN, &fields),
        framed_preimage("sharpebench-attest/chain-receipt/v1", &fields),
        "a commitment pre-image must not be readable as another purpose's"
    );
    assert!(framed_preimage(COMMITMENT_DOMAIN, &fields).starts_with(COMMITMENT_DOMAIN.as_bytes()));
}
