//! Rank-neutral replay diagnostics over a captured run: an exposure-matched
//! random-timing reference and a lagged replay.
//!
//! Both start from a run's recorded decisions and drive rearranged copies of
//! them back through the frozen engine with [`replay_run`], under the run's own
//! window, execution seed and cost model. Neither reads a score, and the gate
//! and the rank read neither: they are reported beside a verified trajectory.
//!
//! * [`timing_null`] asks whether the entrant's timing beats random timing at
//!   its own exposure. The luck floor is a fully invested random allocator and
//!   the significance gate tests mean return above cash, so an entrant that is
//!   flat most of the time and long in a rising window can pass both on drift
//!   alone. The reference keeps the entrant's holding periods (their number,
//!   their lengths and the decisions inside them, hence the gross exposure
//!   level) and moves them to random places in the window. Copies of one
//!   window under different execution seeds share every draw's placement.
//!   It refuses a cost model with execution noise or a liquidity cap, where a
//!   moved holding period does not keep the entrant's exposure.
//! * [`lagged_replay`] replays the recorded decisions `k` bars late for each
//!   declared `k`. A policy that uses current observations loses its edge when
//!   its decisions arrive stale; a static tilt earns the same.
//!
//! # Validity
//!
//! Both compare the entrant's decisions with the same decisions at other
//! times. That comparison is fair only while the entrant's own orders do not
//! move the prices its positions are marked at, see [`VALID_WHEN`]. This engine
//! marks every position at the frozen dataset close and charges own-order
//! impact on the fill price of the trade that causes it, never on a later
//! close, so the condition holds under every shipped cost profile and there is
//! no price-moving market model here to refuse. A market model in which an
//! entrant's orders move later prices would invalidate both diagnostics.

use serde::Serialize;
use sharpebench_core::deflated_sharpe::{observed_sharpe_ratio, sharpe_ratio};
use sharpebench_protocol::{Action, AgentTrajectory, Decision, DecisionStep, Order, RunTrajectory};

use crate::costs::{CostModel, Rng};
use crate::data::Dataset;
use crate::engine::Window;
use crate::env::TradingEnv;
use crate::trajectory::replay_run;

/// The condition both diagnostics need, carried on every report.
pub const VALID_WHEN: &str = "valid only while the entrant's own orders do not move the price: \
     this engine marks every position at the frozen dataset close and charges own-order impact \
     on the fill price alone, never on a later close";

/// What a lagged row does at the end of the window, carried on every lagged
/// replay report.
pub const LAG_END_EFFECT: &str = "a lag-k row never executes the run's last k recorded decisions,      whose fills would fall after the window; the undelayed row executes them on the window's      last k bars";

/// Gross exposure, as a fraction of NAV after the bar's trades, above which a
/// bar counts as invested.
pub const INVESTED_GROSS_FLOOR: f64 = 1e-9;

/// Draws [`TimingNullConfig::default`] declares.
pub const DEFAULT_TIMING_NULL_DRAWS: usize = 200;

/// The most draws a timing reference accepts. At this count the Monte Carlo
/// standard error of a percentile is at most `0.5 / sqrt(100_000)`, about 0.0016.
pub const MAX_TIMING_NULL_DRAWS: usize = 100_000;

/// Why a replay diagnostic was not computed at all.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ReplayNullRefusal {
    /// The trajectory has no runs.
    EmptyTrajectory,
    /// A run's window reaches past the dataset, so the replay would be cut short.
    WindowOutsideData {
        run: usize,
        window_end: usize,
        dataset_len: usize,
    },
    /// A run does not record one decision per bar of its window. Replaying it
    /// would invent holds the entrant never made.
    IncompleteRun {
        run: usize,
        recorded: usize,
        required: usize,
    },
    /// A timing reference with no draws has no distribution.
    NoDraws,
    /// More draws than [`MAX_TIMING_NULL_DRAWS`].
    TooManyDraws { draws: usize, max: usize },
    /// Under execution noise or a liquidity cap, whether and when an order
    /// fills depends on how often and on which bars orders are sent, so a
    /// holding period moved to other bars does not keep the entrant's exposure.
    ExposureNotPreserved {
        execution_noise: bool,
        liquidity_cap: bool,
    },
    /// A lagged replay with no declared lag measures nothing.
    EmptyLagSet,
    /// A run is too short to leave two bars after the longest lag's first held
    /// bar, including a lag so large that the bar count overflows.
    LagTooLong {
        lag: usize,
        run: usize,
        steps: usize,
    },
}

impl std::fmt::Display for ReplayNullRefusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::EmptyTrajectory => write!(f, "the trajectory has no runs to replay"),
            Self::WindowOutsideData {
                run,
                window_end,
                dataset_len,
            } => write!(
                f,
                "run {run} ends at bar {window_end}, past the dataset's {dataset_len} bars"
            ),
            Self::IncompleteRun {
                run,
                recorded,
                required,
            } => write!(
                f,
                "run {run} records {recorded} of {required} decisions: a replay would have to invent the rest"
            ),
            Self::NoDraws => write!(f, "the timing reference needs at least one draw"),
            Self::TooManyDraws { draws, max } => write!(
                f,
                "the timing reference takes at most {max} draws, not {draws}"
            ),
            Self::ExposureNotPreserved {
                execution_noise,
                liquidity_cap,
            } => {
                let cause = match (execution_noise, liquidity_cap) {
                    (true, true) => "execution noise and a liquidity cap",
                    (true, false) => "execution noise",
                    _ => "a liquidity cap",
                };
                write!(
                    f,
                    "the timing reference cannot keep the entrant's exposure under {cause}: fills there depend on when orders are sent"
                )
            }
            Self::EmptyLagSet => write!(f, "the lagged replay needs at least one declared lag"),
            Self::LagTooLong { lag, run, steps } => write!(
                f,
                "lag {lag} leaves fewer than two comparable bars in run {run} ({steps} bars)"
            ),
        }
    }
}

