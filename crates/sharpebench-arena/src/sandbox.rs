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
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread;
use std::time::{Duration, Instant};

use serde::Serialize;

use sharpebench_sim::{Agent, ExternalAgent, TransportDiagnostics, TransportHealth};

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
    /// A named container's post-exit state could not be read or the container
    /// could not be removed. Continuing would either lose a scoring-relevant
    /// resource fact or silently weaken the no-leak lifecycle contract.
    Inspection(String),
}

impl std::fmt::Display for SandboxError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SandboxError::DockerUnavailable(msg) => write!(f, "docker unavailable: {msg}"),
            SandboxError::Spawn(msg) => write!(f, "cannot spawn agent: {msg}"),
            SandboxError::InvalidConfig(msg) => write!(f, "invalid sandbox config: {msg}"),
            SandboxError::Readiness(msg) => write!(f, "sandbox is not field-ready: {msg}"),
            SandboxError::Inspection(msg) => write!(f, "sandbox finalization failed: {msg}"),
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
            args: hardened_docker_args(image, &Retention::AutoRemove),
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

/// How the launched container's lifetime and post-exit state are managed.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Retention {
    /// `--rm`: the daemon removes the container the instant it exits. Used by
    /// the readiness probes, which need no post-exit state.
    AutoRemove,
    /// A named container that is **not** auto-removed, so post-exit state
    /// (`State.OOMKilled`) is still inspectable after the wait. The caller owns
    /// explicit removal — [`SandboxedAgent`] does it in `finish` / `Drop`, which
    /// is what preserves the no-leak property `--rm` provided.
    Inspectable(String),
}

/// Monotonic per-process counter, so two agents spawned in the same nanosecond
/// still get distinct container names.
static CONTAINER_SEQ: AtomicU64 = AtomicU64::new(0);

/// A container name unique across processes (pid) and within one (counter).
fn fresh_container_name() -> String {
    format!(
        "sharpebench-agent-{}-{}",
        std::process::id(),
        CONTAINER_SEQ.fetch_add(1, Ordering::Relaxed)
    )
}

/// A hardened `docker run` invocation with the flags and the trailing image
/// positional held **separately** until assembly.
///
/// The previous shape was one flat argument list ending `"-i", image`, with
/// callers `extend`ing after it — correct only because the image happened to be
/// last. Anyone appending a hardening flag to that list would silently turn it
/// into a *container command* argument (everything after the image positional
/// belongs to the entrant, not to Docker). Holding the flags and the positional
/// apart makes the ordering structural: a flag pushed onto `flags` at any point
/// lands before the image, and a container command can only land after it.
struct HardenedLaunch {
    /// Everything before the image: `run`, the retention choice, the hardening
    /// flags, `-i`. Append further Docker flags here — never a positional.
    flags: Vec<String>,
    /// The trailing image positional. Assembly places it last (or ahead of an
    /// explicit container command), whatever `flags` holds.
    image: String,
}

impl HardenedLaunch {
    fn new(image: &str, retention: &Retention) -> Self {
        Self::new_with_memory(image, retention, "1g")
    }

    /// The production launch uses 1 GiB. A live OOM acceptance test needs a
    /// deliberately small budget so it proves the kernel verdict without asking a CI
    /// runner to waste a gigabyte merely to cross the boundary.
    fn new_with_memory(image: &str, retention: &Retention, memory: &str) -> Self {
        let mut flags: Vec<String> = vec!["run".to_string()];
        match retention {
            Retention::AutoRemove => flags.push("--rm".to_string()),
            Retention::Inspectable(name) => flags.extend(["--name".to_string(), name.clone()]),
        }
        flags.extend(
            [
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
                memory,
                "--memory-swap",
                memory,
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
            ]
            .iter()
            .map(|value| (*value).to_string()),
        );
        Self {
            flags,
            image: image.to_string(),
        }
    }

    /// Assemble `docker run <flags> <image>` — the agent launch, where the
    /// image's own entrypoint is the container command.
    fn into_args(self) -> Vec<String> {
        let mut args = self.flags;
        args.push(self.image);
        args
    }

    /// Assemble `docker run <flags> <image> <command...>` — the probe launch,
    /// where the container command is explicit. The command can only ever land
    /// after the image positional.
    fn into_args_with_command<I>(self, command: I) -> Vec<String>
    where
        I: IntoIterator<Item = String>,
    {
        let mut args = self.into_args();
        args.extend(command);
        args
    }
}

fn hardened_docker_args(image: &str, retention: &Retention) -> Vec<String> {
    HardenedLaunch::new(image, retention).into_args()
}

const READINESS_TIMEOUT: Duration = Duration::from_secs(30);

/// Longest an outbound connect may take and still count as a policy denial.
///
/// A refusal with no route out returns in microseconds; a packet that is dropped
/// somewhere between here and the destination burns the client timeout instead.
/// The two are the same exit status, so elapsed time is the only thing that
/// separates "the boundary refused this" from "the network happened to be
/// broken", and a test that asserted only on the status would pass on the
/// second. The budget is measured net of container startup (see
/// [`check_sandbox_readiness`]), which is why it can be this tight.
const EGRESS_DENIAL_MAX: Duration = Duration::from_millis(1500);

/// Exit status a POSIX shell uses for a command it could not find.
const EXIT_COMMAND_NOT_FOUND: i32 = 127;
/// Exit status reserved by the probe for a container whose network namespace
/// exposes anything beyond loopback. A fast refused TCP connection in such a
/// namespace is not evidence of policy enforcement: the remote service may simply
/// have sent RST.
const EXIT_NETWORK_NAMESPACE_OPEN: i32 = 126;

