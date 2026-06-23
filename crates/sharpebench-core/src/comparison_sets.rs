//! Benchmark Comparison Sets — cross-agent ranking fairness.
//!
//! The Deflated Sharpe is a *within-agent* luck control: it asks whether one
//! agent's edge survives its own search footprint. It says nothing about whether
//! two agents were even scored on the **same** windows. A late entrant that only
//! completed the three easiest windows would otherwise be ranked head-to-head
//! against a veteran scored on twenty — an apples-to-oranges board.
//!
//! A comparison set fixes this. Given a locked roster (the agent_ids that count)
//! and, per agent, the set of window-ids it completed, we:
//!  1. compute the **shared windows** — the windows EVERY roster member completed,
//!  2. restrict each submission to runs on those shared windows before scoring,
//!  3. expose a **qualified** predicate: an agent qualifies for the board once it
//!     has at least `min_shared_windows` windows in the shared set.
//!
//! Pure and deterministic: shared windows are derived by sorted-set intersection
//! over the roster (fixed order → reproducible), and filtering preserves each
//! submission's run order.

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use crate::composite::{AgentSubmission, Run};

/// A run tagged with the window it was produced on. The window-id is an opaque
/// string (e.g. `"2025-Q4"`); equality is exact-string.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TaggedRun {
    pub window_id: String,
    pub run: Run,
}

/// An agent's submission as a set of window-tagged runs (one or more runs per
/// window is allowed — every run on a shared window survives filtering).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TaggedSubmission {
    pub agent_id: String,
    pub runs: Vec<TaggedRun>,
    /// Carried through to the filtered [`AgentSubmission`] unchanged.
    #[serde(default)]
    pub in_sample_trials: u32,
    /// Carried through to the filtered [`AgentSubmission`] unchanged.
    #[serde(default)]
    pub candidates: Vec<Vec<f64>>,
}

impl TaggedSubmission {
    /// The distinct window-ids this agent completed at least one run on.
    pub fn completed_windows(&self) -> BTreeSet<String> {
        self.runs.iter().map(|r| r.window_id.clone()).collect()
    }
}

/// The result of resolving a roster into a comparison set.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ComparisonSet {
    /// The locked roster, in the order supplied.
    pub roster: Vec<String>,
    /// Windows EVERY roster member completed, sorted ascending (deterministic).
    pub shared_windows: Vec<String>,
}

/// Resolve a locked roster into its comparison set: the windows completed by
/// every roster member. A roster member with no submission contributes the empty
/// set, which (correctly) empties the shared set — an agent that completed
/// nothing must not silently widen the comparison.
///
/// `roster` order is preserved on the result; `shared_windows` is sorted so the
/// output is byte-reproducible regardless of submission/iteration order.
pub fn comparison_set(roster: &[String], subs: &[TaggedSubmission]) -> ComparisonSet {
    let mut shared: Option<BTreeSet<String>> = None;
    for agent_id in roster {
        let completed = subs
            .iter()
            .find(|s| &s.agent_id == agent_id)
            .map(TaggedSubmission::completed_windows)
            .unwrap_or_default();
        shared = Some(match shared {
            None => completed,
            Some(acc) => acc.intersection(&completed).cloned().collect(),
        });
    }
    ComparisonSet {
        roster: roster.to_vec(),
        shared_windows: shared.unwrap_or_default().into_iter().collect(),
    }
}

/// Whether an agent qualifies for the board: it must complete at least
/// `min_shared_windows` of the comparison set's shared windows. (Every roster
/// member completes *all* shared windows by construction, so for roster members
/// this reduces to `shared.len() >= min`. The predicate is also meaningful for a
/// prospective entrant being checked against an existing set.)
pub fn qualifies(set: &ComparisonSet, sub: &TaggedSubmission, min_shared_windows: usize) -> bool {
    let completed = sub.completed_windows();
    let shared_completed = set
        .shared_windows
        .iter()
        .filter(|w| completed.contains(*w))
        .count();
    shared_completed >= min_shared_windows
}

/// Restrict a tagged submission to its runs on the shared windows, producing a
/// plain [`AgentSubmission`] ready for `score_agent` / `rank`. Run order within
/// the submission is preserved; `in_sample_trials` and `candidates` pass through.
pub fn restrict_to_shared(set: &ComparisonSet, sub: &TaggedSubmission) -> AgentSubmission {
    let shared: BTreeSet<&str> = set.shared_windows.iter().map(String::as_str).collect();
    let runs = sub
        .runs
        .iter()
        .filter(|r| shared.contains(r.window_id.as_str()))
        .map(|r| r.run.clone())
        .collect();
    AgentSubmission {
        agent_id: sub.agent_id.clone(),
        runs,
        in_sample_trials: sub.in_sample_trials,
        candidates: sub.candidates.clone(),
    }
}

