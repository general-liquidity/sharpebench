//! External entrants for the trajectory commands: `capture --cmd | --http |
//! --image` records an entrant's raw decisions, and `verify-trajectory
//! --reexecute --image` re-runs a digest-pinned image through the same
//! hardened launch `run --image` uses.
//!
//! Either command has one honest outcome per transport problem: a spawn,
//! transport, protocol, resource or finalization failure is recorded the
//! first time it is seen and the command refuses, because an external agent
//! that degrades to a hold would otherwise put the harness's holds into the
//! trajectory as the entrant's own decisions.

use std::cell::RefCell;
use std::process::ExitCode;
use std::rc::Rc;

use sharpebench_harness::FailureKind;
use sharpebench_sim::{Agent, HoldAgent, TransportDiagnostics};

use crate::{emit_json, flag_value, WatchedAgent};

/// The first failure seen while an external agent ran, shared between the
/// agent wrappers of one command and the code that reports its outcome.
pub(crate) type FaultCell = Rc<RefCell<Option<FailureKind>>>;

/// The transport flags that name an external entrant.
const TRANSPORTS: [&str; 3] = ["--cmd", "--http", "--image"];

/// A launched sandboxed entrant: a transport while it runs, then a post-exit
/// resource verdict. `Some(true)` is a breach of the published memory budget.
pub(crate) trait SandboxEntrant: Agent + TransportDiagnostics {
    fn finish(self: Box<Self>) -> Result<Option<bool>, String>;
}

impl SandboxEntrant for sharpebench_arena::SandboxedAgent {
    fn finish(self: Box<Self>) -> Result<Option<bool>, String> {
        sharpebench_arena::SandboxedAgent::finish(*self).map_err(|error| error.to_string())
    }
}

/// How a digest-pinned image becomes a sandboxed entrant. The production
/// implementation is [`DockerSandbox`]; tests substitute a fake so the
/// per-run lifecycle is checked without a daemon.
pub(crate) trait SandboxLauncher {
    /// Refuse before any container starts: no daemon, a reference that is not
    /// digest-pinned, or an image that is not present locally.
    fn admit(&self, image: &str) -> Result<(), String>;
    /// Start one fresh container.
    fn launch(&self, image: &str) -> Result<Box<dyn SandboxEntrant>, String>;
}

/// The hardened Docker boundary, with the checks and launch `run --image`
/// makes for an unscanned image: `resolve_launch` and `require_local_image`
/// before anything starts, then `run_external_sandboxed` per container with
/// default options, so there is no host fallback and no unpinned reference.
pub(crate) struct DockerSandbox;

impl SandboxLauncher for DockerSandbox {
    fn admit(&self, image: &str) -> Result<(), String> {
        sharpebench_arena::resolve_launch(
            sharpebench_arena::docker_available(),
            image,
            &sharpebench_arena::SandboxOptions::default(),
        )
        .and_then(|_| sharpebench_arena::require_local_image(image))
        .map_err(|error| error.to_string())
    }

    fn launch(&self, image: &str) -> Result<Box<dyn SandboxEntrant>, String> {
        sharpebench_arena::run_external_sandboxed(
            image,
            &sharpebench_arena::SandboxOptions::default(),
        )
        .map(|agent| Box::new(agent) as Box<dyn SandboxEntrant>)
        .map_err(|error| error.to_string())
    }
}

/// One sandboxed run. Its transport health is watched like any external
/// agent's, and when the harness drops it at the end of the run the container
/// is finished: an out-of-memory verdict becomes `ResourceLimitExceeded`, as
/// `run --image` folds it with `apply_oom_verdict`, and an indeterminate
/// verdict or failed cleanup becomes a transport failure. Once a failure is
/// recorded the container is not asked again: the outcome is already a
/// refusal, and a silent entrant would otherwise spend a decide timeout on
/// every remaining step.
struct SandboxRun {
    inner: Option<Box<dyn SandboxEntrant>>,
    image: String,
    fault: FaultCell,
}

