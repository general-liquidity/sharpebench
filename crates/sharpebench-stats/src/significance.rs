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

use crate::deflated_sharpe::deflated_sharpe_ratio;
use crate::stats::{mean, norm_ppf};

/// Minimal deterministic PRNG (SplitMix64). Not cryptographic — used only for a
/// reproducible bootstrap.
struct SplitMix64(u64);

impl SplitMix64 {
    fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }
    /// Uniform in [0, 1).
    fn unit(&mut self) -> f64 {
        (self.next_u64() >> 11) as f64 / (1u64 << 53) as f64
    }
    /// Uniform integer in [0, n).
    fn below(&mut self, n: usize) -> usize {
        if n == 0 {
            return 0;
        }
        ((self.unit() * n as f64) as usize).min(n - 1)
    }
}

/// Stationary-bootstrap p-value for the hypothesis that `excess` has a positive
/// mean. `block_prob` is the per-step probability of starting a new block
/// (expected block length = 1/block_prob; ~0.1 is typical for daily data).
/// Returns 1.0 (no evidence) when the observed mean is non-positive.
pub fn bootstrap_pvalue(excess: &[f64], seed: u64, n_boot: usize, block_prob: f64) -> f64 {
    let n = excess.len();
    if n == 0 || n_boot == 0 {
        return 1.0;
    }
    let observed = mean(excess);
    if observed <= 0.0 {
        return 1.0;
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
        if sum / n as f64 >= observed {
            at_least_as_large += 1;
        }
    }
    // +1 smoothing so the p-value is never exactly 0.
    (at_least_as_large as f64 + 1.0) / (n_boot as f64 + 1.0)
}

/// A bootstrapped confidence interval on the Deflated Sharpe Ratio: the sampling
/// uncertainty of the DSR *point estimate* itself, so two boards separated by
/// noise are not hard-ranked as if the difference were real.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DsrConfidence {
    /// Point-estimate DSR on the observed track (equals [`deflated_sharpe_ratio`]).
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
/// given `seed`. A degenerate track (< 2 points, or `n_boot == 0`) returns a
/// zero-width interval at the point estimate.
pub fn bootstrap_dsr_ci(
    returns: &[f64],
    n_trials: u32,
    trials_sr_std: f64,
    seed: u64,
    n_boot: usize,
    block_prob: f64,
    ci: f64,
) -> DsrConfidence {
    let n = returns.len();
    let point = deflated_sharpe_ratio(returns, n_trials, trials_sr_std);
    if n < 2 || n_boot == 0 {
        return DsrConfidence {
            point,
            se: 0.0,
            lower: point,
            upper: point,
        };
    }
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
        boots.push(deflated_sharpe_ratio(&resample, n_trials, trials_sr_std));
    }
    let m = mean(&boots);
    let var = boots.iter().map(|b| (b - m) * (b - m)).sum::<f64>() / boots.len() as f64;
    let se = var.sqrt();
    boots.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let ci = ci.clamp(0.0, 1.0);
    let tail = (1.0 - ci) / 2.0;
    let lo_idx = ((tail * n_boot as f64).floor() as usize).min(n_boot - 1);
    let hi_idx = (((1.0 - tail) * n_boot as f64).ceil() as usize)
        .saturating_sub(1)
        .min(n_boot - 1);
    DsrConfidence {
        point,
        se,
        lower: boots[lo_idx],
        upper: boots[hi_idx],
    }
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
pub fn reality_check_pvalue(field: &[Vec<f64>], seed: u64, n_boot: usize, block_prob: f64) -> f64 {
    if field.is_empty() || n_boot == 0 {
        return 1.0;
    }
    let n = field.iter().map(Vec::len).min().unwrap_or(0);
    if n < 2 {
        return 1.0;
    }
    let sqrt_n = (n as f64).sqrt();
    let means: Vec<f64> = field.iter().map(|f| mean(&f[..n])).collect();
    let observed = means.iter().copied().fold(f64::NEG_INFINITY, f64::max) * sqrt_n;
    if observed <= 0.0 {
        return 1.0;
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
            let v = sqrt_n * (bmean - means[ki]); // centered under the null
            if v > v_star {
                v_star = v;
            }
        }
        if v_star >= observed {
            at_least_as_large += 1;
        }
    }
    (at_least_as_large as f64 + 1.0) / (n_boot as f64 + 1.0)
}

