//! Host-side wiring for the contamination probes: the shape probe
//! ([`sharpebench_sim::run_shape_probe`]) and the presentation suite
//! ([`sharpebench_sim::presentation_suite`]), run against any entrant the host
//! can drive as an [`Agent`] with transport diagnostics: a host process, a
//! sandboxed image or a gateway-attached entrant. The identifiability probe
//! ([`sharpebench_sim::identifiability_probe`]) reaches the same entrants over
//! [`LineExchange`]: the closed question travels the entrant's own transport,
//! and [`ask_external`] reads its typed answer.
//!
//! A transport failure on any arm is a typed refusal of the whole report, never
//! a dropped arm: an external agent that degrades to a hold would otherwise put
//! the harness's holds into the comparison as the entrant's own decisions.
//!
//! Both documents are rank-neutral: nothing here reaches a score, a rank, the
//! gate or the pass^k pool, and both say so on their face.

use serde::Serialize;
use sharpebench_sim::{
    ask_identity, record_run, Agent, ArmRun, AskError, CostModel, Dataset, DecideError,
    IdentifiabilityReport, IdentifiabilityUnavailable, IdentityAnswer, IdentityQuestion,
    LineExchange, PresentationSuite, ShapeProbeRun, ShapeProbeUnavailable, SuiteError,
    TransportDiagnostics, TransportHealth, Window,
};

use crate::{transport_failure, FailureKind};

/// Schema of the shape-probe document.
pub const CONTAMINATION_PROBE_SCHEMA: &str = "sharpebench.contamination-probe.v1";
/// Schema of the presentation-stability document.
pub const PRESENTATION_STABILITY_SCHEMA: &str = "sharpebench.presentation-stability.v1";
/// Schema of the identifiability-probe document.
pub const IDENTIFIABILITY_PROBE_SCHEMA: &str = "sharpebench.identifiability-probe.v1";

/// Run an external agent on one arm and record its decisions, or the typed
/// transport failure that makes the arm unusable.
pub fn record_external_run<A: Agent + TransportDiagnostics>(
    data: &Dataset,
    agent: &mut A,
    window: Window,
    seed: u64,
    costs: CostModel,
) -> Result<ArmRun, FailureKind> {
    let run = record_run(data, agent, window, seed, costs);
    match transport_failure(agent.health()) {
        Some(kind) => Err(kind),
        None => Ok(run),
    }
}

/// Why an entrant gave no scorable answer to a closed question. Either way
/// the whole identifiability document is refused, never scored as a miss: an
/// entrant that refuses or fails would otherwise read as one that cannot
/// identify the series.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum AnswerFailure {
    /// The entrant answered that it declines the question.
    Declined,
    /// The exchange failed, or the answer was malformed, classified the way a
    /// failed decision is.
    Failed { failure: FailureKind },
}

/// The failure a single transport fault is, through the same taxonomy a
/// decision's faults go through.
fn failure_of(error: DecideError) -> FailureKind {
    let mut health = TransportHealth::default();
    health.record(error, false);
    transport_failure(&health).expect("a recorded fault is a failure")
}

/// Ask one closed question over an entrant's transport and read its typed
/// answer, or the typed reason there is none.
pub fn ask_external<C: LineExchange + ?Sized>(
    channel: &mut C,
    question: &IdentityQuestion,
) -> Result<IdentityAnswer, AnswerFailure> {
    ask_identity(channel, question).map_err(|error| match error {
        AskError::Declined => AnswerFailure::Declined,
        AskError::Transport(error) => AnswerFailure::Failed {
            failure: failure_of(error),
        },
    })
}

/// What a document says about the run that produced it.
#[derive(Clone, Debug, Serialize)]
pub struct ProbeContext<'a> {
    pub entrant: &'a str,
    /// The dataset as the operator named it (`synthetic` when none was).
    pub dataset: &'a str,
    pub dataset_sha256: &'a str,
    /// The operator's declared training cutoff disclosure, when one was given.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub declared_training_cutoff: Option<serde_json::Value>,
}

