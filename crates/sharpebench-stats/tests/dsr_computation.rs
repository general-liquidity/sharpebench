//! Finite inputs do not imply finite intermediate estimates.
use sharpebench_stats::deflated_sharpe::{
    deflated_sharpe_ratio, deflated_sharpe_ratio_against_null, expected_max_sharpe,
    probabilistic_sharpe_ratio,
};
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
    for returns in [
        vec![],
        vec![0.01],
        vec![0.01; 8],
        vec![0.0; 8],
        vec![-0.01, 0.02, 0.03, -0.04],
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
