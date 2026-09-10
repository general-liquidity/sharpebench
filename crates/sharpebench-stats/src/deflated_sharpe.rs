//! Sharpe ratio, Probabilistic Sharpe Ratio (PSR), and Deflated Sharpe Ratio (DSR).
//!
//! After Bailey & López de Prado, *The Deflated Sharpe Ratio* (2014). All ratios
//! are computed on **per-period** returns (do not pre-annualize — annualizing a
//! short track inflates the noise these statistics exist to expose).
//!
//! The PSR variance used here is the 2014 one: it corrects for skewness and
//! kurtosis but assumes serially independent returns. Autocorrelated returns
//! make it too small, and so the PSR and DSR point estimates too favorable.
//! López de Prado, Lipton and Zoonekynd, *How to Use the Sharpe Ratio* (2026,
//! eqs. 2, 3 and 5), give a generalized variance with a first-order
//! autocorrelation term that relaxes the assumption; it is not implemented.

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

/// Probabilistic Sharpe Ratio: one minus the one-sided p-value of the test of
/// `H0: SR <= sr_benchmark`, that is, the probability of observing a Sharpe
/// below the observed one if the true Sharpe were exactly `sr_benchmark`
/// (López de Prado, Lipton and Zoonekynd 2026, eq. 9). It corrects for track
/// length, skewness and kurtosis, assuming serially independent returns. It is
/// **not** the probability that the true Sharpe exceeds `sr_benchmark`: that is
/// a posterior, and it needs a prior this statistic does not have. Returns a
/// probability in [0, 1].
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

/// An annualized Sharpe-scale quantity (a Sharpe ratio, or the dispersion of
/// Sharpe ratios across trials) in the per-period unit every statistic in this
/// module is computed in: `annualized / sqrt(periods_per_year)`.
///
/// A Sharpe ratio scales with the square root of the number of periods, so a
/// dispersion of Sharpes does too. This is the one place the conversion is
/// written; `sharpebench_core::per_period_sr_std` and the `sharpebench-edge`
/// verdict both read it from here. It does not validate `periods_per_year`:
/// `+inf` would return a zero dispersion, the most favorable bar there is, so a
/// caller taking the frequency from a user checks it is finite and positive
/// first.
pub fn per_period_from_annualized(annualized: f64, periods_per_year: f64) -> f64 {
    annualized / periods_per_year.sqrt()
}

