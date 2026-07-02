//! E2 - interdependent multi-session scoring (MemoryArena-style).
//!
//! A flat per-task score vector cannot express the thing a memory layer is actually
//! for: carrying knowledge written in one session forward into a later one. Real
//! memory failures are cross-session - the agent learned a fact in session 1, then
//! failed session 4 because it never retained that fact. Scoring session 4 in
//! isolation hides the failure.
//!
//! This leg models sessions as a dependency graph. Each [`SessionScores`] carries a
//! baseline arm (no memory) and a retrieval arm (memory under test) for that
//! session, plus `depends_on`: the earlier sessions whose written memory this
//! session relies on. A session is judged to have **retained** its memory when its
//! retrieval arm beat its baseline arm (a positive per-session lift). A later
//! session's credit is then **conditioned** on retention: if any session it depends
//! on failed to retain, the later session's lift does not count - the agent got the
//! answer without the memory chain that was supposed to produce it.
//!
//! [`multi_session_report`] returns the per-session lift, whether each session
//! retained and had its dependencies satisfied, the conditioned lift, and the
//! cross-session dependency-satisfaction rate over all dependency edges. It also
//! pools every session's paired per-task differences into the shared stationary
//! bootstrap ([`sharpebench_stats::significance::bootstrap_pvalue`]) for an aggregate
//! significance - it does not reinvent the statistics.
//!
//! Pure and deterministic: shares the crate's fixed bootstrap seed and sample count.

use crate::{BOOTSTRAP_BLOCK_PROB, BOOTSTRAP_SAMPLES, BOOTSTRAP_SEED};
use sharpebench_stats::{significance::bootstrap_pvalue, stats::mean};

/// Caller-assigned session identifier. Must be unique within a suite.
pub type SessionId = u64;

/// One session's paired arms plus the earlier sessions its memory depends on.
#[derive(Debug, Clone, PartialEq)]
pub struct SessionScores {
    /// Unique identifier for this session.
    pub session_id: SessionId,
    /// Baseline (no-memory) per-task outcome scores for this session.
    pub baseline: Vec<f64>,
    /// Retrieval (memory-under-test) per-task outcome scores, paired with `baseline`.
    pub retrieval: Vec<f64>,
    /// Earlier sessions whose written memory this session relies on. Each id must
    /// refer to another session in the suite (no self-dependency).
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
    /// `mean(retrieval) - mean(baseline)` for this session.
    pub lift: f64,
    /// Whether the memory demonstrably helped here (`lift > 0`) - i.e. the session
    /// retained and used its memory.
    pub retained: bool,
    /// Whether every session in `depends_on` retained its memory.
    pub dependencies_satisfied: bool,
    /// The lift credited after conditioning: `lift` when `dependencies_satisfied`,
    /// else `0.0`. A later session earns no credit when the memory chain it relies on
    /// was not retained.
    pub conditioned_lift: f64,
}

/// The scored interdependent multi-session ablation.
#[derive(Debug, Clone, PartialEq)]
pub struct MultiSessionReport {
    /// One row per session, in the input order.
    pub per_session: Vec<SessionLift>,
    /// Satisfied dependency edges / total dependency edges across the whole suite.
    /// An edge `later -> earlier` is satisfied when `earlier` retained its memory.
    /// 1.0 when there are no dependency edges (vacuously satisfied).
    pub dependency_satisfaction_rate: f64,
    /// Stationary-bootstrap p-value that the pooled paired per-task lift over every
    /// session has a positive mean, via
    /// [`sharpebench_stats::significance::bootstrap_pvalue`]. 1.0 when the pooled lift
    /// is non-positive.
    pub pooled_lift_pvalue: f64,
    /// Whether the pooled lift is significant at `alpha`.
    pub significant: bool,
    /// The significance threshold used for the verdict.
    pub alpha: f64,
}

