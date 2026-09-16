//! Volatility-response sizing: does an agent's exposure move with the
//! volatility it faced?
//!
//! DX Research Group's six-month production record of LLM trading agents
//! (*What LLM Trading Agents Actually Do in Production*, 2026,
//! arXiv:2609.05663, section 5.1 and Table 3) split 6,400 closed positions
//! into sextiles of entry-time volatility and found a median leverage of 5.0x
//! in every sextile across a 5.7x volatility spread. None of the benchmark's
//! other reports relates exposure to volatility, so an agent that holds the
//! same exposure through a volatility spike and one that targets volatility
//! look alike on every report until the loss lands. This module is that
//! missing view, computed from a recorded run.
//!
//! For every bar `t` of every run in a trajectory it pairs two numbers:
//!
//! - **post-fill gross exposure**: the sum over symbols of
//!   `|shares * close(t)|` for the holdings after the bar's fills, divided by
//!   the NAV at `close(t)` before them. That NAV is the base the engine sizes
//!   target weights against, so a filled target of weight `w` reads as `|w|`.
//!   Dividing by the NAV after the fills would instead make a fully invested
//!   book read above 1x by exactly the bar's own trading costs. The book is
//!   rebuilt by replaying the recorded decisions through [`TradingEnv`], which
//!   runs the same per-step body as [`crate::run_backtest`], so it is the book
//!   the run was scored on;
//! - **trailing realized volatility**: for each symbol, the sample standard
//!   deviation of the `vol_lookback` simple returns that end at bar `t`,
//!   computed from closes `t - vol_lookback ..= t` and never from a later
//!   close; the bar's figure is the mean across the dataset's symbols.
//!
//! Over the pooled pairs it reports Spearman's rank correlation between
//! exposure and volatility, and the median gross exposure in each volatility
//! quintile, each with the number of pairs behind it. A risk-managed agent
//! that sizes down in turbulence shows a negative correlation; an agent that
//! ignores volatility shows none; one that sizes up shows a positive one.
//!
//! Exposure has a declared resolution, `exposure_resolution` (1% of NAV by
//! default). Costs paid out of cash and price drift between rebalances move a
//! book's exposure by fractions of a percent without any sizing decision. When
//! the pooled exposure spans no more than the resolution (buy-and-hold, or
//! always flat), there is no sizing to rank, and the run reports
//! [`SizingUnavailable::ConstantExposure`] rather than a correlation of 0. Too
//! few pairs and constant volatility are reported the same way. Otherwise the
//! correlation ranks exposure rounded to the resolution grid, so movements
//! below it do not order bars the agent sized alike. The quintile medians are
//! of the unrounded exposure.
//!
//! Volatility is a cross-sectional mean, so this is a book-level view. An
//! agent that keeps its gross exposure constant while rotating into calmer
//! names reads as unresponsive here. Pairs from different execution seeds of
//! the same window share bars, so the pair count is not a count of
//! independent observations, and no p-value is reported.
//!
//! The report is a separate record. No gate, eligibility rule or rank reads
//! it, and it is marked `used_by_gate: false`.

use serde::{Deserialize, Serialize};
use sharpebench_protocol::{AgentTrajectory, PositionState, RunTrajectory};
use sharpebench_stats::spearman_rho;

use crate::costs::CostModel;
use crate::data::Dataset;
use crate::engine::Window;
use crate::env::TradingEnv;

/// The identifier the command line accepts for this diagnostic.
pub const SIZING_RESPONSE_ID: &str = "sizing-response";
/// Default number of simple returns in the trailing volatility window.
pub const DEFAULT_VOL_LOOKBACK: usize = 20;
/// Default exposure resolution, in units of NAV.
pub const DEFAULT_EXPOSURE_RESOLUTION: f64 = 0.01;
/// Default fewest pairs a figure is computed from.
pub const DEFAULT_MIN_PAIRS: usize = 30;
/// Volatility buckets in the median table.
pub const VOLATILITY_QUINTILES: usize = 5;
/// Relative spread at or below which the trailing volatility is treated as
/// constant. It absorbs floating-point noise only: a whipsaw series whose
/// returns alternate between the same two values has one volatility, even if
/// the last bits of its standard deviations differ.
const VOLATILITY_TIE_TOLERANCE: f64 = 1e-12;

/// The declared parameters of the diagnostic. Every report carries them.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct SizingResponseConfig {
    /// Simple returns in the trailing volatility window. Bar `t` reads closes
    /// `t - vol_lookback ..= t`, so its first pair is at `t = vol_lookback`.
    pub vol_lookback: usize,
    /// Smallest exposure change, in units of NAV, the diagnostic treats as
    /// sizing: the constant-exposure threshold and the ranking grid.
    pub exposure_resolution: f64,
    /// Fewest pairs the correlation and the quintile table are computed from.
    pub min_pairs: usize,
}

