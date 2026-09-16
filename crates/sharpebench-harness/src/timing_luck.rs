//! Timing-luck floor: how far a reference result moves when only the phase of
//! the evaluation schedule moves.
//!
//! Execution seeds vary fills. Nothing else in the harness varies *when* a
//! window starts, so a board cannot say whether the gap between two rows is
//! larger than what a few bars of schedule phase would produce on its own. This
//! module measures that scale for the protocol and the dataset, with no
//! external entrant and no model: it re-runs the reference field `sharpebench
//! run` fields ([`reference_field`]: buy-and-hold, momentum and the luck-floor
//! random agents) and the no-op hold control with every declared window's start
//! shifted by `0..k` bars, and reports how far the Sharpe ratio and the
//! deflated Sharpe ratio move across those offsets.
//!
//! # Geometry
//!
//! For a declared window `[start, start + len)` and `k` declared offsets, offset
//! `o` evaluates `[start + o, start + o + len - (k - 1))`. Every shifted
//! instance of a window therefore has the same length, and every one lies inside
//! the declared window it perturbs. Three things follow:
//!
//! - no instance reads a bar past its declared window's end, so none reads past
//!   the dataset end;
//! - declared windows that are disjoint stay disjoint at every offset and
//!   across offsets, because each instance stays inside its own window;
//! - `k = 1` evaluates the declared windows exactly, so its figures are the
//!   unshifted ones.
//!
//! The cost is that for `k > 1` each instance is `k - 1` bars shorter than its
//! declared window, which [`ShiftedWindow::instance_len`] states. Instances of
//! one window at different offsets overlap each other (adjacent offsets share
//! all but one bar), so the offsets are not independent draws; the report says
//! so in [`ShiftGeometry::instances_of_one_window_overlap`].
//!
//! # Figures
//!
//! Per offset, each row's track is its seed-averaged, window-major pooled track
//! (`sharpebench_core::composite::pooled_returns`), the series the board's PSR
//! and deflated Sharpe read. The Sharpe ratio is
//! `sharpebench_core::deflated_sharpe::observed_sharpe_ratio` of that track and
//! is absent where the kernel says the track has none. The deflated Sharpe of a
//! reference row is the one `sharpebench_core::rank` publishes for the
//! reference field restricted to the same scope (every window, or one window),
//! so the all-windows figure at `k = 1` is the board's own. The hold control is
//! never ranked; its deflated Sharpe is `sharpebench_core::score_agent`'s. A
//! deflated Sharpe the kernel refused (`deflation_error`) is absent, not zero.
//!
//! Nothing here reaches the gate, eligibility or the rank:
//! [`TimingLuckReport::rank_input`] is always `false`, and the board a caller
//! ranks is never read or modified. Pure and deterministic given its inputs.

use std::fmt;

use serde::Serialize;
use sharpebench_core::composite::pooled_returns;
use sharpebench_core::deflated_sharpe::observed_sharpe_ratio;
use sharpebench_core::{rank, score_agent, AgentSubmission, CompositeScore, ScoreConfig};
use sharpebench_sim::{Agent, BuyAndHold, CostModel, Dataset, HoldAgent, Momentum, Window};

use crate::{luck_floor, run_agent, window_label};

/// Identifier of the report document.
pub const TIMING_LUCK_SCHEMA_VERSION: &str = "sharpebench.timing-luck.v1";

/// The fewest bars a shifted instance may have. A Sharpe ratio needs at least
/// two returns, and a window of `n` bars produces `n` returns.
pub const MIN_INSTANCE_BARS: usize = 2;

/// The scope label of the figures computed over every declared window at once.
pub const ALL_WINDOWS_SCOPE: &str = "all-windows";

/// Agent id of the buy-and-hold reference entrant, as `sharpebench run` fields it.
pub const BUY_AND_HOLD_ID: &str = "buy-and-hold";

/// Agent id of the momentum reference entrant, as `sharpebench run` fields it.
pub const MOMENTUM_ID: &str = "momentum";

