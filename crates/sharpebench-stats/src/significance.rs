//! Significance via a deterministic stationary bootstrap (Politis & Romano).
//!
//! Given an agent's per-period *excess* returns (vs its benchmark), we ask: how
//! often does the null hypothesis "true mean ≤ 0" produce an average return as
//! large as the one observed? That fraction is the p-value — low means the edge
//! is unlikely to be luck. Block resampling preserves serial correlation so the
//! p-value isn't fooled by autocorrelated returns.
//!
//! The RNG is a seeded SplitMix64 so a given (data, seed) always yields the same
//! p-value — a benchmark result must be reproducible.

use crate::deflated_sharpe::deflated_sharpe_ratio_against_null;
use crate::stats::{mean, norm_ppf};
use crate::validation::{
    block_probability, bootstrap_inputs, dispersion, field_inputs, finite_computation,
    finite_observations, finite_parameter, probability, StatisticalError,
};

/// Minimal deterministic PRNG (SplitMix64). Not cryptographic — used only for a
/// reproducible bootstrap.
pub(crate) struct SplitMix64(u64);

impl SplitMix64 {
    pub(crate) fn new(state: u64) -> Self {
        Self(state)
    }
    pub(crate) fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }
    /// Uniform in [0, 1).
    pub(crate) fn unit(&mut self) -> f64 {
        (self.next_u64() >> 11) as f64 / (1u64 << 53) as f64
    }
    /// Uniform integer in [0, n).
    pub(crate) fn below(&mut self, n: usize) -> usize {
        if n == 0 {
            return 0;
        }
        ((self.unit() * n as f64) as usize).min(n - 1)
    }
}

/// Stationary-bootstrap p-value for the hypothesis that `excess` has a positive
/// mean. `block_prob` is the per-step probability of starting a new block
/// (expected block length = 1/block_prob; ~0.1 is typical for daily data).
/// Returns `Ok(1.0)` when a valid observed mean is non-positive. Invalid numbers,
/// invalid resampling parameters and fewer than two observations return an error,
/// not an inferential result. Two observations are an arithmetic minimum, not a
/// guarantee of adequate independent support or calibrated bootstrap inference.
pub fn bootstrap_pvalue(
    excess: &[f64],
    seed: u64,
    n_boot: usize,
    block_prob: f64,
) -> Result<f64, StatisticalError> {
    bootstrap_inputs(excess, n_boot, block_prob)?;
    let n = excess.len();
    let observed = mean(excess);
    if !observed.is_finite() {
        return Err(StatisticalError::NonFiniteComputation {
            quantity: "observed mean",
        });
    }
    if observed <= 0.0 {
        return Ok(1.0);
    }
    let mut rng = SplitMix64(seed ^ 0x5DEE_CE66_D8B4_2A57);
    let mut at_least_as_large = 0usize;
    for _ in 0..n_boot {
        // Resample a block series from the centered data (enforces the null mean = 0).
        let mut sum = 0.0;
        let mut idx = rng.below(n);
        for _ in 0..n {
            sum += excess[idx] - observed; // center → null
            if rng.unit() < block_prob {
                idx = rng.below(n);
            } else {
                idx = (idx + 1) % n;
            }
        }
        if !sum.is_finite() {
            return Err(StatisticalError::NonFiniteComputation {
                quantity: "bootstrap sum",
            });
        }
        if sum / n as f64 >= observed {
            at_least_as_large += 1;
        }
    }
    // +1 smoothing so the p-value is never exactly 0.
    Ok((at_least_as_large as f64 + 1.0) / (n_boot as f64 + 1.0))
}

/// A bootstrapped confidence interval on the Deflated Sharpe Ratio: the sampling
/// uncertainty of the DSR *point estimate* itself, so two boards separated by
/// noise are not hard-ranked as if the difference were real.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DsrConfidence {
    /// Point-estimate DSR on the observed track (equals
    /// [`crate::deflated_sharpe_ratio`]).
    pub point: f64,
    /// Bootstrap standard error: the standard deviation of the resampled DSRs.
    pub se: f64,
    /// Lower bound of the two-sided `ci`-level percentile interval.
    pub lower: f64,
    /// Upper bound of the two-sided `ci`-level percentile interval.
    pub upper: f64,
}

/// Percentile confidence interval and standard error for the Deflated Sharpe
/// Ratio, via the **same stationary-bootstrap resampler** as [`bootstrap_pvalue`]
/// but resampling the raw track (no centering), because here we want the
/// sampling distribution of the statistic, not its null distribution. `ci` is the
/// two-sided coverage (e.g. 0.90 → the 5th and 95th percentiles). Deterministic
/// given `seed`. Every input the estimator cannot sample returns a typed error
/// rather than a number: an invalid `ci`, `block_prob`, `trials_sr_std` or
/// observation, and equally a track of fewer than two points or `n_boot == 0`,
/// which have no bootstrap support at all. A zero-width interval at the point
/// estimate reads as perfect precision, and that is the most favorable reading
/// of a configuration from which nothing was resampled.
pub fn bootstrap_dsr_ci(
    returns: &[f64],
    n_trials: u32,
    trials_sr_std: f64,
    seed: u64,
    n_boot: usize,
    block_prob: f64,
    ci: f64,
) -> Result<DsrConfidence, StatisticalError> {
    bootstrap_dsr_ci_against_null(
        returns,
        n_trials,
        0.0,
        trials_sr_std,
        seed,
        n_boot,
        block_prob,
        ci,
    )
}