impl std::error::Error for ReplayNullRefusal {}

fn check_runs(data: &Dataset, traj: &AgentTrajectory) -> Result<(), ReplayNullRefusal> {
    if traj.runs.is_empty() {
        return Err(ReplayNullRefusal::EmptyTrajectory);
    }
    for (index, run) in traj.runs.iter().enumerate() {
        if run.window_end > data.len() {
            return Err(ReplayNullRefusal::WindowOutsideData {
                run: index,
                window_end: run.window_end,
                dataset_len: data.len(),
            });
        }
        let required = run.window_end.saturating_sub(run.window_start);
        if run.steps.len() != required {
            return Err(ReplayNullRefusal::IncompleteRun {
                run: index,
                recorded: run.steps.len(),
                required,
            });
        }
    }
    Ok(())
}

fn hold() -> Decision {
    Decision {
        orders: Vec::new(),
        reasoning: String::new(),
        cost: None,
    }
}

fn mean(values: &[f64]) -> f64 {
    if values.is_empty() {
        return 0.0;
    }
    values.iter().sum::<f64>() / values.len() as f64
}

/// The run's decisions, each executed `lag` bars after it was recorded. The
/// first `lag` bars hold (the book starts in cash, so they are flat), and the
/// last `lag` recorded decisions fall after the window and never execute.
pub fn lagged_trajectory(run: &RunTrajectory, lag: usize) -> RunTrajectory {
    let steps = run
        .steps
        .iter()
        .enumerate()
        .map(|(index, step)| DecisionStep {
            step: step.step,
            observation_id: step.observation_id.clone(),
            decision: if index < lag {
                hold()
            } else {
                run.steps[index - lag].decision.clone()
            },
        })
        .collect();
    RunTrajectory {
        window_start: run.window_start,
        window_end: run.window_end,
        seed: run.seed,
        steps,
    }
}

/// One row of one run: a lag's figures on the run's compared bars.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct LagRowFigures {
    pub lag: usize,
    /// Per-period Sharpe on the compared bars.
    pub sharpe: f64,
    /// Mean per-period return on the compared bars.
    pub mean_return: f64,
}

/// Why a run has no rows in the lagged replay.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "reason", rename_all = "snake_case")]
pub enum LaggedRunUnavailable {
    /// The row for `lag` never holds a position inside the window, so it has
    /// no opening fill to skip past and nothing to compare.
    NeverFills { lag: usize },
    /// Skipping past every row's opening fill leaves fewer than two bars.
    TooFewComparedBars { skipped_leading_bars: usize },
    /// The kernel's `observed_sharpe_ratio` refuses the row's compared bars:
    /// they are constant, as a flat book's are, or their Sharpe is not finite.
    NoSharpe { lag: usize },
}

/// One run of the lagged replay.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum LaggedRun {
    Available {
        run: usize,
        /// Leading bars left out of every row of this run: through the latest
        /// opening fill of any row, so no row carries its opening trade and
        /// every compared bar is earned by a position each row has taken.
        skipped_leading_bars: usize,
        compared_bars: usize,
        undelayed: LagRowFigures,
        lagged: Vec<LagRowFigures>,
    },
    Unavailable {
        run: usize,
        #[serde(flatten)]
        why: LaggedRunUnavailable,
    },
}

/// One lag's figures across the runs with rows.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct LagFigures {
    pub lag: usize,
    /// Mean over those runs of each run's per-period Sharpe.
    pub mean_sharpe: f64,
    /// Mean per-period return over every compared bar of those runs, pooled.
    pub mean_return: f64,
}

/// Why no lagged figures exist across runs.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LaggedAggregateUnavailable {
    /// Every run is unavailable.
    NoComparableRun,
}

/// The lagged replay across the runs with rows.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum LaggedAggregate {
    Available {
        runs: usize,
        compared_bars: usize,
        undelayed: LagFigures,
        lagged: Vec<LagFigures>,
    },
    Unavailable {
        reason: LaggedAggregateUnavailable,
    },
}

/// [`lagged_replay`]'s report.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct LaggedReplayReport {
    /// The declared lags, ascending and without repeats.
    pub lags: Vec<usize>,
    pub runs: Vec<LaggedRun>,
    pub aggregate: LaggedAggregate,
    pub valid_when: &'static str,
    pub end_effect: &'static str,
}

/// The first bar after whose trades `decisions` leave the book holding a
/// position.
fn first_held_bar(
    env: &mut TradingEnv,
    data: &Dataset,
    window_start: usize,
    decisions: &[Decision],
) -> Option<usize> {
    gross_path(env, data, window_start, decisions)
        .iter()
        .position(|&level| level > 0.0)
}

