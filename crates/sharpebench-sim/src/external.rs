//! External-process agent - speak the JSON protocol to an any-language agent.
//!
//! The adoption surface: an agent is just a subprocess that reads one
//! [`MarketObservation`] (JSON) per line on stdin and writes one [`Decision`]
//! (JSON) per line on stdout. Python, TS, a hosted shim - anything that honors
//! the contract competes.
//!
//! Transport integrity: a decision that fails at the wire is **not** silently
//! reported as a hold (which would bias the return series flat and hide the fault).
//! The HTTP transport retries a transient blip a bounded number of times; both
//! transports drive a per-endpoint [`CircuitBreaker`] and record every fault into a
//! [`TransportHealth`] the harness inspects to surface the failure as a typed
//! `sharpebench_harness::FailureKind` rather than a masked hold. When a decision
//! still cannot be produced the call returns an empty-orders hold (the trait cannot
//! signal an error), but that hold is now *flagged* in the health - the harness no
//! longer mistakes it for a deliberate one.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::time::{Duration, Instant};

use sharpebench_protocol::{Decision, MarketObservation};

use crate::agent::Agent;
use crate::transport::{
    decide_with_retry, CircuitBreaker, DecideError, TransportDiagnostics, TransportHealth,
};

/// Cap on bytes read from an external agent's HTTP response, so a hostile or buggy
/// endpoint can't exhaust the harness's memory.
const MAX_AGENT_RESPONSE: u64 = 8 * 1024 * 1024;

/// Cap on bytes accepted for one stdio decision line, matching the HTTP
/// transport's [`MAX_AGENT_RESPONSE`].
///
/// A subprocess is no more trusted than an HTTP endpoint, and an unbounded
/// `read_line` grows a `String` for as long as the agent keeps writing without a
/// newline: one hostile (or merely wedged) entrant takes the whole harness down
/// with it, losing every other agent's results in the same sweep.
const MAX_AGENT_LINE: u64 = MAX_AGENT_RESPONSE;

/// One item pulled off an agent's stdout.
enum Wire {
    /// A newline-terminated (or final, unterminated) line within budget.
    Line(String),
    /// The agent wrote past [`MAX_AGENT_LINE`] without ending the line.
    Oversized,
}

/// Read one line from `reader`, bounded by [`MAX_AGENT_LINE`]. `Ok(None)` is EOF.
///
/// The budget is applied as `take(MAX_AGENT_LINE + 1)`: reading the extra byte is
/// what makes "the agent used its entire budget" and "the agent went past it"
/// two different observations. Capping at exactly the budget would make a
/// maximal legal line indistinguishable from an infinite one, so the transport
/// would have to reject or accept both.
fn read_wire<R: BufRead>(reader: &mut R) -> std::io::Result<Option<Wire>> {
    let mut buffer = Vec::new();
    reader
        .take(MAX_AGENT_LINE + 1)
        .read_until(b'\n', &mut buffer)?;
    if buffer.is_empty() {
        return Ok(None);
    }
    if buffer.len() as u64 > MAX_AGENT_LINE {
        return Ok(Some(Wire::Oversized));
    }
    // Non-UTF-8 output stays an I/O error, as `read_line` reported it before the
    // budget existed, so this change moves no other outcome between categories.
    String::from_utf8(buffer)
        .map(|line| Some(Wire::Line(line)))
        .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error))
}

/// Default consecutive-fault threshold before an endpoint's circuit breaker trips.
const DEFAULT_BREAKER_THRESHOLD: u32 = 3;

/// Default bounded per-decision retries on a transient HTTP transport blip.
const DEFAULT_HTTP_RETRIES: u32 = 2;

/// Wall-clock budget for one stdio decision. Pipes have no OS-level read
/// timeout (unlike sockets), so a subprocess that consumes the observation but
/// never answers - a shell entrypoint, a wedged agent - would block the harness
/// forever without this bound.
const STDIO_DECIDE_TIMEOUT: Duration = Duration::from_secs(30);

/// An empty-orders hold emitted when a decision could not be produced. The health
/// (not this value) carries whether it was a masked fault vs. a deliberate hold.
fn error_hold(reason: &str) -> Decision {
    Decision {
        orders: Vec::new(),
        reasoning: reason.to_string(),
        cost: None,
    }
}

/// Map a stdlib I/O error to a [`DecideError`], distinguishing a wall-clock timeout
/// (the platform surfaces it as `TimedOut` / `WouldBlock`) from a generic transport
/// break.
fn classify_io(err: &std::io::Error) -> DecideError {
    match err.kind() {
        std::io::ErrorKind::TimedOut | std::io::ErrorKind::WouldBlock => DecideError::Timeout,
        _ => DecideError::Transport,
    }
}

/// Parse and validate one wire decision.
///
/// `DecideError::Protocol` is a unit variant, so the typed value the harness
/// scores on cannot carry text. The diagnostic is therefore written to stderr at
/// the moment the fault is detected: an entrant whose agent is being rejected by
/// the closed contract sees which field did it and where the schema lives,
/// instead of an opaque protocol fault in the failure tally. The tally itself is
/// unchanged, so nothing about scoring depends on this being read.
fn parse_decision(
    response: &str,
    observation: &MarketObservation,
) -> Result<Decision, DecideError> {
    let decision = sharpebench_protocol::decision_from_wire(response).map_err(|diagnostic| {
        eprintln!("agent protocol fault: {diagnostic}");
        DecideError::Protocol
    })?;
    decision.validate_for(observation).map_err(|diagnostic| {
        eprintln!("agent protocol fault: decision is not valid for the observation it answers: {diagnostic}");
        DecideError::Protocol
    })?;
    Ok(decision)
}

