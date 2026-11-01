//! A replayed row's returns depend on the execution model as much as on the
//! decisions: cost model, fixed decision delay, engine and window-by-seed
//! matrix. The signed header binds that identity, so a board scored under other
//! execution settings than it states fails `verify_arena`, while a header
//! signed before the identity existed verifies under its legacy meaning.

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use serde_json::{json, Value};
use sharpebench_arena::execution::{cost_profile, execution_digest};
use sharpebench_arena::{
    head_digest, verify_arena, Arena, ArenaHead, ExecutionStatus, IdentityField, IntakeOptions,
    ReplayWindow, RevealedEntry, SigningKey, ARENA_HEAD_KIND, BOARD_FILE,
    EXECUTED_WINDOW_SCHEMA_VERSION, FAULTED_WINDOW_SCHEMA_VERSION, HEAD_FILE,
    SCORED_FAULTED_WINDOW_SCHEMA_VERSION, SCORED_WINDOW_SCHEMA_VERSION, WINDOWS_DIR, WINDOW_FILE,
    WINDOW_SCHEMA_VERSION,
};
use sharpebench_attest::{content_digest, make_commitment, publish_public_chain, PublicChain};
use sharpebench_core::{AgentSubmission, Run, ScoreConfig};
use sharpebench_harness::cost_model_digest;
use sharpebench_sim::{CostModel, Dataset, FixedDecisionDelay, Momentum};

static NEXT_DIR: AtomicU64 = AtomicU64::new(0);

fn dataset_csv(data: &Dataset) -> String {
    let mut out = String::from("date,symbol,close\n");
    for (symbol, closes) in &data.closes {
        for (date, close) in data.dates.iter().zip(closes) {
            out.push_str(&format!("{date},{symbol},{close}\n"));
        }
    }
    out
}

/// One published window, scored from a replayed capture or from supplied
/// returns, with its window record and signed payloads open for editing.
struct Fixture {
    dir: PathBuf,
    key: SigningKey,
    window: Value,
    payloads: Vec<Value>,
}

