//! `sharpebench leaderboard export`: verify signed boards, then write a
//! canonical JSON record and a Markdown table derived only from them.
//!
//! The committed fixture (`tests/fixtures/leaderboard-arena`) is a two-window
//! arena with its expected export beside it. It is written by the ignored
//! `write_the_cli_fixture` test in `crates/sharpebench-arena/tests/
//! leaderboard_export.rs`. The export of those signed boards must match the
//! committed files byte for byte on every platform.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};

struct Fixture(PathBuf);

impl Fixture {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        loop {
            let path = std::env::temp_dir().join(format!(
                "sharpebench-leaderboard-cli-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            match std::fs::create_dir(&path) {
                Ok(()) => return Self(path),
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(e) => panic!("create fixture: {e}"),
            }
        }
    }

    /// A fresh copy of the committed fixture arena, so nothing writes into
    /// the source tree.
    fn with_arena() -> Self {
        let fx = Self::new();
        copy_tree(&committed().join("arena"), &fx.0.join("arena"));
        fx
    }

    fn cli(&self, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_sharpebench"))
            .current_dir(&self.0)
            .args(args)
            .output()
            .unwrap()
    }

    fn path(&self, rel: &str) -> PathBuf {
        self.0.join(rel)
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.0).expect("remove exclusively owned fixture");
    }
}

fn committed() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/leaderboard-arena")
}

fn copy_tree(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for entry in std::fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let target = to.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_tree(&entry.path(), &target);
        } else {
            std::fs::copy(entry.path(), target).unwrap();
        }
    }
}

fn read(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap()
}

/// The arena key, read from the committed export.
fn arena_key() -> String {
    let record: serde_json::Value =
        serde_json::from_str(&read(&committed().join("leaderboard.json"))).unwrap();
    record["source"]["verifying_key"]
        .as_str()
        .unwrap()
        .to_string()
}

fn code(out: &Output) -> Option<i32> {
    out.status.code()
}

#[test]
fn the_fixture_arena_exports_the_committed_bytes() {
    let fx = Fixture::with_arena();
    let key = arena_key();
    let out = fx.cli(&[
        "leaderboard",
        "export",
        "arena",
        "--out",
        "export",
        "--pubkey",
        &key,
    ]);
    assert_eq!(code(&out), Some(0), "{out:?}");
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(
        stdout.starts_with("exported 5 row(s) from 2 verified board(s)"),
        "{stdout}"
    );
    assert_eq!(
        read(&fx.path("export/leaderboard.json")),
        read(&committed().join("leaderboard.json"))
    );
    assert_eq!(
        read(&fx.path("export/leaderboard.md")),
        read(&committed().join("leaderboard.md"))
    );
    // A second run writes the same bytes.
    let again = fx.cli(&[
        "leaderboard",
        "export",
        "arena",
        "--out",
        "again",
        "--pubkey",
        &key,
    ]);
    assert_eq!(code(&again), Some(0));
    assert_eq!(
        std::fs::read(fx.path("again/leaderboard.json")).unwrap(),
        std::fs::read(fx.path("export/leaderboard.json")).unwrap()
    );
}

#[test]
fn without_out_the_table_or_the_record_goes_to_stdout() {
    let fx = Fixture::with_arena();
    let key = arena_key();
    let table = fx.cli(&["leaderboard", "export", "arena", "--pubkey", &key]);
    assert_eq!(code(&table), Some(0));
    assert_eq!(
        String::from_utf8(table.stdout).unwrap(),
        read(&committed().join("leaderboard.md"))
    );
    let record = fx.cli(&["leaderboard", "export", "arena", "--pubkey", &key, "--json"]);
    assert_eq!(code(&record), Some(0));
    assert_eq!(
        String::from_utf8(record.stdout).unwrap(),
        read(&committed().join("leaderboard.json"))
    );
    // Without a pinned key the embedded one is trusted, and the export says so.
    let embedded = fx.cli(&["leaderboard", "export", "arena", "--json"]);
    assert_eq!(code(&embedded), Some(0));
    let v: serde_json::Value = serde_json::from_slice(&embedded.stdout).unwrap();
    assert_eq!(v["source"]["key_pinned"], false);
    assert_eq!(v["source"]["verifying_key"], key.as_str());
}

#[test]
fn an_arena_that_does_not_verify_writes_nothing() {
    let fx = Fixture::with_arena();
    let wrong = "0".repeat(63) + "1";
    let out = fx.cli(&[
        "leaderboard",
        "export",
        "arena",
        "--out",
        "x",
        "--pubkey",
        &wrong,
    ]);
    assert_eq!(code(&out), Some(1), "{out:?}");
    assert!(!fx.path("x").exists());

    let board = fx.path("arena/windows/w1/board.json");
    let text = read(&board);
    let forged = text.replacen("beta", "bet4", 1);
    assert_ne!(forged, text);
    std::fs::write(&board, forged).unwrap();
    let out = fx.cli(&["leaderboard", "export", "arena", "--out", "x", "--json"]);
    assert_eq!(code(&out), Some(1), "{out:?}");
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["ok"], false);
    assert!(v["error"]
        .as_str()
        .unwrap()
        .starts_with("the arena does not verify"));
    assert!(!fx.path("x").exists());
}

