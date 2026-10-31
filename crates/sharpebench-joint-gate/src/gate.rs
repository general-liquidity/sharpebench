//! Whether a run may call itself frozen.
//!
//! A sealed protocol names a method, a configuration digest and an engine
//! fingerprint, and it is not executable until its budget is approved. Each of
//! those is checked here against what is actually linked and loaded, and each
//! failure names the side that moved.

use sha2::{Digest, Sha256};
use sharpebench_study::{validate, StudyProtocol};

use crate::campaign::{hex, Campaign};
use crate::shards::{RunError, RunIdentity, Tier};

/// Lowercase hex SHA-256 of a protocol as parsed and serialized, the value
/// `seal.json` records. Whitespace cannot move it; any value change does.
pub fn protocol_digest(protocol: &StudyProtocol) -> String {
    let bytes = serde_json::to_vec(protocol).expect("a study protocol serializes");
    hex(&Sha256::digest(&bytes))
}

/// Check `protocols` against this run. Every protocol must validate, which
/// includes an approved budget; bind this campaign by digest; and name the
/// engine that is linked.
pub fn authorize_frozen(
    protocols: &[StudyProtocol],
    campaign: &Campaign,
    engine_fingerprint: &str,
) -> Result<(), RunError> {
    if protocols.is_empty() {
        return Err(RunError::Refused(
            "a frozen run needs its sealed protocols".to_string(),
        ));
    }
    let digest = campaign.digest();
    for protocol in protocols {
        let id = &protocol.protocol_id;
        validate(protocol).map_err(|refusal| {
            RunError::Refused(format!("protocol {id} does not validate: {refusal}"))
        })?;
        if protocol.identity.configuration_digest != digest {
            return Err(RunError::Refused(format!(
                "campaign moved: protocol {id} binds {}, the loaded campaign digests to {digest}",
                protocol.identity.configuration_digest
            )));
        }
        if protocol.identity.scoring_spec_hash != engine_fingerprint {
            return Err(RunError::Refused(format!(
                "engine moved: protocol {id} binds fingerprint {}, the linked engine is \
                 {engine_fingerprint}. Amend the protocol at a raised version before running",
                protocol.identity.scoring_spec_hash
            )));
        }
    }
    Ok(())
}

/// The identity of a run, or the refusal that stops it. A frozen run may not
/// cap its replicates; a smoke run must, so that it cannot pass for the sealed
/// count on the wrong seeds.
pub fn run_identity(
    tier: Tier,
    campaign: &Campaign,
    engine_fingerprint: String,
    replicate_cap: Option<u64>,
) -> Result<RunIdentity, RunError> {
    match (tier, replicate_cap) {
        (Tier::Frozen, Some(_)) => Err(RunError::Refused(
            "a frozen run may not reduce its replicates".to_string(),
        )),
        (Tier::Smoke, None) => Err(RunError::Refused(
            "a smoke run must state --replicates".to_string(),
        )),
        _ => Ok(RunIdentity {
            tier,
            campaign_digest: campaign.digest(),
            engine_fingerprint,
            replicate_cap,
        }),
    }
}
