//! Timing-luck floor: how far a reference result moves when only the phase of
//! its decision schedule moves.
//!
//! A board's reference agents decide on every bar, so they have no schedule to
//! shift. A strategy that rebalances every `m` bars does: its result depends
//! on which bars it rebalances on. This module runs the reference field that
//! `sharpebench run` ranks (buy-and-hold, momentum and the luck-floor random
//! agents), and the no-op hold control, under a declared cadence `m`, once for
//! every schedule phase `0..m`, over the full declared windows. It reports how
//! far each row's Sharpe ratio and deflated Sharpe ratio move across the
//! phases. It measures the protocol and the dataset: no external entrant runs
//! and no model is called.
//!
//! # Schedule
//!
//! Every run starts in cash, so every phase decides on the window's first bar.
//! After that, phase `p` decides on window step `j` exactly when
//! `j % m == p` ([`Cadenced::decides_at`]). Between scheduled decisions the
//! agent submits no orders, so the engine holds its positions; only an order
//! that a cost model with execution noise carried over can still fill. Every
//! phase evaluates every bar of every declared window: nothing is trimmed, and
//! cadence 1 (one phase, a decision on every bar) is the board's reference
//! field exactly.
//!
//! The wrapped agent is asked for a decision only on scheduled bars, so its
//! `i`-th decision is the same call at every phase. A luck-floor row therefore
//! applies the same sequence of random weights at every phase; the phase moves
//! the bars those weights are applied on.
//!
//! # Figures
//!
//! At each phase, each row's track is its seed-averaged, window-major pooled
//! track (`sharpebench_core::composite::pooled_returns`), the series the
//! board's PSR and deflated Sharpe read. The Sharpe ratio is
//! `sharpebench_core::deflated_sharpe::observed_sharpe_ratio` of that track,
//! absent where the kernel says the track has none.
//!
//! The deflated Sharpe holds the deflation bar fixed across phases. A board
//! deflates an entrant against one field, and moving that entrant's schedule
//! does not move the field; here every reference row moves phase together, so
//! a field measured at each phase would move the bar with the random rows and
//! give a row whose own track never changes a spread. So each row's deflation
//! inputs are the ones its score records at phase 0 in the same scope (every
//! window, or one window): from `sharpebench_core::rank` over the reference
//! field for a reference row, from `sharpebench_core::score_agent` alone for the
//! hold control. Every phase's track is deflated with those inputs
//! (`deflated_sharpe_ratio_against_null`), so a deflated Sharpe is reported
//! whenever that deflation succeeds, even where the score's bootstrap interval
//! failed. What each phase's own field would have measured is reported beside
//! it.
//!
//! Nothing here reaches the gate, eligibility or the rank:
//! [`TimingLuckReport::rank_input`] is always `false`, and no board a caller
//! ranks is read or modified. Pure and deterministic given its inputs.

use std::fmt;

use serde::Serialize;
use sharpebench_core::composite::pooled_returns;
use sharpebench_core::deflated_sharpe::{
    deflated_sharpe_ratio_against_null, expected_max_sharpe, observed_sharpe_ratio,
};
use sharpebench_core::stats::mean;
use sharpebench_core::{
    rank, score_agent, AgentSubmission, CompositeScore, ScoreConfig, TrialsSrStdSource,
};
use sharpebench_protocol::{Decision, MarketObservation};
use sharpebench_sim::trajectory::check_window_order;
use sharpebench_sim::{
    Agent, BuyAndHold, CostModel, Dataset, HoldAgent, Momentum, RandomAgent, Window,
};

use crate::{luck_floor_agent_id, run_agent, run_seeded_agent, window_label};

/// Identifier of the report document.
pub const TIMING_LUCK_SCHEMA_VERSION: &str = "sharpebench.timing-luck.v2";

/// The fewest bars a declared window may have: a Sharpe ratio needs two
/// returns, and a window of `n` bars produces `n` returns.
pub const MIN_WINDOW_BARS: usize = 2;

/// The scope label of the figures computed over every declared window at once.
pub const ALL_WINDOWS_SCOPE: &str = "all-windows";

/// Agent id of the buy-and-hold reference entrant, as `sharpebench run` fields it.
pub const BUY_AND_HOLD_ID: &str = "buy-and-hold";

/// Agent id of the momentum reference entrant, as `sharpebench run` fields it.
pub const MOMENTUM_ID: &str = "momentum";

