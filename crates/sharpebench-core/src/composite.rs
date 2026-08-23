//! The composite score + leaderboard ranking — where the gates compose.
//!
//! An agent ranks **only if** every gate holds:
//! 1. its pooled Deflated Sharpe clears `dsr_bar` (survives multiple-testing),
//! 2. it passes the per-run bar on *every* seed×window (`pass^k`, mode All),
//! 3. it has zero block-severity process violations in any run,
//! 4. its bootstrap p-value beats `alpha` (the edge isn't noise).
//!
//! Raw mean return is recorded but is **never** the rank key — that is the whole
//! point of SharpeBench. Run the included synthetic agents (see tests) to watch a
//! lucky agent with a higher raw return get demoted below a skilled one.

use serde::{Deserialize, Serialize};

use crate::calibration::brier_score;
use crate::comparison_sets::{comparison_set, restrict_to_shared, TaggedRun, TaggedSubmission};
use crate::decay::edge_half_life;
use crate::deflated_sharpe::{deflated_sharpe_ratio, probabilistic_sharpe_ratio, sharpe_ratio};
use crate::pass_k::{pass_k, PassMode};
use crate::percentile::percentile_of;
use crate::process::{process_score, ProcessEvent, Trace};
use crate::rolling::rolling_sharpe;
use crate::selection::{selection_robustness, SelectionRobustness};
use crate::significance::bootstrap_pvalue;
use crate::stats::{mean, std_dev};

/// One seed×window run of an agent: its per-period returns plus the decision
/// trace and (optionally) per-decision confidences/outcomes.
///
/// A submission's runs are laid out **window-major** (all seeds of window 0, then
/// window 1, …), the order `sharpebench-harness` produces for every agent in a
/// sweep. Position `i` is therefore the same (window, seed) cell for every agent
/// in a field, which is what lets [`rank`] compare agents on their shared cells.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Run {
    pub returns: Vec<f64>,
    #[serde(default)]
    pub trace: Trace,
    #[serde(default)]
    pub confidences: Vec<f64>,
    #[serde(default)]
    pub outcomes: Vec<bool>,
    /// Compute/token cost incurred to produce this run (any consistent unit).
    /// Used for cost-efficiency reporting; 0.0 = not reported.
    #[serde(default)]
    pub cost: f64,
}

/// An agent's full submission: many runs across seeds × windows.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct AgentSubmission {
    pub agent_id: String,
    pub runs: Vec<Run>,
    /// Number of in-sample backtests/configs the agent searched before submitting.
    /// Folded into the deflation trial footprint so over-searching faces a higher
    /// bar — records data-snooping up front. 0 = undeclared.
    #[serde(default)]
    pub in_sample_trials: u32,
    /// Optional alternative candidate strategies the agent considered, each a
    /// pooled return stream. Used for selection-robustness reporting (best vs
    /// median candidate). Empty = not reported.
    #[serde(default)]
    pub candidates: Vec<Vec<f64>>,
}

/// What to rank eligible agents by.
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum RankKey {
    /// Deflated Sharpe (the default — luck-robust risk-adjusted skill).
    #[default]
    DeflatedSharpe,
    /// Alpha (skill net of market beta).
    Alpha,
}

/// A trading mandate: constraints the agent must respect to be rank-eligible.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Mandate {
    /// Max tolerable drawdown over the pooled track (e.g. 0.20). 1.0 = unconstrained.
    pub max_drawdown: f64,
}

impl Default for Mandate {
    fn default() -> Self {
        Self { max_drawdown: 1.0 }
    }
}

/// Maximum drawdown of the equity curve implied by a return series, in [0, 1].
fn max_drawdown(returns: &[f64]) -> f64 {
    let mut nav = 1.0;
    let mut peak = 1.0;
    let mut mdd = 0.0;
    for &r in returns {
        nav *= 1.0 + r;
        if nav > peak {
            peak = nav;
        }
        if peak > 0.0 {
            let dd = 1.0 - nav / peak;
            if dd > mdd {
                mdd = dd;
            }
        }
    }
    mdd
}

/// Scoring configuration. `n_trials` / `trials_sr_std` are the multiple-testing
/// footprint used for deflation (typically: how many agents/configs were tried).
///
/// Units. The kernel computes every Sharpe ratio **per period** (see
/// `sharpebench-stats`, which refuses to pre-annualize). The thresholds an
/// operator reasons about, however, are quoted **annualized**, because that is
/// the unit the literature and every track record use. `periods_per_year` is
/// the bridge: the annualized inputs (`trials_sr_std`,
/// `per_run_min_annual_sharpe`) are converted to per-period exactly once, at the
/// point of use, through [`per_period_sr_std`] and [`per_run_psr_benchmark`].
/// Nothing else in the kernel knows what a period is.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ScoreConfig {
    pub n_trials: u32,
    /// **Annualized** cross-trial dispersion of Sharpe ratios: the `sqrt(V[SR])`
    /// term in Bailey & López de Prado's expected-maximum Sharpe, which sets how
    /// far the bar rises with `n_trials`. It materially decides who is
    /// rank-eligible.
    ///
    /// The default 0.5 is a **modelling prior, not a measurement**: it is the
    /// working value López de Prado uses in worked examples, adopted before any
    /// field existed to measure it on. That worked example is in annualized
    /// units, and so is this field; the kernel divides it by
    /// `sqrt(periods_per_year)` before it touches a per-period statistic (see
    /// [`per_period_sr_std`]). Applied per period unconverted, 0.5 made the
    /// deflation benchmark an annualized Sharpe of 18 on daily bars and 106 on
    /// hourly bars, which no agent (and no market) has ever cleared. It means
    /// "the best of `n_trials` lucky strategies looks like an annualized Sharpe
    /// of about 1.14 at fifty trials", which is a high bar, not an unreachable
    /// one.
    ///
    /// [`rank`] prefers the *measured* path, the sample standard deviation of
    /// per-period Sharpe ratios across the submitted field, which is exactly the
    /// quantity the formula asks for and is already per-period, so it is never
    /// converted. It falls back to this configured value only when the field is
    /// too small to estimate it (see `min_field_for_measured_sr_std`).
    /// [`score_agent`] has no field and always uses this value. Whichever applied
    /// is stamped on every [`CompositeScore`] as `trials_sr_std` (per period, as
    /// used), `trials_sr_std_annualized` and `trials_sr_std_source`.
    pub trials_sr_std: f64,
    /// Deflated-Sharpe bar an agent must clear to be rank-eligible (e.g. 0.95).
    pub dsr_bar: f64,
    /// Per-run PSR bar each individual run must clear for pass^k: the confidence
    /// with which the run's true Sharpe must exceed `per_run_min_annual_sharpe`.
    ///
    /// PSR scales with `sqrt(n - 1)` of the run's length, so on short windows this
    /// gate is necessarily weak for any bar: a 78-bar window needs an annualized
    /// Sharpe of about 2.3 to reach 0.90 against zero, a 250-bar window about 1.3.
    /// That is a property of the statistic (short tracks carry little evidence),
    /// not a unit error, and it is why pass^k fails whenever any out-of-sample
    /// window is short. Either score longer windows or lower this bar knowingly.
    pub per_run_psr_bar: f64,
    /// **Annualized** Sharpe each run's true Sharpe must exceed with
    /// `per_run_psr_bar` confidence for pass^k. Converted to per period through
    /// [`per_run_psr_benchmark`]. The default 0.0 is the no-edge null, under which
    /// the per-run test is exactly `PSR(returns, 0) >= per_run_psr_bar`; an
    /// operator who wants "beats an annualized 0.5 on every run" sets 0.5 here,
    /// in units that mean the same thing on every timeframe.
    #[serde(default)]
    pub per_run_min_annual_sharpe: f64,
    /// How many return periods make a year on the dataset being scored: the unit
    /// conversion between the annualized thresholds above and the per-period
    /// statistics the kernel computes. Daily equities 252, daily crypto 365,
    /// 4-hour bars 2190, hourly bars 8760, weekly bars 52.
    ///
    /// Getting this wrong is the single most consequential misconfiguration in
    /// the benchmark: the deflation bar scales with `1 / sqrt(periods_per_year)`,
    /// so scoring hourly crypto with the daily default makes the bar about six
    /// times too demanding, and scoring weekly bars with it makes it about half as
    /// demanding as intended. The default is 252 (daily equities), which is what
    /// every score before this field existed silently assumed. The CLI prints the
    /// value it used in every run header for the same reason.
    #[serde(default = "default_periods_per_year")]
    pub periods_per_year: f64,
    /// Significance level for the bootstrap edge test.
    pub alpha: f64,
    pub bootstrap_seed: u64,
    pub n_boot: usize,
    pub block_prob: f64,
    /// Mandate constraints the agent must respect (default: unconstrained).
    #[serde(default)]
    pub mandate: Mandate,
    /// What eligible agents are ranked by (default: deflated Sharpe).
    #[serde(default)]
    pub rank_key: RankKey,
    /// Frozen reference population of Deflated-Sharpe values (e.g. real fund or
    /// human track records) for percentile reporting. Empty = no percentile.
    #[serde(default)]
    pub reference_dsr_population: Vec<f64>,
    /// Window length (in periods) for the rolling-Sharpe stability report over the
    /// pooled track — worst-window Sharpe + fraction-of-positive-windows.
    #[serde(default = "default_rolling_window")]
    pub rolling_window: usize,
    /// Two-sided coverage of the bootstrapped Deflated-Sharpe confidence interval
    /// (e.g. 0.90 → 5th/95th percentiles). Drives the leaderboard tie band: entries
    /// whose DSR CIs overlap are flagged statistically indistinguishable.
    #[serde(default = "default_dsr_ci_level")]
    pub dsr_ci_level: f64,
    /// Compare agents only on the (window × seed) cells **every** agent in the
    /// field completed (default: on). Runs carry no window ids, so [`rank`] keys
    /// each run by its position in the window-major layout (see [`Run`]) and
    /// restricts every submission to the positions all agents completed — the
    /// [`crate::comparison_sets`] intersection, applied by default instead of
    /// left to the caller. For a field where every agent completed the same cells
    /// this is the identity. Agents with no runs at all are scored as-is
    /// (ineligible by construction) and do not define the shared set, so one
    /// empty submission cannot blank the board. Off only reproduces the old
    /// unrestricted behaviour, where an agent scored on an easy subset of cells
    /// could outrank one scored on all of them.
    #[serde(default = "default_shared_run_set")]
    pub shared_run_set: bool,
    /// Minimum number of agents with a finite pooled Sharpe before [`rank`]
    /// *measures* `trials_sr_std` from the field instead of using the configured
    /// value. The relative standard error of a sample standard deviation is about
    /// `1 / sqrt(2 (n - 1))`: 50% at three agents, 35% at five. Below five the
    /// estimate is noisier than the prior it would replace, so five is the floor.
    #[serde(default = "default_min_field_for_measured_sr_std")]
    pub min_field_for_measured_sr_std: usize,
}

