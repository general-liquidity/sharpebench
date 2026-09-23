//! The run tier is a field with consequences, and a frozen protocol is
//! immutable at its version.

mod common;

use common::valid_protocol;
use sharpebench_study::protocol::*;
use sharpebench_study::refusal::ProtocolRefusal;
use sharpebench_study::{check_amendment, validate};

fn frozen_protocol() -> StudyProtocol {
    let mut protocol = valid_protocol();
    protocol.tier = RunTier::FrozenValidation;
    protocol.frozen = true;
    protocol.design.tuning_allowed = false;
    protocol
}

fn ci_protocol() -> StudyProtocol {
    let mut protocol = valid_protocol();
    protocol.tier = RunTier::CiRegression;
    protocol.claims.clear();
    protocol.estimands.clear();
    protocol
}

#[test]
fn frozen_validation_protocol_validates() {
    assert_eq!(validate(&frozen_protocol()), Ok(()));
}

#[test]
fn frozen_validation_tier_refuses_tuning_against_its_own_results() {
    let mut protocol = frozen_protocol();
    protocol.design.tuning_allowed = true;

    assert_eq!(
        validate(&protocol),
        Err(ProtocolRefusal::TuningOnFrozenTier)
    );
}

#[test]
fn frozen_validation_tier_must_be_marked_frozen() {
    let mut protocol = frozen_protocol();
    protocol.frozen = false;

    assert_eq!(
        validate(&protocol),
        Err(ProtocolRefusal::FrozenFlagTierMismatch {
            tier: RunTier::FrozenValidation,
            frozen: false,
        })
    );
}

#[test]
fn development_tier_marked_frozen_is_refused() {
    let mut protocol = valid_protocol();
    protocol.frozen = true;

    assert_eq!(
        validate(&protocol),
        Err(ProtocolRefusal::FrozenFlagTierMismatch {
            tier: RunTier::DevelopmentCalibration,
            frozen: true,
        })
    );
}

/// A fixture suite with pinned outputs is a behaviour check. It carries no
/// claim, and declaring one on it is refused rather than reported with a
/// caveat.
#[test]
fn ci_regression_tier_without_claims_validates() {
    assert_eq!(validate(&ci_protocol()), Ok(()));
}

#[test]
fn ci_regression_tier_carrying_an_error_rate_claim_is_refused() {
    let source = valid_protocol();
    let mut protocol = ci_protocol();
    protocol.estimands = source.estimands.clone();
    protocol.claims = vec![source.claims[0].clone()];

    assert_eq!(
        validate(&protocol),
        Err(ProtocolRefusal::TierClaimBoundary {
            tier: RunTier::CiRegression,
            claim: "C1".to_string(),
            estimand_tag: "per_entry_false_positive",
        })
    );
}

#[test]
fn ci_regression_tier_carrying_a_power_claim_is_refused() {
    let source = valid_protocol();
    let mut protocol = ci_protocol();
    protocol.estimands = source.estimands.clone();
    protocol.claims = vec![source.claims[1].clone()];

    assert_eq!(
        validate(&protocol),
        Err(ProtocolRefusal::TierClaimBoundary {
            tier: RunTier::CiRegression,
            claim: "C2".to_string(),
            estimand_tag: "power_at_effect",
        })
    );
}

#[test]
fn editing_a_frozen_protocol_without_raising_its_version_is_refused() {
    let previous = frozen_protocol();
    let mut next = previous.clone();
    next.inference.required_half_width = 0.05;

    assert_eq!(
        check_amendment(&previous, &next),
        Err(ProtocolRefusal::FrozenProtocolModified {
            protocol_id: previous.protocol_id.clone(),
            version: previous.version,
        })
    );
}

/// The comparison is over the whole document, not a named list of fields, so a
/// change buried in a nested object is caught too.
#[test]
fn a_nested_edit_to_a_frozen_protocol_is_caught() {
    let previous = frozen_protocol();
    let mut next = previous.clone();
    next.design.field_composition.null_entrants += 1;

    assert!(matches!(
        check_amendment(&previous, &next),
        Err(ProtocolRefusal::FrozenProtocolModified { .. })
    ));
}

#[test]
fn editing_a_frozen_protocol_with_a_raised_version_is_accepted() {
    let previous = frozen_protocol();
    let mut next = previous.clone();
    next.inference.required_half_width = 0.05;
    next.version = ProtocolVersion {
        major: 1,
        minor: 1,
        patch: 0,
    };

    assert_eq!(check_amendment(&previous, &next), Ok(()));
}

#[test]
fn lowering_a_frozen_protocols_version_is_refused() {
    let previous = frozen_protocol();
    let mut next = previous.clone();
    next.version = ProtocolVersion {
        major: 0,
        minor: 9,
        patch: 0,
    };

    assert_eq!(
        check_amendment(&previous, &next),
        Err(ProtocolRefusal::VersionNotRaised {
            previous: previous.version,
            next: next.version,
        })
    );
}

#[test]
fn amending_under_a_different_protocol_id_is_refused() {
    let previous = frozen_protocol();
    let mut next = previous.clone();
    next.protocol_id = "some-other-protocol".to_string();

    assert_eq!(
        check_amendment(&previous, &next),
        Err(ProtocolRefusal::ProtocolIdentityChanged {
            previous: previous.protocol_id.clone(),
            next: next.protocol_id.clone(),
        })
    );
}

/// Only the frozen tier is immutable. A development protocol is edited in
/// place, which is what that tier is for.
#[test]
fn editing_an_unfrozen_protocol_needs_no_version_bump() {
    let previous = valid_protocol();
    let mut next = previous.clone();
    next.inference.required_half_width = 0.05;

    assert_eq!(check_amendment(&previous, &next), Ok(()));
}
