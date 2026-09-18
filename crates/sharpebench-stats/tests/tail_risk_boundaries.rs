//! Boundary and convention tests for the opt-in tail-risk diagnostics:
//! `tail_size`, `historical_expected_shortfall` and `loss_frequency`.
//!
//! The estimator is the expected shortfall of the empirical distribution:
//! with `T = n * level` and `m = floor(T)`, the sum of the `m` lowest returns
//! plus `(T - m)` times the next one, divided by `T`. The hand-computed values
//! below are that formula written out by hand for each sample.

use sharpebench_stats::stats::downside_deviation;
use sharpebench_stats::{
    historical_expected_shortfall, loss_frequency, tail_size, ExpectedShortfall, LossFrequency,
    StatisticalError, DEFAULT_MIN_TAIL_OBSERVATIONS, DEFAULT_TAIL_LEVEL,
    MIN_TAIL_OBSERVATIONS_FLOOR,
};

fn repeat(pattern: &[f64], times: usize) -> Vec<f64> {
    pattern
        .iter()
        .copied()
        .cycle()
        .take(pattern.len() * times)
        .collect()
}

/// Frequent small losses against rare large ones: equal mean and equal
/// downside deviation, expected shortfall a factor of two apart, loss
/// frequency a factor of four apart.
#[test]
fn downside_deviation_cannot_separate_what_expected_shortfall_does() {
    // The worked example: 0.02 lost on half the bars, 0.04 on one bar in
    // eight, each loss followed by an offsetting gain. The two downside
    // deviations agree to rounding.
    let often = repeat(&[-0.02, 0.02], 40);
    let rarely = repeat(&[-0.04, 0.04, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0], 10);
    assert_eq!(often.len(), rarely.len());
    assert_eq!(often.iter().sum::<f64>(), 0.0);
    assert_eq!(rarely.iter().sum::<f64>(), 0.0);
    let (dd_often, dd_rarely) = (
        downside_deviation(&often, 0.0),
        downside_deviation(&rarely, 0.0),
    );
    assert!(
        (dd_often - dd_rarely).abs() <= 1e-15,
        "{dd_often} {dd_rarely}"
    );
    let es_often = historical_expected_shortfall(&often, 0.125, 10).unwrap();
    let es_rarely = historical_expected_shortfall(&rarely, 0.125, 10).unwrap();
    assert_eq!(es_often.tail_size, 10.0);
    assert_eq!(es_rarely.tail_size, 10.0);
    assert_eq!(es_often.tail_observations, 10);
    assert!((es_often.tail_mean_return - -0.02).abs() <= 1e-15);
    assert!((es_rarely.tail_mean_return - -0.04).abs() <= 1e-15);
    assert_eq!(loss_frequency(&often).unwrap().frequency, 0.5);
    assert_eq!(loss_frequency(&rarely).unwrap().frequency, 0.125);

    // The same shape in powers of two, where every step is exact: the two
    // downside deviations are the same bits and the shortfalls are exact.
    let often = repeat(&[-0.015625, 0.015625], 40);
    let rarely = repeat(&[-0.03125, 0.03125, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0], 10);
    assert_eq!(
        downside_deviation(&often, 0.0).to_bits(),
        downside_deviation(&rarely, 0.0).to_bits()
    );
    assert_eq!(
        historical_expected_shortfall(&often, 0.125, 10)
            .unwrap()
            .tail_mean_return,
        -0.015625
    );
    assert_eq!(
        historical_expected_shortfall(&rarely, 0.125, 10)
            .unwrap()
            .tail_mean_return,
        -0.03125
    );
}

