//! Cont's stylized facts — a deterministic realism validator for a return dataset.
//!
//! A benchmark is only as honest as the market it simulates. A synthetic (or
//! frozen) dataset that has thin Gaussian tails, no volatility clustering, and is
//! symmetric under time reversal is a *toy*: an agent that wins on it has learned
//! nothing about real markets, and a generator that silently drifts into that toy
//! regime quietly invalidates every score computed on it.
//!
//! This module measures the canonical **stylized facts of asset returns** (Cont,
//! *Empirical properties of asset returns*, 2001) over a return series and certifies
//! that a dataset exhibits them:
//! - **Fat tails** — positive excess kurtosis; extreme moves are far more frequent
//!   than a Gaussian predicts.
//! - **Volatility clustering** — slow-decaying positive autocorrelation in the
//!   *magnitude* of returns (|r| and r²), even though signed returns are ~white.
//! - **Aggregational Gaussianity** — as returns are summed over longer horizons the
//!   distribution walks back toward Gaussian (excess kurtosis shrinks).
//! - **Time-reversal asymmetry (the Zumbach / leverage effect)** — past returns
//!   drive future volatility more than future returns "drive" past volatility; a
//!   time-reversal-symmetric process (Gaussian i.i.d., plain GARCH) has none.
//!
//! Pure and deterministic: plain `f64`, fixed reduction order, no RNG, no I/O, and
//! (like the rest of this crate) no dependencies. The moment primitives are reused
//! verbatim from [`crate::stats`].

use crate::stats::{kurtosis, mean, skewness};

/// The measured stylized-facts profile of a return series. Each field is a plain
/// statistic; the realism predicates ([`StylizedFactsReport::has_fat_tails`] …)
/// compare them against a [`RealismThresholds`].
#[derive(Clone, Debug)]
pub struct StylizedFactsReport {
    /// Excess kurtosis (`kurtosis - 3`). > 0 ⇒ fatter tails than a Gaussian.
    pub excess_kurtosis: f64,
    /// Lag-1 autocorrelation of |returns| — the cleanest single volatility-clustering
    /// signal (magnitudes persist even when signed returns do not).
    pub abs_return_autocorr: f64,
    /// Mean autocorrelation of *squared* returns over the first several lags — the
    /// slow-decaying persistence that is the hallmark of volatility clustering.
    pub vol_clustering_acf: f64,
    /// Skewness of returns — the gain/loss asymmetry (equity indices fall faster
    /// than they rise, so this is typically negative). Reported, not gated.
    pub gain_loss_skew: f64,
    /// Excess-kurtosis *drop* under temporal aggregation (`raw − aggregated`). > 0 ⇒
    /// the distribution becomes more Gaussian at longer horizons (aggregational
    /// Gaussianity); the aggregation block size is [`AGGREGATION_BLOCK`].
    pub aggregational_gaussianity: f64,
    /// Time-reversal-asymmetry (Zumbach/leverage) score: `lev_fwd − lev_rev`, the
    /// difference between "past return → future volatility" and "past volatility →
    /// future return" lead-lag correlation. ~0 under time reversal; markedly
    /// non-zero (usually negative, from the leverage effect) in real markets.
    pub zumbach_asymmetry: f64,
}

/// Non-overlapping block size used for the aggregational-Gaussianity measurement
/// (5 periods ≈ a trading week of daily bars).
pub const AGGREGATION_BLOCK: usize = 5;

/// Number of lags averaged for [`StylizedFactsReport::vol_clustering_acf`] and the
/// Zumbach lead-lag terms.
const CLUSTER_LAGS: usize = 10;

/// Realism-gate thresholds. Defaults are deliberately permissive lower bounds — a
/// dataset only has to *clear* each stylized fact, not match any particular market.
#[derive(Clone, Copy, Debug)]
pub struct RealismThresholds {
    /// Minimum excess kurtosis to count as fat-tailed.
    pub min_excess_kurtosis: f64,
    /// Minimum lag-1 |return| autocorrelation to count as volatility-clustered.
    pub min_abs_return_autocorr: f64,
    /// Minimum excess-kurtosis drop under aggregation to count as aggregationally
    /// Gaussian.
    pub min_aggregational_gaussianity: f64,
    /// Minimum |Zumbach asymmetry| to count as time-reversal-asymmetric.
    pub min_zumbach_asymmetry: f64,
}

