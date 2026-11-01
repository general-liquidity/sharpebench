//! Standard-error certification of an admitted deflated Sharpe.
//!
//! The deflated Sharpe the gate reads divides by the standard error of a Sharpe
//! estimated from serially independent returns, with a skewness and kurtosis
//! correction. Two ways to make that standard error too small were found by
//! `sharpebench grader-search` on 2026-10-30, and each has its own refusal:
//!
//! - serial correlation ([`sharpebench_stats::serial_correlation`]), the
//!   `autocorrelated` move: fewer independent observations than bars;
//! - moment credit ([`sharpebench_stats::moment_credit`]), the
//!   `window_boundary` move: activity in a few bars manufactures a positive
//!   sample skewness the bracket credits.
//!
//! Each is a necessary condition on a deflated Sharpe that already clears the
//! bar, so neither can admit a track, and a track whose correction changes
//! nothing keeps its verdict bit for bit.

use sharpebench_stats::{
    certify_under_serial_correlation_at, certify_without_moment_credit_at, StandardErrorAt,
};

/// The refusal, if any, of a track whose deflated Sharpe clears `dsr_bar`:
/// serial correlation is asked first, then moment credit. `None` when the
/// independent deflated Sharpe is below the bar, which is that gate's refusal,
/// or when both certifications hold.
pub fn standard_error_refusal(
    windows: &[&[f64]],
    n_trials: u32,
    null_mean_sharpe: f64,
    trials_sr_std: f64,
    dsr_bar: f64,
) -> Option<String> {
    standard_error_refusal_at(
        windows,
        n_trials,
        null_mean_sharpe,
        trials_sr_std,
        dsr_bar,
        StandardErrorAt::Observed,
    )
}

/// [`standard_error_refusal`] for a deflated Sharpe whose standard error is
/// evaluated where `at` says (`ScoreConfig::psr_standard_error`). Both terms
/// are asked under the same evaluation as the deflated Sharpe they guard.
pub fn standard_error_refusal_at(
    windows: &[&[f64]],
    n_trials: u32,
    null_mean_sharpe: f64,
    trials_sr_std: f64,
    dsr_bar: f64,
    at: StandardErrorAt,
) -> Option<String> {
    let pooled: Vec<f64> = windows.iter().flat_map(|w| w.iter().copied()).collect();
    certify_under_serial_correlation_at(
        windows,
        n_trials,
        null_mean_sharpe,
        trials_sr_std,
        dsr_bar,
        at,
    )
    .and_then(|()| {
        certify_without_moment_credit_at(
            &pooled,
            n_trials,
            null_mean_sharpe,
            trials_sr_std,
            dsr_bar,
            at,
        )
    })
    .err()
    .map(|error| error.to_string())
}
