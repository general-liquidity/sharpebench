//! sb-harness — run orchestration.
//!
//! Drives an [`Agent`](sb_sim::Agent) through the [`sb_sim`] backtest across every
//! window × seed, capturing each run's return series and decision trace into the
//! [`sb_core::AgentSubmission`] format the scoring kernel consumes. Producing one
//! `Run` per (window, seed) is what makes pass^k and multi-window OOS meaningful.
#![forbid(unsafe_code)]

use sb_core::AgentSubmission;
use sb_sim::{run_backtest, Agent, CostModel, Dataset, Window};

/// Run a fresh agent (produced by `make_agent`) across every `window` × `seed`
/// and assemble the submission — one `Run` per (window, seed).
pub fn run_agent<F>(
    agent_id: &str,
    data: &Dataset,
    windows: &[Window],
    seeds: &[u64],
    costs: CostModel,
    mut make_agent: F,
) -> AgentSubmission
where
    F: FnMut() -> Box<dyn Agent>,
{
    let mut runs = Vec::new();
    for &w in windows {
        for &seed in seeds {
            let mut agent = make_agent();
            runs.push(run_backtest(data, agent.as_mut(), w, seed, costs));
        }
    }
    AgentSubmission {
        agent_id: agent_id.to_string(),
        runs,
    }
}