impl Default for RealismThresholds {
    fn default() -> Self {
        Self {
            min_excess_kurtosis: 0.5,
            min_abs_return_autocorr: 0.02,
            min_aggregational_gaussianity: 0.1,
            // Weak by design: real markets differ in leverage strength (equity
            // indices are strongly time-asymmetric, crypto only mildly), so the bar
            // only has to separate a genuine leverage signal from Gaussian noise
            // (~0.001), not match any one asset class.
            min_zumbach_asymmetry: 0.005,
        }
    }
}

/// A single stylized fact a dataset failed to exhibit.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RealismFailure {
    /// Tails no fatter than a Gaussian's.
    ThinTails,
    /// No persistence in return magnitudes.
    NoVolatilityClustering,
    /// The distribution does not become more Gaussian under aggregation.
    NoAggregationalGaussianity,
    /// The process looks the same run forwards or backwards (no leverage/Zumbach).
    TimeReversalSymmetric,
}

/// The certification verdict: the measured profile, the thresholds applied, whether
/// every gated stylized fact held, and the specific failures if not.
#[derive(Clone, Debug)]
pub struct RealismVerdict {
    pub report: StylizedFactsReport,
    pub thresholds: RealismThresholds,
    /// True iff the dataset exhibits every *gated* stylized fact (fat tails,
    /// volatility clustering, aggregational Gaussianity, time-reversal asymmetry).
    pub realistic: bool,
    pub failures: Vec<RealismFailure>,
}

impl StylizedFactsReport {
    /// Fat tails: excess kurtosis clears the bar.
    pub fn has_fat_tails(&self, t: &RealismThresholds) -> bool {
        self.excess_kurtosis >= t.min_excess_kurtosis
    }
    /// Volatility clustering: |return| autocorrelation clears the bar.
    pub fn has_volatility_clustering(&self, t: &RealismThresholds) -> bool {
        self.abs_return_autocorr >= t.min_abs_return_autocorr
    }
    /// Aggregational Gaussianity: kurtosis shrinks enough under aggregation.
    pub fn has_aggregational_gaussianity(&self, t: &RealismThresholds) -> bool {
        self.aggregational_gaussianity >= t.min_aggregational_gaussianity
    }
    /// Time-reversal asymmetry: the Zumbach/leverage score is large enough in
    /// magnitude (either sign) to distinguish the series from its time reversal.
    pub fn has_time_reversal_asymmetry(&self, t: &RealismThresholds) -> bool {
        self.zumbach_asymmetry.abs() >= t.min_zumbach_asymmetry
    }

    /// Every gated stylized fact holds.
    pub fn is_realistic(&self, t: &RealismThresholds) -> bool {
        self.failures(t).is_empty()
    }

    /// The specific stylized facts the series fails to exhibit (empty ⇒ realistic).
    pub fn failures(&self, t: &RealismThresholds) -> Vec<RealismFailure> {
        let mut out = Vec::new();
        if !self.has_fat_tails(t) {
            out.push(RealismFailure::ThinTails);
        }
        if !self.has_volatility_clustering(t) {
            out.push(RealismFailure::NoVolatilityClustering);
        }
        if !self.has_aggregational_gaussianity(t) {
            out.push(RealismFailure::NoAggregationalGaussianity);
        }
        if !self.has_time_reversal_asymmetry(t) {
            out.push(RealismFailure::TimeReversalSymmetric);
        }
        out
    }
}

/// Biased autocorrelation of `xs` at `lag` (denominator is the full sum of squares,
/// the standard estimator). 0.0 when undefined (too short or constant).
fn autocorr(xs: &[f64], lag: usize) -> f64 {
    let n = xs.len();
    if lag == 0 {
        return 1.0;
    }
    if n <= lag {
        return 0.0;
    }
    let m = mean(xs);
    let mut den = 0.0;
    for x in xs {
        let d = x - m;
        den += d * d;
    }
    if den <= 0.0 {
        return 0.0;
    }
    let mut num = 0.0;
    for i in 0..(n - lag) {
        num += (xs[i] - m) * (xs[i + lag] - m);
    }
    num / den
}