/// An agent that decides only on its schedule: on the first step of a run, and
/// on every step `j` with `j % cadence == phase`. On every other step it
/// submits no orders, so the engine holds its positions. A fresh instance is
/// needed per run, because it counts the steps of the run it is driven through.
pub struct Cadenced {
    inner: Box<dyn Agent>,
    cadence: usize,
    phase: usize,
    step: usize,
}

impl Cadenced {
    /// `None` unless `cadence >= 1` and `phase < cadence`.
    pub fn new(inner: Box<dyn Agent>, cadence: usize, phase: usize) -> Option<Self> {
        (phase < cadence).then_some(Self {
            inner,
            cadence,
            phase,
            step: 0,
        })
    }

    /// Whether phase `phase` of `cadence` decides on window step `step`.
    /// Meaningful for `cadence >= 1` and `phase < cadence`, which
    /// [`Cadenced::new`] enforces.
    pub fn decides_at(cadence: usize, phase: usize, step: usize) -> bool {
        step == 0 || step % cadence == phase
    }

    /// Scheduled decisions in a window of `bars` bars.
    pub fn decisions_in(cadence: usize, phase: usize, bars: usize) -> usize {
        (0..bars)
            .filter(|&step| Self::decides_at(cadence, phase, step))
            .count()
    }
}

impl Agent for Cadenced {
    fn decide(&mut self, obs: &MarketObservation) -> Decision {
        let step = self.step;
        self.step += 1;
        if Self::decides_at(self.cadence, self.phase, step) {
            self.inner.decide(obs)
        } else {
            Decision {
                orders: Vec::new(),
                reasoning: "hold until the next scheduled decision".to_string(),
                cost: None,
            }
        }
    }
}

fn cadenced(inner: Box<dyn Agent>, cadence: usize, phase: usize) -> Box<dyn Agent> {
    Box::new(Cadenced::new(inner, cadence, phase).expect("phase is below the cadence"))
}

/// The seed of the `k`-th luck-floor agent's random agent for execution seed
/// `seed`, as [`crate::luck_floor`] draws it. Cadence 1 reproduces that
/// function's submissions exactly, which the tests pin.
fn luck_floor_seed(k: usize, seed: u64) -> u64 {
    0xF100_0000_0000_0000 ^ (k as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ seed
}

/// The reference field `sharpebench run` ranks when no entrant is named, in
/// its order (buy-and-hold, momentum, then `luck_floor_agents` luck-floor
/// random agents), with every agent deciding on the schedule of `phase` at
/// `cadence`. Cadence 1, phase 0 is that field exactly.
///
/// # Panics
///
/// If `phase >= cadence`.
pub fn cadenced_reference_field(
    data: &Dataset,
    windows: &[Window],
    seeds: &[u64],
    costs: CostModel,
    luck_floor_agents: usize,
    cadence: usize,
    phase: usize,
) -> Vec<AgentSubmission> {
    let mut field = vec![
        run_agent(BUY_AND_HOLD_ID, data, windows, seeds, costs, || {
            cadenced(Box::new(BuyAndHold), cadence, phase)
        }),
        run_agent(MOMENTUM_ID, data, windows, seeds, costs, || {
            cadenced(Box::new(Momentum::default()), cadence, phase)
        }),
    ];
    field.extend((0..luck_floor_agents).map(|k| {
        run_seeded_agent(
            &luck_floor_agent_id(k),
            data,
            windows,
            seeds,
            costs,
            move |seed| {
                cadenced(
                    Box::new(RandomAgent::new(luck_floor_seed(k, seed))),
                    cadence,
                    phase,
                )
            },
        )
    }));
    field
}

/// What to measure: the declared cadence and the reference roster's
/// caller-owned parts.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TimingLuckSpec<'a> {
    /// `m`: the agents decide every `m` bars, and phases `0..m` are evaluated.
    pub cadence: usize,
    /// How many luck-floor random agents the reference field carries.
    pub luck_floor_agents: usize,
    /// Agent id the no-op hold control is reported under.
    pub hold_control_id: &'a str,
}

/// Why no report was produced. Each variant names the input that decided it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "reason", rename_all = "snake_case")]
pub enum TimingLuckUnavailable {
    /// Cadence 0: no schedule was declared.
    NoCadence,
    /// No window was declared.
    NoWindows,
    /// No execution seed was declared.
    NoSeeds,
    /// The scoring frequency is not finite and positive, so no deflated Sharpe
    /// can be computed.
    InvalidPeriodsPerYear,
    /// The declared windows are out of time order or share a bar.
    WindowOrder { detail: String },
    /// A declared window ends after the dataset's last bar.
    WindowPastDatasetEnd { window: String, dataset_len: usize },
    /// A declared window has fewer bars than the cadence (so some phase would
    /// never decide after its first bar) or than [`MIN_WINDOW_BARS`].
    WindowShorterThanCadence {
        window: String,
        window_len: usize,
        cadence: usize,
        min_window_bars: usize,
    },
}

