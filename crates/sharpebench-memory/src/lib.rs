//! # sharpebench-memory - three-arm memory/retrieval ablation harness
//!
//! Proving that a memory or retrieval layer actually improves downstream agent
//! decisions (rather than just adding tokens and latency) is an ablation problem,
//! not a vibe. This crate scores the classic three-arm ablation and reuses
//! [`sharpebench_stats`] for the significance test - it does **not** reinvent the
//! statistics.
//!
//! The three arms:
//! - [`Arm::Baseline`] - the agent with **no memory**. The floor.
//! - [`Arm::Retrieval`] - the agent with the memory/retrieval layer under test.
//!   This is the arm whose lift over baseline we want to prove is real.
//! - [`Arm::Oracle`] - the agent fed **only the gold records** (perfect recall of
//!   exactly the right context). Not achievable in production; it is the ceiling
//!   that bounds how much a retrieval layer could ever help.
//!
//! Each arm supplies a per-task outcome score (decision quality, realized reward,
//! …). [`ablation_report`] returns the retrieval lift over baseline, the p-value
//! of that lift (a stationary-bootstrap test on the paired per-task differences,
//! via [`sharpebench_stats::significance::bootstrap_pvalue`]), the remaining headroom to the
//! oracle ceiling, and what fraction of the achievable ceiling the retrieval layer
//! has captured - plus a significance verdict at a caller-chosen `alpha`.
//!
//! Design invariants, matching the rest of the SharpeBench family:
//! - **Pure.** No I/O, no clock, no ambient randomness. The bootstrap seed is a
//!   fixed constant so a given input always yields the same report.
//! - **Deterministic.** Plain `f64` math, fixed reduction order.
//! - **No `unsafe`.** Inputs are validated only at the boundary
//!   ([`ablation_report`] returns `Err` on empty or mismatched arms).
//!
//! ## Beyond the three arms
//!
//! The three-arm ablation answers "does retrieval help?". Further legs answer the
//! questions a SOTA memory benchmark also has to, each pure, deterministic, and
//! reusing [`sharpebench_stats`] for any significance test:
//! - [`poisoning`] (E1) - does a set of injected corrupted records degrade outcomes
//!   versus the clean-retrieval arm? Behavior-integrity delta, attack-success rate,
//!   and bootstrap significance of the degradation. Money-memory is the
//!   high-severity case.
//! - [`multisession`] (E2) - interdependent multi-session scoring, where a later
//!   session's credit is conditioned on whether the memory an earlier session wrote
//!   was actually retained (not a flat per-task vector). Per-session lift plus a
//!   cross-session dependency-satisfaction rate.
//!
//! ## Example
//!
//! ```
//! use sharpebench_memory::{ablation_report, Arm, ArmScores};
//!
//! let baseline = ArmScores::new(Arm::Baseline, vec![0.10, 0.12, 0.09, 0.11]);
//! let retrieval = ArmScores::new(Arm::Retrieval, vec![0.72, 0.80, 0.75, 0.78]);
//! let oracle = ArmScores::new(Arm::Oracle, vec![0.94, 0.96, 0.93, 0.95]);
//!
//! let report = ablation_report(&baseline, &retrieval, &oracle, 0.05).unwrap();
//! assert!(report.retrieval_lift > 0.0);
//! assert!(report.significant); // the lift is not luck
//! assert!(report.headroom_to_oracle > 0.0); // still short of the ceiling
//! assert!((0.0..=1.0).contains(&report.fraction_of_ceiling));
//! ```
#![forbid(unsafe_code)]

pub mod multisession;
pub mod poisoning;

pub use multisession::{
    multi_session_report, MultiSessionReport, SessionId, SessionLift, SessionScores,
};
pub use poisoning::{poisoning_report, PoisoningReport};

use sharpebench_stats::{significance::bootstrap_pvalue, stats::mean};