/// Default rolling-Sharpe window length (21 periods ≈ one trading month).
fn default_rolling_window() -> usize {
    21
}

/// Default DSR confidence-interval coverage (a 90% two-sided interval).
fn default_dsr_ci_level() -> f64 {
    0.90
}

/// Shared-cell comparison is on by default: fairness is a property of the board,
/// not an opt-in.
fn default_shared_run_set() -> bool {
    true
}

/// Default minimum field size for measuring `trials_sr_std` (see the field doc).
fn default_min_field_for_measured_sr_std() -> usize {
    5
}

/// Default periods per year: daily equity bars, the benchmark's historical
/// assumption. Configs serialized before the field existed deserialize to this,
/// so their meaning is unchanged.
fn default_periods_per_year() -> f64 {
    252.0
}

/// The per-period cross-trial Sharpe dispersion the kernel deflates with on the
/// configured path: `cfg.trials_sr_std / sqrt(cfg.periods_per_year)`.
///
/// A Sharpe ratio scales with the square root of the number of periods, so a
/// dispersion of Sharpes does too; dividing by `sqrt(periods_per_year)` takes
/// the annualized prior to the frequency the statistic is computed at. This is
/// the only place that conversion happens. Every deflation call site in this
/// module reads it from here so the prior can neither be converted twice nor
/// reach a per-period statistic unconverted. The *measured* path in [`rank`]
/// never calls it: the dispersion it measures across the field is already a
/// dispersion of per-period Sharpes.
pub fn per_period_sr_std(cfg: &ScoreConfig) -> f64 {
    cfg.trials_sr_std / cfg.periods_per_year.sqrt()
}

/// The per-period Sharpe benchmark each run's PSR is tested against for
/// pass^k: `cfg.per_run_min_annual_sharpe / sqrt(cfg.periods_per_year)`. The
/// default 0.0 converts to 0.0 on every timeframe, so the default per-run test
/// is the plain `PSR(returns, 0) >= per_run_psr_bar`.
pub fn per_run_psr_benchmark(cfg: &ScoreConfig) -> f64 {
    cfg.per_run_min_annual_sharpe / cfg.periods_per_year.sqrt()
}

/// Where the `trials_sr_std` that deflated a score came from.
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TrialsSrStdSource {
    /// `ScoreConfig::trials_sr_std` — a configured prior (the default 0.5 is a
    /// modelling assumption, not a measurement).
    #[default]
    Configured,
    /// The sample standard deviation of pooled per-period Sharpe ratios across the
    /// ranked field — what the deflation formula actually asks for.
    Measured,
}

impl Default for ScoreConfig {
    fn default() -> Self {
        Self {
            n_trials: 50,
            trials_sr_std: 0.5,
            dsr_bar: 0.95,
            per_run_psr_bar: 0.90,
            per_run_min_annual_sharpe: 0.0,
            periods_per_year: default_periods_per_year(),
            alpha: 0.05,
            bootstrap_seed: 0x5BA7_2026,
            n_boot: 2000,
            block_prob: 0.1,
            mandate: Mandate::default(),
            rank_key: RankKey::default(),
            reference_dsr_population: Vec::new(),
            rolling_window: default_rolling_window(),
            dsr_ci_level: default_dsr_ci_level(),
            shared_run_set: default_shared_run_set(),
            min_field_for_measured_sr_std: default_min_field_for_measured_sr_std(),
        }
    }
}

impl ScoreConfig {
    /// The default configuration for a dataset with `periods_per_year` bars per
    /// year. Prefer this to `Default::default()` whenever the data is not daily
    /// equities: it is the one field that has to match the dataset, and naming it
    /// at construction is harder to forget than patching it afterwards.
    pub fn for_periods_per_year(periods_per_year: f64) -> Self {
        Self {
            periods_per_year,
            ..Self::default()
        }
    }
}

