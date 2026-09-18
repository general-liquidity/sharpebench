//! A calibration pair's outcome is the return of the book its decision left.
//!
//! The return the engine books at step `t + 1` holds two things: the move on the
//! book decision `t` left, and decision `t + 1`'s own fills, fees, financing and
//! borrow. Decision `t`'s confidence is graded on the first part only: the
//! pre-trade mark of its book at the `t + 1` closes, plus the dividends that
//! book earns at `t + 1`, against the NAV step `t` closed at. A decision whose
//! orders the execution noise carried to the next bar (a delayed order, or a
//! partial fill whose remainder carries) opens no pair, because its book is not
//! the one it chose.
//!
//! Hand values were derived in exact rationals with sympy.

use std::collections::BTreeMap;

use sharpebench_core::{score_agent, AgentSubmission, Run, ScoreConfig};
use sharpebench_protocol::{decision_from_wire, Decision, MarketObservation};
use sharpebench_sim::{
    run_backtest, Agent, CostModel, Dataset, ExecutionNoise, TradingEnv, Window,
};

fn brier(run: Run) -> Option<f64> {
    score_agent(
        &AgentSubmission {
            agent_id: "agent".to_string(),
            runs: vec![run],
            in_sample_trials: 0,
            candidates: Vec::new(),
        },
        &ScoreConfig::default(),
    )
    .calibration_brier
}

fn costs(fee_bps: f64) -> CostModel {
    CostModel {
        fee_bps,
        slippage_bps: 0.0,
        impact_bps: 0.0,
        financing_bps: 0.0,
        ..CostModel::default()
    }
}

fn one_symbol(closes: &[f64], dividends: &[f64]) -> Dataset {
    Dataset {
        dates: (0..closes.len())
            .map(|t| format!("2026-01-{t:03}"))
            .collect(),
        closes: BTreeMap::from([("A".to_string(), closes.to_vec())]),
        dividends: BTreeMap::from([("A".to_string(), dividends.to_vec())]),
    }
}

fn order(obs: &MarketObservation, index: usize, weight: f64, confidence: f64) -> String {
    format!(
        r#"{{"symbol":"{}","action":"buy","target_weight":{weight},"confidence":{confidence}}}"#,
        obs.symbols[index].symbol
    )
}

