//! The three opt-in diagnostics the literature audit deferred: the
//! autocorrelation-aware PSR (López de Prado, Lipton and Zoonekynd 2026, eqs. 2
//! and 3, p. 9), the PSR with its standard error evaluated under the null (eqs.
//! 4 and 5, p. 10) and the manipulation-proof performance measure (Goetzmann,
//! Ingersoll, Spiegel and Welch 2007, eq. 18).
//!
//! Reference values marked "Python" come from an independent implementation
//! written from the papers (plain `math`, not this crate): moments with the
//! kernel's normalization (sample standard deviation for the Sharpe, population
//! skewness and kurtosis), the lag-one autocorrelation, eq. 2's bracket and
//! eq. 18 summed directly with `(1 + x) ** (1 - rho)`.

use sharpebench_stats::deflated_sharpe::deflated_sharpe_ratio_against_null;
use sharpebench_stats::stats::{kurtosis, mean, norm_cdf, skewness, std_dev};
use sharpebench_stats::{
    expected_max_sharpe, first_order_autocorrelation, manipulation_proof_performance,
    probabilistic_sharpe_ratio, probabilistic_sharpe_ratio_autocorrelated, sharpe_ratio,
    sharpe_standard_error_autocorrelated, sharpe_variance_factor, StandardErrorAt,
    StatisticalError, DEFAULT_MPPM_RISK_AVERSION,
};

/// A deterministic series with positive first-order autocorrelation (about
/// 0.367): two slow sinusoids around a small positive drift.
fn autocorrelated_series() -> Vec<f64> {
    (0..300)
        .map(|t| {
            let t = t as f64;
            0.0005 + 0.01 * (1.3 * t).sin() + 0.004 * (0.21 * t).cos()
        })
        .collect()
}

fn close(got: f64, want: f64, tol: f64) -> bool {
    (got - want).abs() <= tol
}

/// Eq. 2's bracket for a series' own sample moments, evaluated at `at`. Kept
/// out of the boundary tests below so that they name only the functions whose
/// boundaries they exercise (the paired-boundary gate counts every identifier
/// in a boundary test's body).
fn bracket_of(r: &[f64], at: f64, rho: f64) -> f64 {
    sharpe_variance_factor(at, skewness(r), kurtosis(r), rho).unwrap()
}

/// The finite-moment parameters of `sharpe_variance_factor`, each with one
/// non-finite argument, as `(error name, [sr, skewness, kurtosis])`.
const NON_FINITE_MOMENTS: [(&str, [f64; 3]); 3] = [
    ("sr", [f64::NAN, 0.0, 3.0]),
    ("skewness", [0.1, f64::INFINITY, 3.0]),
    ("kurtosis", [0.1, 0.0, f64::NEG_INFINITY]),
];

// ---------------------------------------------------------------------------
// 1. Autocorrelation-aware PSR
// ---------------------------------------------------------------------------

/// LLZ 2026, p. 9: a two-year monthly track with
/// `(mu, sigma, g3, g4, rho, T) = (0.036%, 0.079%, -2.448, 10.164, 0.2, 24)`
/// has `SR* = 0.456` and `sigma[SR*] = 0.379`, against `0.214` under i.i.d.
/// Normal returns. p. 11: under `SR_0 = 0` the PSR is 0.966, under
/// `SR_0 = 0.1` it is 0.900. The paper's standard error is `sqrt(bracket / T)`.
#[test]
fn reproduces_the_how_to_use_the_sharpe_ratio_worked_example() {
    let sr = 0.036 / 0.079;
    let (g3, g4, rho, t) = (-2.448, 10.164, 0.2, 24.0);
    assert!(close(sr, 0.456, 5e-4), "SR* {sr}");

    let se = |at: f64, g3: f64, g4: f64, rho: f64| {
        (sharpe_variance_factor(at, g3, g4, rho).unwrap() / t).sqrt()
    };
    let observed = se(sr, g3, g4, rho);
    assert!(close(observed, 0.379, 5e-4), "sigma[SR*] {observed}");
    assert!(close(observed, 0.379_489_997_533_331_7, 1e-12));
    let iid_normal = se(sr, 0.0, 3.0, 0.0);
    assert!(close(iid_normal, 0.214, 5e-4), "i.i.d. Normal {iid_normal}");
    // "approximately 43% smaller"
    assert!(close(1.0 - iid_normal / observed, 0.43, 0.01));

    // Eq. 5 at SR_0, then eq. 9 with the exact Normal CDF the paper uses. The
    // kernel's CDF is the frozen Abramowitz-Stegun polynomial, so the PSR itself
    // is checked through the z statistic and the Python value of Z[z].
    for (sr0, want_se, want_psr) in [
        (0.0, 0.25, 0.965_832_005_388_937_1),
        (0.1, 0.276_964_134_761_965_96, 0.900_475_917_403_146),
    ] {
        let null_se = se(sr0, g3, g4, rho);
        assert!(
            close(null_se, want_se, 1e-12),
            "sigma[SR_0 = {sr0}] {null_se}"
        );
        let z = (sr - sr0) / null_se;
        let psr = sharpebench_stats::stats::norm_cdf(z);
        assert!(close(psr, want_psr, 2e-7), "PSR at SR_0 = {sr0}: {psr}");
    }
    assert!(close(
        sharpebench_stats::stats::norm_cdf(sr / 0.25),
        0.966,
        5e-4
    ));
}

