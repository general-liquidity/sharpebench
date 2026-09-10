//! Opt-in Sharpe diagnostics that the gate and the ranking do not read.
//!
//! The 2026-09-10 literature audit (`docs/audits/2026-09-09/LITERATURE-AUDIT.md`)
//! deferred three estimators because switching the gate to any of them would
//! move published values. They live here as separate functions so a caller can
//! ask for them; nothing in `sharpebench-core`'s scoring, eligibility or rank
//! predicate calls this module.
//!
//! - [`probabilistic_sharpe_ratio_autocorrelated`]: the PSR with the
//!   generalized Sharpe variance of López de Prado, Lipton and Zoonekynd,
//!   *How to Use the Sharpe Ratio* (ADIA Lab Research Paper Series No. 19,
//!   2026), eq. 2 (p. 9), which adds a first-order autocorrelation term to the
//!   skewness and kurtosis correction the kernel's PSR already carries.
//! - The same function with [`StandardErrorAt::Benchmark`]: the standard error
//!   evaluated under the null, at the benchmark Sharpe, as in that paper's
//!   eqs. 4 and 5 (p. 10), where the kernel evaluates it at the observed
//!   Sharpe (their eq. 3, p. 9, and Bailey and López de Prado 2012).
//! - [`manipulation_proof_performance`]: the manipulation-proof performance
//!   measure of Goetzmann, Ingersoll, Spiegel and Welch, *Portfolio
//!   Performance Manipulation and Manipulation-Proof Performance Measures*
//!   (RFS 20(5), 2007; working paper eq. 18).
//!
//! Every function refuses an input it cannot score with a typed
//! [`StatisticalError`], as the checked deflation family does: a non-finite
//! observation, a parameter outside its domain, or a computation that does not
//! stay finite is an error, never a substituted number.

use crate::stats::{kurtosis, mean, norm_cdf, skewness, std_dev};
use crate::validation::{
    finite_computation, finite_observations, finite_parameter, StatisticalError,
};

/// Where the standard error of the Sharpe estimator is evaluated when it is
/// turned into a PSR z statistic.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StandardErrorAt {
    /// At the observed Sharpe: López de Prado, Lipton and Zoonekynd (2026),
    /// eq. 3 (p. 9), `sigma^2[SR*] = V[SR^ | SR = SR*]`. This is the
    /// convention of Bailey and López de Prado (2012, 2014) and of the kernel's
    /// `probabilistic_sharpe_ratio` and `deflated_sharpe_ratio`.
    Observed,
    /// At the benchmark, the least favorable case of the null `H0: SR <= SR_0`:
    /// the same paper's eqs. 4 and 5 (p. 10), `z*[SR_0] = (SR* - SR_0) /
    /// sigma[SR_0]` with `sigma[SR_0] = sqrt(V[SR^ | SR = SR_0])`. Skewness,
    /// kurtosis and autocorrelation stay the sample estimates; only the Sharpe
    /// inside the variance moves to `SR_0`.
    Benchmark,
}

fn autocorrelation(rho: f64) -> Result<(), StatisticalError> {
    if !rho.is_finite() || rho <= -1.0 || rho >= 1.0 {
        return Err(StatisticalError::InvalidParameter {
            name: "rho",
            requirement: "must be finite and in (-1, 1)",
        });
    }
    Ok(())
}

fn positive_parameter(value: f64, name: &'static str) -> Result<(), StatisticalError> {
    if !value.is_finite() || value <= 0.0 {
        return Err(StatisticalError::InvalidParameter {
            name,
            requirement: "must be finite and positive",
        });
    }
    Ok(())
}

