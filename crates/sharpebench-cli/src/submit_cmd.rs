//! The submission path for an external agent.
//!
//! - `init-agent <dir> --kind cmd|http|image` writes a minimal, correct agent
//!   for one transport: the policy, a README, and a `sharpebench-agent.json`
//!   spec that says how to reach it.
//! - `check-agent <dir|spec.json>` (or `--cmd`, `--http`, `--image`) drives the
//!   entrant through a small synthetic window and reports, gate by gate, what
//!   the board would refuse and why: the launch, the transport, the wire
//!   contract, the per-decision time and memory budgets, deterministic
//!   re-execution, network reach, and the headline verdict with its reasons.
//!
//! The check runs the same transports, failure taxonomy and re-execution
//! verifier `run`, `capture` and `verify-trajectory --reexecute` use. It adds
//! no scoring rule of its own and changes no score.

use std::cell::Cell;
use std::path::Path;
use std::rc::Rc;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use sharpebench_core::headline::{
    headline, Evidence, Headline, HeadlineContext, HeadlineRow, Verdict,
};
use sharpebench_core::{rank, DisqualThresholds, FailReason, ScoreConfig};
use sharpebench_harness::{FailureKind, ReexecutionError};
use sharpebench_sim::external::CellIsolation;
use sharpebench_sim::transport::DecideError;
use sharpebench_sim::{
    Agent, BuyAndHold, Dataset, ExternalAgent, HoldAgent, HttpAgent, Momentum,
    TransportDiagnostics, Window,
};

use crate::external_capture::{sandbox_factory, FaultCell, SandboxLauncher};
use crate::{emit_json, flag_value};

/// The file `init-agent` writes and `check-agent <dir>` reads.
pub(crate) const SPEC_FILE: &str = "sharpebench-agent.json";
/// Schema of the agent spec.
pub(crate) const SPEC_SCHEMA: &str = "sharpebench.agent-spec.v1";
/// Schema of the `check-agent --json` report.
pub(crate) const REPORT_SCHEMA: &str = "sharpebench.check-agent.v1";
/// The address the HTTP template listens on by default.
const HTTP_TEMPLATE_ADDR: &str = "127.0.0.1:8765";

/// The synthetic check window: four symbols over 80 bars, two windows of 30
/// decisions each, two execution seeds. Small enough to run in seconds, long
/// enough that a ten-bar lookback has history before the first decision.
const CHECK_SYMBOLS: usize = 4;
const CHECK_BARS: usize = 80;
const CHECK_DATA_SEED: u64 = 20_261_102;
const CHECK_WINDOWS: [Window; 2] = [Window { start: 20, end: 50 }, Window { start: 50, end: 80 }];
const CHECK_SEEDS: [u64; 2] = [0, 1];
/// How long the HTTP launch probe waits for a connection.
const CONNECT_PROBE: Duration = Duration::from_secs(2);

/// The transport an agent speaks.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum Kind {
    Cmd,
    Http,
    Image,
}

impl Kind {
    fn parse(raw: &str) -> Option<Self> {
        match raw {
            "cmd" => Some(Self::Cmd),
            "http" => Some(Self::Http),
            "image" => Some(Self::Image),
            _ => None,
        }
    }
}

/// `sharpebench-agent.json`: which transport the agent speaks and how to reach
/// it. Exactly the field named by `kind` is set.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct AgentSpec {
    pub schema_version: String,
    pub kind: Kind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cmd: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub http: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image: Option<String>,
}

// --- init-agent ---------------------------------------------------------------

const AGENT_PY: &str = include_str!("../templates/agent/agent.py");
const SERVER_PY: &str = include_str!("../templates/agent/server.py");
const DOCKERFILE: &str = include_str!("../templates/agent/Dockerfile");
const README_CMD: &str = include_str!("../templates/agent/README-cmd.md");
const README_HTTP: &str = include_str!("../templates/agent/README-http.md");
const README_IMAGE: &str = include_str!("../templates/agent/README-image.md");

/// The interpreter name the templates call when `--python` is not given:
/// `python3` where that is the usual name, `python` on Windows, where
/// `python3` is often an installer stub.
fn default_python() -> &'static str {
    if cfg!(windows) {
        "python"
    } else {
        "python3"
    }
}

/// A Docker repository name derived from the directory name: lower case,
/// with every character Docker does not accept replaced by `-`.
fn image_name(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .map(|c| match c.to_ascii_lowercase() {
            c @ ('a'..='z' | '0'..='9' | '.' | '_' | '-') => c,
            _ => '-',
        })
        .collect();
    let trimmed = cleaned.trim_matches(|c: char| !c.is_ascii_alphanumeric());
    if trimmed.is_empty() {
        "sharpebench-agent".to_string()
    } else {
        trimmed.to_string()
    }
}

/// The files `init-agent` writes for `kind`, as (file name, contents).
pub(crate) fn template_files(kind: Kind, dir: &str, python: &str) -> Vec<(&'static str, String)> {
    let name = Path::new(dir)
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "agent".to_string());
    let image = image_name(&name);
    let fill = |template: &str| {
        template
            .replace("{{NAME}}", &name)
            .replace("{{DIR}}", dir)
            .replace("{{PYTHON}}", python)
            .replace("{{IMAGE}}", &image)
    };
    let spec = |cmd: Option<String>, http: Option<String>, image: Option<String>| AgentSpec {
        schema_version: SPEC_SCHEMA.to_string(),
        kind,
        cmd,
        http,
        image,
    };
    let (mut files, spec) = match kind {
        Kind::Cmd => (
            vec![
                ("agent.py", AGENT_PY.to_string()),
                ("README.md", fill(README_CMD)),
            ],
            spec(Some(format!("{python} agent.py")), None, None),
        ),
        Kind::Http => (
            vec![
                ("server.py", SERVER_PY.to_string()),
                ("README.md", fill(README_HTTP)),
            ],
            spec(None, Some(HTTP_TEMPLATE_ADDR.to_string()), None),
        ),
        Kind::Image => (
            vec![
                ("agent.py", AGENT_PY.to_string()),
                ("Dockerfile", DOCKERFILE.to_string()),
                ("README.md", fill(README_IMAGE)),
            ],
            spec(None, None, Some(image.clone())),
        ),
    };
    let mut spec_json = serde_json::to_string_pretty(&spec).expect("the agent spec serializes");
    spec_json.push('\n');
    files.push((SPEC_FILE, spec_json));
    files
}

