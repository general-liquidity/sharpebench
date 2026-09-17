//! Decision stability over captured trajectories.
//!
//! A trajectory stores each decision with the date of the observation it
//! answered, not the observation itself. The observation is still fixed by the
//! artifact: the engine builds it from the frozen dataset and the book, and the
//! book at a step depends only on the run's seed, the cost model and the
//! decisions recorded before that step. [`replay_observation_digests`] drives the
//! recorded decisions back through [`run_backtest`] and digests every observation
//! the engine presents, which is exactly the observation the agent was shown when
//! the trajectory was captured. The repository's trajectory producer,
//! [`crate::run_agent_capture`], presents unfaulted observations: fault
//! injection is armed only on `run` sweeps, which write no trajectory.
//!
//! [`decision_stability_from_trajectories`] runs the strict artifact checks on
//! every trajectory first, so the replay is against the data, costs and engine
//! the decisions were captured under, then hands the digested decisions to
//! [`sharpebench_core::decision_stability()`]. Each run's content identity is the
//! SHA-256 of its serialized JSON bytes, so a byte copy of a run is an identical
//! replicate. The returned [`DecisionStabilityEvidence`] also records what was
//! measured: the dataset, cost model, engine and runner identities, and a digest
//! of every input trajectory.

use serde::Serialize;
use sharpebench_core::{
    decision_stability, observation_sha256, DecisionStabilityReport, IdenticalReplicates,
    ObservedDecision, ReplicateRun, ScoreConfig,
};
use sharpebench_protocol::{
    AgentTrajectory, Decision, DecisionStep, MarketObservation, RunTrajectory,
};
use sharpebench_sim::{run_backtest, Agent, CostModel, Dataset, Window};

use crate::{trajectory_contract, verify_trajectory_strict};

/// One input trajectory, identified by the SHA-256 of its compact JSON
/// serialization. A byte copy of a file and a repeated capture of a
/// deterministic agent both produce an equal digest: the digest shows that
/// two inputs are equal, not how they came to be.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct InputTrajectory {
    pub trajectory_sha256: String,
    pub runs: usize,
}

/// A decision-stability report with the identities of everything it measured.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct DecisionStabilityEvidence {
    #[serde(flatten)]
    pub report: DecisionStabilityReport,
    /// The dataset, cost model and engine every trajectory was verified against.
    pub dataset_sha256: String,
    pub cost_model_sha256: String,
    pub engine_version: String,
    /// The runner identity every trajectory was verified against, when one was
    /// required.
    pub runner_artifact_sha256: Option<String>,
    /// Every input, in the order given.
    pub inputs: Vec<InputTrajectory>,
    /// Inputs whose digest equals an earlier input's.
    pub identical_inputs: usize,
}

/// Plays a run's recorded decisions and digests each observation it is shown.
struct DigestingReplay<'a> {
    steps: std::slice::Iter<'a, DecisionStep>,
    step: usize,
    digests: Vec<String>,
    refusal: Option<String>,
}

impl Agent for DigestingReplay<'_> {
    fn decide(&mut self, observation: &MarketObservation) -> Decision {
        let Some(recorded) = self.steps.next() else {
            // The engine asks once per window step and the caller supplies one
            // recorded step per window step, so this is never reached.
            return Decision {
                orders: Vec::new(),
                reasoning: String::new(),
                cost: None,
            };
        };
        if self.refusal.is_none() {
            if observation.date != recorded.observation_id {
                self.refusal = Some(format!(
                    "step {} was recorded against observation `{}` but replays against `{}`",
                    self.step, recorded.observation_id, observation.date
                ));
            } else {
                match observation_sha256(observation) {
                    Ok(digest) => self.digests.push(digest),
                    Err(error) => {
                        self.refusal = Some(format!("step {}: {error}", self.step));
                    }
                }
            }
        }
        self.step += 1;
        recorded.decision.clone()
    }
}

