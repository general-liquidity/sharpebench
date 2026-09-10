//! The entrant-serving loop: the runtime that attaches the model gateway to a
//! real sweep.
//!
//! [`ModelGateway`] turns one request line into one response line and nothing
//! more. This module is what feeds it. It drives an entrant process over the
//! stdio pipe the host already owns, hands every gateway request the entrant
//! writes to the broker, writes the broker's answer back, and scores the
//! decision that follows exactly as the external-agent transport would. A sweep
//! built on it binds the money journal into the checkpoint identity and
//! publishes the host-observed usage beside the scored pool, never inside it.
//!
//! # Wire
//!
//! After the host writes one observation line, the entrant may write any number
//! of gateway request lines before its decision line, and each request is
//! answered by exactly one response line on the entrant's stdin. A stdout line
//! is a gateway request when it is a JSON object whose top-level `protocol`
//! member is a string in the [`GATEWAY_PROTOCOL_FAMILY`]; every other line is a
//! decision under the closed decision contract. That contract refuses unknown
//! fields, so no line can be both. A request that names a protocol version this
//! host does not speak is still routed to the broker, which refuses it by type,
//! rather than being misread as a malformed decision.
//!
//! There is no listener, port, URL or TLS on the entrant side. An entrant
//! container launched with `--network none` stays that way: its model calls
//! leave through the same pipe as its decisions, and the host makes them.
//!
//! # What the entrant never sees
//!
//! Routing and credentials are host state (see [`super`]). This module adds
//! the process boundary: [`EntrantLaunch::spawn`] refuses any launch whose
//! program, arguments or environment would carry a route credential, and a
//! launcher that inherits the host environment inherits it with every
//! credential-bearing variable removed.
//!
//! # Whose clock
//!
//! An entrant has a wall-clock budget per decision. Time the host spends
//! serving a gateway request is not the entrant's: the decision deadline moves
//! out by exactly that time, so a slow provider does not turn into an entrant
//! timeout. What bounds the host's share instead is the per-decision request
//! ceiling ([`GatewayLimits::max_requests_per_decision`]), the per-call
//! deadline handed to the adapter, and the sweep budget.

use std::borrow::Cow;
use std::io::{BufRead, BufReader, Read, Write};
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, SyncSender};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use sharpebench_protocol::{Decision, MarketObservation};
use sharpebench_sim::{
    Agent, CircuitBreaker, CostModel, Dataset, DecideError, TransportDiagnostics, TransportHealth,
    Window,
};

use super::{
    encode_refusal, CallPermits, GatewayErrorKind, GatewayLimits, GatewayShutdown, ModelGateway,
    ProviderTransport, RouteTable,
};
use crate::accounting::{MonetarySummary, RateCard};
use crate::gateway_journal::{GatewayBudget, GatewayJournal, JournalIdentity, JournalSaveError};
use crate::{AttemptObservation, ResilientSubmission, ResumePolicy, SweepContract, SweepIdentity};

/// The protocol family a stdout line must name to be read as a gateway request.
pub const GATEWAY_PROTOCOL_FAMILY: &str = "sharpebench.model-gateway.";

/// Framing for the gateway binding folded into a sweep's invocation identity.
pub const GATEWAY_INVOCATION_VERSION: &str = "sharpebench.gateway-invocation.v1";

/// Framing for the digest that binds a money journal to one sweep.
pub const GATEWAY_SWEEP_VERSION: &str = "sharpebench.gateway-sweep.v1";

/// Schema of the usage record published beside the scored pool.
pub const HOST_USAGE_VERSION: &str = "sharpebench.host-observed-usage.v1";

/// Bytes accepted for one entrant stdout line, decision or request. The same
/// cap the external-agent transport puts on a decision line; a request line is
/// then held to the broker's much smaller bound before it is parsed.
const MAX_ENTRANT_LINE: u64 = 8 * 1024 * 1024;