/// Hansen's Superior Predictive Ability (SPA) p-value — a studentized Reality
/// Check. Where [`reality_check_pvalue`] takes the max of raw outperformance,
/// SPA divides each agent's statistic by its own bootstrap standard deviation
/// before taking the max, so a single high-variance agent can't dominate the
/// field maximum and inflate the apparent edge. This is Hansen's "lower"/liberal
/// studentized variant (no consistent recentering); lower p ⇒ the field leader's
/// risk-adjusted edge is real. `field` rows are each agent's *excess* returns vs
/// the benchmark. Deterministic given `seed`.
pub fn spa_pvalue(field: &[Vec<f64>], seed: u64, n_boot: usize, block_prob: f64) -> f64 {
    let k = field.len();
    if k == 0 || n_boot == 0 {
        return 1.0;
    }
    let n = field.iter().map(Vec::len).min().unwrap_or(0);
    if n < 2 {
        return 1.0;
    }
    let sqrt_n = (n as f64).sqrt();
    let means: Vec<f64> = field.iter().map(|f| mean(&f[..n])).collect();

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
                sqrt_n * (bmean - means[ki])
            })
            .collect();
        rows.push(row);
    }

    // omega_k = bootstrap std of the centered statistic (the studentizing scale).
    let omega: Vec<f64> = (0..k)
        .map(|ki| {
            let col_mean = rows.iter().map(|r| r[ki]).sum::<f64>() / n_boot as f64;
            let var = rows.iter().map(|r| (r[ki] - col_mean).powi(2)).sum::<f64>() / n_boot as f64;
            var.sqrt().max(1e-8)
        })
        .collect();

    let t_obs = (0..k)
        .map(|ki| (sqrt_n * means[ki] / omega[ki]).max(0.0))
        .fold(0.0_f64, f64::max);

    let mut at_least_as_large = 0usize;
    for row in &rows {
        let t_star = (0..k)
            .map(|ki| (row[ki] / omega[ki]).max(0.0))
            .fold(0.0_f64, f64::max);
        if t_star >= t_obs {
            at_least_as_large += 1;
        }
    }
    (at_least_as_large as f64 + 1.0) / (n_boot as f64 + 1.0)
}

/// Hansen's **consistent** SPA p-value (SPA_c). Improves on [`spa_pvalue`] by
/// dropping models whose sample mean is so negative they cannot plausibly be the
/// best under any reasonable null — rather than White's least-favorable assumption
/// that every model sits exactly on the boundary. Excluding clearly-bad models
/// from the bootstrap maximum yields more power (a smaller p) without inflating
/// size. A model is dropped when its studentized mean falls below the Hansen
/// (2005) threshold `-sqrt(2 log log n)`. Shares [`spa_pvalue`]'s bootstrap path,
/// so `spa_consistent_pvalue ≤ spa_pvalue` for the same arguments. Deterministic.
pub fn spa_consistent_pvalue(field: &[Vec<f64>], seed: u64, n_boot: usize, block_prob: f64) -> f64 {
    let k = field.len();
    if k == 0 || n_boot == 0 {
        return 1.0;
    }
    let n = field.iter().map(Vec::len).min().unwrap_or(0);
    if n < 2 {
        return 1.0;
    }
    let sqrt_n = (n as f64).sqrt();
    let means: Vec<f64> = field.iter().map(|f| mean(&f[..n])).collect();

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
                sqrt_n * (bmean - means[ki])
            })
            .collect();
        rows.push(row);
    }

    let omega: Vec<f64> = (0..k)
        .map(|ki| {
            let col_mean = rows.iter().map(|r| r[ki]).sum::<f64>() / n_boot as f64;
            let var = rows.iter().map(|r| (r[ki] - col_mean).powi(2)).sum::<f64>() / n_boot as f64;
            var.sqrt().max(1e-8)
        })
        .collect();

    let z: Vec<f64> = (0..k).map(|ki| sqrt_n * means[ki] / omega[ki]).collect();
    let t_obs = z.iter().map(|&v| v.max(0.0)).fold(0.0_f64, f64::max);

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
                    0.0
                } else {
                    (row[ki] / omega[ki]).max(0.0)
                }
            })
            .fold(0.0_f64, f64::max);
        if t_star >= t_obs {
            at_least_as_large += 1;
        }
    }
    (at_least_as_large as f64 + 1.0) / (n_boot as f64 + 1.0)
}

