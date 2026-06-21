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

use crate::stats::mean;

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strong_edge_is_significant() {
        let r: Vec<f64> = (0..200).map(|i| 0.002 + 0.0005 * ((i % 3) as f64 - 1.0)).collect();
        let p = bootstrap_pvalue(&r, 42, 2000, 0.1);
        assert!(p < 0.05, "p={p}");
    }

    #[test]
    fn zero_mean_is_not_significant() {
        let r: Vec<f64> = (0..200).map(|i| if i % 2 == 0 { 0.01 } else { -0.01 }).collect();
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
}
