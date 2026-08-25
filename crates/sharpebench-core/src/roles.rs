//! Multi-agent role attribution — which role in a trading team adds skill?
//!
//! A team submission (analyst, risk manager, PM, …) produces a team return plus
//! a return/signal series per role. We regress the team return on each role to
//! estimate that role's loading on the team outcome — a cheap, deterministic way
//! to see which role is load-bearing and which is dead weight. (After the
//! TradingAgents multi-agent firm structure.)
//!
//! Two producers feed [`attribute_roles`]:
//!
//! - **Live teams**: `sharpebench-harness` runs a simulated multi-agent team and
//!   records one return series per named role, the input this analyzer was
//!   designed for.
//! - **Frozen single-agent submissions**: [`elicit_behavior_roles`] derives
//!   *behavior* roles from what a recorded [`Run`] actually contains. Each run
//!   is classified from its trace's order pattern (block-violating, warned,
//!   idle, or clean-active), runs sharing a class are averaged into one return
//!   stream per class, and the team stream is the equal-weight average of all
//!   runs. [`attribute_behavior_roles`] then answers a question a frozen score
//!   can honestly ask: which *behavior* carries the pooled result, e.g. is the
//!   edge load-bearing on the runs that breached limits?
//!
//! **What cannot be derived from a recorded trace.** True per-member role
//! attribution (analyst vs risk manager vs PM) needs a return or signal series
//! *per role, aligned to the team's periods*. The trace records neither role
//! labels nor per-period alignment (its events carry no period index), so a
//! frozen submission cannot support it without inventing that structure. To
//! record it, a submission would need per-role return streams alongside the
//! team's, which is the shape the harness's live team runner already produces.

use serde::{Deserialize, Serialize};

use crate::attribution::alpha_beta;
use crate::composite::Run;
use crate::process::ProcessEvent;
use crate::stats::mean;

/// One role's contribution to a team.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RoleContribution {
    pub role: String,
    /// Regression beta of the team return on this role — how much the team moves
    /// per unit of this role's signal. Near 0 ⇒ the role isn't load-bearing.
    pub beta_to_team: f64,
    pub mean_return: f64,
}

/// Attribute a team's return to its roles.
pub fn attribute_roles(team: &[f64], roles: &[(String, Vec<f64>)]) -> Vec<RoleContribution> {
    roles
        .iter()
        .map(|(name, r)| {
            let (_, beta) = alpha_beta(team, r);
            RoleContribution {
                role: name.clone(),
                beta_to_team: beta,
                mean_return: mean(r),
            }
        })
        .collect()
}

/// The behavior classes a recorded run's trace can be sorted into, in fixed
/// output order. Precedence when a trace matches several: block-violating, then
/// warned, then idle vs clean-active by whether any order reached the venue.
const BEHAVIOR_ROLES: [&str; 4] = ["clean_active", "idle", "warned", "block_violating"];

fn behavior_role(run: &Run) -> &'static str {
    let events = &run.trace.events;
    if events.iter().any(ProcessEvent::is_block_violation) {
        return "block_violating";
    }
    if events.iter().any(ProcessEvent::is_warn_violation) {
        return "warned";
    }
    let placed_order = events
        .iter()
        .any(|e| matches!(e, ProcessEvent::OrderPlaced { .. }));
    if placed_order {
        "clean_active"
    } else {
        "idle"
    }
}

/// Derive behavior roles from a frozen submission's runs: one `(role, returns)`
/// stream per populated behavior class, each the equal-weight average of its
/// member runs, truncated to the shortest run so every stream aligns with the
/// team stream period by period. Empty when there are no runs or the shortest
/// run has fewer than 2 periods (no regression is estimable). Deterministic:
/// classes appear in the fixed `BEHAVIOR_ROLES` order, and averaging follows
/// run submission order.
pub fn elicit_behavior_roles(runs: &[Run]) -> Vec<(String, Vec<f64>)> {
    let Some(min_len) = runs.iter().map(|r| r.returns.len()).min() else {
        return Vec::new();
    };
    if min_len < 2 {
        return Vec::new();
    }
    BEHAVIOR_ROLES
        .iter()
        .filter_map(|role| {
            let members: Vec<&Run> = runs.iter().filter(|r| behavior_role(r) == *role).collect();
            if members.is_empty() {
                return None;
            }
            let n = members.len() as f64;
            let avg: Vec<f64> = (0..min_len)
                .map(|i| members.iter().map(|r| r.returns[i]).sum::<f64>() / n)
                .collect();
            Some(((*role).to_string(), avg))
        })
        .collect()
}