#[derive(Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
enum Outcome<'a, R: Serialize, U: Serialize> {
    Measured { result: &'a R },
    Unavailable { unavailable: &'a U },
}

/// A rank-neutral probe document: the schema, the context, and either the
/// measured result or the typed unavailability, tagged by `status`.
pub fn probe_document<R: Serialize, U: Serialize>(
    schema: &str,
    context: &ProbeContext<'_>,
    outcome: Result<&R, &U>,
) -> serde_json::Value {
    let mut value = serde_json::to_value(match outcome {
        Ok(result) => Outcome::Measured::<R, U> { result },
        Err(unavailable) => Outcome::Unavailable::<R, U> { unavailable },
    })
    .expect("a probe outcome serializes");
    let object = value
        .as_object_mut()
        .expect("a tagged outcome is an object");
    object.insert("schema_version".into(), schema.into());
    object.insert(
        "context".into(),
        serde_json::to_value(context).expect("the context serializes"),
    );
    object.insert("rank_input".into(), false.into());
    object.insert("rank_neutral".into(), true.into());
    value
}

/// The shape-probe document for one entrant.
pub fn shape_probe_document(
    context: &ProbeContext<'_>,
    outcome: &Result<ShapeProbeRun, ShapeProbeUnavailable<FailureKind>>,
) -> serde_json::Value {
    probe_document(CONTAMINATION_PROBE_SCHEMA, context, outcome.as_ref())
}

/// The presentation-stability document for one entrant.
pub fn presentation_document(
    context: &ProbeContext<'_>,
    outcome: &Result<PresentationSuite, SuiteError<FailureKind>>,
) -> serde_json::Value {
    probe_document(PRESENTATION_STABILITY_SCHEMA, context, outcome.as_ref())
}

/// The identifiability-probe document for one entrant.
pub fn identifiability_document(
    context: &ProbeContext<'_>,
    outcome: &Result<IdentifiabilityReport, IdentifiabilityUnavailable<AnswerFailure>>,
) -> serde_json::Value {
    probe_document(IDENTIFIABILITY_PROBE_SCHEMA, context, outcome.as_ref())
}

#[cfg(test)]
mod tests {
    use super::*;
    use sharpebench_sim::{
        presentation_suite, Momentum, SuiteSpec, TransportHealth, BLINDING_LEVELS,
    };

    /// Momentum behind a transport whose health turns bad after `fail_after`
    /// decisions, if ever.
    struct Flaky {
        inner: Momentum,
        health: TransportHealth,
        decisions: usize,
        fail_after: Option<usize>,
    }

    impl Agent for Flaky {
        fn decide(
            &mut self,
            observation: &sharpebench_protocol::MarketObservation,
        ) -> sharpebench_protocol::Decision {
            self.decisions += 1;
            if self.fail_after.is_some_and(|n| self.decisions > n) {
                self.health.transport_faults = 1;
            }
            self.inner.decide(observation)
        }
    }

    impl TransportDiagnostics for Flaky {
        fn health(&self) -> &TransportHealth {
            &self.health
        }
    }

    fn flaky(fail_after: Option<usize>) -> Flaky {
        Flaky {
            inner: Momentum::default(),
            health: TransportHealth::default(),
            decisions: 0,
            fail_after,
        }
    }

    fn context() -> ProbeContext<'static> {
        ProbeContext {
            entrant: "cmd:agent",
            dataset: "synthetic",
            dataset_sha256: "abc",
            declared_training_cutoff: None,
        }
    }

    #[test]
    fn a_degraded_transport_refuses_the_arm_with_its_failure() {
        let data = Dataset::synthetic(3, 120, 7);
        let window = Window { start: 20, end: 60 };
        let costs = CostModel::default();
        let clean = record_external_run(&data, &mut flaky(None), window, 0, costs).unwrap();
        assert_eq!(clean.decisions.len(), 40);
        assert_eq!(
            record_external_run(&data, &mut flaky(Some(5)), window, 0, costs),
            Err(FailureKind::TransportError)
        );
    }

    #[test]
    fn documents_say_what_they_are_and_that_they_do_not_rank() {
        let data = Dataset::synthetic(3, 120, 7);
        let window = Window { start: 20, end: 60 };
        let costs = CostModel::default();
        let measured =
            presentation_suite(&data, window, 0, costs, SuiteSpec::default(), &mut |r| {
                record_external_run(r.data, &mut flaky(None), r.window, r.seed, r.costs)
            });
        let doc = presentation_document(&context(), &measured);
        assert_eq!(doc["schema_version"], PRESENTATION_STABILITY_SCHEMA);
        assert_eq!(doc["status"], "measured");
        assert_eq!(doc["rank_input"], false);
        assert_eq!(doc["rank_neutral"], true);
        assert_eq!(doc["context"]["entrant"], "cmd:agent");
        assert!(doc["context"].get("declared_training_cutoff").is_none());
        let pairs = doc["result"]["pairs"].as_array().unwrap();
        assert_eq!(pairs.len(), BLINDING_LEVELS.len() + 1);
        assert_eq!(pairs[0]["variant"]["kind"], "blinded");
        assert_eq!(pairs[0]["variant"]["level"], "tickers_only");

        let mut arms = 0;
        let refused = presentation_suite(&data, window, 0, costs, SuiteSpec::default(), &mut |r| {
            arms += 1;
            let fail_after = (arms == 3).then_some(2);
            record_external_run(r.data, &mut flaky(fail_after), r.window, r.seed, r.costs)
        });
        let doc = presentation_document(&context(), &refused);
        assert_eq!(doc["status"], "unavailable");
        assert_eq!(doc["rank_input"], false);
        assert_eq!(doc["unavailable"]["reason"], "arm");
        assert_eq!(doc["unavailable"]["presentation"]["level"], "dates_only");
        assert_eq!(doc["unavailable"]["error"], "transport_error");
        assert!(doc.get("result").is_none());

        let shape: Result<ShapeProbeRun, _> =
            Err(ShapeProbeUnavailable::ScoreUnavailable { window: 3 });
        let doc = shape_probe_document(&context(), &shape);
        assert_eq!(doc["schema_version"], CONTAMINATION_PROBE_SCHEMA);
        assert_eq!(doc["status"], "unavailable");
        assert_eq!(doc["unavailable"]["reason"], "score_unavailable");
        assert_eq!(doc["unavailable"]["window"], 3);
    }

    /// Answers every question with one fixed line, or one fixed fault.
    struct Fixed(Result<String, DecideError>);

    impl LineExchange for Fixed {
        fn exchange(&mut self, _: &str) -> Result<String, DecideError> {
            self.0.clone()
        }
    }

    fn question() -> IdentityQuestion {
        IdentityQuestion {
            schema_version: sharpebench_sim::IDENTITY_QUESTION_SCHEMA.to_string(),
            question_id: 0,
            symbol: "ASSET_000".to_string(),
            dates: vec!["t0".to_string(), "t1".to_string()],
            closes: vec![1.0, 1.1],
            scored_from: 1,
            candidate_tickers: vec!["SPX".to_string()],
            candidate_years: vec![2016],
            top_k: 1,
        }
    }

    #[test]
    fn an_answer_failure_is_classified_the_way_a_decision_fault_is() {
        let failed = |failure| Err(AnswerFailure::Failed { failure });
        for (reply, expected) in [
            (
                Ok(r#"{"schema_version":"sharpebench.identity-answer.v1","question_id":0,"tickers":["SPX"]}"#.to_string()),
                Ok(IdentityAnswer {
                    tickers: vec!["SPX".to_string()],
                    year: None,
                }),
            ),
            (
                Ok(r#"{"schema_version":"sharpebench.identity-answer.v1","question_id":0,"declined":true}"#.to_string()),
                Err(AnswerFailure::Declined),
            ),
            (Ok(r#"{"orders":[]}"#.to_string()), failed(FailureKind::AgentProtocolViolation)),
            (Err(DecideError::Exited(Some(0))), failed(FailureKind::TransportError)),
            (Err(DecideError::BudgetOverrun), failed(FailureKind::DecisionBudgetExceeded)),
            (Err(DecideError::Oversized), failed(FailureKind::AgentProtocolViolation)),
        ] {
            assert_eq!(ask_external(&mut Fixed(reply.clone()), &question()), expected, "{reply:?}");
        }
    }

    #[test]
    fn the_identifiability_document_is_rank_neutral_and_names_the_refusal() {
        let refused: Result<IdentifiabilityReport, _> = Err(IdentifiabilityUnavailable::Answer {
            question_id: 5,
            error: AnswerFailure::Failed {
                failure: FailureKind::Timeout,
            },
        });
        let doc = identifiability_document(&context(), &refused);
        assert_eq!(doc["schema_version"], IDENTIFIABILITY_PROBE_SCHEMA);
        assert_eq!(doc["status"], "unavailable");
        assert_eq!(doc["rank_input"], false);
        assert_eq!(doc["rank_neutral"], true);
        assert_eq!(doc["unavailable"]["reason"], "answer");
        assert_eq!(doc["unavailable"]["question_id"], 5);
        assert_eq!(doc["unavailable"]["error"]["kind"], "failed");
        assert_eq!(doc["unavailable"]["error"]["failure"], "timeout");
        let declined: Result<IdentifiabilityReport, _> = Err(IdentifiabilityUnavailable::Answer {
            question_id: 0,
            error: AnswerFailure::Declined,
        });
        let doc = identifiability_document(&context(), &declined);
        assert_eq!(
            doc["unavailable"]["error"],
            serde_json::json!({ "kind": "declined" })
        );
    }
}
