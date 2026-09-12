//! `sharpebench compare`: the producer that turns two checkpoints the sweep
//! already wrote into a declared comparison, or into a refusal naming the field
//! that decided it.
//!
//! Driven through the actual binary rather than the library types, because the
//! defect the receipt was written against is exactly a comparability rule with
//! no caller: a rule nothing runs declares nothing about a real comparison.

use std::path::Path;
use std::process::Command;

use sharpebench_harness::checkpoint::{SweepCheckpoint, SweepContract, SweepIdentity};
use sharpebench_sim::Window;

fn cli(args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_sharpebench"))
        .args(args)
        .output()
        .expect("the CLI runs")
}

fn digest(byte: &str) -> String {
    byte.repeat(32)
}

/// One arm's checkpoint, bound to a contract whose six identities are all
/// explicit so a test can move exactly one of them.
fn arm(
    directory: &Path,
    name: &str,
    agent_id: &str,
    entrant: &str,
    invocation: &str,
    dataset: &str,
) -> String {
    let mut checkpoint = SweepCheckpoint::new(agent_id, 1, &[7, 9]);
    checkpoint.contract = Some(SweepContract::new(
        SweepIdentity {
            dataset_sha256: digest(dataset),
            cost_model_sha256: digest("11"),
            score_config_sha256: digest("22"),
            runner_artifact_sha256: digest("33"),
            entrant_sha256: digest(entrant),
            invocation_sha256: digest(invocation),
        },
        &[Window { start: 0, end: 40 }],
        &[7, 9],
        2,
    ));
    let path = directory.join(name);
    checkpoint.save(&path).expect("the checkpoint saves");
    path.to_str().expect("a UTF-8 path").to_string()
}

