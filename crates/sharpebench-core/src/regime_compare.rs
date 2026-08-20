//! Regime-conditional distributional comparison: where does the edge actually live?
//!
//! The board deflates and tests significance on *pooled* returns. Pooling is a
//! real blind spot: two strategies can share a pooled distribution and still
//! behave nothing alike once you condition on the market state. One earns its
//! whole edge in a crisis and bleeds the rest of the time; the other is the
//! mirror image. Pooled, they look like twins. [`crate::decay`] gestures at the
//! same worry ("is it a one-regime fluke?") but answers it only through an IC
//! half-life, which is a time axis, not a state axis.
//!
//! So: given per-period returns for two strategies plus a regime label per
//! period, this module compares them *within* each regime and compares
//! **distributions**, not just means. A mean gap that reverses sign between
//! regimes is the headline finding, and
//! [`RegimeDistributionReport::pooled_hides_reversal`] says so outright.
//!
//! Adapted from "Regime-Conditional Distributional Comparison of Trading
//! Strategies: A GAMLSS ZAGA Framework Applied to the S&P 500".
//!
//! # What the ZAGA idea buys, and what is actually implemented here
//!
//! The load-bearing modelling claim in that paper is that a strategy return
//! series is a **mixture**: a point mass at (or near) zero for the periods with
//! no position and no trade, plus a continuous part for the periods that
//! actually traded. Smoothing that mixture into one continuous density is the
//! modelling error the ZAGA (zero-adjusted gamma) form exists to avoid: the zero
//! mass drags the fitted mean and variance toward zero and makes an aggressive,
//! selective strategy look like a timid one.
//!
//! **Implemented here:** the mixture split itself. Per regime, per strategy, the
//! zero/no-trade mass is separated from the continuous part; both are compared.
//! The continuous part is summarised by its mean, standard deviation and median,
//! by the sign split of its non-zero returns, and by a method-of-moments gamma
//! (shape, rate) fitted to the *magnitudes* of those returns. The two continuous
//! parts are also compared non-parametrically with a two-sample
//! Kolmogorov-Smirnov statistic, so a difference in shape shows up even when the
//! means agree.
//!
//! **Deliberately NOT implemented:** a GAMLSS fit. There are no link functions,
//! no penalised splines, no smooth covariate terms for mu / sigma / nu, no
//! iterative backfitting, no likelihood maximisation, and no standard errors or
//! p-values on fitted parameters. The gamma parameters here are moment matches
//! on magnitudes, not maximum-likelihood estimates, and they are descriptive
//! summaries rather than a fitted model. Nobody should read this module as
//! evidence that a GAMLSS ZAGA fit lives in the crate. If you need the real
//! thing, fit it elsewhere and feed the result in.
//!
//! Regime labels are an **input**. This module does not infer regimes and does
//! not contain a classifier: whoever labels the periods owns that judgement, and
//! burying a classifier here would let a regime definition quietly become part
//! of the scoring kernel.

use serde::Serialize;

use crate::stats::{mean, std_dev, variance};

/// Knobs for [`compare_by_regime`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RegimeCompareOpts {
    /// A return whose absolute value is at or below this counts as "no trade"
    /// and lands in the zero mass rather than the continuous part. Exact zeros
    /// are the common case, but a fee-only or rounding-dust period is the same
    /// economic event and should not be modelled as a tiny continuous return.
    pub zero_tol: f64,
    /// Regimes with fewer than this many periods are still reported but are
    /// excluded from the reversal verdict: a two-period "regime" flipping sign
    /// is noise, not a finding.
    pub min_periods: usize,
    /// Mean gaps at or below this magnitude are called a tie rather than an edge.
    pub tie_tol: f64,
}

impl Default for RegimeCompareOpts {
    fn default() -> Self {
        Self {
            zero_tol: 1e-9,
            min_periods: 8,
            tie_tol: 1e-12,
        }
    }
}

