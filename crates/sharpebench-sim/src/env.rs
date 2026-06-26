//! The Gym-style open-loop environment: the caller drives `reset()` / `step()`.
//!
//! [`TradingEnv`] is the open-loop face of the same engine [`crate::run_backtest`]
//! runs closed — both call the one shared [`crate::engine::step_once`] body, so a
//! trajectory the env produces is byte-identical to the equivalent `run_backtest`
//! (proven by `env_step_matches_run_backtest`). Look-ahead is impossible: the env
//! owns the time cursor and only ever builds a point-in-time observation.

use sharpebench_core::ProcessEvent;
use sharpebench_protocol::{Decision, MarketObservation};

use crate::costs::{CostModel, CostProfile};
use crate::data::Dataset;
use crate::engine::{build_observation, nav, step_once, Book, Window};

/// Bars of trailing history to burn in before a scenario's first decision.
const WARMUP: usize = 20;

/// Per-step side channel: the post-step NAV and the process events generated
/// during the step (fills, sim-exploitation guards, captured rationale).
pub struct StepInfo {
    pub nav: f64,
    pub events: Vec<ProcessEvent>,
}

/// The result of one environment step: the next point-in-time observation, this
/// step's portfolio return (the reward), whether the window is exhausted, and the
/// per-step side channel.
pub struct StepResult {
    pub observation: MarketObservation,
    pub reward: f64,
    pub done: bool,
    pub info: StepInfo,
}

/// A steppable, leak-free trading environment over a frozen dataset. The caller
/// drives it: [`reset`](Self::reset) returns the first observation, then each
/// [`step`](Self::step) applies the supplied decision and advances one bar.
pub struct TradingEnv {
    data: Dataset,
    symbols: Vec<String>,
    costs: CostModel,
    window: Window,
    end: usize,
    seed: u64,
    cursor: usize,
    book: Book,
}

impl TradingEnv {
    /// Build an environment that steps `window` over `data` with seeded execution
    /// noise and the given cost model.
    pub fn new(data: Dataset, window: Window, costs: CostModel, seed: u64) -> Self {
        let symbols = data.symbols();
        let end = window.end.min(data.len());
        let book = Book::new(&symbols, seed);
        TradingEnv {
            data,
            symbols,
            costs,
            window,
            end,
            seed,
            cursor: window.start,
            book,
        }
    }

    /// Reset to the start of the window and return the first point-in-time
    /// observation. Re-seeds the book, so it is safe to call repeatedly.
    pub fn reset(&mut self) -> MarketObservation {
        self.book = Book::new(&self.symbols, self.seed);
        self.cursor = self.window.start;
        build_observation(&self.data, &self.symbols, &self.book, self.obs_index())
    }

    /// Apply `decision` at the current bar and advance one step. The returned
    /// `reward` is the bar's portfolio return; `done` is set once the window is
    /// exhausted (further calls re-apply the final bar harmlessly).
    pub fn step(&mut self, decision: Decision) -> StepResult {
        let t = self.obs_index();
        let events_before = self.book.trace.events.len();
        let out = step_once(
            &self.data,
            &self.symbols,
            &mut self.book,
            &self.costs,
            t,
            &decision,
        );
        let events = self.book.trace.events[events_before..].to_vec();
        let nav_after = nav(
            &self.data,
            &self.symbols,
            &self.book.shares,
            self.book.cash,
            t,
        );
        self.cursor += 1;
        let done = self.cursor >= self.end;
        let observation =
            build_observation(&self.data, &self.symbols, &self.book, self.obs_index());
        StepResult {
            observation,
            reward: out.ret,
            done,
            info: StepInfo {
                nav: nav_after,
                events,
            },
        }
    }

    /// The bar index used to build an observation — the cursor, clamped to the last
    /// in-window bar so a terminal observation never leaks a post-window row.
    fn obs_index(&self) -> usize {
        self.cursor.min(self.end.saturating_sub(1))
    }
}

