//! In-process trading agents.
//!
//! External agents speak the JSON [`sharpebench_protocol`] over a container/HTTP boundary;
//! this trait is the in-process equivalent used for reference agents and tests.

use std::collections::BTreeMap;

use sharpebench_protocol::{Action, Decision, MarketObservation, Order};

/// Something that turns a point-in-time observation into trading orders.
pub trait Agent {
    fn decide(&mut self, obs: &MarketObservation) -> Decision;
}

/// A trading *team*: several member agents whose target weights are averaged into
/// one consensus decision (a symbol only one member likes is down-weighted by the
/// whole team's size). Modelled on the TradingAgents multi-agent firm — the team
/// is scored as a unit while [`sharpebench_core::attribute_roles`] estimates each member's
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

/// The do-nothing agent: always holds (empty orders). A trivial baseline, and the
/// graceful fallback when an external agent process can't be spawned mid-run —
/// consistent with how the external transports already degrade to a hold on error.
pub struct HoldAgent;

impl Agent for HoldAgent {
    fn decide(&mut self, _obs: &MarketObservation) -> Decision {
        Decision {
            orders: Vec::new(),
            reasoning: "hold".to_string(),
        }
    }
}

/// A coin-flip "monkey": a fully-invested, long-only portfolio with random
/// weights each step. Seeded so it is reproducible. Run many of these to draw the
/// **luck floor** — the distribution of outcomes from zero skill that a genuine
/// agent must clear to be rank-eligible.
pub struct RandomAgent {
    rng: crate::costs::Rng,
}

impl RandomAgent {
    pub fn new(seed: u64) -> Self {
        Self {
            rng: crate::costs::Rng::new(seed ^ 0x1AC4_0000_2026_0000),
        }
    }
}

impl Agent for RandomAgent {
    fn decide(&mut self, obs: &MarketObservation) -> Decision {
        let raws: Vec<f64> = obs.symbols.iter().map(|_| self.rng.unit()).collect();
        let total: f64 = raws.iter().sum();
        let orders = obs
            .symbols
            .iter()
            .zip(&raws)
            .map(|(s, &r)| {
                let w = if total > 0.0 { r / total } else { 0.0 };
                Order {
                    symbol: s.symbol.clone(),
                    action: if w > 0.0 { Action::Buy } else { Action::Close },
                    target_weight: w,
                    confidence: 0.5,
                }
            })
            .collect();
        Decision {
            orders,
            reasoning: "random allocation (luck floor)".to_string(),
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