/// The passing control. Two arms that differ in the entrant and in nothing else
/// are a model comparison, and the receipt says so and names everything it
/// checked equal to get there. Without this case a `compare` that refused every
/// pair would satisfy the refusal tests below.
#[test]
fn two_arms_differing_only_in_the_entrant_declare_a_comparison() {
    let directory = tempfile::tempdir().expect("a checkpoint directory opens");
    let baseline = arm(
        directory.path(),
        "baseline.json",
        "baseline-agent",
        "aa",
        "cc",
        "dd",
    );
    let treatment = arm(
        directory.path(),
        "treatment.json",
        "treatment-agent",
        "bb",
        "cc",
        "dd",
    );

    let output = cli(&[
        "compare",
        "--axis",
        "entrant",
        "--baseline",
        &baseline,
        "--treatment",
        &treatment,
        "--json",
    ]);
    assert_eq!(
        output.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let doc: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("the comparison emits JSON");

    assert_eq!(doc["used_by_gate"], serde_json::Value::Bool(false));
    let receipt = &doc["receipt"];
    assert_eq!(receipt["axis"], "entrant");
    assert_eq!(receipt["axis_field"], "entrant_sha256");
    assert_eq!(receipt["baseline_agent_id"], "baseline-agent");
    assert_eq!(receipt["treatment_agent_id"], "treatment-agent");
    assert_eq!(receipt["baseline_axis_sha256"], digest("aa"));
    assert_eq!(receipt["treatment_axis_sha256"], digest("bb"));

    // Everything held fixed is named, so a reader who disagrees with the
    // declared axis can check what was held rather than assume anything was.
    let held: Vec<&str> = receipt["held_fixed"]
        .as_array()
        .expect("a list")
        .iter()
        .map(|f| f.as_str().unwrap())
        .collect();
    assert_eq!(
        held,
        vec![
            "dataset_sha256",
            "cost_model_sha256",
            "score_config_sha256",
            "runner_artifact_sha256",
            "invocation_sha256"
        ]
    );
}

/// The refusal the axis exists to produce. Two arms on different datasets are
/// two experiments, and declaring the entrant as the treatment does not make
/// them one.
#[test]
fn an_off_axis_dataset_difference_refuses_and_names_the_field() {
    let directory = tempfile::tempdir().expect("a checkpoint directory opens");
    let baseline = arm(
        directory.path(),
        "baseline.json",
        "baseline-agent",
        "aa",
        "cc",
        "dd",
    );
    let treatment = arm(
        directory.path(),
        "treatment.json",
        "treatment-agent",
        "bb",
        "cc",
        "ee",
    );

    let output = cli(&[
        "compare",
        "--axis",
        "entrant",
        "--baseline",
        &baseline,
        "--treatment",
        &treatment,
        "--json",
    ]);
    assert_eq!(output.status.code(), Some(1));
    let doc: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("the refusal is a document, not a log line");
    let refusal = doc["refusal"].as_str().expect("the refusal names a field");
    assert!(
        refusal.contains("dataset_sha256"),
        "the refusal must name the field that decided it: {refusal}"
    );
    assert!(refusal.contains(&digest("dd")), "{refusal}");
    assert!(refusal.contains(&digest("ee")), "{refusal}");
    assert!(
        doc.get("receipt").is_none(),
        "a refused comparison declares no receipt: {doc}"
    );
}

/// The dataset, the cost model and the runner artifact are never declarable, so
/// the thing that would void a comparison cannot be declared away.
#[test]
fn the_dataset_is_not_a_declarable_axis() {
    let directory = tempfile::tempdir().expect("a checkpoint directory opens");
    let baseline = arm(
        directory.path(),
        "baseline.json",
        "baseline-agent",
        "aa",
        "cc",
        "dd",
    );
    let treatment = arm(
        directory.path(),
        "treatment.json",
        "treatment-agent",
        "aa",
        "cc",
        "ee",
    );

    let output = cli(&[
        "compare",
        "--axis",
        "dataset",
        "--baseline",
        &baseline,
        "--treatment",
        &treatment,
        "--json",
    ]);
    assert_eq!(
        output.status.code(),
        Some(2),
        "an undeclarable axis is a usage error, not a comparison"
    );
    assert!(output.stdout.is_empty(), "nothing is declared");
}

/// An arm with no contract binds nothing, so there is nothing to compare
/// against and the refusal says which arm.
#[test]
fn an_unbound_arm_refuses_rather_than_comparing_nothing() {
    let directory = tempfile::tempdir().expect("a checkpoint directory opens");
    let baseline = arm(
        directory.path(),
        "baseline.json",
        "baseline-agent",
        "aa",
        "cc",
        "dd",
    );
    let path = directory.path().join("unbound.json");
    SweepCheckpoint::new("unbound-agent", 1, &[7, 9])
        .save(&path)
        .expect("the checkpoint saves");
    let treatment = path.to_str().expect("a UTF-8 path").to_string();

    let output = cli(&[
        "compare",
        "--axis",
        "entrant",
        "--baseline",
        &baseline,
        "--treatment",
        &treatment,
        "--json",
    ]);
    assert_eq!(output.status.code(), Some(1));
    let doc: serde_json::Value = serde_json::from_slice(&output.stdout).expect("a document");
    let refusal = doc["refusal"].as_str().expect("a refusal");
    assert!(
        refusal.contains("unbound-agent"),
        "the refusal names the arm that binds nothing: {refusal}"
    );
}

/// A comparison reads. It must leave both checkpoints exactly as it found them,
/// refusal or not.
#[test]
fn a_comparison_writes_to_neither_arm() {
    let directory = tempfile::tempdir().expect("a checkpoint directory opens");
    let baseline = arm(
        directory.path(),
        "baseline.json",
        "baseline-agent",
        "aa",
        "cc",
        "dd",
    );
    let treatment = arm(
        directory.path(),
        "treatment.json",
        "treatment-agent",
        "bb",
        "cc",
        "ee",
    );
    let before = (
        std::fs::read(&baseline).expect("reads"),
        std::fs::read(&treatment).expect("reads"),
    );

    for axis in ["entrant", "invocation", "score-config"] {
        cli(&[
            "compare",
            "--axis",
            axis,
            "--baseline",
            &baseline,
            "--treatment",
            &treatment,
            "--json",
        ]);
    }
    assert_eq!(
        before,
        (
            std::fs::read(&baseline).expect("reads"),
            std::fs::read(&treatment).expect("reads"),
        ),
        "a comparison declares; it does not write"
    );
}