/// The digest ([`observation_sha256`]) of every observation `run`'s recorded
/// decisions answered, in step order.
///
/// `data` and `costs` must be the ones the run was captured under. A recorded
/// observation id that is not the replayed observation's date, a run whose step
/// count is not its window length, and an observation with a non-finite number
/// are refused.
pub fn replay_observation_digests(
    data: &Dataset,
    run: &RunTrajectory,
    costs: CostModel,
) -> Result<Vec<String>, String> {
    let required = run.window_end.saturating_sub(run.window_start);
    if run.window_end > data.len() || run.steps.len() != required {
        return Err(format!(
            "run [{}, {}) seed {} records {} decisions; its window needs {required} within a dataset of {} bars",
            run.window_start,
            run.window_end,
            run.seed,
            run.steps.len(),
            data.len()
        ));
    }
    let mut replay = DigestingReplay {
        steps: run.steps.iter(),
        step: 0,
        digests: Vec::with_capacity(required),
        refusal: None,
    };
    run_backtest(
        data,
        &mut replay,
        Window {
            start: run.window_start,
            end: run.window_end,
        },
        run.seed,
        costs,
    );
    match replay.refusal {
        Some(refusal) => Err(format!(
            "run [{}, {}) seed {}: {refusal}",
            run.window_start, run.window_end, run.seed
        )),
        None => Ok(replay.digests),
    }
}

