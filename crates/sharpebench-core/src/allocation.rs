//! Allocation-vector scoring contract + turnover penalty.
//!
//! SharpeBench's primary contract is order-level (per-order rationale, a risk gate
//! per order, partial fills). This adds a second, additive contract for agents that
//! express intent as a continuous **target-allocation vector** rebalanced each
//! cycle, the way a portfolio-allocation agent does. Two things are scored that the
//! order-level path can't see:
//!
//! - **Weight validity** — a vector that over-leverages (gross > cap) or goes short
//!   when shorts are disallowed is the allocation-analogue of a deny-list breach,
//!   i.e. a discipline-zeroing violation.
//! - **Turnover** — the L1 churn `Σ|wₜ − wₜ₋₁|` across rebalances, a first-class
//!   cost an agent that "wins" by frantic reallocation should be charged for.
//!
//! Pure and deterministic: the caller supplies the realized allocation trajectory.

use serde::{Deserialize, Serialize};

/// One cycle's target-allocation vector (weights per instrument, in a fixed order).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AllocationStep {
    pub weights: Vec<f64>,
}

/// The realized sequence of target allocations the account rebalanced to.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct AllocationTrajectory {
    pub steps: Vec<AllocationStep>,
}

/// Validity limits a target-allocation vector must respect.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AllocationPolicy {
    /// Whether negative (short) weights are permitted.
    pub allow_shorts: bool,
    /// Cap on gross exposure `Σ|wᵢ|` (1.0 = fully invested, no leverage).
    pub max_gross: f64,
    /// Tolerance for the gross-exposure comparison (floating-point slack).
    pub epsilon: f64,
}

impl Default for AllocationPolicy {
    fn default() -> Self {
        AllocationPolicy {
            allow_shorts: false,
            max_gross: 1.0,
            epsilon: 1e-9,
        }
    }
}

/// A specific way a weight vector violates the policy.
#[derive(Clone, Debug, Serialize, PartialEq)]
#[serde(tag = "violation", rename_all = "snake_case")]
pub enum WeightViolation {
    /// A weight is NaN or infinite — an abusive/garbage vector.
    NonFiniteWeight { index: usize },
    /// A negative weight while shorts are disallowed.
    NegativeWeight { index: usize, weight: f64 },
    /// Gross exposure exceeded the leverage cap.
    GrossExposureExceeded { gross: f64, cap: f64 },
}

/// The validity verdict for one weight vector.
#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct WeightValidity {
    pub valid: bool,
    pub violations: Vec<WeightViolation>,
}

/// Validate a single weight vector against the policy.
pub fn check_weights(weights: &[f64], policy: &AllocationPolicy) -> WeightValidity {
    let mut violations = Vec::new();
    let mut gross = 0.0;
    for (index, &w) in weights.iter().enumerate() {
        if !w.is_finite() {
            violations.push(WeightViolation::NonFiniteWeight { index });
            continue;
        }
        if w < 0.0 && !policy.allow_shorts {
            violations.push(WeightViolation::NegativeWeight { index, weight: w });
        }
        gross += w.abs();
    }
    if gross > policy.max_gross + policy.epsilon {
        violations.push(WeightViolation::GrossExposureExceeded {
            gross,
            cap: policy.max_gross,
        });
    }
    WeightValidity {
        valid: violations.is_empty(),
        violations,
    }
}

/// Total L1 turnover `Σₜ Σᵢ |wₜ,ᵢ − wₜ₋₁,ᵢ|`. The first step is measured against an
/// all-cash (all-zero) prior, so initial deployment counts as turnover. Vectors of
/// differing lengths are compared element-wise with the shorter side zero-padded.
pub fn turnover(trajectory: &AllocationTrajectory) -> f64 {
    let mut total = 0.0;
    let mut prev: Vec<f64> = Vec::new();
    for step in &trajectory.steps {
        let n = step.weights.len().max(prev.len());
        for i in 0..n {
            let cur = step.weights.get(i).copied().unwrap_or(0.0);
            let old = prev.get(i).copied().unwrap_or(0.0);
            total += (cur - old).abs();
        }
        prev = step.weights.clone();
    }
    total
}

