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
//!
//! [`selection_robustness`] diagnoses the search after the fact. The second half
//! of this module changes the *choice*: [`percentile_selection`] picks the
//! candidate with the best percentile of a bootstrapped utility distribution
//! instead of the best point estimate, so the winner has to be good on most
//! resampled histories rather than on the one that happened to be observed.

use crate::deflated_sharpe::deflated_sharpe_ratio;
// The one resampling generator in the crate. Two copies of a PRNG in a product
// whose claim is byte-identical recompute is a divergence waiting to happen, so
// `significance` owns it and this module draws from the same definition.
use crate::significance::SplitMix64;
use crate::stats::{mean, std_dev};

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

/// Recommended selection percentile: the **middle** of the measured band.
///
/// The instinct that a lower percentile must be more conservative is wrong here.
/// The extreme lower tail of a bootstrap utility distribution is the disaster the
/// sample barely evidences at all: one or two unlucky resamples, driven by a
/// handful of observations, decide the whole ranking. Optimising against that
/// picks the candidate that is least bad in a scenario nobody has real data for,
/// which is a different failure from the one we are trying to avoid. The middle
/// of the band asks the useful question instead: how does this candidate do on a
/// typical history, rather than on the one that was observed?
pub const DEFAULT_SELECTION_ALPHA: f64 = 0.5;

/// Below this percentile, [`PercentileSelection::alpha_warning`] is raised. Not a
/// hard floor: an operator who genuinely wants tail-robust selection can go
/// lower, but should do it on purpose.
pub const MIN_RECOMMENDED_SELECTION_ALPHA: f64 = 0.3;

/// What a candidate is scored on inside [`percentile_selection`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Utility {
    /// Mean per-period return. Sensitive to concentration: an edge carried by a
    /// few observations shows up as a wide bootstrap distribution.
    MeanReturn,
    /// Per-period Sharpe (mean / standard deviation, not annualized). 0.0 when
    /// the track has no dispersion.
    Sharpe,
}

fn utility_of(xs: &[f64], utility: Utility) -> f64 {
    match utility {
        Utility::MeanReturn => mean(xs),
        Utility::Sharpe => {
            let sd = std_dev(xs);
            if sd == 0.0 {
                0.0
            } else {
                mean(xs) / sd
            }
        }
    }
}

/// One stationary-bootstrap (Politis & Romano) index path over `0..n`: start at a
/// random position, walk forward, and with probability `block_prob` jump to a new
/// random start. Blocks preserve serial correlation, so a candidate whose edge
/// lives in one contiguous stretch is not silently smoothed into a steady one.
fn fill_block_path(rng: &mut SplitMix64, idxs: &mut [usize], n: usize, block_prob: f64) {
    let mut idx = rng.below(n);
    for slot in idxs.iter_mut() {
        *slot = idx;
        if rng.unit() < block_prob {
            idx = rng.below(n);
        } else {
            idx = (idx + 1) % n;
        }
    }
}

/// Per-candidate result inside a [`PercentileSelection`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CandidateUtility {
    /// Position of this candidate in the input slice.
    pub index: usize,
    /// Utility on the observed path: the number a naive argmax would rank on.
    pub point_utility: f64,
    /// The `alpha` percentile of the bootstrapped utility distribution.
    pub percentile_utility: f64,
    /// `point_utility - percentile_utility`. How much of the headline number
    /// fails to survive resampling. A robust candidate carries a small gap; a
    /// lucky one carries a large one, because its utility rests on observations
    /// the bootstrap can decline to draw.
    pub optimism_gap: f64,
}

/// Selection on a percentile of a bootstrapped utility distribution.
#[derive(Clone, Debug, PartialEq)]
pub struct PercentileSelection {
    /// The percentile actually used, clamped to [0, 1].
    pub alpha: f64,
    /// True when `alpha` sits below [`MIN_RECOMMENDED_SELECTION_ALPHA`]. The
    /// result is still computed: this flags a choice, it does not veto one.
    pub alpha_warning: bool,
    /// Every candidate, in input order.
    pub candidates: Vec<CandidateUtility>,
    /// Index of the candidate with the best percentile utility: the robust pick.
    /// `None` for empty input.
    pub selected: Option<usize>,
    /// Index of the candidate with the best point utility: the naive pick.
    pub point_argmax: Option<usize>,
    /// Whether the two agree. Disagreement is the interesting case and is the
    /// whole reason to run this: the observed path was ranking a candidate that
    /// does not hold up across resampled histories.
    pub agrees_with_point_argmax: bool,
    /// Optimism gap of the *point* winner. This is the number to report next to
    /// any headline utility.
    pub point_winner_optimism: f64,
}

