//! The opt-in image preflight as the shipped binary exposes it.
//!
//! The refusal logic itself is unit tested against an injected transport. What
//! this file adds is the surface: the flags a user actually types, on a machine
//! with no Docker daemon, must refuse before any entrant runs and must never
//! emit a leaderboard. None of these assertions needs a daemon, because every
//! one of them is a refusal decided from arguments and one bounded file read.

use std::io::Write;
use std::process::Command;

const BIN: &str = env!("CARGO_BIN_EXE_sharpebench");

const POLICY: &str = r#"{"schema_version":"sharpebench.raw-scan-policy.v1","utf8_sequences":["sharpebench-protected-canary"]}"#;

const UNSANDBOXED_WARNING: &str = "with NO sandbox";

fn pinned() -> String {
    format!("registry.example/agent@sha256:{}", "b".repeat(64))
}

fn run(args: &[&str]) -> (Option<i32>, String, String) {
    let out = Command::new(BIN)
        .args(args)
        .output()
        .expect("the CLI under test must be runnable");
    (
        out.status.code(),
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

fn policy_file(contents: &[u8]) -> tempfile::NamedTempFile {
    let mut file = tempfile::NamedTempFile::new().expect("a policy file opens");
    file.write_all(contents).expect("the policy writes");
    file.flush().expect("the policy flushes");
    file
}

/// A usage error, not a run: exit 2, no board, and no host execution.
fn refuses_with_usage_error(args: &[&str], needle: &str) {
    let (code, stdout, stderr) = run(args);
    assert_eq!(code, Some(2), "expected a usage refusal: {stderr}");
    assert!(stderr.contains(needle), "{stderr}");
    assert!(stdout.is_empty(), "a refused run emits no board: {stdout}");
    assert!(
        !stderr.contains(UNSANDBOXED_WARNING),
        "a refused preflight must never reach the host path: {stderr}"
    );
}

#[test]
fn a_scan_policy_without_an_image_is_a_usage_error() {
    let file = policy_file(POLICY.as_bytes());
    refuses_with_usage_error(
        &["run", "--scan-policy", &file.path().to_string_lossy()],
        "requires --image",
    );
}

#[test]
fn a_scan_policy_with_a_host_command_is_a_usage_error() {
    let file = policy_file(POLICY.as_bytes());
    refuses_with_usage_error(
        &[
            "run",
            "--image",
            &pinned(),
            "--cmd",
            "./agent",
            "--scan-policy",
            &file.path().to_string_lossy(),
        ],
        "exactly one transport",
    );
}

#[test]
fn an_unpinned_scanned_image_is_a_usage_error() {
    let file = policy_file(POLICY.as_bytes());
    refuses_with_usage_error(
        &[
            "run",
            "--image",
            "agent:latest",
            "--scan-policy",
            &file.path().to_string_lossy(),
        ],
        "64 lowercase hex",
    );
}

#[test]
fn a_policy_that_is_not_valid_under_its_schema_is_a_usage_error() {
    let file = policy_file(br#"{"schema_version":"sharpebench.raw-scan-policy.v1"}"#);
    refuses_with_usage_error(
        &[
            "run",
            "--image",
            &pinned(),
            "--scan-policy",
            &file.path().to_string_lossy(),
        ],
        "protected-content rule",
    );
}

#[test]
fn an_oversized_policy_is_a_usage_error() {
    let file = policy_file(&vec![b' '; 64 * 1024 + 8]);
    refuses_with_usage_error(
        &[
            "run",
            "--image",
            &pinned(),
            "--scan-policy",
            &file.path().to_string_lossy(),
        ],
        "64 KiB",
    );
}

#[test]
fn an_absent_policy_file_is_a_usage_error() {
    refuses_with_usage_error(
        &[
            "run",
            "--image",
            &pinned(),
            "--scan-policy",
            "no-such-scan-policy.json",
        ],
        "cannot open scan policy",
    );
}

/// The opt-in has to be discoverable, and it has to say it is `--image` only.
#[test]
fn the_help_text_documents_the_opt_in() {
    let (code, stdout, _) = run(&["--help"]);
    assert_eq!(code, Some(0));
    assert!(stdout.contains("--scan-policy"), "{stdout}");
    assert!(stdout.contains("--image only"), "{stdout}");
}

/// Without `--scan-policy` the legacy path is byte for byte what it was: the
/// image refusal comes from the sandbox launcher, not from the preflight.
#[test]
fn an_unscanned_image_run_keeps_its_legacy_refusal() {
    let (code, stdout, stderr) = run(&["run", "--image", &pinned()]);
    assert_ne!(code, Some(0), "an unlaunchable sandbox must fail: {stderr}");
    assert!(
        stderr.contains("cannot start the sandboxed agent"),
        "{stderr}"
    );
    assert!(
        !stderr.contains("image preflight"),
        "the preflight is opt-in: {stderr}"
    );
    assert!(stdout.is_empty(), "{stdout}");
}