impl Default for SizingResponseConfig {
    fn default() -> Self {
        Self {
            vol_lookback: DEFAULT_VOL_LOOKBACK,
            exposure_resolution: DEFAULT_EXPOSURE_RESOLUTION,
            min_pairs: DEFAULT_MIN_PAIRS,
        }
    }
}

impl SizingResponseConfig {
    /// Refuse parameters under which a figure would not mean what it says: a
    /// standard deviation needs two returns, the grid must be a positive finite
    /// width, and every quintile needs at least one pair.
    pub fn validate(&self) -> Result<(), SizingResponseError> {
        if self.vol_lookback < 2 {
            return Err(SizingResponseError::InvalidConfig(format!(
                "vol_lookback must be at least 2 returns, got {}",
                self.vol_lookback
            )));
        }
        if !self.exposure_resolution.is_finite() || self.exposure_resolution <= 0.0 {
            return Err(SizingResponseError::InvalidConfig(format!(
                "exposure_resolution must be positive and finite, got {}",
                self.exposure_resolution
            )));
        }
        if self.min_pairs < VOLATILITY_QUINTILES {
            return Err(SizingResponseError::InvalidConfig(format!(
                "min_pairs must be at least {VOLATILITY_QUINTILES}, got {}",
                self.min_pairs
            )));
        }
        Ok(())
    }
}

/// Why the diagnostic was not computed at all.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SizingResponseError {
    /// A parameter is out of its domain.
    InvalidConfig(String),
    /// The cost model is invalid, so the run cannot be replayed.
    InvalidCosts(String),
    /// A run does not carry a decision for every bar of its window. Replaying
    /// it would supply holds the agent never chose.
    IncompleteRun {
        run: usize,
        recorded: usize,
        required: usize,
    },
    /// A run's window ends after the dataset does, so this is not the dataset
    /// the run was recorded on.
    WindowOutsideData {
        run: usize,
        window_end: usize,
        dataset_len: usize,
    },
}

impl std::fmt::Display for SizingResponseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidConfig(message) => write!(f, "invalid sizing-response config: {message}"),
            Self::InvalidCosts(message) => write!(f, "invalid cost model: {message}"),
            Self::IncompleteRun {
                run,
                recorded,
                required,
            } => write!(
                f,
                "run {run} records {recorded} of {required} decisions; the sizing-response replay would have to invent the rest"
            ),
            Self::WindowOutsideData {
                run,
                window_end,
                dataset_len,
            } => write!(
                f,
                "run {run} ends at bar {window_end} but the dataset has {dataset_len} bars"
            ),
        }
    }
}

impl std::error::Error for SizingResponseError {}

/// Why one figure is absent. The absent figure is never replaced by a number.
#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
#[serde(tag = "reason", rename_all = "snake_case")]
pub enum SizingUnavailable {
    /// Fewer pairs than the declared minimum.
    TooFewPairs { pairs: usize, min_pairs: usize },
    /// The pooled exposure spans no more than the declared resolution, from
    /// `min_gross_exposure` to `max_gross_exposure` (units of NAV): there is
    /// no sizing to rank.
    ConstantExposure {
        min_gross_exposure: f64,
        max_gross_exposure: f64,
    },
    /// Every pair faced the same trailing volatility: there is nothing to
    /// respond to.
    ConstantVolatility { volatility: f64 },
}

impl std::fmt::Display for SizingUnavailable {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::TooFewPairs { pairs, min_pairs } => {
                write!(f, "{pairs} pairs, fewer than the {min_pairs} required")
            }
            Self::ConstantExposure {
                min_gross_exposure,
                max_gross_exposure,
            } => write!(
                f,
                "gross exposure stays between {min_gross_exposure:.4}x and {max_gross_exposure:.4}x NAV, within the resolution"
            ),
            Self::ConstantVolatility { volatility } => {
                write!(f, "trailing volatility is constant at {volatility:.6}")
            }
        }
    }
}

/// One bar's exposure and the volatility it faced.
#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub struct ExposurePair {
    /// Index of the run in the trajectory.
    pub run: usize,
    /// Dataset bar index.
    pub bar: usize,
    /// Post-fill gross exposure in units of the bar's pre-fill NAV,
    /// unrounded.
    pub gross_exposure: f64,
    /// Cross-sectional mean of the per-symbol trailing volatilities.
    pub trailing_volatility: f64,
}

