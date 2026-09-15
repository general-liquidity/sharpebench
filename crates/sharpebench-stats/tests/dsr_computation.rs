//! Finite inputs do not imply finite intermediate estimates.
use sharpebench_stats::deflated_sharpe::{
    checked_probabilistic_sharpe_ratio, deflated_sharpe_ratio, deflated_sharpe_ratio_against_null,
    expected_max_sharpe, probabilistic_sharpe_ratio,
};
use sharpebench_stats::significance::bootstrap_dsr_ci;
use sharpebench_stats::stats::std_dev;
use sharpebench_stats::validation::StatisticalError;

fn refused(result: Result<f64, StatisticalError>) {
    assert!(
        matches!(result, Err(StatisticalError::NonFiniteComputation { .. })),
        "{result:?}"
    );
}

#[test]
fn finite_observations_with_overflowing_moments_are_not_scores() {
    refused(deflated_sharpe_ratio(&[f64::MAX; 3], 10, 0.5));
    refused(deflated_sharpe_ratio(&[1e200, -1e200, 1e200], 10, 0.5));
}

#[test]
fn finite_dispersion_cannot_hide_overflow_in_the_expected_maximum() {
    refused(expected_max_sharpe(f64::MAX, 500));
}

#[test]
fn finite_null_cannot_hide_overflow_in_the_benchmark_or_z_statistic() {
    let returns = [0.01, 0.02, 0.03];
    refused(deflated_sharpe_ratio_against_null(
        &returns,
        2,
        f64::MAX,
        f64::MAX,
    ));
    refused(deflated_sharpe_ratio_against_null(
        &returns,
        1,
        -f64::MAX,
        0.0,
    ));
}

#[test]
fn finite_checked_deflation_keeps_existing_operation_order_and_fallbacks() {
    // A constant track is not among the fallbacks: it has no Sharpe ratio, and
    // `a_constant_track_has_no_deflated_sharpe` below pins its refusal.
    for returns in [
        vec![],
        vec![0.01],
        vec![-0.01, 0.02, 0.03, -0.04],
        // Nonzero mean and asymmetric moments exercise the variance terms;
        // centered or constant fixtures cannot distinguish those operations.
        vec![-0.01, 0.02],
        vec![0.01, 0.02, 0.03, -0.01],
    ] {
        for (trials, dispersion) in [(1, 0.0), (2, 0.5), (500, 0.5)] {
            let benchmark = expected_max_sharpe(dispersion, trials).unwrap();
            let raw = probabilistic_sharpe_ratio(&returns, benchmark);
            assert_eq!(
                deflated_sharpe_ratio(&returns, trials, dispersion)
                    .unwrap()
                    .to_bits(),
                raw.to_bits()
            );
        }
    }
}

fn constant_track() -> StatisticalError {
    StatisticalError::InvalidParameter {
        name: "returns",
        requirement: "must not be constant: a constant series has no Sharpe ratio",
    }
}

/// Paper audit 2026-09-14, Tier 1 #10. A constant track has a sample variance
/// of zero and so no Sharpe ratio, yet the checked family scored one: an
/// all-zero track as a Sharpe of 0 (PSR 0.5000000005 against zero, and a
/// deflated Sharpe that moved with the deflation bar alone) and a constant
/// positive track as PSR and DSR 1.0. Each is now the typed refusal, on the
/// point estimates and on the interval that brackets them.
#[test]
fn a_constant_track_has_no_deflated_sharpe() {
    let zero = vec![0.0; 408];
    let positive = vec![0.001; 408];
    // Why the predicate is value equality and not a zero computed variance:
    // the rounded mean of 0.001 repeated is a few ULPs off 0.001, which leaves a
    // nonzero computed standard deviation and a Sharpe near 1e15.
    assert_eq!(std_dev(&zero), 0.0);
    assert!(std_dev(&positive) > 0.0 && std_dev(&positive) < 1e-17);
    for track in [zero, positive, vec![-0.002; 60], vec![0.0, 0.0]] {
        let n = track.len();
        assert_eq!(
            deflated_sharpe_ratio(&track, 8, 0.03),
            Err(constant_track()),
            "{n}"
        );
        assert_eq!(
            deflated_sharpe_ratio_against_null(&track, 1, 0.0, 0.0),
            Err(constant_track()),
            "{n}"
        );
        assert_eq!(
            checked_probabilistic_sharpe_ratio(&track, 0.0),
            Err(constant_track()),
            "{n}"
        );
        assert_eq!(
            bootstrap_dsr_ci(&track, 8, 0.03, 7, 200, 0.1, 0.9),
            Err(constant_track()),
            "{n}"
        );
    }
}

/// A non-constant track whose squared deviations all fall below the smallest
/// subnormal has a computed standard deviation of exactly zero. Its Sharpe is
/// not computable in this arithmetic, and it is refused as a non-finite Sharpe
/// rather than taken as a Sharpe of 0.
#[test]
fn a_track_whose_computed_variance_underflows_to_zero_is_refused() {
    let underflow = [1e-170, 2e-170, 1e-170, 2e-170];
    assert_eq!(std_dev(&underflow), 0.0);
    let sharpe = Err(StatisticalError::NonFiniteComputation {
        quantity: "Sharpe ratio",
    });
    assert_eq!(deflated_sharpe_ratio(&underflow, 8, 0.03), sharpe);
    assert_eq!(checked_probabilistic_sharpe_ratio(&underflow, 0.0), sharpe);
}

/// The refusal is not a tolerance. A low-volatility track, and a track that
/// differs from a constant in one observation, keep their numbers: bit for bit
/// the unchecked PSR at the deflation bar and at zero.
#[test]
fn a_dispersed_track_is_not_refused_however_small_its_dispersion() {
    let low_volatility: Vec<f64> = (0..408)
        .map(|i| 1e-4 + 1e-9 * ((i % 7) as f64 - 3.0))
        .collect();
    let mut one_off = vec![0.001; 408];
    one_off[17] = 0.0011;
    let bar = expected_max_sharpe(0.03, 8).unwrap();
    for track in [low_volatility, one_off] {
        assert_eq!(
            deflated_sharpe_ratio(&track, 8, 0.03).unwrap().to_bits(),
            probabilistic_sharpe_ratio(&track, bar).to_bits()
        );
        assert_eq!(
            checked_probabilistic_sharpe_ratio(&track, 0.0)
                .unwrap()
                .to_bits(),
            probabilistic_sharpe_ratio(&track, 0.0).to_bits()
        );
    }
}

/// A resample is not an observed track. A sparse track resampled inside its
/// flat stretch draws a constant series, which keeps the Sharpe of 0 it always
/// had, so the track keeps its interval. The bits are the ones this interval
/// had before constant tracks were refused.
#[test]
fn a_sparse_track_keeps_its_interval_when_a_resample_is_constant() {
    let mut sparse = vec![0.0; 20];
    sparse[0] = 0.01;
    sparse[11] = -0.004;
    let ci = bootstrap_dsr_ci(&sparse, 8, 0.03, 7, 200, 0.1, 0.9)
        .expect("a dispersed track keeps its interval");
    assert_eq!(ci.point.to_bits(), 0x3fe5_2bb3_3bc0_c9be, "point moved");
    assert_eq!(
        ci.lower.to_bits(),
        0x3f62_c39f_ba21_1b80,
        "lower bound moved"
    );
    assert_eq!(
        ci.upper.to_bits(),
        0x3fef_828e_ab12_cd88,
        "upper bound moved"
    );
    assert_eq!(
        ci.se.to_bits(),
        0x3fd4_7857_f8c9_9849,
        "standard error moved"
    );
}
