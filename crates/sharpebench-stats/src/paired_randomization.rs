//! Whole-unit paired label-swap inference for additive score functionals.
//!
//! Each row contains a unit's statistic in the observed and swapped orientations.
//! The two need not be negatives: selection/qualification must be recomputed after
//! swapping the complete unit, not frozen at the observed outcomes. The overall
//! statistic is an equal-unit mean. Under the null, complete arm labels must be
//! independently exchangeable within each declared unit. Row IDs or repeated tasks
//! cannot establish that assumption. This is not a general test of zero mean alone.
//!
//! Tail counting follows the paired-sample permutation convention: inclusive upper
//! tail, exact enumeration when affordable, otherwise seeded draws with replacement
//! and `(extreme + 1) / (draws + 1)`. See the primary implementation documentation:
//! <https://docs.scipy.org/doc/scipy/reference/generated/scipy.stats.permutation_test.html>.

use crate::{significance::SplitMix64, StatisticalError};

/// Bounds the requested work and keeps all reported counts exactly representable.
pub const MAX_PAIR_SWAP_RESAMPLES: usize = 1_000_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PairedSwapConfig {
    /// Enumerate all `2^units` assignments if at most this many; otherwise sample.
    /// Must be in `1..=MAX_PAIR_SWAP_RESAMPLES`. Choose before seeing the outcomes.
    pub resamples: usize,
    /// Used only for Monte Carlo; choose before seeing outcomes, not by p-value search.
    pub seed: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PairedSwapMethod {
    ExactEnumeration,
    MonteCarlo,
}

#[derive(Clone, Debug, PartialEq)]
pub struct PairedSwapTest {
    pub observed_mean: f64,
    pub pvalue: f64,
    pub independent_units: usize,
    /// Actual enumerated assignments or Monte Carlo draws, before any +1 adjustment.
    pub assignments: usize,
    /// Inclusive tail count, before any +1 adjustment.
    pub extreme_assignments: usize,
    pub method: PairedSwapMethod,
    pub seed: Option<u64>,
    /// Conservative absolute tie tolerance: 100 epsilon times the largest absolute
    /// orientation score. This scale is invariant to arm swaps, including at zero.
    pub tie_tolerance: f64,
}

fn checked_mean(
    mut values: impl Iterator<Item = f64>,
    count: usize,
) -> Result<f64, StatisticalError> {
    let total = values.try_fold(0.0, |total, value| {
        let next = total + value;
        if next.is_finite() {
            Ok(next)
        } else {
            Err(StatisticalError::NonFiniteComputation {
                quantity: "paired-swap score sum",
            })
        }
    })?;
    Ok(total / count as f64)
}

/// Test a predeclared equal-unit score for unusually large observed values under
/// independent whole-unit arm exchangeability. `orientations[i][0]` is observed;
/// `[1]` is the score after swapping ALL paired arms in unit `i` and reevaluating
/// every outcome-dependent gate. Do not pass one row per dependent task/session.
///
/// Rows are reduced in input order. Consumers with keyed units should first sort
/// by stable identity. Finite scores and representable intermediate sums are required.
/// Fewer than two units or invalid resampling parameters return a typed error.
pub fn paired_swap_test(
    orientations: &[[f64; 2]],
    config: PairedSwapConfig,
) -> Result<PairedSwapTest, StatisticalError> {
    if orientations.len() < 2 {
        return Err(StatisticalError::InsufficientObservations {
            required: 2,
            actual: orientations.len(),
        });
    }
    if config.resamples == 0 || config.resamples > MAX_PAIR_SWAP_RESAMPLES {
        return Err(StatisticalError::InvalidParameter {
            name: "resamples",
            requirement: "must be in 1..=1_000_000",
        });
    }
    let mut scale: f64 = 0.0;
    for (index, scores) in orientations.iter().enumerate() {
        for &score in scores {
            if !score.is_finite() {
                return Err(StatisticalError::NonFiniteObservation { index });
            }
            scale = scale.max(score.abs());
        }
    }
    let tie_tolerance = 100.0 * f64::EPSILON * scale;
    let observed_mean = checked_mean(
        orientations.iter().map(|scores| scores[0]),
        orientations.len(),
    )?;
    let exact_count = u32::try_from(orientations.len())
        .ok()
        .and_then(|n| 1usize.checked_shl(n));
    let exact = exact_count.is_some_and(|count| count <= config.resamples);
    let assignments = if exact {
        exact_count.unwrap()
    } else {
        config.resamples
    };
    let mut rng = SplitMix64::new(config.seed);
    let mut extreme_assignments = 0;
    for assignment in 0..assignments {
        let value = checked_mean(
            orientations.iter().enumerate().map(|(i, scores)| {
                let swapped = if exact {
                    (assignment >> i) & 1
                } else {
                    (rng.next_u64() & 1) as usize
                };
                scores[swapped]
            }),
            orientations.len(),
        )?;
        if value >= observed_mean || observed_mean - value <= tie_tolerance {
            extreme_assignments += 1;
        }
    }
    let pvalue = if exact {
        extreme_assignments as f64 / assignments as f64
    } else {
        (extreme_assignments + 1) as f64 / (assignments + 1) as f64
    };
    Ok(PairedSwapTest {
        observed_mean,
        pvalue,
        independent_units: orientations.len(),
        assignments,
        extreme_assignments,
        tie_tolerance,
        method: if exact {
            PairedSwapMethod::ExactEnumeration
        } else {
            PairedSwapMethod::MonteCarlo
        },
        seed: if exact { None } else { Some(config.seed) },
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    const CONFIG: PairedSwapConfig = PairedSwapConfig {
        resamples: 9999,
        seed: 42,
    };

    #[test]
    fn exact_tail_uses_both_orientations_not_sign_flips_of_observed_score() {
        // Means: 1.5, 2.5, -2, -1. Only the first two reach observed 1.5.
        let test = paired_swap_test(&[[1.0, 3.0], [2.0, -5.0]], CONFIG).unwrap();
        assert_eq!(test.observed_mean, 1.5);
        assert_eq!(test.pvalue, 0.5);
        assert_eq!((test.extreme_assignments, test.assignments), (2, 4));
        assert_eq!(test.method, PairedSwapMethod::ExactEnumeration);
        assert_eq!(test.seed, None);
    }

    #[test]
    fn exact_all_positive_case_counts_observed_assignment_once() {
        let test = paired_swap_test(&[[1.0, -1.0]; 6], CONFIG).unwrap();
        assert_eq!(test.pvalue, 1.0 / 64.0);
        assert_eq!(test.extreme_assignments, 1);
        assert_eq!(test.assignments, 64);
    }

    #[test]
    fn ties_are_inclusive_including_zero_and_roundoff_near_zero() {
        for value in [0.0, 1.0, -1.0] {
            assert_eq!(
                paired_swap_test(&[[value, value]; 3], CONFIG)
                    .unwrap()
                    .pvalue,
                1.0
            );
        }
        let test = paired_swap_test(&[[1.0, 1.0 - f64::EPSILON], [-1.0, -1.0]], CONFIG).unwrap();
        assert_eq!(test.observed_mean, 0.0);
        assert_eq!(test.pvalue, 1.0);
    }

    #[test]
    fn monte_carlo_is_reproducible_smoothed_and_does_not_shift_by_large_unit_count() {
        let config = PairedSwapConfig {
            resamples: 31,
            seed: 42,
        };
        let units = [[1.0, -1.0]; 70];
        let a = paired_swap_test(&units, config).unwrap();
        let b = paired_swap_test(&units, config).unwrap();
        assert_eq!(a, b);
        assert_eq!(a.method, PairedSwapMethod::MonteCarlo);
        assert_eq!(a.seed, Some(42));
        assert_eq!(a.assignments, 31);
        assert_eq!(a.extreme_assignments, 0);
        assert_eq!(a.pvalue, 1.0 / 32.0);
    }

    #[test]
    fn enumeration_switch_occurs_at_the_full_assignment_count() {
        let units = [[1.0, -1.0]; 3];
        assert_eq!(
            paired_swap_test(
                &units,
                PairedSwapConfig {
                    resamples: 7,
                    ..CONFIG
                }
            )
            .unwrap()
            .method,
            PairedSwapMethod::MonteCarlo
        );
        let exact = paired_swap_test(
            &units,
            PairedSwapConfig {
                resamples: 8,
                ..CONFIG
            },
        )
        .unwrap();
        assert_eq!(exact.method, PairedSwapMethod::ExactEnumeration);
        assert_eq!(exact.pvalue, 0.125);
    }

    #[test]
    fn invalid_support_scores_parameters_and_arithmetic_are_errors_not_pvalues() {
        assert!(paired_swap_test(&[], CONFIG).is_err());
        assert!(paired_swap_test(&[[1.0, -1.0]], CONFIG).is_err());
        for resamples in [0, MAX_PAIR_SWAP_RESAMPLES + 1, usize::MAX] {
            assert!(paired_swap_test(
                &[[1.0, -1.0]; 2],
                PairedSwapConfig {
                    resamples,
                    ..CONFIG
                }
            )
            .is_err());
        }
        for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            for pair in [[value, 0.0], [0.0, value]] {
                assert!(paired_swap_test(&[pair; 2], CONFIG).is_err());
            }
        }
        // Both observed and resampled reductions must be checked.
        assert!(paired_swap_test(&[[f64::MAX, 0.0]; 2], CONFIG).is_err());
        assert!(paired_swap_test(&[[0.0, f64::MAX]; 2], CONFIG).is_err());
    }
}
