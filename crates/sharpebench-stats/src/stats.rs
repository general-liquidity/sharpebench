//! Small, deterministic statistics helpers.
//!
//! Everything here is plain `f64` with a fixed summation order so results are
//! reproducible across platforms.
//!
//! [`erf`], [`norm_cdf`] and [`norm_ppf`] are thin wrappers over `statrs`
//! 0.19.1, which replaced the hand-rolled Abramowitz and Stegun 7.1.26 series
//! and Acklam rational approximation in the 2026-09-10 numerics migration. The
//! two implementations were measured against a 60-digit `mpmath` reference over
//! about 1.9 million grid points and over the arguments the kernel actually
//! passes; `statrs` is closer to the reference on essentially every one of
//! them, by three to seven orders of magnitude. The evidence and the artifact
//! impact are in `docs/audits/2026-09-09/NUMERICS-MIGRATION.md`.
//!
//! The wrappers preserve the previous total contract exactly: [`norm_ppf`]
//! returns negative infinity at or below zero, positive infinity at or above
//! one, and NaN for NaN, because `statrs`'s `Normal::inverse_cdf` panics on all
//! three. [`norm_cdf`] and [`norm_ppf`] use the `erfc` and `erfc_inv` forms
//! rather than `1 + erf`, matching what `Normal::cdf` and `Normal::inverse_cdf`
//! themselves compute bit for bit while avoiding the left-tail cancellation the
//! `erf` form suffers.
//!
//! `tests/special_function_bits.rs` pins the exact bits all three return. Those
//! pins were regenerated for this migration and belong to the release that
//! carries it; the pins v0.19.0 shipped are the pre-migration ones. Swapping
//! these bodies again is a golden-fixture regeneration, not a refactor.
//!
//! The moment estimators ([`mean`], [`variance`], [`std_dev`], [`skewness`],
//! [`kurtosis`]) stay hand-rolled on purpose and were deliberately left out of
//! the migration: the standardized moments use the population normalisation
//! (`m2 = sum((x - mean)^2) / n`) that the 2026-09-07 audit (R03) fixed, and a
//! general-purpose crate's skewness or kurtosis carries its own
//! bias-adjustment convention. Retaining them keeps the required normalization
//! explicit at the call site instead of resting on a dependency's choice.

use std::f64::consts::SQRT_2;

/// Arithmetic mean. Returns 0.0 for an empty slice.
pub fn mean(xs: &[f64]) -> f64 {
    if xs.is_empty() {
        return 0.0;
    }
    xs.iter().sum::<f64>() / xs.len() as f64
}

/// Sample variance (Bessel-corrected, `n - 1`). Returns 0.0 for fewer than 2 points.
pub fn variance(xs: &[f64]) -> f64 {
    let n = xs.len();
    if n < 2 {
        return 0.0;
    }
    let m = mean(xs);
    let ss: f64 = xs.iter().map(|x| (x - m) * (x - m)).sum();
    ss / (n as f64 - 1.0)
}

/// Sample standard deviation.
pub fn std_dev(xs: &[f64]) -> f64 {
    variance(xs).sqrt()
}

/// Downside deviation: the root-mean-square of shortfalls below `target` (the
/// minimum-acceptable return). Upside dispersion is ignored — only returns under
/// the target are penalized. The denominator is the full count `n` (the standard
/// "target downside deviation" convention), so a track with rare-but-deep losses
/// is not flattered by dividing through only its losing periods. 0.0 for fewer
/// than 2 points or no shortfall.
pub fn downside_deviation(xs: &[f64], target: f64) -> f64 {
    if xs.len() < 2 {
        return 0.0;
    }
    let ss: f64 = xs
        .iter()
        .map(|&x| {
            let d = (x - target).min(0.0);
            d * d
        })
        .sum();
    (ss / xs.len() as f64).sqrt()
}

/// Sortino ratio: excess mean return over `target` per unit of [`downside_deviation`].
/// Unlike the Sharpe, it does not punish upside volatility, so it rewards skill
/// that arrives without downside churn. `None` when there is no downside (the ratio
/// is undefined), and `None` when any return or the target is non-finite: a
/// shortfall against a value that is not a real number is not a measured risk.
pub fn sortino_ratio(xs: &[f64], target: f64) -> Option<f64> {
    if !target.is_finite() || crate::validation::finite_observations(xs).is_err() {
        return None;
    }
    let dd = downside_deviation(xs, target);
    if dd == 0.0 {
        return None;
    }
    Some((mean(xs) - target) / dd)
}

// Standardized empirical moments use m2 = sum((x - mean)^2) / n, not
// the n-1 sample variance used to estimate return volatility for the Sharpe.
fn population_std_dev(xs: &[f64], center: f64) -> f64 {
    (xs.iter().map(|x| (x - center).powi(2)).sum::<f64>() / xs.len() as f64).sqrt()
}

