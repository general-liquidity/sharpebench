//! Golden-score regression gate: fixed inputs → fixed, committed scores.
//!
//! `docs/PLAN.md` promises "fixed input trajectories → fixed scores; if a number
//! ever changes, CI fails". These tests make that promise enforceable. Each
//! fixture under `golden/` is the full `Vec<CompositeScore>` the kernel produced
//! for a deterministic input, serialised with `serde_json::to_string_pretty` (the
//! exact bytes `sharpebench score <input> --json` prints). serde_json renders
//! every `f64` with ryu's shortest-round-trip form, which is unique per bit
//! pattern, so even 1-ULP drift in any statistic changes the bytes and fails the
//! assert. The same test file runs on every OS in the CI matrix, which is what
//! turns "byte-identical on any platform" from an architectural claim into a check.
//!
//! This file exercises the scoring kernel only. `synthetic_field.input.json` is a
//! committed trajectory set; the test that proves the simulator still produces
//! those exact bytes lives in `sharpebench-sim/tests/golden_input.rs`, because
//! `sim` depends on `core` and a dev-dependency the other way is a cycle that
//! `cargo publish` rejects (it resolves dev-deps, and `sim` is not on the registry
//! yet when `core` is packaged). Keeping the simulator out of this crate's tests
//! also isolates kernel drift from simulator drift: if this file fails and that
//! one passes, the scorer moved.
//!
//! Regenerating fixtures (deliberately, after a scoring change):
//!
//! ```text
//! SHARPEBENCH_UPDATE_GOLDEN=1 cargo test -p sharpebench-core --test golden_scores
//! ```
//!
//! That rewrites the two `*.scores.json` fixtures from the committed inputs. To
//! regenerate the input itself after a simulator change, run the `sim` test with
//! the same variable. Setting it in CI is refused, so a fixture can only change
//! in a commit a human reviewed.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use sharpebench_core::{rank, AgentSubmission, CompositeScore, ScoreConfig};

const UPDATE_ENV: &str = "SHARPEBENCH_UPDATE_GOLDEN";

fn golden_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("golden")
}

fn suites_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../suites")
}

fn update_requested() -> bool {
    let requested = std::env::var_os(UPDATE_ENV).is_some_and(|v| !v.is_empty() && v != "0");
    // GitHub Actions (and every other mainstream CI) exports `CI=true`. Rewriting
    // fixtures there would silently bless whatever the kernel currently emits,
    // which is exactly the failure mode these tests exist to prevent.
    assert!(
        !(requested && std::env::var_os("CI").is_some()),
        "{UPDATE_ENV} is set in a CI environment; golden fixtures may only be regenerated locally"
    );
    requested
}

/// In update mode every score fixture is rewritten exactly once, before any test
/// reads one. Tests run in parallel, so without this a read-only leg could observe
/// a half-written file from another thread.
fn regenerate_all_once() {
    static DONE: OnceLock<()> = OnceLock::new();
    DONE.get_or_init(|| {
        for (name, contents) in [
            (
                "example_submissions.scores.json",
                score_pretty(&example_submissions()),
            ),
            (
                "synthetic_field.scores.json",
                score_pretty(&committed_synthetic_input()),
            ),
        ] {
            let path = golden_dir().join(name);
            std::fs::write(&path, contents)
                .unwrap_or_else(|e| panic!("write {}: {e}", path.display()));
            eprintln!("rewrote golden fixture {}", path.display());
        }
    });
}

/// Read a committed fixture (regenerating the score fixtures first if requested).
fn golden(name: &str) -> String {
    if update_requested() {
        regenerate_all_once();
    }
    let path = golden_dir().join(name);
    std::fs::read_to_string(&path).unwrap_or_else(|e| {
        panic!(
            "read {}: {e} (run with {UPDATE_ENV}=1 to create it)",
            path.display()
        )
    })
}

/// Compare `actual` to the committed fixture byte-for-byte.
fn assert_matches_golden(name: &str, actual: &str) {
    let expected = golden(name);
    assert!(
        expected == actual,
        "{name} drifted from the committed golden fixture.\n\
         If the change is intentional, regenerate with `{UPDATE_ENV}=1 cargo test -p sharpebench-core --test golden_scores` and review the diff.\n\
         --- expected ---\n{expected}\n--- actual ---\n{actual}"
    );
}

/// `to_string_pretty` plus the trailing newline `println!` adds, so the fixture is
/// the literal stdout of `sharpebench score <input> --json` and CI can `diff` them.
fn pretty_with_newline<T: serde::Serialize>(value: &T) -> String {
    let mut s = serde_json::to_string_pretty(value).expect("serialise");
    s.push('\n');
    s
}

fn score_pretty(subs: &[AgentSubmission]) -> String {
    pretty_with_newline(&rank(subs, &ScoreConfig::default()))
}

/// The public example field, scored with the default config.
fn example_submissions() -> Vec<AgentSubmission> {
    let path = suites_dir().join("example_submissions.json");
    let raw =
        std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    serde_json::from_str(&raw).expect("suites/example_submissions.json parses")
}

/// The committed simulator output. Read as plain JSON, never regenerated here:
/// the kernel has no business knowing how these trajectories were produced.
fn committed_synthetic_input() -> Vec<AgentSubmission> {
    let path = golden_dir().join("synthetic_field.input.json");
    let raw =
        std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    serde_json::from_str(&raw).expect("synthetic_field.input.json parses")
}

#[test]
fn example_submissions_score_is_byte_identical_to_golden() {
    assert_matches_golden(
        "example_submissions.scores.json",
        &score_pretty(&example_submissions()),
    );
}

#[test]
fn committed_synthetic_input_rescored_is_byte_identical_to_golden() {
    assert_matches_golden(
        "synthetic_field.scores.json",
        &score_pretty(&committed_synthetic_input()),
    );
}

#[test]
fn golden_fixtures_are_valid_composite_scores() {
    for name in [
        "example_submissions.scores.json",
        "synthetic_field.scores.json",
    ] {
        let raw = golden(name);
        let scores: Vec<CompositeScore> =
            serde_json::from_str(&raw).unwrap_or_else(|e| panic!("{name} parses: {e}"));
        assert!(!scores.is_empty(), "{name} must contain at least one score");
    }
}