impl fmt::Display for TimingLuckUnavailable {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoCadence => f.write_str("no cadence declared (it must be at least 1)"),
            Self::NoWindows => f.write_str("no evaluation windows declared"),
            Self::NoSeeds => f.write_str("no execution seeds declared"),
            Self::InvalidPeriodsPerYear => {
                f.write_str("periods per year must be finite and positive")
            }
            Self::WindowOrder { detail } => f.write_str(detail),
            Self::WindowPastDatasetEnd {
                window,
                dataset_len,
            } => write!(
                f,
                "window {window} ends past the dataset's {dataset_len} bars"
            ),
            Self::WindowShorterThanCadence {
                window,
                window_len,
                cadence,
                min_window_bars,
            } => write!(
                f,
                "window {window} has {window_len} bars; cadence {cadence} needs at least \
                 {min_window_bars}"
            ),
        }
    }
}

impl std::error::Error for TimingLuckUnavailable {}

/// One declared window and the schedule every phase follows in it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct CadencedWindow {
    /// The declared window, as `start-end`. Every phase evaluates all of it.
    pub window: String,
    pub bars: usize,
    /// Scheduled decisions per run at each phase, phase 0 first. A phase above
    /// 0 counts its decision on the first bar, which is not on its cadence.
    pub decisions_by_phase: Vec<usize>,
}

/// One figure across the phases.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Spread {
    /// The figure at each phase, phase 0 first; `None` where it does not exist.
    pub by_phase: Vec<Option<f64>>,
    /// Phases at which the figure exists. The summaries are taken over these.
    pub phases_measured: usize,
    pub min: Option<f64>,
    pub max: Option<f64>,
    /// `max - min`; `None` when no phase was measured.
    pub range: Option<f64>,
    /// Standard deviation over the measured phases with divisor
    /// `phases_measured`: the phases are every phase of the cadence, not a
    /// sample of them.
    pub std_dev: Option<f64>,
}

impl Spread {
    fn of(by_phase: Vec<Option<f64>>) -> Self {
        let measured: Vec<f64> = by_phase.iter().flatten().copied().collect();
        let min = measured.iter().copied().reduce(f64::min);
        let max = measured.iter().copied().reduce(f64::max);
        let std_dev = (!measured.is_empty()).then(|| {
            let center = mean(&measured);
            let squares: f64 = measured.iter().map(|x| (x - center) * (x - center)).sum();
            (squares / measured.len() as f64).sqrt()
        });
        Self {
            phases_measured: measured.len(),
            range: min.zip(max).map(|(lo, hi)| hi - lo),
            min,
            max,
            std_dev,
            by_phase,
        }
    }
}

/// The deflation every phase of one row in one scope is tested with: the
/// inputs the row's score records at phase 0.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct DeflationBar {
    /// Per-period dispersion of Sharpe ratios across trials.
    pub trials_sr_std: f64,
    /// Measured on the phase-0 reference field, or the configured prior.
    pub trials_sr_std_source: TrialsSrStdSource,
    pub effective_n_trials: u32,
    pub null_mean_per_period: f64,
    /// The per-period Sharpe every phase is tested against; `None` when these
    /// inputs define no bar.
    pub deflation_bar_per_period: Option<f64>,
}

/// The dispersion the reference field at one phase measured for itself, as
/// its score records it. Reported beside the fixed bar, never used by it.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct FieldDispersion {
    pub trials_sr_std: f64,
    pub trials_sr_std_source: TrialsSrStdSource,
}

/// The figures of one row over one scope.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ScopeSpread {
    /// [`ALL_WINDOWS_SCOPE`], or the declared window as `start-end`.
    pub scope: String,
    /// Windows behind each figure: every declared window, or one.
    pub windows: usize,
    /// Phases declared; see each [`Spread::phases_measured`] for how many
    /// produced the figure.
    pub phases: usize,
    pub sharpe: Spread,
    /// Every phase deflated with [`ScopeSpread::deflation`].
    pub deflated_sharpe: Spread,
    pub deflation: DeflationBar,
    /// What each phase's own field measured, phase 0 first. For the hold
    /// control, which is scored alone, this is the configured prior.
    pub field_dispersion_by_phase: Vec<FieldDispersion>,
}