/// Total accepted stdout per entrant process, requests included.
const MAX_ENTRANT_STDOUT: u64 = 64 * 1024 * 1024;

/// The entrant's per-decision wall-clock budget, net of host serving time.
const DEFAULT_DECIDE_TIMEOUT: Duration = Duration::from_secs(30);

const BREAKER_THRESHOLD: u32 = 3;
const DEAD_CHILD_POLL: Duration = Duration::from_millis(25);
const EXIT_DRAIN_GRACE: Duration = Duration::from_millis(500);
#[cfg(unix)]
const TEARDOWN_GRACE: Duration = Duration::from_millis(500);

/// The lifecycle of whatever sits at the other end of the pipes.
pub trait EntrantProcess: Send {
    /// `Some(code)` once the entrant has exited, with its exit code when the
    /// platform reports one. `None` while it runs.
    fn exited(&mut self) -> Option<Option<i32>>;
    /// End the entrant. Called on a timeout and on drop.
    fn terminate(&mut self);
}

/// The two pipe ends the host owns and the process behind them.
pub struct EntrantPipes {
    stdin: Box<dyn Write + Send>,
    stdout: Box<dyn Read + Send>,
    process: Box<dyn EntrantProcess>,
}

impl EntrantPipes {
    /// Pipes to an entrant the caller started some other way, such as an
    /// in-process driver or a launcher this module does not know about.
    pub fn new(
        stdin: Box<dyn Write + Send>,
        stdout: Box<dyn Read + Send>,
        process: Box<dyn EntrantProcess>,
    ) -> Self {
        Self {
            stdin,
            stdout,
            process,
        }
    }
}

/// What environment a spawned process receives.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LaunchEnvironment {
    /// The process is the entrant: the environment is cleared and only these
    /// variables are set.
    Cleared(Vec<(String, String)>),
    /// The process is a launcher that isolates the entrant itself, such as the
    /// Docker client starting a `--network none` container. It needs the host's
    /// configuration (`DOCKER_HOST` and friends), so it inherits the host
    /// environment, minus every variable whose value carries a route credential.
    InheritWithoutCredentials,
}

/// How to start one entrant process.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EntrantLaunch {
    pub program: String,
    pub args: Vec<String>,
    pub environment: LaunchEnvironment,
}

impl EntrantLaunch {
    /// A host-executed entrant with a cleared environment.
    pub fn host(program: impl Into<String>, args: Vec<String>, env: Vec<(String, String)>) -> Self {
        Self {
            program: program.into(),
            args,
            environment: LaunchEnvironment::Cleared(env),
        }
    }

    /// A launcher that isolates the entrant, such as the hardened `docker run`
    /// argv from `sharpebench_arena::sandbox::gateway_launch`.
    pub fn isolating_launcher(program: impl Into<String>, args: Vec<String>) -> Self {
        Self {
            program: program.into(),
            args,
            environment: LaunchEnvironment::InheritWithoutCredentials,
        }
    }

    /// Refuse, before anything is spawned, a launch whose program, arguments or
    /// declared environment would carry a credential from `routes`.
    fn refuse_credentials(&self, routes: &RouteTable) -> std::io::Result<()> {
        let secrets = routes.secrets();
        let carries = |text: &str| {
            secrets
                .iter()
                .any(|secret| !secret.is_empty() && text.contains(secret))
        };
        let declared = match &self.environment {
            LaunchEnvironment::Cleared(vars) => vars.as_slice(),
            LaunchEnvironment::InheritWithoutCredentials => &[],
        };
        if carries(&self.program)
            || self.args.iter().any(|arg| carries(arg))
            || declared
                .iter()
                .any(|(name, value)| carries(name) || carries(value))
        {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "the entrant launch would carry a host credential; credentials stay with the gateway",
            ));
        }
        Ok(())
    }

    /// Start the process with its stdin and stdout piped to the host.
    pub fn spawn(&self, routes: &RouteTable) -> std::io::Result<EntrantPipes> {
        self.refuse_credentials(routes)?;
        let mut command = Command::new(&self.program);
        command
            .args(&self.args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped());
        let environment = match &self.environment {
            LaunchEnvironment::Cleared(vars) => vars.clone(),
            LaunchEnvironment::InheritWithoutCredentials => {
                launcher_environment(std::env::vars(), &routes.secrets())
            }
        };
        command.env_clear().envs(environment);
        // Its own process group, so teardown reaches a wrapper's children too.
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            command.process_group(0);
        }
        let mut child = command.spawn()?;
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| std::io::Error::other("no stdin"))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| std::io::Error::other("no stdout"))?;
        Ok(EntrantPipes::new(
            Box::new(stdin),
            Box::new(stdout),
            Box::new(ChildEntrant(child)),
        ))
    }
}