/// Normalized lead-lag cross-correlation `corr(a_t, b_{t+lag})`, normalized by the
/// full-sample (population) standard deviations. 0.0 when undefined.
fn cross_corr(a: &[f64], b: &[f64], lag: usize) -> f64 {
    let n = a.len().min(b.len());
    if n <= lag {
        return 0.0;
    }
    let ma = mean(&a[..n]);
    let mb = mean(&b[..n]);
    let mut va = 0.0;
    let mut vb = 0.0;
    for i in 0..n {
        va += (a[i] - ma) * (a[i] - ma);
        vb += (b[i] - mb) * (b[i] - mb);
    }
    if va <= 0.0 || vb <= 0.0 {
        return 0.0;
    }
    let mut cov = 0.0;
    for t in 0..(n - lag) {
        cov += (a[t] - ma) * (b[t + lag] - mb);
    }
    // Same normalization for cov (sum, not mean) as va/vb, so this is a correlation.
    cov / (va.sqrt() * vb.sqrt())
}

/// Excess kurtosis of non-overlapping block sums of `xs` (block size `block`).
/// Falls back to the raw excess kurtosis when the series is too short to aggregate.
fn aggregated_excess_kurtosis(xs: &[f64], block: usize) -> f64 {
    if block <= 1 || xs.len() < block * 4 {
        return kurtosis(xs) - 3.0;
    }
    let agg: Vec<f64> = xs
        .chunks_exact(block)
        .map(|c| c.iter().sum::<f64>())
        .collect();
    kurtosis(&agg) - 3.0
}

/// Measure the stylized-facts profile of a return series. Pure; deterministic.
pub fn stylized_facts(returns: &[f64]) -> StylizedFactsReport {
    let abs: Vec<f64> = returns.iter().map(|r| r.abs()).collect();
    let sq: Vec<f64> = returns.iter().map(|r| r * r).collect();

    let excess_kurtosis = kurtosis(returns) - 3.0;
    let abs_return_autocorr = autocorr(&abs, 1);

    let max_lag = CLUSTER_LAGS.min(returns.len().saturating_sub(2)).max(1);
    let vol_clustering_acf = (1..=max_lag).map(|k| autocorr(&sq, k)).sum::<f64>() / max_lag as f64;

    let gain_loss_skew = skewness(returns);

    let raw_xk = excess_kurtosis;
    let agg_xk = aggregated_excess_kurtosis(returns, AGGREGATION_BLOCK);
    let aggregational_gaussianity = raw_xk - agg_xk;

    // Zumbach / leverage time-reversal asymmetry: "past return → future vol" versus
    // "past vol → future return", averaged over the near lags. A time-reversal-
    // symmetric process has these equal (score ~0); the leverage effect makes the
    // forward term negative in real markets.
    let lev_fwd = (1..=max_lag)
        .map(|k| cross_corr(returns, &sq, k))
        .sum::<f64>()
        / max_lag as f64;
    let lev_rev = (1..=max_lag)
        .map(|k| cross_corr(&sq, returns, k))
        .sum::<f64>()
        / max_lag as f64;
    let zumbach_asymmetry = lev_fwd - lev_rev;

    StylizedFactsReport {
        excess_kurtosis,
        abs_return_autocorr,
        vol_clustering_acf,
        gain_loss_skew,
        aggregational_gaussianity,
        zumbach_asymmetry,
    }
}

/// Certify a dataset against the default [`RealismThresholds`].
pub fn validate_dataset(returns: &[f64]) -> RealismVerdict {
    validate_dataset_with(returns, &RealismThresholds::default())
}

