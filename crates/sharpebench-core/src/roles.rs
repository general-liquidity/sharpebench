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
//!   idle, or clean-active). [`attribute_behavior_roles`] then answers a
//!   question a frozen score can honestly ask: which *behavior* carries the
//!   pooled result, e.g. is the edge load-bearing on the runs that breached
//!   limits?
//!
//! **The behavior path is dated by window, not by run ordinal.** A submission's
//! runs are window-major blocks of `execution_seeds_per_window` execution
//! replicates (see [`crate::composite::pooled_returns`]): replicates within a
//! block are repeated executions of the *same* frozen market window, and
//! different blocks are different market windows. Only the first of those is a
//! period-aligned repeat measurement. So a class stream is built by averaging
//! its members *within each window*, and windows are then concatenated in window
//! order, which is exactly the axis the pooled track is reported on. Nothing is
//! averaged across windows and nothing is truncated across windows, so a short
//! window can no longer delete the tail of a long one.
//!
//! **When it is not estimable.** Inside one window, a class loading separates
//! the class from the window only if the window also contains replicates of
//! another class. With one execution per window (the default
//! `execution_seeds_per_window = 1`), every window is single-class, the class
//! stream on its retained windows *is* the team stream there, and every loading
//! would be 1.0 by construction. Such a class is dropped rather than reported:
//! behavior is confounded with the window, and the confounded number was the
//! substance of the audit finding, not its presentation.
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
    /// Regression beta of the team return on this role: how much the team moves
    /// per unit of this role's signal. Near 0 means the role is not load-bearing
    /// on this sample.
    ///
    /// A marginal association from one univariate fit against a regressor that
    /// is not orthogonal to the others, so these loadings do NOT decompose the
    /// team return into additive parts and do not identify what the role caused.
    /// The type and function names read as attribution; the arithmetic delivers
    /// association. Read it beside `periods`, which is the sample it came from.
    pub beta_to_team: f64,
    pub mean_return: f64,
    /// Observations the loading was estimated on. Roles are not required to
    /// share a sample: a behavior class is estimated on the windows it occurs
    /// in, so this can be shorter than the pooled track, and comparing it with
    /// `CompositeScore::pooled_observations` says by how much.
    #[serde(default)]
    pub periods: usize,
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
                periods: team.len().min(r.len()),
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

/// One behavior class paired with the team stream on the *same* retained
/// sample: the windows that class occurs in, in window order, each window
/// contributing its own periods.
#[derive(Clone, Debug, PartialEq)]
pub struct BehaviorRoleStream {
    pub role: String,
    /// Window-averaged team returns over the retained windows. A whole-window
    /// subsequence of the pooled track, never a cross-window average.
    pub team: Vec<f64>,
    /// The class's window-averaged returns over exactly those windows.
    pub returns: Vec<f64>,
}

/// Derive dated behavior-role streams from a frozen submission's runs.
///
/// `seeds_per_window` is the submission's `execution_seeds_per_window`: runs are
/// window-major blocks of that many execution replicates. Within a window the
/// class stream is the equal-weight average of that class's replicates and the
/// team stream is the equal-weight average of all of them; windows are then
/// concatenated in window order. A window whose replicates disagree in length is
/// truncated to its own shortest replicate, which cannot reach across windows.
///
/// A class is returned only when it is estimable: at least one retained window
/// must also contain a replicate of some other class, and the retained sample
/// must hold at least 2 periods. Empty when the run count is not a whole number
/// of windows, since the declared window axis then does not describe the runs.
/// Deterministic: classes appear in the fixed `BEHAVIOR_ROLES` order and
/// averaging follows run submission order.
pub fn elicit_behavior_roles(runs: &[Run], seeds_per_window: usize) -> Vec<BehaviorRoleStream> {
    let width = seeds_per_window.max(1);
    if runs.is_empty() || !runs.len().is_multiple_of(width) {
        return Vec::new();
    }
    let windows: Vec<&[Run]> = runs.chunks_exact(width).collect();
    BEHAVIOR_ROLES
        .iter()
        .filter_map(|role| {
            let mut team = Vec::new();
            let mut returns = Vec::new();
            let mut discriminating = false;
            for window in &windows {
                let members: Vec<&Run> = window
                    .iter()
                    .filter(|r| behavior_role(r) == *role)
                    .collect();
                if members.is_empty() {
                    continue;
                }
                if members.len() < window.len() {
                    discriminating = true;
                }
                let len = window
                    .iter()
                    .map(|r| r.returns.len())
                    .min()
                    .unwrap_or_default();
                let all = window.len() as f64;
                let mine = members.len() as f64;
                for t in 0..len {
                    team.push(window.iter().map(|r| r.returns[t]).sum::<f64>() / all);
                    returns.push(members.iter().map(|r| r.returns[t]).sum::<f64>() / mine);
                }
            }
            if !discriminating || team.len() < 2 {
                return None;
            }
            Some(BehaviorRoleStream {
                role: (*role).to_string(),
                team,
                returns,
            })
        })
        .collect()
}

