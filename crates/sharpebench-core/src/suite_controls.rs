//! Suite controls: what each control was for, and what it actually showed.
//!
//! A control is not an entrant. It is an assertion about the *apparatus*, and
//! different controls assert different things:
//!
//! - a cash or no-op policy establishes that the protocol round-trips and the
//!   accounting closes,
//! - a deliberately invalid order establishes that the refusal path refuses,
//! - a buy-and-hold policy is an economic comparator, and establishes only that
//!   a comparable return series was produced.
//!
//! Collapsing those into one verdict surface loses the question each was asked.
//! A pipeline that works perfectly while running an unprofitable policy should
//! report **control held**, because the control was about the pipeline; reading
//! that as evidence of trading skill is exactly the confusion this module is
//! built to prevent. Symmetrically, a broken accounting path or a refusal path
//! that let an invalid order through must fail *its own* control and no other,
//! so the failure names the component rather than the suite.
//!
//! So every control carries three things that travel together into the
//! evidence: its **identity**, the **property it was intended to establish**,
//! and the **observed outcome** it actually produced. A verdict is withheld with
//! a typed [`ControlShortfall`] rather than reported as a favourable value, and
//! a control with nothing to refuse does not pass vacuously.
//!
//! Controls never enter a ranking. A privileged row that is handed a perfect
//! score because it is the apparatus is a pipeline-validation bypass, not an
//! attainable ceiling, and publishing one on a competitive board misstates what
//! the board measures. [`ControlVerdict`] therefore carries no score field at
//! all, and [`bind_to_suite`] refuses a suite in which a control identity also
//! appears as a ranked entrant.
//!
//! Pure and deterministic: no I/O, no clock, no ambient randomness.

use std::collections::BTreeSet;
use std::fmt;

use serde::{Deserialize, Serialize};

use crate::composite::CompositeScore;
use crate::evidence_coverage::{Coverage, DigestId, EvidenceInventory};

/// How far a closed accounting identity may sit from zero and still be closed.
/// A residual is a sum of signed cash movements that should cancel exactly; this
/// admits float reduction error and nothing else.
pub const ACCOUNTING_RESIDUAL_TOLERANCE: f64 = 1e-12;

/// What a control was intended to establish. Each answers a different question,
/// and a control answers only its own.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ControlProperty {
    /// The protocol round-trips and the accounting identity closes. A cash or
    /// no-op policy: it takes no market risk, so anything it moves is apparatus.
    ProtocolAndAccounting,
    /// The refusal path refuses. A deliberately invalid order is submitted and
    /// must be rejected, every time it is submitted.
    RefusalOfInvalidOrder,
    /// A comparable return series was produced. A buy-and-hold policy: an
    /// economic reference point, never a claim that the apparatus is skilled.
    EconomicComparator,
}

impl ControlProperty {
    /// The property's identifier, as published.
    pub fn identifier(self) -> &'static str {
        match self {
            Self::ProtocolAndAccounting => "protocol_and_accounting",
            Self::RefusalOfInvalidOrder => "refusal_of_invalid_order",
            Self::EconomicComparator => "economic_comparator",
        }
    }
}

impl fmt::Display for ControlProperty {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.identifier())
    }
}

/// What a control run actually produced. One variant per property: an
/// observation of the wrong shape cannot be offered as evidence for a property
/// it does not describe.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "observed", rename_all = "snake_case")]
pub enum ControlObservation {
    /// Decisions the harness asked for, decisions that came back, and the
    /// residual of the accounting identity over the whole control run.
    ProtocolAndAccounting {
        decisions_expected: usize,
        decisions_round_tripped: usize,
        accounting_residual: f64,
    },
    /// Invalid orders deliberately submitted, and refusals observed.
    RefusalOfInvalidOrder {
        invalid_orders_submitted: usize,
        refusals_observed: usize,
    },
    /// Return periods the comparator was expected to produce, periods it did
    /// produce, and its realized mean return. The mean is reported because a
    /// reader wants it; it is not a condition of the control.
    EconomicComparator {
        periods_expected: usize,
        periods_observed: usize,
        mean_return: f64,
    },
}

