//! E2b - scenario-transition manifests over the multi-session DAG.
//!
//! [`crate::multisession`] scores which sessions' memory later sessions rely on.
//! It does not say what crosses a stage boundary, when each stage takes effect, or
//! which earlier guarantees must survive. A [`ScenarioManifest`] declares that per
//! stage transition, and [`scenario_transition_report`] composes it with the
//! existing chain scorer without changing it.
//!
//! **Stage semantics are declared, never defaulted.** Every transition names a
//! [`CarryoverMode`]:
//! - [`CarryoverMode::FreshEpisodeWithMemory`]: the later stage opens on its own
//!   declared initial portfolio. Only memory artifacts in the transition's allowed
//!   set cross the boundary.
//! - [`CarryoverMode::ContinuousPortfolio`]: the later stage opens on the earlier
//!   stage's closing portfolio (cash and positions). Memory still crosses only
//!   through the allowed set.
//!
//! A transition without a mode is refused with
//! [`TransitionError::UndeclaredCarryoverMode`].
//!
//! **Point in time.** The observation for stage `k` ([`observe_stage`]) is built
//! from the manifest, the closing records of stage `k`'s declared predecessors, and
//! fact versions available on or before stage `k`'s effective date. Effective dates
//! strictly increase along every edge, so no later stage can feed an earlier
//! observation.
//!
//! **Failures stay on the record.** A preservation obligation broken by a later
//! stage is reported against that later stage. The earlier stage's own row is not
//! rewritten, and a later stage's success or memory credit never removes an earlier
//! stage from [`TransitionReport::failed_stages`].

use crate::multisession::{multi_session_report, MultiSessionReport, SessionId, SessionScores};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

/// A stage is a session node of the multi-session DAG.
pub type StageId = SessionId;

/// A calendar date `YYYY-MM-DD` on which a stage, fact version or cutoff takes
/// effect. Serialized as that string; parsing refuses anything else.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct EffectiveDate {
    year: u16,
    month: u8,
    day: u8,
}

impl EffectiveDate {
    /// A proleptic Gregorian date with year in `1..=9999`.
    pub fn new(year: u16, month: u8, day: u8) -> Result<Self, TransitionError> {
        let leap =
            (year.is_multiple_of(4) && !year.is_multiple_of(100)) || year.is_multiple_of(400);
        let days_in_month = match month {
            1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
            4 | 6 | 9 | 11 => 30,
            2 if leap => 29,
            2 => 28,
            _ => 0,
        };
        if !(1..=9999).contains(&year) || day == 0 || day > days_in_month {
            return Err(TransitionError::InvalidDate {
                text: format!("{year:04}-{month:02}-{day:02}"),
            });
        }
        Ok(Self { year, month, day })
    }

    /// Parse exactly `YYYY-MM-DD`.
    pub fn parse(text: &str) -> Result<Self, TransitionError> {
        let invalid = || TransitionError::InvalidDate {
            text: text.to_string(),
        };
        let bytes = text.as_bytes();
        let shaped = bytes.len() == 10
            && bytes[4] == b'-'
            && bytes[7] == b'-'
            && bytes
                .iter()
                .enumerate()
                .all(|(i, b)| i == 4 || i == 7 || b.is_ascii_digit());
        if !shaped {
            return Err(invalid());
        }
        let number = |range: std::ops::Range<usize>| text[range].parse::<u16>();
        let (year, month, day) = match (number(0..4), number(5..7), number(8..10)) {
            (Ok(y), Ok(m), Ok(d)) => (y, m as u8, d as u8),
            _ => return Err(invalid()),
        };
        Self::new(year, month, day).map_err(|_| invalid())
    }
}

impl fmt::Display for EffectiveDate {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:04}-{:02}-{:02}", self.year, self.month, self.day)
    }
}

impl Serialize for EffectiveDate {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for EffectiveDate {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let wire = String::deserialize(deserializer)?;
        Self::parse(&wire).map_err(serde::de::Error::custom)
    }
}