/// A destination class the container boundary must refuse.
///
/// One outbound probe against one routable address proves one thing: that this
/// image, on this daemon, could not reach the public internet. It says nothing
/// about the destinations that actually matter when a boundary is misconfigured,
/// which are not on the public internet at all. Each class below is a distinct
/// failure mode with a distinct blast radius, so each is probed on its own rather
/// than being assumed to follow from the others.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EgressTarget {
    /// The cloud instance-metadata service. The credential-theft path: on a
    /// benchmark runner in EC2/GCE this address hands out the runner's own role
    /// credentials to anything that can reach it, and it is reachable without
    /// any routing beyond the link.
    CloudMetadata,
    /// Another address in the same `169.254.0.0/16` link-local block. The
    /// metadata address is the famous one, not the only one: a boundary that
    /// special-cases `169.254.169.254` and leaves the rest of the block routable
    /// has closed a string match, not a network.
    LinkLocalRange,
    /// The guest's own `127.0.0.1` on a port a live listener holds **on the
    /// host**. Inside its own network namespace loopback means the container
    /// itself, and the connect must be refused; a gateway that forwards it
    /// instead reaches the host's debuggers, local databases and Docker socket.
    HostLoopback(u16),
    /// RFC1918 `10.0.0.0/8`: lateral movement to whatever else is on the
    /// operator's network.
    PrivateClassA,
    /// RFC1918 `172.16.0.0/12`, which is also where Docker's own default bridge
    /// networks live.
    PrivateClassB,
    /// RFC1918 `192.168.0.0/16`: the home / small-office LAN the operator's
    /// laptop is usually on.
    PrivateClassC,
    /// A routable public address. The class the original single probe covered.
    PublicInternet,
}

/// Every destination class whose probe needs nothing but the container, in the
/// order a report lists them. [`EgressTarget::HostLoopback`] is deliberately
/// absent: its probe is meaningless without a live host listener to aim at, so it
/// carries that listener's port and is built per run.
pub const EGRESS_DENY_CLASSES: [EgressTarget; 6] = [
    EgressTarget::CloudMetadata,
    EgressTarget::LinkLocalRange,
    EgressTarget::PrivateClassA,
    EgressTarget::PrivateClassB,
    EgressTarget::PrivateClassC,
    EgressTarget::PublicInternet,
];

impl EgressTarget {
    /// What this class is, for a readiness report or a failure message.
    pub fn class(&self) -> &'static str {
        match self {
            EgressTarget::CloudMetadata => "cloud instance metadata",
            EgressTarget::LinkLocalRange => "link-local range",
            EgressTarget::HostLoopback(_) => "host loopback",
            EgressTarget::PrivateClassA => "private 10.0.0.0/8",
            EgressTarget::PrivateClassB => "private 172.16.0.0/12",
            EgressTarget::PrivateClassC => "private 192.168.0.0/16",
            EgressTarget::PublicInternet => "public internet",
        }
    }

    /// The literal `address:port` the probe connects to. Always numeric: a name
    /// would make the result depend on resolution, and a namespace with no route
    /// out fails to resolve before it ever fails to connect, which is a different
    /// fact than the one under test.
    pub fn destination(&self) -> String {
        match self {
            EgressTarget::CloudMetadata => "169.254.169.254:80".to_string(),
            EgressTarget::LinkLocalRange => "169.254.42.1:80".to_string(),
            EgressTarget::HostLoopback(port) => format!("127.0.0.1:{port}"),
            EgressTarget::PrivateClassA => "10.0.0.1:80".to_string(),
            EgressTarget::PrivateClassB => "172.16.0.1:80".to_string(),
            EgressTarget::PrivateClassC => "192.168.0.1:80".to_string(),
            EgressTarget::PublicInternet => "1.1.1.1:80".to_string(),
        }
    }

    /// Attempt one outbound connection to this class. `wget` is in the busybox
    /// userland every POSIX fixture image ships; its absence exits 127 and is
    /// reported as such rather than being counted as a denial, because a probe
    /// that never ran proves nothing about the boundary.
    fn script(&self) -> String {
        format!(
            "set -eu\n\
             [ \"$(ls -1 /sys/class/net)\" = \"lo\" ] || exit {EXIT_NETWORK_NAMESPACE_OPEN}\n\
             command -v wget >/dev/null 2>&1 || exit {EXIT_COMMAND_NOT_FOUND}\n\
             wget -q -T 5 -O /dev/null http://{}/\n",
            self.destination()
        )
    }
}

/// Baseline run: the same image and launch, doing nothing. Its wall time is the
/// container startup cost that [`EGRESS_PROBE`]'s measurement is taken net of.
const STARTUP_BASELINE: &str = "exit 0\n";

/// How an outbound-connect attempt from inside the sandbox ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EgressVerdict {
    /// Refused immediately: there is no route out of the namespace.
    BlockedByPolicy,
    /// The attempt hung until its own client timeout. Indistinguishable from a
    /// broken network, so it is not evidence that the boundary held.
    Timeout,
    /// No client was present, so nothing was attempted.
    ProbeUnavailable,
    /// The container had a non-loopback interface, so a fast failure could be a
    /// remote refusal rather than the sandbox policy. The probe refuses to infer.
    BoundaryMisconfigured,
    /// The probe process died by signal, so its non-success says nothing about
    /// whether the connect was attempted or refused by policy.
    ProbeFailed,
    /// The connection succeeded. The boundary is open.
    Connected,
}

