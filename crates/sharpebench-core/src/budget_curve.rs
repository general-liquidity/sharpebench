//! Luck-robust performance-vs-budget curve: the honest, out-of-sample inversion
//! of an in-distribution scaling law.
//!
//! Some agent benchmarks fit a monotone, saturating law (e.g. a log-sigmoid) to
//! performance measured at increasing interaction budget, and report that curve as
//! a clean "more compute ⇒ more skill" story. That rise is an artifact of measuring
//! **in-distribution**: the agent keeps iterating on the *same* task, so more budget
//! can only help. SharpeBench already ships the single-point version of the same
//! idea, [`crate::composite::CompositeScore::dsr_per_cost`], one scalar of deflated
//! Sharpe per unit of spend. This module is the multi-point generalization, built to
//! the opposite standard: honest, out-of-sample, and deflation-adjusted.
//!
//! **The OOS contract (caller-enforced, as in [`crate::oos`]).** The input is the
//! *same* agent evaluated at a series of increasing training/compute budgets (number
//! of training windows, quantity of history, or the $compute/token channel). Each
//! point carries a slice of **held-out** returns drawn from a window that is
//! **disjoint from that point's training set**: the caller supplies the split and
//! guarantees the disjointness, exactly the convention [`crate::oos::oos_decay`]
//! assumes. A point's Sharpe is therefore genuine generalization, not in-sample fit.
//!
//! **No monotone law is fitted, deliberately.** In trading, out-of-sample
//! performance is typically **non-monotone** in training budget: past some point,
//! more compute buys overfitting, and held-out edge *falls*. Fitting a
//! monotone-saturating curve would paper over exactly the turn-down this benchmark
//! exists to expose. So this module reports the raw curve and the turn-down; it does
//! not smooth, extrapolate, or fit.
//!
//! **Deflated for the search over budgets.** Selecting the best of `N` budget points
//! is itself a search over `N`, a data-snooping channel the single-point score
//! cannot see. The reported peak therefore folds `n_budget_points` into the deflation
//! trial footprint of the peak, exactly as [`crate::composite::score_agent`] folds an
//! agent's declared `in_sample_trials` into `effective_n_trials`. The result,
//! [`BudgetCurveReport::peak_dsr_deflated_for_selection`], is strictly `<=` the naive
//! max, closing the budget-selection snooping loophole with existing machinery.
//!
//! **Reported, never a rank gate.** Everything here is diagnostic. The curve, its
//! peak, and the overfit onset are surfaced for the operator; they do not gate
//! eligibility or rank. Wiring any of this into a rank key would silently recreate
//! the in-sample scaling-law anti-thesis the module is written to refute.

use serde::Serialize;

use crate::deflated_sharpe::{deflated_sharpe_ratio, sharpe_ratio};
use crate::significance::bootstrap_pvalue;

/// Tuning for the budget-curve analysis. All fields default to house constants; the
/// bootstrap seed is fixed so the per-point p-values are reproducible forever.
#[derive(Clone, Debug, Serialize)]
pub struct BudgetCurveOpts {
    /// Periods per year, used only to annualize the reported raw Sharpe for
    /// legibility. The deflated Sharpe is computed on per-period returns and is
    /// never annualized (annualizing a DSR is meaningless).
    pub periods_per_year: f64,
    /// Baseline multiple-testing footprint applied to *every* point's deflated
    /// Sharpe, before the budget-selection surcharge is folded into the peak.
    pub base_n_trials: u32,
    /// Cross-trial dispersion of Sharpe ratios (the deflation footprint's scale),
    /// matching [`crate::composite::ScoreConfig::trials_sr_std`].
    pub trials_sr_std: f64,
    /// Seed for the per-point significance bootstrap (fixed ⇒ reproducible).
    pub bootstrap_seed: u64,
    /// Bootstrap resample count for the per-point p-value.
    pub n_boot: usize,
    /// Stationary-bootstrap block-continuation probability.
    pub block_prob: f64,
}

impl Default for BudgetCurveOpts {
    fn default() -> Self {
        Self {
            periods_per_year: 252.0,
            base_n_trials: 1,
            trials_sr_std: 0.5,
            bootstrap_seed: 0x5BA7_2026,
            n_boot: 2000,
            block_prob: 0.1,
        }
    }
}

