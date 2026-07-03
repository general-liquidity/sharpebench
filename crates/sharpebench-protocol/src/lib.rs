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
    /// Optional self-reported compute/token spend for producing *this* decision.
    /// The engine accumulates it into the run's `cost`, which drives the
    /// cost-normalized leaderboard columns (`return_per_cost` / `dsr_per_cost` =
    /// skill-per-dollar-of-compute). `None` = not reported, so existing agents
    /// need no change and the cost columns stay `None` (back-compat).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cost: Option<DecisionCost>,
}

/// An agent's self-reported spend to produce one decision. Every field defaults to
/// zero so a partial report (e.g. tokens only, no dollar figure) still deserializes.
/// The engine reduces this to a single scalar via [`DecisionCost::billable_units`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct DecisionCost {
    /// Dollar cost of the compute/tokens spent on this decision. The preferred
    /// unit for skill-per-dollar reporting.
    #[serde(default)]
    pub cost_usd: f64,
    /// Prompt/input tokens consumed.
    #[serde(default)]
    pub tokens_in: u64,
    /// Completion/output tokens produced.
    #[serde(default)]
    pub tokens_out: u64,
    /// Reasoning/thinking tokens, reported as a legibility breakdown. Providers
    /// typically already bill these inside `tokens_out`, so they are *not* re-added
    /// into the token total; they are surfaced separately, not double-counted.
    #[serde(default)]
    pub reasoning_tokens: u64,
}

impl DecisionCost {
    /// The single scalar the engine folds into `Run.cost` (any consistent unit,
    /// matching the leaderboard's cost column). Prefers the reported dollar figure;
    /// with no dollars reported it falls back to total billable tokens
    /// (`tokens_in + tokens_out`). Reasoning tokens are a sub-breakdown of the
    /// output and are not added again.
    pub fn billable_units(&self) -> f64 {
        if self.cost_usd > 0.0 {
            self.cost_usd
        } else {
            (self.tokens_in + self.tokens_out) as f64
        }
    }
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
            cost: None,
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
        // A legacy decision omits `cost` entirely (back-compat → None).
        assert!(parsed.cost.is_none());
    }

    #[test]
    fn decision_cost_channel_parses_and_reduces() {
        // An agent self-reporting spend: dollars present → billable = dollars.
        let with_cost = r#"{"orders":[],"reasoning":"","cost":{"cost_usd":0.42,
            "tokens_in":1200,"tokens_out":300,"reasoning_tokens":180}}"#;
        let d: Decision = serde_json::from_str(with_cost).unwrap();
        let c = d.cost.expect("cost channel present");
        assert!((c.cost_usd - 0.42).abs() < 1e-12);
        assert_eq!(c.tokens_in, 1200);
        assert!((c.billable_units() - 0.42).abs() < 1e-12);

        // Tokens-only report (no dollars) → billable = tokens_in + tokens_out;
        // reasoning tokens are a sub-breakdown of the output, not re-added.
        let tokens_only = DecisionCost {
            cost_usd: 0.0,
            tokens_in: 1000,
            tokens_out: 250,
            reasoning_tokens: 200,
        };
        assert!((tokens_only.billable_units() - 1250.0).abs() < 1e-12);

        // `cost` round-trips through JSON.
        let d2 = Decision {
            orders: Vec::new(),
            reasoning: String::new(),
            cost: Some(tokens_only),
        };
        let back: Decision = serde_json::from_str(&serde_json::to_string(&d2).unwrap()).unwrap();
        assert_eq!(back.cost, Some(tokens_only));
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
                        cost: None,
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