/// Drives an external agent subprocess over newline-delimited JSON.
///
/// stdout is drained by a dedicated reader thread and consumed through a
/// channel, because a pipe cannot carry an OS read timeout the way the HTTP
/// transport's socket can: `recv_timeout` is the only way to bound how long a
/// silent subprocess can stall a decision. The thread exits on EOF, which the
/// child's death (see [`Drop`]) guarantees.
pub struct ExternalAgent {
    child: Child,
    stdin: ChildStdin,
    lines: Receiver<std::io::Result<Wire>>,
    timeout: Duration,
    breaker: CircuitBreaker,
    health: TransportHealth,
}

/// Environment variables a hermetically spawned agent keeps, beyond the
/// [`AGENT_ENV_PASSTHROUGH`] escape hatch. The set is what a plain subprocess
/// needs to *run at all*, not what any particular agent might want:
///
/// - `PATH`: resolving the program itself and any children it spawns.
/// - `TEMP` / `TMP` / `TMPDIR`: runtimes create temp files on startup.
/// - `HOME` / `USERPROFILE`: per-user paths interpreters resolve at startup.
/// - Windows `SystemRoot` / `windir`: DLL loading and Winsock initialization
///   fail without them; `ComSpec`, `PATHEXT` and `SystemDrive` are how scripts
///   and shells resolve; `APPDATA` / `LOCALAPPDATA` are required by common
///   runtimes (PowerShell module paths, Python user site, npm).
/// - Unix `LANG` / `LC_ALL` / `TZ`: text encoding and time, when set.
///
/// Everything else — API keys first among them — stays in the harness.
#[cfg(windows)]
const HERMETIC_ENV_ALLOWLIST: &[&str] = &[
    "PATH",
    "PATHEXT",
    "SYSTEMROOT",
    "WINDIR",
    "COMSPEC",
    "SYSTEMDRIVE",
    "TEMP",
    "TMP",
    "USERPROFILE",
    "APPDATA",
    "LOCALAPPDATA",
];
#[cfg(not(windows))]
const HERMETIC_ENV_ALLOWLIST: &[&str] = &["PATH", "HOME", "TMPDIR", "LANG", "LC_ALL", "TZ"];

/// Escape hatch for a legitimately env-dependent agent on the hermetic spawn
/// path: a comma-separated list of variable *names* to pass through from the
/// harness environment (e.g. `SHARPEBENCH_AGENT_ENV=MY_DATA_DIR,MY_TOKEN`).
/// An explicit, visible opt-in per variable — never the whole environment.
pub const AGENT_ENV_PASSTHROUGH: &str = "SHARPEBENCH_AGENT_ENV";

/// The environment a hermetic spawn hands the agent: the platform allowlist,
/// plus `extra` names, plus names listed in [`AGENT_ENV_PASSTHROUGH`] — each
/// resolved against the harness environment. Windows matches names
/// case-insensitively, as the platform does.
fn agent_environment(extra: &[&str]) -> Vec<(String, String)> {
    let passthrough = std::env::var(AGENT_ENV_PASSTHROUGH).unwrap_or_default();
    let wanted: Vec<&str> = HERMETIC_ENV_ALLOWLIST
        .iter()
        .copied()
        .chain(extra.iter().copied())
        .chain(passthrough.split(',').map(str::trim))
        .filter(|name| !name.is_empty())
        .collect();
    std::env::vars()
        .filter(|(key, _)| {
            wanted.iter().any(|name| {
                if cfg!(windows) {
                    name.eq_ignore_ascii_case(key)
                } else {
                    *name == key
                }
            })
        })
        .collect()
}

impl ExternalAgent {
    /// Spawn `program args...` as an agent subprocess with a **cleared**
    /// environment: the agent receives only the fixed hermetic allowlist and the
    /// names opted in via `SHARPEBENCH_AGENT_ENV`, never the
    /// harness's full environment (API keys included). An agent process is no more trusted
    /// than an HTTP endpoint; see [`ExternalAgent::spawn_with_env`] for a
    /// programmatic per-variable pass-through and
    /// [`ExternalAgent::spawn_inheriting`] for trusted transport tooling.
    pub fn spawn(program: &str, args: &[&str]) -> std::io::Result<Self> {
        Self::spawn_with_env(program, args, &[])
    }

    /// Like [`ExternalAgent::spawn`], but additionally passes the named
    /// variables through from the harness environment. For a driver that knows
    /// exactly which variables its agent legitimately needs (e.g. an API key
    /// for a paid-model shim) — still an explicit list, never the world.
    pub fn spawn_with_env(
        program: &str,
        args: &[&str],
        extra_vars: &[&str],
    ) -> std::io::Result<Self> {
        let mut command = Command::new(program);
        command.env_clear().envs(agent_environment(extra_vars));
        Self::spawn_command(command, args)
    }