/// At `rho = 0` with the standard error at the observed Sharpe, the diagnostic
/// is the kernel's PSR, bit for bit, on every series and benchmark tried; and
/// at the deflation bar it is the kernel's DSR.
#[test]
fn zero_autocorrelation_reduces_to_the_kernel_psr_bit_for_bit() {
    let series: Vec<Vec<f64>> = vec![
        autocorrelated_series(),
        (0..250)
            .map(|i| 0.001 + 0.0001 * ((i % 5) as f64 - 2.0))
            .collect(),
        (0..120)
            .map(|i| 0.02 + 0.1 * (i as f64 * 0.9).sin())
            .collect(),
        vec![0.012, -0.004, 0.009, 0.011, -0.002, 0.008, 0.010, -0.001],
        vec![0.01, -0.03],
        vec![0.0; 7],
        {
            let mut s = vec![0.004; 99];
            s.push(-0.3);
            s
        },
    ];
    for r in &series {
        for bench in [0.0, 0.05, -0.1, 0.3, sharpe_ratio(r)] {
            let kernel = probabilistic_sharpe_ratio(r, bench);
            let diag =
                probabilistic_sharpe_ratio_autocorrelated(r, bench, 0.0, StandardErrorAt::Observed)
                    .unwrap();
            assert_eq!(diag.to_bits(), kernel.to_bits(), "bench {bench} on {r:?}");
        }
        let sigma = 0.5_f64.sqrt() / 250.0_f64.sqrt();
        let bar = expected_max_sharpe(sigma, 100).unwrap();
        let dsr = deflated_sharpe_ratio_against_null(r, 100, 0.0, sigma).unwrap();
        let diag =
            probabilistic_sharpe_ratio_autocorrelated(r, bar, 0.0, StandardErrorAt::Observed)
                .unwrap();
        assert_eq!(diag.to_bits(), dsr.to_bits());
    }
}

/// Independent Python on the autocorrelated series: the lag-one
/// autocorrelation and eq. 2's bracket to 1e-12, and the PSR (Python using the
/// same Abramowitz-Stegun CDF) to 1e-12. Positive autocorrelation widens the
/// variance and lowers the PSR, as eq. 5 says it should (p. 11).
#[test]
fn autocorrelated_psr_matches_an_independent_implementation() {
    let r = autocorrelated_series();
    let rho = first_order_autocorrelation(&r).unwrap();
    assert!(close(rho, 0.367_138_720_497_330_9, 1e-12), "rho {rho}");

    let sr = sharpe_ratio(&r);
    assert!(close(sr, 0.066_323_101_546_266_36, 1e-14));
    let v = sharpe_variance_factor(sr, skewness(&r), kurtosis(&r), rho).unwrap();
    assert!(close(v, 2.161_455_784_040_627_7, 1e-12), "bracket {v}");

    let cases = [
        (0.0, rho, StandardErrorAt::Observed, 0.782_321_879_849_799_1),
        (
            0.0,
            rho,
            StandardErrorAt::Benchmark,
            0.782_385_921_473_477_6,
        ),
        (0.0, 0.0, StandardErrorAt::Observed, 0.874_165_075_390_393_8),
        (
            0.0,
            0.0,
            StandardErrorAt::Benchmark,
            0.874_274_751_411_166_4,
        ),
        (
            0.05,
            rho,
            StandardErrorAt::Observed,
            0.576_122_622_992_134_8,
        ),
        (
            0.05,
            rho,
            StandardErrorAt::Benchmark,
            0.576_131_790_375_532_2,
        ),
        (
            0.05,
            0.0,
            StandardErrorAt::Observed,
            0.611_075_049_100_592_3,
        ),
        (0.05, 0.0, StandardErrorAt::Benchmark, 0.611_096_850_704_257),
    ];
    for (bench, rho, at, want) in cases {
        let got = probabilistic_sharpe_ratio_autocorrelated(&r, bench, rho, at).unwrap();
        assert!(
            close(got, want, 1e-12),
            "{bench} {rho} {at:?}: {got} vs {want}"
        );
    }
    let independent =
        probabilistic_sharpe_ratio_autocorrelated(&r, 0.0, 0.0, StandardErrorAt::Observed).unwrap();
    let aware =
        probabilistic_sharpe_ratio_autocorrelated(&r, 0.0, rho, StandardErrorAt::Observed).unwrap();
    assert!(aware < independent);
}

