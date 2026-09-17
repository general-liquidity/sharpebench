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
//!   level) and moves them to random places in the window.
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
use sharpebench_core::deflated_sharpe::sharpe_ratio;
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

/// Gross exposure, as a fraction of NAV after the bar's trades, above which a
/// bar counts as invested.
pub const INVESTED_GROSS_FLOOR: f64 = 1e-9;

/// Draws [`TimingNullConfig::default`] declares.
pub const DEFAULT_TIMING_NULL_DRAWS: usize = 200;

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
    /// A lagged replay with no declared lag measures nothing.
    EmptyLagSet,
    /// A run is too short to leave two bars after the longest lag's first held bar.
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

/// One lag's figures over the bars every declared lag is compared on.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct LagFigures {
    pub lag: usize,
    /// Mean over runs of each run's per-period Sharpe on the compared bars.
    pub mean_sharpe: f64,
    /// Mean per-period return over every run's compared bars, pooled.
    pub mean_return: f64,
    pub sharpe_by_run: Vec<f64>,
    pub mean_return_by_run: Vec<f64>,
}

/// [`lagged_replay`]'s report.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct LaggedReplayReport {
    /// The declared lags, ascending and without repeats.
    pub lags: Vec<usize>,
    /// Leading bars left out of every figure in every run: one more than the
    /// longest lag, so each compared bar is earned by a position every lag has
    /// already taken, and no row carries its opening trade.
    pub skipped_leading_bars: usize,
    /// Compared bars summed over runs.
    pub compared_bars: usize,
    pub runs: usize,
    /// The recorded decisions on time, on the same bars.
    pub undelayed: LagFigures,
    pub lagged: Vec<LagFigures>,
    pub valid_when: &'static str,
}

/// Replay every run's decisions `k` bars late for each declared `k` and report
/// Sharpe and mean return beside the undelayed figures.
///
/// Every row, the undelayed one included, is computed on the same bars: each
/// run's bars after the first `max(lags) + 1`. A lag of zero replays the
/// recorded decisions unchanged, so its row equals the undelayed row. This
/// measures decision-delay sensitivity under whatever `costs` the caller
/// passes, for example the stressed profile with its declared
/// `decision_delay_bars`, which the backtest driver itself does not apply.
pub fn lagged_replay(
    data: &Dataset,
    traj: &AgentTrajectory,
    costs: CostModel,
    lags: &[usize],
) -> Result<LaggedReplayReport, ReplayNullRefusal> {
    check_runs(data, traj)?;
    let declared: std::collections::BTreeSet<usize> = lags.iter().copied().collect();
    let Some(&longest) = declared.last() else {
        return Err(ReplayNullRefusal::EmptyLagSet);
    };
    let skipped = longest + 1;
    for (index, run) in traj.runs.iter().enumerate() {
        if run.steps.len() < skipped + 2 {
            return Err(ReplayNullRefusal::LagTooLong {
                lag: longest,
                run: index,
                steps: run.steps.len(),
            });
        }
    }
    let figures = |lag: usize| {
        let mut sharpe_by_run = Vec::with_capacity(traj.runs.len());
        let mut mean_return_by_run = Vec::with_capacity(traj.runs.len());
        let mut pooled = 0.0;
        let mut bars = 0;
        for run in &traj.runs {
            let replayed = replay_run(data, &lagged_trajectory(run, lag), costs);
            let compared = &replayed.returns[skipped..];
            sharpe_by_run.push(sharpe_ratio(compared));
            mean_return_by_run.push(mean(compared));
            pooled += compared.iter().sum::<f64>();
            bars += compared.len();
        }
        LagFigures {
            lag,
            mean_sharpe: mean(&sharpe_by_run),
            mean_return: pooled / bars as f64,
            sharpe_by_run,
            mean_return_by_run,
        }
    };
    let undelayed = figures(0);
    let compared_bars = traj.runs.iter().map(|run| run.steps.len() - skipped).sum();
    Ok(LaggedReplayReport {
        lags: declared.iter().copied().collect(),
        skipped_leading_bars: skipped,
        compared_bars,
        runs: traj.runs.len(),
        lagged: declared.iter().map(|&lag| figures(lag)).collect(),
        undelayed,
        valid_when: VALID_WHEN,
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
    pub reference_mean_sharpe: f64,
}

impl TimingPercentile {
    fn of(value: f64, reference: &[f64]) -> Self {
        let below = reference.iter().filter(|&&draw| draw < value).count();
        let ties = reference.iter().filter(|&&draw| draw == value).count();
        Self {
            draws: reference.len(),
            below,
            ties,
            percentile: (below as f64 + ties as f64 / 2.0) / reference.len() as f64,
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
        /// Mean invested bars per draw after the engine applied the costs; it
        /// can differ from the entrant's under a liquidity cap or execution noise.
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

/// The draw stream for one run: a pure function of the declared seed and the
/// run's position in the trajectory.
fn run_stream(seed: u64, run: usize) -> Rng {
    Rng::new(mix64(seed ^ mix64(run as u64 ^ 0x7131_4E55_0000_0000)))
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
    let flat = flat_decision(&symbols);
    let mut rng = run_stream(config.seed, index);
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
/// A run that was never invested, or invested on every bar, is typed
/// unavailable rather than given a degenerate percentile. The report is a
/// pure function of the trajectory, the data, `costs` and `config`.
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
            placed,
            TimingPercentile {
                draws: 4,
                below: 1,
                ties: 2,
                percentile: 0.5,
                reference_mean_sharpe: 1.125,
            }
        );
        assert_eq!(TimingPercentile::of(3.0, &[0.5, 2.0]).percentile, 1.0);
        assert_eq!(TimingPercentile::of(0.0, &[0.5, 2.0]).percentile, 0.0);
    }

    #[test]
    fn run_streams_differ_by_run_and_by_seed() {
        let first = |seed, run| run_stream(seed, run).unit();
        assert_eq!(first(4, 2), first(4, 2));
        assert_ne!(first(4, 2), first(4, 3));
        assert_ne!(first(4, 2), first(5, 2));
        let mut rng = Rng::new(0);
        for bound in [1, 2, 7] {
            for _ in 0..200 {
                assert!(below(&mut rng, bound) < bound);
            }
        }
    }
}
