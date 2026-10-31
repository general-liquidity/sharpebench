//! The refusal of a track that never lost: no bar of it is below zero.
//!
//! **Why the moment terms of the PSR are not enough.** The Probabilistic Sharpe
//! Ratio already lowers the score of a negatively skewed, fat-tailed track
//! through the `-g3 * SR` and `(g4 - 1) / 4 * SR^2` terms of its variance
//! (Mertens 2002; Bailey and Lopez de Prado 2012). Those are *sample* moments.
//! A seller of tail insurance collects a premium on most bars and pays a crash
//! on a few; in a window that drew no crash, the sample is a draw from the
//! premium distribution alone, so its skewness and kurtosis are the premium's
//! and the PSR reads a symmetric, thin-tailed, high-Sharpe track. The
//! `grader-search` tail seller (premium `p`, crash rate `c`, crash sized so the
//! expected return of every bar is zero) has true kurtosis of order `1 / c`, a
//! thousand at `c = 0.001`, and sample kurtosis near 3 on every window that
//! missed the crash.
//!
//! **What no scorer of the sample can do.** Conditional on drawing no crash, the
//! tail seller's sample *is* a sample of the premium distribution `F`: the
//! likelihood ratio of a crash-free sample under the tail seller against `F` is
//! `(1 - c)^n`, the same for every crash-free sample, so a rule refuses the
//! crash-free tail seller exactly as often as it refuses an honest track drawn
//! from `F` (Le Cam's two-point argument). More generally no test of a mean has
//! power above its size over all distributions with a finite mean (Bahadur and
//! Savage 1956): a scorer of returns must restrict the class it admits, and it
//! can only refuse an unseen tail by refusing the honest tracks that look like
//! it. Extreme-value estimators (Hill, a generalized Pareto fit on losses) do
//! not escape this: they extrapolate from the exceedances the sample contains,
//! and a sample that never drew the crash contains none of it.
//!
//! **The class this module restricts.** The owner's decision of 2026-10-31 is
//! that a track must show observed risk.
//! [`crate::deflated_sharpe::sign_is_forced_by_moments`] states a sufficient
//! condition for "no bar is a loss" from the mean and standard deviation alone (Samuelson 1968), and it
//! only fires above a per-bar Sharpe of `(n - 1) / sqrt(n)`, 5.3 at thirty bars.
//! A premium of 2e-4 with noise of a quarter of it has a per-bar Sharpe of 4,
//! never loses a bar, and clears that bound. This module asks the question
//! directly: a track with no bar below zero in any window has never shown a
//! loss, so its left tail is unidentified. With no loss in the sample there is
//! no observation from which any estimator, moment or extreme-value, could size
//! one, and the PSR's variance is a statement about the gains only. The test is
//! a sign, so rescaling a track does not move it, and it reads no constant.
//!
//! **What it does not close.** A tail seller whose premium noise is large enough
//! to lose on ordinary bars (a per-bar Sharpe near 1 loses on about one bar in
//! six) shows losses, and on a crash-free window it is a Gaussian high-Sharpe
//! track by every statistic of its sample. By the argument above it stays
//! admissible, as an honest Gaussian track of that Sharpe must. Nor does a
//! fabricated series that posts a token loss: returns alone cannot tell a loss
//! that was engineered from one that was taken, which is a provenance question
//! for intake. Crash replay (stress on scenarios the sample did not contain) is
//! the out-of-sample answer and is not in this module.

/// The requirement [`crate::deflated_sharpe::observed_sharpe_ratio_of_windows`]
/// refuses a track on when no bar of any window is a loss.
///
/// This is a scoring change introduced after release 0.28.0 (grader-search
/// finding of 2026-10-30, owner decision of 2026-10-31 to refuse tracks with no
/// window containing a losing bar).
pub const NO_OBSERVED_LOSS: &str =
    "must show an observed loss: no bar of any window is below zero, so the sample holds no \
     observation of the left tail and its moments describe the gains only";

/// Whether any bar of any window is strictly below zero.
///
/// A bar of exactly zero, either sign, is not a loss: a flat bar risked nothing
/// the sample can see. An empty track has no loss.
pub fn shows_an_observed_loss(windows: &[&[f64]]) -> bool {
    windows.iter().flat_map(|w| w.iter()).any(|x| *x < 0.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_track_that_never_lost_shows_no_loss_at_any_scale() {
        // The grader-search tail seller that missed its crash: premium 2e-4 with
        // noise a quarter of it, per-bar Sharpe 4, below Samuelson's bound at 30
        // bars, and not one losing bar. Rescaling cannot move a sign.
        let gains: Vec<f64> = (0..30)
            .map(|i| 2e-4 + 7e-5 * (f64::from(i) * 1.7).sin())
            .collect();
        assert!(!crate::deflated_sharpe::sign_is_forced_by_moments(&gains));
        for scale in [1e-6, 1.0, 1e6] {
            let scaled: Vec<f64> = gains.iter().map(|x| x * scale).collect();
            assert!(!shows_an_observed_loss(&[&scaled, &scaled]));
        }
    }

    #[test]
    fn one_losing_bar_in_any_window_is_an_observed_loss() {
        let gains = vec![1e-3; 30];
        let mut with_loss = gains.clone();
        with_loss[17] = -2e-3;
        assert!(shows_an_observed_loss(&[&gains, &with_loss]));
        assert!(shows_an_observed_loss(&[&with_loss, &gains]));
    }

    #[test]
    fn flat_bars_of_either_sign_are_not_losses() {
        let flat = [0.0, -0.0, 1e-3, 2e-3];
        assert!(!shows_an_observed_loss(&[&flat]));
        assert!(!shows_an_observed_loss(&[]));
    }
}
