//! Decision stability: how often an agent decides differently when it saw the
//! same observations.
//!
//! pass^k across execution seeds certifies that an outcome repeats. It cannot
//! tell an agent whose decisions are a fixed function of what it saw from one
//! that samples a different choice each time it is asked, because execution
//! noise moves the fills, and with them every later observation, before the two
//! can be compared. This module measures the second property where it is
//! measurable: over replicate runs of one window (execution seeds of the window,
//! or repeated captures of the same seed), at the steps where the replicates
//! share their history.
//!
//! # Grouping
//!
//! Every step carries the SHA-256 of the observation it answered, taken over the
//! `sharpebench/canonical-json/v1` framed pre-image ([`observation_sha256`]).
//! Replicate runs of one window start in one group. At each step a group is split
//! by that step's observation digest, the members are compared, and each part is
//! then split again by the decision its members made. A group at step `t`
//! therefore holds the replicates that were shown equal observations at `t` and
//! at every earlier step, and made the same decisions at every earlier step.
//!
//! The observation history is required because the determinism contract lets a
//! decision depend on the run's earlier observations: two replicates that meet on
//! one observation after their histories differed may decide differently without
//! any non-determinism. The decision history is required because the contract
//! also lets a decision depend on the agent's own earlier decisions, and because
//! a difference the engine does not act on (a confidence, an action label, a
//! restated target) leaves the observations equal. Without it one such
//! difference would be counted again at every later step. Under this rule an
//! agent that honours the contract reports exactly zero.
//!
//! A replicate whose observation history no other member shares leaves the
//! comparison for the rest of the window, and its remaining steps are counted in
//! `steps_excluded_diverged_observation`. A replicate whose decision no other
//! member shared stays compared at that step and leaves afterwards, counted in
//! `steps_excluded_diverged_decision`. For every window with at least two
//! replicates, `steps_compared` plus the two excluded counts equals
//! `steps_total`. A window with a single replicate has nothing to compare
//! against: its steps are counted in `steps_unreplicated` and its rate is typed
//! unavailable.
//!
//! # What "differ" means
//!
//! Two decisions differ when their `orders` arrays have different lengths, or
//! when the orders at any one position differ in `symbol`, `action`,
//! `target_weight` or `confidence` ([`same_decision`]). A stated confidence
//! differs from an unstated one. The fields are the score-bearing order fields of
//! the re-execution contract, and they are compared exactly: a tolerance on
//! `target_weight` would be a free parameter the contract does not define, and
//! the engine executes the exact weight it is given. The one numeric relaxation is
//! that `0` and `-0` are equal, as they are in the canonical form the observation
//! digest uses. `reasoning` and each order's `rationale` are audit text, and the
//! `cost` report is spend rather than choice (token counts can vary between
//! samples that choose the same orders), so none of the three is compared.
//!
//! # The rate
//!
//! The headline rate is pairwise disagreement: over every compared group, the
//! number of replicate pairs whose decisions differ, divided by the number of
//! replicate pairs, `n (n - 1) / 2` for a group of `n`. If replicates choose
//! independently with probabilities `p_j`, a pair differs with probability
//! `1 - sum p_j^2` whatever the group size, so the rate does not move with how
//! many captures were supplied or how long histories stay shared. The share of
//! groups that differ does move with group size (`1 - (1 - q)^n - q^n` for a
//! binary choice with flip probability `q`), so the group counts and the
//! group-size distribution are reported as context only.
//!
//! The independent unit is the replicate run. Groups and pairs at later steps
//! reuse the same runs, so their counts are not sample sizes, and the rate
//! carries no interval.
//!
//! # Copies
//!
//! A replicate carries an identity of its recorded content. Two replicates of
//! one window with equal identities agree by construction if one is a copy of
//! the other, so the report counts them and refuses them unless the caller
//! declares that they are separate executions (a deterministic agent captured
//! twice produces equal content).
//!
//! # Rank neutrality
//!
//! The report is a separate document with `rank_input: false`. Nothing in
//! [`crate::composite`] or any gate reads it.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use serde::Serialize;
use serde_json::Value;
use sha2::{Digest, Sha256};
use sharpebench_protocol::canonical::{versioned_preimage, CanonicalError, CANONICAL_JSON_VERSION};
use sharpebench_protocol::{Decision, MarketObservation, Order};

/// Schema identifier of [`DecisionStabilityReport`].
pub const DECISION_STABILITY_SCHEMA: &str = "sharpebench.decision-stability.v1";

/// How [`observation_sha256`] identifies an observation, as written into the report.
pub const OBSERVATION_DIGEST_RULE: &str =
    "sha256 over the sharpebench/canonical-json/v1 framed preimage of the MarketObservation";

/// Which steps are compared, as written into the report.
pub const GROUPING_RULE: &str = "a group is two or more replicate runs of one window that were shown equal observations at this step and at every earlier step, and made the same decisions at every earlier step";

/// When two decisions differ, as written into the report.
pub const DECISION_DIFFERENCE_RULE: &str = "decisions differ when their orders arrays differ in length or when any position differs in symbol, action, target_weight or confidence (a stated confidence differs from an unstated one), numbers compared exactly with 0 equal to -0; reasoning, rationale and cost are not compared";

/// How the headline rate is computed, as written into the report.
pub const RATE_RULE: &str = "pairwise disagreement: replicate pairs with differing decisions over replicate pairs, summed over compared groups, where a group of n replicates holds n(n-1)/2 pairs";

/// What the counts are counts of, as written into the report.
pub const SAMPLING_UNIT: &str = "the replicate run: groups and pairs at later steps reuse the same runs, so their counts are not sample sizes, and the rate carries no interval";

/// SHA-256 of an observation's `sharpebench/canonical-json/v1` framed pre-image,
/// as lowercase hex.
///
/// `serde_json` writes a non-finite `f64` as `null`, which would give NaN and
/// both infinities one digest. [`MarketObservation`] has no optional field, so a
/// `null` in its JSON value can only be such a number, and the observation is
/// refused as having no canonical form instead.
pub fn observation_sha256(observation: &MarketObservation) -> Result<String, CanonicalError> {
    let value = serde_json::to_value(observation).expect("market observations serialize");
    if contains_null(&value) {
        return Err(CanonicalError::NonFinite(format!(
            "number in the observation dated {}",
            observation.date
        )));
    }
    let preimage = versioned_preimage(&value)?;
    Ok(crate::lower_hex(&Sha256::digest(&preimage)))
}