/// A named bundle of a dataset, the windows to evaluate over it, and the cost
/// model — so "run my agent through the crisis suite under worst-case execution"
/// is one object rather than three loose arguments.
pub struct Scenario {
    pub name: String,
    pub data: Dataset,
    pub windows: Vec<Window>,
    pub costs: CostModel,
}

impl Scenario {
    /// A single full window over `data` (after a [`WARMUP`]-bar burn-in) under `costs`.
    pub fn full(name: impl Into<String>, data: Dataset, costs: CostModel) -> Self {
        let end = data.len();
        let start = WARMUP.min(end.saturating_sub(1));
        Scenario {
            name: name.into(),
            data,
            windows: vec![Window { start, end }],
            costs,
        }
    }

    /// The built-in crisis suite (flash crash + whipsaw) under the given execution
    /// profile — each scenario tests *survival*, not calm-market return.
    pub fn crisis_suite(seed: u64, profile: CostProfile) -> Vec<Scenario> {
        let costs = profile.resolve().costs;
        Dataset::stress_suite(seed)
            .into_iter()
            .map(|(name, data)| Scenario::full(name, data, costs))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::{Agent, BuyAndHold};
    use crate::engine::run_backtest;

    /// The load-bearing guarantee: driving the env with the same (deterministic)
    /// agent reproduces `run_backtest`'s returns AND trace byte-for-byte — proof
    /// that inverting the loop into `reset`/`step` did not change the math.
    #[test]
    fn env_step_matches_run_backtest() {
        let data = Dataset::synthetic(4, 120, 11);
        let window = Window {
            start: 20,
            end: 120,
        };
        let costs = CostModel::default();
        let seed = 7;

        let reference = run_backtest(&data, &mut BuyAndHold, window, seed, costs);

        let mut env = TradingEnv::new(data.clone(), window, costs, seed);
        let mut agent = BuyAndHold;
        let mut obs = env.reset();
        let mut rewards: Vec<f64> = Vec::new();
        let mut events: Vec<ProcessEvent> = Vec::new();
        loop {
            let decision = agent.decide(&obs);
            let res = env.step(decision);
            rewards.push(res.reward);
            events.extend(res.info.events);
            obs = res.observation;
            if res.done {
                break;
            }
        }

        assert_eq!(
            rewards, reference.returns,
            "env rewards must match run_backtest returns byte-for-byte"
        );
        assert_eq!(
            events, reference.trace.events,
            "env per-step events must reassemble run_backtest's trace exactly"
        );
    }

    /// No-lookahead at the observation boundary: every symbol's trailing history
    /// ends exactly at `close_at(t)` — the env can never hand out a future bar.
    #[test]
    fn env_observation_is_point_in_time() {
        let data = Dataset::synthetic(4, 120, 3);
        let window = Window {
            start: 20,
            end: 120,
        };
        let mut env = TradingEnv::new(data.clone(), window, CostModel::default(), 5);
        let mut agent = BuyAndHold;

        let mut obs = env.reset();
        let mut t = window.start;
        loop {
            for snap in &obs.symbols {
                let last = *snap
                    .close_history
                    .last()
                    .expect("a point-in-time history is never empty within the window");
                assert_eq!(
                    last,
                    data.close_at(&snap.symbol, t).unwrap(),
                    "history for {} must end at close_at(t={t})",
                    snap.symbol
                );
            }
            let decision = agent.decide(&obs);
            let res = env.step(decision);
            obs = res.observation;
            t += 1;
            if res.done {
                break;
            }
        }
    }

    /// The crisis suite bundles flash-crash + whipsaw datasets, each with a
    /// non-empty window that a baseline agent can be run through end-to-end.
    #[test]
    fn crisis_suite_scenarios_run() {
        let scenarios = Scenario::crisis_suite(11, CostProfile::WorstCase);
        assert_eq!(scenarios.len(), 2, "flash crash + whipsaw");
        for sc in &scenarios {
            let window = sc.windows[0];
            assert!(
                window.start < window.end,
                "{} has a non-empty window",
                sc.name
            );
            let run = run_backtest(&sc.data, &mut BuyAndHold, window, 1, sc.costs);
            assert_eq!(run.returns.len(), window.end - window.start);
        }
    }
}
