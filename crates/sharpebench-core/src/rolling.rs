//! Rolling-window Sharpe stability — is the deflated edge one lucky window?
//!
//! The Deflated Sharpe is a single pooled-track number; a high value can still
//! hide an edge that lives in one good stretch and is flat-to-negative the rest
//! of the time. This module slides a fixed window over the pooled returns and
//! reports the **worst-window** (non-annualized) Sharpe and the **fraction of
//! windows that are positive**. A robust agent has many positive windows and a
//! worst window that isn't catastrophic; a one-window fluke does not.

use crate::deflated_sharpe::sharpe_ratio;

/// Rolling per-window Sharpe summary over a single return series.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RollingSharpe {
    /// The worst (minimum) per-window Sharpe observed.
    pub min_sharpe: f64,
    /// Fraction of windows whose Sharpe is strictly positive, in [0, 1].
    pub frac_positive: f64,
}

/// Compute the worst-window Sharpe and fraction-of-positive-windows over
/// `returns`, using overlapping windows of length `window` (step 1). Returns
/// `None` when the series is shorter than one full window (`window < 2` is
/// treated as no window).
pub fn rolling_sharpe(returns: &[f64], window: usize) -> Option<RollingSharpe> {
    if window < 2 || returns.len() < window {
        return None;
    }
    let n_windows = returns.len() - window + 1;
    let mut min_sharpe = f64::INFINITY;
    let mut positive = 0usize;
    for start in 0..n_windows {
        let s = sharpe_ratio(&returns[start..start + window]);
        if s < min_sharpe {
            min_sharpe = s;
        }
        if s > 0.0 {
            positive += 1;
        }
    }
    Some(RollingSharpe {
        min_sharpe,
        frac_positive: positive as f64 / n_windows as f64,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn too_short_returns_none() {
        assert!(rolling_sharpe(&[0.1, 0.2], 5).is_none());
        assert!(rolling_sharpe(&[0.1, 0.2, 0.3], 1).is_none());
    }

    #[test]
    fn steady_edge_is_all_positive() {
        // A consistent positive drift → every window has a positive Sharpe and the
        // worst window is still clearly above zero.
        let r: Vec<f64> = (0..60)
            .map(|i| 0.002 + 0.0005 * (i as f64 * 0.7).sin())
            .collect();
        let rs = rolling_sharpe(&r, 21).expect("long enough");
        assert!(
            (rs.frac_positive - 1.0).abs() < 1e-12,
            "frac_positive={}",
            rs.frac_positive
        );
        assert!(rs.min_sharpe > 0.0, "worst window should be positive");
    }

    #[test]
    fn one_lucky_window_drags_min_and_frac() {
        // A single spectacular stretch followed by a long flat-to-negative tail.
        let mut r = vec![0.05; 21]; // one great window
        r.extend(vec![-0.001; 60]); // long bad tail
        let rs = rolling_sharpe(&r, 21).expect("long enough");
        // The worst window is the all-negative one → negative Sharpe.
        assert!(rs.min_sharpe < 0.0, "min_sharpe={}", rs.min_sharpe);
        // Most windows live in the negative tail → far from all-positive.
        assert!(rs.frac_positive < 0.5, "frac_positive={}", rs.frac_positive);
    }
}
