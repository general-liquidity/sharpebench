//! An opt-in multi-objective report for a scored board: the Pareto front over
//! six axes, among every agent and among the rank-eligible ones only, with a
//! bootstrap note of how robust each agent's membership is to sampling noise.
//!
//! The board ranks on one scalar, the deflated Sharpe (or alpha), on purpose:
//! it is the luck-robust choice the benchmark exists to make, and a front over
//! unadjusted axes is exactly what a lucky track can win. This report is
//! therefore **never a rank**. Its rows are in board order, nothing on the board
//! reads it, and the board's own `pareto_optimal` flag (return, drawdown and
//! turnover, in the goldens) is left as it is.
//!
//! The axes, each with its direction:
//!
//! | axis | better |
//! |---|---|
//! | `mean_return`, mean per-period return of the pooled track | higher |
//! | `max_drawdown` of the pooled track | lower |
//! | `turnover`, orders per run (the board's column) | lower |
//! | `cost`, total execution cost (the board's column) | lower |
//! | `volatility`, sample standard deviation of the pooled track | lower |
//! | `expected_shortfall`, historical ES at 5% of the pooled track, as a return | higher |
//!
//! `a` dominates `b` when `a` is at least as good on every axis and strictly
//! better on one; two rows equal on every axis dominate neither, so both can be
//! on the front. A row takes part only when every axis is a finite number: an
//! expected shortfall the tail is too thin to estimate, or a non-finite value,
//! leaves the row out with the reason, and it dominates nobody. A row whose
//! pooled track has no Sharpe ratio is not a front member, as on the board, but
//! it still dominates when its axes are finite, so an agent a never-trading
//! track beats on every axis is not optimal.
//!
//! The uncertainty note resamples every comparable pooled track with the
//! crate's stationary bootstrap ([`sharpebench_stats::stationary_bootstrap_path`]),
//! one seeded index path per replicate shared by tracks of the same length so
//! aligned tracks keep their cross-correlation, recomputes the four
//! return-derived axes (turnover and cost are counts of what the agent did and
//! are held fixed), and reports the fraction of replicates in which each member
//! stays non-dominated. A front membership that holds in a small fraction of
//! replicates is not evidence of a better trade-off.
//!
//! Pure and deterministic: no I/O, no clock.

use std::collections::HashMap;

use serde::Serialize;
use sharpebench_stats::deflated_sharpe::observed_sharpe_ratio_of_windows;
use sharpebench_stats::stats::{mean, std_dev};
use sharpebench_stats::{
    historical_expected_shortfall, stationary_bootstrap_path, DEFAULT_MIN_TAIL_OBSERVATIONS,
    DEFAULT_TAIL_LEVEL,
};

use crate::composite::{
    max_drawdown, restrict_to_shared_positions, window_tracks, AgentSubmission, CompositeScore,
    ScoreConfig,
};

/// One axis of the front.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ParetoAxis {
    MeanReturn,
    MaxDrawdown,
    Turnover,
    Cost,
    Volatility,
    ExpectedShortfall,
}

impl ParetoAxis {
    /// Every axis, in the order [`ParetoAxes::values`] lists them.
    pub const ALL: [ParetoAxis; 6] = [
        ParetoAxis::MeanReturn,
        ParetoAxis::MaxDrawdown,
        ParetoAxis::Turnover,
        ParetoAxis::Cost,
        ParetoAxis::Volatility,
        ParetoAxis::ExpectedShortfall,
    ];

    /// Whether a larger value is better on this axis.
    pub fn higher_is_better(self) -> bool {
        matches!(self, ParetoAxis::MeanReturn | ParetoAxis::ExpectedShortfall)
    }

    fn name(self) -> &'static str {
        match self {
            ParetoAxis::MeanReturn => "mean_return",
            ParetoAxis::MaxDrawdown => "max_drawdown",
            ParetoAxis::Turnover => "turnover",
            ParetoAxis::Cost => "cost",
            ParetoAxis::Volatility => "volatility",
            ParetoAxis::ExpectedShortfall => "expected_shortfall",
        }
    }
}

