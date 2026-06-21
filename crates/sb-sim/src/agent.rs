//! In-process trading agents.
//!
//! External agents speak the JSON [`sb_protocol`] over a container/HTTP boundary;
//! this trait is the in-process equivalent used for reference agents and tests.

use std::collections::BTreeMap;

use sb_protocol::{Action, Decision, MarketObservation, Order};

/// Something that turns a point-in-time observation into trading orders.
pub trait Agent {
    fn decide(&mut self, obs: &MarketObservation) -> Decision;
}

/// A trading *team*: several member agents whose target weights are averaged into
/// one consensus decision (a symbol only one member likes is down-weighted by the
/// whole team's size). Modelled on the TradingAgents multi-agent firm — the team
/// is scored as a unit while [`sb_core::attribute_roles`] estimates each member's
/// load on the team outcome.
pub struct TeamAgent {
    pub members: Vec<Box<dyn Agent>>,
}

impl Agent for TeamAgent {
    fn decide(&mut self, obs: &MarketObservation) -> Decision {
        let n = self.members.len().max(1) as f64;
        let mut weight: BTreeMap<String, f64> = BTreeMap::new();
        let mut conf: BTreeMap<String, f64> = BTreeMap::new();
        let mut votes: BTreeMap<String, f64> = BTreeMap::new();
        for m in self.members.iter_mut() {
            for o in m.decide(obs).orders {
                *weight.entry(o.symbol.clone()).or_default() += o.target_weight;
                *conf.entry(o.symbol.clone()).or_default() += o.confidence;
                *votes.entry(o.symbol).or_default() += 1.0;
            }
        }
        let orders = weight
            .iter()
            .map(|(sym, &w)| {
                let avg_w = (w / n).max(0.0);
                Order {
                    symbol: sym.clone(),
                    action: if avg_w > 0.0 {
                        Action::Buy
                    } else {
                        Action::Close
                    },
                    target_weight: avg_w,
                    confidence: conf[sym] / votes[sym].max(1.0),
                }
            })
            .collect();
        Decision {
            orders,
            reasoning: "team consensus (mean target weight)".to_string(),
        }
    }
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
