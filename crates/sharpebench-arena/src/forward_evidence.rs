//! The forward evidence ledger: an anytime-valid e-process per committed
//! artifact, carried from board to board in the signed window headers.
//!
//! Each forward window is ranked on its own data, and a window lasts weeks, so
//! a per-window verdict has almost no power and nothing carries over. This
//! ledger lets forward evidence accumulate. For every artifact digest the arena
//! has re-executed, it keeps the wealth of a fractional bet on that artifact's
//! own arena-derived returns (the betting e-process in
//! [`sharpebench_stats::forward_eprocess`], whose module docs state the rule and
//! prove that the wealth is a nonnegative supermartingale under the null), and
//! over the family of every artifact ever committed it publishes the e-BH
//! decisions at [`FORWARD_EVIDENCE_LEVEL`]. It is rank-neutral: no score, row or
//! ordering reads it, and pass^k and the deflated Sharpe are reported beside
//! it unchanged.
//!
//! # What is observed
//!
//! - **Only re-executed rows.** An observation is the pooled, seed-averaged
//!   return per bar of a row whose returns the arena re-derived by running the
//!   committed artifact again ([`ReturnsProvenance::ReExecuted`]). Supplied and
//!   replayed rows are never observed: their returns are not shown to come
//!   from the committed artifact without hindsight. Their commitments still
//!   join the family.
//! - **Per artifact digest.** The process belongs to the digest, not the agent
//!   id, so an artifact entered in many windows is one process. Two agents
//!   revealing the same digest in one window are one observation (the first by
//!   agent id); re-execution of one artifact on one dataset is the same track.
//! - **Disjoint, ordered windows.** Observations run in publish order. A window
//!   whose first scored bar is not dated after the ledger's last observed bar is
//!   not applied (`not_applied` says why): overlapping data would repeat bars
//!   the bettor has already seen, and a repeated bar has no conditional mean of
//!   zero. Dates compare as strings, which orders ISO 8601 dates.
//!
//! # The loss bound
//!
//! The bet is capped at `1 / (2 B)` for a per-bar loss bound `B`. The default
//! is [`DEFAULT_LOSS_BOUND`], a bar that loses the whole account. A capture
//! that declares the `drawdown_capped` mandate with bound `D` in `(0, 1]` gives
//! `B = D`: the per-run drawdown is measured on the compounded equity curve,
//! so a drawdown of at most `D` means no bar loses more than `D`. The bound
//! must be fixed before the bars it caps, or the cap could be chosen from them,
//! so the rule is: an artifact's bound is fixed by the first window in which it
//! declares one, and takes effect from its next window. Until then the default
//! applies, and later declarations change nothing. A bar below the bound in
//! force zeroes the artifact's wealth for good (see the stats module for why
//! that keeps the guarantee).
//!
//! The commitment does not bind the declared mandate; the capture carries it.
//! A tighter bound than the artifact's real losses can only zero its own
//! wealth, never raise any wealth, so it cannot manufacture evidence.
//!
//! # The family
//!
//! The family is every artifact committed to an evidence-bearing window: each
//! digest the ledger has observed, plus each commitment that produced no
//! observation (refused, unrevealed, or not re-executed), counted by its
//! commitment hash at unit e-value. Nothing leaves it. An operator who abandons
//! an artifact and commits a fresh one leaves the old one in the family at its
//! last wealth, so restarting until something gets lucky raises the bar the
//! e-BH step-up applies. A commitment counted unresolved is never merged with
//! a digest observed later, which can only make the family larger.
//!
//! # Where it is signed
//!
//! Every board this binary publishes carries a [`ForwardEvidence`] in its
//! header: the ledger before the window (`prior`), the window's commitments and
//! observations, and the ledger after it (`posterior`). The next header's
//! `prior` is this header's `posterior`, so the running e-values travel down
//! the same chain as the boards. `verify_arena` recomputes every posterior
//! from its prior and the signed observations, bit for bit, and fails a board
//! whose ledger does not follow, or a board without one after the ledger began.
//! Boards signed before the ledger existed carry none and verify as before.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use sharpebench_attest::Commitment;
use sharpebench_core::composite::pooled_returns;
use sharpebench_core::AgentSubmission;
use sharpebench_protocol::DeclaredMandate;
use sharpebench_stats::forward_eprocess::{e_benjamini_hochberg, BettorState, DEFAULT_LOSS_BOUND};

