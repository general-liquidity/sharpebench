//! The deflated Sharpe ratio with its standard error corrected for serial
//! correlation, read by the admission test.
//!
//! The kernel's deflated Sharpe divides by the standard error of a Sharpe ratio
//! estimated from serially independent returns. A positively autocorrelated
//! track has fewer independent observations than bars, so that standard error
//! is too small and the deflated Sharpe overstates the evidence. Lo (2002), "The
//! Statistics of Sharpe Ratios", section 4, corrects it for non-i.i.d. returns
//! with the heteroskedasticity and autocorrelation consistent estimator of
//! Newey and West (1987). This module does the same: the independent-returns
//! bracket of the kernel is multiplied by the long-run variance ratio
//!
//! ```text
//! lambda = 1 + 2 * sum_{k=1}^{L} (1 - k / (L + 1)) * rho_k
//! ```
//!
//! with Bartlett weights, `rho_k` the sample autocorrelations measured inside
//! windows ([`within_window_autocorrelations`]), and the bandwidth `L` chosen by
//! the AR(1) plug-in rule of Andrews (1991) for the Bartlett kernel,
//! `1.1447 * (alpha * T)^(1/3)` with `alpha = 4 rho_1^2 / ((1 - rho_1)^2 (1 + rho_1)^2)`,
//! so the window the estimator looks through grows with the persistence it sees.
//! The ratio is exact for the mean term of the Sharpe variance; applying it to
//! the whole bracket is conservative under AR(1), whose skewness and kurtosis
//! terms inflate by less.
//!
//! The estimate is model-free. An AR(1) bracket that extrapolates `rho_1^k`
//! (López de Prado, Lipton and Zoonekynd 2026, eq. 2) would read a periodic
//! track, whose autocorrelations alternate in sign and whose mean is *more*
//! precise than an i.i.d. one, as seven times less precise. Benhamou et al.
//! (2019), "Testing Sharpe ratio: luck or skill?", is not used: its correction
//! does not hold its size under positive autocorrelation.
//!
//! `grader-search` found the gap on 2026-10-30: AR(1), restarted-AR(1) and
//! moving-average ("smoothed", the shape of an appraised book) tracks with a
//! zero conditional mean by construction were admitted on held-out seeds at up
//! to 24 of 64, against 0 to 2 of 64 for an i.i.d. null on the same layout.
//! The stationary bootstrap the gate already runs points the same way as this
//! correction, toward less significance, but its expected block of ten bars is
//! short against an autocorrelation time of fifty.

use crate::deflated_sharpe::{
    deflated_sharpe_ratio_against_null, expected_max_sharpe, observed_sharpe_ratio,
};
use crate::stats::{kurtosis, mean, norm_cdf, skewness};
use crate::validation::{
    finite_computation, finite_observations, finite_parameter, StatisticalError,
};

/// The requirement the admission test refuses a track on when its deflated
/// Sharpe clears the bar only under serial independence.
///
/// A scoring change introduced after release 0.28.0 (grader-search finding of
/// 2026-10-30): such a track was eligible under 0.28.0.
pub const NOT_CERTIFIED_UNDER_SERIAL_CORRELATION: &str =
    "must clear the deflation bar with the Sharpe standard error corrected for the serial \
     correlation measured inside its windows (Lo 2002, Newey-West): a serially correlated track \
     has fewer independent observations than bars, and the independent-returns deflated Sharpe \
     overstates its evidence";

