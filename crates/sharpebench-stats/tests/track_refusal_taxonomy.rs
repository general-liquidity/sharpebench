//! A gate's refusal of a track's shape is told apart from a statistic that
//! could not be computed by the typed requirement, never by the rendered text.

use sharpebench_stats::deflated_sharpe::{observed_sharpe_ratio, observed_sharpe_ratio_of_windows};
use sharpebench_stats::exposure_path::exposure_path_evidence;
use sharpebench_stats::{
    StatisticalError, CONSTANT_TRACK, EXPOSURE_PATH_UNPROVEN, NO_OBSERVED_LOSS, NO_RESOLVED_MOTION,
    NO_WINDOW_VARIES,
};

fn requirement(error: StatisticalError) -> &'static str {
    match error {
        StatisticalError::InvalidParameter { requirement, .. } => requirement,
        other => panic!("expected a requirement, got {other:?}"),
    }
}

#[test]
fn every_shape_refusal_the_gates_raise_classifies_as_a_refusal() {
    let alternating: Vec<f64> = (0..40)
        .map(|i| if i % 2 == 0 { 0.003 } else { -0.001 })
        .collect();
    let gains_only: Vec<f64> = (0..40)
        .map(|i| if i % 2 == 0 { 0.003 } else { 0.001 })
        .collect();
    let level_a = [0.001; 30];
    let level_b = [0.002; 30];
    let perturbed: Vec<f64> = (0..30)
        .map(|i| {
            if i % 2 == 0 {
                0.001 + 1e-8
            } else {
                0.001 - 1e-8
            }
        })
        .collect();
    let losing: Vec<f64> = (0..100)
        .map(|i| if i < 50 { 0.01 } else { -0.01 })
        .collect();
    let cases: [(&str, StatisticalError, &str); 5] = [
        (
            "constant",
            observed_sharpe_ratio(&[0.001; 10]).unwrap_err(),
            CONSTANT_TRACK,
        ),
        (
            "every window flat",
            observed_sharpe_ratio_of_windows(&[&level_a, &level_b]).unwrap_err(),
            NO_WINDOW_VARIES,
        ),
        (
            "no observed risk",
            observed_sharpe_ratio_of_windows(&[&perturbed]).unwrap_err(),
            NO_RESOLVED_MOTION,
        ),
        (
            "no observed loss",
            observed_sharpe_ratio_of_windows(&[&gains_only]).unwrap_err(),
            NO_OBSERVED_LOSS,
        ),
        (
            "exposure path",
            exposure_path_evidence(&[&losing], 0.05).unwrap_err(),
            EXPOSURE_PATH_UNPROVEN,
        ),
    ];
    for (label, error, expected) in cases {
        assert_eq!(requirement(error), expected, "{label}");
        assert!(error.is_track_refusal(), "{label}");
    }
    assert!(observed_sharpe_ratio_of_windows(&[&alternating]).is_ok());
}

#[test]
fn unavailability_does_not_classify_as_a_refusal() {
    let cases = [
        observed_sharpe_ratio(&[0.001]).unwrap_err(),
        observed_sharpe_ratio(&[0.001, f64::NAN]).unwrap_err(),
        exposure_path_evidence(&[&[0.0; 8]], 0.05).unwrap_err(),
        exposure_path_evidence(&[&[0.01, -0.01, 0.02, -0.01]], f64::NAN).unwrap_err(),
        // A refusal wording under another parameter name is not a track refusal.
        StatisticalError::InvalidParameter {
            name: "alpha",
            requirement: CONSTANT_TRACK,
        },
        StatisticalError::InvalidParameter {
            name: "returns",
            requirement: "must be finite",
        },
    ];
    for error in cases {
        assert!(!error.is_track_refusal(), "{error:?}");
    }
}
