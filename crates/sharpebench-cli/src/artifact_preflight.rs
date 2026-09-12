//! Opt-in Docker image preflight for `run --image`.
//!
//! The byte engine in [`sharpebench_harness::artifact_scan`] matches known
//! content in streams a trusted caller hands it. This module is that caller for
//! one artifact class: a locally present, digest-pinned container image. It
//! captures the image's executable configuration and a filesystem snapshot of a
//! container that is created and never started, scans both without extracting
//! anything, and refuses the entrant launch unless every leg completed clean.
//!
//! Trust boundary: Docker's client binary and its daemon are infrastructure.
//! An entrant supplies an image reference and nothing else. There is no flag,
//! environment override or policy field here that selects the Docker executable
//! or a provider endpoint, and the launch that follows a passing preflight uses
//! Docker's own immutable configuration ID rather than the caller's reference.
//!
//! What a negative result is not: this proves that the named streams inside the
//! declared scope did not contain the policy's protected bytes. Compressed,
//! encoded, encrypted or model-internalized copies are outside raw-byte scope,
//! and so is anything the daemon does not put in an export (see
//! [`SCAN_SCOPE`]).
//!
//! # Runtime allowlist and functional probe
//!
//! `--runtime-allowlist` adds the other polarity. The scan policy refuses known
//! content; the allowlist refuses every export entry whose path it does not
//! admit, so what the runtime image may contain is declared rather than
//! guessed at. An image that passes both is then run once, from its
//! configuration ID and under the same hardened launch a sweep uses, against a
//! fixed synthetic observation, and must answer with a valid decision. That is
//! the post-strip functional test: an image restricted to what the allowlist
//! admits is shown to work, not assumed to. The probe runs only after every
//! scan leg authorized the image, so a refused image is still never started.
//!
//! What an allowlist result is not: it proves which paths the export holds. It
//! says nothing about the bytes under an admitted path, and it does not make the
//! image reproducible.

use std::fs::File;
use std::io::{Read, Seek, SeekFrom, Write as _};
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use sharpebench_harness::artifact_scan::{RawScanPolicy, RawScanReport, RawScanner};
use sharpebench_harness::artifact_tar::{scan_tar_snapshot_until, TarScanReport};
use sharpebench_harness::{SweepCheckpoint, SweepContract};

/// Report schema for a completed (passing or refusing) preflight.
pub const REPORT_VERSION: &str = "sharpebench.image-preflight.v1";
/// Report schema for a preflight that could not complete a scan at all.
pub const FAILURE_VERSION: &str = "sharpebench.image-preflight-failure.v1";
/// Invocation framing bound into the checkpoint identity of a scanned run.
pub const INVOCATION_VERSION: &str = "sharpebench.scanned-image-invocation.v1";

/// What the preflight actually looked at.
///
/// The image configuration is the executable part an export cannot show, and
/// the export is the filesystem the daemon writes for a created container.
/// Docker documents that `container export` omits the contents of volumes, so
/// an image declaring volumes is refused rather than reported as scanned.
pub const SCAN_SCOPE: &str = "image-config-and-container-export/v1";

/// Accepted `docker image inspect` / `docker inspect` output.
const MAX_INSPECT_BYTES: u64 = 2 * 1024 * 1024;
/// Accepted `docker create` / `docker rm` output: an ID or a short line.
const MAX_CONTROL_RESPONSE_BYTES: u64 = 1024;
/// Accepted captured stderr before a command is treated as runaway.
const MAX_STDERR_BYTES: u64 = 64 * 1024;
/// Cleanup gets its own allowance: an expired scan budget must not be the
/// reason a container is left behind.
const CLEANUP_ALLOWANCE: Duration = Duration::from_secs(10);
const POLL_INTERVAL: Duration = Duration::from_millis(25);
/// Bytes of untrusted stderr an operator may see, sanitized.
const REDACTED_STDERR_BYTES: usize = 200;
/// Deliberately absent. The snapshot container is created and never started;
/// if some later change ever started one, this entrypoint fails to execute
/// instead of running the entrant.
const NEVER_STARTED_ENTRYPOINT: &str = "/sharpebench-preflight-never-started";

/// Schema of an opt-in runtime allowlist.
pub const ALLOWLIST_VERSION: &str = "sharpebench.runtime-allowlist.v1";
/// Framing of the preflight policy digest when a runtime allowlist applies, so
/// a changed allowlist is a changed policy and cannot resume a checkpoint.
pub const COMBINED_POLICY_VERSION: &str = "sharpebench.image-preflight-policy.v2";
const MAX_ALLOWLIST_BYTES: u64 = 64 * 1024;
const MAX_ALLOWLIST_PATHS: usize = 4096;
const MAX_ALLOWLIST_PATH_BYTES: usize = 1024;
/// Archive-order indices of refused entries a report carries. Entry names are
/// withheld: a report can be published, and a name can be the very thing a
/// policy exists to protect.
const MAX_REPORTED_OUTSIDE: usize = 16;
/// Wall clock for the functional probe, container start included.
const PROBE_ALLOWANCE: Duration = Duration::from_secs(60);
/// Accepted probe output: the external-agent transport's decision-line cap.
const MAX_PROBE_OUTPUT_BYTES: u64 = 8 * 1024 * 1024;
/// The one observation a functional probe hands the image. Deliberately
/// generic: no real instrument, no window date and no dataset value, so the
/// probe tells the image nothing about the evaluation it is entering.
const PROBE_OBSERVATION: &str = r#"{"date":"1970-01-01","cash":1.0,"symbols":[{"symbol":"PROBE","close_history":[1.0]}],"portfolio":[]}"#;

/// Docker's immutable configuration ID for a locally present image.
///
/// The only constructor validates `sha256:<64 lowercase hex>`, and the only
/// caller that gets one out of this module is
/// [`ImagePreflightReport::authorized_image_id`], which yields it exclusively
/// for a preflight that completed, matched nothing and verified its cleanup.
/// That is what lets the launcher's unpinned-reference option be enabled for
/// this value without letting operator input reach an unpinned launch.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ValidatedImageId(String);