use crate::intake::{ReplayWindow, ReturnsProvenance};
use crate::validate_sha256;

/// `schema` of every [`ForwardLedger`] and [`ForwardEvidence`] this binary
/// writes. A change to the bet rule, the family or the level is a new schema.
pub const FORWARD_EVIDENCE_SCHEMA: &str = "sharpebench-arena-forward-evidence/v1";

/// The false-discovery level the ledger's e-BH decisions are taken at,
/// declared here before any evidence exists. It is part of the schema.
pub const FORWARD_EVIDENCE_LEVEL: f64 = 0.05;

/// One re-executed entrant's observations in one window, recorded at scoring.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ForwardObservation {
    pub agent_id: String,
    /// The digest the entrant's commitment opened to.
    pub artifact_digest: String,
    /// The `drawdown_capped` bound the capture declared, when it declared one
    /// in `(0, 1]`. Omitted otherwise.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub declared_loss_bound: Option<f64>,
    /// The pooled, seed-averaged return per bar, in bar order.
    pub returns: Vec<f64>,
}

/// What one scored window contributes to the ledger. Recorded in the window
/// file at scoring when at least one row was re-executed, and signed into the
/// header at publish.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ForwardRecord {
    /// Date label of the first bar that produced a return.
    pub first_bar_date: String,
    /// Date label of the last bar of the revealed dataset.
    pub last_bar_date: String,
    /// One per observed digest, ordered by agent id.
    pub entrants: Vec<ForwardObservation>,
}

/// One artifact's process, as the ledger publishes it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ArtifactEvidence {
    /// Windows that observed this artifact.
    pub windows: u64,
    pub last_window_id: String,
    /// The bound fixed by the artifact's first declaration. Omitted until it
    /// declares one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub declared_loss_bound: Option<f64>,
    /// The bound that will cap its next window: the declared one, else
    /// [`DEFAULT_LOSS_BOUND`].
    pub loss_bound_in_force: f64,
    pub bars: u64,
    pub sum: f64,
    pub sum_sq: f64,
    /// The e-process: current wealth.
    pub e_value: f64,
    pub max_e_value: f64,
    /// `1 / max_e_value`, valid at every bar at once.
    pub anytime_p_value: f64,
    /// A bar fell below the bound in force; the wealth is 0 for good.
    pub breached: bool,
}

impl ArtifactEvidence {
    fn new(window_id: &str) -> Self {
        let mut evidence = Self {
            windows: 0,
            last_window_id: window_id.to_string(),
            declared_loss_bound: None,
            loss_bound_in_force: DEFAULT_LOSS_BOUND,
            bars: 0,
            sum: 0.0,
            sum_sq: 0.0,
            e_value: 0.0,
            max_e_value: 0.0,
            anytime_p_value: 0.0,
            breached: false,
        };
        evidence.set_bettor(BettorState::new());
        evidence
    }

    fn bettor(&self) -> BettorState {
        BettorState {
            bars: self.bars,
            sum: self.sum,
            sum_sq: self.sum_sq,
            wealth: self.e_value,
            max_wealth: self.max_e_value,
            breached: self.breached,
        }
    }

    fn set_bettor(&mut self, state: BettorState) {
        self.bars = state.bars;
        self.sum = state.sum;
        self.sum_sq = state.sum_sq;
        self.e_value = state.wealth;
        self.max_e_value = state.max_wealth;
        self.breached = state.breached;
        self.anytime_p_value = state.anytime_p_value();
    }
}