/// Every weight of eq. 2 is exactly 1 at `rho = 0`, so the bracket is the
/// kernel's `1 - g3 SR + (g4-1)/4 SR^2`; positive rho raises every weight.
#[test]
fn variance_factor_weights_follow_equation_two() {
    let (sr, g3, g4) = (0.3, -1.2, 7.5);
    let iid = 1.0 - g3 * sr + ((g4 - 1.0) / 4.0) * sr * sr;
    assert_eq!(sharpe_variance_factor(sr, g3, g4, 0.0), Ok(iid));
    let rho: f64 = 0.4;
    let want = (1.0 + rho) / (1.0 - rho) - (1.0 + rho + rho * rho) / (1.0 - rho * rho) * g3 * sr
        + (1.0 + rho * rho) / (1.0 - rho * rho) * (g4 - 1.0) / 4.0 * sr * sr;
    assert!(close(
        sharpe_variance_factor(sr, g3, g4, rho).unwrap(),
        want,
        1e-12
    ));
    // Normal i.i.d. at SR = 0: the bracket is 1, the variance 1/T.
    assert_eq!(sharpe_variance_factor(0.0, 0.0, 3.0, 0.0), Ok(1.0));
    // Normal AR(1) at SR = 0: (1 + rho) / (1 - rho), LLZ eq. 60 (p. 40).
    assert_eq!(sharpe_variance_factor(0.0, 0.0, 3.0, 0.5), Ok(3.0));
}

// ---------------------------------------------------------------------------
// 2. Standard error under the null
// ---------------------------------------------------------------------------

/// When the observed Sharpe equals the benchmark, the two evaluations put the
/// same Sharpe into the same variance, so the standard errors coincide bit for
/// bit, and so do the PSRs (both one half, up to the frozen CDF's 1e-9 at
/// zero). Away from that point the standard errors and the PSRs differ.
#[test]
fn null_and_observed_standard_errors_coincide_at_the_benchmark() {
    let r = autocorrelated_series();
    let sr = sharpe_ratio(&r);
    let rho_hat = first_order_autocorrelation(&r).unwrap();
    for rho in [0.0, rho_hat, -0.3] {
        let se = |bench: f64, at| sharpe_standard_error_autocorrelated(&r, bench, rho, at).unwrap();
        assert_eq!(
            se(sr, StandardErrorAt::Observed).to_bits(),
            se(sr, StandardErrorAt::Benchmark).to_bits(),
            "rho {rho}"
        );
        let psr = |at| probabilistic_sharpe_ratio_autocorrelated(&r, sr, rho, at).unwrap();
        let (observed, null) = (
            psr(StandardErrorAt::Observed),
            psr(StandardErrorAt::Benchmark),
        );
        assert_eq!(observed.to_bits(), null.to_bits(), "rho {rho}");
        assert!(close(observed, 0.5, 1e-8));

        // The observed evaluation ignores the benchmark; the null one moves with it.
        assert_eq!(
            se(0.3, StandardErrorAt::Observed),
            se(sr, StandardErrorAt::Observed)
        );
        assert_ne!(
            se(0.3, StandardErrorAt::Benchmark),
            se(sr, StandardErrorAt::Benchmark)
        );
        assert_ne!(
            probabilistic_sharpe_ratio_autocorrelated(&r, 0.3, rho, StandardErrorAt::Observed),
            probabilistic_sharpe_ratio_autocorrelated(&r, 0.3, rho, StandardErrorAt::Benchmark)
        );
    }
    // The standard error is the one the PSR divides by, with T - 1.
    let bracket = sharpe_variance_factor(sr, skewness(&r), kurtosis(&r), rho_hat).unwrap();
    assert_eq!(
        sharpe_standard_error_autocorrelated(&r, 0.0, rho_hat, StandardErrorAt::Observed),
        Ok(bracket.sqrt() / ((r.len() - 1) as f64).sqrt())
    );
}

