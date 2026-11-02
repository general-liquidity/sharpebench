//! The deflation family with the PSR standard error evaluated where the caller
//! says: at the observed Sharpe (the kernel's plug-in, López de Prado, Lipton
//! and Zoonekynd 2026, eq. 3, p. 9) or under the null at the benchmark Sharpe
//! (the same paper's eqs. 4 and 5, p. 10).
//!
//! [`StandardErrorAt::Observed`] delegates to the kernel functions in
//! [`crate::deflated_sharpe`], so it is those functions bit for bit and the
//! kernel module itself is unchanged. [`StandardErrorAt::Benchmark`] evaluates
//! the bracket with [`sharpe_variance_factor`] at zero autocorrelation, the
//! function the `null-se-psr` diagnostic reads, behind the same checked
//! boundary as the kernel PSR: a non-finite input is refused, a constant
//! observed track is refused, and fewer than two returns keep the 0.0
//! convention. Its denominator does not depend on the observed Sharpe, so for
//! fixed moments the PSR is non-decreasing in it, where the plug-in PSR falls
//! once `g3 SR > 2` (`tests/dsr_psr_properties.rs`).
//!
//! This is the opt-in methodology behind `ScoreConfig::psr_standard_error` in
//! `sharpebench-core`. The default configuration never calls the benchmark
//! evaluation.

use crate::deflated_sharpe::{
    checked_probabilistic_sharpe_ratio, deflated_sharpe_ratio_of, expected_max_sharpe,
    observed_sharpe_ratio, Track,
};
use crate::opt_in_diagnostics::{sharpe_variance_factor, StandardErrorAt};
use crate::stats::{kurtosis, mean, norm_cdf, skewness, std_dev};
use crate::validation::{
    finite_computation, finite_observations, finite_parameter, StatisticalError,
};

/// The bracket of the PSR standard error, `T - 1` times the variance of the
/// Sharpe estimator, evaluated where `at` says: at the observed Sharpe `sr` (the
/// kernel's plug-in in its own order of operations, so an
/// [`StandardErrorAt::Observed`] caller keeps its bytes) or at `sr_benchmark`
/// through [`sharpe_variance_factor`] at zero autocorrelation.
pub(crate) fn psr_bracket(
    sr: f64,
    sr_benchmark: f64,
    g3: f64,
    g4: f64,
    at: StandardErrorAt,
) -> Result<f64, StatisticalError> {
    match at {
        StandardErrorAt::Observed => {
            finite_computation(1.0 - g3 * sr + ((g4 - 1.0) / 4.0) * sr * sr, "PSR variance")
        }
        StandardErrorAt::Benchmark => sharpe_variance_factor(sr_benchmark, g3, g4, 0.0),
    }
}

/// [`checked_probabilistic_sharpe_ratio`] with the standard error evaluated
/// where `at` says. [`StandardErrorAt::Observed`] is that function, bit for bit.
pub fn checked_probabilistic_sharpe_ratio_at(
    returns: &[f64],
    sr_benchmark: f64,
    at: StandardErrorAt,
) -> Result<f64, StatisticalError> {
    match at {
        StandardErrorAt::Observed => checked_probabilistic_sharpe_ratio(returns, sr_benchmark),
        StandardErrorAt::Benchmark => {
            finite_observations(returns)?;
            finite_parameter(sr_benchmark, "sr_benchmark")?;
            null_se_psr(returns, sr_benchmark, Track::Observed)
        }
    }
}

/// [`crate::deflated_sharpe::deflated_sharpe_ratio_against_null`] with the PSR
/// standard error evaluated where `at` says. Under
/// [`StandardErrorAt::Benchmark`] the bracket is evaluated at the deflation
/// benchmark `null_mean_sharpe + expected_max_sharpe(..)`, the value the
/// `null-se-psr` diagnostic reports as `psr_at_deflation_bar`.
pub fn deflated_sharpe_ratio_against_null_at(
    returns: &[f64],
    n_trials: u32,
    null_mean_sharpe: f64,
    trials_sr_std: f64,
    at: StandardErrorAt,
) -> Result<f64, StatisticalError> {
    deflated_sharpe_ratio_at_of(
        returns,
        n_trials,
        null_mean_sharpe,
        trials_sr_std,
        Track::Observed,
        at,
    )
}