    /// Spawn with the harness's full environment inherited. **Not** for agent
    /// code: this exists for trusted transport tooling that wraps the agent —
    /// the `docker` client needs `DOCKER_HOST` / `DOCKER_CONFIG` and friends,
    /// while the untrusted code inside the container gets the container's own
    /// fresh environment regardless of what the client process holds.
    pub fn spawn_inheriting(program: &str, args: &[&str]) -> std::io::Result<Self> {
        Self::spawn_command(Command::new(program), args)
    }

    fn spawn_command(mut command: Command, args: &[&str]) -> std::io::Result<Self> {
        command
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped());
        // Give the agent its own process group so teardown can reach the whole
        // tree. An entrant is typically `sh -c ...` or `python -c ...` wrapping a
        // worker; killing only the direct child leaves the grandchild alive,
        // holding the inherited stdout pipe, so the reader thread never sees EOF
        // and every subsequent decision spends the full wall-clock budget waiting
        // on a process whose parent is already dead.
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
        let (tx, lines) = mpsc::channel();
        std::thread::spawn(move || {
            let mut reader = BufReader::new(stdout);
            loop {
                match read_wire(&mut reader) {
                    Ok(None) => break,
                    Ok(Some(wire)) => {
                        // An agent that blew the budget is finished: the rest of
                        // that line is not a sequence of further decisions, and
                        // draining it is the memory burn this cap exists to stop.
                        let overflowed = matches!(wire, Wire::Oversized);
                        if tx.send(Ok(wire)).is_err() || overflowed {
                            break;
                        }
                    }
                    Err(e) => {
                        let _ = tx.send(Err(e));
                        break;
                    }
                }
            }
        });
        Ok(Self {
            child,
            stdin,
            lines,
            timeout: STDIO_DECIDE_TIMEOUT,
            breaker: CircuitBreaker::new(DEFAULT_BREAKER_THRESHOLD),
            health: TransportHealth::default(),
        })
    }

    /// Override the per-decision wall-clock budget (default 30s).
    pub fn with_decide_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    /// Classify a stdin write failure: writing to a child that has already
    /// exited is the exit, not a generic pipe break, so the fault names the
    /// exit status instead of burning classification on the symptom. The pipe
    /// teardown can precede the observable exit by a beat, so the check gets
    /// the same short grace the other exit paths use.
    fn stdin_error(&mut self, error: &std::io::Error) -> DecideError {
        let deadline = Instant::now() + EXIT_DRAIN_GRACE;
        loop {
            match self.child.try_wait() {
                Ok(Some(status)) => return exited_fault(status),
                Ok(None) if Instant::now() < deadline => std::thread::sleep(DEAD_CHILD_POLL),
                _ => return classify_io(error),
            }
        }
    }

    /// The child exited with no pending decision. A fast agent may have
    /// answered and exited in the same instant: its line is either already in
    /// the channel or still in flight through the reader thread, which reaches
    /// EOF (and drops the sender) promptly once the pipe drains. Drain within a
    /// short bounded grace before ruling the exit unanswered — an answered
    /// exit is a SUCCESS, not a failure.
    fn drain_after_exit(
        &mut self,
        status: std::process::ExitStatus,
        obs: &MarketObservation,
    ) -> Result<Decision, DecideError> {
        let deadline = Instant::now() + EXIT_DRAIN_GRACE;
        loop {
            match self.lines.recv_timeout(DEAD_CHILD_POLL) {
                Ok(Ok(Wire::Line(resp))) => return parse_decision(&resp, obs),
                Ok(Ok(Wire::Oversized)) => return Err(oversized_fault()),
                Ok(Err(_)) => return Err(DecideError::Transport),
                Err(RecvTimeoutError::Disconnected) => break,
                Err(RecvTimeoutError::Timeout) if Instant::now() >= deadline => break,
                Err(RecvTimeoutError::Timeout) => {}
            }
        }
        Err(exited_fault(status))
    }

    /// One decision attempt over the stdio pipe, returning a typed [`DecideError`]
    /// rather than degrading to a hold. A closed stdout (EOF) or a broken pipe is a
    /// transport fault; unparseable output is the agent's protocol fault; a
    /// subprocess that stays silent past [`STDIO_DECIDE_TIMEOUT`] is a timeout —
    /// but a subprocess that *exited* with no answer pending is reported as the
    /// exit it is, immediately, instead of spending the full budget to call a
    /// startup crash a timeout.
    fn decide_once(&mut self, obs: &MarketObservation) -> Result<Decision, DecideError> {
        let line = serde_json::to_string(obs).map_err(|_| DecideError::Transport)?;
        if let Err(error) = writeln!(self.stdin, "{line}") {
            return Err(self.stdin_error(&error));
        }
        if let Err(error) = self.stdin.flush() {
            return Err(self.stdin_error(&error));
        }
        let deadline = Instant::now() + self.timeout;
        loop {
            let slice = DEAD_CHILD_POLL.min(deadline.saturating_duration_since(Instant::now()));
            match self.lines.recv_timeout(slice) {
                Ok(Ok(Wire::Line(resp))) => return parse_decision(&resp, obs),
                Ok(Ok(Wire::Oversized)) => return Err(oversized_fault()),
                Ok(Err(_)) => return Err(DecideError::Transport),
                Err(RecvTimeoutError::Disconnected) => {
                    // The reader saw EOF and the channel is empty. EOF races the
                    // process teardown itself — the pipe closes a beat before
                    // `try_wait` can observe the exit — so give the exit the
                    // same short grace the drain gets. A child that merely
                    // closed stdout while still running stays a transport break.
                    let deadline = Instant::now() + EXIT_DRAIN_GRACE;
                    loop {
                        match self.child.try_wait() {
                            Ok(Some(status)) => return Err(exited_fault(status)),
                            Ok(None) if Instant::now() < deadline => {
                                std::thread::sleep(DEAD_CHILD_POLL)
                            }
                            _ => return Err(DecideError::Transport),
                        }
                    }
                }
                Err(RecvTimeoutError::Timeout) => {
                    if let Ok(Some(status)) = self.child.try_wait() {
                        return self.drain_after_exit(status, obs);
                    }
                    if Instant::now() >= deadline {
                        return Err(DecideError::Timeout);
                    }
                }
            }
        }
    }
}

