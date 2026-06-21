//! The point-in-time backtest engine.

use std::collections::BTreeMap;

use sb_core::{ProcessEvent, Run, Trace};
use sb_protocol::{MarketObservation, PositionState, SymbolSnapshot};

use crate::agent::Agent;
use crate::costs::{liquidity_capped_delta, market_impact_frac, CostModel, Rng};
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
            // Liquidity cap: a trade larger than the per-step participation limit
            // only partially fills; the rest is left for later steps.
            let delta_value =
                liquidity_capped_delta(target_value - cur_value, costs.max_participation, cur_nav);
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

        // 4) corporate actions: credit cash dividends on post-trade holdings.
        for s in &symbols {
            let div = data.dividend_at(s, t);
            if div != 0.0 {
                cash += shares[s] * div;
            }
        }

        // 5) financing: charge carry on any leveraged exposure above 1× NAV.
        let positions_value: f64 = symbols.iter().map(|s| shares[s] * price(data, s, t)).sum();
        let nav_now = cash + positions_value;
        if nav_now > 1e-12 {
            let gross = positions_value / nav_now;
            cash -= crate::costs::financing_cost_frac(costs.financing_bps, gross) * nav_now;
        }

        // 6) daily return = post-trade NAV vs the prior step's NAV (captures the
        //    price move on held positions, dividends, financing, and trading costs).
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
    use crate::agent::{Agent, BuyAndHold};
    use sb_protocol::{Action, Decision, MarketObservation, Order};

    /// Test-only agent: levers 2× into the first symbol (gross exposure 2× NAV).
    struct Leveraged;
    impl Agent for Leveraged {
        fn decide(&mut self, obs: &MarketObservation) -> Decision {
            let sym = obs.symbols[0].symbol.clone();
            Decision {
                orders: vec![Order {
                    symbol: sym,
                    action: Action::Buy,
                    target_weight: 2.0,
                    confidence: 0.5,
                }],
                reasoning: "2x leverage".to_string(),
            }
        }
    }

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

    #[test]
    fn dividends_lift_buy_and_hold_return() {
        let base = Dataset::synthetic(3, 120, 11);
        let paying = base.clone().with_dividend_yield(0.001); // 10 bps/step
        let w = Window {
            start: 20,
            end: 120,
        };
        // No execution noise (zero costs) so the only difference is the dividend.
        let no_costs = CostModel {
            fee_bps: 0.0,
            slippage_bps: 0.0,
            impact_bps: 0.0,
            financing_bps: 0.0,
            max_participation: f64::INFINITY,
        };
        let plain = run_backtest(&base, &mut BuyAndHold, w, 0, no_costs);
        let div = run_backtest(&paying, &mut BuyAndHold, w, 0, no_costs);
        let sum_plain: f64 = plain.returns.iter().sum();
        let sum_div: f64 = div.returns.iter().sum();
        assert!(
            sum_div > sum_plain,
            "dividends should raise total return: {sum_div} vs {sum_plain}"
        );
    }

    #[test]
    fn financing_costs_reduce_leveraged_returns() {
        let data = Dataset::synthetic(3, 120, 11);
        let w = Window {
            start: 20,
            end: 120,
        };
        let no_fin = CostModel {
            financing_bps: 0.0,
            ..CostModel::default()
        };
        let with_fin = CostModel {
            financing_bps: 50.0,
            ..CostModel::default()
        };
        let a = run_backtest(&data, &mut Leveraged, w, 0, no_fin);
        let b = run_backtest(&data, &mut Leveraged, w, 0, with_fin);
        assert!(
            b.returns.iter().sum::<f64>() < a.returns.iter().sum::<f64>(),
            "financing should drag a leveraged book's return"
        );
    }

    #[test]
    fn liquidity_cap_changes_fills() {
        let data = Dataset::synthetic(4, 120, 11);
        let w = Window {
            start: 20,
            end: 120,
        };
        let uncapped = CostModel::default(); // max_participation = INF
        let capped = CostModel {
            max_participation: 0.05,
            ..CostModel::default()
        };
        let a = run_backtest(&data, &mut BuyAndHold, w, 0, uncapped);
        let b = run_backtest(&data, &mut BuyAndHold, w, 0, capped);
        assert_ne!(
            a.returns, b.returns,
            "a tight liquidity cap must change fills"
        );
    }
}
