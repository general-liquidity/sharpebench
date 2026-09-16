//! Decision stability: how often an agent decides differently when it saw the
//! same observations.
//!
//! pass^k across execution seeds certifies that an outcome repeats. It cannot
//! tell an agent whose decisions are a fixed function of what it saw from one
//! that samples a different choice each time it is asked, because execution
//! noise moves the fills, and with them every later observation, before the two
//! can be compared. This module measures the second property directly and only
//! where it is measurable: over replicate runs of one window (execution seeds of
//! the window, or repeated captures of the same seed), at the steps where the
//! replicates were shown identical observations.
//!
//! # Grouping
//!
//! Every step carries the SHA-256 of the observation it answered, taken over the
//! `sharpebench/canonical-json/v1` framed pre-image ([`observation_sha256`]).
//! Replicate runs of one window start in one group. At each step a group is
//! split by that step's observation digest, so a group at step `t` holds the
//! replicates whose observations were identical at `t` and at every earlier step
//! of the window. The whole history is required, not only the current
//! observation, because the determinism contract lets a decision depend on the
//! run's earlier observations and on the agent's own earlier decisions: two
//! replicates that meet on one observation after their histories differed may
//! decide differently without any non-determinism. Under this rule an agent that
//! honours the contract reports exactly zero.
//!
//! A replicate whose history is no longer shared by any other replicate leaves
//! the comparison for the rest of the window. Each of its remaining steps is
//! counted in `steps_excluded_diverged_observation`, so for every window with at
//! least two replicates `steps_compared + steps_excluded_diverged_observation`
//! equals `steps_total`. A window with a single replicate has nothing to compare
//! against: its steps are counted in `steps_unreplicated` and its rate is typed
//! unavailable.
//!
//! # What "differ" means
//!
//! Two decisions differ when their `orders` arrays have different lengths, or
//! when the orders at any one position differ in `symbol`, `action`,
//! `target_weight` or `confidence` ([`same_decision`]). The fields are the
//! score-bearing order fields of the re-execution contract, and they are compared
//! exactly: a tolerance on `target_weight` would be a free parameter the contract
//! does not define, and the engine executes the exact weight it is given. The one
//! numeric relaxation is that `0` and `-0` are equal, as they are in the
//! canonical form the observation digest uses. `reasoning` and each order's
//! `rationale` are audit text, and the `cost` report is spend rather than choice
//! (token counts can vary between samples that choose the same orders), so none
//! of the three is compared.
//!
//! A group differs when its members hold more than one distinct decision. The
//! reported rate is the number of differing groups over the number of groups
//! compared, whatever the group sizes.
//!
//! # Rank neutrality
//!
//! The report is a separate document with `rank_input: false`. Nothing in
//! [`crate::composite`] or any gate reads it.

use std::collections::BTreeMap;
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
pub const GROUPING_RULE: &str = "a group is two or more replicate runs of one window whose observation digests are equal at this step and at every earlier step of the window";

/// When two decisions differ, as written into the report.
pub const DECISION_DIFFERENCE_RULE: &str = "decisions differ when their orders arrays differ in length or when any position differs in symbol, action, target_weight or confidence, compared exactly with 0 equal to -0; reasoning, rationale and cost are not compared";

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
        && same_number(left.confidence, right.confidence)
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
    pub steps: Vec<ObservedDecision<'a>>,
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
    /// Replicates exist, but no step's observation history was shared by two of them.
    NoMatchedObservation,
}

/// The share of compared groups whose decisions differ, or why there is none.
#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum StabilityRate {
    /// `groups_with_differing_decisions / groups_compared`, in `[0, 1]`.
    Available {
        value: f64,
    },
    Unavailable {
        reason: StabilityUnavailable,
    },
}