/// What crosses a stage boundary besides allowed memory artifacts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CarryoverMode {
    /// Portfolio, cash and positions reset to the later stage's declared initial
    /// portfolio. Only allowed memory artifacts cross.
    FreshEpisodeWithMemory,
    /// Cash and positions carry from the earlier stage's closing portfolio.
    /// Allowed memory artifacts cross as well.
    ContinuousPortfolio,
}

/// Cash plus signed position notionals, marked in one currency unit. Every value
/// and both totals are finite by construction.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct PortfolioState {
    cash: f64,
    positions: BTreeMap<String, f64>,
}

#[derive(Deserialize)]
#[serde(rename = "PortfolioState", deny_unknown_fields)]
struct PortfolioWire {
    cash: f64,
    #[serde(default)]
    positions: BTreeMap<String, f64>,
}

impl<'de> Deserialize<'de> for PortfolioState {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let wire = PortfolioWire::deserialize(deserializer)?;
        Self::new(wire.cash, wire.positions).map_err(serde::de::Error::custom)
    }
}

impl PortfolioState {
    /// Refuses nonfinite cash, positions, marked value or gross exposure.
    pub fn new(cash: f64, positions: BTreeMap<String, f64>) -> Result<Self, TransitionError> {
        let state = Self { cash, positions };
        let finite = state.cash.is_finite()
            && state.positions.values().all(|v| v.is_finite())
            && state.marked_value().is_finite()
            && state.gross_exposure().is_finite();
        if !finite {
            return Err(TransitionError::NonFinitePortfolio);
        }
        Ok(state)
    }

    pub fn cash(&self) -> f64 {
        self.cash
    }

    pub fn positions(&self) -> &BTreeMap<String, f64> {
        &self.positions
    }

    /// Sum of absolute position notionals, in instrument-name order.
    pub fn gross_exposure(&self) -> f64 {
        self.positions.values().map(|v| v.abs()).sum()
    }

    /// Cash plus signed position notionals, in instrument-name order.
    pub fn marked_value(&self) -> f64 {
        self.cash + self.positions.values().sum::<f64>()
    }
}

/// One dated version of a named fact.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct FactRef {
    pub name: String,
    pub available_from: EffectiveDate,
}

/// A fact version with its value, as shown in an observation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct FactVersion {
    pub name: String,
    pub available_from: EffectiveDate,
    pub value: String,
}

/// Every dated fact version the scenario can reveal. A later version of a name is
/// a revision; it never replaces what an earlier cutoff saw.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FactLog {
    versions: BTreeMap<String, BTreeMap<EffectiveDate, String>>,
}

impl FactLog {
    /// Refuses two versions of one fact with the same availability date.
    pub fn new(versions: Vec<FactVersion>) -> Result<Self, TransitionError> {
        let mut log: BTreeMap<String, BTreeMap<EffectiveDate, String>> = BTreeMap::new();
        for version in versions {
            let dated = log.entry(version.name.clone()).or_default();
            if dated
                .insert(version.available_from, version.value)
                .is_some()
            {
                return Err(TransitionError::DuplicateFactVersion {
                    fact: FactRef {
                        name: version.name,
                        available_from: version.available_from,
                    },
                });
            }
        }
        Ok(Self { versions: log })
    }

    fn contains(&self, fact: &FactRef) -> bool {
        self.versions
            .get(&fact.name)
            .is_some_and(|dated| dated.contains_key(&fact.available_from))
    }

    /// The latest version of each fact available on or before `cutoff`.
    fn as_of(&self, cutoff: EffectiveDate) -> Vec<FactVersion> {
        self.versions
            .iter()
            .filter_map(|(name, dated)| {
                dated
                    .range(..=cutoff)
                    .next_back()
                    .map(|(&available_from, value)| FactVersion {
                        name: name.clone(),
                        available_from,
                        value: value.clone(),
                    })
            })
            .collect()
    }
}

