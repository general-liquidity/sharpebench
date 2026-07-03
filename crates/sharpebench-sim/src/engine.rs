//! The point-in-time backtest engine.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use sharpebench_core::{ProcessEvent, Run, Trace};
use sharpebench_protocol::{Decision, MarketObservation, PositionState, SymbolSnapshot};

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

pub(crate) fn nav(
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

/// The mutable running state of a backtest: holdings, cash, the seeded execution
/// RNG, the accumulating decision trace, and the prior-step NAV used to book the
/// per-step return. Shared by the closed-loop [`run_backtest`] and the open-loop
/// [`crate::env::TradingEnv`] so the two stepping surfaces cannot drift.
///
/// `Clone + Serialize + Deserialize + PartialEq` make it the serializable payload
/// of [`crate::env::EnvState`] — an O(1) snapshot/restore of the whole mutable sim
/// state (holdings, cash, RNG cursor, trace, prior NAV).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct Book {
    pub(crate) shares: BTreeMap<String, f64>,
    pub(crate) cash: f64,
    pub(crate) rng: Rng,
    pub(crate) trace: Trace,
    pub(crate) prev_nav: f64,
}

impl Book {
    pub(crate) fn new(symbols: &[String], seed: u64) -> Self {
        Book {
            shares: symbols.iter().map(|s| (s.clone(), 0.0)).collect(),
            cash: 1.0_f64,
            rng: Rng::new(seed),
            trace: Trace::default(),
            prev_nav: 1.0_f64,
        }
    }
}

/// Build the point-in-time observation handed to the agent at step `t`: trailing
/// closes (≤ `t`), current holdings, and cash. No bar after `t` is reachable.
pub(crate) fn build_observation(
    data: &Dataset,
    symbols: &[String],
    book: &Book,
    t: usize,
) -> MarketObservation {
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
            shares: book.shares[s],
            avg_price: 0.0,
        })
        .collect();
    MarketObservation {
        date: data.dates[t].clone(),
        cash: book.cash,
        symbols: snap,
        portfolio,
    }
}

/// What the engine records for one step: the realized return plus the calibration
/// inputs (stated conviction and whether the step paid off).
pub(crate) struct StepOutcome {
    pub(crate) ret: f64,
    pub(crate) confidence: f64,
    pub(crate) outcome: bool,
}