#[test]
fn usage_errors_exit_2() {
    let fx = Fixture::with_arena();
    std::fs::write(fx.path("board.json"), "{}").unwrap();
    for args in [
        vec!["leaderboard"],
        vec!["leaderboard", "publish"],
        vec!["leaderboard", "export"],
        vec!["leaderboard", "export", "--out", "x"],
        vec!["leaderboard", "export", "arena", "--out"],
        vec!["leaderboard", "export", "arena", "--out", "--json"],
        vec!["leaderboard", "export", "arena", "--key", "k"],
        vec!["leaderboard", "export", "arena", "--public"],
        vec!["leaderboard", "export", "arena", "--expect-head", "nope"],
        vec!["leaderboard", "export", "board.json"],
        vec![
            "leaderboard",
            "export",
            "board.json",
            "--key",
            "k",
            "--public",
        ],
        vec![
            "leaderboard",
            "export",
            "board.json",
            "--public",
            "--expect-head",
            "1:00",
        ],
    ] {
        assert_eq!(code(&fx.cli(&args)), Some(2), "{args:?}");
    }
    assert!(!fx.path("x").exists());
}

/// A board file from `sharpebench sign`, exported under each way it can be
/// verified, and refused under a wrong key or after an edit.
#[test]
fn a_signed_board_file_exports_only_under_a_key_that_verifies_it() {
    use sharpebench_core::{AgentSubmission, Run};
    let fx = Fixture::new();
    let sub = |id: &str, mean: f64| AgentSubmission {
        agent_id: id.to_string(),
        runs: (0..4)
            .map(|_| Run {
                returns: (0..60)
                    .map(|i| mean + 0.004 * (i as f64 * 0.7).sin())
                    .collect(),
                cost: 0.5,
                ..Run::default()
            })
            .collect(),
        in_sample_trials: 0,
        candidates: Vec::new(),
    };
    let field = vec![sub("skilled", 0.002), sub("noise", 0.0)];
    std::fs::write(fx.path("subs.json"), serde_json::to_string(&field).unwrap()).unwrap();
    let signed = fx.cli(&[
        "sign",
        "subs.json",
        "hmac-secret",
        "board.json",
        "--ed25519",
        "ed-secret",
        "--json",
    ]);
    assert_eq!(code(&signed), Some(0), "{signed:?}");
    let signed: serde_json::Value = serde_json::from_slice(&signed.stdout).unwrap();
    let pubkey = signed["verifying_key"].as_str().unwrap().to_string();

    let export = |flags: &[&str]| {
        let mut args = vec!["leaderboard", "export", "board.json", "--json"];
        args.extend_from_slice(flags);
        fx.cli(&args)
    };
    for (flags, scheme, pinned) in [
        (vec!["--key", "hmac-secret"], "hmac-sha256", true),
        (vec!["--public"], "ed25519", false),
        (vec!["--pubkey", pubkey.as_str()], "ed25519", true),
    ] {
        let out = export(&flags);
        assert_eq!(code(&out), Some(0), "{flags:?} {out:?}");
        let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
        assert_eq!(v["source"]["kind"], "signed_board");
        assert_eq!(v["source"]["scheme"], scheme);
        assert_eq!(v["source"]["key_pinned"], pinned);
        assert_eq!(v["boards"][0]["board_path"], "board.json");
        let agents: Vec<&str> = v["rows"]
            .as_array()
            .unwrap()
            .iter()
            .map(|r| r["headline"]["agent_id"].as_str().unwrap())
            .collect();
        assert_eq!(agents, ["skilled", "noise"]);
        assert_eq!(
            v["rows"][0]["headline"]["qualifiers"]["evidence"]["status"],
            "unknown"
        );
        assert_eq!(v["rows"][0]["cost"]["total"], 2.0);
    }
    // Both chains sign the same payloads, so the rows agree.
    let rows = |flags: &[&str]| {
        let v: serde_json::Value = serde_json::from_slice(&export(flags).stdout).unwrap();
        v["rows"].as_array().unwrap().len()
    };
    assert_eq!(rows(&["--key", "hmac-secret"]), rows(&["--public"]));

    assert_eq!(code(&export(&["--key", "wrong-secret"])), Some(1));
    assert_eq!(code(&export(&["--pubkey", &"ab".repeat(32)])), Some(1));
    let text = read(&fx.path("board.json"));
    std::fs::write(fx.path("board.json"), text.replace("skilled", "skil1ed")).unwrap();
    for flags in [vec!["--key", "hmac-secret"], vec!["--public"]] {
        assert_eq!(code(&export(&flags)), Some(1), "{flags:?}");
    }
}