/// Decision stability of one entrant over one or more captured trajectories.
///
/// Every run of every trajectory is a replicate of its window: the execution
/// seeds of one capture, and the same seeds again in a repeated capture. Each
/// trajectory must pass [`verify_trajectory_strict`] against `data`, `costs` and
/// `runner_artifact_sha256` (its score is discarded), and all of them must name
/// the same agent. Runs of one window with byte-identical JSON are refused
/// unless `identical` declares them separate executions. The report is
/// rank-neutral.
pub fn decision_stability_from_trajectories(
    data: &Dataset,
    trajectories: &[AgentTrajectory],
    costs: CostModel,
    runner_artifact_sha256: Option<&str>,
    identical: IdenticalReplicates,
) -> Result<DecisionStabilityEvidence, String> {
    let Some(first) = trajectories.first() else {
        return Err("decision stability needs at least one trajectory".to_string());
    };
    let cfg = ScoreConfig::default();
    let mut runs = Vec::new();
    let mut inputs: Vec<InputTrajectory> = Vec::with_capacity(trajectories.len());
    for (index, trajectory) in trajectories.iter().enumerate() {
        if trajectory.agent_id != first.agent_id {
            return Err(format!(
                "trajectory {index} is agent `{}`, but trajectory 0 is agent `{}`: a stability report covers one entrant",
                trajectory.agent_id, first.agent_id
            ));
        }
        verify_trajectory_strict(data, trajectory, costs, &cfg, runner_artifact_sha256)
            .map_err(|error| format!("trajectory {index}: {error}"))?;
        let bytes = serde_json::to_vec(trajectory).expect("trajectories serialize");
        inputs.push(InputTrajectory {
            trajectory_sha256: sharpebench_attest::content_digest(&bytes),
            runs: trajectory.runs.len(),
        });
        for run in &trajectory.runs {
            let digests = replay_observation_digests(data, run, costs)
                .map_err(|error| format!("trajectory {index}: {error}"))?;
            let bytes = serde_json::to_vec(run).expect("run trajectories serialize");
            runs.push(ReplicateRun {
                window_start: run.window_start,
                window_end: run.window_end,
                content_sha256: sharpebench_attest::content_digest(&bytes),
                steps: digests
                    .into_iter()
                    .zip(&run.steps)
                    .map(|(observation_sha256, step)| ObservedDecision {
                        observation_sha256,
                        decision: &step.decision,
                    })
                    .collect(),
            });
        }
    }
    let report =
        decision_stability(&first.agent_id, &runs, identical).map_err(|error| error.to_string())?;
    let distinct: std::collections::BTreeSet<&str> = inputs
        .iter()
        .map(|input| input.trajectory_sha256.as_str())
        .collect();
    let identical_inputs = inputs.len() - distinct.len();
    let verified = trajectory_contract(data, costs, &[], &[]);
    Ok(DecisionStabilityEvidence {
        report,
        dataset_sha256: verified.dataset_sha256,
        cost_model_sha256: verified.cost_model_sha256,
        engine_version: verified.engine_version,
        runner_artifact_sha256: runner_artifact_sha256.map(str::to_string),
        inputs,
        identical_inputs,
    })
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;

    use super::*;
    use crate::run_agent_capture;
    use sharpebench_core::{StabilityRate, StabilityUnavailable};
    use sharpebench_protocol::{Action, Order};
    use sharpebench_sim::{run_backtest_capture, BuyAndHold, Momentum};

    fn data() -> Dataset {
        Dataset::synthetic(3, 60, 20_260_916)
    }

    fn windows() -> Vec<Window> {
        vec![Window { start: 20, end: 40 }, Window { start: 40, end: 60 }]
    }

    fn momentum_capture(seeds: &[u64]) -> AgentTrajectory {
        run_agent_capture(
            "momentum",
            &data(),
            &windows(),
            seeds,
            CostModel::default(),
            || Box::new(Momentum::default()) as Box<dyn Agent>,
        )
        .1
    }

    fn measure(
        data: &Dataset,
        trajectories: &[AgentTrajectory],
        identical: IdenticalReplicates,
    ) -> Result<DecisionStabilityReport, String> {
        evidence(data, trajectories, identical).map(|evidence| evidence.report)
    }

    fn evidence(
        data: &Dataset,
        trajectories: &[AgentTrajectory],
        identical: IdenticalReplicates,
    ) -> Result<DecisionStabilityEvidence, String> {
        decision_stability_from_trajectories(
            data,
            trajectories,
            CostModel::default(),
            None,
            identical,
        )
    }

    /// Never trades, so every replicate is shown the same observations. The
    /// replicates with an odd creation index state a different confidence on
    /// every fourth step: a choice the scorer reads, with no fill that could
    /// separate the observations.
    struct PlantedFlipper {
        odd: bool,
        step: usize,
    }

    impl Agent for PlantedFlipper {
        fn decide(&mut self, observation: &MarketObservation) -> Decision {
            let confidence = if self.odd && self.step.is_multiple_of(4) {
                0.8
            } else {
                0.2
            };
            self.step += 1;
            Decision {
                orders: vec![Order {
                    symbol: observation.symbols[0].symbol.clone(),
                    action: Action::Hold,
                    target_weight: 0.0,
                    confidence: Some(confidence),
                    rationale: String::new(),
                }],
                reasoning: String::new(),
                cost: None,
            }
        }
    }

    fn planted_capture(seeds: &[u64]) -> AgentTrajectory {
        let created = Cell::new(0usize);
        run_agent_capture(
            "flipper",
            &data(),
            &windows(),
            seeds,
            CostModel::default(),
            || {
                let index = created.get();
                created.set(index + 1);
                Box::new(PlantedFlipper {
                    odd: index % 2 == 1,
                    step: 0,
                }) as Box<dyn Agent>
            },
        )
        .1
    }

    #[test]
    fn replay_reproduces_the_observations_the_agent_was_shown() {
        struct Recording {
            inner: Momentum,
            digests: Vec<String>,
        }
        impl Agent for Recording {
            fn decide(&mut self, observation: &MarketObservation) -> Decision {
                self.digests
                    .push(observation_sha256(observation).expect("finite observation"));
                self.inner.decide(observation)
            }
        }
        let data = data();
        let mut agent = Recording {
            inner: Momentum::default(),
            digests: Vec::new(),
        };
        let (_, run) = run_backtest_capture(
            &data,
            &mut agent,
            Window { start: 20, end: 60 },
            5,
            CostModel::default(),
        );
        let replayed = replay_observation_digests(&data, &run, CostModel::default()).unwrap();
        assert_eq!(replayed.len(), 40);
        assert_eq!(replayed, agent.digests);
    }

    #[test]
    fn a_deterministic_agent_reports_zero_over_seeds_and_declared_repeated_captures() {
        let seeds = [0, 1, 2, 3];
        let report = measure(
            &data(),
            &[momentum_capture(&seeds), momentum_capture(&seeds)],
            IdenticalReplicates::Declared,
        )
        .unwrap();
        assert_eq!(report.agent_id, "momentum");
        assert!(!report.rank_input);
        assert!(report.identical_replicates_declared);
        assert_eq!(report.replicate_runs, 16);
        assert_eq!(report.windows.len(), 2);
        assert!(report.windows.iter().all(|window| window.replicates == 8));
        // Each seed's second capture repeats its first byte for byte.
        assert_eq!(report.totals.identical_replicate_runs, 8);
        // Every step of a seed is matched by the same seed of the repeated
        // capture, so no step leaves the comparison.
        assert_eq!(report.totals.steps_total, 16 * 20);
        assert_eq!(report.totals.steps_compared, report.totals.steps_total);
        assert_eq!(report.totals.steps_excluded_diverged_observation, 0);
        assert_eq!(report.totals.steps_excluded_diverged_decision, 0);
        assert!(report.totals.pairs_compared >= 2 * 20);
        assert_eq!(report.totals.differing_pairs, 0);
        assert_eq!(
            report.totals.pairwise_disagreement,
            StabilityRate::Available { value: 0.0 }
        );
    }

    #[test]
    fn a_copied_capture_is_refused_unless_declared() {
        let capture = momentum_capture(&[0, 1]);
        let copy = capture.clone();
        let refused = measure(
            &data(),
            &[capture.clone(), copy.clone()],
            IdenticalReplicates::Refused,
        )
        .unwrap_err();
        assert_eq!(
            refused,
            "window [20, 40) holds 2 replicate runs identical to another replicate; a copied capture agrees with itself, so declare them separate executions or remove the copies"
        );

        let declared = evidence(
            &data(),
            &[capture.clone(), copy],
            IdenticalReplicates::Declared,
        )
        .unwrap();
        assert_eq!(declared.report.totals.identical_replicate_runs, 4);
        assert_eq!(declared.inputs.len(), 2);
        assert_eq!(declared.inputs[0], declared.inputs[1]);
        assert_eq!(declared.inputs[0].runs, 4);
        assert_eq!(declared.identical_inputs, 1);

        // One capture alone holds no copies: its runs differ in seed.
        let alone = measure(&data(), &[capture], IdenticalReplicates::Refused).unwrap();
        assert_eq!(alone.totals.identical_replicate_runs, 0);
    }

    #[test]
    fn the_evidence_names_what_it_measured() {
        let momentum = momentum_capture(&[0, 1]);
        let other_seeds = momentum_capture(&[2]);
        let runner = "ab".repeat(32);
        let mut bound = [momentum.clone(), other_seeds.clone()];
        for trajectory in &mut bound {
            trajectory.contract.as_mut().unwrap().runner_artifact_sha256 = Some(runner.clone());
        }
        let evidence = decision_stability_from_trajectories(
            &data(),
            &bound,
            CostModel::default(),
            Some(&runner),
            IdenticalReplicates::Refused,
        )
        .unwrap();
        let contract = momentum.contract.as_ref().unwrap();
        assert_eq!(evidence.dataset_sha256, contract.dataset_sha256);
        assert_eq!(evidence.cost_model_sha256, contract.cost_model_sha256);
        assert_eq!(evidence.engine_version, contract.engine_version);
        assert_eq!(
            evidence.runner_artifact_sha256.as_deref(),
            Some(runner.as_str())
        );
        assert_eq!(
            evidence
                .inputs
                .iter()
                .map(|input| input.runs)
                .collect::<Vec<_>>(),
            vec![4, 2]
        );
        assert_eq!(
            evidence.inputs[0].trajectory_sha256,
            sharpebench_attest::content_digest(&serde_json::to_vec(&bound[0]).unwrap())
        );
        assert_ne!(
            evidence.inputs[0].trajectory_sha256,
            evidence.inputs[1].trajectory_sha256
        );
        assert_eq!(evidence.identical_inputs, 0);

        let json = serde_json::to_value(&evidence).unwrap();
        assert_eq!(json["schema"], "sharpebench.decision-stability.v1");
        assert_eq!(json["dataset_sha256"], contract.dataset_sha256.as_str());
        assert_eq!(json["runner_artifact_sha256"], runner.as_str());
        assert_eq!(json["inputs"][1]["runs"], 2);

        let unbound = evidence_without_runner(&[momentum]);
        assert_eq!(unbound.runner_artifact_sha256, None);
    }

    fn evidence_without_runner(trajectories: &[AgentTrajectory]) -> DecisionStabilityEvidence {
        evidence(&data(), trajectories, IdenticalReplicates::Refused).unwrap()
    }

    #[test]
    fn a_planted_split_reports_its_pairwise_rate_and_counts_each_difference_once() {
        // Four replicates per window of 20 steps. The odd replicates choose a
        // different confidence at step 0 (4 of 6 pairs differ) and again at
        // every fourth step; after step 0 the two sides are separate histories,
        // so those repeats are not counted. Per window: 4 / (6 + 19 * 2) = 1 / 11.
        let report = measure(
            &data(),
            &[planted_capture(&[0, 1, 2, 3])],
            IdenticalReplicates::Refused,
        )
        .unwrap();
        for window in &report.windows {
            assert_eq!(window.replicates, 4);
            assert_eq!(window.counts.pairs_compared, 44);
            assert_eq!(window.counts.differing_pairs, 4);
            assert_eq!(window.counts.groups_compared, 39);
            assert_eq!(window.differing_groups.len(), 1);
            assert_eq!(window.differing_groups[0].step, 0);
            assert_eq!(window.differing_groups[0].differing_pairs, 4);
            assert_eq!(window.counts.steps_compared, 80);
        }
        assert_eq!(
            report.totals.pairwise_disagreement,
            StabilityRate::Available { value: 1.0 / 11.0 }
        );
        assert_eq!(report.totals.steps_excluded_diverged_observation, 0);
        assert_eq!(report.totals.steps_excluded_diverged_decision, 0);
    }

    #[test]
    fn steps_after_seeds_fill_differently_are_excluded_and_counted() {
        // Buy-and-hold fills at step 0 at a seed-dependent slippage, so seeds
        // share only their first observation.
        let trajectory = run_agent_capture(
            "buy-and-hold",
            &data(),
            &windows(),
            &[0, 1, 2],
            CostModel::default(),
            || Box::new(BuyAndHold) as Box<dyn Agent>,
        )
        .1;
        let report = measure(&data(), &[trajectory], IdenticalReplicates::Refused).unwrap();
        for window in &report.windows {
            assert_eq!(window.counts.groups_compared, 1);
            assert_eq!(window.counts.pairs_compared, 3);
            assert_eq!(window.counts.steps_compared, 3);
            assert_eq!(window.counts.steps_excluded_diverged_observation, 3 * 19);
        }
        assert_eq!(
            report.totals.steps_excluded_diverged_observation,
            2 * 3 * 19
        );
        assert_eq!(report.totals.steps_excluded_diverged_decision, 0);
        assert_eq!(
            report.totals.pairwise_disagreement,
            StabilityRate::Available { value: 0.0 }
        );
    }

    #[test]
    fn a_single_replicate_is_typed_unavailable() {
        let report = measure(
            &data(),
            &[planted_capture(&[7])],
            IdenticalReplicates::Refused,
        )
        .unwrap();
        assert_eq!(report.replicate_runs, 2);
        assert!(report.windows.iter().all(|window| window.replicates == 1));
        assert_eq!(report.totals.steps_unreplicated, 40);
        assert_eq!(report.totals.pairs_compared, 0);
        assert_eq!(
            report.totals.pairwise_disagreement,
            StabilityRate::Unavailable {
                reason: StabilityUnavailable::SingleReplicate
            }
        );
    }

    #[test]
    fn a_mixed_or_unbound_field_is_refused() {
        let refused = IdenticalReplicates::Refused;
        let mixed = measure(
            &data(),
            &[momentum_capture(&[0]), planted_capture(&[0])],
            refused,
        )
        .unwrap_err();
        assert!(mixed.contains("trajectory 1 is agent `flipper`"), "{mixed}");

        let mut unbound = momentum_capture(&[0]);
        unbound.contract = None;
        let error = measure(&data(), &[unbound], refused).unwrap_err();
        assert!(error.starts_with("trajectory 0: "), "{error}");
        assert!(error.contains("no execution contract"), "{error}");

        let foreign = Dataset::synthetic(3, 60, 1);
        let error = measure(&foreign, &[momentum_capture(&[0])], refused).unwrap_err();
        assert!(error.contains("does not match verifier dataset"), "{error}");

        let empty = measure(&data(), &[], refused).unwrap_err();
        assert_eq!(empty, "decision stability needs at least one trajectory");
    }

    #[test]
    fn a_replay_that_does_not_line_up_is_refused() {
        let trajectory = momentum_capture(&[0]);
        let mut renamed = trajectory.runs[0].clone();
        renamed.steps[3].observation_id = "1999-01-01".to_string();
        let error =
            replay_observation_digests(&data(), &renamed, CostModel::default()).unwrap_err();
        assert!(
            error.contains("step 3 was recorded against observation `1999-01-01`"),
            "{error}"
        );

        let mut short = trajectory.runs[0].clone();
        short.steps.pop();
        let error = replay_observation_digests(&data(), &short, CostModel::default()).unwrap_err();
        assert!(
            error.contains("records 19 decisions; its window needs 20"),
            "{error}"
        );

        let mut beyond = trajectory.runs[1].clone();
        beyond.window_start += 1;
        beyond.window_end += 1;
        let error = replay_observation_digests(&data(), &beyond, CostModel::default()).unwrap_err();
        assert!(error.contains("within a dataset of 60 bars"), "{error}");
    }
}
