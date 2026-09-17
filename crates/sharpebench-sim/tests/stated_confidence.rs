//! The simulator pairs only stated confidences, each with the step that realizes it.
//!
//! Two rules are pinned here. A decision contributes a calibration pair only when
//! at least one of its orders states a confidence, using the mean over the orders
//! that do: a hold, or orders that omit the field, add nothing. And the pair's
//! outcome is the next step's return, because the return booked at step `t` is the
//! price move on the holdings decision `t - 1` chose; the window's final decision
//! has no outcome inside the window and adds no pair.
//!
//! Decisions are built from wire JSON, the path an external entrant takes.

use std::collections::BTreeMap;

use sharpebench_core::{score_agent, AgentSubmission, Run, ScoreConfig};
use sharpebench_protocol::{decision_from_wire, Decision, MarketObservation};
use sharpebench_sim::{
    replay_run, run_backtest, run_backtest_capture, Agent, CostModel, Dataset, HoldAgent, Window,
};

fn score(run: Run) -> sharpebench_core::CompositeScore {
    score_agent(
        &AgentSubmission {
            agent_id: "agent".to_string(),
            runs: vec![run],
            in_sample_trials: 0,
            candidates: Vec::new(),
        },
        &ScoreConfig::default(),
    )
}

fn frictionless() -> CostModel {
    CostModel {
        fee_bps: 0.0,
        slippage_bps: 0.0,
        impact_bps: 0.0,
        financing_bps: 0.0,
        ..CostModel::default()
    }
}

/// An order on the first observed symbol, with `confidence` spliced in verbatim
/// (an empty string omits the key).
fn wire_order(obs: &MarketObservation, index: usize, weight: f64, confidence: &str) -> String {
    format!(
        r#"{{"symbol":"{}","action":"buy","target_weight":{weight}{confidence}}}"#,
        obs.symbols[index].symbol
    )
}

