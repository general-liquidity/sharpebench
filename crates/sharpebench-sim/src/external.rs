//! External-process agent — speak the JSON protocol to an any-language agent.
//!
//! The adoption surface: an agent is just a subprocess that reads one
//! [`MarketObservation`] (JSON) per line on stdin and writes one [`Decision`]
//! (JSON) per line on stdout. Python, TS, a hosted shim — anything that honors
//! the contract competes. On any I/O or parse error the agent is treated as
//! holding (never panics the harness).

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

use sharpebench_protocol::{Decision, MarketObservation};

use crate::agent::Agent;

/// Drives an external agent subprocess over newline-delimited JSON.
pub struct ExternalAgent {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
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
        })
    }

    fn hold() -> Decision {
        Decision {
            orders: Vec::new(),
            reasoning: "external agent error → hold".to_string(),
        }
    }
}

impl Agent for ExternalAgent {
    fn decide(&mut self, obs: &MarketObservation) -> Decision {
        let Ok(line) = serde_json::to_string(obs) else {
            return Self::hold();
        };
        if writeln!(self.stdin, "{line}").is_err() || self.stdin.flush().is_err() {
            return Self::hold();
        }
        let mut resp = String::new();
        match self.stdout.read_line(&mut resp) {
            Ok(0) | Err(_) => Self::hold(),
            Ok(_) => serde_json::from_str(&resp).unwrap_or_else(|_| Self::hold()),
        }
    }
}

impl Drop for ExternalAgent {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// Drives an external agent over HTTP/1.1 — one request/response per decision.
///
/// Targets a plain-HTTP `host:port` endpoint that accepts `POST /decide` with a
/// JSON [`MarketObservation`] body and returns a JSON [`Decision`]. Loopback /
/// in-sandbox only (no TLS), so this is a dependency-free `std::net` client — the
/// benchmark sim keeps its minimal, audited dependency tree. As with the stdio
/// transport, any connection/parse error degrades to a hold (never panics).
pub struct HttpAgent {
    host: String,
    port: u16,
}

impl HttpAgent {
    /// `addr` is `host:port` (e.g. `"127.0.0.1:8080"`); each decision POSTs to
    /// `/decide`. A bare host defaults to port 80.
    pub fn new(addr: impl Into<String>) -> Self {
        let addr = addr.into();
        match addr.rsplit_once(':') {
            Some((h, p)) => Self {
                host: h.to_string(),
                port: p.parse().unwrap_or(80),
            },
            None => Self {
                host: addr,
                port: 80,
            },
        }
    }

    fn decide_checked(&self, obs: &MarketObservation) -> std::io::Result<Decision> {
        let body = serde_json::to_string(obs).map_err(std::io::Error::other)?;
        let mut stream = TcpStream::connect((self.host.as_str(), self.port))?;
        // `Connection: close` lets us read the whole response to EOF — no need to
        // parse Content-Length / chunked encoding for a one-shot request.
        let req = format!(
            "POST /decide HTTP/1.1\r\nHost: {}\r\nContent-Type: application/json\r\n\
             Content-Length: {}\r\nConnection: close\r\n\r\n{}",
            self.host,
            body.len(),
            body
        );
        stream.write_all(req.as_bytes())?;
        stream.flush()?;
        let mut raw = String::new();
        stream.read_to_string(&mut raw)?;
        let json = raw
            .split_once("\r\n\r\n")
            .map(|(_, b)| b)
            .ok_or_else(|| std::io::Error::other("malformed HTTP response"))?;
        serde_json::from_str(json).map_err(std::io::Error::other)
    }
}

impl Agent for HttpAgent {
    fn decide(&mut self, obs: &MarketObservation) -> Decision {
        self.decide_checked(obs).unwrap_or_else(|_| Decision {
            orders: Vec::new(),
            reasoning: "http agent error → hold".to_string(),
        })
    }
}
