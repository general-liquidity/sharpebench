//! The composite score + leaderboard ranking — where the gates compose.
//!
//! An agent ranks **only if** every gate holds:
//! 1. its pooled Deflated Sharpe clears `dsr_bar` (survives multiple-testing),
//! 2. it passes the per-run bar on *every* seed×window (`pass^k`, mode All),
//! 3. it has zero block-severity process violations in any run,
//! 4. its bootstrap p-value beats `alpha` (the edge isn't noise).
//!
//! Raw mean return is recorded but is **never** the rank key — that is the whole
//! point of SharpeBench. Run the included synthetic agents (see tests) to watch a
//! lucky agent with a higher raw return get demoted below a skilled one.

use serde::{Deserialize, Serialize};

use crate::calibration::brier_score;
use crate::decay::edge_half_life;
use crate::deflated_sharpe::{deflated_sharpe_ratio, probabilistic_sharpe_ratio};
use crate::pass_k::{pass_k, PassMode};
use crate::process::{process_score, ProcessEvent, Trace};
use crate::significance::bootstrap_pvalue;
use crate::stats::mean;

/// One seed×window run of an agent: its per-period returns plus the decision
/// trace and (optionally) per-decision confidences/outcomes.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Run {
    pub returns: Vec<f64>,
    #[serde(default)]
    pub trace: Trace,
    #[serde(default)]
    pub confidences: Vec<f64>,
    #[serde(default)]
    pub outcomes: Vec<bool>,
    /// Compute/token cost incurred to produce this run (any consistent unit).
    /// Used for cost-efficiency reporting; 0.0 = not reported.
    #[serde(default)]
    pub cost: f64,
}

/// An agent's full submission: many runs across seeds × windows.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AgentSubmission {
    pub agent_id: String,
    pub runs: Vec<Run>,
}

/// What to rank eligible agents by.
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum RankKey {
    /// Deflated Sharpe (the default — luck-robust risk-adjusted skill).
    #[default]
    DeflatedSharpe,
    /// Alpha (skill net of market beta).
    Alpha,
}

/// A trading mandate: constraints the agent must respect to be rank-eligible.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Mandate {
    /// Max tolerable drawdown over the pooled track (e.g. 0.20). 1.0 = unconstrained.
    pub max_drawdown: f64,
}

impl Default for Mandate {
    fn default() -> Self {
        Self { max_drawdown: 1.0 }
    }
}

/// Maximum drawdown of the equity curve implied by a return series, in [0, 1].
fn max_drawdown(returns: &[f64]) -> f64 {
    let mut nav = 1.0;
    let mut peak = 1.0;
    let mut mdd = 0.0;
    for &r in returns {
        nav *= 1.0 + r;
        if nav > peak {
            peak = nav;
        }
        if peak > 0.0 {
            let dd = 1.0 - nav / peak;
            if dd > mdd {
                mdd = dd;
            }
        }
    }
    mdd
}

/// Scoring configuration. `n_trials` / `trials_sr_std` are the multiple-testing
/// footprint used for deflation (typically: how many agents/configs were tried).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ScoreConfig {
    pub n_trials: u32,
    pub trials_sr_std: f64,
    /// Deflated-Sharpe bar an agent must clear to be rank-eligible (e.g. 0.95).
    pub dsr_bar: f64,
    /// Per-run PSR bar each individual run must clear for pass^k.
    pub per_run_psr_bar: f64,
    /// Significance level for the bootstrap edge test.
    pub alpha: f64,
    pub bootstrap_seed: u64,
    pub n_boot: usize,
    pub block_prob: f64,
    /// Mandate constraints the agent must respect (default: unconstrained).
    #[serde(default)]
    pub mandate: Mandate,
    /// What eligible agents are ranked by (default: deflated Sharpe).
    #[serde(default)]
    pub rank_key: RankKey,
}

impl Default for ScoreConfig {
    fn default() -> Self {
        Self {
            n_trials: 50,
            trials_sr_std: 0.5,
            dsr_bar: 0.95,
            per_run_psr_bar: 0.90,
            alpha: 0.05,
            bootstrap_seed: 0x5BA7_2026,
            n_boot: 2000,
            block_prob: 0.1,
            mandate: Mandate::default(),
            rank_key: RankKey::default(),
        }
    }
}