/// One row's position on the six axes. A value that could not be computed is
/// absent.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize)]
pub struct ParetoAxes {
    pub mean_return: Option<f64>,
    pub max_drawdown: Option<f64>,
    pub turnover: Option<f64>,
    pub cost: Option<f64>,
    pub volatility: Option<f64>,
    pub expected_shortfall: Option<f64>,
}

impl ParetoAxes {
    /// The six values in [`ParetoAxis::ALL`] order.
    pub fn values(&self) -> [Option<f64>; 6] {
        [
            self.mean_return,
            self.max_drawdown,
            self.turnover,
            self.cost,
            self.volatility,
            self.expected_shortfall,
        ]
    }

    /// The six values when every one is a finite number, else the first axis
    /// that is not.
    fn comparable(&self) -> Result<[f64; 6], ParetoAxis> {
        let mut out = [0.0; 6];
        for ((slot, value), axis) in out.iter_mut().zip(self.values()).zip(ParetoAxis::ALL) {
            match value {
                Some(v) if v.is_finite() => *slot = v,
                _ => return Err(axis),
            }
        }
        Ok(out)
    }
}

/// Whether `a` Pareto-dominates `b`: at least as good on every axis and
/// strictly better on one. Both must be finite (see [`ParetoAxes`]); two equal
/// rows dominate neither.
pub fn dominates(a: &[f64; 6], b: &[f64; 6]) -> bool {
    let mut strictly = false;
    for ((x, y), axis) in a.iter().zip(b).zip(ParetoAxis::ALL) {
        let (better, worse) = if axis.higher_is_better() {
            (x > y, x < y)
        } else {
            (x < y, x > y)
        };
        if worse {
            return false;
        }
        strictly |= better;
    }
    strictly
}

/// The resampling the uncertainty note uses.
#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub struct ParetoBootstrap {
    /// Replicates drawn. Zero turns the note off.
    pub n_boot: usize,
    pub seed: u64,
    /// Stationary-bootstrap restart probability (expected block `1 / p`).
    pub block_prob: f64,
}

impl ParetoBootstrap {
    /// The board's own resampling settings: `cfg.n_boot`, `cfg.bootstrap_seed`
    /// and `cfg.block_prob`.
    pub fn from_config(cfg: &ScoreConfig) -> Self {
        Self {
            n_boot: cfg.n_boot,
            seed: cfg.bootstrap_seed,
            block_prob: cfg.block_prob,
        }
    }
}

/// One board row in the report.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ParetoRow {
    pub agent_id: String,
    /// The row's `rank_eligible`, copied so the eligible-only front reads
    /// without the board.
    pub rank_eligible: bool,
    pub axes: ParetoAxes,
    /// Why the row is not a front member, when it is not one: no submission,
    /// no Sharpe ratio, or the first axis that is not a finite number.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub refused: Option<String>,
    /// On the front among every comparable row.
    pub on_front: bool,
    /// On the front among the rank-eligible rows only. `None` for a row that is
    /// not rank-eligible.
    pub on_eligible_front: Option<bool>,
    /// The rows that dominate this one on the all-agent front, in board order.
    pub dominated_by: Vec<String>,
    /// Fraction of bootstrap replicates in which the row is on the all-agent
    /// front. `None` for a non-member or when no replicate was drawn.
    pub front_frequency: Option<f64>,
    /// The same on the eligible-only front. `None` for a row that is not
    /// rank-eligible, a non-member, or when no replicate was drawn.
    pub eligible_front_frequency: Option<f64>,
}

/// The report: a separate record beside the board, never written into it.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ParetoReport {
    /// Always `false`: no gate, eligibility rule or rank predicate reads this
    /// report, and the board is not reordered by it.
    pub used_by_rank: bool,
    pub axes: Vec<ParetoAxis>,
    pub bootstrap: ParetoBootstrap,
    /// Why the uncertainty note is absent, when it is: no replicate requested,
    /// or a resampling setting the bootstrap refuses.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bootstrap_error: Option<String>,
    pub rows: Vec<ParetoRow>,
}