/// The zero-mass / continuous-part split for one strategy inside one regime.
///
/// This is the ZAGA decomposition and nothing more: see the module header for
/// what is and is not fitted.
#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub struct ZagaSplit {
    /// Periods observed in this regime.
    pub n: usize,
    /// Fraction of periods in the zero / no-trade mass, in [0, 1]. This is the
    /// `nu` role in a ZAGA: how often the strategy simply did not play.
    pub zero_mass: f64,
    /// Periods in the continuous part.
    pub n_nonzero: usize,
    /// Fraction of the continuous part that is positive, in [0, 1]. A gamma is
    /// non-negative, so the sign has to be carried separately; this is where it
    /// lives.
    pub positive_share: f64,
    /// Mean of the continuous part (signed).
    pub cont_mean: f64,
    /// Standard deviation of the continuous part (signed returns, sample `n-1`).
    pub cont_sd: f64,
    /// Median of the continuous part (signed).
    pub cont_median: f64,
    /// Method-of-moments gamma shape fitted to the *magnitudes* of the
    /// continuous part. 0.0 when undefined (no dispersion, or nothing to fit).
    pub gamma_shape: f64,
    /// Method-of-moments gamma rate for the same magnitudes. 0.0 when undefined.
    pub gamma_rate: f64,
    /// Mean over all periods in the regime, zeros included. Kept alongside
    /// `cont_mean` because the gap between the two is exactly the distortion the
    /// mixture split exists to expose.
    pub pooled_mean: f64,
}

/// Head-to-head comparison of two strategies inside one regime.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct RegimeComparison {
    /// The caller's regime label.
    pub regime: String,
    /// Periods carrying this label.
    pub n_periods: usize,
    /// Strategy A's mixture split in this regime.
    pub a: ZagaSplit,
    /// Strategy B's mixture split in this regime.
    pub b: ZagaSplit,
    /// `a.zero_mass - b.zero_mass`. Positive means A sat out more often here,
    /// which is a genuine behavioural difference even when the means agree.
    pub zero_mass_gap: f64,
    /// `a.pooled_mean - b.pooled_mean` inside this regime.
    pub mean_gap: f64,
    /// `a.cont_mean - b.cont_mean`: the gap once the no-trade periods are taken
    /// out. Diverging from `mean_gap` means the pooled comparison was mostly
    /// measuring participation rate, not per-trade skill.
    pub cont_mean_gap: f64,
    /// Two-sample Kolmogorov-Smirnov statistic between the two continuous parts,
    /// in [0, 1]. Non-parametric, so a pure shape difference (same mean, fatter
    /// left tail) still registers.
    pub ks_statistic: f64,
    /// `+1` if A leads on `mean_gap` here, `-1` if B leads, `0` within `tie_tol`.
    pub edge_sign: i8,
    /// Whether this regime cleared `min_periods` and therefore counts toward the
    /// reversal verdict.
    pub counted: bool,
}

/// Full regime-conditional comparison of two strategies.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct RegimeDistributionReport {
    /// One entry per distinct regime label, ordered lexicographically by label so
    /// the report is byte-identical on every recompute.
    pub regimes: Vec<RegimeComparison>,
    /// `mean(a) - mean(b)` over every aligned period, ignoring regime. This is
    /// the number the rest of the board would have seen.
    pub pooled_mean_gap: f64,
    /// `+1` / `-1` / `0` for `pooled_mean_gap` under `tie_tol`.
    pub pooled_edge_sign: i8,
    /// Counted regimes whose `edge_sign` is non-zero and opposite to
    /// `pooled_edge_sign`.
    pub reversal_regimes: Vec<String>,
    /// True when the pooled verdict has a sign and at least one counted regime
    /// contradicts it. The pooled number is then not so much wrong as unusable:
    /// it is averaging over a sign change.
    pub pooled_hides_reversal: bool,
    /// Spread between the best and worst counted regime `mean_gap`. A large
    /// spread with no reversal still says the edge is concentrated, not general.
    pub edge_dispersion: f64,
}

