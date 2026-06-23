//! sb-harness — run orchestration.
//!
//! Drives an [`Agent`](sharpebench_sim::Agent) through the [`sharpebench_sim`] backtest across every
//! window × seed, capturing each run's return series and decision trace into the
//! [`sharpebench_core::AgentSubmission`] format the scoring kernel consumes. Producing one
//! `Run` per (window, seed) is what makes pass^k and multi-window OOS meaningful.
#![forbid(unsafe_code)]

use sharpebench_core::AgentSubmission;
use sharpebench_protocol::{AgentTrajectory, RunTrajectory};
use sharpebench_sim::{
    run_backtest, run_backtest_capture, Agent, CostModel, Dataset, RandomAgent, TeamAgent, Window,
};

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
        in_sample_trials: 0,
        candidates: Vec::new(),
    }
}

/// Like [`run_agent`], but also captures the persisted [`AgentTrajectory`] artifact
/// — the agent's raw per-window×seed decisions — alongside the [`AgentSubmission`].
/// The submission's runs and the trajectory's runs are produced in lock-step (same
/// window-major order), so the trajectory can be replayed back into a byte-identical
/// submission by the separate verifier ([`sharpebench_sim::replay_submission`]).
pub fn run_agent_capture<F>(
    agent_id: &str,
    data: &Dataset,
    windows: &[Window],
    seeds: &[u64],
    costs: CostModel,
    mut make_agent: F,
) -> (AgentSubmission, AgentTrajectory)
where
    F: FnMut() -> Box<dyn Agent>,
{
    let mut runs = Vec::new();
    let mut traj_runs: Vec<RunTrajectory> = Vec::new();
    for &w in windows {
        for &seed in seeds {
            let mut agent = make_agent();
            let (run, traj) = run_backtest_capture(data, agent.as_mut(), w, seed, costs);
            runs.push(run);
            traj_runs.push(traj);
        }
    }
    let submission = AgentSubmission {
        agent_id: agent_id.to_string(),
        runs,
        in_sample_trials: 0,
        candidates: Vec::new(),
    };
    let trajectory = AgentTrajectory {
        agent_id: agent_id.to_string(),
        in_sample_trials: 0,
        runs: traj_runs,
    };
    (submission, trajectory)
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
        in_sample_trials: 0,
        candidates: Vec::new(),
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

/// Elicit dominance choices from a live agent to feed
/// [`sharpebench_core::assess_rationality`]. Each scenario presents `n_symbols` assets whose
/// trailing returns encode a known "value", with exactly one clearly-best asset
/// rotated to a different position each time. The agent's choice is read off as the
/// asset it allocates the most weight to. A return-seeking agent should pick the
/// best asset; an indiscriminate one leaves value on the table.
pub fn probe_dominance(
    agent: &mut dyn Agent,
    n_scenarios: usize,
    n_symbols: usize,
) -> Vec<sharpebench_core::DominanceChoice> {
    use sharpebench_protocol::{MarketObservation, PositionState, SymbolSnapshot};
    use std::collections::BTreeMap;

    (0..n_scenarios)
        .map(|s| {
            let best = if n_symbols > 0 { s % n_symbols } else { 0 };
            // Known option values: one clear winner, distinct losers elsewhere.
            let values: Vec<f64> = (0..n_symbols)
                .map(|i| {
                    if i == best {
                        0.05
                    } else {
                        -0.01 - 0.005 * i as f64
                    }
                })
                .collect();
            let symbols: Vec<SymbolSnapshot> = values
                .iter()
                .enumerate()
                .map(|(i, &v)| SymbolSnapshot {
                    symbol: format!("SYM{i:02}"),
                    close_history: vec![1.0, 1.0 + v], // trailing return = v
                    fundamentals: BTreeMap::new(),
                    news: Vec::new(),
                })
                .collect();
            let portfolio = symbols
                .iter()
                .map(|s| PositionState {
                    symbol: s.symbol.clone(),
                    shares: 0.0,
                    avg_price: 0.0,
                })
                .collect();
            let obs = MarketObservation {
                date: format!("t{s}"),
                cash: 1.0,
                symbols,
                portfolio,
            };
            let decision = agent.decide(&obs);
            // Chosen = the asset given the most target weight (first on ties).
            let mut chosen = 0usize;
            let mut best_w = f64::NEG_INFINITY;
            for i in 0..n_symbols {
                let sym = format!("SYM{i:02}");
                let w = decision
                    .orders
                    .iter()
                    .find(|o| o.symbol == sym)
                    .map(|o| o.target_weight)
                    .unwrap_or(0.0);
                if w > best_w {
                    best_w = w;
                    chosen = i;
                }
            }
            sharpebench_core::DominanceChoice {
                options: values,
                chosen,
            }
        })
        .collect()
}

/// Probe an agent's economic rationality over `n_scenarios` dominance menus.
pub fn probe_rationality(
    agent: &mut dyn Agent,
    n_scenarios: usize,
    n_symbols: usize,
) -> sharpebench_core::EconRationalityReport {
    let choices = probe_dominance(agent, n_scenarios, n_symbols);
    sharpebench_core::assess_rationality(&choices, &[], n_symbols)
}

/// Segment a submission's runs by window and report out-of-sample edge decay.
/// [`run_agent`] lays runs out window-major (all seeds of window 0, then window 1,
/// …), so the first `runs.len()/n_windows` runs are the in-sample window and the
/// rest are out-of-sample. Pools each window's returns and calls
/// [`sharpebench_core::oos_decay`].
pub fn oos_decay_of(
    submission: &AgentSubmission,
    n_windows: usize,
) -> sharpebench_core::OosDecayReport {
    let n = submission.runs.len();
    let per = n.checked_div(n_windows).unwrap_or(n).max(1);
    let windows: Vec<Vec<f64>> = (0..n_windows)
        .map(|w| {
            let lo = (w * per).min(n);
            let hi = ((w + 1) * per).min(n);
            submission.runs[lo..hi]
                .iter()
                .flat_map(|r| r.returns.iter().copied())
                .collect()
        })
        .collect();
    sharpebench_core::oos_decay(&windows)
}

/// The output of the separate-verifier path: a score recomputed purely by replaying
/// a persisted [`AgentTrajectory`] through the engine, plus an explanation asserting
/// the recompute is algorithmic (never the agent's self-reported word).
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct VerificationResult {
    pub agent_id: String,
    /// The score recomputed from the replayed submission.
    pub score: sharpebench_core::CompositeScore,
    /// Number of (window × seed) runs replayed from the artifact.
    pub runs_replayed: usize,
    /// Total raw decision steps replayed across all runs.
    pub decisions_replayed: usize,
    /// Human-readable assertion that the score derives from raw decisions alone.
    pub verification_explanation: String,
}

/// Separate-verifier path: ingest a persisted trajectory artifact, **replay** its raw
/// per-step decisions through the frozen dataset's point-in-time engine to regenerate
/// every `Run`, then recompute the composite score with the core scorer. The agent's
/// self-reported returns/metrics are never read — only its decisions are trusted, and
/// the score is derived from replaying them. Mirrors the trust boundary the kernel
/// already enforces, but makes the *artifact → score* recompute explicit and isolated.
pub fn verify_trajectory(
    data: &Dataset,
    traj: &AgentTrajectory,
    costs: CostModel,
    cfg: &sharpebench_core::ScoreConfig,
) -> VerificationResult {
    let submission = sharpebench_sim::replay_submission(data, traj, costs);
    let score = sharpebench_core::score_agent(&submission, cfg);
    let runs_replayed = traj.runs.len();
    let decisions_replayed = traj.runs.iter().map(|r| r.steps.len()).sum();
    VerificationResult {
        agent_id: traj.agent_id.clone(),
        score,
        runs_replayed,
        decisions_replayed,
        verification_explanation: format!(
            "Score recomputed algorithmically by replaying {decisions_replayed} raw \
             decisions across {runs_replayed} runs through the point-in-time engine on \
             the frozen dataset; the agent's self-reported returns and metrics were not \
             read. The artifact carries decisions only — every return, cost, and gate is \
             regenerated by the verifier."
        ),
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
/// [`sharpebench_core::attribute_roles`] needs to estimate who carried the team.
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
        in_sample_trials: 0,
        candidates: Vec::new(),
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
    use sharpebench_core::roles::attribute_roles;
    use sharpebench_sim::{BuyAndHold, Momentum};

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
    fn rationality_probe_separates_discriminating_agents() {
        // Momentum concentrates on the single best asset → fully rational.
        let mut mo = Momentum::default();
        let r_mo = probe_rationality(&mut mo, 8, 4);
        assert_eq!(
            r_mo.rationality_score, 1.0,
            "momentum should pick the winner"
        );

        // Buy-and-hold spreads weight indiscriminately → leaves value on the table.
        let mut bh = BuyAndHold;
        let r_bh = probe_rationality(&mut bh, 8, 4);
        assert!(
            r_mo.rationality_score > r_bh.rationality_score,
            "a discriminating agent should out-score an indiscriminate one"
        );
    }

    #[test]
    fn oos_decay_segments_a_submission_by_window() {
        let data = Dataset::synthetic(4, 160, 7);
        let windows = [
            Window { start: 20, end: 90 },
            Window {
                start: 90,
                end: 160,
            },
        ];
        let seeds: Vec<u64> = (0..3).collect();
        let sub = run_agent("bh", &data, &windows, &seeds, CostModel::default(), || {
            Box::new(BuyAndHold) as Box<dyn Agent>
        });
        let report = oos_decay_of(&sub, 2);
        assert_eq!(report.window_metrics.len(), 2);
    }

    #[test]
    fn capture_replay_score_round_trips_exactly() {
        use sharpebench_core::{score_agent, ScoreConfig};
        use sharpebench_sim::replay_submission;

        let data = Dataset::synthetic(5, 160, 20_260_621);
        let windows = [
            Window { start: 20, end: 90 },
            Window {
                start: 90,
                end: 160,
            },
        ];
        let seeds: Vec<u64> = (0..4).collect();
        let costs = CostModel::default();

        // Direct run, capturing the trajectory artifact alongside the submission.
        let (direct_sub, traj) =
            run_agent_capture("momentum", &data, &windows, &seeds, costs, || {
                Box::new(Momentum::default()) as Box<dyn Agent>
            });

        // Persist + reload the artifact (the verifier ingests JSON off disk).
        let json = serde_json::to_string(&traj).unwrap();
        let reloaded: sharpebench_protocol::AgentTrajectory =
            serde_json::from_str(&json).unwrap();

        // Separate verifier: replay the raw decisions into a fresh submission and
        // score it. The recomputed score must match the direct-run score exactly.
        let replayed_sub = replay_submission(&data, &reloaded, costs);
        let cfg = ScoreConfig::default();
        let direct_score = score_agent(&direct_sub, &cfg);
        let replayed_score = score_agent(&replayed_sub, &cfg);
        assert_eq!(
            serde_json::to_string(&direct_score).unwrap(),
            serde_json::to_string(&replayed_score).unwrap(),
            "replayed score must equal the direct-run score"
        );
    }

    #[test]
    fn tampered_trajectory_scores_differently() {
        use sharpebench_core::{score_agent, ScoreConfig};
        use sharpebench_sim::replay_submission;

        let data = Dataset::synthetic(5, 160, 20_260_621);
        let windows = [Window {
            start: 20,
            end: 160,
        }];
        let seeds: Vec<u64> = (0..4).collect();
        let costs = CostModel::default();
        let (direct_sub, mut traj) =
            run_agent_capture("momentum", &data, &windows, &seeds, costs, || {
                Box::new(Momentum::default()) as Box<dyn Agent>
            });

        // Forge the artifact: double every staked weight. The honest recompute
        // produces a different score than the genuine run — the verifier can't be
        // fooled by editing the persisted decisions.
        for run in &mut traj.runs {
            for step in &mut run.steps {
                for order in &mut step.decision.orders {
                    order.target_weight *= 2.0;
                }
            }
        }
        let cfg = ScoreConfig::default();
        let honest = score_agent(&direct_sub, &cfg);
        let forged = score_agent(&replay_submission(&data, &traj, costs), &cfg);
        assert_ne!(
            honest.raw_mean_return, forged.raw_mean_return,
            "a tampered trajectory must recompute to a different score"
        );
    }

    #[test]
    fn luck_floor_agents_do_not_clear_the_gates() {
        use sharpebench_core::{rank, ScoreConfig};
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
