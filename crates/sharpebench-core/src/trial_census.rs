//! Trial counts taken from the declared roster, published beside the score.
//!
//! A leaderboard row says how good an agent looked. It does not say how many
//! trials that verdict was supposed to rest on. The published failure mode in
//! neighbouring agent benchmarks is the same one every time: the denominator is
//! computed from whichever rows happen to be present, so a trial that failed, or
//! was dropped before assembly, silently leaves the denominator with it and the
//! survivors look better than the run was.
//!
//! [`crate::run_identity`] already refuses a *partial* grid, but the grid it
//! validates is the one the submitted keys describe. If a window failed for
//! every agent and nobody submitted it, the union of submitted cells is a
//! complete product over the remaining windows and the field parses as whole.
//! Nothing in the record then says the window existed.
//!
//! This module makes the expectation an input rather than an inference. A
//! [`TrialRoster`] is *declared* before the suite runs: which entrants, which
//! windows, which seeds. The census counts against that declaration:
//!
//! - `expected` is `agents x windows x seeds` from the roster, never a row count,
//! - `completed` counts declared cells whose single report says completed,
//! - `failed` counts declared cells whose report names a failure, with the reason,
//! - `unreported` counts declared cells for which no report arrived at all,
//! - every non-completion is named by a [`CensusGate`] carrying its reason.
//!
//! The property that makes this worth having: **a failed trial and a dropped
//! trial are both non-completions, so removing a failure improves nothing.**
//! Deleting the failed report moves one cell from `failed` to `unreported`;
//! `expected` is fixed by the roster, `completed` does not move, the
//! [`CohortIdentity`] the comparison is declared over does not move, and
//! [`AgentCensus::complete`] stays false. The only thing the deletion buys is a
//! less specific gate reason.
//!
//! Controls of this kind belong beside the score, not inside it:
//! [`attach_census`] joins a board to its census without touching
//! [`CompositeScore`], so no published score changes shape and an entrant the
//! board never carried a row for still appears, with its counts and its gates.
//!
//! Pure and deterministic: no I/O, no clock, no ambient randomness. This module
//! counts and classifies; nothing here rescores anything.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use serde::{Deserialize, Serialize};

use crate::composite::CompositeScore;
use crate::run_identity::RunKey;

/// Why a roster could not be declared. Both variants describe a declaration that
/// cannot fix an expectation: an empty axis expects nothing, and a repeated
/// member would be counted twice.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CensusError {
    /// One of the three declared axes is empty, so the product is zero and the
    /// roster expects no trials at all.
    EmptyAxis { axis: &'static str },
    /// The same entrant, window or seed was declared more than once.
    DuplicateDeclaration { axis: &'static str, value: String },
}

impl fmt::Display for CensusError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyAxis { axis } => write!(
                f,
                "trial census: the declared {axis} axis is empty, so the roster expects no trials"
            ),
            Self::DuplicateDeclaration { axis, value } => write!(
                f,
                "trial census: {axis} `{value}` is declared more than once"
            ),
        }
    }
}

impl std::error::Error for CensusError {}

/// The trials a suite declared it would run, fixed before it ran.
///
/// Entrant order is the declared order and is preserved on every output, so a
/// report reads in the order the operator wrote the roster. The cell axes are
/// sorted into canonical order, because a cell is an identity and not a
/// position.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TrialRoster {
    agents: Vec<String>,
    windows: Vec<String>,
    seeds: Vec<u64>,
}

impl TrialRoster {
    /// Declare a roster. Refuses an empty axis and a repeated member.
    pub fn declare(
        agents: &[String],
        windows: &[String],
        seeds: &[u64],
    ) -> Result<Self, CensusError> {
        check_unique("entrant", agents.iter().cloned())?;
        check_unique("window", windows.iter().cloned())?;
        check_unique("seed", seeds.iter().map(|s| s.to_string()))?;
        if agents.is_empty() {
            return Err(CensusError::EmptyAxis { axis: "entrant" });
        }
        if windows.is_empty() {
            return Err(CensusError::EmptyAxis { axis: "window" });
        }
        if seeds.is_empty() {
            return Err(CensusError::EmptyAxis { axis: "seed" });
        }
        let mut windows = windows.to_vec();
        windows.sort();
        let mut seeds = seeds.to_vec();
        seeds.sort_unstable();
        Ok(Self {
            agents: agents.to_vec(),
            windows,
            seeds,
        })
    }

