//! Golden-input regression gate for the simulator.
//!
//! `sharpebench-core/golden/synthetic_field.input.json` is a committed set of
//! trajectories built entirely from this crate's public API: one synthetic
//! dataset, walk-forward windows, four reference agents, two execution-noise
//! seeds each. The kernel's golden tests re-score that file without ever calling
//! the simulator. This test closes the other half of the loop: it rebuilds the
//! field from the same constants and asserts the bytes are identical, so a change
//! anywhere in the point-in-time engine, the cost model, the execution noise or
//! the reference agents fails here, and fails here only. A kernel regression
//! fails in `core`; a simulator regression fails in `sim`; the two cannot be
//! confused for one another.
//!
//! The fixture lives under `core/golden/` rather than here because `core`'s
//! tests read it and `core` cannot depend on `sim` (that is a cycle `cargo
//! publish` rejects). It is reached by relative path.
//!
//! Regenerating after a deliberate simulator change:
//!
//! ```text
//! SHARPEBENCH_UPDATE_GOLDEN=1 cargo test -p sharpebench-sim --test golden_input
//! SHARPEBENCH_UPDATE_GOLDEN=1 cargo test -p sharpebench-core --test golden_scores
//! ```
//!
//! in that order: the input first, then the scores of the new input. Setting
//! the variable in CI is refused.

use std::path::{Path, PathBuf};

use sharpebench_core::AgentSubmission;
use sharpebench_sim::{
    run_backtest, walk_forward, Agent, BuyAndHold, CostModel, Dataset, HoldAgent, Momentum,
    RandomAgent,
};

type AgentFactory = Box<dyn Fn() -> Box<dyn Agent>>;

const UPDATE_ENV: &str = "SHARPEBENCH_UPDATE_GOLDEN";
const FIXTURE: &str = "synthetic_field.input.json";

fn fixture_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../sharpebench-core/golden")
        .join(FIXTURE)
}

fn update_requested() -> bool {
    let requested = std::env::var_os(UPDATE_ENV).is_some_and(|v| !v.is_empty() && v != "0");
    assert!(
        !(requested && std::env::var_os("CI").is_some()),
        "{UPDATE_ENV} is set in a CI environment; golden fixtures may only be regenerated locally"
    );
    requested
}

fn pretty_with_newline<T: serde::Serialize>(value: &T) -> String {
    let mut s = serde_json::to_string_pretty(value).expect("serialise");
    s.push('\n');
    s
}

/// A field built entirely from the public simulator API. Pure function of the
/// constants below: no I/O, no clock. These constants are the fixture's identity;
/// change one and the fixture must be regenerated and reviewed.
fn synthetic_field() -> Vec<AgentSubmission> {
    const SYMBOLS: usize = 4;
    const DAYS: usize = 160;
    const DATA_SEED: u64 = 2026;
    const EXEC_SEEDS: [u64; 2] = [1, 2];

    let data = Dataset::synthetic(SYMBOLS, DAYS, DATA_SEED);
    let windows = walk_forward(DAYS, 20, 40, 40);
    assert!(
        !windows.is_empty(),
        "walk_forward must yield at least one window"
    );

    let agents: Vec<(&str, AgentFactory)> = vec![
        ("buy-and-hold", Box::new(|| Box::new(BuyAndHold))),
        ("momentum", Box::new(|| Box::new(Momentum::default()))),
        ("hold", Box::new(|| Box::new(HoldAgent))),
        ("random", Box::new(|| Box::new(RandomAgent::new(7)))),
    ];

    agents
        .into_iter()
        .map(|(id, make)| {
            let mut runs = Vec::new();
            for window in &windows {
                for seed in EXEC_SEEDS {
                    let mut agent = make();
                    runs.push(run_backtest(
                        &data,
                        agent.as_mut(),
                        *window,
                        seed,
                        CostModel::default(),
                    ));
                }
            }
            AgentSubmission {
                agent_id: id.to_string(),
                runs,
                in_sample_trials: 0,
                candidates: Vec::new(),
            }
        })
        .collect()
}

#[test]
fn synthetic_backtest_input_is_byte_identical_to_golden() {
    let actual = pretty_with_newline(&synthetic_field());
    let path = fixture_path();
    if update_requested() {
        std::fs::write(&path, &actual).unwrap_or_else(|e| panic!("write {}: {e}", path.display()));
        eprintln!("rewrote golden fixture {}", path.display());
    }
    let expected = std::fs::read_to_string(&path).unwrap_or_else(|e| {
        panic!(
            "read {}: {e} (run with {UPDATE_ENV}=1 to create it)",
            path.display()
        )
    });
    assert!(
        expected == actual,
        "{FIXTURE} drifted: the simulator no longer reproduces the committed trajectories.\n\
         If the change is intentional, regenerate with `{UPDATE_ENV}=1 cargo test -p sharpebench-sim --test golden_input`, then the core scores, and review both diffs."
    );
}
