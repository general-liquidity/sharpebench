//! Sandboxed execution of untrusted external agents.
//!
//! An arena entrant is untrusted code. This module runs it inside a Docker
//! container with no network, a read-only root, bounded tmpfs scratch space,
//! non-root identity, no Linux capabilities, no-new-privileges, and CPU/RAM/PID
//! limits. Images are digest-pinned by default while speaking the exact
//! stdin/stdout observation/decision protocol that
//! `sharpebench_sim::ExternalAgent` already implements. The container command is
//! the transport process: `ExternalAgent` is wrapped, the protocol is not
//! reimplemented.
//!
//! **Container isolation is the boundary.** Docker's default seccomp profile is
//! retained and the launch removes ambient privilege and writable host state. Host
//! execution of untrusted code remains unsupported: when Docker is absent this
//! module returns a clear error, never a silent unsandboxed fallback. The
//! `allow_unsandboxed` opt-in exists for local development against your OWN
//! agent and defaults to false; it requires an explicit host command as well,
//! so it can never fire by accident.
//!
//! Docker is invoked as the `docker` binary via `std::process` - no Docker
//! client crate, keeping the workspace's audited dependency tree unchanged.

use std::process::{Command, Output, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use serde::Serialize;

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
    /// Development-only escape hatch for mutable image tags. Field runs require
    /// `<repository>@sha256:<64 lowercase hex>` by default so the artifact that
    /// executed is the artifact that was committed.
    pub allow_unpinned_image: bool,
}

/// Why a sandboxed run could not start.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SandboxError {
    /// Docker is not available and unsandboxed execution was not (fully)
    /// opted into. The payload says what to do about it.
    DockerUnavailable(String),
    /// The agent process failed to spawn.
    Spawn(String),
    /// The container artifact or launch option is unsafe/ambiguous.
    InvalidConfig(String),
    /// Docker ran, but the hardened boundary failed a readiness probe.
    Readiness(String),
}

impl std::fmt::Display for SandboxError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SandboxError::DockerUnavailable(msg) => write!(f, "docker unavailable: {msg}"),
            SandboxError::Spawn(msg) => write!(f, "cannot spawn agent: {msg}"),
            SandboxError::InvalidConfig(msg) => write!(f, "invalid sandbox config: {msg}"),
            SandboxError::Readiness(msg) => write!(f, "sandbox is not field-ready: {msg}"),
        }
    }
}

impl std::error::Error for SandboxError {}

/// The launch decision, split out from process spawning so the refusal logic is
/// deterministic and testable without Docker installed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Launch {
    /// A hardened `docker run` invocation ending in the pinned image.
    Docker { program: String, args: Vec<String> },
    /// The opted-in local-dev host command. NOT a sandbox.
    Unsandboxed { program: String, args: Vec<String> },
}