/// How a row entered the report.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ReferenceRole {
    /// A reference entrant of the `run` field, under the cadence, ranked with
    /// the other reference rows at each phase.
    RankedReference,
    /// A suite control. It is never ranked; it is scored alone, under the
    /// configured prior.
    SuiteControl,
}

/// One reference row.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct TimingLuckRow {
    pub agent_id: String,
    pub role: ReferenceRole,
    pub all_windows: ScopeSpread,
    /// One entry per declared window, in declared order.
    pub per_window: Vec<ScopeSpread>,
}

/// The timing-luck floor of one protocol on one dataset at one cadence.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct TimingLuckReport {
    pub schema_version: &'static str,
    /// Always `false`: nothing here is read by the gate, eligibility or the rank.
    pub rank_input: bool,
    /// Bars between scheduled decisions; phases `0..cadence` are evaluated.
    pub cadence: usize,
    /// Execution seeds per window, averaged per bar before any figure is taken.
    pub seeds: usize,
    pub dataset_len: usize,
    pub windows: Vec<CadencedWindow>,
    /// The reference field in its `run` order, then the hold control.
    pub rows: Vec<TimingLuckRow>,
}

fn validate(
    data: &Dataset,
    windows: &[Window],
    seeds: &[u64],
    cfg: &ScoreConfig,
    cadence: usize,
) -> Result<Vec<CadencedWindow>, TimingLuckUnavailable> {
    if cadence == 0 {
        return Err(TimingLuckUnavailable::NoCadence);
    }
    if windows.is_empty() {
        return Err(TimingLuckUnavailable::NoWindows);
    }
    if seeds.is_empty() {
        return Err(TimingLuckUnavailable::NoSeeds);
    }
    if !cfg.periods_per_year.is_finite() || cfg.periods_per_year <= 0.0 {
        return Err(TimingLuckUnavailable::InvalidPeriodsPerYear);
    }
    check_window_order(windows).map_err(|refusal| TimingLuckUnavailable::WindowOrder {
        detail: refusal.to_string(),
    })?;
    let min_window_bars = cadence.max(MIN_WINDOW_BARS);
    windows
        .iter()
        .map(|&window| {
            let label = window_label(window);
            if window.end > data.len() {
                return Err(TimingLuckUnavailable::WindowPastDatasetEnd {
                    window: label,
                    dataset_len: data.len(),
                });
            }
            let bars = window.end.saturating_sub(window.start);
            if bars < min_window_bars {
                return Err(TimingLuckUnavailable::WindowShorterThanCadence {
                    window: label,
                    window_len: bars,
                    cadence,
                    min_window_bars,
                });
            }
            Ok(CadencedWindow {
                window: label,
                bars,
                decisions_by_phase: (0..cadence)
                    .map(|phase| Cadenced::decisions_in(cadence, phase, bars))
                    .collect(),
            })
        })
        .collect()
}

/// `scope` 0 is every window; `scope` `i > 0` is declared window `i - 1`.
fn restrict(sub: &AgentSubmission, scope: usize, seeds: usize) -> AgentSubmission {
    if scope == 0 {
        return sub.clone();
    }
    let lo = (scope - 1) * seeds;
    AgentSubmission {
        runs: sub.runs[lo..lo + seeds].to_vec(),
        ..sub.clone()
    }
}

/// One row's track at one phase in one scope, with the deflation inputs its
/// score at that phase records.
struct Cell {
    pooled: Vec<f64>,
    score: CompositeScore,
}

impl Cell {
    fn new(sub: &AgentSubmission, score: &CompositeScore, seeds: usize) -> Self {
        Self {
            pooled: pooled_returns(sub, seeds),
            score: score.clone(),
        }
    }
}

impl DeflationBar {
    fn of(score: &CompositeScore) -> Self {
        Self {
            trials_sr_std: score.trials_sr_std,
            trials_sr_std_source: score.trials_sr_std_source,
            effective_n_trials: score.effective_n_trials,
            null_mean_per_period: score.deflation_null_mean_per_period,
            deflation_bar_per_period: expected_max_sharpe(
                score.trials_sr_std,
                score.effective_n_trials,
            )
            .ok()
            .map(|bar| score.deflation_null_mean_per_period + bar),
        }
    }