/// Empirical population skewness (third standardized moment, no bias adjustment).
/// Returns the conventional 0.0 fallback for fewer than 2 points or zero variance;
/// that fallback is not evidence that an unobserved distribution is symmetric.
pub fn skewness(xs: &[f64]) -> f64 {
    let n = xs.len();
    if n < 2 {
        return 0.0;
    }
    let m = mean(xs);
    let s = population_std_dev(xs, m);
    if s == 0.0 {
        return 0.0;
    }
    let sum: f64 = xs.iter().map(|x| ((x - m) / s).powi(3)).sum();
    sum / n as f64
}

/// Empirical population kurtosis (fourth standardized moment, **non-excess**).
/// No finite-sample bias adjustment. Normal = 3.0; that value is also the
/// conventional fallback for fewer than 2 points or zero variance, not an estimate
/// of an unobserved distribution's kurtosis.
pub fn kurtosis(xs: &[f64]) -> f64 {
    let n = xs.len();
    if n < 2 {
        return 3.0;
    }
    let m = mean(xs);
    let s = population_std_dev(xs, m);
    if s == 0.0 {
        return 3.0;
    }
    let sum: f64 = xs.iter().map(|x| ((x - m) / s).powi(4)).sum();
    sum / n as f64
}

/// Error function, `statrs::function::erf::erf`.
///
/// Total already: NaN maps to NaN and the infinities map to plus or minus one,
/// so no guard is needed. Maximum absolute error against a 60-digit reference
/// is about 4.9e-11, against about 1.4e-7 for the Abramowitz and Stegun 7.1.26
/// series this replaced.
pub fn erf(x: f64) -> f64 {
    statrs::function::erf::erf(x)
}

/// Standard normal CDF.
///
/// The complementary form `erfc(-x / sqrt 2) / 2` is what `statrs`'s
/// `Normal::cdf` computes, verified bit for bit on the measurement grid. It is
/// used in preference to `(1 + erf(x / sqrt 2)) / 2` because the latter loses
/// the left tail to cancellation.
pub fn norm_cdf(x: f64) -> f64 {
    if x.is_nan() {
        return f64::NAN;
    }
    0.5 * statrs::function::erf::erfc(-x / SQRT_2)
}

