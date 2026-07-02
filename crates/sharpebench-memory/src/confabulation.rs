//! E6 - confabulation / self-reinforcing-error scoring ("honest lying").
//!
//! The most insidious memory failure is not a wrong recall, it is a wrong *belief*
//! that the agent keeps reinforcing and never re-tests. It writes a conclusion to
//! memory, cites it back to itself on later turns (reinforcement), never re-verifies
//! it against fresh evidence, and it later turns out to have been wrong the whole
//! time. The agent was not lying - it honestly believed its own stale note. A memory
//! layer that makes reinforcement easy but re-testing rare manufactures exactly this
//! pathology.
//!
//! Each [`BeliefEvent`] records, for one belief the agent held, whether it was
//! `reinforced` (cited back / strengthened), whether it was `re_tested` (checked
//! against fresh evidence after being formed), and its `later_correct` ground truth
//! once resolved (`None` while still unresolved).
//!
//! [`confabulation_report`] returns the **stale/false-belief regret score**: among
//! beliefs that were reinforced but never re-tested and have since been resolved,
//! the fraction that proved wrong. That population - reinforced, never re-tested - is
//! precisely the confabulation-risk set; the score is how often that risk paid off
//! badly. Pure exact counting, trivially deterministic.

/// One belief the agent held over a session, with the three flags that decide
/// whether it is a confabulation risk and whether that risk went bad.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BeliefEvent {
    /// Whether the belief was reinforced - cited back to itself or otherwise
    /// strengthened on a later turn.
    pub reinforced: bool,
    /// Whether the belief was re-tested against fresh evidence after being formed.
    pub re_tested: bool,
    /// Ground truth once the belief was resolved: `Some(true)` correct, `Some(false)`
    /// wrong, `None` still unresolved.
    pub later_correct: Option<bool>,
}

impl BeliefEvent {
    /// Construct a belief event.
    pub fn new(reinforced: bool, re_tested: bool, later_correct: Option<bool>) -> Self {
        Self {
            reinforced,
            re_tested,
            later_correct,
        }
    }
}

/// The scored confabulation audit.
#[derive(Debug, Clone, PartialEq)]
pub struct ConfabulationReport {
    /// The stale/false-belief regret score: `false_beliefs / resolved_at_risk`, the
    /// fraction of reinforced-but-never-re-tested-and-resolved beliefs that proved
    /// wrong. In `[0, 1]`. 0.0 when nothing in the at-risk set has resolved yet.
    pub confabulation_regret: f64,
    /// Count of beliefs that were reinforced but never re-tested - the confabulation
    /// risk set (including still-unresolved ones).
    pub reinforced_untested: usize,
    /// Of the risk set, how many have resolved (`later_correct.is_some()`) - the
    /// denominator of [`ConfabulationReport::confabulation_regret`].
    pub resolved_at_risk: usize,
    /// Of the risk set, how many proved wrong (`later_correct == Some(false)`) - the
    /// numerator.
    pub false_beliefs: usize,
    /// Of the risk set, how many are still unresolved (`later_correct.is_none()`) -
    /// outstanding confabulation exposure not yet counted for or against.
    pub unresolved_at_risk: usize,
}

/// Score confabulation risk from a session's belief events.
///
/// The at-risk population is the beliefs that were reinforced but never re-tested.
/// The regret score is the share of that population, among those that have resolved,
/// which proved wrong.
///
/// Deterministic: exact integer counting, no randomness.
///
/// # Errors
///
/// Returns `Err` at the boundary when `belief_events` is empty.
pub fn confabulation_report(belief_events: &[BeliefEvent]) -> Result<ConfabulationReport, String> {
    if belief_events.is_empty() {
        return Err("at least one belief event is required".to_string());
    }

    let mut reinforced_untested = 0usize;
    let mut resolved_at_risk = 0usize;
    let mut false_beliefs = 0usize;
    let mut unresolved_at_risk = 0usize;

    for ev in belief_events {
        if ev.reinforced && !ev.re_tested {
            reinforced_untested += 1;
            match ev.later_correct {
                Some(false) => {
                    resolved_at_risk += 1;
                    false_beliefs += 1;
                }
                Some(true) => resolved_at_risk += 1,
                None => unresolved_at_risk += 1,
            }
        }
    }

    let confabulation_regret = if resolved_at_risk == 0 {
        0.0
    } else {
        false_beliefs as f64 / resolved_at_risk as f64
    };

    Ok(ConfabulationReport {
        confabulation_regret,
        reinforced_untested,
        resolved_at_risk,
        false_beliefs,
        unresolved_at_risk,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const EPS: f64 = 1e-9;

    #[test]
    fn reinforced_untested_wrong_beliefs_drive_regret() {
        let events = vec![
            // At risk and wrong: reinforced, never re-tested, proved false.
            BeliefEvent::new(true, false, Some(false)),
            BeliefEvent::new(true, false, Some(false)),
            // At risk but correct.
            BeliefEvent::new(true, false, Some(true)),
            // Re-tested, so not confabulation risk even though wrong.
            BeliefEvent::new(true, true, Some(false)),
            // Not reinforced.
            BeliefEvent::new(false, false, Some(false)),
        ];
        let rep = confabulation_report(&events).unwrap();
        assert_eq!(rep.reinforced_untested, 3);
        assert_eq!(rep.resolved_at_risk, 3);
        assert_eq!(rep.false_beliefs, 2);
        assert_eq!(rep.unresolved_at_risk, 0);
        // 2 of 3 resolved at-risk beliefs were wrong.
        assert!((rep.confabulation_regret - 2.0 / 3.0).abs() < EPS);
    }

    #[test]
    fn no_confabulation_is_zero_regret() {
        // Everything reinforced was also re-tested: no confabulation risk.
        let events = vec![
            BeliefEvent::new(true, true, Some(false)),
            BeliefEvent::new(true, true, Some(true)),
            BeliefEvent::new(false, false, Some(false)),
        ];
        let rep = confabulation_report(&events).unwrap();
        assert_eq!(rep.reinforced_untested, 0);
        assert_eq!(rep.resolved_at_risk, 0);
        assert!((rep.confabulation_regret - 0.0).abs() < EPS);
    }

    #[test]
    fn unresolved_risk_is_excluded_from_the_score() {
        let events = vec![
            BeliefEvent::new(true, false, None), // at risk, not yet resolved
            BeliefEvent::new(true, false, Some(false)), // at risk, wrong
        ];
        let rep = confabulation_report(&events).unwrap();
        assert_eq!(rep.reinforced_untested, 2);
        assert_eq!(rep.unresolved_at_risk, 1);
        assert_eq!(rep.resolved_at_risk, 1);
        assert_eq!(rep.false_beliefs, 1);
        // Only the one resolved at-risk belief counts, and it was wrong.
        assert!((rep.confabulation_regret - 1.0).abs() < EPS);
    }

    #[test]
    fn all_at_risk_beliefs_unresolved_yields_zero_regret() {
        let events = vec![
            BeliefEvent::new(true, false, None),
            BeliefEvent::new(true, false, None),
        ];
        let rep = confabulation_report(&events).unwrap();
        assert_eq!(rep.resolved_at_risk, 0);
        assert!((rep.confabulation_regret - 0.0).abs() < EPS);
    }

    #[test]
    fn empty_input_errors_cleanly() {
        assert!(confabulation_report(&[]).is_err());
    }
}
