//! Deterministic interrupted-save fixtures, not hardware/power-loss experiments.

use std::fs::OpenOptions;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use serde_json::{json, Value};
use sharpebench_arena::{
    verify_arena, Arena, IntakeOptions, RevealedEntry, SigningKey, STATE_FILE, WINDOWS_DIR,
    WINDOW_FILE,
};
use sharpebench_attest::{content_digest, make_commitment};
use sharpebench_core::{AgentSubmission, Run, ScoreConfig};

static NEXT_DIR: AtomicU64 = AtomicU64::new(0);
const REDO: &str = ".arena-redo.json";

struct Fixture(PathBuf);

impl Fixture {
    fn new() -> Self {
        let dir = std::env::temp_dir().join(format!(
            "sharpe-arena-recovery-{}-{}-{}",
            std::process::id(),
            NEXT_DIR.fetch_add(1, Ordering::Relaxed),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let mut arena = Arena::init(&dir).unwrap();
        arena
            .open_window("w1", 10, 20, ScoreConfig::default())
            .unwrap();
        Self(dir)
    }

    fn state(&self) -> Value {
        serde_json::from_slice(&std::fs::read(self.0.join(STATE_FILE)).unwrap()).unwrap()
    }

    fn window(&self) -> Value {
        serde_json::from_slice(
            &std::fs::read(self.0.join(WINDOWS_DIR).join("w1").join(WINDOW_FILE)).unwrap(),
        )
        .unwrap()
    }

    fn entry(path: &str, value: &Value) -> Value {
        let text = serde_json::to_string_pretty(value).unwrap();
        json!({"path": path, "sha256": content_digest(text.as_bytes()), "text": text})
    }

    fn pending(&self, entries: Vec<Value>) {
        std::fs::write(
            self.0.join(REDO),
            serde_json::to_vec(&json!({"version": 1, "entries": entries})).unwrap(),
        )
        .unwrap();
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn load_replays_an_interrupted_windows_first_state_last_transaction() {
    let fixture = Fixture::new();
    let mut state = fixture.state();
    state["current_epoch"] = json!(10);
    let mut window = fixture.window();
    window["status"] = json!("committed");
    fixture.pending(vec![
        Fixture::entry("windows/w1/window.json", &window),
        Fixture::entry(STATE_FILE, &state),
    ]);
    // Simulate interruption after the first atomic replacement, before state.
    std::fs::write(
        fixture.0.join("windows/w1/window.json"),
        serde_json::to_vec_pretty(&window).unwrap(),
    )
    .unwrap();
    let arena = Arena::load(&fixture.0).unwrap();
    assert_eq!(arena.current_epoch(), 10);
    assert_eq!(
        serde_json::to_value(arena.window("w1").unwrap()).unwrap()["status"],
        "committed"
    );
    assert!(!fixture.0.join(REDO).exists());
    assert_eq!(Arena::load(&fixture.0).unwrap().current_epoch(), 10);
}

#[test]
fn read_only_verification_refuses_pending_without_replaying_it() {
    let fixture = Fixture::new();
    let mut state = fixture.state();
    state["current_epoch"] = json!(7);
    fixture.pending(vec![Fixture::entry(STATE_FILE, &state)]);
    let before = std::fs::read(fixture.0.join(STATE_FILE)).unwrap();
    assert!(verify_arena(&fixture.0, None)
        .unwrap_err()
        .contains("pending transaction"));
    assert_eq!(std::fs::read(fixture.0.join(STATE_FILE)).unwrap(), before);
    assert!(fixture.0.join(REDO).exists());
}

#[test]
fn every_recovery_target_is_validated_before_any_replacement() {
    let fixture = Fixture::new();
    let before = std::fs::read(fixture.0.join("windows/w1/window.json")).unwrap();
    let mut window = fixture.window();
    window["status"] = json!("committed");
    fixture.pending(vec![
        Fixture::entry("windows/w1/window.json", &window),
        Fixture::entry("../state.json", &fixture.state()),
        Fixture::entry(STATE_FILE, &fixture.state()),
    ]);
    assert!(Arena::load(&fixture.0).is_err());
    assert_eq!(
        std::fs::read(fixture.0.join("windows/w1/window.json")).unwrap(),
        before
    );
    assert!(fixture.0.join(REDO).exists());
}

#[test]
fn recovery_validates_later_content_before_replacing_earlier_files() {
    let fixture = Fixture::new();
    let before = std::fs::read(fixture.0.join("windows/w1/window.json")).unwrap();
    let mut window = fixture.window();
    window["status"] = json!("committed");
    let malformed = json!({"not": "an arena state"});
    fixture.pending(vec![
        Fixture::entry("windows/w1/window.json", &window),
        Fixture::entry(STATE_FILE, &malformed),
    ]);
    assert!(Arena::load(&fixture.0).is_err());
    assert_eq!(
        std::fs::read(fixture.0.join("windows/w1/window.json")).unwrap(),
        before
    );
}

#[test]
fn stale_writers_do_not_overwrite_a_newer_coherent_snapshot() {
    let fixture = Fixture::new();
    let mut first = Arena::load(&fixture.0).unwrap();
    let mut stale = Arena::load(&fixture.0).unwrap();
    first.advance(5).unwrap();
    assert!(stale.advance(6).unwrap_err().contains("snapshot changed"));
    assert_eq!(Arena::load(&fixture.0).unwrap().current_epoch(), 5);
}

#[test]
fn prospective_snapshot_completeness_is_checked_before_recovery_writes() {
    let fixture = Fixture::new();
    let before = std::fs::read(fixture.0.join("windows/w1/window.json")).unwrap();
    let mut window = fixture.window();
    window["status"] = json!("committed");
    let mut state = fixture.state();
    state["window_order"] = json!(["w1", "missing"]);
    fixture.pending(vec![
        Fixture::entry("windows/w1/window.json", &window),
        Fixture::entry(STATE_FILE, &state),
    ]);
    assert!(Arena::load(&fixture.0).is_err());
    assert_eq!(
        std::fs::read(fixture.0.join("windows/w1/window.json")).unwrap(),
        before
    );
    assert_eq!(fixture.state()["window_order"], json!(["w1"]));
    assert!(fixture.0.join(REDO).exists());
}

#[test]
fn an_existing_os_writer_lock_refuses_a_second_writer_and_reader() {
    let fixture = Fixture::new();
    let lock = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(fixture.0.join(".arena.lock"))
        .unwrap();
    lock.try_lock().unwrap();
    assert!(Arena::load(&fixture.0).is_err());
    assert!(verify_arena(&fixture.0, None).is_err());
    drop(lock);
    assert!(Arena::load(&fixture.0).is_ok());
}

#[test]
fn failed_window_replacement_never_advances_the_state_file_first() {
    let fixture = Fixture::new();
    let mut arena = Arena::load(&fixture.0).unwrap();
    let before = std::fs::read(fixture.0.join(STATE_FILE)).unwrap();
    // An existing target directory is a deterministic filesystem failure,
    // not a claim to reproduce actual disk exhaustion or a power cut.
    let blocked = fixture.0.join("windows/w2/window.json");
    std::fs::create_dir_all(&blocked).unwrap();
    assert!(arena
        .open_window("w2", 10, 20, ScoreConfig::default())
        .is_err());
    assert_eq!(std::fs::read(fixture.0.join(STATE_FILE)).unwrap(), before);
    assert!(!fixture.0.join(REDO).exists());
    std::fs::remove_dir(&blocked).unwrap();
    let mut recovered = Arena::load(&fixture.0).unwrap();
    assert_eq!(recovered.window_ids(), ["w1"]);
    recovered
        .open_window("w2", 10, 20, ScoreConfig::default())
        .unwrap();
}

#[test]
fn an_interrupted_publish_recovers_board_window_and_state_together() {
    let fixture = Fixture::new();
    let mut arena = Arena::load(&fixture.0).unwrap();
    let artifact = content_digest(b"persistence-fixture-entrant");
    arena
        .register_entry("w1", make_commitment("alpha", "w1", &artifact, "salt"))
        .unwrap();
    arena.advance(20).unwrap();
    let dataset = fixture.0.join("dataset.csv");
    std::fs::write(&dataset, b"sym,close\nA,1\nA,1.01\n").unwrap();
    let entry = RevealedEntry {
        agent_id: None,
        submission: Some(AgentSubmission {
            agent_id: "alpha".to_string(),
            runs: vec![Run {
                returns: vec![0.001, -0.001, 0.002],
                ..Run::default()
            }],
            in_sample_trials: 0,
            candidates: Vec::new(),
        }),
        capture: None,
        artifact_digest: artifact,
        salt: "salt".to_string(),
        fault_plan_sha256: None,
    };
    arena
        .reveal_and_score_with(
            "w1",
            &dataset,
            &[entry],
            IntakeOptions {
                allow_supplied_returns: true,
                reexecute: None,
            },
        )
        .unwrap();
    let before = std::fs::read(fixture.0.join(STATE_FILE)).unwrap();
    // board.json replaces successfully; board.md fails on an existing folder.
    let blocked = fixture.0.join("windows/w1/board.md");
    std::fs::create_dir(&blocked).unwrap();
    assert!(arena
        .publish("w1", &SigningKey::derive(b"persistence-fixture-key"))
        .is_err());
    assert_eq!(std::fs::read(fixture.0.join(STATE_FILE)).unwrap(), before);
    assert!(fixture.0.join(REDO).exists());
    assert!(arena.advance(21).unwrap_err().contains("reloaded"));
    std::fs::remove_dir(&blocked).unwrap();
    let recovered = Arena::load(&fixture.0).unwrap();
    assert_eq!(recovered.published_ids(), ["w1"]);
    assert!(verify_arena(&fixture.0, None).unwrap().ok);
    assert!(!fixture.0.join(REDO).exists());
}