/// The host environment a launcher inherits: everything except a variable
/// whose value carries a route credential. Pure in its inputs.
pub fn launcher_environment(
    vars: impl IntoIterator<Item = (String, String)>,
    secrets: &[&str],
) -> Vec<(String, String)> {
    vars.into_iter()
        .filter(|(_, value)| {
            !secrets
                .iter()
                .any(|secret| !secret.is_empty() && value.contains(secret))
        })
        .collect()
}

struct ChildEntrant(Child);

impl EntrantProcess for ChildEntrant {
    fn exited(&mut self) -> Option<Option<i32>> {
        match self.0.try_wait() {
            Ok(Some(status)) => Some(status.code()),
            _ => None,
        }
    }

    fn terminate(&mut self) {
        #[cfg(unix)]
        {
            let leader = self.0.id();
            signal_group(leader, "TERM");
            let deadline = Instant::now() + TEARDOWN_GRACE;
            while matches!(self.0.try_wait(), Ok(None)) && Instant::now() < deadline {
                std::thread::sleep(Duration::from_millis(10));
            }
            signal_group(leader, "KILL");
        }
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// Signal a process group by its leader's pid. The same spelling, and the same
/// load-bearing `--`, as the external-agent transport's teardown.
#[cfg(unix)]
fn signal_group(leader: u32, signal: &str) {
    let _ = Command::new("kill")
        .arg(format!("-{signal}"))
        .arg("--")
        .arg(format!("-{leader}"))
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
}

enum Wire {
    Line(String),
    Oversized,
    OutputBudgetExceeded,
}

/// One line, bounded before it is allocated past the cap. Reading one byte
/// beyond the cap is what tells a maximal line from an endless one.
fn read_wire<R: BufRead>(reader: &mut R) -> std::io::Result<Option<Wire>> {
    let mut buffer = Vec::new();
    reader
        .take(MAX_ENTRANT_LINE + 1)
        .read_until(b'\n', &mut buffer)?;
    if buffer.is_empty() {
        return Ok(None);
    }
    if buffer.len() as u64 > MAX_ENTRANT_LINE {
        return Ok(Some(Wire::Oversized));
    }
    String::from_utf8(buffer)
        .map(|line| Some(Wire::Line(line)))
        .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error))
}

fn spawn_line_reader<R: BufRead + Send + 'static>(
    mut reader: R,
    total_budget: u64,
) -> Receiver<std::io::Result<Wire>> {
    // One queued line and one in flight, each bounded, so a stream of small
    // lines cannot become an unbounded host allocation.
    let (tx, lines) = mpsc::sync_channel(1);
    std::thread::spawn(move || {
        let mut remaining = total_budget;
        loop {
            let (message, terminal) = match read_wire(&mut reader) {
                Ok(None) => break,
                Ok(Some(Wire::Line(line))) => {
                    if line.len() as u64 > remaining {
                        (Ok(Wire::OutputBudgetExceeded), true)
                    } else {
                        remaining -= line.len() as u64;
                        (Ok(Wire::Line(line)), false)
                    }
                }
                Ok(Some(wire)) => (Ok(wire), true),
                Err(error) => (Err(error), true),
            };
            if tx.send(message).is_err() || terminal {
                break;
            }
        }
    });
    lines
}

