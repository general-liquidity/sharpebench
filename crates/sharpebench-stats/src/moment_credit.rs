//! The admission test's floor on the Sharpe standard error: estimated skewness
//! and kurtosis may widen it beyond the Gaussian one, never narrow it.
//!
//! The kernel's deflated Sharpe divides by the standard error of Mertens (2002)
//! and Bailey and López de Prado (2012), whose bracket
//! `1 - g3 SR + (g4 - 1) / 4 SR^2` falls below the Gaussian `1 + SR^2 / 2` of
//! Lo (2002) when the sample skewness is positive or the sample kurtosis is
//! below three. Those are asymptotic corrections. On a track whose activity
//! sits in a few bars, the sample skewness and kurtosis are functions of those
//! few draws, and the credit they grant is the evidence the track lacks.
//!
//! `grader-search` found it on 2026-10-30 as the `window_boundary` move: windows
//! that are flat except for their last three bars, each bar zero-mean by
//! construction. Three equal positive draws in a window of thirty make a sample
//! skewness near 2.7 and a bracket near 0.3 against a Gaussian 1.05, so the
//! deflated Sharpe clears 0.95 about as often as the three draws share a sign,
//! one window in eight. The window edge
//! is not what the move exploits. Every statistic the deflated Sharpe reads is
//! invariant to the order of the bars, and only the block bootstrap, which keeps
//! adjacent bars together, protected the edge placement: the same three bars
//! scattered through a single thirty-bar window were admitted at 14% of 2,000
//! seeds, against 1.75% at the edge and 2.4% for an i.i.d. Gaussian window. A rule on
//! where activity sits would move the attack; this floor reads moments only, so
//! it holds under any placement of the bars and of the window boundaries.
//!
//! The floor can only refuse a track the gate admitted, and leaves a track
//! whose moments widen the interval exactly where the kernel puts it.

use crate::deflated_sharpe::{
    deflated_sharpe_ratio_against_null, expected_max_sharpe, observed_sharpe_ratio,
};
use crate::stats::{kurtosis, norm_cdf, skewness};
use crate::validation::{
    finite_computation, finite_observations, finite_parameter, StatisticalError,
};

/// The requirement the admission test refuses a track on when its deflated
/// Sharpe clears the bar only through the precision its sample skewness or
/// kurtosis credits.
///
/// A scoring change introduced after release 0.28.0 (grader-search finding of
/// 2026-10-30): such a track was eligible under 0.28.0.
pub const NOT_CERTIFIED_WITHOUT_MOMENT_CREDIT: &str =
    "must clear the deflation bar with a Sharpe standard error no smaller than the Gaussian one \
     (Lo 2002): its sample skewness or kurtosis narrows the standard error, and a track whose \
     activity sits in a few bars manufactures that credit";

/// The Gaussian bracket of Lo (2002), `1 + SR^2 / 2`: the variance factor of
/// the Sharpe estimator for i.i.d. Normal returns, the kernel's bracket at
/// skewness 0 and kurtosis 3.
fn gaussian_bracket(sr: f64) -> f64 {
    1.0 + sr * sr / 2.0
}