/// The sample autocorrelations of a windowed track at lags `1..=max_lag`,
/// measured inside its windows only.
///
/// `rho_k = sum_w sum_t (x_wt - m_w)(x_w,t+k - m_w) / sum_w sum_t (x_wt - m_w)^2`,
/// `m_w` the window's own mean. Bars of different windows are different market
/// periods, so no product across a window join is counted, and a window is
/// centred on itself, so a level difference between two regimes is not read
/// as persistence inside either: that dispersion is the between-window share,
/// a separate diagnostic. The estimate does not move when windows are
/// reordered.
///
/// Every return must be finite and some window must vary about its own mean.
pub fn within_window_autocorrelations(
    windows: &[&[f64]],
    max_lag: usize,
) -> Result<Vec<f64>, StatisticalError> {
    let pooled: Vec<f64> = windows.iter().flat_map(|w| w.iter().copied()).collect();
    finite_observations(&pooled)?;
    if pooled.len() < 2 {
        return Err(StatisticalError::InsufficientObservations {
            required: 2,
            actual: pooled.len(),
        });
    }
    let centred: Vec<Vec<f64>> = windows
        .iter()
        .map(|w| {
            let m = mean(w);
            w.iter().map(|x| x - m).collect()
        })
        .collect();
    let squares: f64 = centred.iter().flatten().map(|d| d * d).sum();
    let squares = finite_computation(squares, "return sum of squares")?;
    if squares == 0.0 {
        return Err(StatisticalError::InvalidParameter {
            name: "returns",
            requirement: "must vary inside a window: a series constant within every window has                           no autocorrelation",
        });
    }
    // inv: `out` holds rho_1 .. rho_{k-1}. term: k rises by one to max_lag.
    let mut out = Vec::with_capacity(max_lag);
    for k in 1..=max_lag {
        let cross: f64 = centred
            .iter()
            .filter(|w| w.len() > k)
            .flat_map(|w| w.iter().zip(&w[k..]))
            .map(|(a, b)| a * b)
            .sum();
        let cross = finite_computation(cross, "return lagged cross product")?;
        out.push(finite_computation(
            cross / squares,
            "within-window autocorrelation",
        )?);
    }
    Ok(out)
}

/// The Bartlett bandwidth of Andrews (1991, eq. 6.4 and table 1) under his
/// AR(1) plug-in, `floor(1.1447 * (alpha * T)^(1/3))`, capped at one less than
/// the shortest window so every lag is measured in every window.
fn andrews_bartlett_bandwidth(rho_1: f64, observations: usize, shortest_window: usize) -> usize {
    let alpha = 4.0 * rho_1 * rho_1 / ((1.0 - rho_1).powi(2) * (1.0 + rho_1).powi(2));
    let bandwidth = 1.1447 * (alpha * observations as f64).cbrt();
    let cap = shortest_window.saturating_sub(1);
    if bandwidth.is_finite() && bandwidth >= 0.0 {
        (bandwidth.floor() as usize).min(cap)
    } else {
        cap
    }
}

/// What the serial-correlation correction measured on one track.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SerialCorrection {
    /// The within-window lag-one autocorrelation the bandwidth is chosen from.
    pub lag_one_autocorrelation: f64,
    /// The Bartlett bandwidth, in bars.
    pub bandwidth: usize,
    /// The Newey-West long-run variance ratio, before the floor.
    pub long_run_variance_ratio: f64,
    /// The ratio the bracket is multiplied by: `max(long_run_variance_ratio, 1)`.
    /// Serial correlation that makes the mean more precise is not credited, so
    /// the correction never admits a track the kernel's own test refuses.
    pub applied_ratio: f64,
    /// The deflated Sharpe with the corrected standard error. Equal, bit for bit,
    /// to the kernel's deflated Sharpe when `applied_ratio` is one.
    pub deflated_sharpe: f64,
}

/// The deflated Sharpe of a windowed track against `null_mean_sharpe` plus the
/// expected maximum of `n_trials` trials, with the kernel's bracket multiplied by
/// the floored Newey-West long-run variance ratio, evaluated at the observed
/// Sharpe as the kernel's deflated Sharpe is.
///
/// Scale-free: every input to the statistic is a ratio of moments or of
/// autocovariances, so a rescaled track has the same value.
pub fn serially_corrected_deflated_sharpe(
    windows: &[&[f64]],
    n_trials: u32,
    null_mean_sharpe: f64,
    trials_sr_std: f64,
) -> Result<SerialCorrection, StatisticalError> {
    let pooled: Vec<f64> = windows.iter().flat_map(|w| w.iter().copied()).collect();
    finite_observations(&pooled)?;
    finite_parameter(null_mean_sharpe, "null_mean_sharpe")?;
    let sr_star = finite_computation(
        null_mean_sharpe + expected_max_sharpe(trials_sr_std, n_trials)?,
        "deflation benchmark",
    )?;
    let shortest = windows.iter().map(|w| w.len()).min().unwrap_or(0);
    let rho_1 = within_window_autocorrelations(windows, 1)?
        .first()
        .copied()
        .unwrap_or(0.0);
    let bandwidth = andrews_bartlett_bandwidth(rho_1, pooled.len(), shortest);
    let rhos = within_window_autocorrelations(windows, bandwidth)?;
    let weighted: f64 = rhos
        .iter()
        .enumerate()
        .map(|(i, rho)| (1.0 - (i + 1) as f64 / (bandwidth + 1) as f64) * rho)
        .sum();
    let long_run_variance_ratio =
        finite_computation(1.0 + 2.0 * weighted, "long-run variance ratio")?;
    let applied_ratio = long_run_variance_ratio.max(1.0);

    // The kernel's own computation (`checked_psr`, observed Sharpe), in its
    // order of operations, with the bracket multiplied by the ratio.
    let sr = observed_sharpe_ratio(&pooled)?;
    let g3 = finite_computation(skewness(&pooled), "return skewness")?;
    let g4 = finite_computation(kurtosis(&pooled), "return kurtosis")?;
    let bracket = finite_computation(1.0 - g3 * sr + ((g4 - 1.0) / 4.0) * sr * sr, "PSR variance")?;
    let variance = finite_computation(bracket * applied_ratio, "corrected PSR variance")?;
    let numerator = finite_computation(
        (sr - sr_star) * (pooled.len() as f64 - 1.0).sqrt(),
        "PSR numerator",
    )?;
    let z = finite_computation(numerator / variance.max(1e-12).sqrt(), "PSR z statistic")?;
    Ok(SerialCorrection {
        lag_one_autocorrelation: rho_1,
        bandwidth,
        long_run_variance_ratio,
        applied_ratio,
        deflated_sharpe: finite_computation(norm_cdf(z), "PSR probability")?,
    })
}

