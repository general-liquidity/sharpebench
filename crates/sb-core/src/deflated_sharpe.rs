//! Sharpe ratio, Probabilistic Sharpe Ratio (PSR), and Deflated Sharpe Ratio (DSR).
//!
//! After Bailey & López de Prado, *The Deflated Sharpe Ratio* (2014). All ratios
//! are computed on **per-period** returns (do not pre-annualize — annualizing a
//! short track inflates the noise these statistics exist to expose).

use crate::stats::{kurtosis, mean, norm_cdf, norm_ppf, skewness, std_dev};

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

/// Expected maximum Sharpe ratio under `n_trials` independent strategy trials,
/// given the cross-trial dispersion of Sharpe ratios `trials_sr_std`
/// (Bailey & López de Prado, eq. for E[max SR_N]).
pub fn expected_max_sharpe(trials_sr_std: f64, n_trials: u32) -> f64 {
    let n = n_trials.max(1) as f64;
    if n <= 1.0 || trials_sr_std <= 0.0 {
        return 0.0;
    }
    const GAMMA: f64 = 0.577_215_664_901_532_9; // Euler–Mascheroni
    let e = std::f64::consts::E;
    let z1 = norm_ppf(1.0 - 1.0 / n);
    let z2 = norm_ppf(1.0 - 1.0 / (n * e));
    trials_sr_std * ((1.0 - GAMMA) * z1 + GAMMA * z2)
}

/// Deflated Sharpe Ratio: the PSR computed against the *expected maximum* Sharpe
/// you'd see by chance across `n_trials` strategies. A value near 1.0 means the
/// observed Sharpe is very unlikely to be the product of selection over many
/// trials; near 0.0 means it is indistinguishable from luck.
///
/// `trials_sr_std` is the dispersion of Sharpe ratios across the trials/agents
/// that were tested (the multiple-testing footprint). Larger ⇒ harder to clear.
pub fn deflated_sharpe_ratio(returns: &[f64], n_trials: u32, trials_sr_std: f64) -> f64 {
    let sr_star = expected_max_sharpe(trials_sr_std, n_trials);
    probabilistic_sharpe_ratio(returns, sr_star)
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
        let few = deflated_sharpe_ratio(&r, 2, 0.5);
        let many = deflated_sharpe_ratio(&r, 500, 0.5);
        assert!(
            many < few,
            "many-trial DSR {many} should be < few-trial DSR {few}"
        );
    }
}