/// Ten returns at level 0.3: `T = 3`, the plain mean of the three lowest,
/// (-0.05 - 0.03 - 0.02) / 3.
#[test]
fn an_integer_tail_is_the_mean_of_its_observations() {
    let returns = [
        0.03, -0.05, 0.01, -0.02, 0.04, -0.01, 0.0, 0.02, -0.03, 0.05,
    ];
    let es = historical_expected_shortfall(&returns, 0.3, 3).unwrap();
    assert_eq!(es.tail_size, 3.0);
    assert_eq!(es.tail_observations, 3);
    assert!((es.tail_mean_return - -0.033_333_333_333_333_33).abs() <= 1e-15);
    // The next observation (-0.01) carries no weight: moving it leaves the
    // value unchanged.
    let mut moved = returns;
    moved[5] = -0.019;
    assert_eq!(
        historical_expected_shortfall(&moved, 0.3, 3)
            .unwrap()
            .tail_mean_return
            .to_bits(),
        es.tail_mean_return.to_bits()
    );
}

/// Fourteen returns at level 0.25: `T = 3.5`, `m = 3`, and the fourth lowest
/// enters with weight 0.5: (-0.04 - 0.03 - 0.02 + 0.5 * -0.01) / 3.5.
#[test]
fn a_non_integer_tail_weights_the_boundary_observation_by_its_fraction() {
    let returns = [
        0.02, -0.04, 0.01, -0.01, 0.03, -0.03, 0.0, 0.05, -0.02, 0.01, 0.02, -0.005, 0.04, 0.015,
    ];
    let es = historical_expected_shortfall(&returns, 0.25, 3).unwrap();
    assert_eq!(es.tail_size, 3.5);
    assert_eq!(es.tail_observations, 4);
    assert!((es.tail_mean_return - -0.027_142_857_142_857_14).abs() <= 1e-15);
    // Neither the rounded-down tail (three observations) nor the rounded-up
    // one (four) gives this value.
    assert!((es.tail_mean_return - -0.03).abs() > 1e-3);
    assert!((es.tail_mean_return - -0.025).abs() > 1e-3);
    // The boundary observation moves the value by half its own change.
    let mut moved = returns;
    moved[3] = -0.012;
    let shifted = historical_expected_shortfall(&moved, 0.25, 3).unwrap();
    assert!((shifted.tail_mean_return - es.tail_mean_return - 0.5 * -0.002 / 3.5).abs() <= 1e-15);
    // The observation after it does not move the value at all.
    let mut beyond = returns;
    beyond[11] = -0.009;
    assert_eq!(
        historical_expected_shortfall(&beyond, 0.25, 3)
            .unwrap()
            .tail_mean_return
            .to_bits(),
        es.tail_mean_return.to_bits()
    );
}

/// Tied observations are equal, so the tail they share has one value, and
/// the order of the input never matters, signed zeros included.
#[test]
fn ties_at_the_boundary_and_the_input_order_do_not_move_the_value() {
    let returns = [-0.01, 0.02, -0.03, -0.01, 0.0, -0.01, 0.04, -0.0, -0.01];
    // T = 4.5 with four -0.01 around the boundary: (-0.03 - 0.01 * 3 + 0.5 *
    // -0.01) / 4.5, whichever -0.01 is counted where.
    let es = historical_expected_shortfall(&returns, 0.5, 3).unwrap();
    assert_eq!(es.tail_size, 4.5);
    assert!((es.tail_mean_return - -0.065 / 4.5).abs() <= 1e-15);
    // T = 3: the tail is -0.03 and two of the four tied -0.01.
    let es3 = historical_expected_shortfall(&returns, 1.0 / 3.0, 3).unwrap();
    assert_eq!(es3.tail_size, 3.0);
    assert!((es3.tail_mean_return - -0.05 / 3.0).abs() <= 1e-15);
    for level in [0.5, 1.0 / 3.0, 0.4, 1.0] {
        let reference = historical_expected_shortfall(&returns, level, 3).unwrap();
        for shift in 0..returns.len() {
            let mut permuted = returns;
            permuted.rotate_left(shift);
            if shift % 2 == 1 {
                permuted.reverse();
            }
            let es = historical_expected_shortfall(&permuted, level, 3).unwrap();
            assert_eq!(
                es.tail_mean_return.to_bits(),
                reference.tail_mean_return.to_bits(),
                "level {level} shift {shift}"
            );
            assert_eq!(es, reference);
        }
    }
}

