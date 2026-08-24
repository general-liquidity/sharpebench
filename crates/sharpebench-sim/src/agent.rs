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
                    rationale: format!("team consensus weight {avg_w:.3}"),
                }
            })
            .collect();
        Decision {
            orders,
            reasoning: "team consensus (mean target weight)".to_string(),
            cost: None,
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
                rationale: "equal-weight hold".to_string(),
            })
            .collect();
        Decision {
            orders,
            reasoning: "equal-weight buy-and-hold".to_string(),
            cost: None,
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
            cost: None,
        }
    }
}

/// A coin-flip "monkey": a long-only portfolio with random weights each step —
/// fully invested on multi-symbol universes, random gross exposure (flat or a
/// uniform long weight) on one-symbol universes, where full investment would
/// degenerate into buy-and-hold. Seeded so it is reproducible. Run many of these
/// to draw the **luck floor** — the distribution of outcomes from zero skill
/// that a genuine agent must clear to be rank-eligible.
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
        let single = obs.symbols.len() == 1;
        let orders = obs
            .symbols
            .iter()
            .zip(&raws)
            .map(|(s, &r)| {
                // On a one-symbol universe, fully-invested normalized weights are
                // deterministically r / r = 1.0 — every seed collapses into
                // buy-and-hold and the luck floor is no floor at all. Draw the
                // gross exposure itself instead: flat half the time, otherwise a
                // uniform long weight, so zero-skill outcomes genuinely vary.
                let w = if single {
                    if r < 0.5 {
                        0.0
                    } else {
                        (r - 0.5) * 2.0
                    }
                } else if total > 0.0 {
                    r / total
                } else {
                    0.0
                };
                Order {
                    symbol: s.symbol.clone(),
                    action: if w > 0.0 { Action::Buy } else { Action::Close },
                    target_weight: w,
                    confidence: 0.5,
                    rationale: "random allocation".to_string(),
                }
            })
            .collect();
        Decision {
            orders,
            reasoning: "random allocation (luck floor)".to_string(),
            cost: None,
        }
    }
}

/// A **risk-managed** reference agent: textbook trend filter + inverse-volatility
/// sizing + a drawdown halt, composed exactly as a risk-management primer would.
/// This is deliberately *not* alpha — it is a reference implementation of standard
/// risk discipline, built so the eligibility gates are exercised by at least one
/// entrant that is not unhedged long-only (the methodology paper's first stated
/// limitation). Everything is computed from the observation history alone
/// (point-in-time by construction), fully deterministic, no RNG.
///
/// Rules, in order, each bar:
/// 1. **Trend filter** — build an equal-weight basket index from the trailing
///    common history (each symbol normalized to its first trailing close). The
///    filter is positive when the latest basket value is strictly above its
///    trailing `trend_lookback`-bar mean (default **15**; the engine's
///    observation carries a 20-bar trailing window, and the agent needs
///    `lookback + 1` bars, so the default must fit inside that).
/// 2. **Inverse-vol sizing** — gross exposure = `target_vol / realized_vol`,
///    capped at **1.0**, where `realized_vol` is the standard deviation of the
///    basket's last `vol_lookback` bar-to-bar returns (default **15**) and
///    `target_vol` is a per-bar volatility target (default **0.01**, roughly 16%
///    annualized on daily bars). Zero realized vol sizes at the cap.
/// 3. **Drawdown halt** — track own equity (cash + marked positions) from the
///    observation; when it falls more than `max_drawdown` (default **0.10**)
///    below its running peak, go fully flat and stay flat until the trend filter
///    *turns* positive again (a fresh negative-to-positive transition, not merely
///    an already-positive reading). Re-entry resets the equity peak.
///
/// When the trend filter is negative, or the agent is halted, or the trailing
/// history is shorter than the lookbacks require, the agent is fully flat.
pub struct RiskManaged {
    pub trend_lookback: usize,
    pub vol_lookback: usize,
    /// Per-bar realized-volatility target for inverse-vol sizing.
    pub target_vol: f64,
    /// Peak-to-trough fraction of own equity beyond which the agent halts.
    pub max_drawdown: f64,
    peak_equity: f64,
    halted: bool,
    prev_trend_positive: bool,
}

impl Default for RiskManaged {
    fn default() -> Self {
        Self {
            trend_lookback: 15,
            vol_lookback: 15,
            target_vol: 0.01,
            max_drawdown: 0.10,
            peak_equity: f64::NAN,
            halted: false,
            prev_trend_positive: false,
        }
    }
}

impl RiskManaged {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_config(
        trend_lookback: usize,
        vol_lookback: usize,
        target_vol: f64,
        max_drawdown: f64,
    ) -> Self {
        Self {
            trend_lookback,
            vol_lookback,
            target_vol,
            max_drawdown,
            ..Self::default()
        }
    }
}