/// `init-agent <dir> --kind cmd|http|image [--python <interpreter>]`.
pub(crate) fn run_init(args: &[String], json: bool) -> i32 {
    const USAGE: &str =
        "usage: sharpebench init-agent <dir> --kind cmd|http|image [--python <interpreter>] [--json]";
    if args.iter().any(|arg| arg == "--help" || arg == "-h") {
        println!("{USAGE}");
        return 0;
    }
    let Some(dir) = args.get(2).filter(|arg| !arg.starts_with('-')) else {
        eprintln!("{USAGE}");
        return 2;
    };
    let Some(kind) = flag_value(args, "--kind").and_then(Kind::parse) else {
        eprintln!("error: --kind must be cmd, http or image\n{USAGE}");
        return 2;
    };
    let python = flag_value(args, "--python").unwrap_or(default_python());
    if python.is_empty() || python.contains(char::is_whitespace) || python.starts_with('-') {
        eprintln!("error: --python takes one interpreter name or path without spaces, got `{python}`\n{USAGE}");
        return 2;
    }
    let files = template_files(kind, dir, python);
    let root = Path::new(dir);
    let existing: Vec<String> = files
        .iter()
        .map(|(name, _)| root.join(name))
        .filter(|path| path.exists())
        .map(|path| path.display().to_string())
        .collect();
    if !existing.is_empty() {
        eprintln!(
            "error: init-agent never overwrites a file, and these already exist: {}\nhint: pick a new directory, or move the files away first",
            existing.join(", ")
        );
        return 1;
    }
    if let Err(error) = std::fs::create_dir_all(root) {
        eprintln!("error: cannot create {dir}: {error}");
        return 1;
    }
    for (name, contents) in &files {
        let path = root.join(name);
        if let Err(error) = std::fs::write(&path, contents) {
            eprintln!("error: cannot write {}: {error}", path.display());
            return 1;
        }
    }
    let names: Vec<&str> = files.iter().map(|(name, _)| *name).collect();
    let check: Vec<String> = match kind {
        Kind::Image => vec![
            "check-agent".to_string(),
            "--image".to_string(),
            "<repository@sha256:...>".to_string(),
        ],
        _ => vec!["check-agent".to_string(), dir.to_string()],
    };
    if json {
        emit_json(&serde_json::json!({
            "created": true,
            "dir": dir,
            "kind": kind,
            "files": names,
            "check_with": check,
        }));
    } else {
        println!(
            "wrote a {} agent in {dir}: {}",
            kind_word(kind),
            names.join(", ")
        );
        match kind {
            Kind::Cmd => println!("next: sharpebench check-agent {dir}"),
            Kind::Http => {
                println!("next: start it with `{python} {dir}/server.py --port 8765`, then in another shell run");
                println!("      sharpebench check-agent {dir}");
            }
            Kind::Image => {
                println!("next: build and pin the image as {dir}/README.md shows, then run");
                println!("      sharpebench check-agent --image <repository@sha256:...>");
            }
        }
    }
    0
}

fn kind_word(kind: Kind) -> &'static str {
    match kind {
        Kind::Cmd => "stdio (--cmd)",
        Kind::Http => "HTTP (--http)",
        Kind::Image => "container image (--image)",
    }
}

// --- check-agent: resolving the entrant ------------------------------------------

/// The entrant a check drives.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Target {
    /// A host command: the program and its arguments.
    Cmd(Vec<String>),
    /// A `host:port` endpoint serving `POST /decide`.
    Http(String),
    /// A digest-pinned image reference.
    Image(String),
}

impl Target {
    /// The entrant id `run` gives the same entrant, runner tag included.
    pub(crate) fn agent_id(&self) -> String {
        match self {
            Self::Cmd(words) => format!("cmd:{}", words.join(" ")),
            Self::Http(addr) => format!("http:{addr}"),
            Self::Image(image) => format!("sandbox:{image}"),
        }
    }

    /// The `run` flag and value that field the same entrant.
    fn run_with(&self) -> (&'static str, String) {
        match self {
            Self::Cmd(words) => ("--cmd", words.join(" ")),
            Self::Http(addr) => ("--http", addr.clone()),
            Self::Image(image) => ("--image", image.clone()),
        }
    }

    fn isolation(&self) -> CellIsolation {
        match self {
            Self::Cmd(_) => ExternalAgent::CELL_ISOLATION,
            Self::Http(_) => HttpAgent::CELL_ISOLATION,
            Self::Image(_) => sharpebench_arena::SandboxedAgent::CELL_ISOLATION,
        }
    }
}

/// Read a spec from `path`, a directory holding [`SPEC_FILE`] or the file
/// itself, and turn it into a target. Words of a `cmd` that name a file in the
/// spec's directory are resolved against that directory, so the check runs
/// from any working directory.
pub(crate) fn target_from_spec(path: &Path) -> Result<Target, String> {
    let file = if path.is_dir() {
        path.join(SPEC_FILE)
    } else {
        path.to_path_buf()
    };
    let text = std::fs::read_to_string(&file)
        .map_err(|error| format!("cannot read {}: {error}", file.display()))?;
    let spec: AgentSpec = serde_json::from_str(&text)
        .map_err(|error| format!("{} is not an agent spec: {error}", file.display()))?;
    if spec.schema_version != SPEC_SCHEMA {
        return Err(format!(
            "{} declares schema {}, expected {SPEC_SCHEMA}",
            file.display(),
            spec.schema_version
        ));
    }
    let named = [
        (Kind::Cmd, spec.cmd.is_some()),
        (Kind::Http, spec.http.is_some()),
        (Kind::Image, spec.image.is_some()),
    ];
    if named.iter().any(|(kind, set)| *set != (*kind == spec.kind)) {
        return Err(format!(
            "{} must set exactly the field its kind names (`{}`)",
            file.display(),
            serde_json::to_value(spec.kind)
                .expect("a unit variant serializes")
                .as_str()
                .unwrap_or_default()
        ));
    }
    let base = file.parent().unwrap_or(Path::new("."));
    let target = match spec.kind {
        Kind::Cmd => Target::Cmd(
            spec.cmd
                .unwrap_or_default()
                .split_whitespace()
                .map(|word| resolve_word(base, word))
                .collect(),
        ),
        Kind::Http => Target::Http(spec.http.unwrap_or_default()),
        Kind::Image => Target::Image(spec.image.unwrap_or_default()),
    };
    match &target {
        Target::Cmd(words) if words.is_empty() => {
            Err(format!("{}: `cmd` names no program", file.display()))
        }
        Target::Http(addr) | Target::Image(addr) if addr.trim().is_empty() => Err(format!(
            "{}: the address or image reference is empty",
            file.display()
        )),
        _ => Ok(target),
    }
}

/// A command word that names a file beside the spec becomes that file's
/// absolute path; every other word is kept as written.
fn resolve_word(base: &Path, word: &str) -> String {
    let candidate = base.join(word);
    if Path::new(word).is_relative() && candidate.is_file() {
        std::path::absolute(&candidate)
            .unwrap_or(candidate)
            .display()
            .to_string()
    } else {
        word.to_string()
    }
}