/// Whether a stdout line is addressed to the gateway. The probe skips every
/// other member without building a tree from it, and borrows the protocol
/// string where the line lets it.
fn is_gateway_request(line: &str) -> bool {
    #[derive(Deserialize)]
    struct Probe<'a> {
        #[serde(borrow, default)]
        protocol: Option<Cow<'a, str>>,
    }
    serde_json::from_str::<Probe<'_>>(line)
        .ok()
        .and_then(|probe| probe.protocol)
        .is_some_and(|protocol| protocol.starts_with(GATEWAY_PROTOCOL_FAMILY))
}

fn parse_decision(line: &str, observation: &MarketObservation) -> Result<Decision, DecideError> {
    let decision = sharpebench_protocol::decision_from_wire(line).map_err(|diagnostic| {
        eprintln!("agent protocol fault: {diagnostic}");
        DecideError::Protocol
    })?;
    decision.validate_for(observation).map_err(|diagnostic| {
        eprintln!("agent protocol fault: decision is not valid for the observation it answers: {diagnostic}");
        DecideError::Protocol
    })?;
    Ok(decision)
}

fn error_hold(reason: &str) -> Decision {
    Decision {
        orders: Vec::new(),
        reasoning: reason.to_string(),
        cost: None,
    }
}

/// A line the entrant wrote, and whether its process had already exited.
enum Next {
    Live(String),
    AfterExit(String, Option<i32>),
}

/// An entrant whose model calls go through the host's gateway.
///
/// It is an [`Agent`] like any external agent, so the sweep, the failure
/// taxonomy and the scoring path are the ones every other entrant goes
/// through. It borrows the gateway for its lifetime: one entrant, one broker,
/// one journal.
pub struct GatewayEntrant<'g, 'r, T: ProviderTransport> {
    gateway: &'g mut ModelGateway<'r, T>,
    process: Box<dyn EntrantProcess>,
    input: SyncSender<String>,
    written: Receiver<std::io::Result<()>>,
    lines: Receiver<std::io::Result<Wire>>,
    timeout: Duration,
    timed_out: bool,
    breaker: CircuitBreaker,
    health: TransportHealth,
    served: u32,
}

impl<'g, 'r, T: ProviderTransport> GatewayEntrant<'g, 'r, T> {
    pub fn new(pipes: EntrantPipes, gateway: &'g mut ModelGateway<'r, T>) -> Self {
        let EntrantPipes {
            mut stdin,
            stdout,
            process,
        } = pipes;
        // Blocking pipe writes live on one owned worker, never on the timed
        // decision thread.
        let (input, requests) = mpsc::sync_channel::<String>(1);
        let (completed, written) = mpsc::sync_channel(1);
        std::thread::spawn(move || {
            while let Ok(line) = requests.recv() {
                let result = writeln!(stdin, "{line}").and_then(|()| stdin.flush());
                let failed = result.is_err();
                if completed.send(result).is_err() || failed {
                    break;
                }
            }
        });
        let lines = spawn_line_reader(BufReader::new(stdout), MAX_ENTRANT_STDOUT);
        Self {
            gateway,
            process,
            input,
            written,
            lines,
            timeout: DEFAULT_DECIDE_TIMEOUT,
            timed_out: false,
            breaker: CircuitBreaker::new(BREAKER_THRESHOLD),
            health: TransportHealth::default(),
            served: 0,
        }
    }

    /// Override the entrant's per-decision budget (default 30 s, net of host
    /// serving time).
    pub fn with_decide_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    /// Gateway requests this entrant handed to the broker.
    pub fn requests_served(&self) -> u32 {
        self.served
    }