/// The admission test's serial-correlation term: `Ok` unless the track's
/// deflated Sharpe clears `dsr_bar` under serial independence and does not once
/// its standard error is corrected, which is refused with
/// [`NOT_CERTIFIED_UNDER_SERIAL_CORRELATION`].
///
/// It never admits a track: a track the independent test refuses is left to
/// that refusal, and a correction that cannot be computed on a track the
/// independent test admits is an error, not a pass.
pub fn certify_under_serial_correlation(
    windows: &[&[f64]],
    n_trials: u32,
    null_mean_sharpe: f64,
    trials_sr_std: f64,
    dsr_bar: f64,
) -> Result<(), StatisticalError> {
    let pooled: Vec<f64> = windows.iter().flat_map(|w| w.iter().copied()).collect();
    let independent =
        deflated_sharpe_ratio_against_null(&pooled, n_trials, null_mean_sharpe, trials_sr_std)?;
    if independent < dsr_bar {
        return Ok(());
    }
    let corrected =
        serially_corrected_deflated_sharpe(windows, n_trials, null_mean_sharpe, trials_sr_std)?;
    if corrected.deflated_sharpe < dsr_bar {
        return Err(StatisticalError::InvalidParameter {
            name: "returns",
            requirement: NOT_CERTIFIED_UNDER_SERIAL_CORRELATION,
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A track whose serial correlation makes its mean no less precise scores
    /// exactly as the kernel does, so the rule cannot move a verdict on it: an
    /// alternating track, and a sinusoid whose autocorrelations change sign.
    #[test]
    fn with_no_loss_of_precision_it_is_the_kernel_deflated_sharpe() {
        let alternating: Vec<f64> = (0..60)
            .map(|t| {
                let sign = if t % 2 == 0 { 1.0 } else { -1.0 };
                0.004 + 0.01 * sign * (1.0 + 0.3 * f64::from(t).sin())
            })
            .collect();
        let periodic: Vec<f64> = (0..60)
            .map(|t| 0.004 + 0.006 * (f64::from(t) * 0.7).sin())
            .collect();
        for window in [alternating, periodic] {
            let windows = [window.as_slice(), window.as_slice()];
            let corrected = serially_corrected_deflated_sharpe(&windows, 50, 0.0, 0.03).unwrap();
            assert!(corrected.long_run_variance_ratio <= 1.0, "{corrected:?}");
            assert_eq!(corrected.applied_ratio, 1.0);
            let pooled = windows.concat();
            let kernel = deflated_sharpe_ratio_against_null(&pooled, 50, 0.0, 0.03).unwrap();
            assert_eq!(corrected.deflated_sharpe.to_bits(), kernel.to_bits());
        }
    }

    /// The last bar of one window and the first of the next are not neighbours:
    /// two monotone windows read the same in either order, while the joined
    /// series would count the jump between them.
    #[test]
    fn the_estimate_never_reads_across_a_window_boundary() {
        let up: Vec<f64> = (0..20).map(|t| 0.001 * f64::from(t)).collect();
        let down: Vec<f64> = (0..20).map(|t| 0.03 - 0.001 * f64::from(t)).collect();
        let forward = within_window_autocorrelations(&[&up, &down], 3).unwrap();
        let backward = within_window_autocorrelations(&[&down, &up], 3).unwrap();
        let joined = [up.clone(), down.clone()].concat();
        let as_one = within_window_autocorrelations(&[&joined], 3).unwrap();
        for k in 0..3 {
            assert!((forward[k] - backward[k]).abs() < 1e-12, "lag {}", k + 1);
            assert!((forward[k] - as_one[k]).abs() > 1e-3, "lag {}", k + 1);
        }
    }

    /// An AR(1) window `x_t = drift + phi * (x_{t-1} - drift) + e_t`, with
    /// uniform innovations on `[-scale, scale]` from a fixed xorshift64 stream.
    fn ar1(len: usize, phi: f64, drift: f64, scale: f64, seed: u64) -> Vec<f64> {
        let mut state = seed;
        let mut level = 0.0;
        // inv: `out` holds the first `t` bars and `level` is bar t - 1 minus drift.
        // term: t rises by one to len.
        (0..len)
            .map(|_| {
                state ^= state << 13;
                state ^= state >> 7;
                state ^= state << 17;
                let unit = (state >> 11) as f64 / (1u64 << 53) as f64;
                level = phi * level + scale * (2.0 * unit - 1.0);
                drift + level
            })
            .collect()
    }

    /// Two AR(1) windows at phi = 0.6 with a positive drift: the independent
    /// deflated Sharpe is 0.969, the corrected one 0.893, so a 0.95 bar
    /// separates them.
    fn persistent_track() -> (Vec<f64>, Vec<f64>) {
        (
            ar1(120, 0.6, 0.0008, 0.004, 1),
            ar1(120, 0.6, 0.0008, 0.004, 1001),
        )
    }

    /// Andrews (1991) eq. 6.4 and table 1 for the Bartlett kernel, worked by
    /// hand: alpha = 4 rho^2 / ((1 - rho)^2 (1 + rho)^2).
    ///
    /// - rho 0.5, T 100: alpha = 1 / 0.5625 = 16/9, (alpha T)^(1/3) = 5.623,
    ///   times 1.1447 is 6.437, so 6 bars.
    /// - rho 0.9, T 600: alpha = 3.24 / 0.0361 = 89.75, (alpha T)^(1/3) = 37.76,
    ///   times 1.1447 is 43.23, so 43 bars.
    /// - rho -0.5 is rho 0.5: alpha reads rho squared.
    /// - rho 0: no persistence, no lags.
    /// - the cap is one less than the shortest window, and saturates at zero.
    /// - rho 1: alpha is infinite and the bandwidth is the cap.
    #[test]
    fn the_bandwidth_is_the_andrews_ar1_plug_in_at_hand_computed_values() {
        let cases: [(f64, usize, usize, usize); 9] = [
            (0.5, 100, 1000, 6),
            (0.9, 600, 1000, 43),
            (-0.5, 100, 1000, 6),
            (0.0, 500, 1000, 0),
            (0.5, 100, 5, 4),
            (0.5, 100, 7, 6),
            (0.5, 100, 0, 0),
            (1.0, 100, 30, 29),
            (-1.0, 100, 30, 29),
        ];
        for (rho, observations, shortest, expected) in cases {
            assert_eq!(
                andrews_bartlett_bandwidth(rho, observations, shortest),
                expected,
                "rho {rho}, T {observations}, shortest window {shortest}"
            );
        }
    }

    /// rho_k is the sum of within-window lag-k cross products over the sum of
    /// within-window squares, each window centred on its own mean, and a window
    /// too short for lag k adds nothing at that lag. Worked by hand:
    /// `[1, -1, 1, -1]` (mean 0) has squares 4 and cross products -3, 2, -1 at
    /// lags 1 to 3; `[2, 4, 6]` (mean 4, centred `[-2, 0, 2]`) has squares 8 and
    /// cross products 0, -4 at lags 1 and 2. Pooled: -3/12, -2/12, -1/12.
    #[test]
    fn autocorrelations_pool_cross_products_over_squares_by_window() {
        let rhos = within_window_autocorrelations(&[&[1.0, -1.0, 1.0, -1.0], &[2.0, 4.0, 6.0]], 3)
            .unwrap();
        assert_eq!(rhos, vec![-3.0 / 12.0, -2.0 / 12.0, -1.0 / 12.0]);
    }

    /// On a persistent track the bandwidth is the plug-in at the measured
    /// lag-one autocorrelation, the ratio is the Bartlett-weighted sum through
    /// it, and the deflated Sharpe is the kernel's with its bracket multiplied
    /// by that ratio (Lo 2002, eq. 4 with the Newey-West long-run variance).
    #[test]
    fn a_persistent_track_has_its_bracket_widened_by_the_long_run_ratio() {
        let (a, b) = persistent_track();
        let windows = [a.as_slice(), b.as_slice()];
        let corrected = serially_corrected_deflated_sharpe(&windows, 1, 0.0, 0.0).unwrap();

        let rho_1 = within_window_autocorrelations(&windows, 1).unwrap()[0];
        assert_eq!(corrected.lag_one_autocorrelation, rho_1);
        assert_eq!(
            corrected.bandwidth,
            andrews_bartlett_bandwidth(rho_1, 240, 120)
        );
        assert_eq!(corrected.bandwidth, 9);

        let lags = corrected.bandwidth;
        let rhos = within_window_autocorrelations(&windows, lags).unwrap();
        let weighted: f64 = (1..=lags)
            .map(|k| (1.0 - k as f64 / (lags + 1) as f64) * rhos[k - 1])
            .sum();
        assert_eq!(corrected.long_run_variance_ratio, 1.0 + 2.0 * weighted);
        assert!(corrected.long_run_variance_ratio > 2.0, "{corrected:?}");
        assert_eq!(corrected.applied_ratio, corrected.long_run_variance_ratio);

        let pooled = windows.concat();
        let sr = observed_sharpe_ratio(&pooled).unwrap();
        let bracket = 1.0 - skewness(&pooled) * sr + ((kurtosis(&pooled) - 1.0) / 4.0) * sr * sr;
        let z =
            sr * (pooled.len() as f64 - 1.0).sqrt() / (bracket * corrected.applied_ratio).sqrt();
        assert_eq!(corrected.deflated_sharpe.to_bits(), norm_cdf(z).to_bits());
        let kernel = deflated_sharpe_ratio_against_null(&pooled, 1, 0.0, 0.0).unwrap();
        assert!(corrected.deflated_sharpe < kernel, "{corrected:?} {kernel}");
    }

    const REFUSED: Result<(), StatisticalError> = Err(StatisticalError::InvalidParameter {
        name: "returns",
        requirement: NOT_CERTIFIED_UNDER_SERIAL_CORRELATION,
    });

    /// The rule refuses exactly the tracks whose independent deflated Sharpe is
    /// at or above the bar and whose corrected one is below it, and both
    /// comparisons are inclusive of the bar as the kernel's gate is.
    #[test]
    fn certification_refuses_only_between_the_corrected_and_independent_values() {
        let (a, b) = persistent_track();
        let windows = [a.as_slice(), b.as_slice()];
        let pooled = windows.concat();
        let independent = deflated_sharpe_ratio_against_null(&pooled, 1, 0.0, 0.0).unwrap();
        let corrected = serially_corrected_deflated_sharpe(&windows, 1, 0.0, 0.0)
            .unwrap()
            .deflated_sharpe;
        assert!(
            corrected < 0.95 && 0.95 <= independent,
            "{corrected} {independent}"
        );

        let certify = |bar: f64| certify_under_serial_correlation(&windows, 1, 0.0, 0.0, bar);
        // Cleared only under independence: refused.
        assert_eq!(certify(0.95), REFUSED);
        // A bar exactly at the independent value is cleared by it, so the
        // correction is asked, and refuses.
        assert_eq!(certify(independent), REFUSED);
        // A bar the independent value does not reach is that gate's refusal,
        // not this one's.
        assert_eq!(certify(0.99), Ok(()));
        // A bar exactly at the corrected value is cleared by it.
        assert_eq!(certify(corrected), Ok(()));
        assert_eq!(certify(0.5), Ok(()));
    }

    /// Two bars are the fewest that have a lag: `[1, -1]` centred is itself,
    /// squares 2, lag-one cross product -1, so rho_1 = -0.5. One bar is refused.
    #[test]
    fn two_bars_are_the_boundary_of_a_measurable_autocorrelation() {
        assert_eq!(
            within_window_autocorrelations(&[&[1.0, -1.0]], 1),
            Ok(vec![-0.5])
        );
        assert_eq!(
            within_window_autocorrelations(&[&[1.0]], 1),
            Err(StatisticalError::InsufficientObservations {
                required: 2,
                actual: 1
            })
        );
    }
}