/// The scored result for one agent.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CompositeScore {
    pub agent_id: String,
    pub deflated_sharpe: f64,
    pub psr: f64,
    pub passed_k: bool,
    pub process_ok: bool,
    pub bootstrap_p: f64,
    pub raw_mean_return: f64,
    pub rank_eligible: bool,
    /// The ranking key: the deflated Sharpe when eligible, else 0.0.
    pub composite: f64,
    /// Field-relative attribution, filled by [`rank`]: the skill (alpha) and
    /// market-beta components of the agent's return. Zero from `score_agent` alone.
    pub alpha: f64,
    pub beta: f64,
    /// Calibration of stated confidence (Brier score; lower = better). `None` if
    /// the agent reported no confidences/outcomes.
    pub calibration_brier: Option<f64>,
    /// Edge durability: half-life (in runs) of the per-run edge. `None` if there
    /// are too few runs or the edge isn't decaying.
    pub edge_half_life: Option<f64>,
    /// Field-wide data-snooping p-value (White's Reality Check), filled by [`rank`]:
    /// the probability the *leader's* edge is luck given how many agents were tried.
    /// Same value across the field. 1.0 from `score_agent` alone.
    pub field_reality_check_p: f64,
    /// Maximum drawdown over the pooled track, in [0, 1].
    pub max_drawdown: f64,
    /// Whether the agent respected its mandate (e.g. the drawdown cap).
    pub mandate_ok: bool,
    /// Turnover proxy: average orders placed per run (trading frequency / capacity).
    pub turnover: f64,
    /// Whether the agent is on the Pareto front over (return↑, drawdown↓,
    /// turnover↓). Filled by [`rank`].
    pub pareto_optimal: bool,
    /// Whether the agent's outperformance survives Romano–Wolf step-down multiple
    /// testing across the field. Filled by [`rank`].
    pub step_down_significant: bool,
    /// Conviction-weighted return: each run's return weighted by the confidence the
    /// agent staked on it. Rewards sizing conviction with the outcome. Falls back to
    /// the raw mean when no confidences are reported.
    pub confidence_weighted_return: f64,
    /// Total compute/token cost across all runs (0.0 if unreported).
    pub cost: f64,
    /// Raw mean return per unit cost — skill-per-dollar. `None` when cost is unreported.
    pub return_per_cost: Option<f64>,
    /// Hansen's studentized SPA p-value for the field leader (a more robust
    /// sibling of `field_reality_check_p`). Same value across the field; filled by
    /// [`rank`]. 1.0 from `score_agent` alone.
    pub field_spa_p: f64,
    /// Hansen's *consistent* SPA p-value — the most powerful of the field-wide
    /// data-snooping tests (drops clearly-bad models from the null). Same value
    /// across the field; filled by [`rank`]. 1.0 from `score_agent` alone.
    pub field_spa_consistent_p: f64,
    /// Crowdedness: the agent's mean Pearson correlation with the rest of the
    /// field's return streams, in [-1, 1]. High = riding the same factor as
    /// everyone else (a common beta that decays for the whole board at once);
    /// low/negative = diversifying. Reported, not gating; filled by [`rank`].
    /// `None` from `score_agent` alone (no field context) or with < 2 agents.
    pub field_crowdedness: Option<f64>,
    /// In-sample search budget the agent declared (configs tried before submission).
    pub in_sample_trials: u32,
    /// Effective deflation trial footprint = `cfg.n_trials + in_sample_trials`; the
    /// Deflated Sharpe is computed against this, so over-searching raises the bar.
    pub effective_n_trials: u32,
    /// Percentile (0..=100) of the Deflated Sharpe within the frozen reference
    /// population. `None` when no reference population is configured.
    pub dsr_percentile: Option<f64>,
    /// Deflated Sharpe of the median submitted candidate. `None` if none reported.
    pub selection_median_dsr: Option<f64>,
    /// Best-minus-median candidate Deflated Sharpe — the selection-luck gap.
    /// `None` if no candidates were reported.
    pub selection_gap: Option<f64>,
    /// 1-based ordinal position among rank-eligible agents (scale-invariant rank
    /// mode). 0 = ineligible or scored outside a field. Filled by [`rank`].
    pub rank_ordinal: usize,
    /// Worst (minimum) per-window Sharpe over the pooled track (non-annualized),
    /// using `cfg.rolling_window`. Low/negative = the edge collapses in some
    /// stretch. `None` when the pooled track is shorter than one window.
    pub rolling_min_sharpe: Option<f64>,
    /// Fraction of rolling windows whose Sharpe is positive, in [0, 1]. Near 1 =
    /// the edge is everywhere; low = the deflated edge lives in a few lucky
    /// windows. `None` when the track is too short.
    pub rolling_frac_positive: Option<f64>,
    /// Sortino ratio over the pooled track (excess mean return per unit of
    /// *downside* deviation, MAR = 0): rewards an edge that doesn't arrive with
    /// downside churn. Reported, never the rank key. `None` with no downside.
    pub sortino: Option<f64>,
    /// Downside deviation (RMS of below-target returns) — the denominator of
    /// `sortino`, reported so the figure is legible.
    pub downside_deviation: f64,
    /// Budget-normalized Deflated Sharpe: `deflated_sharpe / cost` — luck-robust
    /// skill per unit of compute/token spend. `None` when cost is unreported.
    pub dsr_per_cost: Option<f64>,
    /// Whether the realized return was floored to a no-skill baseline because the
    /// agent has a block-severity process violation (cheating shouldn't pay).
    pub process_floored: bool,
    /// The agent's realized return after the process floor: its raw mean when the
    /// process is clean, else the no-skill baseline (0.0). Always reported
    /// alongside `raw_mean_return`, which keeps the un-floored value.
    pub realized_floored_return: f64,
    /// Lower bound of the bootstrapped Deflated-Sharpe confidence interval (at
    /// `cfg.dsr_ci_level`). The DSR point estimate is `deflated_sharpe`; this is
    /// how far it might sink under resampling noise.
    pub dsr_ci_low: f64,
    /// Upper bound of the bootstrapped Deflated-Sharpe confidence interval.
    pub dsr_ci_high: f64,
    /// Bootstrap standard error of the Deflated Sharpe (the CI's scale).
    pub dsr_se: f64,
    /// 1-based tie-band index among rank-eligible agents: entries whose DSR
    /// confidence intervals overlap share a band and are statistically
    /// indistinguishable, so they are not hard-ranked against each other. 0 for
    /// ineligible agents or an agent scored outside a field. Filled by [`rank`].
    pub tie_group: usize,
    /// Whether this entry shares its DSR tie band with at least one other eligible
    /// agent (i.e. its rank is not statistically separable from a neighbor).
    /// Filled by [`rank`].
    pub dsr_tied: bool,
    /// The **per-period** cross-trial Sharpe dispersion this score was actually
    /// deflated with: the value handed to the expected-maximum-Sharpe formula.
    /// On the configured path it is `ScoreConfig::trials_sr_std` divided by
    /// `sqrt(periods_per_year)`; on the measured path it is the field's measured
    /// dispersion, unconverted.
    #[serde(default)]
    pub trials_sr_std: f64,
    /// The annualized prior `trials_sr_std` was converted from: `Some` of
    /// `ScoreConfig::trials_sr_std` on the configured path, `None` on the measured
    /// path, where the dispersion was measured per period and no annualized prior
    /// exists. Reported so a score is legible in the units operators quote
    /// without any reader having to redo the conversion.
    #[serde(default)]
    pub trials_sr_std_annualized: Option<f64>,
    /// Whether `trials_sr_std` was measured from the field or taken from the
    /// configured prior. Always `Configured` from `score_agent` alone.
    #[serde(default)]
    pub trials_sr_std_source: TrialsSrStdSource,
    /// Runs the agent submitted, before any shared-cell restriction.
    #[serde(default)]
    pub runs_submitted: usize,
    /// Runs actually scored: the submitted runs on the field's shared cells (see
    /// `ScoreConfig::shared_run_set`). Equals `runs_submitted` from `score_agent`
    /// alone or for a field where every agent completed the same cells.
    #[serde(default)]
    pub runs_scored: usize,
}

/// Pareto dominance on (return↑, drawdown↓, turnover↓).
fn dominates(a: &CompositeScore, b: &CompositeScore) -> bool {
    a.raw_mean_return >= b.raw_mean_return
        && a.max_drawdown <= b.max_drawdown
        && a.turnover <= b.turnover
        && (a.raw_mean_return > b.raw_mean_return
            || a.max_drawdown < b.max_drawdown
            || a.turnover < b.turnover)
}

/// The resolved per-period deflation dispersion a score is computed with, and
/// where it came from. Built exactly once per scoring call so the configured
/// prior is converted in one place ([`per_period_sr_std`]) and the measured
/// dispersion is never converted at all.
#[derive(Clone, Copy)]
struct Deflation {
    /// Per period, as handed to the expected-maximum-Sharpe formula.
    sr_std: f64,
    /// The annualized prior `sr_std` came from; `None` when it was measured.
    annualized: Option<f64>,
    source: TrialsSrStdSource,
}

impl Deflation {
    fn configured(cfg: &ScoreConfig) -> Self {
        Self {
            sr_std: per_period_sr_std(cfg),
            annualized: Some(cfg.trials_sr_std),
            source: TrialsSrStdSource::Configured,
        }
    }

    /// `sr_std` is the field's measured dispersion of per-period Sharpes: already
    /// in the kernel's units, so it must not pass through the conversion.
    fn measured(sr_std: f64) -> Self {
        Self {
            sr_std,
            annualized: None,
            source: TrialsSrStdSource::Measured,
        }
    }
}

/// Score a single agent submission against `cfg`. With no field to measure the
/// cross-trial dispersion on, the configured annualized prior applies, converted
/// to per period once.
pub fn score_agent(sub: &AgentSubmission, cfg: &ScoreConfig) -> CompositeScore {
    score_agent_with(sub, cfg, Deflation::configured(cfg))
}