    fn exit_within_grace(&mut self, deadline: Instant) -> Option<Option<i32>> {
        let grace = (Instant::now() + EXIT_DRAIN_GRACE).min(deadline);
        loop {
            if let Some(code) = self.process.exited() {
                return Some(code);
            }
            if Instant::now() >= grace {
                return None;
            }
            std::thread::sleep(
                DEAD_CHILD_POLL.min(grace.saturating_duration_since(Instant::now())),
            );
        }
    }

    fn send(&mut self, line: String, deadline: Instant) -> Result<(), DecideError> {
        if Instant::now() >= deadline {
            return Err(DecideError::Timeout);
        }
        self.input
            .try_send(line)
            .map_err(|_| DecideError::Transport)?;
        match self
            .written
            .recv_timeout(deadline.saturating_duration_since(Instant::now()))
        {
            Ok(Ok(())) => Ok(()),
            Ok(Err(_)) => Err(self
                .exit_within_grace(deadline)
                .map_or(DecideError::Transport, DecideError::Exited)),
            Err(RecvTimeoutError::Timeout) => Err(DecideError::Timeout),
            Err(RecvTimeoutError::Disconnected) => Err(DecideError::Transport),
        }
    }

    fn next_line(&mut self, deadline: Instant) -> Result<Next, DecideError> {
        loop {
            if Instant::now() >= deadline {
                return Err(DecideError::Timeout);
            }
            let slice = DEAD_CHILD_POLL.min(deadline.saturating_duration_since(Instant::now()));
            match self.lines.recv_timeout(slice) {
                Ok(Ok(Wire::Line(line))) => return Ok(Next::Live(line)),
                Ok(Ok(Wire::Oversized | Wire::OutputBudgetExceeded)) => {
                    return Err(DecideError::Oversized)
                }
                Ok(Err(_)) => return Err(DecideError::Transport),
                Err(RecvTimeoutError::Disconnected) => {
                    return Err(self
                        .exit_within_grace(deadline)
                        .map_or(DecideError::Transport, DecideError::Exited));
                }
                Err(RecvTimeoutError::Timeout) => {
                    let Some(code) = self.process.exited() else {
                        continue;
                    };
                    // A fast entrant can answer and exit in the same instant:
                    // drain its last line before ruling the exit unanswered.
                    let grace = (Instant::now() + EXIT_DRAIN_GRACE).min(deadline);
                    loop {
                        let wait =
                            DEAD_CHILD_POLL.min(grace.saturating_duration_since(Instant::now()));
                        match self.lines.recv_timeout(wait) {
                            Ok(Ok(Wire::Line(line))) => return Ok(Next::AfterExit(line, code)),
                            Ok(Ok(_)) => return Err(DecideError::Oversized),
                            Ok(Err(_)) => return Err(DecideError::Transport),
                            Err(RecvTimeoutError::Disconnected) => break,
                            Err(RecvTimeoutError::Timeout) if Instant::now() >= grace => break,
                            Err(RecvTimeoutError::Timeout) => {}
                        }
                    }
                    return Err(DecideError::Exited(code));
                }
            }
        }
    }

    /// Whether a response line would hand the entrant host-only material:
    /// a route credential, a route destination or the journal's location. The
    /// broker never writes these itself; a provider or proxy that echoes one
    /// into the model text is what this catches. Both the raw and the
    /// JSON-escaped spelling are checked, because the line is JSON.
    fn carries_host_material(&self, line: &str) -> bool {
        let journal = self
            .gateway
            .journal_path
            .as_ref()
            .map(|path| path.display().to_string());
        let carries = self
            .gateway
            .routes
            .routes
            .iter()
            .flat_map(|route| [route.credential.expose(), route.destination.as_str()])
            .chain(journal.as_deref())
            .filter(|needle| needle.len() >= 4)
            .any(|needle| {
                let escaped = serde_json::to_string(needle).expect("a string serializes");
                line.contains(needle) || line.contains(&escaped[1..escaped.len() - 1])
            });
        carries
    }

