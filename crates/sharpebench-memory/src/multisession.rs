//! E2 - interdependent multi-session scoring (MemoryArena-style).
//!
//! The declared graph must be acyclic. Positive lift is an observed proxy, not
//! evidence that an agent actually stored or used a fact. A session qualifies as
//! a prerequisite only if its lift is positive AND all of its own prerequisites
//! qualify. Failed prerequisites therefore block every descendant.
//!
//! One chain is descriptive evidence. Its dependent sessions and tasks are not
//! independent repetitions, and their arbitrary ordering does not define a
//! stationary time series. This API no longer bootstraps a concatenated vector
//! of raw task scores and labels that the significance of a credited effect.

use sharpebench_stats::{
    paired_randomization::{paired_swap_test, PairedSwapConfig, PairedSwapTest},
    stats::mean,
};
use std::collections::{BTreeMap, BTreeSet};

/// Caller-assigned session identifier. Must be unique within a suite.
pub type SessionId = u64;

/// One session's paired arms plus the earlier sessions its memory depends on.
#[derive(Debug, Clone, PartialEq)]
pub struct SessionScores {
    /// Unique identifier for this session.
    pub session_id: SessionId,
    /// Baseline (no-memory) per-task outcome scores for this session.
    pub baseline: Vec<f64>,
    /// Retrieval scores, paired positionally with `baseline`. The caller must
    /// align task identities; equal lengths cannot verify that correspondence.
    pub retrieval: Vec<f64>,
    /// Earlier sessions whose written memory this session relies on. Each id must
    /// refer to another session in the suite. IDs are opaque, not timestamps;
    /// the graph expresses the claimed ordering. Duplicates and cycles are errors.
    pub depends_on: Vec<SessionId>,
}

impl SessionScores {
    /// Construct a session's scores.
    pub fn new(
        session_id: SessionId,
        baseline: Vec<f64>,
        retrieval: Vec<f64>,
        depends_on: Vec<SessionId>,
    ) -> Self {
        Self {
            session_id,
            baseline,
            retrieval,
            depends_on,
        }
    }
}

/// Per-session outcome of the multi-session ablation.
#[derive(Debug, Clone, PartialEq)]
pub struct SessionLift {
    /// The session this row scores.
    pub session_id: SessionId,
    /// Mean of paired `retrieval - baseline` differences for this session.
    pub lift: f64,
    /// Raw positive-lift proxy (`lift > 0`), not proof of memory use.
    pub retained: bool,
    /// Whether every prerequisite has positive lift and qualified prerequisites.
    pub dependencies_satisfied: bool,
    /// `retained && dependencies_satisfied`. Only this qualified proxy may
    /// satisfy a later session's dependency.
    pub qualified_retention: bool,
    /// The lift credited after conditioning: `lift` when `dependencies_satisfied`,
    /// else `0.0`. A later session earns no credit when the memory chain it relies on
    /// was not retained.
    pub conditioned_lift: f64,
}

/// A single chain does not supply independent units for chain-effect inference.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChainInferenceUnavailable {
    IndependentReplicatesRequired,
}

/// The scored interdependent multi-session ablation.
#[derive(Debug, Clone, PartialEq)]
pub struct MultiSessionReport {
    /// One row per session, in the input order.
    pub per_session: Vec<SessionLift>,
    /// Satisfied dependency edges / total dependency edges across the whole suite.
    /// An edge is satisfied only when its prerequisite's entire ancestry qualifies.
    /// 1.0 when there are no dependency edges (vacuously satisfied).
    pub dependency_satisfaction_rate: f64,
    /// Equal-session raw lift. A session's task count does not change its weight.
    pub raw_mean_lift: f64,
    /// Equal-session conditioned lift, including zero-credit sessions in the denominator.
    pub conditioned_mean_lift: f64,
    /// Why no significance verdict is emitted for this single chain.
    pub inference_unavailable: ChainInferenceUnavailable,
    /// Validated requested threshold; it does not create inferential support.
    pub alpha: f64,
}