/// Under the null evaluation at `SR_0 = 0` the skewness and kurtosis terms
/// vanish, whatever the sample moments are: eq. 2's bracket multiplies `g3` by
/// `SR` and `(g4 - 1)/4` by `SR^2`, so at `SR = 0` only the autocorrelation
/// weight `(1 + rho)/(1 - rho)` survives. This is why "the PSR penalizes
/// negative skew and fat tails" is a statement about the kernel's evaluation
/// at the observed Sharpe (eq. 3) and not about the PSR in general.
///
/// The moments here are the deflated-Sharpe worked example's, `g3 = -3` and
/// `g4 = 10`, which are nowhere near the values that would make the bracket 1
/// by coincidence: at a Sharpe of 0.2 the same moments give 1.69.
#[test]
fn the_null_evaluation_at_zero_drops_the_skewness_and_kurtosis_terms() {
    let (g3, g4) = (-3.0, 10.0);
    for rho in [0.0, 0.4, -0.4] {
        let want = (1.0 + rho) / (1.0 - rho);
        assert_eq!(
            sharpe_variance_factor(0.0, g3, g4, rho),
            Ok(want),
            "rho {rho}"
        );
        // Nothing else about the moments is being asserted: away from zero the
        // same call does depend on them.
        assert_ne!(sharpe_variance_factor(0.2, g3, g4, rho), Ok(want));
    }
    assert_eq!(
        sharpe_variance_factor(0.2, g3, g4, 0.0),
        Ok(1.0 - g3 * 0.2 + ((g4 - 1.0) / 4.0) * 0.2 * 0.2)
    );

    // Consequence for the statistic: on a sharply skewed, fat-tailed series the
    // null-evaluated PSR at a zero benchmark is the plain Phi(SR sqrt(T - 1))
    // a Normal sample would give, while the kernel's PSR is moved by the
    // moments and is not.
    let mut r = vec![0.004_f64; 99];
    r.push(-0.3);
    assert!(skewness(&r) < -3.0 && kurtosis(&r) > 10.0);
    let null = probabilistic_sharpe_ratio_autocorrelated(&r, 0.0, 0.0, StandardErrorAt::Benchmark)
        .unwrap();
    let z = sharpe_ratio(&r) * ((r.len() - 1) as f64).sqrt();
    assert_eq!(null.to_bits(), norm_cdf(z).to_bits());
    assert!(!close(probabilistic_sharpe_ratio(&r, 0.0), null, 1e-6));
}

/// A zero benchmark with Normal returns does not make the two coincide: under
/// the null the bracket is 1, at the observed Sharpe it is `1 + SR^2 / 2`. The
/// null's standard error is the smaller, so its PSR is the larger for a
/// positive Sharpe.
#[test]
fn a_zero_benchmark_does_not_make_the_evaluations_coincide() {
    let sr = 0.4;
    assert_eq!(sharpe_variance_factor(0.0, 0.0, 3.0, 0.0), Ok(1.0));
    assert_eq!(
        sharpe_variance_factor(sr, 0.0, 3.0, 0.0),
        Ok(1.0 + sr * sr / 2.0)
    );
    let r = autocorrelated_series();
    let observed =
        probabilistic_sharpe_ratio_autocorrelated(&r, 0.0, 0.0, StandardErrorAt::Observed).unwrap();
    let null = probabilistic_sharpe_ratio_autocorrelated(&r, 0.0, 0.0, StandardErrorAt::Benchmark)
        .unwrap();
    assert!(null > observed, "{null} vs {observed}");
}

// ---------------------------------------------------------------------------
// 3. Manipulation-proof performance measure
// ---------------------------------------------------------------------------

/// A riskless stream earning `c` every period scores its own continuously
/// compounded return, `ln(1 + c) / dt`, whatever the risk aversion (GISW p. 18:
/// Theta is the certainty equivalent).
#[test]
fn a_riskless_stream_scores_its_log_return() {
    for rho in [0.5, 1.0, 2.0, DEFAULT_MPPM_RISK_AVERSION, 4.0, 10.0] {
        for (c, ppy) in [(0.001, 252.0), (0.01, 12.0), (-0.02, 1.0), (0.0, 52.0)] {
            let got = manipulation_proof_performance(&[c; 10], rho, ppy).unwrap();
            let want = c.ln_1p() * ppy;
            assert!(close(got, want, 1e-12), "rho {rho} c {c}: {got} vs {want}");
        }
    }
    assert!(close(
        manipulation_proof_performance(&[0.001; 10], 3.0, 252.0).unwrap(),
        0.251_874_083_937_025_65,
        1e-12
    ));
    // A flat track scores exactly +0.0, never -0.0, at every risk aversion.
    for rho in [0.5, 1.0, 3.0] {
        let flat = manipulation_proof_performance(&[0.0; 10], rho, 252.0).unwrap();
        assert_eq!(flat.to_bits(), 0.0_f64.to_bits(), "rho {rho}");
    }
}