/// Apply `decision` at step `t` and advance one bar: rebalance toward target
/// weights with cost + seeded slippage + own-order market impact + partial fills,
/// credit dividends, charge financing on leverage, then book the post-trade return
/// vs the prior step's NAV. Mutates `book`. This is the single per-step body shared
/// by [`run_backtest`] (closed loop) and [`crate::env::TradingEnv::step`] (open
/// loop), so neither stepping surface can drift from the other.
pub(crate) fn step_once(
    data: &Dataset,
    symbols: &[String],
    book: &mut Book,
    costs: &CostModel,
    t: usize,
    decision: &Decision,
) -> StepOutcome {
    let cur_nav = nav(data, symbols, &book.shares, book.cash, t);

    // rebalance toward target weights with cost + seeded slippage.
    for ord in &decision.orders {
        let p = price(data, &ord.symbol, t);
        if p <= 0.0 {
            continue;
        }
        // Sim-exploitation guard: non-finite or absurd weights are gaming attempts.
        if !ord.target_weight.is_finite() || ord.target_weight.abs() > HARD_WEIGHT_CAP {
            book.trace.events.push(ProcessEvent::ManipulativeOrder);
            continue;
        }
        if ord.target_weight.abs() > CONCENTRATION_CAP {
            book.trace.events.push(ProcessEvent::ConcentrationBreach);
        }
        let target_value = ord.target_weight.max(0.0) * cur_nav;
        let cur_value = book.shares[&ord.symbol] * p;
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
        let slip = (costs.slippage_bps + book.rng.signed_unit().abs() * costs.slippage_bps)
            / 10_000.0
            + market_impact_frac(costs.impact_bps, participation);
        let exec_p = if delta_value > 0.0 {
            p * (1.0 + slip)
        } else {
            p * (1.0 - slip)
        };
        let dshares = delta_value / exec_p;
        let fee = delta_value.abs() * (costs.fee_bps / 10_000.0);
        if let Some(sh) = book.shares.get_mut(&ord.symbol) {
            *sh += dshares;
        }
        book.cash -= dshares * exec_p + fee;
        // Capture the order's stated rationale into the audit trail (score-neutral),
        // so the frozen trace explains *why* each fill happened. Empty = omitted.
        if !ord.rationale.is_empty() {
            book.trace.events.push(ProcessEvent::DecisionRationale {
                symbol: ord.symbol.clone(),
                rationale: ord.rationale.clone(),
            });
        }
        book.trace.events.push(ProcessEvent::OrderPlaced {
            risk_gate_passed: true,
        });
    }

    // corporate actions: credit cash dividends on post-trade holdings.
    for s in symbols {
        let div = data.dividend_at(s, t);
        if div != 0.0 {
            book.cash += book.shares[s] * div;
        }
    }

    // financing: charge carry on any leveraged exposure above 1× NAV.
    let positions_value: f64 = symbols
        .iter()
        .map(|s| book.shares[s] * price(data, s, t))
        .sum();
    let nav_now = book.cash + positions_value;
    if nav_now > 1e-12 {
        let gross = positions_value / nav_now;
        book.cash -= crate::costs::financing_cost_frac(costs.financing_bps, gross) * nav_now;
    }

    // daily return = post-trade NAV vs the prior step's NAV (captures the price
    // move on held positions, dividends, financing, and trading costs).
    let navc = nav(data, symbols, &book.shares, book.cash, t);
    let ret = if book.prev_nav.abs() > 1e-12 {
        navc / book.prev_nav - 1.0
    } else {
        0.0
    };
    // Capture the decision's stated conviction and whether the step paid off, so
    // the scoring kernel's calibration axis is fed from the live run.
    let avg_conf = if decision.orders.is_empty() {
        0.5
    } else {
        decision.orders.iter().map(|o| o.confidence).sum::<f64>() / decision.orders.len() as f64
    };
    book.prev_nav = navc;
    StepOutcome {
        ret,
        confidence: avg_conf,
        outcome: ret > 0.0,
    }
}