/// How often the decide wait wakes to check whether the child has died. The
/// cost of a wake is one `try_wait`, so a tight cadence is cheap next to the
/// 30s budget it saves on a startup crash.
const DEAD_CHILD_POLL: Duration = Duration::from_millis(25);

/// How long a just-exited child's final line gets to travel from the pipe
/// through the reader thread before the exit is ruled unanswered. The reader
/// hits EOF and drops the sender almost immediately once the child is gone, so
/// the normal exit path leaves through `Disconnected` well before this cap.
const EXIT_DRAIN_GRACE: Duration = Duration::from_millis(500);

/// The typed fault for a child that exited without answering, with the exit
/// status named on stderr at the moment it is detected (the unit-style
/// [`DecideError`] payload carries only the code).
fn exited_fault(status: std::process::ExitStatus) -> DecideError {
    eprintln!(
        "agent transport fault: the agent process exited ({status}) with no decision pending"
    );
    DecideError::Exited(status.code())
}

/// The oversized-line protocol fault with its entrant-facing diagnostic.
fn oversized_fault() -> DecideError {
    eprintln!(
        "agent protocol fault: one decision exceeded the {MAX_AGENT_LINE}-byte line \
         budget without a newline; the contract is one JSON decision per line"
    );
    DecideError::Oversized
}

impl Agent for ExternalAgent {
    fn decide(&mut self, obs: &MarketObservation) -> Decision {
        // A tripped breaker fails fast: don't keep hammering a dead subprocess, but
        // still record the masked hold so the run is surfaced as a failure.
        if self.breaker.is_tripped() {
            self.health.record(DecideError::Transport, true);
            return error_hold("external agent circuit open → hold");
        }
        // A dead subprocess pipe cannot recover within the same child, so there is
        // no in-process retry; the harness retries at the run level by respawning.
        match self.decide_once(obs) {
            Ok(d) => {
                self.breaker.record_success();
                d
            }
            Err(e) => {
                let tripped = self.breaker.record_fault();
                self.health.record(e, tripped);
                error_hold("external agent transport fault → hold")
            }
        }
    }
}

impl TransportDiagnostics for ExternalAgent {
    fn health(&self) -> &TransportHealth {
        &self.health
    }
}

/// How long a terminated agent group gets to flush and exit before it is killed.
/// A benchmark wants the agent's last line on the wire, so the sequence is
/// TERM, a short grace, then KILL, rather than a bare KILL.
///
/// Unix-only alongside the group teardown it paces: no other platform here has
/// a process group to signal.
#[cfg(unix)]
const TEARDOWN_GRACE: Duration = Duration::from_millis(500);

/// Signal a whole process group by its leader's pid.
///
/// `kill -<SIGNAL> -- -<pgid>` is the POSIX spelling for "the group". Shelling
/// out to `kill` rather than calling `killpg` keeps this crate free of both a
/// `libc` dependency and the `unsafe` its `#![forbid(unsafe_code)]` rules out,
/// and the agent teardown path is not hot enough for a process spawn to matter.
///
/// The `--` is load-bearing, not decoration: without it `kill` reads the leading
/// `-` of the group id as the start of another option, and at least procps' kill
/// then reports success while signalling something other than the intended
/// group. An argument that must never be parsed as an option is separated from
/// the options.
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