/// Score an interdependent multi-session memory ablation.
///
/// Sessions are supplied in any order; `depends_on` edges express which earlier
/// sessions' memory each session relies on. Credit is gated transitively. A session
/// with satisfied dependencies retains its signed lift, including losses. Blocked
/// sessions contribute zero, not missing rows. Aggregate reductions use session-ID
/// order, independent of input order. Rows remain in the caller's input order.
///
/// # Errors
///
/// Returns `Err` at the boundary when there are no sessions, when a session has an
/// empty or mismatched-length arm pair, when session ids are not unique, when a
/// session depends on itself, or when a `depends_on` id refers to no session in the
/// suite. Duplicate edges, cycles, nonfinite scores/differences/reductions, and
/// alpha outside the finite open interval `(0, 1)` are also errors.
pub fn multi_session_report(
    sessions: &[SessionScores],
    alpha: f64,
) -> Result<MultiSessionReport, String> {
    validate_alpha(alpha)?;
    let graph = SessionGraph::new(sessions)?;
    let lifts = paired_lifts(sessions)?;
    let per_session = graph.score(sessions, &lifts);
    let total_edges: usize = graph.dependencies.iter().map(Vec::len).sum();
    let satisfied_edges = graph
        .dependencies
        .iter()
        .flatten()
        .filter(|&&dep| per_session[dep].qualified_retention)
        .count();
    Ok(MultiSessionReport {
        dependency_satisfaction_rate: if total_edges == 0 {
            1.0
        } else {
            satisfied_edges as f64 / total_edges as f64
        },
        raw_mean_lift: graph.average(&lifts)?,
        conditioned_mean_lift: graph.average(
            &per_session
                .iter()
                .map(|s| s.conditioned_lift)
                .collect::<Vec<_>>(),
        )?,
        per_session,
        inference_unavailable: ChainInferenceUnavailable::IndependentReplicatesRequired,
        alpha,
    })
}

fn validate_alpha(alpha: f64) -> Result<(), String> {
    if !alpha.is_finite() || alpha <= 0.0 || alpha >= 1.0 {
        return Err("alpha must be finite and in (0, 1)".to_string());
    }
    Ok(())
}

fn finite_mean(values: &[f64]) -> Result<f64, String> {
    let value = mean(values);
    if !value.is_finite() {
        return Err("memory lift reduction is not finite".to_string());
    }
    Ok(value)
}

fn paired_lifts(sessions: &[SessionScores]) -> Result<Vec<f64>, String> {
    sessions
        .iter()
        .map(|s| {
            if s.baseline.is_empty() || s.retrieval.is_empty() {
                return Err(format!("session {} has an empty arm", s.session_id));
            }
            if s.baseline.len() != s.retrieval.len() {
                return Err(format!(
                    "session {}: baseline ({}) and retrieval ({}) must be paired",
                    s.session_id,
                    s.baseline.len(),
                    s.retrieval.len()
                ));
            }
            let mut differences = Vec::with_capacity(s.baseline.len());
            for (task, (&baseline, &retrieval)) in s.baseline.iter().zip(&s.retrieval).enumerate() {
                if !baseline.is_finite()
                    || !retrieval.is_finite()
                    || !(retrieval - baseline).is_finite()
                {
                    return Err(format!(
                        "session {} task {task}: scores and paired difference must be finite",
                        s.session_id
                    ));
                }
                differences.push(retrieval - baseline);
            }
            finite_mean(&differences)
        })
        .collect()
}

struct SessionGraph {
    dependencies: Vec<Vec<usize>>,
    topological: Vec<usize>,
    canonical: Vec<usize>,
}

impl SessionGraph {
    fn new(sessions: &[SessionScores]) -> Result<Self, String> {
        if sessions.is_empty() {
            return Err("at least one session is required".to_string());
        }
        let mut ids = BTreeMap::new();
        for (i, session) in sessions.iter().enumerate() {
            if ids.insert(session.session_id, i).is_some() {
                return Err(format!("duplicate session id {}", session.session_id));
            }
        }
        let canonical: Vec<_> = ids.values().copied().collect();
        let mut dependencies = vec![Vec::new(); sessions.len()];
        let mut children = vec![Vec::new(); sessions.len()];
        for (i, session) in sessions.iter().enumerate() {
            let mut unique = BTreeSet::new();
            for dep in &session.depends_on {
                if *dep == session.session_id {
                    return Err(format!("session {} depends on itself", session.session_id));
                }
                let &index = ids.get(dep).ok_or_else(|| {
                    format!(
                        "session {} depends on unknown session {dep}",
                        session.session_id
                    )
                })?;
                if !unique.insert(*dep) {
                    return Err(format!(
                        "session {} repeats dependency {dep}",
                        session.session_id
                    ));
                }
                dependencies[i].push(index);
                children[index].push(i);
            }
        }
        let mut pending: Vec<_> = dependencies.iter().map(Vec::len).collect();
        let mut ready: BTreeSet<_> = canonical
            .iter()
            .copied()
            .filter(|&i| pending[i] == 0)
            .map(|i| (sessions[i].session_id, i))
            .collect();
        let mut topological = Vec::with_capacity(sessions.len());
        // Iterative traversal also handles long chains without recursion/stack exhaustion.
        while let Some((_, i)) = ready.pop_first() {
            topological.push(i);
            for &child in &children[i] {
                pending[child] -= 1;
                if pending[child] == 0 {
                    ready.insert((sessions[child].session_id, child));
                }
            }
        }
        if topological.len() != sessions.len() {
            return Err("session dependency graph contains a cycle".to_string());
        }
        Ok(Self {
            dependencies,
            topological,
            canonical,
        })
    }

