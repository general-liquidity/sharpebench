//! Sharpe ratio, Probabilistic Sharpe Ratio (PSR), and Deflated Sharpe Ratio (DSR).
//!
//! After Bailey & López de Prado, *The Deflated Sharpe Ratio* (2014). All ratios
//! are computed on **per-period** returns (do not pre-annualize — annualizing a
//! short track inflates the noise these statistics exist to expose).

use crate::stats::{kurtosis, mean, norm_cdf, norm_ppf, skewness, std_dev};
use crate::validation::{
    dispersion, finite_computation, finite_observations, finite_parameter, StatisticalError,
};

/// Per-period Sharpe ratio (excess assumed; pass excess returns if you have a
/// non-zero risk-free rate). 0.0 if volatility is 0.
pub fn sharpe_ratio(returns: &[f64]) -> f64 {
    let s = std_dev(returns);
    if s == 0.0 {
        return 0.0;
    }
    mean(returns) / s
}

/// Probabilistic Sharpe Ratio: the probability that the *true* Sharpe exceeds
/// `sr_benchmark`, correcting for track length, skewness and kurtosis of the
/// return distribution. Returns a probability in [0, 1].
pub fn probabilistic_sharpe_ratio(returns: &[f64], sr_benchmark: f64) -> f64 {
    let n = returns.len();
    if n < 2 {
        return 0.0;
    }
    let sr = sharpe_ratio(returns);
    let g3 = skewness(returns);
    let g4 = kurtosis(returns); // non-excess kurtosis (normal = 3)
                                // Denominator of the PSR z-statistic; guarded so it never goes non-positive.
    let denom = (1.0 - g3 * sr + ((g4 - 1.0) / 4.0) * sr * sr)
        .max(1e-12)
        .sqrt();
    let z = (sr - sr_benchmark) * (n as f64 - 1.0).sqrt() / denom;
    norm_cdf(z)
}

/// Checked counterpart for Result-returning deflation. Validate before a
/// numerical floor or CDF saturation can conceal an overflowing computation.
/// The legacy scalar PSR above retains its API and operation order.
fn checked_psr(returns: &[f64], sr_benchmark: f64) -> Result<f64, StatisticalError> {
    let n = returns.len();
    if n < 2 {
        return Ok(0.0);
    }
    let center = finite_computation(mean(returns), "return mean")?;
    let scale = finite_computation(std_dev(returns), "return standard deviation")?;
    let sr = finite_computation(
        if scale == 0.0 { 0.0 } else { center / scale },
        "Sharpe ratio",
    )?;
    let g3 = finite_computation(skewness(returns), "return skewness")?;
    let g4 = finite_computation(kurtosis(returns), "return kurtosis")?;
    let variance =
        finite_computation(1.0 - g3 * sr + ((g4 - 1.0) / 4.0) * sr * sr, "PSR variance")?;
    let denom = variance.max(1e-12).sqrt();
    let numerator = finite_computation(
        (sr - sr_benchmark) * (n as f64 - 1.0).sqrt(),
        "PSR numerator",
    )?;
    let z = finite_computation(numerator / denom, "PSR z statistic")?;
    finite_computation(norm_cdf(z), "PSR probability")
}

/// Expected maximum Sharpe ratio under `n_trials` independent strategy trials,
/// given the cross-trial dispersion of Sharpe ratios `trials_sr_std`
/// (Bailey & López de Prado, eq. for E[max SR_N]).
///
/// `Ok(0.0)` when there is nothing to deflate for: a single trial, or a field
/// whose Sharpes do not disperse at all. A negative or non-finite
/// `trials_sr_std` is neither of those, so it is a typed error rather than the
/// same zero: coercing it would set the deflation bar to its most favorable
/// value and hand back a deflated Sharpe of 1.0 for a malformed footprint.
pub fn expected_max_sharpe(trials_sr_std: f64, n_trials: u32) -> Result<f64, StatisticalError> {
    dispersion(trials_sr_std, "trials_sr_std")?;
    let n = n_trials.max(1) as f64;
    if n <= 1.0 || trials_sr_std == 0.0 {
        return Ok(0.0);
    }
    const GAMMA: f64 = 0.577_215_664_901_532_9; // Euler–Mascheroni
    let e = std::f64::consts::E;
    let z1 = norm_ppf(1.0 - 1.0 / n);
    let z2 = norm_ppf(1.0 - 1.0 / (n * e));
    finite_computation(
        trials_sr_std * ((1.0 - GAMMA) * z1 + GAMMA * z2),
        "expected maximum Sharpe",
    )
}