/// The full allocation score: aggregate weight validity across every step plus the
/// trajectory's turnover.
#[derive(Clone, Debug, Serialize)]
pub struct AllocationReport {
    pub total_turnover: f64,
    /// `total_turnover / steps`, or 0 for an empty trajectory.
    pub mean_turnover: f64,
    /// Every weight violation found, across all steps (a non-empty list = ineligible).
    pub weight_violations: Vec<WeightViolation>,
    pub valid: bool,
}

/// Score an allocation trajectory: validity (any breach across any step zeroes
/// `valid`, mirroring the order-level deny-list semantics) and turnover churn.
pub fn score_allocation(
    trajectory: &AllocationTrajectory,
    policy: &AllocationPolicy,
) -> AllocationReport {
    let mut weight_violations = Vec::new();
    for step in &trajectory.steps {
        weight_violations.extend(check_weights(&step.weights, policy).violations);
    }
    let total_turnover = turnover(trajectory);
    let mean_turnover = if trajectory.steps.is_empty() {
        0.0
    } else {
        total_turnover / trajectory.steps.len() as f64
    };
    AllocationReport {
        total_turnover,
        mean_turnover,
        valid: weight_violations.is_empty(),
        weight_violations,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn traj(steps: &[&[f64]]) -> AllocationTrajectory {
        AllocationTrajectory {
            steps: steps
                .iter()
                .map(|w| AllocationStep {
                    weights: w.to_vec(),
                })
                .collect(),
        }
    }

    #[test]
    fn valid_low_turnover_trajectory_scores_with_hand_computed_turnover() {
        // prior [0,0]:
        //  step1 |0.5-0|+|0.5-0| = 1.0
        //  step2 |0.5-0.5|+|0.5-0.5| = 0.0
        //  step3 |0.0-0.5|+|1.0-0.5| = 1.0  -> total 2.0, mean 2/3
        let t = traj(&[&[0.5, 0.5], &[0.5, 0.5], &[0.0, 1.0]]);
        let r = score_allocation(&t, &AllocationPolicy::default());
        assert!(r.valid, "{:?}", r.weight_violations);
        assert!((r.total_turnover - 2.0).abs() < 1e-12);
        assert!((r.mean_turnover - 2.0 / 3.0).abs() < 1e-12);
    }

    #[test]
    fn over_leveraged_vector_flags_gross_exposure() {
        let t = traj(&[&[0.7, 0.7]]); // gross 1.4 > 1.0 cap
        let r = score_allocation(&t, &AllocationPolicy::default());
        assert!(!r.valid);
        assert!(r.weight_violations.iter().any(|v| matches!(
            v,
            WeightViolation::GrossExposureExceeded { cap, .. } if (*cap - 1.0).abs() < 1e-12
        )));
    }

    #[test]
    fn negative_weight_flags_when_shorts_disallowed() {
        let t = traj(&[&[-0.3, 0.5]]);
        let r = score_allocation(&t, &AllocationPolicy::default());
        assert!(!r.valid);
        assert!(r
            .weight_violations
            .iter()
            .any(|v| matches!(v, WeightViolation::NegativeWeight { index: 0, .. })));
    }

    #[test]
    fn shorts_allowed_permits_negative_within_gross_cap() {
        let policy = AllocationPolicy {
            allow_shorts: true,
            max_gross: 2.0,
            ..Default::default()
        };
        let t = traj(&[&[-0.5, 0.5]]); // gross 1.0 <= 2.0
        let r = score_allocation(&t, &policy);
        assert!(r.valid, "{:?}", r.weight_violations);
    }

    #[test]
    fn non_finite_weight_flags() {
        let t = traj(&[&[f64::NAN, 0.5]]);
        let r = score_allocation(&t, &AllocationPolicy::default());
        assert!(!r.valid);
        assert!(r
            .weight_violations
            .iter()
            .any(|v| matches!(v, WeightViolation::NonFiniteWeight { index: 0 })));
    }

    #[test]
    fn empty_trajectory_is_valid_with_zero_turnover() {
        let r = score_allocation(
            &AllocationTrajectory::default(),
            &AllocationPolicy::default(),
        );
        assert!(r.valid);
        assert_eq!(r.total_turnover, 0.0);
        assert_eq!(r.mean_turnover, 0.0);
    }
}
