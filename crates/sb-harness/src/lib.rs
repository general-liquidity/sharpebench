//! sb-harness — run orchestration.
//!
//! Drives an [`Agent`](sb_sim::Agent) through the [`sb_sim`] backtest across every
//! window × seed, capturing each run's return series and decision trace into the
//! [`sb_core::AgentSubmission`] format the scoring kernel consumes. Producing one
//! `Run` per (window, seed) is what makes pass^k and multi-window OOS meaningful.
#![forbid(unsafe_code)]

use sb_core::AgentSubmission;
use sb_sim::{run_backtest, Agent, CostModel, Dataset, TeamAgent, Window};

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
}
