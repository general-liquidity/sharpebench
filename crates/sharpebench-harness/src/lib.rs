//! sb-harness — run orchestration.
//!
//! Drives an [`sharpebench_sim::Agent`] through the [`sharpebench_sim`] backtest across every
//! window × seed, capturing each run's return series and decision trace into the
//! [`sharpebench_core::AgentSubmission`] format the scoring kernel consumes. Producing one
//! `Run` per (window, seed) is what makes pass^k and multi-window OOS meaningful.
#![forbid(unsafe_code)]

pub mod checkpoint;
pub mod failure;
pub mod perturb;

pub use checkpoint::{
    run_resumable_sweep, run_resumable_sweep_bound, SweepCheckpoint, SweepContract, SweepIdentity,
    TaskRecord, TaskState,
};
pub use failure::{
    apply_oom_verdict, failing_sentinel_run, run_with_retries, FailureKind, FailureLog,
    FailureRecord, RunOutcome,
};

use sharpebench_core::{AgentSubmission, MandateVerdict};
use sharpebench_protocol::{AgentTrajectory, RunTrajectory, TrajectoryContract, TrajectoryWindow};
use sharpebench_sim::{
    run_backtest, run_backtest_capture, Agent, CostModel, Dataset, RandomAgent, TeamAgent,
    TransportDiagnostics, TransportHealth, Window,
};

/// Run a fresh agent (produced by `make_agent`) across every `window` × `seed`
/// and assemble the submission — one `Run` per (window, seed).
pub fn run_agent<F>(
    agent_id: &str,
    data: &Dataset,
    windows: &[Window],
    seeds: &[u64],
    costs: CostModel,
    mut make_agent: F,
) -> AgentSubmission
where
    F: FnMut() -> Box<dyn Agent>,
{
    let mut runs = Vec::new();
    for &w in windows {
        for &seed in seeds {
            let mut agent = make_agent();
            runs.push(run_backtest(data, agent.as_mut(), w, seed, costs));
        }
    }
    AgentSubmission {
        agent_id: agent_id.to_string(),
        runs,
        in_sample_trials: 0,
        candidates: Vec::new(),
    }
}

/// Like [`run_agent`], but also captures the persisted [`AgentTrajectory`] artifact
/// — the agent's raw per-window×seed decisions — alongside the [`AgentSubmission`].
/// The submission's runs and the trajectory's runs are produced in lock-step (same
/// window-major order), so the trajectory can be replayed back into a byte-identical
/// submission by the separate verifier ([`sharpebench_sim::replay_submission`]).
pub fn run_agent_capture<F>(
    agent_id: &str,
    data: &Dataset,
    windows: &[Window],
    seeds: &[u64],
    costs: CostModel,
    mut make_agent: F,
) -> (AgentSubmission, AgentTrajectory)
where
    F: FnMut() -> Box<dyn Agent>,
{
    let mut runs = Vec::new();
    let mut traj_runs: Vec<RunTrajectory> = Vec::new();
    for &w in windows {
        for &seed in seeds {
            let mut agent = make_agent();
            let (run, traj) = run_backtest_capture(data, agent.as_mut(), w, seed, costs);
            runs.push(run);
            traj_runs.push(traj);
        }
    }
    let submission = AgentSubmission {
        agent_id: agent_id.to_string(),
        runs,
        in_sample_trials: 0,
        candidates: Vec::new(),
    };
    let trajectory = AgentTrajectory {
        agent_id: agent_id.to_string(),
        contract: Some(trajectory_contract(data, costs, windows, seeds)),
        in_sample_trials: 0,
        runs: traj_runs,
        declared_mandate: None,
    };
    (submission, trajectory)
}

fn semantic_digest<T: serde::Serialize>(value: &T) -> String {
    let bytes = serde_json::to_vec(value).expect("benchmark semantic inputs serialize");
    sharpebench_attest::content_digest(&bytes)
}

/// Bit-exact identity of the execution-cost model. JSON represents every
/// non-finite float as `null`, but `max_participation = +infinity` is the
/// shipped unlimited-liquidity setting. Hashing the IEEE-754 bits keeps that
/// setting distinct from NaN and any future non-finite sentinel.
pub fn cost_model_digest(costs: CostModel) -> String {
    let noise = costs.noise.map_or_else(
        || "none".to_string(),
        |noise| {
            format!(
                "some:{:016x}:{:016x}:{:016x}:{:016x}",
                noise.delay_prob.to_bits(),
                noise.min_fill_frac.to_bits(),
                noise.carry_floor.to_bits(),
                noise.queue_participation_ref.to_bits()
            )
        },
    );
    let trf = costs.trf_cost.map_or_else(
        || "none".to_string(),
        |value| format!("some:{:016x}", value.to_bits()),
    );
    let preimage = format!(
        "sharpebench-cost-model-v1|{:016x}|{:016x}|{:016x}|{:016x}|{:016x}|{trf}|{noise}",
        costs.fee_bps.to_bits(),
        costs.slippage_bps.to_bits(),
        costs.impact_bps.to_bits(),
        costs.financing_bps.to_bits(),
        costs.max_participation.to_bits(),
    );
    sharpebench_attest::content_digest(preimage.as_bytes())
}

/// Contract for the semantic inputs a trajectory capture consumes. This is
/// public so non-CLI callers can validate the same identity without duplicating
/// canonicalization rules.
pub fn trajectory_contract(
    data: &Dataset,
    costs: CostModel,
    windows: &[Window],
    seeds: &[u64],
) -> TrajectoryContract {
    TrajectoryContract {
        schema_version: TrajectoryContract::SCHEMA_VERSION,
        dataset_sha256: semantic_digest(data),
        cost_model_sha256: cost_model_digest(costs),
        engine_version: env!("CARGO_PKG_VERSION").to_string(),
        windows: windows
            .iter()
            .map(|window| TrajectoryWindow {
                start: window.start,
                end: window.end,
            })
            .collect(),
        seeds: seeds.to_vec(),
        runner_artifact_sha256: None,
    }
}

/// Like [`run_agent`], but the factory receives the run's execution `seed` — for
/// agents (e.g. [`RandomAgent`]) whose behaviour should vary per run rather than
/// being identical across seeds.
pub fn run_seeded_agent<F>(
    agent_id: &str,
    data: &Dataset,
    windows: &[Window],
    seeds: &[u64],
    costs: CostModel,
    mut make_agent: F,
) -> AgentSubmission
where
    F: FnMut(u64) -> Box<dyn Agent>,
{
    let mut runs = Vec::new();
    for &w in windows {
        for &seed in seeds {
            let mut agent = make_agent(seed);
            runs.push(run_backtest(data, agent.as_mut(), w, seed, costs));
        }
    }
    AgentSubmission {
        agent_id: agent_id.to_string(),
        runs,
        in_sample_trials: 0,
        candidates: Vec::new(),
    }
}