fn score_agent_with(sub: &AgentSubmission, cfg: &ScoreConfig, defl: Deflation) -> CompositeScore {
    let pooled: Vec<f64> = sub
        .runs
        .iter()
        .flat_map(|r| r.returns.iter().copied())
        .collect();

    let psr = probabilistic_sharpe_ratio(&pooled, 0.0);
    // Fold the agent's declared in-sample search budget into the deflation trial
    // footprint: an agent that tried 5000 configs to find this strategy faces a
    // higher bar than one that tried none (front-end data-snooping control).
    let effective_n_trials = cfg.n_trials.saturating_add(sub.in_sample_trials);
    let dsr = deflated_sharpe_ratio(&pooled, effective_n_trials, defl.sr_std);

    // pass^k: every run must individually clear the per-run PSR bar against the
    // per-period benchmark the annualized minimum converts to (0 by default).
    let per_run_benchmark = per_run_psr_benchmark(cfg);
    let per_run: Vec<bool> = sub
        .runs
        .iter()
        .map(|r| probabilistic_sharpe_ratio(&r.returns, per_run_benchmark) >= cfg.per_run_psr_bar)
        .collect();
    let passed_k = pass_k(&per_run, PassMode::All);

    // process: a single block-severity violation in any run is disqualifying.
    let process_ok = sub.runs.iter().all(|r| process_score(&r.trace).is_clean());

    let bootstrap_p = bootstrap_pvalue(&pooled, cfg.bootstrap_seed, cfg.n_boot, cfg.block_prob);
    let raw_mean_return = mean(&pooled);

    // Calibration: does stated conviction predict outcomes? (None if not reported.)
    let conf: Vec<f64> = sub
        .runs
        .iter()
        .flat_map(|r| r.confidences.iter().copied())
        .collect();
    let outc: Vec<bool> = sub
        .runs
        .iter()
        .flat_map(|r| r.outcomes.iter().copied())
        .collect();
    let calibration_brier = if !conf.is_empty() && !outc.is_empty() {
        Some(brier_score(&conf, &outc))
    } else {
        None
    };

    // Edge durability: half-life of the per-run edge across runs.
    let per_run_edge: Vec<f64> = sub.runs.iter().map(|r| mean(&r.returns)).collect();
    let edge_half_life_periods = edge_half_life(&per_run_edge);

    // Mandate adherence: does the drawdown respect the mandate's cap?
    let mdd = max_drawdown(&pooled);
    let mandate_ok = mdd <= cfg.mandate.max_drawdown;

    // Turnover proxy: average number of orders placed per run.
    let total_orders: usize = sub
        .runs
        .iter()
        .map(|r| {
            r.trace
                .events
                .iter()
                .filter(|e| matches!(e, ProcessEvent::OrderPlaced { .. }))
                .count()
        })
        .sum();
    let turnover = total_orders as f64 / sub.runs.len().max(1) as f64;

    // Confidence-weighted return: weight each run's return by the conviction
    // staked on it, so sizing-with-conviction beats flat-confidence trading.
    let mut cw_num = 0.0;
    let mut cw_den = 0.0;
    for r in &sub.runs {
        let w = if r.confidences.is_empty() {
            1.0
        } else {
            mean(&r.confidences)
        };
        cw_num += w * mean(&r.returns);
        cw_den += w;
    }
    let confidence_weighted_return = if cw_den > 0.0 {
        cw_num / cw_den
    } else {
        raw_mean_return
    };

    // Cost-efficiency: skill per unit of compute/token spend.
    let cost: f64 = sub.runs.iter().map(|r| r.cost).sum();
    let return_per_cost = if cost > 0.0 {
        Some(raw_mean_return / cost)
    } else {
        None
    };

    // Legibility: percentile of the Deflated Sharpe within the frozen reference
    // population (e.g. real fund track records). None when unconfigured.
    let dsr_percentile = if cfg.reference_dsr_population.is_empty() {
        None
    } else {
        Some(percentile_of(dsr, &cfg.reference_dsr_population))
    };

    // Selection-axis luck: best vs median Deflated Sharpe of the agent's candidate
    // strategies, deflated against the same effective trial footprint. A large gap
    // means the headline result is a lucky pick, not a robust family of edges.
    let (selection_median_dsr, selection_gap) = if sub.candidates.is_empty() {
        (None, None)
    } else {
        let sr: SelectionRobustness =
            selection_robustness(&sub.candidates, effective_n_trials, defl.sr_std);
        (Some(sr.median_dsr), Some(sr.selection_gap))
    };

    // Rolling-Sharpe stability over the pooled track: is the deflated edge one
    // lucky window, or present across the whole track?
    let rolling = rolling_sharpe(&pooled, cfg.rolling_window);
    let rolling_min_sharpe = rolling.map(|r| r.min_sharpe);
    let rolling_frac_positive = rolling.map(|r| r.frac_positive);

    // Downside-risk view: the Sortino rewards an edge that doesn't arrive with
    // downside volatility (reported alongside the Sharpe family, never a gate).
    let sortino = crate::stats::sortino_ratio(&pooled, 0.0);
    let downside_deviation = crate::stats::downside_deviation(&pooled, 0.0);

    // Budget-normalized Deflated Sharpe: luck-robust skill per unit of spend.
    let dsr_per_cost = if cost > 0.0 { Some(dsr / cost) } else { None };

    // Process floor: a block-severity violation forfeits any realized return —
    // it is floored to the no-skill baseline (0.0) so cheating never pays, even
    // for the (display-only) realized-return column. Eligibility logic below is
    // unchanged; `process_ok` still independently disqualifies.
    let process_floored = !process_ok;
    let realized_floored_return = if process_floored {
        0.0
    } else {
        raw_mean_return
    };

    // Sampling uncertainty of the DSR point estimate: a bootstrapped CI + SE, so
    // the leaderboard can flag noise-separated entries as tied rather than impose a
    // false hard ordering. Reuses the stationary-bootstrap resampler.
    let dsr_ci = crate::significance::bootstrap_dsr_ci(
        &pooled,
        effective_n_trials,
        defl.sr_std,
        cfg.bootstrap_seed,
        cfg.n_boot,
        cfg.block_prob,
        cfg.dsr_ci_level,
    );

    let rank_eligible =
        dsr >= cfg.dsr_bar && passed_k && process_ok && bootstrap_p < cfg.alpha && mandate_ok;
    let composite = if rank_eligible { dsr } else { 0.0 };

    CompositeScore {
        agent_id: sub.agent_id.clone(),
        deflated_sharpe: dsr,
        psr,
        passed_k,
        process_ok,
        bootstrap_p,
        raw_mean_return,
        rank_eligible,
        composite,
        alpha: 0.0,
        beta: 0.0,
        calibration_brier,
        edge_half_life: edge_half_life_periods,
        field_reality_check_p: 1.0,
        max_drawdown: mdd,
        mandate_ok,
        turnover,
        pareto_optimal: false,
        step_down_significant: false,
        confidence_weighted_return,
        cost,
        return_per_cost,
        field_spa_p: 1.0,
        field_spa_consistent_p: 1.0,
        field_crowdedness: None,
        in_sample_trials: sub.in_sample_trials,
        effective_n_trials,
        dsr_percentile,
        selection_median_dsr,
        selection_gap,
        rank_ordinal: 0,
        rolling_min_sharpe,
        rolling_frac_positive,
        sortino,
        downside_deviation,
        dsr_per_cost,
        process_floored,
        realized_floored_return,
        dsr_ci_low: dsr_ci.lower,
        dsr_ci_high: dsr_ci.upper,
        dsr_se: dsr_ci.se,
        tie_group: 0,
        dsr_tied: false,
        trials_sr_std: defl.sr_std,
        trials_sr_std_annualized: defl.annualized,
        trials_sr_std_source: defl.source,
        runs_submitted: sub.runs.len(),
        runs_scored: sub.runs.len(),
    }
}

/// Restrict a field to the run positions every non-empty submission completed —
/// the [`crate::comparison_sets`] intersection keyed by window-major position.
/// Output order matches `subs`; empty submissions pass through untouched.
fn restrict_to_shared_positions(subs: &[AgentSubmission]) -> Vec<AgentSubmission> {
    // Zero-padded so the ids sort in position order; `restrict_to_shared` keeps
    // run order anyway, the padding only makes `shared_windows` legible.
    let tag = |i: usize| format!("{i:08}");
    let tagged: Vec<TaggedSubmission> = subs
        .iter()
        .filter(|s| !s.runs.is_empty())
        .map(|s| TaggedSubmission {
            agent_id: s.agent_id.clone(),
            runs: s
                .runs
                .iter()
                .enumerate()
                .map(|(i, run)| TaggedRun {
                    window_id: tag(i),
                    run: run.clone(),
                })
                .collect(),
            in_sample_trials: s.in_sample_trials,
            candidates: s.candidates.clone(),
        })
        .collect();
    let roster: Vec<String> = tagged.iter().map(|s| s.agent_id.clone()).collect();
    let set = comparison_set(&roster, &tagged);
    // `tagged` holds the non-empty submissions in `subs` order, so walking it in
    // lockstep (rather than looking up by id) is exact even if ids repeat.
    let mut tagged_iter = tagged.iter();
    subs.iter()
        .map(|s| {
            if s.runs.is_empty() {
                return s.clone();
            }
            let t = tagged_iter
                .next()
                .expect("one tagged submission per non-empty submission");
            restrict_to_shared(&set, t)
        })
        .collect()
}

/// The field-measured `trials_sr_std`: the sample standard deviation of pooled
/// per-period Sharpe ratios across agents with a finite Sharpe, or `None` when
/// fewer than `min_field` agents qualify. Sharpes are sorted before summing so
/// the submission order of the field cannot move the deflation bar by an ULP.
fn measured_trials_sr_std(pooled: &[Vec<f64>], min_field: usize) -> Option<f64> {
    let mut sharpes: Vec<f64> = pooled
        .iter()
        .filter(|p| p.len() >= 2)
        .map(|p| sharpe_ratio(p))
        .filter(|sr| sr.is_finite())
        .collect();
    if sharpes.len() < min_field.max(2) {
        return None;
    }
    sharpes.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    Some(std_dev(&sharpes))
}