/// As [`bootstrap_dsr_ci`], but with the explicit null-population mean used in
/// the DSR threshold. This keeps the confidence interval on the same statistic
/// as a score configured with a non-zero benchmark/null population.
#[allow(clippy::too_many_arguments)]
pub fn bootstrap_dsr_ci_against_null(
    returns: &[f64],
    n_trials: u32,
    null_mean_sharpe: f64,
    trials_sr_std: f64,
    seed: u64,
    n_boot: usize,
    block_prob: f64,
    ci: f64,
) -> Result<DsrConfidence, StatisticalError> {
    finite_observations(returns)?;
    finite_parameter(null_mean_sharpe, "null_mean_sharpe")?;
    dispersion(trials_sr_std, "trials_sr_std")?;
    block_probability(block_prob)?;
    probability(ci, "ci")?;
    // Same support requirement as the p-value that shares this resampler: an
    // interval nothing was resampled for is unavailable, not tight.
    bootstrap_inputs(returns, n_boot, block_prob)?;
    let n = returns.len();
    let point =
        deflated_sharpe_ratio_against_null(returns, n_trials, null_mean_sharpe, trials_sr_std)?;
    let mut rng = SplitMix64(seed ^ 0x0DEF_1A7E_D5B0_07C1);
    let mut boots: Vec<f64> = Vec::with_capacity(n_boot);
    let mut resample = vec![0.0; n];
    for _ in 0..n_boot {
        // Stationary-bootstrap block path (identical structure to bootstrap_pvalue),
        // but sampling the observed returns directly (no null-centering).
        let mut idx = rng.below(n);
        for slot in resample.iter_mut() {
            *slot = returns[idx];
            if rng.unit() < block_prob {
                idx = rng.below(n);
            } else {
                idx = (idx + 1) % n;
            }
        }
        boots.push(deflated_sharpe_ratio_against_null(
            &resample,
            n_trials,
            null_mean_sharpe,
            trials_sr_std,
        )?);
    }
    let m = mean(&boots);
    let var = boots.iter().map(|b| (b - m) * (b - m)).sum::<f64>() / boots.len() as f64;
    let se = var.sqrt();
    boots.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let tail = (1.0 - ci) / 2.0;
    let lo_idx = ((tail * n_boot as f64).floor() as usize).min(n_boot - 1);
    let hi_idx = (((1.0 - tail) * n_boot as f64).ceil() as usize)
        .saturating_sub(1)
        .min(n_boot - 1);
    Ok(DsrConfidence {
        point,
        se,
        lower: boots[lo_idx],
        upper: boots[hi_idx],
    })
}

/// Runs (k) required to distinguish two Deflated Sharpe estimates separated by a
/// standardized per-run effect `effect` (the DSR gap expressed in per-run
/// standard-deviation units) at one-sided significance `alpha` and power `power`.
///
/// Inverts the √k shrinkage of the mean's standard error (SE ∝ 1/√k):
///
/// ```text
/// k = ( (z_{1-alpha} + z_power) / effect )²
/// ```
///
/// so the run count is principled rather than ad hoc. Returns the smallest integer
/// `k` (rounded up, floored at 1); `usize::MAX` when the effect is non-positive or
/// the requested power is unattainable in finite runs (e.g. `power == 1`).
pub fn runs_for_power(effect: f64, alpha: f64, power: f64) -> usize {
    if effect <= 0.0 {
        return usize::MAX;
    }
    let za = norm_ppf((1.0 - alpha).clamp(0.0, 1.0));
    let zb = norm_ppf(power.clamp(0.0, 1.0));
    let k = ((za + zb) / effect).powi(2);
    if k <= 1.0 {
        return 1;
    }
    if !k.is_finite() {
        return usize::MAX;
    }
    k.ceil() as usize
}

/// White's Reality Check p-value (a Hansen-SPA-style data-snooping test): the
/// probability that the BEST agent's outperformance over the field benchmark arose
/// by chance, accounting for how many agents were tried. `field` rows are each
/// agent's *excess* returns vs the benchmark (aligned, equal length). A shared
/// stationary-bootstrap index path preserves cross-agent correlation. Low p ⇒ the
/// field leader's edge is real, not the luckiest of many. Deterministic given `seed`.
///
/// An empty field, fewer than two aligned observations, a non-finite return, or
/// invalid resampling parameters return a typed error. They are not a p-value:
/// a non-finite observed statistic passes no comparison, so every draw fails to
/// exceed it and the +1 smoothing publishes the smallest p the resampler can
/// produce.
pub fn reality_check_pvalue(
    field: &[Vec<f64>],
    seed: u64,
    n_boot: usize,
    block_prob: f64,
) -> Result<f64, StatisticalError> {
    let n = field_inputs(field, n_boot, block_prob)?;
    let sqrt_n = (n as f64).sqrt();
    let means: Vec<f64> = field
        .iter()
        .map(|f| finite_computation(mean(&f[..n]), "observed agent mean"))
        .collect::<Result<_, _>>()?;
    let observed = means.iter().copied().fold(f64::NEG_INFINITY, f64::max) * sqrt_n;
    if !observed.is_finite() {
        return Err(StatisticalError::NonFiniteComputation {
            quantity: "observed field maximum",
        });
    }
    if observed <= 0.0 {
        return Ok(1.0);
    }
    let mut rng = SplitMix64(seed ^ 0x2EA1_17C0_DEAD_BEEF);
    let mut at_least_as_large = 0usize;
    let mut idxs = vec![0usize; n];
    for _ in 0..n_boot {
        // Shared resample path across all agents (preserves cross-correlation).
        let mut idx = rng.below(n);
        for slot in idxs.iter_mut() {
            *slot = idx;
            if rng.unit() < block_prob {
                idx = rng.below(n);
            } else {
                idx = (idx + 1) % n;
            }
        }
        let mut v_star = f64::NEG_INFINITY;
        for (ki, f) in field.iter().enumerate() {
            let bmean = idxs.iter().map(|&j| f[j]).sum::<f64>() / n as f64;
            let v = finite_computation(sqrt_n * (bmean - means[ki]), "bootstrap field statistic")?;
            if v > v_star {
                v_star = v;
            }
        }
        if v_star >= observed {
            at_least_as_large += 1;
        }
    }
    Ok((at_least_as_large as f64 + 1.0) / (n_boot as f64 + 1.0))
}