/// A submission assembled under the retry-vs-runtime-failure taxonomy: the runs
/// that fed pass^k plus the harness-side [`FailureLog`].
pub struct ResilientSubmission {
    pub submission: AgentSubmission,
    pub failures: FailureLog,
}

/// Like [`run_agent`], but resilient to container/runtime flakiness via the
/// [`failure`] taxonomy. For each (window, seed) the `attempt` closure produces
/// either a scorable [`sharpebench_core::Run`] or a typed [`FailureKind`]; runtime
/// errors are retried up to `max_retries`, agent faults are not.
///
/// The invariant the scorer depends on: **only genuine agent pass/fail runs enter
/// the submission's run pool.** An exhausted runtime error contributes *no* run
/// (it is the harness's fault, logged but never scored), while a non-retryable
/// agent fault contributes a failing sentinel run so it counts against pass^k.
pub fn run_agent_resilient<F>(
    agent_id: &str,
    n_windows: usize,
    seeds: &[u64],
    max_retries: u32,
    expected_run_len: usize,
    attempt: F,
) -> ResilientSubmission
where
    F: FnMut(usize, u64) -> Result<sharpebench_core::Run, FailureKind>,
{
    run_agent_resilient_by_window(
        agent_id,
        &vec![expected_run_len; n_windows],
        seeds,
        max_retries,
        attempt,
    )
}

/// Per-window-length variant of [`run_agent_resilient`]. Use this whenever the
/// execution plan contains unequal windows so an agent-fault sentinel preserves
/// the exact cell geometry instead of inheriting the first window's length.
pub fn run_agent_resilient_by_window<F>(
    agent_id: &str,
    expected_run_lens: &[usize],
    seeds: &[u64],
    max_retries: u32,
    mut attempt: F,
) -> ResilientSubmission
where
    F: FnMut(usize, u64) -> Result<sharpebench_core::Run, FailureKind>,
{
    let mut runs = Vec::new();
    let mut failures = FailureLog::default();
    for (w, &expected_run_len) in expected_run_lens.iter().enumerate() {
        for &seed in seeds {
            let (outcome, _) = run_with_retries(max_retries, || attempt(w, seed));
            match outcome {
                RunOutcome::Completed(run) => runs.push(run),
                RunOutcome::Exhausted { last, attempts } => {
                    // Harness fault: excluded from the pass^k pool entirely.
                    failures.push(FailureRecord {
                        window_index: w,
                        seed,
                        kind: last,
                        attempts,
                        runtime: true,
                    });
                }
                RunOutcome::AgentFault(kind) => {
                    // Agent fault: a genuine pass^k failure (failing sentinel run).
                    failures.push(FailureRecord {
                        window_index: w,
                        seed,
                        kind,
                        attempts: 1,
                        runtime: false,
                    });
                    runs.push(failing_sentinel_run(expected_run_len));
                }
            }
        }
    }
    ResilientSubmission {
        submission: AgentSubmission {
            agent_id: agent_id.to_string(),
            runs,
            in_sample_trials: 0,
            candidates: Vec::new(),
        },
        failures,
    }
}

/// Map an external agent's post-run [`TransportHealth`] to the failure taxonomy, so
/// a masked degrade-to-hold surfaces as a typed failure instead of a flat (and
/// silently mis-attributed) return series. A protocol fault is the agent's own and
/// counts against pass^k ([`FailureKind::AgentProtocolViolation`]); an unrecovered
/// transport blip or a tripped breaker is the harness's fault and is retried at the
/// run level ([`FailureKind::TransportError`]). `None` when the run was clean.
pub fn transport_failure(health: &TransportHealth) -> Option<FailureKind> {
    if health.protocol_faults > 0 {
        Some(FailureKind::AgentProtocolViolation)
    } else if health.tripped || health.transport_faults > 0 {
        Some(FailureKind::TransportError)
    } else {
        None
    }
}

/// Drive an external agent through one backtest, **surfacing** transport failures as
/// a typed [`FailureKind`] instead of letting a masked degrade-to-hold silently bias
/// the return series flat. Returns the scorable [`sharpebench_core::Run`] only when
/// the run was transport-clean; otherwise the classified failure.
pub fn run_external_backtest<A>(
    data: &Dataset,
    agent: &mut A,
    window: Window,
    seed: u64,
    costs: CostModel,
) -> Result<sharpebench_core::Run, FailureKind>
where
    A: Agent + TransportDiagnostics,
{
    let run = run_backtest(data, agent, window, seed, costs);
    match transport_failure(agent.health()) {
        Some(kind) => Err(kind),
        None => Ok(run),
    }
}

/// Run an external agent across every `window` × `seed` under the resilient failure
/// taxonomy, spawning a **fresh** agent per attempt via `spawn` (which returns `None`
/// when the process/endpoint can't be created - a [`FailureKind::SpawnError`]). A
/// transport blip is retried up to `max_retries`; a persistent transport failure is
/// logged and excluded from pass^k (harness fault), while an agent protocol fault
/// becomes a failing sentinel run (counts against pass^k). This is the run/stress/
/// capture path made transport-honest: no masked holds enter the score.
pub fn run_external_agent<A, F>(
    agent_id: &str,
    data: &Dataset,
    windows: &[Window],
    seeds: &[u64],
    costs: CostModel,
    max_retries: u32,
    mut spawn: F,
) -> ResilientSubmission
where
    A: Agent + TransportDiagnostics,
    F: FnMut() -> Option<A>,
{
    let expected_lens: Vec<usize> = windows
        .iter()
        .map(|window| window.end.saturating_sub(window.start))
        .collect();
    run_agent_resilient_by_window(agent_id, &expected_lens, seeds, max_retries, |wi, seed| {
        match spawn() {
            Some(mut agent) => run_external_backtest(data, &mut agent, windows[wi], seed, costs),
            None => Err(FailureKind::SpawnError),
        }
    })
}

/// Produce `n_agents` random "monkey" submissions — the **luck floor**. Each is a
/// distinctly-seeded [`RandomAgent`] run across every window × seed, so the field
/// shows the distribution of zero-skill outcomes a genuine edge must clear. None
/// should be rank-eligible.
pub fn luck_floor(
    data: &Dataset,
    windows: &[Window],
    seeds: &[u64],
    costs: CostModel,
    n_agents: usize,
) -> Vec<AgentSubmission> {
    (0..n_agents)
        .map(|k| {
            let base = 0xF100_0000_0000_0000 ^ (k as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15);
            run_seeded_agent(
                &format!("luck-floor-{k:02}"),
                data,
                windows,
                seeds,
                costs,
                move |seed| Box::new(RandomAgent::new(base ^ seed)) as Box<dyn Agent>,
            )
        })
        .collect()
}