/// A checkable guarantee a stage makes about its own record.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Invariant {
    /// Closing gross exposure must not exceed this finite, non-negative cap.
    GrossExposureCap { max_gross_exposure: f64 },
    /// No fact version used may become available after `cutoff`.
    PointInTime { cutoff: EffectiveDate },
}

impl Invariant {
    fn holds(&self, record: &StageRecord) -> bool {
        match self {
            Self::GrossExposureCap { max_gross_exposure } => {
                record.closing.gross_exposure() <= *max_gross_exposure
            }
            Self::PointInTime { cutoff } => record
                .facts_used
                .iter()
                .all(|fact| fact.available_from <= *cutoff),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NamedInvariant {
    pub name: String,
    pub invariant: Invariant,
}

/// Declaration of one stage. Validated only as part of [`ScenarioManifest::declare`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StageDecl {
    pub stage_id: StageId,
    pub effective_date: EffectiveDate,
    /// Required unless the stage is entered by exactly one continuous transition,
    /// in which case it must be absent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub initial_portfolio: Option<PortfolioState>,
    /// Fact versions the stage's script references. None may be dated after the
    /// stage's effective date.
    #[serde(default)]
    pub fact_refs: Vec<FactRef>,
    /// Guarantees checked on this stage's own record.
    #[serde(default)]
    pub invariants: Vec<NamedInvariant>,
}

/// Declaration of one DAG edge. Validated only as part of [`ScenarioManifest::declare`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TransitionDecl {
    pub from: StageId,
    pub to: StageId,
    /// `None` is refused. It is optional only so that an omission is a typed
    /// refusal rather than a silently chosen default.
    #[serde(default)]
    pub carryover_mode: Option<CarryoverMode>,
    /// Memory artifact names allowed to cross this edge.
    #[serde(default)]
    pub allowed_memory: BTreeSet<String>,
    /// Names of `from`'s invariants that must still hold on `to`'s record.
    #[serde(default)]
    pub preserve: BTreeSet<String>,
}

/// A validated scenario-transition manifest. Stages are held in stage-ID order
/// and transitions in `(from, to)` order.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ScenarioManifest {
    stages: Vec<StageDecl>,
    transitions: Vec<TransitionDecl>,
}

#[derive(Deserialize)]
#[serde(rename = "ScenarioManifest", deny_unknown_fields)]
struct ManifestWire {
    stages: Vec<StageDecl>,
    transitions: Vec<TransitionDecl>,
}

impl<'de> Deserialize<'de> for ScenarioManifest {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let wire = ManifestWire::deserialize(deserializer)?;
        Self::declare(wire.stages, wire.transitions).map_err(serde::de::Error::custom)
    }
}

