//! The conviction-weighted return weighs only confidences an agent stated.
//!
//! A run whose `confidences` are empty stated no conviction. Weighting it at 1.0,
//! the most conviction there is, would fill in a value the agent never gave, so it
//! carries no weight while any other run of the submission states one. With no
//! stated confidence anywhere, every run weighs the same. Every value below is a
//! short binary fraction, so each expected result is exact.

use sharpebench_core::{score_agent, AgentSubmission, Run, ScoreConfig};

fn run(pattern: &[f64], repeats: usize, confidence: Option<f64>) -> Run {
    let returns: Vec<f64> = pattern
        .iter()
        .copied()
        .cycle()
        .take(pattern.len() * repeats)
        .collect();
    let paired = returns.len();
    Run {
        confidences: confidence.map_or_else(Vec::new, |c| vec![c; paired]),
        outcomes: confidence.map_or_else(Vec::new, |_| returns.iter().map(|r| *r > 0.0).collect()),
        returns,
        ..Run::default()
    }
}

fn weighted(runs: Vec<Run>) -> f64 {
    score_agent(
        &AgentSubmission {
            agent_id: "agent".to_string(),
            runs,
            in_sample_trials: 0,
            candidates: Vec::new(),
        },
        &ScoreConfig::default(),
    )
    .confidence_weighted_return
}

/// Mean return 0.03125.
const GAIN: [f64; 2] = [0.0625, 0.0];
/// Mean return -0.03125.
const LOSS: [f64; 2] = [0.0, -0.0625];
/// Mean return 0.375.
const BIG: [f64; 2] = [0.5, 0.25];

#[test]
fn stated_runs_are_weighted_by_their_stated_conviction() {
    let cw = weighted(vec![run(&GAIN, 4, Some(0.25)), run(&LOSS, 4, Some(0.75))]);
    assert_eq!(cw, 0.25 * 0.03125 + 0.75 * -0.03125);
}

#[test]
fn a_run_with_no_stated_confidence_carries_no_weight() {
    let cw = weighted(vec![
        run(&GAIN, 4, Some(0.25)),
        run(&LOSS, 4, Some(0.75)),
        run(&BIG, 4, None),
    ]);
    assert_eq!(
        cw, -0.015625,
        "the unstated run must not enter the weighted mean at full conviction"
    );
}

#[test]
fn with_no_stated_confidence_every_run_weighs_the_same() {
    // Unequal run lengths, so the equal-weight mean of run means (0.203125)
    // differs from the mean of the pooled track (1.75 / 12).
    let cw = weighted(vec![run(&GAIN, 4, None), run(&BIG, 2, None)]);
    assert_eq!(cw, (0.03125 + 0.375) / 2.0);
}

#[test]
fn a_submission_with_no_stated_confidence_has_no_calibration() {
    let score = score_agent(
        &AgentSubmission {
            agent_id: "agent".to_string(),
            runs: vec![run(&GAIN, 4, None), run(&LOSS, 4, None)],
            in_sample_trials: 0,
            candidates: Vec::new(),
        },
        &ScoreConfig::default(),
    );
    assert_eq!(score.calibration_brier, None);
    assert_eq!(score.calibration_observations, 0);
}