/// Elicit dominance choices from a live agent to feed
/// [`sharpebench_core::assess_rationality`]. Each scenario presents `n_symbols` assets whose
/// trailing returns encode a known "value", with exactly one clearly-best asset
/// rotated to a different position each time. The agent's choice is read off as the
/// asset it allocates the most weight to. A return-seeking agent should pick the
/// best asset; an indiscriminate one leaves value on the table.
pub fn probe_dominance(
    agent: &mut dyn Agent,
    n_scenarios: usize,
    n_symbols: usize,
) -> Vec<sharpebench_core::DominanceChoice> {
    use sharpebench_protocol::{MarketObservation, PositionState, SymbolSnapshot};
    use std::collections::BTreeMap;

    (0..n_scenarios)
        .map(|s| {
            let best = if n_symbols > 0 { s % n_symbols } else { 0 };
            // Known option values: one clear winner, distinct losers elsewhere.
            let values: Vec<f64> = (0..n_symbols)
                .map(|i| {
                    if i == best {
                        0.05
                    } else {
                        -0.01 - 0.005 * i as f64
                    }
                })
                .collect();
            let symbols: Vec<SymbolSnapshot> = values
                .iter()
                .enumerate()
                .map(|(i, &v)| SymbolSnapshot {
                    symbol: format!("SYM{i:02}"),
                    close_history: vec![1.0, 1.0 + v], // trailing return = v
                    fundamentals: BTreeMap::new(),
                    news: Vec::new(),
                })
                .collect();
            let portfolio = symbols
                .iter()
                .map(|s| PositionState {
                    symbol: s.symbol.clone(),
                    shares: 0.0,
                    avg_price: 0.0,
                })
                .collect();
            let obs = MarketObservation {
                date: format!("t{s}"),
                cash: 1.0,
                symbols,
                portfolio,
            };
            let decision = agent.decide(&obs);
            // Chosen = the asset given the most target weight (first on ties).
            let mut chosen = 0usize;
            let mut best_w = f64::NEG_INFINITY;
            for i in 0..n_symbols {
                let sym = format!("SYM{i:02}");
                let w = decision
                    .orders
                    .iter()
                    .find(|o| o.symbol == sym)
                    .map(|o| o.target_weight)
                    .unwrap_or(0.0);
                if w > best_w {
                    best_w = w;
                    chosen = i;
                }
            }
            sharpebench_core::DominanceChoice {
                options: values,
                chosen,
            }
        })
        .collect()
}

/// Probe an agent's economic rationality over `n_scenarios` dominance menus.
pub fn probe_rationality(
    agent: &mut dyn Agent,
    n_scenarios: usize,
    n_symbols: usize,
) -> sharpebench_core::EconRationalityReport {
    let choices = probe_dominance(agent, n_scenarios, n_symbols);
    sharpebench_core::assess_rationality(&choices, &[], n_symbols)
}

/// Segment a submission's runs by window and report out-of-sample edge decay.
/// [`run_agent`] lays runs out window-major (all seeds of window 0, then window 1,
/// …), so the first `runs.len()/n_windows` runs are the in-sample window and the
/// rest are out-of-sample. Pools each window's returns and calls
/// [`sharpebench_core::oos_decay`].
pub fn oos_decay_of(
    submission: &AgentSubmission,
    n_windows: usize,
) -> sharpebench_core::OosDecayReport {
    let n = submission.runs.len();
    let per = n.checked_div(n_windows).unwrap_or(n).max(1);
    let windows: Vec<Vec<f64>> = (0..n_windows)
        .map(|w| {
            let lo = (w * per).min(n);
            let hi = ((w + 1) * per).min(n);
            submission.runs[lo..hi]
                .iter()
                .flat_map(|r| r.returns.iter().copied())
                .collect()
        })
        .collect();
    sharpebench_core::oos_decay(&windows)
}

/// The output of the separate-verifier path: a score recomputed purely by replaying
/// a persisted [`AgentTrajectory`] through the engine, plus an explanation asserting
/// the recompute is algorithmic (never the agent's self-reported word).
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct VerificationResult {
    pub agent_id: String,
    /// The score recomputed from the replayed submission.
    pub score: sharpebench_core::CompositeScore,
    /// Number of (window × seed) runs replayed from the artifact.
    pub runs_replayed: usize,
    /// Total raw decision steps replayed across all runs.
    pub decisions_replayed: usize,
    /// Human-readable assertion that the score derives from raw decisions alone.
    pub verification_explanation: String,
    /// A single artifact cannot establish a relative mandate because the
    /// same-cell benchmark trajectory is absent. This field makes that boundary
    /// explicit instead of reporting a contradictory definitive failure.
    pub declared_verdict: DeclaredVerdictVerification,
}

/// Status of the declaration attached to one independently replayed artifact.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum DeclaredVerdictVerification {
    NotDeclared,
    VerifiedWithoutField,
    FieldRequired { benchmark_id: String },
}

/// Separate-verifier path: ingest a persisted trajectory artifact, **replay** its raw
/// per-step decisions through the frozen dataset's point-in-time engine to regenerate
/// every `Run`, then recompute the composite score with the core scorer. The agent's
/// self-reported returns/metrics are never read — only its decisions are trusted, and
/// the score is derived from replaying them. Mirrors the trust boundary the kernel
/// already enforces, but makes the *artifact → score* recompute explicit and isolated.
pub fn verify_trajectory(
    data: &Dataset,
    traj: &AgentTrajectory,
    costs: CostModel,
    cfg: &sharpebench_core::ScoreConfig,
) -> VerificationResult {
    let submission = sharpebench_sim::replay_submission(data, traj, costs);
    // A relative declaration is only meaningful beside its benchmark's
    // same-cell submission. Never score it as a standalone definitive failure:
    // replay the host score and return a typed field-required status instead.
    let declared_verdict = traj.declared_mandate.as_ref().map(MandateVerdict::of);
    let (score, declared_verdict) = match declared_verdict {
        Some(MandateVerdict::RelativeTo { benchmark_id }) => (
            sharpebench_core::score_agent(&submission, cfg),
            DeclaredVerdictVerification::FieldRequired { benchmark_id },
        ),
        Some(_) => (
            sharpebench_core::score_agent_declared(
                &submission,
                traj.declared_mandate.as_ref(),
                cfg,
            ),
            DeclaredVerdictVerification::VerifiedWithoutField,
        ),
        None => (
            sharpebench_core::score_agent(&submission, cfg),
            DeclaredVerdictVerification::NotDeclared,
        ),
    };
    let runs_replayed = traj.runs.len();
    let decisions_replayed = traj.runs.iter().map(|r| r.steps.len()).sum();
    VerificationResult {
        agent_id: traj.agent_id.clone(),
        score,
        runs_replayed,
        decisions_replayed,
        verification_explanation: format!(
            "Score recomputed algorithmically by replaying {decisions_replayed} raw \
             decisions across {runs_replayed} runs through the point-in-time engine on \
             the frozen dataset; the agent's self-reported returns and metrics were not \
             read. The artifact carries decisions only — every return, cost, and gate is \
             regenerated by the verifier."
        ),
        declared_verdict,
    }
}

