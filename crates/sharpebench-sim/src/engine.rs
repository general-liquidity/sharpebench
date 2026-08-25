//! The point-in-time backtest engine.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use sharpebench_core::{ProcessEvent, Run, Trace};
use sharpebench_protocol::{Decision, MarketObservation, PositionState, SymbolSnapshot};

use crate::agent::Agent;
use crate::costs::{liquidity_capped_delta, market_impact_frac, CostModel, ExecutionNoise, Rng};
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
    /// Orders carried to the next bar by the opt-in execution noise (a delayed
    /// order or the unfilled remainder of a partial fill). Empty, and absent from
    /// the serialized snapshot, under the default cost model.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub(crate) pending: BTreeMap<String, PendingOrder>,
    /// Whether this run has deliberately requested a negative target.  Kept
    /// separately from tiny execution-price overshoots around a zero target so
    /// the signed-short financing path can use gross exposure without changing
    /// the committed long-only engine bytes.
    #[serde(default, skip_serializing_if = "is_false")]
    pub(crate) has_short_target: bool,
}

fn is_false(value: &bool) -> bool {
    !*value
}

/// An order carried from a previous bar: the target weight still to be reached.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct PendingOrder {
    pub(crate) target_weight: f64,
}

impl Book {
    pub(crate) fn new(symbols: &[String], seed: u64) -> Self {
        Book {
            shares: symbols.iter().map(|s| (s.clone(), 0.0)).collect(),
            cash: 1.0_f64,
            rng: Rng::new(seed),
            trace: Trace::default(),
            prev_nav: 1.0_f64,
            pending: BTreeMap::new(),
            has_short_target: false,
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

/// Whether attempting a fresh/carry order changed the execution state. The
/// caller needs this distinction to preserve legacy fill-only traces when
/// execution noise is off, while still recording a delayed noisy decision once.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum OrderOutcome {
    Noop,
    Deferred,
    Filled,
}

/// Fill one order (fresh or carried) toward `target_weight` on `symbol` at step
/// `t`: liquidity cap, base seeded slippage, own-order impact, fees, and, when
/// the cost model carries [`ExecutionNoise`], the seed-driven delay / partial
/// fill / queue slippage. With `costs.noise == None` the arithmetic is exactly
/// the historical fill path, in the same order.
#[allow(clippy::too_many_arguments)]
fn apply_order(
    data: &Dataset,
    symbols: &[String],
    book: &mut Book,
    costs: &CostModel,
    seed: u64,
    t: usize,
    cur_nav: f64,
    symbol: &str,
    target_weight: f64,
    carried: bool,
) -> OrderOutcome {
    let p = price(data, symbol, t);
    if p <= 0.0 {
        return OrderOutcome::Noop;
    }
    // The public protocol defines a signed target-weight vector.  Keep the
    // desired value signed so a negative target opens or maintains a short;
    // clamping here used to make every advertised short-capable environment
    // silently behave as long-only.
    let target_value = target_weight * cur_nav;
    let cur_value = book.shares[symbol] * p;
    // Liquidity cap: a trade larger than the per-step participation limit
    // only partially fills; the rest is left for later steps.
    let mut delta_value =
        liquidity_capped_delta(target_value - cur_value, costs.max_participation, cur_nav);
    if delta_value.abs() < 1e-9 {
        return OrderOutcome::Noop;
    }
    let mut queue_slip = 0.0;
    if let Some(noise) = costs.noise {
        let sym_idx = symbols.iter().position(|s| s == symbol).unwrap_or(0);
        let mut nr = ExecutionNoise::stream(seed, t, sym_idx);
        let u_d = nr.unit();
        let u_f = nr.unit();
        let u_q = nr.unit();
        // (a) fill delay: a fresh order may not reach the book this bar. It is
        // carried and fills at the next bar's price; a carried order never waits
        // a second time.
        if !carried && u_d < noise.delay_prob {
            book.pending
                .insert(symbol.to_string(), PendingOrder { target_weight });
            return OrderOutcome::Deferred;
        }
        // (b) partial fill: only a fraction of the target change fills now; the
        // remainder is carried as an order for the same target weight, unless
        // it is below the carry floor, in which case the order fills in full.
        let phi = noise.min_fill_frac + (1.0 - noise.min_fill_frac) * u_f;
        let remainder = (1.0 - phi) * delta_value.abs();
        if remainder > noise.carry_floor * cur_nav.max(0.0) {
            delta_value *= phi;
            book.pending
                .insert(symbol.to_string(), PendingOrder { target_weight });
        }
        // (c) queue-position slippage: an adverse move inside the bar's range,
        // proxied by the close-to-close absolute move since the dataset carries
        // closes only, scaled by participation up to the reference level.
        let prev = if t > 0 {
            data.close_at(symbol, t - 1).unwrap_or(0.0)
        } else {
            0.0
        };
        let range = if prev > 0.0 {
            (p / prev - 1.0).abs()
        } else {
            0.0
        };
        let participation = delta_value.abs() / cur_nav.max(1e-9);
        let scale = if noise.queue_participation_ref > 0.0 {
            (participation / noise.queue_participation_ref).min(1.0)
        } else {
            1.0
        };
        queue_slip = u_q * range * scale;
    }
    // Base seeded slippage plus own-order market impact: the bigger the
    // trade relative to NAV, the more the fill moves against the agent.
    let participation = delta_value.abs() / cur_nav.max(1e-9);
    let slip = (costs.slippage_bps + book.rng.signed_unit().abs() * costs.slippage_bps) / 10_000.0
        + market_impact_frac(costs.impact_bps, participation)
        + queue_slip;
    let exec_p = if delta_value > 0.0 {
        p * (1.0 + slip)
    } else {
        p * (1.0 - slip)
    };
    let dshares = delta_value / exec_p;
    let fee = delta_value.abs() * (costs.fee_bps / 10_000.0);
    if let Some(sh) = book.shares.get_mut(symbol) {
        *sh += dshares;
    }
    book.cash -= dshares * exec_p + fee;
    OrderOutcome::Filled
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
    seed: u64,
    t: usize,
    decision: &Decision,
) -> StepOutcome {
    let cur_nav = nav(data, symbols, &book.shares, book.cash, t);

    // Orders carried from the previous bar by the execution-noise model fill
    // first, at this bar's price, unless the agent re-issues an order on the
    // same symbol this bar (cancel/replace supersedes the carried order).
    if !book.pending.is_empty() {
        let carried = std::mem::take(&mut book.pending);
        for (symbol, pend) in carried {
            if decision.orders.iter().any(|o| o.symbol == symbol) {
                continue;
            }
            let _ = apply_order(
                data,
                symbols,
                book,
                costs,
                seed,
                t,
                cur_nav,
                &symbol,
                pend.target_weight,
                true,
            );
        }
    }

    // Noise is keyed by (seed, step, symbol), so duplicate symbol targets would
    // otherwise reuse one draw. Accept the first target and flag every duplicate
    // instead of making correlated same-bar execution look independent.
    let mut seen_symbols = std::collections::BTreeSet::new();
    // rebalance toward target weights with cost + seeded slippage.
    for ord in &decision.orders {
        if !seen_symbols.insert(&ord.symbol) {
            book.trace.events.push(ProcessEvent::ManipulativeOrder);
            continue;
        }
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
        if ord.target_weight < 0.0 {
            book.has_short_target = true;
        }
        let outcome = apply_order(
            data,
            symbols,
            book,
            costs,
            seed,
            t,
            cur_nav,
            &ord.symbol,
            ord.target_weight,
            false,
        );
        // With legacy noise-off costs, retain the historic trace contract: an
        // order/rationale event denotes an actual fill and no event is emitted
        // for a no-op target. With execution noise, an accepted delayed or
        // partially filled decision is recorded here once; carried fills never
        // duplicate it on later bars.
        if outcome != OrderOutcome::Noop {
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
        // Financing is a function of gross, not net, exposure.  Netting a long
        // against a short must not let a leveraged dollar-neutral book avoid the
        // carry charged to an equally levered long-only book.
        let gross = if book.has_short_target {
            symbols
                .iter()
                .map(|s| (book.shares[s] * price(data, s, t)).abs())
                .sum::<f64>()
                / nav_now
        } else {
            // Preserve the historical long-only arithmetic exactly.  Small
            // execution-price overshoots around a zero target existed in the
            // frozen engine before signed shorts were supported; treating those
            // as intentional shorts would move every committed golden.
            positions_value / nav_now
        };
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
/// `step_once` body the open-loop [`crate::env::TradingEnv`] uses.
pub fn run_backtest(
    data: &Dataset,
    agent: &mut dyn Agent,
    window: Window,
    seed: u64,
    costs: CostModel,
) -> Run {
    costs
        .validate()
        .expect("invalid CostModel: execution noise must be finite and in-domain");
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
        let out = step_once(data, &symbols, &mut book, &costs, seed, t, &decision);
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
    use crate::agent::{Agent, BuyAndHold, Momentum};
    use crate::costs::CostProfile;
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
    fn delayed_fill_records_one_decision_and_keeps_its_rationale() {
        let data = Dataset::synthetic(2, 30, 9);
        let symbols = data.symbols();
        let mut book = Book::new(&symbols, 3);
        let costs = CostModel {
            noise: Some(ExecutionNoise {
                delay_prob: 1.0,
                min_fill_frac: 1.0,
                carry_floor: 0.0,
                queue_participation_ref: 1.0,
            }),
            ..CostModel::default()
        };
        let decision = Decision {
            orders: vec![Order {
                symbol: symbols[0].clone(),
                action: Action::Buy,
                target_weight: 0.2,
                confidence: 0.7,
                rationale: "delayed rationale".to_string(),
            }],
            reasoning: String::new(),
            cost: None,
        };
        // First bar records the decision but delays its fill. The next bar has
        // no new decision; it executes the carry without duplicating audit rows.
        step_once(&data, &symbols, &mut book, &costs, 3, 10, &decision);
        let no_decision = Decision {
            orders: Vec::new(),
            reasoning: String::new(),
            cost: None,
        };
        step_once(&data, &symbols, &mut book, &costs, 3, 11, &no_decision);
        assert_eq!(
            book.trace
                .events
                .iter()
                .filter(|e| matches!(e, ProcessEvent::OrderPlaced { .. }))
                .count(),
            1,
            "one decision, not one event per carried fill"
        );
        assert_eq!(
            book.trace
                .events
                .iter()
                .filter(|e| matches!(e, ProcessEvent::DecisionRationale { rationale, .. } if rationale == "delayed rationale"))
                .count(),
            1,
            "the rationale belongs to the submitted decision even when delayed"
        );
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
            noise: None,
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

    /// Test-only agent that issues one order on the first bar and then holds,
    /// so a carried (delayed or partially filled) order is the only way the
    /// remainder ever reaches the book.
    struct OneShot {
        fired: bool,
    }
    impl Agent for OneShot {
        fn decide(&mut self, obs: &MarketObservation) -> Decision {
            if self.fired {
                return Decision {
                    orders: Vec::new(),
                    reasoning: "hold".to_string(),
                    cost: None,
                };
            }
            self.fired = true;
            Decision {
                orders: vec![Order {
                    symbol: obs.symbols[0].symbol.clone(),
                    action: Action::Buy,
                    target_weight: 0.4,
                    confidence: 0.5,
                    rationale: String::new(),
                }],
                reasoning: "one shot".to_string(),
                cost: None,
            }
        }
    }

    fn realistic() -> CostModel {
        CostProfile::Realistic.resolve().costs
    }

    #[test]
    fn default_profile_ignores_the_noise_machinery() {
        // `noise: None` spelled explicitly and the default model must be the
        // same fills bit for bit, and neither leaves anything pending.
        let data = Dataset::synthetic(4, 120, 11);
        let w = Window {
            start: 20,
            end: 120,
        };
        let explicit = CostModel {
            noise: None,
            ..CostModel::default()
        };
        let a = run_backtest(&data, &mut Momentum::default(), w, 3, CostModel::default());
        let b = run_backtest(&data, &mut Momentum::default(), w, 3, explicit);
        assert_eq!(a.returns, b.returns);
        assert_eq!(a.trace, b.trace);
        let mut book = Book::new(&data.symbols(), 3);
        let obs = build_observation(&data, &data.symbols(), &book, 20);
        let d = Momentum::default().decide(&obs);
        step_once(
            &data,
            &data.symbols(),
            &mut book,
            &CostModel::default(),
            3,
            20,
            &d,
        );
        assert!(book.pending.is_empty());
    }

    #[test]
    fn realistic_profile_changes_fills() {
        let data = Dataset::synthetic(4, 120, 11);
        let w = Window {
            start: 20,
            end: 120,
        };
        let a = run_backtest(&data, &mut BuyAndHold, w, 1, CostModel::default());
        let b = run_backtest(&data, &mut BuyAndHold, w, 1, realistic());
        assert_ne!(
            a.returns, b.returns,
            "the realistic profile must move fills"
        );
        assert_eq!(a.returns.len(), b.returns.len());
    }

    #[test]
    fn realistic_profile_is_deterministic_per_seed() {
        let data = Dataset::synthetic(4, 120, 11);
        let w = Window {
            start: 20,
            end: 120,
        };
        for seed in [1u64, 2, 9] {
            let a = run_backtest(&data, &mut Momentum::default(), w, seed, realistic());
            let b = run_backtest(&data, &mut Momentum::default(), w, seed, realistic());
            assert_eq!(a.returns, b.returns, "seed {seed} must replay bit for bit");
            assert_eq!(a.trace, b.trace);
        }
    }

    #[test]
    fn realistic_seeds_are_distinct_but_bounded() {
        let data = Dataset::synthetic(4, 160, 11);
        let w = Window {
            start: 20,
            end: 160,
        };
        let runs: Vec<Vec<f64>> = (1..=8)
            .map(|s| run_backtest(&data, &mut Momentum::default(), w, s, realistic()).returns)
            .collect();
        for i in 0..runs.len() {
            for j in (i + 1)..runs.len() {
                assert_ne!(runs[i], runs[j], "seeds {i} and {j} coincide");
            }
        }
        // Bounded: execution noise perturbs the run, it does not replace the
        // market. The across-seed dispersion of the run's mean return must sit
        // below the within-run dispersion of returns, and every return finite.
        let means: Vec<f64> = runs
            .iter()
            .map(|r| r.iter().sum::<f64>() / r.len() as f64)
            .collect();
        let m = means.iter().sum::<f64>() / means.len() as f64;
        let across =
            (means.iter().map(|x| (x - m).powi(2)).sum::<f64>() / (means.len() - 1) as f64).sqrt();
        let within = {
            let r = &runs[0];
            let mu = means[0];
            (r.iter().map(|x| (x - mu).powi(2)).sum::<f64>() / (r.len() - 1) as f64).sqrt()
        };
        assert!(runs.iter().flatten().all(|x| x.is_finite()));
        assert!(
            across < within,
            "across-seed std of mean return {across} should be below within-run std {within}"
        );
    }

    #[test]
    fn carried_order_fills_next_bar_for_a_non_reissuing_agent() {
        // Under the realistic profile a one-shot order is either delayed or
        // partially filled at bar 20 with probability one minus a measure-zero
        // event, so the position must keep building on later bars from the
        // carried remainder alone, and the carry must drain (min fill 0.5 per bar).
        let data = Dataset::synthetic(3, 80, 5);
        let symbols = data.symbols();
        let mut book = Book::new(&symbols, 4);
        let mut agent = OneShot { fired: false };
        let costs = realistic();
        let mut held = Vec::new();
        for t in 20..30 {
            let obs = build_observation(&data, &symbols, &book, t);
            let d = agent.decide(&obs);
            step_once(&data, &symbols, &mut book, &costs, 4, t, &d);
            held.push(book.shares[&symbols[0]]);
        }
        assert!(held[0] >= 0.0);
        assert!(
            held.windows(2).any(|p| p[1] > p[0] + 1e-12),
            "a carried order must add to the position on a later bar: {held:?}"
        );
        assert!(
            book.pending.is_empty(),
            "the carry must drain within ten bars: {:?}",
            book.pending
        );
    }

    #[test]
    fn fresh_order_supersedes_a_carried_one() {
        let data = Dataset::synthetic(3, 80, 5);
        let symbols = data.symbols();
        let mut book = Book::new(&symbols, 4);
        book.pending
            .insert(symbols[0].clone(), PendingOrder { target_weight: 0.9 });
        // The agent re-issues a small target on the same symbol: the 0.9 carry
        // must not fill, so the holding stays at or below the fresh target.
        let d = Decision {
            orders: vec![Order {
                symbol: symbols[0].clone(),
                action: Action::Buy,
                target_weight: 0.1,
                confidence: 0.5,
                rationale: String::new(),
            }],
            reasoning: String::new(),
            cost: None,
        };
        step_once(&data, &symbols, &mut book, &realistic(), 4, 20, &d);
        let value = book.shares[&symbols[0]] * data.close_at(&symbols[0], 20).unwrap();
        assert!(
            value <= 0.1 + 1e-9,
            "carried 0.9 target must be superseded: {value}"
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

    #[test]
    fn signed_targets_open_shorts_and_financing_uses_gross_exposure() {
        let data = Dataset::synthetic(2, 80, 17);
        let symbols = data.symbols();
        let mut book = Book::new(&symbols, 3);
        let costs = CostModel {
            fee_bps: 0.0,
            slippage_bps: 0.0,
            impact_bps: 0.0,
            financing_bps: 100.0,
            max_participation: f64::INFINITY,
            trf_cost: None,
            noise: None,
        };
        let decision = Decision {
            orders: vec![
                Order {
                    symbol: symbols[0].clone(),
                    action: Action::Buy,
                    target_weight: 1.0,
                    confidence: 0.5,
                    rationale: String::new(),
                },
                Order {
                    symbol: symbols[1].clone(),
                    action: Action::Sell,
                    target_weight: -1.0,
                    confidence: 0.5,
                    rationale: String::new(),
                },
            ],
            reasoning: "dollar-neutral long/short fixture".to_string(),
            cost: None,
        };

        let outcome = step_once(&data, &symbols, &mut book, &costs, 3, 20, &decision);
        assert!(book.shares[&symbols[0]] > 0.0, "the long leg must open");
        assert!(
            book.shares[&symbols[1]] < 0.0,
            "a negative target must open a short rather than being clamped flat"
        );
        assert!(
            (outcome.ret + 0.01).abs() < 1e-12,
            "2x gross / 0x net exposure must pay one 100bp leveraged carry charge: {}",
            outcome.ret
        );
    }
}