/// What happened to every replayed bar.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
pub struct PairCensus {
    /// Bars replayed across all runs.
    pub bars_replayed: usize,
    /// Bars that became a pair.
    pub pairs: usize,
    /// Bars earlier than `vol_lookback`, with too little history.
    pub bars_without_history: usize,
    /// Bars where some symbol's window holds a non-positive or non-finite
    /// close, so a simple return is undefined.
    pub bars_without_volatility: usize,
    /// Bars whose pre-fill NAV is not positive, or whose exposure is not
    /// finite.
    pub bars_without_exposure: usize,
}

/// The rank correlation between exposure and trailing volatility.
#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub struct RankCorrelation {
    /// Pairs the figure is computed from.
    pub pairs: usize,
    /// Spearman's rho over the grid exposure and the trailing volatility.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub spearman_rho: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unavailable: Option<SizingUnavailable>,
}

/// One volatility quintile. Quintile 1 is the calmest.
#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub struct VolatilityQuintile {
    pub quintile: usize,
    /// Pairs in the quintile. Tied volatilities share a quintile, so counts
    /// can differ and a quintile can be empty.
    pub pairs: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub median_volatility: Option<f64>,
    /// Median unrounded gross exposure, in units of NAV.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub median_gross_exposure: Option<f64>,
}

/// The median table.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct QuintileMedians {
    /// Empty when the table is unavailable.
    pub quintiles: Vec<VolatilityQuintile>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unavailable: Option<SizingUnavailable>,
}

/// The diagnostic for one agent's trajectory.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct SizingResponse {
    pub agent_id: String,
    /// Always `false`: recorded so a reader of the JSON alone cannot mistake
    /// the diagnostic for a score input.
    pub used_by_gate: bool,
    pub config: SizingResponseConfig,
    /// Runs replayed.
    pub runs: usize,
    pub census: PairCensus,
    pub rank_correlation: RankCorrelation,
    pub by_volatility: QuintileMedians,
}

/// Compute the diagnostic for every run of `traj`, pooled.
///
/// `data` and `costs` must be those the trajectory was recorded against, as
/// for [`crate::replay_submission`]. Nothing is scored and no agent is called.
pub fn sizing_response(
    data: &Dataset,
    traj: &AgentTrajectory,
    costs: CostModel,
    cfg: &SizingResponseConfig,
) -> Result<SizingResponse, SizingResponseError> {
    let (pairs, census) = exposure_volatility_pairs(data, traj, costs, cfg)?;
    Ok(SizingResponse {
        agent_id: traj.agent_id.clone(),
        used_by_gate: false,
        config: *cfg,
        runs: traj.runs.len(),
        census,
        rank_correlation: rank_correlation(&pairs, cfg),
        by_volatility: quintile_medians(&pairs, cfg),
    })
}

/// The pairs behind [`sizing_response`], in run then bar order, with the
/// census of the bars that did not become one.
pub fn exposure_volatility_pairs(
    data: &Dataset,
    traj: &AgentTrajectory,
    costs: CostModel,
    cfg: &SizingResponseConfig,
) -> Result<(Vec<ExposurePair>, PairCensus), SizingResponseError> {
    cfg.validate()?;
    costs
        .validate()
        .map_err(SizingResponseError::InvalidCosts)?;
    for (index, run) in traj.runs.iter().enumerate() {
        if run.window_end > data.len() {
            return Err(SizingResponseError::WindowOutsideData {
                run: index,
                window_end: run.window_end,
                dataset_len: data.len(),
            });
        }
        let required = run.window_end.saturating_sub(run.window_start);
        if run.steps.len() != required {
            return Err(SizingResponseError::IncompleteRun {
                run: index,
                recorded: run.steps.len(),
                required,
            });
        }
    }

    let mut pairs = Vec::new();
    let mut census = PairCensus::default();
    for (index, run) in traj.runs.iter().enumerate() {
        for state in replay_book(data, run, costs) {
            census.bars_replayed += 1;
            let volatility = match trailing_volatility(data, state.bar, cfg.vol_lookback) {
                Ok(v) => v,
                Err(VolatilityGap::History) => {
                    census.bars_without_history += 1;
                    continue;
                }
                Err(VolatilityGap::Prices) => {
                    census.bars_without_volatility += 1;
                    continue;
                }
            };
            let exposure = state.gross_value / state.nav_before;
            let measurable = state.nav_before > 0.0 && exposure.is_finite();
            if !measurable {
                census.bars_without_exposure += 1;
                continue;
            }
            census.pairs += 1;
            pairs.push(ExposurePair {
                run: index,
                bar: state.bar,
                gross_exposure: exposure,
                trailing_volatility: volatility,
            });
        }
    }
    Ok((pairs, census))
}

/// The book at one replayed bar.
struct BarState {
    bar: usize,
    /// NAV at the bar's close before its fills: the engine's sizing base.
    nav_before: f64,
    /// Sum of `|shares * close|` after the bar's fills.
    gross_value: f64,
    /// The bar's return, kept so a test can show this is the scored book.
    #[cfg_attr(not(test), allow(dead_code))]
    reward: f64,
}