/// Log returns `m + s` and `m - s` with equal weight give the closed form
/// `m + ln(cosh((1 - rho) s)) / (1 - rho)` per period; and the independent
/// Python on the autocorrelated series agrees to 1e-12.
#[test]
fn mppm_matches_closed_forms_and_an_independent_implementation() {
    let (m, s): (f64, f64) = (0.004, 0.03);
    let two = [(m + s).exp_m1(), (m - s).exp_m1()];
    for rho in [2.0, 3.0, 4.0, 0.5] {
        let k = 1.0 - rho;
        let want = m + (k * s).cosh().ln() / k;
        let got = manipulation_proof_performance(&two, rho, 1.0).unwrap();
        assert!(close(got, want, 1e-14), "rho {rho}: {got} vs {want}");
    }
    let r = autocorrelated_series();
    for (rho, ppy, want) in [
        (3.0, 252.0, 0.105_514_305_744_089_78),
        (1.0, 1.0, 0.000_476_588_797_870_323_04),
        (2.0, 12.0, 0.005_371_780_811_591_666),
    ] {
        let got = manipulation_proof_performance(&r, rho, ppy).unwrap();
        assert!(close(got, want, 1e-12), "rho {rho}: {got} vs {want}");
    }
}

/// The rho = 1 branch is the limit of the general formula: one ULP-sized step
/// of risk aversion either side lands within 1e-9 of it.
#[test]
fn log_utility_is_the_limit_of_the_power_form() {
    let r = autocorrelated_series();
    let at_one = manipulation_proof_performance(&r, 1.0, 252.0).unwrap();
    let mean_log = r.iter().map(|x| x.ln_1p()).sum::<f64>() / r.len() as f64 * 252.0;
    assert!(close(at_one, mean_log, 1e-15));
    for rho in [1.0 - 1e-7, 1.0 + 1e-7] {
        let near = manipulation_proof_performance(&r, rho, 252.0).unwrap();
        assert!(close(near, at_one, 1e-6), "rho {rho}: {near} vs {at_one}");
    }
}

/// GISW's point (introduction and section 1): selling tail risk raises the
/// Sharpe ratio without adding skill. A short-volatility-like stream that
/// collects 1.5% in 99 periods and loses 50% in one has a higher Sharpe, and a
/// higher mean, than a symmetric stream of +5.8% / -4.2%, but a lower MPPM at
/// the recommended risk aversion of 3 and across the paper's 2 to 4 range.
#[test]
fn short_volatility_raises_the_sharpe_but_not_the_mppm() {
    let mut short_vol = vec![0.015; 99];
    short_vol.push(-0.5);
    let symmetric: Vec<f64> = (0..100)
        .map(|i| if i % 2 == 0 { 0.058 } else { -0.042 })
        .collect();

    let (sv_sr, sym_sr) = (sharpe_ratio(&short_vol), sharpe_ratio(&symmetric));
    assert!(close(sv_sr, 0.191_262_135_922_330_07, 1e-12));
    assert!(close(sym_sr, 0.159_197_989_937_059_16, 1e-12));
    assert!(sv_sr > sym_sr);
    assert!(mean(&short_vol) > mean(&symmetric));

    for (rho, sv_want, sym_want) in [
        (2.0, 0.004_641_296_042_200_973, 0.005_504_662_775_654_250_5),
        (
            3.0,
            -0.000_477_337_656_570_842_44,
            0.004_275_936_316_497_879,
        ),
        (4.0, -0.008_800_732_676_449_725, 0.003_053_224_309_525),
    ] {
        let sv = manipulation_proof_performance(&short_vol, rho, 1.0).unwrap();
        let sym = manipulation_proof_performance(&symmetric, rho, 1.0).unwrap();
        assert!(close(sv, sv_want, 1e-12), "short vol at {rho}: {sv}");
        assert!(close(sym, sym_want, 1e-12), "symmetric at {rho}: {sym}");
        assert!(sv < sym, "rho {rho}: short vol {sv} >= symmetric {sym}");
    }
}

