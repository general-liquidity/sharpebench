//! The conviction-weighted return weighs only confidences an agent stated.
//!
//! A run whose `confidences` are empty stated no conviction. Weighting it at 1.0,
//! the most conviction there is, would fill in a value the agent never gave.
//! Weighting it at zero drops it: a failing sentinel run, which states nothing,
//! would vanish from the mean, and an agent could replace the equal-weight mean
//! by stating one confidence in one run. Such a run therefore takes the mean
//! stated weight of the submission (the mean, over the runs that state one, of
//! each run's mean stated confidence). With no stated confidence anywhere,
//! every run weighs 1.0. Every value below except the sentinel example is a
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
fn a_run_with_no_stated_confidence_takes_the_mean_stated_weight() {
    // Run weights 0.25 and 0.75, so each unstated run weighs 0.5 and the
    // weights sum to 2: (0.25 * 0.03125 - 0.75 * 0.03125 + 2 * 0.5 * 0.375) / 2.
    // The first run states one pair and the second eight, and the mean is
    // taken over runs: a mean over pairs would give (0.25 + 8 * 0.75) / 9.
    let mut sparse = run(&GAIN, 4, None);
    sparse.confidences = vec![0.25];
    sparse.outcomes = vec![true];
    let cw = weighted(vec![
        sparse,
        run(&LOSS, 4, Some(0.75)),
        run(&BIG, 4, None),
        run(&BIG, 4, None),
    ]);
    assert_eq!(
        cw, 0.1796875,
        "the unstated runs must enter the weighted mean at the mean stated weight"
    );
}

#[test]
fn one_stated_confidence_does_not_replace_the_equal_weight_mean() {
    // One pair stated in the winning run. Every run then weighs 0.25, which
    // is the equal-weight mean of the run means, (0.03125 - 0.03125 + 0.375) / 3.
    let mut winning = run(&GAIN, 4, None);
    winning.confidences = vec![0.25];
    winning.outcomes = vec![true];
    let cw = weighted(vec![winning, run(&LOSS, 4, None), run(&BIG, 4, None)]);
    assert_eq!(cw, 0.125);
    assert_eq!(
        cw,
        weighted(vec![
            run(&GAIN, 4, None),
            run(&LOSS, 4, None),
            run(&BIG, 4, None)
        ])
    );
}

#[test]
fn a_failing_sentinel_run_stays_in_the_weighted_mean() {
    // Four runs at +0.001 stating 0.6, and the harness's failing sentinel: a
    // flat -0.01 drift with no confidences. Every run weighs 0.6, so the
    // result is the equal-weight mean (4 * 0.001 - 0.01) / 5 = -0.0012
    // in rationals, and the f64 result lies within 1e-15 of it. Weighting the
    // sentinel at zero gave +0.001, and at 1.0 gave -0.0076 / 3.4.
    let steady = || run(&[0.001], 40, Some(0.6));
    let sentinel = run(&[-0.01], 40, None);
    let cw = weighted(vec![steady(), steady(), steady(), steady(), sentinel]);
    assert!((cw - -0.0012).abs() < 1e-15, "{cw}");
    assert!(
        cw < 0.0,
        "a faulted run must not turn a losing submission positive"
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
