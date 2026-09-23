//! The interval and the run-count derivation checked against values computed
//! outside this crate.
//!
//! The Wilson cases are the ones published in the interval-estimation
//! literature and reproduced in standard references (Brown, Cai and DasGupta,
//! "Interval Estimation for a Binomial Proportion", Statistical Science 16(2),
//! 2001; Newcombe, Statistics in Medicine 17, 1998). They are quoted to four
//! decimals there, which is the tolerance asserted below. The zero-event case
//! is also derived in closed form in this file's comments, so a reader can
//! check it without the reference.

use sharpebench_study::protocol::ConfidenceLevel;
use sharpebench_study::refusal::PrecisionError;
use sharpebench_study::{required_simulation_runs, wilson_interval};

/// Four-decimal agreement with a published table.
const TABLE_TOLERANCE: f64 = 5e-5;

fn assert_close(actual: f64, expected: f64, tolerance: f64, what: &str) {
    assert!(
        (actual - expected).abs() <= tolerance,
        "{what}: got {actual}, expected {expected} within {tolerance}"
    );
}

/// The two-sided normal quantiles are pinned constants, so they are checked
/// against the published values rather than against another computation in
/// this crate.
#[test]
fn pinned_normal_quantiles_match_published_values() {
    for (level, expected) in [
        (ConfidenceLevel::Ninety, 1.644_854),
        (ConfidenceLevel::NinetyFive, 1.959_964),
        (ConfidenceLevel::NinetyNine, 2.575_829),
    ] {
        assert_close(
            level.two_sided_z(),
            expected,
            1e-6,
            &format!("z for {}", level.tag()),
        );
    }
}

/// 50 of 100 at 95 percent: the Wilson interval is (0.4038, 0.5962).
#[test]
fn wilson_interval_matches_the_published_half_sample_case() {
    let interval =
        wilson_interval(50, 100, ConfidenceLevel::NinetyFive).expect("a computable interval");
    assert_close(interval.lower, 0.4038, TABLE_TOLERANCE, "lower bound");
    assert_close(interval.upper, 0.5962, TABLE_TOLERANCE, "upper bound");
}

/// 15 of 20 at 95 percent: the Wilson interval is (0.5313, 0.8881). An
/// asymmetric case, which a symmetric Wald-style bug would fail.
#[test]
fn wilson_interval_matches_the_published_asymmetric_case() {
    let interval =
        wilson_interval(15, 20, ConfidenceLevel::NinetyFive).expect("a computable interval");
    assert_close(interval.lower, 0.5313, TABLE_TOLERANCE, "lower bound");
    assert_close(interval.upper, 0.8881, TABLE_TOLERANCE, "upper bound");
}

/// 0 of 20 at 95 percent: the Wilson interval is (0, 0.1611). This is the
/// rare-event case the crate exists for. In closed form, at x = 0 the centre
/// and the spread are both z^2 / (2(n + z^2)), so the lower bound is exactly
/// zero and the upper bound is z^2 / (n + z^2) = 3.84146 / 23.84146 = 0.16113.
/// A Wald interval would report (0, 0), a false certainty.
#[test]
fn wilson_interval_at_zero_events_keeps_a_positive_upper_bound() {
    let interval =
        wilson_interval(0, 20, ConfidenceLevel::NinetyFive).expect("a computable interval");
    // The closed form gives exactly zero; the computed difference of two
    // equal quantities lands within one floating-point step of it.
    assert_close(interval.lower, 0.0, 1e-12, "lower bound");
    assert_close(interval.upper, 0.1611, TABLE_TOLERANCE, "upper bound");
}

#[test]
fn a_wider_confidence_level_gives_a_wider_interval() {
    let narrow = wilson_interval(50, 100, ConfidenceLevel::Ninety).expect("computable");
    let wide = wilson_interval(50, 100, ConfidenceLevel::NinetyNine).expect("computable");
    assert!(
        wide.half_width() > narrow.half_width(),
        "99 percent half width {} should exceed the 90 percent {}",
        wide.half_width(),
        narrow.half_width()
    );
}

#[test]
fn an_interval_needs_trials_and_cannot_have_more_events_than_trials() {
    assert_eq!(
        wilson_interval(0, 0, ConfidenceLevel::NinetyFive),
        Err(PrecisionError::NoTrials)
    );
    assert_eq!(
        wilson_interval(11, 10, ConfidenceLevel::NinetyFive),
        Err(PrecisionError::EventsExceedTrials {
            events: 11,
            trials: 10
        })
    );
}

/// At a zero anticipated rate the half width is z^2 / (2(n + z^2)) exactly, so
/// requiring 0.01 needs n + z^2 >= z^2 / 0.02 = 192.073, that is n >= 188.23,
/// so 189 runs. Derived by hand from the closed form above, not from the code
/// under test.
#[test]
fn required_runs_at_a_zero_rate_matches_the_closed_form() {
    let runs = required_simulation_runs(0.0, 0.01, ConfidenceLevel::NinetyFive)
        .expect("the requirement is reachable");
    assert_eq!(runs, 189);
}

/// Whatever the rate, the returned count is the smallest one that meets the
/// requirement: it meets it and the count below it does not.
#[test]
fn required_runs_is_the_smallest_count_that_meets_the_requirement() {
    for (rate, half_width) in [(0.0, 0.01), (0.05, 0.02), (0.5, 0.05), (0.8, 0.03)] {
        let level = ConfidenceLevel::NinetyFive;
        let runs = required_simulation_runs(rate, half_width, level).expect("reachable");
        assert!(runs >= 1);

        let events_at = |n: u64| ((rate * n as f64).round() as u64).min(n);
        let achieved = wilson_interval(events_at(runs), runs, level)
            .expect("computable")
            .half_width();
        assert!(
            achieved <= half_width,
            "rate {rate}: {runs} runs give {achieved}, wider than the required {half_width}"
        );

        let below = runs - 1;
        let achieved_below = wilson_interval(events_at(below), below, level)
            .expect("computable")
            .half_width();
        assert!(
            achieved_below > half_width,
            "rate {rate}: {below} runs already give {achieved_below}, so {runs} is not minimal"
        );
    }
}

#[test]
fn a_tighter_requirement_needs_more_runs() {
    let level = ConfidenceLevel::NinetyFive;
    let loose = required_simulation_runs(0.05, 0.02, level).expect("reachable");
    let tight = required_simulation_runs(0.05, 0.01, level).expect("reachable");
    assert!(
        tight > loose,
        "halving the half width should not cost fewer runs: {tight} vs {loose}"
    );
}

#[test]
fn a_nonsense_rate_or_half_width_is_refused() {
    assert_eq!(
        required_simulation_runs(1.5, 0.01, ConfidenceLevel::NinetyFive),
        Err(PrecisionError::InvalidParameter {
            name: "anticipated_rate",
            requirement: "must be finite and in [0, 1]",
        })
    );
    assert_eq!(
        required_simulation_runs(0.05, 0.0, ConfidenceLevel::NinetyFive),
        Err(PrecisionError::InvalidParameter {
            name: "required_half_width",
            requirement: "must be finite and in (0, 1)",
        })
    );
}
