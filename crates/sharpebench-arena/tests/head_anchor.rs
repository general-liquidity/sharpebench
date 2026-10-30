//! The signed terminal anchor: dropping final windows with their state entries
//! must fail verification, an old arena must verify under its stated limit, and
//! the one rollback the documents cannot show must fall to a pinned head.

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use serde_json::Value;
use sharpebench_arena::{
    head_digest, verify_arena, verify_arena_expecting, Arena, ArenaHead, ArenaVerification,
    CompletenessStatus, ExpectedHead, VerifyingKey, ARENA_HEAD_KIND, BOARD_FILE, HEAD_FILE,
    LEGACY_COMPLETENESS_LIMIT, STATE_FILE, WINDOWS_DIR,
};
use sharpebench_attest::{publish_public_chain, PublicChain, SigningKey};
use sharpebench_core::ScoreConfig;

static NEXT_DIR: AtomicU64 = AtomicU64::new(0);

fn key() -> SigningKey {
    SigningKey::derive(b"head-anchor-host-secret")
}

fn attacker() -> SigningKey {
    SigningKey::derive(b"head-anchor-attacker-secret")
}

struct Fixture(PathBuf);

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

impl Fixture {
    fn new() -> Self {
        let dir = std::env::temp_dir().join(format!(
            "sharpe-arena-head-{}-{}",
            std::process::id(),
            NEXT_DIR.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = std::fs::remove_dir_all(&dir);
        Arena::init(&dir).unwrap();
        Self(dir)
    }

    /// Publish `count` boards with no entrant, so the fixture needs no capture.
    fn published(count: u64) -> Self {
        let fixture = Self::new();
        for n in 1..=count {
            fixture.publish(n);
        }
        fixture
    }

    fn publish(&self, n: u64) {
        let id = format!("w{n}");
        let mut arena = Arena::load(&self.0).unwrap();
        let start = arena.current_epoch();
        arena
            .open_window(&id, start + 1, start + 2, ScoreConfig::default())
            .unwrap();
        arena.advance(start + 2).unwrap();
        let dataset = self.0.join("dataset.csv");
        std::fs::write(&dataset, "date,symbol,close\n").unwrap();
        arena.reveal_and_score(&id, &dataset, &[]).unwrap();
        arena.publish(&id, &key()).unwrap();
    }

    fn head_bytes(&self) -> Vec<u8> {
        std::fs::read(self.0.join(HEAD_FILE)).unwrap()
    }

    fn write_head_document(&self, document: &PublicChain) {
        std::fs::write(
            self.0.join(HEAD_FILE),
            serde_json::to_vec(document).unwrap(),
        )
        .unwrap();
    }

    fn board(&self, id: &str) -> PublicChain {
        let path = self.0.join(WINDOWS_DIR).join(id).join(BOARD_FILE);
        serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap()
    }

    /// Drop the newest window the way the attack does: its directory and its
    /// entries in the unsigned state file, leaving nothing dangling.
    fn drop_final_window(&self, id: &str) {
        std::fs::remove_dir_all(self.0.join(WINDOWS_DIR).join(id)).unwrap();
        let path = self.0.join(STATE_FILE);
        let mut state: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        for key in ["window_order", "published_order"] {
            state[key].as_array_mut().unwrap().retain(|v| v != id);
        }
        std::fs::write(&path, serde_json::to_vec_pretty(&state).unwrap()).unwrap();
    }

    fn verify(&self) -> ArenaVerification {
        verify_arena(&self.0, None).unwrap()
    }
}

fn head(count: u64, window: &str, digest: String) -> String {
    serde_json::to_string(&ArenaHead {
        kind: ARENA_HEAD_KIND.to_string(),
        published_count: count,
        head_window_id: window.to_string(),
        head_digest: digest,
    })
    .unwrap()
}

fn assert_broken(report: &ArenaVerification, reason: &str) {
    assert!(!report.ok, "{report:?}");
    assert_eq!(report.completeness.status, CompletenessStatus::Broken);
    assert!(report.completeness.detail.contains(reason), "{report:?}");
}

fn expected(report: &ArenaVerification) -> ExpectedHead {
    ExpectedHead {
        published_count: report.completeness.published_count,
        head_digest: report.completeness.head_digest.clone().unwrap(),
    }
}

#[test]
fn every_publish_rewrites_a_head_naming_the_newest_board() {
    let fixture = Fixture::published(2);
    let report = fixture.verify();
    assert!(report.ok, "{report:?}");
    assert_eq!(report.completeness.status, CompletenessStatus::Anchored);
    assert_eq!(report.completeness.published_count, 2);
    let document: PublicChain = serde_json::from_slice(&fixture.head_bytes()).unwrap();
    let recorded: ArenaHead = serde_json::from_str(&document.chain[0].payload).unwrap();
    let last = fixture.board("w2").chain.last().unwrap().signature.clone();
    assert_eq!(recorded.published_count, 2);
    assert_eq!(recorded.head_window_id, "w2");
    assert_eq!(recorded.head_digest, head_digest(&last));
    assert_eq!(
        report.completeness.head_digest.as_ref(),
        Some(&recorded.head_digest)
    );
}

#[test]
fn dropping_the_final_window_with_its_state_entries_fails_verification() {
    let fixture = Fixture::published(3);
    assert!(fixture.verify().ok);
    fixture.drop_final_window("w3");
    let report = fixture.verify();
    // Each surviving board is intact; only the head can see the loss.
    assert!(report
        .windows
        .iter()
        .all(|w| w.chain_ok && w.anchor_ok && w.key_ok));
    assert_broken(&report, "signed head records 3 published window(s) but 2");
}

#[test]
fn dropping_every_window_fails_while_the_head_remains() {
    let fixture = Fixture::published(1);
    fixture.drop_final_window("w1");
    assert_broken(&fixture.verify(), "signed head records 1 published");
}

#[test]
fn deleting_the_head_cannot_pass_an_anchored_arena_off_as_legacy() {
    let fixture = Fixture::published(2);
    std::fs::remove_file(fixture.0.join(HEAD_FILE)).unwrap();
    assert_broken(
        &fixture.verify(),
        "missing but a published board requires it",
    );
    fixture.drop_final_window("w2");
    assert_broken(
        &fixture.verify(),
        "missing but a published board requires it",
    );
}

#[test]
fn a_head_under_another_key_fails_with_or_without_a_pin() {
    let fixture = Fixture::published(2);
    let digest = head_digest(&fixture.board("w2").chain.last().unwrap().signature);
    fixture.write_head_document(&publish_public_chain(&[head(2, "w2", digest)], &attacker()));
    assert_broken(&fixture.verify(), "not signed by the arena key");
    let pinned: VerifyingKey = key().verifying_key();
    let report = verify_arena(&fixture.0, Some(&pinned)).unwrap();
    assert_broken(&report, "not signed by the arena key");
}

#[test]
fn a_signed_document_that_is_not_one_head_is_refused() {
    let fixture = Fixture::published(2);
    fixture.write_head_document(&fixture.board("w2"));
    assert_broken(&fixture.verify(), "not a head");

    let digest = head_digest(&fixture.board("w2").chain.last().unwrap().signature);
    let right = head(2, "w2", digest);
    fixture.write_head_document(&publish_public_chain(
        &[right.clone(), right.clone()],
        &key(),
    ));
    assert_broken(&fixture.verify(), "exactly one link");

    let wrong_kind = right.replace(ARENA_HEAD_KIND, "something-else");
    fixture.write_head_document(&publish_public_chain(&[wrong_kind], &key()));
    assert_broken(&fixture.verify(), "has kind");

    std::fs::write(fixture.0.join(HEAD_FILE), b"not json").unwrap();
    assert_broken(&fixture.verify(), "invalid JSON");
}

#[test]
fn a_head_with_the_right_count_and_the_wrong_digest_or_window_fails() {
    let fixture = Fixture::published(2);
    let digest = head_digest(&fixture.board("w2").chain.last().unwrap().signature);
    let stale = head_digest(&fixture.board("w1").chain.last().unwrap().signature);
    fixture.write_head_document(&publish_public_chain(&[head(2, "w2", stale)], &key()));
    assert_broken(&fixture.verify(), "digest the newest board does not have");
    fixture.write_head_document(&publish_public_chain(&[head(2, "w1", digest)], &key()));
    assert_broken(&fixture.verify(), "digest the newest board does not have");
}

/// The limit the head leaves open, pinned so that it stays stated: an earlier
/// head is genuinely signed, so restoring it with its windows is consistent in
/// the documents, and only a head seen out of band exposes it.
#[test]
fn a_rollback_to_an_earlier_head_needs_a_pinned_head_to_show() {
    let fixture = Fixture::published(1);
    let earlier = fixture.head_bytes();
    let first = expected(&fixture.verify());
    fixture.publish(2);
    let second = expected(&fixture.verify());

    // A pin taken earlier still passes once the arena has grown.
    let grown = verify_arena_expecting(&fixture.0, None, Some(&first)).unwrap();
    assert!(grown.ok, "{grown:?}");

    fixture.drop_final_window("w2");
    std::fs::write(fixture.0.join(HEAD_FILE), &earlier).unwrap();
    let report = fixture.verify();
    assert!(report.ok, "{report:?}");
    assert!(report.completeness.detail.contains("rollback"));

    let pinned = verify_arena_expecting(&fixture.0, None, Some(&second)).unwrap();
    assert_broken(&pinned, "expected head records 2");
}

#[test]
fn a_pinned_head_on_another_history_fails() {
    let fixture = Fixture::published(2);
    let pin = ExpectedHead {
        published_count: 1,
        head_digest: "0".repeat(64),
    };
    let report = verify_arena_expecting(&fixture.0, None, Some(&pin)).unwrap();
    assert_broken(&report, "does not have the expected head digest");
}

#[test]
fn an_arena_published_before_the_head_verifies_under_its_stated_limit() {
    let fixture = Fixture::published(1);
    let board = fixture.board("w1");
    let mut header: Value = serde_json::from_str(&board.chain[0].payload).unwrap();
    header.as_object_mut().unwrap().remove("head_anchored");
    let legacy = publish_public_chain(&[serde_json::to_string(&header).unwrap()], &key());
    let path = fixture.0.join(WINDOWS_DIR).join("w1").join(BOARD_FILE);
    std::fs::write(path, serde_json::to_vec(&legacy).unwrap()).unwrap();
    std::fs::remove_file(fixture.0.join(HEAD_FILE)).unwrap();

    let report = fixture.verify();
    assert!(report.ok, "{report:?}");
    assert_eq!(
        report.completeness.status,
        CompletenessStatus::LegacyUnanchored
    );
    assert_eq!(report.completeness.detail, LEGACY_COMPLETENESS_LIMIT);
    assert_eq!(report.completeness.published_count, 1);

    // A pinned head reaches a legacy arena too: it is what the limit points to.
    let pin = expected(&report);
    assert!(
        verify_arena_expecting(&fixture.0, None, Some(&pin))
            .unwrap()
            .ok
    );

    // The next publish anchors the whole history, the legacy board included.
    fixture.publish(2);
    let report = fixture.verify();
    assert!(report.ok, "{report:?}");
    assert_eq!(report.completeness.status, CompletenessStatus::Anchored);
    assert_eq!(report.completeness.published_count, 2);
}

#[test]
fn an_arena_with_nothing_published_states_what_that_leaves_unproved() {
    let fixture = Fixture::new();
    let report = fixture.verify();
    assert!(report.ok, "{report:?}");
    assert_eq!(
        report.completeness.status,
        CompletenessStatus::NoPublishedWindows
    );
    assert!(report.completeness.detail.contains("cannot show"));
    assert_eq!(report.completeness.head_digest, None);
}

#[test]
fn a_changed_head_makes_a_stale_writer_reload() {
    let fixture = Fixture::published(1);
    let mut stale = Arena::load(&fixture.0).unwrap();
    let digest = head_digest(&fixture.board("w1").chain.last().unwrap().signature);
    fixture.write_head_document(&publish_public_chain(&[head(1, "w1", digest)], &attacker()));
    let error = stale.advance(99).unwrap_err();
    assert!(error.contains("snapshot changed"), "{error}");
}

#[test]
fn expected_head_parsing_is_strict() {
    let digest = "a".repeat(64);
    assert_eq!(
        ExpectedHead::parse(&format!("3:{digest}")).unwrap(),
        ExpectedHead {
            published_count: 3,
            head_digest: digest.clone()
        }
    );
    for bad in [
        digest.clone(),
        format!("0:{digest}"),
        format!("-1:{digest}"),
        format!("x:{digest}"),
        "3:abc".to_string(),
        format!("3:{}", "A".repeat(64)),
    ] {
        assert!(ExpectedHead::parse(&bad).is_err(), "{bad}");
    }
}