impl ValidatedImageId {
    fn parse(raw: &str) -> Option<Self> {
        let hex = raw.strip_prefix("sha256:")?;
        if hex.len() != 64
            || !hex
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
        {
            return None;
        }
        Some(Self(raw.to_string()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// A validated opt-in preflight request. Built only by [`parse_preflight_args`].
pub struct PreflightRequest {
    image: String,
    policy: RawScanPolicy,
    allowlist: Option<RuntimeAllowlist>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AllowlistWire {
    schema_version: String,
    paths: Vec<String>,
}

/// The paths a runtime image may contain. Allowlist polarity: an export entry
/// no path here admits refuses the launch.
///
/// A path ending in `/` admits that directory and everything below it; any
/// other path admits exactly that entry. Paths are relative to the image root,
/// with no `.`, `..` or empty segment. A directory that is an ancestor of an
/// admitted path is admitted itself, because an archive lists the directories
/// it descends through; it admits nothing else below it.
#[derive(Clone, Debug)]
pub struct RuntimeAllowlist {
    paths: Vec<String>,
    digest: String,
}

impl RuntimeAllowlist {
    pub fn from_json(bytes: &[u8]) -> Result<Self, String> {
        if bytes.len() as u64 > MAX_ALLOWLIST_BYTES {
            return Err("runtime allowlist exceeds 64 KiB".into());
        }
        let wire: AllowlistWire = serde_json::from_slice(bytes)
            .map_err(|error| format!("invalid runtime allowlist: {error}"))?;
        if wire.schema_version != ALLOWLIST_VERSION {
            return Err(format!(
                "runtime allowlist schema_version must be {ALLOWLIST_VERSION}"
            ));
        }
        if wire.paths.is_empty() || wire.paths.len() > MAX_ALLOWLIST_PATHS {
            return Err(format!(
                "a runtime allowlist declares 1..={MAX_ALLOWLIST_PATHS} paths"
            ));
        }
        for (index, path) in wire.paths.iter().enumerate() {
            let body = path.strip_suffix('/').unwrap_or(path);
            if path.len() > MAX_ALLOWLIST_PATH_BYTES
                || body.is_empty()
                || path.starts_with('/')
                || !path.bytes().all(|byte| (0x20..0x7f).contains(&byte))
                || body
                    .split('/')
                    .any(|segment| segment.is_empty() || segment == "." || segment == "..")
            {
                return Err(format!(
                    "runtime allowlist path {index} is not a relative path of printable ASCII \
                     without empty, `.` or `..` segments"
                ));
            }
            if wire.paths[..index].contains(path) {
                return Err(format!("runtime allowlist path {index} is declared twice"));
            }
        }
        let digest = sharpebench_attest::content_digest(
            &serde_json::to_vec(&(ALLOWLIST_VERSION, &wire.paths))
                .expect("validated paths serialize"),
        );
        Ok(Self {
            paths: wire.paths,
            digest,
        })
    }

    pub fn digest(&self) -> &str {
        &self.digest
    }

    /// Whether an export entry is admitted. `path` is the entry name as the
    /// archive records it.
    fn admits(&self, path: &str, directory: bool) -> bool {
        let path = path.strip_prefix("./").unwrap_or(path);
        let path = path.trim_start_matches('/').trim_end_matches('/');
        if path.is_empty() || path == "." {
            return directory;
        }
        self.paths
            .iter()
            .any(|allowed| match allowed.strip_suffix('/') {
                Some(root) => {
                    path == root
                        || path
                            .strip_prefix(root)
                            .is_some_and(|rest| rest.starts_with('/'))
                        || (directory
                            && root
                                .strip_prefix(path)
                                .is_some_and(|rest| rest.starts_with('/')))
                }
                None => {
                    path == allowed
                        || (directory
                            && allowed
                                .strip_prefix(path)
                                .is_some_and(|rest| rest.starts_with('/')))
                }
            })
    }
}

/// The shape Docker's init layer gives an entry it puts in every container.
#[derive(Clone, Copy)]
enum DockerInitShape {
    EmptyFile,
    Directory,
    Symlink(&'static str),
}

/// What the daemon's init layer adds to the filesystem of every container it
/// creates, and so to every export, whatever the image holds. Files are
/// created empty (the daemon bind-mounts their contents only when a container
/// starts, and an image's own file at the same path is replaced), directories
/// are mount points or their parents, and `etc/mtab` is a fixed link. Measured
/// against a live daemon in `live_runtime_allowlist_admits_the_fixture_and_its_probe_passes`.
///
/// Admitted without being listed only in exactly this shape: an entry at one of
/// these paths that carries bytes, has another type or links elsewhere is image
/// content and must be allowlisted like any other.
const DOCKER_INIT_ENTRIES: [(&str, DockerInitShape); 12] = [
    (".dockerenv", DockerInitShape::EmptyFile),
    ("dev", DockerInitShape::Directory),
    ("dev/console", DockerInitShape::EmptyFile),
    ("dev/pts", DockerInitShape::Directory),
    ("dev/shm", DockerInitShape::Directory),
    ("etc", DockerInitShape::Directory),
    ("etc/hostname", DockerInitShape::EmptyFile),
    ("etc/hosts", DockerInitShape::EmptyFile),
    ("etc/mtab", DockerInitShape::Symlink("/proc/mounts")),
    ("etc/resolv.conf", DockerInitShape::EmptyFile),
    ("proc", DockerInitShape::Directory),
    ("sys", DockerInitShape::Directory),
];

/// Whether an export entry is one the daemon's init layer put there, in the
/// shape it gives it. `link` is the entry's link target, if it has one.
fn is_docker_init_entry(path: &str, kind: tar::EntryType, size: u64, link: Option<&[u8]>) -> bool {
    let path = path.strip_prefix("./").unwrap_or(path);
    let path = path.trim_start_matches('/').trim_end_matches('/');
    DOCKER_INIT_ENTRIES.iter().any(|(init_path, shape)| {
        *init_path == path
            && match shape {
                DockerInitShape::EmptyFile => kind.is_file() && size == 0,
                DockerInitShape::Directory => kind.is_dir(),
                DockerInitShape::Symlink(target) => {
                    kind.is_symlink() && size == 0 && link == Some(target.as_bytes())
                }
            }
    })
}

/// What the allowlist found in the export. Counts and archive-order indices
/// only; no entry name leaves the preflight.
#[derive(Debug, Serialize)]
pub struct AllowlistReport {
    pub allowlist_sha256: String,
    /// Entries enumerated, directories and links included.
    pub entries: u64,
    /// Entries the allowlist does not name that were admitted as the daemon's
    /// own init-layer entries ([`DOCKER_INIT_ENTRIES`]), in the shape the daemon
    /// gives them.
    pub docker_init_entries: u64,
    pub outside_allowlist: u64,
    pub outside_indices: Vec<u64>,
    /// False when the listing could not be read to the end. An incomplete
    /// listing never admits.
    pub complete: bool,
}

impl AllowlistReport {
    pub fn admits_everything(&self) -> bool {
        self.complete && self.outside_allowlist == 0
    }
}

/// The post-strip functional test: one run of the admitted image against
/// [`PROBE_OBSERVATION`].
#[derive(Debug, Serialize)]
pub struct FunctionalProbeReport {
    pub observation_sha256: String,
    pub passed: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub refusal: Option<&'static str>,
    pub cleanup_verified: bool,
}

/// A preflight that produced at least a configuration scan.
///
/// No `Deserialize`: an entrant-supplied document is not a scan.
#[derive(Debug, Serialize)]
pub struct ImagePreflightReport {
    pub schema_version: &'static str,
    pub scope: &'static str,
    pub image_id: String,
    /// The digest of every policy input the preflight applied. Without a
    /// runtime allowlist this is the scan policy's digest, as it always was.
    pub policy_sha256: String,
    pub configuration: RawScanReport,
    pub filesystem: Option<TarScanReport>,
    pub cleanup_verified: bool,
    /// Present exactly when a runtime allowlist was applied; then
    /// `policy_sha256` binds this and the allowlist digest together.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scan_policy_sha256: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub runtime_allowlist: Option<AllowlistReport>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub functional_probe: Option<FunctionalProbeReport>,
}

impl ImagePreflightReport {
    /// Every leg has to be present and clean: a configuration-only negative, a
    /// partial filesystem scan or an unverified removal all refuse. With a
    /// runtime allowlist, so do an entry it does not admit and a functional
    /// probe that did not pass.
    pub fn authorizes_launch(&self) -> bool {
        let scanned = self.cleanup_verified
            && self.configuration.no_known_matches()
            && self
                .filesystem
                .as_ref()
                .is_some_and(TarScanReport::no_known_matches);
        let allowlisted = self.scan_policy_sha256.is_none()
            || (self
                .runtime_allowlist
                .as_ref()
                .is_some_and(AllowlistReport::admits_everything)
                && self
                    .functional_probe
                    .as_ref()
                    .is_some_and(|probe| probe.passed && probe.cleanup_verified));
        scanned && allowlisted
    }

    /// The single value the entrant may be launched from, and only when the
    /// whole preflight authorizes it.
    pub fn authorized_image_id(&self) -> Option<ValidatedImageId> {
        self.authorizes_launch()
            .then(|| ValidatedImageId::parse(&self.image_id))
            .flatten()
    }
}

/// A preflight that could not produce a scan. Structured and redacted: the
/// message is composed here and never carries captured bytes.
#[derive(Debug, Serialize)]
pub struct PreflightFailure {
    pub schema_version: &'static str,
    pub stage: &'static str,
    pub message: String,
    pub cleanup_verified: bool,
    pub docker_stderr_bytes: usize,
}

impl std::fmt::Display for PreflightFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "image preflight failed at {}: {}",
            self.stage, self.message
        )
    }
}

/// Show the operator what Docker said, sanitized, and only on the console.
///
/// The redacted excerpt stays out of the report: a report can be published, and
/// Docker's diagnostics can quote image-controlled metadata that a scan policy
/// exists to keep out of the open.
fn note_diagnostic(capture: &Capture) {
    if capture.stderr_bytes > 0 {
        eprintln!(
            "note: docker reported ({} bytes, redacted): {}",
            capture.stderr_bytes, capture.redacted_stderr
        );
    }
}

fn failure(stage: &'static str, message: impl Into<String>) -> PreflightFailure {
    PreflightFailure {
        schema_version: FAILURE_VERSION,
        stage,
        message: message.into(),
        cleanup_verified: true,
        docker_stderr_bytes: 0,
    }
}

/// One captured Docker invocation. `stdout` is rewound to the start.
#[derive(Debug)]
pub struct Capture {
    pub success: bool,
    pub stdout: File,
    pub stdout_bytes: u64,
    /// Sanitized and bounded. Never enters a report; operator diagnostics only.
    pub redacted_stderr: String,
    pub stderr_bytes: usize,
}

/// How Docker is invoked. Implemented once for the real client; the test double
/// exists so the refusal paths are provable without a daemon. Nothing an
/// entrant controls can select an implementation.
pub trait DockerTransport {
    fn capture(
        &self,
        args: &[String],
        accepted_stdout_bytes: u64,
        deadline: Instant,
    ) -> Result<Capture, String>;

    /// Like [`DockerTransport::capture`], with `stdin` written to the client
    /// and then closed. Only the functional probe uses it, and only after every
    /// scan leg authorized the image.
    fn probe(
        &self,
        _args: &[String],
        _stdin: &[u8],
        _accepted_stdout_bytes: u64,
        _deadline: Instant,
    ) -> Result<Capture, String> {
        Err("this Docker transport cannot run a functional probe".into())
    }
}

/// The real client: the `docker` binary on the operator's PATH, exactly as the
/// rest of the workspace invokes it.
pub struct DockerProcess;

impl DockerTransport for DockerProcess {
    fn capture(
        &self,
        args: &[String],
        accepted_stdout_bytes: u64,
        deadline: Instant,
    ) -> Result<Capture, String> {
        capture_command("docker", args, None, accepted_stdout_bytes, deadline)
    }

    fn probe(
        &self,
        args: &[String],
        stdin: &[u8],
        accepted_stdout_bytes: u64,
        deadline: Instant,
    ) -> Result<Capture, String> {
        capture_command("docker", args, Some(stdin), accepted_stdout_bytes, deadline)
    }
}

fn terminate(child: &mut Child) {
    let _ = child.kill();
    let _ = child.wait();
}

/// Sizes are checked between polls, so output can overshoot the accepted bound
/// by whatever the child writes inside one interval. This is an accepted-output
/// bound that ends the capture, not a disk quota the kernel enforces.
fn within_caps(stdout: &File, stderr: &File, accepted_stdout_bytes: u64) -> Result<(), String> {
    let out = stdout.metadata().map_err(|error| error.to_string())?.len();
    if out > accepted_stdout_bytes {
        return Err(format!(
            "docker wrote more than the accepted {accepted_stdout_bytes} output bytes"
        ));
    }
    let err = stderr.metadata().map_err(|error| error.to_string())?.len();
    if err > MAX_STDERR_BYTES {
        return Err(format!(
            "docker wrote more than the accepted {MAX_STDERR_BYTES} diagnostic bytes"
        ));
    }
    Ok(())
}

/// Non-graphic and non-ASCII bytes become dots, and the excerpt is short.
///
/// Docker's diagnostics can relay image-controlled metadata and terminal
/// control sequences. They are shown to an operator, so they are sanitized
/// rather than forwarded, and they never enter a machine-read report.
fn redact(bytes: &[u8]) -> String {
    let mut text: String = bytes
        .iter()
        .take(REDACTED_STDERR_BYTES)
        .map(|byte| {
            if (0x20..0x7f).contains(byte) {
                *byte as char
            } else {
                '.'
            }
        })
        .collect();
    if bytes.len() > REDACTED_STDERR_BYTES {
        text.push_str("[truncated]");
    }
    text
}

fn capture_command(
    program: &str,
    args: &[String],
    stdin: Option<&[u8]>,
    accepted_stdout_bytes: u64,
    deadline: Instant,
) -> Result<Capture, String> {
    let mut stdout = tempfile::tempfile().map_err(|error| {
        format!("cannot open an owned capture file for {program} output: {error}")
    })?;
    let mut stderr = tempfile::tempfile().map_err(|error| {
        format!("cannot open an owned capture file for {program} diagnostics: {error}")
    })?;
    let out_handle = stdout
        .try_clone()
        .map_err(|error| format!("cannot hand {program} its output capture: {error}"))?;
    let err_handle = stderr
        .try_clone()
        .map_err(|error| format!("cannot hand {program} its diagnostic capture: {error}"))?;
    let mut child = Command::new(program)
        .args(args)
        .stdin(if stdin.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::from(out_handle))
        .stderr(Stdio::from(err_handle))
        .spawn()
        .map_err(|error| format!("cannot start {program}: {error}"))?;
    // The input is written, then the pipe is closed, so the entrant reads one
    // line and then end of input. A write the child refuses is not an error
    // here: what it wrote back decides the outcome.
    if let (Some(bytes), Some(mut pipe)) = (stdin, child.stdin.take()) {
        let _ = pipe.write_all(bytes);
    }
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) => {
                if let Err(reason) = within_caps(&stdout, &stderr, accepted_stdout_bytes) {
                    terminate(&mut child);
                    return Err(reason);
                }
                if Instant::now() >= deadline {
                    terminate(&mut child);
                    return Err(format!("{program} exceeded the preflight deadline"));
                }
                std::thread::sleep(POLL_INTERVAL);
            }
            Err(error) => {
                terminate(&mut child);
                return Err(format!("cannot poll {program}: {error}"));
            }
        }
    };
    // The child can write between the final poll and its exit, so the sizes are
    // checked again once nothing more can be appended by the client itself.
    within_caps(&stdout, &stderr, accepted_stdout_bytes)?;
    let stdout_bytes = stdout
        .metadata()
        .map_err(|error| format!("cannot size the {program} capture: {error}"))?
        .len();
    stdout
        .seek(SeekFrom::Start(0))
        .map_err(|error| format!("cannot rewind the {program} capture: {error}"))?;
    stderr
        .seek(SeekFrom::Start(0))
        .map_err(|error| format!("cannot rewind the {program} diagnostics: {error}"))?;
    let mut raw = Vec::new();
    stderr
        .take(MAX_STDERR_BYTES)
        .read_to_end(&mut raw)
        .map_err(|error| format!("cannot read the {program} diagnostics: {error}"))?;
    Ok(Capture {
        success: status.success(),
        stdout,
        stdout_bytes,
        redacted_stderr: redact(&raw),
        stderr_bytes: raw.len(),
    })
}

static PREFLIGHT_SEQ: AtomicU64 = AtomicU64::new(0);

/// Unique across processes (pid) and within one (counter), reserved before the
/// create call so removal can be attempted by name even when create's outcome
/// is unknown.
fn reserved_container_name() -> String {
    format!(
        "sharpebench-preflight-{}-{}",
        std::process::id(),
        PREFLIGHT_SEQ.fetch_add(1, Ordering::Relaxed)
    )
}

fn owned(values: &[&str]) -> Vec<String> {
    values.iter().map(|value| (*value).to_string()).collect()
}

fn create_args(name: &str, image_id: &str) -> Vec<String> {
    owned(&[
        "create",
        "--name",
        name,
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
        "--entrypoint",
        NEVER_STARTED_ENTRYPOINT,
        image_id,
    ])
}

/// Opt in only when `--scan-policy` is present.
///
/// Everything here is decided from arguments and one bounded file read, so an
/// invalid policy or a conflicting transport costs zero Docker invocations.
pub fn parse_preflight_args(args: &[String]) -> Result<Option<PreflightRequest>, String> {
    let wants_allowlist = args.iter().any(|arg| arg == "--runtime-allowlist");
    if !args.iter().any(|arg| arg == "--scan-policy") {
        // An allowlist on its own would be silently ignored, which is exactly
        // the warning-not-gate shape a policy must not have.
        if wants_allowlist {
            return Err(
                "--runtime-allowlist requires --scan-policy and --image; it is a leg of the \
                 image preflight, not a policy of its own"
                    .into(),
            );
        }
        return Ok(None);
    }
    let transports: Vec<&str> = ["--image", "--http", "--cmd"]
        .into_iter()
        .filter(|flag| flag_value(args, flag).is_some())
        .collect();
    match transports.as_slice() {
        ["--image"] => {}
        [] => {
            return Err(
                "--scan-policy requires --image <repository@sha256:...>; there is no artifact to \
                 scan for an unspecified or host transport"
                    .into(),
            )
        }
        _ => {
            return Err(format!(
                "--scan-policy accepts exactly one transport and it must be --image; got {}",
                transports.join(" and ")
            ))
        }
    }
    let image = flag_value(args, "--image").expect("the image transport was just matched");
    validate_pinned_reference(image)?;
    let path = flag_value(args, "--scan-policy")
        .filter(|path| !path.starts_with("--"))
        .ok_or("--scan-policy requires a JSON file path")?;
    let file =
        std::fs::File::open(path).map_err(|error| format!("cannot open scan policy: {error}"))?;
    let mut bytes = Vec::new();
    file.take(sharpebench_harness::artifact_scan::MAX_POLICY_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| format!("cannot read scan policy: {error}"))?;
    let policy = RawScanPolicy::from_json(&bytes)?;
    let allowlist = if wants_allowlist {
        let path = flag_value(args, "--runtime-allowlist")
            .filter(|path| !path.starts_with("--"))
            .ok_or("--runtime-allowlist requires a JSON file path")?;
        let file = std::fs::File::open(path)
            .map_err(|error| format!("cannot open runtime allowlist: {error}"))?;
        let mut bytes = Vec::new();
        file.take(MAX_ALLOWLIST_BYTES + 1)
            .read_to_end(&mut bytes)
            .map_err(|error| format!("cannot read runtime allowlist: {error}"))?;
        Some(RuntimeAllowlist::from_json(&bytes)?)
    } else {
        None
    };
    Ok(Some(PreflightRequest {
        image: image.to_string(),
        policy,
        allowlist,
    }))
}