impl Agent for RiskManaged {
    fn decide(&mut self, obs: &MarketObservation) -> Decision {
        // Own equity, marked from the observation's point-in-time closes.
        let mut equity = obs.cash;
        for p in &obs.portfolio {
            if let Some(s) = obs.symbols.iter().find(|s| s.symbol == p.symbol) {
                if let Some(&px) = s.close_history.last() {
                    equity += p.shares * px;
                }
            }
        }
        if !self.peak_equity.is_finite() {
            self.peak_equity = equity;
        }

        // Equal-weight basket over the trailing history common to every symbol.
        let min_len = obs
            .symbols
            .iter()
            .map(|s| s.close_history.len())
            .min()
            .unwrap_or(0);
        let need = self.trend_lookback.max(self.vol_lookback) + 1;
        let mut trend_positive = false;
        let mut gross = 0.0;
        if !obs.symbols.is_empty() && min_len >= need {
            let n_sym = obs.symbols.len() as f64;
            let basket: Vec<f64> = (0..min_len)
                .map(|t| {
                    obs.symbols
                        .iter()
                        .map(|s| {
                            let h = &s.close_history;
                            let tail = &h[h.len() - min_len..];
                            if tail[0] > 0.0 {
                                tail[t] / tail[0]
                            } else {
                                1.0
                            }
                        })
                        .sum::<f64>()
                        / n_sym
                })
                .collect();
            let last = *basket.last().expect("min_len >= need > 0");
            let mean = basket[basket.len() - self.trend_lookback..]
                .iter()
                .sum::<f64>()
                / self.trend_lookback as f64;
            trend_positive = last > mean;

            let rets: Vec<f64> = basket
                .windows(2)
                .map(|w| if w[0] > 0.0 { w[1] / w[0] - 1.0 } else { 0.0 })
                .collect();
            let tail = &rets[rets.len() - self.vol_lookback..];
            let m = tail.iter().sum::<f64>() / tail.len() as f64;
            let var = tail.iter().map(|r| (r - m) * (r - m)).sum::<f64>() / tail.len() as f64;
            let vol = var.sqrt();
            gross = if vol > 0.0 {
                (self.target_vol / vol).min(1.0)
            } else {
                1.0
            };
        }

        // Drawdown halt on own equity; re-enter only on a fresh trend turn.
        if !self.halted {
            if equity > self.peak_equity {
                self.peak_equity = equity;
            }
            if self.peak_equity > 0.0 && equity < self.peak_equity * (1.0 - self.max_drawdown) {
                self.halted = true;
            }
        } else if trend_positive && !self.prev_trend_positive {
            self.halted = false;
            self.peak_equity = equity;
        }
        self.prev_trend_positive = trend_positive;

        let invested = trend_positive && !self.halted && gross > 0.0;
        let w = if invested {
            gross / obs.symbols.len().max(1) as f64
        } else {
            0.0
        };
        let orders = obs
            .symbols
            .iter()
            .map(|s| Order {
                symbol: s.symbol.clone(),
                action: if invested { Action::Buy } else { Action::Close },
                target_weight: w,
                confidence: 0.5,
                rationale: if invested {
                    format!("trend up, vol-scaled gross {gross:.3}")
                } else if self.halted {
                    "drawdown halt".to_string()
                } else {
                    "trend filter negative or warming up".to_string()
                },
            })
            .collect();
        Decision {
            orders,
            reasoning: "risk-managed trend + inverse-vol + drawdown halt".to_string(),
            cost: None,
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
                    rationale: if positive {
                        format!("positive trailing return {sc:.3}")
                    } else {
                        "non-positive trailing return".to_string()
                    },
                }
            })
            .collect();

        Decision {
            orders,
            reasoning: "cross-sectional momentum".to_string(),
            cost: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sharpebench_protocol::{MarketObservation, PositionState, SymbolSnapshot};

    fn obs(histories: &[(&str, Vec<f64>)], cash: f64) -> MarketObservation {
        let symbols: Vec<SymbolSnapshot> = histories
            .iter()
            .map(|(sym, h)| SymbolSnapshot {
                symbol: sym.to_string(),
                close_history: h.clone(),
                fundamentals: BTreeMap::new(),
                news: Vec::new(),
            })
            .collect();
        let portfolio = symbols
            .iter()
            .map(|s| PositionState {
                symbol: s.symbol.clone(),
                shares: 0.0,
                avg_price: 0.0,
            })
            .collect();
        MarketObservation {
            date: "t".to_string(),
            cash,
            symbols,
            portfolio,
        }
    }

    fn geometric(start: f64, per_bar: f64, n: usize) -> Vec<f64> {
        (0..n)
            .map(|t| start * (1.0 + per_bar).powi(t as i32))
            .collect()
    }

    fn gross_of(d: &Decision) -> f64 {
        d.orders.iter().map(|o| o.target_weight).sum()
    }

    #[test]
    fn risk_managed_is_flat_in_a_downtrend() {
        let mut a = RiskManaged::default();
        let d = a.decide(&obs(&[("A", geometric(100.0, -0.01, 30))], 100.0));
        assert_eq!(gross_of(&d), 0.0, "downtrend must be fully flat");
        assert!(d.orders.iter().all(|o| matches!(o.action, Action::Close)));
    }

    #[test]
    fn risk_managed_is_flat_during_warmup() {
        let mut a = RiskManaged::default();
        // 10 bars of strong uptrend: shorter than the lookbacks require.
        let d = a.decide(&obs(&[("A", geometric(100.0, 0.02, 10))], 100.0));
        assert_eq!(gross_of(&d), 0.0, "insufficient history must be flat");
    }

    #[test]
    fn risk_managed_scales_gross_down_with_volatility() {
        // Calm uptrend: constant +1% per bar, zero realized vol → full gross.
        let mut calm = RiskManaged::default();
        let g_calm = gross_of(&calm.decide(&obs(&[("A", geometric(100.0, 0.01, 30))], 100.0)));
        assert!((g_calm - 1.0).abs() < 1e-12, "zero vol sizes at the cap");

        // Volatile uptrend: +10% / -6% alternating (std 0.08 per bar) → gross
        // targeted at 0.01 / 0.08 = 0.125.
        let mut px = 100.0;
        let noisy: Vec<f64> = (0..30)
            .map(|t| {
                px *= if t % 2 == 0 { 1.10 } else { 0.94 };
                px
            })
            .collect();
        let mut hot = RiskManaged::default();
        let g_hot = gross_of(&hot.decide(&obs(&[("A", noisy)], 100.0)));
        assert!(g_hot > 0.0, "uptrend must be invested");
        assert!(
            g_hot < 0.2,
            "high vol must scale gross well below the cap: {g_hot}"
        );
        assert!(g_hot < g_calm);
    }

    #[test]
    fn risk_managed_halts_on_drawdown_and_reenters_on_fresh_trend() {
        let up = geometric(100.0, 0.01, 30);
        let down = geometric(100.0, -0.01, 30);
        let mut a = RiskManaged::default();

        // Invested in the uptrend; equity peak 100.
        let d1 = a.decide(&obs(&[("A", up.clone())], 100.0));
        assert!(gross_of(&d1) > 0.0);

        // Equity drops 15% (beyond the 10% default halt) while the trend is
        // still positive → fully flat.
        let d2 = a.decide(&obs(&[("A", up.clone())], 85.0));
        assert_eq!(gross_of(&d2), 0.0, "drawdown halt must go flat");

        // Trend still positive on the next bar: no fresh turn, still halted.
        let d3 = a.decide(&obs(&[("A", up.clone())], 85.0));
        assert_eq!(
            gross_of(&d3),
            0.0,
            "an already-positive trend is not a re-entry"
        );

        // Trend turns negative, then positive again → re-enter.
        let d4 = a.decide(&obs(&[("A", down)], 85.0));
        assert_eq!(gross_of(&d4), 0.0);
        let d5 = a.decide(&obs(&[("A", up)], 85.0));
        assert!(gross_of(&d5) > 0.0, "fresh trend turn must re-enter");
    }

    #[test]
    fn risk_managed_is_deterministic() {
        let seq = [
            obs(&[("A", geometric(100.0, 0.01, 30))], 100.0),
            obs(&[("A", geometric(100.0, 0.005, 30))], 95.0),
            obs(&[("A", geometric(100.0, -0.01, 30))], 90.0),
            obs(&[("A", geometric(100.0, 0.02, 30))], 92.0),
        ];
        let mut a = RiskManaged::default();
        let mut b = RiskManaged::default();
        for o in &seq {
            let da = a.decide(o);
            let db = b.decide(o);
            assert_eq!(format!("{da:?}"), format!("{db:?}"));
        }
    }

    /// Regression for the single-symbol luck-floor degeneracy: fully-invested
    /// normalized weights on a one-symbol universe are deterministically 1.0,
    /// which made every luck-floor agent a bit-identical clone of buy-and-hold
    /// on the rates dataset. The random exposure path must vary per period and
    /// per seed.
    #[test]
    fn random_agent_varies_on_single_symbol_universe() {
        let o = obs(&[("A", geometric(100.0, 0.001, 30))], 100.0);
        let weights = |seed: u64| -> Vec<f64> {
            let mut a = RandomAgent::new(seed);
            (0..64)
                .map(|_| a.decide(&o).orders[0].target_weight)
                .collect()
        };
        let w1 = weights(1);
        let w2 = weights(2);
        assert!(
            w1.iter().any(|&w| (w - w1[0]).abs() > 1e-12),
            "single-symbol random exposure must vary across periods"
        );
        assert_ne!(w1, w2, "different seeds must produce different exposure");
        assert!(w1.iter().chain(&w2).all(|&w| (0.0..=1.0).contains(&w)));
        assert!(
            w1.contains(&0.0) && w1.iter().any(|&w| w > 0.0),
            "the mixture must include both flat and long periods"
        );
    }
}