fn lagged_run(
    data: &Dataset,
    index: usize,
    run: &RunTrajectory,
    costs: CostModel,
    lags: &[usize],
) -> LaggedRun {
    let unavailable = |why| LaggedRun::Unavailable { run: index, why };
    let mut env = TradingEnv::new(
        data.clone(),
        Window {
            start: run.window_start,
            end: run.window_end,
        },
        costs,
        run.seed,
    );
    let rows: Vec<RunTrajectory> = std::iter::once(0)
        .chain(lags.iter().copied())
        .map(|lag| lagged_trajectory(run, lag))
        .collect();
    let row_lag = |position: usize| if position == 0 { 0 } else { lags[position - 1] };
    let mut latest_opening = 0;
    for (position, row) in rows.iter().enumerate() {
        let decisions: Vec<Decision> = row.steps.iter().map(|s| s.decision.clone()).collect();
        match first_held_bar(&mut env, data, run.window_start, &decisions) {
            Some(bar) => latest_opening = latest_opening.max(bar),
            None => {
                return unavailable(LaggedRunUnavailable::NeverFills {
                    lag: row_lag(position),
                })
            }
        }
    }
    let skipped = latest_opening + 1;
    let bars = run.steps.len();
    if bars < skipped + 2 {
        return unavailable(LaggedRunUnavailable::TooFewComparedBars {
            skipped_leading_bars: skipped,
        });
    }
    let mut figures = Vec::with_capacity(rows.len());
    for (position, row) in rows.iter().enumerate() {
        let lag = row_lag(position);
        let replayed = replay_run(data, row, costs);
        let compared = &replayed.returns[skipped..];
        let Ok(sharpe) = observed_sharpe_ratio(compared) else {
            return unavailable(LaggedRunUnavailable::NoSharpe { lag });
        };
        figures.push(LagRowFigures {
            lag,
            sharpe,
            mean_return: mean(compared),
        });
    }
    let undelayed = figures.remove(0);
    LaggedRun::Available {
        run: index,
        skipped_leading_bars: skipped,
        compared_bars: bars - skipped,
        undelayed,
        lagged: figures,
    }
}

/// Figures for the row `pick` selects, across the comparable runs.
fn across(
    lag: usize,
    comparable: &[(usize, &LagRowFigures, &[LagRowFigures])],
    pick: impl Fn(&(usize, &LagRowFigures, &[LagRowFigures])) -> f64,
    pick_return: impl Fn(&(usize, &LagRowFigures, &[LagRowFigures])) -> f64,
) -> LagFigures {
    let sharpes: Vec<f64> = comparable.iter().map(&pick).collect();
    let bars: usize = comparable.iter().map(|entry| entry.0).sum();
    LagFigures {
        lag,
        mean_sharpe: mean(&sharpes),
        mean_return: comparable
            .iter()
            .map(|entry| pick_return(entry) * entry.0 as f64)
            .sum::<f64>()
            / bars as f64,
    }
}

/// Replay every run's decisions `k` bars late for each declared `k` and report
/// Sharpe and mean return beside the undelayed figures.
///
/// Within a run every row, the undelayed one included, is computed on the same
/// bars: those after the latest bar at which any row first holds a position.
/// No row carries its opening trade, and every compared bar is earned by a
/// position each row has already taken. A lag of zero replays the recorded
/// decisions unchanged, so its row equals the undelayed row. [`LAG_END_EFFECT`]
/// states what happens at the window's end. A run whose rows cannot all be
/// compared is typed unavailable and left out of the aggregate. This measures
/// decision-delay sensitivity under whatever `costs` the caller passes, for
/// example the stressed profile with its declared `decision_delay_bars`, which
/// the backtest driver itself does not apply. Under execution noise a moved
/// decision also meets other noise draws, so a lag row then mixes the delay
/// with a different fill realization.
pub fn lagged_replay(
    data: &Dataset,
    traj: &AgentTrajectory,
    costs: CostModel,
    lags: &[usize],
) -> Result<LaggedReplayReport, ReplayNullRefusal> {
    check_runs(data, traj)?;
    let declared: Vec<usize> = lags
        .iter()
        .copied()
        .collect::<std::collections::BTreeSet<usize>>()
        .into_iter()
        .collect();
    let Some(&longest) = declared.last() else {
        return Err(ReplayNullRefusal::EmptyLagSet);
    };
    // A lag-k row cannot hold a position before bar k, so its first earned bar
    // is at least k + 1; a shorter run has no two comparable bars at all.
    let needed = longest.checked_add(3);
    for (index, run) in traj.runs.iter().enumerate() {
        if needed.is_none_or(|needed| run.steps.len() < needed) {
            return Err(ReplayNullRefusal::LagTooLong {
                lag: longest,
                run: index,
                steps: run.steps.len(),
            });
        }
    }
    let runs: Vec<LaggedRun> = traj
        .runs
        .iter()
        .enumerate()
        .map(|(index, run)| lagged_run(data, index, run, costs, &declared))
        .collect();
    let comparable: Vec<(usize, &LagRowFigures, &[LagRowFigures])> = runs
        .iter()
        .filter_map(|run| match run {
            LaggedRun::Available {
                compared_bars,
                undelayed,
                lagged,
                ..
            } => Some((*compared_bars, undelayed, lagged.as_slice())),
            LaggedRun::Unavailable { .. } => None,
        })
        .collect();
    let aggregate = if comparable.is_empty() {
        LaggedAggregate::Unavailable {
            reason: LaggedAggregateUnavailable::NoComparableRun,
        }
    } else {
        LaggedAggregate::Available {
            runs: comparable.len(),
            compared_bars: comparable.iter().map(|entry| entry.0).sum(),
            undelayed: across(
                0,
                &comparable,
                |entry| entry.1.sharpe,
                |entry| entry.1.mean_return,
            ),
            lagged: declared
                .iter()
                .enumerate()
                .map(|(position, &lag)| {
                    across(
                        lag,
                        &comparable,
                        |entry| entry.2[position].sharpe,
                        |entry| entry.2[position].mean_return,
                    )
                })
                .collect(),
        }
    };
    Ok(LaggedReplayReport {
        lags: declared,
        runs,
        aggregate,
        valid_when: VALID_WHEN,
        end_effect: LAG_END_EFFECT,
    })
}

