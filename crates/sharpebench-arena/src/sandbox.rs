//! Sandboxed execution of untrusted external agents.
//!
//! An arena entrant is untrusted code. This module runs it inside a Docker
//! container (`docker run --rm --network none --memory 1g --cpus 1 -i <image>`)
//! speaking the exact stdin/stdout observation/decision protocol that
//! `sharpebench_sim::ExternalAgent` already implements. The container command is
//! the transport process: `ExternalAgent` is wrapped, the protocol is not
//! reimplemented.
//!
//! **Container isolation is the boundary.** `--network none` cuts the agent off
//! from the network, and the memory/cpu caps bound resource abuse; nothing here
//! attempts syscall filtering or further hardening on top of Docker. Host
//! execution of untrusted code remains unsupported: when Docker is absent this
//! module returns a clear error, never a silent unsandboxed fallback. The
//! `allow_unsandboxed` opt-in exists for local development against your OWN
//! agent and defaults to false; it requires an explicit host command as well,
//! so it can never fire by accident.
//!
//! Docker is invoked as the `docker` binary via `std::process` - no Docker
//! client crate, keeping the workspace's audited dependency tree unchanged.

use std::process::Command;

use sharpebench_sim::ExternalAgent;

/// How a sandboxed agent run is configured.
#[derive(Clone, Debug, Default)]
pub struct SandboxOptions {
    /// Explicit opt-in to run WITHOUT a sandbox when Docker is unavailable.
    /// Defaults to false: no Docker means a hard error. Requires
    /// `unsandboxed_command` to be set as well.
    pub allow_unsandboxed: bool,
    /// The host command (program + args) to run when `allow_unsandboxed` is
    /// set and Docker is absent. Local development only.
    pub unsandboxed_command: Option<Vec<String>>,
}

/// Why a sandboxed run could not start.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SandboxError {
    /// Docker is not available and unsandboxed execution was not (fully)
    /// opted into. The payload says what to do about it.
    DockerUnavailable(String),
    /// The agent process failed to spawn.
    Spawn(String),
}

impl std::fmt::Display for SandboxError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SandboxError::DockerUnavailable(msg) => write!(f, "docker unavailable: {msg}"),
            SandboxError::Spawn(msg) => write!(f, "cannot spawn agent: {msg}"),
        }
    }
}

impl std::error::Error for SandboxError {}

/// The launch decision, split out from process spawning so the refusal logic is
/// deterministic and testable without Docker installed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Launch {
    /// `docker run --rm --network none --memory 1g --cpus 1 -i <image>`.
    Docker { program: String, args: Vec<String> },
    /// The opted-in local-dev host command. NOT a sandbox.
    Unsandboxed { program: String, args: Vec<String> },
}