impl std::fmt::Display for EgressVerdict {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let text = match self {
            EgressVerdict::BlockedByPolicy => {
                "the connect was refused immediately, as a routeless namespace does"
            }
            EgressVerdict::Timeout => {
                "the connect hung until its client timeout, which a broken network also does; \
                 this is not evidence the boundary held"
            }
            EgressVerdict::ProbeUnavailable => {
                "the fixture image has no wget, so no connection was attempted; the probe \
                 proves nothing about this image"
            }
            EgressVerdict::BoundaryMisconfigured => {
                "the container exposes a non-loopback network interface; a failed connect is not evidence of an egress policy"
            }
            EgressVerdict::ProbeFailed => {
                "the probe process ended by signal, so no egress conclusion is possible"
            }
            EgressVerdict::Connected => "the connect SUCCEEDED: the sandbox has egress",
        };
        f.write_str(text)
    }
}

/// Classify one egress attempt from its exit status and the time it took net of
/// container startup. Pure, so the classification is tested without Docker.
fn classify_egress(exit_code: Option<i32>, attempt: Duration) -> EgressVerdict {
    match exit_code {
        Some(0) => EgressVerdict::Connected,
        Some(EXIT_COMMAND_NOT_FOUND) => EgressVerdict::ProbeUnavailable,
        Some(EXIT_NETWORK_NAMESPACE_OPEN) => EgressVerdict::BoundaryMisconfigured,
        None => EgressVerdict::ProbeFailed,
        _ if attempt >= EGRESS_DENIAL_MAX => EgressVerdict::Timeout,
        _ => EgressVerdict::BlockedByPolicy,
    }
}

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

    let (probe, _) = run_in_sandbox(image, HOSTILE_PROBE)?;
    if !probe.status.success() {
        let stderr = String::from_utf8_lossy(&probe.stderr);
        return Err(SandboxError::Readiness(format!(
            "hostile fixture exited with {}: {}",
            probe.status,
            stderr.trim()
        )));
    }

    // Egress is asserted by attempting one, not by reading the interface list:
    // `/sys/class/net` says what was configured, an attempted connect says what
    // the configuration does. Every destination class is attempted, because the
    // ones that matter most when a boundary is misconfigured (metadata service,
    // host loopback, the operator's own subnets) are not on the public internet
    // and do not follow from it being unreachable.
    let mut passed_checks: Vec<String> = [
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
    .collect();

    let startup = measure_startup(image)?;
    // The host-loopback class is probed against a listener this process holds
    // open, and the listener is proved live by connecting to it from here first.
    // Without that control the class is the one fixture that can pass because
    // nothing was listening rather than because the boundary held.
    let listener = std::net::TcpListener::bind(("127.0.0.1", 0)).map_err(|error| {
        SandboxError::Readiness(format!("cannot hold a host listener: {error}"))
    })?;
    let port = listener
        .local_addr()
        .map_err(|error| {
            SandboxError::Readiness(format!("cannot read the listener port: {error}"))
        })?
        .port();
    std::net::TcpStream::connect(("127.0.0.1", port)).map_err(|error| {
        SandboxError::Readiness(format!(
            "the host loopback listener is not reachable from the host itself ({error}), so a \
             refusal from inside the container would prove nothing"
        ))
    })?;

    for target in EGRESS_DENY_CLASSES
        .iter()
        .copied()
        .chain([EgressTarget::HostLoopback(port)])
    {
        let verdict = probe_egress_after(image, target, startup)?;
        if verdict != EgressVerdict::BlockedByPolicy {
            return Err(SandboxError::Readiness(format!(
                "outbound connect to {} ({}) from inside the sandbox: {verdict}",
                target.class(),
                target.destination()
            )));
        }
        passed_checks.push(format!(
            "egress refused by policy, not by timeout: {} ({})",
            target.class(),
            target.destination()
        ));
    }
    drop(listener);

    Ok(SandboxReadiness {
        image: image.to_string(),
        image_id,
        docker_server_version,
        passed_checks,
    })
}

/// Time a do-nothing run of `image`: the container startup cost every egress
/// measurement is taken net of, so [`EGRESS_DENIAL_MAX`] bounds the connect
/// rather than the daemon.
fn measure_startup(image: &str) -> Result<Duration, SandboxError> {
    let (baseline, startup) = run_in_sandbox(image, STARTUP_BASELINE)?;
    if !baseline.status.success() {
        return Err(SandboxError::Readiness(format!(
            "the startup baseline run exited with {}, so the egress measurement has no reference",
            baseline.status
        )));
    }
    Ok(startup)
}

/// Attempt one outbound connection to `target` from inside the hardened boundary
/// and classify how it ended. Measures its own startup baseline; callers probing
/// several classes in a row share one via [`probe_egress_after`].
pub fn probe_egress(image: &str, target: EgressTarget) -> Result<EgressVerdict, SandboxError> {
    let startup = measure_startup(image)?;
    probe_egress_after(image, target, startup)
}

fn probe_egress_after(
    image: &str,
    target: EgressTarget,
    startup: Duration,
) -> Result<EgressVerdict, SandboxError> {
    let (egress, elapsed) = run_in_sandbox(image, &target.script())?;
    Ok(classify_egress(
        egress.status.code(),
        elapsed.saturating_sub(startup),
    ))
}

