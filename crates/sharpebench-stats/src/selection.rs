//! Selection-axis luck control.
//!
//! When an agent searches over many candidate strategies and submits the best
//! one, that best is upward-biased by selection. Reporting the **median**
//! candidate's deflated Sharpe alongside the best exposes agents that only win
//! by cherry-picking — the *selection* axis that pass^k (the reliability axis)
//! and the Deflated Sharpe (the deflation axis) do not directly cover.
//!
//! After ALE-Bench's median-of-candidates selection: a robust agent has a
//! family of edges (small `selection_gap`); a lucky one has a single spike.

use crate::deflated_sharpe::deflated_sharpe_ratio;

/// Deflated-Sharpe summary across a set of candidate return streams.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SelectionRobustness {
    pub n_candidates: usize,
    /// Deflated Sharpe of the best candidate (the headline an agent would submit).
    pub best_dsr: f64,
    /// Deflated Sharpe of the median candidate.
    pub median_dsr: f64,
    /// `best_dsr - median_dsr`. A large gap means the headline result is a lucky
    /// pick rather than a robust family of edges.
    pub selection_gap: f64,
}

/// Median of an already-sorted (ascending) slice. 0.0 for empty.
fn median_sorted(sorted: &[f64]) -> f64 {
    let n = sorted.len();
    if n == 0 {
        return 0.0;
    }
    if n % 2 == 1 {
        sorted[n / 2]
    } else {
        0.5 * (sorted[n / 2 - 1] + sorted[n / 2])
    }
}

/// Compute selection robustness over candidate return streams. Each slice is one
/// candidate strategy's pooled returns; they are deflated with the same trial
/// footprint and summarized. Empty input → all-zero.
pub fn selection_robustness(
    candidates: &[Vec<f64>],
    n_trials: u32,
    trials_sr_std: f64,
) -> SelectionRobustness {
    if candidates.is_empty() {
        return SelectionRobustness {
            n_candidates: 0,
            best_dsr: 0.0,
            median_dsr: 0.0,
            selection_gap: 0.0,
        };
    }
    let mut dsrs: Vec<f64> = candidates
        .iter()
        .map(|c| deflated_sharpe_ratio(c, n_trials, trials_sr_std))
        .collect();
    dsrs.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let best = *dsrs.last().unwrap_or(&0.0);
    let median = median_sorted(&dsrs);
    SelectionRobustness {
        n_candidates: dsrs.len(),
        best_dsr: best,
        median_dsr: median,
        selection_gap: best - median,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Deterministic return stream: constant drift + sinusoidal wiggle.
    fn stream(mean_ret: f64, amp: f64, n: usize) -> Vec<f64> {
        (0..n)
            .map(|i| mean_ret + amp * (i as f64 * 0.7).sin())
            .collect()
    }

    #[test]
    fn cherry_picked_winner_has_large_gap() {
        // One strong candidate among many noisy ones → big selection gap.
        let mut candidates = vec![stream(0.004, 0.001, 80)];
        candidates.extend((0..8).map(|_| stream(0.0, 0.003, 80)));
        let s = selection_robustness(&candidates, 50, 0.5);
        assert_eq!(s.n_candidates, 9);
        assert!(s.best_dsr >= s.median_dsr);
        assert!(
            s.selection_gap > 0.0,
            "a lone winner should leave a positive selection gap: {s:?}"
        );
    }

    #[test]
    fn robust_family_has_small_gap() {
        // Many similarly-skilled candidates → best ≈ median, small gap.
        let candidates: Vec<Vec<f64>> = (0..9).map(|_| stream(0.003, 0.0005, 80)).collect();
        let s = selection_robustness(&candidates, 50, 0.5);
        assert!(
            s.selection_gap < 0.10,
            "a robust family should have a small gap: {s:?}"
        );
    }

    #[test]
    fn empty_is_zero() {
        let s = selection_robustness(&[], 50, 0.5);
        assert_eq!(s.n_candidates, 0);
        assert_eq!(s.selection_gap, 0.0);
    }
}