/// Score and rank a field of agents. Eligible agents sort first (by composite
/// desc); ineligible agents sort last (by raw return desc, for display only).
///
/// Two field-level controls run before any agent is scored, so neither is left
/// for a caller to remember:
/// - **Shared cells** (`cfg.shared_run_set`, default on): every submission is
///   restricted to the run positions all agents completed, so an agent scored on
///   an easy subset of cells is compared on the same cells as everyone else.
/// - **Measured deflation** (`cfg.min_field_for_measured_sr_std`): with enough
///   agents, `trials_sr_std` is the measured Sharpe dispersion of the field rather
///   than the configured prior. Smaller fields use the configured value, byte for
///   byte as before. Which applied is stamped on every score.
///
/// ```
/// use sharpebench_core::{rank, AgentSubmission, Run, ScoreConfig, Trace};
///
/// let mk = |id: &str, returns: Vec<f64>, trials: u32| AgentSubmission {
///     agent_id: id.into(),
///     runs: vec![Run {
///         returns,
///         trace: Trace::default(),
///         confidences: vec![],
///         outcomes: vec![],
///         cost: 0.0,
///     }],
///     in_sample_trials: trials,
///     candidates: vec![],
/// };
///
/// // "lucky" posts a bigger raw return but searched 500 strategies to find it.
/// let board = rank(
///     &[
///         mk("skilled", vec![0.012, 0.008, 0.011, 0.009, 0.010], 1),
///         mk("lucky", vec![0.090, -0.02, 0.001, -0.03, 0.05], 500),
///     ],
///     &ScoreConfig::default(),
/// );
///
/// // One CompositeScore per agent; ranked by deflated Sharpe, not raw return.
/// assert_eq!(board.len(), 2);
/// ```
pub fn rank(subs: &[AgentSubmission], cfg: &ScoreConfig) -> Vec<CompositeScore> {
    // Shared-cell restriction first: everything below — attribution, the
    // data-snooping family, crowdedness and the scores themselves — must see the
    // same field, or the fairness control would apply to the rank key only.
    let restricted;
    let field: &[AgentSubmission] = if cfg.shared_run_set {
        restricted = restrict_to_shared_positions(subs);
        &restricted
    } else {
        subs
    };

    // Pooled returns per agent + an equal-weight market proxy (the field average),
    // used for performance attribution: alpha (skill) vs beta (market exposure).
    let pooled: Vec<Vec<f64>> = field
        .iter()
        .map(|s| {
            s.runs
                .iter()
                .flat_map(|r| r.returns.iter().copied())
                .collect()
        })
        .collect();
    let min_len = pooled.iter().map(Vec::len).min().unwrap_or(0);
    let n_agents = pooled.len().max(1) as f64;
    let market: Vec<f64> = (0..min_len)
        .map(|i| pooled.iter().map(|p| p[i]).sum::<f64>() / n_agents)
        .collect();

    // Measured deflation: with enough agents the field's own Sharpe dispersion
    // replaces the configured prior. The measured value is a dispersion of
    // per-period Sharpes, so it goes in as-is; only the configured prior is
    // annualized and needs converting. A small field scores exactly as
    // `score_agent` would, so the configured path stays byte-identical.
    let defl = measured_trials_sr_std(&pooled, cfg.min_field_for_measured_sr_std)
        .map_or_else(|| Deflation::configured(cfg), Deflation::measured);

    let mut scores: Vec<CompositeScore> = field
        .iter()
        .enumerate()
        .map(|(idx, s)| {
            let mut cs = score_agent_with(s, cfg, defl);
            cs.runs_submitted = subs[idx].runs.len();
            if min_len >= 2 {
                let (alpha, beta) = crate::attribution::alpha_beta(&pooled[idx], &market);
                cs.alpha = alpha;
                cs.beta = beta;
            }
            cs
        })
        .collect();

    // Field-wide data-snooping significance (White's Reality Check): is the
    // leader's edge real after accounting for how many agents were tried?
    if min_len >= 2 {
        let field_excess: Vec<Vec<f64>> = pooled
            .iter()
            .map(|p| {
                p.iter()
                    .take(min_len)
                    .zip(market.iter())
                    .map(|(a, m)| a - m)
                    .collect()
            })
            .collect();
        let rc_p = crate::significance::reality_check_pvalue(
            &field_excess,
            cfg.bootstrap_seed,
            cfg.n_boot,
            cfg.block_prob,
        );
        let spa_p = crate::significance::spa_pvalue(
            &field_excess,
            cfg.bootstrap_seed,
            cfg.n_boot,
            cfg.block_prob,
        );
        let spa_c_p = crate::significance::spa_consistent_pvalue(
            &field_excess,
            cfg.bootstrap_seed,
            cfg.n_boot,
            cfg.block_prob,
        );
        for cs in scores.iter_mut() {
            cs.field_reality_check_p = rc_p;
            cs.field_spa_p = spa_p;
            cs.field_spa_consistent_p = spa_c_p;
        }
        let sd = crate::significance::step_down_significant(
            &field_excess,
            cfg.bootstrap_seed,
            cfg.n_boot,
            cfg.block_prob,
            cfg.alpha,
        );
        for (cs, s) in scores.iter_mut().zip(sd) {
            cs.step_down_significant = s;
        }
    }

    // Crowdedness: how correlated is each agent's return stream with the rest of
    // the field? High = riding the same factor as everyone else (a common beta
    // that decays for the whole board at once); low/negative = diversifying skill.
    // Reported, not gating — the field-relative sibling of decay/calibration.
    if min_len >= 2 && pooled.len() >= 2 {
        let aligned: Vec<&[f64]> = pooled.iter().map(|p| &p[..min_len]).collect();
        for (idx, cs) in scores.iter_mut().enumerate() {
            let peers: Vec<&[f64]> = aligned
                .iter()
                .enumerate()
                .filter(|&(j, _)| j != idx)
                .map(|(_, &p)| p)
                .collect();
            cs.field_crowdedness = crate::correlation::crowdedness(aligned[idx], &peers).mean_corr;
        }
    }

    // Pareto front over (return↑, drawdown↓, turnover↓).
    let pareto: Vec<bool> = (0..scores.len())
        .map(|i| !(0..scores.len()).any(|j| j != i && dominates(&scores[j], &scores[i])))
        .collect();
    for (cs, p) in scores.iter_mut().zip(pareto) {
        cs.pareto_optimal = p;
    }

    let sort_key = |s: &CompositeScore| match cfg.rank_key {
        RankKey::DeflatedSharpe => s.composite,
        RankKey::Alpha => {
            if s.rank_eligible {
                s.alpha
            } else {
                f64::NEG_INFINITY
            }
        }
    };
    scores.sort_by(|a, b| {
        b.rank_eligible
            .cmp(&a.rank_eligible)
            .then(
                sort_key(b)
                    .partial_cmp(&sort_key(a))
                    .unwrap_or(std::cmp::Ordering::Equal),
            )
            .then(
                b.raw_mean_return
                    .partial_cmp(&a.raw_mean_return)
                    .unwrap_or(std::cmp::Ordering::Equal),
            )
    });

    // 1-based ordinal rank among eligible agents (the scale-invariant rank mode,
    // assigned in final sorted order). Ineligible agents keep ordinal 0.
    let mut ord = 0usize;
    for cs in scores.iter_mut() {
        if cs.rank_eligible {
            ord += 1;
            cs.rank_ordinal = ord;
        }
    }

    // DSR tie bands: walking the eligible agents in ranked order, an agent joins
    // the previous band when their DSR confidence intervals overlap (a difference
    // the resampling can't distinguish from noise). A new, separable agent opens a
    // new band. Then flag every agent whose band holds more than one member: its
    // rank is not statistically real. This is the whole point of the board: the
    // "return-rank is luck" failure must not reappear at the DSR.
    let mut group = 0usize;
    let mut prev: Option<usize> = None;
    for i in 0..scores.len() {
        if !scores[i].rank_eligible {
            continue;
        }
        let same_band = matches!(prev, Some(p) if ci_overlap(&scores[p], &scores[i]));
        if !same_band {
            group += 1;
        }
        scores[i].tie_group = group;
        prev = Some(i);
    }
    let mut band_counts = vec![0usize; group + 1];
    for cs in &scores {
        if cs.rank_eligible {
            band_counts[cs.tie_group] += 1;
        }
    }
    for cs in scores.iter_mut() {
        if cs.rank_eligible {
            cs.dsr_tied = band_counts[cs.tie_group] > 1;
        }
    }
    scores
}

