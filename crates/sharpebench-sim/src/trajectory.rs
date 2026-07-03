//! Trajectory capture + replay-recompute — the Tier 1 verification boundary.
//!
//! The persisted artifact is the agent's *raw decisions*, never its returns or any
//! self-reported metric (those are recomputed). [`run_backtest_capture`] records the
//! [`Decision`] the agent emits at every point-in-time step while it drives the
//! identical [`crate::engine::run_backtest`] path; [`replay_run`] feeds those frozen
//! decisions back through the **same** engine to regenerate the [`Run`]
//! byte-for-byte. Because capture and replay share one engine code path and the
//! engine's only other input (the execution seed) is stored in the trajectory, the
//! round trip is exact by construction: replaying a captured trajectory reproduces
//! the original `Run` exactly, so a score recomputed from the artifact is provably
//! the score the agent's decisions actually earned.

use sharpebench_core::Run;
use sharpebench_protocol::{
    AgentTrajectory, Decision, DecisionStep, MarketObservation, RunTrajectory,
};

use crate::agent::Agent;
use crate::costs::CostModel;
use crate::data::Dataset;
use crate::engine::{run_backtest, Window};

/// Wraps an [`Agent`], recording every [`Decision`] it makes (tagged with the
/// observation's date and step) while passing the call straight through. The
/// recorded steps become a [`RunTrajectory`] — the raw, replayable artifact.
struct CapturingAgent<'a> {
    inner: &'a mut dyn Agent,
    window_start: usize,
    steps: Vec<DecisionStep>,
}

impl Agent for CapturingAgent<'_> {
    fn decide(&mut self, obs: &MarketObservation) -> Decision {
        let decision = self.inner.decide(obs);
        self.steps.push(DecisionStep {
            step: self.steps.len(),
            observation_id: obs.date.clone(),
            decision: decision.clone(),
        });
        // The engine sees the byte-identical decision the inner agent returned.
        decision
    }
}

/// Replays a frozen sequence of decisions: returns each recorded [`Decision`] in
/// step order, ignoring the live observation. Past the end (which cannot happen on
/// a faithful replay of the same window) it holds. Driving this through the engine
/// is what makes replay deterministic and engine-identical.
struct ReplayAgent {
    steps: std::vec::IntoIter<DecisionStep>,
}

impl Agent for ReplayAgent {
    fn decide(&mut self, _obs: &MarketObservation) -> Decision {
        self.steps
            .next()
            .map(|s| s.decision)
            .unwrap_or_else(|| Decision {
                orders: Vec::new(),
                reasoning: "replay exhausted → hold".to_string(),
                cost: None,
            })
    }
}

/// Run a backtest while capturing the agent's raw decisions. Returns the same
/// [`Run`] [`run_backtest`] would produce, plus the [`RunTrajectory`] artifact (the
/// per-step decisions + the window/seed coordinates needed to replay it).
pub fn run_backtest_capture(
    data: &Dataset,
    agent: &mut dyn Agent,
    window: Window,
    seed: u64,
    costs: CostModel,
) -> (Run, RunTrajectory) {
    let mut cap = CapturingAgent {
        inner: agent,
        window_start: window.start,
        steps: Vec::new(),
    };
    let run = run_backtest(data, &mut cap, window, seed, costs);
    let traj = RunTrajectory {
        window_start: cap.window_start,
        window_end: window.end,
        seed,
        steps: cap.steps,
    };
    (run, traj)
}

/// Replay one captured run's raw decisions through the identical point-in-time
/// engine to regenerate its [`Run`]. The frozen `data` and `costs` must match those
/// the trajectory was captured against; the window and seed come from the artifact.
///
/// Round-trip invariant: `replay_run(data, &traj, costs)` is byte-identical to the
/// `Run` returned alongside `traj` by [`run_backtest_capture`].
pub fn replay_run(data: &Dataset, traj: &RunTrajectory, costs: CostModel) -> Run {
    let mut agent = ReplayAgent {
        steps: traj.steps.clone().into_iter(),
    };
    run_backtest(
        data,
        &mut agent,
        Window {
            start: traj.window_start,
            end: traj.window_end,
        },
        traj.seed,
        costs,
    )
}

/// Replay a whole [`AgentTrajectory`] back into the [`sharpebench_core::AgentSubmission`]
/// the scoring kernel consumes — recomputing every `Run` from raw decisions alone.
/// This is the engine half of the separate-verifier path: the resulting submission
/// is derived only from the persisted decisions + the frozen dataset, never from any
/// metric the agent reported.
pub fn replay_submission(
    data: &Dataset,
    traj: &AgentTrajectory,
    costs: CostModel,
) -> sharpebench_core::AgentSubmission {
    let runs = traj
        .runs
        .iter()
        .map(|rt| replay_run(data, rt, costs))
        .collect();
    sharpebench_core::AgentSubmission {
        agent_id: traj.agent_id.clone(),
        runs,
        in_sample_trials: traj.in_sample_trials,
        candidates: Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::{BuyAndHold, Momentum};

    #[test]
    fn capture_then_replay_is_byte_identical() {
        let data = Dataset::synthetic(4, 120, 11);
        let window = Window {
            start: 20,
            end: 120,
        };
        let costs = CostModel::default();
        let (direct, traj) =
            run_backtest_capture(&data, &mut Momentum::default(), window, 3, costs);
        let replayed = replay_run(&data, &traj, costs);
        // The whole Run must match field-for-field: returns, trace, conf, outcomes.
        assert_eq!(
            serde_json::to_string(&direct).unwrap(),
            serde_json::to_string(&replayed).unwrap(),
            "replay must reproduce the captured run byte-for-byte"
        );
    }

    #[test]
    fn capture_matches_a_plain_run() {
        let data = Dataset::synthetic(3, 100, 5);
        let window = Window {
            start: 20,
            end: 100,
        };
        let costs = CostModel::default();
        let plain = run_backtest(&data, &mut BuyAndHold, window, 1, costs);
        let (captured, _traj) = run_backtest_capture(&data, &mut BuyAndHold, window, 1, costs);
        assert_eq!(
            serde_json::to_string(&plain).unwrap(),
            serde_json::to_string(&captured).unwrap(),
            "capturing must not perturb the run"
        );
    }

    #[test]
    fn trajectory_records_one_step_per_window_day() {
        let data = Dataset::synthetic(2, 60, 9);
        let window = Window { start: 20, end: 60 };
        let (_run, traj) =
            run_backtest_capture(&data, &mut BuyAndHold, window, 0, CostModel::default());
        assert_eq!(traj.steps.len(), 40, "one decision per step in the window");
        assert_eq!(traj.steps[0].observation_id, data.dates[20]);
        assert_eq!(traj.steps[0].step, 0);
    }

    #[test]
    fn tampered_trajectory_yields_a_different_run() {
        let data = Dataset::synthetic(4, 120, 11);
        let window = Window {
            start: 20,
            end: 120,
        };
        let costs = CostModel::default();
        let (direct, mut traj) =
            run_backtest_capture(&data, &mut Momentum::default(), window, 3, costs);
        // Tamper: inflate every order's target weight. An honest replay through the
        // engine produces a *different* Run — the artifact can't lie about returns.
        for step in &mut traj.steps {
            for order in &mut step.decision.orders {
                order.target_weight *= 2.0;
            }
        }
        let replayed = replay_run(&data, &traj, costs);
        assert_ne!(
            direct.returns, replayed.returns,
            "a tampered trajectory must recompute to different returns"
        );
    }
}
