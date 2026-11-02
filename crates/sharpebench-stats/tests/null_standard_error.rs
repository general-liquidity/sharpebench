//! The deflation family under the null standard error
//! (`StandardErrorAt::Benchmark`): the bootstrap interval, the moment-credit
//! floor and the serial-correlation correction each evaluate their bracket at
//! the deflation benchmark, and each `_at` function with
//! `StandardErrorAt::Observed` is its plug-in namesake bit for bit.

use sharpebench_stats::deflated_sharpe::{
    deflated_sharpe_ratio_against_null, observed_sharpe_ratio,
};
use sharpebench_stats::null_standard_error::deflated_sharpe_ratio_against_null_at;
use sharpebench_stats::significance::{
    bootstrap_dsr_ci_against_null, bootstrap_dsr_ci_against_null_at,
};
use sharpebench_stats::stats::{kurtosis, norm_cdf, skewness};
use sharpebench_stats::{
    certify_under_serial_correlation, certify_under_serial_correlation_at,
    certify_without_moment_credit, certify_without_moment_credit_at,
    deflated_sharpe_without_moment_credit, deflated_sharpe_without_moment_credit_at,
    expected_max_sharpe, serially_corrected_deflated_sharpe, serially_corrected_deflated_sharpe_at,
    sharpe_variance_factor, StandardErrorAt,
};

const NULL: StandardErrorAt = StandardErrorAt::Benchmark;
const PLUG_IN: StandardErrorAt = StandardErrorAt::Observed;

/// Mostly small losses with a large gain every tenth bar: positively skewed,
/// so its bracket takes credit, and a Sharpe well above a small deflation bar.
fn skewed() -> Vec<f64> {
    (0..120)
        .map(|t| {
            if t % 10 == 9 {
                0.06
            } else {
                -0.002 + 0.0005 * (t as f64).sin()
            }
        })
        .collect()
}

/// Two windows of a slow wave plus a fast wobble: strongly autocorrelated
/// within each window, with a positive drift.
fn autocorrelated() -> Vec<Vec<f64>> {
    (0..2)
        .map(|w| {
            (0..150)
                .map(|t| {
                    let t = t as f64 + 150.0 * w as f64;
                    0.003 + 0.01 * (0.05 * t).sin() + 0.002 * (1.7 * t).sin()
                })
                .collect()
        })
        .collect()
}

fn bar(n_trials: u32, null: f64, sr_std: f64) -> f64 {
    null + expected_max_sharpe(sr_std, n_trials).unwrap()
}

fn z_probability(sr: f64, sr_star: f64, n: usize, variance: f64) -> f64 {
    norm_cdf((sr - sr_star) * (n as f64 - 1.0).sqrt() / variance.max(1e-12).sqrt())
}

#[test]
fn the_observed_evaluation_is_each_plug_in_function_bit_for_bit() {
    let xs = skewed();
    let windows = autocorrelated();
    let slices: Vec<&[f64]> = windows.iter().map(Vec::as_slice).collect();
    let (n, null, sr_std) = (50, 0.001, 0.03);
    assert_eq!(
        deflated_sharpe_without_moment_credit_at(&xs, n, null, sr_std, PLUG_IN)
            .unwrap()
            .to_bits(),
        deflated_sharpe_without_moment_credit(&xs, n, null, sr_std)
            .unwrap()
            .to_bits()
    );
    assert_eq!(
        serially_corrected_deflated_sharpe_at(&slices, n, null, sr_std, PLUG_IN).unwrap(),
        serially_corrected_deflated_sharpe(&slices, n, null, sr_std).unwrap()
    );
    assert_eq!(
        bootstrap_dsr_ci_against_null_at(&xs, n, null, sr_std, 3, 300, 0.1, 0.9, PLUG_IN).unwrap(),
        bootstrap_dsr_ci_against_null(&xs, n, null, sr_std, 3, 300, 0.1, 0.9).unwrap()
    );
    for dsr_bar in [0.1, 0.5, 0.9, 0.99] {
        assert_eq!(
            certify_without_moment_credit_at(&xs, n, null, sr_std, dsr_bar, PLUG_IN),
            certify_without_moment_credit(&xs, n, null, sr_std, dsr_bar)
        );
        assert_eq!(
            certify_under_serial_correlation_at(&slices, n, null, sr_std, dsr_bar, PLUG_IN),
            certify_under_serial_correlation(&slices, n, null, sr_std, dsr_bar)
        );
    }
}

