//! Host-side wiring for the contamination probes: the shape probe
//! ([`sharpebench_sim::run_shape_probe`]) and the presentation suite
//! ([`sharpebench_sim::presentation_suite`]), run against any entrant the host
//! can drive as an [`Agent`] with transport diagnostics: a host process, a
//! sandboxed image or a gateway-attached entrant.
//!
//! A transport failure on any arm is a typed refusal of the whole report, never
//! a dropped arm: an external agent that degrades to a hold would otherwise put
//! the harness's holds into the comparison as the entrant's own decisions.
//!
//! Both documents are rank-neutral: nothing here reaches a score, a rank, the
//! gate or the pass^k pool, and both say so on their face.

use serde::Serialize;
use sharpebench_sim::{
    record_run, Agent, ArmRun, CostModel, Dataset, PresentationSuite, ShapeProbeRun,
    ShapeProbeUnavailable, SuiteError, TransportDiagnostics, Window,
};

use crate::{transport_failure, FailureKind};

/// Schema of the shape-probe document.
pub const CONTAMINATION_PROBE_SCHEMA: &str = "sharpebench.contamination-probe.v1";
/// Schema of the presentation-stability document.
pub const PRESENTATION_STABILITY_SCHEMA: &str = "sharpebench.presentation-stability.v1";

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
}
