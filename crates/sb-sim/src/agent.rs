//! In-process trading agents.
//!
//! External agents speak the JSON [`sb_protocol`] over a container/HTTP boundary;
//! this trait is the in-process equivalent used for reference agents and tests.

use sb_protocol::{Action, Decision, MarketObservation, Order};

/// Something that turns a point-in-time observation into trading orders.
pub trait Agent {
    fn decide(&mut self, obs: &MarketObservation) -> Decision;
}

/// Equal-weight buy-and-hold across all symbols — the baseline every agent must beat.
pub struct BuyAndHold;

impl Agent for BuyAndHold {
    fn decide(&mut self, obs: &MarketObservation) -> Decision {
        let n = obs.symbols.len().max(1) as f64;
        let w = 1.0 / n;
        let orders = obs
            .symbols
            .iter()
            .map(|s| Order {
                symbol: s.symbol.clone(),
                action: Action::Buy,
                target_weight: w,
                confidence: 0.5,
            })
            .collect();
        Decision {
            orders,
            reasoning: "equal-weight buy-and-hold".to_string(),
        }
    }
}

/// Cross-sectional momentum: equal-weight the symbols with positive trailing return.
pub struct Momentum {
    pub lookback: usize,
}

impl Default for Momentum {
    fn default() -> Self {
        Self { lookback: 10 }
    }
}

impl Agent for Momentum {
    fn decide(&mut self, obs: &MarketObservation) -> Decision {
        let scores: Vec<(String, f64)> = obs
            .symbols
            .iter()
            .map(|s| {
                let h = &s.close_history;
                let score = if h.len() >= 2 && h[0] > 0.0 {
                    h[h.len() - 1] / h[0] - 1.0
                } else {
                    0.0
                };
                (s.symbol.clone(), score)
            })
            .collect();

        let n_winners = scores.iter().filter(|(_, sc)| *sc > 0.0).count();
        let w = if n_winners > 0 {
            1.0 / n_winners as f64
        } else {
            0.0
        };

        let orders = scores
            .iter()
            .map(|(sym, sc)| {
                let positive = *sc > 0.0;
                Order {
                    symbol: sym.clone(),
                    action: if positive { Action::Buy } else { Action::Close },
                    target_weight: if positive { w } else { 0.0 },
                    confidence: (0.5 + sc.abs()).min(1.0),
                }
            })
            .collect();

        Decision {
            orders,
            reasoning: "cross-sectional momentum".to_string(),
        }
    }
}