/// The bound of the test above. GISW's property 2 is that an uninformed
/// investor cannot *expect* to raise his *estimated* score, at a `rho` chosen so
/// that holding the benchmark is optimal (their eq. 19), which this kernel does
/// not solve for. On a realized finite sample at an off-the-shelf `rho`, the
/// same tail-selling stream beats the symmetric one two ways, so the book must
/// not claim the measure cannot be raised by selling tail risk.
///
/// 1. Below `rho` about 1.6444 the ordering is reversed even with the tail
///    realized, including at `rho = 1`, the geometric-average measure GISW list
///    as unmanipulable against dynamic manipulation (p. 17).
/// 2. `Theta` is a sample average, so a tail that does not land in the sample is
///    invisible to it at every `rho`.
#[test]
fn mppm_does_not_order_tail_selling_last_at_every_rho_or_sample() {
    let mut short_vol = vec![0.015; 99];
    short_vol.push(-0.5);
    let symmetric: Vec<f64> = (0..100)
        .map(|i| if i % 2 == 0 { 0.058 } else { -0.042 })
        .collect();

    // The ordering flips between these two risk aversions, so the pin is the
    // bracket rather than a property that holds on one side of it.
    for (rho, sv_want, sym_want) in [
        (1.0, 0.007_808_254_563_213_695, 0.006_736_416_212_415_563),
        (1.5, 0.006_410_991_723_567_494_5, 0.006_120_349_841_980_698),
    ] {
        let sv = manipulation_proof_performance(&short_vol, rho, 1.0).unwrap();
        let sym = manipulation_proof_performance(&symmetric, rho, 1.0).unwrap();
        assert!(close(sv, sv_want, 1e-12), "short vol at {rho}: {sv}");
        assert!(close(sym, sym_want, 1e-12), "symmetric at {rho}: {sym}");
        assert!(sv > sym, "rho {rho}: short vol {sv} <= symmetric {sym}");
    }
    let sv_18 = manipulation_proof_performance(&short_vol, 1.8, 1.0).unwrap();
    let sym_18 = manipulation_proof_performance(&symmetric, 1.8, 1.0).unwrap();
    assert!(close(sv_18, 0.005_400_176_121_709_164, 1e-12), "{sv_18}");
    assert!(
        close(sym_18, 0.005_750_867_847_539_066_5, 1e-12),
        "{sym_18}"
    );
    assert!(sv_18 < sym_18);

    // The same 99 collecting periods with the loss outside the sample.
    let unrealized = vec![0.015; 99];
    for rho in [0.5, 1.0, 3.0, 4.0] {
        let sv = manipulation_proof_performance(&unrealized, rho, 1.0).unwrap();
        let sym = manipulation_proof_performance(&symmetric, rho, 1.0).unwrap();
        assert!(
            close(sv, 0.014_888_612_493_750_552, 1e-12),
            "rho {rho}: {sv}"
        );
        assert!(sv > sym, "rho {rho}: {sv} <= {sym}");
    }

    // A fatter premium flips the ordering at the recommended risk aversion.
    let mut richer = vec![0.025; 99];
    richer.push(-0.5);
    let sv_3 = manipulation_proof_performance(&richer, 3.0, 1.0).unwrap();
    let sym_3 = manipulation_proof_performance(&symmetric, 3.0, 1.0).unwrap();
    assert!(close(sv_3, 0.008_931_166_804_293_074, 1e-12), "{sv_3}");
    assert!(sv_3 > sym_3, "{sv_3} <= {sym_3}");
}

/// Concavity (GISW p. 17): a mean-preserving spread cannot raise the measure,
/// and more return in any one period always raises it ("arbitrage is good").
#[test]
fn mppm_is_increasing_and_penalizes_a_mean_preserving_spread() {
    // Each outcome of `base` is split into itself plus and minus 2%: the same
    // mean, and more risk in the Rothschild-Stiglitz sense.
    let base = [0.01, 0.02, -0.01, 0.005, 0.0];
    let doubled: Vec<f64> = base.iter().flat_map(|&b| [b, b]).collect();
    let spread: Vec<f64> = base.iter().flat_map(|&b| [b + 0.02, b - 0.02]).collect();
    assert!(close(mean(&doubled), mean(&spread), 1e-15));
    assert!(std_dev(&spread) > std_dev(&doubled));
    for rho in [0.5, 1.0, 2.0, DEFAULT_MPPM_RISK_AVERSION, 4.0] {
        let b = manipulation_proof_performance(&doubled, rho, 12.0).unwrap();
        let s = manipulation_proof_performance(&spread, rho, 12.0).unwrap();
        assert!(s < b, "rho {rho}: spread {s} >= base {b}");
        let mut better = doubled.clone();
        better[4] += 0.001;
        assert!(manipulation_proof_performance(&better, rho, 12.0).unwrap() > b);
    }
}

/// A deep loss with a large risk aversion would overflow `(1 + x)^(1 - rho)`
/// computed directly; the log-space average keeps it finite and correct.
#[test]
fn mppm_is_finite_where_the_direct_power_overflows() {
    let r: [f64; 2] = [1e-9 - 1.0 + 1e-3, 0.01];
    let rho = 200.0;
    assert!((1.0 + r[0]).powf(1.0 - rho).is_infinite());
    let got = manipulation_proof_performance(&r, rho, 1.0).unwrap();
    let l0 = r[0].ln_1p();
    let want = (l0 * (1.0 - rho) + (0.5_f64).ln()) / (1.0 - rho);
    assert!(close(got, want, 1e-9), "{got} vs {want}");
}

// ---------------------------------------------------------------------------
// Paired-boundary tests: the edges of every documented domain.
// ---------------------------------------------------------------------------