impl Fixture {
    fn new(replayed: bool) -> Self {
        let nonce = NEXT_DIR.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!(
            "sharpebench-execution-{}-{nonce}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        let mut arena = Arena::init(&dir).unwrap();
        let csv = dataset_csv(&Dataset::synthetic(4, 60, 7));
        let dataset = dir.join("dataset.csv");
        std::fs::write(&dataset, &csv).unwrap();
        arena
            .open_window("w1", 10, 20, ScoreConfig::default())
            .unwrap();
        let scorer = arena.window("w1").unwrap().scorer_artifact_sha256.clone();
        let artifact = content_digest(b"execution-identity-entrant");
        arena
            .register_entry("w1", make_commitment("alpha", "w1", &artifact, "salt"))
            .unwrap();
        arena.advance(20).unwrap();
        let entry = if replayed {
            let replay = ReplayWindow::parse(csv.as_bytes(), &ScoreConfig::default()).unwrap();
            let (_, mut capture) = sharpebench_harness::run_agent_capture(
                &format!("sandbox:test/alpha@sha256:{artifact}"),
                &replay.data,
                &replay.windows,
                &replay.seeds,
                replay.costs,
                || Box::new(Momentum::default()),
            );
            capture.contract.as_mut().unwrap().runner_artifact_sha256 = Some(scorer);
            RevealedEntry {
                live_log: None,
                agent_id: Some("alpha".to_string()),
                submission: None,
                capture: Some(capture),
                artifact_digest: artifact,
                salt: "salt".to_string(),
                fault_plan_sha256: None,
            }
        } else {
            RevealedEntry {
                live_log: None,
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
            }
        };
        arena
            .reveal_and_score_with(
                "w1",
                &dataset,
                &[entry],
                IntakeOptions {
                    allow_supplied_returns: !replayed,
                    reexecute: None,
                },
            )
            .unwrap();
        let key = SigningKey::derive(b"execution-identity-key");
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

    /// Write the window record as edited, and re-sign the board and head with
    /// the arena key, so a case fails on what it changes and not on the chain.
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
        let head = ArenaHead {
            kind: ARENA_HEAD_KIND.to_string(),
            published_count: 1,
            head_window_id: "w1".to_string(),
            head_digest: head_digest(&board.chain.last().unwrap().signature),
        };
        let document = publish_public_chain(&[serde_json::to_string(&head).unwrap()], &self.key);
        std::fs::write(
            self.dir.join(HEAD_FILE),
            serde_json::to_vec(&document).unwrap(),
        )
        .unwrap();
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

/// `costs` written into an identity as a scorer that replayed under them would
/// write it: profile, bit digest and execution digest all recomputed.
fn relabel(execution: &mut Value, costs: CostModel, scorer: &str) {
    let cost_sha = cost_model_digest(costs);
    let windows: Vec<(usize, usize)> =
        serde_json::from_value(execution["windows"].clone()).unwrap();
    let seeds: Vec<u64> = serde_json::from_value(execution["seeds"].clone()).unwrap();
    let engine = execution["engine_version"].as_str().unwrap().to_string();
    execution["cost_profile"] = serde_json::to_value(cost_profile(costs)).unwrap();
    execution["execution_sha256"] = json!(execution_digest(
        scorer, &cost_sha, &engine, &windows, &seeds
    ));
    execution["cost_model_sha256"] = json!(cost_sha);
}

fn delayed() -> CostModel {
    CostModel {
        fixed_delay: Some(FixedDecisionDelay::Fifo { bars: 2 }),
        ..CostModel::default()
    }
}

#[test]
fn a_replayed_board_signs_the_execution_its_captures_were_checked_against() {
    let fixture = Fixture::new(true);
    let header = &fixture.payloads[0];
    let execution = &header["execution"];
    assert_eq!(
        execution["cost_model_sha256"],
        cost_model_digest(CostModel::default()).as_str()
    );
    assert_eq!(
        execution["cost_profile"],
        serde_json::to_value(cost_profile(CostModel::default())).unwrap()
    );
    assert_eq!(execution, &fixture.window["execution"]);
    assert_eq!(
        fixture.window["schema_version"],
        EXECUTED_WINDOW_SCHEMA_VERSION
    );

    let report = verify_arena(&fixture.dir, None).unwrap();
    assert!(report.ok, "{report:?}");
    assert_eq!(report.windows[0].execution, ExecutionStatus::Bound);
}

/// The operator relabels the window record as if it had been scored with a
/// fixed delay. The signed header still names the immediate execution the
/// rows were replayed under, so the board does not stand for that window.
#[test]
fn a_window_relabelled_with_other_execution_settings_fails_verification() {
    let mut fixture = Fixture::new(true);
    let scorer = fixture.window["scorer_artifact_sha256"]
        .as_str()
        .unwrap()
        .to_string();
    relabel(&mut fixture.window["execution"], delayed(), &scorer);
    let base = fixture.dir.join(WINDOWS_DIR).join("w1");
    std::fs::write(
        base.join(WINDOW_FILE),
        serde_json::to_vec(&fixture.window).unwrap(),
    )
    .unwrap();

    let report = verify_arena(&fixture.dir, None).unwrap();
    assert!(!report.ok);
    let fields: Vec<IdentityField> = report.windows[0]
        .identity_mismatches
        .iter()
        .map(|m| m.field)
        .collect();
    assert_eq!(fields, [IdentityField::Execution], "{report:?}");
}

/// A header whose cost profile was changed without its digests, as an edit to
/// the readable mirror alone would leave it, fails on the identity itself even
/// when the window record says the same.
#[test]
fn an_execution_profile_that_does_not_recompute_fails_verification() {
    let mut fixture = Fixture::new(true);
    let delayed = serde_json::to_value(cost_profile(delayed())).unwrap();
    fixture.payloads[0]["execution"]["cost_profile"] = delayed.clone();
    fixture.window["execution"]["cost_profile"] = delayed;
    fixture.write();

    let report = verify_arena(&fixture.dir, None).unwrap();
    assert!(!report.ok);
    let window = &report.windows[0];
    assert!(window.identity_mismatches.is_empty(), "{report:?}");
    let error = window.execution_error.as_deref().unwrap();
    assert!(error.contains("digests to"), "{error}");
    // The arena refuses to load the same record.
    let refusal = Arena::load(&fixture.dir).err().unwrap();
    assert!(refusal.contains("digests to"), "{refusal}");
}

/// A matrix other than the one a forward window scores, digests recomputed,
/// is refused: the identity must name the windows and seeds the arena runs.
#[test]
fn an_execution_matrix_the_arena_does_not_score_fails_verification() {
    let mut fixture = Fixture::new(true);
    let scorer = fixture.window["scorer_artifact_sha256"]
        .as_str()
        .unwrap()
        .to_string();
    for execution in [
        &mut fixture.payloads[0]["execution"],
        &mut fixture.window["execution"],
    ] {
        execution["seeds"] = json!([0, 1, 2, 3, 4, 5, 6, 7, 8, 9]);
        relabel(execution, CostModel::default(), &scorer);
    }
    fixture.write();

    let report = verify_arena(&fixture.dir, None).unwrap();
    assert!(!report.ok);
    let error = report.windows[0].execution_error.as_deref().unwrap();
    assert!(error.contains("execution matrix"), "{error}");
}

/// Removing the identity from both documents, as a header signed before it
/// existed lacks it, leaves the board verifying under its legacy meaning, and
/// says so; it is not reported as bound.
#[test]
fn a_header_signed_before_the_identity_existed_verifies_as_legacy_unbound() {
    let mut fixture = Fixture::new(true);
    fixture.payloads[0]
        .as_object_mut()
        .unwrap()
        .remove("execution");
    fixture.window.as_object_mut().unwrap().remove("execution");
    fixture.window["schema_version"] = json!(SCORED_WINDOW_SCHEMA_VERSION);
    fixture.payloads[0]["schema_version"] = json!(SCORED_WINDOW_SCHEMA_VERSION);
    fixture.write();

    let report = verify_arena(&fixture.dir, None).unwrap();
    assert!(report.ok, "{report:?}");
    assert_eq!(report.windows[0].execution, ExecutionStatus::LegacyUnbound);
    Arena::load(&fixture.dir).unwrap();
}

/// A window that replayed nothing executed nothing: no identity is recorded
/// or signed, the record keeps its version, and none can be added.
#[test]
fn a_board_that_replayed_nothing_binds_no_execution_and_refuses_one() {
    let mut fixture = Fixture::new(false);
    assert!(fixture.payloads[0].get("execution").is_none());
    assert!(fixture.window.get("execution").is_none());
    assert_eq!(
        fixture.window["schema_version"],
        SCORED_WINDOW_SCHEMA_VERSION
    );
    let report = verify_arena(&fixture.dir, None).unwrap();
    assert!(report.ok, "{report:?}");
    assert_eq!(report.windows[0].execution, ExecutionStatus::NotReplayed);

    let borrowed = Fixture::new(true).payloads[0]["execution"].clone();
    fixture.payloads[0]["execution"] = borrowed.clone();
    fixture.window["execution"] = borrowed;
    fixture.write();
    let report = verify_arena(&fixture.dir, None).unwrap();
    assert!(!report.ok);
    let error = report.windows[0].execution_error.as_deref().unwrap();
    assert!(error.contains("replayed no capture"), "{error}");
}

/// A record carrying the identity is not one a binary that predates it loads,
/// so a rollback refuses it instead of dropping the identity on save.
#[test]
fn a_window_recording_execution_carries_a_version_older_binaries_refuse() {
    let fixture = Fixture::new(true);
    for known in [
        WINDOW_SCHEMA_VERSION,
        FAULTED_WINDOW_SCHEMA_VERSION,
        SCORED_WINDOW_SCHEMA_VERSION,
        SCORED_FAULTED_WINDOW_SCHEMA_VERSION,
    ] {
        assert_ne!(fixture.window["schema_version"], known);
    }
}
