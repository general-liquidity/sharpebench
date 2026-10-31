//! The sealed joint-gate protocols against the contract: coherent in every
//! respect but the one the owner has to supply, and amendable only upward.

use sharpebench_study::protocol::{ProtocolVersion, RunTier};
use sharpebench_study::{check_amendment, validate, ProtocolRefusal, StudyProtocol};

const SEALED: [&str; 2] = [
    include_str!("../../../docs/studies/joint-gate/statistical.protocol.json"),
    include_str!("../../../docs/studies/joint-gate/integrity.protocol.json"),
];

fn sealed() -> Vec<StudyProtocol> {
    SEALED
        .iter()
        .map(|text| StudyProtocol::from_json(text).expect("a sealed protocol parses"))
        .collect()
}

fn approved(protocol: &StudyProtocol) -> StudyProtocol {
    let mut next = protocol.clone();
    next.version = ProtocolVersion {
        major: protocol.version.major,
        minor: protocol.version.minor + 1,
        patch: 0,
    };
    next.budget.approved = true;
    next.budget.approver = "named owner".to_string();
    next
}

#[test]
fn sealed_means_frozen_tier_no_tuning_and_not_yet_approved() {
    for protocol in sealed() {
        assert_eq!(protocol.tier, RunTier::FrozenValidation);
        assert!(protocol.frozen);
        assert!(!protocol.design.tuning_allowed);
        assert!(
            matches!(
                validate(&protocol),
                Err(ProtocolRefusal::UnapprovedBudget { .. })
            ),
            "the seal must stop at approval and nowhere else: {:?}",
            validate(&protocol)
        );
    }
}

#[test]
fn approval_is_the_only_thing_missing() {
    for protocol in sealed() {
        validate(&approved(&protocol)).expect("every other check holds");
    }
}

#[test]
fn approval_must_raise_the_version() {
    for protocol in sealed() {
        let next = approved(&protocol);
        check_amendment(&protocol, &next).expect("a raised version is an amendment");
        let mut same = next;
        same.version = protocol.version;
        assert!(matches!(
            check_amendment(&protocol, &same),
            Err(ProtocolRefusal::FrozenProtocolModified { .. })
        ));
    }
}

#[test]
fn cutting_the_planned_runs_below_precision_is_refused() {
    for protocol in sealed() {
        let mut cut = approved(&protocol);
        cut.simulation.planned_runs = protocol.simulation.planned_runs - 1;
        assert!(
            matches!(
                validate(&cut),
                Err(ProtocolRefusal::SimulationCountBelowPrecision { .. })
            ),
            "{:?}",
            validate(&cut)
        );
    }
}
