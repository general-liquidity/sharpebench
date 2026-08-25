//! The composite score + leaderboard ranking — where the gates compose.
//!
//! An agent ranks **only if** every gate holds:
//! 1. its pooled Deflated Sharpe clears `dsr_bar` (survives multiple-testing),
//! 2. it passes the per-run bar on enough seed×window runs (`pass^k`; every run
//!    under the default `PassMode::All`),
//! 3. it has zero block-severity process violations in any run,
//! 4. its bootstrap p-value beats `alpha` (the edge isn't noise).
//!
//! Raw mean return is recorded but is **never** the rank key — that is the whole
//! point of SharpeBench. Run the included synthetic agents (see tests) to watch a
//! lucky agent with a higher raw return get demoted below a skilled one.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
pub use sharpebench_protocol::DeclaredMandate;

use crate::calibration::brier_score;
use crate::comparison_sets::{comparison_set, restrict_to_shared, TaggedRun, TaggedSubmission};
use crate::decay::edge_half_life;
use crate::deflated_sharpe::{
    deflated_sharpe_ratio_against_null, expected_max_sharpe, probabilistic_sharpe_ratio,
    sharpe_ratio,
};
use crate::econrationality::{elicit_revealed_selection, rationality_score};
use crate::pass_k::{pass_k, PassMode};
use crate::percentile::percentile_of;
use crate::process::{process_score, ProcessEvent, ProcessScore, Trace};
use crate::rediscovery::{clone_clusters, CLONE_COLLAPSE_COSINE};
use crate::roles::{attribute_behavior_roles, RoleContribution};
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

/// Mandate declarations for a field, keyed by agent id: the input
/// [`rank_declared`] scores beside the host verdict. An agent absent from the
/// map is undeclared and scored exactly as [`rank`] scores it.
pub type MandateDeclarations = BTreeMap<String, DeclaredMandate>;

/// One submission object as it arrives on the wire: every [`AgentSubmission`]
/// key plus an optional `declared_mandate` (see [`DeclaredMandate`]). The
/// declaration is part of the submitted object, not of the host's config, so a
/// submitter states the mandate and the host cannot restate it. Parse a field
/// as `Vec<DeclaredSubmission>` and hand it to [`split_declarations`].
#[derive(Clone, Debug, Deserialize)]
pub struct DeclaredSubmission {
    #[serde(flatten)]
    pub submission: AgentSubmission,
    #[serde(default)]
    pub declared_mandate: Option<DeclaredMandate>,
}

/// Separate a parsed field into the submissions and their declarations, the two
/// arguments of [`rank_declared`]. Undeclared submissions leave no entry.
pub fn split_declarations(
    field: Vec<DeclaredSubmission>,
) -> (Vec<AgentSubmission>, MandateDeclarations) {
    let mut declarations = MandateDeclarations::new();
    let subs = field
        .into_iter()
        .map(|d| {
            if let Some(m) = d.declared_mandate {
                declarations.insert(d.submission.agent_id.clone(), m);
            }
            d.submission
        })
        .collect();
    (subs, declarations)
}

/// The reliability verdict a [`DeclaredMandate`] resolves to: the question the
/// kernel actually tests for it. [`DeclaredMandate::OutperformBuyAndHold`] resolves to
/// [`MandateVerdict::RelativeTo`] buy-and-hold, so two declarations that ask the
/// same question share one verdict, and one **mandate class**: agents are
/// compared on their declared column only against agents whose resolved
/// verdict is equal to theirs (same variant, same benchmark id, same bound).
/// Serialized like the declaration, e.g. `{"kind":"relative_to","benchmark_id":
/// "buy-and-hold"}`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum MandateVerdict {
    /// pass^k in [`PassMode::All`] on raw returns: the shipped default.
    AbsoluteReturn,
    /// pass^k in [`PassMode::RelativeToBenchmark`] against `benchmark_id`'s run
    /// in the same cell (see [`per_run_passes`]).
    RelativeTo { benchmark_id: String },
    /// pass^k in [`PassMode::Any`] on raw returns plus a per-run drawdown bound:
    /// the never-catastrophic verdict of
    /// [`ScoreConfig::reliability_never_catastrophic`], with the bound declared
    /// by the submitter rather than configured by the host.
    DrawdownCapped { max_per_run_drawdown: f64 },
}

impl MandateVerdict {
    /// Resolve a declaration to the verdict the kernel tests.
    pub fn of(declared: &DeclaredMandate) -> Self {
        match declared {
            DeclaredMandate::AbsoluteReturn => Self::AbsoluteReturn,
            DeclaredMandate::RelativeTo { benchmark_id } => Self::RelativeTo {
                benchmark_id: benchmark_id.clone(),
            },
            DeclaredMandate::DrawdownCapped {
                max_per_run_drawdown,
            } => Self::DrawdownCapped {
                max_per_run_drawdown: *max_per_run_drawdown,
            },
            DeclaredMandate::OutperformBuyAndHold => Self::RelativeTo {
                benchmark_id: default_benchmark_agent_id(),
            },
        }
    }

    fn pass_mode(&self) -> PassMode {
        match self {
            Self::AbsoluteReturn => PassMode::All,
            Self::RelativeTo { .. } => PassMode::RelativeToBenchmark,
            Self::DrawdownCapped { .. } => PassMode::Any,
        }
    }

    /// The agent id whose same-cell runs this verdict tests excess over, if any.
    pub fn benchmark_id(&self) -> Option<&str> {
        match self {
            Self::RelativeTo { benchmark_id } => Some(benchmark_id),
            Self::AbsoluteReturn | Self::DrawdownCapped { .. } => None,
        }
    }

    /// Whether the agent's worst single-run drawdown satisfies the verdict's own
    /// bound. `true` for verdicts without one. A declared bound outside `(0, 1]`
    /// (including NaN) is a misdeclaration and fails closed.
    fn drawdown_bound_holds(&self, worst_run_drawdown: f64) -> bool {
        match self {
            Self::DrawdownCapped {
                max_per_run_drawdown: x,
            } => *x > 0.0 && *x <= 1.0 && worst_run_drawdown <= *x,
            Self::AbsoluteReturn | Self::RelativeTo { .. } => true,
        }
    }

    /// The verdict in words, for a board row: `absolute return`,
    /// `relative to buy-and-hold`, `drawdown capped at 0.20 per run`.
    pub fn describe(&self) -> String {
        match self {
            Self::AbsoluteReturn => "absolute return".to_string(),
            Self::RelativeTo { benchmark_id } => format!("relative to {benchmark_id}"),
            Self::DrawdownCapped {
                max_per_run_drawdown,
            } => format!("drawdown capped at {max_per_run_drawdown:.2} per run"),
        }
    }
}

/// A total order over mandate classes for grouping: variant, then benchmark
/// id, then the exact bits of the declared bound (so two bounds that print
/// alike are still two classes).
fn class_key(v: &MandateVerdict) -> (u8, &str, u64) {
    match v {
        MandateVerdict::AbsoluteReturn => (0, "", 0),
        MandateVerdict::RelativeTo { benchmark_id } => (1, benchmark_id, 0),
        MandateVerdict::DrawdownCapped {
            max_per_run_drawdown,
        } => (2, "", max_per_run_drawdown.to_bits()),
    }
}

/// A declaration resolved against the field: the mandate plus the benchmark
/// submission its verdict names, looked up by the caller in the field being
/// ranked (`None` when the verdict names no benchmark, when the field lacks it,
/// or when there is no field, all of which fail the relative test closed).
struct ResolvedDeclaration<'a> {
    mandate: &'a DeclaredMandate,
    benchmark: Option<&'a AgentSubmission>,
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
    /// Max tolerable drawdown of any single run (one window times one seed), in
    /// [0, 1]. 1.0 = unconstrained, the default.
    ///
    /// The pooled bound is a whole-track budget, and it cannot tell a track that
    /// loses 15% in one bear window and nothing elsewhere from one that loses
    /// 4% in each of five windows: both sit inside a 20% cap, and they are
    /// different agents to hand capital to. This bound is checked on every run
    /// separately, from that run's own starting equity, so it says what the
    /// pooled cap cannot: no single regime, under any execution seed, may lose
    /// more than this. It is the safety half of the "never catastrophic in any
    /// regime" verdict (see [`ScoreConfig::reliability_never_catastrophic`]).
    ///
    /// Drawdown is multiplicative, so a run's own drawdown is never above the
    /// pooled track's (every within-run peak-to-trough pair is also a pooled
    /// pair). The bound therefore only bites when set below `max_drawdown`,
    /// which is how it is meant to be used: a loose whole-track budget and a
    /// tight per-regime one.
    #[serde(default = "default_max_run_drawdown")]
    pub max_run_drawdown: f64,
}

impl Default for Mandate {
    fn default() -> Self {
        Self {
            max_drawdown: 1.0,
            max_run_drawdown: default_max_run_drawdown(),
        }
    }
}

