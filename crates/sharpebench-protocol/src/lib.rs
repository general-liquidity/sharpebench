//! The language-agnostic agent ⇄ harness protocol.
//!
//! Agents are **external** — a container or HTTP endpoint, in any language — not
//! Rust code. Each decision step the harness sends a [`MarketObservation`] (JSON)
//! and the agent replies with a [`Decision`] (JSON). Keeping this surface tiny and
//! stable is what lets any vendor compete (and is the whole adoption story).
//!
//! All observations are **point-in-time**: `close_history`, `fundamentals` and
//! `news` only ever contain information available at or before `date`.
#![forbid(unsafe_code)]

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// What the agent sees at one decision point.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MarketObservation {
    /// ISO-8601 date of the decision point.
    pub date: String,
    pub cash: f64,
    pub symbols: Vec<SymbolSnapshot>,
    pub portfolio: Vec<PositionState>,
}

/// Point-in-time data for one instrument.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SymbolSnapshot {
    pub symbol: String,
    /// Trailing closes up to and including `date` (oldest first).
    pub close_history: Vec<f64>,
    /// Named fundamental fields (e.g. `pe`, `revenue_yoy`). Empty if unavailable.
    #[serde(default)]
    pub fundamentals: BTreeMap<String, f64>,
    /// Headlines published on or before `date`.
    #[serde(default)]
    pub news: Vec<String>,
}

/// The agent's current holding in one instrument.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PositionState {
    pub symbol: String,
    pub shares: f64,
    pub avg_price: f64,
}

/// What the agent returns.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Decision {
    pub orders: Vec<Order>,
    /// Free-text rationale, captured into the trajectory for auditability.
    #[serde(default)]
    pub reasoning: String,
}

/// A single per-instrument instruction.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Order {
    pub symbol: String,
    pub action: Action,
    /// Target portfolio weight for this symbol in [0, 1] (signed for shorts).
    pub target_weight: f64,
    /// Stated conviction in [0, 1]; scored for calibration.
    #[serde(default = "default_confidence")]
    pub confidence: f64,
    /// Optional one-line rationale for *this* order, captured into the run trace
    /// (audit trail). Defaults to empty so existing agents need no change.
    #[serde(default)]
    pub rationale: String,
}

/// Discrete action label (sizing is carried by `target_weight`).
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Action {
    Buy,
    Sell,
    Hold,
    Close,
}

fn default_confidence() -> f64 {
    0.5
}

/// One captured decision step of a single backtest run: the agent's *raw* output
/// at one point-in-time observation. This is the persisted artifact — it holds the
/// agent's [`Decision`] (orders, sizing, conviction, reasoning) tagged with the
/// observation it was made against, and deliberately stores **no** returns, NAV, or
/// any self-reported metric. The score is recomputed by replaying these decisions
/// through the engine, never read from the agent's word.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DecisionStep {
    /// 0-based step index within the run's window (`window.start + step` is the
    /// dataset index the observation was drawn from).
    pub step: usize,
    /// Stable id of the point-in-time observation this decision answered — the
    /// observation's ISO date. Lets a verifier confirm the decision lines up with
    /// the frozen dataset's bar at the replayed step.
    pub observation_id: String,
    /// The agent's raw decision at this step (orders + reasoning).
    pub decision: Decision,
}

/// One captured backtest run (a single window × seed): the ordered sequence of the
/// agent's raw decision steps, plus the (window, seed) coordinates needed to replay
/// it through the identical point-in-time engine path.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RunTrajectory {
    /// Inclusive window start (dataset index of the first decision step).
    pub window_start: usize,
    /// Exclusive window end.
    pub window_end: usize,
    /// Execution seed the run was driven with (governs slippage noise on replay).
    pub seed: u64,
    /// The raw decisions, in step order.
    pub steps: Vec<DecisionStep>,
}

/// An agent's full captured trajectory: every (window × seed) run's raw decisions.
/// Serde-(de)serializable to JSON; this is the on-disk artifact a separate verifier
/// ingests to recompute the score from raw decisions alone.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AgentTrajectory {
    pub agent_id: String,
    /// In-sample search budget the agent declared (mirrors `AgentSubmission`), so a
    /// recomputed submission carries the same deflation footprint.
    #[serde(default)]
    pub in_sample_trials: u32,
    /// One captured run per (window, seed), in the same order the harness produced
    /// them (window-major: all seeds of window 0, then window 1, …).
    pub runs: Vec<RunTrajectory>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn observation_and_decision_roundtrip() {
        let obs = MarketObservation {
            date: "2025-01-01".to_string(),
            cash: 1.0,
            symbols: vec![SymbolSnapshot {
                symbol: "A".to_string(),
                close_history: vec![1.0, 2.0],
                fundamentals: Default::default(),
                news: vec!["headline".to_string()],
            }],
            portfolio: vec![PositionState {
                symbol: "A".to_string(),
                shares: 1.0,
                avg_price: 2.0,
            }],
        };
        let back: MarketObservation =
            serde_json::from_str(&serde_json::to_string(&obs).unwrap()).unwrap();
        assert_eq!(back.symbols[0].symbol, "A");

        let d = Decision {
            orders: vec![Order {
                symbol: "A".to_string(),
                action: Action::Buy,
                target_weight: 0.5,
                confidence: 0.9,
                rationale: "trailing breakout".to_string(),
            }],
            reasoning: "r".to_string(),
        };
        let db: Decision = serde_json::from_str(&serde_json::to_string(&d).unwrap()).unwrap();
        assert_eq!(db.orders[0].action, Action::Buy);
        // The per-order rationale survives the JSON round-trip into the trajectory.
        assert_eq!(db.orders[0].rationale, "trailing breakout");

        // Older agents that omit `rationale` still deserialize (default empty).
        let legacy = r#"{"orders":[{"symbol":"A","action":"buy","target_weight":0.5}]}"#;
        let parsed: Decision = serde_json::from_str(legacy).unwrap();
        assert_eq!(parsed.orders[0].rationale, "");
        assert!((parsed.orders[0].confidence - 0.5).abs() < 1e-12);
    }

    #[test]
    fn trajectory_roundtrips_through_json() {
        let traj = AgentTrajectory {
            agent_id: "a".to_string(),
            in_sample_trials: 7,
            runs: vec![RunTrajectory {
                window_start: 20,
                window_end: 30,
                seed: 3,
                steps: vec![DecisionStep {
                    step: 0,
                    observation_id: "2025-001".to_string(),
                    decision: Decision {
                        orders: vec![Order {
                            symbol: "A".to_string(),
                            action: Action::Buy,
                            target_weight: 0.25,
                            confidence: 0.8,
                            rationale: String::new(),
                        }],
                        reasoning: "r".to_string(),
                    },
                }],
            }],
        };
        let back: AgentTrajectory =
            serde_json::from_str(&serde_json::to_string(&traj).unwrap()).unwrap();
        assert_eq!(back.agent_id, "a");
        assert_eq!(back.in_sample_trials, 7);
        assert_eq!(back.runs[0].seed, 3);
        assert_eq!(back.runs[0].steps[0].observation_id, "2025-001");
        assert_eq!(back.runs[0].steps[0].decision.orders[0].target_weight, 0.25);
    }
}
