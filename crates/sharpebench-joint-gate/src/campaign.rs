//! The campaign document: every factor a run enumerates.
//!
//! The study protocol (`sharpebench-study`) is a closed contract that names one
//! method, its claims and its precision. It has no field for a grid of rules or
//! generators, and it should not grow one: those belong to this study, not to
//! every study. The campaign carries them, and the protocol binds the campaign
//! by digest in `identity.configuration_digest`, so a sealed protocol fixes
//! every cell below.
//!
//! Like the protocol, this document is closed: an unknown key is refused at
//! load, not read past.

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use sharpebench_core::composite::ScoreConfig;
use sharpebench_core::PassMode;

/// Identifier of this document's shape.
pub const CAMPAIGN_CONTRACT: &str = "sharpebench/joint-gate-campaign/v1";

/// A campaign that cannot be run as written.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CampaignError(pub String);

impl std::fmt::Display for CampaignError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for CampaignError {}

fn refuse<T>(detail: impl Into<String>) -> Result<T, CampaignError> {
    Err(CampaignError(detail.into()))
}

/// How many runs must clear the per-run bar.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Reliability {
    /// The shipped rule: every run.
    AllRuns,
    /// At least this share of the runs, rounded up. A six-window four-of-six
    /// variant is 2/3 of the runs, not four whole windows: the kernel's
    /// `PassMode::AtLeast` counts runs, and the study varies the shipped knob.
    AtLeastShare { share: f64 },
}

/// One candidate eligibility rule: the study variable. Everything not named
/// here stays at its shipped `ScoreConfig` default.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Rule {
    pub id: String,
    /// Host lower bound on trials. `rank` raises it to the field size.
    pub host_n: u32,
    /// Annualized dispersion prior. It sets both `trials_sr_std` and
    /// `min_measured_trials_sr_std`, so measurement can tighten the bar but
    /// never relax it, the relation the shipped defaults have.
    pub dispersion_prior: f64,
    pub windows: usize,
    pub per_run_psr_bar: f64,
    pub dsr_bar: f64,
    pub reliability: Reliability,
}

/// Marginal shape of one innovation.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Tail {
    Normal,
    /// Student t with integer degrees of freedom above two, unit variance.
    StudentT {
        nu: u32,
    },
}

/// GARCH(1,1) conditional variance with unit unconditional variance.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Garch {
    pub alpha: f64,
    pub beta: f64,
}

/// Two-state Markov regime with a zero unconditional mean: `calm` and `stress`
/// differ in mean and volatility, so a window sits in a regime and its Sharpe is
/// not the population's. This is a null: the long-run mean is exactly zero.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Regime {
    /// Probability of staying calm, per period.
    pub stay_calm: f64,
    /// Probability of staying stressed, per period.
    pub stay_stress: f64,
    /// Calm per-period mean in units of the calm-state volatility.
    pub calm_mean: f64,
    /// Stress volatility over calm volatility.
    pub stress_vol_ratio: f64,
}

/// One data-generating process for a zero-skill path.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Dgp {
    pub id: String,
    /// First-order autocorrelation of the path.
    pub rho: f64,
    pub tail: Tail,
    pub garch: Option<Garch>,
    pub regime: Option<Regime>,
    /// Share of each entrant's variance that is a common path shared by the
    /// whole field. Zero is independent entrants.
    pub shared_variance: f64,
}

/// Which verdict a cell reads.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SeedPath {
    /// The shipped path: seed-averaged pooled track, per-run pass^k.
    Average,
    /// The alternative under test: score each execution seed as its own path,
    /// eligible only when eligible on every one.
    PerSeed,
}

/// What a block's cells are for.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BlockRole {
    /// Decides size control. Zero effect.
    PrimaryNull,
    /// Decides power.
    PrimaryPower,
    /// Reported beside the decision, never part of it.
    Sensitivity,
}