/// The bracket of López de Prado, Lipton and Zoonekynd (2026), eq. 2 (p. 9):
/// `T` times the asymptotic variance of the Sharpe estimator when returns
/// have skewness `skewness`, Pearson (non-excess) kurtosis `kurtosis` and
/// first-order autocorrelation `rho`,
///
/// ```text
/// (1+rho)/(1-rho) - (1+rho+rho^2)/(1-rho^2) * g3 * SR
///                 + (1+rho^2)/(1-rho^2) * (g4-1)/4 * SR^2
/// ```
///
/// The paper derives it in Appendix A.1 (eqs. 34 to 58, pp. 35 to 39) for a
/// stationary AR(1) series, `x_t = rho x_{t-1} + e_t` (eq. 44, p. 37), with
/// `rho = Cor[x_t, x_{t+1}]` (eq. 34, p. 35). At `rho = 0` the three weights
/// are exactly 1 and the bracket is the kernel's
/// `1 - g3 SR + (g4-1)/4 SR^2`, the i.i.d. non-Normal variance of Bailey and
/// López de Prado (2012).
///
/// `sr`, `skewness` and `kurtosis` must be finite and `rho` finite and in
/// (-1, 1). A negative bracket is refused, not floored: it is not reachable
/// from sample moments at `rho = 0` (the Pearson inequality
/// `g4 >= 1 + g3^2` keeps the quadratic non-negative), but a strongly negative
/// `rho` with skewed returns, or moments that violate that inequality, can
/// drive it below zero, and a floored variance there would turn a failed
/// approximation into a PSR of 0 or 1.
pub fn sharpe_variance_factor(
    sr: f64,
    skewness: f64,
    kurtosis: f64,
    rho: f64,
) -> Result<f64, StatisticalError> {
    finite_parameter(sr, "sr")?;
    finite_parameter(skewness, "skewness")?;
    finite_parameter(kurtosis, "kurtosis")?;
    autocorrelation(rho)?;
    let rho2 = rho * rho;
    let a = (1.0 + rho) / (1.0 - rho);
    let b = (1.0 + rho + rho2) / (1.0 - rho2);
    let c = (1.0 + rho2) / (1.0 - rho2);
    // Grouped so that, with a = b = c = 1 at rho = 0, every rounding step is
    // the kernel's `1.0 - g3 * sr + ((g4 - 1.0) / 4.0) * sr * sr`.
    let variance = finite_computation(
        a - b * (skewness * sr) + c * (((kurtosis - 1.0) / 4.0) * sr * sr),
        "Sharpe variance",
    )?;
    if variance < 0.0 {
        return Err(StatisticalError::InvalidParameter {
            name: "sharpe_variance",
            requirement: "must be non-negative: the moments and autocorrelation are inconsistent",
        });
    }
    Ok(variance)
}

/// The sample first-order autocorrelation of `returns`,
/// `sum_{t<T} (r_t - m)(r_{t+1} - m) / sum_t (r_t - m)^2`, the plug-in
/// estimate of `rho = Cor[x_t, x_{t+1}]` in López de Prado, Lipton and
/// Zoonekynd (2026), eq. 34 (p. 35). The paper re-estimates rho on each sample
/// (p. 13) and does not fix a finite-sample estimator; this is the standard
/// one, and it lies in [-1, 1] by the Cauchy-Schwarz inequality.
///
/// Every return must be finite. Fewer than two returns, or a constant series,
/// has no autocorrelation and is refused.
pub fn first_order_autocorrelation(returns: &[f64]) -> Result<f64, StatisticalError> {
    finite_observations(returns)?;
    if returns.len() < 2 {
        return Err(StatisticalError::InsufficientObservations {
            required: 2,
            actual: returns.len(),
        });
    }
    let m = finite_computation(mean(returns), "return mean")?;
    let squares: f64 = returns.iter().map(|x| (x - m) * (x - m)).sum();
    let squares = finite_computation(squares, "return sum of squares")?;
    if squares == 0.0 {
        return Err(StatisticalError::InvalidParameter {
            name: "returns",
            requirement: "must not be constant: a constant series has no autocorrelation",
        });
    }
    let cross: f64 = returns.windows(2).map(|w| (w[0] - m) * (w[1] - m)).sum();
    let cross = finite_computation(cross, "return lag-one cross product")?;
    finite_computation(cross / squares, "first-order autocorrelation")
}

