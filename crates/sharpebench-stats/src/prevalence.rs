//! The share of truly skilled tracks behind an observed eligible share.
//!
//! A gate that admits a skilled track with probability `sensitivity` (its
//! measured power) and a zero-skill track with probability `false_eligibility`
//! (one minus its specificity) admits, in a field whose true share of skilled
//! tracks is `pi`, an expected share
//!
//! ```text
//! apparent = pi * sensitivity + (1 - pi) * false_eligibility.
//! ```
//!
//! Solving for `pi` gives the estimator of Rogan and Gladen (1978):
//!
//! ```text
//! pi = (apparent + specificity - 1) / (sensitivity + specificity - 1)
//!    = (apparent - false_eligibility) / (sensitivity - false_eligibility).
//! ```
//!
//! After Rogan and Gladen (1978), "Estimating prevalence from the results of a
//! screening test". The estimator exists only when the gate tells skill from
//! its absence at all, `sensitivity + specificity > 1`; at or below that line
//! the denominator is zero or negative and there is nothing to correct with, so
//! it is refused rather than reported. The raw estimate leaves `[0, 1]` when the
//! observed share is below the false-eligibility rate (it would be negative) or
//! above the power (it would exceed one); the reported estimate is clamped to
//! the nearer end and says which end it was clamped to.
//!
//! The estimator treats `sensitivity` and `false_eligibility` as known. They
//! are measured, so [`rogan_gladen_bounds`] carries an interval for each of the
//! three inputs through the formula: the corrected share is a linear-fractional
//! function of them with a positive denominator over the box, so its extremes
//! over the box lie at the box's corners. If each input interval covers its
//! true value with probability at least `1 - a`, the box covers all three at
//! once with probability at least `1 - 3a` (the union bound), and so does the
//! corrected interval.

use crate::StatisticalError;

/// The requirement a gate that cannot tell skill from its absence is refused on.
pub const NOT_INFORMATIVE: &str =
    "must exceed false_eligibility: a gate with sensitivity + specificity <= 1 tells skill from its absence no better than chance, so its eligible share cannot be corrected";

/// Which end of `[0, 1]` the raw estimate was clamped to, if any.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Clamp {
    /// The raw estimate was already in `[0, 1]`.
    None,
    /// The observed share was below the false-eligibility rate: raw below zero.
    Zero,
    /// The observed share was above the power: raw above one.
    One,
}

/// A Rogan-Gladen corrected share.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CorrectedPrevalence {
    /// `(apparent - false_eligibility) / (sensitivity - false_eligibility)`,
    /// unclamped.
    pub raw: f64,
    /// `raw` clamped to `[0, 1]`.
    pub estimate: f64,
    /// Which end `estimate` was clamped to.
    pub clamp: Clamp,
}

/// An interval for the corrected share.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PrevalenceBounds {
    pub lower: f64,
    pub upper: f64,
    /// False when the input box reaches a gate that cannot tell skill from its
    /// absence (the lowest sensitivity is at or below the highest
    /// false-eligibility rate): the formula is unbounded there and the interval
    /// is the whole of `[0, 1]`.
    pub informative: bool,
}

fn share(name: &'static str, x: f64) -> Result<f64, StatisticalError> {
    if x.is_finite() && (0.0..=1.0).contains(&x) {
        Ok(x)
    } else {
        Err(StatisticalError::InvalidParameter {
            name,
            requirement: "must be finite and in [0, 1]",
        })
    }
}

fn share_interval(
    name: &'static str,
    (lo, hi): (f64, f64),
) -> Result<(f64, f64), StatisticalError> {
    let (lo, hi) = (share(name, lo)?, share(name, hi)?);
    if lo > hi {
        return Err(StatisticalError::InvalidParameter {
            name,
            requirement: "interval must have lower <= upper",
        });
    }
    Ok((lo, hi))
}

fn clamped(raw: f64) -> (f64, Clamp) {
    if raw < 0.0 {
        (0.0, Clamp::Zero)
    } else if raw > 1.0 {
        (1.0, Clamp::One)
    } else {
        (raw, Clamp::None)
    }
}

/// The Rogan-Gladen corrected share of skilled tracks from an observed eligible
/// share `apparent`, the gate's power `sensitivity` and its false-eligibility
/// rate `false_eligibility` (one minus specificity), each finite and in
/// `[0, 1]`.
///
/// Refused with [`NOT_INFORMATIVE`] when `sensitivity <= false_eligibility`,
/// that is when `sensitivity + specificity <= 1`.
pub fn rogan_gladen(
    apparent: f64,
    sensitivity: f64,
    false_eligibility: f64,
) -> Result<CorrectedPrevalence, StatisticalError> {
    let apparent = share("apparent", apparent)?;
    let sensitivity = share("sensitivity", sensitivity)?;
    let false_eligibility = share("false_eligibility", false_eligibility)?;
    if sensitivity <= false_eligibility {
        return Err(StatisticalError::InvalidParameter {
            name: "sensitivity",
            requirement: NOT_INFORMATIVE,
        });
    }
    let raw = (apparent - false_eligibility) / (sensitivity - false_eligibility);
    let (estimate, clamp) = clamped(raw);
    Ok(CorrectedPrevalence {
        raw,
        estimate,
        clamp,
    })
}