fn contains_null(value: &Value) -> bool {
    match value {
        Value::Null => true,
        Value::Array(items) => items.iter().any(contains_null),
        Value::Object(members) => members.values().any(contains_null),
        _ => false,
    }
}

/// Whether two decisions are the same choice under [`DECISION_DIFFERENCE_RULE`].
pub fn same_decision(left: &Decision, right: &Decision) -> bool {
    left.orders.len() == right.orders.len()
        && left
            .orders
            .iter()
            .zip(&right.orders)
            .all(|(left, right)| same_order(left, right))
}

fn same_order(left: &Order, right: &Order) -> bool {
    left.symbol == right.symbol
        && left.action == right.action
        && same_number(left.target_weight, right.target_weight)
        && same_confidence(left.confidence, right.confidence)
}

fn same_confidence(left: Option<f64>, right: Option<f64>) -> bool {
    match (left, right) {
        (Some(left), Some(right)) => same_number(left, right),
        (None, None) => true,
        _ => false,
    }
}

/// Numeric equality, so `0 == -0`, or identical bits, so a NaN a deterministic
/// agent repeats is not counted as a change of mind.
fn same_number(left: f64, right: f64) -> bool {
    left == right || left.to_bits() == right.to_bits()
}

/// One recorded decision and the digest of the observation it answered.
#[derive(Clone, Debug)]
pub struct ObservedDecision<'a> {
    pub observation_sha256: String,
    pub decision: &'a Decision,
}

/// One replicate run of a window: a decision for every step, in step order.
#[derive(Clone, Debug)]
pub struct ReplicateRun<'a> {
    /// Inclusive window start.
    pub window_start: usize,
    /// Exclusive window end.
    pub window_end: usize,
    /// Identity of the run's recorded content, such as a digest of its bytes.
    /// Two runs of one window with equal identities are counted as identical
    /// replicates.
    pub content_sha256: String,
    pub steps: Vec<ObservedDecision<'a>>,
}

/// Whether replicate runs with identical content may be measured.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IdenticalReplicates {
    /// Refuse them: a copy of a capture agrees with it by construction.
    Refused,
    /// The caller declares them separate executions, as two captures of a
    /// deterministic agent are.
    Declared,
}

/// Input the report refuses rather than measuring.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DecisionStabilityError {
    /// A run does not carry one decision for every step of its window, so its
    /// later steps could not be lined up with the other replicates.
    IncompleteRun {
        run: usize,
        recorded: usize,
        required: usize,
    },
    /// A window holds replicate runs with identical content and the caller did
    /// not declare them separate executions.
    UndeclaredIdenticalReplicates {
        window_start: usize,
        window_end: usize,
        identical: usize,
    },
}

impl fmt::Display for DecisionStabilityError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::IncompleteRun {
                run,
                recorded,
                required,
            } => write!(
                f,
                "replicate run {run} records {recorded} of the {required} decisions its window requires"
            ),
            Self::UndeclaredIdenticalReplicates {
                window_start,
                window_end,
                identical,
            } => write!(
                f,
                "window [{window_start}, {window_end}) holds {identical} replicate runs identical to another replicate; a copied capture agrees with itself, so declare them separate executions or remove the copies"
            ),
        }
    }
}

impl std::error::Error for DecisionStabilityError {}

/// Why a stability rate has no value.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum StabilityUnavailable {
    /// No window has two or more replicate runs.
    SingleReplicate,
    /// Replicates exist, but no step's history was shared by two of them.
    NoMatchedObservation,
}

/// The pairwise disagreement rate, or why there is none.
#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum StabilityRate {
    /// `differing_pairs / pairs_compared`, in `[0, 1]`.
    Available {
        value: f64,
    },
    Unavailable {
        reason: StabilityUnavailable,
    },
}

/// Step, group and pair counts, for one window or summed over all windows.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct StabilityCounts {
    /// Every recorded step of every replicate run counted here.
    pub steps_total: usize,
    /// Steps that belong to a compared group.
    pub steps_compared: usize,
    /// Steps of a replicate whose observation history no other replicate
    /// shared.
    pub steps_excluded_diverged_observation: usize,
    /// Steps after a replicate made a decision no other member of its group
    /// made.
    pub steps_excluded_diverged_decision: usize,
    /// Steps of a window that has a single replicate run.
    pub steps_unreplicated: usize,
    /// Replicate runs whose content identity equals an earlier run's in the
    /// same window.
    pub identical_replicate_runs: usize,
    /// Context only: the share of differing groups depends on group size.
    pub groups_compared: usize,
    /// Context only.
    pub groups_with_differing_decisions: usize,
    /// Compared groups by their number of replicates.
    pub group_sizes: BTreeMap<usize, usize>,
    pub pairs_compared: usize,
    pub differing_pairs: usize,
    /// The headline rate.
    pub pairwise_disagreement: StabilityRate,
}

/// A compared group whose members did not all decide the same.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct DifferingGroup {
    /// 0-based step within the window.
    pub step: usize,
    pub observation_sha256: String,
    /// Replicate runs in the group.
    pub replicates: usize,
    /// Distinct decisions among them, at least two.
    pub distinct_decisions: usize,
    /// Replicate pairs in the group whose decisions differ.
    pub differing_pairs: usize,
}

/// Decision stability over the replicate runs of one window.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct WindowStability {
    pub window_start: usize,
    pub window_end: usize,
    /// Replicate runs of the window: the independent unit.
    pub replicates: usize,
    #[serde(flatten)]
    pub counts: StabilityCounts,
    /// Every differing group, in step order.
    pub differing_groups: Vec<DifferingGroup>,
}