impl ControlObservation {
    /// The property this observation is capable of speaking to.
    pub fn property(&self) -> ControlProperty {
        match self {
            Self::ProtocolAndAccounting { .. } => ControlProperty::ProtocolAndAccounting,
            Self::RefusalOfInvalidOrder { .. } => ControlProperty::RefusalOfInvalidOrder,
            Self::EconomicComparator { .. } => ControlProperty::EconomicComparator,
        }
    }
}

/// One control as declared and run: identity, intended property, observed
/// outcome.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ControlRun {
    pub control_id: String,
    /// What this control was intended to establish, declared independently of
    /// what it observed.
    pub property: ControlProperty,
    pub observation: ControlObservation,
}

/// Why a control did not establish its property. Each variant names the
/// component that fell short, never the suite.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "shortfall", rename_all = "snake_case")]
pub enum ControlShortfall {
    /// Decisions went out and did not all come back: the protocol did not
    /// round-trip.
    ProtocolIncomplete {
        expected: usize,
        round_tripped: usize,
    },
    /// The accounting identity did not close within
    /// [`ACCOUNTING_RESIDUAL_TOLERANCE`].
    AccountingDidNotClose { residual: f64 },
    /// No invalid order was submitted, so the refusal path was never exercised.
    /// A control that ran nothing establishes nothing.
    RefusalNotExercised,
    /// An invalid order was accepted. The refusal path is the component at
    /// fault.
    InvalidOrderAccepted { submitted: usize, refused: usize },
    /// The comparator did not produce the return series it was asked for, so it
    /// is not comparable to anything. Independent of the series' sign.
    ComparatorSeriesIncomplete { expected: usize, observed: usize },
    /// The comparator's realized mean return is not a finite number, so no
    /// usable comparable series was established. Also independent of sign: a
    /// negative mean is a number and this is not one.
    ComparatorReturnNotFinite { mean_return: f64 },
}

/// The verdict on one control.
///
/// It has no score field and never gains one: a control is apparatus evidence,
/// and a number here would be read as a competitive result.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ControlVerdict {
    pub control_id: String,
    /// The property the control was intended to establish.
    pub property: ControlProperty,
    /// The outcome it observed.
    pub observation: ControlObservation,
    /// True exactly when `shortfalls` is empty.
    pub held: bool,
    /// Every way the observation fell short of the intended property.
    pub shortfalls: Vec<ControlShortfall>,
    /// One line naming identity, intent and outcome together, for a reader who
    /// is looking at the row rather than parsing it.
    pub detail: String,
}

/// The controls of one suite, kept apart from its entrant rankings.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SuiteControlEvidence {
    /// One verdict per declared control, in declared order.
    pub controls: Vec<ControlVerdict>,
    /// Every control established its own property.
    pub all_held: bool,
}

impl SuiteControlEvidence {
    /// The verdict for one control identity.
    pub fn control(&self, control_id: &str) -> Option<&ControlVerdict> {
        self.controls.iter().find(|c| c.control_id == control_id)
    }

    /// Whether every control declared for `property` held. `None` when the suite
    /// declared no control for that property, which is not the same as passing.
    pub fn property_held(&self, property: ControlProperty) -> Option<bool> {
        let mut declared = self
            .controls
            .iter()
            .filter(|c| c.property == property)
            .peekable();
        declared.peek()?;
        Some(declared.all(|c| c.held))
    }

    /// The identities of the controls that did not hold, in declared order.
    pub fn failed(&self) -> Vec<&str> {
        self.controls
            .iter()
            .filter(|c| !c.held)
            .map(|c| c.control_id.as_str())
            .collect()
    }
}

