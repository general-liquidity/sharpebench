//! `sharpebench decision-stability`, driven through the built binary.
//!
//! The library cases in `sharpebench-core` and `sharpebench-harness` cover the
//! grouping and the replay. These cases cover what an operator runs: capturing
//! with the binary, reading several trajectory files, binding them to the
//! running executable and the resolved dataset, and the emitted document.
//! Every refusal asserts its own cause, because several causes exit nonzero.

use std::cell::Cell;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use serde_json::{json, Value};
use sharpebench_harness::run_agent_capture;
use sharpebench_protocol::{Action, AgentTrajectory, Decision, MarketObservation, Order};
use sharpebench_sim::{Agent, CostModel, Dataset, Window};

fn cli(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_sharpebench"))
        .args(args)
        .output()
        .expect("the CLI runs")
}

fn stdout_json(output: &Output) -> Value {
    assert!(
        output.status.success(),
        "exit {:?}, stderr: {}",
        output.status.code(),
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).expect("the report is JSON")
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

fn path(dir: &Path, name: &str) -> String {
    dir.join(name).to_string_lossy().into_owned()
}

/// The digest the binary reports as its own runner identity.
fn binary_sha256() -> String {
    sharpebench_attest::content_digest(
        &std::fs::read(env!("CARGO_BIN_EXE_sharpebench")).expect("the built binary reads"),
    )
}

/// The dataset and windows the binary resolves when `--data` is absent.
fn default_field() -> (Dataset, Vec<Window>) {
    (
        Dataset::synthetic(8, 180, 20_260_621),
        vec![
            Window {
                start: 20,
                end: 100,
            },
            Window {
                start: 100,
                end: 180,
            },
        ],
    )
}

/// Never trades; the odd-numbered agents it creates state a different
/// confidence on every fourth step.
struct PlantedFlipper {
    odd: bool,
    step: usize,
}

impl Agent for PlantedFlipper {
    fn decide(&mut self, observation: &MarketObservation) -> Decision {
        let confidence = if self.odd && self.step.is_multiple_of(4) {
            0.9
        } else {
            0.1
        };
        self.step += 1;
        Decision {
            orders: vec![Order {
                symbol: observation.symbols[0].symbol.clone(),
                action: Action::Hold,
                target_weight: 0.0,
                confidence,
                rationale: String::new(),
            }],
            reasoning: String::new(),
            cost: None,
        }
    }
}

/// A flipper trajectory over the binary's default field, bound to the binary.
fn write_flipper(dir: &Path, name: &str, seeds: &[u64]) -> String {
    let (data, windows) = default_field();
    let created = Cell::new(0usize);
    let (_, mut trajectory): (_, AgentTrajectory) = run_agent_capture(
        "flipper",
        &data,
        &windows,
        seeds,
        CostModel::default(),
        || {
            let index = created.get();
            created.set(index + 1);
            Box::new(PlantedFlipper {
                odd: index % 2 == 1,
                step: 0,
            }) as Box<dyn Agent>
        },
    );
    trajectory
        .contract
        .as_mut()
        .expect("captures are bound")
        .runner_artifact_sha256 = Some(binary_sha256());
    let out = path(dir, name);
    std::fs::write(&out, serde_json::to_vec(&trajectory).unwrap()).unwrap();
    out
}

fn capture(dir: &Path, agent: &str, name: &str) -> String {
    let out = path(dir, name);
    let output = cli(&["capture", agent, &out]);
    assert!(output.status.success(), "{}", stderr(&output));
    out
}

#[test]
fn a_deterministic_reference_agent_reports_zero_over_repeated_captures() {
    let dir = tempfile::tempdir().unwrap();
    let first = capture(dir.path(), "momentum", "first.json");
    let second = capture(dir.path(), "momentum", "second.json");

    let report = stdout_json(&cli(&["decision-stability", &first, &second, "--json"]));
    assert_eq!(report["schema"], "sharpebench.decision-stability.v1");
    assert_eq!(report["agent_id"], "momentum");
    assert_eq!(report["rank_input"], false);
    assert_eq!(report["replicate_runs"], 32);
    assert_eq!(report["steps_total"], 32 * 80);
    // Each seed is matched by the same seed of the other capture at every step.
    assert_eq!(report["steps_compared"], 32 * 80);
    assert_eq!(report["steps_excluded_diverged_observation"], 0);
    assert_eq!(report["groups_with_differing_decisions"], 0);
    assert!(report["groups_compared"].as_u64().unwrap() >= 2 * 80);
    assert_eq!(
        report["differing_fraction"],
        json!({"status": "available", "value": 0.0})
    );
    let windows: Vec<(u64, u64, u64)> = report["windows"]
        .as_array()
        .unwrap()
        .iter()
        .map(|window| {
            (
                window["window_start"].as_u64().unwrap(),
                window["window_end"].as_u64().unwrap(),
                window["replicates"].as_u64().unwrap(),
            )
        })
        .collect();
    assert_eq!(windows, vec![(20, 100, 16), (100, 180, 16)]);
}

#[test]
fn execution_seeds_of_one_capture_diverge_and_the_excluded_steps_are_counted() {
    let dir = tempfile::tempdir().unwrap();
    let only = capture(dir.path(), "buy-and-hold", "only.json");
    let report = stdout_json(&cli(&["decision-stability", &only, "--json"]));
    // Buy-and-hold fills on its first step at a seed-dependent price, so the
    // eight seeds share one observation per window.
    assert_eq!(report["groups_compared"], 2);
    assert_eq!(report["steps_compared"], 2 * 8);
    assert_eq!(report["steps_excluded_diverged_observation"], 2 * 8 * 79);
    assert_eq!(report["steps_unreplicated"], 0);
    assert_eq!(
        report["differing_fraction"],
        json!({"status": "available", "value": 0.0})
    );
}

#[test]
fn a_planted_flip_is_reported_at_its_planted_rate() {
    let dir = tempfile::tempdir().unwrap();
    let flipper = write_flipper(dir.path(), "flipper.json", &[0, 1, 2, 3]);
    let report = stdout_json(&cli(&["decision-stability", &flipper, "--json"]));
    assert_eq!(report["groups_compared"], 160);
    assert_eq!(report["groups_with_differing_decisions"], 40);
    assert_eq!(
        report["differing_fraction"],
        json!({"status": "available", "value": 0.25})
    );
    let first_window = &report["windows"][0];
    assert_eq!(
        first_window["differing_groups"].as_array().unwrap().len(),
        20
    );
    assert_eq!(first_window["differing_groups"][1]["step"], 4);
    assert_eq!(first_window["differing_groups"][1]["distinct_decisions"], 2);

    let text = cli(&["decision-stability", &flipper]);
    assert!(text.status.success(), "{}", stderr(&text));
    let text = String::from_utf8_lossy(&text.stdout);
    assert!(text.contains("rank-neutral, not a rank input"), "{text}");
    assert!(
        text.contains("differing fraction : 0.2500 (40 of 160 groups)"),
        "{text}"
    );
}

#[test]
fn a_single_replicate_is_typed_unavailable() {
    let dir = tempfile::tempdir().unwrap();
    let alone = write_flipper(dir.path(), "alone.json", &[3]);
    let output = cli(&["decision-stability", &alone, "--json"]);
    let report = stdout_json(&output);
    assert_eq!(
        report["differing_fraction"],
        json!({"status": "unavailable", "reason": "single_replicate"})
    );
    assert_eq!(report["steps_unreplicated"], 160);
    assert_eq!(report["groups_compared"], 0);

    let text = cli(&["decision-stability", &alone]);
    assert!(String::from_utf8_lossy(&text.stdout).contains("unavailable (single_replicate)"));
}

#[test]
fn usage_errors_name_their_cause() {
    let dir = tempfile::tempdir().unwrap();
    let flipper = write_flipper(dir.path(), "flipper.json", &[0, 1]);

    let none = cli(&["decision-stability", "--json"]);
    assert_eq!(none.status.code(), Some(2));
    assert!(stderr(&none).contains("name at least one trajectory"));
    assert!(stderr(&none).contains("usage: sharpebench decision-stability"));

    let twice = cli(&["decision-stability", &flipper, &flipper]);
    assert_eq!(twice.status.code(), Some(2));
    assert!(
        stderr(&twice).contains("is named more than once"),
        "{}",
        stderr(&twice)
    );

    let unknown = cli(&["decision-stability", &flipper, "--seeds", "4"]);
    assert_eq!(unknown.status.code(), Some(2));
    assert!(stderr(&unknown).contains("unknown option `--seeds`"));

    let dangling = cli(&["decision-stability", &flipper, "--data"]);
    assert_eq!(dangling.status.code(), Some(2));
    assert!(stderr(&dangling).contains("--data requires a CSV path"));
}

#[test]
fn a_field_that_cannot_be_replayed_as_captured_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let flipper = write_flipper(dir.path(), "flipper.json", &[0, 1]);
    let momentum = capture(dir.path(), "momentum", "momentum.json");

    let mixed = cli(&["decision-stability", &flipper, &momentum]);
    assert_eq!(mixed.status.code(), Some(1));
    assert!(
        stderr(&mixed).contains("a stability report covers one entrant"),
        "{}",
        stderr(&mixed)
    );

    let mut foreign: AgentTrajectory =
        serde_json::from_slice(&std::fs::read(&flipper).unwrap()).unwrap();
    foreign.contract.as_mut().unwrap().runner_artifact_sha256 = Some("ab".repeat(32));
    let foreign_path: PathBuf = dir.path().join("foreign.json");
    std::fs::write(&foreign_path, serde_json::to_vec(&foreign).unwrap()).unwrap();
    let refused = cli(&["decision-stability", foreign_path.to_str().unwrap()]);
    assert_eq!(refused.status.code(), Some(1));
    assert!(
        stderr(&refused).contains("does not match verifier runner"),
        "{}",
        stderr(&refused)
    );

    let missing = cli(&["decision-stability", &path(dir.path(), "absent.json")]);
    assert_eq!(missing.status.code(), Some(1));
    assert!(stderr(&missing).contains("cannot read"));
}