/// The interval brackets the null-SE deflated Sharpe, its point is that
/// statistic bit for bit, and it moves with the evaluation.
#[test]
fn the_bootstrap_interval_brackets_the_null_standard_error_statistic() {
    let xs = skewed();
    let (n, null, sr_std) = (50, 0.0, 0.03);
    let ci =
        bootstrap_dsr_ci_against_null_at(&xs, n, null, sr_std, 9, 400, 0.1, 0.9, NULL).unwrap();
    let point = deflated_sharpe_ratio_against_null_at(&xs, n, null, sr_std, NULL).unwrap();
    assert_eq!(ci.point.to_bits(), point.to_bits());
    assert!(ci.lower <= ci.upper && ci.se >= 0.0);
    let plug_in = bootstrap_dsr_ci_against_null(&xs, n, null, sr_std, 9, 400, 0.1, 0.9).unwrap();
    assert_ne!(ci, plug_in);
    assert_ne!(
        point.to_bits(),
        deflated_sharpe_ratio_against_null(&xs, n, null, sr_std)
            .unwrap()
            .to_bits()
    );
}

/// Under the null evaluation the Gaussian floor is `1 + SR_0^2 / 2` at the
/// deflation benchmark, applied where the skewness narrows the bracket there.
#[test]
fn the_moment_credit_floor_is_the_gaussian_bracket_at_the_benchmark() {
    let xs = skewed();
    let (n, null, sr_std) = (50, 0.0, 0.03);
    let sr_star = bar(n, null, sr_std);
    let sr = observed_sharpe_ratio(&xs).unwrap();
    let (g3, g4) = (skewness(&xs), kurtosis(&xs));
    let bracket = sharpe_variance_factor(sr_star, g3, g4, 0.0).unwrap();
    let floor = 1.0 + sr_star * sr_star / 2.0;
    assert!(
        bracket < floor,
        "the bracket must take credit at the benchmark"
    );
    // The floor at the observed Sharpe is a different number, so a floor read
    // at the wrong point is visible.
    assert!(1.0 + sr * sr / 2.0 > floor * 1.01);

    let floored = deflated_sharpe_without_moment_credit_at(&xs, n, null, sr_std, NULL).unwrap();
    assert_eq!(
        floored.to_bits(),
        z_probability(sr, sr_star, xs.len(), floor).to_bits()
    );
    let kernel = deflated_sharpe_ratio_against_null_at(&xs, n, null, sr_std, NULL).unwrap();
    assert!(kernel > floored, "{kernel} vs {floored}");

    // Refused only between the two: above the gate's own value the gate
    // refuses, below the floored one the floor holds.
    let between = (kernel + floored) / 2.0;
    assert!(certify_without_moment_credit_at(&xs, n, null, sr_std, between, NULL).is_err());
    assert_eq!(
        certify_without_moment_credit_at(&xs, n, null, sr_std, kernel + 1e-9, NULL),
        Ok(())
    );
    assert_eq!(
        certify_without_moment_credit_at(&xs, n, null, sr_std, floored - 1e-9, NULL),
        Ok(())
    );
}

/// Under the null evaluation the long-run variance ratio multiplies the
/// bracket at the deflation benchmark.
#[test]
fn the_serial_correction_multiplies_the_bracket_at_the_benchmark() {
    let windows = autocorrelated();
    let slices: Vec<&[f64]> = windows.iter().map(Vec::as_slice).collect();
    let pooled: Vec<f64> = windows.concat();
    let (n, null, sr_std) = (50, 0.0, 0.03);
    let sr_star = bar(n, null, sr_std);
    let corrected = serially_corrected_deflated_sharpe_at(&slices, n, null, sr_std, NULL).unwrap();
    assert!(corrected.applied_ratio > 1.0, "{corrected:?}");
    let sr = observed_sharpe_ratio(&pooled).unwrap();
    let bracket =
        sharpe_variance_factor(sr_star, skewness(&pooled), kurtosis(&pooled), 0.0).unwrap();
    assert_eq!(
        corrected.deflated_sharpe.to_bits(),
        z_probability(sr, sr_star, pooled.len(), bracket * corrected.applied_ratio).to_bits()
    );
    let plug_in = serially_corrected_deflated_sharpe(&slices, n, null, sr_std).unwrap();
    assert_eq!(plug_in.applied_ratio, corrected.applied_ratio);
    assert_ne!(
        plug_in.deflated_sharpe.to_bits(),
        corrected.deflated_sharpe.to_bits()
    );

    let independent =
        deflated_sharpe_ratio_against_null_at(&pooled, n, null, sr_std, NULL).unwrap();
    assert!(independent > corrected.deflated_sharpe);
    let between = (independent + corrected.deflated_sharpe) / 2.0;
    assert!(certify_under_serial_correlation_at(&slices, n, null, sr_std, between, NULL).is_err());
    assert_eq!(
        certify_under_serial_correlation_at(&slices, n, null, sr_std, independent + 1e-9, NULL),
        Ok(())
    );
    assert_eq!(
        certify_under_serial_correlation_at(
            &slices,
            n,
            null,
            sr_std,
            corrected.deflated_sharpe - 1e-9,
            NULL
        ),
        Ok(())
    );
}