/// Why a suite's controls could not be evaluated.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ControlError {
    /// The suite declared no controls at all. An empty battery is not a passing
    /// one, so it is refused rather than reported as `all_held`.
    NoControls,
    /// The same control identity was declared twice.
    DuplicateControl { control_id: String },
    /// The observation describes a different property from the declared one, so
    /// it cannot be evidence for the declared one.
    PropertyMismatch {
        control_id: String,
        declared: ControlProperty,
        observed: ControlProperty,
    },
    /// A control identity also appears as a ranked entrant. Apparatus evidence
    /// and competitive results are different records and must not share a row.
    ControlRankedAsEntrant { control_id: String },
}

impl fmt::Display for ControlError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoControls => write!(
                f,
                "suite controls: the suite declared no controls; an empty battery \
                 establishes nothing and is not reported as held"
            ),
            Self::DuplicateControl { control_id } => write!(
                f,
                "suite controls: control `{control_id}` is declared more than once"
            ),
            Self::PropertyMismatch {
                control_id,
                declared,
                observed,
            } => write!(
                f,
                "suite controls: control `{control_id}` was declared to establish \
                 `{declared}` but observed `{observed}`, which cannot be evidence for it"
            ),
            Self::ControlRankedAsEntrant { control_id } => write!(
                f,
                "suite controls: `{control_id}` is a control and also a ranked entrant. \
                 A control is validation of the apparatus, not an attainable score"
            ),
        }
    }
}

impl std::error::Error for ControlError {}

fn shortfalls_of(observation: &ControlObservation) -> Vec<ControlShortfall> {
    let mut out = Vec::new();
    match *observation {
        ControlObservation::ProtocolAndAccounting {
            decisions_expected,
            decisions_round_tripped,
            accounting_residual,
        } => {
            if decisions_round_tripped != decisions_expected || decisions_expected == 0 {
                out.push(ControlShortfall::ProtocolIncomplete {
                    expected: decisions_expected,
                    round_tripped: decisions_round_tripped,
                });
            }
            // Not `!(abs <= tol)`: a NaN residual is not a closed identity, and
            // every comparison against it is false either way round.
            if !accounting_residual.is_finite()
                || accounting_residual.abs() > ACCOUNTING_RESIDUAL_TOLERANCE
            {
                out.push(ControlShortfall::AccountingDidNotClose {
                    residual: accounting_residual,
                });
            }
        }
        ControlObservation::RefusalOfInvalidOrder {
            invalid_orders_submitted,
            refusals_observed,
        } => {
            if invalid_orders_submitted == 0 {
                out.push(ControlShortfall::RefusalNotExercised);
            } else if refusals_observed != invalid_orders_submitted {
                out.push(ControlShortfall::InvalidOrderAccepted {
                    submitted: invalid_orders_submitted,
                    refused: refusals_observed,
                });
            }
        }
        // The comparator establishes that a comparable series exists. Its sign
        // and its size are reported and deliberately not conditions: an
        // unprofitable comparator is a working apparatus running a policy that
        // lost money, which is a fact about the market, not a broken control.
        // Finiteness is a different question from sign: a negative or zero mean
        // is a number a reader can compare against, and a NaN or an infinity is
        // the arithmetic saying it produced none, which establishes no
        // comparable series at all. The accounting control above refuses a
        // non-finite residual for the same reason.
        ControlObservation::EconomicComparator {
            periods_expected,
            periods_observed,
            mean_return,
        } => {
            if periods_observed != periods_expected || periods_expected == 0 {
                out.push(ControlShortfall::ComparatorSeriesIncomplete {
                    expected: periods_expected,
                    observed: periods_observed,
                });
            }
            if !mean_return.is_finite() {
                out.push(ControlShortfall::ComparatorReturnNotFinite { mean_return });
            }
        }
    }
    out
}