/// Rank candidates on the `alpha` percentile of their bootstrapped utility rather
/// than on the argmax of a point estimate.
///
/// After "(Non-Parametric) Bootstrap Robust Optimization for Portfolios and
/// Trading Strategies". The point estimate answers "how did this do on the one
/// path we saw?"; resampling that path and taking a percentile answers "how does
/// this do across the histories that path is consistent with?", and only the
/// second question distinguishes an edge from a draw. Resampling uses the
/// stationary bootstrap, so serially correlated returns are not flattened.
///
/// Each candidate is a slice of per-period returns; they need not be the same
/// length. `alpha` should normally be [`DEFAULT_SELECTION_ALPHA`]: see the note
/// there on why reaching for the lower tail is a mistake. Deterministic given
/// `seed`, and a candidate's resample stream depends only on its own position, so
/// appending a candidate does not perturb the ones before it.
///
/// Empty input, `n_boot == 0`, or an empty candidate series all degrade quietly:
/// a candidate with no returns scores 0.0 on both legs.
pub fn percentile_selection(
    candidates: &[Vec<f64>],
    utility: Utility,
    alpha: f64,
    seed: u64,
    n_boot: usize,
    block_prob: f64,
) -> PercentileSelection {
    let alpha = alpha.clamp(0.0, 1.0);
    let alpha_warning = alpha < MIN_RECOMMENDED_SELECTION_ALPHA;

    let mut out: Vec<CandidateUtility> = Vec::with_capacity(candidates.len());
    for (ki, c) in candidates.iter().enumerate() {
        let point_utility = utility_of(c, utility);
        let n = c.len();
        let percentile_utility = if n < 2 || n_boot == 0 {
            point_utility
        } else {
            // Seed per candidate position so the stream is stable under append.
            let mut rng = SplitMix64::new(
                seed ^ 0x5E1E_C710_2026_A1FA ^ (ki as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15),
            );
            let mut idxs = vec![0usize; n];
            let mut resample = vec![0.0; n];
            let mut boots: Vec<f64> = Vec::with_capacity(n_boot);
            for _ in 0..n_boot {
                fill_block_path(&mut rng, &mut idxs, n, block_prob);
                for (slot, &j) in resample.iter_mut().zip(idxs.iter()) {
                    *slot = c[j];
                }
                boots.push(utility_of(&resample, utility));
            }
            boots.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
            let q = ((alpha * n_boot as f64).floor() as usize).min(n_boot - 1);
            boots[q]
        };
        out.push(CandidateUtility {
            index: ki,
            point_utility,
            percentile_utility,
            optimism_gap: point_utility - percentile_utility,
        });
    }

    // First-index-wins on ties, so the ranking is stable and reproducible.
    let best_by = |key: fn(&CandidateUtility) -> f64| -> Option<usize> {
        out.iter().fold(None, |acc: Option<usize>, c| match acc {
            Some(b) if key(&out[b]) >= key(c) => Some(b),
            _ => Some(c.index),
        })
    };
    let selected = best_by(|c| c.percentile_utility);
    let point_argmax = best_by(|c| c.point_utility);
    let point_winner_optimism = point_argmax.map_or(0.0, |i| out[i].optimism_gap);

    PercentileSelection {
        alpha,
        alpha_warning,
        candidates: out,
        selected,
        point_argmax,
        agrees_with_point_argmax: selected == point_argmax,
        point_winner_optimism,
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

    /// A genuinely steady earner: the same modest return every period.
    fn steady(n: usize) -> Vec<f64> {
        (0..n)
            .map(|i| 0.003 + 0.0001 * (i as f64 * 0.9).sin())
            .collect()
    }

    /// A candidate whose entire headline mean comes from a single observation.
    /// Nothing about it is repeatable, but on the observed path it wins.
    fn one_lucky_spike(n: usize) -> Vec<f64> {
        let mut v = vec![0.0005; n];
        v[n / 2] = 0.35;
        v
    }

    #[test]
    fn percentile_selection_prefers_the_robust_candidate_over_the_lucky_one() {
        let robust = steady(100);
        let lucky = one_lucky_spike(100);
        let candidates = vec![robust, lucky];
        let s = percentile_selection(&candidates, Utility::MeanReturn, 0.3, 11, 4000, 0.1);

        assert_eq!(
            s.point_argmax,
            Some(1),
            "on the observed path the spike wins: {:?}",
            s.candidates
        );
        assert_eq!(
            s.selected,
            Some(0),
            "across resampled histories the steady earner wins: {:?}",
            s.candidates
        );
        assert!(!s.agrees_with_point_argmax);
        assert!(
            !s.alpha_warning,
            "0.3 is the recommended floor, not below it"
        );
    }

    #[test]
    fn the_lucky_candidate_carries_the_larger_optimism_gap() {
        let candidates = vec![steady(100), one_lucky_spike(100)];
        let s = percentile_selection(&candidates, Utility::MeanReturn, 0.3, 11, 4000, 0.1);
        let robust = s.candidates[0];
        let lucky = s.candidates[1];
        assert!(
            lucky.optimism_gap > 10.0 * robust.optimism_gap.abs(),
            "the gap is the tell: robust {robust:?} vs lucky {lucky:?}"
        );
        assert!(
            (s.point_winner_optimism - lucky.optimism_gap).abs() < 1e-15,
            "the reported optimism belongs to the point winner"
        );
    }

    #[test]
    fn a_clear_winner_still_wins_at_the_default_alpha() {
        // No luck to unwind here: one candidate is simply better everywhere, so
        // the middle of the band agrees with the point estimate.
        let candidates = vec![
            stream(0.001, 0.0005, 120),
            stream(0.006, 0.0005, 120),
            stream(0.002, 0.0005, 120),
        ];
        let s = percentile_selection(
            &candidates,
            Utility::MeanReturn,
            DEFAULT_SELECTION_ALPHA,
            5,
            1000,
            0.1,
        );
        assert_eq!(s.selected, Some(1));
        assert!(s.agrees_with_point_argmax);
    }

    #[test]
    fn sharpe_utility_ranks_on_risk_adjusted_terms() {
        // Same mean, wildly different dispersion: mean-return selection calls it
        // a tie and Sharpe selection does not.
        let calm = stream(0.003, 0.0005, 150);
        let wild = stream(0.003, 0.02, 150);
        let candidates = vec![wild, calm];
        let s = percentile_selection(
            &candidates,
            Utility::Sharpe,
            DEFAULT_SELECTION_ALPHA,
            3,
            800,
            0.1,
        );
        assert_eq!(s.selected, Some(1), "the calm track is the better bet");
    }

    #[test]
    fn a_low_alpha_warns_and_the_default_does_not() {
        let candidates = vec![steady(60), steady(60)];
        let low = percentile_selection(&candidates, Utility::MeanReturn, 0.05, 1, 200, 0.1);
        assert!(low.alpha_warning, "the extreme tail should announce itself");
        assert_eq!(low.alpha, 0.05, "but the result is still computed");
        let mid = percentile_selection(
            &candidates,
            Utility::MeanReturn,
            DEFAULT_SELECTION_ALPHA,
            1,
            200,
            0.1,
        );
        assert!(!mid.alpha_warning);
        // Out-of-range alphas clamp rather than panic.
        let hi = percentile_selection(&candidates, Utility::MeanReturn, 4.0, 1, 200, 0.1);
        assert_eq!(hi.alpha, 1.0);
    }

    #[test]
    fn percentile_selection_is_deterministic_and_stable_under_append() {
        let base = vec![steady(80), one_lucky_spike(80)];
        let a = percentile_selection(&base, Utility::MeanReturn, 0.4, 99, 500, 0.1);
        let b = percentile_selection(&base, Utility::MeanReturn, 0.4, 99, 500, 0.1);
        assert_eq!(a, b, "same (data, seed) must reproduce byte-for-byte");

        let mut extended = base.clone();
        extended.push(stream(0.001, 0.001, 80));
        let c = percentile_selection(&extended, Utility::MeanReturn, 0.4, 99, 500, 0.1);
        assert_eq!(
            &c.candidates[..2],
            &a.candidates[..],
            "appending a candidate must not perturb the earlier streams"
        );
    }

    #[test]
    fn degenerate_inputs_are_inert() {
        let empty = percentile_selection(&[], Utility::MeanReturn, 0.5, 1, 100, 0.1);
        assert!(empty.selected.is_none());
        assert!(empty.point_argmax.is_none());
        assert!(empty.agrees_with_point_argmax, "nothing to disagree about");
        assert_eq!(empty.point_winner_optimism, 0.0);

        // A one-point track and a zero-bootstrap budget both fall back to the
        // point estimate rather than inventing a distribution.
        let single = percentile_selection(&[vec![0.01]], Utility::MeanReturn, 0.5, 1, 100, 0.1);
        assert_eq!(single.candidates[0].optimism_gap, 0.0);
        let no_boot = percentile_selection(&[steady(50)], Utility::MeanReturn, 0.5, 1, 0, 0.1);
        assert_eq!(no_boot.candidates[0].optimism_gap, 0.0);
    }

    /// The resampler in this module is a copy of `significance`'s private one.
    /// Recompute `bootstrap_pvalue` through the copy and require exact agreement,
    /// so the two can never drift apart unnoticed.
    #[test]
    fn resampler_reproduces_the_significance_module_path() {
        let data: Vec<f64> = (0..80)
            .map(|i| 0.002 + 0.001 * (i as f64 * 0.7).sin())
            .collect();
        let (seed, n_boot, block_prob) = (42u64, 500usize, 0.1);
        let n = data.len();
        let observed = mean(&data);
        assert!(observed > 0.0, "otherwise bootstrap_pvalue short-circuits");

        let mut rng = SplitMix64::new(seed ^ 0x5DEE_CE66_D8B4_2A57);
        let mut idxs = vec![0usize; n];
        let mut at_least_as_large = 0usize;
        for _ in 0..n_boot {
            fill_block_path(&mut rng, &mut idxs, n, block_prob);
            let mut sum = 0.0;
            for &j in idxs.iter() {
                sum += data[j] - observed;
            }
            if sum / n as f64 >= observed {
                at_least_as_large += 1;
            }
        }
        let mine = (at_least_as_large as f64 + 1.0) / (n_boot as f64 + 1.0);
        assert_eq!(
            mine,
            crate::significance::bootstrap_pvalue(&data, seed, n_boot, block_prob),
            "the copied resampler must walk the identical path"
        );
    }
}
