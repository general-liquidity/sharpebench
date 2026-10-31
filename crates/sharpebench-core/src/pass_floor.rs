//! The Sharpe a run needs to clear the per-run bar of pass^k, per declared
//! window, so a window no realistic track can pass is told apart from a field
//! that failed.
//!
//! pass^k's per-run test is `PSR(returns, b) >= per_run_psr_bar`, and PSR's
//! z-statistic is `(SR - b) * sqrt(n - 1)` scaled by a skew and kurtosis term.
//! Dropping that term, a run of `n` bars clears the bar only when its
//! per-period Sharpe reaches
//!
//! ```text
//! floor(n) = b + Z(per_run_psr_bar) / sqrt(n - 1)
//! ```
//!
//! which [`pass_floor_annualized`] reports annualized with `periods_per_year`.
//! It is the table in `docs/book/src/methodology-pass-k.md` computed for the
//! windows a run actually declared (a test ties the two together). Under
//! normal moments the dropped term is `sqrt(1 + SR^2 / 2) >= 1`, so the exact
//! requirement is at least this floor; a positively skewed run can clear the
//! bar slightly below it. It is a disclosure about the window, not a gate, and
//! it moves no score or rank.
//!
//! Under the relative verdict the series under test is the excess over the
//! benchmark's run, so the floor is a floor on the excess Sharpe. The formula
//! is the same.
//!
//! Pure and deterministic: no I/O, no clock.

use sharpebench_stats::stats::norm_ppf;

use crate::composite::{per_run_psr_benchmark, Run, ScoreConfig};
use crate::pass_k::PassMode;

/// The annualized Sharpe above which a window's floor is flagged as out of
/// practical reach. A stated bound, not a measurement: it is chosen so that
/// the floors the methodology chapter tabulates for 78 daily bars (2.32) read
/// as out of reach and those for 250 (1.29) and 1300 (0.56) do not. It decides
/// a flag only.
pub const PASS_FLOOR_PLAUSIBLE_MAX_ANNUAL_SHARPE: f64 = 2.0;

/// The floor, annualized, for one run of `bars` bars under `cfg`, or `None`
/// when no finite floor exists: the bar is at or below zero (every run clears
/// it, see [`window_is_plausible`]) or is unreachable (a bar of one or more, a
/// non-finite bar or frequency, or a run too short to have a Sharpe ratio).
pub fn pass_floor_annualized(bars: usize, cfg: &ScoreConfig) -> Option<f64> {
    let bar = cfg.per_run_psr_bar;
    let periods = cfg.periods_per_year;
    if !bar.is_finite() || bar <= 0.0 || bar >= 1.0 || bars < 2 {
        return None;
    }
    if !periods.is_finite() || periods <= 0.0 {
        return None;
    }
    let per_period = per_run_psr_benchmark(cfg) + norm_ppf(bar) / ((bars - 1) as f64).sqrt();
    let annualized = per_period * periods.sqrt();
    annualized.is_finite().then_some(annualized)
}

/// Whether a run of `bars` bars can clear the per-run bar in practice: its
/// floor exists and is at most [`PASS_FLOOR_PLAUSIBLE_MAX_ANNUAL_SHARPE`], or
/// the bar is at or below zero, which every PSR clears.
pub fn window_is_plausible(bars: usize, cfg: &ScoreConfig) -> bool {
    let bar = cfg.per_run_psr_bar;
    if bar.is_finite() && bar <= 0.0 {
        return true;
    }
    pass_floor_annualized(bars, cfg).is_some_and(|f| f <= PASS_FLOOR_PLAUSIBLE_MAX_ANNUAL_SHARPE)
}

/// One declared window: its length and its floor.
#[derive(Clone, Debug, PartialEq)]
pub struct WindowPassFloor {
    /// The shortest run in the window, which sets the window's floor.
    pub bars: usize,
    /// [`pass_floor_annualized`] at `bars`.
    pub floor_annualized_sharpe: Option<f64>,
    /// [`window_is_plausible`] at `bars`.
    pub plausible: bool,
    /// Runs in the window.
    pub runs: usize,
}