/// The deflated Sharpe of an observed track or of a bootstrap resample, under
/// either evaluation. The observed evaluation is the kernel's own function.
pub(crate) fn deflated_sharpe_ratio_at_of(
    returns: &[f64],
    n_trials: u32,
    null_mean_sharpe: f64,
    trials_sr_std: f64,
    track: Track,
    at: StandardErrorAt,
) -> Result<f64, StatisticalError> {
    match at {
        StandardErrorAt::Observed => {
            deflated_sharpe_ratio_of(returns, n_trials, null_mean_sharpe, trials_sr_std, track)
        }
        StandardErrorAt::Benchmark => {
            finite_observations(returns)?;
            finite_parameter(null_mean_sharpe, "null_mean_sharpe")?;
            let sr_star = finite_computation(
                null_mean_sharpe + expected_max_sharpe(trials_sr_std, n_trials)?,
                "deflation benchmark",
            )?;
            null_se_psr(returns, sr_star, track)
        }
    }
}

/// The kernel's checked PSR computation with the bracket at `sr_benchmark`.
/// The Sharpe of a [`Track::Resample`] keeps the kernel's convention for a
/// resample that draws one repeated value: a Sharpe of 0.
fn null_se_psr(returns: &[f64], sr_benchmark: f64, track: Track) -> Result<f64, StatisticalError> {
    let n = returns.len();
    if n < 2 {
        return Ok(0.0);
    }
    let sr = match track {
        Track::Observed => observed_sharpe_ratio(returns)?,
        Track::Resample => {
            let center = finite_computation(mean(returns), "return mean")?;
            let scale = finite_computation(std_dev(returns), "return standard deviation")?;
            finite_computation(
                if scale == 0.0 { 0.0 } else { center / scale },
                "Sharpe ratio",
            )?
        }
    };
    let g3 = finite_computation(skewness(returns), "return skewness")?;
    let g4 = finite_computation(kurtosis(returns), "return kurtosis")?;
    let variance = psr_bracket(sr, sr_benchmark, g3, g4, StandardErrorAt::Benchmark)?;
    let denom = variance.max(1e-12).sqrt();
    let numerator = finite_computation(
        (sr - sr_benchmark) * (n as f64 - 1.0).sqrt(),
        "PSR numerator",
    )?;
    let z = finite_computation(numerator / denom, "PSR z statistic")?;
    finite_computation(norm_cdf(z), "PSR probability")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A resample that draws one repeated value keeps the kernel's Sharpe-of-0
    /// convention under the null evaluation, where an observed constant track
    /// is refused.
    #[test]
    fn a_constant_resample_scores_a_zero_sharpe_and_an_observed_one_is_refused() {
        let flat = [0.0; 20];
        let at = StandardErrorAt::Benchmark;
        assert!(null_se_psr(&flat, 0.0, Track::Observed).is_err());
        assert_eq!(null_se_psr(&flat, 0.0, Track::Resample), Ok(norm_cdf(0.0)));
        assert!(null_se_psr(&flat, 0.1, Track::Resample).unwrap() < norm_cdf(0.0));
        // A resample that varies has the observed track's Sharpe, bit for bit.
        let varied = [0.01, -0.02, 0.03, 0.005, -0.004, 0.02];
        for (null, sr_std) in [(0.0, 0.0), (0.002, 0.03)] {
            assert_eq!(
                deflated_sharpe_ratio_at_of(&varied, 20, null, sr_std, Track::Resample, at)
                    .map(f64::to_bits),
                deflated_sharpe_ratio_at_of(&varied, 20, null, sr_std, Track::Observed, at)
                    .map(f64::to_bits)
            );
        }
        // Two returns are enough for a statistic; one keeps the 0.0 convention.
        let two = [0.01, 0.03];
        let sr = 0.02 / (0.0002f64).sqrt();
        assert_eq!(
            checked_probabilistic_sharpe_ratio_at(&two, 0.0, at).map(f64::to_bits),
            Ok(norm_cdf(sr).to_bits())
        );
        assert_eq!(
            checked_probabilistic_sharpe_ratio_at(&[0.01], 0.0, at),
            Ok(0.0)
        );
        assert_eq!(
            deflated_sharpe_ratio_at_of(&flat, 1, 0.0, 0.0, Track::Resample, at),
            Ok(norm_cdf(0.0))
        );
    }
}