fn describe(run: &ControlRun, shortfalls: &[ControlShortfall]) -> String {
    let outcome = match run.observation {
        ControlObservation::ProtocolAndAccounting {
            decisions_expected,
            decisions_round_tripped,
            accounting_residual,
        } => format!(
            "{decisions_round_tripped}/{decisions_expected} decisions round-tripped, \
             accounting residual {accounting_residual:e}"
        ),
        ControlObservation::RefusalOfInvalidOrder {
            invalid_orders_submitted,
            refusals_observed,
        } => format!("{refusals_observed}/{invalid_orders_submitted} invalid orders refused"),
        ControlObservation::EconomicComparator {
            periods_expected,
            periods_observed,
            mean_return,
        } => format!(
            "{periods_observed}/{periods_expected} periods produced, \
             mean return {mean_return:.6} (reported, not a condition)"
        ),
    };
    let verdict = if shortfalls.is_empty() {
        "held".to_string()
    } else {
        format!("withheld: {shortfalls:?}")
    };
    format!(
        "`{}` was run to establish `{}`; it observed {outcome}; {verdict}",
        run.control_id, run.property
    )
}

/// Evaluate a suite's controls, each against its own declared property.
///
/// Refuses an empty battery, a repeated identity, and an observation that
/// describes a different property from the declared one.
pub fn evaluate_controls(runs: &[ControlRun]) -> Result<SuiteControlEvidence, ControlError> {
    if runs.is_empty() {
        return Err(ControlError::NoControls);
    }
    let mut seen: BTreeSet<&str> = BTreeSet::new();
    let mut controls = Vec::with_capacity(runs.len());
    for run in runs {
        if !seen.insert(run.control_id.as_str()) {
            return Err(ControlError::DuplicateControl {
                control_id: run.control_id.clone(),
            });
        }
        let observed = run.observation.property();
        if observed != run.property {
            return Err(ControlError::PropertyMismatch {
                control_id: run.control_id.clone(),
                declared: run.property,
                observed,
            });
        }
        let shortfalls = shortfalls_of(&run.observation);
        controls.push(ControlVerdict {
            control_id: run.control_id.clone(),
            property: run.property,
            observation: run.observation,
            held: shortfalls.is_empty(),
            detail: describe(run, &shortfalls),
            shortfalls,
        });
    }
    let all_held = controls.iter().all(|c| c.held);
    Ok(SuiteControlEvidence { controls, all_held })
}

/// Evaluate a suite's controls and bind them to the suite they belong to,
/// beside its board rather than inside it.
///
/// Refuses a suite in which a control identity is also a ranked entrant. That
/// row would be a privileged entrant whose result is the apparatus validating
/// itself, and it would be read off the board as an attainable score.
pub fn bind_to_suite(
    runs: &[ControlRun],
    board: &[CompositeScore],
) -> Result<SuiteControlEvidence, ControlError> {
    for run in runs {
        if board.iter().any(|s| s.agent_id == run.control_id) {
            return Err(ControlError::ControlRankedAsEntrant {
                control_id: run.control_id.clone(),
            });
        }
    }
    evaluate_controls(runs)
}

const COVERED_RUN: Coverage = Coverage::Covered {
    digest: DigestId::RunProvenance,
};

/// Coverage declaration for [`ControlVerdict`], the record that binds a
/// control's identity, its intended property and its observed outcome into the
/// suite's evidence.
///
/// Bound by [`DigestId::RunProvenance`]: a control describes how the suite ran,
/// not how an agent scored, and it must not move when the field's composition
/// changes.
pub const SUITE_CONTROL_INVENTORY: EvidenceInventory = EvidenceInventory {
    document: "sharpebench_core::suite_controls::ControlVerdict",
    fields: &[
        ("control_id", COVERED_RUN),
        ("property", COVERED_RUN),
        ("observation", COVERED_RUN),
        ("held", COVERED_RUN),
        ("shortfalls", COVERED_RUN),
        (
            "detail",
            Coverage::Excluded {
                reason: "a rendering of control_id, property, observation and shortfalls, all \
                         of which are covered by run_provenance; binding the prose would let a \
                         wording edit break a digest over unchanged evidence",
            },
        ),
    ],
};