/// Strict separate-verifier path. It refuses legacy or mismatched artifacts
/// before replay, so a deterministic recomputation cannot silently be a
/// deterministic recomputation of different market conditions.
pub fn verify_trajectory_strict(
    data: &Dataset,
    traj: &AgentTrajectory,
    costs: CostModel,
    cfg: &sharpebench_core::ScoreConfig,
    runner_artifact_sha256: Option<&str>,
) -> Result<VerificationResult, String> {
    if traj.runs.is_empty() {
        return Err("trajectory has no runs; an empty artifact measured nothing".to_string());
    }
    if let Some(index) = traj.runs.iter().position(|run| run.steps.is_empty()) {
        return Err(format!(
            "trajectory run {index} has no decision steps; partial evidence cannot be graded as a completed run"
        ));
    }
    let expected = trajectory_contract(data, costs, &[], &[]);
    let contract = traj.contract.as_ref().ok_or_else(|| {
        "trajectory has no execution contract; recapture it or pass the CLI's explicit legacy override"
            .to_string()
    })?;
    if contract.schema_version != expected.schema_version {
        return Err(format!(
            "trajectory contract schema {} is unsupported (expected {})",
            contract.schema_version, expected.schema_version
        ));
    }
    if contract.dataset_sha256 != expected.dataset_sha256 {
        return Err(format!(
            "trajectory dataset {} does not match verifier dataset {}",
            contract.dataset_sha256, expected.dataset_sha256
        ));
    }
    if contract.cost_model_sha256 != expected.cost_model_sha256 {
        return Err(format!(
            "trajectory cost model {} does not match verifier cost model {}",
            contract.cost_model_sha256, expected.cost_model_sha256
        ));
    }
    if contract.engine_version != expected.engine_version {
        return Err(format!(
            "trajectory engine {} does not match verifier engine {}",
            contract.engine_version, expected.engine_version
        ));
    }
    if contract.windows.is_empty() || contract.seeds.is_empty() {
        return Err(
            "trajectory contract does not declare its complete window-by-seed execution matrix"
                .to_string(),
        );
    }
    let mut unique_windows = std::collections::BTreeSet::new();
    for window in &contract.windows {
        if window.start >= window.end || window.end > data.len() {
            return Err(format!(
                "trajectory contract window [{}, {}) is outside dataset length {}",
                window.start,
                window.end,
                data.len()
            ));
        }
        if !unique_windows.insert((window.start, window.end)) {
            return Err(format!(
                "trajectory contract repeats window [{}, {})",
                window.start, window.end
            ));
        }
    }
    let unique_seeds: std::collections::BTreeSet<u64> = contract.seeds.iter().copied().collect();
    if unique_seeds.len() != contract.seeds.len() {
        return Err("trajectory contract repeats an execution seed".to_string());
    }
    let expected_runs = contract
        .windows
        .len()
        .checked_mul(contract.seeds.len())
        .ok_or_else(|| "trajectory execution matrix is too large".to_string())?;
    if traj.runs.len() != expected_runs {
        return Err(format!(
            "trajectory execution matrix is incomplete: expected {expected_runs} runs from {} windows x {} seeds, found {}",
            contract.windows.len(),
            contract.seeds.len(),
            traj.runs.len()
        ));
    }
    for (index, (window, seed)) in contract
        .windows
        .iter()
        .flat_map(|window| contract.seeds.iter().map(move |seed| (window, seed)))
        .enumerate()
    {
        let run = &traj.runs[index];
        if (run.window_start, run.window_end, run.seed) != (window.start, window.end, *seed) {
            return Err(format!(
                "trajectory run {index} has coordinates [{}, {}) seed {}, expected [{}, {}) seed {}",
                run.window_start,
                run.window_end,
                run.seed,
                window.start,
                window.end,
                seed
            ));
        }
        let expected_steps = window.end - window.start;
        if run.steps.len() != expected_steps {
            return Err(format!(
                "trajectory run {index} has {} decision steps, expected {expected_steps}",
                run.steps.len()
            ));
        }
        for (step_index, step) in run.steps.iter().enumerate() {
            if step.step != step_index {
                return Err(format!(
                    "trajectory run {index} decision {step_index} declares step {}",
                    step.step
                ));
            }
            let expected_observation = &data.dates[window.start + step_index];
            if &step.observation_id != expected_observation {
                return Err(format!(
                    "trajectory run {index} decision {step_index} names observation `{}`, expected `{expected_observation}`",
                    step.observation_id
                ));
            }
        }
    }
    match (
        contract.runner_artifact_sha256.as_deref(),
        runner_artifact_sha256,
    ) {
        (Some(captured), Some(current)) if captured != current => {
            return Err(format!(
                "trajectory runner {captured} does not match verifier runner {current}"
            ));
        }
        (None, Some(_)) => {
            return Err(
                "trajectory has no runner artifact identity; strict cross-process verification requires the exact capture binary"
                    .to_string(),
            );
        }
        _ => {}
    }
    let mut bound_cfg = cfg.clone();
    bound_cfg.execution_seeds_per_window = contract.seeds.len();
    Ok(verify_trajectory(data, traj, costs, &bound_cfg))
}

/// One member of a trading team: a name plus a factory for fresh instances (the
/// engine needs an independent agent per run).
pub struct TeamMember {
    pub name: String,
    pub make: Box<dyn Fn() -> Box<dyn Agent>>,
}

impl TeamMember {
    pub fn new<F>(name: &str, make: F) -> Self
    where
        F: Fn() -> Box<dyn Agent> + 'static,
    {
        Self {
            name: name.to_string(),
            make: Box::new(make),
        }
    }
}

/// A team run: the team's own submission (scored as a unit) plus each member's
/// solo pooled return series over the same windows × seeds — exactly the inputs
/// [`sharpebench_core::attribute_roles`] needs to estimate who carried the team.
pub struct TeamResult {
    pub team: AgentSubmission,
    pub role_returns: Vec<(String, Vec<f64>)>,
}