impl ScenarioManifest {
    /// Validate a manifest on its own terms. Binding to a session DAG happens in
    /// [`scenario_transition_report`].
    pub fn declare(
        mut stages: Vec<StageDecl>,
        mut transitions: Vec<TransitionDecl>,
    ) -> Result<Self, TransitionError> {
        stages.sort_by_key(|s| s.stage_id);
        for pair in stages.windows(2) {
            if pair[0].stage_id == pair[1].stage_id {
                return Err(TransitionError::DuplicateStage {
                    stage: pair[0].stage_id,
                });
            }
        }
        for stage in &stages {
            let mut names = BTreeSet::new();
            for named in &stage.invariants {
                if !names.insert(named.name.as_str()) {
                    return Err(TransitionError::DuplicateInvariant {
                        stage: stage.stage_id,
                        name: named.name.clone(),
                    });
                }
                if let Invariant::GrossExposureCap { max_gross_exposure } = named.invariant {
                    if !max_gross_exposure.is_finite() || max_gross_exposure < 0.0 {
                        return Err(TransitionError::InvalidInvariant {
                            stage: stage.stage_id,
                            name: named.name.clone(),
                        });
                    }
                }
            }
            for fact in &stage.fact_refs {
                if fact.available_from > stage.effective_date {
                    return Err(TransitionError::FutureFactReference {
                        stage: stage.stage_id,
                        fact: fact.clone(),
                        effective_date: stage.effective_date,
                    });
                }
            }
        }
        transitions.sort_by_key(|t| (t.from, t.to));
        let manifest = Self {
            stages,
            transitions,
        };
        let mut seen = BTreeSet::new();
        for t in &manifest.transitions {
            let from = manifest.find_stage(t.from)?;
            let to = manifest.find_stage(t.to)?;
            if !seen.insert((t.from, t.to)) {
                return Err(TransitionError::DuplicateTransition {
                    from: t.from,
                    to: t.to,
                });
            }
            if t.carryover_mode.is_none() {
                return Err(TransitionError::UndeclaredCarryoverMode {
                    from: t.from,
                    to: t.to,
                });
            }
            if from.effective_date >= to.effective_date {
                return Err(TransitionError::NonIncreasingEffectiveDate {
                    from: t.from,
                    to: t.to,
                    from_date: from.effective_date,
                    to_date: to.effective_date,
                });
            }
            if let Some(name) = t
                .preserve
                .iter()
                .find(|name| !from.invariants.iter().any(|i| &i.name == *name))
            {
                return Err(TransitionError::UnknownObligation {
                    from: t.from,
                    to: t.to,
                    name: name.clone(),
                });
            }
        }
        for stage in &manifest.stages {
            let continuous = manifest
                .incoming(stage.stage_id)
                .filter(|t| t.carryover_mode == Some(CarryoverMode::ContinuousPortfolio))
                .count();
            match (continuous, &stage.initial_portfolio) {
                (0, None) => {
                    return Err(TransitionError::MissingInitialPortfolio {
                        stage: stage.stage_id,
                    })
                }
                (1, Some(_)) => {
                    return Err(TransitionError::UnexpectedInitialPortfolio {
                        stage: stage.stage_id,
                    })
                }
                (0 | 1, _) => {}
                _ => {
                    return Err(TransitionError::MultipleContinuousPredecessors {
                        stage: stage.stage_id,
                    })
                }
            }
        }
        Ok(manifest)
    }

    pub fn stages(&self) -> &[StageDecl] {
        &self.stages
    }

    pub fn transitions(&self) -> &[TransitionDecl] {
        &self.transitions
    }

    fn find_stage(&self, stage: StageId) -> Result<&StageDecl, TransitionError> {
        self.stages
            .binary_search_by_key(&stage, |s| s.stage_id)
            .map(|i| &self.stages[i])
            .map_err(|_| TransitionError::UnknownStage { stage })
    }

    fn incoming(&self, stage: StageId) -> impl Iterator<Item = &TransitionDecl> {
        self.transitions.iter().filter(move |t| t.to == stage)
    }

    /// Stage IDs and transitions must be exactly the DAG's sessions and edges.
    fn check_dag(&self, sessions: &[SessionScores]) -> Result<(), TransitionError> {
        let edges: BTreeSet<(StageId, StageId)> = sessions
            .iter()
            .flat_map(|s| s.depends_on.iter().map(move |&dep| (dep, s.session_id)))
            .collect();
        let nodes: BTreeSet<StageId> = sessions.iter().map(|s| s.session_id).collect();
        if let Some(stage) = self.stages.iter().find(|s| !nodes.contains(&s.stage_id)) {
            return Err(TransitionError::StageNotInDag {
                stage: stage.stage_id,
            });
        }
        if let Some(&session) = nodes.iter().find(|&&id| self.find_stage(id).is_err()) {
            return Err(TransitionError::SessionWithoutStage { session });
        }
        if let Some(t) = self
            .transitions
            .iter()
            .find(|t| !edges.contains(&(t.from, t.to)))
        {
            return Err(TransitionError::TransitionNotInDag {
                from: t.from,
                to: t.to,
            });
        }
        if let Some(&(from, to)) = edges.iter().find(|&&(from, to)| {
            !self
                .transitions
                .iter()
                .any(|t| t.from == from && t.to == to)
        }) {
            return Err(TransitionError::MissingTransition { from, to });
        }
        Ok(())
    }
}

