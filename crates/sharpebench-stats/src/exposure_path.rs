//! Evidence measured against the risk an exposure history implies.
//!
//! A return series records what a strategy earned, not what it risked. A
//! strategy that sizes from its own profit and loss can therefore make a
//! no-skill series look like evidence: it trades until it is ahead and then
//! goes flat, or it shrinks exposure once ahead, so the bars that could have
//! taken the gain back are never drawn. Optional stopping of this kind is not
//! visible to a Sharpe ratio, which reads the dispersion of the bars that were
//! traded and so shrinks with the exposure the strategy stopped carrying.
//!
//! [`forgone_risk_z`] counts that risk. In each window the exposure rate is
//! the running mean square `V_t / t` of the returns so far, a quantity that does
//! not fall while the strategy keeps its exposure and falls as soon as it stops.
//! The window is charged its length times the largest rate it showed after its
//! first quarter, so going flat or de-risking leaves the charge where the
//! exposure history put it, while raising exposure raises it. The statistic is
//! the pooled profit over the square root of the pooled charge.
//!
//! **Why the threshold is `Phi^-1(1 - alpha / 2)`.** Under no edge, with
//! conditionally symmetric bar outcomes and exposure that never exceeds the
//! charged rate, a window's profit is a martingale stopped at some time no
//! later than its end. Levy's reflection principle bounds the chance that such
//! a walk ever reaches a level by twice the chance that it ends there:
//! `P(max_k S_k >= a) <= 2 P(S_n >= a)`. Halving alpha restores the gate's
//! size whatever the stopping rule. A strategy that keeps its exposure has a
//! charge near its realized variance, so the statistic is near its ordinary
//! t-statistic and the gate costs it only the factor of two in the tail.
//!
//! What it does not cover: a bar distribution that is skewed conditionally on
//! the past (a sold tail) is outside the symmetric case, and an exposure rate
//! below the first-quarter burn-in is not charged in full. Both are stated
//! limits, not claims.

use crate::stats::norm_ppf;
use crate::StatisticalError;

/// The requirement [`exposure_path_evidence`] refuses a track on.
///
/// This is a scoring change introduced after release 0.28.0 (grader-search
/// leverage-timing finding of 2026-10-30): a track whose profit does not clear
/// the risk its own exposure history implies is not rank-eligible.
pub const EXPOSURE_PATH_UNPROVEN: &str =
    "must clear the risk its exposure history implies: the profit, charged at the largest \
     exposure rate any window showed after its first quarter for the whole window, does not \
     clear the reflection-principle bound at alpha / 2, so stopping or de-risking once ahead, \
     or raising exposure after losses, could have produced it";

/// The fraction of a window that passes before its exposure rate is read.
///
/// The running mean square of the first few bars is one or two draws, and a
/// maximum over it would charge an honest track for its noisiest bar. One
/// quarter is the burn-in; a strategy that stops before it is still charged the
/// rate it showed over a quarter of the window, so its statistic is at most
/// half its ordinary t-statistic.
pub const EXPOSURE_BURN_IN_FRACTION: f64 = 0.25;

/// The pooled profit over the square root of the pooled forgone-risk charge,
/// or `None` when no window carried any exposure or the result is not finite.
///
/// For a window `w` of `n` bars, `t0 = max(1, ceil(n / 4))`,
/// `rate_w = max over t >= t0 of (sum of x_i^2 for i <= t) / t`, and the window
/// is charged `n * rate_w`. Scaling every return by `c > 0` scales the profit by
/// `c` and the charge by `c^2`, so the statistic is scale-free.
pub fn forgone_risk_z(windows: &[&[f64]]) -> Option<f64> {
    let mut profit = 0.0_f64;
    let mut charge = 0.0_f64;
    // inv: profit and charge are the sums over windows[..w] of each window's
    // profit and its charge n * rate.
    // term: w increases by 1 and is bounded by windows.len().
    for window in windows {
        let n = window.len();
        let burn_in = ((EXPOSURE_BURN_IN_FRACTION * n as f64).ceil() as usize).max(1);
        let mut squares = 0.0_f64;
        let mut rate = 0.0_f64;
        // inv: at the top of bar t (1-based), squares = sum of x_i^2 for i < t,
        // and rate = max of squares_u / u over burn_in <= u < t (0 if none).
        // term: t increases by 1 and is bounded by n.
        for (index, &x) in window.iter().enumerate() {
            let t = index + 1;
            squares += x * x;
            if t >= burn_in {
                rate = rate.max(squares / t as f64);
            }
            profit += x;
        }
        charge += n as f64 * rate;
    }
    let z = profit / charge.sqrt();
    (charge > 0.0 && z.is_finite()).then_some(z)
}