/// `pub(crate)` so the rescore command binds an image reference to the same
/// pinning rule the preflight applies, rather than restating it.
pub(crate) fn validate_pinned_reference(image: &str) -> Result<(), String> {
    let Some((repository, digest)) = image.rsplit_once("@sha256:") else {
        return Err(
            "a scanned image must be pinned as <repository>@sha256:<64 lowercase hex>".into(),
        );
    };
    if repository.is_empty()
        || repository.starts_with('-')
        || image.chars().any(char::is_whitespace)
        || digest.len() != 64
        || !digest
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    {
        return Err(
            "a scanned image must be pinned as <repository>@sha256:<64 lowercase hex>".into(),
        );
    }
    Ok(())
}

/// Duplicated from `main.rs` so this module stays independent of argument
/// plumbing; three similar lines beat threading a parser through.
fn flag_value<'a>(args: &'a [String], flag: &str) -> Option<&'a str> {
    args.iter()
        .position(|arg| arg == flag)
        .and_then(|index| args.get(index + 1))
        .map(String::as_str)
}

struct InspectedImage {
    id: ValidatedImageId,
    config: Value,
}

/// Validate the pinned image document Docker returned.
///
/// Docker 29 omits empty configuration fields entirely, so the shape check is
/// "an object", not a list of required keys. `Volumes` is the field that
/// decides scope: it is absent on an image with no `VOLUME`, `{"/path":{}}`
/// when one is declared, and `null` on older daemons. Absent, null and empty
/// all mean "no declared volumes"; anything else refuses, because a container
/// export leaves volume contents out and a filesystem scan would then be
/// reported over a scope it did not cover.
fn validate_image_document(document: &Value) -> Result<InspectedImage, PreflightFailure> {
    let id = document
        .get("Id")
        .and_then(Value::as_str)
        .and_then(ValidatedImageId::parse)
        .ok_or_else(|| {
            failure(
                "image_inspect",
                "docker did not report a full lowercase sha256 configuration ID for the image",
            )
        })?;
    if document.get("Os").and_then(Value::as_str) != Some("linux") {
        return Err(failure(
            "image_inspect",
            "only Linux images are in scope for the container-export snapshot",
        ));
    }
    let config = document.get("Config").ok_or_else(|| {
        failure(
            "image_inspect",
            "the inspected image carries no executable configuration to scan",
        )
    })?;
    if !config.is_object() {
        return Err(failure(
            "image_inspect",
            "the inspected image configuration is not an object",
        ));
    }
    match config.get("Volumes") {
        None | Some(Value::Null) => {}
        Some(Value::Object(volumes)) if volumes.is_empty() => {}
        Some(Value::Object(_)) => {
            return Err(failure(
                "image_inspect",
                "the image declares volumes and a container export omits their contents, so the \
                 declared scan scope cannot be covered",
            ))
        }
        Some(_) => {
            return Err(failure(
                "image_inspect",
                "the inspected image reports declared volumes in an unexpected shape",
            ))
        }
    }
    Ok(InspectedImage {
        id,
        config: config.clone(),
    })
}

/// Scan the serialized configuration and, separately, every decoded string and
/// object key inside it.
///
/// A protected sequence containing a newline appears in the serialized document
/// as the two bytes `\` and `n`, so a raw search over serialized JSON alone
/// would miss it. Feeding the decoded values closes that, and the same is true
/// of any other JSON escape the daemon emits.
fn scan_configuration(config: &Value, policy: RawScanPolicy) -> RawScanReport {
    let mut scanner = RawScanner::new(policy);
    let serialized = serde_json::to_vec(config).expect("an inspected configuration re-serializes");
    if !scanner.scan_file(
        b"image/config/serialized",
        serialized.len() as u64,
        &serialized[..],
    ) {
        return scanner.finish();
    }
    let mut decoded = Vec::new();
    collect_text(config, &mut decoded);
    for (index, text) in decoded.iter().enumerate() {
        let name = format!("image/config/decoded/{index}");
        if !scanner.scan_file(name.as_bytes(), text.len() as u64, text.as_bytes()) {
            break;
        }
    }
    scanner.finish()
}

fn collect_text(value: &Value, out: &mut Vec<String>) {
    match value {
        Value::String(text) => {
            if !text.is_empty() {
                out.push(text.clone());
            }
        }
        Value::Array(items) => items.iter().for_each(|item| collect_text(item, out)),
        Value::Object(fields) => {
            for (key, item) in fields {
                if !key.is_empty() {
                    out.push(key.clone());
                }
                collect_text(item, out);
            }
        }
        _ => {}
    }
}

fn read_capture_json(
    capture: &mut Capture,
    accepted: u64,
    stage: &'static str,
) -> Result<Value, PreflightFailure> {
    if capture.stdout_bytes > accepted {
        return Err(failure(stage, "docker output exceeded its accepted size"));
    }
    let mut bytes = Vec::new();
    (&mut capture.stdout)
        .take(accepted)
        .read_to_end(&mut bytes)
        .map_err(|error| failure(stage, format!("cannot read the docker capture: {error}")))?;
    if bytes.len() as u64 != capture.stdout_bytes {
        return Err(failure(
            stage,
            "the docker capture changed while it was read",
        ));
    }
    serde_json::from_slice(&bytes)
        .map_err(|_| failure(stage, "docker did not return a JSON document"))
}

fn capture_json(
    docker: &dyn DockerTransport,
    args: &[String],
    accepted: u64,
    deadline: Instant,
    stage: &'static str,
) -> Result<Value, PreflightFailure> {
    let mut capture = docker
        .capture(args, accepted, deadline)
        .map_err(|error| failure(stage, error))?;
    if !capture.success {
        note_diagnostic(&capture);
        let mut refusal = failure(stage, "docker refused the request");
        refusal.docker_stderr_bytes = capture.stderr_bytes;
        return Err(refusal);
    }
    read_capture_json(&mut capture, accepted, stage)
}

/// Attempt removal by the reserved name and report whether it is verified.
///
/// A nonzero result is not read as "already clean": Docker returns nonzero both
/// for a container that never existed and for one it could not remove, and the
/// two cannot be told apart from the exit status. Uncertainty refuses.
fn remove_container(docker: &dyn DockerTransport, name: &str) -> bool {
    let args = owned(&["rm", "--force", "--volumes", name]);
    let deadline = Instant::now() + CLEANUP_ALLOWANCE;
    match docker.capture(&args, MAX_CONTROL_RESPONSE_BYTES, deadline) {
        Ok(capture) => capture.success,
        Err(_) => false,
    }
}

fn validate_container_document(
    document: &Value,
    image_id: &ValidatedImageId,
) -> Result<(), &'static str> {
    if document.get("Image").and_then(Value::as_str) != Some(image_id.as_str()) {
        return Err("the created container does not carry the inspected image ID");
    }
    if document
        .pointer("/State/Status")
        .and_then(Value::as_str)
        .is_none_or(|status| status != "created")
    {
        return Err("the snapshot container is not in the created state");
    }
    if document.pointer("/State/Running").and_then(Value::as_bool) != Some(false) {
        return Err("the snapshot container does not report a stopped process");
    }
    match document.get("Mounts") {
        None | Some(Value::Null) => Ok(()),
        Some(Value::Array(mounts)) if mounts.is_empty() => Ok(()),
        Some(_) => Err("the snapshot container carries mounts, whose contents an export omits"),
    }
}

/// Capture and scan one pinned image without ever starting its entrypoint.
///
/// Limitations, stated rather than papered over: the wall-clock deadline ends
/// the client this process spawned. It cannot interrupt a blocked OS read, it
/// does not reach Docker CLI descendants, and it does not stop daemon-side work
/// that continues after the client is killed, which is why removal is attempted
/// on every exit path and why an unverified removal refuses. A remote or
/// rootless Docker context is the operator's configuration and is trusted as
/// infrastructure; the environment this process inherits reaches the client
/// unchanged, so an operator who points `DOCKER_HOST` elsewhere has scanned an
/// image on that host, not this one.
pub fn preflight_image(
    request: &PreflightRequest,
    docker: &dyn DockerTransport,
) -> Result<ImagePreflightReport, PreflightFailure> {
    let scan_policy_sha256 = request.policy.digest();
    let policy_sha256 = match &request.allowlist {
        None => scan_policy_sha256.clone(),
        Some(allowlist) => sharpebench_attest::content_digest(
            &serde_json::to_vec(&(
                COMBINED_POLICY_VERSION,
                &scan_policy_sha256,
                allowlist.digest(),
            ))
            .expect("two digests serialize"),
        ),
    };
    let deadline = Instant::now() + Duration::from_secs(request.policy.limits().max_seconds);

    let document = capture_json(
        docker,
        &owned(&["image", "inspect", "--format", "{{json .}}", &request.image]),
        MAX_INSPECT_BYTES,
        deadline,
        "image_inspect",
    )?;
    let image = validate_image_document(&document)?;

    let configuration = scan_configuration(&image.config, request.policy.clone());
    let report = |configuration, filesystem, cleanup_verified| ImagePreflightReport {
        schema_version: REPORT_VERSION,
        scope: SCAN_SCOPE,
        image_id: image.id.as_str().to_string(),
        policy_sha256: policy_sha256.clone(),
        configuration,
        filesystem,
        cleanup_verified,
        scan_policy_sha256: request
            .allowlist
            .as_ref()
            .map(|_| scan_policy_sha256.clone()),
        runtime_allowlist: None,
        functional_probe: None,
    };
    if !configuration.no_known_matches() {
        // Nothing was created, so nothing can leak. Refusing here is the point
        // of scanning the configuration first.
        return Ok(report(configuration, None, true));
    }

    let name = reserved_container_name();
    let created = docker.capture(
        &create_args(&name, image.id.as_str()),
        MAX_CONTROL_RESPONSE_BYTES,
        deadline,
    );
    // From here every exit path attempts removal by the reserved name, including
    // the path where create's outcome is unknown.
    let create_failed = match &created {
        Ok(capture) if capture.success => None,
        Ok(capture) => {
            note_diagnostic(capture);
            Some(capture.stderr_bytes)
        }
        Err(_) => Some(0),
    };
    if let Some(stderr_bytes) = create_failed {
        let cleanup_verified = remove_container(docker, &name);
        return Err(PreflightFailure {
            schema_version: FAILURE_VERSION,
            stage: "container_create",
            message: "docker could not create the stopped snapshot container".into(),
            cleanup_verified,
            docker_stderr_bytes: stderr_bytes,
        });
    }

    let inspected = match capture_json(
        docker,
        &owned(&["inspect", "--format", "{{json .}}", &name]),
        MAX_INSPECT_BYTES,
        deadline,
        "container_inspect",
    ) {
        Ok(inspected) => inspected,
        Err(mut error) => {
            error.cleanup_verified = remove_container(docker, &name);
            return Err(error);
        }
    };
    if let Err(reason) = validate_container_document(&inspected, &image.id) {
        let cleanup_verified = remove_container(docker, &name);
        return Err(PreflightFailure {
            schema_version: FAILURE_VERSION,
            stage: "container_inspect",
            message: reason.into(),
            cleanup_verified,
            docker_stderr_bytes: 0,
        });
    }

    let exported = docker.capture(
        &owned(&["export", &name]),
        request.policy.limits().max_total_bytes,
        deadline,
    );
    let snapshot = match exported {
        Ok(capture) if capture.success => capture.stdout,
        other => {
            let stderr_bytes = other
                .map(|capture| {
                    note_diagnostic(&capture);
                    capture.stderr_bytes
                })
                .unwrap_or(0);
            let cleanup_verified = remove_container(docker, &name);
            return Err(PreflightFailure {
                schema_version: FAILURE_VERSION,
                stage: "container_export",
                message: "docker could not export the stopped snapshot container".into(),
                cleanup_verified,
                docker_stderr_bytes: stderr_bytes,
            });
        }
    };
    // A second handle on the same owned capture, for the allowlist listing.
    let listing = request
        .allowlist
        .as_ref()
        .map(|_| snapshot.try_clone().ok());
    // The same policy deadline that bounded the capture bounds the scan.
    let filesystem = scan_tar_snapshot_until(snapshot, request.policy.clone(), deadline);
    let cleanup_verified = remove_container(docker, &name);
    let mut report = report(configuration, Some(filesystem), cleanup_verified);
    if let (Some(allowlist), Some(listing)) = (&request.allowlist, listing) {
        // Names are read only from an archive the scan enumerated completely
        // and found clean, so its structure and metadata sizes are already
        // validated when the listing walks it.
        if report
            .filesystem
            .as_ref()
            .is_some_and(TarScanReport::no_known_matches)
        {
            let checked = check_allowlist(listing, allowlist, deadline);
            let admitted = checked.admits_everything();
            report.runtime_allowlist = Some(checked);
            if admitted && report.cleanup_verified {
                report.functional_probe = Some(functional_probe(docker, &image.id));
            }
        }
    }
    Ok(report)
}