/// Fixed bootstrap parameters so the report is reproducible for a given input.
/// A benchmark verdict must not move when re-run. Shared with the poisoning and
/// multi-session legs so every significance test in the crate resamples identically.
pub(crate) const BOOTSTRAP_SEED: u64 = 0x5EED_A11A_B1E5_0001;
pub(crate) const BOOTSTRAP_SAMPLES: usize = 4000;
/// Per-step probability of starting a new block (expected block length = 1/p).
/// ~0.1 is the standard stationary-bootstrap default for lightly serial data.
pub(crate) const BOOTSTRAP_BLOCK_PROB: f64 = 0.1;

/// Denominators below this magnitude are treated as zero when forming
/// [`AblationReport::fraction_of_ceiling`].
const CEILING_EPSILON: f64 = 1e-12;

/// Which arm of the ablation a score series belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Arm {
    /// No memory. The floor.
    Baseline,
    /// The memory/retrieval layer under test.
    Retrieval,
    /// Gold records only - perfect recall. The ceiling.
    Oracle,
    /// The retrieval layer with corrupted records injected into memory - the
    /// adversarial arm scored by [`poisoning::poisoning_report`].
    Poisoned,
}

impl Arm {
    /// Stable lowercase label, handy for logs and leaderboards.
    pub fn as_str(&self) -> &'static str {
        match self {
            Arm::Baseline => "baseline",
            Arm::Retrieval => "retrieval",
            Arm::Oracle => "oracle",
            Arm::Poisoned => "poisoned",
        }
    }
}

/// Per-task outcome scores for one arm (decision quality, realized reward, …).
/// One entry per task, in a caller-fixed task order. The baseline and retrieval
/// arms must share that order and length so the ablation can pair them per task.
#[derive(Debug, Clone, PartialEq)]
pub struct ArmScores {
    /// Which arm produced these scores.
    pub arm: Arm,
    /// One outcome score per task.
    pub scores: Vec<f64>,
}

impl ArmScores {
    /// Construct an arm's score series.
    pub fn new(arm: Arm, scores: Vec<f64>) -> Self {
        Self { arm, scores }
    }

    /// Number of tasks scored for this arm.
    pub fn len(&self) -> usize {
        self.scores.len()
    }

    /// Whether the arm has no scores.
    pub fn is_empty(&self) -> bool {
        self.scores.is_empty()
    }

    /// Mean outcome across this arm's tasks.
    pub fn mean(&self) -> f64 {
        mean(&self.scores)
    }
}

/// The scored three-arm ablation.
#[derive(Debug, Clone, PartialEq)]
pub struct AblationReport {
    /// Mean(Retrieval) - Mean(Baseline): how much the memory layer moved outcomes.
    pub retrieval_lift: f64,
    /// Stationary-bootstrap p-value that the paired per-task lift is > 0, via
    /// [`sharpebench_stats::significance::bootstrap_pvalue`]. Low ⇒ the lift is unlikely to be
    /// luck. 1.0 when the observed lift is non-positive.
    pub lift_pvalue: f64,
    /// Mean(Oracle) - Mean(Retrieval): outcome still left on the table versus a
    /// perfect-recall ceiling. Negative means retrieval beat the supplied oracle.
    pub headroom_to_oracle: f64,
    /// retrieval_lift / (Mean(Oracle) - Mean(Baseline)): the fraction of the
    /// achievable ceiling the retrieval layer captured. ~1.0 ⇒ near the ceiling.
    /// 0.0 when the ceiling gap is degenerate (oracle no better than baseline).
    pub fraction_of_ceiling: f64,
    /// Whether the lift is significant at `alpha` (i.e. `lift_pvalue < alpha`).
    pub significant: bool,
    /// The significance threshold used for the verdict.
    pub alpha: f64,
}