/// Which rows of `points` are non-dominated among `candidates`, against every
/// row in `dominators`. `points[i]` is `None` for a row that is not comparable.
fn front(points: &[Option<[f64; 6]>], candidates: &[bool], dominators: &[bool]) -> Vec<bool> {
    // inv: out[i] holds for the rows before i exactly when row i is a candidate
    // with a point that no other dominator's point dominates.
    (0..points.len())
        .map(|i| {
            candidates[i]
                && points[i].is_some_and(|p| {
                    !(0..points.len()).any(|j| {
                        j != i && dominators[j] && points[j].is_some_and(|q| dominates(&q, &p))
                    })
                })
        })
        .collect()
}

/// The four return-derived axes of a pooled track, plus the two board counts.
fn axes_of(pooled: &[f64], turnover: f64, cost: f64) -> (ParetoAxes, Option<String>) {
    let shortfall =
        historical_expected_shortfall(pooled, DEFAULT_TAIL_LEVEL, DEFAULT_MIN_TAIL_OBSERVATIONS);
    let axes = ParetoAxes {
        mean_return: Some(mean(pooled)),
        max_drawdown: Some(max_drawdown(pooled)),
        turnover: Some(turnover),
        cost: Some(cost),
        volatility: Some(std_dev(pooled)),
        expected_shortfall: shortfall.as_ref().ok().map(|es| es.tail_mean_return),
    };
    (axes, shortfall.err().map(|e| e.to_string()))
}

/// Compute the report for every row of `board`, in board order.
///
/// `subs` and `cfg` must be the field and configuration the board was ranked
/// from: the pooled track is rebuilt as the board builds it (the shared-cell
/// restriction when `cfg.shared_run_set` is on, then replicate averaging at
/// `cfg.execution_seeds_per_window`), each row matched to its submission by
/// `agent_id`, first match. The board is read, never written.
pub fn pareto_report(
    subs: &[AgentSubmission],
    board: &[CompositeScore],
    cfg: &ScoreConfig,
    bootstrap: ParetoBootstrap,
) -> ParetoReport {
    let restricted;
    let field: &[AgentSubmission] = if cfg.shared_run_set {
        restricted = restrict_to_shared_positions(subs);
        &restricted
    } else {
        subs
    };
    let seeds = cfg.execution_seeds_per_window;
    let mut pooled: Vec<Vec<f64>> = Vec::with_capacity(board.len());
    let mut rows: Vec<ParetoRow> = Vec::with_capacity(board.len());
    for row in board {
        let submission = field.iter().find(|s| s.agent_id == row.agent_id);
        let tracks = submission.map(|s| window_tracks(s, seeds));
        let track: Vec<f64> = tracks.as_ref().map(|t| t.concat()).unwrap_or_default();
        let (axes, shortfall_error) = axes_of(&track, row.turnover, row.cost);
        let sharpe = tracks.as_ref().map(|t| {
            let slices: Vec<&[f64]> = t.iter().map(Vec::as_slice).collect();
            observed_sharpe_ratio_of_windows(&slices).map(|_| ())
        });
        // The first reason that applies: no submission, an expected
        // shortfall the tail cannot estimate, an axis that is not finite, no
        // Sharpe ratio.
        let refused = match (&sharpe, shortfall_error, axes.comparable()) {
            (None, _, _) => Some("no submission for this row in the field".to_string()),
            (_, Some(e), _) => Some(format!("expected_shortfall unavailable: {e}")),
            (_, None, Err(axis)) => Some(format!("{} is not a finite number", axis.name())),
            (Some(Err(e)), None, Ok(_)) => Some(format!("no Sharpe ratio: {e}")),
            (Some(Ok(())), None, Ok(_)) => None,
        };
        pooled.push(track);
        rows.push(ParetoRow {
            agent_id: row.agent_id.clone(),
            rank_eligible: row.rank_eligible,
            axes,
            refused,
            on_front: false,
            on_eligible_front: None,
            dominated_by: Vec::new(),
            front_frequency: None,
            eligible_front_frequency: None,
        });
    }

    let points: Vec<Option<[f64; 6]>> = rows.iter().map(|r| r.axes.comparable().ok()).collect();
    let comparable: Vec<bool> = points.iter().map(Option::is_some).collect();
    let member: Vec<bool> = rows.iter().map(|r| r.refused.is_none()).collect();
    let eligible: Vec<bool> = rows.iter().map(|r| r.rank_eligible).collect();
    let eligible_member: Vec<bool> = member
        .iter()
        .zip(&eligible)
        .map(|(m, e)| *m && *e)
        .collect();
    let eligible_dominator: Vec<bool> = comparable
        .iter()
        .zip(&eligible)
        .map(|(c, e)| *c && *e)
        .collect();

    let all_front = front(&points, &member, &comparable);
    let eligible_front = front(&points, &eligible_member, &eligible_dominator);
    for (i, row) in rows.iter_mut().enumerate() {
        row.on_front = all_front[i];
        row.on_eligible_front = eligible[i].then_some(eligible_front[i]);
        if let Some(p) = points[i] {
            row.dominated_by = (0..points.len())
                .filter(|&j| j != i && points[j].is_some_and(|q| dominates(&q, &p)))
                .map(|j| board[j].agent_id.clone())
                .collect();
        }
    }

    let bootstrap_error = bootstrap_frequencies(
        &mut rows,
        &pooled,
        &member,
        &eligible_member,
        &eligible,
        bootstrap,
    )
    .err();
    ParetoReport {
        used_by_rank: false,
        axes: ParetoAxis::ALL.to_vec(),
        bootstrap,
        bootstrap_error,
        rows,
    }
}

