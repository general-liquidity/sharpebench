//! The declared synthetic stress set, and the replay of one agent through it.
//!
//! `sharpebench stress` and the `--stress-replay` record on `sharpebench run`
//! both take their scenarios from [`stress_scenarios`], so there is one stress
//! path. The kernel that turns replayed returns into a rank-neutral record, and
//! the list of what the replay cannot catch, is
//! [`sharpebench_core::stress_replay`]. The record is a **mitigation** for the
//! `crash_unseen_in_sample` tail seller, not a closure: it adds out-of-sample
//! evidence from synthetic shapes and says nothing about crashes shaped unlike
//! them. No market data is read.

use sharpebench_core::stress_replay::{assess, ScenarioRuns, StressReplay};
use sharpebench_core::Run;
use sharpebench_sim::{run_backtest, Agent, BuyAndHold, CostModel, Dataset, Window};

/// The seed of the declared stress datasets.
pub const STRESS_SUITE_SEED: u64 = 20_260_621;
/// Bars before the scored stress window, so lookback agents have history.
pub const STRESS_WARMUP_BARS: usize = 20;
/// Execution seeds per stress window.
pub const STRESS_EXECUTION_SEEDS: u64 = 6;

/// One declared stress scenario, contamination-masked, with its window, seeds
/// and costs.
pub struct StressScenario {
    pub name: &'static str,
    pub data: Dataset,
    pub windows: Vec<Window>,
    pub seeds: Vec<u64>,
    pub costs: CostModel,
}

/// The declared stress set: [`Dataset::stress_suite`] at [`STRESS_SUITE_SEED`],
/// masked, one window from [`STRESS_WARMUP_BARS`] to the end, default costs.
pub fn stress_scenarios() -> Vec<StressScenario> {
    Dataset::stress_suite(STRESS_SUITE_SEED)
        .into_iter()
        .map(|(name, data)| {
            let data = data.masked();
            let windows = vec![Window {
                start: STRESS_WARMUP_BARS,
                end: data.len(),
            }];
            StressScenario {
                name,
                data,
                windows,
                seeds: (0..STRESS_EXECUTION_SEEDS).collect(),
                costs: CostModel::default(),
            }
        })
        .collect()
}

/// Replay one agent through every declared stress scenario. `cell` runs the
/// agent once on a scenario's window and seed; an `Err` makes the record
/// unavailable with that reason, never a zero loss. The benchmark is an
/// unlevered equal-weight [`BuyAndHold`] run on the same cells.
pub fn replay_under_stress<F>(mut cell: F) -> StressReplay
where
    F: FnMut(&StressScenario, Window, u64) -> Result<Run, String>,
{
    let mut replayed = Vec::new();
    for scenario in stress_scenarios() {
        let mut agent = Vec::new();
        let mut benchmark = Vec::new();
        // inv: `agent` and `benchmark` hold one series per (window, seed) cell
        // already visited, in the same order.
        for &window in &scenario.windows {
            for &seed in &scenario.seeds {
                match cell(&scenario, window, seed) {
                    Ok(run) => agent.push(run.returns),
                    Err(reason) => {
                        return StressReplay::unavailable(format!(
                            "scenario `{}` seed {seed}: {reason}",
                            scenario.name
                        ))
                    }
                }
                let mut hold = BuyAndHold;
                benchmark.push(
                    run_backtest(&scenario.data, &mut hold, window, seed, scenario.costs).returns,
                );
            }
        }
        replayed.push(ScenarioRuns {
            scenario: scenario.name.to_string(),
            agent,
            benchmark,
        });
    }
    assess(&replayed)
}

/// [`replay_under_stress`] for an in-process agent: a fresh agent per cell.
pub fn replay_in_process<F>(mut make_agent: F) -> StressReplay
where
    F: FnMut(u64) -> Box<dyn Agent>,
{
    replay_under_stress(|scenario, window, seed| {
        let mut agent = make_agent(seed);
        Ok(run_backtest(
            &scenario.data,
            agent.as_mut(),
            window,
            seed,
            scenario.costs,
        ))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use sharpebench_protocol::{Action, Decision, MarketObservation, Order};

    /// A carry book that holds every symbol at `leverage` times NAV. On calm
    /// data it is a positive-drift track; through a crash it loses `leverage`
    /// times the market's move. It stands in for the tail seller the grader
    /// search reports as `crash_unseen_in_sample`.
    struct LeveredCarry(f64);

    impl Agent for LeveredCarry {
        fn decide(&mut self, obs: &MarketObservation) -> Decision {
            let w = self.0 / obs.symbols.len().max(1) as f64;
            Decision {
                orders: obs
                    .symbols
                    .iter()
                    .map(|s| Order {
                        symbol: s.symbol.clone(),
                        action: Action::Buy,
                        target_weight: w,
                        confidence: None,
                        rationale: String::new(),
                    })
                    .collect(),
                reasoning: String::new(),
                cost: None,
            }
        }
    }

    fn scenario<'a>(
        replay: &'a StressReplay,
        name: &str,
    ) -> &'a sharpebench_core::stress_replay::ScenarioStress {
        match replay {
            StressReplay::Replayed { scenarios, .. } => scenarios
                .iter()
                .find(|s| s.scenario == name)
                .expect("scenario replayed"),
            StressReplay::Unavailable { reason, .. } => panic!("unavailable: {reason}"),
        }
    }

    #[test]
    fn levered_carry_is_marked_on_the_flash_crash_and_buy_and_hold_is_not() {
        let levered = replay_in_process(|_| Box::new(LeveredCarry(2.0)));
        assert_eq!(levered.marked(), Some(true), "{levered:?}");
        let crash = scenario(&levered, "flash_crash");
        assert!(
            crash.exceeds_limit && crash.agent_loss > 1.5 * crash.benchmark_loss,
            "{crash:?}"
        );

        let hold = replay_in_process(|_| Box::new(BuyAndHold));
        assert_eq!(hold.marked(), Some(false), "{hold:?}");
    }

    #[test]
    fn a_failed_cell_makes_the_record_unavailable() {
        let replay = replay_under_stress(|_, _, seed| {
            if seed == 3 {
                Err("transport failed".to_string())
            } else {
                Ok(Run {
                    returns: vec![0.0; 10],
                    ..Run::default()
                })
            }
        });
        assert_eq!(replay.marked(), None, "{replay:?}");
    }

    #[test]
    fn the_stress_set_is_the_declared_suite() {
        let names: Vec<_> = stress_scenarios().iter().map(|s| s.name).collect();
        let suite: Vec<_> = Dataset::stress_suite(STRESS_SUITE_SEED)
            .into_iter()
            .map(|(n, _)| n)
            .collect();
        assert_eq!(names, suite);
    }
}