/// Run a single backtest of `agent` over `window` with seeded execution noise,
/// returning an [`sharpebench_core::Run`] (per-period returns + decision trace).
/// The closed-loop driver: it owns the `decide → step` loop, calling the same
/// [`step_once`] body the open-loop [`crate::env::TradingEnv`] uses.
pub fn run_backtest(
    data: &Dataset,
    agent: &mut dyn Agent,
    window: Window,
    seed: u64,
    costs: CostModel,
) -> Run {
    let symbols = data.symbols();
    let end = window.end.min(data.len());
    let mut book = Book::new(&symbols, seed);
    let mut returns: Vec<f64> = Vec::new();
    let mut confidences: Vec<f64> = Vec::new();
    let mut outcomes: Vec<bool> = Vec::new();
    // Accumulate the agent's self-reported *compute* cost (distinct from trading
    // cost, which is already baked into `returns`). Feeds `Run.cost`, which drives
    // the cost-normalized leaderboard columns (`return_per_cost` / `dsr_per_cost`).
    let mut compute_cost = 0.0_f64;

    for t in window.start..end {
        let obs = build_observation(data, &symbols, &book, t);
        let decision = agent.decide(&obs);
        if let Some(c) = &decision.cost {
            compute_cost += c.billable_units();
        }
        let out = step_once(data, &symbols, &mut book, &costs, t, &decision);
        returns.push(out.ret);
        confidences.push(out.confidence);
        outcomes.push(out.outcome);
    }

    Run {
        returns,
        trace: book.trace,
        confidences,
        outcomes,
        cost: compute_cost,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::{Agent, BuyAndHold};
    use sharpebench_protocol::{Action, Decision, MarketObservation, Order};

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
                    rationale: "2x leverage".to_string(),
                }],
                reasoning: "2x leverage".to_string(),
                cost: None,
            }
        }
    }

    /// Test-only agent that buys the first symbol with a stated per-order rationale.
    struct RationaleAgent;
    impl Agent for RationaleAgent {
        fn decide(&mut self, obs: &MarketObservation) -> Decision {
            let sym = obs.symbols[0].symbol.clone();
            Decision {
                orders: vec![Order {
                    symbol: sym,
                    action: Action::Buy,
                    target_weight: 0.2,
                    confidence: 0.7,
                    rationale: "momentum breakout".to_string(),
                }],
                reasoning: "single-name buy".to_string(),
                cost: None,
            }
        }
    }

    /// Test-only agent that buys the first symbol AND self-reports a per-decision
    /// compute cost, the external-LLM path the cost channel exists for.
    struct CostlyAgent;
    impl Agent for CostlyAgent {
        fn decide(&mut self, obs: &MarketObservation) -> Decision {
            use sharpebench_protocol::DecisionCost;
            let sym = obs.symbols[0].symbol.clone();
            Decision {
                orders: vec![Order {
                    symbol: sym,
                    action: Action::Buy,
                    target_weight: 0.2,
                    confidence: 0.6,
                    rationale: String::new(),
                }],
                reasoning: "costly".to_string(),
                cost: Some(DecisionCost {
                    cost_usd: 0.01,
                    tokens_in: 100,
                    tokens_out: 50,
                    reasoning_tokens: 0,
                }),
            }
        }
    }

    #[test]
    fn self_reported_cost_populates_run_cost_and_dsr_per_cost() {
        use sharpebench_core::{score_agent, AgentSubmission, ScoreConfig};
        let data = Dataset::synthetic(3, 80, 7);
        let window = Window { start: 20, end: 80 };
        let run = run_backtest(&data, &mut CostlyAgent, window, 1, CostModel::default());
        // 60 steps × $0.01 = $0.60 of self-reported compute cost.
        assert!(
            (run.cost - 0.60).abs() < 1e-9,
            "each decision's cost must accumulate into Run.cost: got {}",
            run.cost
        );
        // The cost-normalized leaderboard columns are now live (Some, not None).
        let sub = AgentSubmission {
            agent_id: "costly".to_string(),
            runs: vec![run],
            in_sample_trials: 0,
            candidates: Vec::new(),
        };
        let score = score_agent(&sub, &ScoreConfig::default());
        assert!(score.return_per_cost.is_some(), "return_per_cost goes live");
        assert!(score.dsr_per_cost.is_some(), "dsr_per_cost goes live");

        // A cost-silent agent leaves the columns None (back-compat).
        let free = run_backtest(&data, &mut BuyAndHold, window, 1, CostModel::default());
        assert_eq!(free.cost, 0.0);
        let free_sub = AgentSubmission {
            agent_id: "free".to_string(),
            runs: vec![free],
            in_sample_trials: 0,
            candidates: Vec::new(),
        };
        let free_score = score_agent(&free_sub, &ScoreConfig::default());
        assert!(free_score.dsr_per_cost.is_none());
    }

    #[test]
    fn per_order_rationale_is_captured_into_the_trace() {
        use sharpebench_core::ProcessEvent;
        let data = Dataset::synthetic(3, 60, 5);
        let run = run_backtest(
            &data,
            &mut RationaleAgent,
            Window { start: 20, end: 60 },
            1,
            CostModel::default(),
        );
        let found = run.trace.events.iter().any(|e| {
            matches!(e, ProcessEvent::DecisionRationale { rationale, .. } if rationale == "momentum breakout")
        });
        assert!(found, "the order rationale must land in the audit trace");
        // It is score-neutral: the run is still process-clean.
        assert!(sharpebench_core::process::process_score(&run.trace).is_clean());
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
            trf_cost: None,
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