/// Run one `/bin/sh` script inside the hardened boundary against `image`,
/// returning its output and how long the whole container took.
fn run_in_sandbox(image: &str, script: &str) -> Result<(Output, Duration), SandboxError> {
    let args = HardenedLaunch::new(image, &Retention::AutoRemove).into_args_with_command([
        "/bin/sh".to_string(),
        "-ceu".to_string(),
        script.to_string(),
    ]);
    let started = Instant::now();
    let output = command_output_with_timeout("docker", &args, READINESS_TIMEOUT)
        .map_err(SandboxError::Readiness)?;
    Ok((output, started.elapsed()))
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

/// Refuse a reference Docker cannot resolve to a locally present image.
///
/// [`run_external_sandboxed`] cannot see this and is not the place to: the
/// launch passes `--pull never`, and `docker run` against an image that is not
/// there still *spawns*, exits on its own, and reaches the harness as an agent
/// that answers nothing. A driver that wants an absent artifact to be a refusal
/// before the sweep starts, rather than a dead agent inside it, asks here first.
pub fn require_local_image(image: &str) -> Result<(), SandboxError> {
    validate_image(image, false)?;
    let inspected = docker_output(&["image", "inspect", "--format", "{{.Id}}", image])?;
    if !inspected.status.success() {
        return Err(SandboxError::InvalidConfig(format!(
            "the pinned image is not present locally and nothing is pulled: {}",
            String::from_utf8_lossy(&inspected.stderr).trim()
        )));
    }
    Ok(())
}

/// Post-exit inspection of a named container, injectable so the classification
/// path is testable on a machine with no Docker daemon. The live implementation
/// is [`DockerCli`]; the live leg runs only in the Docker-enabled CI job.
pub trait ContainerInspector {
    /// Whether the kernel OOM-killed the named container (`State.OOMKilled`).
    fn oom_killed(&self, name: &str) -> Result<bool, String>;
    /// Remove the named container, force-stopping it if still running. This is
    /// the explicit replacement for `--rm`, so failure is observable rather
    /// than silently treated as cleanup.
    fn remove(&self, name: &str) -> Result<(), String>;
}

/// The real inspector: shells out to the `docker` binary, like every other
/// Docker interaction in this module.
pub struct DockerCli;

impl ContainerInspector for DockerCli {
    fn oom_killed(&self, name: &str) -> Result<bool, String> {
        let output = Command::new("docker")
            .args(["inspect", "--format", "{{.State.OOMKilled}}", name])
            .stdin(Stdio::null())
            .output()
            .map_err(|error| format!("cannot run docker inspect for {name}: {error}"))?;
        if !output.status.success() {
            return Err(format!(
                "docker inspect for {name} exited {}: {}",
                output.status,
                String::from_utf8_lossy(&output.stderr).trim()
            ));
        }
        match String::from_utf8_lossy(&output.stdout).trim() {
            "true" => Ok(true),
            "false" => Ok(false),
            value => Err(format!(
                "docker inspect for {name} returned an invalid OOMKilled value {value:?}"
            )),
        }
    }

    fn remove(&self, name: &str) -> Result<(), String> {
        let status = Command::new("docker")
            .args(["rm", "-f", name])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map_err(|error| format!("cannot run docker rm for {name}: {error}"))?;
        if status.success() {
            Ok(())
        } else {
            Err(format!("docker rm for {name} exited {status}"))
        }
    }
}

/// A sandboxed external agent plus the handle to its container.
///
/// An entrant killed by the sandbox's `--memory` limit exits 137, which the
/// transport cannot tell apart from any other SIGKILL — so exceeding a
/// *published* resource budget, a scoring-relevant fact, was invisible to the
/// failure taxonomy. The container is therefore launched **named and without
/// `--rm`**, and [`SandboxedAgent::finish`] reads `State.OOMKilled` after the
/// wait, then removes the container explicitly. Removal uses `docker rm -f`, so
/// the no-leak property `--rm` provided is preserved on both the `finish` path
/// and `Drop` (a harness killed with SIGKILL between spawn and drop can still
/// leave a stopped container behind; the deterministic `sharpebench-agent-*`
/// name prefix makes such a remnant findable).
pub struct SandboxedAgent {
    /// `Some` for the whole life of the value; taken in `finish_with` so the
    /// docker client is reaped (the container has exited) before inspection.
    agent: Option<ExternalAgent>,
    /// The container name, or `None` for an opted-in unsandboxed local run.
    container: Option<String>,
}

impl SandboxedAgent {
    /// Override the per-decision wall-clock budget on the wrapped transport.
    pub fn with_decide_timeout(mut self, timeout: Duration) -> Self {
        self.agent = self.agent.take().map(|a| a.with_decide_timeout(timeout));
        self
    }

    /// The launched container's name, when there is one.
    pub fn container_name(&self) -> Option<&str> {
        self.container.as_deref()
    }

    /// Tear the agent down, then report whether the kernel OOM-killed its
    /// container, removing the container afterwards. `Some(true)` means the
    /// entrant exceeded the published `--memory` budget; the driver folds that
    /// into the failure taxonomy via `sharpebench_harness::apply_oom_verdict`.
    /// `None` only for an explicitly opted-in unsandboxed run. An indeterminate
    /// container verdict or failed cleanup is a [`SandboxError::Inspection`],
    /// because silently scoring either state would weaken the sandbox contract.
    pub fn finish(self) -> Result<Option<bool>, SandboxError> {
        self.finish_with(&DockerCli)
    }

    /// [`SandboxedAgent::finish`] with an injected inspector, so the
    /// inspect-then-remove sequence is testable without a Docker daemon.
    pub fn finish_with(
        mut self,
        inspector: &dyn ContainerInspector,
    ) -> Result<Option<bool>, SandboxError> {
        // Reap the docker client first: once it is gone the container has
        // exited (or is orphaned and about to be force-removed), so the state
        // read below is final rather than mid-run.
        self.agent = None;
        let Some(name) = self.container.take() else {
            return Ok(None);
        };
        let verdict = inspector.oom_killed(&name);
        let removed = inspector.remove(&name);
        match (verdict, removed) {
            (Ok(oom_killed), Ok(())) => Ok(Some(oom_killed)),
            (Err(inspect), Ok(())) => Err(SandboxError::Inspection(inspect)),
            (Ok(_), Err(remove)) => Err(SandboxError::Inspection(remove)),
            (Err(inspect), Err(remove)) => Err(SandboxError::Inspection(format!(
                "{inspect}; cleanup also failed: {remove}"
            ))),
        }
    }
}

impl Agent for SandboxedAgent {
    fn decide(
        &mut self,
        obs: &sharpebench_protocol::MarketObservation,
    ) -> sharpebench_protocol::Decision {
        self.agent
            .as_mut()
            .expect("agent is present until finish consumes the value")
            .decide(obs)
    }
}

impl TransportDiagnostics for SandboxedAgent {
    fn health(&self) -> &TransportHealth {
        self.agent
            .as_ref()
            .expect("agent is present until finish consumes the value")
            .health()
    }
}

impl Drop for SandboxedAgent {
    fn drop(&mut self) {
        // A drop without `finish` (a panic, an early return) must not leak the
        // non-`--rm` container: reap the client, then force-remove.
        self.agent = None;
        if let Some(name) = self.container.take() {
            let _ = DockerCli.remove(&name);
        }
    }
}

/// Run an external agent sandboxed in Docker, returning the live
/// [`SandboxedAgent`] transport (drive it with the harness exactly like any
/// other external agent, then call [`SandboxedAgent::finish`] for the post-run
/// resource verdict). See the module docs for the isolation contract.
pub fn run_external_sandboxed(
    image: &str,
    opts: &SandboxOptions,
) -> Result<SandboxedAgent, SandboxError> {
    // The refusal logic is shared with `resolve_launch`; the sandboxed branch
    // then swaps `--rm` for a fresh name so post-exit state stays inspectable.
    let launch = resolve_launch(docker_available(), image, opts)?;
    let (program, args, container) = match launch {
        Launch::Docker { program, .. } => {
            let name = fresh_container_name();
            let args = hardened_docker_args(image, &Retention::Inspectable(name.clone()));
            (program, args, Some(name))
        }
        Launch::Unsandboxed { program, args } => (program, args, None),
    };
    let arg_refs: Vec<&str> = args.iter().map(String::as_str).collect();
    // The Docker branch spawns `spawn_inheriting`, deliberately: the child is
    // the trusted `docker` client, which needs its `DOCKER_HOST` /
    // `DOCKER_CONFIG` context, while the untrusted entrant inside the container
    // gets the container's own fresh environment regardless — nothing of the
    // harness environment crosses the boundary. The opted-in local-dev command
    // IS the agent process, so it gets the hermetic spawn like every other
    // host-executed agent (`SHARPEBENCH_AGENT_ENV` passes named variables).
    let agent = if container.is_some() {
        ExternalAgent::spawn_inheriting(&program, &arg_refs)
    } else {
        ExternalAgent::spawn(&program, &arg_refs)
    }
    .map_err(|e| SandboxError::Spawn(e.to_string()))?;
    Ok(SandboxedAgent {
        agent: Some(agent),
        container,
    })
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

    /// The pinning leg is decided before Docker is contacted, so it is the part
    /// of the presence check that holds on a machine with no daemon. The
    /// presence leg itself needs one, and runs in the Docker-enabled CI job
    /// through the CLI's `--image` refusal test.
    #[test]
    fn an_unpinned_reference_is_refused_without_contacting_docker() {
        let error = require_local_image("some/agent:latest")
            .expect_err("a mutable tag is never a field artifact");
        // The message is asserted, not just the variant: the presence leg refuses
        // with the same variant on a machine with no daemon, so a variant-only
        // assertion would pass here even if the pinning leg were gone.
        assert!(
            error.to_string().contains("must be pinned"),
            "the refusal must be about the missing digest: {error}"
        );
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

    /// The agent launch must keep post-exit state: `--rm` would let the daemon
    /// destroy `State.OOMKilled` before it can be read, making a kill by the
    /// published `--memory` budget indistinguishable from any other SIGKILL.
    #[test]
    fn an_inspectable_launch_is_named_and_not_auto_removed() {
        let image = format!("fixture@sha256:{}", "a".repeat(64));
        let retention = Retention::Inspectable("sharpebench-agent-test-0".to_string());
        let args = hardened_docker_args(&image, &retention);
        assert!(
            !args.iter().any(|a| a == "--rm"),
            "--rm destroys the exited container's state before docker inspect can read it: {args:?}"
        );
        let name_at = args
            .iter()
            .position(|a| a == "--name")
            .expect("the container must be named so it can be inspected and removed");
        assert_eq!(args[name_at + 1], "sharpebench-agent-test-0");
        assert_eq!(
            args.last().map(String::as_str),
            Some(image.as_str()),
            "the image stays the trailing positional"
        );
        // The hardening flags must be identical to the probe launch apart from
        // the retention choice, so the inspectable path weakens nothing.
        let mut auto = hardened_docker_args(&image, &Retention::AutoRemove);
        auto.retain(|a| a != "--rm");
        let mut named = args;
        named.retain(|a| a != "--name" && a != "sharpebench-agent-test-0");
        assert_eq!(auto, named);
    }

    #[test]
    fn container_names_are_unique_within_a_process() {
        assert_ne!(fresh_container_name(), fresh_container_name());
    }

    /// An inspector whose calls are journaled, so the inspect-then-remove
    /// sequence is provable without a Docker daemon (the live leg runs only in
    /// the Docker-enabled CI job).
    struct FakeInspector {
        verdict: Result<bool, String>,
        removal: Result<(), String>,
        calls: std::cell::RefCell<Vec<String>>,
    }

    impl ContainerInspector for FakeInspector {
        fn oom_killed(&self, name: &str) -> Result<bool, String> {
            self.calls.borrow_mut().push(format!("inspect {name}"));
            self.verdict.clone()
        }
        fn remove(&self, name: &str) -> Result<(), String> {
            self.calls.borrow_mut().push(format!("remove {name}"));
            self.removal.clone()
        }
    }

    #[test]
    fn finish_inspects_before_removing_and_reports_the_verdict() {
        for verdict in [true, false] {
            let inspector = FakeInspector {
                verdict: Ok(verdict),
                removal: Ok(()),
                calls: std::cell::RefCell::new(Vec::new()),
            };
            let agent = SandboxedAgent {
                agent: None,
                container: Some("c-1".to_string()),
            };
            assert_eq!(agent.finish_with(&inspector), Ok(Some(verdict)));
            assert_eq!(
                *inspector.calls.borrow(),
                vec!["inspect c-1".to_string(), "remove c-1".to_string()],
                "the state must be read before the container is destroyed, and \
                 the container must always be removed (the explicit replacement \
                 for --rm)"
            );
        }
    }

    #[test]
    fn finish_refuses_an_indeterminate_verdict_or_failed_cleanup() {
        for (verdict, removal, expected) in [
            (
                Err("inspect unavailable".to_string()),
                Ok(()),
                "inspect unavailable",
            ),
            (
                Ok(false),
                Err("container remains".to_string()),
                "container remains",
            ),
        ] {
            let inspector = FakeInspector {
                verdict,
                removal,
                calls: std::cell::RefCell::new(Vec::new()),
            };
            let agent = SandboxedAgent {
                agent: None,
                container: Some("c-1".to_string()),
            };
            let error = agent
                .finish_with(&inspector)
                .expect_err("unknown state or failed removal must not look like success");
            assert!(error.to_string().contains(expected), "{error}");
            assert_eq!(
                *inspector.calls.borrow(),
                vec!["inspect c-1".to_string(), "remove c-1".to_string()],
                "cleanup is attempted even when inspection fails"
            );
        }
    }

    #[test]
    fn an_unsandboxed_run_has_no_container_and_no_verdict() {
        let inspector = FakeInspector {
            verdict: Ok(true),
            removal: Ok(()),
            calls: std::cell::RefCell::new(Vec::new()),
        };
        let agent = SandboxedAgent {
            agent: None,
            container: None,
        };
        assert_eq!(agent.finish_with(&inspector), Ok(None));
        assert!(
            inspector.calls.borrow().is_empty(),
            "nothing to inspect or remove without a container"
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

    /// Each verdict is asserted against the neighbour it would otherwise be
    /// confused with: a bare "the connect did not succeed" assertion passes for a
    /// dropped packet and for a fixture with no client installed, neither of
    /// which is evidence that the boundary refused anything.
    #[test]
    fn an_egress_denial_is_distinguished_from_the_failures_that_look_like_one() {
        let instant = Duration::from_millis(3);
        assert_eq!(
            classify_egress(Some(1), instant),
            EgressVerdict::BlockedByPolicy
        );
        // Same non-zero exit, but it burned the client timeout: a broken network
        // produces exactly this, so it must not read as a denial.
        assert_eq!(
            classify_egress(Some(1), EGRESS_DENIAL_MAX),
            EgressVerdict::Timeout
        );
        assert_eq!(
            classify_egress(Some(1), Duration::from_secs(5)),
            EgressVerdict::Timeout
        );
        // Killed by a signal (no exit code) never proves that a connect reached
        // the policy boundary, however quickly it happened.
        assert_eq!(classify_egress(None, instant), EgressVerdict::ProbeFailed);
        assert_eq!(
            classify_egress(None, Duration::from_secs(5)),
            EgressVerdict::ProbeFailed
        );
        // No client in the image: nothing was attempted, so nothing was shown.
        assert_eq!(
            classify_egress(Some(EXIT_COMMAND_NOT_FOUND), instant),
            EgressVerdict::ProbeUnavailable
        );
        // The one outcome that means the sandbox leaks.
        assert_eq!(classify_egress(Some(0), instant), EgressVerdict::Connected);
        assert_eq!(
            classify_egress(Some(EXIT_NETWORK_NAMESPACE_OPEN), instant),
            EgressVerdict::BoundaryMisconfigured
        );
    }

    #[test]
    fn every_probe_refuses_to_pass_when_its_client_is_missing() {
        for target in EGRESS_DENY_CLASSES
            .iter()
            .copied()
            .chain([EgressTarget::HostLoopback(9)])
        {
            let script = target.script();
            assert!(
                script.contains(&format!("|| exit {EXIT_COMMAND_NOT_FOUND}")),
                "an absent client must exit as not-found, not fall through to a denial: {script}"
            );
            assert!(
                script.contains(&format!("|| exit {EXIT_NETWORK_NAMESPACE_OPEN}")),
                "the probe must refuse to infer policy from timing when the network namespace is open: {script}"
            );
            assert!(
                script.contains(&format!("http://{}/", target.destination())),
                "the probe must connect to its own class's destination: {script}"
            );
        }
    }

    /// The taxonomy, not the count: each class is a distinct failure mode with a
    /// distinct blast radius, and dropping one silently narrows what the boundary
    /// is asserted to refuse.
    #[test]
    fn the_deny_taxonomy_covers_every_class_a_boundary_must_refuse() {
        let destinations: Vec<String> = EGRESS_DENY_CLASSES
            .iter()
            .map(EgressTarget::destination)
            .collect();
        for expected in [
            "169.254.169.254:80", // the credential-theft path
            "169.254.42.1:80",    // the rest of the link-local block
            "10.0.0.1:80",
            "172.16.0.1:80",
            "192.168.0.1:80",
            "1.1.1.1:80",
        ] {
            assert!(
                destinations.iter().any(|value| value == expected),
                "{expected} is not in the deny taxonomy: {destinations:?}"
            );
        }
        let mut unique = destinations.clone();
        unique.sort();
        unique.dedup();
        assert_eq!(
            unique.len(),
            destinations.len(),
            "two classes probing the same destination test one thing twice: {destinations:?}"
        );
        // The host-loopback class is excluded from the const on purpose: its
        // destination is only meaningful with a live listener's port in it.
        assert!(!EGRESS_DENY_CLASSES
            .iter()
            .any(|target| matches!(target, EgressTarget::HostLoopback(_))));
        assert_eq!(
            EgressTarget::HostLoopback(4711).destination(),
            "127.0.0.1:4711"
        );
    }

    /// Every destination is a literal address. A name would make the result
    /// depend on resolution, and a routeless namespace fails to resolve before it
    /// fails to connect — a different fact than the one under test.
    #[test]
    fn no_probe_depends_on_name_resolution() {
        for target in EGRESS_DENY_CLASSES
            .iter()
            .copied()
            .chain([EgressTarget::HostLoopback(9)])
        {
            let destination = target.destination();
            let (address, port) = destination
                .rsplit_once(':')
                .expect("a destination is address:port");
            assert!(
                address.parse::<std::net::Ipv4Addr>().is_ok(),
                "{address} is not a literal address"
            );
            assert!(port.parse::<u16>().is_ok(), "{port} is not a port");
        }
    }

    #[test]
    fn field_readiness_requires_a_pinned_fixture_before_contacting_docker() {
        assert!(matches!(
            check_sandbox_readiness("alpine:latest"),
            Err(SandboxError::InvalidConfig(_))
        ));
    }

    /// A flag appended to a launch after construction must land before the
    /// image positional however the command is assembled — the previous flat
    /// list made "append a hardening flag" silently mean "hand the entrant a
    /// container argument".
    #[test]
    fn a_flag_appended_late_lands_before_the_image_positional() {
        let image = format!("fixture@sha256:{}", "a".repeat(64));
        for with_command in [false, true] {
            let mut launch = HardenedLaunch::new(&image, &Retention::AutoRemove);
            launch.flags.push("--appended-hardening-flag".to_string());
            let args = if with_command {
                launch.into_args_with_command(["/bin/sh".to_string(), "-c".to_string()])
            } else {
                launch.into_args()
            };
            let flag_at = args
                .iter()
                .position(|a| a == "--appended-hardening-flag")
                .expect("the appended flag must be in the command");
            let image_at = args
                .iter()
                .position(|a| *a == image)
                .expect("the image positional must be in the command");
            assert!(
                flag_at < image_at,
                "an appended flag after the image is a container argument, not \
                 a Docker flag: {args:?}"
            );
            if with_command {
                assert_eq!(
                    args[image_at + 1],
                    "/bin/sh",
                    "the command follows the image"
                );
            } else {
                assert_eq!(args.last(), Some(&image), "the image stays trailing");
            }
        }
    }

    #[test]
    fn hostile_probe_runs_inside_the_same_hardened_boundary() {
        let image = format!("fixture@sha256:{}", "a".repeat(64));
        let args = HardenedLaunch::new(&image, &Retention::AutoRemove).into_args_with_command([
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
        assert_eq!(readiness.passed_checks.len(), 14, "{readiness:?}");
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
        for target in EGRESS_DENY_CLASSES {
            assert!(
                readiness
                    .passed_checks
                    .iter()
                    .any(|check| check.contains(&target.destination())),
                "the {} class is missing from {:?}",
                target.class(),
                readiness.passed_checks
            );
        }
        assert!(
            readiness
                .passed_checks
                .iter()
                .any(|check| check.contains("host loopback")),
            "the host-loopback class is missing from {:?}",
            readiness.passed_checks
        );
    }

    /// One egress fixture: the class must come back as a policy denial, not as a
    /// timeout, not as a fixture with no client in it, and not as a connection.
    ///
    /// The elapsed-time leg is what makes this worth writing. A refusal with no
    /// route out returns in microseconds while a dropped packet burns the client
    /// timeout, and both are the same non-zero exit — so an assertion of "the
    /// connect failed" passes on a runner whose network is merely broken, on an
    /// image with no `wget`, and on a boundary that is not enforcing anything.
    /// [`classify_egress`] separates the four, and each fixture asserts the one
    /// classification that is evidence.
    fn assert_egress_blocked(target: EgressTarget) {
        assert!(
            docker_available(),
            "this test was requested explicitly with --ignored, so an absent Docker daemon is a \
             failure, not a reason to pass"
        );
        let image = live_fixture_image();
        let verdict = probe_egress(&image, target).unwrap_or_else(|error| {
            panic!(
                "the {} probe could not run, so the class is untested: {error}",
                target.class()
            )
        });
        assert_eq!(
            verdict,
            EgressVerdict::BlockedByPolicy,
            "{} ({}): {verdict}",
            target.class(),
            target.destination()
        );
    }

    #[test]
    #[ignore = "needs a running Docker daemon and SHARPEBENCH_SANDBOX_FIXTURE"]
    fn live_egress_to_cloud_metadata_is_denied() {
        assert_egress_blocked(EgressTarget::CloudMetadata);
    }

    #[test]
    #[ignore = "needs a running Docker daemon and SHARPEBENCH_SANDBOX_FIXTURE"]
    fn live_egress_to_the_link_local_range_is_denied() {
        assert_egress_blocked(EgressTarget::LinkLocalRange);
    }

    #[test]
    #[ignore = "needs a running Docker daemon and SHARPEBENCH_SANDBOX_FIXTURE"]
    fn live_egress_to_private_class_a_is_denied() {
        assert_egress_blocked(EgressTarget::PrivateClassA);
    }

    #[test]
    #[ignore = "needs a running Docker daemon and SHARPEBENCH_SANDBOX_FIXTURE"]
    fn live_egress_to_private_class_b_is_denied() {
        assert_egress_blocked(EgressTarget::PrivateClassB);
    }

    #[test]
    #[ignore = "needs a running Docker daemon and SHARPEBENCH_SANDBOX_FIXTURE"]
    fn live_egress_to_private_class_c_is_denied() {
        assert_egress_blocked(EgressTarget::PrivateClassC);
    }

    #[test]
    #[ignore = "needs a running Docker daemon and SHARPEBENCH_SANDBOX_FIXTURE"]
    fn live_egress_to_the_public_internet_is_denied() {
        assert_egress_blocked(EgressTarget::PublicInternet);
    }

    /// The host-loopback class, with the one control that keeps it honest: a
    /// listener this process holds open and has itself connected to. Inside the
    /// container `127.0.0.1` must mean the container; a gateway that forwards it
    /// to the host's loopback reaches host debuggers, local databases and the
    /// Docker socket, and this is the fixture that would see it.
    #[test]
    #[ignore = "needs a running Docker daemon and SHARPEBENCH_SANDBOX_FIXTURE"]
    fn live_egress_to_the_host_loopback_is_denied() {
        let listener =
            std::net::TcpListener::bind(("127.0.0.1", 0)).expect("the host must allow a listener");
        let port = listener
            .local_addr()
            .expect("a bound listener has a port")
            .port();
        std::net::TcpStream::connect(("127.0.0.1", port)).expect(
            "the control connect proves the listener is live, so a refusal means something",
        );
        assert_egress_blocked(EgressTarget::HostLoopback(port));
        drop(listener);
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
        // The post-run resource verdict: a container that merely idled was not
        // OOM-killed, and `finish` must both read that state (possible only
        // because the launch is not `--rm`) and then remove the container so
        // nothing leaks.
        let name = agent
            .container_name()
            .expect("a Docker launch always names its container")
            .to_string();
        assert_eq!(
            agent.finish(),
            Ok(Some(false)),
            "an idle container must report OOMKilled=false, not an indeterminable state"
        );
        let inspect = Command::new("docker")
            .args(["inspect", &name])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .expect("docker must run");
        assert!(
            !inspect.success(),
            "the container must be removed after finish; a remnant re-creates the leak --rm prevented"
        );
    }

    /// Exercise the Docker/kernel fact that the injected inspector otherwise has to
    /// stand in for: crossing the actual cgroup memory budget sets
    /// ``State.OOMKilled=true`` before the named container is removed.
    #[test]
    #[ignore = "needs a running Docker daemon and SHARPEBENCH_SANDBOX_FIXTURE"]
    fn live_memory_limit_sets_the_oom_killed_verdict() {
        assert!(
            docker_available(),
            "this test was requested explicitly with --ignored, so an absent Docker daemon is a failure"
        );
        let image = live_fixture_image();
        let name = fresh_container_name();
        let args = HardenedLaunch::new_with_memory(
            &image,
            &Retention::Inspectable(name.clone()),
            "32m",
        )
        .into_args_with_command([
            "/bin/sh".to_string(),
            "-ceu".to_string(),
            // BusyBox awk grows one in-process string until the cgroup kills it.
            // Streaming /dev/zero would not work: a pipe holds only a bounded buffer.
            "command -v awk >/dev/null 2>&1 || exit 127; awk 'BEGIN { x=\"0123456789abcdef\"; for (i=0; i<30; i++) x=x x; print length(x) }'"
                .to_string(),
        ]);
        let status = Command::new("docker")
            .args(&args)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .expect("docker must launch the OOM fixture");
        let verdict = DockerCli.oom_killed(&name);
        DockerCli
            .remove(&name)
            .expect("the OOM fixture container must be removed");

        assert!(
            !status.success(),
            "the allocator unexpectedly stayed inside 32 MiB"
        );
        assert_eq!(
            verdict,
            Ok(true),
            "the live cgroup kill must be visible through the production Docker inspector"
        );
        let inspect = Command::new("docker")
            .args(["inspect", &name])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .expect("docker must run");
        assert!(
            !inspect.success(),
            "the OOM fixture container must be removed"
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
