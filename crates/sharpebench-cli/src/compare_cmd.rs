//! `sharpebench compare` — declare, or refuse, a comparison between two sweep
//! arms.
//!
//! [`sharpebench_harness::checkpoint::SweepContract`] binds six identities into
//! every checkpoint the resumable sweep writes, and binding alone never said
//! which of the six an experiment varies on purpose. Two arms that differ in the
//! entrant are a model comparison; two that differ in the dataset are two
//! different experiments wearing one table, and nothing in the checkpoint files
//! distinguishes them for a reader.
//!
//! [`ComparisonReceipt::declare`] answers that, and this command is its
//! producer: it reads the two checkpoints the sweep already wrote, declares the
//! axis the operator says is the treatment, and emits the receipt or the refusal
//! naming the field that decided it. The receipt is a statement about two
//! recorded runs. It reads the checkpoints, writes neither, and touches no
//! score: the gate, eligibility and the rank never see it.

use std::path::Path;

use serde::Serialize;
use sharpebench_harness::checkpoint::{ComparisonReceipt, SweepCheckpoint, TreatmentAxis};

const USAGE: &str = "usage: sharpebench compare --axis <entrant|invocation|score-config> \
                     --baseline <checkpoint.json> --treatment <checkpoint.json> [--json]";

/// The emitted document. `used_by_gate` is recorded for the reason
/// `SuiteEvidence` records it: a reader holding the JSON alone must not mistake
/// a comparability declaration for a result.
#[derive(Debug, Serialize)]
struct ComparisonDocument<'a> {
    used_by_gate: bool,
    baseline_checkpoint: &'a str,
    treatment_checkpoint: &'a str,
    receipt: ComparisonReceipt,
}

fn parse_axis(value: &str) -> Option<TreatmentAxis> {
    match value {
        "entrant" => Some(TreatmentAxis::Entrant),
        "invocation" => Some(TreatmentAxis::Invocation),
        "score-config" | "score_config" => Some(TreatmentAxis::ScoreConfig),
        _ => None,
    }
}

fn load(path: &str) -> Result<SweepCheckpoint, String> {
    SweepCheckpoint::load(Path::new(path)).map_err(|error| format!("cannot read {path}: {error}"))
}

/// The operator entry point. `0` declares the comparison, `1` refuses it, `2` is
/// a usage error.
pub fn run(args: &[String], json: bool) -> i32 {
    let (Some(axis), Some(baseline_path), Some(treatment_path)) = (
        crate::flag_value(args, "--axis"),
        crate::flag_value(args, "--baseline"),
        crate::flag_value(args, "--treatment"),
    ) else {
        eprintln!("{USAGE}");
        return 2;
    };
    let Some(axis) = parse_axis(axis) else {
        eprintln!("error: `{axis}` is not a declarable treatment axis.\n{USAGE}");
        return 2;
    };
    let (baseline, treatment) = match (load(baseline_path), load(treatment_path)) {
        (Ok(baseline), Ok(treatment)) => (baseline, treatment),
        (Err(error), _) | (_, Err(error)) => {
            eprintln!("error: {error}");
            return 1;
        }
    };

    match ComparisonReceipt::declare(axis, &baseline, &treatment) {
        Ok(receipt) => {
            if json {
                crate::emit_json(&ComparisonDocument {
                    used_by_gate: false,
                    baseline_checkpoint: baseline_path,
                    treatment_checkpoint: treatment_path,
                    receipt,
                });
            } else {
                println!(
                    "comparable on `{}`. Not used by the gate, eligibility or the rank.",
                    receipt.axis_field
                );
                println!(
                    "  baseline  {} {}",
                    receipt.baseline_agent_id, receipt.baseline_axis_sha256
                );
                println!(
                    "  treatment {} {}",
                    receipt.treatment_agent_id, receipt.treatment_axis_sha256
                );
                println!("  held fixed: {}", receipt.held_fixed.join(", "));
            }
            0
        }
        Err(refusal) => {
            if json {
                // The refusal is the result, so it is emitted as a document
                // rather than printed to stderr and lost by a machine reader.
                crate::emit_json(&serde_json::json!({
                    "used_by_gate": false,
                    "baseline_checkpoint": baseline_path,
                    "treatment_checkpoint": treatment_path,
                    "refusal": refusal.to_string(),
                }));
            } else {
                eprintln!("refused: {refusal}");
            }
            1
        }
    }
}
