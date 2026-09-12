//! What a suite declared it would run, what became of those trials, and what
//! its controls showed: one record, published beside the board.
//!
//! [`crate::trial_census`] and [`crate::suite_controls`] each answer half of
//! "was this a real run?". A producer that emits the census alone publishes
//! trial counts with no evidence the apparatus worked; one that emits the
//! controls alone publishes apparatus evidence with no statement of how many
//! trials the scores rest on. [`SuiteEvidence`] is the shape a run emits, and
//! [`suite_evidence`] is the only way to build one.
//!
//! Routing the controls through [`bind_to_suite`] rather than
//! [`crate::evaluate_controls`] is the point of the constructor: a producer
//! cannot reach the evidence record while skipping the refusal of a control
//! that is also a ranked entrant. Making the check part of the only
//! construction path means a new producer inherits it instead of having to
//! remember it.
//!
//! Nothing here is scoring. `used_by_gate` is recorded on the record itself,
//! always false, for the reason [`crate::SharpeDiagnostics`] records it: a
//! reader holding the JSON and nothing else must not mistake apparatus evidence
//! for a board column. The census does not move a score, the controls carry no
//! score field, and neither reaches the gate, eligibility or the rank.
//!
//! Pure and deterministic: no I/O, no clock, no ambient randomness.

use serde::Serialize;

use crate::composite::CompositeScore;
use crate::suite_controls::{
    bind_to_suite, ControlBinding, ControlError, ControlRun, SuiteControlEvidence,
};
use crate::trial_census::{census, TrialCensus, TrialReport, TrialRoster};

/// The apparatus record of one suite: its trial counts against the roster it
/// declared, and the verdict on each of its controls.
///
/// Serialize only. A deserializable `used_by_gate` would be a field an input
/// document could set to true, and the whole purpose of the field is that it
/// cannot be anything but false.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct SuiteEvidence {
    /// Always `false`: recorded so a reader of the JSON alone cannot mistake
    /// apparatus evidence for a board column.
    pub used_by_gate: bool,
    /// Trial counts against the declared roster.
    pub trials: TrialCensus,
    /// One verdict per declared control, kept apart from the entrant rankings.
    pub controls: SuiteControlEvidence,
    /// The `run_provenance` digest over those controls, with the statement of
    /// which of their fields it covers and which it does not. Reporting an
    /// identity beside a result and binding it are different things, and this
    /// is the second one.
    pub control_binding: ControlBinding,
}

/// Assemble the evidence a suite publishes beside its board.
///
/// `roster` is the declaration made before the run; `reports` is what the
/// producer observed for each declared cell; `controls` are the suite's
/// controls; `board` is the ranked field, used only to refuse a control that is
/// also a ranked entrant. The board is read, never written: no score changes
/// shape or value here.
pub fn suite_evidence(
    roster: &TrialRoster,
    reports: &[TrialReport],
    controls: &[ControlRun],
    board: &[CompositeScore],
) -> Result<SuiteEvidence, ControlError> {
    let controls = bind_to_suite(controls, board)?;
    let control_binding = controls
        .binding()
        .map_err(|detail| ControlError::ControlBindingIncomplete { detail })?;
    Ok(SuiteEvidence {
        used_by_gate: false,
        trials: census(roster, reports),
        controls,
        control_binding,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::composite::{rank, AgentSubmission, Run, ScoreConfig};
    use crate::suite_controls::{ControlObservation, ControlProperty};

    fn ids(names: &[&str]) -> Vec<String> {
        names.iter().map(|n| (*n).to_string()).collect()
    }

    fn roster() -> TrialRoster {
        TrialRoster::declare(&ids(&["alpha"]), &ids(&["w1"]), &[7, 9])
            .expect("a well formed roster")
    }

    fn held_control(control_id: &str) -> ControlRun {
        ControlRun {
            control_id: control_id.to_string(),
            property: ControlProperty::ProtocolAndAccounting,
            observation: ControlObservation::ProtocolAndAccounting {
                decisions_expected: 40,
                decisions_round_tripped: 40,
                accounting_residual: 0.0,
            },
        }
    }

    fn board(agent_id: &str) -> Vec<CompositeScore> {
        let returns: Vec<f64> = (0..64)
            .map(|i| 0.001 + 0.0001 * f64::from(i % 5 - 2))
            .collect();
        rank(
            &[AgentSubmission {
                agent_id: agent_id.to_string(),
                runs: vec![Run {
                    returns,
                    ..Run::default()
                }],
                in_sample_trials: 0,
                candidates: Vec::new(),
            }],
            &ScoreConfig::default(),
        )
    }

    #[test]
    fn the_record_states_it_is_not_used_by_the_gate() {
        let evidence = suite_evidence(
            &roster(),
            &[TrialReport::completed("alpha", "w1", 7)],
            &[held_control("pipeline-hold")],
            &board("alpha"),
        )
        .expect("a control that is not an entrant binds");
        assert!(!evidence.used_by_gate);
        let json = serde_json::to_value(&evidence).expect("the evidence serializes");
        assert_eq!(json["used_by_gate"], serde_json::Value::Bool(false));
    }

    /// The census half is counted against the roster, not against the reports:
    /// one of the two declared cells never reported and the expectation stays
    /// two.
    #[test]
    fn the_census_half_counts_against_the_declaration() {
        let evidence = suite_evidence(
            &roster(),
            &[TrialReport::completed("alpha", "w1", 7)],
            &[held_control("pipeline-hold")],
            &board("alpha"),
        )
        .expect("a control that is not an entrant binds");
        assert_eq!(evidence.trials.expected, 2);
        assert_eq!(evidence.trials.completed, 1);
        assert_eq!(evidence.trials.unreported, 1);
        assert!(!evidence.trials.suite_complete());
    }

    /// The controls half carries identity, intent and outcome together.
    #[test]
    fn the_controls_half_carries_identity_intent_and_outcome() {
        let evidence = suite_evidence(
            &roster(),
            &[],
            &[held_control("pipeline-hold")],
            &board("alpha"),
        )
        .expect("a control that is not an entrant binds");
        let verdict = evidence
            .controls
            .control("pipeline-hold")
            .expect("the declared control has a verdict");
        assert_eq!(verdict.property, ControlProperty::ProtocolAndAccounting);
        assert_eq!(
            verdict.observation,
            ControlObservation::ProtocolAndAccounting {
                decisions_expected: 40,
                decisions_round_tripped: 40,
                accounting_residual: 0.0,
            }
        );
        assert!(verdict.held);
        assert!(evidence.controls.all_held);
    }

    /// The reason the constructor exists: the only path to the record goes
    /// through `bind_to_suite`, so a producer cannot publish a control that is
    /// also a ranked entrant by assembling the record itself.
    #[test]
    fn a_control_that_is_also_a_ranked_entrant_is_refused() {
        let error = suite_evidence(&roster(), &[], &[held_control("alpha")], &board("alpha"))
            .expect_err("a control that is also a ranked entrant is refused");
        assert_eq!(
            error,
            ControlError::ControlRankedAsEntrant {
                control_id: "alpha".to_string()
            }
        );
    }

    /// An empty battery is refused rather than published as `all_held`, and the
    /// refusal reaches the producer through this constructor too.
    #[test]
    fn an_empty_battery_is_refused() {
        let error = suite_evidence(&roster(), &[], &[], &board("alpha"))
            .expect_err("an empty battery establishes nothing");
        assert_eq!(error, ControlError::NoControls);
    }
}
