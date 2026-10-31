//! The installed CLI driving the arena clock: a day with no transition writes
//! nothing, on a fresh arena and on the repository's own operating record, and
//! `expire-empty` and `verify --expect-head` behave as the book says.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};

const SCORER: &str = "1111111111111111111111111111111111111111111111111111111111111111";

struct Fixture(PathBuf);

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

impl Fixture {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "sharpebench-arena-clock-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }

    fn cli(&self, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_sharpebench"))
            .current_dir(&self.0)
            .args(args)
            .output()
            .unwrap()
    }

    fn expect(&self, code: i32, args: &[&str]) -> String {
        let output = self.cli(args);
        assert_eq!(
            output.status.code(),
            Some(code),
            "{args:?}\nstdout: {}\nstderr: {}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8_lossy(&output.stdout).into_owned()
    }

    fn window_status(&self) -> serde_json::Value {
        let path = self.0.join("arena/windows/w1/window.json");
        serde_json::from_slice::<serde_json::Value>(&std::fs::read(path).unwrap()).unwrap()
            ["status"]
            .clone()
    }

    fn opened(&self) {
        self.expect(0, &["arena", "init", "arena"]);
        self.expect(
            0,
            &[
                "arena",
                "open",
                "arena",
                "w1",
                "10",
                "20",
                "--scorer-artifact-sha256",
                SCORER,
            ],
        );
    }
}

fn snapshot(root: &Path) -> Vec<(PathBuf, Vec<u8>)> {
    let mut pending = vec![root.to_path_buf()];
    let mut files = Vec::new();
    while let Some(dir) = pending.pop() {
        for entry in std::fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                pending.push(path);
            } else if path.file_name().is_some_and(|name| name != ".arena.lock") {
                let bytes = std::fs::read(&path).unwrap();
                files.push((path, bytes));
            }
        }
    }
    files.sort();
    files
}

fn copy_tree(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for entry in std::fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        if entry.path().is_dir() {
            copy_tree(&entry.path(), &to.join(entry.file_name()));
        } else {
            std::fs::copy(entry.path(), to.join(entry.file_name())).unwrap();
        }
    }
}

fn repository_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .unwrap()
        .to_path_buf()
}

#[test]
fn only_if_due_writes_nothing_until_a_window_would_change() {
    let fx = Fixture::new();
    fx.opened();
    let arena = fx.0.join("arena");
    let quiet = |epoch: &str| {
        let before = snapshot(&arena);
        let out = fx.expect(0, &["arena", "advance", "arena", epoch, "--only-if-due"]);
        assert!(out.contains("nothing written"), "{out}");
        assert_eq!(snapshot(&arena), before, "epoch {epoch} wrote state");
    };
    let moves = |epoch: &str| {
        let before = snapshot(&arena);
        fx.expect(0, &["arena", "advance", "arena", epoch, "--only-if-due"]);
        assert_ne!(snapshot(&arena), before, "epoch {epoch} wrote nothing");
    };
    quiet("9");
    moves("10");
    assert_eq!(fx.window_status(), "committed");
    quiet("19");
    moves("20");
    quiet("21");
    fx.expect(0, &["arena", "expire-empty", "arena", "w1"]);
    assert_eq!(fx.window_status(), "expired-empty");
    quiet("1000");
    // The flag gates the write. Without it the epoch still moves on request.
    let before = snapshot(&arena);
    fx.expect(0, &["arena", "advance", "arena", "1000"]);
    assert_ne!(snapshot(&arena), before);
}

/// The repository's own record holds superseded windows frozen at `open` with
/// past deadlines. The clock read those and rewrote state every day.
#[test]
fn the_operating_record_has_no_transition_due() {
    let fx = Fixture::new();
    copy_tree(&repository_root().join("arena"), &fx.0.join("arena"));
    let arena = fx.0.join("arena");
    let before = snapshot(&arena);
    let out = fx.expect(0, &["arena", "advance", "arena", "99999", "--only-if-due"]);
    assert!(out.contains("nothing written"), "{out}");
    assert_eq!(snapshot(&arena), before);
    fx.expect(0, &["arena", "verify", "arena"]);
}

#[test]
fn the_clock_workflow_asks_the_arena_what_is_due() {
    let workflow =
        std::fs::read_to_string(repository_root().join(".github/workflows/arena-clock.yml"))
            .unwrap();
    assert!(workflow.contains(r#"arena advance "$ARENA_DIR" "$EPOCH" --only-if-due"#));
    assert!(!workflow.contains("jq "), "the workflow judges due itself");
}

#[test]
fn expire_empty_reports_and_refuses_through_the_cli() {
    let fx = Fixture::new();
    fx.expect(2, &["arena", "expire-empty", "arena"]);
    fx.opened();
    fx.expect(1, &["arena", "expire-empty", "arena", "w1"]);
    assert_eq!(fx.window_status(), "open");
    fx.expect(0, &["arena", "advance", "arena", "20"]);
    let out = fx.expect(0, &["arena", "expire-empty", "arena", "w1", "--json"]);
    let value: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(value["status"], "expired-empty");
    assert_eq!(value["expired_at_epoch"], 20);
}

#[test]
fn verify_states_the_limit_and_refuses_a_bad_expected_head() {
    let fx = Fixture::new();
    fx.opened();
    let out = fx.expect(0, &["arena", "verify", "arena"]);
    assert!(
        out.contains("completeness limit: no published window"),
        "{out}"
    );
    let digest = "a".repeat(64);
    fx.expect(2, &["arena", "verify", "arena", "--expect-head"]);
    fx.expect(2, &["arena", "verify", "arena", "--expect-head", "0:abc"]);
    let pin = format!("1:{digest}");
    let output = fx.cli(&["arena", "verify", "arena", "--expect-head", &pin]);
    assert_eq!(output.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("expected head records 1"), "{stderr}");
}