/// Replay `run`'s recorded decisions and read the book after every bar.
fn replay_book(data: &Dataset, run: &RunTrajectory, costs: CostModel) -> Vec<BarState> {
    let window = Window {
        start: run.window_start,
        end: run.window_end,
    };
    let mut env = TradingEnv::new(data.clone(), window, costs, run.seed);
    // An observation carries the book as it stands before the bar it is
    // handed out for: its cash and holdings. Its price history is not read.
    let mut book = env.reset();
    let mut states = Vec::with_capacity(run.steps.len());
    for (offset, step) in run.steps.iter().enumerate() {
        let bar = run.window_start + offset;
        let mark = |p: &PositionState| p.shares * data.close_at(&p.symbol, bar).unwrap_or(0.0);
        // The engine's own pre-trade NAV: cash plus holdings in symbol order.
        let nav_before = book.cash + book.portfolio.iter().map(mark).sum::<f64>();
        let result = env.step(step.decision.clone());
        let gross_value = result
            .observation
            .portfolio
            .iter()
            .map(|p| mark(p).abs())
            .sum::<f64>();
        states.push(BarState {
            bar,
            nav_before,
            gross_value,
            reward: result.reward,
        });
        book = result.observation;
    }
    states
}

enum VolatilityGap {
    History,
    Prices,
}

/// Mean across symbols of the sample standard deviation of the `lookback`
/// simple returns ending at bar `t`. Reads closes `t - lookback ..= t` only,
/// through [`Dataset::history`].
fn trailing_volatility(data: &Dataset, t: usize, lookback: usize) -> Result<f64, VolatilityGap> {
    let symbols = data.symbols();
    if symbols.is_empty() || t < lookback {
        return Err(VolatilityGap::History);
    }
    let mut total = 0.0;
    for symbol in &symbols {
        let closes = data.history(symbol, t, lookback + 1);
        if closes.len() != lookback + 1 {
            return Err(VolatilityGap::History);
        }
        if closes.iter().any(|c| !c.is_finite() || *c <= 0.0) {
            return Err(VolatilityGap::Prices);
        }
        let returns: Vec<f64> = closes.windows(2).map(|w| w[1] / w[0] - 1.0).collect();
        let n = returns.len() as f64;
        let mean = returns.iter().sum::<f64>() / n;
        let variance = returns.iter().map(|r| (r - mean).powi(2)).sum::<f64>() / (n - 1.0);
        total += variance.sqrt();
    }
    let volatility = total / symbols.len() as f64;
    if volatility.is_finite() {
        Ok(volatility)
    } else {
        Err(VolatilityGap::Prices)
    }
}

fn volatilities(pairs: &[ExposurePair]) -> Vec<f64> {
    pairs.iter().map(|p| p.trailing_volatility).collect()
}

fn range(values: &[f64]) -> (f64, f64) {
    values
        .iter()
        .fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), &v| {
            (lo.min(v), hi.max(v))
        })
}

fn constant_volatility(vols: &[f64]) -> Option<SizingUnavailable> {
    let (lo, hi) = range(vols);
    (hi - lo <= VOLATILITY_TIE_TOLERANCE * hi.abs())
        .then_some(SizingUnavailable::ConstantVolatility { volatility: lo })
}

fn too_few(pairs: &[ExposurePair], cfg: &SizingResponseConfig) -> Option<SizingUnavailable> {
    (pairs.len() < cfg.min_pairs).then_some(SizingUnavailable::TooFewPairs {
        pairs: pairs.len(),
        min_pairs: cfg.min_pairs,
    })
}

fn rank_correlation(pairs: &[ExposurePair], cfg: &SizingResponseConfig) -> RankCorrelation {
    let unavailable = |reason| RankCorrelation {
        pairs: pairs.len(),
        spearman_rho: None,
        unavailable: Some(reason),
    };
    if let Some(reason) = too_few(pairs, cfg) {
        return unavailable(reason);
    }
    let exposure: Vec<f64> = pairs.iter().map(|p| p.gross_exposure).collect();
    let (lo, hi) = range(&exposure);
    if hi - lo <= cfg.exposure_resolution {
        return unavailable(SizingUnavailable::ConstantExposure {
            min_gross_exposure: lo,
            max_gross_exposure: hi,
        });
    }
    // A span wider than one grid step always leaves at least two cells.
    let cells: Vec<f64> = exposure
        .iter()
        .map(|e| (e / cfg.exposure_resolution).round())
        .collect();
    let vols = volatilities(pairs);
    if let Some(reason) = constant_volatility(&vols) {
        return unavailable(reason);
    }
    match spearman_rho(&cells, &vols) {
        Some(rho) => RankCorrelation {
            pairs: pairs.len(),
            spearman_rho: Some(rho),
            unavailable: None,
        },
        // Both series vary and are finite, so the only way left for the
        // correlation to be undefined is a volatility series that is constant
        // below the tie tolerance's reach.
        None => unavailable(SizingUnavailable::ConstantVolatility {
            volatility: vols[0],
        }),
    }
}