/// What the agent did in one stage, as reported by the caller.
#[derive(Debug, Clone, PartialEq)]
pub struct StageRecord {
    pub stage_id: StageId,
    pub closing: PortfolioState,
    /// Memory artifacts this stage wrote, by name.
    pub memory_written: BTreeMap<String, String>,
    /// Memory artifact names this stage read from earlier stages.
    pub memory_read: BTreeSet<String>,
    /// Fact versions the stage's decisions used.
    pub facts_used: Vec<FactRef>,
    /// Caller-detected safety failures (for example a constitution breach).
    pub reported_safety_failures: Vec<String>,
}

impl StageRecord {
    /// A record with a closing portfolio and nothing else.
    pub fn new(stage_id: StageId, closing: PortfolioState) -> Self {
        Self {
            stage_id,
            closing,
            memory_written: BTreeMap::new(),
            memory_read: BTreeSet::new(),
            facts_used: Vec::new(),
            reported_safety_failures: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct IncomingTransition {
    pub from: StageId,
    pub mode: CarryoverMode,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CarriedArtifact {
    pub from: StageId,
    pub name: String,
    pub content: String,
}

/// Everything stage `k` may see at its effective date.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct StageObservation {
    pub stage_id: StageId,
    pub effective_date: EffectiveDate,
    pub incoming: Vec<IncomingTransition>,
    pub opening_portfolio: PortfolioState,
    /// Allowed artifacts that a predecessor actually wrote, in `(from, name)` order.
    pub carried_memory: Vec<CarriedArtifact>,
    /// Latest version of each fact available on or before `effective_date`.
    pub facts: Vec<FactVersion>,
}

/// Build the point-in-time observation for `stage`.
///
/// Reads the manifest, the records of `stage`'s declared predecessors only, and
/// fact versions available on or before `stage`'s effective date.
///
/// # Errors
///
/// Unknown stage, record set that is not exactly one record per manifest stage,
/// or a declared fact reference absent from the log.
pub fn observe_stage(
    manifest: &ScenarioManifest,
    records: &[StageRecord],
    facts: &FactLog,
    stage: StageId,
) -> Result<StageObservation, TransitionError> {
    let records = index_records(manifest, records)?;
    observe(manifest, &records, facts, stage)
}

fn index_records<'a>(
    manifest: &ScenarioManifest,
    records: &'a [StageRecord],
) -> Result<BTreeMap<StageId, &'a StageRecord>, TransitionError> {
    let mut by_id = BTreeMap::new();
    for record in records {
        manifest
            .find_stage(record.stage_id)
            .map_err(|_| TransitionError::UnknownStageRecord {
                stage: record.stage_id,
            })?;
        if by_id.insert(record.stage_id, record).is_some() {
            return Err(TransitionError::DuplicateStageRecord {
                stage: record.stage_id,
            });
        }
    }
    if let Some(stage) = manifest
        .stages
        .iter()
        .find(|s| !by_id.contains_key(&s.stage_id))
    {
        return Err(TransitionError::MissingStageRecord {
            stage: stage.stage_id,
        });
    }
    Ok(by_id)
}

fn observe(
    manifest: &ScenarioManifest,
    records: &BTreeMap<StageId, &StageRecord>,
    facts: &FactLog,
    stage: StageId,
) -> Result<StageObservation, TransitionError> {
    let decl = manifest.find_stage(stage)?;
    if let Some(fact) = decl.fact_refs.iter().find(|f| !facts.contains(f)) {
        return Err(TransitionError::UnknownFactReference {
            stage,
            fact: fact.clone(),
        });
    }
    let cutoff = decl.effective_date;
    let mut incoming = Vec::new();
    let mut carried_memory = Vec::new();
    let mut opening_portfolio = decl.initial_portfolio.clone();
    for t in manifest.incoming(stage) {
        let mode = t
            .carryover_mode
            .ok_or(TransitionError::UndeclaredCarryoverMode {
                from: t.from,
                to: t.to,
            })?;
        let predecessor = records[&t.from];
        if mode == CarryoverMode::ContinuousPortfolio {
            opening_portfolio = Some(predecessor.closing.clone());
        }
        incoming.push(IncomingTransition { from: t.from, mode });
        for name in &t.allowed_memory {
            if let Some(content) = predecessor.memory_written.get(name) {
                carried_memory.push(CarriedArtifact {
                    from: t.from,
                    name: name.clone(),
                    content: content.clone(),
                });
            }
        }
    }
    Ok(StageObservation {
        stage_id: stage,
        effective_date: decl.effective_date,
        incoming,
        opening_portfolio: opening_portfolio
            .ok_or(TransitionError::MissingInitialPortfolio { stage })?,
        carried_memory,
        facts: facts.as_of(cutoff),
    })
}

/// A failure of a stage's own record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StageFailure {
    /// Caller-reported safety failure.
    Reported { label: String },
    /// One of the stage's own declared invariants does not hold.
    InvariantViolated { name: String },
    /// A fact version used was not yet available at the stage's effective date.
    LookaheadUse { fact: FactRef },
}