/// [`elicit_behavior_roles`] regressed class by class against the team stream on
/// that class's own retained windows. Answers, from recorded data alone: which
/// behavior class is load-bearing for the pooled result? Reported on
/// `CompositeScore`, never gating.
pub fn attribute_behavior_roles(runs: &[Run], seeds_per_window: usize) -> Vec<RoleContribution> {
    elicit_behavior_roles(runs, seeds_per_window)
        .into_iter()
        .map(|stream| {
            // Exact alignment, asserted rather than assumed. `alpha_beta` pairs
            // by index and silently truncates to the shorter series, so a role
            // stream one period short would be regressed against a mismatched
            // team prefix with no signal at all. `elicit_behavior_roles` builds
            // both from the same retained windows, so a mismatch here is a bug
            // in this module, not bad input.
            debug_assert_eq!(
                stream.team.len(),
                stream.returns.len(),
                "role {} and its team stream must cover the same retained sample",
                stream.role
            );
            let paired = stream.team.len().min(stream.returns.len());
            let (_, beta) = alpha_beta(&stream.team[..paired], &stream.returns[..paired]);
            RoleContribution {
                role: stream.role,
                beta_to_team: beta,
                mean_return: mean(&stream.returns),
                periods: stream.returns.len(),
            }
        })
        .collect()
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
        // Four execution replicates of ONE market window: two warned runs carry
        // the whole signal, two clean-active runs are flat. The elicited
        // attribution must load the "warned" role, not the clean one. This is
        // the input shape the diagnostic was always implicitly assuming, and its
        // numbers are pinned here unchanged.
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
        let attr = attribute_behavior_roles(&runs, 4);
        assert_eq!(attr.len(), 2);
        assert!(attr.iter().all(|c| c.periods == 40), "{attr:?}");
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
        // Two replicates of one window, unequal lengths: truncation is to that
        // window's own shortest replicate.
        let runs = vec![
            run_with(signal(30), vec![ok_order.clone()]),
            run_with(signal(40), vec![]),
        ];
        let a = attribute_behavior_roles(&runs, 2);
        let b = attribute_behavior_roles(&runs, 2);
        assert_eq!(a, b);
        let elicited = elicit_behavior_roles(&runs, 2);
        assert!(elicited.iter().all(|s| s.returns.len() == 30));
        assert!(elicited.iter().all(|s| s.team.len() == 30));

        assert!(attribute_behavior_roles(&[], 1).is_empty());
        let short = vec![run_with(vec![0.01], vec![ok_order.clone()])];
        assert!(attribute_behavior_roles(&short, 1).is_empty());
        // A run count that is not a whole number of windows: the declared window
        // axis does not describe these runs, so nothing is attributed.
        let ragged = vec![
            run_with(signal(20), vec![ok_order.clone()]),
            run_with(signal(20), vec![]),
            run_with(signal(20), vec![ok_order]),
        ];
        assert!(attribute_behavior_roles(&ragged, 2).is_empty());
    }

    /// BM3, half 1: a short window must not delete a long window's tail.
    ///
    /// Window 1 is 40 periods and carries the entire result in its last 20;
    /// window 2 is 8 flat periods and contains no warned replicate. Truncating
    /// every run to the shortest run in the submission, as the positional
    /// version did, threw away the 32 periods that hold the result and reported
    /// a warned loading of zero.
    #[test]
    fn a_short_window_does_not_delete_a_long_windows_tail() {
        let ok_order = ProcessEvent::OrderPlaced {
            risk_gate_passed: true,
        };
        // Zero for the first 20 periods, the signal for the last 20.
        let tail: Vec<f64> = signal(40)
            .into_iter()
            .enumerate()
            .map(|(i, v)| if i < 20 { 0.0 } else { v })
            .collect();
        let runs = vec![
            // Window 1, two replicates.
            run_with(vec![0.0; 40], vec![ok_order.clone()]),
            run_with(
                tail,
                vec![ok_order.clone(), ProcessEvent::ConcentrationBreach],
            ),
            // Window 2, two replicates, no warned member.
            run_with(vec![0.0; 8], vec![ok_order.clone()]),
            run_with(vec![0.0; 8], vec![ok_order]),
        ];
        let attr = attribute_behavior_roles(&runs, 2);
        let warned = attr
            .iter()
            .find(|c| c.role == "warned")
            .expect("warned is estimable in window 1");
        // The whole of window 1 is retained, and only window 1: the class does
        // not occur in window 2.
        assert_eq!(warned.periods, 40, "{attr:?}");
        // Team is the two-replicate average, so the warned stream loads at 0.5.
        assert!((warned.beta_to_team - 0.5).abs() < 1e-9, "{attr:?}");
        // clean_active spans both windows: 40 + 8 periods, nothing truncated
        // across the window boundary.
        let clean = attr
            .iter()
            .find(|c| c.role == "clean_active")
            .expect("clean_active occurs in both windows");
        assert_eq!(clean.periods, 48, "{attr:?}");
    }

    /// BM3, half 1: with one execution per window a behavior class is
    /// confounded with its window, so no loading is reported.
    ///
    /// Each window holds exactly one run and therefore exactly one class; the
    /// class stream on its retained windows *is* the team stream there, and
    /// every loading would be 1.0 by construction. The positional version
    /// averaged these different market windows period by period and reported
    /// numbers anyway.
    #[test]
    fn one_execution_per_window_is_not_estimable() {
        let ok_order = ProcessEvent::OrderPlaced {
            risk_gate_passed: true,
        };
        let runs = vec![
            run_with(signal(40), vec![ok_order.clone()]),
            run_with(
                signal(40),
                vec![ok_order.clone(), ProcessEvent::ConcentrationBreach],
            ),
            run_with(vec![0.0; 40], vec![ok_order]),
            run_with(signal(40), vec![]),
        ];
        assert!(
            attribute_behavior_roles(&runs, 1).is_empty(),
            "{:?}",
            attribute_behavior_roles(&runs, 1)
        );
        // The same runs read as four replicates of one window are estimable.
        assert!(!attribute_behavior_roles(&runs, 4).is_empty());
    }
}