    /// The declared entrants, in declared order.
    pub fn agents(&self) -> &[String] {
        &self.agents
    }

    /// Every declared cell, in canonical `(window, seed)` order.
    pub fn cells(&self) -> Vec<RunKey> {
        self.windows
            .iter()
            .flat_map(|window| {
                self.seeds.iter().map(move |seed| RunKey {
                    window: window.clone(),
                    seed: *seed,
                })
            })
            .collect()
    }

    /// Trials one entrant is expected to complete: `windows x seeds`.
    pub fn expected_per_agent(&self) -> usize {
        self.windows.len() * self.seeds.len()
    }

    /// Trials the whole suite is expected to complete: `entrants x windows x seeds`.
    pub fn expected(&self) -> usize {
        self.agents.len() * self.expected_per_agent()
    }

    /// The cohort this roster declares the comparison over.
    pub fn cohort(&self) -> CohortIdentity {
        let mut agents = self.agents.clone();
        agents.sort();
        CohortIdentity {
            agents,
            windows: self.windows.clone(),
            seeds: self.seeds.clone(),
        }
    }
}

fn check_unique<I: Iterator<Item = String>>(
    axis: &'static str,
    values: I,
) -> Result<(), CensusError> {
    let mut seen: BTreeSet<String> = BTreeSet::new();
    for value in values {
        if !seen.insert(value.clone()) {
            return Err(CensusError::DuplicateDeclaration { axis, value });
        }
    }
    Ok(())
}

/// The comparison cohort, as declared. Two censuses are over the same cohort
/// exactly when these compare equal, and this is a function of the roster alone,
/// so no arriving or missing report can move it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CohortIdentity {
    /// Declared entrants, sorted, so ordering of the declaration is not identity.
    pub agents: Vec<String>,
    /// Declared windows, sorted.
    pub windows: Vec<String>,
    /// Declared execution seeds, sorted.
    pub seeds: Vec<u64>,
}

/// What became of one declared trial.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum TrialOutcome {
    /// The trial ran to completion and produced the run the field carries.
    Completed,
    /// The trial did not produce a run. `reason` is the producer's own
    /// classification, reported verbatim.
    Failed { reason: String },
}

/// One trial's outcome, as reported by whatever produced the field.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TrialReport {
    pub agent_id: String,
    #[serde(flatten)]
    pub key: RunKey,
    #[serde(flatten)]
    pub outcome: TrialOutcome,
}

impl TrialReport {
    /// A completed trial.
    pub fn completed(agent_id: &str, window: &str, seed: u64) -> Self {
        Self {
            agent_id: agent_id.to_string(),
            key: RunKey {
                window: window.to_string(),
                seed,
            },
            outcome: TrialOutcome::Completed,
        }
    }

    /// A trial that failed, with the producer's reason.
    pub fn failed(agent_id: &str, window: &str, seed: u64, reason: &str) -> Self {
        Self {
            agent_id: agent_id.to_string(),
            key: RunKey {
                window: window.to_string(),
                seed,
            },
            outcome: TrialOutcome::Failed {
                reason: reason.to_string(),
            },
        }
    }
}

/// Why a declared trial is not a clean completion. Every variant names the cell;
/// none of them is inferred from the number of rows present.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "gate", rename_all = "snake_case")]
pub enum CensusGate {
    /// The producer reported the trial as failed, with this reason.
    TrialFailed {
        window: String,
        seed: u64,
        reason: String,
    },
    /// The roster declared the trial and no report arrived for it. This is where
    /// a dropped failure lands, which is why dropping one buys nothing.
    TrialUnreported { window: String, seed: u64 },
    /// Two or more reports arrived for the same declared cell, so its outcome is
    /// not established. Not counted as a completion.
    TrialReportedTwice { window: String, seed: u64 },
    /// A report arrived for a cell the roster never declared. Counted nowhere;
    /// an undeclared trial cannot pay for a declared one.
    OffRosterCell { window: String, seed: u64 },
}

/// One declared entrant's trial counts and the gates standing against it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentCensus {
    pub agent_id: String,
    /// `windows x seeds` from the roster. Never a row count.
    pub expected: usize,
    /// Declared cells with exactly one report, saying completed.
    pub completed: usize,
    /// Declared cells whose report names a failure.
    pub failed: usize,
    /// Declared cells no report arrived for.
    pub unreported: usize,
    /// Every non-completion and every off-roster report, in canonical cell order.
    pub gates: Vec<CensusGate>,
}