/// One budget point's out-of-sample readout.
#[derive(Clone, Debug, Serialize)]
pub struct BudgetPoint {
    /// The budget (training windows, history quantity, or $compute/token spend).
    pub budget: f64,
    /// Number of held-out returns backing this point.
    pub n_returns: usize,
    /// Deflated Sharpe on this point's held-out returns, at the baseline footprint
    /// (`opts.base_n_trials`). The luck-robust, out-of-sample skill at this budget.
    pub oos_dsr: f64,
    /// Raw (per-period, non-annualized) Sharpe on the held-out returns, reported
    /// for context alongside the deflated figure, never a gate.
    pub oos_sharpe: f64,
    /// `oos_sharpe` scaled by `sqrt(periods_per_year)`; legibility only.
    pub oos_sharpe_annualized: f64,
    /// Bootstrap p-value that this point's held-out edge is real (not luck).
    pub oos_p_value: f64,
    /// `(oos_dsr[i] - oos_dsr[i-1]) / (budget[i] - budget[i-1])`: the multi-point
    /// generalization of the single-point `dsr_per_cost`. `None` for the first
    /// point. `<= 0` means more budget bought *no* extra held-out edge: the
    /// overfit signature single-point scoring cannot see.
    pub marginal_dsr_per_budget: Option<f64>,
}

/// The full curve-level report.
#[derive(Clone, Debug, Serialize)]
pub struct BudgetCurveReport {
    /// Per-point readouts, in input (increasing-budget) order.
    pub points: Vec<BudgetPoint>,
    /// Number of budget points (the size of the budget-selection search).
    pub n_budget_points: usize,
    /// Budget at which `oos_dsr` is maximized (the first such budget on ties).
    pub peak_budget: f64,
    /// The naive maximum `oos_dsr` over the curve (at the baseline footprint).
    pub peak_dsr: f64,
    /// The peak's deflated Sharpe recomputed with `n_budget_points` folded into the
    /// trial footprint (`base_n_trials + n_budget_points`): the honest peak that
    /// pays for the search over budgets. Strictly `<=` [`Self::peak_dsr`].
    pub peak_dsr_deflated_for_selection: f64,
    /// The first budget where `marginal_dsr_per_budget <= 0` (more compute lowered
    /// held-out edge). `None` when the curve never turns down. This is the overfit
    /// onset a monotone-fit scaling law structurally cannot report.
    pub overfit_onset: Option<f64>,
    /// Whether `oos_dsr` strictly increases across *every* consecutive point: a
    /// genuinely monotone-improving curve, which is rare and honest in trading.
    pub is_monotone_improving: bool,
}

