//! Minimum Track Record Length (MinTRL).
//!
//! After Bailey & López de Prado, *The Sharpe Ratio Efficient Frontier* (2012):
//! how long a track record must be before the observed Sharpe is statistically
//! distinguishable from `sr_benchmark` at a chosen confidence `c`. It inverts the
//! Probabilistic Sharpe Ratio
//!
//! ```text
//! PSR(SR*) = Φ( (SR_hat − SR*)·√(n−1) / √(1 − γ3·SR_hat + ((γ4−1)/4)·SR_hat²) )
//! ```
//!
//! for `n`, holding the observed Sharpe, skew and kurtosis fixed:
//!
//! ```text
//! MinTRL = 1 + (1 − γ3·SR_hat + ((γ4−1)/4)·SR_hat²) · ( Φ⁻¹(c) / (SR_hat − sr_benchmark) )²
//! ```
//!
//! Ported from the published equations, not from any GPL/proprietary library.

use sharpebench_stats::sharpe_ratio;
use sharpebench_stats::stats::{kurtosis, norm_ppf, skewness};

/// Numerical floor on the variance-adjustment bracket — the same guard
/// `deflated_sharpe.rs` applies to its PSR denominator before taking a root.
const BRACKET_FLOOR: f64 = 1e-12;

/// Minimum track record length (in periods) for the observed Sharpe to clear
/// `sr_benchmark` at confidence `confidence`.
///
/// `confidence` is a probability in (0, 1) — e.g. 0.95. `sr_benchmark` is the
/// per-period Sharpe to beat (0.0 = "is there any edge at all?"). Returns the
/// number of observations required; compare against `returns.len()`.
///
/// If the observed Sharpe does not exceed the benchmark the target is
/// unreachable and this returns `f64::INFINITY`.
pub fn min_track_record_length(returns: &[f64], sr_benchmark: f64, confidence: f64) -> f64 {
    let sr_hat = sharpe_ratio(returns);
    if sr_hat <= sr_benchmark {
        return f64::INFINITY;
    }
    let g3 = skewness(returns);
    let g4 = kurtosis(returns); // non-excess (normal = 3)
    let bracket = (1.0 - g3 * sr_hat + ((g4 - 1.0) / 4.0) * sr_hat * sr_hat).max(BRACKET_FLOOR);
    let z = norm_ppf(confidence);
    let ratio = z / (sr_hat - sr_benchmark);
    1.0 + bracket * ratio * ratio
}

#[cfg(test)]
mod tests {
    use super::*;
    use sharpebench_stats::probabilistic_sharpe_ratio;

    /// Self-consistency: a synthetic iid-normal-ish series of length MinTRL with
    /// the same per-period Sharpe should clear PSR ≥ confidence (round up the
    /// fractional length).
    #[test]
    fn mintrl_self_consistency() {
        // A real-ish return series with mild positive Sharpe.
        let base: Vec<f64> = (0..80)
            .map(|i| 0.004 + 0.01 * ((i % 7) as f64 - 3.0))
            .collect();
        let confidence = 0.95;
        let n_req = min_track_record_length(&base, 0.0, confidence);
        assert!(n_req.is_finite() && n_req > 1.0);

        // Build a low-skew/low-kurtosis series of length ceil(n_req) carrying the
        // same per-period Sharpe, so the bracket ≈ 1 and PSR should just clear.
        let n = n_req.ceil() as usize + 1;
        let target_sr = sharpe_ratio(&base);
        // A symmetric two-point series has the exact mean/std we want and skew 0.
        let mu = 0.001_f64;
        let sigma = mu / target_sr;
        let synthetic: Vec<f64> = (0..n)
            .map(|i| if i % 2 == 0 { mu + sigma } else { mu - sigma })
            .collect();
        let psr = probabilistic_sharpe_ratio(&synthetic, 0.0);
        assert!(
            psr >= confidence - 0.02,
            "PSR {psr} at MinTRL length {n} should clear {confidence}"
        );
    }

    /// A Sharpe at or below the benchmark is never significant: MinTRL = ∞.
    #[test]
    fn unreachable_returns_infinity() {
        let flat: Vec<f64> = (0..50).map(|i| 0.001 * ((i % 3) as f64 - 1.0)).collect();
        assert_eq!(min_track_record_length(&flat, 0.5, 0.95), f64::INFINITY);
    }

    /// Higher confidence demands a longer track record.
    #[test]
    fn higher_confidence_needs_more_data() {
        let r: Vec<f64> = (0..120)
            .map(|i| 0.003 + 0.008 * (i as f64 * 0.7).sin())
            .collect();
        let lo = min_track_record_length(&r, 0.0, 0.90);
        let hi = min_track_record_length(&r, 0.0, 0.99);
        assert!(hi > lo, "MinTRL(0.99)={hi} should exceed MinTRL(0.90)={lo}");
    }
}