impl AgentCensus {
    /// Whether every declared trial completed. The comparison is against the
    /// roster's expectation, so a missing report is a shortfall rather than a
    /// smaller denominator.
    pub fn complete(&self) -> bool {
        self.completed == self.expected
    }

    /// Whether no gate stands against the entrant. Distinct from
    /// [`complete`](Self::complete): an off-roster report gates the entrant
    /// without reducing its completions.
    pub fn ungated(&self) -> bool {
        self.gates.is_empty()
    }
}

/// The suite's trial counts against its declared roster.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TrialCensus {
    /// The declared comparison cohort. A function of the roster alone.
    pub cohort: CohortIdentity,
    /// Declared trials: `entrants x windows x seeds`.
    pub expected: usize,
    /// Declared trials that completed.
    pub completed: usize,
    /// Declared trials reported as failed.
    pub failed: usize,
    /// Declared trials no report arrived for.
    pub unreported: usize,
    /// One entry per declared entrant, in declared order.
    pub per_agent: Vec<AgentCensus>,
    /// Entrants that reported trials without being on the roster, sorted. Their
    /// reports are counted nowhere.
    pub off_roster_agents: Vec<String>,
}

impl TrialCensus {
    /// The census row for one declared entrant.
    pub fn agent(&self, agent_id: &str) -> Option<&AgentCensus> {
        self.per_agent.iter().find(|a| a.agent_id == agent_id)
    }

    /// Whether every declared trial of every declared entrant completed with no
    /// gate anywhere.
    pub fn suite_complete(&self) -> bool {
        self.completed == self.expected
            && self.off_roster_agents.is_empty()
            && self.per_agent.iter().all(AgentCensus::ungated)
    }
}

/// Count a suite's reported trials against its declared roster.
///
/// Reports are matched to declared cells by identity. Anything the roster did
/// not declare is gated, not counted; anything the roster declared and no report
/// covers is counted as unreported, which is the same non-completion a failure
/// is. The order of `reports` does not affect any output.
pub fn census(roster: &TrialRoster, reports: &[TrialReport]) -> TrialCensus {
    let declared_cells: BTreeSet<RunKey> = roster.cells().into_iter().collect();
    let declared_agents: BTreeSet<&str> = roster.agents().iter().map(String::as_str).collect();

    let mut by_agent: BTreeMap<&str, BTreeMap<&RunKey, Vec<&TrialOutcome>>> = BTreeMap::new();
    let mut off_roster_agents: BTreeSet<String> = BTreeSet::new();
    for report in reports {
        if !declared_agents.contains(report.agent_id.as_str()) {
            off_roster_agents.insert(report.agent_id.clone());
            continue;
        }
        by_agent
            .entry(report.agent_id.as_str())
            .or_default()
            .entry(&report.key)
            .or_default()
            .push(&report.outcome);
    }

    let expected_per_agent = roster.expected_per_agent();
    let mut per_agent: Vec<AgentCensus> = Vec::with_capacity(roster.agents().len());
    for agent_id in roster.agents() {
        let reported = by_agent.remove(agent_id.as_str()).unwrap_or_default();
        let mut completed = 0usize;
        let mut failed = 0usize;
        let mut unreported = 0usize;
        let mut gates: Vec<CensusGate> = Vec::new();

        for key in roster.cells() {
            match reported.get(&key).map(Vec::as_slice) {
                None => {
                    unreported += 1;
                    gates.push(CensusGate::TrialUnreported {
                        window: key.window.clone(),
                        seed: key.seed,
                    });
                }
                Some([TrialOutcome::Completed]) => completed += 1,
                Some([TrialOutcome::Failed { reason }]) => {
                    failed += 1;
                    gates.push(CensusGate::TrialFailed {
                        window: key.window.clone(),
                        seed: key.seed,
                        reason: reason.to_string(),
                    });
                }
                Some(_) => gates.push(CensusGate::TrialReportedTwice {
                    window: key.window.clone(),
                    seed: key.seed,
                }),
            }
        }

        for key in reported.keys() {
            if !declared_cells.contains(*key) {
                gates.push(CensusGate::OffRosterCell {
                    window: key.window.clone(),
                    seed: key.seed,
                });
            }
        }

        per_agent.push(AgentCensus {
            agent_id: agent_id.clone(),
            expected: expected_per_agent,
            completed,
            failed,
            unreported,
            gates,
        });
    }

    TrialCensus {
        cohort: roster.cohort(),
        expected: roster.expected(),
        completed: per_agent.iter().map(|a| a.completed).sum(),
        failed: per_agent.iter().map(|a| a.failed).sum(),
        unreported: per_agent.iter().map(|a| a.unreported).sum(),
        per_agent,
        off_roster_agents: off_roster_agents.into_iter().collect(),
    }
}

