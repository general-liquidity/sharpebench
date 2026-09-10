//! Fault plans at the checkpoint and ledger boundary: a plan is bound into the
//! resume identity, injected faults are recorded on the attempt that saw them,
//! and a sweep with no plan writes exactly what it wrote before.

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use sharpebench_harness::checkpoint::run_resumable_sweep_faulted;
use sharpebench_harness::fault_plan::modes::run_faulted_backtest_observed;
use sharpebench_harness::fault_plan::{
    bind_invocation, CellId, ContractRelaxation, FaultMode, FaultPlan, FaultSpec,
    FaultedObservation, COHORT_SCALE,
};
use sharpebench_harness::{
    run_resumable_sweep_observed, AttemptDuration, AttemptObservation, AttemptRecord, FailureKind,
    ResumePolicy, SweepCheckpoint, SweepContract, SweepIdentity,
};
use sharpebench_protocol::{Action, Decision, MarketObservation, Order};
use sharpebench_sim::{Agent, CostModel, Dataset, TransportDiagnostics, TransportHealth, Window};

struct Steady {
    health: TransportHealth,
}

impl Agent for Steady {
    fn decide(&mut self, observation: &MarketObservation) -> Decision {
        Decision {
            orders: vec![Order {
                symbol: observation.symbols[0].symbol.clone(),
                action: Action::Buy,
                target_weight: 0.3,
                confidence: 0.5,
                rationale: String::new(),
            }],
            reasoning: String::new(),
            cost: None,
        }
    }
}

impl TransportDiagnostics for Steady {
    fn health(&self) -> &TransportHealth {
        &self.health
    }
}

fn steady() -> Steady {
    Steady {
        health: TransportHealth::default(),
    }
}

fn plan(seed: u64) -> FaultPlan {
    FaultPlan::new(
        seed,
        vec![
            ContractRelaxation::SubmissionAcceptance,
            ContractRelaxation::ReadYourWrites,
        ],
        vec![
            FaultSpec {
                id: "limit".to_string(),
                group: None,
                cohort_ppm: COHORT_SCALE,
                fault: FaultMode::RateLimit {
                    max_rejected_presentations: 3,
                },
            },
            FaultSpec {
                id: "lag".to_string(),
                group: None,
                cohort_ppm: COHORT_SCALE / 2,
                fault: FaultMode::ProjectionLag { max_lag_steps: 2 },
            },
        ],
    )
    .unwrap()
}

fn windows() -> Vec<Window> {
    vec![Window { start: 2, end: 22 }, Window { start: 22, end: 42 }]
}

const SEEDS: [u64; 2] = [5, 6];

fn contract(invocation: String) -> SweepContract {
    SweepContract::new(
        SweepIdentity {
            dataset_sha256: "11".repeat(32),
            cost_model_sha256: "22".repeat(32),
            score_config_sha256: "33".repeat(32),
            runner_artifact_sha256: "44".repeat(32),
            entrant_sha256: "55".repeat(32),
            invocation_sha256: invocation,
        },
        &windows(),
        &SEEDS,
        0,
    )
}

fn tmp_path(tag: &str) -> PathBuf {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    std::env::temp_dir().join(format!(
        "sharpebench-fault-{tag}-{}-{}.json",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ))
}

/// Run a faulted checkpoint sweep; `fail` names the cells that come back as
/// an unrecovered transport failure instead of running.
fn faulted_sweep(
    path: &std::path::Path,
    plan: &FaultPlan,
    policy: ResumePolicy,
    fail: &[(usize, u64)],
) -> std::io::Result<sharpebench_harness::ResilientSubmission> {
    let data = Dataset::synthetic(2, 50, 3);
    let windows = windows();
    let contract = contract(bind_invocation(&"66".repeat(32), Some(plan)));
    run_resumable_sweep_faulted(path, "entrant", &contract, &windows, policy, |w, seed| {
        if fail.contains(&(w, seed)) {
            return FaultedObservation::from(AttemptObservation::from(Err(
                FailureKind::TransportError,
            )));
        }
        let mut agent = steady();
        run_faulted_backtest_observed(
            &data,
            &mut agent,
            windows[w],
            seed,
            CostModel::default(),
            None,
            Some(plan),
        )
    })
}

