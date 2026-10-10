//! Certification claims must follow signed row provenance, not metadata alone.

#[path = "../../sharpebench-cli/src/arena_cmd.rs"]
mod arena_cmd;

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use serde_json::{json, Value};
use sharpebench_arena::{
    verify_arena, Arena, IntakeOptions, RevealedEntry, SigningKey, BOARD_FILE, WINDOWS_DIR,
    WINDOW_FILE,
};
use sharpebench_attest::{content_digest, make_commitment, publish_public_chain, PublicChain};
use sharpebench_core::{AgentSubmission, Run, ScoreConfig};

static NEXT_DIR: AtomicU64 = AtomicU64::new(0);

struct Fixture {
    dir: PathBuf,
    key: SigningKey,
    window: Value,
    payloads: Vec<Value>,
}

impl Fixture {
    fn new() -> Self {
        let nonce = NEXT_DIR.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!(
            "sharpebench-certification-{}-{nonce}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let mut arena = Arena::init(&dir).unwrap();
        let dataset = dir.join("dataset.csv");
        std::fs::write(&dataset, b"sym,close\nA,1.0\nA,1.01\n").unwrap();
        arena
            .open_window("w1", 10, 20, ScoreConfig::default())
            .unwrap();
        let artifact = content_digest(b"certification-fixture-entrant");
        arena
            .register_entry("w1", make_commitment("alpha", "w1", &artifact, "salt"))
            .unwrap();
        arena.advance(20).unwrap();
        let entry = RevealedEntry {
            agent_id: None,
            submission: Some(AgentSubmission {
                agent_id: "alpha".to_string(),
                runs: vec![Run {
                    returns: (0..40).map(|i| 0.001 * (i as f64 + 1.0).sin()).collect(),
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
        let key = SigningKey::derive(b"certification-fixture-key");
        arena.publish("w1", &key).unwrap();
        let base = dir.join(WINDOWS_DIR).join("w1");
        let window =
            serde_json::from_slice(&std::fs::read(base.join(WINDOW_FILE)).unwrap()).unwrap();
        let board: PublicChain =
            serde_json::from_slice(&std::fs::read(base.join(BOARD_FILE)).unwrap()).unwrap();
        let payloads = board
            .chain
            .iter()
            .map(|link| serde_json::from_str(&link.payload).unwrap())
            .collect();
        Self {
            dir,
            key,
            window,
            payloads,
        }
    }

    fn claim(&mut self, certifying: Option<bool>, supplied: bool) {
        self.window["certifying"] = json!(certifying);
        self.payloads[0]["certifying"] = json!(certifying);
        self.window["supplied_returns_accepted"] = json!(supplied);
        self.payloads[0]["supplied_returns_accepted"] = json!(supplied);
    }

    fn write(&self) {
        let base = self.dir.join(WINDOWS_DIR).join("w1");
        std::fs::write(
            base.join(WINDOW_FILE),
            serde_json::to_vec(&self.window).unwrap(),
        )
        .unwrap();
        let strings: Vec<String> = self
            .payloads
            .iter()
            .map(|value| serde_json::to_string(value).unwrap())
            .collect();
        let board = publish_public_chain(&strings, &self.key);
        std::fs::write(base.join(BOARD_FILE), serde_json::to_vec(&board).unwrap()).unwrap();
    }

    fn rejected(&self, reason: &str) {
        self.write();
        let report = verify_arena(&self.dir, None).unwrap();
        let window = &report.windows[0];
        // These are genuinely signed, identity-matched documents. The failure
        // must come from semantic recomputation, not a signature mismatch.
        assert!(window.chain_ok && window.anchor_ok && window.key_ok);
        assert!(window.identity_mismatches.is_empty(), "{report:?}");
        assert!(
            !report.ok,
            "contradictory certification was accepted: {report:?}"
        );
        assert!(window.detail.contains(reason), "{report:?}");
        let value = serde_json::to_value(window).unwrap();
        assert!(value["certification_error"].is_string());
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

#[test]
fn current_noncertifying_board_retains_clean_report_bytes() {
    let fixture = Fixture::new();
    let report = verify_arena(&fixture.dir, None).unwrap();
    assert!(report.ok);
    assert_eq!(report.windows[0].detail, "ok");
    assert!(!serde_json::to_string(&report)
        .unwrap()
        .contains("certification_error"));
}

#[test]
fn supplied_and_replayed_rows_cannot_support_a_true_claim() {
    for provenance in ["supplied", "replayed"] {
        let mut fixture = Fixture::new();
        fixture.claim(Some(true), false);
        fixture.payloads[1]["returns_provenance"] = json!(provenance);
        fixture.rejected("signed rows derive false");
    }
}

#[test]
fn supplied_intake_with_reexecuted_rows_is_still_noncertifying() {
    let mut fixture = Fixture::new();
    fixture.claim(Some(true), true);
    fixture.payloads[1]["returns_provenance"] = json!("re-executed");
    fixture.rejected("signed rows derive false");
}

#[test]
fn an_empty_signed_board_certifies_nothing() {
    let mut fixture = Fixture::new();
    fixture.claim(Some(true), false);
    fixture.payloads.truncate(1);
    fixture.rejected("signed rows derive false");
}

#[test]
fn a_false_claim_cannot_disagree_with_reexecuted_rows_either() {
    let mut fixture = Fixture::new();
    fixture.claim(Some(false), false);
    fixture.payloads[1]["returns_provenance"] = json!("re-executed");
    fixture.rejected("signed rows derive true");
    fixture.claim(Some(true), false);
    fixture.write();
    assert!(verify_arena(&fixture.dir, None).unwrap().ok);
}

#[test]
fn current_rows_need_provenance_and_unique_identity() {
    let mut fixture = Fixture::new();
    fixture.payloads[1]
        .as_object_mut()
        .unwrap()
        .remove("returns_provenance");
    fixture.rejected("no returns provenance");
    fixture.payloads[1]["returns_provenance"] = json!("supplied");
    fixture.payloads.push(fixture.payloads[1].clone());
    fixture.rejected("repeats agent");
}

#[test]
fn malformed_signed_rows_fail_the_explicit_claim() {
    let mut fixture = Fixture::new();
    fixture.payloads[1] = json!({});
    fixture.rejected("not a scored row");
}

#[test]
fn cli_verify_returns_failure_for_a_signed_certification_contradiction() {
    let mut fixture = Fixture::new();
    fixture.claim(Some(true), false);
    fixture.write();
    let args = vec![
        "sharpebench".to_string(),
        "arena".to_string(),
        "verify".to_string(),
        fixture.dir.to_str().unwrap().to_string(),
    ];
    for json_output in [false, true] {
        assert_eq!(arena_cmd::run(&args, json_output), 1);
    }
}

#[test]
fn legacy_headers_stay_valid_but_never_gain_certification() {
    let mut fixture = Fixture::new();
    fixture.claim(None, false);
    fixture.window["schema_version"] = json!(2);
    fixture.payloads[0]["schema_version"] = json!(2);
    fixture
        .window
        .as_object_mut()
        .unwrap()
        .remove("returns_provenance");
    fixture.payloads[1]
        .as_object_mut()
        .unwrap()
        .remove("returns_provenance");
    fixture.write();
    let report = verify_arena(&fixture.dir, None).unwrap();
    assert!(report.ok, "{report:?}");
    let header: sharpebench_arena::WindowHeader =
        serde_json::from_value(fixture.payloads[0].clone()).unwrap();
    assert_ne!(header.certifying, Some(true));
    assert!(!serde_json::to_string(&report)
        .unwrap()
        .contains("certification_error"));
}