fn sign_of(x: f64, tie_tol: f64) -> i8 {
    if x > tie_tol {
        1
    } else if x < -tie_tol {
        -1
    } else {
        0
    }
}

fn sorted_copy(xs: &[f64]) -> Vec<f64> {
    let mut v = xs.to_vec();
    v.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    v
}

fn median_of(xs: &[f64]) -> f64 {
    let n = xs.len();
    if n == 0 {
        return 0.0;
    }
    let s = sorted_copy(xs);
    if n % 2 == 1 {
        s[n / 2]
    } else {
        0.5 * (s[n / 2 - 1] + s[n / 2])
    }
}

/// Two-sample Kolmogorov-Smirnov statistic: the largest gap between the two
/// empirical CDFs. Ties are advanced together so repeated values do not inflate
/// the statistic. 0.0 when either sample is empty.
fn ks_two_sample(a: &[f64], b: &[f64]) -> f64 {
    if a.is_empty() || b.is_empty() {
        return 0.0;
    }
    let xa = sorted_copy(a);
    let xb = sorted_copy(b);
    let na = xa.len() as f64;
    let nb = xb.len() as f64;
    let (mut i, mut j) = (0usize, 0usize);
    let mut d = 0.0_f64;
    loop {
        let v = match (xa.get(i), xb.get(j)) {
            (Some(&va), Some(&vb)) => va.min(vb),
            (Some(&va), None) => va,
            (None, Some(&vb)) => vb,
            (None, None) => break,
        };
        while i < xa.len() && xa[i] <= v {
            i += 1;
        }
        while j < xb.len() && xb[j] <= v {
            j += 1;
        }
        let diff = (i as f64 / na - j as f64 / nb).abs();
        if diff > d {
            d = diff;
        }
    }
    d
}

/// Split one strategy's returns inside one regime into zero mass plus continuous
/// part, and summarise both.
fn zaga_split(xs: &[f64], zero_tol: f64) -> ZagaSplit {
    let n = xs.len();
    let cont: Vec<f64> = xs.iter().copied().filter(|r| r.abs() > zero_tol).collect();
    let n_nonzero = cont.len();
    let zero_mass = if n == 0 {
        0.0
    } else {
        (n - n_nonzero) as f64 / n as f64
    };
    let positive_share = if n_nonzero == 0 {
        0.0
    } else {
        cont.iter().filter(|&&r| r > 0.0).count() as f64 / n_nonzero as f64
    };

    // Gamma by moment matching on magnitudes. The gamma is the continuous half of
    // a ZAGA and is defined on the positive line, so the sign is carried by
    // `positive_share` rather than folded in here.
    let mags: Vec<f64> = cont.iter().map(|r| r.abs()).collect();
    let mm = mean(&mags);
    let mv = variance(&mags);
    let (gamma_shape, gamma_rate) = if mm > 0.0 && mv > 0.0 {
        (mm * mm / mv, mm / mv)
    } else {
        (0.0, 0.0)
    };

    ZagaSplit {
        n,
        zero_mass,
        n_nonzero,
        positive_share,
        cont_mean: mean(&cont),
        cont_sd: std_dev(&cont),
        cont_median: median_of(&cont),
        gamma_shape,
        gamma_rate,
        pooled_mean: mean(xs),
    }
}