/// The entrant named on the command line: a spec path, or exactly one of
/// `--cmd`, `--http` and `--image`.
fn target_from_args(args: &[String]) -> Result<Target, String> {
    let spec = args.get(2).filter(|arg| !arg.starts_with("--"));
    let flags: Vec<&str> = ["--cmd", "--http", "--image"]
        .into_iter()
        .filter(|flag| args.iter().any(|arg| arg == flag))
        .collect();
    match (spec, flags.as_slice()) {
        (Some(path), []) => target_from_spec(Path::new(path)),
        (None, [flag]) => {
            let value = flag_value(args, flag)
                .filter(|value| !value.starts_with("--") && !value.trim().is_empty())
                .ok_or_else(|| format!("{flag} needs a value"))?;
            Ok(match *flag {
                "--cmd" => Target::Cmd(value.split_whitespace().map(String::from).collect()),
                "--http" => Target::Http(value.to_string()),
                _ => Target::Image(value.to_string()),
            })
        }
        _ => Err("name one entrant: an agent directory or spec file, or exactly one of --cmd, --http or --image".to_string()),
    }
}

// --- check-agent: the gates ---------------------------------------------------------

/// The outcome of one gate.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Status {
    /// The gate held.
    Pass,
    /// The board would refuse or fail the entrant here.
    Fail,
    /// Reported beside the score, never a refusal.
    Advisory,
    /// An earlier failure stopped the check before this gate.
    NotRun,
}

impl Status {
    fn word(self) -> &'static str {
        match self {
            Self::Pass => "pass",
            Self::Fail => "FAIL",
            Self::Advisory => "advisory",
            Self::NotRun => "not run",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub(crate) struct Gate {
    pub gate: &'static str,
    pub status: Status,
    pub detail: String,
}

fn gate(gate: &'static str, status: Status, detail: impl Into<String>) -> Gate {
    Gate {
        gate,
        status,
        detail: detail.into(),
    }
}

/// Everything one check found.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub(crate) struct CheckReport {
    pub schema_version: &'static str,
    pub agent_id: String,
    pub ok: bool,
    pub check_window: serde_json::Value,
    pub gates: Vec<Gate>,
    pub headline: Option<Headline>,
    pub run_with: [String; 2],
}

/// What the probe wrappers record across every agent of one pass.
#[derive(Default)]
struct Watch {
    /// The first failure, in the harness's taxonomy.
    fault: FaultCell,
    /// The transport's own reading of that failure, when the transport is one
    /// this module wraps (not the sandbox).
    cause: Rc<Cell<Option<DecideError>>>,
    /// Decisions answered, and the slowest one.
    decisions: Rc<Cell<usize>>,
    slowest: Rc<Cell<Duration>>,
}

/// An external agent whose health is read after every decision. After the
/// first failure the check is already a refusal, so the agent is not asked
/// again and a silent entrant does not spend a decision budget per step.
struct Probe<A> {
    inner: A,
    fault: FaultCell,
    cause: Rc<Cell<Option<DecideError>>>,
}

impl<A: Agent + TransportDiagnostics> Agent for Probe<A> {
    fn decide(
        &mut self,
        observation: &sharpebench_protocol::MarketObservation,
    ) -> sharpebench_protocol::Decision {
        if self.fault.borrow().is_some() {
            return HoldAgent.decide(observation);
        }
        let decision = self.inner.decide(observation);
        if let Some(kind) = sharpebench_harness::transport_failure(self.inner.health()) {
            self.fault.borrow_mut().get_or_insert(kind);
            self.cause.set(self.inner.health().last_error);
        }
        decision
    }
}

/// Times every decision of the agent it wraps.
struct Timed {
    inner: Box<dyn Agent>,
    decisions: Rc<Cell<usize>>,
    slowest: Rc<Cell<Duration>>,
}

impl Agent for Timed {
    fn decide(
        &mut self,
        observation: &sharpebench_protocol::MarketObservation,
    ) -> sharpebench_protocol::Decision {
        let started = Instant::now();
        let decision = self.inner.decide(observation);
        self.slowest.set(self.slowest.get().max(started.elapsed()));
        self.decisions.set(self.decisions.get() + 1);
        decision
    }
}

/// A fresh agent per run, watched and timed. A failed spawn is recorded as a
/// [`FailureKind::SpawnError`] and the run is driven by an inert hold.
fn factory<'a>(
    target: &'a Target,
    launcher: &'a dyn SandboxLauncher,
    watch: &Watch,
) -> impl FnMut() -> Box<dyn Agent> + 'a {
    let fault = watch.fault.clone();
    let cause = watch.cause.clone();
    let decisions = watch.decisions.clone();
    let slowest = watch.slowest.clone();
    let mut sandbox = match target {
        Target::Image(image) => Some(sandbox_factory(image, launcher, fault.clone())),
        _ => None,
    };
    move || {
        let inner: Box<dyn Agent> = if fault.borrow().is_some() {
            Box::new(HoldAgent)
        } else {
            match target {
                Target::Cmd(words) => {
                    let rest: Vec<&str> = words[1..].iter().map(String::as_str).collect();
                    match ExternalAgent::spawn(&words[0], &rest) {
                        Ok(agent) => Box::new(Probe {
                            inner: agent,
                            fault: fault.clone(),
                            cause: cause.clone(),
                        }),
                        Err(_) => {
                            fault.borrow_mut().get_or_insert(FailureKind::SpawnError);
                            Box::new(HoldAgent)
                        }
                    }
                }
                Target::Http(addr) => Box::new(Probe {
                    inner: HttpAgent::new(addr.clone()),
                    fault: fault.clone(),
                    cause: cause.clone(),
                }),
                Target::Image(_) => (sandbox.as_mut().expect("an image target has a sandbox"))(),
            }
        };
        Box::new(Timed {
            inner,
            decisions: decisions.clone(),
            slowest: slowest.clone(),
        })
    }
}

/// Can the entrant be reached at all? `Err` carries the plain-language reason.
fn launch(target: &Target, launcher: &dyn SandboxLauncher) -> Result<String, String> {
    match target {
        Target::Cmd(words) => {
            let rest: Vec<&str> = words[1..].iter().map(String::as_str).collect();
            ExternalAgent::spawn(&words[0], &rest)
                .map(|_| format!("`{}` started as a host process", words[0]))
                .map_err(|error| {
                    format!(
                        "`{}` could not be started ({error}). Check that the program exists and is on PATH. The agent gets a cleared environment: PATH and platform essentials only, plus the names listed in SHARPEBENCH_AGENT_ENV",
                        words[0]
                    )
                })
        }
        Target::Http(addr) => {
            use std::net::{TcpStream, ToSocketAddrs};
            let addrs: Vec<_> = addr
                .to_socket_addrs()
                .map_err(|error| format!("`{addr}` is not a host:port address ({error})"))?
                .collect();
            let mut last = String::from("the name resolved to no address");
            for socket in addrs {
                match TcpStream::connect_timeout(&socket, CONNECT_PROBE) {
                    Ok(_) => return Ok(format!("{addr} accepted a connection")),
                    Err(error) => last = error.to_string(),
                }
            }
            Err(format!(
                "nothing accepted a connection at {addr} ({last}). The harness neither starts nor stops an HTTP endpoint: start it first"
            ))
        }
        Target::Image(image) => launcher
            .admit(image)
            .map(|()| format!("{image} is digest-pinned and present locally"))
            .map_err(|error| {
                format!(
                    "the sandbox refused {image}: {error}. The harness runs only a digest-pinned reference (repository@sha256:...) that is already present locally, and it never pulls"
                )
            }),
    }
}