/// Step and group counts, for one window or summed over all windows.
#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub struct StabilityCounts {
    /// Every recorded step of every replicate run counted here.
    pub steps_total: usize,
    /// Steps that belong to a compared group.
    pub steps_compared: usize,
    /// Steps of a replicate whose observation history no other replicate shared
    /// at that step or earlier.
    pub steps_excluded_diverged_observation: usize,
    /// Steps of a window that has a single replicate run.
    pub steps_unreplicated: usize,
    pub groups_compared: usize,
    pub groups_with_differing_decisions: usize,
    pub differing_fraction: StabilityRate,
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
}

/// Decision stability over the replicate runs of one window.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct WindowStability {
    pub window_start: usize,
    pub window_end: usize,
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
/// way; [`observation_sha256`] is the rule this crate's callers use.
pub fn decision_stability(
    agent_id: &str,
    runs: &[ReplicateRun<'_>],
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

    let mut totals = empty_counts(0);
    for window in &windows {
        totals.steps_total += window.counts.steps_total;
        totals.steps_compared += window.counts.steps_compared;
        totals.steps_excluded_diverged_observation +=
            window.counts.steps_excluded_diverged_observation;
        totals.steps_unreplicated += window.counts.steps_unreplicated;
        totals.groups_compared += window.counts.groups_compared;
        totals.groups_with_differing_decisions += window.counts.groups_with_differing_decisions;
    }
    let reason = if windows.iter().any(|window| window.replicates >= 2) {
        StabilityUnavailable::NoMatchedObservation
    } else {
        StabilityUnavailable::SingleReplicate
    };
    totals.differing_fraction = rate(
        totals.groups_compared,
        totals.groups_with_differing_decisions,
        reason,
    );

    Ok(DecisionStabilityReport {
        schema: DECISION_STABILITY_SCHEMA.to_string(),
        agent_id: agent_id.to_string(),
        rank_input: false,
        observation_digest: format!("{OBSERVATION_DIGEST_RULE} ({CANONICAL_JSON_VERSION})"),
        grouping: GROUPING_RULE.to_string(),
        decision_difference: DECISION_DIFFERENCE_RULE.to_string(),
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
        steps_unreplicated: 0,
        groups_compared: 0,
        groups_with_differing_decisions: 0,
        differing_fraction: StabilityRate::Unavailable {
            reason: StabilityUnavailable::SingleReplicate,
        },
    }
}

fn rate(compared: usize, differing: usize, reason: StabilityUnavailable) -> StabilityRate {
    if compared == 0 {
        StabilityRate::Unavailable { reason }
    } else {
        StabilityRate::Available {
            value: differing as f64 / compared as f64,
        }
    }
}

fn window_stability(start: usize, end: usize, runs: &[&ReplicateRun<'_>]) -> WindowStability {
    let steps = end.saturating_sub(start);
    let mut counts = empty_counts(runs.len() * steps);
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
                counts.steps_compared += members.len();
                counts.groups_compared += 1;
                let distinct = distinct_decisions(
                    members
                        .iter()
                        .map(|&member| runs[member].steps[step].decision),
                );
                if distinct > 1 {
                    counts.groups_with_differing_decisions += 1;
                    differing_groups.push(DifferingGroup {
                        step,
                        observation_sha256: digest.to_string(),
                        replicates: members.len(),
                        distinct_decisions: distinct,
                    });
                }
                refined.push(members);
            }
        }
        groups = refined;
    }
    counts.differing_fraction = rate(
        counts.groups_compared,
        counts.groups_with_differing_decisions,
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

fn distinct_decisions<'a>(decisions: impl Iterator<Item = &'a Decision>) -> usize {
    let mut representatives: Vec<&Decision> = Vec::new();
    for decision in decisions {
        if !representatives
            .iter()
            .any(|representative| same_decision(representative, decision))
        {
            representatives.push(decision);
        }
    }
    representatives.len()
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
            confidence,
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

    /// A replicate run of window `[start, start + steps.len())`.
    fn run<'a>(start: usize, steps: &[(&str, &'a Decision)]) -> ReplicateRun<'a> {
        ReplicateRun {
            window_start: start,
            window_end: start + steps.len(),
            steps: steps
                .iter()
                .map(|&(digest, decision)| ObservedDecision {
                    observation_sha256: digest.to_string(),
                    decision,
                })
                .collect(),
        }
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
        for changed in [
            decision(vec![order("B", Action::Buy, 0.5, 0.7)]),
            decision(vec![order("A", Action::Sell, 0.5, 0.7)]),
            decision(vec![order("A", Action::Buy, 0.5000001, 0.7)]),
            decision(vec![order("A", Action::Buy, 0.5, 0.71)]),
            decision(vec![
                order("A", Action::Buy, 0.5, 0.7),
                order("B", Action::Buy, 0.0, 0.7),
            ]),
            hold(),
        ] {
            assert!(!same_decision(&base, &changed), "{changed:?}");
            assert!(!same_decision(&changed, &base), "{changed:?}");
        }
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
    }

    #[test]
    fn a_deterministic_field_reports_exactly_zero_over_matched_groups() {
        let (a, b) = (buy(0.5), hold());
        let replicate = [("o0", &a), ("o1", &b), ("o2", &b)];
        let runs = vec![
            run(10, &replicate),
            run(10, &replicate),
            run(10, &replicate),
        ];
        let report = decision_stability("steady", &runs).unwrap();
        assert_eq!(report.totals.groups_compared, 3);
        assert_eq!(report.totals.groups_with_differing_decisions, 0);
        assert_eq!(
            report.totals.differing_fraction,
            StabilityRate::Available { value: 0.0 }
        );
        assert_eq!(report.totals.steps_compared, 9);
        assert_eq!(report.totals.steps_excluded_diverged_observation, 0);
        assert!(report.windows[0].differing_groups.is_empty());
    }

    #[test]
    fn a_planted_flip_on_alternate_replicates_reports_the_planted_rate() {
        // Four replicates, eight identical observations. The odd replicates
        // choose differently at every fourth step: the planted rate is 2 / 8.
        let (steady, flipped) = (buy(0.5), buy(0.25));
        let digests = ["s0", "s1", "s2", "s3", "s4", "s5", "s6", "s7"];
        let replicate = |odd: bool| -> Vec<(&str, &Decision)> {
            digests
                .iter()
                .enumerate()
                .map(|(step, digest)| {
                    let chosen = if odd && step.is_multiple_of(4) {
                        &flipped
                    } else {
                        &steady
                    };
                    (*digest, chosen)
                })
                .collect()
        };
        let runs: Vec<ReplicateRun<'_>> = (0..4)
            .map(|index| run(30, &replicate(index % 2 == 1)))
            .collect();
        let report = decision_stability("flipper", &runs).unwrap();
        let window = &report.windows[0];
        assert_eq!(window.counts.groups_compared, 8);
        assert_eq!(window.counts.groups_with_differing_decisions, 2);
        assert_eq!(
            window.counts.differing_fraction,
            StabilityRate::Available { value: 0.25 }
        );
        assert_eq!(
            window.differing_groups,
            vec![
                DifferingGroup {
                    step: 0,
                    observation_sha256: "s0".to_string(),
                    replicates: 4,
                    distinct_decisions: 2,
                },
                DifferingGroup {
                    step: 4,
                    observation_sha256: "s4".to_string(),
                    replicates: 4,
                    distinct_decisions: 2,
                },
            ]
        );
    }

    #[test]
    fn a_group_counts_every_distinct_decision() {
        let (a, b, c) = (buy(0.1), buy(0.2), buy(0.3));
        let runs = vec![
            run(0, &[("o", &a)]),
            run(0, &[("o", &b)]),
            run(0, &[("o", &a)]),
            run(0, &[("o", &c)]),
        ];
        let report = decision_stability("three-minds", &runs).unwrap();
        assert_eq!(report.windows[0].differing_groups[0].distinct_decisions, 3);
        assert_eq!(report.windows[0].differing_groups[0].replicates, 4);
    }

    #[test]
    fn diverged_observations_are_excluded_for_the_rest_of_the_window_and_counted() {
        // Replicate c is shown a different observation at step 1 and never
        // rejoins; its differing step-2 decision is not a comparison.
        let (x, y) = (buy(0.5), buy(0.9));
        let a = [("o0", &x), ("o1", &x), ("o2", &x), ("o3", &x)];
        let c = [("o0", &x), ("fill", &x), ("o2", &y), ("o3", &y)];
        let runs = vec![run(5, &a), run(5, &a), run(5, &c)];
        let report = decision_stability("slipped", &runs).unwrap();
        let counts = report.windows[0].counts;
        assert_eq!(counts.steps_total, 12);
        assert_eq!(counts.steps_compared, 3 + 2 + 2 + 2);
        assert_eq!(counts.steps_excluded_diverged_observation, 3);
        assert_eq!(counts.steps_unreplicated, 0);
        assert_eq!(counts.groups_compared, 4);
        assert_eq!(counts.groups_with_differing_decisions, 0);
    }

    #[test]
    fn a_coincident_observation_after_divergence_is_not_compared() {
        // Both replicates are shown "o2" at step 2, but their step-1
        // observations differed, so a deterministic agent may answer "o2"
        // differently. Only step 0 is a group.
        let (x, y) = (buy(0.5), buy(0.9));
        let left = [("o0", &x), ("left", &x), ("o2", &x)];
        let right = [("o0", &x), ("right", &x), ("o2", &y)];
        let report = decision_stability("history", &[run(0, &left), run(0, &right)]).unwrap();
        let counts = report.windows[0].counts;
        assert_eq!(counts.groups_compared, 1);
        assert_eq!(counts.groups_with_differing_decisions, 0);
        assert_eq!(counts.steps_compared, 2);
        assert_eq!(counts.steps_excluded_diverged_observation, 4);
        assert_eq!(
            counts.differing_fraction,
            StabilityRate::Available { value: 0.0 }
        );
    }

    #[test]
    fn a_split_group_keeps_comparing_each_part() {
        let (x, y) = (buy(0.5), buy(0.9));
        let a = [("o0", &x), ("p", &x), ("p1", &x)];
        let b = [("o0", &x), ("p", &x), ("p1", &y)];
        let c = [("o0", &x), ("q", &x), ("q1", &x)];
        let d = [("o0", &x), ("q", &x), ("q1", &x)];
        let report =
            decision_stability("pairs", &[run(0, &a), run(0, &b), run(0, &c), run(0, &d)]).unwrap();
        let window = &report.windows[0];
        assert_eq!(window.counts.groups_compared, 5);
        assert_eq!(window.counts.groups_with_differing_decisions, 1);
        assert_eq!(window.counts.steps_compared, 12);
        assert_eq!(window.differing_groups[0].step, 2);
        assert_eq!(window.differing_groups[0].observation_sha256, "p1");
        assert_eq!(window.differing_groups[0].replicates, 2);
        assert_eq!(
            window.counts.differing_fraction,
            StabilityRate::Available { value: 0.2 }
        );
    }

    #[test]
    fn a_single_replicate_is_typed_unavailable() {
        let x = buy(0.5);
        let report = decision_stability("alone", &[run(0, &[("o0", &x), ("o1", &x)])]).unwrap();
        assert_eq!(
            report.totals.differing_fraction,
            StabilityRate::Unavailable {
                reason: StabilityUnavailable::SingleReplicate
            }
        );
        let window = &report.windows[0];
        assert_eq!(window.replicates, 1);
        assert_eq!(window.counts.steps_total, 2);
        assert_eq!(window.counts.steps_unreplicated, 2);
        assert_eq!(window.counts.steps_compared, 0);
        assert_eq!(window.counts.steps_excluded_diverged_observation, 0);
        assert_eq!(window.counts.groups_compared, 0);
        assert_eq!(
            window.counts.differing_fraction,
            StabilityRate::Unavailable {
                reason: StabilityUnavailable::SingleReplicate
            }
        );

        let empty = decision_stability("nobody", &[]).unwrap();
        assert_eq!(empty.replicate_runs, 0);
        assert!(empty.windows.is_empty());
        assert_eq!(
            empty.totals.differing_fraction,
            StabilityRate::Unavailable {
                reason: StabilityUnavailable::SingleReplicate
            }
        );
    }

    #[test]
    fn two_replicates_that_never_share_an_observation_are_typed_unavailable() {
        let x = buy(0.5);
        let runs = vec![run(0, &[("a", &x)]), run(0, &[("b", &x)])];
        let report = decision_stability("apart", &runs).unwrap();
        let unmatched = StabilityRate::Unavailable {
            reason: StabilityUnavailable::NoMatchedObservation,
        };
        assert_eq!(report.totals.differing_fraction, unmatched);
        assert_eq!(report.windows[0].counts.differing_fraction, unmatched);
        assert_eq!(report.totals.steps_excluded_diverged_observation, 2);
    }

    #[test]
    fn windows_are_reported_separately_and_summed() {
        let (x, y) = (buy(0.5), buy(0.9));
        let runs = vec![
            run(50, &[("w", &x), ("w1", &x), ("w2", &x)]),
            run(0, &[("v", &x), ("v1", &x)]),
            run(50, &[("w", &y), ("w1", &x), ("other", &x)]),
            run(0, &[("v", &x), ("v1", &y)]),
            run(90, &[("u", &x)]),
        ];
        let report = decision_stability("field", &runs).unwrap();
        assert_eq!(report.replicate_runs, 5);
        let spans: Vec<_> = report
            .windows
            .iter()
            .map(|window| (window.window_start, window.window_end, window.replicates))
            .collect();
        assert_eq!(spans, vec![(0, 2, 2), (50, 53, 2), (90, 91, 1)]);
        assert_eq!(
            report.windows[0].counts.differing_fraction,
            StabilityRate::Available { value: 0.5 }
        );
        assert_eq!(
            report.windows[1].counts.differing_fraction,
            StabilityRate::Available { value: 0.5 }
        );
        let totals = report.totals;
        assert_eq!(totals.steps_total, 4 + 6 + 1);
        assert_eq!(totals.steps_compared, 4 + 4);
        assert_eq!(totals.steps_excluded_diverged_observation, 2);
        assert_eq!(totals.steps_unreplicated, 1);
        assert_eq!(totals.groups_compared, 2 + 2);
        assert_eq!(totals.groups_with_differing_decisions, 2);
        assert_eq!(
            totals.differing_fraction,
            StabilityRate::Available { value: 0.5 }
        );
    }

    #[test]
    fn an_incomplete_run_is_refused() {
        let x = buy(0.5);
        let mut short = run(0, &[("o0", &x), ("o1", &x)]);
        short.window_end = 3;
        let complete = run(0, &[("o0", &x), ("o1", &x), ("o2", &x)]);
        let error = decision_stability("short", &[complete, short]).unwrap_err();
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
    fn the_report_is_rank_neutral_and_names_its_rules() {
        let x = buy(0.5);
        let runs = vec![run(0, &[("o", &x)]), run(0, &[("o", &x)])];
        let report = decision_stability("steady", &runs).unwrap();
        let json = serde_json::to_value(&report).unwrap();
        assert_eq!(json["schema"], "sharpebench.decision-stability.v1");
        assert_eq!(json["agent_id"], "steady");
        assert_eq!(json["rank_input"], false);
        assert_eq!(
            json["observation_digest"],
            format!("{OBSERVATION_DIGEST_RULE} (sharpebench/canonical-json/v1)")
        );
        assert_eq!(json["grouping"], GROUPING_RULE);
        assert_eq!(json["decision_difference"], DECISION_DIFFERENCE_RULE);
        assert_eq!(json["groups_compared"], 1);
        assert_eq!(
            json["differing_fraction"],
            serde_json::json!({"status": "available", "value": 0.0})
        );
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