impl Agent for SandboxRun {
    fn decide(
        &mut self,
        observation: &sharpebench_protocol::MarketObservation,
    ) -> sharpebench_protocol::Decision {
        if self.fault.borrow().is_some() {
            return HoldAgent.decide(observation);
        }
        let inner = self
            .inner
            .as_mut()
            .expect("the entrant is present until the run is dropped");
        let decision = inner.decide(observation);
        if let Some(kind) = sharpebench_harness::transport_failure(inner.health()) {
            self.fault.borrow_mut().get_or_insert(kind);
        }
        decision
    }
}

impl Drop for SandboxRun {
    fn drop(&mut self) {
        let Some(inner) = self.inner.take() else {
            return;
        };
        match inner.finish() {
            Ok(Some(true)) => *self.fault.borrow_mut() = Some(FailureKind::ResourceLimitExceeded),
            Ok(_) => {}
            Err(error) => {
                eprintln!(
                    "error: sandboxed agent `{}` could not be finalized: {error}",
                    self.image
                );
                self.fault
                    .borrow_mut()
                    .get_or_insert(FailureKind::TransportError);
            }
        }
    }
}

/// A fresh container per call. Once a failure is recorded the outcome is
/// already a refusal, so no further container is started and the remaining
/// runs are driven by an inert hold agent whose decisions are never used.
pub(crate) fn sandbox_factory<'a>(
    image: &'a str,
    launcher: &'a dyn SandboxLauncher,
    fault: FaultCell,
) -> impl FnMut() -> Box<dyn Agent> + 'a {
    move || {
        if fault.borrow().is_some() {
            return Box::new(HoldAgent) as Box<dyn Agent>;
        }
        match launcher.launch(image) {
            Ok(inner) => Box::new(SandboxRun {
                inner: Some(inner),
                image: image.to_string(),
                fault: fault.clone(),
            }),
            Err(error) => {
                eprintln!("error: cannot start the sandboxed agent `{image}`: {error}");
                fault.borrow_mut().get_or_insert(FailureKind::SpawnError);
                Box::new(HoldAgent)
            }
        }
    }
}

/// Is any external transport named on a `capture` command line?
pub(crate) fn names_external_entrant(args: &[String]) -> bool {
    args.iter().any(|arg| TRANSPORTS.contains(&arg.as_str()))
}

/// The value of a transport flag that must carry one.
fn transport_value<'a>(args: &'a [String], flag: &str) -> Result<&'a str, String> {
    flag_value(args, flag)
        .filter(|value| !value.starts_with("--") && !value.trim().is_empty())
        .ok_or_else(|| format!("{flag} needs a value"))
}

/// How a captured external trajectory names its entrant, and the flag that
/// re-runs it: `cmd:<command line>` (`--cmd`), `http:<addr>` (`--http`) or
/// `sandbox:<repository@sha256:...>` (`--image`).
struct Entrant {
    agent_id: String,
    flag: &'static str,
    target: String,
}

