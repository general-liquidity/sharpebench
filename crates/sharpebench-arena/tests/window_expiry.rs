//! A window nobody entered ends at `expired-empty`: reachable only through the
//! arena, only once nothing can happen to it, and never over an entrant.

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use serde_json::{json, Value};
use sharpebench_arena::{verify_arena, Arena, WindowStatus, STATE_FILE, WINDOWS_DIR, WINDOW_FILE};
use sharpebench_attest::{content_digest, make_commitment, SigningKey};
use sharpebench_core::ScoreConfig;

static NEXT_DIR: AtomicU64 = AtomicU64::new(0);
const REDO: &str = ".arena-redo.json";

struct Fixture(PathBuf);

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

impl Fixture {
    fn new() -> (Self, Arena) {
        let dir = std::env::temp_dir().join(format!(
            "sharpe-arena-expiry-{}-{}",
            std::process::id(),
            NEXT_DIR.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = std::fs::remove_dir_all(&dir);
        let mut arena = Arena::init(&dir).unwrap();
        arena
            .open_window("w1", 10, 20, ScoreConfig::default())
            .unwrap();
        (Self(dir), arena)
    }

    fn window_path(&self) -> PathBuf {
        self.0.join(WINDOWS_DIR).join("w1").join(WINDOW_FILE)
    }

    fn window(&self) -> Value {
        serde_json::from_slice(&std::fs::read(self.window_path()).unwrap()).unwrap()
    }

    fn write_window(&self, value: &Value) {
        std::fs::write(
            self.window_path(),
            serde_json::to_vec_pretty(value).unwrap(),
        )
        .unwrap();
    }
}

type Edit = fn(&mut Value);

fn expired() -> (Fixture, Arena) {
    let (fixture, mut arena) = Fixture::new();
    arena.advance(25).unwrap();
    assert_eq!(arena.expire_empty_window("w1"), Ok(25));
    (fixture, arena)
}

#[test]
fn an_empty_window_past_its_reveal_expires_and_keeps_its_frozen_record() {
    let (fixture, mut arena) = Fixture::new();
    arena.advance(25).unwrap();
    let before = fixture.window();
    assert_eq!(arena.expire_empty_window("w1"), Ok(25));

    let after = fixture.window();
    assert_eq!(after["status"], "expired-empty");
    assert_eq!(after["expired_at_epoch"], 25);
    let mut restored = after.clone();
    let record = restored.as_object_mut().unwrap();
    record.remove("expired_at_epoch");
    record.insert("status".to_string(), json!("committed"));
    assert_eq!(restored, before);

    let reloaded = Arena::load(&fixture.0).unwrap();
    let window = reloaded.window("w1").unwrap();
    assert_eq!(window.status, WindowStatus::ExpiredEmpty);
    assert_eq!(window.expired_at_epoch, Some(25));
    let report = verify_arena(&fixture.0, None).unwrap();
    assert!(report.ok && report.windows.is_empty(), "{report:?}");
}

#[test]
fn a_window_that_could_still_change_does_not_expire() {
    let (fixture, mut arena) = Fixture::new();
    let error = arena.expire_empty_window("w1").unwrap_err();
    assert!(error.contains("only a committed window"), "{error}");
    arena.advance(19).unwrap();
    let error = arena.expire_empty_window("w1").unwrap_err();
    assert!(error.contains("has not reached its data-reveal"), "{error}");
    assert_eq!(fixture.window()["status"], "committed");
    assert!(arena.expire_empty_window("missing").is_err());
}

#[test]
fn the_reveal_epoch_itself_is_late_enough() {
    let (_fixture, mut arena) = Fixture::new();
    arena.advance(20).unwrap();
    assert_eq!(arena.expire_empty_window("w1"), Ok(20));
}

#[test]
fn a_window_with_an_entrant_never_expires() {
    let (fixture, mut arena) = Fixture::new();
    let digest = content_digest(b"artifact");
    arena
        .register_entry("w1", make_commitment("alpha", "w1", &digest, "salt"))
        .unwrap();
    arena.advance(25).unwrap();
    let error = arena.expire_empty_window("w1").unwrap_err();
    assert!(error.contains("holds an entry"), "{error}");
    assert_eq!(fixture.window()["status"], "committed");
}

#[test]
fn a_scored_window_never_expires() {
    let (fixture, mut arena) = Fixture::new();
    arena.advance(25).unwrap();
    let dataset = fixture.0.join("dataset.csv");
    std::fs::write(&dataset, "date,symbol,close\n").unwrap();
    arena.reveal_and_score("w1", &dataset, &[]).unwrap();
    let error = arena.expire_empty_window("w1").unwrap_err();
    assert!(error.contains("only a committed window"), "{error}");
}

#[test]
fn an_expired_window_is_terminal() {
    let (fixture, mut arena) = expired();
    let dataset = fixture.0.join("dataset.csv");
    std::fs::write(&dataset, "date,symbol,close\n").unwrap();
    let digest = content_digest(b"artifact");
    assert!(arena
        .register_entry("w1", make_commitment("alpha", "w1", &digest, "salt"))
        .is_err());
    assert!(arena.reveal_and_score("w1", &dataset, &[]).is_err());
    assert!(arena
        .publish("w1", &SigningKey::derive(b"expiry-host"))
        .is_err());
    assert!(arena.expire_empty_window("w1").is_err());
    assert!(Arena::supersede_empty_window(&fixture.0, "w1", "reuse the id").is_err());
    assert!(arena
        .open_window("w1", 40, 50, ScoreConfig::default())
        .is_err());
}

#[test]
fn later_advances_leave_the_expired_record_byte_identical() {
    let (fixture, mut arena) = expired();
    let before = std::fs::read(fixture.window_path()).unwrap();
    arena.advance(99).unwrap();
    assert_eq!(std::fs::read(fixture.window_path()).unwrap(), before);
}

#[test]
fn a_record_expiry_could_not_have_written_is_refused_at_load() {
    let cases: [(&str, Edit); 4] = [
        ("records no expiry epoch", |w| {
            w.as_object_mut().unwrap().remove("expired_at_epoch");
        }),
        ("holds an entry", |w| {
            w["refusals"] = json!([{"agent_id": "alpha", "reason": "buried"}]);
        }),
        ("before its data-reveal epoch", |w| {
            w["expired_at_epoch"] = json!(5);
        }),
        ("is not expired", |w| {
            w["status"] = json!("committed");
        }),
    ];
    for (reason, edit) in cases {
        let (fixture, _arena) = expired();
        let mut window = fixture.window();
        edit(&mut window);
        fixture.write_window(&window);
        let error = Arena::load(&fixture.0).err().expect("load must refuse");
        assert!(error.contains(reason), "{reason}: {error}");
    }
}

#[test]
fn recovery_refuses_to_rewrite_an_expired_window() {
    let (fixture, _arena) = expired();
    let mut revived = fixture.window();
    revived["status"] = json!("committed");
    revived.as_object_mut().unwrap().remove("expired_at_epoch");
    let window_text = serde_json::to_string_pretty(&revived).unwrap();
    let state_text = std::fs::read_to_string(fixture.0.join(STATE_FILE)).unwrap();
    let entry = |path: String, text: String| json!({"path": path, "sha256": content_digest(text.as_bytes()), "text": text});
    let entries = vec![
        entry(format!("{WINDOWS_DIR}/w1/{WINDOW_FILE}"), window_text),
        entry(STATE_FILE.to_string(), state_text),
    ];
    std::fs::write(
        fixture.0.join(REDO),
        serde_json::to_vec(&json!({"version": 1, "entries": entries})).unwrap(),
    )
    .unwrap();
    let error = Arena::load(&fixture.0).err().expect("recovery must refuse");
    assert!(error.contains("terminal window"), "{error}");
    assert_eq!(fixture.window()["status"], "expired-empty");
}

#[test]
fn a_transition_is_due_only_when_a_window_would_change() {
    let (_fixture, mut arena) = Fixture::new();
    assert!(!arena.transition_due(9), "open before the deadline");
    assert!(arena.transition_due(10), "open at the deadline");
    arena.advance(10).unwrap();
    assert!(!arena.transition_due(19), "committed before the reveal");
    assert!(arena.transition_due(20), "committed at the reveal");
    arena.advance(20).unwrap();
    assert!(!arena.transition_due(21), "reveal already reached");
    arena.expire_empty_window("w1").unwrap();
    assert!(!arena.transition_due(1_000), "terminal");
}

#[test]
fn a_superseded_record_keeps_no_transition_due() {
    let (fixture, _arena) = Fixture::new();
    Arena::supersede_empty_window(&fixture.0, "w1", "withdrawn").unwrap();
    assert_eq!(fixture.window()["status"], "open");
    assert!(!Arena::load(&fixture.0).unwrap().transition_due(1_000));
}
