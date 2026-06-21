//! External-process agent — speak the JSON protocol to an any-language agent.
//!
//! The adoption surface: an agent is just a subprocess that reads one
//! [`MarketObservation`] (JSON) per line on stdin and writes one [`Decision`]
//! (JSON) per line on stdout. Python, TS, a hosted shim — anything that honors
//! the contract competes. On any I/O or parse error the agent is treated as
//! holding (never panics the harness).

use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

use sb_protocol::{Decision, MarketObservation};

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
