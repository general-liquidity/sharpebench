//! Opt-in tail-risk diagnostics that the gate and the ranking do not read.
//!
//! Downside deviation, the board's downside dispersion, is a root mean square
//! of shortfalls over every observation, so it mixes how often a track loses
//! with how much it loses when it does. A track losing 0.02 on half its bars
//! and one losing 0.04 on one bar in eight have the same downside deviation
//! (`0.5 * 0.02^2 = 0.125 * 0.04^2`), yet the average of their worst eighth
//! differs by a factor of two. [`historical_expected_shortfall`] reports that
//! severity with the count of observations behind it, and [`loss_frequency`]
//! reports the frequency. Nothing in `sharpebench-core`'s scoring,
//! eligibility or rank predicate calls this module.
//!
//! A sample tail only contains the losses that landed in the sample: a track
//! that sells insurance and has not yet paid out shows no tail here.

use crate::validation::{finite_computation, finite_observations, StatisticalError};

/// The tail level the board diagnostic reports at: the worst 5% of the
/// pooled observations.
pub const DEFAULT_TAIL_LEVEL: f64 = 0.05;

/// The fewest whole tail observations the board diagnostic reports an
/// expected shortfall from. Ten admits one 252-bar daily window at the default
/// level (a tail of 12.6 observations) and refuses a 100-bar one (5).
pub const DEFAULT_MIN_TAIL_OBSERVATIONS: usize = 10;

/// The smallest minimum [`historical_expected_shortfall`] accepts. The mean of
/// one or two observations is a data point, not an estimate of a tail.
pub const MIN_TAIL_OBSERVATIONS_FLOOR: usize = 3;

/// How far, relative to itself, a tail size may sit from an integer and still
/// be taken as that integer. A stored level differs from its decimal value by
/// at most half a unit in the last place, a relative error of at most
/// `f64::EPSILON / 2`, and the product with the observation count rounds once
/// more by at most as much.
const TAIL_SIZE_SNAP: f64 = 2.0 * f64::EPSILON;

/// The tail size `T = observations * level`: how many observations, possibly
/// fractional, the lower `level` tail of a sample of `observations` holds.
///
/// `level` is the tail fraction, not a confidence: it must be finite and in
/// `(0, 1]`, and 0.05 means the worst 5%. A product within
/// `2 * f64::EPSILON` of an integer, relative to itself, is that integer, so a
/// decimal level such as 0.07, which binary floating point stores slightly
/// above 0.07, gives 100 observations a tail of exactly 7 rather than
/// 7.000000000000001. Any other product is kept as it is.
pub fn tail_size(observations: usize, level: f64) -> Result<f64, StatisticalError> {
    if !level.is_finite() || level <= 0.0 || level > 1.0 {
        return Err(StatisticalError::InvalidParameter {
            name: "level",
            requirement: "must be finite and in (0, 1]",
        });
    }
    let size = observations as f64 * level;
    let whole = size.round();
    Ok(if (size - whole).abs() <= TAIL_SIZE_SNAP * size {
        whole
    } else {
        size
    })
}

/// A historical expected shortfall and the tail it was computed from.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ExpectedShortfall {
    /// The mean return over the lower tail, in the units of the returns. A
    /// return, not a positive loss: -0.04 means the tail lost 4% a period on
    /// average. Positive when even the worst observations gained.
    pub tail_mean_return: f64,
    /// `T`, the tail size from [`tail_size`], which the tail sum is divided by.
    pub tail_size: f64,
    /// The observations that enter the tail sum with a positive weight,
    /// `ceil(T)`: every whole one, and the one that straddles the boundary when
    /// `T` is not an integer.
    pub tail_observations: usize,
}

/// The historical expected shortfall of `returns` at tail fraction `level`:
/// the expected shortfall of the sample's empirical distribution, in which
/// each observation has probability `1/n`.
///
/// With `x_(1) <= ... <= x_(n)` the sorted returns, `T = tail_size(n, level)`
/// and `m = floor(T)`,
///
/// ```text
/// ES = ( x_(1) + ... + x_(m) + (T - m) * x_(m+1) ) / T
/// ```
///
/// At an integer `T` this is the plain mean of the `T` lowest returns. At a
/// non-integer `T` the observation that straddles the boundary enters with the
/// fraction `T - m` of it that lies inside the tail, so the tail carries
/// exactly probability `level`: the definition of Acerbi and Tasche (2002),
/// which weights an atom at the quantile this way, applied to the empirical
/// distribution. The value is continuous in `level`. Ties need no rule: tied
/// observations are equal, so which of them falls inside the tail or on its
/// boundary does not change the value, and any permutation of the input gives
/// the same bits.
///
/// Every return must be finite. `level` is as in [`tail_size`]: finite and in
/// `(0, 1]`, where 1 gives the mean of the whole sample.
/// `min_tail_observations` must be at least [`MIN_TAIL_OBSERVATIONS_FLOOR`].
/// The tail must hold at least that many whole observations, `m >= min`;
/// otherwise the estimate is refused with
/// `InsufficientObservations { required: min_tail_observations, actual: m }`
/// rather than computed from the few points there are.
pub fn historical_expected_shortfall(
    returns: &[f64],
    level: f64,
    min_tail_observations: usize,
) -> Result<ExpectedShortfall, StatisticalError> {
    finite_observations(returns)?;
    if min_tail_observations < MIN_TAIL_OBSERVATIONS_FLOOR {
        return Err(StatisticalError::InvalidParameter {
            name: "min_tail_observations",
            requirement:
                "must be at least 3: the mean of one or two observations is not a tail estimate",
        });
    }
    let size = tail_size(returns.len(), level)?;
    let whole = size.floor() as usize;
    if whole < min_tail_observations {
        return Err(StatisticalError::InsufficientObservations {
            required: min_tail_observations,
            actual: whole,
        });
    }
    let mut sorted = returns.to_vec();
    sorted.sort_by(f64::total_cmp);
    let full: f64 = sorted[..whole].iter().sum();
    let fraction = size - whole as f64;
    let boundary = if fraction > 0.0 {
        fraction * sorted[whole]
    } else {
        0.0
    };
    let tail_mean_return = finite_computation((full + boundary) / size, "expected shortfall")?;
    Ok(ExpectedShortfall {
        tail_mean_return,
        tail_size: size,
        tail_observations: size.ceil() as usize,
    })
}

/// How often a track loses.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LossFrequency {
    /// Observations strictly below zero. A zero return, of either sign, is not
    /// a loss.
    pub losses: usize,
    /// Every observation.
    pub observations: usize,
    /// `losses / observations`, in `[0, 1]`.
    pub frequency: f64,
}

/// The fraction of `returns` strictly below zero. Every return must be finite
/// and there must be at least one.
pub fn loss_frequency(returns: &[f64]) -> Result<LossFrequency, StatisticalError> {
    finite_observations(returns)?;
    if returns.is_empty() {
        return Err(StatisticalError::InsufficientObservations {
            required: 1,
            actual: 0,
        });
    }
    let losses = returns.iter().filter(|&&x| x < 0.0).count();
    Ok(LossFrequency {
        losses,
        observations: returns.len(),
        frequency: losses as f64 / returns.len() as f64,
    })
}