/// Do two agents' Deflated-Sharpe confidence intervals overlap? Overlapping CIs
/// mean the difference in their DSR point estimates is within sampling noise, so
/// they belong in the same tie band rather than being hard-ranked.
fn ci_overlap(a: &CompositeScore, b: &CompositeScore) -> bool {
    a.dsr_ci_low <= b.dsr_ci_high && b.dsr_ci_low <= a.dsr_ci_high
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::deflated_sharpe::expected_max_sharpe;
    use crate::process::ProcessEvent;

    /// Deterministic run: mean drift + a sinusoidal wiggle (no RNG → reproducible).
    fn run(mean_ret: f64, amp: f64, n: usize) -> Run {
        let returns = (0..n)
            .map(|i| mean_ret + amp * (i as f64 * 0.7).sin())
            .collect();
        Run {
            returns,
            trace: Trace::default(),
            confidences: Vec::new(),
            outcomes: Vec::new(),
            cost: 0.0,
        }
    }

    fn agent(id: &str, runs: Vec<Run>) -> AgentSubmission {
        AgentSubmission {
            agent_id: id.to_string(),
            runs,
            in_sample_trials: 0,
            candidates: Vec::new(),
        }
    }

    #[test]
    fn skilled_is_eligible() {
        let s = score_agent(
            &agent("skilled", (0..5).map(|_| run(0.002, 0.0005, 60)).collect()),
            &ScoreConfig::default(),
        );
        assert!(s.rank_eligible, "skilled should be eligible: {s:?}");
        assert!(s.passed_k && s.process_ok);
    }

    #[test]
    fn lucky_high_return_fails_pass_k() {
        // One spectacular run, four noisy zero-mean runs → high raw return, but
        // it does not clear the bar on every run.
        let mut runs = vec![run(0.02, 0.002, 60)];
        runs.extend((0..4).map(|_| run(0.0, 0.003, 60)));
        let s = score_agent(&agent("lucky", runs), &ScoreConfig::default());
        assert!(!s.passed_k, "lucky should fail pass^k");
        assert!(!s.rank_eligible, "lucky must not be rank-eligible: {s:?}");
    }

    #[test]
    fn process_violator_is_disqualified() {
        let mut runs: Vec<Run> = (0..5).map(|_| run(0.002, 0.0005, 60)).collect();
        runs[0].trace.events.push(ProcessEvent::OrderPlaced {
            risk_gate_passed: false,
        });
        let s = score_agent(&agent("violator", runs), &ScoreConfig::default());
        assert!(!s.process_ok);
        assert!(!s.rank_eligible, "a risk-gate bypass must disqualify");
    }

    /// The headline property: a lucky agent with a *higher raw return* ranks
    /// BELOW a skilled agent, because it can't clear the luck-robust gates.
    #[test]
    fn deflation_demotes_luck() {
        let skilled = agent("skilled", (0..5).map(|_| run(0.002, 0.0005, 60)).collect());
        let lucky = {
            let mut runs = vec![run(0.02, 0.002, 60)];
            runs.extend((0..4).map(|_| run(0.0, 0.003, 60)));
            agent("lucky", runs)
        };
        let board = rank(&[lucky.clone(), skilled.clone()], &ScoreConfig::default());

        // Sanity: the lucky agent really does have the higher raw return.
        let lucky_raw = board
            .iter()
            .find(|s| s.agent_id == "lucky")
            .unwrap()
            .raw_mean_return;
        let skilled_raw = board
            .iter()
            .find(|s| s.agent_id == "skilled")
            .unwrap()
            .raw_mean_return;
        assert!(
            lucky_raw > skilled_raw,
            "lucky raw {lucky_raw} should exceed skilled {skilled_raw}"
        );

        // Yet the board ranks the skilled agent first.
        assert_eq!(board[0].agent_id, "skilled");
        assert!(board[0].rank_eligible && !board[1].rank_eligible);
    }

    #[test]
    fn confidence_weighting_rewards_conviction() {
        // Confident on the winning run, cautious on the losing one → the
        // conviction-weighted return beats the flat raw mean.
        let win = Run {
            returns: vec![0.01; 30],
            trace: Trace::default(),
            confidences: vec![0.9; 30],
            outcomes: Vec::new(),
            cost: 0.0,
        };
        let lose = Run {
            returns: vec![-0.005; 30],
            trace: Trace::default(),
            confidences: vec![0.1; 30],
            outcomes: Vec::new(),
            cost: 0.0,
        };
        let s = score_agent(&agent("conv", vec![win, lose]), &ScoreConfig::default());
        assert!(
            s.confidence_weighted_return > s.raw_mean_return,
            "cwr {} should beat raw {}",
            s.confidence_weighted_return,
            s.raw_mean_return
        );
    }

    #[test]
    fn cost_efficiency_reported_only_with_cost() {
        let mut r = run(0.002, 0.0005, 30);
        r.cost = 4.0;
        let s = score_agent(&agent("paid", vec![r]), &ScoreConfig::default());
        assert_eq!(s.cost, 4.0);
        assert!(s.return_per_cost.is_some());

        let free = score_agent(
            &agent("free", vec![run(0.002, 0.0005, 30)]),
            &ScoreConfig::default(),
        );
        assert!(free.return_per_cost.is_none());
    }

    #[test]
    fn in_sample_search_raises_the_deflation_bar() {
        let runs: Vec<Run> = (0..5).map(|_| run(0.002, 0.0005, 60)).collect();
        let base = score_agent(&agent("base", runs.clone()), &ScoreConfig::default());
        let mut over = agent("over", runs);
        over.in_sample_trials = 5000;
        let s = score_agent(&over, &ScoreConfig::default());
        assert_eq!(s.effective_n_trials, 5050);
        assert!(
            s.deflated_sharpe <= base.deflated_sharpe,
            "more in-sample search must not raise DSR ({} vs {})",
            s.deflated_sharpe,
            base.deflated_sharpe
        );
    }

    #[test]
    fn percentile_reported_only_with_reference() {
        let none = score_agent(
            &agent("p", (0..5).map(|_| run(0.002, 0.0005, 60)).collect()),
            &ScoreConfig::default(),
        );
        assert!(none.dsr_percentile.is_none());
        let cfg = ScoreConfig {
            reference_dsr_population: vec![0.0, 0.3, 0.6, 0.9],
            ..ScoreConfig::default()
        };
        let some = score_agent(
            &agent("p", (0..5).map(|_| run(0.002, 0.0005, 60)).collect()),
            &cfg,
        );
        assert!(some.dsr_percentile.is_some());
    }

    #[test]
    fn rolling_sharpe_reported_for_long_tracks() {
        let s = score_agent(
            &agent("roll", (0..5).map(|_| run(0.002, 0.0005, 60)).collect()),
            &ScoreConfig::default(),
        );
        // 300 pooled points ≥ 21-window → both reported, steady edge is all-positive.
        assert!(s.rolling_min_sharpe.is_some());
        let fp = s.rolling_frac_positive.expect("reported");
        assert!(
            (fp - 1.0).abs() < 1e-12,
            "steady edge → all windows positive"
        );
    }

    #[test]
    fn rolling_sharpe_none_when_track_too_short() {
        let cfg = ScoreConfig {
            rolling_window: 100,
            ..ScoreConfig::default()
        };
        let s = score_agent(&agent("short", vec![run(0.002, 0.0005, 30)]), &cfg);
        assert!(s.rolling_min_sharpe.is_none());
        assert!(s.rolling_frac_positive.is_none());
    }

    #[test]
    fn dsr_per_cost_reported_only_with_cost() {
        let mut r = run(0.002, 0.0005, 60);
        r.cost = 5.0;
        let paid = score_agent(&agent("paid", vec![r]), &ScoreConfig::default());
        let dpc = paid.dsr_per_cost.expect("reported with cost");
        assert!((dpc - paid.deflated_sharpe / 5.0).abs() < 1e-12);

        let free = score_agent(
            &agent("free", vec![run(0.002, 0.0005, 60)]),
            &ScoreConfig::default(),
        );
        assert!(free.dsr_per_cost.is_none());
    }

    #[test]
    fn process_violation_floors_realized_return() {
        let mut runs: Vec<Run> = (0..5).map(|_| run(0.02, 0.0005, 60)).collect();
        runs[0].trace.events.push(ProcessEvent::OrderPlaced {
            risk_gate_passed: false,
        });
        let s = score_agent(&agent("cheater", runs), &ScoreConfig::default());
        assert!(s.process_floored, "block violation must set the floor flag");
        assert_eq!(
            s.realized_floored_return, 0.0,
            "floored to no-skill baseline"
        );
        assert!(
            s.raw_mean_return > 0.0,
            "raw return is preserved un-floored"
        );
        assert!(!s.rank_eligible, "eligibility logic intact");
    }

    #[test]
    fn clean_process_is_not_floored() {
        let s = score_agent(
            &agent("clean", (0..5).map(|_| run(0.002, 0.0005, 60)).collect()),
            &ScoreConfig::default(),
        );
        assert!(!s.process_floored);
        assert_eq!(s.realized_floored_return, s.raw_mean_return);
    }

    #[test]
    fn overlapping_dsr_cis_flag_a_tie_and_separation_gets_a_distinct_band() {
        // A bar low enough to admit a clearly-weaker (but still real) agent, so we
        // can exhibit both an indistinguishable pair and a separable outsider.
        let cfg = ScoreConfig {
            n_trials: 2,
            trials_sr_std: 0.01,
            dsr_bar: 0.10,
            per_run_psr_bar: 0.05,
            alpha: 0.9,
            n_boot: 600,
            ..ScoreConfig::default()
        };
        // Two identical strong agents: same track ⇒ identical DSR CIs ⇒ tied.
        let strong_a = agent("strong_a", (0..3).map(|_| run(0.01, 0.001, 60)).collect());
        let strong_b = agent("strong_b", (0..3).map(|_| run(0.01, 0.001, 60)).collect());
        // A much weaker but still-eligible agent: its DSR CI sits well below.
        let weak = agent("weak", (0..3).map(|_| run(0.001, 0.02, 60)).collect());

        let board = rank(&[strong_a, strong_b, weak], &cfg);
        let get = |id: &str| board.iter().find(|s| s.agent_id == id).unwrap();
        let (a, b, w) = (get("strong_a"), get("strong_b"), get("weak"));

        assert!(
            a.rank_eligible && b.rank_eligible && w.rank_eligible,
            "all three should clear the (deliberately low) bar"
        );
        // Overlapping CIs ⇒ same tie band, both flagged indistinguishable.
        assert_eq!(a.tie_group, b.tie_group, "identical CIs share a band");
        assert!(a.dsr_tied && b.dsr_tied, "the pair is flagged tied");
        // The weaker agent's CI is separable ⇒ its own band, not tied.
        assert_ne!(
            w.tie_group, a.tie_group,
            "separable agent gets a distinct band"
        );
        assert!(!w.dsr_tied, "a distinct band is not a tie");
        assert!(
            w.dsr_ci_high < a.dsr_ci_low,
            "weak CI upper {} should sit below strong CI lower {}",
            w.dsr_ci_high,
            a.dsr_ci_low
        );
    }

    #[test]
    fn rank_ordinal_is_one_based_among_eligible() {
        let skilled = agent("skilled", (0..5).map(|_| run(0.002, 0.0005, 60)).collect());
        let lucky = {
            let mut runs = vec![run(0.02, 0.002, 60)];
            runs.extend((0..4).map(|_| run(0.0, 0.003, 60)));
            agent("lucky", runs)
        };
        let board = rank(&[lucky, skilled], &ScoreConfig::default());
        assert_eq!(board[0].rank_ordinal, 1, "leader is ordinal 1");
        assert_eq!(board[1].rank_ordinal, 0, "ineligible gets ordinal 0");
    }

    /// A field where every agent completed the identical cells must rank exactly
    /// as it did before the shared-cell restriction existed: the restriction is
    /// the identity there, down to the last bit of every statistic.
    #[test]
    fn homogeneous_field_is_unchanged_by_shared_run_set() {
        let skilled = agent("skilled", (0..5).map(|_| run(0.002, 0.0005, 60)).collect());
        let lucky = {
            let mut runs = vec![run(0.02, 0.002, 60)];
            runs.extend((0..4).map(|_| run(0.0, 0.003, 60)));
            agent("lucky", runs)
        };
        let steady = agent("steady", (0..5).map(|_| run(0.001, 0.001, 60)).collect());
        let field = [lucky, skilled, steady];
        let on = rank(&field, &ScoreConfig::default());
        let off = rank(
            &field,
            &ScoreConfig {
                shared_run_set: false,
                ..ScoreConfig::default()
            },
        );
        assert_eq!(on, off, "identical cells ⇒ identical board");
        assert!(on
            .iter()
            .all(|s| s.runs_scored == 5 && s.runs_submitted == 5));
    }

    /// The fairness property: an entrant whose runs cover only the easy cells is
    /// compared on the shared cells and gains no rank from the subset.
    #[test]
    fn easy_subset_entrant_is_compared_on_the_shared_cells() {
        // Cells 0–1 are easy (steady edge), cells 2–4 are hard (noisy, no edge).
        // The veteran completed all five; the entrant only the two easy ones, with
        // returns identical to the veteran's on those cells.
        let easy = || run(0.004, 0.0005, 60);
        let hard = || run(0.0, 0.004, 60);
        let veteran = agent("veteran", vec![easy(), easy(), hard(), hard(), hard()]);
        let entrant = agent("entrant", vec![easy(), easy()]);
        let field = [veteran, entrant];

        // Unrestricted, the subset pays: the entrant clears every gate while the
        // veteran's hard cells sink it.
        let off = rank(
            &field,
            &ScoreConfig {
                shared_run_set: false,
                ..ScoreConfig::default()
            },
        );
        assert_eq!(off[0].agent_id, "entrant");
        assert!(off[0].rank_eligible && !off[1].rank_eligible);

        // On the shared cells both are scored on exactly the two easy runs, so
        // their statistics coincide and the entrant holds no rank over the veteran.
        let on = rank(&field, &ScoreConfig::default());
        let get = |id: &str| on.iter().find(|s| s.agent_id == id).unwrap();
        let (v, e) = (get("veteran"), get("entrant"));
        assert_eq!(v.runs_scored, 2);
        assert_eq!(e.runs_scored, 2);
        assert_eq!(v.runs_submitted, 5);
        assert_eq!(e.runs_submitted, 2);
        assert_eq!(v.deflated_sharpe.to_bits(), e.deflated_sharpe.to_bits());
        assert_eq!(v.rank_eligible, e.rank_eligible);
        assert_eq!(v.tie_group, e.tie_group, "indistinguishable ⇒ one band");
        assert!(v.dsr_tied && e.dsr_tied);
        assert!(
            e.rank_ordinal == 0 || v.rank_ordinal <= e.rank_ordinal,
            "the entrant must not rank above the veteran"
        );
    }

    /// An empty submission is ineligible by construction and must not blank the
    /// shared set for everyone else.
    #[test]
    fn empty_submission_does_not_empty_the_shared_cells() {
        let skilled = agent("skilled", (0..5).map(|_| run(0.002, 0.0005, 60)).collect());
        let ghost = agent("ghost", Vec::new());
        let board = rank(&[ghost, skilled], &ScoreConfig::default());
        let get = |id: &str| board.iter().find(|s| s.agent_id == id).unwrap();
        assert_eq!(get("skilled").runs_scored, 5);
        assert!(get("skilled").rank_eligible);
        assert!(!get("ghost").rank_eligible);
    }

    /// Below the measurement floor the configured `trials_sr_std` applies and the
    /// board is byte-identical to scoring each agent alone with the same config.
    #[test]
    fn small_field_keeps_the_configured_sr_std_byte_identical() {
        let cfg = ScoreConfig::default();
        let field: Vec<AgentSubmission> = (0..4)
            .map(|i| {
                let m = 0.001 + 0.001 * i as f64;
                agent(
                    &format!("a{i}"),
                    (0..5).map(|_| run(m, 0.001, 60)).collect(),
                )
            })
            .collect();
        let board = rank(&field, &cfg);
        assert_eq!(board.len(), 4);
        for s in &board {
            assert_eq!(s.trials_sr_std_source, TrialsSrStdSource::Configured);
            assert_eq!(s.trials_sr_std.to_bits(), per_period_sr_std(&cfg).to_bits());
            assert_eq!(s.trials_sr_std_annualized, Some(cfg.trials_sr_std));
            let alone = score_agent(
                field.iter().find(|a| a.agent_id == s.agent_id).unwrap(),
                &cfg,
            );
            assert_eq!(s.deflated_sharpe.to_bits(), alone.deflated_sharpe.to_bits());
            assert_eq!(s.dsr_ci_low.to_bits(), alone.dsr_ci_low.to_bits());
        }
        // Pinning the measured path off reproduces the same board exactly.
        let pinned = rank(
            &field,
            &ScoreConfig {
                min_field_for_measured_sr_std: usize::MAX,
                ..cfg
            },
        );
        assert_eq!(board, pinned);
    }

    /// With enough agents the deflation uses the field's measured Sharpe
    /// dispersion, every score says so, and submission order cannot move it.
    #[test]
    fn large_field_measures_trials_sr_std_from_the_field() {
        let cfg = ScoreConfig::default();
        // Moderate Sharpes (roughly 0.1 to 0.7 per period) keep the PSR in its
        // sensitive range, so a change in the deflation bar shows in the DSR.
        let field: Vec<AgentSubmission> = (0..5)
            .map(|i| {
                let m = 0.0002 + 0.0003 * i as f64;
                agent(
                    &format!("a{i}"),
                    (0..5).map(|_| run(m, 0.003, 60)).collect(),
                )
            })
            .collect();
        let board = rank(&field, &cfg);

        // Hand-computed reference: sample std of sorted pooled Sharpes.
        let mut sharpes: Vec<f64> = field
            .iter()
            .map(|a| {
                let pooled: Vec<f64> = a
                    .runs
                    .iter()
                    .flat_map(|r| r.returns.iter().copied())
                    .collect();
                sharpe_ratio(&pooled)
            })
            .collect();
        sharpes.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let expected = std_dev(&sharpes);
        assert!(expected > 0.0 && expected.is_finite());

        let mut moved = false;
        for s in &board {
            assert_eq!(s.trials_sr_std_source, TrialsSrStdSource::Measured);
            assert_eq!(s.trials_sr_std.to_bits(), expected.to_bits());
            assert_eq!(s.trials_sr_std_annualized, None);
            let alone = score_agent(
                field.iter().find(|a| a.agent_id == s.agent_id).unwrap(),
                &cfg,
            );
            moved |= s.deflated_sharpe.to_bits() != alone.deflated_sharpe.to_bits();
        }
        assert!(
            moved,
            "measured dispersion must actually move the deflation"
        );

        let mut reversed = field.clone();
        reversed.reverse();
        let again = rank(&reversed, &cfg);
        assert_eq!(again[0].trials_sr_std.to_bits(), expected.to_bits());
    }

    /// A deterministic, roughly Gaussian return series with *exactly* the
    /// requested per-period mean and sample standard deviation (a 64-bit LCG
    /// feeding a sum of twelve uniforms, then re-standardized), so a test can
    /// state an agent's per-period Sharpe to the digit without an RNG dependency.
    fn gaussian_like(n: usize, mean_ret: f64, sd: f64, seed: u64) -> Vec<f64> {
        let mut state = seed;
        let mut next = || {
            state = state
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            (state >> 11) as f64 / (1u64 << 53) as f64
        };
        let raw: Vec<f64> = (0..n)
            .map(|_| (0..12).map(|_| next()).sum::<f64>() - 6.0)
            .collect();
        let (m, s) = (mean(&raw), std_dev(&raw));
        raw.iter().map(|x| mean_ret + sd * (x - m) / s).collect()
    }

    fn pooled_of(sub: &AgentSubmission) -> Vec<f64> {
        sub.runs
            .iter()
            .flat_map(|r| r.returns.iter().copied())
            .collect()
    }

    /// The configured prior is annualized; the value the deflation actually sees
    /// is that prior divided by `sqrt(periods_per_year)`, exactly once.
    #[test]
    fn annualized_prior_is_converted_per_period_once() {
        let cfg = ScoreConfig {
            trials_sr_std: 0.5,
            periods_per_year: 8760.0,
            ..ScoreConfig::default()
        };
        let expected = 0.5 / 8760f64.sqrt();
        assert_eq!(per_period_sr_std(&cfg).to_bits(), expected.to_bits());

        // A moderate Sharpe (about 0.09 per period) keeps the DSR off its 0/1
        // saturation so the three candidate conversions are distinguishable.
        let sub = agent("a", (0..5).map(|_| run(0.0002, 0.003, 300)).collect());
        let s = score_agent(&sub, &cfg);
        assert_eq!(s.trials_sr_std_source, TrialsSrStdSource::Configured);
        assert_eq!(s.trials_sr_std.to_bits(), expected.to_bits());
        assert_eq!(s.trials_sr_std_annualized, Some(0.5));
        // The DSR was computed with the converted value: not with the raw prior,
        // and not with the prior converted twice.
        let pooled = pooled_of(&sub);
        let once = deflated_sharpe_ratio(&pooled, cfg.n_trials, expected);
        let raw = deflated_sharpe_ratio(&pooled, cfg.n_trials, 0.5);
        let twice = deflated_sharpe_ratio(&pooled, cfg.n_trials, expected / 8760f64.sqrt());
        assert_eq!(s.deflated_sharpe.to_bits(), once.to_bits());
        assert_ne!(s.deflated_sharpe.to_bits(), raw.to_bits());
        assert_ne!(s.deflated_sharpe.to_bits(), twice.to_bits());
    }

    /// The measured path measures a dispersion of per-period Sharpes, so it is
    /// reported and used as-is: `periods_per_year` must not touch it.
    #[test]
    fn measured_sr_std_is_never_reconverted() {
        let field: Vec<AgentSubmission> = (0..5)
            .map(|i| {
                let m = 0.0002 + 0.0003 * i as f64;
                agent(
                    &format!("a{i}"),
                    (0..5).map(|_| run(m, 0.003, 60)).collect(),
                )
            })
            .collect();
        let mut sharpes: Vec<f64> = field.iter().map(|a| sharpe_ratio(&pooled_of(a))).collect();
        sharpes.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let raw_measured = std_dev(&sharpes);

        for ppy in [1.0, 252.0, 8760.0] {
            let cfg = ScoreConfig::for_periods_per_year(ppy);
            let board = rank(&field, &cfg);
            for s in &board {
                assert_eq!(s.trials_sr_std_source, TrialsSrStdSource::Measured);
                assert_eq!(s.trials_sr_std.to_bits(), raw_measured.to_bits());
                assert_eq!(s.trials_sr_std_annualized, None);
                let pooled = pooled_of(field.iter().find(|a| a.agent_id == s.agent_id).unwrap());
                let expected = deflated_sharpe_ratio(&pooled, cfg.n_trials, raw_measured);
                assert_eq!(s.deflated_sharpe.to_bits(), expected.to_bits());
            }
        }
    }

    /// The regression the unit fix protects: a daily index-like track (per-period
    /// mean 0.0004, sd 0.011, a Sharpe of 0.036 per day or about 0.57 annualized)
    /// scored with a deflation prior it can legitimately clear is rank-eligible
    /// once the prior is read as annualized, and was not when the same number was
    /// applied per period (`periods_per_year = 1` reproduces the old units).
    ///
    /// The prior here is 0.1 annualized at fifty trials (a field of similar
    /// index strategies), not the 0.5 default: at 0.5 the expected best-of-fifty
    /// luck is an annualized Sharpe of about 1.14, which the index itself sits
    /// below, so no track length clears it. That is the correct verdict for that
    /// prior, and it is the reason the default must be read as annualized: per
    /// period, the same 0.5 demanded an annualized 18.
    #[test]
    fn daily_buy_and_hold_clears_the_corrected_bar() {
        let runs: Vec<Run> = (0..3)
            .map(|i| Run {
                returns: gaussian_like(2500, 0.0004, 0.011, 0x5B_A7 + i),
                ..Run::default()
            })
            .collect();
        let index = agent("buy-and-hold", runs);
        let cfg = ScoreConfig {
            n_trials: 50,
            trials_sr_std: 0.1,
            periods_per_year: 252.0,
            ..ScoreConfig::default()
        };
        let s = score_agent(&index, &cfg);
        assert!(s.psr > 0.99, "the index is clearly positive: {s:?}");
        assert!(
            s.rank_eligible,
            "an index-like daily track must clear an annualized 0.1 prior: {s:?}"
        );

        let old_units = ScoreConfig {
            periods_per_year: 1.0,
            ..cfg
        };
        let old = score_agent(&index, &old_units);
        assert!(
            !old.rank_eligible && old.deflated_sharpe < 0.05,
            "applied per period the same prior was unreachable: {old:?}"
        );
    }

    /// Pins the direction of the conversion: the same annualized prior spread
    /// over more periods per year is a smaller per-period dispersion, so the
    /// per-period deflation benchmark is lower on hourly bars than on daily.
    #[test]
    fn hourly_bar_is_stricter_per_period_than_daily_for_the_same_annualized_prior() {
        let daily = ScoreConfig::for_periods_per_year(252.0);
        let hourly = ScoreConfig::for_periods_per_year(8760.0);
        let star = |cfg: &ScoreConfig| expected_max_sharpe(per_period_sr_std(cfg), cfg.n_trials);
        assert!(star(&hourly) > 0.0 && star(&daily) > 0.0);
        assert!(
            star(&hourly) < star(&daily),
            "hourly sr_star {} must be below daily {}",
            star(&hourly),
            star(&daily)
        );
        // And the annualized benchmark they imply is the same number.
        let ann_h = star(&hourly) * 8760f64.sqrt();
        let ann_d = star(&daily) * 252f64.sqrt();
        assert!((ann_h - ann_d).abs() < 1e-12, "{ann_h} vs {ann_d}");
    }

    /// With the default `per_run_min_annual_sharpe = 0.0` the per-run test is the
    /// one the benchmark always ran, `PSR(returns, 0) >= per_run_psr_bar`, on
    /// every timeframe.
    #[test]
    fn default_min_annual_sharpe_is_identical_to_the_old_per_run_test() {
        let mut runs = vec![run(0.02, 0.002, 60), run(0.0, 0.003, 60)];
        runs.extend((0..3).map(|_| run(0.002, 0.0005, 60)));
        let sub = agent("mixed", runs);
        let clean = agent("clean", (0..3).map(|_| run(0.002, 0.0005, 60)).collect());
        for ppy in [52.0, 252.0, 8760.0] {
            let cfg = ScoreConfig::for_periods_per_year(ppy);
            assert_eq!(per_run_psr_benchmark(&cfg).to_bits(), 0f64.to_bits());
            let old: Vec<bool> = sub
                .runs
                .iter()
                .map(|r| probabilistic_sharpe_ratio(&r.returns, 0.0) >= cfg.per_run_psr_bar)
                .collect();
            assert!(old.iter().any(|&p| p) && old.iter().any(|&p| !p));
            assert_eq!(
                score_agent(&sub, &cfg).passed_k,
                pass_k(&old, PassMode::All)
            );
            assert!(score_agent(&clean, &cfg).passed_k);
        }
        // A non-zero annualized minimum converts per period and raises the bar.
        let strict = ScoreConfig {
            per_run_min_annual_sharpe: 3.0,
            ..ScoreConfig::for_periods_per_year(252.0)
        };
        assert_eq!(
            per_run_psr_benchmark(&strict).to_bits(),
            (3.0 / 252f64.sqrt()).to_bits()
        );
        let weak = agent("weak", (0..3).map(|_| run(0.0005, 0.004, 60)).collect());
        assert!(score_agent(&weak, &ScoreConfig::for_periods_per_year(252.0)).passed_k);
        assert!(
            !score_agent(&weak, &strict).passed_k,
            "a minimum annualized Sharpe of 3.0 must fail a 0.0005/0.004 track"
        );
    }
}