/// Expected maximum Sharpe ratio under `n_trials` independent strategy trials,
/// given the cross-trial dispersion of Sharpe ratios `trials_sr_std`
/// (Bailey & López de Prado, eq. for E[max SR_N]).
///
/// `trials_sr_std` is a **standard deviation**, `sqrt(V[{SR_n}])`, in the same
/// per-period units as the Sharpe it is compared with. The paper states the
/// variance: its worked example's `V[{SR_n}] = 1/2` annualized is a standard
/// deviation of `sqrt(0.5 / 250)` per period at 250 periods a year, which the
/// worked-example test below reproduces.
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
/// you'd see by chance across `n_trials` strategies, so one minus the p-value of
/// the test whose null is that the observed Sharpe is the best of `n_trials`
/// zero-skill trials. A value near 1.0 means a Sharpe this high would rarely be
/// observed under that null; near 0.0 means selection alone readily produces
/// it. Like the PSR it does not say how likely the strategy is to be skilled,
/// and it inherits the PSR's serial-independence assumption.
///
/// `trials_sr_std` is the per-period standard deviation of Sharpe ratios across
/// the trials/agents that were tested (the multiple-testing footprint). Larger
/// ⇒ harder to clear.
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

    /// The annualized prior 0.5 on daily bars is 0.5 / sqrt(252) per period,
    /// and on weekly bars 0.5 / sqrt(52): the literals are Python's
    /// `0.5 / math.sqrt(n)`, the same two correctly rounded IEEE operations.
    /// Dividing instead of multiplying, or dropping the root, lands orders of
    /// magnitude away.
    #[test]
    fn annualized_quantities_convert_by_the_root_of_the_frequency() {
        assert_eq!(
            per_period_from_annualized(0.5, 252.0),
            0.031_497_039_417_435_6
        );
        assert_eq!(
            per_period_from_annualized(0.5, 52.0),
            0.069_337_524_528_153_64
        );
        assert_eq!(per_period_from_annualized(0.5, 1.0), 0.5);
        assert_eq!(per_period_from_annualized(0.0, 252.0), 0.0);
    }

    /// The conversion documents a finite, positive frequency and leaves the
    /// check to its callers. Pin what it does at and past that boundary, so a
    /// caller that skips the check gets a known result: one period a year is
    /// the identity, an infinite frequency is a zero dispersion (the most
    /// favorable bar, which is why callers must refuse it), zero divides to
    /// infinity and a negative frequency is NaN.
    #[test]
    fn per_period_from_annualized_boundary_frequencies() {
        assert_eq!(per_period_from_annualized(0.5, 1.0), 0.5);
        assert_eq!(per_period_from_annualized(0.5, f64::INFINITY), 0.0);
        assert_eq!(per_period_from_annualized(0.5, 0.0), f64::INFINITY);
        assert!(per_period_from_annualized(0.5, -1.0).is_nan());
        assert!(per_period_from_annualized(0.5, f64::NAN).is_nan());
    }

    /// A series whose sample Sharpe is `sr`, built as `c + b * x` over the
    /// pattern `levels` of `(value, count)` pairs. Skewness and kurtosis are
    /// invariant under that positive affine map, so the pattern fixes them.
    fn series_with_sharpe(levels: &[(f64, usize)], sr: f64) -> Vec<f64> {
        let base: Vec<f64> = levels
            .iter()
            .flat_map(|&(v, k)| std::iter::repeat_n(v, k))
            .collect();
        let b = 0.01;
        let c = sr * b * std_dev(&base) - b * mean(&base);
        base.iter().map(|x| c + b * x).collect()
    }

    /// F11: Bailey and López de Prado (2014), "A numerical example", pp. 9-10 of
    /// the working paper: N = 100 trials, V[{SR_n}] = 1/2 annualized, T = 1250
    /// daily returns at 250 a year, skewness -3, kurtosis 10 and an annualized
    /// Sharpe of 2.5. The paper prints SR_0 ≈ 0.1132 per period and DSR ≈ 0.9004,
    /// then DSR = 0.9505 at N = 46, and DSR = 0.9505 at N = 88 had the returns
    /// been Normal. Each is reproduced through the public entry points to the
    /// printed four decimals.
    ///
    /// The returns are a constructed series, not the paper's. A two-point
    /// distribution has kurtosis exactly skewness² + 1, so 1145 high and 105 low
    /// values give skewness -2.9994 and kurtosis 9.9965; a symmetric three-point
    /// series with 208 / 834 / 208 gives skewness 0 and kurtosis 3.0048.
    #[test]
    fn reproduces_the_deflated_sharpe_worked_example() {
        let per_period = |annual: f64| annual / 250.0_f64.sqrt();
        // The paper's dispersion is a variance of 1/2, so a standard deviation
        // of sqrt(1/2) ≈ 0.707 annualized.
        let sigma = per_period(0.5_f64.sqrt());
        let sr = per_period(2.5);
        let near = |got: f64, want: f64| (got - want).abs() < 5e-5;

        let sr0 = expected_max_sharpe(sigma, 100).unwrap();
        assert!(near(sr0, 0.1132), "SR_0 {sr0}");
        // Reading 0.5 as a standard deviation lowers the bar by sqrt(2).
        let half = expected_max_sharpe(per_period(0.5), 100).unwrap();
        assert!(
            (sr0 / half - 2.0_f64.sqrt()).abs() < 1e-12,
            "{sr0} / {half}"
        );

        let skewed = series_with_sharpe(&[(1.0, 1145), (0.0, 105)], sr);
        assert_eq!(skewed.len(), 1250);
        assert!((sharpe_ratio(&skewed) - sr).abs() < 1e-12);
        assert!((skewness(&skewed) + 3.0).abs() < 1e-3);
        assert!((kurtosis(&skewed) - 10.0).abs() < 1e-2);
        let dsr = deflated_sharpe_ratio(&skewed, 100, sigma).unwrap();
        assert!(near(dsr, 0.9004), "DSR at N = 100: {dsr}");
        let dsr46 = deflated_sharpe_ratio(&skewed, 46, sigma).unwrap();
        assert!(near(dsr46, 0.9505), "DSR at N = 46: {dsr46}");

        let normal = series_with_sharpe(&[(-1.0, 208), (0.0, 834), (1.0, 208)], sr);
        assert_eq!(normal.len(), 1250);
        assert!(skewness(&normal).abs() < 1e-12);
        assert!((kurtosis(&normal) - 3.0).abs() < 1e-2);
        let dsr88 = deflated_sharpe_ratio(&normal, 88, sigma).unwrap();
        assert!(near(dsr88, 0.9505), "Normal DSR at N = 88: {dsr88}");
    }
}