/// [`elicit_behavior_roles`] fed into [`attribute_roles`], with the team stream
/// the equal-weight average across all runs (same truncation). Answers, from
/// recorded data alone: which behavior class is load-bearing for the pooled
/// result? Reported on `CompositeScore`, never gating.
pub fn attribute_behavior_roles(runs: &[Run]) -> Vec<RoleContribution> {
    let roles = elicit_behavior_roles(runs);
    if roles.is_empty() {
        return Vec::new();
    }
    let min_len = roles.iter().map(|(_, r)| r.len()).min().unwrap_or(0);
    let n = runs.len() as f64;
    let team: Vec<f64> = (0..min_len)
        .map(|i| runs.iter().map(|r| r.returns[i]).sum::<f64>() / n)
        .collect();
    attribute_roles(&team, &roles)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::process::Trace;

    #[test]
    fn load_bearing_role_dominates() {
        let team: Vec<f64> = (0..40).map(|i| 0.001 * (i as f64 * 0.3).sin()).collect();
        let roles = vec![
            ("driver".to_string(), team.clone()),
            (
                "noise".to_string(),
                (0..40).map(|i| 0.001 * (i as f64 * 1.7).cos()).collect(),
            ),
        ];
        let attr = attribute_roles(&team, &roles);
        assert!(
            (attr[0].beta_to_team - 1.0).abs() < 1e-6,
            "driver={:?}",
            attr[0]
        );
        assert!(
            attr[0].beta_to_team.abs() > attr[1].beta_to_team.abs(),
            "driver should out-load noise"
        );
    }

    fn run_with(returns: Vec<f64>, events: Vec<ProcessEvent>) -> Run {
        Run {
            returns,
            trace: Trace { events },
            ..Run::default()
        }
    }

    fn signal(n: usize) -> Vec<f64> {
        (0..n)
            .map(|i| 0.002 + 0.003 * (i as f64 * 0.7).sin())
            .collect()
    }

    #[test]
    fn behavior_classification_follows_severity_precedence() {
        let ok_order = ProcessEvent::OrderPlaced {
            risk_gate_passed: true,
        };
        assert_eq!(
            behavior_role(&run_with(signal(4), vec![ok_order.clone()])),
            "clean_active"
        );
        assert_eq!(behavior_role(&run_with(signal(4), vec![])), "idle");
        assert_eq!(
            behavior_role(&run_with(
                signal(4),
                vec![ok_order.clone(), ProcessEvent::ConcentrationBreach]
            )),
            "warned"
        );
        assert_eq!(
            behavior_role(&run_with(
                signal(4),
                vec![
                    ProcessEvent::ConcentrationBreach,
                    ProcessEvent::DenylistBypass
                ]
            )),
            "block_violating"
        );
    }

    #[test]
    fn warned_runs_carrying_the_edge_are_load_bearing() {
        // Two warned runs carry the whole signal; two clean-active runs are flat.
        // The elicited attribution must load the "warned" role, not the clean one.
        let ok_order = ProcessEvent::OrderPlaced {
            risk_gate_passed: true,
        };
        let runs = vec![
            run_with(vec![0.0; 40], vec![ok_order.clone()]),
            run_with(
                signal(40),
                vec![ok_order.clone(), ProcessEvent::ConcentrationBreach],
            ),
            run_with(vec![0.0; 40], vec![ok_order.clone()]),
            run_with(
                signal(40),
                vec![ok_order, ProcessEvent::ConcentrationBreach],
            ),
        ];
        let attr = attribute_behavior_roles(&runs);
        assert_eq!(attr.len(), 2);
        assert_eq!(attr[0].role, "clean_active");
        assert_eq!(attr[1].role, "warned");
        // Team = warned/2, and beta is "team moved per unit of role signal", so
        // the warned stream loads at 0.5; the flat clean stream (zero variance)
        // regresses at 0.
        assert!((attr[1].beta_to_team - 0.5).abs() < 1e-9, "{attr:?}");
        assert!(attr[0].beta_to_team.abs() < 1e-9, "{attr:?}");
        assert!(
            attr[1].beta_to_team.abs() > attr[0].beta_to_team.abs(),
            "the warned role must out-load the clean one"
        );
    }

    #[test]
    fn elicitor_is_deterministic_and_declines_degenerate_input() {
        let ok_order = ProcessEvent::OrderPlaced {
            risk_gate_passed: true,
        };
        let runs = vec![
            run_with(signal(30), vec![ok_order.clone()]),
            run_with(signal(40), vec![]),
        ];
        let a = attribute_behavior_roles(&runs);
        let b = attribute_behavior_roles(&runs);
        assert_eq!(a, b);
        // Streams are truncated to the shortest run.
        let elicited = elicit_behavior_roles(&runs);
        assert!(elicited.iter().all(|(_, r)| r.len() == 30));

        assert!(attribute_behavior_roles(&[]).is_empty());
        let short = vec![run_with(vec![0.01], vec![ok_order])];
        assert!(attribute_behavior_roles(&short).is_empty());
    }
}