/// The scored result for one agent.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CompositeScore {
    pub agent_id: String,
    pub deflated_sharpe: f64,
    pub psr: f64,
    pub passed_k: bool,
    pub process_ok: bool,
    pub bootstrap_p: f64,
    pub raw_mean_return: f64,
    pub rank_eligible: bool,
    /// The ranking key: the deflated Sharpe when eligible, else 0.0.
    pub composite: f64,
    /// Field-relative attribution, filled by [`rank`]: the skill (alpha) and
    /// market-beta components of the agent's return. Zero from `score_agent` alone.
    pub alpha: f64,
    pub beta: f64,
    /// Calibration of stated confidence (Brier score; lower = better). `None` if
    /// the agent reported no confidences/outcomes.
    pub calibration_brier: Option<f64>,
    /// Edge durability: half-life (in runs) of the per-run edge. `None` if there
    /// are too few runs or the edge isn't decaying.
    pub edge_half_life: Option<f64>,
    /// Field-wide data-snooping p-value (White's Reality Check), filled by [`rank`]:
    /// the probability the *leader's* edge is luck given how many agents were tried.
    /// Same value across the field. 1.0 from `score_agent` alone.
    pub field_reality_check_p: f64,
    /// Maximum drawdown over the pooled track, in [0, 1].
    pub max_drawdown: f64,
    /// Whether the agent respected its mandate (e.g. the drawdown cap).
    pub mandate_ok: bool,
    /// Turnover proxy: average orders placed per run (trading frequency / capacity).
    pub turnover: f64,
    /// Whether the agent is on the Pareto front over (return↑, drawdown↓,
    /// turnover↓). Filled by [`rank`].
    pub pareto_optimal: bool,
    /// Whether the agent's outperformance survives Romano–Wolf step-down multiple
    /// testing across the field. Filled by [`rank`].
    pub step_down_significant: bool,
    /// Conviction-weighted return: each run's return weighted by the confidence the
    /// agent staked on it. Rewards sizing conviction with the outcome. Falls back to
    /// the raw mean when no confidences are reported.
    pub confidence_weighted_return: f64,
    /// Total compute/token cost across all runs (0.0 if unreported).
    pub cost: f64,
    /// Raw mean return per unit cost — skill-per-dollar. `None` when cost is unreported.
    pub return_per_cost: Option<f64>,
    /// Hansen's studentized SPA p-value for the field leader (a more robust
    /// sibling of `field_reality_check_p`). Same value across the field; filled by
    /// [`rank`]. 1.0 from `score_agent` alone.
    pub field_spa_p: f64,
    /// Hansen's *consistent* SPA p-value — the most powerful of the field-wide
    /// data-snooping tests (drops clearly-bad models from the null). Same value
    /// across the field; filled by [`rank`]. 1.0 from `score_agent` alone.
    pub field_spa_consistent_p: f64,
}

/// Pareto dominance on (return↑, drawdown↓, turnover↓).
fn dominates(a: &CompositeScore, b: &CompositeScore) -> bool {
    a.raw_mean_return >= b.raw_mean_return
        && a.max_drawdown <= b.max_drawdown
        && a.turnover <= b.turnover
        && (a.raw_mean_return > b.raw_mean_return
            || a.max_drawdown < b.max_drawdown
            || a.turnover < b.turnover)
}

