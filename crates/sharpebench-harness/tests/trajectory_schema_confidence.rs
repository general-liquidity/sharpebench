//! Trajectory contract schema 3 marks the optional order confidence.
//!
//! A schema-2 capture wrote a confidence on every order, including the 0.5 the
//! wire default filled in for an entrant that stated none. Strict verification
//! refuses that schema. The explicit legacy regrade still replays such an
//! artifact, and then counts every recorded confidence as stated; this file pins
//! both halves so the documented consequence stays true.

use sharpebench_core::ScoreConfig;
use sharpebench_harness::{run_agent_capture, verify_trajectory, verify_trajectory_strict};
use sharpebench_protocol::{decision_from_wire, AgentTrajectory, Decision, MarketObservation};
use sharpebench_sim::{Agent, CostModel, Dataset, Window};

/// Orders every step; states 0.6 on every second decision and nothing otherwise.
struct Alternating {
    decided: usize,
}

impl Agent for Alternating {
    fn decide(&mut self, obs: &MarketObservation) -> Decision {
        self.decided += 1;
        let confidence = if self.decided.is_multiple_of(2) {
            r#","confidence":0.6"#
        } else {
            ""
        };
        decision_from_wire(&format!(
            r#"{{"orders":[{{"symbol":"{}","action":"buy","target_weight":0.3{confidence}}}]}}"#,
            obs.symbols[0].symbol
        ))
        .expect("the test decision conforms to the wire contract")
    }
}

fn capture() -> (Dataset, CostModel, AgentTrajectory) {
    let data = Dataset::synthetic(2, 80, 20_260_916);
    let costs = CostModel::default();
    let (_, trajectory) = run_agent_capture(
        "alternating",
        &data,
        &[Window { start: 20, end: 80 }],
        &[0],
        costs,
        || Box::new(Alternating { decided: 0 }) as Box<dyn Agent>,
    );
    (data, costs, trajectory)
}

#[test]
fn strict_verification_refuses_a_schema_2_capture() {
    let (data, costs, mut trajectory) = capture();
    let cfg = ScoreConfig::default();
    let fresh = verify_trajectory_strict(&data, &trajectory, costs, &cfg, None)
        .expect("a fresh library capture verifies");
    assert_eq!(
        fresh.score.calibration_observations, 29,
        "30 stated decisions, the last of which has no outcome in the window"
    );

    trajectory
        .contract
        .as_mut()
        .expect("captures are bound")
        .schema_version = 2;
    let error = verify_trajectory_strict(&data, &trajectory, costs, &cfg, None)
        .expect_err("a schema-2 capture cannot tell stated from filled-in confidences");
    assert!(
        error.contains("trajectory contract schema 2 is unsupported (expected 3)"),
        "{error}"
    );
}

#[test]
fn a_legacy_regrade_counts_recorded_filled_in_confidences_as_stated() {
    let (data, costs, trajectory) = capture();
    let cfg = ScoreConfig::default();
    let fresh = verify_trajectory(&data, &trajectory, costs, &cfg);

    // What a schema-2 capture of the same entrant recorded: a 0.5 on every order
    // that stated nothing.
    let mut legacy = serde_json::to_value(&trajectory).unwrap();
    for run in legacy["runs"].as_array_mut().unwrap() {
        for step in run["steps"].as_array_mut().unwrap() {
            for order in step["decision"]["orders"].as_array_mut().unwrap() {
                let order = order.as_object_mut().unwrap();
                order.entry("confidence").or_insert(serde_json::json!(0.5));
            }
        }
    }
    let legacy: AgentTrajectory = serde_json::from_value(legacy).unwrap();
    let regraded = verify_trajectory(&data, &legacy, costs, &cfg);

    assert_eq!(fresh.score.calibration_observations, 29);
    assert_eq!(
        regraded.score.calibration_observations, 59,
        "the legacy regrade cannot tell the filled-in 0.5 values from stated ones"
    );
    assert_eq!(
        fresh.score.deflated_sharpe.to_bits(),
        regraded.score.deflated_sharpe.to_bits(),
        "confidence never moves the returns"
    );
}
