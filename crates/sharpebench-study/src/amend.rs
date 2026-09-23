//! Amendment rules. A frozen protocol is immutable at its version: any edit
//! to it is a new version or it is refused.

use serde_json::Value;

use crate::protocol::StudyProtocol;
use crate::refusal::ProtocolRefusal;

/// Check an amendment of `previous` into `next`.
///
/// The rules, in order:
///
/// 1. The two must be the same protocol. A changed `protocol_id` is a new
///    protocol, not an amendment.
/// 2. A frozen protocol whose content changed must raise its version.
/// 3. A frozen protocol's version, once raised, may not be lowered back.
///
/// Content is compared as the serialized document with `version` removed, so a
/// change to any other field, at any depth, counts as an edit. An amendment
/// that changes nothing but the version is permitted: re-versioning a
/// byte-identical protocol is not an edit to its content.
pub fn check_amendment(
    previous: &StudyProtocol,
    next: &StudyProtocol,
) -> Result<(), ProtocolRefusal> {
    if previous.protocol_id != next.protocol_id {
        return Err(ProtocolRefusal::ProtocolIdentityChanged {
            previous: previous.protocol_id.clone(),
            next: next.protocol_id.clone(),
        });
    }
    if !previous.frozen {
        return Ok(());
    }
    if next.version < previous.version {
        return Err(ProtocolRefusal::VersionNotRaised {
            previous: previous.version,
            next: next.version,
        });
    }
    if content_without_version(previous) != content_without_version(next)
        && next.version == previous.version
    {
        return Err(ProtocolRefusal::FrozenProtocolModified {
            protocol_id: previous.protocol_id.clone(),
            version: previous.version,
        });
    }
    Ok(())
}

fn content_without_version(protocol: &StudyProtocol) -> Value {
    let mut value = serde_json::to_value(protocol).expect("a study protocol serializes");
    if let Value::Object(map) = &mut value {
        map.remove("version");
    }
    value
}