/// Score an interdependent multi-session memory ablation.
///
/// Sessions are supplied in any order; `depends_on` edges express which earlier
/// sessions' memory each session relies on. Retention is judged per session
/// (retrieval beat baseline), and a session's conditioned credit is gated on all of
/// its dependencies having retained.
///
/// Deterministic: the pooled bootstrap uses the crate's fixed seed and sample count.
///
/// # Errors
///
/// Returns `Err` at the boundary when there are no sessions, when a session has an
/// empty or mismatched-length arm pair, when session ids are not unique, when a
/// session depends on itself, or when a `depends_on` id refers to no session in the
/// suite.
pub fn multi_session_report(
    sessions: &[SessionScores],
    alpha: f64,
) -> Result<MultiSessionReport, String> {
    if sessions.is_empty() {
        return Err("at least one session is required".to_string());
    }

    let ids: Vec<SessionId> = sessions.iter().map(|s| s.session_id).collect();
    for (i, s) in sessions.iter().enumerate() {
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
        if ids[..i].contains(&s.session_id) {
            return Err(format!("duplicate session id {}", s.session_id));
        }
        for dep in &s.depends_on {
            if *dep == s.session_id {
                return Err(format!("session {} depends on itself", s.session_id));
            }
            if !ids.contains(dep) {
                return Err(format!(
                    "session {} depends on unknown session {}",
                    s.session_id, dep
                ));
            }
        }
    }

    // First pass: per-session lift and retention.
    let mut retained_of: Vec<(SessionId, bool)> = Vec::with_capacity(sessions.len());
    let mut lifts: Vec<f64> = Vec::with_capacity(sessions.len());
    for s in sessions {
        let lift = mean(&s.retrieval) - mean(&s.baseline);
        lifts.push(lift);
        retained_of.push((s.session_id, lift > 0.0));
    }
    let retained = |id: SessionId| -> bool {
        retained_of
            .iter()
            .find(|(sid, _)| *sid == id)
            .map(|(_, r)| *r)
            .unwrap_or(false)
    };

    // Second pass: condition each session on its dependencies retaining.
    let mut per_session = Vec::with_capacity(sessions.len());
    let mut satisfied_edges = 0usize;
    let mut total_edges = 0usize;
    for (s, &lift) in sessions.iter().zip(lifts.iter()) {
        let mut deps_ok = true;
        for dep in &s.depends_on {
            total_edges += 1;
            if retained(*dep) {
                satisfied_edges += 1;
            } else {
                deps_ok = false;
            }
        }
        per_session.push(SessionLift {
            session_id: s.session_id,
            lift,
            retained: lift > 0.0,
            dependencies_satisfied: deps_ok,
            conditioned_lift: if deps_ok { lift } else { 0.0 },
        });
    }

    let dependency_satisfaction_rate = if total_edges == 0 {
        1.0
    } else {
        satisfied_edges as f64 / total_edges as f64
    };

    // Pooled paired per-task lift across every session for an aggregate significance.
    let mut pooled: Vec<f64> = Vec::new();
    for s in sessions {
        pooled.extend(
            s.retrieval
                .iter()
                .zip(s.baseline.iter())
                .map(|(r, b)| r - b),
        );
    }
    let pooled_lift_pvalue = bootstrap_pvalue(
        &pooled,
        BOOTSTRAP_SEED,
        BOOTSTRAP_SAMPLES,
        BOOTSTRAP_BLOCK_PROB,
    );

    Ok(MultiSessionReport {
        per_session,
        dependency_satisfaction_rate,
        pooled_lift_pvalue,
        significant: pooled_lift_pvalue < alpha,
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
        assert!(rep.significant, "pooled p {}", rep.pooled_lift_pvalue);
    }

    #[test]
    fn no_edges_is_vacuously_satisfied() {
        let s1 = SessionScores::new(1, vec![0.1, 0.1], vec![0.5, 0.5], vec![]);
        let s2 = SessionScores::new(2, vec![0.2, 0.2], vec![0.6, 0.6], vec![]);
        let rep = multi_session_report(&[s1, s2], 0.05).unwrap();
        assert!((rep.dependency_satisfaction_rate - 1.0).abs() < EPS);
    }

    #[test]
    fn null_lift_across_sessions_is_not_significant() {
        let s1 = SessionScores::new(1, vec![0.40, 0.42, 0.41], vec![0.40, 0.42, 0.41], vec![]);
        let s2 = SessionScores::new(2, vec![0.30, 0.31, 0.32], vec![0.30, 0.31, 0.32], vec![1]);
        let rep = multi_session_report(&[s1, s2], 0.05).unwrap();
        assert!(!rep.significant);
        assert!((rep.pooled_lift_pvalue - 1.0).abs() < EPS);
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