/// The plain-language reading of a failure the capture recorded.
fn fault_detail(kind: FailureKind, cause: Option<DecideError>) -> (&'static str, String) {
    match kind {
        FailureKind::AgentProtocolViolation => (
            "protocol",
            "the agent sent a decision the wire contract refuses: not one JSON object per line, an unknown key, an order for a symbol the observation did not offer, a second order for one symbol, a target_weight outside [-1, 1] or a confidence outside [0, 1]. The line beginning `agent protocol fault:` above names the exact problem. On the board this run is a failing run and counts against pass^k".to_string(),
        ),
        FailureKind::DecisionBudgetExceeded => (
            "budget",
            format!(
                "the agent took the observation and did not answer within the {} s per-decision budget. On the board this run is a failing run and counts against pass^k",
                sharpebench_sim::external::STDIO_DECIDE_TIMEOUT.as_secs()
            ),
        ),
        FailureKind::ResourceLimitExceeded => (
            "budget",
            "the container exceeded its published memory limit and the kernel stopped it. On the board this run is a failing run and counts against pass^k".to_string(),
        ),
        FailureKind::SpawnError => (
            "transport",
            "a fresh agent could not be started for a later run. The harness starts a fresh agent for every window and seed".to_string(),
        ),
        FailureKind::TransportError | FailureKind::Timeout => (
            "transport",
            match cause {
                Some(DecideError::Exited(Some(code))) => format!(
                    "the agent process exited with status {code} before it answered. Run the command by hand and feed it one observation line to see its error output"
                ),
                Some(DecideError::Exited(None)) => {
                    "the agent process was stopped by a signal before it answered".to_string()
                }
                Some(DecideError::Timeout) => {
                    "the exchange did not finish within the per-decision budget, and the harness could not charge the delay to the agent (an observation that was never accepted, or a slow HTTP endpoint)".to_string()
                }
                _ => "the pipe or connection broke before the agent answered: the agent closed stdout, the endpoint reset the connection, or the response had no HTTP body".to_string(),
            } + ". The board retries such a run, and a sweep whose retries run out is not certifying",
        ),
    }
}

/// One plain sentence per hard or advisory reason.
pub(crate) fn explain(reason: FailReason) -> &'static str {
    match reason {
        FailReason::FailedPassK => "pass^k failed: at least one run did not clear the per-run PSR bar, so the edge did not hold on every window and seed",
        FailReason::DsrBelowBar => "the deflated Sharpe ratio is below its bar, so the edge does not survive the correction for the number of strategies tried",
        FailReason::ProcessViolation => "a block-severity process event was recorded, such as a bypassed risk gate",
        FailReason::BootstrapInsignificant => "the bootstrap test could not tell the returns from noise",
        FailReason::MandateBreached => "a run breached the drawdown mandate",
        FailReason::DeflationUnavailable => "the deflated Sharpe ratio or its bar could not be computed",
        FailReason::DeflationRefused => "the track has no Sharpe ratio: it is constant, flat in every window, or never loses",
        FailReason::DsrIntervalUnavailable => "the bootstrap interval of the deflated Sharpe ratio could not be estimated",
        FailReason::BootstrapUnavailable => "the bootstrap test could not be computed",
        FailReason::StandardErrorUncertified => "the deflated Sharpe ratio cleared its bar only with a standard error the certification refuses",
        FailReason::ExposurePathUnproven => "the profit does not clear the risk the exposure history implies",
        FailReason::ExposurePathUnavailable => "the exposure-path statistic could not be computed",
        FailReason::SelectionUnavailable => "advisory: the candidate-selection statistics could not be computed",
        FailReason::HighSelectionGap => "advisory: the best candidate is far ahead of the median candidate, which looks like a lucky pick",
        FailReason::IsRediscovery => "advisory: the returns nearly duplicate a known strategy",
        FailReason::OosDecay => "advisory: the edge decays out of sample",
    }
}

/// The evidence-mode disclosure `run` publishes for this transport, as the
/// headline's qualifier.
fn evidence_of(isolation: CellIsolation) -> (serde_json::Value, Evidence) {
    let mode = crate::cell_isolation::run_evidence_mode(isolation);
    let name = mode["mode"].as_str().unwrap_or_default().to_string();
    let evidence = if mode["certifying"] == true {
        Evidence::Certifying { mode: name }
    } else {
        Evidence::Noncertifying {
            mode: name,
            reasons: mode["noncertifying_reasons"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|reason| reason.as_str().map(String::from))
                .collect(),
        }
    };
    (mode, evidence)
}