/// Run a `members` team as a consensus [`TeamAgent`] across every window × seed,
/// and separately run each member solo to capture its role-level return series.
pub fn run_team(
    team_id: &str,
    data: &Dataset,
    windows: &[Window],
    seeds: &[u64],
    costs: CostModel,
    members: &[TeamMember],
) -> TeamResult {
    let mut runs = Vec::new();
    for &w in windows {
        for &seed in seeds {
            let instances: Vec<Box<dyn Agent>> = members.iter().map(|m| (m.make)()).collect();
            let mut team = TeamAgent { members: instances };
            runs.push(run_backtest(data, &mut team, w, seed, costs));
        }
    }
    let team = AgentSubmission {
        agent_id: team_id.to_string(),
        runs,
        in_sample_trials: 0,
        candidates: Vec::new(),
    };

    let role_returns = members
        .iter()
        .map(|m| {
            let sub = run_agent(&m.name, data, windows, seeds, costs, || (m.make)());
            let pooled: Vec<f64> = sub
                .runs
                .iter()
                .flat_map(|r| r.returns.iter().copied())
                .collect();
            (m.name.clone(), pooled)
        })
        .collect();

    TeamResult { team, role_returns }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sharpebench_core::roles::attribute_roles;
    use sharpebench_protocol::DeclaredMandate;
    use sharpebench_sim::{BuyAndHold, DecideError, Momentum};

    /// A mock external agent whose per-decision transport health is scripted, so we
    /// can exercise the surface-vs-mask path without a real subprocess / socket. It
    /// always returns a hold, but records a fault into its health when told to.
    struct MockExternal {
        fault: Option<DecideError>,
        health: TransportHealth,
    }
    impl MockExternal {
        fn clean() -> Self {
            Self {
                fault: None,
                health: TransportHealth::default(),
            }
        }
        fn faulty(err: DecideError) -> Self {
            Self {
                fault: Some(err),
                health: TransportHealth::default(),
            }
        }
    }
    impl Agent for MockExternal {
        fn decide(
            &mut self,
            _obs: &sharpebench_protocol::MarketObservation,
        ) -> sharpebench_protocol::Decision {
            if let Some(err) = self.fault {
                // A transport blip degrades to a hold, but is *recorded* - the whole
                // point: this hold is a masked fault, not a deliberate one.
                self.health.record(err, false);
            }
            sharpebench_protocol::Decision {
                orders: Vec::new(),
                reasoning: "mock".to_string(),
                cost: None,
            }
        }
    }
    impl TransportDiagnostics for MockExternal {
        fn health(&self) -> &TransportHealth {
            &self.health
        }
    }

    #[test]
    fn transport_fault_surfaces_as_a_distinct_failure_not_a_hold() {
        let data = Dataset::synthetic(3, 60, 5);
        let window = Window { start: 20, end: 60 };
        let costs = CostModel::default();

        // A clean external agent scores as a normal run.
        let mut clean = MockExternal::clean();
        assert!(run_external_backtest(&data, &mut clean, window, 1, costs).is_ok());

        // A transport blip is surfaced as TransportError, NOT an indistinguishable
        // hold that would silently flatten the return series.
        let mut faulty = MockExternal::faulty(DecideError::Transport);
        assert_eq!(
            run_external_backtest(&data, &mut faulty, window, 1, costs).unwrap_err(),
            FailureKind::TransportError,
        );

        // A protocol fault is the agent's own → AgentProtocolViolation.
        let mut bad = MockExternal::faulty(DecideError::Protocol);
        assert_eq!(
            run_external_backtest(&data, &mut bad, window, 1, costs).unwrap_err(),
            FailureKind::AgentProtocolViolation,
        );
    }

    #[test]
    fn run_external_agent_retries_a_transient_blip_then_recovers() {
        let data = Dataset::synthetic(3, 60, 5);
        let windows = [Window { start: 20, end: 60 }];
        let seeds = [0u64];
        // The first spawned instance faults its whole run; a fresh respawn is clean.
        let mut spawned = 0u32;
        let res = run_external_agent(
            "flaky-endpoint",
            &data,
            &windows,
            &seeds,
            CostModel::default(),
            2,
            || {
                spawned += 1;
                if spawned == 1 {
                    Some(MockExternal::faulty(DecideError::Transport))
                } else {
                    Some(MockExternal::clean())
                }
            },
        );
        // Retried at the run level and recovered → a genuine completed run, nothing
        // logged as a failure.
        assert!(res.failures.is_empty(), "a recovered blip logs no failure");
        assert_eq!(
            res.submission.runs.len(),
            1,
            "the recovered run feeds pass^k"
        );
    }

    #[test]
    fn run_external_agent_fails_a_dead_endpoint_explicitly() {
        let data = Dataset::synthetic(3, 60, 5);
        let windows = [Window { start: 20, end: 60 }];
        let seeds = [0u64, 1u64];
        // Every respawn is still broken → the transport error exhausts its retries
        // and is logged as a runtime failure, excluded from the pass^k pool (rather
        // than masquerading as a stream of holds).
        let res = run_external_agent(
            "dead-endpoint",
            &data,
            &windows,
            &seeds,
            CostModel::default(),
            2,
            || Some(MockExternal::faulty(DecideError::Transport)),
        );
        assert_eq!(
            res.failures.runtime_failures(),
            2,
            "both seeds fail explicitly"
        );
        assert_eq!(res.failures.agent_faults(), 0);
        assert!(
            res.submission.runs.is_empty(),
            "no masked-hold run enters the score"
        );
    }

    #[test]
    fn run_external_agent_spawn_failure_is_a_runtime_error() {
        let data = Dataset::synthetic(3, 60, 5);
        let windows = [Window { start: 20, end: 60 }];
        let seeds = [0u64];
        let res = run_external_agent::<MockExternal, _>(
            "unspawnable",
            &data,
            &windows,
            &seeds,
            CostModel::default(),
            1,
            || None,
        );
        assert_eq!(res.failures.runtime_failures(), 1);
        assert!(res.submission.runs.is_empty());
    }

    /// A reward-hacking "cheat" agent: tries to exploit the simulator by placing an
    /// absurd-size order (gaming the fill engine), with maxed-out self-reported
    /// confidence. The sim flags the absurd weight as a `ManipulativeOrder`
    /// (block-severity), so the scorer must rank it ineligible.
    struct CheatAgent;
    impl Agent for CheatAgent {
        fn decide(
            &mut self,
            obs: &sharpebench_protocol::MarketObservation,
        ) -> sharpebench_protocol::Decision {
            use sharpebench_protocol::{Action, Decision, Order};
            let sym = obs.symbols[0].symbol.clone();
            Decision {
                orders: vec![Order {
                    symbol: sym,
                    action: Action::Buy,
                    target_weight: 1.0e9, // absurd size → sim-exploitation attempt
                    confidence: 1.0,      // inflated conviction
                    rationale: "exploit the fill engine".to_string(),
                }],
                reasoning: "cheat".to_string(),
                cost: None,
            }
        }
    }

    #[test]
    fn cheat_agent_is_demoted_and_never_ranks() {
        use sharpebench_core::{rank, ScoreConfig};
        let data = Dataset::synthetic(5, 120, 20_260_621);
        let windows = [Window {
            start: 20,
            end: 120,
        }];
        let seeds: Vec<u64> = (0..5).collect();
        let costs = CostModel::default();

        let cheat = run_agent("cheat", &data, &windows, &seeds, costs, || {
            Box::new(CheatAgent) as Box<dyn Agent>
        });
        let honest = run_agent("momentum", &data, &windows, &seeds, costs, || {
            Box::new(Momentum::default()) as Box<dyn Agent>
        });
        let board = rank(&[cheat, honest], &ScoreConfig::default());
        let cheat_s = board.iter().find(|s| s.agent_id == "cheat").unwrap();
        assert!(
            !cheat_s.rank_eligible,
            "a sim-exploiting cheat must never be rank-eligible: {cheat_s:?}"
        );
        assert!(
            !cheat_s.process_ok,
            "the manipulative order must mark the process dirty"
        );
        // And it must sort below any eligible honest agent.
        assert!(
            board[0].agent_id != "cheat",
            "the cheat must not lead the board"
        );
    }

    #[test]
    fn team_run_produces_alignable_role_series() {
        let data = Dataset::synthetic(4, 80, 20_260_621);
        let windows = [Window { start: 20, end: 80 }];
        let seeds: Vec<u64> = (0..3).collect();
        let members = [
            TeamMember::new("momentum", || {
                Box::new(Momentum::default()) as Box<dyn Agent>
            }),
            TeamMember::new("buy-and-hold", || Box::new(BuyAndHold) as Box<dyn Agent>),
        ];
        let res = run_team(
            "team",
            &data,
            &windows,
            &seeds,
            CostModel::default(),
            &members,
        );

        assert_eq!(res.role_returns.len(), 2);
        let team_pooled: Vec<f64> = res
            .team
            .runs
            .iter()
            .flat_map(|r| r.returns.iter().copied())
            .collect();
        for (_, series) in &res.role_returns {
            assert_eq!(series.len(), team_pooled.len(), "role series must align");
        }
        // The attribution analyzer accepts the produced series.
        let attr = attribute_roles(&team_pooled, &res.role_returns);
        assert_eq!(attr.len(), 2);
    }

    #[test]
    fn rationality_probe_separates_discriminating_agents() {
        // Momentum concentrates on the single best asset → fully rational.
        let mut mo = Momentum::default();
        let r_mo = probe_rationality(&mut mo, 8, 4);
        assert_eq!(
            r_mo.rationality_score, 1.0,
            "momentum should pick the winner"
        );

        // Buy-and-hold spreads weight indiscriminately → leaves value on the table.
        let mut bh = BuyAndHold;
        let r_bh = probe_rationality(&mut bh, 8, 4);
        assert!(
            r_mo.rationality_score > r_bh.rationality_score,
            "a discriminating agent should out-score an indiscriminate one"
        );
    }

    #[test]
    fn oos_decay_segments_a_submission_by_window() {
        let data = Dataset::synthetic(4, 160, 7);
        let windows = [
            Window { start: 20, end: 90 },
            Window {
                start: 90,
                end: 160,
            },
        ];
        let seeds: Vec<u64> = (0..3).collect();
        let sub = run_agent("bh", &data, &windows, &seeds, CostModel::default(), || {
            Box::new(BuyAndHold) as Box<dyn Agent>
        });
        let report = oos_decay_of(&sub, 2);
        assert_eq!(report.window_metrics.len(), 2);
    }

    #[test]
    fn capture_replay_score_round_trips_exactly() {
        use sharpebench_core::{score_agent, ScoreConfig};
        use sharpebench_sim::replay_submission;

        let data = Dataset::synthetic(5, 160, 20_260_621);
        let windows = [
            Window { start: 20, end: 90 },
            Window {
                start: 90,
                end: 160,
            },
        ];
        let seeds: Vec<u64> = (0..4).collect();
        let costs = CostModel::default();

        // Direct run, capturing the trajectory artifact alongside the submission.
        let (direct_sub, traj) =
            run_agent_capture("momentum", &data, &windows, &seeds, costs, || {
                Box::new(Momentum::default()) as Box<dyn Agent>
            });

        // Persist + reload the artifact (the verifier ingests JSON off disk).
        let json = serde_json::to_string(&traj).unwrap();
        let reloaded: sharpebench_protocol::AgentTrajectory = serde_json::from_str(&json).unwrap();

        // Separate verifier: replay the raw decisions into a fresh submission and
        // score it. The recomputed score must match the direct-run score exactly.
        let replayed_sub = replay_submission(&data, &reloaded, costs);
        let cfg = ScoreConfig::default();
        let direct_score = score_agent(&direct_sub, &cfg);
        let replayed_score = score_agent(&replayed_sub, &cfg);
        assert_eq!(
            serde_json::to_string(&direct_score).unwrap(),
            serde_json::to_string(&replayed_score).unwrap(),
            "replayed score must equal the direct-run score"
        );
    }

    #[test]
    fn tampered_trajectory_scores_differently() {
        use sharpebench_core::{score_agent, ScoreConfig};
        use sharpebench_sim::replay_submission;

        let data = Dataset::synthetic(5, 160, 20_260_621);
        let windows = [Window {
            start: 20,
            end: 160,
        }];
        let seeds: Vec<u64> = (0..4).collect();
        let costs = CostModel::default();
        let (direct_sub, mut traj) =
            run_agent_capture("momentum", &data, &windows, &seeds, costs, || {
                Box::new(Momentum::default()) as Box<dyn Agent>
            });

        // Forge the artifact: double every staked weight. The honest recompute
        // produces a different score than the genuine run — the verifier can't be
        // fooled by editing the persisted decisions.
        for run in &mut traj.runs {
            for step in &mut run.steps {
                for order in &mut step.decision.orders {
                    order.target_weight *= 2.0;
                }
            }
        }
        let cfg = ScoreConfig::default();
        let honest = score_agent(&direct_sub, &cfg);
        let forged = score_agent(&replay_submission(&data, &traj, costs), &cfg);
        assert_ne!(
            honest.raw_mean_return, forged.raw_mean_return,
            "a tampered trajectory must recompute to a different score"
        );
    }

    #[test]
    fn strict_replay_refuses_same_geometry_with_different_market_data() {
        let data = Dataset::synthetic(3, 80, 20_260_621);
        let windows = [Window { start: 20, end: 80 }];
        let costs = CostModel::default();
        let (_, trajectory) = run_agent_capture("momentum", &data, &windows, &[0], costs, || {
            Box::new(Momentum::default()) as Box<dyn Agent>
        });
        let mut changed = data.clone();
        *changed
            .closes
            .values_mut()
            .next()
            .and_then(|series| series.get_mut(40))
            .expect("fixture has the changed bar") += 1.0;

        let error = verify_trajectory_strict(
            &changed,
            &trajectory,
            costs,
            &sharpebench_core::ScoreConfig::default(),
            None,
        )
        .expect_err("same dates and windows over different prices must be refused");
        assert!(error.contains("trajectory dataset"));
    }

    #[test]
    fn strict_replay_refuses_unbound_and_different_runner_artifacts() {
        let data = Dataset::synthetic(3, 80, 20_260_621);
        let windows = [Window { start: 20, end: 80 }];
        let costs = CostModel::default();
        let (_, mut trajectory) =
            run_agent_capture("momentum", &data, &windows, &[0], costs, || {
                Box::new(Momentum::default()) as Box<dyn Agent>
            });
        trajectory
            .contract
            .as_mut()
            .expect("new captures are bound")
            .runner_artifact_sha256 = Some("11".repeat(32));
        let error = verify_trajectory_strict(
            &data,
            &trajectory,
            costs,
            &sharpebench_core::ScoreConfig::default(),
            Some(&"22".repeat(32)),
        )
        .expect_err("different executable identities must be visible");
        assert!(error.contains("trajectory runner"));

        trajectory.contract = None;
        let error = verify_trajectory_strict(
            &data,
            &trajectory,
            costs,
            &sharpebench_core::ScoreConfig::default(),
            None,
        )
        .expect_err("legacy artifacts cannot prove their conditions");
        assert!(error.contains("no execution contract"));

        let (_, trajectory) = run_agent_capture("momentum", &data, &windows, &[0], costs, || {
            Box::new(Momentum::default()) as Box<dyn Agent>
        });
        let error = verify_trajectory_strict(
            &data,
            &trajectory,
            costs,
            &sharpebench_core::ScoreConfig::default(),
            Some(&"22".repeat(32)),
        )
        .expect_err("a verifier requiring exact runner identity must not accept an absent one");
        assert!(error.contains("no runner artifact identity"));
    }

    #[test]
    fn strict_replay_refuses_empty_or_partial_evidence_envelopes() {
        let data = Dataset::synthetic(3, 80, 20_260_621);
        let costs = CostModel::default();
        let (_, mut trajectory) = run_agent_capture(
            "momentum",
            &data,
            &[Window { start: 20, end: 80 }],
            &[0],
            costs,
            || Box::new(Momentum::default()) as Box<dyn Agent>,
        );
        let run = trajectory.runs.pop().expect("fixture has one run");
        let error = verify_trajectory_strict(
            &data,
            &trajectory,
            costs,
            &sharpebench_core::ScoreConfig::default(),
            None,
        )
        .expect_err("an empty envelope cannot prove a benchmark result");
        assert!(error.contains("measured nothing"));

        trajectory.runs.push(RunTrajectory {
            steps: Vec::new(),
            ..run
        });
        let error = verify_trajectory_strict(
            &data,
            &trajectory,
            costs,
            &sharpebench_core::ScoreConfig::default(),
            None,
        )
        .expect_err("a run with no decisions cannot count as completed evidence");
        assert!(error.contains("partial evidence"));
    }

    #[test]
    fn strict_replay_requires_the_exact_declared_execution_matrix() {
        let data = Dataset::synthetic(3, 100, 20_260_621);
        let windows = [
            Window { start: 20, end: 60 },
            Window {
                start: 60,
                end: 100,
            },
        ];
        let seeds = [3, 5];
        let costs = CostModel::default();
        let (_, trajectory) = run_agent_capture("momentum", &data, &windows, &seeds, costs, || {
            Box::new(Momentum::default()) as Box<dyn Agent>
        });
        let verify = |candidate: &AgentTrajectory| {
            verify_trajectory_strict(
                &data,
                candidate,
                costs,
                &sharpebench_core::ScoreConfig::default(),
                None,
            )
        };

        let mut missing = trajectory.clone();
        missing.runs.pop();
        let error = verify(&missing).expect_err("a missing cell must be refused");
        assert!(error.contains("execution matrix is incomplete"), "{error}");

        let mut duplicated = trajectory.clone();
        duplicated.runs[1] = duplicated.runs[0].clone();
        let error = verify(&duplicated).expect_err("a duplicated cell must be refused");
        assert!(error.contains("has coordinates"), "{error}");

        let mut reordered = trajectory.clone();
        reordered.runs.swap(0, 1);
        let error = verify(&reordered).expect_err("cell order is part of the contract");
        assert!(error.contains("has coordinates"), "{error}");

        let mut truncated = trajectory.clone();
        truncated.runs[0].steps.truncate(1);
        let error = verify(&truncated).expect_err("a nonempty partial run must be refused");
        assert!(error.contains("decision steps"), "{error}");

        let mut wrong_step = trajectory.clone();
        wrong_step.runs[0].steps[1].step = 9;
        let error = verify(&wrong_step).expect_err("step metadata must be verified");
        assert!(error.contains("declares step"), "{error}");

        let mut wrong_observation = trajectory.clone();
        wrong_observation.runs[0].steps[1].observation_id = "wrong-date".to_string();
        let error = verify(&wrong_observation).expect_err("observation identity must be verified");
        assert!(error.contains("names observation"), "{error}");

        verify(&trajectory).expect("the intact execution matrix verifies");
    }

    #[test]
    fn strict_replay_derives_the_execution_replicate_count_from_the_contract() {
        let data = Dataset::synthetic(4, 120, 20_260_621);
        let windows = [
            Window { start: 20, end: 70 },
            Window {
                start: 70,
                end: 120,
            },
        ];
        let seeds = [0, 1, 2, 3];
        let costs = CostModel::default();
        let (direct, trajectory) =
            run_agent_capture("momentum", &data, &windows, &seeds, costs, || {
                Box::new(Momentum::default()) as Box<dyn Agent>
            });
        let direct_cfg = sharpebench_core::ScoreConfig {
            execution_seeds_per_window: seeds.len(),
            ..sharpebench_core::ScoreConfig::default()
        };
        let direct_score = sharpebench_core::score_agent(&direct, &direct_cfg);
        let verified = verify_trajectory_strict(
            &data,
            &trajectory,
            costs,
            &sharpebench_core::ScoreConfig::default(),
            None,
        )
        .expect("the verifier derives the four-replicate grouping");
        assert_eq!(
            serde_json::to_string(&direct_score).unwrap(),
            serde_json::to_string(&verified.score).unwrap(),
            "strict replay must preserve the capture's replicate semantics"
        );
    }

    #[test]
    fn cost_contract_distinguishes_non_finite_bit_patterns() {
        let infinite = CostModel::default();
        let mut nan = infinite;
        nan.max_participation = f64::NAN;
        assert_ne!(
            cost_model_digest(infinite),
            cost_model_digest(nan),
            "JSON would encode both values as null, so the contract must hash their bits"
        );
    }

    #[test]
    fn runtime_crash_is_not_counted_as_an_agent_failure() {
        // A skilled agent whose container crashes (runtime error) on the first
        // seed of each window, then recovers on retry. pass^k must see only the
        // recovered, genuine runs — no failing run is injected for the crash.
        let skilled = |_w: usize, _seed: u64| {
            Ok(sharpebench_core::Run {
                returns: (0..60)
                    .map(|i| 0.002 + 0.0005 * (i as f64 * 0.7).sin())
                    .collect(),
                trace: Default::default(),
                confidences: Vec::new(),
                outcomes: Vec::new(),
                cost: 0.0,
            })
        };
        let mut crash_then_ok_calls = 0u32;
        let res = run_agent_resilient("flaky-but-skilled", 1, &[0, 1, 2], 3, 60, |w, seed| {
            crash_then_ok_calls += 1;
            // Crash on the very first attempt of the whole submission, recover after.
            if crash_then_ok_calls == 1 {
                Err(FailureKind::SpawnError)
            } else {
                skilled(w, seed)
            }
        });
        // The spawn error was retried, not logged as exhausted, and every run pool
        // entry is a genuine completed run (no sentinel injected).
        assert!(res.failures.is_empty(), "a recovered crash logs nothing");
        assert_eq!(res.submission.runs.len(), 3, "3 genuine runs feed pass^k");

        use sharpebench_core::{score_agent, ScoreConfig};
        let s = score_agent(&res.submission, &ScoreConfig::default());
        assert!(s.passed_k, "recovered runs are all skilled → pass^k holds");
    }

    #[test]
    fn exhausted_runtime_error_does_not_pollute_pass_k() {
        // One seed's container never comes back; the other two are skilled. The
        // dead seed contributes NO run (harness fault), so the surviving pass^k
        // pool is the two genuine skilled runs.
        let res = run_agent_resilient("one-dead-container", 1, &[0, 1, 2], 2, 60, |_w, seed| {
            if seed == 1 {
                Err(FailureKind::TransportError)
            } else {
                Ok(sharpebench_core::Run {
                    returns: (0..60)
                        .map(|i| 0.002 + 0.0005 * (i as f64 * 0.7).sin())
                        .collect(),
                    trace: Default::default(),
                    confidences: Vec::new(),
                    outcomes: Vec::new(),
                    cost: 0.0,
                })
            }
        });
        assert_eq!(res.failures.runtime_failures(), 1);
        assert_eq!(res.failures.agent_faults(), 0);
        assert_eq!(res.submission.runs.len(), 2, "dead container adds no run");
    }

    #[test]
    fn resilient_sentinels_preserve_unequal_window_lengths() {
        let res = run_agent_resilient_by_window(
            "malformed-agent",
            &[40, 60],
            &[0],
            0,
            |_window, _seed| Err(FailureKind::AgentProtocolViolation),
        );
        let lengths: Vec<usize> = res
            .submission
            .runs
            .iter()
            .map(|run| run.returns.len())
            .collect();
        assert_eq!(lengths, vec![40, 60]);
    }

    #[test]
    fn agent_fault_counts_against_pass_k() {
        // A malformed-output agent fault is the agent's own failure: it becomes a
        // failing sentinel run and breaks pass^k, exactly as a genuine bad run would.
        let res = run_agent_resilient("malformed-agent", 1, &[0, 1, 2], 3, 60, |_w, seed| {
            if seed == 2 {
                Err(FailureKind::AgentProtocolViolation)
            } else {
                Ok(sharpebench_core::Run {
                    returns: (0..60)
                        .map(|i| 0.002 + 0.0005 * (i as f64 * 0.7).sin())
                        .collect(),
                    trace: Default::default(),
                    confidences: Vec::new(),
                    outcomes: Vec::new(),
                    cost: 0.0,
                })
            }
        });
        assert_eq!(res.failures.agent_faults(), 1);
        assert_eq!(res.failures.runtime_failures(), 0);
        assert_eq!(
            res.submission.runs.len(),
            3,
            "the fault becomes a sentinel run"
        );

        use sharpebench_core::{score_agent, ScoreConfig};
        let s = score_agent(&res.submission, &ScoreConfig::default());
        assert!(!s.passed_k, "an agent fault must break pass^k");
    }

    #[test]
    fn luck_floor_agents_do_not_clear_the_gates() {
        use sharpebench_core::{rank, ScoreConfig};
        let data = Dataset::synthetic(5, 120, 20_260_621);
        let windows = [Window {
            start: 20,
            end: 120,
        }];
        let seeds: Vec<u64> = (0..5).collect();
        let floor = luck_floor(&data, &windows, &seeds, CostModel::default(), 4);
        assert_eq!(floor.len(), 4);
        let board = rank(&floor, &ScoreConfig::default());
        assert!(
            board.iter().all(|s| !s.rank_eligible),
            "a random monkey must never be rank-eligible"
        );
    }

    #[test]
    fn standalone_replay_does_not_definitively_fail_a_relative_declaration() {
        let data = Dataset::synthetic(2, 60, 12);
        let windows = [Window { start: 20, end: 60 }];
        let (_sub, mut trajectory) = run_agent_capture(
            "candidate",
            &data,
            &windows,
            &[1],
            CostModel::default(),
            || Box::new(BuyAndHold),
        );
        trajectory.declared_mandate = Some(DeclaredMandate::RelativeTo {
            benchmark_id: "buy-and-hold".to_string(),
        });
        let verified = verify_trajectory(
            &data,
            &trajectory,
            CostModel::default(),
            &sharpebench_core::ScoreConfig::default(),
        );
        assert_eq!(
            verified.declared_verdict,
            DeclaredVerdictVerification::FieldRequired {
                benchmark_id: "buy-and-hold".to_string(),
            }
        );
        assert!(verified.score.declared_mandate_eligible.is_none());
    }
}