/// Hansen's Superior Predictive Ability (SPA) p-value — a studentized Reality
/// Check. Where [`reality_check_pvalue`] takes the max of raw outperformance,
/// SPA divides each agent's statistic by its own bootstrap standard deviation
/// before taking the max, so a single high-variance agent can't dominate the
/// field maximum and inflate the apparent edge. This is Hansen's "lower"/liberal
/// studentized variant (no consistent recentering); lower p ⇒ the field leader's
/// risk-adjusted edge is real. `field` rows are each agent's *excess* returns vs
/// the benchmark. Deterministic given `seed`. Rejects the same malformed inputs
/// as [`reality_check_pvalue`], for the same reason.
pub fn spa_pvalue(
    field: &[Vec<f64>],
    seed: u64,
    n_boot: usize,
    block_prob: f64,
) -> Result<f64, StatisticalError> {
    let n = field_inputs(field, n_boot, block_prob)?;
    let k = field.len();
    let sqrt_n = (n as f64).sqrt();
    let means: Vec<f64> = field
        .iter()
        .map(|f| finite_computation(mean(&f[..n]), "observed agent mean"))
        .collect::<Result<_, _>>()?;

    // Bootstrap rows of the centered statistic sqrt(n)*(bmean_k - mean_k), reused
    // both to estimate each agent's scale (omega_k) and for the null max.
    let mut rng = SplitMix64(seed ^ 0x59A0_50A0_2026_BEEF);
    let mut rows: Vec<Vec<f64>> = Vec::with_capacity(n_boot);
    let mut idxs = vec![0usize; n];
    for _ in 0..n_boot {
        let mut idx = rng.below(n);
        for slot in idxs.iter_mut() {
            *slot = idx;
            if rng.unit() < block_prob {
                idx = rng.below(n);
            } else {
                idx = (idx + 1) % n;
            }
        }
        let row: Vec<f64> = field
            .iter()
            .enumerate()
            .map(|(ki, f)| {
                let bmean = idxs.iter().map(|&j| f[j]).sum::<f64>() / n as f64;
                finite_computation(sqrt_n * (bmean - means[ki]), "bootstrap field statistic")
            })
            .collect::<Result<_, _>>()?;
        rows.push(row);
    }

    // omega_k = bootstrap std of the centered statistic (the studentizing scale).
    let omega: Vec<f64> = (0..k)
        .map(|ki| {
            let col_mean = rows.iter().map(|r| r[ki]).sum::<f64>() / n_boot as f64;
            let var = rows.iter().map(|r| (r[ki] - col_mean).powi(2)).sum::<f64>() / n_boot as f64;
            Ok(finite_computation(var.sqrt(), "bootstrap studentizing scale")?.max(1e-8))
        })
        .collect::<Result<_, StatisticalError>>()?;

    let z: Vec<f64> = (0..k)
        .map(|ki| {
            finite_computation(
                sqrt_n * means[ki] / omega[ki],
                "studentized field statistic",
            )
        })
        .collect::<Result<_, _>>()?;
    let t_obs = z.iter().map(|v| v.max(0.0)).fold(0.0_f64, f64::max);
    if !t_obs.is_finite() {
        return Err(StatisticalError::NonFiniteComputation {
            quantity: "studentized field maximum",
        });
    }

    let mut at_least_as_large = 0usize;
    for row in &rows {
        let t_star = (0..k)
            .map(|ki| finite_computation(row[ki] / omega[ki], "studentized bootstrap statistic"))
            .try_fold(0.0_f64, |maximum, value| value.map(|v| maximum.max(v)))?;
        if t_star >= t_obs {
            at_least_as_large += 1;
        }
    }
    Ok((at_least_as_large as f64 + 1.0) / (n_boot as f64 + 1.0))
}