#[cfg(test)]
mod tests {
    use super::*;
    use crate::composite::{rank, AgentSubmission, Run, ScoreConfig};

    fn cash(residual: f64) -> ControlRun {
        ControlRun {
            control_id: "cash".to_string(),
            property: ControlProperty::ProtocolAndAccounting,
            observation: ControlObservation::ProtocolAndAccounting {
                decisions_expected: 100,
                decisions_round_tripped: 100,
                accounting_residual: residual,
            },
        }
    }

    fn refusal(submitted: usize, refused: usize) -> ControlRun {
        ControlRun {
            control_id: "invalid-order".to_string(),
            property: ControlProperty::RefusalOfInvalidOrder,
            observation: ControlObservation::RefusalOfInvalidOrder {
                invalid_orders_submitted: submitted,
                refusals_observed: refused,
            },
        }
    }

    fn comparator(mean_return: f64) -> ControlRun {
        ControlRun {
            control_id: "buy-and-hold".to_string(),
            property: ControlProperty::EconomicComparator,
            observation: ControlObservation::EconomicComparator {
                periods_expected: 250,
                periods_observed: 250,
                mean_return,
            },
        }
    }

    /// The distinction the module exists for: the apparatus worked, the policy
    /// lost money, and those are different findings.
    #[test]
    fn an_unprofitable_comparator_still_holds_its_control() {
        let evidence =
            evaluate_controls(&[cash(0.0), refusal(3, 3), comparator(-0.004)]).expect("evaluates");
        let bh = evidence.control("buy-and-hold").expect("declared control");
        assert!(
            bh.held,
            "a complete series from a losing policy still establishes the comparator: {bh:?}"
        );
        assert!(bh.shortfalls.is_empty());
        assert!(evidence.all_held, "{evidence:?}");
        // And the record says what it does and does not mean.
        assert!(bh.detail.contains("economic_comparator"), "{}", bh.detail);
        assert!(
            bh.detail.contains("not a condition"),
            "the row must not read as a skill claim: {}",
            bh.detail
        );
    }

    #[test]
    fn a_broken_accounting_path_fails_only_its_own_control() {
        let evidence =
            evaluate_controls(&[cash(1e-6), refusal(3, 3), comparator(0.002)]).expect("evaluates");
        assert_eq!(evidence.failed(), vec!["cash"], "{evidence:?}");
        assert_eq!(
            evidence.control("cash").expect("declared").shortfalls,
            vec![ControlShortfall::AccountingDidNotClose { residual: 1e-6 }]
        );
        assert_eq!(
            evidence.property_held(ControlProperty::ProtocolAndAccounting),
            Some(false)
        );
        assert_eq!(
            evidence.property_held(ControlProperty::RefusalOfInvalidOrder),
            Some(true),
            "the refusal path is a different question and answered it"
        );
        assert!(!evidence.all_held);
    }

    #[test]
    fn a_refusal_path_that_accepted_an_invalid_order_fails_only_its_own_control() {
        let evidence =
            evaluate_controls(&[cash(0.0), refusal(3, 2), comparator(0.002)]).expect("evaluates");
        assert_eq!(evidence.failed(), vec!["invalid-order"], "{evidence:?}");
        assert_eq!(
            evidence
                .control("invalid-order")
                .expect("declared")
                .shortfalls,
            vec![ControlShortfall::InvalidOrderAccepted {
                submitted: 3,
                refused: 2,
            }]
        );
        assert_eq!(
            evidence.property_held(ControlProperty::ProtocolAndAccounting),
            Some(true),
            "the accounting path is a different question and answered it"
        );
    }