/// Resolve the comparison set and return one filtered [`AgentSubmission`] per
/// roster member, in roster order, each restricted to the shared windows. Roster
/// members without a submission yield an empty-run submission (they score as
/// ineligible downstream, never widening the comparison). This is the single
/// entry point a caller uses before `rank`.
pub fn restrict_field(roster: &[String], subs: &[TaggedSubmission]) -> Vec<AgentSubmission> {
    let set = comparison_set(roster, subs);
    roster
        .iter()
        .map(
            |agent_id| match subs.iter().find(|s| &s.agent_id == agent_id) {
                Some(sub) => restrict_to_shared(&set, sub),
                None => AgentSubmission {
                    agent_id: agent_id.clone(),
                    runs: Vec::new(),
                    in_sample_trials: 0,
                    candidates: Vec::new(),
                },
            },
        )
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(mean_ret: f64, n: usize) -> Run {
        Run {
            returns: (0..n)
                .map(|i| mean_ret + 0.0005 * (i as f64 * 0.7).sin())
                .collect(),
            trace: Default::default(),
            confidences: Vec::new(),
            outcomes: Vec::new(),
            cost: 0.0,
        }
    }

    fn tagged(agent_id: &str, windows: &[&str]) -> TaggedSubmission {
        TaggedSubmission {
            agent_id: agent_id.to_string(),
            runs: windows
                .iter()
                .map(|w| TaggedRun {
                    window_id: (*w).to_string(),
                    run: run(0.002, 40),
                })
                .collect(),
            in_sample_trials: 0,
            candidates: Vec::new(),
        }
    }

    #[test]
    fn shared_is_intersection_sorted() {
        let veteran = tagged("vet", &["w3", "w1", "w2", "w4"]);
        let entrant = tagged("new", &["w2", "w3"]);
        let roster = vec!["vet".to_string(), "new".to_string()];
        let set = comparison_set(&roster, &[veteran, entrant]);
        assert_eq!(set.shared_windows, vec!["w2".to_string(), "w3".to_string()]);
        assert_eq!(set.roster, roster);
    }

    #[test]
    fn missing_roster_member_empties_shared() {
        let veteran = tagged("vet", &["w1", "w2"]);
        let roster = vec!["vet".to_string(), "ghost".to_string()];
        let set = comparison_set(&roster, &[veteran]);
        assert!(set.shared_windows.is_empty());
    }

    #[test]
    fn restrict_keeps_only_shared_runs() {
        let veteran = tagged("vet", &["w1", "w2", "w3"]);
        let entrant = tagged("new", &["w2", "w3", "w9"]);
        let roster = vec!["vet".to_string(), "new".to_string()];
        let field = restrict_field(&roster, &[veteran, entrant]);
        // Shared = {w2, w3}: each filtered submission keeps exactly 2 runs.
        assert_eq!(field.len(), 2);
        assert_eq!(field[0].agent_id, "vet");
        assert_eq!(field[0].runs.len(), 2);
        assert_eq!(field[1].runs.len(), 2);
    }

    #[test]
    fn qualifies_on_min_shared() {
        let veteran = tagged("vet", &["w1", "w2", "w3"]);
        let entrant = tagged("new", &["w2", "w3"]);
        let roster = vec!["vet".to_string(), "new".to_string()];
        let set = comparison_set(&roster, &[veteran, entrant.clone()]);
        // Shared = {w2, w3}, size 2.
        assert!(qualifies(&set, &entrant, 2));
        assert!(!qualifies(&set, &entrant, 3));
        // A prospective entrant missing a shared window does not qualify at 2.
        let thin = tagged("thin", &["w2", "w7"]);
        assert!(!qualifies(&set, &thin, 2));
        assert!(qualifies(&set, &thin, 1));
    }

    #[test]
    fn empty_roster_yields_empty_set() {
        let set = comparison_set(&[], &[]);
        assert!(set.shared_windows.is_empty());
        assert!(set.roster.is_empty());
        assert!(restrict_field(&[], &[]).is_empty());
    }

    #[test]
    fn multiple_runs_per_window_all_survive() {
        let mut sub = tagged("multi", &["w1", "w1", "w2"]);
        // sub has two runs on w1, one on w2.
        let other = tagged("other", &["w1", "w2"]);
        let roster = vec!["multi".to_string(), "other".to_string()];
        let set = comparison_set(&roster, &[sub.clone(), other]);
        assert_eq!(set.shared_windows, vec!["w1".to_string(), "w2".to_string()]);
        let filtered = restrict_to_shared(&set, &sub);
        // Both w1 runs + the w2 run survive (shared = {w1, w2}).
        assert_eq!(filtered.runs.len(), 3);
        // Adding an unshared window's run must not survive.
        sub.runs.push(TaggedRun {
            window_id: "w9".to_string(),
            run: run(0.002, 40),
        });
        assert_eq!(restrict_to_shared(&set, &sub).runs.len(), 3);
    }
}