/// Compute the luck-robust budget curve from ordered `(budget, held_out_returns)`
/// points: the same agent at increasing budget, each point's returns drawn from a
/// window disjoint from that point's training set (the caller enforces the split).
///
/// # Errors
///
/// Returns `Err` at the boundary when: the input is empty or has a single point (a
/// curve needs at least two); the budgets are not strictly increasing; or any point
/// has fewer than two held-out returns (a Sharpe needs dispersion).
pub fn budget_curve(
    points: &[(f64, &[f64])],
    opts: &BudgetCurveOpts,
) -> Result<BudgetCurveReport, String> {
    if points.len() < 2 {
        return Err(format!(
            "at least two budget points are required, got {}",
            points.len()
        ));
    }
    let n_budget_points = points.len();

    for i in 0..n_budget_points {
        let (budget, returns) = points[i];
        if returns.len() < 2 {
            return Err(format!(
                "point {i} (budget {budget}) has {} held-out returns; at least 2 are required",
                returns.len()
            ));
        }
        if i > 0 {
            let prev = points[i - 1].0;
            if budget <= prev {
                return Err(format!(
                    "budgets must be strictly increasing: point {i} budget {budget} is not > point {} budget {prev}",
                    i - 1
                ));
            }
        }
    }

    let ann = opts.periods_per_year.max(0.0).sqrt();
    let mut curve: Vec<BudgetPoint> = Vec::with_capacity(n_budget_points);
    for i in 0..n_budget_points {
        let (budget, returns) = points[i];
        let oos_dsr = deflated_sharpe_ratio(returns, opts.base_n_trials, opts.trials_sr_std);
        let oos_sharpe = sharpe_ratio(returns);
        let oos_p_value =
            bootstrap_pvalue(returns, opts.bootstrap_seed, opts.n_boot, opts.block_prob);
        let marginal_dsr_per_budget = if i == 0 {
            None
        } else {
            let db = budget - points[i - 1].0;
            Some((oos_dsr - curve[i - 1].oos_dsr) / db)
        };
        curve.push(BudgetPoint {
            budget,
            n_returns: returns.len(),
            oos_dsr,
            oos_sharpe,
            oos_sharpe_annualized: oos_sharpe * ann,
            oos_p_value,
            marginal_dsr_per_budget,
        });
    }

    // Peak = argmax oos_dsr, first occurrence on ties (so an improving-then-plateau
    // curve peaks at the *start* of the plateau, not somewhere along the flat tail).
    let mut peak_idx = 0usize;
    for i in 1..n_budget_points {
        if curve[i].oos_dsr > curve[peak_idx].oos_dsr {
            peak_idx = i;
        }
    }
    let peak_budget = curve[peak_idx].budget;
    let peak_dsr = curve[peak_idx].oos_dsr;

    // Fold the budget-selection search into the peak's deflation footprint, exactly
    // as composite.rs folds `in_sample_trials`: selecting the best of N budgets is a
    // search over N, so the honest peak clears a higher bar.
    let peak_footprint = opts.base_n_trials.saturating_add(n_budget_points as u32);
    let peak_dsr_deflated_for_selection =
        deflated_sharpe_ratio(points[peak_idx].1, peak_footprint, opts.trials_sr_std);

    // First budget where more compute did not raise held-out edge.
    let overfit_onset = curve
        .iter()
        .skip(1)
        .find(|p| matches!(p.marginal_dsr_per_budget, Some(m) if m <= 0.0))
        .map(|p| p.budget);

    // Strictly rising across every step ⇒ genuinely monotone-improving.
    let is_monotone_improving = curve
        .iter()
        .skip(1)
        .all(|p| matches!(p.marginal_dsr_per_budget, Some(m) if m > 0.0));

    Ok(BudgetCurveReport {
        points: curve,
        n_budget_points,
        peak_budget,
        peak_dsr,
        peak_dsr_deflated_for_selection,
        overfit_onset,
        is_monotone_improving,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Deterministic pseudo-return window: a mean drift plus a sinusoidal wiggle so
    /// the Sharpe is finite and mid-range (no RNG ⇒ reproducible). Mirrors the
    /// generator used across composite.rs's tests.
    fn window(mean_ret: f64, amp: f64, n: usize) -> Vec<f64> {
        (0..n)
            .map(|i| mean_ret + amp * (i as f64 * 0.7).sin())
            .collect()
    }

    /// Assemble a `&[(f64, &[f64])]` from owned windows without lifetime pain.
    fn curve_of(pts: &[(f64, Vec<f64>)]) -> Vec<(f64, &[f64])> {
        pts.iter().map(|(b, w)| (*b, w.as_slice())).collect()
    }

    #[test]
    fn overfitting_curve_flags_the_turn_down_before_the_max_budget() {
        // Held-out edge rises then falls as budget grows: the classic overfit arc.
        // Amplitude is held constant (identical skew/kurtosis ⇒ the DSR is strictly
        // monotone in the mean), so the mean alone drives a clean, non-saturated arc.
        let pts = vec![
            (1.0, window(0.0007, 0.02, 40)),
            (2.0, window(0.0014, 0.02, 40)),
            (3.0, window(0.0020, 0.02, 40)), // peak
            (4.0, window(0.0013, 0.02, 40)),
            (5.0, window(0.0006, 0.02, 40)), // max budget, worst edge
        ];
        let c = curve_of(&pts);
        let r = budget_curve(&c, &BudgetCurveOpts::default()).unwrap();

        assert_eq!(r.peak_budget, 3.0, "peak is the interior budget: {r:?}");
        assert!(
            r.peak_budget < 5.0,
            "peak must precede the max budget (the EdgeBench differentiator)"
        );
        assert_eq!(
            r.overfit_onset,
            Some(4.0),
            "onset is the first budget where held-out edge stopped rising"
        );
        assert!(!r.is_monotone_improving, "a turn-down is not monotone");
    }

    #[test]
    fn improving_then_plateauing_curve_has_no_early_onset() {
        // Edge climbs then holds flat: honest saturation, not overfitting.
        let plateau = window(0.0020, 0.02, 40);
        let pts = vec![
            (1.0, window(0.0007, 0.02, 40)),
            (2.0, window(0.0014, 0.02, 40)),
            (3.0, plateau.clone()),
            (4.0, plateau.clone()),
            (5.0, plateau),
        ];
        let c = curve_of(&pts);
        let r = budget_curve(&c, &BudgetCurveOpts::default()).unwrap();

        assert_eq!(r.peak_budget, 3.0, "peak sits at the plateau start: {r:?}");
        // The only non-positive marginal is the flat tail (identical windows ⇒
        // marginal exactly 0), never an interior turn-down.
        match r.overfit_onset {
            None => {}
            Some(b) => assert!(
                b >= 4.0,
                "any onset must be on the flat tail (budget {b}), not before the plateau"
            ),
        }
        assert!(
            !r.is_monotone_improving,
            "a flat tail is not strictly rising"
        );
    }

    #[test]
    fn deflation_for_selection_strictly_raises_the_bar() {
        // Any curve with >= 2 points: selecting the best budget is a search over N,
        // so the selection-deflated peak must sit strictly below the naive max.
        let pts = vec![
            (1.0, window(0.0010, 0.02, 40)),
            (2.0, window(0.0014, 0.02, 40)),
            (3.0, window(0.0012, 0.02, 40)),
        ];
        let c = curve_of(&pts);
        let r = budget_curve(&c, &BudgetCurveOpts::default()).unwrap();
        assert!(
            r.peak_dsr_deflated_for_selection < r.peak_dsr,
            "selection deflation must strictly lower the peak: {} !< {}",
            r.peak_dsr_deflated_for_selection,
            r.peak_dsr
        );
    }

    #[test]
    fn marginal_is_none_first_then_tracks_the_dsr_sign() {
        let pts = vec![
            (1.0, window(0.0007, 0.02, 40)),
            (2.0, window(0.0020, 0.02, 40)), // DSR up ⇒ marginal > 0
            (3.0, window(0.0005, 0.02, 40)), // DSR down ⇒ marginal < 0
        ];
        let c = curve_of(&pts);
        let r = budget_curve(&c, &BudgetCurveOpts::default()).unwrap();

        assert!(
            r.points[0].marginal_dsr_per_budget.is_none(),
            "first is None"
        );

        let m1 = r.points[1].marginal_dsr_per_budget.unwrap();
        let d1 = r.points[1].oos_dsr - r.points[0].oos_dsr;
        assert!(m1 > 0.0 && d1 > 0.0, "rising step ⇒ positive marginal");
        assert_eq!(
            m1.signum(),
            d1.signum(),
            "marginal sign tracks the DSR delta"
        );

        let m2 = r.points[2].marginal_dsr_per_budget.unwrap();
        let d2 = r.points[2].oos_dsr - r.points[1].oos_dsr;
        assert!(m2 < 0.0 && d2 < 0.0, "falling step ⇒ negative marginal");
        assert_eq!(
            m2.signum(),
            d2.signum(),
            "marginal sign tracks the DSR delta"
        );
    }

    #[test]
    fn monotone_improving_true_only_for_a_strictly_rising_curve() {
        let rising_pts = [
            (1.0, window(0.0006, 0.02, 40)),
            (2.0, window(0.0013, 0.02, 40)),
            (3.0, window(0.0020, 0.02, 40)),
        ];
        let rising = curve_of(&rising_pts);
        let r = budget_curve(&rising, &BudgetCurveOpts::default()).unwrap();
        assert!(r.is_monotone_improving, "strictly rising DSR ⇒ true: {r:?}");
        assert!(r.overfit_onset.is_none(), "a rising curve never turns down");

        // Insert a dip ⇒ no longer monotone.
        let dipped_pts = [
            (1.0, window(0.0006, 0.02, 40)),
            (2.0, window(0.0020, 0.02, 40)),
            (3.0, window(0.0013, 0.02, 40)),
        ];
        let dipped = curve_of(&dipped_pts);
        let r2 = budget_curve(&dipped, &BudgetCurveOpts::default()).unwrap();
        assert!(!r2.is_monotone_improving, "a dip breaks monotonicity");
    }

    #[test]
    fn degenerate_inputs_error_cleanly() {
        let opts = BudgetCurveOpts::default();

        // Empty.
        assert!(budget_curve(&[], &opts).is_err());

        // Single point.
        let one = window(0.005, 0.005, 40);
        assert!(budget_curve(&[(1.0, one.as_slice())], &opts).is_err());

        // Non-increasing budgets.
        let a = window(0.005, 0.005, 40);
        let b = window(0.005, 0.005, 40);
        assert!(budget_curve(&[(2.0, a.as_slice()), (2.0, b.as_slice())], &opts).is_err());
        assert!(budget_curve(&[(3.0, a.as_slice()), (1.0, b.as_slice())], &opts).is_err());

        // A point with an empty / too-short return window.
        let full = window(0.005, 0.005, 40);
        let empty: Vec<f64> = Vec::new();
        assert!(budget_curve(&[(1.0, full.as_slice()), (2.0, empty.as_slice())], &opts).is_err());
        let single = vec![0.01];
        assert!(budget_curve(&[(1.0, full.as_slice()), (2.0, single.as_slice())], &opts).is_err());
    }

    #[test]
    fn annualized_sharpe_scales_the_raw_sharpe() {
        let pts_owned = [
            (1.0, window(0.005, 0.008, 80)),
            (2.0, window(0.006, 0.007, 80)),
        ];
        let pts = curve_of(&pts_owned);
        let opts = BudgetCurveOpts {
            periods_per_year: 252.0,
            ..BudgetCurveOpts::default()
        };
        let r = budget_curve(&pts, &opts).unwrap();
        let p0 = &r.points[0];
        assert!(
            (p0.oos_sharpe_annualized - p0.oos_sharpe * 252.0_f64.sqrt()).abs() < 1e-12,
            "annualized = raw * sqrt(periods_per_year)"
        );
    }
}