/// One statistical block: the full product of its lists.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StatBlock {
    pub id: String,
    pub role: BlockRole,
    pub replicates: u64,
    pub dgps: Vec<String>,
    pub history_bars: Vec<usize>,
    /// True annualized Sharpe planted on the focal entrant.
    pub effects_annual: Vec<f64>,
    pub rules: Vec<String>,
    pub field_size: usize,
    pub execution_seeds: usize,
    /// Execution noise per seed, in units of the path's volatility.
    pub seed_noise_eta: f64,
    pub block_probs: Vec<f64>,
    pub seed_paths: Vec<SeedPath>,
}

/// The negative controls. Each is one integrity kind in `controls.rs`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ControlKind {
    LookAheadOracle,
    Fabrication,
    ProcessViolation,
    MandateBreach,
    ConstantWindowGriefer,
    DynamicLeverage,
    CostLosingRandom,
    PassiveBeta,
}

impl ControlKind {
    pub fn tag(self) -> &'static str {
        match self {
            Self::LookAheadOracle => "look_ahead_oracle",
            Self::Fabrication => "fabrication",
            Self::ProcessViolation => "process_violation",
            Self::MandateBreach => "mandate_breach",
            Self::ConstantWindowGriefer => "constant_window_griefer",
            Self::DynamicLeverage => "dynamic_leverage",
            Self::CostLosingRandom => "cost_losing_random",
            Self::PassiveBeta => "passive_beta",
        }
    }
}

/// One integrity block: a planted violation or negative control with a
/// detection rate, not a power curve.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IntegrityBlock {
    pub kind: ControlKind,
    pub replicates: u64,
    pub history_bars: usize,
    pub windows: usize,
    pub execution_seeds: usize,
    /// Rule the control is scored under.
    pub rule: String,
}

/// The predeclared reading of the finished run. Numbers only: the logic is in
/// `report.rs` and has tests of its own.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DecisionPlan {
    /// Whole-field false-eligibility limit a rule must keep in every null cell.
    pub whole_field_limit: f64,
    /// Lower bound on power a rule must show in every target cell.
    pub power_bound: f64,
    /// `(effect, history)` pairs a rule must reach, any one of which makes it
    /// feasible at that target.
    pub power_targets: Vec<PowerTarget>,
    /// Lower bound every integrity control's detection rate must show.
    pub detection_bound: f64,
}

/// One target the outcome is stated against.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PowerTarget {
    pub effect_annual: f64,
    pub history_bars: usize,
}

/// The whole campaign.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Campaign {
    pub contract: String,
    pub campaign_id: String,
    pub master_seed: u64,
    pub periods_per_year: f64,
    pub vol_per_period: f64,
    /// Replicates per shard, the unit of work and of resume.
    pub shard_replicates: u64,
    pub rules: Vec<Rule>,
    pub dgps: Vec<Dgp>,
    pub statistical: Vec<StatBlock>,
    pub integrity: Vec<IntegrityBlock>,
    pub decision: DecisionPlan,
}

impl Campaign {
    /// Parse and validate.
    pub fn from_json(text: &str) -> Result<Self, CampaignError> {
        let campaign: Campaign =
            serde_json::from_str(text).map_err(|e| CampaignError(format!("malformed: {e}")))?;
        campaign.validate()?;
        Ok(campaign)
    }

    /// Lowercase hex SHA-256 of the canonical serialization, the digest a
    /// protocol's `configuration_digest` binds. Canonical means the serialized
    /// struct, not the file bytes, so whitespace cannot move it and a change to
    /// any value must.
    pub fn digest(&self) -> String {
        let bytes = serde_json::to_vec(self).expect("a campaign serializes");
        hex(&Sha256::digest(&bytes))
    }

    pub fn rule(&self, id: &str) -> Option<&Rule> {
        self.rules.iter().find(|r| r.id == id)
    }

    pub fn dgp(&self, id: &str) -> Option<&Dgp> {
        self.dgps.iter().find(|d| d.id == id)
    }