/// A board row with its trial counts beside it.
///
/// `score` is optional because the board is built from the rows that exist and
/// the census is built from the roster that was declared. A declared entrant
/// whose every trial failed has no row and would otherwise vanish from the
/// published record entirely; here it appears, scoreless, with the counts and
/// the reasons.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CensusedScore {
    pub agent_id: String,
    /// The host row, absent when the scored field carried no submission for this
    /// declared entrant.
    pub score: Option<CompositeScore>,
    pub trials: AgentCensus,
    /// Host eligibility **and** a complete, ungated declared cohort. Monotone in
    /// the reports: no deletion can raise it.
    pub reported_eligible: bool,
}

/// Join a scored board to its census, in declared roster order.
///
/// Rows for agents the roster did not declare are dropped rather than ranked:
/// the published cohort is the declared one. Nothing about [`CompositeScore`] is
/// modified, so every published score keeps its shape and its value.
pub fn attach_census(board: &[CompositeScore], census: &TrialCensus) -> Vec<CensusedScore> {
    census
        .per_agent
        .iter()
        .map(|trials| {
            let score = board
                .iter()
                .find(|s| s.agent_id == trials.agent_id)
                .cloned();
            let reported_eligible = score.as_ref().is_some_and(|s| s.rank_eligible)
                && trials.complete()
                && trials.ungated();
            CensusedScore {
                agent_id: trials.agent_id.clone(),
                score,
                trials: trials.clone(),
                reported_eligible,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::composite::{rank, AgentSubmission, Run, ScoreConfig};

    fn ids(names: &[&str]) -> Vec<String> {
        names.iter().map(|n| (*n).to_string()).collect()
    }

    /// Two entrants, two windows, three seeds: twelve declared trials. The axes
    /// are deliberately unequal, so a sum of them is not the product.
    fn roster() -> TrialRoster {
        TrialRoster::declare(&ids(&["alpha", "beta"]), &ids(&["w1", "w2"]), &[7, 9, 11])
            .expect("a well formed roster")
    }

    fn all_completed() -> Vec<TrialReport> {
        let mut out = Vec::new();
        for agent in ["alpha", "beta"] {
            for window in ["w1", "w2"] {
                for seed in [7, 9, 11] {
                    out.push(TrialReport::completed(agent, window, seed));
                }
            }
        }
        out
    }

    #[test]
    fn expected_comes_from_the_roster_not_from_the_rows() {
        // Five of the eight declared trials reported anything at all. The
        // expectation is still eight, because the roster declared eight.
        let reports: Vec<TrialReport> = all_completed().into_iter().take(5).collect();
        let c = census(&roster(), &reports);
        assert_eq!(c.expected, 12, "{c:?}");
        assert_eq!(c.completed, 5, "{c:?}");
        assert_eq!(c.unreported, 7, "{c:?}");
    }

    #[test]
    fn a_failed_trial_is_not_a_completion() {
        let mut reports = all_completed();
        reports[0] = TrialReport::failed("alpha", "w1", 7, "gateway refused the journal");
        let c = census(&roster(), &reports);
        assert_eq!(c.completed, 11, "{c:?}");
        assert_eq!(c.failed, 1, "{c:?}");
        let alpha = c.agent("alpha").expect("declared entrant");
        assert_eq!(
            alpha.gates,
            vec![CensusGate::TrialFailed {
                window: "w1".to_string(),
                seed: 7,
                reason: "gateway refused the journal".to_string(),
            }],
            "the reason is carried, not just the count"
        );
    }

    /// The regression this module exists for. Deleting the failed report is the
    /// cheapest way to make a suite look whole; it must buy nothing.
    #[test]
    fn removing_a_failed_trial_cannot_improve_eligibility_or_the_cohort() {
        let mut with_failure = all_completed();
        with_failure[0] = TrialReport::failed("alpha", "w1", 7, "sim crashed");
        let dropped: Vec<TrialReport> = with_failure.iter().skip(1).cloned().collect();

        let before = census(&roster(), &with_failure);
        let after = census(&roster(), &dropped);

        assert_eq!(
            before.expected, after.expected,
            "the denominator is declared"
        );
        assert_eq!(before.completed, after.completed, "no completion appeared");
        assert_eq!(before.cohort, after.cohort, "the declared cohort is fixed");
        // The failure did not disappear; it changed name.
        assert_eq!((before.failed, before.unreported), (1, 0));
        assert_eq!((after.failed, after.unreported), (0, 1));

        let before_alpha = before.agent("alpha").expect("declared entrant");
        let after_alpha = after.agent("alpha").expect("declared entrant");
        assert!(!before_alpha.complete());
        assert!(
            !after_alpha.complete(),
            "deleting the failed report must not complete the cohort: {after_alpha:?}"
        );
    }

    #[test]
    fn an_undeclared_cell_is_not_a_completion() {
        let mut reports = all_completed();
        reports[0] = TrialReport::failed("alpha", "w1", 7, "sim crashed");
        // A trial on a window nobody declared, reported as a completion. It
        // cannot pay for the declared cell that failed.
        reports.push(TrialReport::completed("alpha", "w-easy", 7));

        let c = census(&roster(), &reports);
        assert_eq!(c.expected, 12);
        assert_eq!(c.completed, 11, "{c:?}");
        let alpha = c.agent("alpha").expect("declared entrant");
        assert!(!alpha.ungated(), "{alpha:?}");
        assert!(
            alpha.gates.contains(&CensusGate::OffRosterCell {
                window: "w-easy".to_string(),
                seed: 7,
            }),
            "{alpha:?}"
        );
    }

    #[test]
    fn an_off_roster_entrant_is_counted_nowhere() {
        let mut reports = all_completed();
        reports.push(TrialReport::completed("gatecrasher", "w1", 7));
        let c = census(&roster(), &reports);
        assert_eq!(c.off_roster_agents, vec!["gatecrasher".to_string()]);
        assert_eq!(
            (c.expected, c.completed),
            (12, 12),
            "an entrant the roster never declared moves no count: {c:?}"
        );
        assert!(c.agent("gatecrasher").is_none());
        assert!(
            !c.suite_complete(),
            "every declared trial completed, but an undeclared entrant reported              into the suite, so the suite is not clean: {c:?}"
        );
    }

    #[test]
    fn a_cell_reported_twice_is_not_established() {
        let mut reports = all_completed();
        reports.push(TrialReport::completed("alpha", "w1", 7));
        let c = census(&roster(), &reports);
        assert_eq!(
            c.completed, 11,
            "the ambiguous cell counts as no completion"
        );
        let alpha = c.agent("alpha").expect("declared entrant");
        assert!(alpha.gates.contains(&CensusGate::TrialReportedTwice {
            window: "w1".to_string(),
            seed: 7,
        }));
        assert!(!alpha.complete());
    }

    #[test]
    fn the_declared_cohort_ignores_declaration_order() {
        let a = TrialRoster::declare(&ids(&["alpha", "beta"]), &ids(&["w1", "w2"]), &[7, 9])
            .expect("roster");
        let b = TrialRoster::declare(&ids(&["beta", "alpha"]), &ids(&["w2", "w1"]), &[9, 7])
            .expect("roster");
        assert_eq!(a.cohort(), b.cohort());
        // Declared order still drives the report order.
        assert_eq!(census(&a, &[]).per_agent[0].agent_id, "alpha");
        assert_eq!(census(&b, &[]).per_agent[0].agent_id, "beta");
    }

    #[test]
    fn a_roster_refuses_an_empty_axis_and_a_repeat() {
        assert_eq!(
            TrialRoster::declare(&ids(&["alpha"]), &[], &[7]),
            Err(CensusError::EmptyAxis { axis: "window" })
        );
        assert_eq!(
            TrialRoster::declare(&ids(&["alpha", "alpha"]), &ids(&["w1"]), &[7]),
            Err(CensusError::DuplicateDeclaration {
                axis: "entrant",
                value: "alpha".to_string(),
            })
        );
    }

    fn submission(id: &str, mean: f64) -> AgentSubmission {
        AgentSubmission {
            agent_id: id.to_string(),
            runs: (0..4)
                .map(|_| Run {
                    returns: (0..60)
                        .map(|i| mean + 0.0005 * (i as f64 * 0.7).sin())
                        .collect(),
                    ..Run::default()
                })
                .collect(),
            ..AgentSubmission::default()
        }
    }

    #[test]
    fn counts_travel_beside_the_score_and_a_shortfall_withholds_eligibility() {
        let board = rank(
            &[submission("alpha", 0.002), submission("beta", 0.0019)],
            &ScoreConfig::default(),
        );
        assert!(
            board
                .iter()
                .find(|s| s.agent_id == "alpha")
                .expect("the board carried alpha")
                .rank_eligible,
            "the fixture must make alpha host-eligible, or withholding proves nothing: {board:?}"
        );

        let whole = attach_census(&board, &census(&roster(), &all_completed()));
        let alpha_whole = &whole[0];
        assert_eq!(alpha_whole.agent_id, "alpha");
        assert_eq!(alpha_whole.trials.expected, 6);
        assert_eq!(alpha_whole.trials.completed, 6);
        assert!(
            alpha_whole.reported_eligible,
            "with a whole declared cohort the host verdict stands unchanged: {alpha_whole:?}"
        );

        // Now drop one of alpha's declared trials. The host score is identical;
        // the published eligibility is withheld because the cohort is short.
        let short: Vec<TrialReport> = all_completed()
            .into_iter()
            .filter(|r| !(r.agent_id == "alpha" && r.key.window == "w2" && r.key.seed == 9))
            .collect();
        let gated = attach_census(&board, &census(&roster(), &short));
        let alpha_gated = &gated[0];
        assert_eq!(
            alpha_gated.score, alpha_whole.score,
            "the score does not move"
        );
        assert!(
            !alpha_gated.reported_eligible,
            "an incomplete declared cohort cannot report as eligible: {alpha_gated:?}"
        );
    }

    #[test]
    fn a_whole_cohort_cannot_confer_the_eligibility_the_host_withheld() {
        // The counts are a second gate on the host verdict, never a substitute
        // for it: a complete, ungated cohort published beside an ineligible row
        // must still read as ineligible.
        let board = rank(&[submission("loser", -0.003)], &ScoreConfig::default());
        let host = &board[0];
        assert!(
            !host.rank_eligible,
            "the fixture must be host-ineligible, or the join proves nothing: {host:?}"
        );

        let one = TrialRoster::declare(&ids(&["loser"]), &ids(&["w1", "w2"]), &[7, 9, 11])
            .expect("roster");
        let reports: Vec<TrialReport> = one
            .cells()
            .into_iter()
            .map(|k| TrialReport::completed("loser", &k.window, k.seed))
            .collect();
        let rows = attach_census(&board, &census(&one, &reports));
        assert!(rows[0].trials.complete() && rows[0].trials.ungated());
        assert!(
            !rows[0].reported_eligible,
            "a complete cohort cannot promote a host-ineligible row: {:?}",
            rows[0]
        );
    }

    #[test]
    fn a_declared_entrant_with_no_row_still_appears_with_its_counts() {
        // The board is built from the rows that exist; `beta` never submitted.
        let board = rank(&[submission("alpha", 0.002)], &ScoreConfig::default());
        let reports: Vec<TrialReport> = all_completed()
            .into_iter()
            .filter(|r| r.agent_id == "alpha")
            .collect();
        let rows = attach_census(&board, &census(&roster(), &reports));
        let beta = rows.iter().find(|r| r.agent_id == "beta").expect(
            "a declared entrant absent from the board must still be published with its counts",
        );
        assert!(beta.score.is_none());
        assert_eq!(beta.trials.expected, 6);
        assert_eq!(beta.trials.completed, 0);
        assert_eq!(beta.trials.unreported, 6);
        assert!(!beta.reported_eligible);
    }

    #[test]
    fn a_whole_suite_reports_complete() {
        let c = census(&roster(), &all_completed());
        assert!(c.suite_complete(), "{c:?}");
        assert_eq!(
            (c.expected, c.completed, c.failed, c.unreported),
            (12, 12, 0, 0)
        );
    }
}