/// The reference field `sharpebench run` ranks when no external entrant is
/// named, in its order: buy-and-hold, momentum, then `luck_floor_agents`
/// luck-floor random agents ([`luck_floor`]).
pub fn reference_field(
    data: &Dataset,
    windows: &[Window],
    seeds: &[u64],
    costs: CostModel,
    luck_floor_agents: usize,
) -> Vec<AgentSubmission> {
    let mut field = vec![
        run_agent(BUY_AND_HOLD_ID, data, windows, seeds, costs, || {
            Box::new(BuyAndHold) as Box<dyn Agent>
        }),
        run_agent(MOMENTUM_ID, data, windows, seeds, costs, || {
            Box::new(Momentum::default()) as Box<dyn Agent>
        }),
    ];
    field.extend(luck_floor(data, windows, seeds, costs, luck_floor_agents));
    field
}

/// What to measure: the declared number of start offsets and the reference
/// roster's caller-owned parts.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TimingLuckSpec<'a> {
    /// `k`: offsets `0..k` are evaluated. `1` evaluates the declared windows only.
    pub offsets: usize,
    /// How many luck-floor random agents the reference field carries.
    pub luck_floor_agents: usize,
    /// Agent id the no-op hold control is reported under.
    pub hold_control_id: &'a str,
}

/// Why no report was produced. Each variant names the input that decided it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "reason", rename_all = "snake_case")]
pub enum TimingLuckUnavailable {
    /// `k = 0`: no offset was declared, so nothing was measured.
    NoOffsets,
    /// No window was declared.
    NoWindows,
    /// No execution seed was declared.
    NoSeeds,
    /// A declared window ends after the dataset's last bar.
    WindowPastDatasetEnd { window: String, dataset_len: usize },
    /// A declared window is too short for `k` offsets: its shifted instances
    /// would have fewer than [`MIN_INSTANCE_BARS`] bars.
    WindowTooShortForOffsets {
        window: String,
        window_len: usize,
        offsets: usize,
        min_instance_bars: usize,
    },
}

impl fmt::Display for TimingLuckUnavailable {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoOffsets => f.write_str("no start offsets declared (k must be at least 1)"),
            Self::NoWindows => f.write_str("no evaluation windows declared"),
            Self::NoSeeds => f.write_str("no execution seeds declared"),
            Self::WindowPastDatasetEnd {
                window,
                dataset_len,
            } => write!(
                f,
                "window {window} ends past the dataset's {dataset_len} bars"
            ),
            Self::WindowTooShortForOffsets {
                window,
                window_len,
                offsets,
                min_instance_bars,
            } => write!(
                f,
                "window {window} has {window_len} bars; {offsets} start offsets leave shifted \
                 windows shorter than {min_instance_bars} bars"
            ),
        }
    }
}

impl std::error::Error for TimingLuckUnavailable {}

/// One declared window and the instances the offsets evaluate inside it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ShiftedWindow {
    /// The declared window, as `start-end`.
    pub declared: String,
    /// Bars in every shifted instance: the declared length minus `k - 1`.
    pub instance_len: usize,
    /// The instance at offset 0, as `start-end`.
    pub first_instance: String,
    /// The instance at offset `k - 1`, as `start-end`. It ends where the
    /// declared window ends.
    pub last_instance: String,
    /// Bars the first and last instances have in common.
    pub bars_shared_by_first_and_last_instance: usize,
    #[serde(skip)]
    start: usize,
}

/// Where the offsets were evaluated, and what that implies for independence.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ShiftGeometry {
    /// `k`, the declared number of start offsets.
    pub offsets: usize,
    pub dataset_len: usize,
    pub min_instance_bars: usize,
    pub windows: Vec<ShiftedWindow>,
    /// Whether the declared windows are pairwise disjoint.
    pub declared_windows_disjoint: bool,
    /// Whether the bars read by the instances of one declared window, over all
    /// offsets, are disjoint from those read for every other declared window.
    pub instances_of_distinct_windows_disjoint: bool,
    /// Whether instances of one window at different offsets share bars. True
    /// whenever `k > 1`: the offsets are overlapping shifts, not independent
    /// samples.
    pub instances_of_one_window_overlap: bool,
}

fn overlaps(a: (usize, usize), b: (usize, usize)) -> bool {
    a.0 < b.1 && b.0 < a.1
}

fn pairwise_disjoint(spans: &[(usize, usize)]) -> bool {
    spans
        .iter()
        .enumerate()
        .all(|(i, a)| spans[i + 1..].iter().all(|b| !overlaps(*a, *b)))
}