/// The Probabilistic Sharpe Ratio with the autocorrelation-aware variance of
/// López de Prado, Lipton and Zoonekynd (2026), eq. 2 (p. 9), evaluated where
/// `at` says: at the observed Sharpe (eq. 3, p. 9) or at `sr_benchmark`, the
/// null (eqs. 4 and 5, p. 10). The result is `Z[z*]`, one minus the one-sided
/// p-value of `H0: SR <= sr_benchmark` (eq. 9, p. 11), not the probability that
/// the true Sharpe exceeds the benchmark.
///
/// `rho` is the first-order autocorrelation to assume: pass
/// [`first_order_autocorrelation`] of the same returns for the paper's plug-in
/// estimate, or `0.0` for serial independence. It must be finite and in
/// (-1, 1). `sr_benchmark` is per period, like every Sharpe here, and must be
/// finite. Every return must be finite and there must be at least two.
///
/// The z statistic scales by `sqrt(T - 1)`, the convention of Bailey and López
/// de Prado (2012) and of the kernel, where the 2026 paper writes `1/T` inside
/// the variance (eqs. 2, 3 and 5); the two differ by `sqrt(T / (T - 1))`. The
/// kernel's convention is kept so that `rho = 0.0` with
/// [`StandardErrorAt::Observed`] reproduces `probabilistic_sharpe_ratio` bit
/// for bit, including its `1e-12` variance floor, whenever that variance is
/// non-negative. This diagnostic is not read by the gate or the rank.
pub fn probabilistic_sharpe_ratio_autocorrelated(
    returns: &[f64],
    sr_benchmark: f64,
    rho: f64,
    at: StandardErrorAt,
) -> Result<f64, StatisticalError> {
    let inputs = checked_inputs(returns, sr_benchmark, rho, at)?;
    let denom = inputs.variance.max(1e-12).sqrt();
    let numerator = finite_computation(
        (inputs.sr - sr_benchmark) * (inputs.n as f64 - 1.0).sqrt(),
        "PSR numerator",
    )?;
    let z = finite_computation(numerator / denom, "PSR z statistic")?;
    finite_computation(norm_cdf(z), "PSR probability")
}

/// The standard error of the Sharpe estimator that
/// [`probabilistic_sharpe_ratio_autocorrelated`] divides by,
/// `sqrt(bracket / (T - 1))`, with the bracket of López de Prado, Lipton and
/// Zoonekynd (2026), eq. 2 (p. 9), evaluated at the observed Sharpe (eq. 3) or
/// at `sr_benchmark` (eq. 5, p. 10) as `at` says, and the same `1e-12` floor
/// on the bracket. The paper's `sigma[SR*]` and `sigma[SR_0]` use `T` in place
/// of `T - 1`; see the PSR's documentation for why the kernel's convention is
/// kept.
///
/// The domains are the PSR's: every return finite, at least two of them,
/// `sr_benchmark` finite, `rho` finite and in (-1, 1). With
/// [`StandardErrorAt::Observed`] `sr_benchmark` does not enter the value but
/// is still checked.
pub fn sharpe_standard_error_autocorrelated(
    returns: &[f64],
    sr_benchmark: f64,
    rho: f64,
    at: StandardErrorAt,
) -> Result<f64, StatisticalError> {
    let inputs = checked_inputs(returns, sr_benchmark, rho, at)?;
    finite_computation(
        inputs.variance.max(1e-12).sqrt() / (inputs.n as f64 - 1.0).sqrt(),
        "Sharpe standard error",
    )
}

struct SharpeInputs {
    n: usize,
    sr: f64,
    variance: f64,
}

fn checked_inputs(
    returns: &[f64],
    sr_benchmark: f64,
    rho: f64,
    at: StandardErrorAt,
) -> Result<SharpeInputs, StatisticalError> {
    finite_observations(returns)?;
    finite_parameter(sr_benchmark, "sr_benchmark")?;
    autocorrelation(rho)?;
    let n = returns.len();
    if n < 2 {
        return Err(StatisticalError::InsufficientObservations {
            required: 2,
            actual: n,
        });
    }
    let center = finite_computation(mean(returns), "return mean")?;
    let scale = finite_computation(std_dev(returns), "return standard deviation")?;
    let sr = finite_computation(
        if scale == 0.0 { 0.0 } else { center / scale },
        "Sharpe ratio",
    )?;
    let g3 = finite_computation(skewness(returns), "return skewness")?;
    let g4 = finite_computation(kurtosis(returns), "return kurtosis")?;
    let evaluated_at = match at {
        StandardErrorAt::Observed => sr,
        StandardErrorAt::Benchmark => sr_benchmark,
    };
    let variance = sharpe_variance_factor(evaluated_at, g3, g4, rho)?;
    Ok(SharpeInputs { n, sr, variance })
}