/// Successful, live verification of the field sandbox boundary.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct SandboxReadiness {
    /// Digest-pinned image reference used for the hostile probe.
    pub image: String,
    /// Docker's immutable image ID for the locally present artifact.
    pub image_id: String,
    /// Docker daemon version that enforced the boundary.
    pub docker_server_version: String,
    /// Hostile checks that the container passed.
    pub passed_checks: Vec<String>,
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
        validate_image(image, opts.allow_unpinned_image)?;
        return Ok(Launch::Docker {
            program: "docker".to_string(),
            args: hardened_docker_args(image),
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

fn hardened_docker_args(image: &str) -> Vec<String> {
    [
        "run",
        "--rm",
        "--init",
        "--pull",
        "never",
        "--network",
        "none",
        "--ipc",
        "none",
        "--read-only",
        "--cap-drop",
        "ALL",
        "--security-opt",
        "no-new-privileges=true",
        "--user",
        "65532:65532",
        "--memory",
        "1g",
        "--memory-swap",
        "1g",
        "--cpus",
        "1",
        "--pids-limit",
        "128",
        "--ulimit",
        "nofile=256:256",
        "--tmpfs",
        "/tmp:rw,noexec,nosuid,nodev,size=64m,mode=1777",
        "--tmpfs",
        "/run:rw,noexec,nosuid,nodev,size=16m,mode=1777",
        "--log-driver",
        "none",
        "-i",
        image,
    ]
    .iter()
    .map(|value| (*value).to_string())
    .collect()
}

const READINESS_TIMEOUT: Duration = Duration::from_secs(30);

const HOSTILE_PROBE: &str = r#"
set -eu
[ "$(id -u)" = "65532" ]
[ "$(awk '/^CapEff:/ {print $2}' /proc/self/status)" = "0000000000000000" ]
[ "$(awk '/^NoNewPrivs:/ {print $2}' /proc/self/status)" = "1" ]
[ "$(ls -1 /sys/class/net)" = "lo" ]
awk '$5 == "/" { if ($6 !~ /(^|,)ro(,|$)/) exit 1; found=1 } END { if (!found) exit 1 }' /proc/self/mountinfo
if touch /etc/sharpebench-root-write 2>/dev/null; then exit 41; fi
touch /tmp/sharpebench-write-ok
printf '#!/bin/sh\nexit 0\n' > /tmp/sharpebench-exec-denied
chmod +x /tmp/sharpebench-exec-denied
if /tmp/sharpebench-exec-denied 2>/dev/null; then exit 42; fi
touch /run/sharpebench-write-ok
"#;

fn command_output_with_timeout(
    program: &str,
    args: &[String],
    timeout: Duration,
) -> Result<Output, String> {
    let mut child = Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| format!("cannot start {program}: {error}"))?;
    let deadline = Instant::now() + timeout;
    loop {
        match child.try_wait() {
            Ok(Some(_)) => {
                return child
                    .wait_with_output()
                    .map_err(|error| format!("cannot collect {program} output: {error}"));
            }
            Ok(None) if Instant::now() < deadline => {
                thread::sleep(Duration::from_millis(25));
            }
            Ok(None) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!(
                    "{program} exceeded the {}s readiness timeout",
                    timeout.as_secs()
                ));
            }
            Err(error) => return Err(format!("cannot poll {program}: {error}")),
        }
    }
}

fn docker_output(args: &[&str]) -> Result<Output, SandboxError> {
    let owned: Vec<String> = args.iter().map(|value| (*value).to_string()).collect();
    command_output_with_timeout("docker", &owned, READINESS_TIMEOUT)
        .map_err(SandboxError::DockerUnavailable)
}

/// Prove that the live Docker boundary is ready for a field run.
///
/// Unlike the ordinary Docker smoke test, this check never skips. It requires a
/// locally present, digest-pinned POSIX fixture image with `/bin/sh`, `id`,
/// `awk`, `ls`, `touch`, and `chmod`, then runs hostile attempts against every
/// boundary field execution relies on. No image is pulled (`--pull never`).
pub fn check_sandbox_readiness(image: &str) -> Result<SandboxReadiness, SandboxError> {
    validate_image(image, false)?;

    let version = docker_output(&["version", "--format", "{{.Server.Version}}"])?;
    if !version.status.success() {
        return Err(SandboxError::DockerUnavailable(
            String::from_utf8_lossy(&version.stderr).trim().to_string(),
        ));
    }
    let docker_server_version = String::from_utf8_lossy(&version.stdout).trim().to_string();
    if docker_server_version.is_empty() {
        return Err(SandboxError::DockerUnavailable(
            "Docker returned no server version".to_string(),
        ));
    }

    let inspected = docker_output(&["image", "inspect", "--format", "{{.Id}}", image])?;
    if !inspected.status.success() {
        return Err(SandboxError::Readiness(format!(
            "the pinned fixture image is not present locally: {}",
            String::from_utf8_lossy(&inspected.stderr).trim()
        )));
    }
    let image_id = String::from_utf8_lossy(&inspected.stdout)
        .trim()
        .to_string();
    if !image_id.starts_with("sha256:") {
        return Err(SandboxError::Readiness(
            "Docker returned an invalid image ID".to_string(),
        ));
    }

    let mut args = hardened_docker_args(image);
    args.extend([
        "/bin/sh".to_string(),
        "-ceu".to_string(),
        HOSTILE_PROBE.to_string(),
    ]);
    let probe = command_output_with_timeout("docker", &args, READINESS_TIMEOUT)
        .map_err(SandboxError::Readiness)?;
    if !probe.status.success() {
        let stderr = String::from_utf8_lossy(&probe.stderr);
        return Err(SandboxError::Readiness(format!(
            "hostile fixture exited with {}: {}",
            probe.status,
            stderr.trim()
        )));
    }

    Ok(SandboxReadiness {
        image: image.to_string(),
        image_id,
        docker_server_version,
        passed_checks: [
            "non-root uid",
            "zero Linux capabilities",
            "no-new-privileges",
            "network namespace exposes loopback only",
            "root filesystem is read-only",
            "bounded tmpfs is writable",
            "tmpfs is noexec",
        ]
        .iter()
        .map(|value| (*value).to_string())
        .collect(),
    })
}