/// The declared size and seed of a timing reference.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct TimingNullConfig {
    pub draws: usize,
    pub seed: u64,
}

impl Default for TimingNullConfig {
    fn default() -> Self {
        Self {
            draws: DEFAULT_TIMING_NULL_DRAWS,
            seed: 0,
        }
    }
}

/// Why a timing reference has no distribution to place the entrant in.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TimingNullUnavailable {
    /// No bar was invested: there is no exposure to move.
    NeverInvested,
    /// Every bar was invested: the only placement is the entrant's own.
    AlwaysInvested,
    /// Across runs only: no run was both invested and flat, and the runs do
    /// not share one of the two reasons above.
    NoRunWithTimingFreedom,
}

/// The entrant's exposure as the reference preserves it.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ExposureProfile {
    pub bars: usize,
    /// Bars whose post-trade gross exposure exceeds [`INVESTED_GROSS_FLOOR`].
    pub invested_bars: usize,
    /// Maximal runs of consecutive invested bars.
    pub holding_periods: usize,
    pub longest_holding_bars: usize,
    /// Mean post-trade gross exposure over invested bars, leaving out any bar
    /// whose NAV is zero; zero when never invested.
    pub mean_gross_when_invested: f64,
}

/// Where a Sharpe falls among the reference draws.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct TimingPercentile {
    pub draws: usize,
    /// Draws with a strictly lower Sharpe.
    pub below: usize,
    /// Draws with exactly the same Sharpe.
    pub ties: usize,
    /// Mid-rank percentile, `(below + ties / 2) / draws`.
    pub percentile: f64,
    /// Plug-in Monte Carlo standard error of `percentile` over the draws:
    /// each draw scores 1, 1/2 or 0, and this is the standard deviation of
    /// those scores over the square root of `draws`. It is zero when every
    /// draw falls on one side, where the resolution is `1 / draws`.
    pub monte_carlo_standard_error: f64,
    pub reference_mean_sharpe: f64,
}

impl TimingPercentile {
    fn of(value: f64, reference: &[f64]) -> Self {
        let below = reference.iter().filter(|&&draw| draw < value).count();
        let ties = reference.iter().filter(|&&draw| draw == value).count();
        let draws = reference.len() as f64;
        let percentile = (below as f64 + ties as f64 / 2.0) / draws;
        let second_moment = (below as f64 + ties as f64 / 4.0) / draws;
        let variance = (second_moment - percentile * percentile).max(0.0);
        Self {
            draws: reference.len(),
            below,
            ties,
            percentile,
            monte_carlo_standard_error: (variance / draws).sqrt(),
            reference_mean_sharpe: mean(reference),
        }
    }
}

/// One run's timing reference.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum RunTimingNull {
    Available {
        run: usize,
        window_start: usize,
        window_end: usize,
        seed: u64,
        exposure: ExposureProfile,
        /// Per-period Sharpe of the run's replay, the figure verification reports.
        entrant_sharpe: f64,
        reference: TimingPercentile,
        /// How many different decision sequences the draws come from: the
        /// distinct orders of the holding periods (periods with the same
        /// orders are interchangeable) times the `C(flat + 1, periods)` ways
        /// to spread the flat bars. It saturates at `u64::MAX`. When it is
        /// not far above `draws`, the draws repeat placements.
        distinct_placements: u64,
        /// Mean invested bars per draw as the engine executed them. It equals
        /// the entrant's invested bars except where a moved order meets a close
        /// that is not positive, or a trade value below the engine's `1e-9`
        /// minimum, both of which the engine skips.
        reference_mean_invested_bars: f64,
    },
    Unavailable {
        run: usize,
        reason: TimingNullUnavailable,
        exposure: ExposureProfile,
    },
}

/// The timing reference across every run with timing freedom.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum TimingNullAggregate {
    Available {
        runs: usize,
        /// Mean of the entrant's per-run Sharpe over those runs.
        entrant_mean_sharpe: f64,
        /// Draw `i` of the reference is the mean of every such run's draw `i`.
        reference: TimingPercentile,
    },
    Unavailable {
        reason: TimingNullUnavailable,
    },
}

/// [`timing_null`]'s report.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct TimingNullReport {
    pub draws: usize,
    pub seed: u64,
    pub runs: Vec<RunTimingNull>,
    pub aggregate: TimingNullAggregate,
    pub valid_when: &'static str,
}