    /// Refuse a campaign that names something it does not define or a cell the
    /// kernel could not run.
    pub fn validate(&self) -> Result<(), CampaignError> {
        if self.contract != CAMPAIGN_CONTRACT {
            return refuse(format!("unsupported contract {}", self.contract));
        }
        if self.shard_replicates == 0 {
            return refuse("shard_replicates must be at least one");
        }
        if !(self.periods_per_year.is_finite() && self.periods_per_year > 0.0) {
            return refuse("periods_per_year must be finite and positive");
        }
        if !(self.vol_per_period.is_finite() && self.vol_per_period > 0.0) {
            return refuse("vol_per_period must be finite and positive");
        }
        let unique = |ids: Vec<&str>, what: &str| -> Result<(), CampaignError> {
            let n = ids.len();
            if ids.into_iter().collect::<BTreeSet<_>>().len() != n {
                return refuse(format!("duplicate {what} id"));
            }
            Ok(())
        };
        unique(self.rules.iter().map(|r| r.id.as_str()).collect(), "rule")?;
        unique(self.dgps.iter().map(|d| d.id.as_str()).collect(), "dgp")?;
        unique(
            self.statistical.iter().map(|b| b.id.as_str()).collect(),
            "block",
        )?;
        for rule in &self.rules {
            rule.validate()?;
        }
        for dgp in &self.dgps {
            dgp.validate()?;
        }
        for block in &self.statistical {
            self.validate_block(block)?;
        }
        for block in &self.integrity {
            let Some(rule) = self.rule(&block.rule) else {
                return refuse(format!("integrity block names unknown rule {}", block.rule));
            };
            if block.replicates == 0 || block.execution_seeds == 0 {
                return refuse("an integrity block needs replicates and seeds");
            }
            if block.windows != rule.windows {
                return refuse(format!(
                    "integrity block {} has {} windows, its rule {}",
                    block.kind.tag(),
                    block.windows,
                    rule.windows
                ));
            }
            if block.history_bars == 0 || !block.history_bars.is_multiple_of(block.windows) {
                return refuse("integrity history must be whole equal windows");
            }
        }
        let d = &self.decision;
        for (name, value) in [
            ("whole_field_limit", d.whole_field_limit),
            ("power_bound", d.power_bound),
            ("detection_bound", d.detection_bound),
        ] {
            if !(value.is_finite() && (0.0..=1.0).contains(&value)) {
                return refuse(format!("{name} must be finite and in [0, 1]"));
            }
        }
        for target in &d.power_targets {
            let covered = self.statistical.iter().any(|b| {
                b.role == BlockRole::PrimaryPower
                    && b.history_bars.contains(&target.history_bars)
                    && b.effects_annual.contains(&target.effect_annual)
            });
            if !covered {
                return refuse(format!(
                    "power target {} over {} bars has no primary cell",
                    target.effect_annual, target.history_bars
                ));
            }
        }
        Ok(())
    }

    fn validate_block(&self, block: &StatBlock) -> Result<(), CampaignError> {
        let name = &block.id;
        if block.replicates == 0 {
            return refuse(format!("block {name} has no replicates"));
        }
        if block.field_size < 2 {
            return refuse(format!("block {name} needs a focal entrant and a peer"));
        }
        if block.execution_seeds == 0 {
            return refuse(format!("block {name} needs execution seeds"));
        }
        if !(block.seed_noise_eta.is_finite() && block.seed_noise_eta >= 0.0) {
            return refuse(format!("block {name} eta must be finite and nonnegative"));
        }
        for (list, what) in [
            (block.dgps.len(), "dgps"),
            (block.history_bars.len(), "history_bars"),
            (block.effects_annual.len(), "effects_annual"),
            (block.rules.len(), "rules"),
            (block.block_probs.len(), "block_probs"),
            (block.seed_paths.len(), "seed_paths"),
        ] {
            if list == 0 {
                return refuse(format!("block {name} lists no {what}"));
            }
        }
        if block.role == BlockRole::PrimaryNull && block.effects_annual != [0.0] {
            return refuse(format!("primary null block {name} plants an effect"));
        }
        for id in &block.dgps {
            if self.dgp(id).is_none() {
                return refuse(format!("block {name} names unknown dgp {id}"));
            }
        }
        for id in &block.rules {
            let Some(rule) = self.rule(id) else {
                return refuse(format!("block {name} names unknown rule {id}"));
            };
            for &bars in &block.history_bars {
                if bars == 0 || !bars.is_multiple_of(rule.windows) {
                    return refuse(format!(
                        "block {name}: {bars} bars is not {} equal windows",
                        rule.windows
                    ));
                }
            }
        }
        for &p in &block.block_probs {
            if !(p.is_finite() && 0.0 < p && p <= 1.0) {
                return refuse(format!("block {name} block_prob must be in (0, 1]"));
            }
        }
        for &s in &block.effects_annual {
            if !s.is_finite() {
                return refuse(format!("block {name} effect must be finite"));
            }
        }
        Ok(())
    }
}