/// Drive `target` through the synthetic check window and grade every gate.
pub(crate) fn check(
    target: &Target,
    launcher: &dyn SandboxLauncher,
    costs: sharpebench_sim::CostModel,
) -> CheckReport {
    let data = Dataset::synthetic(CHECK_SYMBOLS, CHECK_BARS, CHECK_DATA_SEED);
    let agent_id = target.agent_id();
    let cfg = ScoreConfig {
        execution_seeds_per_window: CHECK_SEEDS.len(),
        ..ScoreConfig::default()
    };
    let mut gates = Vec::new();
    let mut headline_row = None;

    let stopped = |gates: &mut Vec<Gate>, names: &[&'static str], why: &str| {
        for name in names {
            gates.push(gate(name, Status::NotRun, why));
        }
    };

    match launch(target, launcher) {
        Err(reason) => {
            gates.push(gate("launch", Status::Fail, reason));
            stopped(
                &mut gates,
                &["transport", "protocol", "budget", "determinism", "verdict"],
                "the entrant could not be launched",
            );
        }
        Ok(detail) => {
            gates.push(gate("launch", Status::Pass, detail));
            let watch = Watch::default();
            let (submission, trajectory) = sharpebench_harness::run_agent_capture(
                &agent_id,
                &data,
                &CHECK_WINDOWS,
                &CHECK_SEEDS,
                costs,
                factory(target, launcher, &watch),
            );
            let fault = watch.fault.borrow().clone();
            match fault {
                Some(kind) => {
                    let (failed, detail) = fault_detail(kind, watch.cause.get());
                    for name in ["transport", "protocol", "budget"] {
                        gates.push(if name == failed {
                            gate(name, Status::Fail, detail.clone())
                        } else {
                            gate(name, Status::NotRun, "the check stopped at the first fault")
                        });
                    }
                    stopped(
                        &mut gates,
                        &["determinism", "verdict"],
                        "the check stopped at the first fault",
                    );
                }
                None => {
                    gates.push(gate(
                        "transport",
                        Status::Pass,
                        format!(
                            "{} runs completed with a fresh agent each",
                            trajectory.runs.len()
                        ),
                    ));
                    gates.push(gate(
                        "protocol",
                        Status::Pass,
                        format!(
                            "{} decisions parsed and matched the observations they answered",
                            watch.decisions.get()
                        ),
                    ));
                    gates.push(gate(
                        "budget",
                        Status::Pass,
                        format!(
                            "the slowest decision took {} ms of the {} ms budget",
                            watch.slowest.get().as_millis(),
                            match target {
                                Target::Http(_) => sharpebench_sim::external::HTTP_DECIDE_TIMEOUT,
                                _ => sharpebench_sim::external::STDIO_DECIDE_TIMEOUT,
                            }
                            .as_millis()
                        ),
                    ));
                    gates.push(determinism(
                        target,
                        launcher,
                        &data,
                        &trajectory,
                        costs,
                        &cfg,
                    ));
                    let mut field = vec![
                        submission,
                        sharpebench_harness::run_agent(
                            "buy-and-hold",
                            &data,
                            &CHECK_WINDOWS,
                            &CHECK_SEEDS,
                            costs,
                            || Box::new(BuyAndHold) as Box<dyn Agent>,
                        ),
                        sharpebench_harness::run_agent(
                            "momentum",
                            &data,
                            &CHECK_WINDOWS,
                            &CHECK_SEEDS,
                            costs,
                            || Box::new(Momentum::default()) as Box<dyn Agent>,
                        ),
                    ];
                    field.extend(sharpebench_harness::luck_floor(
                        &data,
                        &CHECK_WINDOWS,
                        &CHECK_SEEDS,
                        costs,
                        crate::LUCK_FLOOR_AGENTS,
                    ));
                    let board = rank(&field, &cfg);
                    let row = board
                        .iter()
                        .find(|row| row.agent_id == agent_id)
                        .expect("the entrant is ranked into the check field");
                    let context = HeadlineContext {
                        evidence: evidence_of(target.isolation()).1,
                        ..HeadlineContext::default()
                    };
                    let h = headline(
                        HeadlineRow::Scored(row),
                        &DisqualThresholds::from_score_config(&cfg),
                        &context,
                    );
                    gates.push(verdict_gate(&h));
                    headline_row = Some(h);
                }
            }
        }
    }
    gates.push(network_gate(target));
    let (flag, value) = target.run_with();
    CheckReport {
        schema_version: REPORT_SCHEMA,
        agent_id,
        ok: gates.iter().all(|g| g.status != Status::Fail),
        check_window: serde_json::json!({
            "dataset": "synthetic",
            "symbols": CHECK_SYMBOLS,
            "bars": CHECK_BARS,
            "windows": CHECK_WINDOWS.iter().map(|w| sharpebench_harness::window_label(*w)).collect::<Vec<_>>(),
            "seeds": CHECK_SEEDS,
        }),
        gates,
        headline: headline_row,
        run_with: [flag.to_string(), value],
    }
}

/// Re-execute every captured run with a fresh agent and compare each
/// score-bearing decision, as `verify-trajectory --reexecute` does.
fn determinism(
    target: &Target,
    launcher: &dyn SandboxLauncher,
    data: &Dataset,
    trajectory: &sharpebench_protocol::AgentTrajectory,
    costs: sharpebench_sim::CostModel,
    cfg: &ScoreConfig,
) -> Gate {
    let watch = Watch::default();
    let outcome = sharpebench_harness::verify_trajectory_reexecuted(
        data,
        trajectory,
        costs,
        cfg,
        None,
        factory(target, launcher, &watch),
    );
    let fault = watch.fault.borrow().clone();
    match (fault, outcome) {
        (Some(kind), _) => gate(
            "determinism",
            Status::Fail,
            format!(
                "re-execution hit a fault, so the decisions could not be compared: {}",
                fault_detail(kind, watch.cause.get()).1
            ),
        ),
        (None, Ok(_)) => gate(
            "determinism",
            Status::Pass,
            format!(
                "re-executing all {} runs with fresh agents reproduced every one of {} decisions",
                trajectory.runs.len(),
                watch.decisions.get()
            ),
        ),
        (None, Err(ReexecutionError::Diverged(d))) => gate(
            "determinism",
            Status::Fail,
            format!(
                "run {} step {} (observation {}) answered differently when re-executed: recorded {}, re-executed {}. A decision may depend only on the run's observations and the agent's own earlier decisions in that run: no clock, no ambient randomness, no state kept between runs. `verify-trajectory --reexecute` refuses such a trajectory",
                d.run, d.step, d.observation_id, d.recorded, d.reexecuted
            ),
        ),
        (None, Err(ReexecutionError::Refused(reason))) => gate(
            "determinism",
            Status::Fail,
            format!("the strict trajectory check refused the capture: {reason}"),
        ),
    }
}

/// The headline verdict and its reasons. Never a failure here: on a synthetic
/// window this short almost any agent gets a verdict other than
/// `skill_supported`, so the gate shows which reasons the scorer gives.
fn verdict_gate(h: &Headline) -> Gate {
    let mut detail = format!("headline verdict {}", h.verdict.as_str());
    let reasons: Vec<&str> = h
        .hard_reasons
        .iter()
        .chain(&h.qualifiers.advisory)
        .map(|reason| explain(*reason))
        .collect();
    if !reasons.is_empty() {
        detail.push_str(": ");
        detail.push_str(&reasons.join("; "));
    }
    if let Some(cause) = &h.cause {
        detail.push_str(&format!(" ({cause})"));
    }
    detail.push_str(". This short synthetic window is not evidence either way; the verdict shows which reasons the scorer gives");
    let status = if h.verdict == Verdict::SkillSupported {
        Status::Pass
    } else {
        Status::Advisory
    };
    gate("verdict", status, detail)
}

/// Whether a row from this transport can certify on a historical window.
fn network_gate(target: &Target) -> Gate {
    let isolation = target.isolation();
    let (mode, _) = evidence_of(isolation);
    if mode["certifying"] == true {
        gate(
            "network",
            Status::Pass,
            "the container runs with --network none, so its row on a historical window can certify",
        )
    } else {
        gate(
            "network",
            Status::Advisory,
            format!(
                "under {} the agent can reach the network, so its row on a historical window is published noncertifying (evidence mode {}). Score and rank are unchanged; run it with --image to certify",
                target.run_with().0,
                mode["mode"].as_str().unwrap_or_default()
            ),
        )
    }
}