fn decision(orders: &[String]) -> Decision {
    decision_from_wire(&format!(r#"{{"orders":[{}]}}"#, orders.join(",")))
        .expect("the test decision conforms to the wire contract")
}

/// Buys the first symbol every step and never states a confidence.
struct Unstated;
impl Agent for Unstated {
    fn decide(&mut self, obs: &MarketObservation) -> Decision {
        decision(&[wire_order(obs, 0, 0.3, "")])
    }
}

/// Buys the first symbol every step and states `0.9`.
struct Stated;
impl Agent for Stated {
    fn decide(&mut self, obs: &MarketObservation) -> Decision {
        decision(&[wire_order(obs, 0, 0.3, r#","confidence":0.9"#)])
    }
}

/// Two orders per step, only the first of which states a confidence.
struct HalfStated;
impl Agent for HalfStated {
    fn decide(&mut self, obs: &MarketObservation) -> Decision {
        decision(&[
            wire_order(obs, 0, 0.2, r#","confidence":0.8"#),
            wire_order(obs, 1, 0.2, ""),
        ])
    }
}

/// States 0.9 on its first `trades` decisions, then holds.
struct TradesThenHolds {
    trades: usize,
    decided: usize,
}
impl Agent for TradesThenHolds {
    fn decide(&mut self, obs: &MarketObservation) -> Decision {
        self.decided += 1;
        if self.decided <= self.trades {
            decision(&[wire_order(obs, 0, 0.3, r#","confidence":0.9"#)])
        } else {
            decision(&[])
        }
    }
}

#[test]
fn an_order_without_confidence_adds_no_calibration_pair() {
    let data = Dataset::synthetic(2, 80, 7);
    let run = run_backtest(
        &data,
        &mut Unstated,
        Window { start: 20, end: 80 },
        1,
        CostModel::default(),
    );
    assert_eq!(run.returns.len(), 60);
    assert!(run.confidences.is_empty(), "{:?}", run.confidences);
    assert!(run.outcomes.is_empty());
    let score = score(run);
    assert_eq!(score.calibration_brier, None);
    assert_eq!(score.calibration_observations, 0);
}

#[test]
fn a_hold_only_run_reports_no_calibration() {
    let data = Dataset::synthetic(3, 80, 7);
    let run = run_backtest(
        &data,
        &mut HoldAgent,
        Window { start: 20, end: 80 },
        1,
        CostModel::default(),
    );
    assert_eq!(run.returns.len(), 60);
    assert!(run.confidences.is_empty() && run.outcomes.is_empty());
    let score = score(run);
    assert_eq!(
        score.calibration_brier, None,
        "an agent that never stated a confidence has no calibration"
    );
    assert_eq!(score.calibration_observations, 0);
}

#[test]
fn a_stated_confidence_still_counts() {
    let data = Dataset::synthetic(2, 80, 7);
    let run = run_backtest(
        &data,
        &mut Stated,
        Window { start: 20, end: 80 },
        1,
        CostModel::default(),
    );
    assert_eq!(
        run.confidences,
        vec![0.9; 59],
        "every decision but the last has its outcome inside the window"
    );
    assert_eq!(run.outcomes.len(), 59);
    let expected_outcomes: Vec<bool> = run.returns[1..].iter().map(|r| *r > 0.0).collect();
    assert_eq!(run.outcomes, expected_outcomes);
    let score = score(run);
    assert_eq!(score.calibration_observations, 59);
    assert!(score.calibration_brier.is_some());
}

#[test]
fn only_orders_that_state_a_confidence_enter_the_step_mean() {
    let data = Dataset::synthetic(2, 60, 3);
    let run = run_backtest(
        &data,
        &mut HalfStated,
        Window { start: 20, end: 60 },
        1,
        CostModel::default(),
    );
    assert_eq!(
        run.confidences,
        vec![0.8; 39],
        "the unstated order must not pull the step mean toward a filled-in value"
    );
}

#[test]
fn trading_then_holding_counts_only_the_stated_decisions() {
    let data = Dataset::synthetic(2, 280, 11);
    let mut agent = TradesThenHolds {
        trades: 20,
        decided: 0,
    };
    let run = run_backtest(
        &data,
        &mut agent,
        Window {
            start: 20,
            end: 270,
        },
        1,
        CostModel::default(),
    );
    assert_eq!(run.returns.len(), 250);
    let score = score(run);
    assert_eq!(
        score.calibration_observations, 20,
        "20 stated trades, not the 250 bars of the run"
    );
}

/// A cost-free series that alternates between two prices, so every move is
/// knowable from the last one.
fn alternating(n: usize) -> Dataset {
    let closes: Vec<f64> = (0..n)
        .map(|t| if t % 2 == 0 { 100.0 } else { 110.0 })
        .collect();
    Dataset {
        dates: (0..n).map(|t| format!("2026-01-{t:03}")).collect(),
        closes: BTreeMap::from([("A".to_string(), closes)]),
        dividends: BTreeMap::new(),
    }
}

/// Fully long every bar; states 1.0 exactly when the last move was down, which on
/// the alternating series is exactly when the next bar pays off, and 0.0 otherwise.
struct NextBarForecaster;
impl Agent for NextBarForecaster {
    fn decide(&mut self, obs: &MarketObservation) -> Decision {
        let history = &obs.symbols[0].close_history;
        let last_move_down = history[history.len() - 1] < history[history.len() - 2];
        let confidence = if last_move_down { "1.0" } else { "0.0" };
        decision(&[wire_order(
            obs,
            0,
            1.0,
            &format!(r#","confidence":{confidence}"#),
        )])
    }
}

#[test]
fn calibration_pairs_each_confidence_with_the_step_that_realizes_it() {
    let data = alternating(40);
    let run = run_backtest(
        &data,
        &mut NextBarForecaster,
        Window { start: 2, end: 40 },
        1,
        frictionless(),
    );
    assert_eq!(run.returns.len(), 38);
    assert_eq!(run.returns[0], 0.0, "the first bar only opens the position");
    assert!(run.returns[1] > 0.0 && run.returns[2] < 0.0);
    assert_eq!(
        run.confidences.len(),
        37,
        "the final decision has no outcome"
    );
    let score = score(run);
    assert_eq!(score.calibration_observations, 37);
    assert_eq!(
        score.calibration_brier,
        Some(0.0),
        "a perfect next-bar forecaster must be graded against the bar it forecast"
    );
}

/// Alternates between a stated order, an unstated order and a hold.
struct Mixed {
    decided: usize,
}
impl Agent for Mixed {
    fn decide(&mut self, obs: &MarketObservation) -> Decision {
        self.decided += 1;
        match self.decided % 3 {
            0 => decision(&[wire_order(obs, 0, 0.4, r#","confidence":0.7"#)]),
            1 => decision(&[wire_order(obs, 1, 0.2, "")]),
            _ => decision(&[]),
        }
    }
}

#[test]
fn replay_from_serialized_trajectory_reproduces_the_pairs() {
    let data = Dataset::synthetic(2, 90, 5);
    let window = Window { start: 20, end: 90 };
    let costs = CostModel::default();
    let (direct, trajectory) =
        run_backtest_capture(&data, &mut Mixed { decided: 0 }, window, 4, costs);
    let bytes = serde_json::to_string(&trajectory).unwrap();
    assert!(
        bytes.contains(r#""target_weight":0.2,"rationale""#),
        "the unstated order must be captured without a confidence"
    );
    let restored = serde_json::from_str(&bytes).unwrap();
    let replayed = replay_run(&data, &restored, costs);
    assert_eq!(
        serde_json::to_string(&direct).unwrap(),
        serde_json::to_string(&replayed).unwrap(),
        "replay from the serialized artifact must reproduce the captured run"
    );
    assert_eq!(direct.confidences, vec![0.7; direct.confidences.len()]);
    assert_eq!(direct.confidences.len(), direct.outcomes.len());
    assert!(!direct.confidences.is_empty());
}