/// A resume under a changed plan is refused as a different contract, and the
/// checkpoint is left exactly as it was; the unchanged plan resumes.
#[test]
fn a_changed_fault_plan_refuses_to_resume() {
    let path = tmp_path("resume");
    let first = faulted_sweep(&path, &plan(1), ResumePolicy::UnfinishedOnly, &[(1, 6)]).unwrap();
    assert_eq!(first.failures.runtime_failures(), 1);
    let before = std::fs::read(&path).unwrap();

    let refused = faulted_sweep(&path, &plan(2), ResumePolicy::RetryRuntimeFailures, &[]);
    let error = refused
        .err()
        .expect("a different plan is a different experiment");
    assert_eq!(error.kind(), std::io::ErrorKind::InvalidData);
    assert!(error.to_string().contains("contract differs"), "{error}");
    assert_eq!(std::fs::read(&path).unwrap(), before, "never overwritten");

    let resumed = faulted_sweep(&path, &plan(1), ResumePolicy::RetryRuntimeFailures, &[]).unwrap();
    assert_eq!(resumed.failures.runtime_failures(), 0);
    assert_eq!(resumed.submission.runs.len(), 4);
    std::fs::remove_file(&path).unwrap();
}

/// Every attempt made under a plan carries its evidence, so a ledger reader
/// can tell an injected fault (evidence, no failure kind) from a genuine one
/// (a failure kind the plan did not produce).
#[test]
fn injected_faults_are_recorded_on_the_attempt_ledger() {
    let path = tmp_path("ledger");
    let plan = plan(1);
    faulted_sweep(&path, &plan, ResumePolicy::UnfinishedOnly, &[(0, 5)]).unwrap();
    let checkpoint = SweepCheckpoint::load(&path).unwrap();
    std::fs::remove_file(&path).unwrap();
    let ledger = checkpoint.attempt_ledger();
    assert_eq!(ledger.len(), 4);
    let digest = plan.digest();
    for (task, record) in checkpoint.tasks.iter().zip(&ledger.attempts) {
        if (task.window, task.seed) == (0, 5) {
            assert!(record.is_failure());
            assert!(
                record.injected_faults.is_none(),
                "a genuine transport failure carries no injected evidence"
            );
            continue;
        }
        assert!(!record.is_failure());
        let evidence = record.injected_faults.as_ref().expect("faulted attempt");
        assert_eq!(evidence.plan_sha256, digest);
        assert_eq!(
            evidence.cell,
            CellId::new(windows()[task.window], task.seed)
        );
        assert!(evidence.assigned.contains(&"limit".to_string()));
        assert!(evidence
            .events
            .iter()
            .any(|event| event.fault_id() == "limit"));
    }
    let cells: Vec<CellId> = windows()
        .iter()
        .flat_map(|&w| SEEDS.map(|seed| CellId::new(w, seed)))
        .collect();
    let rows = plan.denominators_with_evidence(&cells, &ledger);
    let limit = rows.iter().find(|row| row.fault_id == "limit").unwrap();
    assert_eq!((limit.cells, limit.assigned, limit.fired), (4, 4, 3));
    let lag = rows.iter().find(|row| row.fault_id == "lag").unwrap();
    assert!(lag.fired <= lag.assigned && lag.assigned <= lag.cells);
}

/// With no plan the ledger record serializes exactly as it did before the
/// field existed, and an unfaulted sweep's checkpoint never mentions faults.
#[test]
fn an_unfaulted_sweep_writes_no_fault_evidence() {
    let record = AttemptRecord::completed(AttemptDuration::Unavailable);
    assert_eq!(
        serde_json::to_string(&record).unwrap(),
        r#"{"outcome":{"outcome":"completed"},"duration":{"source":"unavailable"}}"#
    );
    let path = tmp_path("plain");
    let data = Dataset::synthetic(2, 50, 3);
    let windows = windows();
    run_resumable_sweep_observed(
        &path,
        "entrant",
        &contract("66".repeat(32)),
        &windows,
        ResumePolicy::UnfinishedOnly,
        |w, seed| {
            let mut agent = steady();
            let faulted = run_faulted_backtest_observed(
                &data,
                &mut agent,
                windows[w],
                seed,
                CostModel::default(),
                None,
                None,
            );
            assert!(faulted.injected_faults.is_none());
            faulted.observation
        },
    )
    .unwrap();
    let written = std::fs::read_to_string(&path).unwrap();
    std::fs::remove_file(&path).unwrap();
    assert!(!written.contains("injected_faults"));
    assert!(
        written.contains(&"66".repeat(32)),
        "the invocation is unbound"
    );
}