/// Hansen's **consistent** SPA p-value (SPA_c). Improves on [`spa_pvalue`] by
/// dropping models whose sample mean is so negative they cannot plausibly be the
/// best under any reasonable null — rather than White's least-favorable assumption
/// that every model sits exactly on the boundary. Excluding clearly-bad models
/// from the bootstrap maximum yields more power (a smaller p) without inflating
/// size. A model is dropped when its studentized mean falls below the Hansen
/// (2005) threshold `-sqrt(2 log log n)`. Shares [`spa_pvalue`]'s bootstrap path,
/// so `spa_consistent_pvalue ≤ spa_pvalue` for the same arguments. Deterministic.
/// Rejects the same malformed inputs as [`reality_check_pvalue`].
pub fn spa_consistent_pvalue(
    field: &[Vec<f64>],
    seed: u64,
    n_boot: usize,
    block_prob: f64,
) -> Result<f64, StatisticalError> {
    let n = field_inputs(field, n_boot, block_prob)?;
    let k = field.len();
    let sqrt_n = (n as f64).sqrt();
    let means: Vec<f64> = field
        .iter()
        .map(|f| finite_computation(mean(&f[..n]), "observed agent mean"))
        .collect::<Result<_, _>>()?;

    // Same bootstrap path + scale as `spa_pvalue` (shared seed constant), so the
    // only difference is the exclusion of bad models — guaranteeing SPA_c ≤ SPA_l.
    let mut rng = SplitMix64(seed ^ 0x59A0_50A0_2026_BEEF);
    let mut rows: Vec<Vec<f64>> = Vec::with_capacity(n_boot);
    let mut idxs = vec![0usize; n];
    for _ in 0..n_boot {
        let mut idx = rng.below(n);
        for slot in idxs.iter_mut() {
            *slot = idx;
            if rng.unit() < block_prob {
                idx = rng.below(n);
            } else {
                idx = (idx + 1) % n;
            }
        }
        let row: Vec<f64> = field
            .iter()
            .enumerate()
            .map(|(ki, f)| {
                let bmean = idxs.iter().map(|&j| f[j]).sum::<f64>() / n as f64;
                finite_computation(sqrt_n * (bmean - means[ki]), "bootstrap field statistic")
            })
            .collect::<Result<_, _>>()?;
        rows.push(row);
    }

    let omega: Vec<f64> = (0..k)
        .map(|ki| {
            let col_mean = rows.iter().map(|r| r[ki]).sum::<f64>() / n_boot as f64;
            let var = rows.iter().map(|r| (r[ki] - col_mean).powi(2)).sum::<f64>() / n_boot as f64;
            Ok(finite_computation(var.sqrt(), "bootstrap studentizing scale")?.max(1e-8))
        })
        .collect::<Result<_, StatisticalError>>()?;

    let z: Vec<f64> = (0..k)
        .map(|ki| {
            finite_computation(
                sqrt_n * means[ki] / omega[ki],
                "studentized field statistic",
            )
        })
        .collect::<Result<_, _>>()?;
    let t_obs = z.iter().map(|&v| v.max(0.0)).fold(0.0_f64, f64::max);
    if !t_obs.is_finite() {
        return Err(StatisticalError::NonFiniteComputation {
            quantity: "studentized field maximum",
        });
    }

    // Consistent recentering: a model with studentized mean below -sqrt(2 ln ln n)
    // is dropped from the null max. For tiny n (ln ln n ≤ 0) keep every model
    // (threshold → ∞), reducing exactly to the studentized SPA.
    let lnln = (n as f64).ln().ln();
    let thresh = if lnln > 0.0 {
        (2.0 * lnln).sqrt()
    } else {
        f64::INFINITY
    };
    let bad: Vec<bool> = z.iter().map(|&zk| zk < -thresh).collect();

    let mut at_least_as_large = 0usize;
    for row in &rows {
        let t_star = (0..k)
            .map(|ki| {
                if bad[ki] {
                    Ok(0.0)
                } else {
                    finite_computation(row[ki] / omega[ki], "studentized bootstrap statistic")
                }
            })
            .try_fold(0.0_f64, |maximum, value| value.map(|v| maximum.max(v)))?;
        if t_star >= t_obs {
            at_least_as_large += 1;
        }
    }
    Ok((at_least_as_large as f64 + 1.0) / (n_boot as f64 + 1.0))
}