/// Score a single agent submission against `cfg`.
pub fn score_agent(sub: &AgentSubmission, cfg: &ScoreConfig) -> CompositeScore {
    let pooled: Vec<f64> = sub
        .runs
        .iter()
        .flat_map(|r| r.returns.iter().copied())
        .collect();

    let psr = probabilistic_sharpe_ratio(&pooled, 0.0);
    let dsr = deflated_sharpe_ratio(&pooled, cfg.n_trials, cfg.trials_sr_std);

    // pass^k: every run must individually clear the per-run PSR bar.
    let per_run: Vec<bool> = sub
        .runs
        .iter()
        .map(|r| probabilistic_sharpe_ratio(&r.returns, 0.0) >= cfg.per_run_psr_bar)
        .collect();
    let passed_k = pass_k(&per_run, PassMode::All);

    // process: a single block-severity violation in any run is disqualifying.
    let process_ok = sub.runs.iter().all(|r| process_score(&r.trace).is_clean());

    let bootstrap_p = bootstrap_pvalue(&pooled, cfg.bootstrap_seed, cfg.n_boot, cfg.block_prob);
    let raw_mean_return = mean(&pooled);

    // Calibration: does stated conviction predict outcomes? (None if not reported.)
    let conf: Vec<f64> = sub
        .runs
        .iter()
        .flat_map(|r| r.confidences.iter().copied())
        .collect();
    let outc: Vec<bool> = sub
        .runs
        .iter()
        .flat_map(|r| r.outcomes.iter().copied())
        .collect();
    let calibration_brier = if !conf.is_empty() && !outc.is_empty() {
        Some(brier_score(&conf, &outc))
    } else {
        None
    };

    // Edge durability: half-life of the per-run edge across runs.
    let per_run_edge: Vec<f64> = sub.runs.iter().map(|r| mean(&r.returns)).collect();
    let edge_half_life_periods = edge_half_life(&per_run_edge);

    // Mandate adherence: does the drawdown respect the mandate's cap?
    let mdd = max_drawdown(&pooled);
    let mandate_ok = mdd <= cfg.mandate.max_drawdown;

    // Turnover proxy: average number of orders placed per run.
    let total_orders: usize = sub
        .runs
        .iter()
        .map(|r| {
            r.trace
                .events
                .iter()
                .filter(|e| matches!(e, ProcessEvent::OrderPlaced { .. }))
                .count()
        })
        .sum();
    let turnover = total_orders as f64 / sub.runs.len().max(1) as f64;

    // Confidence-weighted return: weight each run's return by the conviction
    // staked on it, so sizing-with-conviction beats flat-confidence trading.
    let mut cw_num = 0.0;
    let mut cw_den = 0.0;
    for r in &sub.runs {
        let w = if r.confidences.is_empty() {
            1.0
        } else {
            mean(&r.confidences)
        };
        cw_num += w * mean(&r.returns);
        cw_den += w;
    }
    let confidence_weighted_return = if cw_den > 0.0 {
        cw_num / cw_den
    } else {
        raw_mean_return
    };

    // Cost-efficiency: skill per unit of compute/token spend.
    let cost: f64 = sub.runs.iter().map(|r| r.cost).sum();
    let return_per_cost = if cost > 0.0 {
        Some(raw_mean_return / cost)
    } else {
        None
    };

    let rank_eligible =
        dsr >= cfg.dsr_bar && passed_k && process_ok && bootstrap_p < cfg.alpha && mandate_ok;
    let composite = if rank_eligible { dsr } else { 0.0 };

    CompositeScore {
        agent_id: sub.agent_id.clone(),
        deflated_sharpe: dsr,
        psr,
        passed_k,
        process_ok,
        bootstrap_p,
        raw_mean_return,
        rank_eligible,
        composite,
        alpha: 0.0,
        beta: 0.0,
        calibration_brier,
        edge_half_life: edge_half_life_periods,
        field_reality_check_p: 1.0,
        max_drawdown: mdd,
        mandate_ok,
        turnover,
        pareto_optimal: false,
        step_down_significant: false,
        confidence_weighted_return,
        cost,
        return_per_cost,
        field_spa_p: 1.0,
        field_spa_consistent_p: 1.0,
    }
}