/// Post-trade gross exposure at every bar of `decisions`, driven through the
/// same engine body as [`replay_run`] on the run's window and seed.
fn gross_path(
    env: &mut TradingEnv,
    data: &Dataset,
    window_start: usize,
    decisions: &[Decision],
) -> Vec<f64> {
    env.reset();
    decisions
        .iter()
        .enumerate()
        .map(|(index, decision)| {
            let bar = window_start + index;
            let stepped = env.step(decision.clone());
            let value: f64 = stepped
                .observation
                .portfolio
                .iter()
                .map(|position| {
                    (position.shares * data.close_at(&position.symbol, bar).unwrap_or(0.0)).abs()
                })
                .sum();
            let nav = stepped.info.nav.abs();
            if nav > 0.0 {
                value / nav
            } else if value > 0.0 {
                f64::INFINITY
            } else {
                0.0
            }
        })
        .collect()
}

fn invested(gross: f64) -> bool {
    gross > INVESTED_GROSS_FLOOR
}

/// Maximal runs of invested bars, as `(first bar, length)`.
fn holding_periods(gross: &[f64]) -> Vec<(usize, usize)> {
    let mut periods = Vec::new();
    let mut open: Option<usize> = None;
    for (bar, &level) in gross.iter().enumerate() {
        match (invested(level), open) {
            (true, None) => open = Some(bar),
            (false, Some(first)) => {
                periods.push((first, bar - first));
                open = None;
            }
            _ => {}
        }
    }
    if let Some(first) = open {
        periods.push((first, gross.len() - first));
    }
    periods
}

fn exposure_profile(gross: &[f64], periods: &[(usize, usize)]) -> ExposureProfile {
    let levels: Vec<f64> = gross
        .iter()
        .copied()
        .filter(|&level| invested(level) && level.is_finite())
        .collect();
    ExposureProfile {
        bars: gross.len(),
        invested_bars: periods.iter().map(|&(_, length)| length).sum(),
        holding_periods: periods.len(),
        longest_holding_bars: periods.iter().map(|&(_, length)| length).max().unwrap_or(0),
        mean_gross_when_invested: mean(&levels),
    }
}

/// A uniform index below `bound` (which must be positive).
fn below(rng: &mut Rng, bound: usize) -> usize {
    ((rng.unit() * bound as f64) as usize).min(bound - 1)
}

/// Random first bars for holding periods of the given lengths, laid out in a
/// random order across `bars` bars. Consecutive periods keep at least one flat
/// bar between them, so none merge and the multiset of lengths is unchanged;
/// the remaining flat bars are spread over the gaps as a uniformly random weak
/// composition. Returns `(period index, first bar)` in layout order.
fn random_layout(lengths: &[usize], bars: usize, rng: &mut Rng) -> Vec<(usize, usize)> {
    let count = lengths.len();
    let mut order: Vec<usize> = (0..count).collect();
    for i in (1..count).rev() {
        order.swap(i, below(rng, i + 1));
    }
    let flat = bars - lengths.iter().sum::<usize>();
    let spare = flat + 1 - count;
    // Choose `count` separator slots among `spare + count`: the unchosen slots
    // before, between and after them are the spare flat bars of each gap.
    let mut slots: Vec<usize> = (0..spare + count).collect();
    for i in 0..count {
        let pick = i + below(rng, slots.len() - i);
        slots.swap(i, pick);
    }
    let mut separators = slots[..count].to_vec();
    separators.sort_unstable();
    let mut layout = Vec::with_capacity(count);
    let mut cursor = 0;
    let mut previous: Option<usize> = None;
    for (position, (&period, &separator)) in order.iter().zip(&separators).enumerate() {
        let spare_before = match previous {
            None => separator,
            Some(last) => separator - last - 1,
        };
        cursor += spare_before + usize::from(position > 0);
        layout.push((period, cursor));
        cursor += lengths[period];
        previous = Some(separator);
    }
    layout
}

fn flat_decision(symbols: &[String]) -> Decision {
    Decision {
        orders: symbols
            .iter()
            .map(|symbol| Order {
                symbol: symbol.clone(),
                action: Action::Close,
                target_weight: 0.0,
                confidence: None,
                rationale: String::new(),
            })
            .collect(),
        reasoning: String::new(),
        cost: None,
    }
}