/// Rank-neutral decision-stability report for one entrant.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct DecisionStabilityReport {
    pub schema: String,
    pub agent_id: String,
    /// Always `false`: no gate or rank reads this report.
    pub rank_input: bool,
    pub observation_digest: String,
    pub grouping: String,
    pub decision_difference: String,
    pub rate: String,
    pub sampling_unit: String,
    /// Whether the caller declared identical replicate runs separate executions.
    pub identical_replicates_declared: bool,
    pub replicate_runs: usize,
    /// Sums over `windows`. The rate is unavailable only when every window's is.
    #[serde(flatten)]
    pub totals: StabilityCounts,
    /// One entry per distinct `(window_start, window_end)`, in ascending order.
    pub windows: Vec<WindowStability>,
}

/// Build the report over `runs`, grouping them into windows by their
/// `(window_start, window_end)`.
///
/// Every run must carry exactly `window_end - window_start` decisions. The
/// digests are compared as given, so every run must have been digested the same
/// way; [`observation_sha256`] is the rule this crate's callers use. A window
/// holding runs with equal `content_sha256` is refused unless `identical` is
/// [`IdenticalReplicates::Declared`].
pub fn decision_stability(
    agent_id: &str,
    runs: &[ReplicateRun<'_>],
    identical: IdenticalReplicates,
) -> Result<DecisionStabilityReport, DecisionStabilityError> {
    for (index, run) in runs.iter().enumerate() {
        let required = run.window_end.saturating_sub(run.window_start);
        if run.steps.len() != required {
            return Err(DecisionStabilityError::IncompleteRun {
                run: index,
                recorded: run.steps.len(),
                required,
            });
        }
    }

    let mut by_window: BTreeMap<(usize, usize), Vec<&ReplicateRun<'_>>> = BTreeMap::new();
    for run in runs {
        by_window
            .entry((run.window_start, run.window_end))
            .or_default()
            .push(run);
    }
    let windows: Vec<WindowStability> = by_window
        .into_iter()
        .map(|((start, end), replicates)| window_stability(start, end, &replicates))
        .collect();

    if identical == IdenticalReplicates::Refused {
        if let Some(window) = windows
            .iter()
            .find(|window| window.counts.identical_replicate_runs > 0)
        {
            return Err(DecisionStabilityError::UndeclaredIdenticalReplicates {
                window_start: window.window_start,
                window_end: window.window_end,
                identical: window.counts.identical_replicate_runs,
            });
        }
    }

    let mut totals = empty_counts(0);
    for window in &windows {
        let counts = &window.counts;
        totals.steps_total += counts.steps_total;
        totals.steps_compared += counts.steps_compared;
        totals.steps_excluded_diverged_observation += counts.steps_excluded_diverged_observation;
        totals.steps_excluded_diverged_decision += counts.steps_excluded_diverged_decision;
        totals.steps_unreplicated += counts.steps_unreplicated;
        totals.identical_replicate_runs += counts.identical_replicate_runs;
        totals.groups_compared += counts.groups_compared;
        totals.groups_with_differing_decisions += counts.groups_with_differing_decisions;
        for (&size, &groups) in &counts.group_sizes {
            *totals.group_sizes.entry(size).or_insert(0) += groups;
        }
        totals.pairs_compared += counts.pairs_compared;
        totals.differing_pairs += counts.differing_pairs;
    }
    let reason = if windows.iter().any(|window| window.replicates >= 2) {
        StabilityUnavailable::NoMatchedObservation
    } else {
        StabilityUnavailable::SingleReplicate
    };
    totals.pairwise_disagreement = rate(totals.pairs_compared, totals.differing_pairs, reason);

    Ok(DecisionStabilityReport {
        schema: DECISION_STABILITY_SCHEMA.to_string(),
        agent_id: agent_id.to_string(),
        rank_input: false,
        observation_digest: format!("{OBSERVATION_DIGEST_RULE} ({CANONICAL_JSON_VERSION})"),
        grouping: GROUPING_RULE.to_string(),
        decision_difference: DECISION_DIFFERENCE_RULE.to_string(),
        rate: RATE_RULE.to_string(),
        sampling_unit: SAMPLING_UNIT.to_string(),
        identical_replicates_declared: identical == IdenticalReplicates::Declared,
        replicate_runs: runs.len(),
        totals,
        windows,
    })
}

fn empty_counts(steps_total: usize) -> StabilityCounts {
    StabilityCounts {
        steps_total,
        steps_compared: 0,
        steps_excluded_diverged_observation: 0,
        steps_excluded_diverged_decision: 0,
        steps_unreplicated: 0,
        identical_replicate_runs: 0,
        groups_compared: 0,
        groups_with_differing_decisions: 0,
        group_sizes: BTreeMap::new(),
        pairs_compared: 0,
        differing_pairs: 0,
        pairwise_disagreement: StabilityRate::Unavailable {
            reason: StabilityUnavailable::SingleReplicate,
        },
    }
}

fn rate(pairs: usize, differing: usize, reason: StabilityUnavailable) -> StabilityRate {
    if pairs == 0 {
        StabilityRate::Unavailable { reason }
    } else {
        StabilityRate::Available {
            value: differing as f64 / pairs as f64,
        }
    }
}

/// Unordered pairs among `n` replicates.
fn pairs_among(n: usize) -> usize {
    n * (n - 1) / 2
}

fn window_stability(start: usize, end: usize, runs: &[&ReplicateRun<'_>]) -> WindowStability {
    let steps = end.saturating_sub(start);
    let mut counts = empty_counts(runs.len() * steps);
    let distinct_contents: BTreeSet<&str> =
        runs.iter().map(|run| run.content_sha256.as_str()).collect();
    counts.identical_replicate_runs = runs.len() - distinct_contents.len();
    let mut differing_groups = Vec::new();
    if runs.len() < 2 {
        counts.steps_unreplicated = counts.steps_total;
        return WindowStability {
            window_start: start,
            window_end: end,
            replicates: runs.len(),
            counts,
            differing_groups,
        };
    }

    let mut groups: Vec<Vec<usize>> = vec![(0..runs.len()).collect()];
    for step in 0..steps {
        let mut refined = Vec::with_capacity(groups.len());
        for group in &groups {
            let mut by_digest: BTreeMap<&str, Vec<usize>> = BTreeMap::new();
            for &member in group {
                by_digest
                    .entry(runs[member].steps[step].observation_sha256.as_str())
                    .or_default()
                    .push(member);
            }
            for (digest, members) in by_digest {
                if members.len() < 2 {
                    // No other replicate shares this history, now or later.
                    counts.steps_excluded_diverged_observation += steps - step;
                    continue;
                }
                let classes =
                    decision_classes(&members, |member| runs[member].steps[step].decision);
                let pairs = pairs_among(members.len());
                let agreeing: usize = classes.iter().map(|class| pairs_among(class.len())).sum();
                counts.steps_compared += members.len();
                counts.groups_compared += 1;
                *counts.group_sizes.entry(members.len()).or_insert(0) += 1;
                counts.pairs_compared += pairs;
                counts.differing_pairs += pairs - agreeing;
                if classes.len() > 1 {
                    counts.groups_with_differing_decisions += 1;
                    differing_groups.push(DifferingGroup {
                        step,
                        observation_sha256: digest.to_string(),
                        replicates: members.len(),
                        distinct_decisions: classes.len(),
                        differing_pairs: pairs - agreeing,
                    });
                }
                for class in classes {
                    if class.len() < 2 {
                        // Compared at this step; its decision history is now
                        // its own for the steps that follow.
                        counts.steps_excluded_diverged_decision += steps - step - 1;
                    } else {
                        refined.push(class);
                    }
                }
            }
        }
        groups = refined;
    }
    counts.pairwise_disagreement = rate(
        counts.pairs_compared,
        counts.differing_pairs,
        StabilityUnavailable::NoMatchedObservation,
    );
    WindowStability {
        window_start: start,
        window_end: end,
        replicates: runs.len(),
        counts,
        differing_groups,
    }
}

/// `members` partitioned by [`same_decision`], each class in first-appearance order.
fn decision_classes<'a>(
    members: &[usize],
    decision_of: impl Fn(usize) -> &'a Decision,
) -> Vec<Vec<usize>> {
    let mut classes: Vec<Vec<usize>> = Vec::new();
    for &member in members {
        let decision = decision_of(member);
        match classes
            .iter_mut()
            .find(|class| same_decision(decision_of(class[0]), decision))
        {
            Some(class) => class.push(member),
            None => classes.push(vec![member]),
        }
    }
    classes
}

#[cfg(test)]
mod tests {
    use super::*;
    use sharpebench_protocol::{Action, DecisionCost, PositionState, SymbolSnapshot};

    fn order(symbol: &str, action: Action, target_weight: f64, confidence: f64) -> Order {
        Order {
            symbol: symbol.to_string(),
            action,
            target_weight,
            confidence: Some(confidence),
            rationale: String::new(),
        }
    }

    fn decision(orders: Vec<Order>) -> Decision {
        Decision {
            orders,
            reasoning: String::new(),
            cost: None,
        }
    }

    fn buy(weight: f64) -> Decision {
        decision(vec![order("A", Action::Buy, weight, 0.5)])
    }

    fn hold() -> Decision {
        decision(Vec::new())
    }

    /// A replicate run of window `[start, start + steps.len())` whose content
    /// identity is `content`.
    fn run_as<'a>(content: &str, start: usize, steps: &[(&str, &'a Decision)]) -> ReplicateRun<'a> {
        ReplicateRun {
            window_start: start,
            window_end: start + steps.len(),
            content_sha256: content.to_string(),
            steps: steps
                .iter()
                .map(|&(digest, decision)| ObservedDecision {
                    observation_sha256: digest.to_string(),
                    decision,
                })
                .collect(),
        }
    }

    /// Replicates with distinct content identities.
    fn runs<'a>(start: usize, replicates: &[&[(&str, &'a Decision)]]) -> Vec<ReplicateRun<'a>> {
        replicates
            .iter()
            .enumerate()
            .map(|(index, steps)| run_as(&format!("run-{start}-{index}"), start, steps))
            .collect()
    }

    fn measure(runs: &[ReplicateRun<'_>]) -> DecisionStabilityReport {
        decision_stability("entrant", runs, IdenticalReplicates::Refused).unwrap()
    }

    fn available(value: f64) -> StabilityRate {
        StabilityRate::Available { value }
    }

    fn observation() -> MarketObservation {
        MarketObservation {
            date: "2026-01-02".to_string(),
            cash: 1.0,
            symbols: vec![SymbolSnapshot {
                symbol: "A".to_string(),
                close_history: vec![10.0, 10.5],
                fundamentals: [("pe".to_string(), 12.0)].into_iter().collect(),
                news: vec!["headline".to_string()],
            }],
            portfolio: vec![PositionState {
                symbol: "A".to_string(),
                shares: 0.0,
                avg_price: 0.0,
            }],
        }
    }

    #[test]
    fn the_observation_digest_is_sha256_of_the_canonical_framed_preimage() {
        let observation = observation();
        let value = serde_json::to_value(&observation).unwrap();
        let expected = crate::lower_hex(&Sha256::digest(
            versioned_preimage(&value).unwrap().as_slice(),
        ));
        assert_eq!(observation_sha256(&observation).unwrap(), expected);
        assert_eq!(expected.len(), 64);
    }

    #[test]
    fn the_observation_digest_moves_with_the_observation_and_not_with_the_sign_of_zero() {
        let base = observation_sha256(&observation()).unwrap();
        let mut moved = observation();
        moved.cash = 0.999;
        assert_ne!(observation_sha256(&moved).unwrap(), base);

        let mut negative_zero = observation();
        negative_zero.portfolio[0].shares = -0.0;
        assert_eq!(observation_sha256(&negative_zero).unwrap(), base);
    }

    #[test]
    fn a_finite_observation_serializes_without_a_null() {
        // The non-finite check reads a null as a non-finite number. This fails
        // the day MarketObservation gains a field that serializes as null.
        let empty = MarketObservation {
            date: String::new(),
            cash: 0.0,
            symbols: vec![SymbolSnapshot {
                symbol: String::new(),
                close_history: Vec::new(),
                fundamentals: BTreeMap::new(),
                news: Vec::new(),
            }],
            portfolio: Vec::new(),
        };
        assert!(!contains_null(&serde_json::to_value(&empty).unwrap()));
        assert!(observation_sha256(&empty).is_ok());
    }

    #[test]
    fn a_non_finite_observation_has_no_digest() {
        let mut cash = observation();
        cash.cash = f64::NAN;
        let mut history = observation();
        history.symbols[0].close_history[1] = f64::INFINITY;
        let mut fundamentals = observation();
        fundamentals.symbols[0]
            .fundamentals
            .insert("pe".to_string(), f64::NEG_INFINITY);
        for refused in [cash, history, fundamentals] {
            assert!(
                matches!(
                    observation_sha256(&refused),
                    Err(CanonicalError::NonFinite(ref message)) if message.contains("2026-01-02")
                ),
                "a non-finite number must not collapse into a null digest"
            );
        }
    }

    #[test]
    fn decisions_differ_on_each_score_bearing_order_field() {
        let base = decision(vec![order("A", Action::Buy, 0.5, 0.7)]);
        assert!(same_decision(&base, &base.clone()));
        let mut unstated = base.clone();
        unstated.orders[0].confidence = None;
        for changed in [
            decision(vec![order("B", Action::Buy, 0.5, 0.7)]),
            decision(vec![order("A", Action::Sell, 0.5, 0.7)]),
            decision(vec![order("A", Action::Buy, 0.5000001, 0.7)]),
            decision(vec![order("A", Action::Buy, 0.5, 0.71)]),
            decision(vec![
                order("A", Action::Buy, 0.5, 0.7),
                order("B", Action::Buy, 0.0, 0.7),
            ]),
            unstated.clone(),
            hold(),
        ] {
            assert!(!same_decision(&base, &changed), "{changed:?}");
            assert!(!same_decision(&changed, &base), "{changed:?}");
        }
        assert!(
            same_decision(&unstated, &unstated.clone()),
            "two unstated confidences are the same statement"
        );
    }

    #[test]
    fn order_position_matters_but_audit_text_and_cost_do_not() {
        let first = decision(vec![
            order("A", Action::Buy, 0.5, 0.7),
            order("B", Action::Sell, -0.2, 0.6),
        ]);
        let swapped = decision(vec![first.orders[1].clone(), first.orders[0].clone()]);
        assert!(!same_decision(&first, &swapped));

        let mut annotated = first.clone();
        annotated.reasoning = "a different explanation".to_string();
        annotated.orders[0].rationale = "another rationale".to_string();
        annotated.cost = Some(DecisionCost {
            cost_usd: 0.25,
            tokens_in: 900,
            tokens_out: 120,
            reasoning_tokens: 40,
        });
        assert!(same_decision(&first, &annotated));
    }

    #[test]
    fn numbers_compare_exactly_with_zero_equal_to_negative_zero() {
        assert!(same_number(0.0, -0.0));
        assert!(same_number(f64::NAN, f64::NAN));
        assert!(!same_number(f64::NAN, -f64::NAN));
        assert!(!same_number(0.3, 0.30000000000000004));
        assert!(same_decision(&buy(0.0), &buy(-0.0)));
        assert!(same_confidence(Some(0.0), Some(-0.0)));
        assert!(!same_confidence(Some(0.5), Some(0.6)));
        assert!(!same_confidence(None, Some(0.5)));
    }

    #[test]
    fn a_deterministic_field_reports_exactly_zero_over_matched_groups() {
        let (a, b) = (buy(0.5), hold());
        let replicate: &[(&str, &Decision)] = &[("o0", &a), ("o1", &b), ("o2", &b)];
        let report = measure(&runs(10, &[replicate, replicate, replicate]));
        let totals = &report.totals;
        assert_eq!(totals.groups_compared, 3);
        assert_eq!(totals.pairs_compared, 9);
        assert_eq!(totals.differing_pairs, 0);
        assert_eq!(totals.pairwise_disagreement, available(0.0));
        assert_eq!(totals.steps_compared, 9);
        assert_eq!(totals.steps_excluded_diverged_observation, 0);
        assert_eq!(totals.steps_excluded_diverged_decision, 0);
        assert_eq!(totals.group_sizes, BTreeMap::from([(3, 3)]));
        assert!(report.windows[0].differing_groups.is_empty());
    }

    /// A field whose groups are exactly the outcome distribution of `size`
    /// independent replicates choosing among `choices`, where choice `c` has
    /// probability `weights[c] / weights.sum()`. Every arrangement of choices is
    /// one window of one step, repeated `prod weights[c]` times, so empirical
    /// frequencies equal the probabilities and the rate equals its expectation.
    fn exact_outcome_field(size: usize, weights: &[usize]) -> (Vec<Decision>, Vec<Vec<usize>>) {
        let decisions: Vec<Decision> = (0..weights.len())
            .map(|choice| buy(0.1 * (choice + 1) as f64))
            .collect();
        let mut arrangements = Vec::new();
        let mut current = vec![0usize; size];
        loop {
            let copies: usize = current.iter().map(|&choice| weights[choice]).product();
            for _ in 0..copies {
                arrangements.push(current.clone());
            }
            let Some(position) = current
                .iter()
                .rposition(|&choice| choice + 1 < weights.len())
            else {
                break;
            };
            current[position] += 1;
            for later in &mut current[position + 1..] {
                *later = 0;
            }
        }
        (decisions, arrangements)
    }

    fn measure_outcome_field(size: usize, weights: &[usize]) -> DecisionStabilityReport {
        let (decisions, arrangements) = exact_outcome_field(size, weights);
        let field: Vec<ReplicateRun<'_>> = arrangements
            .iter()
            .enumerate()
            .flat_map(|(window, choices)| {
                let decisions = &decisions;
                choices.iter().enumerate().map(move |(replicate, &choice)| {
                    run_as(
                        &format!("{window}-{replicate}"),
                        window,
                        &[("same", &decisions[choice])],
                    )
                })
            })
            .collect();
        measure(&field)
    }

    #[test]
    fn the_pairwise_rate_is_invariant_to_group_size_under_independent_flips() {
        // Binary choice, flip probability q = 1/4: every pair differs with
        // probability 2q(1-q) = 3/8 whatever the group size, while the share
        // of differing groups is 1 - (3/4)^n - (1/4)^n. Both values were
        // checked with sympy in exact rationals.
        let group_share = [
            (2, 3.0 / 8.0),
            (3, 9.0 / 16.0),
            (4, 87.0 / 128.0),
            (5, 195.0 / 256.0),
            (6, 1683.0 / 2048.0),
        ];
        for (size, share) in group_share {
            let report = measure_outcome_field(size, &[3, 1]);
            let totals = &report.totals;
            assert_eq!(
                totals.groups_compared,
                4usize.pow(size as u32),
                "n = {size}"
            );
            assert_eq!(
                totals.group_sizes,
                BTreeMap::from([(size, 4usize.pow(size as u32))])
            );
            assert_eq!(
                totals.pairs_compared,
                totals.groups_compared * size * (size - 1) / 2
            );
            assert_eq!(
                totals.pairwise_disagreement,
                available(3.0 / 8.0),
                "n = {size}"
            );
            let observed_share =
                totals.groups_with_differing_decisions as f64 / totals.groups_compared as f64;
            assert_eq!(observed_share, share, "n = {size}");
        }

        // Three choices with probabilities 1/2, 1/4, 1/4: 1 - sum p^2 = 5/8.
        for size in 2..=5 {
            let report = measure_outcome_field(size, &[2, 1, 1]);
            assert_eq!(
                report.totals.pairwise_disagreement,
                available(5.0 / 8.0),
                "n = {size}"
            );
        }
    }

    #[test]
    fn a_group_counts_every_distinct_decision_and_every_differing_pair() {
        // Classes of sizes 3, 2 and 1: 15 pairs, 3 + 1 agreeing, 11 differing.
        let (a, b, c) = (buy(0.1), buy(0.2), buy(0.3));
        let field = runs(
            0,
            &[
                &[("o", &a)],
                &[("o", &b)],
                &[("o", &a)],
                &[("o", &c)],
                &[("o", &a)],
                &[("o", &b)],
            ],
        );
        let report = measure(&field);
        assert_eq!(
            report.windows[0].differing_groups,
            vec![DifferingGroup {
                step: 0,
                observation_sha256: "o".to_string(),
                replicates: 6,
                distinct_decisions: 3,
                differing_pairs: 11,
            }]
        );
        assert_eq!(report.totals.pairs_compared, 15);
        assert_eq!(report.totals.differing_pairs, 11);
        assert_eq!(report.totals.pairwise_disagreement, available(11.0 / 15.0));
    }

    fn parity_split<'a>(
        steady: &'a Decision,
        flipped: &'a Decision,
        flip_at: impl Fn(usize) -> bool,
    ) -> Vec<Vec<(String, &'a Decision)>> {
        (0..4)
            .map(|replicate| {
                (0..20)
                    .map(|step| {
                        let chosen = if replicate % 2 == 1 && flip_at(step) {
                            flipped
                        } else {
                            steady
                        };
                        (format!("s{step}"), chosen)
                    })
                    .collect()
            })
            .collect()
    }

    fn measure_owned(replicates: &[Vec<(String, &Decision)>]) -> DecisionStabilityReport {
        let borrowed: Vec<Vec<(&str, &Decision)>> = replicates
            .iter()
            .map(|steps| {
                steps
                    .iter()
                    .map(|(digest, d)| (digest.as_str(), *d))
                    .collect()
            })
            .collect();
        let slices: Vec<&[(&str, &Decision)]> = borrowed.iter().map(Vec::as_slice).collect();
        measure(&runs(30, &slices))
    }

    #[test]
    fn a_planted_split_on_the_first_step_reports_its_pairwise_rate() {
        // Four replicates, twenty identical observations; the odd replicates
        // choose differently at step 0. Step 0 holds 4 of 6 differing pairs;
        // the two agreeing pairs then carry 19 more steps: 4 / 44 = 1 / 11.
        let (steady, flipped) = (buy(0.5), buy(0.25));
        let report = measure_owned(&parity_split(&steady, &flipped, |step| step == 0));
        let window = &report.windows[0];
        assert_eq!(window.counts.pairs_compared, 6 + 19 * 2);
        assert_eq!(window.counts.differing_pairs, 4);
        assert_eq!(window.counts.pairwise_disagreement, available(1.0 / 11.0));
        assert_eq!(window.counts.groups_compared, 1 + 19 * 2);
        assert_eq!(window.counts.group_sizes, BTreeMap::from([(2, 38), (4, 1)]));
        assert_eq!(window.counts.steps_compared, 80);
        assert_eq!(
            window.differing_groups,
            vec![DifferingGroup {
                step: 0,
                observation_sha256: "s0".to_string(),
                replicates: 4,
                distinct_decisions: 2,
                differing_pairs: 4,
            }]
        );
    }

    #[test]
    fn a_difference_the_engine_does_not_act_on_is_counted_once() {
        // The same odd replicates choose differently at every fourth step, and
        // the observations never move. After step 0 the two sides are
        // separate histories, so the later choices are not counted again.
        let (steady, flipped) = (buy(0.5), buy(0.25));
        let once = measure_owned(&parity_split(&steady, &flipped, |step| step == 0));
        let repeated = measure_owned(&parity_split(&steady, &flipped, |step| {
            step.is_multiple_of(4)
        }));
        assert_eq!(repeated.windows[0].counts, once.windows[0].counts);
        assert_eq!(repeated.windows[0].differing_groups.len(), 1);
    }

    #[test]
    fn diverged_observations_are_excluded_for_the_rest_of_the_window_and_counted() {
        // Replicate c is shown a different observation at step 1 and never
        // rejoins; its differing step-2 decision is not a comparison.
        let (x, y) = (buy(0.5), buy(0.9));
        let a: &[(&str, &Decision)] = &[("o0", &x), ("o1", &x), ("o2", &x), ("o3", &x)];
        let c: &[(&str, &Decision)] = &[("o0", &x), ("fill", &x), ("o2", &y), ("o3", &y)];
        let report = measure(&runs(5, &[a, a, c]));
        let counts = &report.windows[0].counts;
        assert_eq!(counts.steps_total, 12);
        assert_eq!(counts.steps_compared, 3 + 2 + 2 + 2);
        assert_eq!(counts.steps_excluded_diverged_observation, 3);
        assert_eq!(counts.steps_excluded_diverged_decision, 0);
        assert_eq!(counts.steps_unreplicated, 0);
        assert_eq!(counts.groups_compared, 4);
        assert_eq!(counts.differing_pairs, 0);
        assert_eq!(counts.pairs_compared, 3 + 1 + 1 + 1);
    }

    #[test]
    fn diverged_decisions_are_compared_once_then_excluded_and_counted() {
        // Replicate c decides differently at step 1 while its observations stay
        // equal: it is compared at step 1 and excluded for steps 2 and 3.
        let (x, y) = (buy(0.5), buy(0.9));
        let a: &[(&str, &Decision)] = &[("o0", &x), ("o1", &x), ("o2", &x), ("o3", &x)];
        let c: &[(&str, &Decision)] = &[("o0", &x), ("o1", &y), ("o2", &y), ("o3", &y)];
        let report = measure(&runs(5, &[a, a, c]));
        let counts = &report.windows[0].counts;
        assert_eq!(counts.steps_total, 12);
        assert_eq!(counts.steps_compared, 3 + 3 + 2 + 2);
        assert_eq!(counts.steps_excluded_diverged_decision, 2);
        assert_eq!(counts.steps_excluded_diverged_observation, 0);
        assert_eq!(counts.pairs_compared, 3 + 3 + 1 + 1);
        assert_eq!(counts.differing_pairs, 2);
        assert_eq!(counts.pairwise_disagreement, available(0.25));

        // At the last step a differing replicate has no later steps to lose.
        let d: &[(&str, &Decision)] = &[("o0", &x), ("o1", &x), ("o2", &x), ("o3", &y)];
        let last = measure(&runs(5, &[a, a, d]));
        assert_eq!(last.windows[0].counts.steps_excluded_diverged_decision, 0);
        assert_eq!(last.windows[0].counts.steps_compared, 12);
    }

    #[test]
    fn a_coincident_observation_after_divergence_is_not_compared() {
        // Both replicates are shown "o2" at step 2, but their step-1
        // observations differed, so a deterministic agent may answer "o2"
        // differently. Only step 0 is a group.
        let (x, y) = (buy(0.5), buy(0.9));
        let left: &[(&str, &Decision)] = &[("o0", &x), ("left", &x), ("o2", &x)];
        let right: &[(&str, &Decision)] = &[("o0", &x), ("right", &x), ("o2", &y)];
        let report = measure(&runs(0, &[left, right]));
        let counts = &report.windows[0].counts;
        assert_eq!(counts.groups_compared, 1);
        assert_eq!(counts.differing_pairs, 0);
        assert_eq!(counts.steps_compared, 2);
        assert_eq!(counts.steps_excluded_diverged_observation, 4);
        assert_eq!(counts.pairwise_disagreement, available(0.0));
    }

    #[test]
    fn a_split_group_keeps_comparing_each_part() {
        let (x, y) = (buy(0.5), buy(0.9));
        let a: &[(&str, &Decision)] = &[("o0", &x), ("p", &x), ("p1", &x)];
        let b: &[(&str, &Decision)] = &[("o0", &x), ("p", &x), ("p1", &y)];
        let c: &[(&str, &Decision)] = &[("o0", &x), ("q", &x), ("q1", &x)];
        let report = measure(&runs(0, &[a, b, c, c]));
        let window = &report.windows[0];
        assert_eq!(window.counts.groups_compared, 5);
        assert_eq!(window.counts.group_sizes, BTreeMap::from([(2, 4), (4, 1)]));
        assert_eq!(window.counts.groups_with_differing_decisions, 1);
        assert_eq!(window.counts.steps_compared, 12);
        assert_eq!(window.counts.pairs_compared, 6 + 1 + 1 + 1 + 1);
        assert_eq!(window.counts.differing_pairs, 1);
        assert_eq!(window.counts.pairwise_disagreement, available(0.1));
        assert_eq!(window.differing_groups[0].step, 2);
        assert_eq!(window.differing_groups[0].observation_sha256, "p1");
        assert_eq!(window.differing_groups[0].replicates, 2);
    }

    #[test]
    fn a_single_replicate_is_typed_unavailable() {
        let x = buy(0.5);
        let report = measure(&runs(0, &[&[("o0", &x), ("o1", &x)]]));
        let single = StabilityRate::Unavailable {
            reason: StabilityUnavailable::SingleReplicate,
        };
        assert_eq!(report.totals.pairwise_disagreement, single);
        let window = &report.windows[0];
        assert_eq!(window.replicates, 1);
        assert_eq!(window.counts.steps_total, 2);
        assert_eq!(window.counts.steps_unreplicated, 2);
        assert_eq!(window.counts.steps_compared, 0);
        assert_eq!(window.counts.steps_excluded_diverged_observation, 0);
        assert_eq!(window.counts.groups_compared, 0);
        assert_eq!(window.counts.pairs_compared, 0);
        assert_eq!(window.counts.pairwise_disagreement, single);

        let empty = measure(&[]);
        assert_eq!(empty.replicate_runs, 0);
        assert!(empty.windows.is_empty());
        assert_eq!(empty.totals.pairwise_disagreement, single);
    }

    #[test]
    fn two_replicates_that_never_share_an_observation_are_typed_unavailable() {
        let x = buy(0.5);
        let report = measure(&runs(0, &[&[("a", &x)], &[("b", &x)]]));
        let unmatched = StabilityRate::Unavailable {
            reason: StabilityUnavailable::NoMatchedObservation,
        };
        assert_eq!(report.totals.pairwise_disagreement, unmatched);
        assert_eq!(report.windows[0].counts.pairwise_disagreement, unmatched);
        assert_eq!(report.totals.steps_excluded_diverged_observation, 2);
    }

    #[test]
    fn windows_are_reported_separately_and_summed() {
        let (x, y) = (buy(0.5), buy(0.9));
        let mut field = runs(
            50,
            &[
                &[("w", &x), ("w1", &x), ("w2", &x)],
                &[("w", &x), ("w1", &y), ("w2", &x)],
                &[("w", &x), ("w1", &x), ("other", &x)],
            ],
        );
        field.extend(runs(
            0,
            &[&[("v", &x), ("v1", &x)], &[("v", &y), ("v1", &x)]],
        ));
        field.extend(runs(90, &[&[("u", &x)]]));
        let report = measure(&field);
        assert_eq!(report.replicate_runs, 6);
        let spans: Vec<_> = report
            .windows
            .iter()
            .map(|window| (window.window_start, window.window_end, window.replicates))
            .collect();
        assert_eq!(spans, vec![(0, 2, 2), (50, 53, 3), (90, 91, 1)]);
        // Window 0: one pair differs at step 0, then both sides are excluded.
        assert_eq!(
            report.windows[0].counts.pairwise_disagreement,
            available(1.0)
        );
        assert_eq!(report.windows[0].counts.steps_excluded_diverged_decision, 2);
        // Window 50: step 0 has 3 agreeing pairs; step 1 has 2 of 3 differing;
        // step 2 has the two agreeing replicates shown different observations.
        let window = &report.windows[1].counts;
        assert_eq!(window.pairs_compared, 3 + 3);
        assert_eq!(window.differing_pairs, 2);
        assert_eq!(window.steps_excluded_diverged_decision, 1);
        assert_eq!(window.steps_excluded_diverged_observation, 2);

        let totals = &report.totals;
        assert_eq!(totals.steps_total, 4 + 9 + 1);
        assert_eq!(totals.steps_compared, 2 + 6);
        assert_eq!(totals.steps_excluded_diverged_observation, 2);
        assert_eq!(totals.steps_excluded_diverged_decision, 2 + 1);
        assert_eq!(totals.steps_unreplicated, 1);
        assert_eq!(totals.groups_compared, 1 + 2);
        assert_eq!(totals.groups_with_differing_decisions, 1 + 1);
        assert_eq!(totals.group_sizes, BTreeMap::from([(2, 1), (3, 2)]));
        assert_eq!(totals.pairs_compared, 1 + 6);
        assert_eq!(totals.differing_pairs, 1 + 2);
        assert_eq!(totals.pairwise_disagreement, available(3.0 / 7.0));
    }

    #[test]
    fn an_incomplete_run_is_refused() {
        let x = buy(0.5);
        let mut field = runs(
            0,
            &[
                &[("o0", &x), ("o1", &x), ("o2", &x)],
                &[("o0", &x), ("o1", &x)],
            ],
        );
        field[1].window_end = 3;
        let error = decision_stability("short", &field, IdenticalReplicates::Declared).unwrap_err();
        assert_eq!(
            error,
            DecisionStabilityError::IncompleteRun {
                run: 1,
                recorded: 2,
                required: 3,
            }
        );
        assert_eq!(
            error.to_string(),
            "replicate run 1 records 2 of the 3 decisions its window requires"
        );
    }

    #[test]
    fn identical_replicates_are_counted_and_refused_unless_declared() {
        let (x, y) = (buy(0.5), buy(0.9));
        let steps: &[(&str, &Decision)] = &[("o", &x)];
        let other: &[(&str, &Decision)] = &[("o", &y)];
        let field = vec![
            run_as("copy", 0, steps),
            run_as("copy", 0, steps),
            run_as("copy", 0, steps),
            run_as("fresh", 0, other),
            run_as("copy", 7, steps),
            run_as("elsewhere", 7, steps),
        ];

        let refused =
            decision_stability("copied", &field, IdenticalReplicates::Refused).unwrap_err();
        assert_eq!(
            refused,
            DecisionStabilityError::UndeclaredIdenticalReplicates {
                window_start: 0,
                window_end: 1,
                identical: 2,
            }
        );
        assert!(refused
            .to_string()
            .starts_with("window [0, 1) holds 2 replicate runs identical to another replicate"));

        let declared = decision_stability("copied", &field, IdenticalReplicates::Declared).unwrap();
        assert!(declared.identical_replicates_declared);
        assert_eq!(declared.windows[0].counts.identical_replicate_runs, 2);
        assert_eq!(declared.windows[1].counts.identical_replicate_runs, 0);
        assert_eq!(declared.totals.identical_replicate_runs, 2);
        // The copies stay in the rate once declared: 3 of 6 pairs differ in
        // window 0 and none in window 7.
        assert_eq!(declared.totals.pairs_compared, 6 + 1);
        assert_eq!(declared.totals.differing_pairs, 3);

        let distinct = measure(&runs(0, &[steps, steps]));
        assert!(!distinct.identical_replicates_declared);
        assert_eq!(distinct.totals.identical_replicate_runs, 0);
    }

    #[test]
    fn the_report_is_rank_neutral_and_names_its_rules() {
        let x = buy(0.5);
        let report = measure(&runs(0, &[&[("o", &x)], &[("o", &x)]]));
        let json = serde_json::to_value(&report).unwrap();
        assert_eq!(json["schema"], "sharpebench.decision-stability.v1");
        assert_eq!(json["agent_id"], "entrant");
        assert_eq!(json["rank_input"], false);
        assert_eq!(
            json["observation_digest"],
            format!("{OBSERVATION_DIGEST_RULE} (sharpebench/canonical-json/v1)")
        );
        assert_eq!(json["grouping"], GROUPING_RULE);
        assert_eq!(json["decision_difference"], DECISION_DIFFERENCE_RULE);
        assert_eq!(json["rate"], RATE_RULE);
        assert_eq!(json["sampling_unit"], SAMPLING_UNIT);
        assert_eq!(json["identical_replicates_declared"], false);
        assert_eq!(json["groups_compared"], 1);
        assert_eq!(json["pairs_compared"], 1);
        assert_eq!(json["group_sizes"], serde_json::json!({"2": 1}));
        assert_eq!(
            json["pairwise_disagreement"],
            serde_json::json!({"status": "available", "value": 0.0})
        );
        assert_eq!(json["windows"][0]["replicates"], 2);
        assert_eq!(json["windows"][0]["steps_compared"], 2);
        assert_eq!(
            serde_json::to_value(StabilityRate::Unavailable {
                reason: StabilityUnavailable::NoMatchedObservation
            })
            .unwrap(),
            serde_json::json!({"status": "unavailable", "reason": "no_matched_observation"})
        );
    }
}