/// Certify a dataset against explicit thresholds.
pub fn validate_dataset_with(returns: &[f64], thresholds: &RealismThresholds) -> RealismVerdict {
    let report = stylized_facts(returns);
    let failures = report.failures(thresholds);
    RealismVerdict {
        realistic: failures.is_empty(),
        report,
        thresholds: *thresholds,
        failures,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A tiny deterministic LCG in [0, 1) — no crate deps, byte-identical anywhere.
    struct Lcg(u64);
    impl Lcg {
        fn u(&mut self) -> f64 {
            self.0 = self
                .0
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            (self.0 >> 11) as f64 / (1u64 << 53) as f64
        }
        /// Approximate standard normal (sum of 12 uniforms − 6; mean 0, var 1).
        fn z(&mut self) -> f64 {
            let mut s = 0.0;
            for _ in 0..12 {
                s += self.u();
            }
            s - 6.0
        }
    }

    /// Thin-tailed Gaussian i.i.d. returns — the null "toy market".
    fn gaussian_iid(n: usize, seed: u64) -> Vec<f64> {
        let mut r = Lcg(seed);
        (0..n).map(|_| 0.0005 + 0.01 * r.z()).collect()
    }

    /// A leverage stochastic-vol series: stable AR(1) log-vol with a leverage term
    /// (negative returns raise next-period vol) and heavy-tailed innovations. Built
    /// to exhibit every stylized fact at once.
    fn leverage_sv(n: usize, seed: u64) -> Vec<f64> {
        let mut r = Lcg(seed);
        let mean_lv = -4.6_f64;
        let mut log_vol = mean_lv;
        let mut z_prev = 0.0_f64;
        let mut out = Vec::with_capacity(n);
        for _ in 0..n {
            let eta = r.z();
            log_vol = mean_lv + 0.94 * (log_vol - mean_lv) - 0.20 * z_prev + 0.30 * eta;
            let heavy = if r.u() < 0.04 { 3.5 } else { 1.0 };
            let z = r.z() * heavy;
            out.push(0.0003 + log_vol.exp() * z);
            z_prev = z;
        }
        out
    }

    #[test]
    fn gaussian_iid_is_not_realistic() {
        let v = validate_dataset(&gaussian_iid(4000, 12345));
        assert!(!v.realistic, "thin Gaussian toy must fail: {:?}", v.report);
        assert!(
            v.failures.contains(&RealismFailure::ThinTails),
            "no fat tails in a Gaussian: {:?}",
            v.report
        );
        // Sanity on the raw statistics: near-zero excess kurtosis and no clustering.
        assert!(v.report.excess_kurtosis.abs() < 0.5);
        assert!(v.report.abs_return_autocorr < 0.05);
    }

    #[test]
    fn fat_tailed_clustered_series_is_realistic() {
        let v = validate_dataset(&leverage_sv(4000, 99999));
        assert!(v.realistic, "leverage-SV must certify: {:?}", v);
        assert!(v.failures.is_empty());
        // Each stylized fact is on the realistic side of its bar.
        assert!(v.report.excess_kurtosis > 1.0, "fat tails");
        assert!(v.report.abs_return_autocorr > 0.1, "volatility clustering");
        assert!(
            v.report.vol_clustering_acf > 0.0,
            "squared-return persistence"
        );
        assert!(
            v.report.aggregational_gaussianity > 0.0,
            "kurtosis falls under aggregation"
        );
        assert!(
            v.report.zumbach_asymmetry.abs() >= 0.01,
            "time-reversal asymmetry from leverage"
        );
    }

    #[test]
    fn zumbach_asymmetry_is_directional_leverage() {
        // The leverage term makes past returns predict future volatility more than
        // the reverse, so the forward-minus-reverse score is negative.
        let v = stylized_facts(&leverage_sv(4000, 7));
        assert!(
            v.zumbach_asymmetry < 0.0,
            "leverage → negative Zumbach score, got {}",
            v.zumbach_asymmetry
        );
    }

    #[test]
    fn time_symmetric_process_has_no_zumbach() {
        // Gaussian i.i.d. is time-reversal symmetric: the score sits near zero.
        let v = stylized_facts(&gaussian_iid(4000, 4242));
        assert!(
            v.zumbach_asymmetry.abs() < 0.01,
            "iid is time-symmetric, got {}",
            v.zumbach_asymmetry
        );
    }

    #[test]
    fn constant_and_short_series_are_safe() {
        // No panics, no NaNs, and (correctly) not certified realistic.
        for r in [vec![], vec![0.0; 8], vec![0.001; 3], vec![0.01, -0.01]] {
            let v = validate_dataset(&r);
            assert!(v.report.excess_kurtosis.is_finite());
            assert!(v.report.abs_return_autocorr.is_finite());
            assert!(v.report.zumbach_asymmetry.is_finite());
            assert!(!v.realistic);
        }
    }

    #[test]
    fn thresholds_are_configurable() {
        let returns = gaussian_iid(2000, 1);
        // An impossible-to-fail threshold set certifies anything finite.
        let lax = RealismThresholds {
            min_excess_kurtosis: f64::NEG_INFINITY,
            min_abs_return_autocorr: f64::NEG_INFINITY,
            min_aggregational_gaussianity: f64::NEG_INFINITY,
            min_zumbach_asymmetry: 0.0,
        };
        assert!(validate_dataset_with(&returns, &lax).realistic);
    }
}