/// An interval for the corrected share from `(lower, upper)` intervals for the
/// apparent share, the sensitivity and the false-eligibility rate, each bound
/// finite and in `[0, 1]`: the smallest and largest Rogan-Gladen value over the
/// eight corners of the box, each clamped to `[0, 1]`.
///
/// When the lowest sensitivity is at or below the highest false-eligibility
/// rate, some point of the box is a gate with `sensitivity + specificity <= 1`;
/// the formula is unbounded there, so the interval is `[0, 1]` and
/// `informative` is false. The point estimate is refused separately, by
/// [`rogan_gladen`], only when the point inputs themselves cross that line.
pub fn rogan_gladen_bounds(
    apparent: (f64, f64),
    sensitivity: (f64, f64),
    false_eligibility: (f64, f64),
) -> Result<PrevalenceBounds, StatisticalError> {
    let (p_lo, p_hi) = share_interval("apparent", apparent)?;
    let (se_lo, se_hi) = share_interval("sensitivity", sensitivity)?;
    let (fp_lo, fp_hi) = share_interval("false_eligibility", false_eligibility)?;
    if se_lo <= fp_hi {
        return Ok(PrevalenceBounds {
            lower: 0.0,
            upper: 1.0,
            informative: false,
        });
    }
    let mut lower = f64::INFINITY;
    let mut upper = f64::NEG_INFINITY;
    // inv: lower and upper are the least and greatest raw values over the
    // corners visited so far; every denominator is at least se_lo - fp_hi > 0.
    // term: a fixed eight corners.
    for p in [p_lo, p_hi] {
        for se in [se_lo, se_hi] {
            for fp in [fp_lo, fp_hi] {
                let raw = (p - fp) / (se - fp);
                lower = lower.min(raw);
                upper = upper.max(raw);
            }
        }
    }
    Ok(PrevalenceBounds {
        lower: clamped(lower).0,
        upper: clamped(upper).0,
        informative: true,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    // Dyadic inputs, so every difference below is exact and each estimate is
    // the one rounding of an exact quotient.

    #[test]
    fn corrected_share_matches_hand_computed_cases() {
        // (3/8 - 1/8) / (7/8 - 1/8) = (1/4) / (3/4) = 1/3.
        let c = rogan_gladen(0.375, 0.875, 0.125).unwrap();
        assert_eq!(c.raw, 1.0 / 3.0);
        assert_eq!(c.estimate, 1.0 / 3.0);
        assert_eq!(c.clamp, Clamp::None);
        // (1/2 - 1/4) / (3/4 - 1/4) = 1/2, and the same through the
        // specificity form (1/2 + 3/4 - 1) / (3/4 + 3/4 - 1).
        let c = rogan_gladen(0.5, 0.75, 0.25).unwrap();
        assert_eq!(c.raw, 0.5);
        assert_eq!(c.raw, (0.5 + 0.75 - 1.0) / (0.75 + 0.75 - 1.0));
        // A perfect gate leaves the share as observed.
        assert_eq!(rogan_gladen(0.3, 1.0, 0.0).unwrap().estimate, 0.3);
        // An apparent share equal to the false-eligibility rate is all false
        // positives, one equal to the power is all skilled.
        assert_eq!(rogan_gladen(0.125, 0.875, 0.125).unwrap().estimate, 0.0);
        assert_eq!(rogan_gladen(0.875, 0.875, 0.125).unwrap().estimate, 1.0);
    }

    #[test]
    fn rogan_gladen_boundaries() {
        // Below the false-eligibility rate: (1/16 - 1/8) / (3/4) = -1/12, clamped to 0.
        let c = rogan_gladen(0.0625, 0.875, 0.125).unwrap();
        assert_eq!(c.raw, -1.0 / 12.0);
        assert_eq!((c.estimate, c.clamp), (0.0, Clamp::Zero));
        // Above the power: (15/16 - 1/8) / (3/4) = 13/12, clamped to 1.
        let c = rogan_gladen(0.9375, 0.875, 0.125).unwrap();
        assert_eq!(c.raw, 13.0 / 12.0);
        assert_eq!((c.estimate, c.clamp), (1.0, Clamp::One));
        // Sensitivity + specificity = 1 and below: refused, never reported.
        for (se, fp) in [(0.5, 0.5), (0.25, 0.5), (0.0, 0.0), (1.0, 1.0)] {
            assert_eq!(
                rogan_gladen(0.5, se, fp),
                Err(StatisticalError::InvalidParameter {
                    name: "sensitivity",
                    requirement: NOT_INFORMATIVE,
                }),
                "se {se}, fp {fp}"
            );
        }
        // The smallest margin above the line is still corrected.
        let se = 0.5 + f64::EPSILON / 2.0;
        assert_eq!(rogan_gladen(0.5, se, 0.5).unwrap().estimate, 0.0);
        // The ends of [0, 1] are accepted; anything outside is refused by name.
        assert_eq!(rogan_gladen(0.0, 1.0, 0.0).unwrap().estimate, 0.0);
        assert_eq!(rogan_gladen(1.0, 1.0, 0.0).unwrap().estimate, 1.0);
        for (p, se, fp, name) in [
            (-0.0625, 0.875, 0.125, "apparent"),
            (1.0625, 0.875, 0.125, "apparent"),
            (f64::NAN, 0.875, 0.125, "apparent"),
            (0.5, 1.0625, 0.125, "sensitivity"),
            (0.5, f64::INFINITY, 0.125, "sensitivity"),
            (0.5, 0.875, -0.0625, "false_eligibility"),
            (0.5, 0.875, f64::NAN, "false_eligibility"),
        ] {
            assert_eq!(
                rogan_gladen(p, se, fp),
                Err(StatisticalError::InvalidParameter {
                    name,
                    requirement: "must be finite and in [0, 1]",
                })
            );
        }
    }

    #[test]
    fn bounds_are_the_extreme_corners_hand_computed() {
        // p in [1/4, 1/2], se in [3/4, 1], fp in [0, 1/8].
        // Lowest: p = 1/4, se = 1, fp = 1/8: (1/8) / (7/8) = 1/7.
        // Highest: p = 1/2, se = 3/4, fp = 0: (1/2) / (3/4) = 2/3.
        let b = rogan_gladen_bounds((0.25, 0.5), (0.75, 1.0), (0.0, 0.125)).unwrap();
        assert_eq!(
            b,
            PrevalenceBounds {
                lower: 1.0 / 7.0,
                upper: 2.0 / 3.0,
                informative: true,
            }
        );
        // A point box gives the point estimate at both ends.
        let b = rogan_gladen_bounds((0.375, 0.375), (0.875, 0.875), (0.125, 0.125)).unwrap();
        assert_eq!((b.lower, b.upper), (1.0 / 3.0, 1.0 / 3.0));
        // An apparent share above the power at the box's high end: the upper
        // corner (15/16 - 0) / (3/4) = 5/4 clamps to 1, the lower corner
        // (1/2 - 1/4) / (7/8 - 1/4) = 2/5 stays.
        let b = rogan_gladen_bounds((0.5, 0.9375), (0.75, 0.875), (0.0, 0.25)).unwrap();
        assert_eq!((b.lower, b.upper), (0.4, 1.0));
        // And below the false-eligibility rate at its low end:
        // (0 - 1/4) / (3/4 - 1/4) = -1/2 clamps to 0; (1/4 - 0) / (3/4) = 1/3.
        let b = rogan_gladen_bounds((0.0, 0.25), (0.75, 0.875), (0.0, 0.25)).unwrap();
        assert_eq!((b.lower, b.upper), (0.0, 1.0 / 3.0));
    }

    #[test]
    fn rogan_gladen_bounds_boundaries() {
        // The box touches sensitivity = false-eligibility: uninformative.
        let whole = PrevalenceBounds {
            lower: 0.0,
            upper: 1.0,
            informative: false,
        };
        assert_eq!(
            rogan_gladen_bounds((0.25, 0.5), (0.5, 1.0), (0.0, 0.5)).unwrap(),
            whole
        );
        assert_eq!(
            rogan_gladen_bounds((0.25, 0.5), (0.25, 1.0), (0.0, 0.5)).unwrap(),
            whole
        );
        // Just inside, the interval is informative.
        let se_lo = 0.5 + f64::EPSILON / 2.0;
        assert!(
            rogan_gladen_bounds((0.25, 0.5), (se_lo, 1.0), (0.0, 0.5))
                .unwrap()
                .informative
        );
        // Bad inputs are refused by name.
        assert_eq!(
            rogan_gladen_bounds((0.5, 0.25), (0.75, 1.0), (0.0, 0.125)),
            Err(StatisticalError::InvalidParameter {
                name: "apparent",
                requirement: "interval must have lower <= upper",
            })
        );
        assert_eq!(
            rogan_gladen_bounds((0.25, 0.5), (0.75, 1.5), (0.0, 0.125)),
            Err(StatisticalError::InvalidParameter {
                name: "sensitivity",
                requirement: "must be finite and in [0, 1]",
            })
        );
        assert_eq!(
            rogan_gladen_bounds((0.25, 0.5), (0.75, 1.0), (f64::NAN, 0.125)),
            Err(StatisticalError::InvalidParameter {
                name: "false_eligibility",
                requirement: "must be finite and in [0, 1]",
            })
        );
    }
}
