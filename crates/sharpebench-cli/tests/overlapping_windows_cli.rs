//! `verify-trajectory` refuses a capture over overlapping windows, driven
//! through the built binary. The trajectory is otherwise a faithful capture on
//! the CLI's own synthetic dataset, bound to this binary, so the only reason
//! for the refusal is the overlap.

use std::process::{Command, Output};

use sharpebench_harness::run_agent_capture;
use sharpebench_sim::{Agent, BuyAndHold, CostModel, Dataset, Window};

fn cli(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_sharpebench"))
        .args(args)
        .output()
        .expect("the CLI runs")
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

/// A capture the CLI would verify if its windows were disjoint: the dataset
/// `verify-trajectory` uses without `--data`, and this binary's identity.
fn write_capture(dir: &std::path::Path, name: &str, windows: &[Window]) -> String {
    let data = Dataset::synthetic(8, 180, 20_260_621);
    let seeds: Vec<u64> = (0..2).collect();
    let (_, mut trajectory) = run_agent_capture(
        "buy-and-hold",
        &data,
        windows,
        &seeds,
        CostModel::default(),
        || Box::new(BuyAndHold) as Box<dyn Agent>,
    );
    let runner = sharpebench_attest::content_digest(
        &std::fs::read(env!("CARGO_BIN_EXE_sharpebench")).expect("the built binary reads"),
    );
    trajectory
        .contract
        .as_mut()
        .expect("captures are bound")
        .runner_artifact_sha256 = Some(runner);
    let path = dir.join(name);
    std::fs::write(&path, serde_json::to_string(&trajectory).unwrap()).unwrap();
    path.to_str().unwrap().to_string()
}

#[test]
fn verify_trajectory_refuses_overlapping_windows_and_accepts_adjacent_ones() {
    let dir = tempfile::tempdir().unwrap();
    let overlapping = write_capture(
        dir.path(),
        "overlapping.json",
        &[
            Window {
                start: 20,
                end: 100,
            },
            Window {
                start: 60,
                end: 140,
            },
        ],
    );
    let refused = cli(&["verify-trajectory", &overlapping]);
    assert_eq!(refused.status.code(), Some(1), "{}", stderr(&refused));
    assert!(
        stderr(&refused).contains(
            "trajectory contract: evaluation windows 0 [20, 100) and 1 [60, 140) overlap on bars [60, 100)"
        ),
        "{}",
        stderr(&refused)
    );

    // The explicit legacy regrade does not check, and says it proves nothing
    // about the original execution conditions.
    let legacy = cli(&[
        "verify-trajectory",
        &overlapping,
        "--allow-unbound-trajectory",
    ]);
    assert!(legacy.status.success(), "{}", stderr(&legacy));

    let adjacent = write_capture(
        dir.path(),
        "adjacent.json",
        &[
            Window {
                start: 20,
                end: 100,
            },
            Window {
                start: 100,
                end: 180,
            },
        ],
    );
    let accepted = cli(&["verify-trajectory", &adjacent]);
    assert!(accepted.status.success(), "{}", stderr(&accepted));
}