impl ShiftGeometry {
    /// Validate the declaration and lay out the instances, or say why not.
    pub fn resolve(
        dataset_len: usize,
        windows: &[Window],
        seeds: usize,
        offsets: usize,
    ) -> Result<Self, TimingLuckUnavailable> {
        if offsets == 0 {
            return Err(TimingLuckUnavailable::NoOffsets);
        }
        if windows.is_empty() {
            return Err(TimingLuckUnavailable::NoWindows);
        }
        if seeds == 0 {
            return Err(TimingLuckUnavailable::NoSeeds);
        }
        let lag = offsets - 1;
        let mut rows = Vec::with_capacity(windows.len());
        for &window in windows {
            let declared = window_label(window);
            if window.end > dataset_len {
                return Err(TimingLuckUnavailable::WindowPastDatasetEnd {
                    window: declared,
                    dataset_len,
                });
            }
            let window_len = window.end.saturating_sub(window.start);
            let Some(instance_len) = window_len
                .checked_sub(lag)
                .filter(|len| *len >= MIN_INSTANCE_BARS)
            else {
                return Err(TimingLuckUnavailable::WindowTooShortForOffsets {
                    window: declared,
                    window_len,
                    offsets,
                    min_instance_bars: MIN_INSTANCE_BARS,
                });
            };
            rows.push(ShiftedWindow {
                declared,
                instance_len,
                first_instance: window_label(Window {
                    start: window.start,
                    end: window.start + instance_len,
                }),
                last_instance: window_label(Window {
                    start: window.start + lag,
                    end: window.end,
                }),
                bars_shared_by_first_and_last_instance: instance_len.saturating_sub(lag),
                start: window.start,
            });
        }
        let declared: Vec<(usize, usize)> = windows.iter().map(|w| (w.start, w.end)).collect();
        // The bars every offset of a window reads together: from the first
        // instance's start to the last instance's end.
        let read: Vec<(usize, usize)> = rows
            .iter()
            .map(|row| (row.start, row.start + lag + row.instance_len))
            .collect();
        Ok(Self {
            offsets,
            dataset_len,
            min_instance_bars: MIN_INSTANCE_BARS,
            instances_of_one_window_overlap: offsets > 1,
            declared_windows_disjoint: pairwise_disjoint(&declared),
            instances_of_distinct_windows_disjoint: pairwise_disjoint(&read),
            windows: rows,
        })
    }

    /// The windows evaluated at `offset`, in declared order.
    pub fn instances(&self, offset: usize) -> Vec<Window> {
        self.windows
            .iter()
            .map(|row| Window {
                start: row.start + offset,
                end: row.start + offset + row.instance_len,
            })
            .collect()
    }
}

/// One figure across the offsets.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Spread {
    /// The figure at each offset, offset 0 first; `None` where it does not exist.
    pub by_offset: Vec<Option<f64>>,
    /// Offsets at which the figure exists. The spread is taken over these only.
    pub offsets_measured: usize,
    pub min: Option<f64>,
    pub max: Option<f64>,
    /// `max - min` over the measured offsets; `None` when none was measured.
    pub range: Option<f64>,
}

impl Spread {
    fn of(by_offset: Vec<Option<f64>>) -> Self {
        let measured: Vec<f64> = by_offset.iter().flatten().copied().collect();
        let min = measured.iter().copied().reduce(f64::min);
        let max = measured.iter().copied().reduce(f64::max);
        Self {
            offsets_measured: measured.len(),
            range: min.zip(max).map(|(lo, hi)| hi - lo),
            min,
            max,
            by_offset,
        }
    }
}

/// The figures of one row over one scope.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ScopeSpread {
    /// [`ALL_WINDOWS_SCOPE`], or the declared window as `start-end`.
    pub scope: String,
    /// Windows behind each figure: every declared window, or one.
    pub windows: usize,
    /// Offsets declared; see each [`Spread::offsets_measured`] for how many
    /// produced the figure.
    pub offsets: usize,
    pub sharpe: Spread,
    pub deflated_sharpe: Spread,
}

/// How a row entered the report.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ReferenceRole {
    /// A reference entrant of the `run` field. Its deflated Sharpe is the one
    /// `rank` publishes over the reference field in the same scope.
    RankedReference,
    /// A suite control. It is never ranked; its deflated Sharpe is the one
    /// `score_agent` gives it alone, under the configured prior.
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