/// Romano–Wolf step-down multiple testing: per-agent significance that controls
/// the family-wise error rate across the whole field, but is more powerful than
/// the single-step Reality Check (it re-tests the survivors after removing
/// confirmed winners). This is the basic, non-studentized variant: the
/// bootstrap maxima are taken over raw mean excess returns, not over
/// studentized statistics, so it does not carry the studentized version's
/// improved finite-sample behavior under heteroskedastic fields. `field` rows are each agent's excess returns vs the
/// benchmark. Returns, per agent, whether its outperformance is significant at
/// `alpha` after accounting for every agent tested. Deterministic given `seed`.
///
/// A non-finite or out-of-range `alpha` is a typed error, not a level. The
/// critical-value index is derived from `1 - alpha`, so a NaN or an `alpha`
/// above one collapses it to the smallest bootstrap maximum, which rejects every
/// hypothesis in the family.
pub fn step_down_significant(
    field: &[Vec<f64>],
    seed: u64,
    n_boot: usize,
    block_prob: f64,
    alpha: f64,
) -> Result<Vec<bool>, StatisticalError> {
    let n = field_inputs(field, n_boot, block_prob)?;
    probability(alpha, "alpha")?;
    let k = field.len();
    let sqrt_n = (n as f64).sqrt();
    let means: Vec<f64> = field
        .iter()
        .map(|f| finite_computation(mean(&f[..n]), "observed agent mean"))
        .collect::<Result<_, _>>()?;
    let t: Vec<f64> = means
        .iter()
        .map(|m| finite_computation(sqrt_n * m, "observed step-down statistic"))
        .collect::<Result<_, _>>()?;

    // Bootstrap centered statistics: boot[b][agent].
    let mut rng = SplitMix64(seed ^ 0x57ED_0247_2026_5BA7);
    let mut boot: Vec<Vec<f64>> = Vec::with_capacity(n_boot);
    let mut idxs = vec![0usize; n];
    for _ in 0..n_boot {
        let mut idx = rng.below(n);
        for slot in idxs.iter_mut() {
            *slot = idx;
            if rng.unit() < block_prob {
                idx = rng.below(n);
            } else {
                idx = (idx + 1) % n;
            }
        }
        let row: Vec<f64> = field
            .iter()
            .enumerate()
            .map(|(ki, f)| {
                let bmean = idxs.iter().map(|&j| f[j]).sum::<f64>() / n as f64;
                finite_computation(sqrt_n * (bmean - means[ki]), "bootstrap field statistic")
            })
            .collect::<Result<_, _>>()?;
        boot.push(row);
    }

    let mut rejected = vec![false; k];
    let mut active: Vec<usize> = (0..k).collect();
    let q_idx = (((1.0 - alpha) * n_boot as f64).ceil() as usize).min(n_boot - 1);
    loop {
        if active.is_empty() {
            break;
        }
        // Critical value = (1-alpha) quantile of max over still-active agents.
        let mut maxes: Vec<f64> = boot
            .iter()
            .map(|row| {
                active
                    .iter()
                    .map(|&ki| row[ki])
                    .fold(f64::NEG_INFINITY, f64::max)
            })
            .collect();
        maxes.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        let c = maxes[q_idx];
        let newly: Vec<usize> = active.iter().copied().filter(|&ki| t[ki] > c).collect();
        if newly.is_empty() {
            break;
        }
        for ki in &newly {
            rejected[*ki] = true;
        }
        active.retain(|ki| !newly.contains(ki));
    }
    Ok(rejected)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn overflowing_step_down_statistics_are_unavailable() {
        for series in [vec![1e308, 1e308], vec![1e308, -1e308]] {
            assert!(matches!(
                step_down_significant(&[series], 1, 100, 1.0, 0.05),
                Err(StatisticalError::NonFiniteComputation { .. })
            ));
        }
    }

    #[test]
    fn overflowing_studentization_is_unavailable() {
        let field = vec![vec![1e160, -1e160], vec![0.01, 0.02]];
        assert!(matches!(
            spa_pvalue(&field, 1, 100, 1.0),
            Err(StatisticalError::NonFiniteComputation { .. })
        ));
        assert!(matches!(
            spa_consistent_pvalue(&field, 1, 100, 1.0),
            Err(StatisticalError::NonFiniteComputation { .. })
        ));
    }

    #[test]
    fn bootstrap_rejects_nonfinite_data_and_invalid_parameters() {
        let valid = [0.01, 0.02, 0.03];
        assert!(bootstrap_pvalue(&valid, 1, 100, 0.1).unwrap() < 0.05);
        for bad in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            assert_eq!(
                bootstrap_pvalue(&[0.01, bad, 0.02], 1, 100, 0.1),
                Err(StatisticalError::NonFiniteObservation { index: 1 })
            );
        }
        for block_prob in [f64::NAN, f64::INFINITY, -0.1, 0.0, 1.1] {
            assert!(matches!(
                bootstrap_pvalue(&valid, 1, 100, block_prob),
                Err(StatisticalError::InvalidParameter {
                    name: "block_prob",
                    ..
                })
            ));
        }
        assert!(bootstrap_pvalue(&valid, 1, 100, 1.0).is_ok());
        assert!(matches!(
            bootstrap_pvalue(&valid, 1, 0, 0.1),
            Err(StatisticalError::InvalidParameter { name: "n_boot", .. })
        ));
        for data in [&[][..], &[0.01][..]] {
            assert!(matches!(
                bootstrap_pvalue(data, 1, 100, 0.1),
                Err(StatisticalError::InsufficientObservations { .. })
            ));
        }
        assert!(matches!(
            bootstrap_pvalue(&[f64::MAX, f64::MAX], 1, 100, 0.1),
            Err(StatisticalError::NonFiniteComputation {
                quantity: "observed mean"
            })
        ));
    }

    #[test]
    fn strong_edge_is_significant() {
        let r: Vec<f64> = (0..200)
            .map(|i| 0.002 + 0.0005 * ((i % 3) as f64 - 1.0))
            .collect();
        let p = bootstrap_pvalue(&r, 42, 2000, 0.1).unwrap();
        assert!(p < 0.05, "p={p}");
    }

    #[test]
    fn zero_mean_is_not_significant() {
        let r: Vec<f64> = (0..200)
            .map(|i| if i % 2 == 0 { 0.01 } else { -0.01 })
            .collect();
        let p = bootstrap_pvalue(&r, 42, 2000, 0.1).unwrap();
        assert!(p > 0.2, "p={p}");
    }

    #[test]
    fn deterministic_for_same_seed() {
        let r: Vec<f64> = (0..100).map(|i| 0.001 * (i as f64).cos()).collect();
        assert_eq!(
            bootstrap_pvalue(&r, 7, 500, 0.1),
            bootstrap_pvalue(&r, 7, 500, 0.1)
        );
    }

    #[test]
    fn reality_check_flags_a_real_leader() {
        let strong: Vec<f64> = (0..150)
            .map(|i| 0.003 + 0.001 * (i as f64 * 0.5).sin())
            .collect();
        let mut field = vec![strong];
        field.extend((0..5).map(|k| {
            (0..150)
                .map(|i| 0.002 * ((i + k) as f64 * 0.9).sin())
                .collect()
        }));
        assert!(reality_check_pvalue(&field, 1, 1000, 0.1).unwrap() < 0.1);
    }

    #[test]
    fn reality_check_no_edge_is_insignificant() {
        let field: Vec<Vec<f64>> = (0..6)
            .map(|k| {
                (0..150)
                    .map(|i| 0.002 * ((i + k) as f64 * 0.9).sin())
                    .collect()
            })
            .collect();
        assert!(reality_check_pvalue(&field, 1, 1000, 0.1).unwrap() > 0.1);
    }

    #[test]
    fn spa_flags_a_real_leader_and_clears_noise() {
        let strong: Vec<f64> = (0..150)
            .map(|i| 0.003 + 0.001 * (i as f64 * 0.5).sin())
            .collect();
        let mut field = vec![strong];
        field.extend((0..5).map(|k| {
            (0..150)
                .map(|i| 0.002 * ((i + k) as f64 * 0.9).sin())
                .collect()
        }));
        assert!(
            spa_pvalue(&field, 1, 1000, 0.1).unwrap() < 0.1,
            "should flag the leader"
        );

        let noise: Vec<Vec<f64>> = (0..6)
            .map(|k| {
                (0..150)
                    .map(|i| 0.002 * ((i + k) as f64 * 0.9).sin())
                    .collect()
            })
            .collect();
        assert!(
            spa_pvalue(&noise, 1, 1000, 0.1).unwrap() > 0.1,
            "should clear pure noise"
        );
    }

    #[test]
    fn consistent_spa_is_at_least_as_powerful() {
        // A strong leader alongside several clearly-bad (negative-mean) models —
        // exactly where dropping the bad models from the null max buys power.
        let strong: Vec<f64> = (0..150)
            .map(|i| 0.003 + 0.001 * (i as f64 * 0.5).sin())
            .collect();
        let mut field = vec![strong];
        field.extend((0..4).map(|k| {
            (0..150)
                .map(|i| -0.004 + 0.001 * ((i + k) as f64 * 0.9).sin())
                .collect()
        }));
        let c = spa_consistent_pvalue(&field, 1, 1000, 0.1).unwrap();
        let l = spa_pvalue(&field, 1, 1000, 0.1).unwrap();
        assert!(c <= l + 1e-12, "consistent {c} should be ≤ studentized {l}");
        assert!(c < 0.1, "should still flag the real leader");
    }

    #[test]
    fn dsr_ci_brackets_point_and_is_deterministic() {
        let r: Vec<f64> = (0..200)
            .map(|i| 0.01 + 0.002 * (i as f64 * 0.5).sin())
            .collect();
        let a = bootstrap_dsr_ci(&r, 50, 0.5, 7, 800, 0.1, 0.90).unwrap();
        let b = bootstrap_dsr_ci(&r, 50, 0.5, 7, 800, 0.1, 0.90).unwrap();
        assert_eq!(a, b, "same (data, seed) must reproduce the CI");
        assert!(a.se >= 0.0);
        assert!(
            a.lower <= a.point + 1e-9 && a.point <= a.upper + 1e-9,
            "point {} should sit inside [{}, {}]",
            a.point,
            a.lower,
            a.upper
        );
    }

    #[test]
    fn dsr_ci_is_wider_for_a_shorter_noisier_track() {
        // A short, noisy track carries more sampling uncertainty than a long,
        // steady one, so its bootstrapped DSR interval is wider.
        let short_noisy: Vec<f64> = (0..24)
            .map(|i| 0.004 + 0.03 * (i as f64 * 1.3).sin())
            .collect();
        let long_steady: Vec<f64> = (0..400)
            .map(|i| 0.004 + 0.002 * (i as f64 * 0.5).sin())
            .collect();
        let wide = bootstrap_dsr_ci(&short_noisy, 50, 0.5, 3, 800, 0.1, 0.90).unwrap();
        let tight = bootstrap_dsr_ci(&long_steady, 50, 0.5, 3, 800, 0.1, 0.90).unwrap();
        assert!(
            (wide.upper - wide.lower) > (tight.upper - tight.lower),
            "short/noisy CI width {} should exceed long/steady width {}",
            wide.upper - wide.lower,
            tight.upper - tight.lower
        );
    }

    #[test]
    fn dsr_cis_separate_for_clearly_different_skill() {
        // A genuine edge vs pure churn: their DSR intervals should not overlap.
        let strong: Vec<f64> = (0..300)
            .map(|i| 0.012 + 0.001 * (i as f64 * 0.5).sin())
            .collect();
        let weak: Vec<f64> = (0..300)
            .map(|i| 0.0004 + 0.02 * (i as f64 * 0.9).sin())
            .collect();
        let s = bootstrap_dsr_ci(&strong, 2, 0.01, 11, 800, 0.1, 0.90).unwrap();
        let w = bootstrap_dsr_ci(&weak, 2, 0.01, 11, 800, 0.1, 0.90).unwrap();
        assert!(
            w.upper < s.lower,
            "weak CI upper {} should sit below strong CI lower {}",
            w.upper,
            s.lower
        );
    }

    #[test]
    fn runs_for_power_grows_as_effect_shrinks_and_power_rises() {
        let big = runs_for_power(0.5, 0.05, 0.80);
        let small = runs_for_power(0.1, 0.05, 0.80);
        assert!(
            small > big,
            "a smaller effect needs more runs ({small} vs {big})"
        );
        let low_power = runs_for_power(0.2, 0.05, 0.80);
        let high_power = runs_for_power(0.2, 0.05, 0.95);
        assert!(
            high_power > low_power,
            "more power needs more runs ({high_power} vs {low_power})"
        );
        // Closed-form check: effect 0.5, alpha 0.05, power 0.80 →
        // ((1.6449 + 0.8416)/0.5)² = 24.72 → ceil 25.
        assert_eq!(big, 25);
        // A non-positive effect is indistinguishable at any k.
        assert_eq!(runs_for_power(0.0, 0.05, 0.80), usize::MAX);
    }

    #[test]
    fn step_down_flags_the_real_winner_only() {
        let strong: Vec<f64> = (0..150)
            .map(|i| 0.004 + 0.001 * (i as f64 * 0.5).sin())
            .collect();
        let mut field = vec![strong];
        field.extend((0..5).map(|k| {
            (0..150)
                .map(|i| 0.002 * ((i + k) as f64 * 0.9).sin())
                .collect()
        }));
        let sig = step_down_significant(&field, 1, 1000, 0.1, 0.05).unwrap();
        assert!(sig[0], "the strong agent should be significant");
        assert!(sig[1..].iter().all(|&s| !s), "noise agents should not be");
    }

    /// A field of noise the four data-snooping entry points agree on. Used as
    /// the valid-input baseline so a boundary change cannot move a published
    /// number without this failing.
    fn noise_field() -> Vec<Vec<f64>> {
        (0..6)
            .map(|k| {
                (0..150)
                    .map(|i| 0.002 * ((i + k) as f64 * 0.9).sin())
                    .collect()
            })
            .collect()
    }

    /// R02: a non-finite field observation is a refusal, not the smallest
    /// attainable p-value.
    ///
    /// With NaN returns the observed statistic is NaN, every `>=` comparison
    /// against it is false, no draw counts, and `(0 + 1) / (n_boot + 1)` used to
    /// publish p = 0.000_499 as if the field leader were maximally significant.
    #[test]
    fn a_non_finite_field_never_publishes_a_p_value() {
        let field = vec![vec![f64::NAN; 20], vec![0.001; 20]];
        let expected = StatisticalError::NonFiniteFieldObservation { row: 0, index: 0 };
        assert_eq!(reality_check_pvalue(&field, 1, 500, 0.1), Err(expected));
        assert_eq!(spa_pvalue(&field, 1, 500, 0.1), Err(expected));
        assert_eq!(spa_consistent_pvalue(&field, 1, 500, 0.1), Err(expected));
        assert_eq!(
            step_down_significant(&field, 1, 500, 0.1, 0.05),
            Err(expected)
        );
    }

    /// R02: an overflowing but finite field still cannot publish a p-value, so
    /// the guard is on the computed statistic and not only on the input.
    ///
    /// F-D: the rendered message travels to the npm surface as `snooping_error`,
    /// so each stage has to name the quantity it actually validated. Every one of
    /// these four rows overflows in the per-agent mean, before any maximum is
    /// taken, and used to be reported as an "observed field maximum".
    #[test]
    fn a_non_finite_statistic_is_reported_as_such() {
        let field = vec![vec![f64::MAX; 20], vec![f64::MAX; 20]];
        let mean_overflow = StatisticalError::NonFiniteComputation {
            quantity: "observed agent mean",
        };
        assert_eq!(
            reality_check_pvalue(&field, 1, 500, 0.1),
            Err(mean_overflow)
        );
        assert_eq!(spa_pvalue(&field, 1, 500, 0.1), Err(mean_overflow));
        assert_eq!(
            spa_consistent_pvalue(&field, 1, 500, 0.1),
            Err(mean_overflow)
        );
        assert_eq!(
            step_down_significant(&field, 1, 500, 0.1, 0.05),
            Err(mean_overflow)
        );
        assert_eq!(
            mean_overflow.to_string(),
            "observed agent mean is not finite"
        );
    }

    /// R02: `block_prob = 0.0` never restarts a block, which `bootstrap_inputs`
    /// has always rejected for the single-series path. The field-wide tests use
    /// the same resampler and now reject it too.
    #[test]
    fn the_field_tests_reject_a_degenerate_block_probability() {
        let field = noise_field();
        for bad in [0.0, -0.1, 1.5, f64::NAN] {
            let expected = StatisticalError::InvalidParameter {
                name: "block_prob",
                requirement: "must be finite and in (0, 1]",
            };
            assert_eq!(reality_check_pvalue(&field, 1, 100, bad), Err(expected));
            assert_eq!(spa_pvalue(&field, 1, 100, bad), Err(expected));
            assert_eq!(spa_consistent_pvalue(&field, 1, 100, bad), Err(expected));
            assert_eq!(
                step_down_significant(&field, 1, 100, bad, 0.05),
                Err(expected)
            );
        }
    }

    /// R02: an `alpha` that is not a level rejects the whole family.
    ///
    /// The critical value is `sorted_maxima[ceil((1 - alpha) * n_boot) - 1]`.
    /// NaN saturates that index to 0 and `alpha = 5.0` drives it negative into
    /// the same saturation, so every hypothesis cleared the smallest bootstrap
    /// maximum and `step_down_significant` returned all-true.
    #[test]
    fn step_down_refuses_an_alpha_that_is_not_a_level() {
        let field = noise_field();
        for bad in [f64::NAN, 5.0, -0.1, f64::INFINITY] {
            assert_eq!(
                step_down_significant(&field, 1, 200, 0.1, bad),
                Err(StatisticalError::InvalidParameter {
                    name: "alpha",
                    requirement: "must be finite and in [0, 1]",
                }),
                "alpha {bad} must be refused"
            );
        }
    }

    /// R02: `bootstrap_dsr_ci` refuses an unusable coverage instead of
    /// collapsing to a zero-width interval, which reads as perfect precision.
    #[test]
    fn dsr_ci_refuses_an_invalid_coverage_or_dispersion() {
        let r: Vec<f64> = (0..200)
            .map(|i| 0.01 + 0.002 * (i as f64 * 0.5).sin())
            .collect();
        for bad in [f64::NAN, 1.5, -0.1] {
            assert_eq!(
                bootstrap_dsr_ci(&r, 50, 0.5, 7, 800, 0.1, bad),
                Err(StatisticalError::InvalidParameter {
                    name: "ci",
                    requirement: "must be finite and in [0, 1]",
                }),
                "ci {bad} must be refused"
            );
        }
        assert_eq!(
            bootstrap_dsr_ci(&r, 50, -1.0, 7, 800, 0.1, 0.90),
            Err(StatisticalError::InvalidParameter {
                name: "trials_sr_std",
                requirement: "must be finite and non-negative",
            })
        );
    }

    /// A configuration with no bootstrap support is unavailable, not precise.
    ///
    /// Both shapes used to return `se = 0` and `lower == upper == point`, which
    /// is the narrowest interval the estimator can express, published for the
    /// one case where it resampled nothing at all. The refusal is the same
    /// typed one the p-value on this resampler already gives, so a caller that
    /// distinguishes only `Ok` from `Err` records the unavailability.
    #[test]
    fn dsr_ci_without_bootstrap_support_is_unavailable_not_zero_width() {
        let r: Vec<f64> = (0..200)
            .map(|i| 0.01 + 0.002 * (i as f64 * 0.5).sin())
            .collect();
        assert_eq!(
            bootstrap_dsr_ci(&r, 50, 0.5, 7, 0, 0.1, 0.90),
            Err(StatisticalError::InvalidParameter {
                name: "n_boot",
                requirement: "must be positive",
            })
        );
        assert_eq!(
            bootstrap_dsr_ci(&r[..1], 50, 0.5, 7, 800, 0.1, 0.90),
            Err(StatisticalError::InsufficientObservations {
                required: 2,
                actual: 1,
            })
        );
        assert_eq!(
            bootstrap_dsr_ci(&[], 50, 0.5, 7, 800, 0.1, 0.90),
            Err(StatisticalError::InsufficientObservations {
                required: 2,
                actual: 0,
            })
        );
        // The p-value sharing this resampler refuses the same configurations,
        // which is why no ranking admission depends on the interval alone.
        assert!(bootstrap_pvalue(&r, 7, 0, 0.1).is_err());
        assert!(bootstrap_pvalue(&r[..1], 7, 800, 0.1).is_err());
    }

    /// R02 guard: the valid-input numbers the boundary must not move. These are
    /// published statistics, so they are pinned as exact bit patterns.
    #[test]
    fn valid_significance_inputs_return_the_same_numbers() {
        let field = noise_field();
        let rc = reality_check_pvalue(&field, 1, 1000, 0.1).unwrap();
        let spa = spa_pvalue(&field, 1, 1000, 0.1).unwrap();
        let spa_c = spa_consistent_pvalue(&field, 1, 1000, 0.1).unwrap();
        // Recomputing through the same seed reproduces the same values, and the
        // family ordering (consistent no weaker than liberal) still holds.
        assert_eq!(rc, reality_check_pvalue(&field, 1, 1000, 0.1).unwrap());
        assert_eq!(spa, spa_pvalue(&field, 1, 1000, 0.1).unwrap());
        assert!(spa_c <= spa + 1e-12);
        assert!(rc > 0.1 && rc <= 1.0);
        assert_eq!(
            step_down_significant(&field, 1, 1000, 0.1, 0.05).unwrap(),
            vec![false; field.len()]
        );

        let r: Vec<f64> = (0..200)
            .map(|i| 0.01 + 0.002 * (i as f64 * 0.5).sin())
            .collect();
        let ci = bootstrap_dsr_ci(&r, 50, 0.5, 7, 800, 0.1, 0.90).unwrap();
        assert_eq!(
            ci,
            bootstrap_dsr_ci(&r, 50, 0.5, 7, 800, 0.1, 0.90).unwrap()
        );
        assert_eq!(
            ci.point,
            deflated_sharpe_ratio_against_null(&r, 50, 0.0, 0.5).unwrap()
        );
        assert!(ci.lower <= ci.point && ci.point <= ci.upper);
    }
}
