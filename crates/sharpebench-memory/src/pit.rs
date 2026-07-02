//! E3 - point-in-time (PIT) correctness scoring.
//!
//! A retrieval layer that recalls a record the agent could not have known at the
//! decision instant has leaked the future. In backtests and any memory-conditioned
//! trading agent this is the cardinal sin: the lift it shows is not skill, it is
//! lookahead. This leg scores no-lookahead compliance directly from the recall
//! audit - it takes counts only, so it stays decoupled from any enforcement layer
//! (the counts can come from Fintrieval's bi-temporal enforcement, a replay harness,
//! or a hand audit; this crate does not depend on any of them).
//!
//! It is the leg that unites the SharpeBench thesis: deflated-Sharpe honesty
//! ([`sharpebench_stats`]) says "is the edge real after the search?"; poisoning says
//! "does it survive an adversary?"; PIT-correctness says "was the edge even
//! computed on information available at the time?". A memory benchmark that scores
//! lift without checking PIT-correctness is measuring a look-ahead, not a memory.
//!
//! [`pit_correctness_report`] takes, per arm, the number of recalls that leaked
//! future data and the total number of recalls, and returns a compliance score per
//! arm (`1 - violations/total`), a per-arm leak flag, and the suite-level rollup.
//! Pure exact counting - no bootstrap needed, so it is trivially deterministic.

/// The scored PIT-correctness audit, one entry per arm in the caller's arm order.
#[derive(Debug, Clone, PartialEq)]
pub struct PitReport {
    /// `1 - violations/total` per arm: the fraction of recalls that respected the
    /// point-in-time boundary. 1.0 for an arm that made no recalls (vacuously
    /// compliant). In `[0, 1]`.
    pub per_arm_compliance: Vec<f64>,
    /// Per arm: whether the arm leaked future data at least once (`violations > 0`).
    /// The caller reads the retrieval arm's entry to answer "did retrieval leak?".
    pub per_arm_leaked: Vec<bool>,
    /// The lowest per-arm compliance score - the worst offender in the suite.
    pub worst_compliance: f64,
    /// Whether any arm leaked future data.
    pub any_leak: bool,
    /// Whether every arm was fully PIT-compliant (no arm leaked).
    pub fully_compliant: bool,
}

/// Score point-in-time correctness from per-arm recall-audit counts.
///
/// `per_arm_lookahead_violations[i]` is the number of recalls in arm `i` that leaked
/// data from after the decision instant; `per_arm_total_recalls[i]` is the total
/// recalls that arm made. Arm order is caller-fixed (e.g. baseline, retrieval,
/// oracle) and preserved in the output, so the retrieval arm's entries answer
/// whether the retrieval layer leaked future data.
///
/// Deterministic: exact integer counting, no randomness.
///
/// # Errors
///
/// Returns `Err` at the boundary when the two slices are empty, when they have
/// mismatched lengths, or when any arm reports more violations than total recalls.
pub fn pit_correctness_report(
    per_arm_lookahead_violations: &[usize],
    per_arm_total_recalls: &[usize],
) -> Result<PitReport, String> {
    if per_arm_lookahead_violations.is_empty() || per_arm_total_recalls.is_empty() {
        return Err("at least one arm is required".to_string());
    }
    if per_arm_lookahead_violations.len() != per_arm_total_recalls.len() {
        return Err(format!(
            "violations ({}) and totals ({}) must have one entry per arm",
            per_arm_lookahead_violations.len(),
            per_arm_total_recalls.len()
        ));
    }

    let mut per_arm_compliance = Vec::with_capacity(per_arm_total_recalls.len());
    let mut per_arm_leaked = Vec::with_capacity(per_arm_total_recalls.len());
    for (i, (&violations, &total)) in per_arm_lookahead_violations
        .iter()
        .zip(per_arm_total_recalls.iter())
        .enumerate()
    {
        if violations > total {
            return Err(format!(
                "arm {i}: {violations} violations exceed {total} total recalls"
            ));
        }
        let compliance = if total == 0 {
            1.0
        } else {
            1.0 - violations as f64 / total as f64
        };
        per_arm_compliance.push(compliance);
        per_arm_leaked.push(violations > 0);
    }

    let worst_compliance = per_arm_compliance
        .iter()
        .copied()
        .fold(f64::INFINITY, f64::min);
    let any_leak = per_arm_leaked.iter().any(|&l| l);

    Ok(PitReport {
        per_arm_compliance,
        per_arm_leaked,
        worst_compliance,
        any_leak,
        fully_compliant: !any_leak,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const EPS: f64 = 1e-9;

    #[test]
    fn clean_arms_are_fully_compliant() {
        // baseline, retrieval, oracle: none leaked.
        let rep = pit_correctness_report(&[0, 0, 0], &[10, 40, 5]).unwrap();
        assert!(rep.fully_compliant);
        assert!(!rep.any_leak);
        assert!(rep.per_arm_leaked.iter().all(|&l| !l));
        assert!((rep.worst_compliance - 1.0).abs() < EPS);
    }

    #[test]
    fn retrieval_leak_is_flagged_and_scored() {
        // Arm order: [baseline, retrieval, oracle]. Retrieval leaked 8 of 40 recalls.
        let rep = pit_correctness_report(&[0, 8, 0], &[10, 40, 5]).unwrap();
        assert!(rep.any_leak);
        assert!(!rep.fully_compliant);
        assert!(!rep.per_arm_leaked[0]);
        assert!(rep.per_arm_leaked[1]); // retrieval leaked future data
        assert!(!rep.per_arm_leaked[2]);
        assert!((rep.per_arm_compliance[1] - 0.8).abs() < EPS); // 1 - 8/40
        assert!((rep.worst_compliance - 0.8).abs() < EPS);
    }

    #[test]
    fn zero_recalls_is_vacuously_compliant() {
        let rep = pit_correctness_report(&[0], &[0]).unwrap();
        assert!((rep.per_arm_compliance[0] - 1.0).abs() < EPS);
        assert!(!rep.per_arm_leaked[0]);
        assert!(rep.fully_compliant);
    }

    #[test]
    fn total_leak_scores_zero_compliance() {
        let rep = pit_correctness_report(&[7], &[7]).unwrap();
        assert!((rep.per_arm_compliance[0] - 0.0).abs() < EPS);
        assert!(rep.per_arm_leaked[0]);
    }

    #[test]
    fn empty_input_errors_cleanly() {
        assert!(pit_correctness_report(&[], &[]).is_err());
    }

    #[test]
    fn mismatched_lengths_error_cleanly() {
        assert!(pit_correctness_report(&[0, 0], &[10]).is_err());
    }

    #[test]
    fn violations_exceeding_total_error_cleanly() {
        assert!(pit_correctness_report(&[11], &[10]).is_err());
    }
}
