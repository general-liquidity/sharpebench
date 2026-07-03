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
//! [`sharpebench_harness`] `FailureKind` rather than a masked hold. When a decision
//! still cannot be produced the call returns an empty-orders hold (the trait cannot
//! signal an error), but that hold is now *flagged* in the health - the harness no
//! longer mistakes it for a deliberate one.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

use sharpebench_protocol::{Decision, MarketObservation};

use crate::agent::Agent;
use crate::transport::{
    decide_with_retry, CircuitBreaker, DecideError, TransportDiagnostics, TransportHealth,
};

/// Cap on bytes read from an external agent's HTTP response, so a hostile or buggy
/// endpoint can't exhaust the harness's memory.
const MAX_AGENT_RESPONSE: u64 = 8 * 1024 * 1024;

/// Default consecutive-fault threshold before an endpoint's circuit breaker trips.
const DEFAULT_BREAKER_THRESHOLD: u32 = 3;

/// Default bounded per-decision retries on a transient HTTP transport blip.
const DEFAULT_HTTP_RETRIES: u32 = 2;

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

/// Drives an external agent subprocess over newline-delimited JSON.
pub struct ExternalAgent {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
    breaker: CircuitBreaker,
    health: TransportHealth,
}

impl ExternalAgent {
    /// Spawn `program args...` as an agent subprocess.
    pub fn spawn(program: &str, args: &[&str]) -> std::io::Result<Self> {
        let mut child = Command::new(program)
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()?;
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| std::io::Error::other("no stdin"))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| std::io::Error::other("no stdout"))?;
        Ok(Self {
            child,
            stdin,
            stdout: BufReader::new(stdout),
            breaker: CircuitBreaker::new(DEFAULT_BREAKER_THRESHOLD),
            health: TransportHealth::default(),
        })
    }

    /// One decision attempt over the stdio pipe, returning a typed [`DecideError`]
    /// rather than degrading to a hold. A closed stdout (EOF) or a broken pipe is a
    /// transport fault; unparseable output is the agent's protocol fault.
    fn decide_once(&mut self, obs: &MarketObservation) -> Result<Decision, DecideError> {
        let line = serde_json::to_string(obs).map_err(|_| DecideError::Transport)?;
        writeln!(self.stdin, "{line}").map_err(|e| classify_io(&e))?;
        self.stdin.flush().map_err(|e| classify_io(&e))?;
        let mut resp = String::new();
        match self.stdout.read_line(&mut resp) {
            Ok(0) => Err(DecideError::Transport),
            Ok(_) => serde_json::from_str(&resp).map_err(|_| DecideError::Protocol),
            Err(e) => Err(classify_io(&e)),
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

impl Drop for ExternalAgent {
    fn drop(&mut self) {
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
        serde_json::from_str(json).map_err(|_| DecideError::Protocol)
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