/// The timing-luck floor of one protocol on one dataset.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct TimingLuckReport {
    pub schema_version: &'static str,
    /// Always `false`: nothing here is read by the gate, eligibility or the rank.
    pub rank_input: bool,
    /// Execution seeds per window, averaged per bar before any figure is taken.
    pub seeds: usize,
    pub geometry: ShiftGeometry,
    /// The reference field in its `run` order, then the hold control.
    pub rows: Vec<TimingLuckRow>,
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

fn sharpe_of(sub: &AgentSubmission, seeds: usize) -> Option<f64> {
    observed_sharpe_ratio(&pooled_returns(sub, seeds)).ok()
}

fn deflated_of(score: &CompositeScore) -> Option<f64> {
    score
        .deflation_error
        .is_none()
        .then_some(score.deflated_sharpe)
}

/// Per scope, per row, per offset: `(sharpe, deflated_sharpe)`.
type Figures = Vec<Vec<Vec<(Option<f64>, Option<f64>)>>>;

/// Measure the timing-luck floor of `windows` × `seeds` on `data` under
/// `costs` and `cfg`. `cfg.execution_seeds_per_window` is set to
/// `seeds.len()`, as `sharpebench run` sets it.
pub fn timing_luck(
    data: &Dataset,
    windows: &[Window],
    seeds: &[u64],
    costs: CostModel,
    cfg: &ScoreConfig,
    spec: TimingLuckSpec<'_>,
) -> Result<TimingLuckReport, TimingLuckUnavailable> {
    let geometry = ShiftGeometry::resolve(data.len(), windows, seeds.len(), spec.offsets)?;
    let mut cfg = cfg.clone();
    cfg.execution_seeds_per_window = seeds.len();
    let width = seeds.len();
    let scopes = windows.len() + 1;

    let mut roster: Vec<(String, ReferenceRole)> = Vec::new();
    let mut figures: Figures = vec![Vec::new(); scopes];
    for offset in 0..spec.offsets {
        let shifted = geometry.instances(offset);
        let field = reference_field(data, &shifted, seeds, costs, spec.luck_floor_agents);
        let control = run_agent(spec.hold_control_id, data, &shifted, seeds, costs, || {
            Box::new(HoldAgent) as Box<dyn Agent>
        });
        if offset == 0 {
            roster = field
                .iter()
                .map(|sub| (sub.agent_id.clone(), ReferenceRole::RankedReference))
                .chain([(control.agent_id.clone(), ReferenceRole::SuiteControl)])
                .collect();
            for by_row in &mut figures {
                *by_row = vec![Vec::with_capacity(spec.offsets); roster.len()];
            }
        }
        for (scope, by_row) in figures.iter_mut().enumerate() {
            let scoped: Vec<AgentSubmission> = field
                .iter()
                .map(|sub| restrict(sub, scope, width))
                .collect();
            let board = rank(&scoped, &cfg);
            let control = restrict(&control, scope, width);
            let cells = scoped
                .iter()
                .map(|sub| {
                    let deflated = board
                        .iter()
                        .find(|score| score.agent_id == sub.agent_id)
                        .and_then(deflated_of);
                    (sharpe_of(sub, width), deflated)
                })
                .chain([(
                    sharpe_of(&control, width),
                    deflated_of(&score_agent(&control, &cfg)),
                )]);
            for (by_offset, cell) in by_row.iter_mut().zip(cells) {
                by_offset.push(cell);
            }
        }
    }

    let scope_spread = |scope: usize, cells: &[(Option<f64>, Option<f64>)]| ScopeSpread {
        scope: match scope {
            0 => ALL_WINDOWS_SCOPE.to_string(),
            i => geometry.windows[i - 1].declared.clone(),
        },
        windows: if scope == 0 { windows.len() } else { 1 },
        offsets: spec.offsets,
        sharpe: Spread::of(cells.iter().map(|cell| cell.0).collect()),
        deflated_sharpe: Spread::of(cells.iter().map(|cell| cell.1).collect()),
    };
    let rows = roster
        .into_iter()
        .enumerate()
        .map(|(row, (agent_id, role))| TimingLuckRow {
            agent_id,
            role,
            all_windows: scope_spread(0, &figures[0][row]),
            per_window: (1..scopes)
                .map(|scope| scope_spread(scope, &figures[scope][row]))
                .collect(),
        })
        .collect();

    Ok(TimingLuckReport {
        schema_version: TIMING_LUCK_SCHEMA_VERSION,
        rank_input: false,
        seeds: width,
        geometry,
        rows,
    })
}