    /// The deflated Sharpe of `pooled` under this bar. A refusal of the
    /// score's bootstrap interval, which the score also reports through
    /// `deflation_error`, does not reach this computation.
    fn deflate(&self, pooled: &[f64]) -> Option<f64> {
        deflated_sharpe_ratio_against_null(
            pooled,
            self.effective_n_trials,
            self.null_mean_per_period,
            self.trials_sr_std,
        )
        .ok()
    }
}

/// Per scope, per row, per phase.
type Cells = Vec<Vec<Vec<Cell>>>;

/// Measure the timing-luck floor of `windows` x `seeds` on `data` under `costs`
/// and `cfg` at `spec.cadence`. `cfg.execution_seeds_per_window` is set to
/// `seeds.len()`, as `sharpebench run` sets it.
pub fn timing_luck(
    data: &Dataset,
    windows: &[Window],
    seeds: &[u64],
    costs: CostModel,
    cfg: &ScoreConfig,
    spec: TimingLuckSpec<'_>,
) -> Result<TimingLuckReport, TimingLuckUnavailable> {
    let cadenced_windows = validate(data, windows, seeds, cfg, spec.cadence)?;
    let mut cfg = cfg.clone();
    cfg.execution_seeds_per_window = seeds.len();
    let width = seeds.len();
    let scopes = windows.len() + 1;

    let mut roster: Vec<(String, ReferenceRole)> = Vec::new();
    let mut cells: Cells = (0..scopes).map(|_| Vec::new()).collect();
    for phase in 0..spec.cadence {
        let field = cadenced_reference_field(
            data,
            windows,
            seeds,
            costs,
            spec.luck_floor_agents,
            spec.cadence,
            phase,
        );
        let control = run_agent(spec.hold_control_id, data, windows, seeds, costs, || {
            cadenced(Box::new(HoldAgent), spec.cadence, phase)
        });
        if phase == 0 {
            roster = field
                .iter()
                .map(|sub| (sub.agent_id.clone(), ReferenceRole::RankedReference))
                .chain([(control.agent_id.clone(), ReferenceRole::SuiteControl)])
                .collect();
            for by_row in &mut cells {
                *by_row = (0..roster.len())
                    .map(|_| Vec::with_capacity(spec.cadence))
                    .collect();
            }
        }
        for (scope, by_row) in cells.iter_mut().enumerate() {
            let scoped: Vec<AgentSubmission> = field
                .iter()
                .map(|sub| restrict(sub, scope, width))
                .collect();
            let board = rank(&scoped, &cfg);
            let control = restrict(&control, scope, width);
            let control_cell = Cell::new(&control, &score_agent(&control, &cfg), width);
            let row_cells = scoped
                .iter()
                .map(|sub| {
                    let score = board
                        .iter()
                        .find(|score| score.agent_id == sub.agent_id)
                        .expect("rank scores every submission of a complete field");
                    Cell::new(sub, score, width)
                })
                .chain([control_cell]);
            for (by_phase, cell) in by_row.iter_mut().zip(row_cells) {
                by_phase.push(cell);
            }
        }
    }

    let scope_spread = |scope: usize, cells: &[Cell]| {
        let deflation = DeflationBar::of(&cells[0].score);
        ScopeSpread {
            scope: match scope {
                0 => ALL_WINDOWS_SCOPE.to_string(),
                i => cadenced_windows[i - 1].window.clone(),
            },
            windows: if scope == 0 { windows.len() } else { 1 },
            phases: spec.cadence,
            sharpe: Spread::of(
                cells
                    .iter()
                    .map(|c| observed_sharpe_ratio(&c.pooled).ok())
                    .collect(),
            ),
            deflated_sharpe: Spread::of(
                cells.iter().map(|c| deflation.deflate(&c.pooled)).collect(),
            ),
            field_dispersion_by_phase: cells
                .iter()
                .map(|c| FieldDispersion {
                    trials_sr_std: c.score.trials_sr_std,
                    trials_sr_std_source: c.score.trials_sr_std_source,
                })
                .collect(),
            deflation,
        }
    };
    let rows = roster
        .into_iter()
        .enumerate()
        .map(|(row, (agent_id, role))| TimingLuckRow {
            agent_id,
            role,
            all_windows: scope_spread(0, &cells[0][row]),
            per_window: (1..scopes)
                .map(|scope| scope_spread(scope, &cells[scope][row]))
                .collect(),
        })
        .collect();

    Ok(TimingLuckReport {
        schema_version: TIMING_LUCK_SCHEMA_VERSION,
        rank_input: false,
        cadence: spec.cadence,
        seeds: width,
        dataset_len: data.len(),
        windows: cadenced_windows,
        rows,
    })
}