/// Unconstrained per-run drawdown. Configs serialized before the field existed
/// deserialize to this, so the mandate they expressed is unchanged.
fn default_max_run_drawdown() -> f64 {
    1.0
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
    /// Host-side lower bound on the number of trials. [`score_agent`] uses this
    /// value directly because it has no field context. [`rank`] first replaces
    /// it with `max(n_trials, submitted_field_size)`, then each submission's
    /// declared private trials are added. A ranked host therefore cannot erase
    /// strategies it can observe by configuring `n_trials = 1`.
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
    /// Explicit per-period mean Sharpe of the fixed null/trial population. The
    /// DSR threshold is `null_mean + expected_max_sharpe(sigma, N)`, not merely
    /// its dispersion term. The shipped default is the explicit conservative
    /// zero-Sharpe null (0.0), not a claim about a turnover-matched cost model.
    /// Any non-zero benchmark must be recorded here rather than smuggled in by
    /// centering returns or by relabeling a candidate field's mean as a null.
    #[serde(default)]
    pub deflation_null_mean_per_period: f64,
    /// Deflated-Sharpe bar an agent must clear to be rank-eligible (e.g. 0.95).
    pub dsr_bar: f64,
    /// Per-run PSR bar each individual run must clear for pass^k: the confidence
    /// with which the run's true Sharpe must exceed `per_run_min_annual_sharpe`.
    ///
    /// PSR scales with `sqrt(n - 1)` of the run's length, so on short windows this
    /// gate is necessarily weak for any bar: reaching 0.90 against zero needs a
    /// per-period Sharpe of `Z(0.90) / sqrt(n - 1)` under normal moments, which is
    /// 0.146 on a 78-bar window and 0.081 on a 250-bar one. Annualized those are
    /// about 1.05 and 1.29 on weekly and daily bars respectively; the per-period
    /// number is what the kernel compares against.
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
    /// How many of the per-run tests must pass for pass^k. The default
    /// [`PassMode::All`] is the eligibility gate the benchmark has always run: a
    /// money agent that is safe on average is not safe, so every window and every
    /// seed must clear the bar. It is also why a long-only agent is ineligible on
    /// any dataset with a bear window. That is the right verdict for "profitable
    /// in every regime", and the wrong question for "has an edge and never blows
    /// up", which is what [`PassMode::Any`] or [`PassMode::AtLeast`] combined
    /// with `mandate.max_run_drawdown` asks. The mode is a config field, not a
    /// constant, so the two verdicts can be produced from two configs and shown
    /// side by side; it is not a knob to turn quietly.
    #[serde(default)]
    pub pass_mode: PassMode,
    /// The agent whose runs are the benchmark under
    /// [`PassMode::RelativeToBenchmark`]. Under every mode it also names the
    /// fixed, aligned series used by the field-level RC/SPA/Romano--Wolf
    /// diagnostics; if it is absent the scorer records the explicit
    /// `zero-return-cash` fallback instead of substituting the candidate-field
    /// mean. Default `"buy-and-hold"`, the reference agent every harness field
    /// carries. The
    /// benchmark is looked up **in the field being ranked**, by id, so its run at
    /// position `i` is the same (window, seed) cell as every other agent's run
    /// `i`: same frozen bars, same window, same execution seed, same cost model.
    /// Nothing is fetched from outside the field, which is what rules out
    /// leakage. [`score_agent`] has no field and therefore no benchmark; under
    /// the relative mode it fails every run (see [`per_run_passes`]).
    #[serde(default = "default_benchmark_agent_id")]
    pub benchmark_agent_id: String,
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
    /// Collapse near-clone submissions to one vote each before [`rank`] measures
    /// `trials_sr_std` (default: on). Two pooled streams are clones when their
    /// `|cosine|` meets [`crate::rediscovery::CLONE_COLLAPSE_COSINE`] (0.995, a
    /// constant of its own: the rediscovery screen's 0.97 flags similar
    /// strategies for review and would silence honest collinear agents, which
    /// the benchmark's own luck floor is against buy-and-hold); clusters are the
    /// connected components of that relation and each contributes its median
    /// Sharpe once, both to the dispersion estimate and to the field count the
    /// measurement floor is checked against. Clones are still scored and still
    /// appear on the board; they just do not vote on the bar. A field with no
    /// clones measures exactly as before. Off only reproduces the sock-puppet
    /// exposure the self-audit documents, where 200 near-duplicate agents shrink
    /// the measured dispersion and lower the deflation bar for everyone.
    #[serde(default = "default_dedup_clones_for_measured_sr_std")]
    pub dedup_clones_for_measured_sr_std: bool,
    /// Annualized lower bound for a field-measured cross-trial Sharpe
    /// dispersion. A field is allowed to demonstrate that strategies are *more*
    /// heterogeneous than the precommitted prior, but it cannot lower the
    /// deflation bar merely by recruiting a broad set of genuinely dissimilar,
    /// low-dispersion streams. The bound is converted to per-period units once,
    /// alongside [`trials_sr_std`](Self::trials_sr_std).
    ///
    /// This field and `trials_sr_std` both default to 0.5. They are independently
    /// configurable, so an operator changing the prior must also choose the
    /// measured-field floor explicitly. With the shipped pair of defaults,
    /// measurement is a one-way safety update: it can tighten the bar but cannot
    /// relax it. Every resulting score records when this floor binds.
    #[serde(default = "default_min_measured_trials_sr_std")]
    pub min_measured_trials_sr_std: f64,
    /// Number of execution-seed replicates in each window-major run block.
    ///
    /// A run is one stochastic execution of the *same* frozen market window.
    /// Replicates therefore estimate the conditional return of that window; they
    /// are not additional independent market-time observations.  When this is
    /// greater than one, scoring first averages aligned returns within each
    /// window's replicate block and only then computes PSR, DSR, bootstrap and
    /// every pooled-track diagnostic. `pass^k` deliberately remains per run:
    /// it asks whether every execution was safe. The default one preserves the
    /// public API for callers with one execution per window.
    #[serde(default = "default_execution_seeds_per_window")]
    pub execution_seeds_per_window: usize,
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

/// Clone collapse before the measurement is on by default: a field-level control
/// against a field-level attack is not an opt-in.
fn default_dedup_clones_for_measured_sr_std() -> bool {
    true
}

/// Do not let the submitted field reduce the precommitted deflation prior by
/// default. See [`ScoreConfig::min_measured_trials_sr_std`].
fn default_min_measured_trials_sr_std() -> f64 {
    0.5
}

fn default_execution_seeds_per_window() -> usize {
    1
}

/// Default periods per year: daily equity bars, the benchmark's historical
/// assumption. Configs serialized before the field existed deserialize to this,
/// so their meaning is unchanged.
fn default_periods_per_year() -> f64 {
    252.0
}

/// Default benchmark agent for the relative verdict: the buy-and-hold reference
/// agent, under the id the harness and the CLI give it.
fn default_benchmark_agent_id() -> String {
    "buy-and-hold".to_string()
}

/// The per-run pass vector pass^k aggregates: one bool per run, in submission
/// order. This is the kernel's own test, exposed so a report can say *which*
/// runs passed rather than only whether all did.
///
/// Under [`PassMode::All`], [`PassMode::Any`] and [`PassMode::AtLeast`] run `i`
/// passes iff `PSR(returns_i, per_run_psr_benchmark(cfg)) >= cfg.per_run_psr_bar`;
/// `benchmark` is ignored.
///
/// Under [`PassMode::RelativeToBenchmark`] run `i` passes iff all of:
/// 1. `benchmark` is present and has a run at position `i` of the same length
///    (the same (window, seed) cell; a missing or misaligned cell is a failure,
///    never a silent fallback to the absolute test);
/// 2. the excess series `e_t = returns_i[t] - benchmark_i[t]` has strictly
///    positive standard deviation;
/// 3. `PSR(e, per_run_psr_benchmark(cfg)) >= cfg.per_run_psr_bar`.
///
/// Rule 2 is what stops the benchmark from certifying itself. Its own excess
/// series is identically zero, and a zero-dispersion series carries no evidence
/// of outperformance at all: `sharpe_ratio` defines it as 0 and PSR then returns
/// `norm_cdf(0) = 0.5`, so it already fails any bar above one half, but an
/// operator who lowered `per_run_psr_bar` to 0.5 would admit it. The rule makes
/// the refusal unconditional: a run indistinguishable from the benchmark is a
/// run with no excess edge, and the relative verdict is a claim about excess
/// edge in every window. It also refuses a clone of the benchmark under another
/// id, for the same reason.
pub fn per_run_passes(
    sub: &AgentSubmission,
    benchmark: Option<&AgentSubmission>,
    cfg: &ScoreConfig,
) -> Vec<bool> {
    let bar = per_run_psr_benchmark(cfg);
    match cfg.pass_mode {
        PassMode::All | PassMode::Any | PassMode::AtLeast(_) => sub
            .runs
            .iter()
            .map(|r| probabilistic_sharpe_ratio(&r.returns, bar) >= cfg.per_run_psr_bar)
            .collect(),
        PassMode::RelativeToBenchmark => sub
            .runs
            .iter()
            .enumerate()
            .map(|(i, r)| {
                benchmark
                    .and_then(|b| b.runs.get(i))
                    .and_then(|b| excess_returns(&r.returns, &b.returns))
                    .is_some_and(|e| {
                        std_dev(&e) > 0.0
                            && probabilistic_sharpe_ratio(&e, bar) >= cfg.per_run_psr_bar
                    })
            })
            .collect(),
    }
}

/// Period-by-period excess of `returns` over `benchmark`, or `None` when the two
/// series are not the same length (not the same cell).
fn excess_returns(returns: &[f64], benchmark: &[f64]) -> Option<Vec<f64>> {
    (returns.len() == benchmark.len())
        .then(|| returns.iter().zip(benchmark).map(|(a, b)| a - b).collect())
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
    /// The field was measured, but its value was below the operator's
    /// precommitted floor and the floor was applied instead.
    MeasuredFloored,
}

impl Default for ScoreConfig {
    fn default() -> Self {
        Self {
            n_trials: 50,
            trials_sr_std: 0.5,
            deflation_null_mean_per_period: 0.0,
            dsr_bar: 0.95,
            per_run_psr_bar: 0.90,
            per_run_min_annual_sharpe: 0.0,
            pass_mode: PassMode::default(),
            benchmark_agent_id: default_benchmark_agent_id(),
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
            dedup_clones_for_measured_sr_std: default_dedup_clones_for_measured_sr_std(),
            min_measured_trials_sr_std: default_min_measured_trials_sr_std(),
            execution_seeds_per_window: default_execution_seeds_per_window(),
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

    /// The "never catastrophic in any regime" reliability verdict, as one named
    /// preset so an ablation against the default is a one-line change of config
    /// rather than a patched binary.
    ///
    /// The default config certifies *profitable in every regime with 90%
    /// confidence*: pass^k in [`PassMode::All`] requires every window and every
    /// seed to clear the per-run PSR bar. This preset certifies something
    /// different: *never draws down more than `max_run_dd` in any regime, with the
    /// edge tested on the pooled track*. It sets `pass_mode` to [`PassMode::Any`]
    /// (one run clearing the per-run bar is enough) and
    /// `mandate.max_run_drawdown` to `max_run_dd`, and leaves every other gate
    /// where the default has it: the pooled Deflated Sharpe must still clear
    /// `dsr_bar`, the bootstrap edge test must still reject noise, the process
    /// must still be clean, and the pooled drawdown mandate still applies. The
    /// edge is therefore tested once, on the whole track, and reliability is
    /// asked of the loss side only.
    ///
    /// This is a **weaker safety claim** than the default. It admits a
    /// regime-dependent edge such as equity beta, which the default correctly
    /// refuses because owning the index is not safe in a bear market. It also
    /// admits an agent whose edge lives in one run, provided that run is large
    /// enough for the pooled track to survive deflation and the bootstrap; the
    /// default refuses that agent through pass^k, and this preset has given that
    /// refusal up. That is the point of running both: which agents clear which
    /// gate, on which asset class and timeframe, is the table the paper is for.
    /// It is suitable for an ablation, not as the default for a money agent, and
    /// nothing here changes the default.
    pub fn reliability_never_catastrophic(max_run_dd: f64) -> Self {
        Self {
            pass_mode: PassMode::Any,
            mandate: Mandate {
                max_run_drawdown: max_run_dd,
                ..Mandate::default()
            },
            ..Self::default()
        }
    }

    /// The mandate-relative reliability verdict, as one named preset.
    ///
    /// The default certifies *profitable in every regime*: an all-weather
    /// absolute-return mandate, which is why the index itself cannot pass on any
    /// range containing a bear window. This preset certifies *beats owning the
    /// universe in every regime*: it sets `pass_mode` to
    /// [`PassMode::RelativeToBenchmark`] and names `benchmark_agent_id`, so each
    /// run's PSR is tested on its excess return over the benchmark's run in the
    /// same (window, seed) cell (see [`per_run_passes`] for the exact rule and
    /// for why the benchmark itself, whose excess is identically zero, fails).
    /// Every other gate is the default's: the pooled Deflated Sharpe, the
    /// bootstrap, the process audit and the drawdown mandate are all still
    /// computed on the agent's own raw returns, so this is a different
    /// reliability question, not a weaker set of gates. It is opt-in and changes
    /// nothing about the default.
    pub fn relative_to_benchmark(benchmark_agent_id: &str) -> Self {
        Self {
            pass_mode: PassMode::RelativeToBenchmark,
            benchmark_agent_id: benchmark_agent_id.to_string(),
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
    /// Whether the agent respected its mandate: both the pooled drawdown cap and
    /// the per-run cap.
    pub mandate_ok: bool,
    /// The largest maximum drawdown of any single run, in [0, 1], each run
    /// measured from its own starting equity: the number the per-run mandate
    /// bound is checked against. Never above `max_drawdown`, the pooled figure,
    /// which also counts losing streaks that span runs. 0.0 with no runs.
    #[serde(default)]
    pub worst_run_drawdown: f64,
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
    /// Fixed benchmark used by the field-wide White/SPA tests. This is distinct
    /// from the equal-weight field proxy used only for alpha/beta attribution.
    /// When the named benchmark is absent, the tests use the explicit zero
    /// return (cash) benchmark and stamp `"zero-return-cash"` here.
    #[serde(default)]
    pub field_significance_benchmark: String,
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
    /// Annualized equivalent of the exact per-period dispersion that deflated
    /// this score. Unlike `trials_sr_std_annualized`, this is present for both
    /// configured and measured paths, making a field measurement's units
    /// explicit without ever feeding an annualized value back into DSR.
    #[serde(default)]
    pub trials_sr_std_annualized_equivalent: f64,
    /// Expected maximum per-period Sharpe under the effective trial footprint
    /// and the exact dispersion used by DSR: the deflation bar, in the same
    /// units as the pooled per-period Sharpe.
    #[serde(default)]
    pub deflation_bar_per_period: f64,
    /// `deflation_bar_per_period * sqrt(periods_per_year)`, reported solely for
    /// legibility across dataset frequencies.
    #[serde(default)]
    pub deflation_bar_annualized_equivalent: f64,
    /// The fixed null-population mean included in the deflation bar, per period.
    #[serde(default)]
    pub deflation_null_mean_per_period: f64,
    /// Number of temporally distinct pooled observations used by PSR, DSR,
    /// bootstrap and pooled diagnostics. With execution replicates this is not
    /// multiplied by the replicate count.
    #[serde(default)]
    pub pooled_observations: usize,
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
    /// Graded process-discipline scalar over all runs' concatenated traces, in
    /// [0, 1]: any block-severity violation zeroes it; each warn-severity event
    /// costs 0.1 (floored at 0). **Reported, never eligibility**: the only
    /// binary process gate remains `process_ok` (zero block violations). Warn
    /// events additionally order agents within a DSR tie band in [`rank`]:
    /// ordering within statistical ties only, because warn events are real
    /// information but not calibrated enough to gate. Scores archived before
    /// this field existed deserialize to the clean value 1.0; `process_ok`
    /// stays authoritative for them.
    #[serde(default = "default_process_score")]
    pub process_score: f64,
    /// Count of warn-severity process events across all runs (concentration
    /// breaches, hedged tail-selling). Reported, never eligibility.
    #[serde(default)]
    pub process_warnings: usize,
    /// Economic-rationality score of the submission's one recorded choice:
    /// submitting this track out of its declared candidate set, valued by
    /// per-period Sharpe (see [`crate::econrationality`]). 1.0 = the pick
    /// respected first-order dominance, 0.0 = a declared candidate strictly
    /// dominated it. `None` when the submission declared no comparable
    /// candidates. Reported, never gating.
    #[serde(default)]
    pub econ_rationality_score: Option<f64>,
    /// Count of first-order-dominance violations in the recorded selection
    /// choice (0 or 1, since one choice is recorded per submission). `None` when
    /// nothing was elicitable. Reported, never gating.
    #[serde(default)]
    pub econ_dominance_violations: Option<usize>,
    /// Behavior-role attribution over the recorded runs (see
    /// [`crate::roles::attribute_behavior_roles`]): the regression loading of
    /// each populated behavior class (clean-active / idle / warned /
    /// block-violating) on the equal-weight team stream. Empty when not
    /// estimable. Reported, never gating.
    #[serde(default)]
    pub role_contributions: Vec<RoleContribution>,
    /// The mandate the submitter declared (see [`DeclaredMandate`]), echoed so
    /// the record says what was asked. `None` = undeclared; the five declared
    /// fields are then absent from the serialized score, so an undeclared
    /// field's bytes are unchanged.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub declared_mandate: Option<DeclaredMandate>,
    /// The verdict the declaration resolved to and was tested under
    /// ([`MandateVerdict::of`]). `None` when undeclared.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub verdict_applied: Option<MandateVerdict>,
    /// pass^k under `verdict_applied`: the declared verdict's own per-run series
    /// and aggregation. `passed_k` stays the host verdict's. `None` when
    /// undeclared.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub declared_passed_k: Option<bool>,
    /// Eligibility under the declared verdict: the host predicate with
    /// `passed_k` replaced by `declared_passed_k` and, for a drawdown-capped
    /// verdict, the declared per-run bound added. Every other gate (deflated
    /// Sharpe, bootstrap, process, the host's drawdown mandate) is the same
    /// test on the same raw returns, so a declaration can only select the
    /// reliability question, never relax a gate. **Reported, never rank**: the
    /// board sorts on `rank_eligible` under the host verdict, this column is
    /// labeled beside it, and `rank_ordinal` counts host-eligible agents only.
    /// `None` when undeclared.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub declared_mandate_eligible: Option<bool>,
    /// 1-based position by deflated Sharpe among the agents that are eligible
    /// under the **same** resolved verdict (the agent's mandate class), filled
    /// by [`rank_declared`]. Agents in different classes are never ordered
    /// against each other on this column, and no class is mixed with the host
    /// board's ordinal. `None` when undeclared or not declared-eligible.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub declared_mandate_ordinal: Option<usize>,
}

impl CompositeScore {
    /// The board-row wording of eligibility under the declared verdict, e.g.
    /// `eligible under declared verdict (relative to buy-and-hold); host-board
    /// ineligible`. This deliberately does not say that the mandate condition
    /// itself failed: `declared_mandate_eligible` is the full eligibility
    /// conjunction with the declared reliability verdict substituted, so DSR,
    /// bootstrap, process, or the host drawdown gate may be the refusing term.
    /// The second clause is `rank_eligible` under the host verdict. `None` when
    /// undeclared.
    pub fn mandate_verdict_label(&self) -> Option<String> {
        let verdict = self.verdict_applied.as_ref()?;
        let eligibility = if self.declared_mandate_eligible == Some(true) {
            "eligible"
        } else {
            "ineligible"
        };
        let host = if self.rank_eligible {
            "host-board eligible"
        } else {
            "host-board ineligible"
        };
        Some(format!(
            "{eligibility} under declared verdict ({}); {host}",
            verdict.describe()
        ))
    }
}

/// Scores archived before `process_score` existed carry no graded scalar; they
/// deserialize to the clean 1.0 (with `process_warnings` 0), and `process_ok`
/// remains the authoritative record of whether the trace was clean.
fn default_process_score() -> f64 {
    1.0
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
    null_mean_per_period: f64,
}

impl Deflation {
    fn configured(cfg: &ScoreConfig) -> Self {
        Self {
            sr_std: per_period_sr_std(cfg),
            annualized: Some(cfg.trials_sr_std),
            source: TrialsSrStdSource::Configured,
            null_mean_per_period: cfg.deflation_null_mean_per_period,
        }
    }

    /// `sr_std` is the field's measured dispersion of per-period Sharpes: already
    /// in the kernel's units, so it must not pass through the conversion.
    fn measured(sr_std: f64, cfg: &ScoreConfig) -> Self {
        let floor = cfg.min_measured_trials_sr_std / cfg.periods_per_year.sqrt();
        let floored = sr_std < floor;
        Self {
            sr_std: sr_std.max(floor),
            annualized: None,
            source: if floored {
                TrialsSrStdSource::MeasuredFloored
            } else {
                TrialsSrStdSource::Measured
            },
            null_mean_per_period: cfg.deflation_null_mean_per_period,
        }
    }
}

/// Score a single agent submission against `cfg`. With no field to measure the
/// cross-trial dispersion on, the configured annualized prior applies, converted
/// to per period once.
pub fn score_agent(sub: &AgentSubmission, cfg: &ScoreConfig) -> CompositeScore {
    score_agent_with(sub, cfg, Deflation::configured(cfg), None, None)
}

/// [`score_agent`] with the agent's declared mandate. With no field there is no
/// benchmark, so a declaration that names one ([`DeclaredMandate::RelativeTo`],
/// [`DeclaredMandate::OutperformBuyAndHold`]) is not decidable without its
/// aligned field benchmark. This single-submission API therefore records the
/// declaration and resolved verdict but leaves the declared pass and eligibility
/// fields unavailable (`None`); it never encodes missing field context as a
/// failed mandate. Use [`rank_declared`] to test a relative declaration against
/// its field. `None` scores exactly as [`score_agent`].
pub fn score_agent_declared(
    sub: &AgentSubmission,
    declared: Option<&DeclaredMandate>,
    cfg: &ScoreConfig,
) -> CompositeScore {
    if let Some(mandate) = declared {
        let verdict = MandateVerdict::of(mandate);
        if verdict.benchmark_id().is_some() {
            let mut score = score_agent(sub, cfg);
            score.declared_mandate = Some(mandate.clone());
            score.verdict_applied = Some(verdict);
            return score;
        }
    }
    let resolved = declared.map(|mandate| ResolvedDeclaration {
        mandate,
        benchmark: None,
    });
    score_agent_with(sub, cfg, Deflation::configured(cfg), None, resolved)
}

fn score_agent_with(
    sub: &AgentSubmission,
    cfg: &ScoreConfig,
    defl: Deflation,
    benchmark: Option<&AgentSubmission>,
    declared: Option<ResolvedDeclaration<'_>>,
) -> CompositeScore {
    let pooled = pooled_returns(sub, cfg.execution_seeds_per_window);

    let psr = probabilistic_sharpe_ratio(&pooled, 0.0);
    // Fold the agent's declared in-sample search budget into the deflation trial
    // footprint: an agent that tried 5000 configs to find this strategy faces a
    // higher bar than one that tried none (front-end data-snooping control).
    let effective_n_trials = cfg.n_trials.saturating_add(sub.in_sample_trials);
    let dsr = deflated_sharpe_ratio_against_null(
        &pooled,
        effective_n_trials,
        defl.null_mean_per_period,
        defl.sr_std,
    );
    let deflation_bar_per_period =
        defl.null_mean_per_period + expected_max_sharpe(defl.sr_std, effective_n_trials);

    // pass^k: each run individually clears the per-run PSR bar against the
    // per-period benchmark the annualized minimum converts to (0 by default), on
    // its raw returns or, under the relative mode, on its excess over the
    // benchmark agent's run in the same cell.
    let per_run = per_run_passes(sub, benchmark, cfg);
    let passed_k = pass_k(&per_run, cfg.pass_mode);

    // process: a single block-severity violation in any run is disqualifying.
    // Alongside the binary gate, the graded scalar and the warn count are
    // reported: warn-severity events (concentration breaches, hedged
    // tail-selling) are real information, but not calibrated enough to gate, so
    // they never touch `rank_eligible`; they surface in the score and, in
    // [`rank`], order agents within a DSR tie band only.
    let per_run_process: Vec<ProcessScore> =
        sub.runs.iter().map(|r| process_score(&r.trace)).collect();
    let process_ok = per_run_process.iter().all(ProcessScore::is_clean);
    let process_warnings: usize = per_run_process.iter().map(|p| p.warn_violations).sum();
    // The graded score of the concatenated trace: any block zeroes it, each
    // warn costs 0.1 (floored at 0), the same schedule as the per-trace scorer.
    let graded_process_score = if process_ok {
        (1.0 - process_warnings as f64 * 0.1).max(0.0)
    } else {
        0.0
    };

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

    // Mandate adherence: the pooled track must respect the whole-track cap and
    // every run must respect the per-run cap. Both default to 1.0, under which
    // the check is the pooled one the benchmark always ran.
    let mdd = max_drawdown(&pooled);
    let worst_run_drawdown = sub
        .runs
        .iter()
        .map(|r| max_drawdown(&r.returns))
        .fold(0.0, f64::max);
    let mandate_ok =
        mdd <= cfg.mandate.max_drawdown && worst_run_drawdown <= cfg.mandate.max_run_drawdown;

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
    let dsr_ci = crate::significance::bootstrap_dsr_ci_against_null(
        &pooled,
        effective_n_trials,
        defl.null_mean_per_period,
        defl.sr_std,
        cfg.bootstrap_seed,
        cfg.n_boot,
        cfg.block_prob,
        cfg.dsr_ci_level,
    );

    // Economic rationality, elicited from the one choice a frozen submission
    // records: submitting this track out of the declared candidate set (see
    // [`crate::econrationality::elicit_revealed_selection`]). Reported, never
    // gating; `None` when the submission declared nothing comparable.
    let econ_choice = elicit_revealed_selection(&sub.candidates, &pooled);
    let (econ_rationality_score, econ_dominance_violations) = match econ_choice {
        Some(choice) => {
            let violations = usize::from(choice.is_dominated());
            (Some(rationality_score(&[choice])), Some(violations))
        }
        None => (None, None),
    };

    // Behavior-role attribution over the recorded runs (see
    // [`crate::roles::attribute_behavior_roles`]): which behavior class
    // (clean-active, idle, warned, block-violating) is load-bearing for the
    // pooled result. Reported, never gating; empty when not estimable.
    let role_contributions = attribute_behavior_roles(&sub.runs);

    let rank_eligible =
        dsr >= cfg.dsr_bar && passed_k && process_ok && bootstrap_p < cfg.alpha && mandate_ok;
    let composite = if rank_eligible { dsr } else { 0.0 };

    // The declared verdict, if any: the same predicate as `rank_eligible` with
    // only the pass^k question exchanged (series and aggregation from the
    // resolved verdict) and, for a drawdown-capped verdict, the declared per-run
    // bound added. `dsr`, `bootstrap_p`, `process_ok` and `mandate_ok` are the
    // values computed above, on raw returns, so a declaration cannot move them.
    let declared = declared.map(|d| {
        let verdict = MandateVerdict::of(d.mandate);
        let verdict_cfg = ScoreConfig {
            pass_mode: verdict.pass_mode(),
            benchmark_agent_id: verdict
                .benchmark_id()
                .map_or_else(|| cfg.benchmark_agent_id.clone(), str::to_string),
            ..cfg.clone()
        };
        let per_run = per_run_passes(sub, d.benchmark, &verdict_cfg);
        let declared_passed_k = pass_k(&per_run, verdict_cfg.pass_mode);
        let eligible = dsr >= cfg.dsr_bar
            && declared_passed_k
            && process_ok
            && bootstrap_p < cfg.alpha
            && mandate_ok
            && verdict.drawdown_bound_holds(worst_run_drawdown);
        (d.mandate.clone(), verdict, declared_passed_k, eligible)
    });
    let (declared_mandate, verdict_applied, declared_passed_k, declared_mandate_eligible) =
        match declared {
            Some((m, v, p, e)) => (Some(m), Some(v), Some(p), Some(e)),
            None => (None, None, None, None),
        };

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
        worst_run_drawdown,
        turnover,
        pareto_optimal: false,
        step_down_significant: false,
        confidence_weighted_return,
        cost,
        return_per_cost,
        field_spa_p: 1.0,
        field_spa_consistent_p: 1.0,
        field_significance_benchmark: "unscored".to_string(),
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
        trials_sr_std_annualized_equivalent: defl.sr_std * cfg.periods_per_year.sqrt(),
        deflation_bar_per_period,
        deflation_bar_annualized_equivalent: deflation_bar_per_period * cfg.periods_per_year.sqrt(),
        deflation_null_mean_per_period: defl.null_mean_per_period,
        pooled_observations: pooled.len(),
        trials_sr_std_source: defl.source,
        runs_submitted: sub.runs.len(),
        runs_scored: sub.runs.len(),
        process_score: graded_process_score,
        process_warnings,
        econ_rationality_score,
        econ_dominance_violations,
        role_contributions,
        declared_mandate,
        verdict_applied,
        declared_passed_k,
        declared_mandate_eligible,
        declared_mandate_ordinal: None,
    }
}

/// Returns the temporally ordered pooled track used by PSR, DSR, bootstrap and
/// all pooled diagnostics. Runs are window-major. With `seeds_per_window > 1`,
/// aligned seed executions are Monte-Carlo replicates of one window and are
/// averaged per bar before concatenation. Thus eight executions of a 409-bar
/// window contribute 409 market observations, not 3,272 pseudo-independent
/// ones. A malformed final block or unequal return lengths is rejected loudly:
/// silently truncating it would conceal a misaligned market-time axis.
pub fn pooled_returns(sub: &AgentSubmission, seeds_per_window: usize) -> Vec<f64> {
    let width = seeds_per_window.max(1);
    if width == 1 {
        return sub
            .runs
            .iter()
            .flat_map(|r| r.returns.iter().copied())
            .collect();
    }
    assert!(
        sub.runs.len().is_multiple_of(width),
        "{} runs cannot form complete {}-execution window blocks",
        sub.runs.len(),
        width
    );
    sub.runs
        .chunks_exact(width)
        .flat_map(|replicates| {
            let len = replicates.first().map_or(0, |r| r.returns.len());
            assert!(
                replicates.iter().all(|r| r.returns.len() == len),
                "execution replicates in one window must have equal return lengths"
            );
            (0..len).map(move |t| {
                replicates.iter().map(|r| r.returns[t]).sum::<f64>() / replicates.len() as f64
            })
        })
        .collect()
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
///
/// With `dedup_clones` the qualifying streams are first partitioned by
/// [`clone_clusters`] at [`CLONE_COLLAPSE_COSINE`], and each cluster votes once
/// with its median Sharpe (the lower middle of the sorted cluster), so a
/// sock-puppet flood of near-duplicate streams counts as the one strategy it
/// is. The partition and the median are functions of the streams alone, so the
/// result is order-independent; a field with no clones is all singletons and
/// measures byte for byte as it would without the collapse.
fn measured_trials_sr_std(
    pooled: &[Vec<f64>],
    min_field: usize,
    dedup_clones: bool,
) -> Option<f64> {
    let qualifying: Vec<(&[f64], f64)> = pooled
        .iter()
        .filter(|p| p.len() >= 2)
        .map(|p| (p.as_slice(), sharpe_ratio(p)))
        .filter(|(_, sr)| sr.is_finite())
        .collect();
    let mut sharpes: Vec<f64> = if dedup_clones {
        let streams: Vec<Vec<f64>> = qualifying.iter().map(|(p, _)| p.to_vec()).collect();
        clone_clusters(&streams, CLONE_COLLAPSE_COSINE, false)
            .iter()
            .map(|members| {
                let mut cluster: Vec<f64> = members.iter().map(|&i| qualifying[i].1).collect();
                cluster.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
                cluster[(cluster.len() - 1) / 2]
            })
            .collect()
    } else {
        qualifying.iter().map(|(_, sr)| *sr).collect()
    };
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
/// - **Clone collapse** (`cfg.dedup_clones_for_measured_sr_std`, default on):
///   near-duplicate streams (`|cosine| >= CLONE_COLLAPSE_COSINE`) vote once
///   on that measurement, so a sock-puppet flood cannot shrink the dispersion and
///   lower the bar. Clones are still scored and still appear on the board.
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
    rank_declared(subs, &MandateDeclarations::new(), cfg)
}

/// [`rank`] with each agent's declared mandate (see [`DeclaredMandate`]).
///
/// The board is the one [`rank`] produces, byte for byte: every agent is scored
/// under the host verdict (`cfg.pass_mode`), `rank_eligible` decides the sort
/// and `rank_ordinal` counts host-eligible agents only. A declaration adds a
/// second, labeled verdict to the agent's row (`verdict_applied`,
/// `declared_passed_k`, `declared_mandate_eligible`) and never touches the
/// first. The rule is:
///
/// > A declaration selects which reliability question pass^k asks of the agent
/// > and, for a drawdown-capped mandate, adds a per-run bound; every other gate
/// > is the same test on the same raw returns. Eligibility is reported per
/// > verdict; the board ranks under the host verdict; declared eligibility is
/// > an additional column, ordered only within the agent's mandate class.
///
/// The mandate class of an agent is its resolved [`MandateVerdict`]
/// (`OutperformBuyAndHold` and `RelativeTo { "buy-and-hold" }` share one). Within a
/// class, declared-eligible agents are ordered by deflated Sharpe, then agent
/// id, and get a 1-based `declared_mandate_ordinal`; across classes, and
/// against the host board, nothing is compared, because the columns answer
/// different questions. A relative verdict's benchmark is looked up by id in
/// the field being ranked, in the same window-major cells; a field without it
/// fails that agent's declared verdict closed. With an empty map this is
/// [`rank`].
pub fn rank_declared(
    subs: &[AgentSubmission],
    declarations: &MandateDeclarations,
    cfg: &ScoreConfig,
) -> Vec<CompositeScore> {
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

    // A ranked field exposes at least one tried strategy per distinct entry.
    // Refuse a host configuration that understates that observable search
    // footprint. `score_agent` has no field context and therefore retains the
    // configured value; ranking uses max(configured N, field size), then adds
    // each entrant's own declared private trials in `score_agent_with`.
    let mut rank_cfg = cfg.clone();
    rank_cfg.n_trials = cfg
        .n_trials
        .max(u32::try_from(field.len()).unwrap_or(u32::MAX));

    // Pooled returns per agent + an equal-weight market proxy (the field average),
    // used for performance attribution: alpha (skill) vs beta (market exposure).
    let pooled: Vec<Vec<f64>> = field
        .iter()
        .map(|s| pooled_returns(s, rank_cfg.execution_seeds_per_window))
        .collect();
    let min_len = pooled.iter().map(Vec::len).min().unwrap_or(0);
    let n_agents = pooled.len().max(1) as f64;
    let market: Vec<f64> = (0..min_len)
        .map(|i| pooled.iter().map(|p| p[i]).sum::<f64>() / n_agents)
        .collect();

    // The RC/SPA null is predictive superiority against one fixed benchmark,
    // not superiority against the candidates' equal-weight average (which is an
    // attribution device only). Prefer the explicitly named benchmark's aligned
    // stream; with no such entrant, cash (zero return) is the declared null.
    let (significance_benchmark, significance_benchmark_label): (Vec<f64>, String) = field
        .iter()
        .position(|s| s.agent_id == rank_cfg.benchmark_agent_id)
        .filter(|&idx| pooled[idx].len() >= min_len)
        .map(|idx| {
            (
                pooled[idx][..min_len].to_vec(),
                rank_cfg.benchmark_agent_id.clone(),
            )
        })
        .unwrap_or_else(|| (vec![0.0; min_len], "zero-return-cash".to_string()));

    // Measured deflation: with enough agents the field's own Sharpe dispersion
    // replaces the configured prior. The measured value is a dispersion of
    // per-period Sharpes, so it goes in as-is; only the configured prior is
    // annualized and needs converting. A small field scores exactly as
    // `score_agent` would, so the configured path stays byte-identical.
    let defl = measured_trials_sr_std(
        &pooled,
        cfg.min_field_for_measured_sr_std,
        cfg.dedup_clones_for_measured_sr_std,
    )
    .map_or_else(
        || Deflation::configured(cfg),
        |sr_std| Deflation::measured(sr_std, cfg),
    );

    // The relative verdict's benchmark is a member of this same (restricted)
    // field, so its run `i` is every other agent's cell `i`. Looked up only
    // under that mode: the default path never touches it.
    let benchmark = match cfg.pass_mode {
        PassMode::RelativeToBenchmark => {
            field.iter().find(|s| s.agent_id == cfg.benchmark_agent_id)
        }
        PassMode::All | PassMode::Any | PassMode::AtLeast(_) => None,
    };

    let mut scores: Vec<CompositeScore> = field
        .iter()
        .enumerate()
        .map(|(idx, s)| {
            // The declared verdict's benchmark is resolved in the same
            // restricted field, so its cells line up like the host verdict's.
            let declared = declarations
                .get(&s.agent_id)
                .map(|mandate| ResolvedDeclaration {
                    mandate,
                    benchmark: MandateVerdict::of(mandate)
                        .benchmark_id()
                        .and_then(|id| field.iter().find(|b| b.agent_id == id)),
                });
            let mut cs = score_agent_with(s, &rank_cfg, defl, benchmark, declared);
            cs.runs_submitted = subs[idx].runs.len();
            cs.field_significance_benchmark = significance_benchmark_label.clone();
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
                    .zip(significance_benchmark.iter())
                    .map(|(a, b)| a - b)
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

    // Within a tie band the DSR ordering is statistical noise, so a cleaner
    // process breaks the tie: eligible band members reorder by graded process
    // score (descending) ahead of the DSR order they arrived in. This is
    // **ordering within statistical ties only, never eligibility**: warn
    // events are real information, but not calibrated enough to gate, and
    // block-severity gating is untouched. Band membership itself was fixed
    // above from the DSR-sorted walk; the stable sort keeps the DSR order for
    // members with equal process scores, so an all-clean board is unchanged.
    // Eligible agents sort first and bands are assigned in one forward walk,
    // so each band is a contiguous slice.
    let mut i = 0;
    while i < scores.len() {
        if !scores[i].rank_eligible {
            i += 1;
            continue;
        }
        let band = scores[i].tie_group;
        let mut j = i + 1;
        while j < scores.len() && scores[j].rank_eligible && scores[j].tie_group == band {
            j += 1;
        }
        scores[i..j].sort_by(|a, b| {
            b.process_score
                .partial_cmp(&a.process_score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        i = j;
    }

    // 1-based ordinal rank among eligible agents (the scale-invariant rank mode,
    // assigned in final sorted order). Ineligible agents keep ordinal 0.
    let mut ord = 0usize;
    for cs in scores.iter_mut() {
        if cs.rank_eligible {
            ord += 1;
            cs.rank_ordinal = ord;
        }
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

    // Declared-mandate ordinal: within each mandate class (one resolved
    // verdict), the declared-eligible agents ordered by deflated Sharpe, then
    // id. The board order above is untouched.
    let mut classes: Vec<(&MandateVerdict, usize)> = scores
        .iter()
        .enumerate()
        .filter(|(_, s)| s.declared_mandate_eligible == Some(true))
        .filter_map(|(i, s)| s.verdict_applied.as_ref().map(|v| (v, i)))
        .collect();
    classes.sort_by(|(va, ia), (vb, ib)| {
        let (a, b) = (&scores[*ia], &scores[*ib]);
        class_key(va)
            .cmp(&class_key(vb))
            .then(
                b.deflated_sharpe
                    .partial_cmp(&a.deflated_sharpe)
                    .unwrap_or(std::cmp::Ordering::Equal),
            )
            .then(a.agent_id.cmp(&b.agent_id))
    });
    let mut ordinals: Vec<(usize, usize)> = Vec::with_capacity(classes.len());
    let mut prev: Option<&MandateVerdict> = None;
    let mut ord = 0usize;
    for (v, i) in classes {
        if prev != Some(v) {
            ord = 0;
        }
        ord += 1;
        ordinals.push((i, ord));
        prev = Some(v);
    }
    for (i, ord) in ordinals {
        scores[i].declared_mandate_ordinal = Some(ord);
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
    use crate::deflated_sharpe::{deflated_sharpe_ratio, expected_max_sharpe};
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
    fn ranked_field_size_is_an_observable_trial_floor() {
        let field: Vec<_> = (0..8)
            .map(|i| {
                agent(
                    &format!("agent-{i}"),
                    vec![run(0.002 + f64::from(i) * 0.00001, 0.0005, 60)],
                )
            })
            .collect();
        let cfg = ScoreConfig {
            n_trials: 1,
            min_field_for_measured_sr_std: usize::MAX,
            ..ScoreConfig::default()
        };
        let board = rank(&field, &cfg);
        assert!(board.iter().all(|score| score.effective_n_trials == 8));

        let standalone = score_agent(&field[0], &cfg);
        assert_eq!(standalone.effective_n_trials, 1);
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
                // Each agent gets its own phase: five drifts on one shared
                // wiggle would be a single clone cluster under the collapse.
                let phase = 0.6 * i as f64;
                agent(
                    &format!("a{i}"),
                    (0..5)
                        .map(|_| {
                            let mut r = run(m, 0.003, 60);
                            r.returns = (0..60)
                                .map(|t| m + 0.003 * (t as f64 * 0.7 + phase).sin())
                                .collect();
                            r
                        })
                        .collect(),
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

    /// A Sybil need not submit literal clones to compress a field measurement:
    /// many genuinely distinct streams can be engineered to have almost the
    /// same Sharpe. The precommitted floor makes that construction unable to
    /// relax deflation, while preserving the measured path when it is stricter.
    #[test]
    fn measured_dispersion_cannot_fall_below_the_precommitted_floor() {
        let cfg = ScoreConfig {
            // Disable clone collapse: this is deliberately a *dissimilar*
            // stream construction, not the near-copy attack covered elsewhere.
            dedup_clones_for_measured_sr_std: false,
            min_measured_trials_sr_std: 0.5,
            ..ScoreConfig::default()
        };
        let field: Vec<AgentSubmission> = (0..5)
            .map(|i| {
                // Cyclic shifts have exactly the same mean and variance, but
                // point in different directions and are not clone-connected.
                let phase = i as f64 * 36.0;
                let returns = (0..180)
                    .map(|t| {
                        0.001 + 0.0004 * (std::f64::consts::TAU * (t as f64 + phase) / 180.0).sin()
                    })
                    .collect();
                agent(
                    &format!("low-dispersion-{i}"),
                    vec![Run {
                        returns,
                        ..Run::default()
                    }],
                )
            })
            .collect();
        let board = rank(&field, &cfg);
        let floor = per_period_sr_std(&cfg);
        assert!(board.iter().all(|s| {
            s.trials_sr_std_source == TrialsSrStdSource::MeasuredFloored
                && s.trials_sr_std.to_bits() == floor.to_bits()
        }));
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

    #[test]
    fn execution_replicates_are_averaged_not_counted_as_extra_market_time() {
        // Two 3-bar windows, each run twice under stochastic execution. The
        // pooled inference track must retain six market bars rather than twelve
        // pseudo-independent seed outcomes.
        let sub = agent(
            "replicated",
            vec![
                run(0.01, 0.0, 3),
                run(0.03, 0.0, 3),
                run(-0.02, 0.0, 3),
                run(0.00, 0.0, 3),
            ],
        );
        let cfg = ScoreConfig {
            execution_seeds_per_window: 2,
            ..ScoreConfig::default()
        };
        let pooled = pooled_returns(&sub, cfg.execution_seeds_per_window);
        assert_eq!(pooled.len(), 6);
        assert_eq!(pooled, vec![0.02, 0.02, 0.02, -0.01, -0.01, -0.01]);
        let score = score_agent(&sub, &cfg);
        assert_eq!(score.pooled_observations, 6);
        assert_eq!(
            score.psr.to_bits(),
            probabilistic_sharpe_ratio(&pooled, 0.0).to_bits(),
            "PSR must see the de-duplicated time axis"
        );
    }

    #[test]
    fn one_execution_per_window_preserves_legacy_pooling() {
        let sub = agent("single", vec![run(0.01, 0.002, 4), run(-0.01, 0.001, 5)]);
        assert_eq!(
            pooled_returns(&sub, 1),
            pooled_of(&sub),
            "the default is byte-compatible for one execution per window"
        );
    }

    #[test]
    #[should_panic(expected = "cannot form complete")]
    fn incomplete_execution_replicate_block_is_rejected() {
        let sub = agent(
            "bad",
            vec![
                run(0.01, 0.001, 3),
                run(0.01, 0.001, 3),
                run(0.01, 0.001, 3),
            ],
        );
        let _ = pooled_returns(&sub, 2);
    }

    #[test]
    #[should_panic(expected = "equal return lengths")]
    fn unequal_execution_replicate_lengths_are_rejected() {
        let sub = agent("bad", vec![run(0.01, 0.001, 3), run(0.01, 0.001, 4)]);
        let _ = pooled_returns(&sub, 2);
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

    /// Near-clone streams vote once on the measured dispersion: a field of five
    /// distinct agents plus twenty leveraged copies of one of them measures
    /// exactly what the five distinct agents measure, the copies are still on
    /// the board, and submission order cannot move the result. A field with no
    /// clones measures byte for byte as it does with the collapse off.
    #[test]
    fn clone_clusters_vote_once_on_the_measured_sr_std() {
        let cfg = ScoreConfig::default();
        let distinct: Vec<AgentSubmission> = (0..5)
            .map(|i| {
                agent(
                    &format!("a{i}"),
                    vec![Run {
                        returns: gaussian_like(120, 0.0002 + 0.0003 * i as f64, 0.003, 11 + i),
                        trace: Trace::default(),
                        confidences: vec![],
                        outcomes: vec![],
                        cost: 0.0,
                    }],
                )
            })
            .collect();
        let base = rank(&distinct, &cfg);
        assert_eq!(base[0].trials_sr_std_source, TrialsSrStdSource::Measured);
        let off = rank(
            &distinct,
            &ScoreConfig {
                dedup_clones_for_measured_sr_std: false,
                ..cfg.clone()
            },
        );
        assert_eq!(base, off, "no clones: the collapse is the identity");

        let mut flooded = distinct.clone();
        for k in 0..20 {
            let mut copy = distinct[2].clone();
            copy.agent_id = format!("copy{k:02}");
            let lever = 1.0 + 0.05 * k as f64;
            copy.runs[0].returns.iter_mut().for_each(|r| *r *= lever);
            flooded.push(copy);
        }
        let board = rank(&flooded, &cfg);
        assert_eq!(board.len(), 25, "clones are still scored");
        assert_eq!(
            board[0].trials_sr_std.to_bits(),
            base[0].trials_sr_std.to_bits(),
            "twenty copies of one stream are one vote"
        );
        let mut shuffled = flooded.clone();
        shuffled.rotate_left(7);
        shuffled.swap(0, 12);
        let again = rank(&shuffled, &cfg);
        assert_eq!(
            again[0].trials_sr_std.to_bits(),
            base[0].trials_sr_std.to_bits()
        );

        let exposed = rank(
            &flooded,
            &ScoreConfig {
                dedup_clones_for_measured_sr_std: false,
                ..cfg.clone()
            },
        );
        assert!(
            exposed[0].trials_sr_std < base[0].trials_sr_std,
            "without the collapse the copies shrink the dispersion"
        );
    }

    /// The measured path measures a dispersion of per-period Sharpes, so it is
    /// reported and used as-is: `periods_per_year` must not touch it.
    #[test]
    fn measured_sr_std_is_never_reconverted() {
        let field: Vec<AgentSubmission> = (0..5)
            .map(|i| {
                let m = 0.0002 + 0.0003 * i as f64;
                // Each agent gets its own phase: five drifts on one shared
                // wiggle would be a single clone cluster under the collapse.
                let phase = 0.6 * i as f64;
                agent(
                    &format!("a{i}"),
                    (0..5)
                        .map(|_| {
                            let mut r = run(m, 0.003, 60);
                            r.returns = (0..60)
                                .map(|t| m + 0.003 * (t as f64 * 0.7 + phase).sin())
                                .collect();
                            r
                        })
                        .collect(),
                )
            })
            .collect();
        let mut sharpes: Vec<f64> = field.iter().map(|a| sharpe_ratio(&pooled_of(a))).collect();
        sharpes.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let raw_measured = std_dev(&sharpes);

        for ppy in [1.0, 252.0, 8760.0] {
            // This test isolates unit conversion of a measured value. Disable
            // the separate field-integrity floor, which is covered by its own
            // regression test above.
            let cfg = ScoreConfig {
                min_measured_trials_sr_std: 0.0,
                ..ScoreConfig::for_periods_per_year(ppy)
            };
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
    /// A mixed field: two agents that clear every run, one that fails a single
    /// run. A `Run` that fails the per-run PSR bar is the point of pass^k.
    fn field_with_one_single_run_failure() -> Vec<AgentSubmission> {
        let clean = |id: &str| agent(id, (0..6).map(|_| run(0.002, 0.0005, 60)).collect());
        let mut runs: Vec<Run> = (0..5).map(|_| run(0.002, 0.0005, 60)).collect();
        runs.push(run(-0.002, 0.0005, 60));
        vec![clean("a"), agent("one_bad_run", runs), clean("b")]
    }

    /// `pass_mode` defaults to `All`, and under it the eligibility vector of a
    /// mixed field is exactly what the hard-coded `PassMode::All` produced: an
    /// agent that fails a single run is ineligible, every clean agent is not.
    #[test]
    fn default_pass_mode_is_all_and_matches_the_old_gate() {
        let cfg = ScoreConfig::default();
        assert_eq!(cfg.pass_mode, PassMode::All);
        assert_eq!(cfg.mandate.max_run_drawdown, 1.0);

        let field = field_with_one_single_run_failure();
        let board = rank(&field, &cfg);
        let benchmark = per_run_psr_benchmark(&cfg);
        for s in &board {
            let sub = field.iter().find(|a| a.agent_id == s.agent_id).unwrap();
            let old_per_run: Vec<bool> = sub
                .runs
                .iter()
                .map(|r| probabilistic_sharpe_ratio(&r.returns, benchmark) >= cfg.per_run_psr_bar)
                .collect();
            let old_passed_k = pass_k(&old_per_run, PassMode::All);
            assert_eq!(s.passed_k, old_passed_k, "{}", s.agent_id);
            let old_eligible = s.deflated_sharpe >= cfg.dsr_bar
                && old_passed_k
                && s.process_ok
                && s.bootstrap_p < cfg.alpha
                && s.max_drawdown <= cfg.mandate.max_drawdown;
            assert_eq!(s.rank_eligible, old_eligible, "{}", s.agent_id);
        }
        let get = |id: &str| board.iter().find(|s| s.agent_id == id).unwrap();
        assert!(get("a").rank_eligible && get("b").rank_eligible);
        assert!(!get("one_bad_run").rank_eligible);
        assert!(!get("one_bad_run").passed_k);
    }

    /// A regime-dependent edge: profitable on five of six runs, losing on one.
    /// `All` refuses it (not profitable in every regime); `Any` admits it once it
    /// also clears the DSR, the bootstrap, the process check and both drawdown
    /// bounds, which is the "edge tested on the pooled track" half of the preset.
    #[test]
    fn any_mode_admits_a_regime_dependent_edge_the_all_mode_rejects() {
        let mut runs: Vec<Run> = (0..5).map(|_| run(0.003, 0.0005, 60)).collect();
        runs.push(run(-0.001, 0.0005, 60));
        let sub = agent("regime_edge", runs);

        let all = score_agent(&sub, &ScoreConfig::default());
        assert!(!all.passed_k && !all.rank_eligible, "{all:?}");

        let any = score_agent(&sub, &ScoreConfig::reliability_never_catastrophic(0.20));
        // Every other gate holds on its own, so pass^k is the only thing that moved.
        assert!(any.deflated_sharpe >= 0.95);
        assert!(any.bootstrap_p < 0.05 && any.process_ok && any.mandate_ok);
        assert!(any.worst_run_drawdown < 0.20 && any.max_drawdown < 0.20);
        assert!(any.passed_k && any.rank_eligible, "{any:?}");
    }

    /// The reason the per-run bound exists: a track whose pooled drawdown is
    /// inside the pooled cap can still contain one run that draws down past the
    /// per-run cap, because a strong earlier run lifts the pooled peak. The
    /// pooled bound misses it; the per-run bound must not.
    #[test]
    fn per_run_drawdown_bound_rejects_one_catastrophic_run_that_the_pooled_bound_misses() {
        // Five strong runs, then a run that falls 14% from its own start. The
        // pooled cap is a whole-track budget of 20%, which this track respects;
        // the per-run cap says no single regime may lose more than 10%, which
        // it does not.
        let mut runs: Vec<Run> = (0..5).map(|_| run(0.004, 0.0005, 60)).collect();
        let mut crash = run(0.004, 0.0005, 60);
        for r in &mut crash.returns[10..15] {
            *r = -0.03;
        }
        runs.push(crash);
        let sub = agent("one_blowup", runs);

        let loose_pooled = ScoreConfig {
            mandate: Mandate {
                max_drawdown: 0.20,
                max_run_drawdown: 1.0,
            },
            pass_mode: PassMode::Any,
            ..ScoreConfig::default()
        };
        let pooled_only = score_agent(&sub, &loose_pooled);
        assert!(pooled_only.max_drawdown <= 0.20, "{pooled_only:?}");
        assert!(
            pooled_only.mandate_ok && pooled_only.rank_eligible,
            "{pooled_only:?}"
        );

        let mut preset = ScoreConfig::reliability_never_catastrophic(0.10);
        preset.mandate.max_drawdown = 0.20;
        let bounded = score_agent(&sub, &preset);
        assert!(bounded.worst_run_drawdown > 0.10, "{bounded:?}");
        assert!(bounded.max_drawdown <= 0.20, "the pooled bound still holds");
        assert!(bounded.passed_k, "pass^k is not what rejects it");
        assert!(!bounded.mandate_ok && !bounded.rank_eligible, "{bounded:?}");
    }

    /// Eligibility under the preset is a superset of eligibility under the
    /// default on the same field: the preset relaxes pass^k, keeps every other
    /// gate, and the per-run bound is chosen loose enough that a default-eligible
    /// agent (which clears PSR 0.90 on every run) cannot trip it.
    ///
    /// Every agent submits six runs so the shared-cell restriction is the
    /// identity, and the field stays below the measured-deflation floor so the
    /// two boards differ only in the gates under test.
    #[test]
    fn never_catastrophic_preset_is_weaker_than_the_default() {
        let mut field = field_with_one_single_run_failure();
        field.pop();
        let mut runs: Vec<Run> = (0..5).map(|_| run(0.004, 0.0005, 60)).collect();
        let mut crash = run(0.004, 0.0005, 60);
        crash.returns[10] = -0.30;
        runs.push(crash);
        field.push(agent("blowup", runs));
        let mut lucky = vec![run(0.02, 0.002, 60)];
        lucky.extend((0..5).map(|_| run(0.0, 0.003, 60)));
        field.push(agent("lucky", lucky));
        assert!(field.iter().all(|a| a.runs.len() == 6));
        assert!(field.len() < ScoreConfig::default().min_field_for_measured_sr_std);

        let default = rank(&field, &ScoreConfig::default());
        let preset = rank(&field, &ScoreConfig::reliability_never_catastrophic(0.20));
        let eligible = |board: &[CompositeScore]| -> Vec<String> {
            let mut v: Vec<String> = board
                .iter()
                .filter(|s| s.rank_eligible)
                .map(|s| s.agent_id.clone())
                .collect();
            v.sort();
            v
        };
        let (d, p) = (eligible(&default), eligible(&preset));
        assert_eq!(d, vec!["a".to_string()], "{default:?}");
        assert!(
            d.iter().all(|id| p.contains(id)),
            "default {d:?} not within preset {p:?}"
        );
        assert!(p.contains(&"one_bad_run".to_string()) && !d.contains(&"one_bad_run".to_string()));
        assert!(
            !p.contains(&"blowup".to_string()),
            "a blow-up is refused under both"
        );
        // The cost of the weaker claim, stated rather than hidden: the one-hot-run
        // agent was refused by the default through pass^k alone, and its pooled
        // track clears the DSR and the bootstrap, so `Any` admits it. Reliability
        // is asked of the loss side only under this preset.
        assert!(
            p.contains(&"lucky".to_string()) && !d.contains(&"lucky".to_string()),
            "{preset:?}"
        );
    }

    /// A field for the relative verdict: a benchmark that is profitable in five
    /// windows and loses in a sixth (a bear window), and agents defined by how
    /// they sit against it cell by cell. Six runs each, so the shared-cell
    /// restriction is the identity; five agents, so deflation stays configured.
    fn relative_field() -> Vec<AgentSubmission> {
        let bench: Vec<Run> = (0..5)
            .map(|_| run(0.002, 0.0005, 60))
            .chain(std::iter::once(run(-0.003, 0.0005, 60)))
            .collect();
        // Beats the benchmark by a steady margin in every cell, including the
        // bear window, where it still loses money.
        let beats: Vec<Run> = bench
            .iter()
            .map(|b| {
                let mut r = b.clone();
                for (i, x) in r.returns.iter_mut().enumerate() {
                    *x += 0.001 + 0.0002 * (i as f64 * 0.7).sin();
                }
                r
            })
            .collect();
        // Beats it in five cells and trails it in the bear window.
        let mut five_of_six = beats.clone();
        for (i, x) in five_of_six[5].returns.iter_mut().enumerate() {
            *x = bench[5].returns[i] - 0.001;
        }
        // Profitable in every cell yet below the benchmark in every cell.
        let trails: Vec<Run> = bench
            .iter()
            .map(|b| {
                let mut r = b.clone();
                for x in r.returns.iter_mut() {
                    *x = x.abs() * 0.5 + 0.0005;
                }
                r
            })
            .collect();
        vec![
            agent("buy-and-hold", bench.clone()),
            agent("beats", beats),
            agent("five-of-six", five_of_six),
            agent("trails", trails),
            agent("clone", bench),
        ]
    }

    /// The default pass^k verdict does not read the benchmark id, but the
    /// field-wide RC/SPA tests do: they require a named fixed null rather than
    /// silently comparing agents to their own field average.
    #[test]
    fn fixed_significance_benchmark_is_separate_from_default_pass_verdict() {
        let field = relative_field();
        let default = rank(&field, &ScoreConfig::default());
        let renamed = rank(
            &field,
            &ScoreConfig {
                benchmark_agent_id: "no-such-agent".to_string(),
                ..ScoreConfig::default()
            },
        );
        assert_eq!(
            default.iter().map(|s| s.rank_eligible).collect::<Vec<_>>(),
            renamed.iter().map(|s| s.rank_eligible).collect::<Vec<_>>(),
            "changing the significance null must not change default pass^k eligibility"
        );
        assert!(default
            .iter()
            .all(|s| s.field_significance_benchmark == "buy-and-hold"));
        assert!(renamed
            .iter()
            .all(|s| s.field_significance_benchmark == "zero-return-cash"));
        assert_eq!(
            serde_json::from_str::<ScoreConfig>("{\"n_trials\":1,\"trials_sr_std\":0.5,\"dsr_bar\":0.9,\"per_run_psr_bar\":0.9,\"alpha\":0.05,\"bootstrap_seed\":1,\"n_boot\":10,\"block_prob\":0.1}")
                .unwrap()
                .benchmark_agent_id,
            "buy-and-hold"
        );
    }

    /// The relative verdict asks a different question of each cell, and the two
    /// verdicts disagree in both directions: an agent that beats the benchmark
    /// in every window while losing money in the bear one fails the default and
    /// passes relative; an agent profitable everywhere but below the benchmark
    /// everywhere passes the default and fails relative. The benchmark itself
    /// and a clone of it fail relative on the zero-excess rule, and one trailing
    /// cell is enough to fail.
    #[test]
    fn relative_verdict_tests_excess_over_the_same_cell() {
        let field = relative_field();
        let cfg = ScoreConfig::relative_to_benchmark("buy-and-hold");
        let default = rank(&field, &ScoreConfig::default());
        let relative = rank(&field, &cfg);
        let passed = |board: &[CompositeScore], id: &str| {
            board.iter().find(|s| s.agent_id == id).unwrap().passed_k
        };
        assert!(!passed(&default, "beats") && passed(&relative, "beats"));
        assert!(passed(&default, "trails") && !passed(&relative, "trails"));
        // The benchmark has a bear window, so it fails the default too; under
        // relative it and its clone fail for a different reason, zero excess.
        assert!(!passed(&default, "buy-and-hold") && !passed(&relative, "buy-and-hold"));
        assert!(!passed(&relative, "clone"));
        assert!(!passed(&relative, "five-of-six"));

        // The per-cell vector says which cells, not only whether all.
        let bench = &field[0];
        let five = per_run_passes(&field[2], Some(bench), &cfg);
        assert_eq!(five, vec![true, true, true, true, true, false]);
        let zero = per_run_passes(bench, Some(bench), &cfg);
        assert_eq!(zero, vec![false; 6]);
        // The zero-excess refusal is unconditional: even a bar of 0.5, which
        // `norm_cdf(0)` would reach, does not admit the benchmark.
        let lax = ScoreConfig {
            per_run_psr_bar: 0.5,
            ..cfg.clone()
        };
        assert_eq!(per_run_passes(bench, Some(bench), &lax), vec![false; 6]);

        // Only pass^k moves: every other statistic of every agent is the
        // default's, because it is computed on the agent's own raw returns.
        for d in &default {
            let r = relative.iter().find(|s| s.agent_id == d.agent_id).unwrap();
            assert_eq!(d.deflated_sharpe.to_bits(), r.deflated_sharpe.to_bits());
            assert_eq!(d.psr.to_bits(), r.psr.to_bits());
            assert_eq!(d.bootstrap_p.to_bits(), r.bootstrap_p.to_bits());
            assert_eq!(
                d.worst_run_drawdown.to_bits(),
                r.worst_run_drawdown.to_bits()
            );
            assert_eq!(d.process_ok, r.process_ok);
        }
    }

    /// No benchmark, no relative verdict: a field without the named agent, a
    /// single-agent `score_agent`, and a misaligned benchmark all fail every
    /// run rather than falling back to the absolute test.
    #[test]
    fn relative_verdict_without_a_benchmark_fails_every_run() {
        let field = relative_field();
        let cfg = ScoreConfig::relative_to_benchmark("absent");
        assert!(rank(&field, &cfg).iter().all(|s| !s.passed_k));
        let alone = score_agent(
            &field[1],
            &ScoreConfig::relative_to_benchmark("buy-and-hold"),
        );
        assert!(!alone.passed_k);
        let mut short = field[0].clone();
        short.runs[2].returns.pop();
        let v = per_run_passes(
            &field[1],
            Some(&short),
            &ScoreConfig::relative_to_benchmark("buy-and-hold"),
        );
        assert_eq!(v, vec![true, true, false, true, true, true]);
        let mut fewer = field[0].clone();
        fewer.runs.truncate(4);
        let v = per_run_passes(
            &field[1],
            Some(&fewer),
            &ScoreConfig::relative_to_benchmark("buy-and-hold"),
        );
        assert_eq!(v, vec![true, true, true, true, false, false]);
    }

    /// The preset changes exactly two fields of the default.
    #[test]
    fn relative_preset_differs_only_in_mode_and_benchmark_id() {
        let mut preset = serde_json::to_value(ScoreConfig::relative_to_benchmark("index")).unwrap();
        let default = serde_json::to_value(ScoreConfig::default()).unwrap();
        assert_eq!(preset["pass_mode"], "relative_to_benchmark");
        assert_eq!(preset["benchmark_agent_id"], "index");
        preset["pass_mode"] = default["pass_mode"].clone();
        preset["benchmark_agent_id"] = default["benchmark_agent_id"].clone();
        assert_eq!(preset, default);
    }

    /// Warn-severity events lower the reported graded process score and count,
    /// and change **nothing** about eligibility: the predicate the paper states
    /// (zero block-severity violations) is untouched by warns.
    #[test]
    fn warn_events_lower_process_score_and_count_but_not_eligibility() {
        let clean = agent("clean", (0..5).map(|_| run(0.002, 0.0005, 60)).collect());
        let mut warned_runs: Vec<Run> = (0..5).map(|_| run(0.002, 0.0005, 60)).collect();
        warned_runs[0]
            .trace
            .events
            .push(ProcessEvent::ConcentrationBreach);
        warned_runs[3]
            .trace
            .events
            .push(ProcessEvent::ConcentrationBreach);
        let warned = agent("warned", warned_runs);

        let cfg = ScoreConfig::default();
        let c = score_agent(&clean, &cfg);
        let w = score_agent(&warned, &cfg);

        assert_eq!(c.process_warnings, 0);
        assert_eq!(c.process_score, 1.0);
        assert_eq!(w.process_warnings, 2);
        assert!((w.process_score - 0.8).abs() < 1e-9);

        // Eligibility and every gate input are identical with and without warns.
        assert!(c.process_ok && w.process_ok);
        assert_eq!(c.rank_eligible, w.rank_eligible);
        assert!(w.rank_eligible);
        assert_eq!(c.deflated_sharpe.to_bits(), w.deflated_sharpe.to_bits());
        assert_eq!(c.composite.to_bits(), w.composite.to_bits());
    }

    /// A block-severity violation zeroes the graded scalar too (not just the gate).
    #[test]
    fn block_violation_zeroes_graded_process_score() {
        let mut runs: Vec<Run> = (0..3).map(|_| run(0.002, 0.0005, 60)).collect();
        runs[1].trace.events.push(ProcessEvent::DenylistBypass);
        runs[2].trace.events.push(ProcessEvent::ConcentrationBreach);
        let s = score_agent(&agent("blocked", runs), &ScoreConfig::default());
        assert_eq!(s.process_score, 0.0);
        assert_eq!(s.process_warnings, 1, "warns are still counted");
        assert!(!s.process_ok && !s.rank_eligible);
    }

    /// Within a DSR tie band, the cleaner process ranks first, regardless of
    /// submission order. This is ordering within statistical ties only: both
    /// agents stay eligible, tied, and in the same band.
    #[test]
    fn tie_band_orders_cleaner_process_first() {
        let cfg = ScoreConfig {
            n_trials: 2,
            trials_sr_std: 0.01,
            dsr_bar: 0.10,
            per_run_psr_bar: 0.05,
            alpha: 0.9,
            n_boot: 600,
            ..ScoreConfig::default()
        };
        let clean = agent("clean", (0..3).map(|_| run(0.01, 0.001, 60)).collect());
        let mut warned_runs: Vec<Run> = (0..3).map(|_| run(0.01, 0.001, 60)).collect();
        warned_runs[0]
            .trace
            .events
            .push(ProcessEvent::ConcentrationBreach);
        let warned = agent("warned", warned_runs);

        for field in [
            vec![clean.clone(), warned.clone()],
            vec![warned.clone(), clean.clone()],
        ] {
            let board = rank(&field, &cfg);
            assert_eq!(board[0].agent_id, "clean", "cleaner process leads the band");
            assert_eq!(board[1].agent_id, "warned");
            assert_eq!(board[0].rank_ordinal, 1);
            assert_eq!(board[1].rank_ordinal, 2);
            // Ordering within a statistical tie only, never eligibility.
            assert!(board[0].rank_eligible && board[1].rank_eligible);
            assert_eq!(board[0].tie_group, board[1].tie_group);
            assert!(board[0].dsr_tied && board[1].dsr_tied);
        }
    }

    /// The process tie-break never crosses bands: a separably stronger agent
    /// with a warn still ranks above a weaker clean agent in another band.
    #[test]
    fn process_tie_break_never_crosses_tie_bands() {
        let cfg = ScoreConfig {
            n_trials: 2,
            trials_sr_std: 0.01,
            dsr_bar: 0.10,
            per_run_psr_bar: 0.05,
            alpha: 0.9,
            n_boot: 600,
            ..ScoreConfig::default()
        };
        let mut strong_runs: Vec<Run> = (0..3).map(|_| run(0.01, 0.001, 60)).collect();
        strong_runs[0]
            .trace
            .events
            .push(ProcessEvent::ConcentrationBreach);
        let strong_warned = agent("strong_warned", strong_runs);
        let weak_clean = agent("weak_clean", (0..3).map(|_| run(0.001, 0.02, 60)).collect());

        let board = rank(&[weak_clean, strong_warned], &cfg);
        assert!(board.iter().all(|s| s.rank_eligible));
        assert_eq!(board[0].agent_id, "strong_warned");
        assert_ne!(board[0].tie_group, board[1].tie_group, "separable bands");
        assert!(board[0].process_score < board[1].process_score);
    }

    /// The econ-rationality axis is elicited from the declared candidate set and
    /// reported only.
    #[test]
    fn econ_rationality_reported_from_declared_candidates() {
        let cfg = ScoreConfig::default();
        let runs: Vec<Run> = (0..5).map(|_| run(0.002, 0.0005, 60)).collect();

        // No declared candidates: nothing elicitable.
        let none = score_agent(&agent("plain", runs.clone()), &cfg);
        assert!(none.econ_rationality_score.is_none());
        assert!(none.econ_dominance_violations.is_none());

        // A declared candidate with a strictly higher Sharpe than the submitted
        // pooled track: the recorded selection is dominated.
        let mut dominated = agent("dominated", runs.clone());
        dominated.candidates = vec![(0..300)
            .map(|i| 0.01 + 0.0005 * (i as f64 * 0.7).sin())
            .collect()];
        let d = score_agent(&dominated, &cfg);
        assert_eq!(d.econ_rationality_score, Some(0.0));
        assert_eq!(d.econ_dominance_violations, Some(1));

        // Submitting the best of one's declared set is rational.
        let mut rational = agent("rational", runs);
        rational.candidates = vec![(0..300)
            .map(|i| 0.0001 + 0.0005 * (i as f64 * 0.7).sin())
            .collect()];
        let r = score_agent(&rational, &cfg);
        assert_eq!(r.econ_rationality_score, Some(1.0));
        assert_eq!(r.econ_dominance_violations, Some(0));

        // Reported only: the elicited verdict never moves eligibility.
        assert_eq!(d.rank_eligible, r.rank_eligible);
    }

    /// Behavior-role attribution is reported on the score, deterministic, and
    /// keyed by the trace's order patterns.
    #[test]
    fn behavior_role_contributions_are_reported() {
        let mut runs: Vec<Run> = (0..3).map(|_| run(0.002, 0.0005, 60)).collect();
        for r in &mut runs {
            r.trace.events.push(ProcessEvent::OrderPlaced {
                risk_gate_passed: true,
            });
        }
        runs[2].trace.events.push(ProcessEvent::ConcentrationBreach);
        let s = score_agent(&agent("mixed", runs), &ScoreConfig::default());
        let names: Vec<&str> = s
            .role_contributions
            .iter()
            .map(|c| c.role.as_str())
            .collect();
        assert_eq!(names, vec!["clean_active", "warned"]);
        let again = score_agent(
            &agent("mixed", {
                let mut runs: Vec<Run> = (0..3).map(|_| run(0.002, 0.0005, 60)).collect();
                for r in &mut runs {
                    r.trace.events.push(ProcessEvent::OrderPlaced {
                        risk_gate_passed: true,
                    });
                }
                runs[2].trace.events.push(ProcessEvent::ConcentrationBreach);
                runs
            }),
            &ScoreConfig::default(),
        );
        assert_eq!(s.role_contributions, again.role_contributions);

        let empty = score_agent(&agent("none", Vec::new()), &ScoreConfig::default());
        assert!(empty.role_contributions.is_empty());
    }

    /// Scores archived before the reported fields existed still parse, with the
    /// documented defaults.
    #[test]
    fn archived_scores_without_new_fields_deserialize_with_defaults() {
        let s = score_agent(
            &agent("archived", (0..3).map(|_| run(0.002, 0.0005, 60)).collect()),
            &ScoreConfig::default(),
        );
        let mut v = serde_json::to_value(&s).expect("serialize");
        let obj = v.as_object_mut().expect("object");
        for field in [
            "process_score",
            "process_warnings",
            "econ_rationality_score",
            "econ_dominance_violations",
            "role_contributions",
        ] {
            assert!(obj.remove(field).is_some(), "{field} should be present");
        }
        let parsed: CompositeScore = serde_json::from_value(v).expect("archived JSON parses");
        assert_eq!(parsed.process_score, 1.0);
        assert_eq!(parsed.process_warnings, 0);
        assert!(parsed.econ_rationality_score.is_none());
        assert!(parsed.econ_dominance_violations.is_none());
        assert!(parsed.role_contributions.is_empty());
    }

    /// `worst_run_drawdown` is the maximum of the per-run drawdowns, each from
    /// its own starting equity, and is not the pooled track's drawdown.
    #[test]
    fn worst_run_drawdown_is_the_max_over_runs_not_the_pooled_track() {
        // Two losing runs of four 5% losses each: every run draws down 18.5%
        // from its own start, while the pooled track, which strings them
        // together, draws down 33.7%. The per-run figure is the max over runs,
        // not the pooled number.
        let down = || Run {
            returns: vec![-0.05; 4],
            ..Run::default()
        };
        let up = Run {
            returns: vec![0.10; 10],
            ..Run::default()
        };
        let sub = agent("runs", vec![up.clone(), down(), down()]);
        let s = score_agent(&sub, &ScoreConfig::default());

        let expected = max_drawdown(&up.returns).max(max_drawdown(&down().returns));
        assert_eq!(s.worst_run_drawdown.to_bits(), expected.to_bits());
        assert!((s.worst_run_drawdown - (1.0 - 0.95f64.powi(4))).abs() < 1e-12);
        assert_eq!(
            s.max_drawdown.to_bits(),
            max_drawdown(&pooled_of(&sub)).to_bits()
        );
        assert!((s.max_drawdown - (1.0 - 0.95f64.powi(8))).abs() < 1e-12);
        assert!(s.max_drawdown > s.worst_run_drawdown);

        let empty = score_agent(&agent("none", Vec::new()), &ScoreConfig::default());
        assert_eq!(empty.worst_run_drawdown, 0.0);
    }

    // ---- declared mandates -------------------------------------------------

    fn declare(pairs: &[(&str, DeclaredMandate)]) -> MandateDeclarations {
        pairs
            .iter()
            .map(|(id, m)| (id.to_string(), m.clone()))
            .collect()
    }

    /// Everything on a score except the five declared fields, for asserting the
    /// host-verdict columns are untouched by a declaration.
    fn without_declared(mut s: CompositeScore) -> CompositeScore {
        s.declared_mandate = None;
        s.verdict_applied = None;
        s.declared_passed_k = None;
        s.declared_mandate_eligible = None;
        s.declared_mandate_ordinal = None;
        s
    }

    /// A declaration is recorded on the score with the verdict it resolved to,
    /// an undeclared agent carries none of the five fields (absent from its
    /// JSON, not null), and the empty map reproduces `rank` exactly.
    #[test]
    fn declaration_is_recorded_and_absent_by_default() {
        let field = relative_field();
        let cfg = ScoreConfig::default();
        assert_eq!(
            rank(&field, &cfg),
            rank_declared(&field, &declare(&[]), &cfg)
        );

        let board = rank_declared(
            &field,
            &declare(&[
                ("beats", DeclaredMandate::OutperformBuyAndHold),
                (
                    "trails",
                    DeclaredMandate::DrawdownCapped {
                        max_per_run_drawdown: 0.2,
                    },
                ),
            ]),
            &cfg,
        );
        let get = |id: &str| board.iter().find(|s| s.agent_id == id).unwrap();
        assert_eq!(
            get("beats").declared_mandate,
            Some(DeclaredMandate::OutperformBuyAndHold)
        );
        assert_eq!(
            get("beats").verdict_applied,
            Some(MandateVerdict::RelativeTo {
                benchmark_id: "buy-and-hold".to_string()
            })
        );
        assert_eq!(
            get("trails").verdict_applied,
            Some(MandateVerdict::DrawdownCapped {
                max_per_run_drawdown: 0.2
            })
        );
        let json = serde_json::to_string(get("beats")).unwrap();
        assert!(json.contains("\"declared_mandate\":{\"kind\":\"outperform_buy_and_hold\"}"));
        assert!(json.contains(
            "\"verdict_applied\":{\"kind\":\"relative_to\",\"benchmark_id\":\"buy-and-hold\"}"
        ));
        for id in ["buy-and-hold", "five-of-six", "clone"] {
            let s = get(id);
            assert!(s.declared_mandate.is_none() && s.verdict_applied.is_none());
            assert!(s.declared_passed_k.is_none() && s.declared_mandate_eligible.is_none());
            let json = serde_json::to_string(s).unwrap();
            assert!(!json.contains("declared_") && !json.contains("verdict_applied"));
        }
        // The board row says which verdict was applied and how it went.
        assert_eq!(
            get("beats").mandate_verdict_label().as_deref(),
            Some(if get("beats").declared_mandate_eligible == Some(true) {
                "eligible under declared verdict (relative to buy-and-hold); host-board ineligible"
            } else {
                "ineligible under declared verdict (relative to buy-and-hold); host-board ineligible"
            })
        );
        assert!(get("clone").mandate_verdict_label().is_none());

        // The wire shape: the same object as a submission plus `declared_mandate`.
        let wire = r#"[{"agent_id":"a","runs":[{"returns":[0.01,0.02]}],
            "declared_mandate":{"kind":"drawdown_capped","max_per_run_drawdown":0.2}},
            {"agent_id":"b","runs":[{"returns":[0.01,0.02]}]}]"#;
        let parsed: Vec<DeclaredSubmission> = serde_json::from_str(wire).unwrap();
        let (subs, decls) = split_declarations(parsed);
        assert_eq!(subs.len(), 2);
        assert_eq!(subs[0].runs[0].returns, vec![0.01, 0.02]);
        assert_eq!(decls.len(), 1);
        assert_eq!(
            decls["a"],
            DeclaredMandate::DrawdownCapped {
                max_per_run_drawdown: 0.2
            }
        );
    }

    /// An OutperformBuyAndHold declaration is judged relative to itself: the
    /// verdict applied is the relative one, its excess series is identically
    /// zero, and it fails on the zero-excess rule. A clone under another id
    /// fails the same way; an agent that beats it in every cell passes the
    /// declared pass^k.
    #[test]
    fn buy_and_hold_under_long_only_beta_gets_the_relative_verdict_and_fails_it() {
        let field = relative_field();
        let board = rank_declared(
            &field,
            &declare(&[
                ("buy-and-hold", DeclaredMandate::OutperformBuyAndHold),
                ("clone", DeclaredMandate::OutperformBuyAndHold),
                ("beats", DeclaredMandate::OutperformBuyAndHold),
            ]),
            &ScoreConfig::default(),
        );
        let get = |id: &str| board.iter().find(|s| s.agent_id == id).unwrap();
        let relative = Some(MandateVerdict::RelativeTo {
            benchmark_id: "buy-and-hold".to_string(),
        });
        assert_eq!(get("buy-and-hold").verdict_applied, relative);
        assert_eq!(get("buy-and-hold").declared_passed_k, Some(false));
        assert_eq!(get("buy-and-hold").declared_mandate_eligible, Some(false));
        assert_eq!(
            get("buy-and-hold").mandate_verdict_label().as_deref(),
            Some(
                "ineligible under declared verdict (relative to buy-and-hold); host-board ineligible"
            )
        );
        assert_eq!(get("clone").declared_passed_k, Some(false));
        assert_eq!(get("beats").declared_passed_k, Some(true));
        // The host verdict on the same rows is what `rank` says.
        let plain = rank(&field, &ScoreConfig::default());
        for s in &board {
            let p = plain.iter().find(|x| x.agent_id == s.agent_id).unwrap();
            assert_eq!(&without_declared(s.clone()), p, "{}", s.agent_id);
        }
    }

    /// A declaration selects the reliability question and nothing else: under
    /// every declaration kind, every host-verdict column is byte-identical to
    /// `rank`, declared eligibility implies the DSR, bootstrap, process and
    /// host-mandate gates, and raising the DSR bar refuses every declaration.
    #[test]
    fn a_declaration_cannot_flip_dsr_eligibility_or_move_the_host_columns() {
        let field = field_with_one_single_run_failure();
        let kinds = [
            DeclaredMandate::AbsoluteReturn,
            DeclaredMandate::OutperformBuyAndHold,
            DeclaredMandate::RelativeTo {
                benchmark_id: "a".to_string(),
            },
            DeclaredMandate::DrawdownCapped {
                max_per_run_drawdown: 0.5,
            },
        ];
        let cfg = ScoreConfig::default();
        let plain = rank(&field, &cfg);
        for kind in &kinds {
            let decls: MandateDeclarations = field
                .iter()
                .map(|s| (s.agent_id.clone(), kind.clone()))
                .collect();
            let board = rank_declared(&field, &decls, &cfg);
            for s in &board {
                let p = plain.iter().find(|x| x.agent_id == s.agent_id).unwrap();
                assert_eq!(&without_declared(s.clone()), p, "{kind:?} {}", s.agent_id);
                if s.declared_mandate_eligible == Some(true) {
                    assert!(s.deflated_sharpe >= cfg.dsr_bar);
                    assert!(s.bootstrap_p < cfg.alpha);
                    assert!(s.process_ok && s.mandate_ok);
                }
            }
            // Under `AbsoluteReturn` the declared verdict is the host's default
            // verdict, so the two columns agree on every row.
            if *kind == DeclaredMandate::AbsoluteReturn {
                for s in &board {
                    assert_eq!(s.declared_mandate_eligible, Some(s.rank_eligible));
                    assert_eq!(s.declared_passed_k, Some(s.passed_k));
                }
            }
            // No declaration survives a DSR bar nobody clears.
            let strict = ScoreConfig {
                dsr_bar: 10.0,
                ..ScoreConfig::default()
            };
            for s in rank_declared(&field, &decls, &strict) {
                assert_eq!(s.declared_mandate_eligible, Some(false), "{kind:?}");
                assert!(!s.rank_eligible);
            }
        }
        // The one-bad-run agent is refused by the default (one losing window)
        // and passes its drawdown-capped declaration: one run clears the bar
        // and no run draws down past the declared bound. That is the case a
        // declaration exists for, and it is reported, not ranked.
        let capped = rank_declared(
            &field,
            &declare(&[(
                "one_bad_run",
                DeclaredMandate::DrawdownCapped {
                    max_per_run_drawdown: 0.5,
                },
            )]),
            &cfg,
        );
        let bad = capped.iter().find(|s| s.agent_id == "one_bad_run").unwrap();
        assert!(!bad.rank_eligible && !bad.passed_k);
        assert_eq!(bad.declared_passed_k, Some(true));
        assert_eq!(bad.declared_mandate_eligible, Some(true), "{bad:?}");
        assert_eq!(
            bad.mandate_verdict_label().as_deref(),
            Some(
                "eligible under declared verdict (drawdown capped at 0.50 per run); host-board ineligible"
            )
        );
    }

    /// A benchmark the field does not contain, a relative declaration scored
    /// without a field, and a drawdown bound outside (0, 1] all fail the
    /// declared verdict closed; none falls back to the absolute test.
    #[test]
    fn misdeclared_mandates_fail_closed() {
        let field = relative_field();
        let cfg = ScoreConfig::default();
        let board = rank_declared(
            &field,
            &declare(&[(
                "beats",
                DeclaredMandate::RelativeTo {
                    benchmark_id: "absent".to_string(),
                },
            )]),
            &cfg,
        );
        let beats = board.iter().find(|s| s.agent_id == "beats").unwrap();
        assert_eq!(beats.declared_passed_k, Some(false));
        assert_eq!(beats.declared_mandate_eligible, Some(false));
        // The same agent beats buy-and-hold in every cell when the benchmark is
        // named correctly, so the refusal above is the misdeclaration's.
        let ok = rank_declared(
            &field,
            &declare(&[("beats", DeclaredMandate::OutperformBuyAndHold)]),
            &cfg,
        );
        assert_eq!(
            ok.iter()
                .find(|s| s.agent_id == "beats")
                .unwrap()
                .declared_passed_k,
            Some(true)
        );

        // No field, no benchmark.
        let alone = score_agent_declared(
            &field[1],
            Some(&DeclaredMandate::OutperformBuyAndHold),
            &cfg,
        );
        assert_eq!(alone.declared_passed_k, None);
        assert_eq!(alone.declared_mandate_eligible, None);
        assert_eq!(
            score_agent_declared(&field[1], None, &cfg),
            score_agent(&field[1], &cfg)
        );

        // A clean agent passes a well-formed drawdown declaration and fails
        // every malformed one, on the bound alone.
        let clean = field_with_one_single_run_failure();
        let capped = |x: f64| {
            let board = rank_declared(
                &clean,
                &declare(&[(
                    "a",
                    DeclaredMandate::DrawdownCapped {
                        max_per_run_drawdown: x,
                    },
                )]),
                &cfg,
            );
            let a = board.iter().find(|s| s.agent_id == "a").unwrap().clone();
            (a.declared_passed_k, a.declared_mandate_eligible)
        };
        assert_eq!(capped(0.5), (Some(true), Some(true)));
        for bad in [0.0, -0.2, 1.5, f64::NAN, f64::INFINITY] {
            assert_eq!(capped(bad), (Some(true), Some(false)), "bound {bad}");
        }
    }

    /// Declared eligibility never moves the board. An agent that is
    /// host-ineligible and declared-eligible keeps ordinal 0 and its position;
    /// its declared ordinal counts only agents in its own mandate class, so two
    /// agents under different verdicts are each first in their class and never
    /// ordered against each other or against the host ordinal.
    #[test]
    fn declared_eligibility_is_ranked_within_its_mandate_class_only() {
        let field = field_with_one_single_run_failure();
        let cfg = ScoreConfig::default();
        let plain = rank(&field, &cfg);
        let board = rank_declared(
            &field,
            &declare(&[
                (
                    "one_bad_run",
                    DeclaredMandate::DrawdownCapped {
                        max_per_run_drawdown: 0.5,
                    },
                ),
                ("a", DeclaredMandate::AbsoluteReturn),
                ("b", DeclaredMandate::AbsoluteReturn),
            ]),
            &cfg,
        );
        let order = |b: &[CompositeScore]| b.iter().map(|s| s.agent_id.clone()).collect::<Vec<_>>();
        assert_eq!(order(&board), order(&plain));
        let get = |id: &str| board.iter().find(|s| s.agent_id == id).unwrap();
        let bad = get("one_bad_run");
        assert!(!bad.rank_eligible && bad.rank_ordinal == 0);
        assert_eq!(bad.declared_mandate_eligible, Some(true));
        assert_eq!(bad.declared_mandate_ordinal, Some(1));
        // The absolute-return class holds the two host-eligible agents, ordered
        // by DSR then id, and is a separate class from the drawdown-capped one.
        let (a, b) = (get("a"), get("b"));
        assert!(a.rank_eligible && b.rank_eligible);
        let ords: Vec<Option<usize>> = [a, b].iter().map(|s| s.declared_mandate_ordinal).collect();
        assert!(
            ords.contains(&Some(1)) && ords.contains(&Some(2)),
            "{ords:?}"
        );
        let (first, second) = if a.declared_mandate_ordinal == Some(1) {
            (a, b)
        } else {
            (b, a)
        };
        assert!(
            first.deflated_sharpe > second.deflated_sharpe
                || (first.deflated_sharpe == second.deflated_sharpe
                    && first.agent_id < second.agent_id)
        );
        // Host ordinals are unchanged by the declarations.
        for s in &board {
            let p = plain.iter().find(|x| x.agent_id == s.agent_id).unwrap();
            assert_eq!(s.rank_ordinal, p.rank_ordinal);
        }
    }
}
