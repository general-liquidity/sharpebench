//! Declared metadata for each operation of the agent wire contract.
//!
//! Every operation states three things, independent of transport: whether it
//! changes benchmark state (`mutates_state`), whether repeating it has the same
//! effect as doing it once (`idempotency`), and whether a caller may repeat it
//! without being asked (`automatic_retries`). The declaration is published in
//! `schema/decision.schema.json` as the `x-operation`, `x-mutates-state`,
//! `x-idempotency` and `x-automatic-retries` annotations, so an entrant and the
//! harness read the same statement. `tests/schema_drift.rs` fails when the
//! published annotations and [`OPERATIONS`] disagree.
//!
//! There is one derivation and no per-operation override: an operation that
//! does not mutate state is safe to repeat, and only a safe operation may be
//! retried automatically ([`Operation::metadata`]). The archive this was ported
//! from allows audited exceptions (an idempotent write declared safe). Bench
//! considered one for target-weight orders and declined it: a target is
//! absolute, so repeating it cannot double exposure, but under the partial-fill
//! and participation-cap cost models a second application fills more of the
//! remaining gap and pays for it, which is not the same effect as applying it
//! once.
//!
//! Changing this table changes what entrants were told, so the table has a
//! content identity ([`operation_contract_preimage`]) and its digest is pinned
//! by a test in `sharpebench-harness`.

use serde::{Deserialize, Serialize};

use crate::canonical::{versioned_preimage, CanonicalError};

/// Whether repeating an operation has the same effect as performing it once.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Idempotency {
    Safe,
    NotGuaranteed,
}

/// Whether a caller may repeat the operation on its own initiative.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AutomaticRetries {
    Allowed,
    Forbidden,
}

/// The published triple for one operation.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OperationMetadata {
    pub mutates_state: bool,
    pub idempotency: Idempotency,
    pub automatic_retries: AutomaticRetries,
}

/// One operation of the wire contract and where the schema declares it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Operation {
    /// The operation's published name, the value of `x-operation`.
    pub name: &'static str,
    /// JSON pointer, within `decision.schema.json`, of the object that carries
    /// this operation's annotations.
    pub schema_pointer: &'static str,
    /// Whether performing the operation changes benchmark state.
    pub mutates_state: bool,
}

impl Operation {
    /// The single derivation from `mutates_state` to the published triple.
    pub const fn metadata(&self) -> OperationMetadata {
        let idempotency = if self.mutates_state {
            Idempotency::NotGuaranteed
        } else {
            Idempotency::Safe
        };
        OperationMetadata {
            mutates_state: self.mutates_state,
            idempotency,
            automatic_retries: match idempotency {
                Idempotency::Safe => AutomaticRetries::Allowed,
                Idempotency::NotGuaranteed => AutomaticRetries::Forbidden,
            },
        }
    }
}

/// Every operation of the agent wire contract.
///
/// - `decide`: the harness sends one [`crate::MarketObservation`] and the agent
///   answers with one [`crate::Decision`]. Answering changes no benchmark
///   state; the harness applies the returned decision separately, once. It is
///   therefore safe, and the HTTP transport does retry it after a transport
///   fault. An agent must answer a repeated request for the same step (same
///   run, same observation `date`) with the same decision and must not count
///   it as a new step.
/// - `rebalance_to_target`: the engine moves the portfolio toward each
///   [`crate::Order`]'s `target_weight`. This changes state and is not
///   repeatable for free (see the module note), so it is never retried; the
///   engine applies each accepted decision's orders exactly once per step.
pub const OPERATIONS: &[Operation] = &[
    Operation {
        name: "decide",
        schema_pointer: "",
        mutates_state: false,
    },
    Operation {
        name: "rebalance_to_target",
        schema_pointer: "/$defs/Order",
        mutates_state: true,
    },
];

/// The canonical bytes that identify the declared operation table, framed
/// under [`crate::canonical::CANONICAL_JSON_VERSION`]. Hash with a content
/// digest such as `sharpebench_attest::content_digest`.
pub fn operation_contract_preimage() -> Result<Vec<u8>, CanonicalError> {
    let table: serde_json::Map<String, serde_json::Value> = OPERATIONS
        .iter()
        .map(|operation| {
            (
                operation.name.to_string(),
                serde_json::to_value(operation.metadata()).expect("metadata serializes"),
            )
        })
        .collect();
    versioned_preimage(&serde_json::Value::Object(table))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_triple_round_trips_under_its_published_spelling() {
        for operation in OPERATIONS {
            let metadata = operation.metadata();
            let json = serde_json::to_string(&metadata).unwrap();
            assert_eq!(
                serde_json::from_str::<OperationMetadata>(&json).unwrap(),
                metadata
            );
        }
        assert_eq!(
            serde_json::to_string(&OPERATIONS[0].metadata()).unwrap(),
            r#"{"mutates_state":false,"idempotency":"safe","automatic_retries":"allowed"}"#
        );
        assert_eq!(
            serde_json::to_string(&OPERATIONS[1].metadata()).unwrap(),
            r#"{"mutates_state":true,"idempotency":"not_guaranteed","automatic_retries":"forbidden"}"#
        );
        assert!(serde_json::from_str::<OperationMetadata>(
            r#"{"mutates_state":true,"idempotency":"maybe","automatic_retries":"allowed"}"#
        )
        .is_err());
    }

    #[test]
    fn only_a_safe_operation_may_be_retried_automatically() {
        for operation in OPERATIONS {
            let metadata = operation.metadata();
            assert_eq!(
                metadata.automatic_retries == AutomaticRetries::Allowed,
                metadata.idempotency == Idempotency::Safe,
                "{}",
                operation.name
            );
            assert_eq!(
                metadata.idempotency == Idempotency::Safe,
                !metadata.mutates_state,
                "{}",
                operation.name
            );
        }
    }

    #[test]
    fn operation_names_are_unique_and_the_preimage_is_stable() {
        let names: std::collections::BTreeSet<&str> =
            OPERATIONS.iter().map(|operation| operation.name).collect();
        assert_eq!(names.len(), OPERATIONS.len());
        assert_eq!(
            operation_contract_preimage().unwrap(),
            operation_contract_preimage().unwrap()
        );
    }

    #[test]
    fn the_preimage_carries_every_declared_operation() {
        // The digest itself is pinned in the harness; this keeps the protocol
        // crate's own tests able to tell a real preimage from an empty one.
        let preimage = operation_contract_preimage().unwrap();
        let text = String::from_utf8_lossy(&preimage);
        for operation in OPERATIONS {
            assert!(
                text.contains(&format!("\"{}\"", operation.name)),
                "{} missing from {text}",
                operation.name
            );
        }
    }
}
