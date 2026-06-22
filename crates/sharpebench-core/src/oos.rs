//! Out-of-sample decay — how much does an agent's edge erode after the window it
//! was (implicitly) tuned on?
//!
//! An edge that is strong in the first window and gone by the third is the classic
//! overfit signature. This analyzer takes per-window pooled returns (window 0 =
//! earliest / in-sample, later windows = out-of-sample) and reports a Sharpe-proxy
//! per window plus how much of the in-sample edge is retained out of sample. Pure
//! and deterministic; the caller supplies the window segmentation.

use serde::Serialize;

use crate::stats::{mean, std_dev};

/// Per-window edge-durability report.
#[derive(Clone, Debug, Serialize)]
pub struct OosDecayReport {
    /// Sharpe-proxy (mean / std) for each window, in input order.
    pub window_metrics: Vec<f64>,
    /// The in-sample metric (window 0).
    pub in_sample: f64,
    /// The mean metric across the out-of-sample windows (1..).
    pub out_of_sample: f64,
    /// Fraction of the in-sample edge retained out of sample (`oos / is`). 1.0 =
    /// no decay; < 1 = decay; < 0 = the edge flipped sign out of sample.
    pub retention: f64,
    /// Whether the metric strictly decreases across every consecutive window — a
    /// clean monotone-decay signature.
    pub monotone_decay: bool,
}

fn window_metric(returns: &[f64]) -> f64 {
    let s = std_dev(returns);
    if s > 1e-12 {
        mean(returns) / s
    } else {
        mean(returns)
    }
}

/// Compute the OOS-decay report from per-window pooled returns.
pub fn oos_decay(windows: &[Vec<f64>]) -> OosDecayReport {
    let window_metrics: Vec<f64> = windows.iter().map(|w| window_metric(w)).collect();
    let n = window_metrics.len();
    if n == 0 {
        return OosDecayReport {
            window_metrics,
            in_sample: 0.0,
            out_of_sample: 0.0,
            retention: 1.0,
            monotone_decay: false,
        };
    }
    let in_sample = window_metrics[0];
    let out_of_sample = if n > 1 {
        mean(&window_metrics[1..])
    } else {
        in_sample
    };
    let retention = if in_sample.abs() > 1e-12 {
        out_of_sample / in_sample
    } else {
        1.0
    };
    let monotone_decay = n >= 2 && window_metrics.windows(2).all(|w| w[1] < w[0]);
    OosDecayReport {
        window_metrics,
        in_sample,
        out_of_sample,
        retention,
        monotone_decay,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decaying_edge_has_low_retention_and_monotone_flag() {
        let windows = vec![vec![0.02; 50], vec![0.01; 50], vec![0.005; 50]];
        let r = oos_decay(&windows);
        assert!(r.retention < 1.0, "retention={}", r.retention);
        assert!(r.monotone_decay);
    }

    #[test]
    fn stable_edge_retains_and_is_not_flagged() {
        let windows = vec![vec![0.01; 50], vec![0.01; 50]];
        let r = oos_decay(&windows);
        assert!((r.retention - 1.0).abs() < 1e-9);
        assert!(!r.monotone_decay, "equal windows are not decay");
    }

    #[test]
    fn improving_edge_retains_above_one() {
        let windows = vec![vec![0.01; 50], vec![0.02; 50]];
        let r = oos_decay(&windows);
        assert!(r.retention > 1.0);
        assert!(!r.monotone_decay);
    }
}