/// `capture <out.json> --cmd "<prog>" | --http <addr> | --image <ref>`: record
/// an external entrant's trajectory over the transports `run` supports.
pub(crate) fn run_capture_external(
    args: &[String],
    json: bool,
    launcher: &dyn SandboxLauncher,
) -> ExitCode {
    use sharpebench_sim::{CostModel, ExternalAgent, HttpAgent};

    let usage = "usage: sharpebench capture <out.json> (--cmd \"<prog>\" | --http <addr> | --image <repository@sha256:...>) [--data <csv>] [--json]";
    let named: Vec<&str> = TRANSPORTS
        .into_iter()
        .filter(|flag| args.iter().any(|arg| arg == flag))
        .collect();
    if named.len() != 1 {
        eprintln!("error: capture records one entrant; pass exactly one of --cmd, --http or --image\n{usage}");
        return ExitCode::from(2);
    }
    let flag = named[0];
    let Some(out) = args.get(2).filter(|out| !out.starts_with("--")) else {
        eprintln!("{usage}");
        return ExitCode::from(2);
    };
    if let Some(extra) = args.get(3).filter(|arg| !arg.starts_with("--")) {
        eprintln!(
            "error: {flag} names the entrant, so capture takes only <out.json>; `{extra}` is not accepted beside it\n{usage}"
        );
        return ExitCode::from(2);
    }
    if args.iter().any(|arg| arg == "--scan-policy") {
        eprintln!("error: --scan-policy is a `run --image` preflight; capture launches the pinned reference itself and does not scan it");
        return ExitCode::from(2);
    }
    let target = match transport_value(args, flag) {
        Ok(target) => target.to_string(),
        Err(error) => {
            eprintln!("error: {error}\n{usage}");
            return ExitCode::from(2);
        }
    };
    let (data, windows) = match crate::resolve_dataset(args) {
        Ok(dw) => dw,
        Err(e) => {
            eprintln!("error: {e}");
            return ExitCode::FAILURE;
        }
    };
    let seeds: Vec<u64> = (0..8).collect();
    let costs = CostModel::default();
    let fault: FaultCell = Rc::new(RefCell::new(None));

    let (entrant, make): (Entrant, Box<dyn FnMut() -> Box<dyn Agent> + '_>) = match flag {
        "--cmd" => {
            let parts: Vec<String> = target.split_whitespace().map(String::from).collect();
            let (prog, rest) = parts
                .split_first()
                .map(|(prog, rest)| (prog.clone(), rest.to_vec()))
                .expect("a nonblank --cmd value has a program");
            eprintln!(
                "warning: capture --cmd runs `{prog}` directly on this host with NO sandbox, once per captured run. Only point it at an agent you trust; use --image for an untrusted entrant."
            );
            let rest_refs: Vec<&str> = rest.iter().map(String::as_str).collect();
            if ExternalAgent::spawn(&prog, &rest_refs).is_err() {
                eprintln!("error: cannot spawn agent `{target}`");
                return ExitCode::FAILURE;
            }
            let fault = fault.clone();
            (
                Entrant {
                    agent_id: format!("cmd:{}", parts.join(" ")),
                    flag: "--cmd",
                    target: parts.join(" "),
                },
                Box::new(move || {
                    if fault.borrow().is_some() {
                        return Box::new(HoldAgent) as Box<dyn Agent>;
                    }
                    let rest_refs: Vec<&str> = rest.iter().map(String::as_str).collect();
                    match ExternalAgent::spawn(&prog, &rest_refs) {
                        Ok(agent) => Box::new(WatchedAgent {
                            inner: agent,
                            fault: fault.clone(),
                        }),
                        Err(_) => {
                            fault.borrow_mut().get_or_insert(FailureKind::SpawnError);
                            Box::new(HoldAgent)
                        }
                    }
                }),
            )
        }
        "--http" => {
            let fault = fault.clone();
            let addr = target.clone();
            (
                Entrant {
                    agent_id: format!("http:{target}"),
                    flag: "--http",
                    target: target.clone(),
                },
                Box::new(move || {
                    if fault.borrow().is_some() {
                        return Box::new(HoldAgent) as Box<dyn Agent>;
                    }
                    Box::new(WatchedAgent {
                        inner: HttpAgent::new(addr.clone()),
                        fault: fault.clone(),
                    })
                }),
            )
        }
        _ => {
            if let Err(error) = launcher.admit(&target) {
                eprintln!("error: cannot start the sandboxed agent `{target}`: {error}");
                return ExitCode::FAILURE;
            }
            (
                Entrant {
                    agent_id: format!("sandbox:{target}"),
                    flag: "--image",
                    target: target.clone(),
                },
                Box::new(sandbox_factory(&target, launcher, fault.clone())),
            )
        }
    };
    let (_sub, mut traj) = sharpebench_harness::run_agent_capture(
        &entrant.agent_id,
        &data,
        &windows,
        &seeds,
        costs,
        make,
    );
    if let Some(kind) = fault.borrow().clone() {
        if json {
            emit_json(&serde_json::json!({
                "captured": false,
                "error": "capture_transport_failure",
                "failure": kind,
                "agent": entrant.agent_id,
            }));
        }
        eprintln!(
            "error: capture of `{}` hit a {kind:?} failure, so its trajectory would record degraded decisions as the entrant's own; nothing was written",
            entrant.agent_id
        );
        return ExitCode::FAILURE;
    }
    match crate::current_executable_sha256() {
        Ok(digest) => {
            if let Some(contract) = &mut traj.contract {
                contract.runner_artifact_sha256 = Some(digest);
            }
        }
        Err(error) => {
            eprintln!("error: {error}");
            return ExitCode::FAILURE;
        }
    }
    let payload = match serde_json::to_string_pretty(&traj) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("error: serializing trajectory: {e}");
            return ExitCode::FAILURE;
        }
    };
    if let Err(e) = std::fs::write(out, payload) {
        eprintln!("error: cannot write {out}: {e}");
        return ExitCode::FAILURE;
    }
    if json {
        emit_json(&serde_json::json!({
            "captured": true,
            "agent_id": entrant.agent_id,
            "runs": traj.runs.len(),
            "path": out,
            "reexecute_with": [entrant.flag, entrant.target],
        }));
    } else {
        println!(
            "captured trajectory for `{}` ({} runs) -> {out}",
            entrant.agent_id,
            traj.runs.len()
        );
        let data = flag_value(args, "--data")
            .map(|path| format!(" --data {path}"))
            .unwrap_or_default();
        println!(
            "re-execute it with: sharpebench verify-trajectory {out}{data} --reexecute {} \"{}\"",
            entrant.flag, entrant.target
        );
    }
    ExitCode::SUCCESS
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;
    use std::process::ExitCode;

    use sharpebench_sim::{BuyAndHold, Momentum, TransportHealth};

    use super::*;

    const IMAGE: &str =
        "example/entrant@sha256:0000000000000000000000000000000000000000000000000000000000000000";

    /// What the fake containers do, by launch index.
    #[derive(Clone, Copy)]
    struct Script {
        /// The policy every container runs: momentum when true, else buy-and-hold.
        momentum: bool,
        /// Refuse to start this launch.
        refuse_launch: Option<usize>,
        /// This launch reports a transport fault after its first decision.
        transport_fault: Option<usize>,
        /// This launch finishes with an out-of-memory verdict.
        oom: Option<usize>,
        /// This launch cannot be finalized.
        finalize_error: Option<usize>,
    }

    const CLEAN: Script = Script {
        momentum: false,
        refuse_launch: None,
        transport_fault: None,
        oom: None,
        finalize_error: None,
    };

    /// A daemon-free stand-in for the Docker boundary that records every
    /// container it starts and every container that is finished.
    struct FakeSandbox {
        script: Script,
        admit: Result<(), String>,
        launched: Cell<usize>,
        finished: Rc<RefCell<Vec<usize>>>,
    }

    impl FakeSandbox {
        fn new(script: Script) -> Self {
            Self {
                script,
                admit: Ok(()),
                launched: Cell::new(0),
                finished: Rc::new(RefCell::new(Vec::new())),
            }
        }

        fn refusing(reason: &str) -> Self {
            Self {
                admit: Err(reason.to_string()),
                ..Self::new(CLEAN)
            }
        }
    }

    struct FakeContainer {
        id: usize,
        policy: Box<dyn Agent>,
        health: TransportHealth,
        script: Script,
        finished: Rc<RefCell<Vec<usize>>>,
    }

    impl Agent for FakeContainer {
        fn decide(
            &mut self,
            observation: &sharpebench_protocol::MarketObservation,
        ) -> sharpebench_protocol::Decision {
            let decision = self.policy.decide(observation);
            if self.script.transport_fault == Some(self.id) {
                self.health.transport_faults = 1;
            }
            decision
        }
    }

    impl TransportDiagnostics for FakeContainer {
        fn health(&self) -> &TransportHealth {
            &self.health
        }
    }

    impl SandboxEntrant for FakeContainer {
        fn finish(self: Box<Self>) -> Result<Option<bool>, String> {
            self.finished.borrow_mut().push(self.id);
            if self.script.finalize_error == Some(self.id) {
                return Err("inspection failed".to_string());
            }
            Ok(Some(self.script.oom == Some(self.id)))
        }
    }

    impl SandboxLauncher for FakeSandbox {
        fn admit(&self, image: &str) -> Result<(), String> {
            assert_eq!(image, IMAGE);
            self.admit.clone()
        }

        fn launch(&self, image: &str) -> Result<Box<dyn SandboxEntrant>, String> {
            assert_eq!(image, IMAGE);
            let id = self.launched.get();
            self.launched.set(id + 1);
            if self.script.refuse_launch == Some(id) {
                return Err("no such container".to_string());
            }
            let policy: Box<dyn Agent> = if self.script.momentum {
                Box::new(Momentum::default())
            } else {
                Box::new(BuyAndHold)
            };
            Ok(Box::new(FakeContainer {
                id,
                policy,
                health: TransportHealth::default(),
                script: self.script,
                finished: self.finished.clone(),
            }))
        }
    }

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|arg| arg.to_string()).collect()
    }

    fn scratch(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "sharpebench-external-capture-{}-{name}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn capture(out: &std::path::Path, sandbox: &FakeSandbox) -> ExitCode {
        run_capture_external(
            &args(&[
                "sharpebench",
                "capture",
                out.to_str().unwrap(),
                "--image",
                IMAGE,
            ]),
            true,
            sandbox,
        )
    }

    fn load(path: &std::path::Path) -> sharpebench_protocol::AgentTrajectory {
        serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap()
    }

    /// What `run_reexecution` computes for `--image`: the strict checks, then
    /// every run re-executed with a container from the factory.
    fn reexecute(
        traj: &sharpebench_protocol::AgentTrajectory,
        sandbox: &FakeSandbox,
    ) -> (
        Result<sharpebench_harness::VerificationResult, sharpebench_harness::ReexecutionError>,
        Option<FailureKind>,
    ) {
        let (data, _) = crate::resolve_dataset(&[]).unwrap();
        let runner = crate::current_executable_sha256().unwrap();
        let fault: FaultCell = Rc::new(RefCell::new(None));
        let outcome = sharpebench_harness::verify_trajectory_reexecuted(
            &data,
            traj,
            sharpebench_sim::CostModel::default(),
            &sharpebench_core::ScoreConfig::default(),
            Some(&runner),
            sandbox_factory(IMAGE, sandbox, fault.clone()),
        );
        let fault = fault.borrow().clone();
        (outcome, fault)
    }

    fn verify_image(traj: &std::path::Path, sandbox: &FakeSandbox) -> ExitCode {
        let (data, _) = crate::resolve_dataset(&[]).unwrap();
        crate::run_reexecution(
            &args(&[
                "sharpebench",
                "verify-trajectory",
                traj.to_str().unwrap(),
                "--reexecute",
                "--image",
                IMAGE,
            ]),
            &data,
            &load(traj),
            sharpebench_sim::CostModel::default(),
            &sharpebench_core::ScoreConfig::default(),
            true,
            sandbox,
        )
    }

    fn assert_one_container_per_run(sandbox: &FakeSandbox, runs: usize) {
        assert_eq!(sandbox.launched.get(), runs, "one fresh container per run");
        assert_eq!(
            *sandbox.finished.borrow(),
            (0..runs).collect::<Vec<_>>(),
            "every container is finished once, in launch order, before the next starts"
        );
    }

    #[test]
    fn a_sandboxed_capture_names_its_image_and_reexecutes_in_fresh_containers() {
        let dir = scratch("roundtrip");
        let out = dir.join("traj.json");
        let captured = FakeSandbox::new(CLEAN);
        assert_eq!(capture(&out, &captured), ExitCode::SUCCESS);
        let traj = load(&out);
        assert_eq!(traj.agent_id, format!("sandbox:{IMAGE}"));
        assert_eq!(traj.runs.len(), 16);
        assert_one_container_per_run(&captured, 16);

        // The same image re-executes: every decision repeats.
        let steady = FakeSandbox::new(CLEAN);
        let (outcome, fault) = reexecute(&traj, &steady);
        assert!(outcome.is_ok(), "{outcome:?}");
        assert_eq!(fault, None);
        assert_one_container_per_run(&steady, 16);
        let steady = FakeSandbox::new(CLEAN);
        assert_eq!(verify_image(&out, &steady), ExitCode::SUCCESS);
        assert_one_container_per_run(&steady, 16);

        // A different policy behind the same reference diverges, typed, and
        // the container that diverged is still finished.
        let drifted = FakeSandbox::new(Script {
            momentum: true,
            ..CLEAN
        });
        let (outcome, fault) = reexecute(&traj, &drifted);
        assert!(
            matches!(
                outcome,
                Err(sharpebench_harness::ReexecutionError::Diverged(_))
            ),
            "{outcome:?}"
        );
        assert_eq!(fault, None, "a divergence is not a transport failure");
        assert_eq!(
            drifted.launched.get(),
            drifted.finished.borrow().len(),
            "no container outlives the refusal"
        );
        let drifted = FakeSandbox::new(Script {
            momentum: true,
            ..CLEAN
        });
        assert_eq!(verify_image(&out, &drifted), ExitCode::FAILURE);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_sandbox_failure_is_typed_and_stops_launching() {
        let dir = scratch("failures");
        let out = dir.join("traj.json");
        assert_eq!(capture(&out, &FakeSandbox::new(CLEAN)), ExitCode::SUCCESS);
        let traj = load(&out);
        let cases = [
            (
                Script {
                    oom: Some(3),
                    ..CLEAN
                },
                FailureKind::ResourceLimitExceeded,
                4,
            ),
            (
                Script {
                    finalize_error: Some(2),
                    ..CLEAN
                },
                FailureKind::TransportError,
                3,
            ),
            (
                Script {
                    transport_fault: Some(5),
                    ..CLEAN
                },
                FailureKind::TransportError,
                6,
            ),
            (
                Script {
                    refuse_launch: Some(0),
                    ..CLEAN
                },
                FailureKind::SpawnError,
                1,
            ),
        ];
        for (script, kind, launches) in cases {
            // Re-execution: the failure is reported, never compared, and no
            // container is started after it.
            let sandbox = FakeSandbox::new(script);
            let (_, fault) = reexecute(&traj, &sandbox);
            assert_eq!(fault, Some(kind.clone()));
            assert_eq!(sandbox.launched.get(), launches);
            let started = launches - usize::from(script.refuse_launch.is_some());
            assert_eq!(sandbox.finished.borrow().len(), started);
            assert_eq!(
                verify_image(&out, &FakeSandbox::new(script)),
                ExitCode::FAILURE
            );

            // Capture: the same failure writes no trajectory.
            let refused = dir.join("refused.json");
            let sandbox = FakeSandbox::new(script);
            assert_eq!(capture(&refused, &sandbox), ExitCode::FAILURE);
            assert!(!refused.exists(), "{kind:?} wrote a trajectory");
            assert_eq!(sandbox.launched.get(), launches);
        }
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn an_image_the_boundary_refuses_launches_nothing() {
        let dir = scratch("refused");
        let out = dir.join("traj.json");
        assert_eq!(capture(&out, &FakeSandbox::new(CLEAN)), ExitCode::SUCCESS);
        let sandbox = FakeSandbox::refusing("docker unavailable");
        assert_eq!(verify_image(&out, &sandbox), ExitCode::FAILURE);
        assert_eq!(sandbox.launched.get(), 0);
        let refused = dir.join("refused.json");
        let sandbox = FakeSandbox::refusing("docker unavailable");
        assert_eq!(capture(&refused, &sandbox), ExitCode::FAILURE);
        assert!(!refused.exists());
        assert_eq!(sandbox.launched.get(), 0);
        let _ = std::fs::remove_dir_all(dir);
    }
}