#[test]
fn sharpe_variance_factor_boundary_inputs() {
    let rho_err = Err(StatisticalError::InvalidParameter {
        name: "rho",
        requirement: "must be finite and in (-1, 1)",
    });
    for bad in [
        1.0,
        -1.0,
        1.5,
        -1.5,
        f64::NAN,
        f64::INFINITY,
        f64::NEG_INFINITY,
    ] {
        assert_eq!(
            sharpe_variance_factor(0.1, 0.0, 3.0, bad),
            rho_err,
            "rho {bad}"
        );
    }
    // Just inside (-1, 1) is accepted.
    let edge = 1.0 - f64::EPSILON;
    assert!(sharpe_variance_factor(0.1, 0.0, 3.0, edge).unwrap() > 0.0);
    assert!(sharpe_variance_factor(0.0, 0.0, 3.0, -edge).unwrap() >= 0.0);
    for (name, args) in NON_FINITE_MOMENTS {
        assert_eq!(
            sharpe_variance_factor(args[0], args[1], args[2], 0.0),
            Err(StatisticalError::InvalidParameter {
                name,
                requirement: "must be finite",
            })
        );
    }
    // The Pearson bound g4 = 1 + g3^2 at SR = 2 / g3 makes the i.i.d. bracket
    // exactly zero: accepted, it is the edge. A strongly negative rho with
    // skewed returns drives it negative: refused, not floored.
    assert_eq!(sharpe_variance_factor(-1.0, -2.0, 5.0, 0.0), Ok(0.0));
    assert_eq!(
        sharpe_variance_factor(0.5, 3.0, 10.0, -0.9),
        Err(StatisticalError::InvalidParameter {
            name: "sharpe_variance",
            requirement: "must be non-negative: the moments and autocorrelation are inconsistent",
        })
    );
    // Kurtosis below the Pearson bound is inconsistent at rho = 0 too.
    assert!(sharpe_variance_factor(1.0, 0.0, -10.0, 0.0).is_err());
}

#[test]
fn first_order_autocorrelation_boundary_inputs() {
    assert_eq!(
        first_order_autocorrelation(&[]),
        Err(StatisticalError::InsufficientObservations {
            required: 2,
            actual: 0
        })
    );
    assert_eq!(
        first_order_autocorrelation(&[0.01]),
        Err(StatisticalError::InsufficientObservations {
            required: 2,
            actual: 1
        })
    );
    // Two points are the smallest sample: x and -x, so -1/2.
    assert_eq!(first_order_autocorrelation(&[0.01, 0.03]), Ok(-0.5));
    assert_eq!(
        first_order_autocorrelation(&[0.02; 5]),
        Err(StatisticalError::InvalidParameter {
            name: "returns",
            requirement: "must not be constant: a constant series has no autocorrelation",
        })
    );
    for bad in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        assert_eq!(
            first_order_autocorrelation(&[0.01, bad, 0.02]),
            Err(StatisticalError::NonFiniteObservation { index: 1 })
        );
    }
    // Finite returns whose squares overflow are a computation, not a number.
    assert_eq!(
        first_order_autocorrelation(&[1e300, -1e300, 1e300]),
        Err(StatisticalError::NonFiniteComputation {
            quantity: "return sum of squares"
        })
    );
    // Alternating signs sit near the -1 edge but inside it.
    let alternating: Vec<f64> = (0..1000)
        .map(|i| if i % 2 == 0 { 0.01 } else { -0.01 })
        .collect();
    let rho = first_order_autocorrelation(&alternating).unwrap();
    assert!(rho > -1.0 && rho < -0.99, "{rho}");
}

#[test]
fn probabilistic_sharpe_ratio_autocorrelated_boundary_inputs() {
    let r = autocorrelated_series();
    let at = StandardErrorAt::Observed;
    for bad in [1.0, -1.0, f64::NAN, f64::INFINITY] {
        assert_eq!(
            probabilistic_sharpe_ratio_autocorrelated(&r, 0.0, bad, at),
            Err(StatisticalError::InvalidParameter {
                name: "rho",
                requirement: "must be finite and in (-1, 1)",
            })
        );
    }
    for bad in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        assert_eq!(
            probabilistic_sharpe_ratio_autocorrelated(&r, bad, 0.0, at),
            Err(StatisticalError::InvalidParameter {
                name: "sr_benchmark",
                requirement: "must be finite",
            })
        );
        assert_eq!(
            probabilistic_sharpe_ratio_autocorrelated(&[0.01, bad], 0.0, 0.0, at),
            Err(StatisticalError::NonFiniteObservation { index: 1 })
        );
    }
    for short in [&[][..], &[0.01][..]] {
        assert_eq!(
            probabilistic_sharpe_ratio_autocorrelated(short, 0.0, 0.0, at),
            Err(StatisticalError::InsufficientObservations {
                required: 2,
                actual: short.len()
            })
        );
    }
    // Two observations are the smallest accepted sample, and the result is a
    // probability in [0, 1] at both evaluations.
    for at in [StandardErrorAt::Observed, StandardErrorAt::Benchmark] {
        let p = probabilistic_sharpe_ratio_autocorrelated(&[0.01, 0.03], 0.0, 0.0, at).unwrap();
        assert!((0.0..=1.0).contains(&p));
    }
    // A negative variance is refused, not floored into a PSR of zero or one:
    // strongly positive skew, a strongly negative rho and a benchmark near the
    // bracket's minimum (about 0.1 per period here) take it below zero.
    let mut skewed = vec![0.004; 99];
    skewed.push(0.3);
    let refusal = Err(StatisticalError::InvalidParameter {
        name: "sharpe_variance",
        requirement: "must be non-negative: the moments and autocorrelation are inconsistent",
    });
    let at = StandardErrorAt::Benchmark;
    assert_eq!(
        probabilistic_sharpe_ratio_autocorrelated(&skewed, 0.1, -0.9, at),
        refusal
    );
    assert_eq!(
        sharpe_standard_error_autocorrelated(&skewed, 0.1, -0.9, at),
        refusal
    );
}