/// The ledger after some number of evidence-bearing windows.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ForwardLedger {
    /// Always [`FORWARD_EVIDENCE_SCHEMA`].
    pub schema: String,
    /// Always [`FORWARD_EVIDENCE_LEVEL`].
    pub level: f64,
    /// Evidence-bearing windows folded in.
    pub windows: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_window_id: Option<String>,
    /// The last observed bar's date. Omitted until a window is applied.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_bar_date: Option<String>,
    /// Every observed artifact, by digest.
    pub artifacts: BTreeMap<String, ArtifactEvidence>,
    /// Commitment hashes that produced no observation, each a family member at
    /// unit e-value.
    pub unresolved_commitments: BTreeSet<String>,
    /// `artifacts` plus `unresolved_commitments`.
    pub family_size: u64,
    /// The digests e-BH rejects at `level` over the whole family, in digest
    /// order.
    pub discoveries: Vec<String>,
}

impl ForwardLedger {
    /// The ledger before any evidence-bearing window.
    pub fn genesis() -> Self {
        Self {
            schema: FORWARD_EVIDENCE_SCHEMA.to_string(),
            level: FORWARD_EVIDENCE_LEVEL,
            windows: 0,
            last_window_id: None,
            last_bar_date: None,
            artifacts: BTreeMap::new(),
            unresolved_commitments: BTreeSet::new(),
            family_size: 0,
            discoveries: Vec::new(),
        }
    }

    /// Recompute `family_size`, `discoveries` and each entry's derived fields.
    fn derive(&mut self) -> Result<(), String> {
        for evidence in self.artifacts.values_mut() {
            evidence.loss_bound_in_force =
                evidence.declared_loss_bound.unwrap_or(DEFAULT_LOSS_BOUND);
            evidence.anytime_p_value = evidence.bettor().anytime_p_value();
        }
        let mut e_values: Vec<f64> = self.artifacts.values().map(|e| e.e_value).collect();
        e_values.extend(std::iter::repeat_n(1.0, self.unresolved_commitments.len()));
        self.family_size = e_values.len() as u64;
        let rejected = e_benjamini_hochberg(&e_values, self.level)
            .map_err(|e| format!("forward ledger e-BH: {e}"))?;
        self.discoveries = self
            .artifacts
            .keys()
            .zip(&rejected)
            .filter(|(_, rejected)| **rejected)
            .map(|(digest, _)| digest.clone())
            .collect();
        Ok(())
    }

    /// The schema, the level and every process are ones this binary writes.
    fn check(&self) -> Result<(), String> {
        if self.schema != FORWARD_EVIDENCE_SCHEMA {
            return Err(format!(
                "forward ledger schema `{}` is not `{FORWARD_EVIDENCE_SCHEMA}`",
                self.schema
            ));
        }
        if self.level != FORWARD_EVIDENCE_LEVEL {
            return Err(format!(
                "forward ledger level {} is not {FORWARD_EVIDENCE_LEVEL}",
                self.level
            ));
        }
        for (digest, evidence) in &self.artifacts {
            evidence
                .bettor()
                .check()
                .map_err(|e| format!("forward ledger artifact `{digest}`: {e}"))?;
            if let Some(bound) = evidence.declared_loss_bound {
                valid_declared_bound(bound).ok_or_else(|| {
                    format!("forward ledger artifact `{digest}` declares bound {bound}")
                })?;
            }
        }
        Ok(())
    }
}

/// The ledger update one published window carries in its signed header.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ForwardEvidence {
    /// Always [`FORWARD_EVIDENCE_SCHEMA`].
    pub schema: String,
    /// The ledger before this window: the previous board's `posterior`, or
    /// [`ForwardLedger::genesis`] on the first evidence-bearing board.
    pub prior: ForwardLedger,
    /// The window's commitments, every one a family member.
    pub commitments: Vec<Commitment>,
    /// The window's observations, as its file records them. Omitted when no
    /// row was re-executed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub record: Option<ForwardRecord>,
    /// Why `record` was not folded in. Omitted when it was, or when there is
    /// none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub not_applied: Option<String>,
    /// The ledger after this window.
    pub posterior: ForwardLedger,
}

