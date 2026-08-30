//! `sharpebench run` must route an external entrant through the container
//! boundary when it is given one, and must say so out loud when it is not.
//!
//! These assertions are about the *routing*, not about Docker: they hold with or
//! without a daemon, because every way this cannot be a sandboxed run is a
//! refusal (no daemon, a reference that is not digest-pinned, or an image that
//! is not present locally) and no refusal is allowed to become a host execution.
//! On a machine with a daemon the second test exercises the presence leg, which
//! is the only leg that needs one.

use std::process::Command;

const BIN: &str = env!("CARGO_BIN_EXE_sharpebench");

const UNSANDBOXED_WARNING: &str = "with NO sandbox";

fn run(args: &[&str]) -> (bool, String, String) {
    let out = Command::new(BIN)
        .args(args)
        .output()
        .expect("the CLI under test must be runnable");
    (
        out.status.success(),
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

/// A mutable tag is refused before any container starts, and the refusal does
/// NOT degrade into running the entrant on the host.
#[test]
fn an_unpinned_image_is_refused_and_never_falls_through_to_the_host() {
    let (ok, stdout, stderr) = run(&["run", "--image", "some/agent:latest"]);
    assert!(!ok, "an unlaunchable sandbox must fail the run: {stderr}");
    assert!(
        stderr.contains("cannot start the sandboxed agent"),
        "the refusal must name the sandbox: {stderr}"
    );
    // The plausible neighbour: silently running the entrant unsandboxed, which is
    // exactly the defect this path exists to remove. It would print the warning
    // and, on success, a leaderboard.
    assert!(
        !stderr.contains(UNSANDBOXED_WARNING),
        "a refused sandbox must not hand the entrant to the host path: {stderr}"
    );
    assert!(
        stdout.is_empty(),
        "a refused sandbox must not emit a board: {stdout}"
    );
}

/// A digest-pinned reference gets past validation, so the remaining refusals are
/// the daemon and the artifact: no daemon here, an image that is not present
/// where there is one. Same rule either way: a refusal is a refusal, never a
/// downgrade.
#[test]
fn a_pinned_image_that_cannot_launch_is_still_a_refusal() {
    let image = format!("some/agent@sha256:{}", "a".repeat(64));
    let (ok, stdout, stderr) = run(&["run", "--image", &image]);
    assert!(!ok, "an unlaunchable sandbox must fail the run: {stderr}");
    assert!(
        !stderr.contains(UNSANDBOXED_WARNING),
        "a refused sandbox must not hand the entrant to the host path: {stderr}"
    );
    assert!(stdout.is_empty(), "no board on refusal: {stdout}");
}

/// Host execution is still available, but it announces itself on every run and in
/// both output modes, so an unsandboxed run cannot be mistaken for a sandboxed one.
#[test]
fn the_host_path_announces_that_it_is_unsandboxed() {
    for extra in [&[][..], &["--json"][..]] {
        let mut args = vec!["run", "--cmd", "sharpebench-no-such-agent-binary"];
        args.extend_from_slice(extra);
        let (_, _, stderr) = run(&args);
        assert!(
            stderr.contains(UNSANDBOXED_WARNING),
            "the unsandboxed downgrade must be recorded (args {args:?}): {stderr}"
        );
        assert!(
            stderr.contains("--image"),
            "the warning must point at the sandboxed alternative: {stderr}"
        );
    }
}