/// Is the Docker CLI present and answering? Checked by running `docker version`.
pub fn docker_available() -> bool {
    Command::new("docker")
        .arg("version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// Decide how (or whether) to launch `image`, given whether Docker is present.
/// With Docker: the sandboxed container command. Without Docker: a hard
/// [`SandboxError::DockerUnavailable`] unless BOTH `allow_unsandboxed` and an
/// explicit host command were supplied. There is no silent fallback path.
pub fn resolve_launch(
    docker_present: bool,
    image: &str,
    opts: &SandboxOptions,
) -> Result<Launch, SandboxError> {
    if docker_present {
        return Ok(Launch::Docker {
            program: "docker".to_string(),
            args: [
                "run",
                "--rm",
                "--network",
                "none",
                "--memory",
                "1g",
                "--cpus",
                "1",
                "-i",
                image,
            ]
            .iter()
            .map(|s| s.to_string())
            .collect(),
        });
    }
    if !opts.allow_unsandboxed {
        return Err(SandboxError::DockerUnavailable(
            "install Docker to run untrusted agents sandboxed; host execution of untrusted \
             code is unsupported (allow_unsandboxed + an explicit host command exist for \
             local dev against your own agent only)"
                .to_string(),
        ));
    }
    match &opts.unsandboxed_command {
        Some(cmd) if !cmd.is_empty() => Ok(Launch::Unsandboxed {
            program: cmd[0].clone(),
            args: cmd[1..].to_vec(),
        }),
        _ => Err(SandboxError::DockerUnavailable(
            "allow_unsandboxed is set but no unsandboxed_command was supplied; refusing to \
             guess a host command"
                .to_string(),
        )),
    }
}

/// Run an external agent sandboxed in Docker, returning the live
/// [`ExternalAgent`] transport (drive it with the harness exactly like any
/// other external agent). See the module docs for the isolation contract.
pub fn run_external_sandboxed(
    image: &str,
    opts: &SandboxOptions,
) -> Result<ExternalAgent, SandboxError> {
    let launch = resolve_launch(docker_available(), image, opts)?;
    let (program, args) = match &launch {
        Launch::Docker { program, args } | Launch::Unsandboxed { program, args } => {
            (program.as_str(), args)
        }
    };
    let arg_refs: Vec<&str> = args.iter().map(String::as_str).collect();
    ExternalAgent::spawn(program, &arg_refs).map_err(|e| SandboxError::Spawn(e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unsandboxed_is_opt_in_and_defaults_off() {
        let opts = SandboxOptions::default();
        assert!(!opts.allow_unsandboxed);
        // No Docker + no opt-in = a hard, explanatory error. Never a fallback.
        let err = resolve_launch(false, "some/image", &opts).unwrap_err();
        assert!(matches!(err, SandboxError::DockerUnavailable(_)));
    }

    #[test]
    fn opt_in_without_a_command_still_refuses() {
        let opts = SandboxOptions {
            allow_unsandboxed: true,
            unsandboxed_command: None,
        };
        assert!(matches!(
            resolve_launch(false, "some/image", &opts),
            Err(SandboxError::DockerUnavailable(_))
        ));
    }

    #[test]
    fn opt_in_with_a_command_runs_that_command_unsandboxed() {
        let opts = SandboxOptions {
            allow_unsandboxed: true,
            unsandboxed_command: Some(vec!["python".to_string(), "agent.py".to_string()]),
        };
        assert_eq!(
            resolve_launch(false, "some/image", &opts).unwrap(),
            Launch::Unsandboxed {
                program: "python".to_string(),
                args: vec!["agent.py".to_string()],
            }
        );
    }

    #[test]
    fn docker_present_always_launches_the_exact_container_command() {
        // Even with the opt-in set, Docker present means the sandbox is used.
        let opts = SandboxOptions {
            allow_unsandboxed: true,
            unsandboxed_command: Some(vec!["python".to_string()]),
        };
        let Launch::Docker { program, args } =
            resolve_launch(true, "sharpebench/agent:1", &opts).unwrap()
        else {
            panic!("expected the Docker launch");
        };
        assert_eq!(program, "docker");
        assert_eq!(
            args,
            vec![
                "run",
                "--rm",
                "--network",
                "none",
                "--memory",
                "1g",
                "--cpus",
                "1",
                "-i",
                "sharpebench/agent:1"
            ]
        );
    }

    /// Live Docker smoke test: spawns a real container and drives one decision
    /// through the wrapped `ExternalAgent` transport. Skipped (with a message)
    /// when Docker is absent, so CI without Docker stays green and honest.
    #[test]
    fn docker_spawn_smoke() {
        if !docker_available() {
            eprintln!("SKIP docker_spawn_smoke: docker is not available on this machine");
            return;
        }
        use sharpebench_sim::Agent;
        let mut agent = run_external_sandboxed("alpine", &SandboxOptions::default())
            .expect("docker is available, so the sandboxed spawn must work");
        let obs = sharpebench_protocol_obs();
        // alpine's default entrypoint does not speak the protocol; the transport
        // must surface that as a flagged hold, not a hang or a panic.
        let decision = agent.decide(&obs);
        assert!(decision.orders.is_empty());
    }

    #[cfg(test)]
    fn sharpebench_protocol_obs() -> sharpebench_protocol::MarketObservation {
        sharpebench_protocol::MarketObservation {
            date: "2026-01-01".to_string(),
            cash: 1000.0,
            symbols: Vec::new(),
            portfolio: Vec::new(),
        }
    }
}
