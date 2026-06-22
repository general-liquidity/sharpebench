//! Benchmark self-audit — does SharpeBench resist being gamed?
//!
//! Most agent benchmarks can be gamed: a model that learns the judge's biases,
//! a submission tuned to a single lucky seed, a strategy that wins by ignoring
//! risk limits. The integrity literature (BenchJack; Berkeley RDI's survey of
//! eight gameable benchmarks) shows this is the norm, not the exception.
//!
//! SharpeBench is judge-free and deterministic, so its defenses are *assertions*,
//! not opinions. This module fires a battery of known attacks at the live scorer
//! and checks each one is demoted. It ships with the benchmark (CLI `audit`) so
//! anyone can re-run the integrity proof — and so a future change that silently
//! weakens a gate fails the audit instead of passing unnoticed.

use serde::Serialize;

use crate::composite::{rank, score_agent, AgentSubmission, Mandate, Run, ScoreConfig};
use crate::process::{ProcessEvent, Trace};

/// One attack and whether the scorer defended against it.
#[derive(Clone, Debug, Serialize)]
pub struct AuditCase {
    pub name: String,
    /// What the attacker tries to exploit.
    pub attack: String,
    /// Whether the scorer demoted the attack as intended.
    pub defended: bool,
    pub detail: String,
}

/// The full self-audit result.
#[derive(Clone, Debug, Serialize)]
pub struct SelfAuditReport {
    pub cases: Vec<AuditCase>,
    pub all_defended: bool,
}

fn run_with(returns: Vec<f64>, trace: Trace) -> Run {
    Run {
        returns,
        trace,
        confidences: Vec::new(),
        outcomes: Vec::new(),
        cost: 0.0,
    }
}

/// A clean, steadily-skilled run: positive drift with a small wiggle.
fn skilled_run(n: usize) -> Run {
    run_with(
        (0..n)
            .map(|i| 0.002 + 0.0005 * (i as f64 * 0.7).sin())
            .collect(),
        Trace::default(),
    )
}

fn agent(id: &str, runs: Vec<Run>) -> AgentSubmission {
    AgentSubmission {
        agent_id: id.to_string(),
        runs,
    }
}

/// Run every attack against the scorer and report whether each was demoted.
pub fn run_self_audit() -> SelfAuditReport {
    let cfg = ScoreConfig::default();
    let mut cases = Vec::new();

    // 1) Luck, not skill: one spectacular run + noise → highest raw return, yet
    //    must rank below a steadily-skilled agent and be ineligible.
    {
        let lucky = {
            let mut runs = vec![run_with(
                (0..60)
                    .map(|i| 0.02 + 0.002 * (i as f64 * 0.7).sin())
                    .collect(),
                Trace::default(),
            )];
            runs.extend((0..4).map(|_| {
                run_with(
                    (0..60).map(|i| 0.003 * (i as f64 * 0.7).sin()).collect(),
                    Trace::default(),
                )
            }));
            agent("lucky", runs)
        };
        let skilled = agent("skilled", (0..5).map(|_| skilled_run(60)).collect());
        let board = rank(&[lucky, skilled], &cfg);
        let lucky_s = board.iter().find(|s| s.agent_id == "lucky").unwrap();
        let skilled_s = board.iter().find(|s| s.agent_id == "skilled").unwrap();
        let defended = board[0].agent_id == "skilled"
            && !lucky_s.rank_eligible
            && lucky_s.raw_mean_return > skilled_s.raw_mean_return;
        cases.push(AuditCase {
            name: "luck-not-skill".into(),
            attack: "win on a single lucky seed with the highest raw return".into(),
            defended,
            detail: format!(
                "lucky raw={:.4} eligible={}, skilled ranks #1={}",
                lucky_s.raw_mean_return, lucky_s.rank_eligible, defended
            ),
        });
    }

    // 2) Risk-gate bypass: place an order that skipped the risk gate.
    {
        let mut runs: Vec<Run> = (0..5).map(|_| skilled_run(60)).collect();
        runs[0].trace.events.push(ProcessEvent::OrderPlaced {
            risk_gate_passed: false,
        });
        let s = score_agent(&agent("bypass", runs), &cfg);
        cases.push(AuditCase {
            name: "risk-gate-bypass".into(),
            attack: "place an order that skipped the pre-trade risk gate".into(),
            defended: !s.rank_eligible && !s.process_ok,
            detail: format!("process_ok={}, eligible={}", s.process_ok, s.rank_eligible),
        });
    }

    // 3) Simulator exploitation: a manipulative order (block-severity).
    {
        let mut runs: Vec<Run> = (0..5).map(|_| skilled_run(60)).collect();
        runs[0].trace.events.push(ProcessEvent::ManipulativeOrder);
        let s = score_agent(&agent("manip", runs), &cfg);
        cases.push(AuditCase {
            name: "sim-exploitation".into(),
            attack: "submit a manipulative / absurd-size order to exploit the sim".into(),
            defended: !s.rank_eligible && !s.process_ok,
            detail: format!("process_ok={}, eligible={}", s.process_ok, s.rank_eligible),
        });
    }

    // 4) Mandate breach: blow through the drawdown cap to chase return.
    {
        let tight = ScoreConfig {
            mandate: Mandate { max_drawdown: 0.05 },
            ..cfg.clone()
        };
        let mut returns: Vec<f64> = (0..60).map(|_| 0.003).collect();
        returns[30] = -0.20; // a 20% drop blows a 5% cap
        let s = score_agent(
            &agent("breacher", vec![run_with(returns, Trace::default())]),
            &tight,
        );
        cases.push(AuditCase {
            name: "mandate-breach".into(),
            attack: "exceed the drawdown mandate to reach for return".into(),
            defended: !s.rank_eligible && !s.mandate_ok,
            detail: format!(
                "mdd={:.3} mandate_ok={}, eligible={}",
                s.max_drawdown, s.mandate_ok, s.rank_eligible
            ),
        });
    }

    // 5) Inconsistency: huge raw return concentrated in one run → fails pass^k.
    {
        let mut runs = vec![run_with(
            (0..60)
                .map(|i| 0.03 + 0.002 * (i as f64 * 0.7).sin())
                .collect(),
            Trace::default(),
        )];
        runs.extend((0..4).map(|_| {
            run_with(
                (0..60).map(|i| 0.004 * (i as f64 * 0.7).sin()).collect(),
                Trace::default(),
            )
        }));
        let s = score_agent(&agent("spiky", runs), &cfg);
        cases.push(AuditCase {
            name: "raw-return-cannot-buy-rank".into(),
            attack: "post the biggest raw return but only on some runs".into(),
            defended: !s.rank_eligible && !s.passed_k,
            detail: format!(
                "raw={:.4} passed_k={}, eligible={}",
                s.raw_mean_return, s.passed_k, s.rank_eligible
            ),
        });
    }

    let all_defended = cases.iter().all(|c| c.defended);
    SelfAuditReport {
        cases,
        all_defended,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn benchmark_resists_every_known_attack() {
        let report = run_self_audit();
        for c in &report.cases {
            assert!(c.defended, "undefended attack: {} — {}", c.name, c.detail);
        }
        assert!(report.all_defended);
    }
}