/// Inverse standard normal CDF. Returns plus or minus infinity at and beyond
/// the boundaries, and NaN for NaN.
///
/// The body is what `statrs`'s `Normal::inverse_cdf` computes for the standard
/// normal, verified bit for bit on the measurement grid, but reached through
/// `erfc_inv` directly: `Normal::inverse_cdf` panics on NaN and on any argument
/// outside `[0, 1]`, where this function has always returned NaN and the signed
/// infinities. Writing `0.0 -` rather than a leading minus reproduces the
/// positive zero callers have always seen at `p = 0.5`.
pub fn norm_ppf(p: f64) -> f64 {
    if p.is_nan() {
        return f64::NAN;
    }
    if p <= 0.0 {
        return f64::NEG_INFINITY;
    }
    if p >= 1.0 {
        return f64::INFINITY;
    }
    0.0 - SQRT_2 * statrs::function::erf::erfc_inv(2.0 * p)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn approx(a: f64, b: f64, eps: f64) -> bool {
        (a - b).abs() < eps
    }

    #[test]
    fn mean_and_std() {
        let xs = [1.0, 2.0, 3.0, 4.0, 5.0];
        assert!(approx(mean(&xs), 3.0, 1e-12));
        assert!(approx(std_dev(&xs), 1.5811388300841898, 1e-9));
    }

    #[test]
    fn standardized_moments_use_one_population_normalization() {
        // Exact central moments, not another implementation of the same loop:
        // [0,0,0,1]: m2=3/16, m3=3/32, m4=21/256.
        let asymmetric = [0.0, 0.0, 0.0, 1.0];
        assert!(approx(skewness(&asymmetric), 2.0 / 3.0_f64.sqrt(), 1e-12));
        assert!(approx(kurtosis(&asymmetric), 7.0 / 3.0, 1e-12));
        // [1,2,3,4]: m2=5/4 and m4=41/16, hence kurtosis=41/25.
        assert!(approx(kurtosis(&[1.0, 2.0, 3.0, 4.0]), 41.0 / 25.0, 1e-12));
        assert!(approx(skewness(&[1.0, 2.0, 3.0, 4.0]), 0.0, 1e-12));
    }

    #[test]
    fn standardized_moments_are_defined_for_small_nonconstant_samples() {
        assert!(approx(skewness(&[1.0, 2.0]), 0.0, 1e-12));
        assert!(approx(kurtosis(&[1.0, 2.0]), 1.0, 1e-12));
        assert!(approx(kurtosis(&[1.0, 2.0, 3.0]), 1.5, 1e-12));
        assert!(approx(
            skewness(&[0.0, 0.0, 1.0]),
            1.0 / 2.0_f64.sqrt(),
            1e-12
        ));
        // Affine transforms preserve kurtosis; reflection reverses skewness.
        assert!(approx(kurtosis(&[10.0, 10.0, 10.0, 8.0]), 7.0 / 3.0, 1e-12));
        assert!(approx(
            skewness(&[10.0, 10.0, 10.0, 8.0]),
            -2.0 / 3.0_f64.sqrt(),
            1e-12
        ));
    }

    #[test]
    fn downside_deviation_and_sortino() {
        // xs = [0.01, -0.02, 0.03, -0.04], target 0:
        //   shortfalls² = 0.02² + 0.04² = 0.0004 + 0.0016 = 0.002; /4 = 0.0005
        //   downside_deviation = sqrt(0.0005) = 0.0223607
        //   mean = -0.005 → sortino = -0.005 / 0.0223607 = -0.223607
        let xs = [0.01, -0.02, 0.03, -0.04];
        assert!(approx(
            downside_deviation(&xs, 0.0),
            0.0223606797749979,
            1e-12
        ));
        assert!(approx(
            sortino_ratio(&xs, 0.0).unwrap(),
            -0.2236067977,
            1e-9
        ));
    }

    #[test]
    fn sortino_is_none_without_downside() {
        // All returns at or above target → no shortfall → undefined ratio.
        assert_eq!(downside_deviation(&[0.01, 0.02, 0.03], 0.0), 0.0);
        assert!(sortino_ratio(&[0.01, 0.02, 0.03], 0.0).is_none());
    }

    #[test]
    fn sortino_ignores_upside_volatility() {
        // Two tracks, same downside, but the second has wild *upside* swings. The
        // Sortino is identical (upside is not punished); the Sharpe would differ.
        let calm = [0.01, -0.01, 0.01, -0.01];
        let spiky = [0.50, -0.01, 0.40, -0.01];
        assert!(approx(
            downside_deviation(&calm, 0.0),
            downside_deviation(&spiky, 0.0),
            1e-12
        ));
    }

    #[test]
    fn norm_cdf_known_values() {
        assert!(approx(norm_cdf(0.0), 0.5, 1e-6));
        assert!(approx(norm_cdf(1.96), 0.975, 1e-3));
        assert!(approx(norm_cdf(-1.96), 0.025, 1e-3));
    }

    // The three assertions below are the migration's regression: every one of
    // them fails against the Abramowitz-Stegun / Acklam bodies that shipped
    // through v0.19.0. The reference values are 60-digit `mpmath` evaluations
    // rounded to f64, recorded in `docs/audits/2026-09-09/NUMERICS-MIGRATION.md`.

    #[test]
    fn special_functions_are_accurate_to_near_machine_precision() {
        // A&S 7.1.26 gives erf(0) = 1e-9 and erf(0.5) to only 1.5e-7.
        assert_eq!(erf(0.0), 0.0);
        assert_eq!(norm_cdf(0.0), 0.5);
        assert!(approx(erf(0.5), 0.5204998778130465, 1e-9));
        assert!(approx(erf(1.0), 0.8427007929497149, 1e-9));
        assert!(approx(norm_cdf(1.96), 0.9750021048517796, 1e-9));
        assert!(approx(norm_cdf(-3.0), 0.001349898031630095, 1e-12));
        // Acklam is good to about 1.2e-9 relative; this is good to about 1e-15.
        assert!(approx(norm_ppf(0.975), 1.959963984540054, 1e-12));
        assert!(approx(norm_ppf(0.995), 2.5758293035489004, 1e-12));
        assert!(approx(norm_ppf(0.9), 1.2815515655446004, 1e-12));
    }

    #[test]
    fn special_functions_stay_total_at_the_boundaries() {
        // statrs's Normal::inverse_cdf panics on all four of these arguments.
        assert!(norm_ppf(f64::NAN).is_nan());
        assert_eq!(norm_ppf(-0.1), f64::NEG_INFINITY);
        assert_eq!(norm_ppf(1.1), f64::INFINITY);
        assert_eq!(norm_ppf(f64::INFINITY), f64::INFINITY);
        assert_eq!(norm_ppf(f64::NEG_INFINITY), f64::NEG_INFINITY);
        assert_eq!(norm_ppf(0.0), f64::NEG_INFINITY);
        assert_eq!(norm_ppf(1.0), f64::INFINITY);
        // p = 0.5 keeps the positive zero the pre-migration body returned.
        assert!(norm_ppf(0.5).is_sign_positive());
        assert_eq!(norm_ppf(0.5), 0.0);
        assert!(erf(f64::NAN).is_nan());
        assert!(norm_cdf(f64::NAN).is_nan());
        assert_eq!(erf(f64::INFINITY), 1.0);
        assert_eq!(erf(f64::NEG_INFINITY), -1.0);
        assert_eq!(norm_cdf(f64::INFINITY), 1.0);
        assert_eq!(norm_cdf(f64::NEG_INFINITY), 0.0);
    }

    #[test]
    fn ppf_is_inverse_of_cdf() {
        for &p in &[0.05, 0.25, 0.5, 0.75, 0.95] {
            let x = norm_ppf(p);
            assert!(approx(norm_cdf(x), p, 1e-4), "p={p}");
        }
    }
}
