//! Rank-neutral disclosure of how much state an external entrant can carry
//! between the cells of one sweep.
//!
//! Execution seeds of one window replay the same bars. An `--image` entrant
//! gets a fresh container per cell, removed afterwards. A `--cmd` entrant gets
//! a fresh host process per cell, but runs unsandboxed and can leave files for
//! the next one. An `--http` endpoint is run by the operator and outlives every
//! cell. The board row says which of these applied.
//!
//! The class is read from the runner tag that `external_entrant_label` puts in
//! front of the entrant id (`sandbox:`, `cmd:`, `http:`). Only the transport
//! flag decides that tag, so nothing an entrant sends can change the class. The
//! id is what a checkpoint is bound to, and the tag is also part of the
//! invocation material behind `invocation_sha256`, so a checkpoint written under
//! one class cannot be resumed under another. No digest changes to carry it.
//!
//! This discloses; it does not detect. An entrant under a runner that allows
//! carryover may carry nothing, and nothing here looks for it.

use sharpebench_arena::SandboxedAgent;
use sharpebench_sim::external::CellIsolation;
use sharpebench_sim::{ExternalAgent, HttpAgent};

pub const CELL_ISOLATION_VERSION: &str = "sharpebench.cell-isolation.v1";

/// The isolation class of the runner behind `agent_id`, or `None` for an id
/// no external transport produced.
pub fn of_entrant(agent_id: &str) -> Option<CellIsolation> {
    match agent_id.split_once(':')?.0 {
        "sandbox" => Some(SandboxedAgent::CELL_ISOLATION),
        "cmd" => Some(ExternalAgent::CELL_ISOLATION),
        "http" => Some(HttpAgent::CELL_ISOLATION),
        _ => None,
    }
}

/// The object published on the entrant's row, beside `attempt_accounting`.
pub fn disclosure(isolation: CellIsolation) -> serde_json::Value {
    serde_json::json!({
        "schema_version": CELL_ISOLATION_VERSION,
        "class": isolation,
        "runner_discards_state_between_cells": isolation.runner_discards_state(),
        "detects_state_carryover": false,
        "rank_neutral": true,
    })
}

/// The human-output line, printed beside the attempt accounting.
pub fn print(agent_id: &str) {
    if let Some(isolation) = of_entrant(agent_id) {
        let class = serde_json::to_value(isolation).expect("a unit variant serializes");
        eprintln!(
            "cell isolation for {agent_id}: {} (disclosed from the runner, not a check on the entrant)",
            class.as_str().unwrap_or_default()
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(flags: &[&str]) -> Vec<String> {
        std::iter::once("sharpebench")
            .chain(std::iter::once("run"))
            .chain(flags.iter().copied())
            .map(String::from)
            .collect()
    }

    fn class_for(flags: &[&str]) -> serde_json::Value {
        let label = crate::external_entrant_label(&args(flags)).expect("a transport flag");
        disclosure(of_entrant(&label).expect("an external runner tag"))["class"].clone()
    }

    /// One class per runner kind, taken from the label the run branch ranks the
    /// entrant under.
    #[test]
    fn each_runner_kind_discloses_its_own_class() {
        let pinned = format!("registry.invalid/agent@sha256:{}", "a".repeat(64));
        assert_eq!(class_for(&["--image", &pinned]), "container_per_cell");
        assert_eq!(
            class_for(&["--cmd", "python agent.py --fast"]),
            "process_per_cell_host_writable"
        );
        assert_eq!(
            class_for(&["--cmd", r"C:\tools\agent.exe"]),
            "process_per_cell_host_writable"
        );
        assert_eq!(
            class_for(&["--http", "127.0.0.1:8080"]),
            "operator_endpoint"
        );
        // The CLI's own precedence: with several flags, `--http` wins.
        assert_eq!(
            class_for(&["--cmd", "agent", "--http", "127.0.0.1:1"]),
            "operator_endpoint"
        );
    }

    #[test]
    fn only_the_container_runner_discards_state() {
        let container = disclosure(CellIsolation::ContainerPerCell);
        assert_eq!(container["runner_discards_state_between_cells"], true);
        for leaky in [
            CellIsolation::ProcessPerCellHostWritable,
            CellIsolation::OperatorEndpoint,
        ] {
            let value = disclosure(leaky);
            assert_eq!(value["runner_discards_state_between_cells"], false);
            assert_eq!(value["detects_state_carryover"], false);
            assert_eq!(value["rank_neutral"], true);
            assert_eq!(value["schema_version"], CELL_ISOLATION_VERSION);
        }
    }

    /// An id no external transport produced carries no class, so a reference
    /// row, or an entrant id with no runner tag, is never labelled.
    #[test]
    fn an_untagged_id_has_no_class() {
        for id in ["buy-and-hold", "external", "gateway:scripted", "", ":x"] {
            assert_eq!(of_entrant(id), None, "{id}");
        }
    }

    /// The row carries the disclosure for the entrant only, and adding it moves
    /// no score, no order and no reference row.
    #[test]
    fn the_board_row_carries_the_class_without_changing_the_board() {
        let res =
            sharpebench_harness::run_agent_resilient("http:127.0.0.1:9", 1, &[7], 2, 40, |_, _| {
                Ok(sharpebench_harness::failing_sentinel_run(40))
            });
        let reference = sharpebench_core::AgentSubmission {
            agent_id: "reference".into(),
            ..res.submission.clone()
        };
        let board = sharpebench_core::rank(
            &[res.submission, reference],
            &sharpebench_core::ScoreConfig::default(),
        );
        let metadata = |cell_isolation| crate::ExternalRowMetadata {
            agent: "http:127.0.0.1:9",
            attempts: res.attempts,
            monetary_cost: &res.monetary_cost,
            artifact_preflight: None,
            fault_injection: None,
            cell_isolation,
        };
        let without = crate::run_board_json(&board, Some(metadata(None)));
        let mut with =
            crate::run_board_json(&board, Some(metadata(of_entrant("http:127.0.0.1:9"))));
        let rows = with.as_array_mut().expect("a board is an array");
        let entrant = rows
            .iter_mut()
            .find(|row| row["agent_id"] == "http:127.0.0.1:9")
            .expect("the entrant row");
        let disclosed = entrant
            .as_object_mut()
            .expect("a row")
            .remove("cell_isolation")
            .expect("the entrant row carries the class");
        assert_eq!(disclosed["class"], "operator_endpoint");
        assert_eq!(with, without);
        assert!(without
            .as_array()
            .expect("an array")
            .iter()
            .all(|row| row.get("cell_isolation").is_none()));
    }
}
