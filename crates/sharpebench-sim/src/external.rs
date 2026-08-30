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
use std::time::Duration;

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

impl ExternalAgent {
    /// Spawn `program args...` as an agent subprocess.
    pub fn spawn(program: &str, args: &[&str]) -> std::io::Result<Self> {
        let mut command = Command::new(program);
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

    /// One decision attempt over the stdio pipe, returning a typed [`DecideError`]
    /// rather than degrading to a hold. A closed stdout (EOF) or a broken pipe is a
    /// transport fault; unparseable output is the agent's protocol fault; a
    /// subprocess that stays silent past [`STDIO_DECIDE_TIMEOUT`] is a timeout.
    fn decide_once(&mut self, obs: &MarketObservation) -> Result<Decision, DecideError> {
        let line = serde_json::to_string(obs).map_err(|_| DecideError::Transport)?;
        writeln!(self.stdin, "{line}").map_err(|e| classify_io(&e))?;
        self.stdin.flush().map_err(|e| classify_io(&e))?;
        match self.lines.recv_timeout(self.timeout) {
            Ok(Ok(Wire::Line(resp))) => parse_decision(&resp, obs),
            Ok(Ok(Wire::Oversized)) => {
                eprintln!(
                    "agent protocol fault: one decision exceeded the {MAX_AGENT_LINE}-byte line \
                     budget without a newline; the contract is one JSON decision per line"
                );
                Err(DecideError::Oversized)
            }
            Ok(Err(_)) => Err(DecideError::Transport),
            Err(RecvTimeoutError::Timeout) => Err(DecideError::Timeout),
            Err(RecvTimeoutError::Disconnected) => Err(DecideError::Transport),
        }
    }
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
        // The marker makes the grandchild identifiable in `ps` without matching
        // any other sleeper this suite runs in parallel.
        let marker = format!("sharpebench-teardown-{}", std::process::id());
        let script = format!("sh -c 'sleep 30' {marker} & read line; exit 0");

        // Counting in-process rather than through `grep` keeps the search pattern
        // out of the process table it is searching, which is the classic way this
        // kind of check ends up matching itself and reporting nonsense.
        let alive = |marker: &str| {
            let out = Command::new("ps").args(["-eo", "args"]).output();
            String::from_utf8_lossy(&out.expect("ps must run").stdout)
                .lines()
                .filter(|line| line.contains(marker))
                .count()
        };

        let agent =
            ExternalAgent::spawn("sh", &["-c", &script]).expect("the platform shell must spawn");
        std::thread::sleep(Duration::from_millis(300));
        assert!(
            alive(&marker) >= 2,
            "the fixture must actually have a live grandchild, or the assertion \
             below would hold for a test that proved nothing"
        );

        drop(agent);
        std::thread::sleep(Duration::from_millis(300));
        assert_eq!(
            alive(&marker),
            0,
            "the backgrounded grandchild outlived the teardown, so it is still \
             holding the stdout pipe open"
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