/// Walk the export's entry names against the allowlist. An unreadable
/// listing, an unreadable name or an expired deadline leaves the report
/// incomplete, and an incomplete report never admits.
fn check_allowlist(
    listing: Option<File>,
    allowlist: &RuntimeAllowlist,
    deadline: Instant,
) -> AllowlistReport {
    let mut report = AllowlistReport {
        allowlist_sha256: allowlist.digest().to_string(),
        entries: 0,
        docker_init_entries: 0,
        outside_allowlist: 0,
        outside_indices: Vec::new(),
        complete: false,
    };
    let Some(mut listing) = listing else {
        return report;
    };
    if listing.seek(SeekFrom::Start(0)).is_err() {
        return report;
    }
    let mut archive = tar::Archive::new(listing);
    let Ok(entries) = archive.entries() else {
        return report;
    };
    for entry in entries {
        if Instant::now() >= deadline {
            return report;
        }
        let Ok(entry) = entry else {
            return report;
        };
        let kind = entry.header().entry_type();
        let admitted = std::str::from_utf8(&entry.path_bytes()).is_ok_and(|path| {
            allowlist.admits(path, kind.is_dir()) || {
                // An unreadable size is never "empty".
                let size = entry.header().size().unwrap_or(u64::MAX);
                let docker =
                    is_docker_init_entry(path, kind, size, entry.link_name_bytes().as_deref());
                report.docker_init_entries += u64::from(docker);
                docker
            }
        });
        if !admitted {
            if report.outside_indices.len() < MAX_REPORTED_OUTSIDE {
                report.outside_indices.push(report.entries);
            }
            report.outside_allowlist += 1;
        }
        report.entries += 1;
    }
    report.complete = true;
    report
}

/// Run the admitted image once, from its configuration ID and under the
/// hardened launch a sweep uses, and require a valid decision for
/// [`PROBE_OBSERVATION`]. The container is removed by name afterwards, and an
/// unverified removal fails the probe.
fn functional_probe(
    docker: &dyn DockerTransport,
    image_id: &ValidatedImageId,
) -> FunctionalProbeReport {
    let observation_sha256 = sharpebench_attest::content_digest(PROBE_OBSERVATION.as_bytes());
    let options = sharpebench_arena::SandboxOptions {
        allow_unpinned_image: true,
        ..sharpebench_arena::SandboxOptions::default()
    };
    let launch =
        match sharpebench_arena::sandbox::plan_gateway_launch(true, image_id.as_str(), &options) {
            Ok(launch) => launch,
            Err(_) => {
                return FunctionalProbeReport {
                    observation_sha256,
                    passed: false,
                    refusal: Some("launch_refused"),
                    cleanup_verified: true,
                }
            }
        };
    let Some(name) = launch.container.clone() else {
        return FunctionalProbeReport {
            observation_sha256,
            passed: false,
            refusal: Some("launch_refused"),
            cleanup_verified: true,
        };
    };
    let mut input = PROBE_OBSERVATION.as_bytes().to_vec();
    input.push(b'\n');
    let captured = docker.probe(
        &launch.args,
        &input,
        MAX_PROBE_OUTPUT_BYTES,
        Instant::now() + PROBE_ALLOWANCE,
    );
    let cleanup_verified = remove_container(docker, &name);
    let refusal = match captured {
        Err(_) => Some("probe_did_not_complete"),
        Ok(mut capture) => first_decision_refusal(&mut capture),
    };
    FunctionalProbeReport {
        observation_sha256,
        passed: refusal.is_none() && cleanup_verified,
        refusal,
        cleanup_verified,
    }
}

/// `None` when the first line the image wrote is a decision valid for the
/// probe observation; otherwise why not.
fn first_decision_refusal(capture: &mut Capture) -> Option<&'static str> {
    if capture.stdout_bytes > MAX_PROBE_OUTPUT_BYTES {
        return Some("probe_output_exceeded");
    }
    let mut bytes = Vec::new();
    if (&mut capture.stdout)
        .take(MAX_PROBE_OUTPUT_BYTES)
        .read_to_end(&mut bytes)
        .is_err()
    {
        return Some("probe_output_unreadable");
    }
    let Some(line) = bytes
        .split(|byte| *byte == b'\n')
        .next()
        .and_then(|line| std::str::from_utf8(line).ok())
        .map(|line| line.trim_end_matches('\r'))
        .filter(|line| !line.is_empty())
    else {
        return Some("no_decision");
    };
    let observation: sharpebench_protocol::MarketObservation =
        serde_json::from_str(PROBE_OBSERVATION).expect("the probe observation is valid");
    match sharpebench_protocol::decision_from_wire(line) {
        Ok(decision) if decision.validate_for(&observation).is_ok() => None,
        _ => Some("invalid_decision"),
    }
}

/// The CLI entry point: parse the opt-in, then run it.
///
/// Argument and policy validation happens entirely before the transport is
/// touched, so a conflicting transport or an unusable policy costs no Docker
/// invocation at all.
pub fn preflight_from_args(
    args: &[String],
    docker: &dyn DockerTransport,
) -> Result<Option<ImagePreflightReport>, PreflightFailure> {
    let request = parse_preflight_args(args).map_err(|error| failure("arguments", error))?;
    match request {
        None => Ok(None),
        Some(request) => preflight_image(&request, docker).map(Some),
    }
}

/// The entrant material a scanned run binds into its checkpoint identity.
///
/// The image configuration ID is the deployment identity, so it is bound along
/// with the policy digest and the scanned scope. Export timestamps are not:
/// they change on every capture and would make a resumed sweep look like a
/// different experiment. An unscanned run keeps its legacy material untouched.
pub fn scanned_invocation_material(
    label: &str,
    image_id: &str,
    policy_sha256: &str,
) -> Result<Vec<u8>, String> {
    serde_json::to_vec(&(
        INVOCATION_VERSION,
        label,
        image_id,
        policy_sha256,
        SCAN_SCOPE,
    ))
    .map_err(|error| format!("cannot frame the scanned invocation identity: {error}"))
}