    #[test]
    fn a_refusal_control_with_nothing_to_refuse_does_not_hold() {
        let evidence = evaluate_controls(&[refusal(0, 0)]).expect("evaluates");
        let verdict = evidence.control("invalid-order").expect("declared");
        assert!(
            !verdict.held,
            "a refusal control that refused nothing because nothing was submitted \
             establishes nothing: {verdict:?}"
        );
        assert_eq!(
            verdict.shortfalls,
            vec![ControlShortfall::RefusalNotExercised]
        );
    }

    #[test]
    fn an_incomplete_comparator_series_fails_whatever_its_return() {
        let mut run = comparator(0.05);
        run.observation = ControlObservation::EconomicComparator {
            periods_expected: 250,
            periods_observed: 12,
            mean_return: 0.05,
        };
        let evidence = evaluate_controls(&[run]).expect("evaluates");
        let verdict = evidence.control("buy-and-hold").expect("declared");
        assert!(!verdict.held);
        assert_eq!(
            verdict.shortfalls,
            vec![ControlShortfall::ComparatorSeriesIncomplete {
                expected: 250,
                observed: 12,
            }]
        );
    }

    /// Sign is not a condition and finiteness is. A mean that is not a number
    /// establishes no comparable series, and the arm that only counted periods
    /// reported one as held.
    #[test]
    fn a_non_finite_comparator_return_does_not_establish_the_comparator() {
        for mean in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            let evidence = evaluate_controls(&[comparator(mean)]).expect("evaluates");
            let verdict = evidence.control("buy-and-hold").expect("declared");
            assert!(
                !verdict.held,
                "mean {mean} is not a series anything can be compared against: {verdict:?}"
            );
            assert_eq!(
                verdict.shortfalls.len(),
                1,
                "the series itself is complete, so finiteness is the only shortfall: {verdict:?}"
            );
            assert!(
                matches!(
                    verdict.shortfalls[0],
                    ControlShortfall::ComparatorReturnNotFinite { .. }
                ),
                "{verdict:?}"
            );
            assert!(!evidence.all_held);
        }