impl Drop for ExternalAgent {
    fn drop(&mut self) {
        // The group was established at spawn, so the leader's pid is the group id.
        #[cfg(unix)]
        {
            let leader = self.child.id();
            signal_group(leader, "TERM");
            let deadline = std::time::Instant::now() + TEARDOWN_GRACE;
            while matches!(self.child.try_wait(), Ok(None)) && std::time::Instant::now() < deadline
            {
                std::thread::sleep(Duration::from_millis(10));
            }
            // Anything still in the group after the grace period, grandchildren
            // holding the stdout pipe included, goes now.
            signal_group(leader, "KILL");
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// Drives an external agent over HTTP/1.1 - one request/response per decision.
///
/// Targets a plain-HTTP `host:port` endpoint that accepts `POST /decide` with a
/// JSON [`MarketObservation`] body and returns a JSON [`Decision`]. Loopback /
/// in-sandbox only (no TLS), so this is a dependency-free `std::net` client - the
/// benchmark sim keeps its minimal, audited dependency tree. Each decision opens a
/// fresh connection, so a transient blip is retried a bounded number of times before
/// the fault is recorded and the breaker advances.
pub struct HttpAgent {
    host: String,
    port: u16,
    retries: u32,
    breaker: CircuitBreaker,
    health: TransportHealth,
}

impl HttpAgent {
    /// `addr` is `host:port` (e.g. `"127.0.0.1:8080"`); each decision POSTs to
    /// `/decide`. A bare host defaults to port 80. Uses the default retry / breaker
    /// budget; see [`HttpAgent::with_resilience`] to tune it.
    pub fn new(addr: impl Into<String>) -> Self {
        Self::with_resilience(addr, DEFAULT_HTTP_RETRIES, DEFAULT_BREAKER_THRESHOLD)
    }

    /// Like [`HttpAgent::new`] but with an explicit per-decision retry budget and
    /// circuit-breaker threshold.
    pub fn with_resilience(addr: impl Into<String>, retries: u32, breaker_threshold: u32) -> Self {
        let addr = addr.into();
        let (host, port) = match addr.rsplit_once(':') {
            Some((h, p)) => (h.to_string(), p.parse().unwrap_or(80)),
            None => (addr, 80),
        };
        Self {
            host,
            port,
            retries,
            breaker: CircuitBreaker::new(breaker_threshold),
            health: TransportHealth::default(),
        }
    }

    /// One decision attempt over a fresh connection, returning a typed
    /// [`DecideError`]. A connect / write / read break or malformed HTTP framing is a
    /// transport fault; a non-JSON body is the agent's protocol fault.
    fn decide_once(&self, obs: &MarketObservation) -> Result<Decision, DecideError> {
        let body = serde_json::to_string(obs).map_err(|_| DecideError::Transport)?;
        let mut stream =
            TcpStream::connect((self.host.as_str(), self.port)).map_err(|e| classify_io(&e))?;
        // Bound time so a slow/stalled agent endpoint can't hang the harness.
        let timeout = std::time::Duration::from_secs(30);
        stream
            .set_read_timeout(Some(timeout))
            .map_err(|e| classify_io(&e))?;
        stream
            .set_write_timeout(Some(timeout))
            .map_err(|e| classify_io(&e))?;
        // `Connection: close` lets us read the whole response to EOF - no need to
        // parse Content-Length / chunked encoding for a one-shot request.
        let req = format!(
            "POST /decide HTTP/1.1\r\nHost: {}\r\nContent-Type: application/json\r\n\
             Content-Length: {}\r\nConnection: close\r\n\r\n{}",
            self.host,
            body.len(),
            body
        );
        stream
            .write_all(req.as_bytes())
            .map_err(|e| classify_io(&e))?;
        stream.flush().map_err(|e| classify_io(&e))?;
        // Cap the response size so a hostile endpoint can't exhaust memory.
        let mut raw = String::new();
        (&stream)
            .take(MAX_AGENT_RESPONSE)
            .read_to_string(&mut raw)
            .map_err(|e| classify_io(&e))?;
        let json = raw
            .split_once("\r\n\r\n")
            .map(|(_, b)| b)
            .ok_or(DecideError::Transport)?;
        parse_decision(json, obs)
    }
}

impl Agent for HttpAgent {
    fn decide(&mut self, obs: &MarketObservation) -> Decision {
        if self.breaker.is_tripped() {
            self.health.record(DecideError::Transport, true);
            return error_hold("http agent circuit open → hold");
        }
        match decide_with_retry(self.retries, || self.decide_once(obs)) {
            Ok(d) => {
                self.breaker.record_success();
                d
            }
            Err(e) => {
                let tripped = self.breaker.record_fault();
                self.health.record(e, tripped);
                error_hold("http agent transport fault → hold")
            }
        }
    }
}

impl TransportDiagnostics for HttpAgent {
    fn health(&self) -> &TransportHealth {
        &self.health
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn one_symbol_observation() -> MarketObservation {
        MarketObservation {
            date: "2026-01-01".to_string(),
            cash: 1000.0,
            symbols: vec![sharpebench_protocol::SymbolSnapshot {
                symbol: "A".to_string(),
                close_history: vec![100.0],
                fundamentals: Default::default(),
                news: Vec::new(),
            }],
            portfolio: Vec::new(),
        }
    }

    #[test]
    fn external_wire_parser_fails_closed_on_semantic_faults() {
        let obs = one_symbol_observation();
        assert!(parse_decision(
            r#"{"orders":[{"symbol":"A","action":"sell","target_weight":-0.5}]}"#,
            &obs
        )
        .is_ok());
        for invalid in [
            r#"{"orders":[{"symbol":"UNKNOWN","action":"buy","target_weight":0.1}]}"#,
            r#"{"orders":[{"symbol":"A","action":"buy","target_weight":0.1},{"symbol":"A","action":"buy","target_weight":0.2}]}"#,
            r#"{"orders":[{"symbol":"A","action":"buy","target_weight":2.0}]}"#,
            r#"{"orders":[],"unknown":true}"#,
        ] {
            assert!(matches!(
                parse_decision(invalid, &obs),
                Err(DecideError::Protocol)
            ));
        }
    }

    /// The line budget is a boundary, so it is tested at the boundary: a line
    /// that uses all of it is legal, and one byte more is not. A test that only
    /// fed it something enormous would pass against a cap set anywhere at all.
    #[test]
    fn one_decision_line_is_bounded_at_the_budget_and_not_before_it() {
        let budget = MAX_AGENT_LINE as usize;

        // Exactly the budget, newline included: still a decision.
        let mut maximal = vec![b'x'; budget - 1];
        maximal.push(b'\n');
        let mut reader = std::io::Cursor::new(maximal);
        let Ok(Some(Wire::Line(line))) = read_wire(&mut reader) else {
            panic!("a line that exactly fits the budget must be delivered");
        };
        assert_eq!(line.len(), budget);

        // One byte past it with no newline in sight: the unbounded read this
        // replaces would have kept growing the buffer from here.
        let mut reader = std::io::Cursor::new(vec![b'x'; budget + 1]);
        assert!(
            matches!(read_wire(&mut reader), Ok(Some(Wire::Oversized))),
            "a line past the budget must be refused, not accumulated"
        );

        // A final line with no trailing newline is short, not oversized.
        let mut reader = std::io::Cursor::new(b"{}".to_vec());
        let Ok(Some(Wire::Line(line))) = read_wire(&mut reader) else {
            panic!("an unterminated short line is still a line");
        };
        assert_eq!(line, "{}");

        let mut reader = std::io::Cursor::new(Vec::new());
        assert!(matches!(read_wire(&mut reader), Ok(None)), "empty is EOF");
    }

    /// An oversized line is the entrant's fault, not the harness's, so it must
    /// land in the same bucket as unparseable output rather than being retried
    /// like a broken pipe.
    #[test]
    fn an_oversized_line_is_scored_as_an_agent_fault() {
        assert!(!DecideError::Oversized.is_retryable());
        let mut health = TransportHealth::default();
        health.record(DecideError::Oversized, false);
        assert_eq!(health.protocol_faults, 1);
        assert_eq!(
            health.transport_faults, 0,
            "an oversized line is not a runtime blip and must not be retried as one"
        );
    }

    /// A grandchild that inherits stdout keeps the pipe open after its parent
    /// dies, so a teardown that reaches only the direct child leaves the reader
    /// thread waiting on an EOF that never comes. Unix-only: the fix is a
    /// process group, which is a Unix concept.
    #[cfg(unix)]
    #[test]
    fn teardown_reaches_a_grandchild_that_holds_the_stdout_pipe() {
        // The grandchild reports its own pid instead of being searched for in
        // `ps`. A marker in the argument list does not survive the exec a shell
        // performs for the last command of a script, and whether it performs one
        // differs between the shells `/bin/sh` is on Linux and on macOS, so the
        // marker version of this fixture failed its own self-check on macOS
        // while proving nothing about teardown. A pid survives exec.
        let pidfile =
            std::env::temp_dir().join(format!("sharpebench-teardown-{}.pid", std::process::id()));
        let _ = std::fs::remove_file(&pidfile);
        // `exec` keeps the pid the inner shell just reported, and the sleeper
        // inherits the stdout pipe, which is the whole point of the fixture.
        let script = format!(
            "sh -c 'echo $$ > \"{}\" ; exec sleep 30' & read line; exit 0",
            pidfile.display()
        );

        let alive = |pid: &str| {
            Command::new("kill")
                .args(["-0", pid])
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .expect("kill must run")
                .success()
        };

        let agent =
            ExternalAgent::spawn("sh", &["-c", &script]).expect("the platform shell must spawn");

        let deadline = std::time::Instant::now() + Duration::from_secs(10);
        let pid = loop {
            let reported = std::fs::read_to_string(&pidfile).unwrap_or_default();
            let reported = reported.trim().to_string();
            if !reported.is_empty() {
                break reported;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "the fixture never reported a grandchild, so it would have proved \
                 nothing about teardown"
            );
            std::thread::sleep(Duration::from_millis(20));
        };
        assert!(
            alive(&pid),
            "the fixture must actually have a live grandchild, or the assertion \
             below would hold for a test that proved nothing"
        );

        drop(agent);
        // Generous next to the teardown grace: the assertion is that the group
        // signal reaches the grandchild at all, not how fast the kernel reaps it.
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        while alive(&pid) && std::time::Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(50));
        }
        let outlived = alive(&pid);
        let _ = std::fs::remove_file(&pidfile);
        assert!(
            !outlived,
            "the backgrounded grandchild outlived the teardown, so it is still \
             holding the stdout pipe open"
        );
    }

    /// Spawn an agent that reports, in its decision's `reasoning`, whether the
    /// named environment variable reached it and whether `PATH` did.
    #[cfg(windows)]
    fn spawn_windows_fixture(argument: &str) -> ExternalAgent {
        let fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests")
            .join("fixtures")
            .join("stdio-agent.cmd");
        assert!(fixture.is_file(), "the Windows agent fixture must exist");
        // Cargo runs unit tests with the package root as cwd. Keeping the
        // command itself relative avoids cmd.exe's special, lossy treatment of
        // nested quotes after `/C` when a checkout path contains spaces.
        let command = format!(r"call tests\fixtures\stdio-agent.cmd {argument}");
        ExternalAgent::spawn("cmd.exe", &["/D", "/Q", "/C", &command])
            .expect("spawning the checked-in Windows agent fixture must work")
    }

    fn spawn_env_probe(var: &str) -> ExternalAgent {
        #[cfg(windows)]
        {
            spawn_windows_fixture(var)
        }
        #[cfg(not(windows))]
        {
            ExternalAgent::spawn(
                "sh",
                &[
                    "-c",
                    &format!(
                        "read line; c=0; [ -n \"${{{var}}}\" ] && c=1; p=0; [ -n \"$PATH\" ] && p=1; \
                         printf '{{\"orders\":[],\"reasoning\":\"var=%s path=%s\"}}\\n' \"$c\" \"$p\""
                    ),
                ],
            )
            .expect("spawning the platform shell must work")
        }
    }

    /// The hermetic spawn must clear the harness environment — an API key set in
    /// the harness must NOT arrive in the agent — while PATH (allowlisted)
    /// must survive, or nothing with an interpreter could run at all.
    #[test]
    fn a_spawned_agent_does_not_inherit_the_harness_environment() {
        std::env::set_var("SHARPEBENCH_TEST_LEAK", "a-secret-the-agent-must-not-see");
        let mut agent = spawn_env_probe("SHARPEBENCH_TEST_LEAK");
        let decision = agent.decide(&one_symbol_observation());
        assert_eq!(
            decision.reasoning, "var=0 path=1",
            "the canary must be cleared and PATH must survive"
        );
        assert!(
            !agent.health().degraded(),
            "the probe decision itself must be clean"
        );
    }

    /// The escape hatch: a variable *named* in SHARPEBENCH_AGENT_ENV passes
    /// through, so a legitimately env-dependent agent still works without
    /// reopening the whole environment.
    #[test]
    fn a_variable_named_in_the_passthrough_reaches_the_agent() {
        std::env::set_var("SHARPEBENCH_TEST_EXTRA", "42");
        // The same value the sibling test sets, so the two cannot race however
        // the test threads interleave.
        std::env::set_var(
            AGENT_ENV_PASSTHROUGH,
            "SHARPEBENCH_TEST_EXTRA, SHARPEBENCH_TEST_PURE_B ,",
        );
        let mut agent = spawn_env_probe("SHARPEBENCH_TEST_EXTRA");
        let decision = agent.decide(&one_symbol_observation());
        assert_eq!(decision.reasoning, "var=1 path=1");
    }

    /// The pure resolution logic, pinned directly: allowlist + programmatic
    /// extras + the passthrough names, and nothing else.
    #[test]
    fn agent_environment_resolves_allowlist_extras_and_passthrough_only() {
        std::env::set_var("SHARPEBENCH_TEST_PURE_A", "1");
        std::env::set_var("SHARPEBENCH_TEST_PURE_B", "2");
        std::env::set_var("SHARPEBENCH_TEST_PURE_C", "3");
        // The same value the sibling test sets, so the two cannot race however
        // the test threads interleave.
        std::env::set_var(
            AGENT_ENV_PASSTHROUGH,
            "SHARPEBENCH_TEST_EXTRA, SHARPEBENCH_TEST_PURE_B ,",
        );
        let env = agent_environment(&["SHARPEBENCH_TEST_PURE_A"]);
        let has = |name: &str| {
            env.iter().any(|(key, _)| {
                if cfg!(windows) {
                    key.eq_ignore_ascii_case(name)
                } else {
                    key == name
                }
            })
        };
        assert!(has("PATH"), "PATH is allowlisted");
        assert!(has("SHARPEBENCH_TEST_PURE_A"), "programmatic extra");
        assert!(has("SHARPEBENCH_TEST_PURE_B"), "passthrough name (trimmed)");
        assert!(
            !has("SHARPEBENCH_TEST_PURE_C"),
            "a variable nobody named must not leak through"
        );
    }

    /// Spawn a subprocess that consumes its stdin but never writes a line to
    /// stdout - the shape of a wedged agent (or a shell entrypoint that talks
    /// only to stderr).
    fn spawn_silent_agent() -> ExternalAgent {
        #[cfg(windows)]
        let agent = ExternalAgent::spawn(
            "powershell",
            &[
                "-NoProfile",
                "-Command",
                "$null = [Console]::In.ReadLine(); Start-Sleep -Seconds 60",
            ],
        );
        #[cfg(not(windows))]
        let agent = ExternalAgent::spawn("sh", &["-c", "read line; sleep 60"]);
        agent.expect("spawning the platform shell must work")
    }

    /// A child that dies at startup must be reported as its exit, immediately —
    /// not after burning the full decide budget and masquerading as a timeout.
    /// The default 30s budget is deliberately left in place: the assertion that
    /// the decision returns in a fraction of it is the fail-fast property.
    #[test]
    fn a_child_dead_at_startup_fails_fast_as_an_exit_not_a_timeout() {
        #[cfg(windows)]
        // Use the native command interpreter for an immediate, dependency-free
        // exit. Starting PowerShell can itself take most of the 30-second
        // decision budget on a loaded CI runner, which tests PowerShell startup
        // latency rather than this transport's child-exit polling.
        let agent = ExternalAgent::spawn("cmd.exe", &["/D", "/Q", "/C", "exit /B 3"]);
        #[cfg(not(windows))]
        let agent = ExternalAgent::spawn("sh", &["-c", "exit 3"]);
        let mut agent = agent.expect("spawning the platform shell must work");
        let start = std::time::Instant::now();
        let decision = agent.decide(&one_symbol_observation());
        assert!(
            start.elapsed() < Duration::from_secs(20),
            "a dead child must be detected long before the 30s decide budget"
        );
        assert!(decision.orders.is_empty(), "a faulted decision is a hold");
        assert_eq!(
            agent.health().last_error,
            Some(DecideError::Exited(Some(3))),
            "the fault must name the exit, not report a timeout or a bare transport break"
        );
    }

    /// The race the exit polling must not lose: an agent that answers and exits
    /// in the same breath has SUCCEEDED. Its line may still be in flight through
    /// the reader thread when `try_wait` first sees the exit, so the drain has
    /// to pick it up rather than ruling the exit unanswered.
    #[test]
    fn an_agent_that_answers_then_exits_is_a_success_not_an_exit_fault() {
        #[cfg(windows)]
        let mut agent = spawn_windows_fixture("--decision-only");
        #[cfg(not(windows))]
        let agent = ExternalAgent::spawn("sh", &["-c", "read line; echo '{\"orders\":[]}'"]);
        #[cfg(not(windows))]
        let mut agent = agent.expect("spawning the platform shell must work");
        let decision = agent.decide(&one_symbol_observation());
        assert!(decision.orders.is_empty());
        assert!(
            !agent.health().degraded(),
            "an answered exit is a clean decision, not a fault: {:?}",
            agent.health()
        );
    }

    /// The drain semantics, pinned deterministically (the integration test above
    /// exercises the race only when the exit poll happens to win it): a line
    /// still in flight when the exit is observed must be delivered as a
    /// SUCCESS, and only an exit with truly nothing pending is ruled the typed
    /// exit fault.
    #[test]
    fn the_post_exit_drain_delivers_a_line_in_flight_and_rules_only_on_silence() {
        use std::process::ExitStatus;

        fn finished_status(code: &str) -> ExitStatus {
            #[cfg(windows)]
            let status = Command::new("powershell")
                .args(["-NoProfile", "-Command", &format!("exit {code}")])
                .stdin(Stdio::null())
                .status();
            #[cfg(not(windows))]
            let status = Command::new("sh")
                .args(["-c", &format!("exit {code}")])
                .stdin(Stdio::null())
                .status();
            status.expect("the platform shell must run")
        }

        // A live-but-silent agent supplies the child/stdin plumbing; its real
        // channel is swapped for one the test controls.
        let make_agent = |rx| {
            let mut agent = spawn_silent_agent();
            agent.lines = rx;
            agent
        };
        let status = finished_status("7");
        let obs = one_symbol_observation();

        // A line that arrives during the drain window is the agent's answer.
        let (tx, rx) = mpsc::channel();
        let mut agent = make_agent(rx);
        let sender = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(100));
            let _ = tx.send(Ok(Wire::Line(r#"{"orders":[]}"#.to_string())));
        });
        let drained = agent.drain_after_exit(status, &obs);
        sender.join().expect("sender thread must finish");
        assert!(
            drained.is_ok(),
            "a line in flight at exit time is a success, not an exit fault: {drained:?}"
        );

        // Nothing pending and the sender gone: the exit is ruled as the fault.
        let (tx, rx) = mpsc::channel::<std::io::Result<Wire>>();
        drop(tx);
        let mut agent = make_agent(rx);
        assert_eq!(
            agent.drain_after_exit(status, &obs).unwrap_err(),
            DecideError::Exited(Some(7)),
            "an exit with nothing pending must carry its status"
        );
    }

    /// The EOF shortcut cannot see this one: a backgrounded grandchild inherits
    /// stdout, so the reader thread never reaches EOF and only the exit poll in
    /// the decide wait can notice the parent died. Unix-only fixture (the shell
    /// job-control idiom); the polled code path itself is platform-neutral and
    /// the ubuntu/macos CI legs run this.
    #[cfg(unix)]
    #[test]
    fn a_dead_child_whose_pipe_is_held_open_is_still_detected() {
        let mut agent = ExternalAgent::spawn("sh", &["-c", "sleep 60 & exit 3"])
            .expect("the platform shell must spawn");
        let start = std::time::Instant::now();
        let decision = agent.decide(&one_symbol_observation());
        assert!(
            start.elapsed() < Duration::from_secs(20),
            "the exit poll must detect the death long before the 30s budget"
        );
        assert!(decision.orders.is_empty(), "a faulted decision is a hold");
        assert_eq!(
            agent.health().last_error,
            Some(DecideError::Exited(Some(3))),
            "with the pipe held open, only the exit poll can classify this"
        );
    }

    #[test]
    fn a_silent_subprocess_times_out_instead_of_hanging_the_harness() {
        let mut agent = spawn_silent_agent().with_decide_timeout(Duration::from_millis(300));
        let obs = MarketObservation {
            date: "2026-01-01".to_string(),
            cash: 1000.0,
            symbols: Vec::new(),
            portfolio: Vec::new(),
        };
        let start = std::time::Instant::now();
        let decision = agent.decide(&obs);
        assert!(
            start.elapsed() < Duration::from_secs(30),
            "the decision must return within the wall-clock budget, not block on the pipe"
        );
        assert!(decision.orders.is_empty(), "a timed-out decision is a hold");
        let health = agent.health();
        assert_eq!(
            health.last_error,
            Some(DecideError::Timeout),
            "the stall is recorded as a timeout fault, not mistaken for a deliberate hold"
        );
        assert!(health.degraded());
    }
}
