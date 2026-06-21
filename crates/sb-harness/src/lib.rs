//! sb-harness — run orchestration.
//!
//! Drives an [`Agent`](sb_sim::Agent) through the [`sb_sim`] backtest across every
//! window × seed, capturing each run's return series and decision trace into the
//! [`sb_core::AgentSubmission`] format the scoring kernel consumes. Producing one
//! `Run` per (window, seed) is what makes pass^k and multi-window OOS meaningful.
#![forbid(unsafe_code)]

use sb_core::AgentSubmission;
use sb_sim::{run_backtest, Agent, CostModel, Dataset, RandomAgent, TeamAgent, Window};

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

/// Like [`run_agent`], but the factory receives the run's execution `seed` — for
/// agents (e.g. [`RandomAgent`]) whose behaviour should vary per run rather than
/// being identical across seeds.
pub fn run_seeded_agent<F>(
    agent_id: &str,
    data: &Dataset,
    windows: &[Window],
    seeds: &[u64],
    costs: CostModel,
    mut make_agent: F,
) -> AgentSubmission
where
    F: FnMut(u64) -> Box<dyn Agent>,
{
    let mut runs = Vec::new();
    for &w in windows {
        for &seed in seeds {
            let mut agent = make_agent(seed);
            runs.push(run_backtest(data, agent.as_mut(), w, seed, costs));
        }
    }
    AgentSubmission {
        agent_id: agent_id.to_string(),
        runs,
    }
}

/// Produce `n_agents` random "monkey" submissions — the **luck floor**. Each is a
/// distinctly-seeded [`RandomAgent`] run across every window × seed, so the field
/// shows the distribution of zero-skill outcomes a genuine edge must clear. None
/// should be rank-eligible.
pub fn luck_floor(
    data: &Dataset,
    windows: &[Window],
    seeds: &[u64],
    costs: CostModel,
    n_agents: usize,
) -> Vec<AgentSubmission> {
    (0..n_agents)
        .map(|k| {
            let base = 0xF100_0000_0000_0000 ^ (k as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15);
            run_seeded_agent(
                &format!("luck-floor-{k:02}"),
                data,
                windows,
                seeds,
                costs,
                move |seed| Box::new(RandomAgent::new(base ^ seed)) as Box<dyn Agent>,
            )
        })
        .collect()
}

/// One member of a trading team: a name plus a factory for fresh instances (the
/// engine needs an independent agent per run).
pub struct TeamMember {
    pub name: String,
    pub make: Box<dyn Fn() -> Box<dyn Agent>>,
}

impl TeamMember {
    pub fn new<F>(name: &str, make: F) -> Self
    where
        F: Fn() -> Box<dyn Agent> + 'static,
    {
        Self {
            name: name.to_string(),
            make: Box::new(make),
        }
    }
}

/// A team run: the team's own submission (scored as a unit) plus each member's
/// solo pooled return series over the same windows × seeds — exactly the inputs
/// [`sb_core::attribute_roles`] needs to estimate who carried the team.
pub struct TeamResult {
    pub team: AgentSubmission,
    pub role_returns: Vec<(String, Vec<f64>)>,
}

/// Run a `members` team as a consensus [`TeamAgent`] across every window × seed,
/// and separately run each member solo to capture its role-level return series.
pub fn run_team(
    team_id: &str,
    data: &Dataset,
    windows: &[Window],
    seeds: &[u64],
    costs: CostModel,
    members: &[TeamMember],
) -> TeamResult {
    let mut runs = Vec::new();
    for &w in windows {
        for &seed in seeds {
            let instances: Vec<Box<dyn Agent>> = members.iter().map(|m| (m.make)()).collect();
            let mut team = TeamAgent { members: instances };
            runs.push(run_backtest(data, &mut team, w, seed, costs));
        }
    }
    let team = AgentSubmission {
        agent_id: team_id.to_string(),
        runs,
    };

    let role_returns = members
        .iter()
        .map(|m| {
            let sub = run_agent(&m.name, data, windows, seeds, costs, || (m.make)());
            let pooled: Vec<f64> = sub
                .runs
                .iter()
                .flat_map(|r| r.returns.iter().copied())
                .collect();
            (m.name.clone(), pooled)
        })
        .collect();

    TeamResult { team, role_returns }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sb_core::roles::attribute_roles;
    use sb_sim::{BuyAndHold, Momentum};

    #[test]
    fn team_run_produces_alignable_role_series() {
        let data = Dataset::synthetic(4, 80, 20_260_621);
        let windows = [Window { start: 20, end: 80 }];
        let seeds: Vec<u64> = (0..3).collect();
        let members = [
            TeamMember::new("momentum", || {
                Box::new(Momentum::default()) as Box<dyn Agent>
            }),
            TeamMember::new("buy-and-hold", || Box::new(BuyAndHold) as Box<dyn Agent>),
        ];
        let res = run_team(
            "team",
            &data,
            &windows,
            &seeds,
            CostModel::default(),
            &members,
        );

        assert_eq!(res.role_returns.len(), 2);
        let team_pooled: Vec<f64> = res
            .team
            .runs
            .iter()
            .flat_map(|r| r.returns.iter().copied())
            .collect();
        for (_, series) in &res.role_returns {
            assert_eq!(series.len(), team_pooled.len(), "role series must align");
        }
        // The attribution analyzer accepts the produced series.
        let attr = attribute_roles(&team_pooled, &res.role_returns);
        assert_eq!(attr.len(), 2);
    }

    #[test]
    fn luck_floor_agents_do_not_clear_the_gates() {
        use sb_core::{rank, ScoreConfig};
        let data = Dataset::synthetic(5, 120, 20_260_621);
        let windows = [Window {
            start: 20,
            end: 120,
        }];
        let seeds: Vec<u64> = (0..5).collect();
        let floor = luck_floor(&data, &windows, &seeds, CostModel::default(), 4);
        assert_eq!(floor.len(), 4);
        let board = rank(&floor, &ScoreConfig::default());
        assert!(
            board.iter().all(|s| !s.rank_eligible),
            "a random monkey must never be rank-eligible"
        );
    }
}