/// Romano–Wolf step-down multiple testing: per-agent significance that controls
/// the family-wise error rate across the whole field, but is more powerful than
/// the single-step Reality Check (it re-tests the survivors after removing
/// confirmed winners). `field` rows are each agent's excess returns vs the
/// benchmark. Returns, per agent, whether its outperformance is significant at
/// `alpha` after accounting for every agent tested. Deterministic given `seed`.
pub fn step_down_significant(
    field: &[Vec<f64>],
    seed: u64,
    n_boot: usize,
    block_prob: f64,
    alpha: f64,
) -> Vec<bool> {
    let k = field.len();
    if k == 0 {
        return Vec::new();
    }
    let n = field.iter().map(Vec::len).min().unwrap_or(0);
    if n < 2 || n_boot == 0 {
        return vec![false; k];
    }
    let sqrt_n = (n as f64).sqrt();
    let means: Vec<f64> = field.iter().map(|f| mean(&f[..n])).collect();
    let t: Vec<f64> = means.iter().map(|m| sqrt_n * m).collect();

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
                sqrt_n * (bmean - means[ki])
            })
            .collect();
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
    rejected
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strong_edge_is_significant() {
        let r: Vec<f64> = (0..200)
            .map(|i| 0.002 + 0.0005 * ((i % 3) as f64 - 1.0))
            .collect();
        let p = bootstrap_pvalue(&r, 42, 2000, 0.1);
        assert!(p < 0.05, "p={p}");
    }

    #[test]
    fn zero_mean_is_not_significant() {
        let r: Vec<f64> = (0..200)
            .map(|i| if i % 2 == 0 { 0.01 } else { -0.01 })
            .collect();
        let p = bootstrap_pvalue(&r, 42, 2000, 0.1);
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
        assert!(reality_check_pvalue(&field, 1, 1000, 0.1) < 0.1);
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
        assert!(reality_check_pvalue(&field, 1, 1000, 0.1) > 0.1);
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
            spa_pvalue(&field, 1, 1000, 0.1) < 0.1,
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
            spa_pvalue(&noise, 1, 1000, 0.1) > 0.1,
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
        let c = spa_consistent_pvalue(&field, 1, 1000, 0.1);
        let l = spa_pvalue(&field, 1, 1000, 0.1);
        assert!(c <= l + 1e-12, "consistent {c} should be ≤ studentized {l}");
        assert!(c < 0.1, "should still flag the real leader");
    }

    #[test]
    fn dsr_ci_brackets_point_and_is_deterministic() {
        let r: Vec<f64> = (0..200)
            .map(|i| 0.01 + 0.002 * (i as f64 * 0.5).sin())
            .collect();
        let a = bootstrap_dsr_ci(&r, 50, 0.5, 7, 800, 0.1, 0.90);
        let b = bootstrap_dsr_ci(&r, 50, 0.5, 7, 800, 0.1, 0.90);
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
        let wide = bootstrap_dsr_ci(&short_noisy, 50, 0.5, 3, 800, 0.1, 0.90);
        let tight = bootstrap_dsr_ci(&long_steady, 50, 0.5, 3, 800, 0.1, 0.90);
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
        let s = bootstrap_dsr_ci(&strong, 2, 0.01, 11, 800, 0.1, 0.90);
        let w = bootstrap_dsr_ci(&weak, 2, 0.01, 11, 800, 0.1, 0.90);
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
        let sig = step_down_significant(&field, 1, 1000, 0.1, 0.05);
        assert!(sig[0], "the strong agent should be significant");
        assert!(sig[1..].iter().all(|&s| !s), "noise agents should not be");
    }
}