    fn decide_once(&mut self, observation: &MarketObservation) -> Result<Decision, DecideError> {
        if self.timed_out {
            return Err(DecideError::Timeout);
        }
        let mut deadline = Instant::now()
            .checked_add(self.timeout)
            .ok_or(DecideError::Transport)?;
        let line = serde_json::to_string(observation).map_err(|_| DecideError::Transport)?;
        self.send(line, deadline)?;
        let mut requests = 0u32;
        loop {
            let (line, exited) = match self.next_line(deadline)? {
                Next::Live(line) => (line, None),
                Next::AfterExit(line, code) => (line, Some(code)),
            };
            let line = line.trim_end_matches(['\n', '\r']);
            if !is_gateway_request(line) {
                return parse_decision(line, observation);
            }
            // Nobody is left to read the answer, so nothing is spent on it.
            if let Some(code) = exited {
                return Err(DecideError::Exited(code));
            }
            requests = requests.saturating_add(1);
            let started = Instant::now();
            let response = if requests > self.gateway.limits.max_requests_per_decision {
                encode_refusal(GatewayErrorKind::DecisionRequestLimit)
            } else {
                self.served = self.served.saturating_add(1);
                let answered = self.gateway.serve_line(line);
                // A gate on the delivered bytes, not a warning: the call is
                // charged either way, but host material never crosses.
                if self.carries_host_material(&answered) {
                    encode_refusal(GatewayErrorKind::ResponseWithheld)
                } else {
                    answered
                }
            };
            // The host's serving time is not the entrant's.
            deadline = deadline
                .checked_add(started.elapsed())
                .ok_or(DecideError::Transport)?;
            self.send(response, deadline)?;
        }
    }
}

impl<T: ProviderTransport> Agent for GatewayEntrant<'_, '_, T> {
    fn decide(&mut self, observation: &MarketObservation) -> Decision {
        if self.breaker.is_tripped() {
            self.health.record(DecideError::Transport, true);
            return error_hold("gateway entrant circuit open -> hold");
        }
        match self.decide_once(observation) {
            Ok(decision) => {
                self.breaker.record_success();
                decision
            }
            Err(error) => {
                if error == DecideError::Timeout {
                    // A late line must never answer a later observation.
                    self.timed_out = true;
                    self.process.terminate();
                }
                let tripped = self.breaker.record_fault();
                self.health.record(error, tripped);
                error_hold("gateway entrant transport fault -> hold")
            }
        }
    }
}

impl<T: ProviderTransport> TransportDiagnostics for GatewayEntrant<'_, '_, T> {
    fn health(&self) -> &TransportHealth {
        &self.health
    }
}

impl<T: ProviderTransport> Drop for GatewayEntrant<'_, '_, T> {
    fn drop(&mut self) {
        self.process.terminate();
    }
}

/// One attempt: drive the entrant behind `pipes` through one backtest with its
/// model calls going through `gateway`. The transport-honest failure mapping is
/// the external-agent one, unchanged.
pub fn gateway_backtest<T: ProviderTransport>(
    data: &Dataset,
    pipes: EntrantPipes,
    gateway: &mut ModelGateway<'_, T>,
    window: Window,
    seed: u64,
    costs: CostModel,
    card: Option<&RateCard>,
) -> AttemptObservation {
    let mut entrant = GatewayEntrant::new(pipes, gateway);
    crate::run_external_backtest_observed(data, &mut entrant, window, seed, costs, card)
}

/// The invocation identity of a gateway-attached sweep: the caller's own
/// invocation digest with the route table and the budget folded in. Changing a
/// model, a revision, a price, a destination or a ceiling then cannot resume
/// the checkpoint; rotating a credential can, because credentials are not in
/// the route identity.
pub fn gateway_invocation_sha256(
    invocation_sha256: &str,
    routes: &RouteTable,
    budget: GatewayBudget,
) -> String {
    sharpebench_attest::content_digest(
        &serde_json::to_vec(&(
            GATEWAY_INVOCATION_VERSION,
            invocation_sha256,
            routes.identity_digest(),
            budget,
        ))
        .expect("a gateway invocation serializes"),
    )
}