#[test]
fn sharpe_standard_error_autocorrelated_boundary_inputs() {
    let r = autocorrelated_series();
    for at in [StandardErrorAt::Observed, StandardErrorAt::Benchmark] {
        for bad in [1.0, -1.0, f64::NAN] {
            assert!(sharpe_standard_error_autocorrelated(&r, 0.0, bad, at).is_err());
        }
        assert_eq!(
            sharpe_standard_error_autocorrelated(&r, f64::NAN, 0.0, at),
            Err(StatisticalError::InvalidParameter {
                name: "sr_benchmark",
                requirement: "must be finite",
            })
        );
        assert_eq!(
            sharpe_standard_error_autocorrelated(&[0.01], 0.0, 0.0, at),
            Err(StatisticalError::InsufficientObservations {
                required: 2,
                actual: 1
            })
        );
        // Two observations: T - 1 = 1, so the error is the bracket's root.
        let two = [0.01, 0.03];
        let sr = sharpe_ratio(&two);
        let bench = if at == StandardErrorAt::Observed {
            0.0
        } else {
            sr
        };
        let bracket = bracket_of(&two, sr, 0.0);
        assert_eq!(
            sharpe_standard_error_autocorrelated(&two, bench, 0.0, at),
            Ok(bracket.sqrt())
        );
    }
    // A bracket of zero takes the same 1e-12 floor as the kernel's PSR.
    let edge = [0.0, 0.0, 0.0, 0.0, 1.0];
    let zero_at = 2.0 / skewness(&edge);
    let bracket = bracket_of(&edge, zero_at, 0.0);
    assert!(bracket.abs() < 1e-12, "{bracket}");
    assert_eq!(
        sharpe_standard_error_autocorrelated(&edge, zero_at, 0.0, StandardErrorAt::Benchmark),
        Ok(1e-12_f64.sqrt() / 2.0)
    );
}

#[test]
fn manipulation_proof_performance_boundary_inputs() {
    let r = [0.01, -0.02, 0.03];
    for bad in [0.0, -0.0, -3.0, f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        assert_eq!(
            manipulation_proof_performance(&r, bad, 252.0),
            Err(StatisticalError::InvalidParameter {
                name: "risk_aversion",
                requirement: "must be finite and positive",
            }),
            "risk aversion {bad}"
        );
        assert_eq!(
            manipulation_proof_performance(&r, 3.0, bad),
            Err(StatisticalError::InvalidParameter {
                name: "periods_per_year",
                requirement: "must be finite and positive",
            }),
            "periods per year {bad}"
        );
    }
    // The smallest positive risk aversion and frequency are accepted.
    assert!(manipulation_proof_performance(&r, f64::MIN_POSITIVE, 1.0).is_ok());
    assert!(manipulation_proof_performance(&r, 3.0, f64::MIN_POSITIVE).is_ok());
    assert_eq!(
        manipulation_proof_performance(&[], 3.0, 252.0),
        Err(StatisticalError::InsufficientObservations {
            required: 1,
            actual: 0
        })
    );
    // One observation is the smallest sample.
    assert!(close(
        manipulation_proof_performance(&[0.01], 3.0, 1.0).unwrap(),
        0.01_f64.ln_1p(),
        1e-15
    ));
    // A gross return of zero or below is outside the domain; just above it is in.
    for bad in [-1.0, -1.5] {
        assert_eq!(
            manipulation_proof_performance(&[0.01, bad], 3.0, 1.0),
            Err(StatisticalError::InvalidParameter {
                name: "returns",
                requirement: "must each exceed -1: a gross return must be positive",
            })
        );
    }
    let near_ruin = manipulation_proof_performance(&[0.01, -1.0 + 1e-12], 3.0, 1.0).unwrap();
    assert!(near_ruin.is_finite() && near_ruin < -10.0, "{near_ruin}");
    for bad in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        assert_eq!(
            manipulation_proof_performance(&[0.01, bad], 3.0, 1.0),
            Err(StatisticalError::NonFiniteObservation { index: 1 })
        );
    }
}