/// The floor of every declared window of `runs`, in window order. Runs are
/// window-major with `cfg.execution_seeds_per_window` seeds per window (at
/// least one); a short final group is its own window.
pub fn window_pass_floors(runs: &[Run], cfg: &ScoreConfig) -> Vec<WindowPassFloor> {
    let width = cfg.execution_seeds_per_window.max(1);
    // inv: one entry per window visited, in order; ends after
    // ceil(runs.len() / width) chunks.
    runs.chunks(width)
        .map(|window| {
            let bars = window.iter().map(|r| r.returns.len()).min().unwrap_or(0);
            WindowPassFloor {
                bars,
                floor_annualized_sharpe: pass_floor_annualized(bars, cfg),
                plausible: window_is_plausible(bars, cfg),
                runs: window.len(),
            }
        })
        .collect()
}

/// Whether pass^k under `mode` is within practical reach on these windows:
/// enough runs sit in plausible windows to meet the mode's count. Under
/// [`PassMode::All`] and [`PassMode::RelativeToBenchmark`] that is every run,
/// so one implausible window puts the verdict out of practical reach for any
/// entrant (a run can still clear an implausible floor; the flag says the bar
/// was unusually high, not that nothing passed). No windows is out of reach,
/// as pass^k of no runs is a failure.
pub fn pass_k_floors_plausible(floors: &[WindowPassFloor], mode: PassMode) -> bool {
    let total: usize = floors.iter().map(|w| w.runs).sum();
    let reachable: usize = floors.iter().filter(|w| w.plausible).map(|w| w.runs).sum();
    if total == 0 {
        return false;
    }
    match mode {
        PassMode::All | PassMode::RelativeToBenchmark => reachable == total,
        PassMode::Any => reachable >= 1,
        PassMode::AtLeast(k) => reachable >= k,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn runs(lengths: &[usize]) -> Vec<Run> {
        lengths
            .iter()
            .map(|&n| Run {
                returns: vec![0.0; n],
                ..Run::default()
            })
            .collect()
    }

    /// The row publishes how many of the k runs passed beside the bool, and
    /// the floors of its windows, from the same per-run vector pass^k reads.
    #[test]
    fn a_scored_row_carries_its_pass_count_and_window_floors() {
        use crate::composite::{per_run_passes, score_agent, AgentSubmission};
        let strong = |n: usize| Run {
            returns: (0..n)
                .map(|i| 0.5 + if i % 2 == 0 { 1.0 } else { -1.0 })
                .collect(),
            ..Run::default()
        };
        let flat = |n: usize| Run {
            returns: (0..n)
                .map(|i| if i % 2 == 0 { 1.0 } else { -1.0 })
                .collect(),
            ..Run::default()
        };
        let sub = AgentSubmission {
            agent_id: "mixed".into(),
            runs: vec![strong(250), flat(250), strong(78)],
            ..AgentSubmission::default()
        };
        let cfg = ScoreConfig::for_periods_per_year(252.0);
        let score = score_agent(&sub, &cfg);
        let passes = per_run_passes(&sub, None, &cfg);
        assert_eq!(passes, [true, false, true]);
        assert!(!score.passed_k);
        assert_eq!(score.runs_passed, Some(2));
        assert_eq!(score.runs_scored, 3, "k");
        assert_eq!(score.window_bars, [250, 250, 78]);
        assert_eq!(
            score.window_pass_floors,
            [250, 250, 78].map(|n| pass_floor_annualized(n, &cfg))
        );
        assert_eq!(
            score.implausible_windows,
            [2],
            "78 daily bars need about 2.32"
        );
        assert_eq!(score.pass_k_floors_plausible, Some(false));
    }

    /// The methodology chapter's table, read from the book, is what this module
    /// computes: each row's bar count, at the default bar on daily bars, rounds
    /// to the tabulated annualized Sharpe. A change on either side fails here.
    #[test]
    fn the_floor_reproduces_the_methodology_table() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../docs/book/src/methodology-pass-k.md");
        let text = std::fs::read_to_string(&path).expect("the methodology chapter is readable");
        let cfg = ScoreConfig::for_periods_per_year(252.0);
        let mut rows = 0;
        for line in text.lines() {
            let cells: Vec<&str> = line.split('|').map(str::trim).collect();
            let [_, bars, sharpe, _] = cells.as_slice() else {
                continue;
            };
            let (Some(bars), Ok(sharpe)) = (
                bars.strip_suffix(" bars")
                    .and_then(|b| b.parse::<usize>().ok()),
                sharpe.parse::<f64>(),
            ) else {
                continue;
            };
            let floor = pass_floor_annualized(bars, &cfg).expect("a finite floor");
            assert_eq!(format!("{floor:.2}"), format!("{sharpe:.2}"), "{bars} bars");
            rows += 1;
        }
        assert_eq!(rows, 3, "the chapter's three tabulated window lengths");
    }

    /// The floor is the kernel's own boundary: a run just above it passes the
    /// per-run test and one just below fails, on normal-like returns.
    #[test]
    fn a_run_at_the_floor_sits_on_the_kernel_boundary() {
        use sharpebench_stats::deflated_sharpe::checked_probabilistic_sharpe_ratio;
        let cfg = ScoreConfig::for_periods_per_year(252.0);
        let n = 250;
        // A symmetric two-point series has zero skew; its kurtosis term is
        // small at these Sharpe ratios, so the exact boundary sits just above
        // the floor.
        let floor = pass_floor_annualized(n, &cfg).unwrap() / 252f64.sqrt();
        let series = |sr: f64| -> Vec<f64> {
            (0..n)
                .map(|i| sr + if i % 2 == 0 { 1.0 } else { -1.0 })
                .collect()
        };
        let psr = |sr| checked_probabilistic_sharpe_ratio(&series(sr), 0.0).unwrap();
        assert!(psr(floor * 0.98) < cfg.per_run_psr_bar);
        assert!(psr(floor * 1.05) >= cfg.per_run_psr_bar);
    }

    #[test]
    fn edge_bars_and_bars_without_a_finite_floor() {
        let cfg = ScoreConfig::default();
        assert_eq!(pass_floor_annualized(0, &cfg), None);
        assert_eq!(pass_floor_annualized(1, &cfg), None);
        assert!(!window_is_plausible(1, &cfg), "one bar has no Sharpe ratio");
        for bar in [1.0, f64::NAN, f64::INFINITY] {
            let cfg = ScoreConfig {
                per_run_psr_bar: bar,
                ..ScoreConfig::default()
            };
            assert_eq!(pass_floor_annualized(250, &cfg), None, "{bar}");
            assert!(!window_is_plausible(250, &cfg), "{bar}");
        }
        let lenient = ScoreConfig {
            per_run_psr_bar: 0.0,
            ..ScoreConfig::default()
        };
        assert_eq!(pass_floor_annualized(250, &lenient), None);
        assert!(
            window_is_plausible(1, &lenient),
            "every PSR clears a zero bar"
        );
        let bad_frequency = ScoreConfig {
            periods_per_year: f64::INFINITY,
            ..ScoreConfig::default()
        };
        assert_eq!(pass_floor_annualized(250, &bad_frequency), None);
    }

    /// A benchmark raises the floor by exactly the annualized minimum.
    #[test]
    fn the_floor_adds_the_annualized_minimum() {
        let base = ScoreConfig::for_periods_per_year(252.0);
        let raised = ScoreConfig {
            per_run_min_annual_sharpe: 0.5,
            ..base.clone()
        };
        let gap = pass_floor_annualized(250, &raised).unwrap()
            - pass_floor_annualized(250, &base).unwrap();
        assert!((gap - 0.5).abs() < 1e-12, "{gap}");
    }

    /// One short window puts pass^k out of practical reach under the modes that need
    /// every run, but not under one that needs a single run.
    #[test]
    fn an_implausible_window_decides_reach_by_mode() {
        let cfg = ScoreConfig {
            execution_seeds_per_window: 2,
            ..ScoreConfig::for_periods_per_year(252.0)
        };
        let floors = window_pass_floors(&runs(&[1300, 1300, 78, 80]), &cfg);
        assert_eq!(floors.len(), 2);
        assert_eq!(floors[1].bars, 78, "the shortest run sets the window");
        assert!(floors[0].plausible && !floors[1].plausible);
        assert!(!pass_k_floors_plausible(&floors, PassMode::All));
        assert!(!pass_k_floors_plausible(
            &floors,
            PassMode::RelativeToBenchmark
        ));
        assert!(pass_k_floors_plausible(&floors, PassMode::Any));
        assert!(pass_k_floors_plausible(&floors, PassMode::AtLeast(2)));
        assert!(!pass_k_floors_plausible(&floors, PassMode::AtLeast(3)));
        assert!(!pass_k_floors_plausible(&[], PassMode::Any));
        // A short final group is its own window.
        let ragged = window_pass_floors(&runs(&[250, 250, 250]), &cfg);
        assert_eq!(ragged.iter().map(|w| w.runs).collect::<Vec<_>>(), [2, 1]);
    }
}
