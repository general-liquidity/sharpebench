//! The point-in-time backtest engine.

use std::collections::BTreeMap;

use sb_core::{ProcessEvent, Run, Trace};
use sb_protocol::{MarketObservation, PositionState, SymbolSnapshot};

use crate::agent::Agent;
use crate::costs::{market_impact_frac, CostModel, Rng};
use crate::data::Dataset;

const LOOKBACK: usize = 20;
/// Per-name weight above which we record a (warn-severity) concentration breach.
const CONCENTRATION_CAP: f64 = 0.5;
/// Per-name weight beyond which (or if non-finite) an order is treated as a
/// simulator-exploitation attempt — a block-severity violation.
const HARD_WEIGHT_CAP: f64 = 5.0;

/// A simulation window over the dataset's date axis: steps `[start, end)`.
#[derive(Clone, Copy, Debug)]
pub struct Window {
    pub start: usize,
    pub end: usize,
}

fn price(data: &Dataset, symbol: &str, t: usize) -> f64 {
    data.close_at(symbol, t).unwrap_or(0.0)
}

fn nav(
    data: &Dataset,
    symbols: &[String],
    shares: &BTreeMap<String, f64>,
    cash: f64,
    t: usize,
) -> f64 {
    cash + symbols
        .iter()
        .map(|s| shares[s] * price(data, s, t))
        .sum::<f64>()
}

/// Run a single backtest of `agent` over `window` with seeded execution noise,
/// returning an [`sb_core::Run`] (per-period returns + decision trace).
pub fn run_backtest(
    data: &Dataset,
    agent: &mut dyn Agent,
    window: Window,
    seed: u64,
    costs: CostModel,
) -> Run {
    let symbols = data.symbols();
    let mut shares: BTreeMap<String, f64> = symbols.iter().map(|s| (s.clone(), 0.0)).collect();
    let mut cash = 1.0_f64;
    let mut rng = Rng::new(seed);
    let mut trace = Trace::default();
    let mut returns: Vec<f64> = Vec::new();
    let mut confidences: Vec<f64> = Vec::new();
    let mut outcomes: Vec<bool> = Vec::new();

    let end = window.end.min(data.len());
    if window.start >= end {
        return Run {
            returns,
            trace,
            confidences,
            outcomes,
            cost: 0.0,
        };
    }

    let mut prev_nav = 1.0_f64;

    for t in window.start..end {
        // 1) point-in-time observation (no bar after `t` is reachable).
        let snap: Vec<SymbolSnapshot> = symbols
            .iter()
            .map(|s| SymbolSnapshot {
                symbol: s.clone(),
                close_history: data.history(s, t, LOOKBACK),
                fundamentals: BTreeMap::new(),
                news: Vec::new(),
            })
            .collect();
        let portfolio: Vec<PositionState> = symbols
            .iter()
            .map(|s| PositionState {
                symbol: s.clone(),
                shares: shares[s],
                avg_price: 0.0,
            })
            .collect();
        let obs = MarketObservation {
            date: data.dates[t].clone(),
            cash,
            symbols: snap,
            portfolio,
        };

        // 2) agent decides.
        let decision = agent.decide(&obs);
        let cur_nav = nav(data, &symbols, &shares, cash, t);

        // 3) rebalance toward target weights with cost + seeded slippage.
        for ord in &decision.orders {
            let p = price(data, &ord.symbol, t);
            if p <= 0.0 {
                continue;
            }
            // Sim-exploitation guard: non-finite or absurd weights are gaming attempts.
            if !ord.target_weight.is_finite() || ord.target_weight.abs() > HARD_WEIGHT_CAP {
                trace.events.push(ProcessEvent::ManipulativeOrder);
                continue;
            }
            if ord.target_weight.abs() > CONCENTRATION_CAP {
                trace.events.push(ProcessEvent::ConcentrationBreach);
            }
            let target_value = ord.target_weight.max(0.0) * cur_nav;
            let cur_value = shares[&ord.symbol] * p;
            let delta_value = target_value - cur_value;
            if delta_value.abs() < 1e-9 {
                continue;
            }
            // Base seeded slippage plus own-order market impact: the bigger the
            // trade relative to NAV, the more the fill moves against the agent.
            let participation = delta_value.abs() / cur_nav.max(1e-9);
            let slip = (costs.slippage_bps + rng.signed_unit().abs() * costs.slippage_bps)
                / 10_000.0
                + market_impact_frac(costs.impact_bps, participation);
            let exec_p = if delta_value > 0.0 {
                p * (1.0 + slip)
            } else {
                p * (1.0 - slip)
            };
            let dshares = delta_value / exec_p;
            let fee = delta_value.abs() * (costs.fee_bps / 10_000.0);
            if let Some(sh) = shares.get_mut(&ord.symbol) {
                *sh += dshares;
            }
            cash -= dshares * exec_p + fee;
            trace.events.push(ProcessEvent::OrderPlaced {
                risk_gate_passed: true,
            });
        }

        // 4) daily return = post-trade NAV vs the prior step's NAV (captures both
        //    the price move on held positions and today's trading costs).
        let navc = nav(data, &symbols, &shares, cash, t);
        let ret = if prev_nav.abs() > 1e-12 {
            navc / prev_nav - 1.0
        } else {
            0.0
        };
        returns.push(ret);
        // Capture the decision's stated conviction and whether the step paid off,
        // so the scoring kernel's calibration axis is fed from the live run.
        let avg_conf = if decision.orders.is_empty() {
            0.5
        } else {
            decision.orders.iter().map(|o| o.confidence).sum::<f64>() / decision.orders.len() as f64
        };
        confidences.push(avg_conf);
        outcomes.push(ret > 0.0);
        prev_nav = navc;
    }

    Run {
        returns,
        trace,
        confidences,
        outcomes,
        cost: 0.0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::BuyAndHold;

    #[test]
    fn backtest_produces_returns_and_trace() {
        let data = Dataset::synthetic(4, 120, 11);
        let mut agent = BuyAndHold;
        let run = run_backtest(
            &data,
            &mut agent,
            Window {
                start: 20,
                end: 120,
            },
            1,
            CostModel::default(),
        );
        assert_eq!(run.returns.len(), 100);
        assert!(!run.trace.events.is_empty());
    }

    #[test]
    fn different_seeds_diverge() {
        let data = Dataset::synthetic(4, 120, 11);
        let w = Window {
            start: 20,
            end: 120,
        };
        let a = run_backtest(&data, &mut BuyAndHold, w, 1, CostModel::default());
        let b = run_backtest(&data, &mut BuyAndHold, w, 2, CostModel::default());
        assert_ne!(a.returns, b.returns, "execution seed should vary returns");
    }
}