fn quintile_medians(pairs: &[ExposurePair], cfg: &SizingResponseConfig) -> QuintileMedians {
    let unavailable = |reason| QuintileMedians {
        quintiles: Vec::new(),
        unavailable: Some(reason),
    };
    if let Some(reason) = too_few(pairs, cfg) {
        return unavailable(reason);
    }
    let vols = volatilities(pairs);
    if let Some(reason) = constant_volatility(&vols) {
        return unavailable(reason);
    }
    let n = pairs.len();
    let mut sorted = vols.clone();
    sorted.sort_by(f64::total_cmp);
    let mut buckets: Vec<(Vec<f64>, Vec<f64>)> =
        vec![(Vec::new(), Vec::new()); VOLATILITY_QUINTILES];
    for pair in pairs {
        let v = pair.trailing_volatility;
        // The midrank of `v` is (below + at_or_below + 1) / 2, so tied
        // volatilities always land in the same quintile.
        let below = sorted.partition_point(|x| *x < v);
        let at_or_below = sorted.partition_point(|x| *x <= v);
        let quintile = ((below + at_or_below - 1) * VOLATILITY_QUINTILES) / (2 * n);
        let bucket = &mut buckets[quintile];
        bucket.0.push(v);
        bucket.1.push(pair.gross_exposure);
    }
    QuintileMedians {
        quintiles: buckets
            .into_iter()
            .enumerate()
            .map(|(i, (mut vol, mut exposure))| VolatilityQuintile {
                quintile: i + 1,
                pairs: vol.len(),
                median_volatility: median(&mut vol),
                median_gross_exposure: median(&mut exposure),
            })
            .collect(),
        unavailable: None,
    }
}