/// The deflated Sharpe of `returns` against `null_mean_sharpe` plus the expected
/// maximum of `n_trials` trials, with the bracket floored at the Gaussian one:
/// `max(1 - g3 SR + (g4 - 1) / 4 SR^2, 1 + SR^2 / 2)`, evaluated at the observed
/// Sharpe as the kernel's deflated Sharpe is.
///
/// Equal, bit for bit, to the kernel's deflated Sharpe on a track whose moments
/// already put the bracket at or above the Gaussian one. Scale-free and
/// invariant to the order of the returns.
pub fn deflated_sharpe_without_moment_credit(
    returns: &[f64],
    n_trials: u32,
    null_mean_sharpe: f64,
    trials_sr_std: f64,
) -> Result<f64, StatisticalError> {
    finite_observations(returns)?;
    finite_parameter(null_mean_sharpe, "null_mean_sharpe")?;
    let sr_star = finite_computation(
        null_mean_sharpe + expected_max_sharpe(trials_sr_std, n_trials)?,
        "deflation benchmark",
    )?;
    if returns.len() < 2 {
        return Err(StatisticalError::InsufficientObservations {
            required: 2,
            actual: returns.len(),
        });
    }
    let sr = observed_sharpe_ratio(returns)?;
    let g3 = finite_computation(skewness(returns), "return skewness")?;
    let g4 = finite_computation(kurtosis(returns), "return kurtosis")?;
    let kernel = finite_computation(1.0 - g3 * sr + ((g4 - 1.0) / 4.0) * sr * sr, "PSR variance")?;
    let variance = kernel.max(gaussian_bracket(sr));
    let numerator = finite_computation(
        (sr - sr_star) * (returns.len() as f64 - 1.0).sqrt(),
        "PSR numerator",
    )?;
    let z = finite_computation(numerator / variance.max(1e-12).sqrt(), "PSR z statistic")?;
    finite_computation(norm_cdf(z), "PSR probability")
}