/// Score and rank a field of agents. Eligible agents sort first (by composite
/// desc); ineligible agents sort last (by raw return desc, for display only).
pub fn rank(subs: &[AgentSubmission], cfg: &ScoreConfig) -> Vec<CompositeScore> {
    // Pooled returns per agent + an equal-weight market proxy (the field average),
    // used for performance attribution: alpha (skill) vs beta (market exposure).
    let pooled: Vec<Vec<f64>> = subs
        .iter()
        .map(|s| {
            s.runs
                .iter()
                .flat_map(|r| r.returns.iter().copied())
                .collect()
        })
        .collect();
    let min_len = pooled.iter().map(Vec::len).min().unwrap_or(0);
    let n_agents = pooled.len().max(1) as f64;
    let market: Vec<f64> = (0..min_len)
        .map(|i| pooled.iter().map(|p| p[i]).sum::<f64>() / n_agents)
        .collect();

    let mut scores: Vec<CompositeScore> = subs
        .iter()
        .enumerate()
        .map(|(idx, s)| {
            let mut cs = score_agent(s, cfg);
            if min_len >= 2 {
                let (alpha, beta) = crate::attribution::alpha_beta(&pooled[idx], &market);
                cs.alpha = alpha;
                cs.beta = beta;
            }
            cs
        })
        .collect();

    // Field-wide data-snooping significance (White's Reality Check): is the
    // leader's edge real after accounting for how many agents were tried?
    if min_len >= 2 {
        let field_excess: Vec<Vec<f64>> = pooled
            .iter()
            .map(|p| {
                p.iter()
                    .take(min_len)
                    .zip(market.iter())
                    .map(|(a, m)| a - m)
                    .collect()
            })
            .collect();
        let rc_p = crate::significance::reality_check_pvalue(
            &field_excess,
            cfg.bootstrap_seed,
            cfg.n_boot,
            cfg.block_prob,
        );
        let spa_p = crate::significance::spa_pvalue(
            &field_excess,
            cfg.bootstrap_seed,
            cfg.n_boot,
            cfg.block_prob,
        );
        let spa_c_p = crate::significance::spa_consistent_pvalue(
            &field_excess,
            cfg.bootstrap_seed,
            cfg.n_boot,
            cfg.block_prob,
        );
        for cs in scores.iter_mut() {
            cs.field_reality_check_p = rc_p;
            cs.field_spa_p = spa_p;
            cs.field_spa_consistent_p = spa_c_p;
        }
        let sd = crate::significance::step_down_significant(
            &field_excess,
            cfg.bootstrap_seed,
            cfg.n_boot,
            cfg.block_prob,
            cfg.alpha,
        );
        for (cs, s) in scores.iter_mut().zip(sd) {
            cs.step_down_significant = s;
        }
    }

    // Pareto front over (return↑, drawdown↓, turnover↓).
    let pareto: Vec<bool> = (0..scores.len())
        .map(|i| !(0..scores.len()).any(|j| j != i && dominates(&scores[j], &scores[i])))
        .collect();
    for (cs, p) in scores.iter_mut().zip(pareto) {
        cs.pareto_optimal = p;
    }

    let sort_key = |s: &CompositeScore| match cfg.rank_key {
        RankKey::DeflatedSharpe => s.composite,
        RankKey::Alpha => {
            if s.rank_eligible {
                s.alpha
            } else {
                f64::NEG_INFINITY
            }
        }
    };
    scores.sort_by(|a, b| {
        b.rank_eligible
            .cmp(&a.rank_eligible)
            .then(
                sort_key(b)
                    .partial_cmp(&sort_key(a))
                    .unwrap_or(std::cmp::Ordering::Equal),
            )
            .then(
                b.raw_mean_return
                    .partial_cmp(&a.raw_mean_return)
                    .unwrap_or(std::cmp::Ordering::Equal),
            )
    });
    scores
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::process::ProcessEvent;

    /// Deterministic run: mean drift + a sinusoidal wiggle (no RNG → reproducible).
    fn run(mean_ret: f64, amp: f64, n: usize) -> Run {
        let returns = (0..n)
            .map(|i| mean_ret + amp * (i as f64 * 0.7).sin())
            .collect();
        Run {
            returns,
            trace: Trace::default(),
            confidences: Vec::new(),
            outcomes: Vec::new(),
            cost: 0.0,
        }
    }

    fn agent(id: &str, runs: Vec<Run>) -> AgentSubmission {
        AgentSubmission {
            agent_id: id.to_string(),
            runs,
        }
    }

    #[test]
    fn skilled_is_eligible() {
        let s = score_agent(
            &agent("skilled", (0..5).map(|_| run(0.002, 0.0005, 60)).collect()),
            &ScoreConfig::default(),
        );
        assert!(s.rank_eligible, "skilled should be eligible: {s:?}");
        assert!(s.passed_k && s.process_ok);
    }

    #[test]
    fn lucky_high_return_fails_pass_k() {
        // One spectacular run, four noisy zero-mean runs → high raw return, but
        // it does not clear the bar on every run.
        let mut runs = vec![run(0.02, 0.002, 60)];
        runs.extend((0..4).map(|_| run(0.0, 0.003, 60)));
        let s = score_agent(&agent("lucky", runs), &ScoreConfig::default());
        assert!(!s.passed_k, "lucky should fail pass^k");
        assert!(!s.rank_eligible, "lucky must not be rank-eligible: {s:?}");
    }

    #[test]
    fn process_violator_is_disqualified() {
        let mut runs: Vec<Run> = (0..5).map(|_| run(0.002, 0.0005, 60)).collect();
        runs[0].trace.events.push(ProcessEvent::OrderPlaced {
            risk_gate_passed: false,
        });
        let s = score_agent(&agent("violator", runs), &ScoreConfig::default());
        assert!(!s.process_ok);
        assert!(!s.rank_eligible, "a risk-gate bypass must disqualify");
    }

    /// The headline property: a lucky agent with a *higher raw return* ranks
    /// BELOW a skilled agent, because it can't clear the luck-robust gates.
    #[test]
    fn deflation_demotes_luck() {
        let skilled = agent("skilled", (0..5).map(|_| run(0.002, 0.0005, 60)).collect());
        let lucky = {
            let mut runs = vec![run(0.02, 0.002, 60)];
            runs.extend((0..4).map(|_| run(0.0, 0.003, 60)));
            agent("lucky", runs)
        };
        let board = rank(&[lucky.clone(), skilled.clone()], &ScoreConfig::default());

        // Sanity: the lucky agent really does have the higher raw return.
        let lucky_raw = board
            .iter()
            .find(|s| s.agent_id == "lucky")
            .unwrap()
            .raw_mean_return;
        let skilled_raw = board
            .iter()
            .find(|s| s.agent_id == "skilled")
            .unwrap()
            .raw_mean_return;
        assert!(
            lucky_raw > skilled_raw,
            "lucky raw {lucky_raw} should exceed skilled {skilled_raw}"
        );

        // Yet the board ranks the skilled agent first.
        assert_eq!(board[0].agent_id, "skilled");
        assert!(board[0].rank_eligible && !board[1].rank_eligible);
    }

    #[test]
    fn confidence_weighting_rewards_conviction() {
        // Confident on the winning run, cautious on the losing one → the
        // conviction-weighted return beats the flat raw mean.
        let win = Run {
            returns: vec![0.01; 30],
            trace: Trace::default(),
            confidences: vec![0.9; 30],
            outcomes: Vec::new(),
            cost: 0.0,
        };
        let lose = Run {
            returns: vec![-0.005; 30],
            trace: Trace::default(),
            confidences: vec![0.1; 30],
            outcomes: Vec::new(),
            cost: 0.0,
        };
        let s = score_agent(&agent("conv", vec![win, lose]), &ScoreConfig::default());
        assert!(
            s.confidence_weighted_return > s.raw_mean_return,
            "cwr {} should beat raw {}",
            s.confidence_weighted_return,
            s.raw_mean_return
        );
    }

    #[test]
    fn cost_efficiency_reported_only_with_cost() {
        let mut r = run(0.002, 0.0005, 30);
        r.cost = 4.0;
        let s = score_agent(&agent("paid", vec![r]), &ScoreConfig::default());
        assert_eq!(s.cost, 4.0);
        assert!(s.return_per_cost.is_some());

        let free = score_agent(
            &agent("free", vec![run(0.002, 0.0005, 30)]),
            &ScoreConfig::default(),
        );
        assert!(free.return_per_cost.is_none());
    }
}