/// The digest a sweep's money journal is bound to.
pub fn gateway_sweep_sha256(agent_id: &str, contract: &SweepContract) -> String {
    sharpebench_attest::content_digest(
        &serde_json::to_vec(&(GATEWAY_SWEEP_VERSION, agent_id, contract))
            .expect("a sweep contract serializes"),
    )
}

/// Where a gateway-attached sweep keeps its state and what it runs.
pub struct GatewaySweep<'a> {
    pub checkpoint: &'a Path,
    pub journal: &'a Path,
    pub agent_id: &'a str,
    /// The sweep identity without the gateway. Its invocation digest gets the
    /// gateway binding folded in before the contract is built.
    pub identity: SweepIdentity,
    pub windows: &'a [Window],
    pub seeds: &'a [u64],
    pub max_retries: u32,
    pub policy: ResumePolicy,
}

/// The host side of the gateway: everything an entrant may neither see nor set.
pub struct GatewayHost<'r, T> {
    pub routes: &'r RouteTable,
    pub permits: &'r CallPermits,
    pub transport: T,
    pub budget: GatewayBudget,
    pub limits: GatewayLimits,
}

/// What the host observed a sweep's model calls cost. Published beside the
/// scored pool and never read by any score, rank or pass^k pool.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct HostObservedUsage {
    pub schema_version: &'static str,
    pub route_table_sha256: String,
    pub sweep_sha256: String,
    /// `estimated` with `usage_source: host_observed` only when every call was
    /// priced from provider-reported usage; otherwise `unavailable`.
    pub monetary_cost: MonetarySummary,
    pub calls_started: u32,
    pub priced_calls: u32,
    pub unknown_calls: u32,
    pub released_calls: u32,
    pub overspent_usd_nanos: String,
    pub overspent_calls: u32,
    pub ceiling_breached: bool,
    /// The gateway lost its journal file to another writer and stopped
    /// spending. The figures above are this process's in-memory record.
    pub journal_ownership_lost: bool,
    pub rank_neutral: bool,
}

impl HostObservedUsage {
    fn of<T: ProviderTransport>(gateway: &ModelGateway<'_, T>, sweep_sha256: String) -> Self {
        let journal = gateway.journal();
        let state = journal.spend();
        Self {
            schema_version: HOST_USAGE_VERSION,
            route_table_sha256: journal.identity.route_table_sha256.clone(),
            sweep_sha256,
            monetary_cost: journal.monetary_summary(gateway.routes.single_rate_card()),
            calls_started: state.calls_started,
            priced_calls: state.priced_calls,
            unknown_calls: state.unknown_calls,
            released_calls: state.released_calls,
            overspent_usd_nanos: state.overspent_usd_nanos.to_string(),
            overspent_calls: state.overspent_calls,
            ceiling_breached: journal.ceiling_breached(),
            journal_ownership_lost: gateway.journal_conflict(),
            rank_neutral: true,
        }
    }
}

/// A finished gateway-attached sweep.
pub struct GatewaySweepOutcome {
    /// The scored pool, failures and attempt accounting, exactly as any
    /// external sweep produces them.
    pub result: ResilientSubmission,
    /// The host-observed usage, kept apart from `result` so no scoring path
    /// can read it.
    pub host_observed: HostObservedUsage,
    /// The contract the checkpoint is bound to, gateway binding included.
    pub contract: SweepContract,
}