/// The relative risk aversion Goetzmann, Ingersoll, Spiegel and Welch use in
/// their simulations and recommend as consistent with the market portfolio
/// (working paper p. 18: "We have used a relative risk aversion of 3"; they
/// also report 2 and 4 as a sensitivity range).
pub const DEFAULT_MPPM_RISK_AVERSION: f64 = 3.0;

/// The manipulation-proof performance measure of Goetzmann, Ingersoll,
/// Spiegel and Welch (2007), working paper eq. (18), printed p. 18 (PDF p. 20),
/// also eq. (1), printed p. 2:
///
/// ```text
/// Theta = 1 / ((1 - rho) dt) * ln( (1/T) sum_t [ (1 + r_f)^-1 (1 + r_f + x_t) ]^(1 - rho) )
/// ```
///
/// with the per-period risk-free rate `r_f = 0`, the benchmark's zero-rate
/// cash convention, so each term is `(1 + x_t)^(1 - rho)` for the per-period
/// (not annualized) return `x_t`. `dt = 1 / periods_per_year` is the time
/// between observations in years, so the result is, in the paper's words, the
/// annualized continuously compounded excess-return certainty equivalent: a
/// riskless return of `exp(Theta dt) - 1` every period scores the same.
/// `periods_per_year = 1.0` gives the per-period value.
///
/// `risk_aversion` is the paper's `rho` (not an autocorrelation). It must be
/// finite and positive: the measure is manipulation-proof only when the
/// per-period function is strictly concave (p. 17), which needs `rho > 0`;
/// [`DEFAULT_MPPM_RISK_AVERSION`] is 3. At `rho = 1` the formula's limit is
/// the average log return, the geometric-average measure the paper lists among
/// those that cannot be dynamically manipulated (p. 17), and that limit is
/// computed directly. `periods_per_year` must be finite and positive. Every
/// return must be finite and above -1 (a gross return must be positive), and
/// there must be at least one.
///
/// The average is taken in log space with the largest term factored out, so a
/// large `rho` or a deep loss does not overflow the intermediate power. This
/// diagnostic is not read by the gate or the rank.
pub fn manipulation_proof_performance(
    returns: &[f64],
    risk_aversion: f64,
    periods_per_year: f64,
) -> Result<f64, StatisticalError> {
    finite_observations(returns)?;
    positive_parameter(risk_aversion, "risk_aversion")?;
    positive_parameter(periods_per_year, "periods_per_year")?;
    if returns.is_empty() {
        return Err(StatisticalError::InsufficientObservations {
            required: 1,
            actual: 0,
        });
    }
    if returns.iter().any(|&x| x <= -1.0) {
        return Err(StatisticalError::InvalidParameter {
            name: "returns",
            requirement: "must each exceed -1: a gross return must be positive",
        });
    }
    let n = returns.len() as f64;
    let dt = 1.0 / periods_per_year;
    let log_gross: Vec<f64> = returns.iter().map(|x| x.ln_1p()).collect();
    if risk_aversion == 1.0 {
        let mean_log = log_gross.iter().sum::<f64>() / n;
        return finite_computation(mean_log / dt, "manipulation-proof performance");
    }
    let k = 1.0 - risk_aversion;
    // ln((1/T) sum exp(k l_t)) = s + ln_1p((1/T) sum expm1(k l_t - s)), with
    // s = max_t k l_t, so no term overflows and none underflows to a zero sum.
    let scaled: Vec<f64> = log_gross.iter().map(|l| k * l).collect();
    let shift = scaled.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    let excess = scaled.iter().map(|y| (y - shift).exp_m1()).sum::<f64>() / n;
    let log_mean = finite_computation(shift + excess.ln_1p(), "MPPM log mean")?;
    // `+ 0.0` turns the -0.0 a flat track gives under rho > 1 into 0.0.
    finite_computation(log_mean / (k * dt) + 0.0, "manipulation-proof performance")
}
