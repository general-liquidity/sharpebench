//! Small, dependency-free, deterministic statistics helpers.
//!
//! Everything here is plain `f64` with a fixed summation order so results are
//! reproducible across platforms. Approximations (erf, inverse-normal) are the
//! standard published closed forms and are unit-tested against known values.

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
/// is undefined).
pub fn sortino_ratio(xs: &[f64], target: f64) -> Option<f64> {
    let dd = downside_deviation(xs, target);
    if dd == 0.0 {
        return None;
    }
    Some((mean(xs) - target) / dd)
}

/// Population skewness (third standardized moment). 0.0 if undefined.
pub fn skewness(xs: &[f64]) -> f64 {
    let n = xs.len();
    if n < 3 {
        return 0.0;
    }
    let m = mean(xs);
    let s = std_dev(xs);
    if s == 0.0 {
        return 0.0;
    }
    let sum: f64 = xs.iter().map(|x| ((x - m) / s).powi(3)).sum();
    sum / n as f64
}

/// Population kurtosis (fourth standardized moment, **non-excess**; normal = 3.0).
pub fn kurtosis(xs: &[f64]) -> f64 {
    let n = xs.len();
    if n < 4 {
        return 3.0;
    }
    let m = mean(xs);
    let s = std_dev(xs);
    if s == 0.0 {
        return 3.0;
    }
    let sum: f64 = xs.iter().map(|x| ((x - m) / s).powi(4)).sum();
    sum / n as f64
}

/// Error function (Abramowitz & Stegun 7.1.26, max abs error ~1.5e-7).
pub fn erf(x: f64) -> f64 {
    let sign = if x < 0.0 { -1.0 } else { 1.0 };
    let x = x.abs();
    let t = 1.0 / (1.0 + 0.3275911 * x);
    let y = 1.0
        - (((((1.061405429 * t - 1.453152027) * t) + 1.421413741) * t - 0.284496736) * t
            + 0.254829592)
            * t
            * (-x * x).exp();
    sign * y
}

/// Standard normal CDF.
pub fn norm_cdf(x: f64) -> f64 {
    0.5 * (1.0 + erf(x / std::f64::consts::SQRT_2))
}

/// Inverse standard normal CDF (Acklam's rational approximation).
/// Returns ±∞ at the boundaries.
pub fn norm_ppf(p: f64) -> f64 {
    if p <= 0.0 {
        return f64::NEG_INFINITY;
    }
    if p >= 1.0 {
        return f64::INFINITY;
    }
    const A: [f64; 6] = [
        -3.969683028665376e+01,
        2.209460984245205e+02,
        -2.759285104469687e+02,
        1.38357751867269e+02,
        -3.066479806614716e+01,
        2.506628277459239e+00,
    ];
    const B: [f64; 5] = [
        -5.447609879822406e+01,
        1.615858368580409e+02,
        -1.556989798598866e+02,
        6.680131188771972e+01,
        -1.328068155288572e+01,
    ];
    const C: [f64; 6] = [
        -7.784894002430293e-03,
        -3.223964580411365e-01,
        -2.400758277161838e+00,
        -2.549732539343734e+00,
        4.374664141464968e+00,
        2.938163982698783e+00,
    ];
    const D: [f64; 4] = [
        7.784695709041462e-03,
        3.224671290700398e-01,
        2.445134137142996e+00,
        3.754408661907416e+00,
    ];
    const P_LOW: f64 = 0.02425;
    const P_HIGH: f64 = 1.0 - P_LOW;

    if p < P_LOW {
        let q = (-2.0 * p.ln()).sqrt();
        (((((C[0] * q + C[1]) * q + C[2]) * q + C[3]) * q + C[4]) * q + C[5])
            / ((((D[0] * q + D[1]) * q + D[2]) * q + D[3]) * q + 1.0)
    } else if p <= P_HIGH {
        let q = p - 0.5;
        let r = q * q;
        (((((A[0] * r + A[1]) * r + A[2]) * r + A[3]) * r + A[4]) * r + A[5]) * q
            / (((((B[0] * r + B[1]) * r + B[2]) * r + B[3]) * r + B[4]) * r + 1.0)
    } else {
        let q = (-2.0 * (1.0 - p).ln()).sqrt();
        -(((((C[0] * q + C[1]) * q + C[2]) * q + C[3]) * q + C[4]) * q + C[5])
            / ((((D[0] * q + D[1]) * q + D[2]) * q + D[3]) * q + 1.0)
    }
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

    #[test]
    fn ppf_is_inverse_of_cdf() {
        for &p in &[0.05, 0.25, 0.5, 0.75, 0.95] {
            let x = norm_ppf(p);
            assert!(approx(norm_cdf(x), p, 1e-4), "p={p}");
        }
    }
}
