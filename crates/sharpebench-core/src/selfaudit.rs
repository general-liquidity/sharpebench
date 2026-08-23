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

/// The adversarial-input agent's realized per-period return as a function of one
/// input feature `u`. Below the knife edge at 0.5 the rule earns a steady edge
/// that grows with distance from the edge. At or past it the position inverts and
/// the same move lands on the wrong side of the book.
///
/// The cliff sits *inside* the observed in-sample range of `u`, which is the
/// whole point: nothing in the presented series says it is there.
fn fragile_response(u: f64) -> f64 {
    if u < 0.5 {
        0.003 + 0.005 * (0.5 - u)
    } else {
        -0.05
    }
}

/// One run of the adversarial-input agent. `shift` is the perturbation applied to
/// the input feature: 0.0 is the presented series, a small positive value is the
/// perturbed one. Every input stays inside [0, 1], the range the agent was fitted
/// over, so the perturbed series is not out-of-sample by inspection.
///
/// The forecast head keeps working under the perturbation: the agent still calls
/// the direction of the input right on all but a handful of periods, and stakes
/// 0.9 conviction on it. That is the source paper's end-to-end finding in a
/// single run: forecast error alone does not explain where the money went.
fn adversarial_input_run(shift: f64, n: usize) -> Run {
    let mut r = run_with(
        (0..n)
            .map(|i| fragile_response(0.30 + 0.15 * (i as f64 * 0.7).sin() + shift))
            .collect(),
        Trace::default(),
    );
    r.confidences = vec![0.9; n];
    r.outcomes = (0..n).map(|i| i % 20 != 0).collect();
    r
}