/// The admission test's moment-credit term: `Ok` unless the track's deflated
/// Sharpe clears `dsr_bar` and does not once the bracket is floored at the
/// Gaussian one, which is refused with [`NOT_CERTIFIED_WITHOUT_MOMENT_CREDIT`].
///
/// It never admits a track: a track the kernel's test refuses is left to that
/// refusal, and a floor that cannot be computed on a track the kernel admits is
/// an error, not a pass.
pub fn certify_without_moment_credit(
    returns: &[f64],
    n_trials: u32,
    null_mean_sharpe: f64,
    trials_sr_std: f64,
    dsr_bar: f64,
) -> Result<(), StatisticalError> {
    let kernel =
        deflated_sharpe_ratio_against_null(returns, n_trials, null_mean_sharpe, trials_sr_std)?;
    if kernel < dsr_bar {
        return Ok(());
    }
    let floored =
        deflated_sharpe_without_moment_credit(returns, n_trials, null_mean_sharpe, trials_sr_std)?;
    if floored < dsr_bar {
        return Err(StatisticalError::InvalidParameter {
            name: "returns",
            requirement: NOT_CERTIFIED_WITHOUT_MOMENT_CREDIT,
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Moments that already widen the interval leave the kernel's deflated
    /// Sharpe as it is, bit for bit: a negatively skewed track.
    #[test]
    fn a_track_whose_moments_widen_the_interval_keeps_the_kernel_value() {
        let returns: Vec<f64> = (0..120)
            .map(|t| {
                if t % 10 == 0 {
                    -0.02
                } else {
                    0.004 + 0.001 * f64::from(t).sin()
                }
            })
            .collect();
        let kernel = deflated_sharpe_ratio_against_null(&returns, 50, 0.0, 0.03).unwrap();
        let floored = deflated_sharpe_without_moment_credit(&returns, 50, 0.0, 0.03).unwrap();
        assert_eq!(floored.to_bits(), kernel.to_bits());
    }

    /// Three positive draws in a flat window: the kernel's bracket credits the
    /// skewness they make and the floor takes the credit back.
    #[test]
    fn a_window_of_three_positive_draws_loses_its_skewness_credit() {
        let mut returns = vec![0.0; 30];
        returns[27..].copy_from_slice(&[0.0006, 0.0006, 0.0011]);
        let kernel = deflated_sharpe_ratio_against_null(&returns, 50, 0.0, 0.03).unwrap();
        let floored = deflated_sharpe_without_moment_credit(&returns, 50, 0.0, 0.03).unwrap();
        assert!(kernel >= 0.95, "{kernel}");
        assert!(floored < 0.95, "{floored}");
        assert_eq!(
            certify_without_moment_credit(&returns, 50, 0.0, 0.03, 0.95),
            Err(StatisticalError::InvalidParameter {
                name: "returns",
                requirement: NOT_CERTIFIED_WITHOUT_MOMENT_CREDIT,
            })
        );
    }

    fn three_positive_draws() -> Vec<f64> {
        let mut returns = vec![0.0; 30];
        returns[27..].copy_from_slice(&[0.0006, 0.0006, 0.0011]);
        returns
    }

    /// Lo (2002): the variance factor of the Sharpe estimator under i.i.d.
    /// Normal returns is `1 + SR^2 / 2`. At SR 0.5 that is 1.125, at SR 3 it is
    /// 5.5, and at SR 0 it is 1.
    #[test]
    fn the_gaussian_bracket_is_lo_2002_at_hand_computed_values() {
        assert_eq!(gaussian_bracket(0.0), 1.0);
        assert_eq!(gaussian_bracket(0.5), 1.125);
        assert_eq!(gaussian_bracket(3.0), 5.5);
        assert_eq!(gaussian_bracket(-3.0), 5.5);
    }

    /// Where the floor binds, the deflated Sharpe is the PSR with the Gaussian
    /// bracket in place of the kernel's, bit for bit.
    #[test]
    fn a_floored_track_is_scored_with_the_gaussian_bracket() {
        let returns = three_positive_draws();
        let sr = observed_sharpe_ratio(&returns).unwrap();
        let kernel_bracket =
            1.0 - skewness(&returns) * sr + ((kurtosis(&returns) - 1.0) / 4.0) * sr * sr;
        assert!(kernel_bracket < gaussian_bracket(sr), "{kernel_bracket}");
        let z = sr * (returns.len() as f64 - 1.0).sqrt() / (1.0 + sr * sr / 2.0).sqrt();
        let floored = deflated_sharpe_without_moment_credit(&returns, 1, 0.0, 0.0).unwrap();
        assert_eq!(floored.to_bits(), norm_cdf(z).to_bits());
    }

    /// The rule refuses exactly the tracks whose kernel deflated Sharpe is at
    /// or above the bar and whose floored one is below it, both comparisons
    /// inclusive of the bar as the kernel's gate is.
    #[test]
    fn certification_refuses_only_between_the_floored_and_kernel_values() {
        let returns = three_positive_draws();
        let kernel = deflated_sharpe_ratio_against_null(&returns, 50, 0.0, 0.03).unwrap();
        let floored = deflated_sharpe_without_moment_credit(&returns, 50, 0.0, 0.03).unwrap();
        assert!(floored < kernel, "{floored} {kernel}");
        let refused = Err(StatisticalError::InvalidParameter {
            name: "returns",
            requirement: NOT_CERTIFIED_WITHOUT_MOMENT_CREDIT,
        });
        let certify = |bar: f64| certify_without_moment_credit(&returns, 50, 0.0, 0.03, bar);
        assert_eq!(certify(kernel), refused);
        assert_eq!(certify(floored), Ok(()));
        // A bar above the kernel value is the kernel gate's refusal, not this one.
        assert_eq!(certify((kernel + 1.0) / 2.0), Ok(()));
    }

    /// Two returns are the fewest the floored deflated Sharpe reads; one is
    /// refused by count, not scored.
    #[test]
    fn two_returns_are_the_boundary_of_a_floored_deflated_sharpe() {
        let two = deflated_sharpe_without_moment_credit(&[0.01, 0.03], 1, 0.0, 0.0).unwrap();
        let kernel = deflated_sharpe_ratio_against_null(&[0.01, 0.03], 1, 0.0, 0.0).unwrap();
        assert!(two.is_finite() && two <= kernel, "{two} {kernel}");
        assert_eq!(
            deflated_sharpe_without_moment_credit(&[0.01], 1, 0.0, 0.0),
            Err(StatisticalError::InsufficientObservations {
                required: 2,
                actual: 1
            })
        );
    }
}