/// An earlier stage's invariant that this later stage broke.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreservationViolation {
    pub declared_by: StageId,
    pub obligation: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct StageOutcome {
    pub stage_id: StageId,
    pub effective_date: EffectiveDate,
    pub observation: StageObservation,
    pub closing_portfolio: PortfolioState,
    /// Closing minus opening marked value.
    pub stage_pnl: f64,
    pub own_failures: Vec<StageFailure>,
    /// Reported against this (later) stage; the declaring stage's row is untouched.
    pub preservation_violations: Vec<PreservationViolation>,
    /// No own failures and no preservation violations.
    pub clean: bool,
    /// Copied from the chain scorer's row for this session.
    pub lift: f64,
    pub qualified_retention: bool,
    pub conditioned_lift: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TransitionReport {
    /// The unchanged multi-session chain report for the same sessions.
    pub chain: MultiSessionReport,
    /// One row per stage in `(effective_date, stage_id)` order.
    pub stages: Vec<StageOutcome>,
    /// Every stage that is not clean, in the same order. Later success or credit
    /// never removes an entry.
    pub failed_stages: Vec<StageId>,
    pub scenario_clean: bool,
}

/// Score a multi-session chain under a scenario-transition manifest.
///
/// The chain part is [`multi_session_report`] on `sessions`, unchanged. The
/// manifest must name exactly the DAG's sessions and edges.
///
/// # Errors
///
/// Any chain-scorer refusal, a manifest that does not match the DAG, a record set
/// that is not one record per stage, an undeclared fact reference or use, a memory
/// read outside the allowed carryover or not produced upstream, and nonfinite PnL.
pub fn scenario_transition_report(
    sessions: &[SessionScores],
    manifest: &ScenarioManifest,
    records: &[StageRecord],
    facts: &FactLog,
    alpha: f64,
) -> Result<TransitionReport, TransitionError> {
    let chain = multi_session_report(sessions, alpha).map_err(TransitionError::Chain)?;
    manifest.check_dag(sessions)?;
    let records = index_records(manifest, records)?;
    let mut order: Vec<&StageDecl> = manifest.stages.iter().collect();
    order.sort_by_key(|s| (s.effective_date, s.stage_id));
    let mut stages = Vec::with_capacity(order.len());
    for decl in order {
        let stage = decl.stage_id;
        let record = records[&stage];
        let observation = observe(manifest, &records, facts, stage)?;
        for name in &record.memory_read {
            if observation.carried_memory.iter().any(|a| &a.name == name) {
                continue;
            }
            let allowed = manifest
                .incoming(stage)
                .any(|t| t.allowed_memory.contains(name));
            return Err(if allowed {
                TransitionError::UnproducedCarryover {
                    stage,
                    artifact: name.clone(),
                }
            } else {
                TransitionError::DisallowedCarryover {
                    stage,
                    artifact: name.clone(),
                }
            });
        }
        let mut own_failures: Vec<StageFailure> = record
            .reported_safety_failures
            .iter()
            .map(|label| StageFailure::Reported {
                label: label.clone(),
            })
            .collect();
        for fact in &record.facts_used {
            if !facts.contains(fact) {
                return Err(TransitionError::UnknownFactReference {
                    stage,
                    fact: fact.clone(),
                });
            }
            if fact.available_from > decl.effective_date {
                own_failures.push(StageFailure::LookaheadUse { fact: fact.clone() });
            }
        }
        own_failures.extend(
            decl.invariants
                .iter()
                .filter(|named| !named.invariant.holds(record))
                .map(|named| StageFailure::InvariantViolated {
                    name: named.name.clone(),
                }),
        );
        let mut preservation_violations = Vec::new();
        for t in manifest.incoming(stage) {
            let declaring = manifest.find_stage(t.from)?;
            for named in declaring
                .invariants
                .iter()
                .filter(|named| t.preserve.contains(&named.name))
            {
                if !named.invariant.holds(record) {
                    preservation_violations.push(PreservationViolation {
                        declared_by: t.from,
                        obligation: named.name.clone(),
                    });
                }
            }
        }
        let stage_pnl =
            record.closing.marked_value() - observation.opening_portfolio.marked_value();
        if !stage_pnl.is_finite() {
            return Err(TransitionError::NonFinitePnl { stage });
        }
        let row = chain
            .per_session
            .iter()
            .find(|row| row.session_id == stage)
            .ok_or(TransitionError::StageNotInDag { stage })?;
        let clean = own_failures.is_empty() && preservation_violations.is_empty();
        stages.push(StageOutcome {
            stage_id: stage,
            effective_date: decl.effective_date,
            observation,
            closing_portfolio: record.closing.clone(),
            stage_pnl,
            own_failures,
            preservation_violations,
            clean,
            lift: row.lift,
            qualified_retention: row.qualified_retention,
            conditioned_lift: row.conditioned_lift,
        });
    }
    let failed_stages: Vec<StageId> = stages
        .iter()
        .filter(|s| !s.clean)
        .map(|s| s.stage_id)
        .collect();
    Ok(TransitionReport {
        chain,
        scenario_clean: failed_stages.is_empty(),
        failed_stages,
        stages,
    })
}

/// Why a manifest, observation or transition report was refused.
#[derive(Debug, Clone, PartialEq)]
pub enum TransitionError {
    InvalidDate {
        text: String,
    },
    NonFinitePortfolio,
    DuplicateFactVersion {
        fact: FactRef,
    },
    DuplicateStage {
        stage: StageId,
    },
    DuplicateInvariant {
        stage: StageId,
        name: String,
    },
    InvalidInvariant {
        stage: StageId,
        name: String,
    },
    /// Point-in-time rule: a stage's script references a fact version dated after
    /// the stage's own effective date.
    FutureFactReference {
        stage: StageId,
        fact: FactRef,
        effective_date: EffectiveDate,
    },
    UnknownStage {
        stage: StageId,
    },
    DuplicateTransition {
        from: StageId,
        to: StageId,
    },
    UndeclaredCarryoverMode {
        from: StageId,
        to: StageId,
    },
    NonIncreasingEffectiveDate {
        from: StageId,
        to: StageId,
        from_date: EffectiveDate,
        to_date: EffectiveDate,
    },
    UnknownObligation {
        from: StageId,
        to: StageId,
        name: String,
    },
    MissingInitialPortfolio {
        stage: StageId,
    },
    UnexpectedInitialPortfolio {
        stage: StageId,
    },
    MultipleContinuousPredecessors {
        stage: StageId,
    },
    StageNotInDag {
        stage: StageId,
    },
    SessionWithoutStage {
        session: SessionId,
    },
    TransitionNotInDag {
        from: StageId,
        to: StageId,
    },
    MissingTransition {
        from: StageId,
        to: StageId,
    },
    UnknownStageRecord {
        stage: StageId,
    },
    DuplicateStageRecord {
        stage: StageId,
    },
    MissingStageRecord {
        stage: StageId,
    },
    UnknownFactReference {
        stage: StageId,
        fact: FactRef,
    },
    /// A stage read a memory artifact that no incoming transition allows.
    DisallowedCarryover {
        stage: StageId,
        artifact: String,
    },
    /// An allowed artifact was read but no predecessor on an allowing edge wrote it.
    UnproducedCarryover {
        stage: StageId,
        artifact: String,
    },
    NonFinitePnl {
        stage: StageId,
    },
    /// The unchanged multi-session chain scorer refused the sessions.
    Chain(String),
}

impl fmt::Display for TransitionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        use TransitionError as E;
        match self {
            E::InvalidDate { text } => write!(f, "invalid effective date {text:?}; expected a real YYYY-MM-DD date"),
            E::NonFinitePortfolio => write!(f, "portfolio cash, positions and totals must be finite"),
            E::DuplicateFactVersion { fact } => write!(f, "fact {} has two versions available from {}", fact.name, fact.available_from),
            E::DuplicateStage { stage } => write!(f, "stage {stage} is declared twice"),
            E::DuplicateInvariant { stage, name } => write!(f, "stage {stage} declares invariant {name:?} twice"),
            E::InvalidInvariant { stage, name } => write!(f, "stage {stage} invariant {name:?} needs a finite non-negative cap"),
            E::FutureFactReference { stage, fact, effective_date } => write!(f, "stage {stage} (effective {effective_date}) references fact {} available only from {}", fact.name, fact.available_from),
            E::UnknownStage { stage } => write!(f, "transition references undeclared stage {stage}"),
            E::DuplicateTransition { from, to } => write!(f, "transition {from} -> {to} is declared twice"),
            E::UndeclaredCarryoverMode { from, to } => write!(f, "transition {from} -> {to} declares no carryover mode"),
            E::NonIncreasingEffectiveDate { from, to, from_date, to_date } => write!(f, "effective dates must strictly increase along {from} -> {to}, got {from_date} then {to_date}"),
            E::UnknownObligation { from, to, name } => write!(f, "transition {from} -> {to} preserves {name:?}, which stage {from} does not declare"),
            E::MissingInitialPortfolio { stage } => write!(f, "stage {stage} has no continuous predecessor and no initial portfolio"),
            E::UnexpectedInitialPortfolio { stage } => write!(f, "stage {stage} carries a continuous portfolio and must not declare an initial one"),
            E::MultipleContinuousPredecessors { stage } => write!(f, "stage {stage} has more than one continuous-portfolio predecessor"),
            E::StageNotInDag { stage } => write!(f, "stage {stage} is not a session of the DAG"),
            E::SessionWithoutStage { session } => write!(f, "session {session} has no stage in the manifest"),
            E::TransitionNotInDag { from, to } => write!(f, "transition {from} -> {to} is not a DAG dependency edge"),
            E::MissingTransition { from, to } => write!(f, "DAG edge {from} -> {to} has no declared transition"),
            E::UnknownStageRecord { stage } => write!(f, "record for undeclared stage {stage}"),
            E::DuplicateStageRecord { stage } => write!(f, "stage {stage} has two records"),
            E::MissingStageRecord { stage } => write!(f, "stage {stage} has no record"),
            E::UnknownFactReference { stage, fact } => write!(f, "stage {stage} references fact {} from {}, which the log does not hold", fact.name, fact.available_from),
            E::DisallowedCarryover { stage, artifact } => write!(f, "stage {stage} read memory {artifact:?}, which no incoming transition allows"),
            E::UnproducedCarryover { stage, artifact } => write!(f, "stage {stage} read allowed memory {artifact:?}, which no allowing predecessor wrote"),
            E::NonFinitePnl { stage } => write!(f, "stage {stage} PnL is not finite"),
            E::Chain(error) => write!(f, "multi-session chain: {error}"),
        }
    }
}

impl std::error::Error for TransitionError {}