/// Compare two strategies regime by regime.
///
/// `a` and `b` are per-period returns for the two strategies and `regimes` is
/// the regime label for the same periods; all three are aligned and the shortest
/// of them sets the comparison length, so a caller who trims one series does not
/// silently compare misaligned periods. A regime with no periods simply does not
/// appear.
///
/// Pure and deterministic: no randomness, no clock, and regimes come out in
/// lexicographic label order.
pub fn compare_by_regime(
    a: &[f64],
    b: &[f64],
    regimes: &[&str],
    opts: RegimeCompareOpts,
) -> RegimeDistributionReport {
    let n = a.len().min(b.len()).min(regimes.len());
    if n == 0 {
        return RegimeDistributionReport {
            regimes: Vec::new(),
            pooled_mean_gap: 0.0,
            pooled_edge_sign: 0,
            reversal_regimes: Vec::new(),
            pooled_hides_reversal: false,
            edge_dispersion: 0.0,
        };
    }

    let mut labels: Vec<String> = regimes[..n].iter().map(|s| (*s).to_string()).collect();
    labels.sort();
    labels.dedup();

    let pooled_mean_gap = mean(&a[..n]) - mean(&b[..n]);
    let pooled_edge_sign = sign_of(pooled_mean_gap, opts.tie_tol);

    let mut out: Vec<RegimeComparison> = Vec::with_capacity(labels.len());
    for label in labels {
        let idx: Vec<usize> = (0..n).filter(|&i| regimes[i] == label).collect();
        let ra: Vec<f64> = idx.iter().map(|&i| a[i]).collect();
        let rb: Vec<f64> = idx.iter().map(|&i| b[i]).collect();
        let sa = zaga_split(&ra, opts.zero_tol);
        let sb = zaga_split(&rb, opts.zero_tol);
        let mean_gap = sa.pooled_mean - sb.pooled_mean;
        let cont_a: Vec<f64> = ra
            .iter()
            .copied()
            .filter(|r| r.abs() > opts.zero_tol)
            .collect();
        let cont_b: Vec<f64> = rb
            .iter()
            .copied()
            .filter(|r| r.abs() > opts.zero_tol)
            .collect();
        out.push(RegimeComparison {
            regime: label,
            n_periods: idx.len(),
            zero_mass_gap: sa.zero_mass - sb.zero_mass,
            mean_gap,
            cont_mean_gap: sa.cont_mean - sb.cont_mean,
            ks_statistic: ks_two_sample(&cont_a, &cont_b),
            edge_sign: sign_of(mean_gap, opts.tie_tol),
            counted: idx.len() >= opts.min_periods,
            a: sa,
            b: sb,
        });
    }

    let reversal_regimes: Vec<String> = out
        .iter()
        .filter(|r| {
            r.counted
                && r.edge_sign != 0
                && pooled_edge_sign != 0
                && r.edge_sign != pooled_edge_sign
        })
        .map(|r| r.regime.clone())
        .collect();

    let counted_gaps: Vec<f64> = out
        .iter()
        .filter(|r| r.counted)
        .map(|r| r.mean_gap)
        .collect();
    let edge_dispersion = if counted_gaps.len() < 2 {
        0.0
    } else {
        let hi = counted_gaps
            .iter()
            .copied()
            .fold(f64::NEG_INFINITY, f64::max);
        let lo = counted_gaps.iter().copied().fold(f64::INFINITY, f64::min);
        hi - lo
    };

    RegimeDistributionReport {
        regimes: out,
        pooled_mean_gap,
        pooled_edge_sign,
        pooled_hides_reversal: !reversal_regimes.is_empty(),
        reversal_regimes,
        edge_dispersion,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Alternating labels so both regimes get a deterministic, equal-sized slice.
    fn labels(n: usize) -> Vec<&'static str> {
        (0..n)
            .map(|i| if i % 2 == 0 { "bull" } else { "bear" })
            .collect()
    }

    #[test]
    fn pooling_hides_a_sign_reversal() {
        // A wins in bull by exactly as much as it loses in bear, so the pooled
        // gap is ~0 and the pooled comparison says "identical". It is not.
        let n = 120;
        let lab = labels(n);
        let a: Vec<f64> = (0..n)
            .map(|i| if i % 2 == 0 { 0.010 } else { -0.010 })
            .collect();
        let b: Vec<f64> = (0..n)
            .map(|i| if i % 2 == 0 { -0.010 } else { 0.010 })
            .collect();
        let rep = compare_by_regime(&a, &b, &lab, RegimeCompareOpts::default());
        assert!(
            rep.pooled_mean_gap.abs() < 1e-12,
            "pooled gap should vanish: {}",
            rep.pooled_mean_gap
        );
        assert_eq!(rep.regimes.len(), 2);
        let bull = rep.regimes.iter().find(|r| r.regime == "bull").unwrap();
        let bear = rep.regimes.iter().find(|r| r.regime == "bear").unwrap();
        assert_eq!(bull.edge_sign, 1);
        assert_eq!(bear.edge_sign, -1);
        assert!(rep.edge_dispersion > 0.03, "{}", rep.edge_dispersion);
    }

    #[test]
    fn reversal_is_flagged_against_a_signed_pooled_verdict() {
        // A is better overall, but strictly worse in "crisis".
        let mut a = Vec::new();
        let mut b = Vec::new();
        let mut lab: Vec<&'static str> = Vec::new();
        for _ in 0..60 {
            a.push(0.01);
            b.push(0.001);
            lab.push("calm");
        }
        for _ in 0..20 {
            a.push(-0.02);
            b.push(0.005);
            lab.push("crisis");
        }
        let rep = compare_by_regime(&a, &b, &lab, RegimeCompareOpts::default());
        assert_eq!(rep.pooled_edge_sign, 1, "A leads pooled");
        assert!(rep.pooled_hides_reversal);
        assert_eq!(rep.reversal_regimes, vec!["crisis".to_string()]);
    }

    #[test]
    fn consistent_edge_reports_no_reversal() {
        let n = 100;
        let lab = labels(n);
        let a: Vec<f64> = (0..n).map(|i| 0.004 + 0.001 * (i as f64).sin()).collect();
        let b: Vec<f64> = (0..n).map(|i| 0.001 + 0.001 * (i as f64).sin()).collect();
        let rep = compare_by_regime(&a, &b, &lab, RegimeCompareOpts::default());
        assert!(!rep.pooled_hides_reversal);
        assert!(rep.reversal_regimes.is_empty());
        assert!(rep.regimes.iter().all(|r| r.edge_sign == 1));
    }

    #[test]
    fn zero_mass_separates_a_selective_strategy_from_a_timid_one() {
        // A trades one period in four at 4%; B trades every period at 1%. Same
        // pooled mean, completely different behaviour: only the mixture split
        // sees it.
        let n = 80;
        let lab = vec!["all"; n];
        let a: Vec<f64> = (0..n)
            .map(|i| if i % 4 == 0 { 0.04 } else { 0.0 })
            .collect();
        let b: Vec<f64> = vec![0.01; n];
        let rep = compare_by_regime(&a, &b, &lab, RegimeCompareOpts::default());
        let r = &rep.regimes[0];
        assert!(
            r.mean_gap.abs() < 1e-12,
            "pooled means match: {}",
            r.mean_gap
        );
        assert!(
            (r.a.zero_mass - 0.75).abs() < 1e-12,
            "A sits out 3 of 4: {}",
            r.a.zero_mass
        );
        assert!(r.b.zero_mass.abs() < 1e-12, "B always trades");
        assert!(
            r.zero_mass_gap > 0.7,
            "the split should shout: {}",
            r.zero_mass_gap
        );
        assert!(
            (r.cont_mean_gap - 0.03).abs() < 1e-12,
            "per-trade gap is 4% vs 1%: {}",
            r.cont_mean_gap
        );
        assert!(
            r.ks_statistic > 0.9,
            "disjoint supports: {}",
            r.ks_statistic
        );
    }

    #[test]
    fn ks_catches_a_shape_difference_at_equal_means() {
        // Same mean, same zero mass, different dispersion.
        let n = 100;
        let lab = vec!["all"; n];
        let a: Vec<f64> = (0..n)
            .map(|i| if i % 2 == 0 { 0.001 } else { -0.001 })
            .collect();
        let b: Vec<f64> = (0..n)
            .map(|i| if i % 2 == 0 { 0.05 } else { -0.05 })
            .collect();
        let rep = compare_by_regime(&a, &b, &lab, RegimeCompareOpts::default());
        let r = &rep.regimes[0];
        assert!(r.mean_gap.abs() < 1e-12);
        assert!(
            r.ks_statistic > 0.4,
            "shape difference should register: {}",
            r.ks_statistic
        );
        assert!(r.b.cont_sd > r.a.cont_sd);
    }

    #[test]
    fn gamma_moments_match_a_known_magnitude_sample() {
        // Magnitudes {1, 2, 3, 4}: mean 2.5, sample variance 5/3.
        // shape = 2.5^2 / (5/3) = 3.75, rate = 2.5 / (5/3) = 1.5.
        let a = vec![1.0, -2.0, 3.0, -4.0];
        let b = vec![1.0, -2.0, 3.0, -4.0];
        let lab = vec!["all"; 4];
        let rep = compare_by_regime(&a, &b, &lab, RegimeCompareOpts::default());
        let s = rep.regimes[0].a;
        assert!((s.gamma_shape - 3.75).abs() < 1e-9, "{}", s.gamma_shape);
        assert!((s.gamma_rate - 1.5).abs() < 1e-9, "{}", s.gamma_rate);
        assert!((s.positive_share - 0.5).abs() < 1e-12);
    }

    #[test]
    fn short_regimes_do_not_drive_the_reversal_verdict() {
        // A three-period "crisis" reversal is noise and must not be counted.
        let mut a = vec![0.01; 60];
        let mut b = vec![0.001; 60];
        let mut lab: Vec<&'static str> = vec!["calm"; 60];
        a.extend([-0.05, -0.05, -0.05]);
        b.extend([0.02, 0.02, 0.02]);
        lab.extend(["crisis", "crisis", "crisis"]);
        let rep = compare_by_regime(&a, &b, &lab, RegimeCompareOpts::default());
        let crisis = rep.regimes.iter().find(|r| r.regime == "crisis").unwrap();
        assert_eq!(crisis.edge_sign, -1, "the reversal is still reported");
        assert!(!crisis.counted, "but it is not counted");
        assert!(!rep.pooled_hides_reversal);
    }

    #[test]
    fn misaligned_inputs_truncate_to_the_shortest() {
        let a = vec![0.01; 10];
        let b = vec![0.0; 4];
        let lab = vec!["all"; 7];
        let rep = compare_by_regime(&a, &b, &lab, RegimeCompareOpts::default());
        assert_eq!(rep.regimes[0].n_periods, 4);
    }

    #[test]
    fn empty_input_is_inert() {
        let rep = compare_by_regime(&[], &[], &[], RegimeCompareOpts::default());
        assert!(rep.regimes.is_empty());
        assert_eq!(rep.pooled_edge_sign, 0);
        assert!(!rep.pooled_hides_reversal);
    }

    #[test]
    fn output_order_is_lexicographic_and_reproducible() {
        let lab = vec!["zulu", "alpha", "mike", "alpha", "zulu", "mike"];
        let a = vec![0.01, 0.02, 0.03, 0.04, 0.05, 0.06];
        let b = vec![0.0; 6];
        let first = compare_by_regime(&a, &b, &lab, RegimeCompareOpts::default());
        let second = compare_by_regime(&a, &b, &lab, RegimeCompareOpts::default());
        assert_eq!(first, second, "recompute must be identical");
        let order: Vec<&str> = first.regimes.iter().map(|r| r.regime.as_str()).collect();
        assert_eq!(order, vec!["alpha", "mike", "zulu"]);
    }
}