/// Fill `front_frequency` and `eligible_front_frequency` from `bootstrap.n_boot`
/// replicates, or say why there are none.
fn bootstrap_frequencies(
    rows: &mut [ParetoRow],
    pooled: &[Vec<f64>],
    member: &[bool],
    eligible_member: &[bool],
    eligible: &[bool],
    bootstrap: ParetoBootstrap,
) -> Result<(), String> {
    if bootstrap.n_boot == 0 {
        return Err("no bootstrap replicates requested".to_string());
    }
    let n = rows.len();
    let mut on_all = vec![0usize; n];
    let mut on_eligible = vec![0usize; n];
    // inv: on_all[i] and on_eligible[i] count the replicates before b in which
    // row i was on that front.
    for b in 0..bootstrap.n_boot {
        let seed = bootstrap.seed ^ 0x9A2E_7F0E_7AB1_E5ED_u64.wrapping_add(b as u64);
        let mut paths: HashMap<usize, Vec<usize>> = HashMap::new();
        let mut points: Vec<Option<[f64; 6]>> = Vec::with_capacity(n);
        for (i, row) in rows.iter().enumerate() {
            // A row that is not comparable on the observed track stays out of
            // every replicate; the rest are resampled.
            if row.axes.comparable().is_err() {
                points.push(None);
                continue;
            }
            let len = pooled[i].len();
            let path = match paths.get(&len) {
                Some(path) => path,
                None => {
                    let path = stationary_bootstrap_path(seed, len, len, bootstrap.block_prob)
                        .map_err(|e| e.to_string())?;
                    paths.entry(len).or_insert(path)
                }
            };
            let resample: Vec<f64> = path.iter().map(|&k| pooled[i][k]).collect();
            let turnover = row.axes.turnover.unwrap_or(f64::NAN);
            let cost = row.axes.cost.unwrap_or(f64::NAN);
            points.push(axes_of(&resample, turnover, cost).0.comparable().ok());
        }
        let dominators: Vec<bool> = points.iter().map(Option::is_some).collect();
        let eligible_dominators: Vec<bool> = dominators
            .iter()
            .zip(eligible)
            .map(|(d, e)| *d && *e)
            .collect();
        let all_front = front(&points, member, &dominators);
        let eligible_front = front(&points, eligible_member, &eligible_dominators);
        for i in 0..n {
            on_all[i] += usize::from(all_front[i]);
            on_eligible[i] += usize::from(eligible_front[i]);
        }
    }
    let total = bootstrap.n_boot as f64;
    for (i, row) in rows.iter_mut().enumerate() {
        if member[i] {
            row.front_frequency = Some(on_all[i] as f64 / total);
        }
        if eligible_member[i] {
            row.eligible_front_frequency = Some(on_eligible[i] as f64 / total);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::composite::{rank, Run};

    fn sub(id: &str, returns: Vec<f64>) -> AgentSubmission {
        AgentSubmission {
            agent_id: id.into(),
            runs: vec![Run {
                returns,
                ..Run::default()
            }],
            in_sample_trials: 0,
            candidates: vec![],
        }
    }

    /// 400 bars, so the 5% tail holds 20 observations and the expected
    /// shortfall is estimable.
    fn good() -> Vec<f64> {
        (0..400)
            .map(|t| 0.001 + 0.01 * (1.3 * t as f64).sin())
            .collect()
    }

    /// Every bar of `bad` is below the same bar of `good`, with twice the
    /// dispersion, so `good` dominates it on every resample of a shared path.
    fn bad() -> Vec<f64> {
        good().iter().map(|x| 2.0 * x - 0.02).collect()
    }

    fn row<'a>(report: &'a ParetoReport, id: &str) -> &'a ParetoRow {
        report.rows.iter().find(|r| r.agent_id == id).unwrap()
    }

    fn boot(n_boot: usize) -> ParetoBootstrap {
        ParetoBootstrap {
            n_boot,
            seed: 7,
            block_prob: 0.1,
        }
    }

    #[test]
    fn dominates_boundary_needs_no_worse_axis_and_one_strictly_better() {
        let base = [0.001, 0.2, 3.0, 1.0, 0.01, -0.02];
        let directions = [1.0, -1.0, -1.0, -1.0, -1.0, 1.0];
        for (i, axis) in ParetoAxis::ALL.iter().enumerate() {
            assert_eq!(axis.higher_is_better(), directions[i] > 0.0, "{axis:?}");
            let mut better = base;
            better[i] += directions[i] * 0.001;
            assert!(dominates(&better, &base), "{axis:?} better");
            assert!(!dominates(&base, &better), "{axis:?} worse");
            // Better on this axis, worse on another: neither dominates.
            let other = (i + 1) % 6;
            let mut traded = better;
            traded[other] -= directions[other] * 0.001;
            assert!(!dominates(&traded, &base), "{axis:?} traded");
            assert!(!dominates(&base, &traded), "{axis:?} traded back");
        }
        assert!(!dominates(&base, &base), "a tie dominates nobody");
    }

    #[test]
    fn a_dominated_agent_is_off_the_front_on_every_replicate_and_names_its_dominator() {
        let subs = vec![sub("good", good()), sub("bad", bad())];
        let cfg = ScoreConfig::default();
        let board = rank(&subs, &cfg);
        let report = pareto_report(&subs, &board, &cfg, boot(64));
        assert!(!report.used_by_rank);
        assert_eq!(report.axes, ParetoAxis::ALL.to_vec());
        assert_eq!(report.bootstrap_error, None);
        let (g, b) = (row(&report, "good"), row(&report, "bad"));
        assert_eq!((g.on_front, g.refused.as_deref()), (true, None));
        assert!(g.dominated_by.is_empty());
        assert!(!b.on_front);
        assert_eq!(b.dominated_by, vec!["good".to_string()]);
        assert_eq!(g.front_frequency, Some(1.0));
        assert_eq!(b.front_frequency, Some(0.0));
        assert_eq!(g.axes.mean_return, Some(mean(&good())));
        assert_eq!(g.axes.volatility, Some(std_dev(&good())));
        assert_eq!(g.axes.max_drawdown, Some(max_drawdown(&good())));
        assert_eq!(g.axes.turnover, Some(0.0));
        assert_eq!(g.axes.cost, Some(0.0));
        assert!(g.axes.expected_shortfall.unwrap() > b.axes.expected_shortfall.unwrap());
    }

    #[test]
    fn identical_agents_tie_and_both_stay_on_the_front() {
        let subs = vec![sub("good", good()), sub("twin", good()), sub("bad", bad())];
        let cfg = ScoreConfig::default();
        let board = rank(&subs, &cfg);
        let report = pareto_report(&subs, &board, &cfg, boot(16));
        for id in ["good", "twin"] {
            let r = row(&report, id);
            assert!(r.on_front, "{id}");
            assert!(r.dominated_by.is_empty(), "{id}: {:?}", r.dominated_by);
            assert_eq!(r.front_frequency, Some(1.0), "{id}");
        }
        let mut dominators = row(&report, "bad").dominated_by.clone();
        dominators.sort();
        assert_eq!(dominators, vec!["good".to_string(), "twin".to_string()]);
    }

    #[test]
    fn a_non_finite_axis_refuses_the_row_and_it_dominates_nobody() {
        let subs = vec![sub("good", good()), sub("bad", bad())];
        let cfg = ScoreConfig::default();
        let mut board = rank(&subs, &cfg);
        for r in board.iter_mut().filter(|r| r.agent_id == "good") {
            r.cost = f64::NAN;
        }
        let report = pareto_report(&subs, &board, &cfg, boot(8));
        let g = row(&report, "good");
        assert_eq!(g.refused.as_deref(), Some("cost is not a finite number"));
        assert_eq!(
            (g.on_front, g.front_frequency, g.dominated_by.len()),
            (false, None, 0)
        );
        let b = row(&report, "bad");
        assert!(b.on_front, "a refused row dominates nobody");
        assert!(b.dominated_by.is_empty());
        assert_eq!(b.front_frequency, Some(1.0));
    }

    #[test]
    fn refused_rows_say_why_and_a_flat_track_still_dominates() {
        let subs = vec![
            sub("bad", bad()),
            sub("flat", vec![0.0; 400]),
            sub("short", good()[..60].to_vec()),
        ];
        let cfg = ScoreConfig {
            shared_run_set: false,
            ..ScoreConfig::default()
        };
        let board = rank(&subs, &cfg);
        let report = pareto_report(&subs, &board, &cfg, boot(8));
        let flat = row(&report, "flat");
        assert!(flat
            .refused
            .as_deref()
            .is_some_and(|r| r.starts_with("no Sharpe ratio: ")));
        assert_eq!((flat.on_front, flat.front_frequency), (false, None));
        let short = row(&report, "short");
        assert!(short
            .refused
            .as_deref()
            .is_some_and(|r| r.starts_with("expected_shortfall unavailable: ")));
        assert_eq!(short.axes.expected_shortfall, None);
        assert_eq!((short.on_front, short.front_frequency), (false, None));
        let b = row(&report, "bad");
        assert_eq!(b.dominated_by, vec!["flat".to_string()]);
        assert_eq!((b.on_front, b.front_frequency), (false, Some(0.0)));
    }

    /// Returns large enough that the tail sum overflows: the shortfall refuses
    /// the row with its own reason, ahead of the other axes that overflow too.
    #[test]
    fn an_overflowing_shortfall_refuses_the_row_with_its_reason() {
        let huge: Vec<f64> = (0..400)
            .map(|t| if t % 2 == 0 { f64::MAX } else { f64::MAX / 2.0 })
            .collect();
        let subs = vec![sub("good", good()), sub("huge", huge)];
        let cfg = ScoreConfig {
            shared_run_set: false,
            ..ScoreConfig::default()
        };
        let board = rank(&subs, &cfg);
        let report = pareto_report(&subs, &board, &cfg, boot(4));
        let huge = row(&report, "huge");
        assert_eq!(
            huge.refused.as_deref(),
            Some("expected_shortfall unavailable: expected shortfall is not finite")
        );
        assert_eq!(huge.axes.expected_shortfall, None);
        assert!(!huge.on_front);
        assert!(row(&report, "good").on_front);
    }

    #[test]
    fn a_row_with_no_submission_is_refused() {
        let subs = vec![sub("good", good())];
        let cfg = ScoreConfig::default();
        let mut board = rank(&subs, &cfg);
        board[0].agent_id = "missing".into();
        let report = pareto_report(&subs, &board, &cfg, boot(4));
        assert_eq!(
            report.rows[0].refused.as_deref(),
            Some("no submission for this row in the field")
        );
        assert!(!report.rows[0].on_front);
    }

    #[test]
    fn the_eligible_front_reads_eligible_rows_only() {
        let subs = vec![sub("good", good()), sub("bad", bad())];
        let cfg = ScoreConfig::default();
        let mut board = rank(&subs, &cfg);
        for r in board.iter_mut() {
            r.rank_eligible = r.agent_id == "bad";
        }
        let report = pareto_report(&subs, &board, &cfg, boot(16));
        let (g, b) = (row(&report, "good"), row(&report, "bad"));
        assert!(!g.rank_eligible && b.rank_eligible);
        assert_eq!(
            (g.on_eligible_front, g.eligible_front_frequency),
            (None, None)
        );
        assert_eq!((b.on_front, b.on_eligible_front), (false, Some(true)));
        assert_eq!(b.eligible_front_frequency, Some(1.0));
        assert_eq!(b.front_frequency, Some(0.0));
    }

    #[test]
    fn the_report_is_in_board_order_and_leaves_the_board_as_it_was() {
        let subs = vec![sub("bad", bad()), sub("good", good())];
        let cfg = ScoreConfig::default();
        let board = rank(&subs, &cfg);
        let before = serde_json::to_string(&board).unwrap();
        let report = pareto_report(&subs, &board, &cfg, boot(8));
        assert_eq!(serde_json::to_string(&board).unwrap(), before);
        let ids: Vec<&str> = report.rows.iter().map(|r| r.agent_id.as_str()).collect();
        let board_ids: Vec<&str> = board.iter().map(|r| r.agent_id.as_str()).collect();
        assert_eq!(ids, board_ids);
        assert_eq!(report, pareto_report(&subs, &board, &cfg, boot(8)));
    }

    #[test]
    fn no_replicates_or_a_refused_block_probability_leave_the_note_absent_with_a_reason() {
        let subs = vec![sub("good", good()), sub("bad", bad())];
        let cfg = ScoreConfig::default();
        let board = rank(&subs, &cfg);
        let none = pareto_report(&subs, &board, &cfg, boot(0));
        assert_eq!(
            none.bootstrap_error.as_deref(),
            Some("no bootstrap replicates requested")
        );
        let refused = pareto_report(
            &subs,
            &board,
            &cfg,
            ParetoBootstrap {
                block_prob: 0.0,
                ..boot(4)
            },
        );
        assert!(refused.bootstrap_error.is_some());
        for report in [none, refused] {
            assert!(report
                .rows
                .iter()
                .all(|r| r.front_frequency.is_none() && r.eligible_front_frequency.is_none()));
            assert!(row(&report, "good").on_front);
        }
        assert_eq!(
            ParetoBootstrap::from_config(&cfg),
            ParetoBootstrap {
                n_boot: cfg.n_boot,
                seed: cfg.bootstrap_seed,
                block_prob: cfg.block_prob,
            }
        );
    }

    /// The note is a frequency, not a flag: two agents that trade return for
    /// risk can each leave the front on some replicates and stay on it on
    /// others.
    #[test]
    fn a_trade_off_shows_a_membership_frequency_strictly_between_zero_and_one() {
        let calm: Vec<f64> = (0..400)
            .map(|t| 0.0004 + 0.004 * (0.7 * t as f64).sin())
            .collect();
        let bold: Vec<f64> = (0..400)
            .map(|t| 0.0005 + 0.004 * (0.7 * t as f64).sin() + 0.0001 * (2.9 * t as f64).cos())
            .collect();
        let subs = vec![sub("calm", calm), sub("bold", bold)];
        let cfg = ScoreConfig::default();
        let board = rank(&subs, &cfg);
        let report = pareto_report(&subs, &board, &cfg, boot(200));
        let freqs: Vec<f64> = report
            .rows
            .iter()
            .map(|r| r.front_frequency.unwrap())
            .collect();
        assert!(freqs.iter().all(|f| (0.0..=1.0).contains(f)), "{freqs:?}");
        assert!(
            freqs.iter().any(|f| *f > 0.0 && *f < 1.0),
            "expected a fractional membership, got {freqs:?}"
        );
        // The note is a function of its seed: the same seed, field and
        // settings reproduce these frequencies to the last bit.
        let pinned: Vec<(&str, u64)> = report
            .rows
            .iter()
            .map(|r| (r.agent_id.as_str(), r.front_frequency.unwrap().to_bits()))
            .collect();
        assert_eq!(
            pinned,
            vec![
                ("bold", 1.0f64.to_bits()),
                ("calm", 4_605_966_446_900_627_374)
            ]
        );
    }
}