impl Rule {
    fn validate(&self) -> Result<(), CampaignError> {
        if self.host_n == 0 {
            return refuse(format!("rule {} host_n must be at least one", self.id));
        }
        if self.windows == 0 {
            return refuse(format!("rule {} needs a window", self.id));
        }
        if !(self.dispersion_prior.is_finite() && self.dispersion_prior >= 0.0) {
            return refuse(format!("rule {} prior must be nonnegative", self.id));
        }
        for bar in [self.per_run_psr_bar, self.dsr_bar] {
            if !(bar.is_finite() && (0.0..=1.0).contains(&bar)) {
                return refuse(format!("rule {} bars must be in [0, 1]", self.id));
            }
        }
        if let Reliability::AtLeastShare { share } = self.reliability {
            if !(share.is_finite() && 0.0 < share && share <= 1.0) {
                return refuse(format!("rule {} share must be in (0, 1]", self.id));
            }
        }
        Ok(())
    }

    /// The exact `ScoreConfig` a cell scores under. Every other field is the
    /// shipped default, so a default added upstream reaches the study by
    /// construction and moves the effective-config digest.
    pub fn score_config(
        &self,
        periods_per_year: f64,
        seeds_per_window: usize,
        block_prob: f64,
    ) -> ScoreConfig {
        let runs = self.windows * seeds_per_window;
        let pass_mode = match self.reliability {
            Reliability::AllRuns => PassMode::All,
            Reliability::AtLeastShare { share } => {
                // The slack absorbs a share like 2/3 rounding just above a whole
                // run count, which would otherwise demand one run more.
                PassMode::AtLeast((share * runs as f64 - 1e-9).ceil() as usize)
            }
        };
        ScoreConfig {
            n_trials: self.host_n,
            trials_sr_std: self.dispersion_prior,
            min_measured_trials_sr_std: self.dispersion_prior,
            dsr_bar: self.dsr_bar,
            per_run_psr_bar: self.per_run_psr_bar,
            pass_mode,
            execution_seeds_per_window: seeds_per_window,
            block_prob,
            ..ScoreConfig::for_periods_per_year(periods_per_year)
        }
    }
}

impl Dgp {
    fn validate(&self) -> Result<(), CampaignError> {
        let id = &self.id;
        if !(self.rho.is_finite() && self.rho.abs() < 1.0) {
            return refuse(format!("dgp {id} rho must be inside (-1, 1)"));
        }
        if !(self.shared_variance.is_finite() && (0.0..=1.0).contains(&self.shared_variance)) {
            return refuse(format!("dgp {id} shared_variance must be in [0, 1]"));
        }
        if let Tail::StudentT { nu } = self.tail {
            if nu <= 2 {
                return refuse(format!("dgp {id} needs nu above two for a finite variance"));
            }
        }
        if let Some(g) = self.garch {
            if !(g.alpha >= 0.0 && g.beta >= 0.0 && g.alpha + g.beta < 1.0) {
                return refuse(format!("dgp {id} garch is not covariance stationary"));
            }
        }
        if let Some(r) = self.regime {
            let probability = |p: f64| p.is_finite() && 0.0 < p && p < 1.0;
            if !(probability(r.stay_calm)
                && probability(r.stay_stress)
                && r.stress_vol_ratio.is_finite()
                && r.stress_vol_ratio > 0.0
                && r.calm_mean.is_finite())
            {
                return refuse(format!("dgp {id} regime is outside its domain"));
            }
        }
        Ok(())
    }
}

/// Lowercase hex of `bytes`.
pub fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}