/// `check-agent <dir|spec.json> | --cmd "<prog>" | --http <addr> | --image <ref>`.
pub(crate) fn run_check(args: &[String], json: bool, launcher: &dyn SandboxLauncher) -> i32 {
    const USAGE: &str = "usage: sharpebench check-agent (<agent-dir> | <spec.json> | --cmd \"<prog>\" | --http <addr> | --image <repository@sha256:...>) [--short-borrow-bps <bps>] [--json]";
    if args.iter().any(|arg| arg == "--help" || arg == "-h") {
        println!("{USAGE}");
        return 0;
    }
    let target = match target_from_args(args) {
        Ok(target) => target,
        Err(error) => {
            eprintln!("error: {error}\n{USAGE}");
            return 2;
        }
    };
    let costs = match crate::cost_model_from_args(args) {
        Ok(costs) => costs,
        Err(error) => {
            eprintln!("error: {error}");
            return 2;
        }
    };
    if let Target::Cmd(words) = &target {
        eprintln!(
            "warning: check-agent --cmd runs `{}` directly on this host with NO sandbox, once per run. Only point it at an agent you trust",
            words[0]
        );
    }
    let report = check(&target, launcher, costs);
    if json {
        emit_json(&report);
    } else {
        print_report(&report);
    }
    if report.ok {
        0
    } else {
        1
    }
}