        // The control on the control: a comparator that lost money is a working
        // apparatus, and must keep holding, or the check above would be passing
        // by refusing every comparator it is shown.
        let losing = evaluate_controls(&[comparator(-0.1)]).expect("evaluates");
        let verdict = losing.control("buy-and-hold").expect("declared");
        assert!(
            verdict.held,
            "a finite losing mean is a fact about the market, not a broken control: {verdict:?}"
        );
        assert!(verdict.shortfalls.is_empty());
        assert!(losing.all_held);
    }

    #[test]
    fn a_protocol_control_that_was_asked_for_nothing_does_not_hold() {
        // Zero decisions out and zero back is a perfect round-trip rate over an
        // empty run. The same vacuity the refusal control refuses.
        let run = ControlRun {
            control_id: "cash".to_string(),
            property: ControlProperty::ProtocolAndAccounting,
            observation: ControlObservation::ProtocolAndAccounting {
                decisions_expected: 0,
                decisions_round_tripped: 0,
                accounting_residual: 0.0,
            },
        };
        let evidence = evaluate_controls(&[run]).expect("evaluates");
        let verdict = evidence.control("cash").expect("declared");
        assert!(!verdict.held, "{verdict:?}");
        assert_eq!(
            verdict.shortfalls,
            vec![ControlShortfall::ProtocolIncomplete {
                expected: 0,
                round_tripped: 0,
            }]
        );
    }

    #[test]
    fn a_residual_exactly_at_the_tolerance_still_closes() {
        // The tolerance is inclusive: it admits float reduction error at the
        // bound, and the shortfall begins past it.
        let at = evaluate_controls(&[cash(ACCOUNTING_RESIDUAL_TOLERANCE)]).expect("evaluates");
        assert!(
            at.control("cash").expect("declared").held,
            "{:?}",
            at.control("cash")
        );
        let past =
            evaluate_controls(&[cash(ACCOUNTING_RESIDUAL_TOLERANCE * 2.0)]).expect("evaluates");
        assert!(!past.control("cash").expect("declared").held);
    }

    #[test]
    fn an_observation_of_another_property_is_refused() {
        // A passing accounting observation offered as evidence that the refusal
        // path refuses.
        let run = ControlRun {
            control_id: "invalid-order".to_string(),
            property: ControlProperty::RefusalOfInvalidOrder,
            observation: ControlObservation::ProtocolAndAccounting {
                decisions_expected: 100,
                decisions_round_tripped: 100,
                accounting_residual: 0.0,
            },
        };
        assert_eq!(
            evaluate_controls(&[run]),
            Err(ControlError::PropertyMismatch {
                control_id: "invalid-order".to_string(),
                declared: ControlProperty::RefusalOfInvalidOrder,
                observed: ControlProperty::ProtocolAndAccounting,
            })
        );
    }

    #[test]
    fn an_empty_battery_is_refused_rather_than_reported_as_held() {
        assert_eq!(evaluate_controls(&[]), Err(ControlError::NoControls));
    }

    #[test]
    fn a_repeated_control_identity_is_refused() {
        assert_eq!(
            evaluate_controls(&[cash(0.0), cash(0.0)]),
            Err(ControlError::DuplicateControl {
                control_id: "cash".to_string(),
            })
        );
    }

    fn board_with(agent_id: &str) -> Vec<CompositeScore> {
        rank(
            &[AgentSubmission {
                agent_id: agent_id.to_string(),
                runs: (0..4)
                    .map(|_| Run {
                        returns: (0..60)
                            .map(|i| 0.002 + 0.0005 * (i as f64 * 0.7).sin())
                            .collect(),
                        ..Run::default()
                    })
                    .collect(),
                ..AgentSubmission::default()
            }],
            &ScoreConfig::default(),
        )
    }

    #[test]
    fn a_control_cannot_also_be_a_ranked_entrant() {
        let board = board_with("buy-and-hold");
        assert_eq!(
            bind_to_suite(&[cash(0.0), comparator(0.002)], &board),
            Err(ControlError::ControlRankedAsEntrant {
                control_id: "buy-and-hold".to_string(),
            })
        );
        // The same controls bind cleanly beside a board of actual entrants.
        let entrants = board_with("momentum");
        assert!(bind_to_suite(&[cash(0.0), comparator(0.002)], &entrants).is_ok());
    }

    #[test]
    fn a_control_verdict_carries_no_competitive_score() {
        let evidence = evaluate_controls(&[comparator(0.002)]).expect("evaluates");
        let json = serde_json::to_value(&evidence.controls[0]).expect("serializes");
        let keys: BTreeSet<String> = json
            .as_object()
            .expect("a verdict serializes as an object")
            .keys()
            .cloned()
            .collect();
        for forbidden in [
            "composite",
            "deflated_sharpe",
            "psr",
            "rank_eligible",
            "rank_ordinal",
        ] {
            assert!(
                !keys.contains(forbidden),
                "a control verdict must carry no competitive score: found `{forbidden}` in {keys:?}"
            );
        }
    }

    #[test]
    fn the_control_inventory_matches_the_verdict_it_declares() {
        let evidence = evaluate_controls(&[cash(1e-6)]).expect("evaluates");
        let json = serde_json::to_value(&evidence.controls[0]).expect("serializes");
        let names: Vec<String> = json
            .as_object()
            .expect("a verdict serializes as an object")
            .keys()
            .cloned()
            .collect();
        let audit = SUITE_CONTROL_INVENTORY.audit(&names);
        assert!(audit.is_complete(), "{audit:?}");
        assert_eq!(
            SUITE_CONTROL_INVENTORY.fields_for(DigestId::RunProvenance),
            vec![
                "control_id",
                "property",
                "observation",
                "held",
                "shortfalls"
            ],
            "identity, intent and outcome are all bound"
        );
    }
}