fn median(values: &mut [f64]) -> Option<f64> {
    if values.is_empty() {
        return None;
    }
    values.sort_by(f64::total_cmp);
    let mid = values.len() / 2;
    Some(if values.len().is_multiple_of(2) {
        (values[mid - 1] + values[mid]) / 2.0
    } else {
        values[mid]
    })
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;
    use crate::agent::{Agent, BuyAndHold, HoldAgent, Momentum};
    use crate::trajectory::{replay_run, run_backtest_capture};
    use sharpebench_protocol::{Action, Decision, MarketObservation, Order};

    /// Sample standard deviation of the simple returns in a close history.
    fn observed_volatility(closes: &[f64]) -> f64 {
        let returns: Vec<f64> = closes.windows(2).map(|w| w[1] / w[0] - 1.0).collect();
        let n = returns.len() as f64;
        let mean = returns.iter().sum::<f64>() / n;
        (returns.iter().map(|r| (r - mean).powi(2)).sum::<f64>() / (n - 1.0)).sqrt()
    }

    /// Sizes the first symbol from its own observed volatility, clamped to
    /// `[0, cap]`: `target / vol` targets volatility, `vol * scale` does the
    /// opposite.
    struct VolSizer {
        size: fn(f64) -> f64,
    }

    impl Agent for VolSizer {
        fn decide(&mut self, obs: &MarketObservation) -> Decision {
            let snapshot = &obs.symbols[0];
            let weight = (self.size)(observed_volatility(&snapshot.close_history));
            Decision {
                orders: vec![Order {
                    symbol: snapshot.symbol.clone(),
                    action: Action::Buy,
                    target_weight: weight,
                    confidence: 0.5,
                    rationale: String::new(),
                }],
                reasoning: String::new(),
                cost: None,
            }
        }
    }

    fn vol_targeting() -> VolSizer {
        VolSizer {
            size: |vol| (0.004 / vol).min(1.0),
        }
    }

    fn vol_chasing() -> VolSizer {
        VolSizer {
            size: |vol| (vol * 60.0).min(2.0),
        }
    }

    /// One symbol whose per-bar volatility alternates between a calm regime
    /// (0.4%) and a turbulent one (3%) every 60 bars. The shock sequence is a
    /// fixed pseudo-random draw, so each regime's realized volatility varies
    /// bar to bar instead of collapsing into a whipsaw's single value.
    fn volatility_shift_panel() -> Dataset {
        let bars = 360;
        let mut state: u64 = 0x5EED_2026_0916;
        let mut price = 100.0;
        let mut closes = Vec::with_capacity(bars);
        for t in 0..bars {
            state = state
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            let unit = (state >> 11) as f64 / (1u64 << 53) as f64;
            let sigma = if (t / 60) % 2 == 0 { 0.004 } else { 0.03 };
            price *= 1.0 + sigma * (unit - 0.5) * 2.0 * 3f64.sqrt();
            closes.push(price);
        }
        let mut series = BTreeMap::new();
        series.insert("AAA".to_string(), closes);
        Dataset {
            dates: (0..bars).map(|t| format!("t{t:03}")).collect(),
            closes: series,
            dividends: BTreeMap::new(),
        }
    }

    fn capture(data: &Dataset, agent: &mut dyn Agent, seeds: &[u64]) -> AgentTrajectory {
        let runs = seeds
            .iter()
            .map(|&seed| {
                let window = Window {
                    start: 20,
                    end: data.len(),
                };
                run_backtest_capture(data, agent, window, seed, CostModel::default()).1
            })
            .collect();
        AgentTrajectory {
            agent_id: "entrant".to_string(),
            contract: None,
            in_sample_trials: 0,
            declared_mandate: None,
            runs,
        }
    }

    fn report(data: &Dataset, traj: &AgentTrajectory) -> SizingResponse {
        sizing_response(
            data,
            traj,
            CostModel::default(),
            &SizingResponseConfig::default(),
        )
        .unwrap()
    }

    #[test]
    fn a_volatility_targeting_agent_sizes_down_and_correlates_negatively() {
        let data = volatility_shift_panel();
        let traj = capture(&data, &mut vol_targeting(), &[1, 2]);
        let report = report(&data, &traj);
        let rho = report.rank_correlation.spearman_rho.unwrap();
        assert!(rho < -0.5, "rho = {rho}");
        assert_eq!(report.rank_correlation.pairs, 2 * (360 - 20));
        assert!(!report.used_by_gate);
        let medians: Vec<f64> = report
            .by_volatility
            .quintiles
            .iter()
            .map(|q| q.median_gross_exposure.unwrap())
            .collect();
        assert!(
            medians[0] > 2.0 * medians[4],
            "calm quintile must hold more than twice the turbulent one: {medians:?}"
        );
        let counted: usize = report.by_volatility.quintiles.iter().map(|q| q.pairs).sum();
        assert_eq!(counted, report.census.pairs);
    }

    #[test]
    fn an_agent_that_sizes_up_in_turbulence_correlates_positively() {
        let data = volatility_shift_panel();
        let traj = capture(&data, &mut vol_chasing(), &[1]);
        let report = report(&data, &traj);
        let rho = report.rank_correlation.spearman_rho.unwrap();
        assert!(rho > 0.5, "rho = {rho}");
        let quintiles = &report.by_volatility.quintiles;
        assert!(
            quintiles[4].median_gross_exposure.unwrap()
                > 2.0 * quintiles[0].median_gross_exposure.unwrap(),
            "{quintiles:?}"
        );
    }

    #[test]
    fn buy_and_hold_reports_constant_exposure_not_a_correlation() {
        let data = volatility_shift_panel();
        let traj = capture(&data, &mut BuyAndHold, &[1, 2]);
        let report = report(&data, &traj);
        assert_eq!(report.rank_correlation.spearman_rho, None);
        assert_eq!(report.rank_correlation.pairs, 2 * (360 - 20));
        let Some(SizingUnavailable::ConstantExposure {
            min_gross_exposure,
            max_gross_exposure,
        }) = report.rank_correlation.unavailable
        else {
            panic!("{:?}", report.rank_correlation);
        };
        // The engine skips rebalances below 1e-9 of NAV, so the held weight
        // drifts from 1 by no more than that between fills.
        assert!((min_gross_exposure - 1.0).abs() < 1e-9);
        assert!((max_gross_exposure - 1.0).abs() < 1e-9);
        // The median table still says, truthfully, that exposure was flat.
        for q in &report.by_volatility.quintiles {
            assert!((q.median_gross_exposure.unwrap() - 1.0).abs() < 1e-9);
        }
    }

    #[test]
    fn a_flat_agent_reports_constant_zero_exposure() {
        let data = volatility_shift_panel();
        let traj = capture(&data, &mut HoldAgent, &[1]);
        assert_eq!(
            report(&data, &traj).rank_correlation.unavailable,
            Some(SizingUnavailable::ConstantExposure {
                min_gross_exposure: 0.0,
                max_gross_exposure: 0.0,
            })
        );
    }

    #[test]
    fn too_few_pairs_and_constant_volatility_are_typed() {
        let data = volatility_shift_panel();
        let mut traj = capture(&data, &mut vol_targeting(), &[1]);
        let short = SizingResponseConfig {
            min_pairs: 400,
            ..SizingResponseConfig::default()
        };
        let r = sizing_response(&data, &traj, CostModel::default(), &short).unwrap();
        let reason = SizingUnavailable::TooFewPairs {
            pairs: 340,
            min_pairs: 400,
        };
        assert_eq!(r.rank_correlation.unavailable, Some(reason));
        assert_eq!(r.by_volatility.unavailable, Some(reason));
        assert!(r.by_volatility.quintiles.is_empty());

        // A whipsaw's returns alternate between two values: one volatility.
        let whipsaw = Dataset::whipsaw(1, 120, 0.02, 0);
        traj = capture(&whipsaw, &mut Momentum::default(), &[1]);
        let r = report(&whipsaw, &traj);
        assert!(matches!(
            r.by_volatility.unavailable,
            Some(SizingUnavailable::ConstantVolatility { .. })
        ));
        assert!(r.rank_correlation.spearman_rho.is_none());
    }

    #[test]
    fn trailing_volatility_never_reads_a_later_close() {
        let data = volatility_shift_panel();
        let traj = capture(&data, &mut vol_targeting(), &[3]);
        let cfg = SizingResponseConfig::default();
        let cut = 150;
        // Same history through `cut`, a violent break on the bar after it.
        let mut later = data.clone();
        for close in later.closes.get_mut("AAA").unwrap()[cut + 1..].iter_mut() {
            *close *= 3.0;
        }
        let before = exposure_volatility_pairs(&data, &traj, CostModel::default(), &cfg)
            .unwrap()
            .0;
        let after = exposure_volatility_pairs(&later, &traj, CostModel::default(), &cfg)
            .unwrap()
            .0;
        let upto = |pairs: &[ExposurePair]| -> Vec<ExposurePair> {
            pairs.iter().copied().filter(|p| p.bar <= cut).collect()
        };
        assert_eq!(upto(&before).last().unwrap().bar, cut);
        assert_eq!(upto(&before), upto(&after));
        // The break does reach the first bar whose window contains it.
        let first_after = |pairs: &[ExposurePair]| {
            pairs
                .iter()
                .find(|p| p.bar == cut + 1)
                .unwrap()
                .trailing_volatility
        };
        assert!(first_after(&after) > 10.0 * first_after(&before));
    }

    #[test]
    fn the_first_pair_waits_for_a_full_window() {
        let data = volatility_shift_panel();
        let traj = capture(&data, &mut vol_targeting(), &[1]);
        let cfg = SizingResponseConfig {
            vol_lookback: 30,
            ..SizingResponseConfig::default()
        };
        let (pairs, census) =
            exposure_volatility_pairs(&data, &traj, CostModel::default(), &cfg).unwrap();
        assert_eq!(pairs[0].bar, 30);
        assert_eq!(census.bars_replayed, 340);
        assert_eq!(census.bars_without_history, 10);
        assert_eq!(census.pairs, 330);
        let closes = data.history("AAA", 30, 31);
        assert_eq!(pairs[0].trailing_volatility, observed_volatility(&closes));
    }

    #[test]
    fn the_replayed_book_is_the_scored_book() {
        let data = Dataset::synthetic(3, 120, 11);
        let window = Window {
            start: 20,
            end: 120,
        };
        let costs = crate::costs::CostProfile::Realistic.resolve().costs;
        let (run, traj) = run_backtest_capture(&data, &mut Momentum::default(), window, 5, costs);
        let rewards: Vec<f64> = replay_book(&data, &traj, costs)
            .iter()
            .map(|s| s.reward)
            .collect();
        assert_eq!(rewards, run.returns);
        assert_eq!(rewards, replay_run(&data, &traj, costs).returns);
    }

    #[test]
    fn gross_exposure_counts_shorts_at_their_absolute_value() {
        let data =
            Dataset::from_csv("date,symbol,close\nt0,AAA,100\nt0,BBB,50\nt1,AAA,100\nt1,BBB,50\n")
                .unwrap();
        let decision = Decision {
            orders: vec![
                Order {
                    symbol: "AAA".into(),
                    action: Action::Buy,
                    target_weight: 0.75,
                    confidence: 0.5,
                    rationale: String::new(),
                },
                Order {
                    symbol: "BBB".into(),
                    action: Action::Sell,
                    target_weight: -0.5,
                    confidence: 0.5,
                    rationale: String::new(),
                },
            ],
            reasoning: String::new(),
            cost: None,
        };
        let free = CostModel {
            fee_bps: 0.0,
            slippage_bps: 0.0,
            impact_bps: 0.0,
            financing_bps: 0.0,
            ..CostModel::default()
        };
        let run = RunTrajectory {
            window_start: 0,
            window_end: 1,
            seed: 0,
            steps: vec![sharpebench_protocol::DecisionStep {
                step: 0,
                observation_id: "t0".into(),
                decision,
            }],
        };
        let state = &replay_book(&data, &run, free)[0];
        assert_eq!(state.nav_before, 1.0);
        assert!((state.gross_value - 1.25).abs() < 1e-12);
    }

    #[test]
    fn exposure_is_measured_against_the_nav_the_target_was_sized_on() {
        // The opening buy pays about 55 bp of fees, slippage and impact.
        // Against the post-fill NAV that bar would read about 1.0055x; against
        // the sizing base it reads the 1x the agent asked for.
        let data = volatility_shift_panel();
        let traj = capture(&data, &mut BuyAndHold, &[1]);
        let states = replay_book(&data, &traj.runs[0], CostModel::default());
        assert!(states[0].reward < -0.005, "{}", states[0].reward);
        assert!((states[0].gross_value / states[0].nav_before - 1.0).abs() < 1e-12);
        assert!(states[0].gross_value / (1.0 + states[0].reward) > 1.005);
    }

    #[test]
    fn a_short_or_misaligned_trajectory_is_refused() {
        let data = volatility_shift_panel();
        let mut traj = capture(&data, &mut vol_targeting(), &[1]);
        let cfg = SizingResponseConfig::default();
        traj.runs[0].steps.pop();
        assert_eq!(
            sizing_response(&data, &traj, CostModel::default(), &cfg),
            Err(SizingResponseError::IncompleteRun {
                run: 0,
                recorded: 339,
                required: 340,
            })
        );
        let mut shorter = data.clone();
        shorter.dates.truncate(300);
        for series in shorter.closes.values_mut() {
            series.truncate(300);
        }
        let traj = capture(&data, &mut vol_targeting(), &[1]);
        assert_eq!(
            sizing_response(&shorter, &traj, CostModel::default(), &cfg),
            Err(SizingResponseError::WindowOutsideData {
                run: 0,
                window_end: 360,
                dataset_len: 300,
            })
        );
    }

    #[test]
    fn out_of_domain_parameters_are_refused() {
        let data = volatility_shift_panel();
        let traj = capture(&data, &mut HoldAgent, &[1]);
        for cfg in [
            SizingResponseConfig {
                vol_lookback: 1,
                ..SizingResponseConfig::default()
            },
            SizingResponseConfig {
                exposure_resolution: 0.0,
                ..SizingResponseConfig::default()
            },
            SizingResponseConfig {
                exposure_resolution: f64::NAN,
                ..SizingResponseConfig::default()
            },
            SizingResponseConfig {
                min_pairs: 4,
                ..SizingResponseConfig::default()
            },
        ] {
            assert!(matches!(
                sizing_response(&data, &traj, CostModel::default(), &cfg),
                Err(SizingResponseError::InvalidConfig(_))
            ));
        }
    }

    #[test]
    fn tied_volatilities_share_a_quintile() {
        let pair = |bar: usize, vol: f64| ExposurePair {
            run: 0,
            bar,
            gross_exposure: bar as f64 / 10.0,
            trailing_volatility: vol,
        };
        // Ten pairs, four of them tied at the second-lowest volatility.
        let vols = [0.1, 0.2, 0.2, 0.2, 0.2, 0.3, 0.4, 0.5, 0.6, 0.7];
        let pairs: Vec<ExposurePair> = vols.iter().enumerate().map(|(i, &v)| pair(i, v)).collect();
        let cfg = SizingResponseConfig {
            min_pairs: 10,
            ..SizingResponseConfig::default()
        };
        let table = quintile_medians(&pairs, &cfg);
        let counts: Vec<usize> = table.quintiles.iter().map(|q| q.pairs).collect();
        // Midranks 1, 3.5 x4, 6, 7, 8, 9, 10 over n = 10.
        assert_eq!(counts, [1, 4, 1, 2, 2]);
        assert_eq!(table.quintiles[1].median_volatility, Some(0.2));
        assert_eq!(table.quintiles[1].median_gross_exposure, Some(0.25));
        assert_eq!(table.quintiles[0].median_gross_exposure, Some(0.0));
    }

    #[test]
    fn the_report_serializes_reasons_by_name() {
        let reason = SizingUnavailable::ConstantExposure {
            min_gross_exposure: 1.0,
            max_gross_exposure: 1.0,
        };
        assert_eq!(
            serde_json::to_value(reason).unwrap(),
            serde_json::json!({
                "reason": "constant_exposure",
                "min_gross_exposure": 1.0,
                "max_gross_exposure": 1.0,
            })
        );
        assert_eq!(
            serde_json::to_value(SizingUnavailable::TooFewPairs {
                pairs: 3,
                min_pairs: 30,
            })
            .unwrap(),
            serde_json::json!({"reason": "too_few_pairs", "pairs": 3, "min_pairs": 30})
        );
    }
}