/// Deflated Sharpe Ratio: the PSR computed against the *expected maximum* Sharpe
/// you'd see by chance across `n_trials` strategies. A value near 1.0 means the
/// observed Sharpe is very unlikely to be the product of selection over many
/// trials; near 0.0 means it is indistinguishable from luck.
///
/// `trials_sr_std` is the dispersion of Sharpe ratios across the trials/agents
/// that were tested (the multiple-testing footprint). Larger ⇒ harder to clear.
pub fn deflated_sharpe_ratio(
    returns: &[f64],
    n_trials: u32,
    trials_sr_std: f64,
) -> Result<f64, StatisticalError> {
    deflated_sharpe_ratio_against_null(returns, n_trials, 0.0, trials_sr_std)
}

/// Deflated Sharpe Ratio against an explicit per-period null population. Bailey
/// & López de Prado's expected maximum is `E[SR_null] + sigma_SR * k(N)`;
/// `null_mean_sharpe` is the first term and `expected_max_sharpe` supplies the
/// second. The zero-mean convenience wrapper above preserves the historic API.
pub fn deflated_sharpe_ratio_against_null(
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
    checked_psr(returns, sr_star)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A long, steady, low-vol track clears PSR vs 0 easily.
    #[test]
    fn psr_high_for_consistent_edge() {
        let r: Vec<f64> = (0..250)
            .map(|i| 0.001 + 0.0001 * ((i % 5) as f64 - 2.0))
            .collect();
        assert!(probabilistic_sharpe_ratio(&r, 0.0) > 0.99);
    }

    /// Deflating by many trials lowers the score: the same track is less
    /// convincing once you admit it was the best of many. Uses a *moderate*
    /// Sharpe (~0.3/period) so PSR is in its sensitive range, not saturated.
    #[test]
    fn deflation_penalizes_many_trials() {
        let r: Vec<f64> = (0..120)
            .map(|i| 0.02 + 0.1 * (i as f64 * 0.9).sin())
            .collect();
        let few = deflated_sharpe_ratio(&r, 2, 0.5).unwrap();
        let many = deflated_sharpe_ratio(&r, 500, 0.5).unwrap();
        assert!(
            many < few,
            "many-trial DSR {many} should be < few-trial DSR {few}"
        );
    }

    /// R02: a negative dispersion is a malformed footprint, not "no search".
    ///
    /// `expected_max_sharpe(-1.0, 500)` used to fall into the `<= 0.0` branch
    /// and return 0.0, so the deflation bar collapsed to the benchmark and a
    /// clean track came back with a deflated Sharpe of 1.0: the most favorable
    /// answer available, produced by the most obviously invalid input.
    #[test]
    fn a_negative_dispersion_is_refused_not_treated_as_no_search() {
        let expected = Err(StatisticalError::InvalidParameter {
            name: "trials_sr_std",
            requirement: "must be finite and non-negative",
        });
        for bad in [-1.0, -1e-12, f64::NAN, f64::INFINITY] {
            assert_eq!(expected_max_sharpe(bad, 500), expected, "std {bad}");
            assert_eq!(
                deflated_sharpe_ratio(&[0.01, 0.02, 0.03], 500, bad),
                expected
            );
        }
    }

    /// R02: a non-finite observation or null cannot produce a deflated Sharpe.
    #[test]
    fn non_finite_inputs_are_refused() {
        assert_eq!(
            deflated_sharpe_ratio(&[0.01, f64::NAN, 0.03], 10, 0.5),
            Err(StatisticalError::NonFiniteObservation { index: 1 })
        );
        assert_eq!(
            deflated_sharpe_ratio_against_null(&[0.01, 0.02], 10, f64::NAN, 0.5),
            Err(StatisticalError::InvalidParameter {
                name: "null_mean_sharpe",
                requirement: "must be finite",
            })
        );
    }

    /// R02 guard: the two branches that legitimately carry no deflation still
    /// return exactly 0.0, and a valid footprint returns the same number the
    /// closed form always gave.
    #[test]
    fn valid_deflation_inputs_return_the_same_numbers() {
        assert_eq!(expected_max_sharpe(0.0, 500), Ok(0.0));
        assert_eq!(expected_max_sharpe(0.5, 1), Ok(0.0));
        assert_eq!(expected_max_sharpe(0.5, 0), Ok(0.0));

        const GAMMA: f64 = 0.577_215_664_901_532_9;
        let n = 500.0_f64;
        let z1 = norm_ppf(1.0 - 1.0 / n);
        let z2 = norm_ppf(1.0 - 1.0 / (n * std::f64::consts::E));
        assert_eq!(
            expected_max_sharpe(0.5, 500),
            Ok(0.5 * ((1.0 - GAMMA) * z1 + GAMMA * z2))
        );

        let r: Vec<f64> = (0..120)
            .map(|i| 0.02 + 0.1 * (i as f64 * 0.9).sin())
            .collect();
        let sr_star = expected_max_sharpe(0.5, 500).unwrap();
        assert_eq!(
            deflated_sharpe_ratio(&r, 500, 0.5),
            Ok(probabilistic_sharpe_ratio(&r, sr_star))
        );
    }
}