fn validate_image(image: &str, allow_unpinned: bool) -> Result<(), SandboxError> {
    if image.is_empty()
        || image.starts_with('-')
        || image.chars().any(|character| character.is_whitespace())
    {
        return Err(SandboxError::InvalidConfig(
            "image must be one non-option Docker image reference".to_string(),
        ));
    }
    if allow_unpinned {
        return Ok(());
    }
    let Some((repository, digest)) = image.rsplit_once("@sha256:") else {
        return Err(SandboxError::InvalidConfig(
            "field images must be pinned as <repository>@sha256:<64 lowercase hex>; set \
             allow_unpinned_image only for a local development smoke test"
                .to_string(),
        ));
    };
    if repository.is_empty()
        || digest.len() != 64
        || !digest
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    {
        return Err(SandboxError::InvalidConfig(
            "image digest must be 64 lowercase hexadecimal characters".to_string(),
        ));
    }
    Ok(())
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
            allow_unpinned_image: false,
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
            allow_unpinned_image: false,
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
            allow_unpinned_image: false,
        };
        let digest = "0".repeat(64);
        let image = format!("sharpebench/agent@sha256:{digest}");
        let Launch::Docker { program, args } = resolve_launch(true, &image, &opts).unwrap() else {
            panic!("expected the Docker launch");
        };
        assert_eq!(program, "docker");
        assert_eq!(
            args,
            vec![
                "run",
                "--rm",
                "--init",
                "--pull",
                "never",
                "--network",
                "none",
                "--ipc",
                "none",
                "--read-only",
                "--cap-drop",
                "ALL",
                "--security-opt",
                "no-new-privileges=true",
                "--user",
                "65532:65532",
                "--memory",
                "1g",
                "--memory-swap",
                "1g",
                "--cpus",
                "1",
                "--pids-limit",
                "128",
                "--ulimit",
                "nofile=256:256",
                "--tmpfs",
                "/tmp:rw,noexec,nosuid,nodev,size=64m,mode=1777",
                "--tmpfs",
                "/run:rw,noexec,nosuid,nodev,size=16m,mode=1777",
                "--log-driver",
                "none",
                "-i",
                image.as_str()
            ]
        );
    }

    #[test]
    fn docker_refuses_mutable_or_option_like_images_by_default() {
        assert!(matches!(
            resolve_launch(true, "agent:latest", &SandboxOptions::default()),
            Err(SandboxError::InvalidConfig(_))
        ));
        assert!(matches!(
            resolve_launch(true, "--privileged", &SandboxOptions::default()),
            Err(SandboxError::InvalidConfig(_))
        ));
        let opts = SandboxOptions {
            allow_unpinned_image: true,
            ..SandboxOptions::default()
        };
        assert!(matches!(
            resolve_launch(true, "agent:dev", &opts),
            Ok(Launch::Docker { .. })
        ));
    }

    #[test]
    fn field_readiness_requires_a_pinned_fixture_before_contacting_docker() {
        assert!(matches!(
            check_sandbox_readiness("alpine:latest"),
            Err(SandboxError::InvalidConfig(_))
        ));
    }

    #[test]
    fn hostile_probe_runs_inside_the_same_hardened_boundary() {
        let image = format!("fixture@sha256:{}", "a".repeat(64));
        let mut args = hardened_docker_args(&image);
        args.extend([
            "/bin/sh".to_string(),
            "-ceu".to_string(),
            HOSTILE_PROBE.to_string(),
        ]);
        assert!(args.windows(2).any(|pair| pair == ["--network", "none"]));
        assert!(args
            .windows(2)
            .any(|pair| pair == ["--read-only", "--cap-drop"]));
        assert!(args.iter().any(|arg| arg == "no-new-privileges=true"));
        assert!(args.iter().any(|arg| arg.contains("noexec")));
        assert!(args
            .last()
            .unwrap()
            .contains("touch /etc/sharpebench-root-write"));
    }

    /// The digest-pinned POSIX fixture image the live tests below run against,
    /// supplied by the Docker-enabled CI job. Requiring it (rather than
    /// defaulting to a mutable tag) keeps the live boundary tests honest about
    /// which artifact actually executed.
    fn live_fixture_image() -> String {
        std::env::var("SHARPEBENCH_SANDBOX_FIXTURE").expect(
            "the live sandbox tests need SHARPEBENCH_SANDBOX_FIXTURE set to a digest-pinned \
             POSIX fixture image that is already present locally (--pull never)",
        )
    }

    /// The hostile probe, executed for real.
    ///
    /// `#[ignore]` rather than a runtime skip: a skip that returns green is
    /// indistinguishable from a pass in the `cargo test` summary, which is what
    /// let the container boundary go unverified while the suite reported
    /// "0 ignored". Ignored tests are counted and named, so an operator can see
    /// that the boundary was not exercised. CI runs them with
    /// `cargo test -p sharpebench-arena -- --ignored` on a Docker-enabled runner.
    #[test]
    #[ignore = "needs a running Docker daemon and SHARPEBENCH_SANDBOX_FIXTURE"]
    fn live_hostile_probe_passes_inside_the_hardened_boundary() {
        assert!(
            docker_available(),
            "this test was requested explicitly with --ignored, so an absent Docker daemon is a \
             failure, not a reason to pass"
        );
        let image = live_fixture_image();
        let readiness = check_sandbox_readiness(&image)
            .unwrap_or_else(|error| panic!("the hardened boundary is not field-ready: {error}"));
        assert_eq!(readiness.image, image);
        assert!(readiness.image_id.starts_with("sha256:"));
        assert!(!readiness.docker_server_version.is_empty());
        // Every boundary the probe asserts on must be reported as passed; a
        // shorter list means a check was quietly dropped.
        assert_eq!(readiness.passed_checks.len(), 7, "{readiness:?}");
        for expected in [
            "non-root uid",
            "zero Linux capabilities",
            "no-new-privileges",
            "network namespace exposes loopback only",
            "root filesystem is read-only",
            "bounded tmpfs is writable",
            "tmpfs is noexec",
        ] {
            assert!(
                readiness
                    .passed_checks
                    .iter()
                    .any(|check| check == expected),
                "{expected:?} missing from {:?}",
                readiness.passed_checks
            );
        }
    }

    /// Live Docker smoke test: spawns a real container through the production
    /// entry point and drives one decision over the wrapped `ExternalAgent`
    /// transport.
    ///
    /// The old assertion was `decision.orders.is_empty()`, which holds for any
    /// failure mode including a container that never started, because
    /// `error_hold` also returns no orders. What distinguishes the two is the
    /// transport health: a container that never started closes stdout at once
    /// and the reader channel disconnects (`DecideError::Transport`), while a
    /// container that *ran* and simply does not speak the protocol keeps
    /// consuming stdin and stays silent (`DecideError::Timeout`). This test
    /// requires the latter, so it fails if the image did not execute.
    #[test]
    #[ignore = "needs a running Docker daemon and SHARPEBENCH_SANDBOX_FIXTURE"]
    fn docker_spawn_smoke() {
        assert!(
            docker_available(),
            "this test was requested explicitly with --ignored, so an absent Docker daemon is a \
             failure, not a reason to pass"
        );
        use sharpebench_sim::{Agent, DecideError, TransportDiagnostics};
        let image = live_fixture_image();
        let mut agent = run_external_sandboxed(&image, &SandboxOptions::default())
            .expect("docker is available and the image is pinned, so the spawn must work");
        agent = agent.with_decide_timeout(Duration::from_secs(5));
        let obs = sharpebench_protocol_obs();
        let decision = agent.decide(&obs);
        assert!(decision.orders.is_empty(), "a faulted decision is a hold");
        let health = agent.health();
        assert!(
            health.degraded(),
            "the fault must be flagged, not mistaken for a deliberate hold"
        );
        assert_eq!(
            health.last_error,
            Some(DecideError::Timeout),
            "the container must be alive and silent (it ran but does not speak the protocol); \
             a Transport fault here means it never started"
        );
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