/// A `drawdown_capped` bound the ledger accepts as a loss bound.
fn valid_declared_bound(bound: f64) -> Option<f64> {
    (bound > 0.0 && bound <= 1.0).then_some(bound)
}

/// The loss bound a capture's declared mandate implies, when it implies one.
pub fn declared_loss_bound(mandate: Option<&DeclaredMandate>) -> Option<f64> {
    match mandate {
        Some(DeclaredMandate::DrawdownCapped {
            max_per_run_drawdown,
        }) => valid_declared_bound(*max_per_run_drawdown),
        _ => None,
    }
}

/// One admitted entrant as the recorder sees it.
pub(crate) struct Admitted<'a> {
    pub agent_id: &'a str,
    pub artifact_digest: &'a str,
    pub mandate: Option<&'a DeclaredMandate>,
    pub submission: &'a AgentSubmission,
    pub provenance: ReturnsProvenance,
}

/// The record a scored window keeps: every re-executed entrant with a finite
/// pooled track, one per digest, ordered by agent id. `None` when there is
/// none, so a window with no re-executed row keeps the bytes it always had.
pub(crate) fn record_window(
    replay: Option<&ReplayWindow>,
    seeds_per_window: usize,
    admitted: &[Admitted<'_>],
) -> Option<ForwardRecord> {
    let replay = replay?;
    let first = replay.windows.first()?.start;
    let last = replay.windows.last()?.end.checked_sub(1)?;
    let first_bar_date = replay.data.dates.get(first)?.clone();
    let last_bar_date = replay.data.dates.get(last)?.clone();
    let mut by_agent: Vec<&Admitted<'_>> = admitted
        .iter()
        .filter(|a| a.provenance == ReturnsProvenance::ReExecuted)
        .collect();
    by_agent.sort_by(|a, b| a.agent_id.cmp(b.agent_id));
    let mut seen = BTreeSet::new();
    let mut entrants = Vec::new();
    for a in by_agent {
        let returns = pooled_returns(a.submission, seeds_per_window);
        if returns.is_empty() || returns.iter().any(|x| !x.is_finite()) {
            continue;
        }
        if !seen.insert(a.artifact_digest) {
            continue;
        }
        entrants.push(ForwardObservation {
            agent_id: a.agent_id.to_string(),
            artifact_digest: a.artifact_digest.to_string(),
            declared_loss_bound: declared_loss_bound(a.mandate),
            returns,
        });
    }
    (!entrants.is_empty()).then_some(ForwardRecord {
        first_bar_date,
        last_bar_date,
        entrants,
    })
}

/// A record is one [`record_window`] can write.
fn check_record(record: &ForwardRecord) -> Result<(), String> {
    if record.entrants.is_empty() {
        return Err("forward record has no entrant".to_string());
    }
    if record.first_bar_date > record.last_bar_date {
        return Err("forward record's first bar is dated after its last".to_string());
    }
    let mut digests = BTreeSet::new();
    for pair in record.entrants.windows(2) {
        if pair[0].agent_id >= pair[1].agent_id {
            return Err("forward record entrants are not in strict agent id order".to_string());
        }
    }
    for entrant in &record.entrants {
        validate_sha256("forward record artifact", &entrant.artifact_digest)?;
        if !digests.insert(&entrant.artifact_digest) {
            return Err(format!(
                "forward record repeats artifact `{}`",
                entrant.artifact_digest
            ));
        }
        if entrant.returns.is_empty() || entrant.returns.iter().any(|x| !x.is_finite()) {
            return Err(format!(
                "forward record entrant `{}` has no finite track",
                entrant.agent_id
            ));
        }
        if let Some(bound) = entrant.declared_loss_bound {
            valid_declared_bound(bound).ok_or_else(|| {
                format!(
                    "forward record entrant `{}` declares bound {bound}",
                    entrant.agent_id
                )
            })?;
        }
    }
    Ok(())
}

/// Fold one published window into `prior`: the whole ledger rule. Returns the
/// signed update. The arena calls this at publish, and `verify_arena` calls it
/// again on the signed prior and observations to check the posterior.
pub fn fold_window(
    prior: &ForwardLedger,
    window_id: &str,
    commitments: &[Commitment],
    record: Option<&ForwardRecord>,
) -> Result<ForwardEvidence, String> {
    prior.check()?;
    if let Some(record) = record {
        check_record(record)?;
    }
    let mut posterior = prior.clone();
    let not_applied = match (record, &prior.last_bar_date) {
        (Some(record), Some(last)) if record.first_bar_date <= *last => Some(format!(
            "the window's first scored bar ({}) is not after the ledger's last observed bar ({last}), so its data overlaps bars already observed",
            record.first_bar_date
        )),
        _ => None,
    };
    let applied = record.filter(|_| not_applied.is_none());
    let observed: BTreeSet<&str> = applied
        .map(|r| r.entrants.iter().map(|e| e.agent_id.as_str()).collect())
        .unwrap_or_default();
    for commitment in commitments {
        if !observed.contains(commitment.agent_id.as_str()) {
            posterior
                .unresolved_commitments
                .insert(commitment.commit_hash.clone());
        }
    }
    if let Some(record) = applied {
        for entrant in &record.entrants {
            let evidence = posterior
                .artifacts
                .entry(entrant.artifact_digest.clone())
                .or_insert_with(|| ArtifactEvidence::new(window_id));
            // The bound fixed by an earlier window; this window's declaration
            // can only take effect from the next one.
            let bound = evidence.declared_loss_bound.unwrap_or(DEFAULT_LOSS_BOUND);
            let mut bettor = evidence.bettor();
            for (bar, x) in entrant.returns.iter().enumerate() {
                bettor.step(*x, bound).map_err(|e| {
                    format!("forward ledger, `{}` bar {bar}: {e}", entrant.agent_id)
                })?;
            }
            evidence.set_bettor(bettor);
            if evidence.declared_loss_bound.is_none() {
                evidence.declared_loss_bound = entrant.declared_loss_bound;
            }
            evidence.windows += 1;
            evidence.last_window_id = window_id.to_string();
        }
        posterior.last_bar_date = Some(record.last_bar_date.clone());
    }
    posterior.windows += 1;
    posterior.last_window_id = Some(window_id.to_string());
    posterior.derive()?;
    Ok(ForwardEvidence {
        schema: FORWARD_EVIDENCE_SCHEMA.to_string(),
        prior: prior.clone(),
        commitments: commitments.to_vec(),
        record: record.cloned(),
        not_applied,
        posterior,
    })
}

/// Why `evidence`, signed on a board whose predecessor's ledger is
/// `expected_prior`, does not follow: a prior that is not the predecessor's
/// posterior, an observation of a row the board did not re-execute, or a
/// posterior that does not recompute. `None` when it follows.
pub(crate) fn evidence_error(
    evidence: &ForwardEvidence,
    expected_prior: &ForwardLedger,
    window_id: &str,
    reexecuted_rows: &BTreeSet<String>,
) -> Option<String> {
    if evidence.schema != FORWARD_EVIDENCE_SCHEMA {
        return Some(format!(
            "schema `{}` is not `{FORWARD_EVIDENCE_SCHEMA}`",
            evidence.schema
        ));
    }
    if evidence.prior != *expected_prior {
        return Some("the prior ledger is not the previous board's posterior".to_string());
    }
    if let Some(record) = &evidence.record {
        if let Some(entrant) = record
            .entrants
            .iter()
            .find(|e| !reexecuted_rows.contains(&e.agent_id))
        {
            return Some(format!(
                "`{}` is observed but has no re-executed row on this board",
                entrant.agent_id
            ));
        }
    }
    match fold_window(
        &evidence.prior,
        window_id,
        &evidence.commitments,
        evidence.record.as_ref(),
    ) {
        Err(error) => Some(error),
        Ok(recomputed) if recomputed != *evidence => Some(
            "the posterior ledger does not recompute from the prior and the observations"
                .to_string(),
        ),
        Ok(_) => None,
    }
}