fn mix64(mut z: u64) -> u64 {
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

/// The draw stream for one window: a pure function of the declared seed and
/// the window. Every run over the same window reads the same stream, so copies
/// of one window under different execution seeds, which carry the same holding
/// periods, place them at the same bars in every draw.
fn window_stream(seed: u64, window_start: usize, window_end: usize) -> Rng {
    Rng::new(mix64(
        seed ^ mix64(window_start as u64 ^ 0x7131_4E55_0000_0000)
            ^ mix64(window_end as u64 ^ 0x2B7E_1516_28AE_D2A6).rotate_left(17),
    ))
}

/// `C(n, k)`, or `None` above `u128::MAX`.
fn binomial(n: u128, k: u128) -> Option<u128> {
    if k > n {
        return Some(0);
    }
    let k = k.min(n - k);
    let mut value: u128 = 1;
    for i in 0..k {
        value = value.checked_mul(n - i)? / (i + 1);
    }
    Some(value)
}

/// How many distinct decision sequences [`random_layout`] can place: the
/// distinct orders of the periods, with periods whose orders match counted as
/// one, times the weak compositions of the spare flat bars. Saturates at
/// `u64::MAX`.
fn distinct_placements(periods: &[&[Decision]], bars: usize) -> u64 {
    let key = |period: &[Decision]| -> Vec<Vec<(String, u64)>> {
        period
            .iter()
            .map(|decision| {
                decision
                    .orders
                    .iter()
                    .map(|order| (order.symbol.clone(), order.target_weight.to_bits()))
                    .collect()
            })
            .collect()
    };
    let mut multiplicities = std::collections::BTreeMap::new();
    for period in periods {
        *multiplicities.entry(key(period)).or_insert(0u128) += 1;
    }
    let count = periods.len() as u128;
    let held: usize = periods.iter().map(|period| period.len()).sum();
    let flat = (bars - held) as u128;
    let mut remaining = count;
    let mut total = binomial(flat + 1, count);
    for &same in multiplicities.values() {
        total = total.and_then(|t| t.checked_mul(binomial(remaining, same)?));
        remaining -= same;
    }
    total.map_or(u64::MAX, |t| u64::try_from(t).unwrap_or(u64::MAX))
}

struct RunDraws {
    entry: RunTimingNull,
    /// Per-draw Sharpe, present only when the run has timing freedom.
    reference: Option<(f64, Vec<f64>)>,
}

fn run_timing_null(
    data: &Dataset,
    index: usize,
    run: &RunTrajectory,
    costs: CostModel,
    config: TimingNullConfig,
) -> RunDraws {
    let symbols = data.symbols();
    let decisions: Vec<Decision> = run.steps.iter().map(|step| step.decision.clone()).collect();
    let bars = decisions.len();
    let mut env = TradingEnv::new(
        data.clone(),
        Window {
            start: run.window_start,
            end: run.window_end,
        },
        costs,
        run.seed,
    );
    let gross = gross_path(&mut env, data, run.window_start, &decisions);
    let periods = holding_periods(&gross);
    let exposure = exposure_profile(&gross, &periods);
    let unavailable = if periods.is_empty() {
        Some(TimingNullUnavailable::NeverInvested)
    } else if exposure.invested_bars == bars {
        Some(TimingNullUnavailable::AlwaysInvested)
    } else {
        None
    };
    if let Some(reason) = unavailable {
        return RunDraws {
            entry: RunTimingNull::Unavailable {
                run: index,
                reason,
                exposure,
            },
            reference: None,
        };
    }

    let entrant_sharpe = sharpe_ratio(&replay_run(data, run, costs).returns);
    let lengths: Vec<usize> = periods.iter().map(|&(_, length)| length).collect();
    let slices: Vec<&[Decision]> = periods
        .iter()
        .map(|&(first, length)| &decisions[first..first + length])
        .collect();
    let placements = distinct_placements(&slices, bars);
    let flat = flat_decision(&symbols);
    let mut rng = window_stream(config.seed, run.window_start, run.window_end);
    let mut reference = Vec::with_capacity(config.draws);
    let mut invested_bars = 0usize;
    for _ in 0..config.draws {
        let mut placed = vec![flat.clone(); bars];
        for (period, first) in random_layout(&lengths, bars, &mut rng) {
            let (source, length) = periods[period];
            placed[first..first + length].clone_from_slice(&decisions[source..source + length]);
        }
        invested_bars += gross_path(&mut env, data, run.window_start, &placed)
            .into_iter()
            .filter(|&level| invested(level))
            .count();
        let draw = RunTrajectory {
            window_start: run.window_start,
            window_end: run.window_end,
            seed: run.seed,
            steps: placed
                .into_iter()
                .zip(&run.steps)
                .map(|(decision, step)| DecisionStep {
                    step: step.step,
                    observation_id: step.observation_id.clone(),
                    decision,
                })
                .collect(),
        };
        reference.push(sharpe_ratio(&replay_run(data, &draw, costs).returns));
    }
    RunDraws {
        entry: RunTimingNull::Available {
            run: index,
            window_start: run.window_start,
            window_end: run.window_end,
            seed: run.seed,
            exposure,
            entrant_sharpe,
            reference: TimingPercentile::of(entrant_sharpe, &reference),
            distinct_placements: placements,
            reference_mean_invested_bars: invested_bars as f64 / config.draws as f64,
        },
        reference: Some((entrant_sharpe, reference)),
    }
}

/// Place each run's Sharpe among `config.draws` seeded replays that keep the
/// entrant's holding periods and move them to random bars of the same window.
///
/// A bar is invested when the book's gross exposure after that bar's trades
/// exceeds [`INVESTED_GROSS_FLOOR`] of NAV. Each holding period carries the
/// entrant's own decisions for its bars; every bar outside the placed periods
/// closes every position. The draws replay through [`replay_run`] under the
/// run's window, execution seed and `costs`, so they pay the same frictions.
/// Runs over the same window draw the same placements, so execution-seed
/// copies of one window are not counted as independent timing choices.
/// A run that was never invested, or invested on every bar, is typed
/// unavailable rather than given a degenerate percentile. A cost model with
/// execution noise or a finite liquidity cap is refused: there, whether an
/// order fills depends on when and how often orders are sent, so moved periods
/// do not keep the entrant's exposure. The report is a pure function of the
/// trajectory, the data, `costs` and `config`.
pub fn timing_null(
    data: &Dataset,
    traj: &AgentTrajectory,
    costs: CostModel,
    config: TimingNullConfig,
) -> Result<TimingNullReport, ReplayNullRefusal> {
    check_runs(data, traj)?;
    if config.draws == 0 {
        return Err(ReplayNullRefusal::NoDraws);
    }
    if config.draws > MAX_TIMING_NULL_DRAWS {
        return Err(ReplayNullRefusal::TooManyDraws {
            draws: config.draws,
            max: MAX_TIMING_NULL_DRAWS,
        });
    }
    let execution_noise = costs.noise.is_some();
    let liquidity_cap = costs.max_participation.is_finite();
    if execution_noise || liquidity_cap {
        return Err(ReplayNullRefusal::ExposureNotPreserved {
            execution_noise,
            liquidity_cap,
        });
    }
    let per_run: Vec<RunDraws> = traj
        .runs
        .iter()
        .enumerate()
        .map(|(index, run)| run_timing_null(data, index, run, costs, config))
        .collect();
    let free: Vec<&(f64, Vec<f64>)> = per_run
        .iter()
        .filter_map(|run| run.reference.as_ref())
        .collect();
    let aggregate = if free.is_empty() {
        let mut reasons = per_run.iter().filter_map(|run| match run.entry {
            RunTimingNull::Unavailable { reason, .. } => Some(reason),
            RunTimingNull::Available { .. } => None,
        });
        let first = reasons.next();
        let reason = match first {
            Some(shared) if reasons.all(|reason| reason == shared) => shared,
            _ => TimingNullUnavailable::NoRunWithTimingFreedom,
        };
        TimingNullAggregate::Unavailable { reason }
    } else {
        let entrant: Vec<f64> = free.iter().map(|(sharpe, _)| *sharpe).collect();
        let reference: Vec<f64> = (0..config.draws)
            .map(|draw| {
                mean(
                    &free
                        .iter()
                        .map(|(_, draws)| draws[draw])
                        .collect::<Vec<_>>(),
                )
            })
            .collect();
        let entrant_mean_sharpe = mean(&entrant);
        TimingNullAggregate::Available {
            runs: free.len(),
            entrant_mean_sharpe,
            reference: TimingPercentile::of(entrant_mean_sharpe, &reference),
        }
    };
    Ok(TimingNullReport {
        draws: config.draws,
        seed: config.seed,
        runs: per_run.into_iter().map(|run| run.entry).collect(),
        aggregate,
        valid_when: VALID_WHEN,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn holding_periods_are_the_maximal_invested_runs() {
        let gross = [0.0, 0.5, 0.4, 0.0, 0.0, 1e-9, 2e-9, 0.2];
        assert_eq!(holding_periods(&gross), vec![(1, 2), (6, 2)]);
        assert_eq!(holding_periods(&[0.3, 0.3]), vec![(0, 2)]);
        assert!(holding_periods(&[0.0, 1e-9]).is_empty());
        let profile = exposure_profile(&gross, &holding_periods(&gross));
        assert_eq!(profile.bars, 8);
        assert_eq!(profile.invested_bars, 4);
        assert_eq!(profile.holding_periods, 2);
        assert_eq!(profile.longest_holding_bars, 2);
        assert!((profile.mean_gross_when_invested - (0.5 + 0.4 + 2e-9 + 0.2) / 4.0).abs() < 1e-15);
    }

    fn check_layout(lengths: &[usize], bars: usize, layout: &[(usize, usize)]) {
        let mut seen: Vec<usize> = layout.iter().map(|&(period, _)| period).collect();
        seen.sort_unstable();
        assert_eq!(seen, (0..lengths.len()).collect::<Vec<_>>());
        for pair in layout.windows(2) {
            let (period, first) = pair[0];
            assert!(
                first + lengths[period] < pair[1].1,
                "periods must stay apart: {layout:?}"
            );
        }
        let &(period, first) = layout.last().unwrap();
        assert!(first + lengths[period] <= bars, "{layout:?}");
    }

    #[test]
    fn a_layout_keeps_every_period_whole_and_apart_inside_the_window() {
        let mut rng = Rng::new(3);
        for case in 0..300usize {
            let count = 1 + case % 6;
            let lengths: Vec<usize> = (0..count).map(|_| 1 + below(&mut rng, 5)).collect();
            let tight = lengths.iter().sum::<usize>() + count - 1;
            let bars = tight + below(&mut rng, 8);
            let layout = random_layout(&lengths, bars, &mut rng);
            check_layout(&lengths, bars, &layout);
            // The laid-out gross path has exactly the entrant's periods.
            let mut gross = vec![0.0; bars];
            for &(period, first) in &layout {
                gross[first..first + lengths[period]].fill(1.0);
            }
            let mut placed: Vec<usize> = holding_periods(&gross).iter().map(|p| p.1).collect();
            let mut declared = lengths.clone();
            placed.sort_unstable();
            declared.sort_unstable();
            assert_eq!(placed, declared);
        }
    }

    #[test]
    fn a_layout_draws_every_placement_about_equally_often() {
        // Two one-bar periods in four bars have three placements of the flat
        // bars and two orders: six layouts, each with probability one sixth.
        let mut rng = Rng::new(11);
        let mut counts = std::collections::BTreeMap::new();
        for _ in 0..6_000 {
            *counts
                .entry(random_layout(&[1, 1], 4, &mut rng))
                .or_insert(0usize) += 1;
        }
        assert_eq!(counts.len(), 6, "{counts:?}");
        assert!(
            counts.values().all(|&n| (850..=1_150).contains(&n)),
            "{counts:?}"
        );
        // One period of two bars in five: first bar 0, 1, 2 or 3.
        let mut firsts = [0usize; 4];
        for _ in 0..4_000 {
            firsts[random_layout(&[2], 5, &mut rng)[0].1] += 1;
        }
        assert!(
            firsts.iter().all(|&n| (850..=1_150).contains(&n)),
            "{firsts:?}"
        );
    }

    #[test]
    fn the_percentile_counts_ties_as_half() {
        let placed = TimingPercentile::of(1.0, &[0.5, 1.0, 1.0, 2.0]);
        assert_eq!(
            (placed.draws, placed.below, placed.ties, placed.percentile),
            (4, 1, 2, 0.5)
        );
        assert_eq!(placed.reference_mean_sharpe, 1.125);
        // Scores 1, 1/2, 1/2, 0: plug-in variance 1/8, standard error
        // sqrt(1/8 / 4) = sqrt(2) / 8 (checked with sympy).
        assert!((placed.monte_carlo_standard_error - 2f64.sqrt() / 8.0).abs() < 1e-15);
        // Without ties it is sqrt(p (1 - p) / n): p = 2/5, n = 5, sqrt(30) / 25.
        let untied = TimingPercentile::of(1.0, &[0.5, 0.7, 2.0, 3.0, 4.0]);
        assert_eq!(untied.percentile, 0.4);
        assert!((untied.monte_carlo_standard_error - 30f64.sqrt() / 25.0).abs() < 1e-15);
        for (value, percentile) in [(3.0, 1.0), (0.0, 0.0)] {
            let edge = TimingPercentile::of(value, &[0.5, 2.0]);
            assert_eq!(edge.percentile, percentile);
            assert_eq!(edge.monte_carlo_standard_error, 0.0);
        }
    }

    fn period(weights: &[f64]) -> Vec<Decision> {
        weights
            .iter()
            .map(|&weight| Decision {
                orders: vec![Order {
                    symbol: "S0".to_string(),
                    action: Action::Buy,
                    target_weight: weight,
                    confidence: None,
                    rationale: String::new(),
                }],
                reasoning: String::new(),
                cost: None,
            })
            .collect()
    }

    #[test]
    fn the_placement_count_is_the_support_of_the_layout() {
        // Each case: periods by their order weights, bars, and the count a
        // brute-force enumeration in Python gives.
        let cases: [(&[&[f64]], usize, u64); 6] = [
            (&[&[0.1]], 2, 2),
            (&[&[0.1], &[0.2]], 5, 12),
            (&[&[0.1], &[0.1]], 5, 6),
            (&[&[0.1, 0.2], &[0.3], &[0.1, 0.2]], 9, 30),
            (&[&[0.1, 0.2], &[0.1, 0.2], &[0.1, 0.2]], 10, 10),
            (&[&[0.1], &[0.2], &[0.3]], 6, 24),
        ];
        let mut rng = Rng::new(9);
        for (weights, bars, expected) in cases {
            let periods: Vec<Vec<Decision>> = weights.iter().map(|w| period(w)).collect();
            let slices: Vec<&[Decision]> = periods.iter().map(Vec::as_slice).collect();
            assert_eq!(distinct_placements(&slices, bars), expected, "{weights:?}");
            // The layout reaches exactly that many distinct sequences.
            let lengths: Vec<usize> = periods.iter().map(Vec::len).collect();
            let mut seen = std::collections::BTreeSet::new();
            for _ in 0..20_000 {
                let mut sequence = vec![None; bars];
                for (index, first) in random_layout(&lengths, bars, &mut rng) {
                    for (offset, decision) in periods[index].iter().enumerate() {
                        sequence[first + offset] = Some(decision.orders[0].target_weight.to_bits());
                    }
                }
                seen.insert(sequence);
            }
            assert_eq!(seen.len() as u64, expected, "{weights:?}");
        }
        // Two equal periods and one other in 63 bars: C(61, 3) * 3! / 2!.
        let pair = period(&[0.1]);
        let other = period(&[0.2]);
        assert_eq!(distinct_placements(&[&pair, &pair, &other], 63), 107_970);
        // Twenty different one-bar periods in 200 bars: C(181, 20) * 20! is
        // far above u64::MAX.
        let many: Vec<Vec<Decision>> = (1..=20).map(|i| period(&[f64::from(i) / 100.0])).collect();
        let slices: Vec<&[Decision]> = many.iter().map(Vec::as_slice).collect();
        assert_eq!(distinct_placements(&slices, 200), u64::MAX);
        assert_eq!(binomial(5, 7), Some(0));
        assert_eq!(binomial(60, 30), Some(118_264_581_564_861_424));
    }

    #[test]
    fn window_streams_differ_by_window_and_by_seed() {
        let first = |seed, start, end| window_stream(seed, start, end).unit();
        assert_eq!(first(4, 20, 100), first(4, 20, 100));
        assert_ne!(first(4, 20, 100), first(4, 21, 100));
        assert_ne!(first(4, 20, 100), first(4, 20, 101));
        assert_ne!(first(4, 20, 100), first(5, 20, 100));
        // Swapping start and end is a different window and a different stream.
        assert_ne!(first(4, 20, 100), first(4, 100, 20));
        let mut rng = Rng::new(0);
        for bound in [1, 2, 7] {
            for _ in 0..200 {
                assert!(below(&mut rng, bound) < bound);
            }
        }
    }
}