    fn score(&self, sessions: &[SessionScores], lifts: &[f64]) -> Vec<SessionLift> {
        let mut rows: Vec<_> = sessions
            .iter()
            .zip(lifts)
            .map(|(s, &lift)| SessionLift {
                session_id: s.session_id,
                lift,
                retained: lift > 0.0,
                dependencies_satisfied: false,
                qualified_retention: false,
                conditioned_lift: 0.0,
            })
            .collect();
        for &i in &self.topological {
            let deps_ok = self.dependencies[i]
                .iter()
                .all(|&dep| rows[dep].qualified_retention);
            rows[i].dependencies_satisfied = deps_ok;
            rows[i].qualified_retention = deps_ok && rows[i].retained;
            rows[i].conditioned_lift = if deps_ok { lifts[i] } else { 0.0 };
        }
        rows
    }

    fn average(&self, values: &[f64]) -> Result<f64, String> {
        finite_mean(
            &self
                .canonical
                .iter()
                .map(|&i| values[i])
                .collect::<Vec<_>>(),
        )
    }
}

/// One independently repeated COMPLETE session graph, not a task or session within
/// a chain. The caller must justify independence and whole-arm exchangeability
/// under the null; distinct identifiers do not establish either property.
#[derive(Debug, Clone, PartialEq)]
pub struct MemoryChainReplicate {
    pub replicate_id: u64,
    pub sessions: Vec<SessionScores>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct MemoryReplicateReport {
    pub replicate_id: u64,
    pub chain: MultiSessionReport,
}

/// Equal-replicate inference for predeclared complete-chain arm comparisons.
#[derive(Debug, Clone, PartialEq)]
pub struct ReplicatedMultiSessionReport {
    /// Sorted by replicate ID. Each single chain remains descriptive in isolation.
    pub replicates: Vec<MemoryReplicateReport>,
    /// Diagnostic raw equal-session/equal-replicate lift test, without credit gates.
    pub raw_lift_test: PairedSwapTest,
    /// Primary dependency-conditioned test. This is NOT a sign flip of the
    /// observed credited scores: gates are reevaluated when each full chain swaps.
    pub conditioned_lift_test: PairedSwapTest,
    /// Positive observed conditioned lift AND its upper-tail p-value < alpha.
    /// Raw significance cannot satisfy this field. No multiple-testing adjustment
    /// for selecting graphs, configurations or reported comparisons is implied.
    pub conditioned_significant: bool,
    pub alpha: f64,
}

/// Evaluate a predeclared chain-credit rule over independent paired replicates.
///
/// Every replicate must have the same session IDs, edges and per-session task
/// counts. Pairing actual task identities and equal score units are caller
/// preconditions, not inferred from vector lengths. Sessions are equally weighted
/// within each replicate; replicates are equally weighted in both tests.
///
/// Under the null, baseline/retrieval labels must be exchangeable for the entire
/// chain, independently across replicates. Within-chain task/session dependence is
/// preserved by swapping all arms together. The graph and scoring/analysis choices
/// must precede outcomes; this test does not account for searching them. It is not
/// a proof of memory use, causality, or generalization to other tasks.
///
/// Each complete chain has only two possible orientations. We reevaluate the DAG
/// for both once, then the shared permutation engine enumerates/samples joint
/// assignments of those orientation scores. This is equivalent to rerunning the
/// deterministic DAG scorer at every draw, without repeating identical work.
///
/// # Errors
///
/// Requires at least two unique replicate IDs, complete matching geometry, valid
/// single-chain inputs, finite reductions and valid resampling parameters.
///
/// # Example
///
/// ```
/// use sharpebench_memory::{replicated_multi_session_report, MemoryChainReplicate, SessionScores};
/// use sharpebench_stats::paired_randomization::PairedSwapConfig;
/// // Synthetic API example, not measured agent performance.
/// let replicates: Vec<_> = (0..6).map(|replicate_id| MemoryChainReplicate {
///     replicate_id,
///     sessions: vec![
///         SessionScores::new(10, vec![0.0], vec![1.0], vec![]),
///         SessionScores::new(20, vec![0.0], vec![2.0], vec![10]),
///     ],
/// }).collect();
/// let report = replicated_multi_session_report(
///     &replicates, 0.05, PairedSwapConfig { resamples: 9999, seed: 42 },
/// ).expect("valid synthetic fixture");
/// assert_eq!(report.conditioned_lift_test.observed_mean, 1.5);
/// assert_eq!(report.conditioned_lift_test.pvalue, 1.0 / 64.0);
/// assert!(report.conditioned_significant);
/// ```
pub fn replicated_multi_session_report(
    replicates: &[MemoryChainReplicate],
    alpha: f64,
    config: PairedSwapConfig,
) -> Result<ReplicatedMultiSessionReport, String> {
    validate_alpha(alpha)?;
    if replicates.len() < 2 {
        return Err("at least two independent complete-chain replicates are required".to_string());
    }
    let mut by_id = BTreeMap::new();
    for replicate in replicates {
        if by_id.insert(replicate.replicate_id, replicate).is_some() {
            return Err(format!("duplicate replicate id {}", replicate.replicate_id));
        }
    }
    let mut expected_geometry = None;
    let mut reports = Vec::with_capacity(replicates.len());
    let mut raw_orientations = Vec::with_capacity(replicates.len());
    let mut conditioned_orientations = Vec::with_capacity(replicates.len());
    for (&replicate_id, replicate) in &by_id {
        let sessions = &replicate.sessions;
        let chain = multi_session_report(sessions, alpha)
            .map_err(|error| format!("replicate {replicate_id}: {error}"))?;
        let geometry: BTreeMap<_, _> = sessions
            .iter()
            .map(|s| {
                (
                    s.session_id,
                    (
                        s.baseline.len(),
                        s.depends_on.iter().copied().collect::<BTreeSet<_>>(),
                    ),
                )
            })
            .collect();
        match &expected_geometry {
            Some(expected) if expected != &geometry => {
                return Err(format!("replicate {replicate_id}: session IDs, dependency edges and task counts must match the complete reference graph"));
            }
            None => expected_geometry = Some(geometry),
            _ => {}
        }
        let graph = SessionGraph::new(sessions)?;
        let swapped_lifts: Vec<_> = chain.per_session.iter().map(|row| -row.lift).collect();
        // Negating a raw paired difference swaps its arms. Negating a conditioned
        // score would incorrectly freeze data-dependent qualification at observed data.
        let swapped = graph.score(sessions, &swapped_lifts);
        raw_orientations.push([chain.raw_mean_lift, graph.average(&swapped_lifts)?]);
        conditioned_orientations.push([
            chain.conditioned_mean_lift,
            graph.average(
                &swapped
                    .iter()
                    .map(|row| row.conditioned_lift)
                    .collect::<Vec<_>>(),
            )?,
        ]);
        reports.push(MemoryReplicateReport {
            replicate_id,
            chain,
        });
    }
    let raw_lift_test = paired_swap_test(&raw_orientations, config).map_err(|e| e.to_string())?;
    let conditioned_lift_test =
        paired_swap_test(&conditioned_orientations, config).map_err(|e| e.to_string())?;
    let conditioned_significant =
        conditioned_lift_test.observed_mean > 0.0 && conditioned_lift_test.pvalue < alpha;
    Ok(ReplicatedMultiSessionReport {
        replicates: reports,
        raw_lift_test,
        conditioned_lift_test,
        conditioned_significant,
        alpha,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const EPS: f64 = 1e-9;

    #[test]
    fn dependency_conditioning_credits_only_retained_chains() {
        // Session 1 retains (memory helps). Session 2 depends on 1 and also retains.
        // Session 3 depends on a session (4) that did NOT retain, so it is not credited.
        let s1 = SessionScores::new(1, vec![0.10, 0.12, 0.11], vec![0.70, 0.72, 0.71], vec![]);
        let s2 = SessionScores::new(2, vec![0.20, 0.22, 0.21], vec![0.80, 0.82, 0.81], vec![1]);
        let s4 = SessionScores::new(4, vec![0.50, 0.52, 0.51], vec![0.40, 0.42, 0.41], vec![]);
        let s3 = SessionScores::new(3, vec![0.10, 0.11, 0.12], vec![0.60, 0.61, 0.62], vec![4]);

        let rep = multi_session_report(&[s1, s2, s4, s3], 0.05).unwrap();

        let by_id = |id: SessionId| rep.per_session.iter().find(|r| r.session_id == id).unwrap();
        assert!(by_id(1).retained);
        assert!(by_id(2).dependencies_satisfied);
        assert!(by_id(2).conditioned_lift > 0.0);
        assert!(!by_id(4).retained); // baseline beat retrieval
        assert!(!by_id(3).dependencies_satisfied);
        assert!((by_id(3).conditioned_lift - 0.0).abs() < EPS); // credit withheld
        assert!(by_id(3).lift > 0.0); // raw lift is still positive

        // 2 edges total (2->1 satisfied, 3->4 not) => 0.5
        assert!((rep.dependency_satisfaction_rate - 0.5).abs() < EPS);
        assert!(rep.raw_mean_lift > rep.conditioned_mean_lift);
        assert_eq!(
            rep.inference_unavailable,
            ChainInferenceUnavailable::IndependentReplicatesRequired
        );
    }

    #[test]
    fn no_edges_is_vacuously_satisfied() {
        let s1 = SessionScores::new(1, vec![0.1, 0.1], vec![0.5, 0.5], vec![]);
        let s2 = SessionScores::new(2, vec![0.2, 0.2], vec![0.6, 0.6], vec![]);
        let rep = multi_session_report(&[s1, s2], 0.05).unwrap();
        assert!((rep.dependency_satisfaction_rate - 1.0).abs() < EPS);
    }

    #[test]
    fn null_lift_across_sessions_has_no_invented_significance() {
        let s1 = SessionScores::new(1, vec![0.40, 0.42, 0.41], vec![0.40, 0.42, 0.41], vec![]);
        let s2 = SessionScores::new(2, vec![0.30, 0.31, 0.32], vec![0.30, 0.31, 0.32], vec![1]);
        let rep = multi_session_report(&[s1, s2], 0.05).unwrap();
        assert_eq!(rep.raw_mean_lift, 0.0);
        assert_eq!(rep.conditioned_mean_lift, 0.0);
        assert_eq!(
            rep.inference_unavailable,
            ChainInferenceUnavailable::IndependentReplicatesRequired
        );
        // depended-on session 1 did not retain (flat), so the edge is unsatisfied.
        assert!((rep.dependency_satisfaction_rate - 0.0).abs() < EPS);
    }

    #[test]
    fn empty_suite_errors_cleanly() {
        assert!(multi_session_report(&[], 0.05).is_err());
    }

    #[test]
    fn mismatched_arm_lengths_error_cleanly() {
        let s = SessionScores::new(1, vec![0.1, 0.2, 0.3], vec![0.5, 0.6], vec![]);
        assert!(multi_session_report(&[s], 0.05).is_err());
    }

    #[test]
    fn empty_arm_errors_cleanly() {
        let s = SessionScores::new(1, vec![], vec![], vec![]);
        assert!(multi_session_report(&[s], 0.05).is_err());
    }

    #[test]
    fn duplicate_ids_error_cleanly() {
        let a = SessionScores::new(1, vec![0.1, 0.2], vec![0.5, 0.6], vec![]);
        let b = SessionScores::new(1, vec![0.1, 0.2], vec![0.5, 0.6], vec![]);
        assert!(multi_session_report(&[a, b], 0.05).is_err());
    }

    #[test]
    fn self_dependency_errors_cleanly() {
        let s = SessionScores::new(1, vec![0.1, 0.2], vec![0.5, 0.6], vec![1]);
        assert!(multi_session_report(&[s], 0.05).is_err());
    }

    #[test]
    fn unknown_dependency_errors_cleanly() {
        let s = SessionScores::new(1, vec![0.1, 0.2], vec![0.5, 0.6], vec![99]);
        assert!(multi_session_report(&[s], 0.05).is_err());
    }
}