fn decision(orders: &[String]) -> Decision {
    decision_from_wire(&format!(r#"{{"orders":[{}]}}"#, orders.join(",")))
        .expect("the test decision conforms to the wire contract")
}

/// Buys the whole book stating 0.9, exits stating 0.2, then holds.
struct BuyThenExit {
    decided: usize,
}
impl Agent for BuyThenExit {
    fn decide(&mut self, obs: &MarketObservation) -> Decision {
        self.decided += 1;
        match self.decided {
            1 => decision(&[order(obs, 0, 1.0, 0.9)]),
            2 => decision(&[order(obs, 0, 0.0, 0.2)]),
            _ => decision(&[]),
        }
    }
}

#[test]
fn the_next_decision_s_trading_cost_is_not_the_previous_decision_s_outcome() {
    // A 1% fee. Decision 0 buys at 100, paying 0.01, so step 0 closes at NAV
    // 99/100. The price rises to 100.5: the held book is worth 199/200, a
    // return of 1/198. Decision 1 then sells at a 0.01005 fee, so the return
    // booked at step 1 is -101/19800, a loss that belongs to decision 1.
    let data = one_symbol(&[100.0, 100.5, 100.5], &[0.0; 3]);
    let run = run_backtest(
        &data,
        &mut BuyThenExit { decided: 0 },
        Window { start: 0, end: 3 },
        1,
        costs(100.0),
    );
    assert!(
        (run.returns[1] - -101.0 / 19800.0).abs() < 1e-15,
        "{:?}",
        run.returns
    );
    assert_eq!(run.confidences, vec![0.9, 0.2]);
    assert_eq!(
        run.outcomes,
        vec![true, false],
        "the bought book gained; the exit left a flat book that earned nothing"
    );
    // (0.1^2 + 0.2^2) / 2; grading decision 0 on the booked return gave 17/40.
    let b = brier(run).expect("two stated pairs");
    assert!((b - 1.0 / 40.0).abs() < 1e-12, "{b}");
}

#[test]
fn dividends_on_the_held_book_count_toward_its_outcome() {
    // Frictionless. Decision 0 buys 0.01 shares at 100. The price falls to
    // 99.9 and the share pays 0.2, so the held book returns -1/1000 on price
    // and +1/1000 with the dividend. Decision 1 exits before the engine
    // credits the dividend to post-trade holdings, so the booked return is
    // -1/1000 and never saw it.
    let data = one_symbol(&[100.0, 99.9, 99.9], &[0.0, 0.2, 0.0]);
    let run = run_backtest(
        &data,
        &mut BuyThenExit { decided: 0 },
        Window { start: 0, end: 3 },
        1,
        costs(0.0),
    );
    assert!((run.returns[1] - -0.001).abs() < 1e-15, "{:?}", run.returns);
    assert_eq!(run.outcomes, vec![true, false]);
}

/// Rebalances the first symbol to an alternating weight every step, stating a
/// confidence that varies with the step, so fees land on every bar.
struct Churner {
    decided: usize,
}
impl Agent for Churner {
    fn decide(&mut self, obs: &MarketObservation) -> Decision {
        self.decided += 1;
        let weight = if self.decided.is_multiple_of(2) {
            0.9
        } else {
            0.1
        };
        let confidence = 0.5 + 0.4 * ((self.decided % 5) as f64 / 4.0);
        decision(&[order(obs, 0, weight, confidence), order(obs, 1, 0.3, 0.6)])
    }
}

/// The held-book return of every step after the first, rebuilt from the
/// open-loop environment's public state: the post-trade portfolio and cash the
/// next observation reports, marked at the next closes with the next dividends,
/// against the post-trade NAV the step reported.
fn held_book_returns(data: &Dataset, window: Window, costs: CostModel, seed: u64) -> Vec<f64> {
    let mut env = TradingEnv::new(data.clone(), window, costs, seed);
    let mut agent = Churner { decided: 0 };
    let mut obs = env.reset();
    let mut marks = Vec::new();
    for t in window.start..window.end {
        let step = env.step(agent.decide(&obs));
        if t + 1 < window.end {
            let book = &step.observation.portfolio;
            let marked = step.observation.cash
                + book
                    .iter()
                    .map(|p| p.shares * data.close_at(&p.symbol, t + 1).unwrap())
                    .sum::<f64>();
            let dividends: f64 = book
                .iter()
                .map(|p| p.shares * data.dividend_at(&p.symbol, t + 1))
                .sum();
            marks.push((marked + dividends) / step.info.nav - 1.0);
        }
        obs = step.observation;
    }
    marks
}

#[test]
fn outcomes_match_the_held_book_rebuilt_from_the_open_loop_state() {
    let data = Dataset::synthetic(2, 120, 13).with_dividend_yield(0.0004);
    let window = Window {
        start: 20,
        end: 120,
    };
    let costs = CostModel {
        fee_bps: 30.0,
        ..CostModel::default()
    };
    let run = run_backtest(&data, &mut Churner { decided: 0 }, window, 3, costs);
    let marks = held_book_returns(&data, window, costs, 3);
    assert_eq!(run.outcomes.len(), 99);
    let expected: Vec<bool> = marks.iter().map(|m| *m > 0.0).collect();
    assert_eq!(run.outcomes, expected);
    // The fixture is one where the old reading differs: some booked returns
    // take the next decision's fees and flip the sign of the outcome.
    let booked: Vec<bool> = run.returns[1..].iter().map(|r| *r > 0.0).collect();
    let flipped = booked.iter().zip(&expected).filter(|(a, b)| a != b).count();
    assert!(flipped > 0, "the fixture must separate the two readings");
}

/// States 0.9 on a one-symbol buy every step.
struct Stated;
impl Agent for Stated {
    fn decide(&mut self, obs: &MarketObservation) -> Decision {
        decision(&[order(obs, 0, 0.3, 0.9)])
    }
}

fn noisy(delay_prob: f64, min_fill_frac: f64, carry_floor: f64) -> CostModel {
    CostModel {
        noise: Some(ExecutionNoise {
            delay_prob,
            min_fill_frac,
            carry_floor,
            queue_participation_ref: 0.10,
        }),
        ..CostModel::default()
    }
}

/// Whether each decision left one of its own orders pending, read off the
/// serialized environment state after the step.
fn deferred_steps(data: &Dataset, window: Window, costs: CostModel, seed: u64) -> Vec<bool> {
    let mut env = TradingEnv::new(data.clone(), window, costs, seed);
    let mut obs = env.reset();
    let mut deferred = Vec::new();
    for _ in window.start..window.end {
        let step = env.step(Stated.decide(&obs));
        let state = serde_json::to_value(env.clone_state()).unwrap();
        let symbol = &obs.symbols[0].symbol;
        deferred.push(state["book"]["pending"].get(symbol).is_some());
        obs = step.observation;
    }
    deferred
}

#[test]
fn a_decision_the_noise_deferred_opens_no_pair() {
    let data = Dataset::synthetic(2, 80, 7);
    let window = Window { start: 20, end: 80 };

    // Every fresh order waits a bar: no decision holds the book it chose.
    let delayed = run_backtest(&data, &mut Stated, window, 1, noisy(1.0, 0.5, 0.001));
    assert!(delayed.confidences.is_empty(), "{:?}", delayed.confidences);
    assert!(delayed.outcomes.is_empty());

    // Noise that never defers pairs every decision but the last.
    let prompt = run_backtest(&data, &mut Stated, window, 1, noisy(0.0, 1.0, 0.001));
    assert_eq!(prompt.confidences, vec![0.9; 59]);

    // Partial fills carry a remainder on some bars and not on others. A pair
    // exists exactly for the decisions that left nothing pending, except the
    // last decision, whose outcome lies outside the window.
    let partial = noisy(0.2, 0.3, 0.02);
    let run = run_backtest(&data, &mut Stated, window, 5, partial);
    let deferred = deferred_steps(&data, window, partial, 5);
    let expected = deferred[..deferred.len() - 1]
        .iter()
        .filter(|d| !**d)
        .count();
    assert!(
        expected > 0 && expected < 59,
        "the fixture must defer some decisions and not others: {expected}"
    );
    assert_eq!(run.confidences.len(), expected);
    assert_eq!(run.outcomes.len(), expected);
}