/// Level 1 is the whole sample: the tail mean is the sample mean.
#[test]
fn a_level_of_one_averages_every_observation() {
    let returns = [0.25, -0.5, 0.125, -0.0625, 0.1875];
    assert_eq!(
        historical_expected_shortfall(&returns, 1.0, 5),
        Ok(ExpectedShortfall {
            tail_mean_return: 0.0,
            tail_size: 5.0,
            tail_observations: 5,
        })
    );
    let returns = [0.5, -0.25, 0.75, 1.0];
    assert_eq!(
        historical_expected_shortfall(&returns, 1.0, 3)
            .unwrap()
            .tail_mean_return,
        0.5
    );
}

/// A decimal level whose stored value is off by one unit in the last place
/// still gives the integer tail its decimal value implies; a product that is
/// genuinely fractional is kept.
#[test]
fn tail_size_boundaries_snap_only_rounding_error() {
    assert_ne!(100.0 * 0.07, 7.0, "the raw product is not an integer");
    assert_eq!(tail_size(100, 0.07), Ok(7.0));
    let returns: Vec<f64> = (0..100_i32).map(|i| f64::from(i) * 0.001 - 0.05).collect();
    let es = historical_expected_shortfall(&returns, 0.07, 7).unwrap();
    assert_eq!(es.tail_size, 7.0);
    assert_eq!(es.tail_observations, 7);
    let worst_seven: f64 = returns[..7].iter().sum();
    assert_eq!(es.tail_mean_return, worst_seven / 7.0);

    assert_eq!(tail_size(25, 0.1), Ok(2.5));
    assert_eq!(tail_size(3, 0.5), Ok(1.5));
    assert_eq!(tail_size(14, 0.25), Ok(3.5));
    assert_eq!(tail_size(0, 0.5), Ok(0.0));
    assert_eq!(tail_size(7, 1.0), Ok(7.0));
    assert_eq!(tail_size(160, DEFAULT_TAIL_LEVEL), Ok(8.0));
    // Not near an integer, so kept as the product is.
    assert_eq!(
        tail_size(252, DEFAULT_TAIL_LEVEL),
        Ok(252.0 * DEFAULT_TAIL_LEVEL)
    );
    assert!((tail_size(252, DEFAULT_TAIL_LEVEL).unwrap() - 12.6).abs() < 1e-12);
    let tiny = tail_size(10, f64::MIN_POSITIVE).unwrap();
    assert!(tiny > 0.0 && tiny < 1e-300);
}

#[test]
fn level_boundaries_are_refused_by_name() {
    let returns = repeat(&[-0.01, 0.01], 50);
    for level in [
        0.0,
        -0.0,
        -0.05,
        1.0 + f64::EPSILON,
        2.0,
        f64::NAN,
        f64::INFINITY,
        f64::NEG_INFINITY,
    ] {
        let refused = Err(StatisticalError::InvalidParameter {
            name: "level",
            requirement: "must be finite and in (0, 1]",
        });
        assert_eq!(tail_size(100, level), refused, "{level}");
        assert_eq!(
            historical_expected_shortfall(&returns, level, 3).map(|_| ()),
            refused.map(|_| ()),
            "{level}"
        );
    }
    assert!(historical_expected_shortfall(&returns, 1.0, 3).is_ok());
    assert!(historical_expected_shortfall(&returns, 0.03, 3).is_ok());
}