/// Refuse a track whose profit does not clear the risk its exposure history
/// implies at level `alpha`, one-sided, with the reflection principle's factor
/// of two: the bound is `Phi^-1(1 - alpha / 2)`. Returns the statistic when it
/// clears. The bound saturates: an alpha at or above 2 admits every statistic
/// and an alpha at or below 0 admits none.
pub fn exposure_path_evidence(windows: &[&[f64]], alpha: f64) -> Result<f64, StatisticalError> {
    // An alpha of 1 or more switches the bound off, as it switches off the
    // bootstrap gate `p < alpha`; only an alpha that is not a number is refused.
    if alpha.is_nan() {
        return Err(StatisticalError::InvalidParameter {
            name: "alpha",
            requirement: "must be a number",
        });
    }
    let z = forgone_risk_z(windows).ok_or(StatisticalError::NonFiniteComputation {
        quantity: "forgone-risk statistic",
    })?;
    if z >= norm_ppf(1.0 - alpha / 2.0) {
        Ok(z)
    } else {
        Err(StatisticalError::InvalidParameter {
            name: "returns",
            requirement: EXPOSURE_PATH_UNPROVEN,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The documented edges: no exposure and a non-finite result have no
    /// statistic, and the bound saturates at alpha 2 (admit all) and 0 (admit
    /// none).
    #[test]
    fn forgone_risk_and_the_alpha_bound_hold_at_their_boundary_values() {
        let flat: &[f64] = &[0.0; 8];
        assert_eq!(forgone_risk_z(&[flat]), None, "no exposure, no statistic");
        assert_eq!(forgone_risk_z(&[]), None, "no window, no statistic");
        let huge: &[f64] = &[f64::MAX, f64::MAX, f64::MAX, f64::MAX];
        assert_eq!(
            forgone_risk_z(&[huge]),
            None,
            "an overflowing charge is not finite"
        );
        let track: &[f64] = &[0.02, -0.01, 0.03, 0.01, -0.02, 0.04, 0.01, 0.02];
        let z = forgone_risk_z(&[track]).unwrap();
        assert!(z.is_finite());
        let scaled: Vec<f64> = track.iter().map(|x| x * 3.0).collect();
        assert!((forgone_risk_z(&[&scaled]).unwrap() - z).abs() < 1e-12);
        assert!(exposure_path_evidence(&[track], 2.0).is_ok());
        assert!(exposure_path_evidence(&[track], 0.0).is_err());
    }

    fn pm(n: usize, s: f64) -> Vec<f64> {
        (0..n).map(|i| if i % 3 == 2 { -s } else { s }).collect()
    }

    #[test]
    fn going_flat_keeps_the_charge_the_exposure_history_set() {
        // Three wins at 0.01 and nothing after: the ordinary t-statistic over the
        // traded bars is sqrt(3), and the charge is the whole window at 1e-4.
        let mut window = vec![0.01; 3];
        window.extend(vec![0.0; 9]);
        let z = forgone_risk_z(&[&window]).unwrap();
        assert!((z - 0.03 / (12.0_f64 * 1e-4).sqrt()).abs() < 1e-12, "{z}");
        // The same profit carried at the same exposure to the end is charged the same.
        let kept = pm(12, 0.01);
        let charged = forgone_risk_z(&[&kept]).unwrap();
        assert!((charged - 0.04 / (12.0_f64 * 1e-4).sqrt()).abs() < 1e-12);
    }

    #[test]
    fn statistic_is_scale_free() {
        let a = pm(30, 0.003);
        let b: Vec<f64> = a.iter().map(|x| x * 1e-6).collect();
        let za = forgone_risk_z(&[&a]).unwrap();
        let zb = forgone_risk_z(&[&b]).unwrap();
        assert!((za - zb).abs() < 1e-9, "{za} {zb}");
    }

    #[test]
    fn the_first_quarter_is_not_read_as_an_exposure_rate() {
        // One 0.04 bar then flat, over 8 bars: the burn-in is 2 bars, so the
        // rate is read from t = 2 (16e-4 / 2) and never from the single draw at
        // t = 1 (16e-4). The charge is 8 * 8e-4 and the statistic exactly 0.5.
        let mut window = vec![0.0; 8];
        window[0] = 0.04;
        let z = forgone_risk_z(&[&window]).unwrap();
        assert!((z - 0.5).abs() < 1e-12, "{z}");
    }

    #[test]
    fn the_bound_is_the_reflection_bound_at_half_alpha() {
        // Bars of equal size keep the rate at s^2 throughout, so the statistic
        // is (wins - losses) / sqrt(n): 1.8 clears the one-sided 1.645 and not
        // the reflection bound 1.96 at alpha 0.05; 2.0 clears both.
        let track = |wins: usize| -> Vec<f64> {
            (0..100)
                .map(|i| if i < wins { 0.01 } else { -0.01 })
                .collect()
        };
        let (below, above) = (track(59), track(60));
        assert!(matches!(
            exposure_path_evidence(&[&below], 0.05),
            Err(StatisticalError::InvalidParameter {
                requirement: EXPOSURE_PATH_UNPROVEN,
                ..
            })
        ));
        let z = exposure_path_evidence(&[&above], 0.05).unwrap();
        assert!((z - 2.0).abs() < 1e-12, "{z}");
    }

    #[test]
    fn no_exposure_and_bad_alpha_are_refused_by_name() {
        assert_eq!(forgone_risk_z(&[&[0.0, 0.0]]), None);
        assert_eq!(forgone_risk_z(&[]), None);
        assert!(matches!(
            exposure_path_evidence(&[&pm(30, 0.01)], f64::NAN),
            Err(StatisticalError::InvalidParameter { name: "alpha", .. })
        ));
    }
}