/// The checkpoint and the journal are one record of one sweep: either both
/// exist or the pair is refused.
///
/// A checkpoint without its journal would resume and report the calls its
/// finished cells made as never made. A journal with calls but no checkpoint
/// would re-run cells whose calls it already charged. Both are refused rather
/// than guessed at. A fresh pair starts by writing an empty journal, so a
/// checkpoint can never exist before its journal does.
fn admit_pair(
    checkpoint: &Path,
    journal: &Path,
    identity: &JournalIdentity,
) -> std::io::Result<GatewayJournal> {
    let checkpoint_exists = checkpoint.try_exists()?;
    match GatewayJournal::load_bound(journal, identity) {
        Ok(existing) => {
            if !checkpoint_exists && !existing.records().is_empty() {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "the gateway journal records calls but its sweep checkpoint is missing; a new checkpoint would re-run cells this journal already charged",
                ));
            }
            Ok(existing)
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            if checkpoint_exists {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "the sweep checkpoint exists but its gateway journal is missing; resuming would report the calls its finished cells made as never made",
                ));
            }
            let mut fresh = GatewayJournal::new(identity.clone());
            fresh.save(journal).map_err(|error| match error {
                JournalSaveError::Io(error) => error,
                conflict => {
                    std::io::Error::new(std::io::ErrorKind::InvalidData, conflict.to_string())
                }
            })?;
            Ok(fresh)
        }
        Err(error) => Err(error),
    }
}

/// Run, or resume, a sweep whose entrant reaches its model through the host.
///
/// `attempt` runs one (window, seed) cell with the gateway it is handed,
/// typically by spawning the entrant and calling [`gateway_backtest`]. The
/// checkpoint is bound to the route table and the budget through its
/// invocation digest, and the journal is bound to the checkpoint's contract,
/// so neither can be resumed against the other's experiment. Completed cells
/// are skipped on resume, which means they make no new calls; their spend is
/// folded from the journal, never re-quoted.
pub fn run_gateway_sweep<'r, T, F>(
    sweep: GatewaySweep<'_>,
    host: GatewayHost<'r, T>,
    mut attempt: F,
) -> std::io::Result<GatewaySweepOutcome>
where
    T: ProviderTransport,
    F: FnMut(usize, u64, &mut ModelGateway<'r, T>) -> AttemptObservation,
{
    let mut identity = sweep.identity;
    identity.invocation_sha256 =
        gateway_invocation_sha256(&identity.invocation_sha256, host.routes, host.budget);
    let contract = SweepContract::new(identity, sweep.windows, sweep.seeds, sweep.max_retries);
    let sweep_sha256 = gateway_sweep_sha256(sweep.agent_id, &contract);
    let journal_identity = JournalIdentity::new(host.routes.identity_digest(), host.budget)
        .for_sweep(sweep_sha256.clone());
    let journal = admit_pair(sweep.checkpoint, sweep.journal, &journal_identity)?;
    let mut gateway = ModelGateway {
        routes: host.routes,
        permits: host.permits,
        transport: host.transport,
        journal,
        journal_path: Some(sweep.journal.to_path_buf()),
        limits: host.limits,
        shutdown: GatewayShutdown::new(),
        dispatches: 0,
        journal_conflict: false,
    };
    let result = crate::run_resumable_sweep_observed(
        sweep.checkpoint,
        sweep.agent_id,
        &contract,
        sweep.windows,
        sweep.policy,
        |window, seed| attempt(window, seed, &mut gateway),
    )?;
    let host_observed = HostObservedUsage::of(&gateway, sweep_sha256);
    Ok(GatewaySweepOutcome {
        result,
        host_observed,
        contract,
    })
}

/// Put the host-observed usage on the entrant's row of a JSON board, beside its
/// scores. The board stays an array and every other field is untouched.
pub fn attach_host_observed_usage(
    board: &mut serde_json::Value,
    agent_id: &str,
    usage: &HostObservedUsage,
) {
    let usage = serde_json::to_value(usage).expect("host-observed usage serializes");
    if let Some(rows) = board.as_array_mut() {
        for row in rows {
            if row["agent_id"].as_str() == Some(agent_id) {
                row["host_observed_usage"] = usage.clone();
            }
        }
    }
}

#[cfg(test)]
#[path = "gateway_serve_tests.rs"]
mod tests;