/// The tail must hold the stated minimum of whole observations. At exactly
/// the minimum it is reported; one short, and even a fractional excess does
/// not count, it is refused with the whole count it had.
#[test]
fn minimum_tail_boundaries_refuse_a_short_tail() {
    let returns = repeat(&[-0.02, 0.01, 0.0, 0.03], 25);
    // T = 5 at the default level over 100 observations.
    assert!(historical_expected_shortfall(&returns, 0.05, 5).is_ok());
    assert_eq!(
        historical_expected_shortfall(&returns, 0.05, 6),
        Err(StatisticalError::InsufficientObservations {
            required: 6,
            actual: 5,
        })
    );
    // T = 5.9: five whole observations and a fraction of a sixth.
    assert!((tail_size(100, 0.059).unwrap() - 5.9).abs() < 1e-12);
    assert_eq!(
        historical_expected_shortfall(&returns, 0.059, 6),
        Err(StatisticalError::InsufficientObservations {
            required: 6,
            actual: 5,
        })
    );
    // The board default: 200 observations at 5% is exactly ten.
    let long = repeat(&[-0.02, 0.01, 0.0, 0.03], 50);
    assert_eq!(
        historical_expected_shortfall(&long, DEFAULT_TAIL_LEVEL, DEFAULT_MIN_TAIL_OBSERVATIONS)
            .unwrap()
            .tail_observations,
        10
    );
    let short = repeat(&[-0.02, 0.01, 0.0, 0.03], 49);
    assert_eq!(
        historical_expected_shortfall(&short, DEFAULT_TAIL_LEVEL, DEFAULT_MIN_TAIL_OBSERVATIONS),
        Err(StatisticalError::InsufficientObservations {
            required: 10,
            actual: 9,
        })
    );
    assert_eq!(
        historical_expected_shortfall(&[], 0.5, 3),
        Err(StatisticalError::InsufficientObservations {
            required: 3,
            actual: 0,
        })
    );
    // A minimum below the floor is refused whatever the sample.
    assert_eq!(MIN_TAIL_OBSERVATIONS_FLOOR, 3);
    for min in [0, 1, 2] {
        assert!(
            matches!(
                historical_expected_shortfall(&returns, 1.0, min),
                Err(StatisticalError::InvalidParameter {
                    name: "min_tail_observations",
                    ..
                })
            ),
            "{min}"
        );
    }
    assert!(historical_expected_shortfall(&returns[..3], 1.0, 3).is_ok());
    assert!(historical_expected_shortfall(&returns[..2], 1.0, 3).is_err());
}

#[test]
fn non_finite_boundaries_are_refused() {
    let mut returns = repeat(&[-0.02, 0.01], 10);
    returns[7] = f64::NAN;
    assert_eq!(
        historical_expected_shortfall(&returns, 0.5, 3),
        Err(StatisticalError::NonFiniteObservation { index: 7 })
    );
    assert_eq!(
        loss_frequency(&returns),
        Err(StatisticalError::NonFiniteObservation { index: 7 })
    );
    returns[7] = f64::NEG_INFINITY;
    assert_eq!(
        historical_expected_shortfall(&returns, 0.5, 3),
        Err(StatisticalError::NonFiniteObservation { index: 7 })
    );
    // Finite observations whose tail sum overflows.
    assert_eq!(
        historical_expected_shortfall(&[-f64::MAX; 4], 1.0, 3),
        Err(StatisticalError::NonFiniteComputation {
            quantity: "expected shortfall",
        })
    );
    assert_eq!(
        historical_expected_shortfall(&[f64::MAX, f64::MAX, f64::MAX], 1.0, 3),
        Err(StatisticalError::NonFiniteComputation {
            quantity: "expected shortfall",
        })
    );
}

/// A loss is strictly below zero; a zero of either sign is not one.
#[test]
fn loss_frequency_boundaries() {
    assert_eq!(
        loss_frequency(&[0.0, -0.0, -1e-300, 0.01, -0.02]),
        Ok(LossFrequency {
            losses: 2,
            observations: 5,
            frequency: 0.4,
        })
    );
    assert_eq!(loss_frequency(&[0.0, 0.01]).unwrap().frequency, 0.0);
    assert_eq!(loss_frequency(&[-0.01]).unwrap().frequency, 1.0);
    assert_eq!(
        loss_frequency(&[]),
        Err(StatisticalError::InsufficientObservations {
            required: 1,
            actual: 0,
        })
    );
    let rare = repeat(&[-0.04, 0.04, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0], 3);
    assert_eq!(
        loss_frequency(&rare).unwrap(),
        LossFrequency {
            losses: 3,
            observations: 24,
            frequency: 0.125,
        }
    );
}