fn print_report(report: &CheckReport) {
    let windows: Vec<String> = CHECK_WINDOWS
        .iter()
        .map(|w| sharpebench_harness::window_label(*w))
        .collect();
    let seeds: Vec<String> = CHECK_SEEDS.iter().map(u64::to_string).collect();
    println!("check-agent: {}", report.agent_id);
    println!(
        "synthetic check window: {CHECK_SYMBOLS} symbols, windows {}, seeds {}\n",
        windows.join(" and "),
        seeds.join(" and ")
    );
    for g in &report.gates {
        println!("  {:<9} {:<12} {}", g.status.word(), g.gate, g.detail);
    }
    println!();
    let failed: Vec<&str> = report
        .gates
        .iter()
        .filter(|g| g.status == Status::Fail)
        .map(|g| g.gate)
        .collect();
    if failed.is_empty() {
        println!("no gate failed. Field it with:");
        println!(
            "  sharpebench run {} \"{}\"",
            report.run_with[0], report.run_with[1]
        );
    } else {
        println!(
            "failed: {}. The book page \"Submit your agent\" explains each gate.",
            failed.join(", ")
        );
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use sharpebench_protocol::{Decision, MarketObservation};
    use sharpebench_sim::TransportHealth;

    use super::*;
    use crate::external_capture::SandboxEntrant;

    const IMAGE: &str =
        "example/entrant@sha256:1111111111111111111111111111111111111111111111111111111111111111";

    /// What every fake container of one check does.
    #[derive(Clone, Copy, PartialEq)]
    enum Behaviour {
        Clean,
        /// Reports a protocol fault on its first decision.
        Protocol,
        /// Reports a budget overrun on its first decision.
        Overrun,
        /// Finishes with an out-of-memory verdict.
        Oom,
        /// The fifth container onwards answers differently: the second pass
        /// (the re-execution) disagrees with the first.
        Drifts,
    }

    struct FakeSandbox {
        behaviour: Behaviour,
        admit: Result<(), String>,
        launched: Rc<Cell<usize>>,
    }

    impl FakeSandbox {
        fn new(behaviour: Behaviour) -> Self {
            Self {
                behaviour,
                admit: Ok(()),
                launched: Rc::new(Cell::new(0)),
            }
        }
    }

    struct FakeContainer {
        behaviour: Behaviour,
        index: usize,
        health: TransportHealth,
    }

    impl Agent for FakeContainer {
        fn decide(&mut self, observation: &MarketObservation) -> Decision {
            match self.behaviour {
                Behaviour::Protocol => self.health.protocol_faults = 1,
                Behaviour::Overrun => self.health.budget_overruns = 1,
                _ => {}
            }
            let mut decision = BuyAndHold.decide(observation);
            if self.behaviour == Behaviour::Drifts && self.index >= CHECK_WINDOWS.len() * 2 {
                decision.orders.truncate(1);
            }
            decision
        }
    }

    impl TransportDiagnostics for FakeContainer {
        fn health(&self) -> &TransportHealth {
            &self.health
        }
    }

    /// `check-agent` asks no identity question, so the side channel the
    /// identifiability probe uses is never opened here.
    impl sharpebench_sim::LineExchange for FakeContainer {
        fn exchange(&mut self, _request: &str) -> Result<String, DecideError> {
            Err(DecideError::Protocol)
        }
    }

    impl SandboxEntrant for FakeContainer {
        fn finish(self: Box<Self>) -> Result<Option<bool>, String> {
            Ok(Some(self.behaviour == Behaviour::Oom))
        }
    }

    impl SandboxLauncher for FakeSandbox {
        fn admit(&self, image: &str) -> Result<(), String> {
            assert_eq!(image, IMAGE);
            self.admit.clone()
        }

        fn launch(&self, image: &str) -> Result<Box<dyn SandboxEntrant>, String> {
            assert_eq!(image, IMAGE);
            let index = self.launched.get();
            self.launched.set(index + 1);
            Ok(Box::new(FakeContainer {
                behaviour: self.behaviour,
                index,
                health: TransportHealth::default(),
            }))
        }
    }

    fn run_check_on(sandbox: &FakeSandbox) -> CheckReport {
        check(
            &Target::Image(IMAGE.to_string()),
            sandbox,
            sharpebench_sim::CostModel::default(),
        )
    }

    fn status_of(report: &CheckReport, name: &str) -> (Status, String) {
        let g = report
            .gates
            .iter()
            .find(|g| g.gate == name)
            .unwrap_or_else(|| panic!("no {name} gate in {:?}", report.gates));
        (g.status, g.detail.clone())
    }

    fn statuses(report: &CheckReport) -> Vec<(&'static str, Status)> {
        report.gates.iter().map(|g| (g.gate, g.status)).collect()
    }

    #[test]
    fn a_clean_container_passes_every_gate_and_certifies() {
        let sandbox = FakeSandbox::new(Behaviour::Clean);
        let report = run_check_on(&sandbox);
        assert!(report.ok, "{:?}", report.gates);
        assert_eq!(
            statuses(&report),
            vec![
                ("launch", Status::Pass),
                ("transport", Status::Pass),
                ("protocol", Status::Pass),
                ("budget", Status::Pass),
                ("determinism", Status::Pass),
                ("verdict", Status::Advisory),
                ("network", Status::Pass),
            ]
        );
        // Four captured runs and four re-executed ones, one container each.
        assert_eq!(sandbox.launched.get(), 8);
        assert!(status_of(&report, "protocol")
            .1
            .starts_with("120 decisions"));
        assert!(status_of(&report, "determinism")
            .1
            .contains("all 4 runs with fresh agents reproduced every one of 120 decisions"));
        let headline = report
            .headline
            .as_ref()
            .expect("a clean check has a headline");
        assert_eq!(headline.agent_id, format!("sandbox:{IMAGE}"));
        assert_eq!(
            headline.qualifiers.evidence,
            Evidence::Certifying {
                mode: "backtest".to_string()
            }
        );
        assert_eq!(report.run_with, ["--image".to_string(), IMAGE.to_string()]);
        assert_eq!(
            report.check_window["windows"],
            serde_json::json!(["20-50", "50-80"])
        );
    }

    #[test]
    fn each_entrant_fault_fails_its_own_gate_and_stops_the_check() {
        for (behaviour, failed, says) in [
            (Behaviour::Protocol, "protocol", "agent protocol fault:"),
            (Behaviour::Overrun, "budget", "per-decision budget"),
            (Behaviour::Oom, "budget", "memory limit"),
        ] {
            let report = run_check_on(&FakeSandbox::new(behaviour));
            assert!(!report.ok);
            let (status, detail) = status_of(&report, failed);
            assert_eq!(status, Status::Fail, "{failed}");
            assert!(detail.contains(says), "{detail}");
            assert!(detail.contains("counts against pass^k"), "{detail}");
            for other in ["transport", "protocol", "budget", "determinism", "verdict"] {
                if other != failed {
                    assert_eq!(status_of(&report, other).0, Status::NotRun, "{other}");
                }
            }
            assert!(report.headline.is_none());
        }
    }

    #[test]
    fn a_container_that_answers_differently_on_reexecution_fails_determinism() {
        let report = run_check_on(&FakeSandbox::new(Behaviour::Drifts));
        assert!(!report.ok);
        let (status, detail) = status_of(&report, "determinism");
        assert_eq!(status, Status::Fail);
        assert!(detail.starts_with("run 0 step 0 (observation "), "{detail}");
        assert!(
            detail.contains("no clock, no ambient randomness"),
            "{detail}"
        );
        // The capture itself was clean, so the verdict is still reported.
        assert!(report.headline.is_some());
    }

    #[test]
    fn a_refused_image_fails_launch_and_runs_nothing() {
        let sandbox = FakeSandbox {
            admit: Err("the image is not present locally".to_string()),
            ..FakeSandbox::new(Behaviour::Clean)
        };
        let report = run_check_on(&sandbox);
        assert_eq!(sandbox.launched.get(), 0);
        let (status, detail) = status_of(&report, "launch");
        assert_eq!(status, Status::Fail);
        assert!(detail.contains("not present locally"), "{detail}");
        assert!(detail.contains("never pulls"), "{detail}");
        assert_eq!(status_of(&report, "determinism").0, Status::NotRun);
        // The network disclosure needs no run.
        assert_eq!(status_of(&report, "network").0, Status::Pass);
    }

    #[test]
    fn host_transports_are_advisory_noncertifying_on_the_network_gate() {
        for (target, flag) in [
            (Target::Cmd(vec!["agent".to_string()]), "--cmd"),
            (Target::Http("127.0.0.1:9".to_string()), "--http"),
        ] {
            let g = network_gate(&target);
            assert_eq!(g.status, Status::Advisory);
            assert!(g.detail.contains(flag), "{}", g.detail);
            assert!(g.detail.contains("backtest_unobserved"), "{}", g.detail);
            assert!(matches!(
                evidence_of(target.isolation()).1,
                Evidence::Noncertifying { reasons, .. } if reasons == ["historical window with a network-reachable entrant"]
            ));
        }
    }

    #[test]
    fn an_endpoint_nobody_serves_fails_launch() {
        // Bind and drop a listener so the port is known to be closed.
        let port = std::net::TcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap()
            .port();
        let report = check(
            &Target::Http(format!("127.0.0.1:{port}")),
            &FakeSandbox::new(Behaviour::Clean),
            sharpebench_sim::CostModel::default(),
        );
        let (status, detail) = status_of(&report, "launch");
        assert_eq!(status, Status::Fail);
        assert!(detail.contains("start it first"), "{detail}");
        assert!(launch(
            &Target::Http("no port".to_string()),
            &FakeSandbox::new(Behaviour::Clean)
        )
        .unwrap_err()
        .contains("not a host:port address"));
    }

    #[test]
    fn a_missing_program_fails_launch_with_the_environment_hint() {
        let detail = launch(
            &Target::Cmd(vec!["sharpebench-no-such-program-4f1".to_string()]),
            &FakeSandbox::new(Behaviour::Clean),
        )
        .unwrap_err();
        assert!(detail.contains("could not be started"), "{detail}");
        assert!(detail.contains("SHARPEBENCH_AGENT_ENV"), "{detail}");
    }

    #[test]
    fn transport_faults_name_their_cause() {
        let cases = [
            (Some(DecideError::Exited(Some(3))), "exited with status 3"),
            (Some(DecideError::Exited(None)), "stopped by a signal"),
            (Some(DecideError::Timeout), "could not charge the delay"),
            (Some(DecideError::Transport), "pipe or connection broke"),
            (None, "pipe or connection broke"),
        ];
        for (cause, says) in cases {
            for kind in [FailureKind::TransportError, FailureKind::Timeout] {
                let (gate, detail) = fault_detail(kind, cause);
                assert_eq!(gate, "transport");
                assert!(detail.contains(says), "{detail}");
                assert!(detail.ends_with("is not certifying"), "{detail}");
            }
        }
        let (gate, detail) = fault_detail(FailureKind::SpawnError, None);
        assert_eq!(gate, "transport");
        assert!(detail.contains("could not be started for a later run"));
    }

    #[test]
    fn every_reason_has_its_own_explanation() {
        use FailReason::*;
        let all = [
            FailedPassK,
            DsrBelowBar,
            ProcessViolation,
            BootstrapInsignificant,
            MandateBreached,
            DeflationUnavailable,
            DeflationRefused,
            DsrIntervalUnavailable,
            BootstrapUnavailable,
            StandardErrorUncertified,
            ExposurePathUnproven,
            ExposurePathUnavailable,
            SelectionUnavailable,
            HighSelectionGap,
            IsRediscovery,
            OosDecay,
        ];
        let texts: std::collections::BTreeSet<&str> = all.iter().map(|r| explain(*r)).collect();
        assert_eq!(texts.len(), all.len());
        for reason in all {
            assert_eq!(
                explain(reason).starts_with("advisory:"),
                reason.is_advisory(),
                "{reason:?}"
            );
        }
    }

    #[test]
    fn the_verdict_gate_passes_only_a_skill_supported_row() {
        let base = || Headline {
            schema: sharpebench_core::headline::SCHEMA_VERSION.to_string(),
            agent_id: "a".to_string(),
            verdict: Verdict::SkillSupported,
            hard_reasons: Vec::new(),
            cause: None,
            qualifiers: sharpebench_core::headline::Qualifiers {
                tier: sharpebench_core::EvaluationTier::HistoricalAdvisory,
                evidence: Evidence::Unknown,
                tied: false,
                tie_group: 1,
                stress: sharpebench_core::headline::StressMark::NotRun,
                advisory: Vec::new(),
                declared_mandate: None,
                rank_mode: None,
            },
        };
        let pass = verdict_gate(&base());
        assert_eq!(pass.status, Status::Pass);
        assert!(pass
            .detail
            .starts_with("headline verdict skill_supported. This"));
        let mut refused = base();
        refused.verdict = Verdict::DegenerateTrack;
        refused.hard_reasons = vec![FailReason::DeflationRefused];
        refused.qualifiers.advisory = vec![FailReason::OosDecay];
        refused.cause = Some("constant track".to_string());
        let g = verdict_gate(&refused);
        assert_eq!(g.status, Status::Advisory);
        assert!(
            g.detail.starts_with(&format!(
                "headline verdict degenerate_track: {}; {} (constant track). This",
                explain(FailReason::DeflationRefused),
                explain(FailReason::OosDecay)
            )),
            "{}",
            g.detail
        );
    }

    fn scratch(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("sharpebench-submit-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|arg| arg.to_string()).collect()
    }

    #[test]
    fn every_template_spec_reads_back_as_its_transport() {
        let root = scratch("specs");
        for kind in [Kind::Cmd, Kind::Http, Kind::Image] {
            let dir = root.join(format!("{kind:?}"));
            let dir_text = dir.to_str().unwrap();
            assert_eq!(
                run_init(
                    &args(&[
                        "sb",
                        "init-agent",
                        dir_text,
                        "--kind",
                        &format!("{kind:?}").to_lowercase(),
                        "--python",
                        "py"
                    ]),
                    true
                ),
                0
            );
            let target = target_from_spec(&dir).unwrap();
            match (kind, target) {
                (Kind::Cmd, Target::Cmd(words)) => {
                    assert_eq!(words[0], "py");
                    // The script word is resolved against the agent directory.
                    assert_eq!(
                        Path::new(&words[1]),
                        std::path::absolute(dir.join("agent.py")).unwrap()
                    );
                }
                (Kind::Http, Target::Http(addr)) => assert_eq!(addr, HTTP_TEMPLATE_ADDR),
                (Kind::Image, Target::Image(image)) => assert_eq!(image, "image"),
                other => panic!("{other:?}"),
            }
            // A second init into the same directory refuses and changes nothing.
            let before = std::fs::read(dir.join("README.md")).unwrap();
            assert_eq!(
                run_init(
                    &args(&["sb", "init-agent", dir_text, "--kind", "cmd"]),
                    true
                ),
                1
            );
            assert_eq!(std::fs::read(dir.join("README.md")).unwrap(), before);
        }
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn templates_fill_every_placeholder() {
        for kind in [Kind::Cmd, Kind::Http, Kind::Image] {
            for (name, text) in template_files(kind, "agents/My Agent", "py3") {
                assert!(
                    !text.contains("{{NAME}}") && !text.contains("{{DIR}}"),
                    "{name}"
                );
                assert!(
                    !text.contains("{{PYTHON}}") && !text.contains("{{IMAGE}}"),
                    "{name}"
                );
                assert!(!text.contains('\u{2014}'), "{name} has an em dash");
                if name == "README.md" {
                    assert!(text.starts_with("# My Agent\n"), "{name}");
                    assert!(text.contains("agents/My Agent"), "{name}");
                }
            }
        }
        let image_readme = template_files(Kind::Image, "agents/My Agent", "py3")
            .into_iter()
            .find(|(name, _)| *name == "README.md")
            .unwrap()
            .1;
        assert!(image_readme.contains("docker build -t my-agent agents/My Agent"));
        assert_eq!(image_name("--Odd Name!"), "odd-name");
        assert_eq!(image_name("???"), "sharpebench-agent");
    }

    #[test]
    fn malformed_specs_and_command_lines_are_refused() {
        let root = scratch("bad-specs");
        let write = |name: &str, body: &str| {
            let path = root.join(name);
            std::fs::write(&path, body).unwrap();
            path
        };
        for (name, body, says) in [
            (
                "schema.json",
                r#"{"schema_version":"v0","kind":"cmd","cmd":"x"}"#,
                "expected sharpebench.agent-spec.v1",
            ),
            (
                "mixed.json",
                r#"{"schema_version":"sharpebench.agent-spec.v1","kind":"cmd","cmd":"x","http":"h:1"}"#,
                "exactly the field its kind names (`cmd`)",
            ),
            (
                "missing.json",
                r#"{"schema_version":"sharpebench.agent-spec.v1","kind":"http"}"#,
                "exactly the field its kind names (`http`)",
            ),
            (
                "blank.json",
                r#"{"schema_version":"sharpebench.agent-spec.v1","kind":"cmd","cmd":"  "}"#,
                "names no program",
            ),
            (
                "empty.json",
                r#"{"schema_version":"sharpebench.agent-spec.v1","kind":"image","image":" "}"#,
                "is empty",
            ),
            (
                "extra.json",
                r#"{"schema_version":"sharpebench.agent-spec.v1","kind":"http","http":"h:1","note":1}"#,
                "is not an agent spec",
            ),
        ] {
            let error = target_from_spec(&write(name, body)).unwrap_err();
            assert!(error.contains(says), "{name}: {error}");
        }
        assert!(target_from_spec(&root.join("absent.json"))
            .unwrap_err()
            .contains("cannot read"));
        for argv in [
            vec!["sb", "check-agent"],
            vec!["sb", "check-agent", "dir", "--http", "h:1"],
            vec!["sb", "check-agent", "--cmd", "a", "--http", "h:1"],
            vec!["sb", "check-agent", "--cmd", "--json"],
        ] {
            assert!(target_from_args(&args(&argv)).is_err(), "{argv:?}");
        }
        assert_eq!(
            target_from_args(&args(&["sb", "check-agent", "--cmd", "py  agent.py"])).unwrap(),
            Target::Cmd(vec!["py".to_string(), "agent.py".to_string()])
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn init_agent_refuses_bad_arguments_before_writing() {
        let root = scratch("init-args");
        let dir = root.join("agent");
        let dir_text = dir.to_str().unwrap();
        for argv in [
            vec!["sb", "init-agent"],
            vec!["sb", "init-agent", dir_text],
            vec!["sb", "init-agent", dir_text, "--kind", "wasm"],
            vec![
                "sb",
                "init-agent",
                dir_text,
                "--kind",
                "cmd",
                "--python",
                "my python",
            ],
        ] {
            assert_eq!(run_init(&args(&argv), true), 2, "{argv:?}");
        }
        assert!(!dir.exists());
        let _ = std::fs::remove_dir_all(&root);
    }
}
