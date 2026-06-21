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
            }],
            reasoning: "r".to_string(),
        };
        let db: Decision = serde_json::from_str(&serde_json::to_string(&d).unwrap()).unwrap();
        assert_eq!(db.orders[0].action, Action::Buy);
    }
}