/// Score a three-arm memory ablation and test whether the retrieval layer's lift
/// over baseline is statistically real.
///
/// The lift significance is a stationary-bootstrap test on the **paired** per-task
/// differences `retrieval[i] - baseline[i]`, delegated to
/// [`sharpebench_stats::significance::bootstrap_pvalue`] - this crate does not implement its own
/// statistics. Pairing requires the baseline and retrieval arms to be aligned and
/// equal length; the oracle arm only contributes its mean, so it may differ in
/// length.
///
/// Deterministic: the bootstrap seed and sample count are fixed constants.
///
/// # Errors
///
/// Returns `Err` at the boundary when any arm is empty, when an arm is tagged with
/// the wrong [`Arm`] for its position, or when the baseline and retrieval arms have
/// mismatched lengths (they cannot be paired).
pub fn ablation_report(
    baseline: &ArmScores,
    retrieval: &ArmScores,
    oracle: &ArmScores,
    alpha: f64,
) -> Result<AblationReport, String> {
    if baseline.arm != Arm::Baseline {
        return Err(format!(
            "baseline arm mistagged as {}",
            baseline.arm.as_str()
        ));
    }
    if retrieval.arm != Arm::Retrieval {
        return Err(format!(
            "retrieval arm mistagged as {}",
            retrieval.arm.as_str()
        ));
    }
    if oracle.arm != Arm::Oracle {
        return Err(format!("oracle arm mistagged as {}", oracle.arm.as_str()));
    }
    if baseline.is_empty() || retrieval.is_empty() || oracle.is_empty() {
        return Err("every arm must have at least one scored task".to_string());
    }
    if baseline.len() != retrieval.len() {
        return Err(format!(
            "baseline ({}) and retrieval ({}) must be paired: equal task counts",
            baseline.len(),
            retrieval.len()
        ));
    }

    let base_mean = baseline.mean();
    let retr_mean = retrieval.mean();
    let oracle_mean = oracle.mean();

    let retrieval_lift = retr_mean - base_mean;

    // Paired per-task lift, fed to the shared stationary bootstrap. The bootstrap
    // returns 1.0 when the observed mean is non-positive, so a null lift is not
    // spuriously significant.
    let paired: Vec<f64> = retrieval
        .scores
        .iter()
        .zip(baseline.scores.iter())
        .map(|(r, b)| r - b)
        .collect();
    let lift_pvalue = bootstrap_pvalue(
        &paired,
        BOOTSTRAP_SEED,
        BOOTSTRAP_SAMPLES,
        BOOTSTRAP_BLOCK_PROB,
    );

    let headroom_to_oracle = oracle_mean - retr_mean;

    let ceiling_gap = oracle_mean - base_mean;
    let fraction_of_ceiling = if ceiling_gap.abs() < CEILING_EPSILON {
        0.0
    } else {
        retrieval_lift / ceiling_gap
    };

    Ok(AblationReport {
        retrieval_lift,
        lift_pvalue,
        headroom_to_oracle,
        fraction_of_ceiling,
        significant: lift_pvalue < alpha,
        alpha,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const EPS: f64 = 1e-9;

    fn baseline(scores: Vec<f64>) -> ArmScores {
        ArmScores::new(Arm::Baseline, scores)
    }
    fn retrieval(scores: Vec<f64>) -> ArmScores {
        ArmScores::new(Arm::Retrieval, scores)
    }
    fn oracle(scores: Vec<f64>) -> ArmScores {
        ArmScores::new(Arm::Oracle, scores)
    }

    #[test]
    fn clear_lift_is_significant_and_within_ceiling() {
        let b = baseline(vec![0.10, 0.12, 0.09, 0.11, 0.08, 0.10]);
        let r = retrieval(vec![0.72, 0.80, 0.75, 0.78, 0.74, 0.79]);
        let o = oracle(vec![0.94, 0.96, 0.93, 0.95, 0.92, 0.97]);

        let rep = ablation_report(&b, &r, &o, 0.05).unwrap();

        assert!(rep.retrieval_lift > 0.5, "lift {}", rep.retrieval_lift);
        assert!(rep.significant, "pvalue {}", rep.lift_pvalue);
        assert!(rep.lift_pvalue < 0.05);
        // Captured most, but not all, of the achievable ceiling.
        assert!(
            rep.fraction_of_ceiling > 0.0 && rep.fraction_of_ceiling < 1.0,
            "fraction {}",
            rep.fraction_of_ceiling
        );
        assert!(rep.headroom_to_oracle > 0.0);
    }

    #[test]
    fn null_lift_is_not_significant() {
        let scores = vec![0.40, 0.42, 0.38, 0.41, 0.39];
        let b = baseline(scores.clone());
        let r = retrieval(scores.clone());
        let o = oracle(vec![0.90, 0.91, 0.89, 0.92, 0.88]);

        let rep = ablation_report(&b, &r, &o, 0.05).unwrap();

        assert!((rep.retrieval_lift - 0.0).abs() < EPS);
        assert!(!rep.significant);
        // bootstrap returns 1.0 for a non-positive observed mean.
        assert!((rep.lift_pvalue - 1.0).abs() < EPS);
        // No lift captured, so zero fraction of the ceiling.
        assert!((rep.fraction_of_ceiling - 0.0).abs() < EPS);
    }

    #[test]
    fn headroom_and_fraction_are_computed_correctly() {
        let b = baseline(vec![0.0, 0.2]); // mean 0.1
        let r = retrieval(vec![0.5, 0.7]); // mean 0.6
        let o = oracle(vec![0.8, 1.0]); // mean 0.9

        let rep = ablation_report(&b, &r, &o, 0.05).unwrap();

        assert!((rep.retrieval_lift - 0.5).abs() < EPS);
        assert!((rep.headroom_to_oracle - 0.3).abs() < EPS); // 0.9 - 0.6
                                                             // ceiling gap = 0.9 - 0.1 = 0.8; fraction = 0.5 / 0.8 = 0.625
        assert!((rep.fraction_of_ceiling - 0.625).abs() < EPS);
    }

    #[test]
    fn degenerate_ceiling_yields_zero_fraction() {
        let b = baseline(vec![0.5, 0.5]);
        let r = retrieval(vec![0.6, 0.6]);
        let o = oracle(vec![0.5, 0.5]); // oracle no better than baseline
        let rep = ablation_report(&b, &r, &o, 0.05).unwrap();
        assert!((rep.fraction_of_ceiling - 0.0).abs() < EPS);
    }

    #[test]
    fn empty_arm_errors_cleanly() {
        let b = baseline(vec![]);
        let r = retrieval(vec![0.5]);
        let o = oracle(vec![0.9]);
        assert!(ablation_report(&b, &r, &o, 0.05).is_err());
    }

    #[test]
    fn mismatched_pairing_errors_cleanly() {
        let b = baseline(vec![0.1, 0.2, 0.3]);
        let r = retrieval(vec![0.5, 0.6]); // shorter - cannot pair
        let o = oracle(vec![0.9, 0.9, 0.9]);
        assert!(ablation_report(&b, &r, &o, 0.05).is_err());
    }

    #[test]
    fn mistagged_arm_errors_cleanly() {
        // Oracle scores handed in the baseline slot.
        let mistagged = ArmScores::new(Arm::Oracle, vec![0.1, 0.2]);
        let r = retrieval(vec![0.5, 0.6]);
        let o = oracle(vec![0.9, 0.9]);
        assert!(ablation_report(&mistagged, &r, &o, 0.05).is_err());
    }

    #[test]
    fn arm_labels_are_stable() {
        assert_eq!(Arm::Baseline.as_str(), "baseline");
        assert_eq!(Arm::Retrieval.as_str(), "retrieval");
        assert_eq!(Arm::Oracle.as_str(), "oracle");
        assert_eq!(Arm::Poisoned.as_str(), "poisoned");
    }
}
