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

use crate::deflated_sharpe::{deflated_sharpe_ratio, probabilistic_sharpe_ratio};
use crate::pass_k::{pass_k, PassMode};
use crate::process::{process_score, Trace};
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
}

/// An agent's full submission: many runs across seeds × windows.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AgentSubmission {
    pub agent_id: String,
    pub runs: Vec<Run>,
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
        }
    }
}

/// The scored result for one agent.
#[derive(Clone, Debug, Serialize)]
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

    let rank_eligible = dsr >= cfg.dsr_bar && passed_k && process_ok && bootstrap_p < cfg.alpha;
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
    scores.sort_by(|a, b| {
        b.rank_eligible
            .cmp(&a.rank_eligible)
            .then(
                b.composite
                    .partial_cmp(&a.composite)
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
}