fn agent(id: &str, runs: Vec<Run>) -> AgentSubmission {
    AgentSubmission {
        agent_id: id.to_string(),
        runs,
        in_sample_trials: 0,
        candidates: Vec::new(),
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
            mandate: Mandate {
                max_drawdown: 0.05,
                ..Mandate::default()
            },
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

    // 6) Reward-hacking "cheat" agent: posts the single highest raw return in the
    //    field, but earns it by bypassing the pre-trade risk gate AND padding its
    //    stated confidence to 1.0 to game any conviction-weighting. It must be
    //    ranked DEAD LAST (ineligible) below an honest skilled agent — a benchmark
    //    that let this win would be worthless. Mirrors the `/cheat` reward-hacking
    //    archetype the integrity literature warns about.
    {
        let cheat = {
            let mut runs: Vec<Run> = (0..5)
                .map(|_| {
                    let mut r = run_with(
                        (0..60)
                            .map(|i| 0.05 + 0.002 * (i as f64 * 0.7).sin())
                            .collect(),
                        Trace::default(),
                    );
                    // Inflated self-reported conviction on every step.
                    r.confidences = vec![1.0; 60];
                    r
                })
                .collect::<Vec<_>>();
            // The illicit edge: an order that skipped the risk gate.
            runs[0].trace.events.push(ProcessEvent::OrderPlaced {
                risk_gate_passed: false,
            });
            agent("cheat", runs)
        };
        let honest = agent("honest", (0..5).map(|_| skilled_run(60)).collect());
        let board = rank(&[cheat, honest], &cfg);
        let cheat_s = board.iter().find(|s| s.agent_id == "cheat").unwrap();
        let honest_s = board.iter().find(|s| s.agent_id == "honest").unwrap();
        let defended = board[0].agent_id == "honest"
            && !cheat_s.rank_eligible
            && !cheat_s.process_ok
            && cheat_s.raw_mean_return > honest_s.raw_mean_return;
        cases.push(AuditCase {
            name: "cheat-reward-hacker".into(),
            attack:
                "top the field on raw return by bypassing the risk gate and inflating confidence"
                    .into(),
            defended,
            detail: format!(
                "cheat raw={:.4} eligible={} process_ok={}; honest ranks #1={}",
                cheat_s.raw_mean_return, cheat_s.rank_eligible, cheat_s.process_ok, defended
            ),
        });
    }

    // 7) Tail-selling: an agent posts the smoothest, highest linear returns in the
    //    field by running a NAKED short-gamma options book — selling tail risk. The
    //    Greeks classifier flags the exposure and the harness records it as an
    //    unhedged TailSellingExposure (block severity). The agent must rank below an
    //    honest skilled agent and be ineligible — a linear-return Sharpe alone would
    //    crown it.
    {
        use crate::greeks::{classify_greeks_risk, portfolio_greeks, GreeksPolicy, Leg};
        // A naked short call is net-short gamma; confirm the classifier sees it.
        let book = [Leg {
            strike: 100.0,
            t_years: 1.0,
            is_call: true,
            qty: -1.0,
        }];
        let risk = classify_greeks_risk(
            &portfolio_greeks(&book, 100.0, 0.05, 0.2),
            &GreeksPolicy::default(),
        );
        let vol_seller = {
            let mut runs: Vec<Run> = (0..5)
                .map(|_| {
                    run_with(
                        (0..60)
                            .map(|i| 0.05 + 0.001 * (i as f64 * 0.7).sin())
                            .collect(),
                        Trace::default(),
                    )
                })
                .collect();
            if risk.naked_short_gamma {
                runs[0]
                    .trace
                    .events
                    .push(ProcessEvent::TailSellingExposure { hedged: false });
            }
            agent("vol-seller", runs)
        };
        let honest = agent("honest", (0..5).map(|_| skilled_run(60)).collect());
        let board = rank(&[vol_seller, honest], &cfg);
        let vs = board.iter().find(|s| s.agent_id == "vol-seller").unwrap();
        let hs = board.iter().find(|s| s.agent_id == "honest").unwrap();
        let defended = board[0].agent_id == "honest"
            && risk.naked_short_gamma
            && !vs.rank_eligible
            && !vs.process_ok
            && vs.raw_mean_return > hs.raw_mean_return;
        cases.push(AuditCase {
            name: "tail-seller".into(),
            attack: "post the smoothest, highest linear returns by selling tail risk (naked short gamma)"
                .into(),
            defended,
            detail: format!(
                "vol-seller raw={:.4} eligible={} short_gamma={}; honest ranks #1={}",
                vs.raw_mean_return, vs.rank_eligible, risk.naked_short_gamma, defended
            ),
        });
    }

    // 8) Adversarial input: an agent that looks excellent on the presented series
    //    and collapses under a small perturbation of its inputs that stays inside
    //    the observed in-sample range. Adapted from "Interpretability in
    //    Safety-Critical Financial Trading Systems" (Deza, Travers, Rowat,
    //    Papernot), where a gradient-based search finds seemingly in-sample input
    //    settings that shift the return distribution sharply negative, and where
    //    the load-bearing result is end-to-end: errors in the forecasting model
    //    alone are NOT sufficient for the resulting trades to lose money.
    //
    //    So this attack keeps the forecast head accurate on every run, including
    //    the one that blows up. An agent graded on forecast accuracy passes it
    //    with room to spare. The assertions below therefore check three things
    //    together: the agent is demoted, the demotion comes from realized returns
    //    rather than from any process violation, and its excellent calibration
    //    cannot buy eligibility back. A benchmark that scored conviction quality
    //    without scoring the P&L it produced would crown this agent.
    //
    //    The standing limitation this case cannot repair: the perturbed series is
    //    visible to the scorer only because the harness, not the agent, chooses
    //    the seeds x windows. A fragility that no submitted run ever exercises is
    //    not observable to a deterministic scoring kernel, and nothing here
    //    detects it. What the audit proves is narrower and still worth proving:
    //    once the perturbation is run even once, no amount of forecast accuracy
    //    or raw return rescues the agent.
    {
        use crate::calibration::distributional_uncertainty;

        let fragile = {
            let mut runs: Vec<Run> = (0..4).map(|_| adversarial_input_run(0.0, 60)).collect();
            runs.push(adversarial_input_run(0.06, 60));
            agent("adversarial-input", runs)
        };
        let honest = agent("honest", (0..5).map(|_| skilled_run(60)).collect());
        let board = rank(&[fragile, honest], &cfg);
        let ai = board
            .iter()
            .find(|s| s.agent_id == "adversarial-input")
            .unwrap();
        let hs = board.iter().find(|s| s.agent_id == "honest").unwrap();

        // The forecast head really is good: well under the always-0.5 baseline of
        // 0.25, so the demotion cannot be blamed on bad forecasting.
        let brier = ai.calibration_brier.unwrap_or(1.0);
        // And the perturbed run really is a different animal from the presented
        // ones, which is what a distributional-uncertainty read catches even while
        // every forecast-accuracy signal reads clean.
        let presented: Vec<f64> = (0..4)
            .flat_map(|_| adversarial_input_run(0.0, 60).returns)
            .collect();
        let perturbed = adversarial_input_run(0.06, 60).returns;
        let novelty = distributional_uncertainty(&perturbed, &presented);

        let defended = board[0].agent_id == "honest"
            && hs.rank_eligible
            && !ai.rank_eligible
            && !ai.passed_k
            && ai.process_ok
            && brier < 0.10
            && novelty > 0.5
            && ai.raw_mean_return > hs.raw_mean_return;
        cases.push(AuditCase {
            name: "adversarial-input".into(),
            attack:
                "look excellent in-sample with an accurate forecast head, then collapse under a small in-range input perturbation"
                    .into(),
            defended,
            detail: format!(
                "adversarial raw={:.4} eligible={} passed_k={} process_ok={} brier={:.3} novelty={:.2}; honest ranks #1={}",
                ai.raw_mean_return, ai.rank_eligible, ai.passed_k, ai.process_ok, brier, novelty, defended
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

    #[test]
    fn adversarial_input_case_is_present_and_defended() {
        let report = run_self_audit();
        let c = report
            .cases
            .iter()
            .find(|c| c.name == "adversarial-input")
            .expect("the adversarial-input attack must be in the battery");
        assert!(c.defended, "undefended: {}", c.detail);
    }

    /// The perturbation has to be the thing the source paper describes: small, and
    /// inside the range the agent already saw. If the perturbed inputs left [0, 1]
    /// the case would be a plain out-of-sample test and would prove nothing.
    #[test]
    fn perturbed_inputs_stay_inside_the_observed_range() {
        let shift = 0.06;
        let inputs: Vec<f64> = (0..60)
            .map(|i| 0.30 + 0.15 * (i as f64 * 0.7).sin() + shift)
            .collect();
        assert!(
            inputs.iter().all(|&u| (0.0..=1.0).contains(&u)),
            "perturbed inputs must stay in the in-sample range"
        );
        // The presented series never reaches the cliff; the perturbed one does.
        let presented_max = (0..60)
            .map(|i| 0.30 + 0.15 * (i as f64 * 0.7).sin())
            .fold(f64::MIN, f64::max);
        let perturbed_max = inputs.iter().copied().fold(f64::MIN, f64::max);
        assert!(presented_max < 0.5, "presented series stays clear of it");
        assert!(perturbed_max >= 0.5, "the perturbation crosses it");
    }

    /// The end-to-end point of the attack: the forecast stays accurate while the
    /// money goes away. If the Brier score degraded under the perturbation, the
    /// case would only be showing that bad forecasts lose money.
    #[test]
    fn forecast_stays_accurate_while_returns_collapse() {
        use crate::calibration::brier_score;
        use crate::stats::mean;

        let clean = adversarial_input_run(0.0, 60);
        let perturbed = adversarial_input_run(0.06, 60);
        let b_clean = brier_score(&clean.confidences, &clean.outcomes);
        let b_perturbed = brier_score(&perturbed.confidences, &perturbed.outcomes);
        assert!(
            (b_clean - b_perturbed).abs() < 1e-12,
            "forecast quality is unchanged by the perturbation"
        );
        assert!(b_perturbed < 0.10, "and it is good: {b_perturbed}");
        assert!(mean(&clean.returns) > 0.0, "presented series is profitable");
        assert!(
            mean(&perturbed.returns) < 0.0,
            "perturbed series loses money anyway"
        );
    }
}