/// Refuse a resume whose checkpoint was bound to a different contract.
///
/// The sweep layer treats a contract mismatch as "start a fresh sweep", which
/// would overwrite the file. A scanned run must not silently do that when the
/// scan policy changed, so the CLI checks first and leaves the bytes alone.
pub fn checkpoint_admits(
    path: &Path,
    agent_id: &str,
    contract: &SweepContract,
) -> Result<(), String> {
    if !path.exists() {
        return Ok(());
    }
    let existing = SweepCheckpoint::load(path)
        .map_err(|error| format!("cannot read the existing checkpoint: {error}"))?;
    if existing.matches_bound(agent_id, contract) {
        return Ok(());
    }
    Err(format!(
        "the checkpoint at {} was bound to a different scanned invocation; a changed scan policy, \
         image identity or execution is a new experiment, not a resume",
        path.display()
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::io::Write;

    const POLICY: &str = r#"{"schema_version":"sharpebench.raw-scan-policy.v1","utf8_sequences":["sharpebench-protected-canary"]}"#;

    fn policy() -> RawScanPolicy {
        RawScanPolicy::from_json(POLICY.as_bytes()).expect("the fixture policy validates")
    }

    fn request(image: &str) -> PreflightRequest {
        PreflightRequest {
            image: image.to_string(),
            policy: policy(),
            allowlist: None,
        }
    }

    fn pinned() -> String {
        format!("registry.example/agent@sha256:{}", "b".repeat(64))
    }

    fn image_id() -> String {
        format!("sha256:{}", "c".repeat(64))
    }

    fn spooled(bytes: &[u8]) -> File {
        let mut file = tempfile::tempfile().expect("a capture file opens");
        file.write_all(bytes)
            .expect("the capture file accepts bytes");
        file.seek(SeekFrom::Start(0)).expect("the capture rewinds");
        file
    }

    fn ok(bytes: &[u8]) -> Result<Capture, String> {
        Ok(Capture {
            success: true,
            stdout: spooled(bytes),
            stdout_bytes: bytes.len() as u64,
            redacted_stderr: String::new(),
            stderr_bytes: 0,
        })
    }

    fn refused() -> Result<Capture, String> {
        Ok(Capture {
            success: false,
            stdout: spooled(b""),
            stdout_bytes: 0,
            redacted_stderr: "docker: no".into(),
            stderr_bytes: 10,
        })
    }

    /// A TAR archive with one regular entry, built by hand so the fixture does
    /// not depend on a Docker daemon.
    fn tar_with(path: &str, body: &[u8]) -> Vec<u8> {
        let mut header = tar::Header::new_gnu();
        header.set_path(path).expect("the fixture path fits");
        header.set_size(body.len() as u64);
        header.set_mode(0o644);
        header.set_cksum();
        let mut archive = Vec::new();
        archive.extend_from_slice(header.as_bytes());
        archive.extend_from_slice(body);
        archive.resize(archive.len().div_ceil(512) * 512, 0);
        archive.extend_from_slice(&[0; 1024]);
        archive
    }

    /// A TAR archive with several entries; a path ending in `/` is a directory.
    fn tar_entries(entries: &[(&str, &[u8])]) -> Vec<u8> {
        let mut archive = Vec::new();
        for (path, body) in entries {
            let mut header = tar::Header::new_gnu();
            header.set_path(path).expect("the fixture path fits");
            if path.ends_with('/') {
                header.set_entry_type(tar::EntryType::Directory);
                header.set_size(0);
                header.set_mode(0o755);
            } else {
                header.set_size(body.len() as u64);
                header.set_mode(0o644);
            }
            header.set_cksum();
            archive.extend_from_slice(header.as_bytes());
            if !path.ends_with('/') {
                archive.extend_from_slice(body);
                archive.resize(archive.len().div_ceil(512) * 512, 0);
            }
        }
        archive.extend_from_slice(&[0; 1024]);
        archive
    }

    struct Fake {
        calls: RefCell<Vec<String>>,
        image_inspect: String,
        container_inspect: String,
        export: Vec<u8>,
        create_ok: bool,
        export_ok: bool,
        remove_ok: bool,
        /// What the image writes to stdout when probed; `None` fails the probe
        /// at the transport.
        probe_output: Option<Vec<u8>>,
        probe_input: RefCell<Vec<u8>>,
        /// Whether removing the probe's container succeeds, independently of
        /// the snapshot container.
        probe_remove_ok: bool,
    }

    impl Fake {
        fn new() -> Self {
            Self {
                calls: RefCell::new(Vec::new()),
                image_inspect: format!(
                    r#"{{"Id":"{}","Os":"linux","Config":{{"Env":["PATH=/usr/bin"],"Cmd":["/bin/sh"]}}}}"#,
                    image_id()
                ),
                container_inspect: format!(
                    r#"{{"Image":"{}","State":{{"Status":"created","Running":false}},"Mounts":[]}}"#,
                    image_id()
                ),
                export: tar_with("etc/hostname", b"clean\n"),
                create_ok: true,
                export_ok: true,
                remove_ok: true,
                probe_output: Some(br#"{"orders":[],"reasoning":"flat"}"#.to_vec()),
                probe_input: RefCell::new(Vec::new()),
                probe_remove_ok: true,
            }
        }

        fn subcommands(&self) -> Vec<String> {
            self.calls.borrow().clone()
        }
    }

    impl DockerTransport for Fake {
        fn capture(
            &self,
            args: &[String],
            _accepted_stdout_bytes: u64,
            _deadline: Instant,
        ) -> Result<Capture, String> {
            self.calls.borrow_mut().push(args.join(" "));
            match args[0].as_str() {
                "image" => ok(self.image_inspect.as_bytes()),
                "inspect" => ok(self.container_inspect.as_bytes()),
                "create" if self.create_ok => ok(b"containerid\n"),
                "create" => refused(),
                "export" if self.export_ok => ok(&self.export),
                "export" => refused(),
                "rm" if !self.probe_remove_ok
                    && args
                        .last()
                        .is_some_and(|name| name.starts_with("sharpebench-agent-")) =>
                {
                    refused()
                }
                "rm" if self.remove_ok => ok(b"containerid\n"),
                "rm" => refused(),
                other => panic!("the preflight issued an unexpected docker subcommand {other}"),
            }
        }

        fn probe(
            &self,
            args: &[String],
            stdin: &[u8],
            _accepted_stdout_bytes: u64,
            _deadline: Instant,
        ) -> Result<Capture, String> {
            self.calls
                .borrow_mut()
                .push(format!("probe {}", args.join(" ")));
            self.probe_input.borrow_mut().extend_from_slice(stdin);
            match &self.probe_output {
                Some(output) => ok(output),
                None => Err("the probe did not complete".into()),
            }
        }
    }

    fn removal_attempted(fake: &Fake) -> bool {
        fake.subcommands()
            .iter()
            .any(|call| call.starts_with("rm --force --volumes sharpebench-preflight-"))
    }

    fn allowlist(paths: &[&str]) -> RuntimeAllowlist {
        RuntimeAllowlist::from_json(
            serde_json::json!({"schema_version": ALLOWLIST_VERSION, "paths": paths})
                .to_string()
                .as_bytes(),
        )
        .expect("the fixture allowlist validates")
    }

    fn allowlisted(paths: &[&str]) -> PreflightRequest {
        PreflightRequest {
            allowlist: Some(allowlist(paths)),
            ..request(&pinned())
        }
    }

    fn probed(fake: &Fake) -> bool {
        fake.subcommands()
            .iter()
            .any(|call| call.starts_with("probe "))
    }

    /// Allowlist polarity end to end: every export entry is admitted, the
    /// admitted image is run once against the probe observation under the
    /// hardened launch, answers with a valid decision, and only then is the
    /// launch authorized. The probe container is removed by name.
    #[test]
    fn an_allowlisted_image_is_probed_before_it_is_authorized() {
        let mut fake = Fake::new();
        fake.export = tar_entries(&[
            ("app/", b""),
            ("app/agent", b"#!/bin/sh\n"),
            ("etc/", b""),
            ("etc/hostname", b"clean\n"),
        ]);
        let report = preflight_image(&allowlisted(&["app/", "etc/hostname"]), &fake)
            .expect("the preflight completes");
        let checked = report
            .runtime_allowlist
            .as_ref()
            .expect("the allowlist ran");
        assert_eq!((checked.entries, checked.outside_allowlist), (4, 0));
        assert!(checked.complete);
        let probe = report.functional_probe.as_ref().expect("the probe ran");
        assert!(probe.passed && probe.cleanup_verified, "{probe:?}");
        assert!(report.authorizes_launch(), "{report:?}");

        let calls = fake.subcommands();
        let probe_call = calls
            .iter()
            .find(|call| call.starts_with("probe "))
            .expect("a probe call");
        assert!(probe_call.contains("--network none"), "{probe_call}");
        assert!(probe_call.ends_with(&image_id()), "{probe_call}");
        let snapshot_removed = calls
            .iter()
            .position(|call| call.starts_with("rm --force --volumes sharpebench-preflight-"))
            .expect("the snapshot container is removed");
        let probed_at = calls
            .iter()
            .position(|call| call.starts_with("probe "))
            .expect("probed");
        assert!(
            snapshot_removed < probed_at,
            "the probe runs after the scan legs"
        );
        assert!(calls
            .iter()
            .any(|call| call.starts_with("rm --force --volumes sharpebench-agent-")));
        assert_eq!(
            fake.probe_input.borrow().as_slice(),
            format!("{PROBE_OBSERVATION}\n").as_bytes()
        );
    }

    /// An entry the allowlist does not name refuses, is reported by index
    /// rather than by name, and the image is never started.
    #[test]
    fn an_entry_outside_the_allowlist_refuses_before_anything_runs() {
        let mut fake = Fake::new();
        fake.export = tar_entries(&[
            ("etc/", b""),
            ("etc/hostname", b"clean\n"),
            ("srv/", b""),
            ("srv/window-scores.csv", b"1,2,3\n"),
        ]);
        let report = preflight_image(&allowlisted(&["etc/hostname"]), &fake)
            .expect("the preflight completes");
        let checked = report
            .runtime_allowlist
            .as_ref()
            .expect("the allowlist ran");
        assert_eq!(checked.outside_allowlist, 2);
        assert_eq!(checked.outside_indices, vec![2, 3]);
        assert!(report.functional_probe.is_none());
        assert!(!report.authorizes_launch());
        assert!(!probed(&fake), "a refused image is never started");
        let published = serde_json::to_string(&report).expect("serializes");
        assert!(!published.contains("window-scores"), "{published}");
        assert!(!published.contains("srv/"), "{published}");
    }

    /// The post-strip functional test is a gate: an image that does not answer,
    /// answers with something other than a valid decision, or cannot be removed
    /// afterwards is refused even though every scan leg passed.
    #[test]
    fn an_admitted_image_that_fails_its_functional_probe_refuses() {
        for (output, refusal) in [
            (None, "probe_did_not_complete"),
            (Some(b"".to_vec()), "no_decision"),
            (Some(b"not json\n".to_vec()), "invalid_decision"),
            (
                Some(
                    br#"{"orders":[{"symbol":"AAPL","action":"buy","target_weight":0.5}]}"#
                        .to_vec(),
                ),
                "invalid_decision",
            ),
        ] {
            let mut fake = Fake::new();
            fake.probe_output = output;
            let report = preflight_image(&allowlisted(&["etc/hostname"]), &fake)
                .expect("the preflight completes");
            let probe = report.functional_probe.as_ref().expect("the probe ran");
            assert_eq!(probe.refusal, Some(refusal));
            assert!(!probe.passed && probe.cleanup_verified);
            assert!(!report.authorizes_launch(), "{report:?}");
        }

        let mut fake = Fake::new();
        fake.probe_remove_ok = false;
        let report = preflight_image(&allowlisted(&["etc/hostname"]), &fake)
            .expect("the preflight completes");
        let probe = report.functional_probe.as_ref().expect("the probe ran");
        assert_eq!(probe.refusal, None, "the image answered");
        assert!(!probe.passed && !probe.cleanup_verified);
        assert!(!report.authorizes_launch());

        let mut fake = Fake::new();
        fake.probe_output = Some(br#"{"orders":[]}"#.to_vec());
        let report = preflight_image(&allowlisted(&["etc/hostname"]), &fake)
            .expect("the preflight completes");
        assert!(report.authorizes_launch());
    }

    /// Without an allowlist the report and the policy digest are exactly what
    /// they were; with one, the digest binds both inputs, so a changed
    /// allowlist is a changed experiment.
    #[test]
    fn the_policy_digest_binds_the_allowlist_only_when_one_applies() {
        let plain = preflight_image(&request(&pinned()), &Fake::new()).expect("completes");
        assert_eq!(plain.policy_sha256, policy().digest());
        let published = serde_json::to_value(&plain).expect("serializes");
        for absent in [
            "scan_policy_sha256",
            "runtime_allowlist",
            "functional_probe",
        ] {
            assert!(
                published.get(absent).is_none(),
                "{absent} leaked into a legacy report"
            );
        }
        assert!(!probed(&Fake::new()));

        let one = preflight_image(&allowlisted(&["etc/hostname"]), &Fake::new()).expect("ok");
        let two =
            preflight_image(&allowlisted(&["etc/hostname", "app/"]), &Fake::new()).expect("ok");
        assert_eq!(
            one.scan_policy_sha256.as_deref(),
            Some(policy().digest().as_str())
        );
        assert_ne!(one.policy_sha256, plain.policy_sha256);
        assert_ne!(one.policy_sha256, two.policy_sha256);
    }

    #[test]
    fn the_allowlist_admits_subtrees_exact_entries_and_their_ancestors_only() {
        let list = allowlist(&["usr/lib/python3/", "etc/hostname"]);
        for (path, directory, admitted) in [
            ("usr/lib/python3/", true, true),
            ("usr/lib/python3/os.py", false, true),
            ("./usr/lib/python3/json/", true, true),
            ("usr/", true, true),
            ("usr/lib/", true, true),
            ("etc/", true, true),
            ("etc/hostname", false, true),
            ("./", true, true),
            ("usr/lib/python3x", false, false),
            ("usr/lib/other.so", false, false),
            ("usr/lib", false, false),
            ("etc/hostname/", true, true),
            ("etc/hostname/nested", false, false),
            ("etc/passwd", false, false),
            ("srv/", true, false),
        ] {
            assert_eq!(list.admits(path, directory), admitted, "{path}");
        }
    }

    /// A TAR archive of typed entries: `('f', path, body)` is a regular file,
    /// `('d', path, _)` a directory and `('l', path, target)` a symlink.
    fn tar_typed(entries: &[(char, &str, &[u8])]) -> Vec<u8> {
        let mut archive = Vec::new();
        for (kind, path, body) in entries {
            let mut header = tar::Header::new_gnu();
            header.set_path(path).expect("the fixture path fits");
            header.set_mode(0o755);
            match kind {
                'd' => {
                    header.set_entry_type(tar::EntryType::Directory);
                    header.set_size(0);
                }
                'l' => {
                    header.set_entry_type(tar::EntryType::Symlink);
                    header.set_size(0);
                    header
                        .set_link_name(std::str::from_utf8(body).expect("a UTF-8 target"))
                        .expect("the fixture target fits");
                }
                _ => header.set_size(body.len() as u64),
            }
            header.set_cksum();
            archive.extend_from_slice(header.as_bytes());
            if *kind == 'f' {
                archive.extend_from_slice(body);
                archive.resize(archive.len().div_ceil(512) * 512, 0);
            }
        }
        archive.extend_from_slice(&[0; 1024]);
        archive
    }

    /// Every entry Docker's init layer adds to a created container, in the
    /// shape the live daemon gives it.
    const DOCKER_INIT_EXPORT: [(char, &str, &[u8]); 12] = [
        ('f', ".dockerenv", b""),
        ('d', "dev/", b""),
        ('f', "dev/console", b""),
        ('d', "dev/pts/", b""),
        ('d', "dev/shm/", b""),
        ('d', "etc/", b""),
        ('f', "etc/hostname", b""),
        ('f', "etc/hosts", b""),
        ('l', "etc/mtab", b"/proc/mounts"),
        ('f', "etc/resolv.conf", b""),
        ('d', "proc/", b""),
        ('d', "sys/", b""),
    ];

    /// The daemon's own init-layer entries need no allowlist line, because an
    /// export always holds them and they carry no image content. The same
    /// paths carrying bytes, of another type or linking elsewhere are image
    /// content and still refuse by index.
    #[test]
    fn docker_init_entries_are_admitted_only_in_the_shape_docker_gives_them() {
        let mut entries = DOCKER_INIT_EXPORT.to_vec();
        entries.extend([('d', "app/", &b""[..]), ('f', "app/agent", b"#!/bin/sh\n")]);
        let mut fake = Fake::new();
        fake.export = tar_typed(&entries);
        let report =
            preflight_image(&allowlisted(&["app/"]), &fake).expect("the preflight completes");
        let checked = report
            .runtime_allowlist
            .as_ref()
            .expect("the allowlist ran");
        assert_eq!(
            (
                checked.entries,
                checked.docker_init_entries,
                checked.outside_allowlist
            ),
            (14, 12, 0),
            "{checked:?}"
        );
        assert!(report.authorizes_launch(), "{report:?}");

        for (index, content) in [
            ('f', ".dockerenv", &b"x"[..]),
            ('f', "etc/hosts", b"10.0.0.1 held-out-store\n"),
            ('f', "etc/resolv.conf", b"nameserver 10.0.0.1\n"),
            ('l', "etc/mtab", b"/srv/window-scores.csv"),
            ('f', "etc/mtab", b""),
            ('f', "proc", b""),
            ('d', "etc/hostname/", b""),
            ('l', "dev/console", b"/proc/mounts"),
        ]
        .into_iter()
        .enumerate()
        {
            let mut fake = Fake::new();
            fake.export = tar_typed(&[('d', "app/", b""), content]);
            let report =
                preflight_image(&allowlisted(&["app/"]), &fake).expect("the preflight completes");
            let checked = report
                .runtime_allowlist
                .as_ref()
                .expect("the allowlist ran");
            assert_eq!(
                (checked.docker_init_entries, checked.outside_allowlist),
                (0, 1),
                "case {index}: {checked:?}"
            );
            assert_eq!(checked.outside_indices, vec![1], "case {index}");
            assert!(!report.authorizes_launch(), "case {index}");
            assert!(
                !probed(&fake),
                "case {index}: a refused image is never started"
            );
        }

        // Below an init mount point is image content, not Docker's.
        let mut fake = Fake::new();
        fake.export = tar_typed(&[('d', "proc/", b""), ('f', "proc/cached", b"")]);
        let report =
            preflight_image(&allowlisted(&["app/"]), &fake).expect("the preflight completes");
        let checked = report
            .runtime_allowlist
            .as_ref()
            .expect("the allowlist ran");
        assert_eq!(checked.outside_indices, vec![1], "{checked:?}");
    }

    #[test]
    fn a_malformed_allowlist_is_refused() {
        for body in [
            r#"{"schema_version":"sharpebench.runtime-allowlist.v0","paths":["app/"]}"#,
            r#"{"schema_version":"sharpebench.runtime-allowlist.v1","paths":[]}"#,
            r#"{"schema_version":"sharpebench.runtime-allowlist.v1","paths":["/app/"]}"#,
            r#"{"schema_version":"sharpebench.runtime-allowlist.v1","paths":["app/../etc"]}"#,
            r#"{"schema_version":"sharpebench.runtime-allowlist.v1","paths":["app//x"]}"#,
            r#"{"schema_version":"sharpebench.runtime-allowlist.v1","paths":["./app"]}"#,
            r#"{"schema_version":"sharpebench.runtime-allowlist.v1","paths":["/"]}"#,
            r#"{"schema_version":"sharpebench.runtime-allowlist.v1","paths":["app/","app/"]}"#,
            r#"{"schema_version":"sharpebench.runtime-allowlist.v1","paths":["app/"],"extra":1}"#,
        ] {
            assert!(
                RuntimeAllowlist::from_json(body.as_bytes()).is_err(),
                "{body} must be refused"
            );
        }
    }

    /// Generic naming: nothing the preflight names for Docker or hands the
    /// image carries the entrant's reference, the policy or the allowlist.
    /// After the one inspect that resolves it, the image is reached only by its
    /// configuration ID, and the containers are named by process and counter
    /// alone.
    #[test]
    fn host_named_artifacts_carry_no_evaluation_identity() {
        let fake = Fake::new();
        let request = allowlisted(&["etc/hostname"]);
        let report = preflight_image(&request, &fake).expect("completes");
        let allowlist_digest = request
            .allowlist
            .as_ref()
            .expect("set")
            .digest()
            .to_string();
        let calls = fake.subcommands();
        assert!(calls[0].starts_with("image inspect"));
        for call in &calls[1..] {
            assert!(!call.contains("registry.example"), "{call}");
            assert!(!call.contains(&report.policy_sha256), "{call}");
            assert!(!call.contains(&allowlist_digest), "{call}");
            let words: Vec<&str> = call.split(' ').collect();
            if let Some(at) = words.iter().position(|word| *word == "--name") {
                let name = words[at + 1];
                let generic =
                    ["sharpebench-preflight-", "sharpebench-agent-"]
                        .iter()
                        .any(|prefix| {
                            name.strip_prefix(prefix).is_some_and(|rest| {
                                rest.split('-').count() == 2
                                    && rest.split('-').all(|part| {
                                        !part.is_empty() && part.bytes().all(|b| b.is_ascii_digit())
                                    })
                            })
                        });
                assert!(generic, "{name} is not a generic name");
            }
        }
        let observation: sharpebench_protocol::MarketObservation =
            serde_json::from_str(PROBE_OBSERVATION).expect("valid");
        assert_eq!(observation.symbols.len(), 1);
        assert_eq!(observation.symbols[0].symbol, "PROBE");
        assert_eq!(observation.date, "1970-01-01");
    }

    #[test]
    fn an_allowlist_without_a_scan_policy_issues_no_docker_command() {
        let error = refused_arguments(&[
            "run",
            "--image",
            &pinned(),
            "--runtime-allowlist",
            "allow.json",
        ]);
        assert!(error.contains("requires --scan-policy"), "{error}");
    }

    #[test]
    fn a_clean_image_authorizes_the_validated_configuration_id() {
        let fake = Fake::new();
        let report = preflight_image(&request(&pinned()), &fake).expect("a clean image completes");
        assert!(report.authorizes_launch(), "{report:?}");
        assert_eq!(
            report
                .authorized_image_id()
                .map(|id| id.as_str().to_string()),
            Some(image_id())
        );
        assert_eq!(report.scope, SCAN_SCOPE);
        assert!(report.cleanup_verified);
        assert!(removal_attempted(&fake));
    }

    #[test]
    fn a_known_archive_match_refuses_and_still_removes_the_container() {
        let mut fake = Fake::new();
        fake.export = tar_with("etc/secret", b"sharpebench-protected-canary\n");
        let report = preflight_image(&request(&pinned()), &fake).expect("the scan completes");
        assert!(!report.authorizes_launch());
        assert!(report.authorized_image_id().is_none());
        assert!(report.cleanup_verified, "removal must still be verified");
        assert!(removal_attempted(&fake));
    }

    /// The needle carries a real newline. It appears in the serialized document
    /// only as an escape, so this fails if the decoded strings are not scanned.
    #[test]
    fn an_escaped_configuration_match_refuses_before_any_container_is_created() {
        let mut fake = Fake::new();
        fake.image_inspect = format!(
            r#"{{"Id":"{}","Os":"linux","Config":{{"Env":["SECRET=alpha\nsharpebench-protected-canary"]}}}}"#,
            image_id()
        );
        let policy = RawScanPolicy::from_json(
            br#"{"schema_version":"sharpebench.raw-scan-policy.v1","utf8_sequences":["alpha\nsharpebench-protected-canary"]}"#,
        )
        .expect("the escaped-needle policy validates");
        let report = preflight_image(
            &PreflightRequest {
                image: pinned(),
                policy,
                allowlist: None,
            },
            &fake,
        )
        .expect("the configuration scan completes");
        assert!(!report.authorizes_launch());
        assert!(report.filesystem.is_none());
        assert_eq!(fake.subcommands().len(), 1, "{:?}", fake.subcommands());
        assert!(fake.subcommands()[0].starts_with("image inspect"));
    }

    #[test]
    fn declared_volumes_are_refused_before_the_snapshot_is_created() {
        let mut fake = Fake::new();
        fake.image_inspect = format!(
            r#"{{"Id":"{}","Os":"linux","Config":{{"Volumes":{{"/data":{{}}}}}}}}"#,
            image_id()
        );
        let error = preflight_image(&request(&pinned()), &fake).expect_err("volumes refuse");
        assert_eq!(error.stage, "image_inspect");
        assert!(error.message.contains("omits their contents"), "{error}");
        assert_eq!(fake.subcommands().len(), 1);
    }

    /// Docker 29 omits the field entirely, older daemons send null, and an
    /// empty object is also "no declared volumes". None of the three refuses.
    #[test]
    fn omitted_null_and_empty_volumes_all_mean_no_declared_volumes() {
        for volumes in ["", r#","Volumes":null"#, r#","Volumes":{}"#] {
            let mut fake = Fake::new();
            fake.image_inspect = format!(
                r#"{{"Id":"{}","Os":"linux","Config":{{"Cmd":["/bin/sh"]{volumes}}}}}"#,
                image_id()
            );
            let report =
                preflight_image(&request(&pinned()), &fake).expect("no declared volumes completes");
            assert!(
                report.authorizes_launch(),
                "{volumes:?} refused: {report:?}"
            );
        }
    }

    #[test]
    fn a_non_linux_image_is_out_of_scope() {
        let mut fake = Fake::new();
        fake.image_inspect = format!(r#"{{"Id":"{}","Os":"windows","Config":{{}}}}"#, image_id());
        let error = preflight_image(&request(&pinned()), &fake).expect_err("non-Linux refuses");
        assert_eq!(error.stage, "image_inspect");
        assert_eq!(fake.subcommands().len(), 1);
    }

    #[test]
    fn a_mutable_image_id_is_never_accepted_as_a_configuration_id() {
        let mut fake = Fake::new();
        fake.image_inspect = r#"{"Id":"alpine:3.22","Os":"linux","Config":{}}"#.to_string();
        let error = preflight_image(&request(&pinned()), &fake).expect_err("an alias refuses");
        assert_eq!(error.stage, "image_inspect");
    }

    #[test]
    fn a_malformed_archive_fails_closed() {
        let mut fake = Fake::new();
        fake.export = vec![0x7f; 900];
        let report = preflight_image(&request(&pinned()), &fake).expect("the scan completes");
        assert!(!report.authorizes_launch());
        assert!(report
            .filesystem
            .as_ref()
            .is_some_and(|scan| !scan.no_known_matches()));
        assert!(removal_attempted(&fake));
    }

    #[test]
    fn a_create_error_fails_closed_and_still_attempts_removal() {
        let mut fake = Fake::new();
        fake.create_ok = false;
        let error =
            preflight_image(&request(&pinned()), &fake).expect_err("a create error refuses");
        assert_eq!(error.stage, "container_create");
        assert!(removal_attempted(&fake));
    }

    #[test]
    fn an_export_error_fails_closed_and_still_attempts_removal() {
        let mut fake = Fake::new();
        fake.export_ok = false;
        let error =
            preflight_image(&request(&pinned()), &fake).expect_err("an export error refuses");
        assert_eq!(error.stage, "container_export");
        assert!(removal_attempted(&fake));
    }

    #[test]
    fn a_cleanup_error_cannot_authorize_a_launch() {
        let mut fake = Fake::new();
        fake.remove_ok = false;
        let report = preflight_image(&request(&pinned()), &fake).expect("the scan completes");
        assert!(report.configuration.no_known_matches());
        assert!(report
            .filesystem
            .as_ref()
            .is_some_and(TarScanReport::no_known_matches));
        assert!(!report.cleanup_verified);
        assert!(!report.authorizes_launch(), "an unverified removal refuses");
        assert!(report.authorized_image_id().is_none());
    }

    #[test]
    fn a_container_built_from_another_image_refuses_before_export() {
        let mut fake = Fake::new();
        fake.container_inspect = format!(
            r#"{{"Image":"sha256:{}","State":{{"Status":"created","Running":false}},"Mounts":[]}}"#,
            "d".repeat(64)
        );
        let error = preflight_image(&request(&pinned()), &fake).expect_err("a swap refuses");
        assert_eq!(error.stage, "container_inspect");
        assert!(!fake
            .subcommands()
            .iter()
            .any(|call| call.starts_with("export")));
        assert!(removal_attempted(&fake));
    }

    #[test]
    fn a_running_container_refuses_before_export() {
        let mut fake = Fake::new();
        fake.container_inspect = format!(
            r#"{{"Image":"{}","State":{{"Status":"running","Running":true}},"Mounts":[]}}"#,
            image_id()
        );
        let error = preflight_image(&request(&pinned()), &fake).expect_err("a running one refuses");
        assert_eq!(error.stage, "container_inspect");
        assert!(!fake
            .subcommands()
            .iter()
            .any(|call| call.starts_with("export")));
    }

    #[test]
    fn a_mounted_container_refuses_before_export() {
        let mut fake = Fake::new();
        fake.container_inspect = format!(
            r#"{{"Image":"{}","State":{{"Status":"created","Running":false}},"Mounts":[{{"Type":"volume"}}]}}"#,
            image_id()
        );
        let error = preflight_image(&request(&pinned()), &fake).expect_err("a mount refuses");
        assert_eq!(error.stage, "container_inspect");
        assert!(!fake
            .subcommands()
            .iter()
            .any(|call| call.starts_with("export")));
    }

    /// The container is created from the configuration ID and its entrypoint is
    /// replaced with an absent path, so nothing the image declares can run.
    #[test]
    fn the_snapshot_container_is_created_from_the_id_and_never_started() {
        let fake = Fake::new();
        preflight_image(&request(&pinned()), &fake).expect("a clean image completes");
        let create = fake
            .subcommands()
            .into_iter()
            .find(|call| call.starts_with("create "))
            .expect("a create call was issued");
        assert!(create.ends_with(&image_id()), "{create}");
        assert!(create.contains("--pull never"), "{create}");
        assert!(create.contains("--network none"), "{create}");
        assert!(create.contains("--ipc none"), "{create}");
        assert!(create.contains("--read-only"), "{create}");
        assert!(create.contains("--cap-drop ALL"), "{create}");
        assert!(create.contains("no-new-privileges=true"), "{create}");
        assert!(
            create.contains(&format!("--entrypoint {NEVER_STARTED_ENTRYPOINT}")),
            "{create}"
        );
        assert!(
            !fake
                .subcommands()
                .iter()
                .any(|call| call.starts_with("start")),
            "the snapshot container must never be started"
        );
    }

    /// Any invocation at all is a failure: these arguments must be refused
    /// before Docker is reached.
    struct Never;

    impl DockerTransport for Never {
        fn capture(&self, args: &[String], _: u64, _: Instant) -> Result<Capture, String> {
            panic!("no docker command may be issued, got {args:?}")
        }
    }

    fn refused_arguments(args: &[&str]) -> String {
        let failure =
            preflight_from_args(&argv(args), &Never).expect_err("these arguments must be refused");
        assert_eq!(failure.stage, "arguments");
        failure.message
    }

    fn policy_file() -> tempfile::NamedTempFile {
        let mut file = tempfile::NamedTempFile::new().expect("a policy file opens");
        file.write_all(POLICY.as_bytes())
            .expect("the policy writes");
        file.flush().expect("the policy flushes");
        file
    }

    fn argv(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| (*value).to_string()).collect()
    }

    #[test]
    fn an_absent_opt_in_leaves_the_legacy_path_untouched() {
        assert!(
            preflight_from_args(&argv(&["run", "--image", &pinned()]), &Never)
                .expect("no policy is not an error")
                .is_none()
        );
    }

    #[test]
    fn a_policy_without_an_image_issues_no_docker_command() {
        let file = policy_file();
        let path = file.path().to_string_lossy().into_owned();
        let error = refused_arguments(&["run", "--scan-policy", &path]);
        assert!(error.contains("requires --image"), "{error}");
    }

    #[test]
    fn a_transport_conflict_issues_no_docker_command() {
        let file = policy_file();
        let path = file.path().to_string_lossy().into_owned();
        let error = refused_arguments(&[
            "run",
            "--image",
            &pinned(),
            "--cmd",
            "./agent",
            "--scan-policy",
            &path,
        ]);
        assert!(error.contains("exactly one transport"), "{error}");
    }

    #[test]
    fn an_unpinned_scanned_image_issues_no_docker_command() {
        let file = policy_file();
        let path = file.path().to_string_lossy().into_owned();
        let error = refused_arguments(&["run", "--image", "agent:latest", "--scan-policy", &path]);
        assert!(error.contains("64 lowercase hex"), "{error}");
    }

    #[test]
    fn a_malformed_policy_issues_no_docker_command() {
        let mut file = tempfile::NamedTempFile::new().expect("a policy file opens");
        file.write_all(b"{\"schema_version\":\"other\"}")
            .expect("the policy writes");
        file.flush().expect("the policy flushes");
        let path = file.path().to_string_lossy().into_owned();
        let error = refused_arguments(&["run", "--image", &pinned(), "--scan-policy", &path]);
        assert!(error.contains("unsupported scan policy version"), "{error}");
    }

    #[test]
    fn an_oversized_policy_issues_no_docker_command() {
        let mut file = tempfile::NamedTempFile::new().expect("a policy file opens");
        file.write_all(&vec![b' '; 64 * 1024 + 8])
            .expect("the policy writes");
        file.flush().expect("the policy flushes");
        let path = file.path().to_string_lossy().into_owned();
        let error = refused_arguments(&["run", "--image", &pinned(), "--scan-policy", &path]);
        assert!(error.contains("64 KiB"), "{error}");
    }

    #[test]
    fn the_scanned_invocation_binds_policy_image_and_scope() {
        let material =
            scanned_invocation_material("sandbox:agent", &image_id(), "policy-digest").unwrap();
        let text = String::from_utf8(material).expect("the framing is UTF-8");
        assert!(text.contains(INVOCATION_VERSION), "{text}");
        assert!(text.contains(&image_id()), "{text}");
        assert!(text.contains("policy-digest"), "{text}");
        assert!(text.contains(SCAN_SCOPE), "{text}");
        assert!(!text.contains("Created"), "no capture timestamp is bound");
    }

    fn contract(invocation: &str) -> SweepContract {
        SweepContract::new(
            sharpebench_harness::SweepIdentity {
                dataset_sha256: "dataset".into(),
                cost_model_sha256: "costs".into(),
                score_config_sha256: "config".into(),
                runner_artifact_sha256: "runner".into(),
                entrant_sha256: "entrant".into(),
                invocation_sha256: invocation.into(),
            },
            &[sharpebench_sim::Window { start: 0, end: 4 }],
            &[0],
            2,
        )
    }

    #[test]
    fn a_changed_scan_policy_refuses_a_resume_without_touching_the_checkpoint() {
        let directory = tempfile::tempdir().expect("a checkpoint directory opens");
        let path = directory.path().join("sweep.json");
        let mut checkpoint = SweepCheckpoint::new("sandbox:agent", 1, &[0]);
        checkpoint.contract = Some(contract("policy-a"));
        checkpoint.save(&path).expect("the checkpoint saves");
        let before = std::fs::read(&path).expect("the checkpoint reads back");

        checkpoint_admits(&path, "sandbox:agent", &contract("policy-a"))
            .expect("the same scanned invocation resumes");
        let error = checkpoint_admits(&path, "sandbox:agent", &contract("policy-b"))
            .expect_err("a changed policy refuses");
        assert!(error.contains("different scanned invocation"), "{error}");
        assert_eq!(
            before,
            std::fs::read(&path).expect("the checkpoint reads back"),
            "a refused resume must not mutate the checkpoint"
        );
    }

    #[test]
    fn an_absent_checkpoint_admits_a_first_run() {
        let directory = tempfile::tempdir().expect("a checkpoint directory opens");
        checkpoint_admits(
            &directory.path().join("absent.json"),
            "sandbox:agent",
            &contract("policy-a"),
        )
        .expect("a first run has nothing to contradict");
    }

    #[test]
    fn accepted_output_bounds_end_a_capture() {
        let (program, args) = shell(&["echo AAAAAAAA"]);
        let error = capture_command(
            program,
            &args,
            None,
            2,
            Instant::now() + Duration::from_secs(20),
        )
        .expect_err("an oversized capture is refused");
        assert!(error.contains("accepted 2 output bytes"), "{error}");
    }

    #[test]
    fn a_nonzero_exit_status_is_surfaced_not_swallowed() {
        let (program, args) = shell(&["exit 3"]);
        let capture = capture_command(
            program,
            &args,
            None,
            1024,
            Instant::now() + Duration::from_secs(20),
        )
        .expect("the capture completes");
        assert!(!capture.success);
    }

    #[test]
    fn a_capture_is_rewound_before_it_is_parsed() {
        let (program, args) = shell(&["echo hello"]);
        let mut capture = capture_command(
            program,
            &args,
            None,
            1024,
            Instant::now() + Duration::from_secs(20),
        )
        .expect("the capture completes");
        assert_eq!(
            capture.stdout.stream_position().expect("the capture seeks"),
            0
        );
        let mut text = String::new();
        capture
            .stdout
            .read_to_string(&mut text)
            .expect("the capture reads");
        assert!(text.starts_with("hello"), "{text:?}");
        assert_eq!(capture.stdout_bytes, text.len() as u64);
    }

    #[test]
    fn an_expired_deadline_kills_and_reaps_the_child() {
        let (program, args) = sleeper();
        let started = Instant::now();
        let error = capture_command(program, &args, None, 1024 * 1024, Instant::now())
            .expect_err("an expired deadline refuses");
        assert!(error.contains("preflight deadline"), "{error}");
        assert!(
            started.elapsed() < Duration::from_secs(20),
            "the child must be killed, not waited out"
        );
    }

    #[test]
    fn untrusted_diagnostics_are_sanitized_and_bounded() {
        assert_eq!(
            redact(b"docker: no such\x1b[31m image\n"),
            "docker: no such.[31m image."
        );
        let long = redact(&vec![b'A'; REDACTED_STDERR_BYTES + 1]);
        assert!(long.ends_with("[truncated]"), "{long}");
        assert_eq!(long.len(), REDACTED_STDERR_BYTES + "[truncated]".len());
    }

    #[cfg(windows)]
    fn shell(script: &[&str]) -> (&'static str, Vec<String>) {
        let mut args = vec!["/c".to_string()];
        args.extend(script.iter().map(|part| (*part).to_string()));
        ("cmd", args)
    }

    #[cfg(not(windows))]
    fn shell(script: &[&str]) -> (&'static str, Vec<String>) {
        let mut args = vec!["-c".to_string()];
        args.extend(script.iter().map(|part| (*part).to_string()));
        ("/bin/sh", args)
    }

    #[cfg(windows)]
    fn sleeper() -> (&'static str, Vec<String>) {
        shell(&["ping -n 60 127.0.0.1"])
    }

    #[cfg(not(windows))]
    fn sleeper() -> (&'static str, Vec<String>) {
        shell(&["sleep 60"])
    }

    /// The one leg a daemon-free machine cannot prove. It runs only in the
    /// live-container CI job, by this exact name.
    #[test]
    #[ignore = "needs a running Docker daemon and SHARPEBENCH_SANDBOX_FIXTURE"]
    fn live_docker_image_preflight() {
        let image = std::env::var("SHARPEBENCH_SANDBOX_FIXTURE").expect(
            "the live preflight test needs SHARPEBENCH_SANDBOX_FIXTURE set to a digest-pinned \
             image that is present locally",
        );
        let clean = preflight_image(&request(&image), &DockerProcess)
            .expect("a clean pinned fixture completes");
        assert!(
            clean.authorizes_launch(),
            "a clean fixture must authorize: {clean:?}"
        );
        assert!(clean.cleanup_verified);
        let filesystem = clean.filesystem.as_ref().expect("a filesystem scan ran");
        assert!(filesystem.archive_sha256.is_some());

        // A needle that is genuinely in the fixture's TAR headers.
        let needle_policy = RawScanPolicy::from_json(
            br#"{"schema_version":"sharpebench.raw-scan-policy.v1","utf8_sequences":["etc/alpine-release"]}"#,
        )
        .expect("the live needle policy validates");
        let matched = preflight_image(
            &PreflightRequest {
                image,
                policy: needle_policy,
                allowlist: None,
            },
            &DockerProcess,
        )
        .expect("a matching scan still completes");
        assert!(
            !matched.authorizes_launch(),
            "a known match must refuse: {matched:?}"
        );
        assert!(matched.cleanup_verified, "removal must still be verified");
        assert_eq!(
            matched.image_id, clean.image_id,
            "both captures must report the same configuration ID"
        );
    }

    fn live_fixture() -> String {
        std::env::var("SHARPEBENCH_SANDBOX_FIXTURE").expect(
            "the live allowlist tests need SHARPEBENCH_SANDBOX_FIXTURE set to a digest-pinned \
             image that is present locally",
        )
    }

    /// One archive entry as the live measurement records it.
    #[derive(Clone, Debug, PartialEq, Eq)]
    struct Listed {
        path: String,
        kind: tar::EntryType,
        size: u64,
        link: Option<String>,
    }

    impl std::fmt::Display for Listed {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(f, "{} {:?} size={}", self.path, self.kind, self.size)?;
            if let Some(link) = &self.link {
                write!(f, " -> {link}")?;
            }
            Ok(())
        }
    }

    fn normalized(path: &str) -> String {
        let path = path.strip_prefix("./").unwrap_or(path);
        path.trim_start_matches('/')
            .trim_end_matches('/')
            .to_string()
    }

    /// Every entry of a TAR stream, in archive order.
    fn list_tar(reader: impl Read) -> Vec<Listed> {
        let mut archive = tar::Archive::new(reader);
        archive
            .entries()
            .expect("a readable archive")
            .map(|entry| {
                let entry = entry.expect("a readable entry");
                Listed {
                    path: normalized(
                        std::str::from_utf8(&entry.path_bytes()).expect("a UTF-8 entry name"),
                    ),
                    kind: entry.header().entry_type(),
                    size: entry.header().size().expect("a readable size"),
                    link: entry
                        .link_name_bytes()
                        .map(|link| String::from_utf8_lossy(&link).into_owned()),
                }
            })
            .collect()
    }

    fn docker_stdout(args: &[&str]) -> String {
        let output = Command::new("docker")
            .args(args)
            .stdin(Stdio::null())
            .output()
            .expect("docker must run");
        assert!(
            output.status.success(),
            "docker {args:?} failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8_lossy(&output.stdout).trim().to_string()
    }

    /// The export of a created, never-started container, taken exactly as the
    /// preflight takes it: the same create arguments and the same capture.
    fn live_export_listing(image: &str) -> Vec<Listed> {
        let deadline = Instant::now() + Duration::from_secs(120);
        let name = reserved_container_name();
        let created = DockerProcess
            .capture(
                &create_args(&name, image),
                MAX_CONTROL_RESPONSE_BYTES,
                deadline,
            )
            .expect("docker create runs");
        assert!(created.success, "{}", created.redacted_stderr);
        let exported = DockerProcess
            .capture(&owned(&["export", &name]), 1 << 30, deadline)
            .expect("docker export runs");
        assert!(
            remove_container(&DockerProcess, &name),
            "the listing container must be removed"
        );
        assert!(exported.success, "{}", exported.redacted_stderr);
        list_tar(exported.stdout)
    }

    /// What the image itself holds: its layers from `docker save`, applied in
    /// order with whiteouts, independently of any container the daemon creates.
    fn live_image_listing(image: &str) -> Vec<Listed> {
        let saved = DockerProcess
            .capture(
                &owned(&["save", image]),
                1 << 30,
                Instant::now() + Duration::from_secs(120),
            )
            .expect("docker save runs");
        assert!(saved.success, "{}", saved.redacted_stderr);
        let mut blobs = std::collections::HashMap::new();
        let mut links = std::collections::HashMap::new();
        let mut archive = tar::Archive::new(saved.stdout);
        for entry in archive.entries().expect("a readable image archive") {
            let mut entry = entry.expect("a readable image entry");
            let path = normalized(&String::from_utf8_lossy(&entry.path_bytes()));
            if entry.header().entry_type().is_symlink() {
                let target = entry.link_name_bytes().expect("a link has a target");
                let target = String::from_utf8_lossy(&target).into_owned();
                // A save deduplicates a layer as a relative link to another.
                let mut resolved: Vec<&str> = path.split('/').collect();
                resolved.pop();
                for segment in target.split('/') {
                    match segment {
                        ".." => {
                            resolved.pop();
                        }
                        "." | "" => {}
                        segment => resolved.push(segment),
                    }
                }
                let resolved = resolved.join("/");
                links.insert(path, resolved);
            } else if entry.header().entry_type().is_file() {
                let mut bytes = Vec::new();
                entry.read_to_end(&mut bytes).expect("a readable blob");
                blobs.insert(path, bytes);
            }
        }
        let manifest: Value =
            serde_json::from_slice(&blobs["manifest.json"]).expect("a manifest.json");
        let layers = manifest[0]["Layers"].as_array().expect("a layer list");
        let mut merged: Vec<Listed> = Vec::new();
        for layer in layers {
            let mut path = normalized(layer.as_str().expect("a layer path"));
            while let Some(target) = links.get(&path) {
                path = target.clone();
            }
            let bytes = &blobs[&path];
            let entries = match bytes.get(..4) {
                Some([0x1f, 0x8b, ..]) => {
                    let output = Command::new("gzip")
                        .arg("-dc")
                        .stdin(spooled(bytes))
                        .output()
                        .expect("gzip must run");
                    assert!(output.status.success(), "gzip could not read layer {path}");
                    list_tar(&output.stdout[..])
                }
                Some([0x28, 0xb5, 0x2f, 0xfd]) => panic!("layer {path} is zstd; add a decoder"),
                _ => list_tar(&bytes[..]),
            };
            println!("image layer {path}: {} entries", entries.len());
            for entry in entries {
                let (parent, base) = entry.path.rsplit_once('/').unwrap_or(("", &entry.path));
                if base == ".wh..wh..opq" {
                    merged.retain(|kept| !kept.path.starts_with(&format!("{parent}/")));
                } else if let Some(hidden) = base.strip_prefix(".wh.") {
                    let hidden = if parent.is_empty() {
                        hidden.to_string()
                    } else {
                        format!("{parent}/{hidden}")
                    };
                    merged.retain(|kept| {
                        kept.path != hidden && !kept.path.starts_with(&format!("{hidden}/"))
                    });
                } else {
                    merged.retain(|kept| kept.path != entry.path);
                    merged.push(entry);
                }
            }
        }
        merged
    }

    /// Export entries that are not the image's own: a path the image does not
    /// hold, or one whose type, size or link target the daemon changed.
    fn docker_added<'a>(image: &[Listed], export: &'a [Listed]) -> Vec<&'a Listed> {
        export
            .iter()
            .filter(|entry| !entry.path.is_empty())
            .filter(|entry| {
                image
                    .iter()
                    .find(|own| own.path == entry.path)
                    .is_none_or(|own| {
                        own.kind != entry.kind || own.size != entry.size || own.link != entry.link
                    })
            })
            .collect()
    }

    fn allowlist_of(paths: &[&str]) -> RuntimeAllowlist {
        let json = serde_json::json!({"schema_version": ALLOWLIST_VERSION, "paths": paths});
        let bytes = json.to_string();
        println!(
            "generated allowlist: {} paths, {} bytes",
            paths.len(),
            bytes.len()
        );
        RuntimeAllowlist::from_json(bytes.as_bytes()).expect("the generated allowlist validates")
    }

    /// The fixture's own paths, each admitted exactly (no subtree).
    fn image_paths(image: &[Listed]) -> Vec<&str> {
        image
            .iter()
            .map(|entry| entry.path.as_str())
            .filter(|path| !path.is_empty())
            .collect()
    }

    /// The pinned fixture with an entrypoint that answers one observation with
    /// a hold. Committed from a created, never-started container, so its
    /// filesystem is the fixture's; only the configuration changes.
    fn commit_answering_image(fixture: &str) -> String {
        let name = reserved_container_name();
        docker_stdout(&["create", "--name", &name, "--pull", "never", fixture]);
        let entrypoint = serde_json::to_string(&[
            "/bin/sh",
            "-c",
            r#"IFS= read -r observation; echo '{"orders":[],"reasoning":"probe"}'"#,
        ])
        .expect("an entrypoint serializes");
        let id = docker_stdout(&[
            "commit",
            "--change",
            &format!("ENTRYPOINT {entrypoint}"),
            &name,
        ]);
        docker_stdout(&["rm", "--force", &name]);
        id
    }

    /// The runtime allowlist and the functional probe against a real daemon.
    ///
    /// Measures what the daemon adds to a created container's export (printed
    /// for the audit record), requires every such entry to be one
    /// [`DOCKER_INIT_ENTRIES`] admits in its shape, then runs the preflight
    /// with an allowlist naming exactly the fixture's own paths. The pinned
    /// fixture's own entrypoint is `/bin/sh` reading the observation as a
    /// script, so its probe must refuse with `no_decision`: that is the probe
    /// running the image, not assuming it. The same filesystem with an
    /// entrypoint that answers must pass and authorize.
    #[test]
    #[ignore = "needs a running Docker daemon and SHARPEBENCH_SANDBOX_FIXTURE"]
    fn live_runtime_allowlist_admits_the_fixture_and_its_probe_passes() {
        let fixture = live_fixture();
        println!(
            "daemon: server {} / {} / {}",
            docker_stdout(&["version", "--format", "{{.Server.Version}}"]),
            docker_stdout(&["info", "--format", "{{.Driver}} {{json .DriverStatus}}"]),
            docker_stdout(&[
                "info",
                "--format",
                "cgroup {{.CgroupDriver}} v{{.CgroupVersion}}"
            ]),
        );
        let image = live_image_listing(&fixture);
        let export = live_export_listing(&fixture);
        println!(
            "fixture {fixture}: {} image entries, {} export entries",
            image.len(),
            export.len()
        );
        let added = docker_added(&image, &export);
        for entry in &added {
            let own = image.iter().find(|own| own.path == entry.path);
            println!(
                "docker-added export entry: {entry} (image holds: {})",
                own.map_or("nothing".to_string(), ToString::to_string)
            );
        }
        for own in &image {
            if !export.iter().any(|entry| entry.path == own.path) {
                println!("image entry absent from the export: {own}");
            }
        }
        for entry in &added {
            assert!(
                is_docker_init_entry(
                    &entry.path,
                    entry.kind,
                    entry.size,
                    entry.link.as_deref().map(str::as_bytes)
                ),
                "the daemon added {entry}, which the init-entry rule does not admit"
            );
        }
        let new_paths = added
            .iter()
            .filter(|entry| !image.iter().any(|own| own.path == entry.path))
            .count() as u64;

        let allowlist = allowlist_of(&image_paths(&image));
        let pinned = preflight_image(
            &PreflightRequest {
                image: fixture.clone(),
                policy: policy(),
                allowlist: Some(allowlist.clone()),
            },
            &DockerProcess,
        )
        .expect("the pinned fixture's preflight completes");
        println!(
            "pinned fixture report: {}",
            serde_json::to_string(&pinned).expect("serializes")
        );
        let checked = pinned
            .runtime_allowlist
            .as_ref()
            .expect("the allowlist ran");
        assert!(checked.admits_everything(), "{checked:?}");
        assert_eq!(checked.entries, export.len() as u64);
        assert_eq!(checked.docker_init_entries, new_paths);
        let probe = pinned.functional_probe.as_ref().expect("the probe ran");
        assert_eq!(probe.refusal, Some("no_decision"), "{probe:?}");
        assert!(probe.cleanup_verified && !pinned.authorizes_launch());

        let answering = commit_answering_image(&fixture);
        let report = preflight_image(
            &PreflightRequest {
                image: answering.clone(),
                policy: policy(),
                allowlist: Some(allowlist),
            },
            &DockerProcess,
        );
        docker_stdout(&["image", "rm", "--force", &answering]);
        let report = report.expect("the answering image's preflight completes");
        println!(
            "answering image report: {}",
            serde_json::to_string(&report).expect("serializes")
        );
        let checked = report
            .runtime_allowlist
            .as_ref()
            .expect("the allowlist ran");
        assert!(checked.admits_everything(), "{checked:?}");
        let probe = report.functional_probe.as_ref().expect("the probe ran");
        assert!(probe.passed && probe.cleanup_verified, "{probe:?}");
        assert!(report.cleanup_verified && report.authorizes_launch());
        assert_eq!(
            report.authorized_image_id().map(|id| id.0),
            Some(report.image_id.clone())
        );
    }

    /// An allowlist that names every fixture path but one refuses, reports the
    /// one entry by its archive-order index in an independent export of the
    /// same image, withholds its name and never starts the image.
    #[test]
    #[ignore = "needs a running Docker daemon and SHARPEBENCH_SANDBOX_FIXTURE"]
    fn live_runtime_allowlist_refuses_an_omitted_path_by_index() {
        const OMITTED: &str = "etc/alpine-release";
        let fixture = live_fixture();
        let image = live_image_listing(&fixture);
        let export = live_export_listing(&fixture);
        assert!(image.iter().any(|entry| entry.path == OMITTED));
        let index = export
            .iter()
            .position(|entry| entry.path == OMITTED)
            .expect("the export holds the omitted path") as u64;
        let paths: Vec<&str> = image_paths(&image)
            .into_iter()
            .filter(|path| *path != OMITTED)
            .collect();
        let report = preflight_image(
            &PreflightRequest {
                image: fixture,
                policy: policy(),
                allowlist: Some(allowlist_of(&paths)),
            },
            &DockerProcess,
        )
        .expect("the preflight completes");
        let published = serde_json::to_string(&report).expect("serializes");
        println!("omitted-path report: {published}");
        let checked = report
            .runtime_allowlist
            .as_ref()
            .expect("the allowlist ran");
        assert!(checked.complete);
        assert_eq!(checked.outside_allowlist, 1, "{checked:?}");
        assert_eq!(checked.outside_indices, vec![index], "{checked:?}");
        assert!(
            report.functional_probe.is_none(),
            "a refused image is never started"
        );
        assert!(report.cleanup_verified && !report.authorizes_launch());
        assert!(!published.contains("alpine-release"), "{published}");
    }
}
