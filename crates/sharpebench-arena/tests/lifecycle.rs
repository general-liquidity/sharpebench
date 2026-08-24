//! Full forward-league lifecycle over a temp directory: deterministic given
//! explicit epochs, refusing what the primitives refuse, and publishing boards
//! that verify from the documents alone.

use std::path::{Path, PathBuf};

use sharpebench_arena::{
    verify_arena, Arena, RevealedEntry, WindowStatus, BOARD_FILE, WINDOWS_DIR,
};
use sharpebench_attest::{
    content_digest, make_commitment, publish_public_chain, PublicChain, SigningKey,
};
use sharpebench_core::{AgentSubmission, Run, ScoreConfig};
use sharpebench_sim::Dataset;

fn temp_arena_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "sharpebench-arena-test-{tag}-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    dir
}

fn signing_key() -> SigningKey {
    SigningKey::derive(b"arena-test-host-secret")
}

/// Serialize the synthetic dataset to deterministic bytes (BTreeMap iteration
/// is ordered) so the same seed yields the same dataset hash on every run.
fn dataset_bytes(data: &Dataset) -> Vec<u8> {
    let mut out = String::new();
    for (sym, closes) in &data.closes {
        for (t, c) in closes.iter().enumerate() {
            out.push_str(&format!("{sym},{t},{c}\n"));
        }
    }
    out.into_bytes()
}

fn write_dataset(dir: &Path, seed: u64) -> PathBuf {
    let data = Dataset::synthetic(4, 60, seed);
    let path = dir.join("dataset.csv");
    std::fs::write(&path, dataset_bytes(&data)).unwrap();
    path
}

/// A submission whose returns come from the synthetic dataset's own closes
/// (symbol `sym_idx`), so the scored field is derived from real dataset bytes.
fn submission_from_dataset(agent_id: &str, sym_idx: usize, seed: u64) -> AgentSubmission {
    let data = Dataset::synthetic(4, 60, seed);
    let symbols = data.symbols();
    let closes = &data.closes[&symbols[sym_idx % symbols.len()]];
    let returns: Vec<f64> = closes.windows(2).map(|w| w[1] / w[0] - 1.0).collect();
    AgentSubmission {
        agent_id: agent_id.to_string(),
        runs: vec![Run {
            returns,
            ..Run::default()
        }],
        in_sample_trials: 0,
        candidates: Vec::new(),
    }
}

fn entry(agent_id: &str, sym_idx: usize, seed: u64, digest: &str, salt: &str) -> RevealedEntry {
    RevealedEntry {
        submission: submission_from_dataset(agent_id, sym_idx, seed),
        artifact_digest: digest.to_string(),
        salt: salt.to_string(),
    }
}

/// Drive one window through the whole lifecycle; returns the published board path.
fn run_window(arena: &mut Arena, dir: &Path, id: &str, base_epoch: u64, seed: u64) -> PathBuf {
    let deadline = base_epoch + 10;
    let reveal = base_epoch + 20;
    arena
        .open_window(id, deadline, reveal, ScoreConfig::default())
        .unwrap();
    let digest = content_digest(format!("artifact-{id}").as_bytes());
    for agent in ["alpha", "beta"] {
        arena
            .register_entry(
                id,
                make_commitment(agent, id, &digest, &format!("salt-{agent}")),
            )
            .unwrap();
    }
    arena.advance(reveal).unwrap();
    let dataset = write_dataset(dir, seed);
    let entries = vec![
        entry("alpha", 0, seed, &digest, "salt-alpha"),
        entry("beta", 1, seed, &digest, "salt-beta"),
    ];
    arena.reveal_and_score(id, &dataset, &entries).unwrap();
    arena.publish(id, &signing_key()).unwrap()
}

#[test]
fn full_lifecycle_is_deterministic_and_verifies() {
    let dir = temp_arena_dir("lifecycle");
    let mut arena = Arena::init(&dir).unwrap();

    arena
        .open_window("2026-W01", 10, 20, ScoreConfig::default())
        .unwrap();
    assert_eq!(arena.window("2026-W01").unwrap().status, WindowStatus::Open);

    let digest = content_digest(b"frozen-alpha-artifact");
    arena
        .register_entry(
            "2026-W01",
            make_commitment("alpha", "2026-W01", &digest, "salt-a"),
        )
        .unwrap();
    arena
        .register_entry(
            "2026-W01",
            make_commitment("beta", "2026-W01", &digest, "salt-b"),
        )
        .unwrap();

    // Duplicate registration is refused with the registry's own semantics.
    let err = arena
        .register_entry(
            "2026-W01",
            make_commitment("alpha", "2026-W01", &digest, "another"),
        )
        .unwrap_err();
    assert!(err.contains("already registered"), "{err}");

    // Scoring before the reveal epoch is refused.
    arena.advance(10).unwrap();
    assert_eq!(
        arena.window("2026-W01").unwrap().status,
        WindowStatus::Committed
    );
    let dataset = write_dataset(&dir, 42);
    let entries = vec![
        entry("alpha", 0, 42, &digest, "salt-a"),
        entry("beta", 1, 42, &digest, "salt-b"),
    ];
    let err = arena
        .reveal_and_score("2026-W01", &dataset, &entries)
        .unwrap_err();
    assert!(err.contains("locked"), "{err}");

    arena.advance(20).unwrap();
    let scores = arena
        .reveal_and_score("2026-W01", &dataset, &entries)
        .unwrap();
    assert_eq!(scores.len(), 2);
    let w = arena.window("2026-W01").unwrap();
    assert_eq!(w.status, WindowStatus::Scoring);
    assert_eq!(
        w.dataset_hash.as_deref().unwrap(),
        content_digest(&std::fs::read(&dataset).unwrap()),
        "the window record binds the exact revealed dataset bytes"
    );

    let board_path = arena.publish("2026-W01", &signing_key()).unwrap();
    assert!(board_path.ends_with(Path::new("2026-W01").join(BOARD_FILE)));
    assert_eq!(
        arena.window("2026-W01").unwrap().status,
        WindowStatus::Published
    );
    // board.md exists beside board.json.
    assert!(board_path.with_file_name("board.md").exists());

    // Determinism: a second arena driven with the same epochs, inputs, and key
    // produces byte-identical board.json.
    let dir2 = temp_arena_dir("lifecycle-repeat");
    let mut arena2 = Arena::init(&dir2).unwrap();
    arena2
        .open_window("2026-W01", 10, 20, ScoreConfig::default())
        .unwrap();
    arena2
        .register_entry(
            "2026-W01",
            make_commitment("alpha", "2026-W01", &digest, "salt-a"),
        )
        .unwrap();
    arena2
        .register_entry(
            "2026-W01",
            make_commitment("beta", "2026-W01", &digest, "salt-b"),
        )
        .unwrap();
    arena2.advance(20).unwrap();
    let dataset2 = write_dataset(&dir2, 42);
    arena2
        .reveal_and_score("2026-W01", &dataset2, &entries)
        .unwrap();
    let board_path2 = arena2.publish("2026-W01", &signing_key()).unwrap();
    assert_eq!(
        std::fs::read(&board_path).unwrap(),
        std::fs::read(&board_path2).unwrap(),
        "same epochs + inputs + key must publish byte-identical boards"
    );

    let _ = std::fs::remove_dir_all(&dir);
    let _ = std::fs::remove_dir_all(&dir2);
}

#[test]
fn late_commitment_is_refused() {
    let dir = temp_arena_dir("late-commit");
    let mut arena = Arena::init(&dir).unwrap();
    arena
        .open_window("w", 5, 10, ScoreConfig::default())
        .unwrap();
    arena.advance(5).unwrap();
    let err = arena
        .register_entry("w", make_commitment("late", "w", "digest", "salt"))
        .unwrap_err();
    // The status flip at the deadline refuses it before the registry even runs;
    // the registry backstops the same rule when advance was never called.
    assert!(err.contains("not open"), "{err}");

    // Without an advance call flipping the status, the wrapped registry itself
    // refuses: epoch >= unlock is "too late to commit".
    let dir2 = temp_arena_dir("late-commit-registry");
    let mut arena2 = Arena::init(&dir2).unwrap();
    arena2
        .open_window("w", 5, 10, ScoreConfig::default())
        .unwrap();
    // Reload with a state file whose epoch equals the deadline but status Open.
    // Simplest honest route: advance to 4 (still open), then hand-advance the
    // epoch by editing state.json as a crashed scheduler might leave it.
    arena2.advance(4).unwrap();
    let state_path = dir2.join("state.json");
    let state = std::fs::read_to_string(&state_path).unwrap();
    std::fs::write(
        &state_path,
        state.replace("\"current_epoch\": 4", "\"current_epoch\": 5"),
    )
    .unwrap();
    let mut arena2 = Arena::load(&dir2).unwrap();
    let err = arena2
        .register_entry("w", make_commitment("late", "w", "digest", "salt"))
        .unwrap_err();
    assert!(err.contains("too late to commit"), "{err}");

    let _ = std::fs::remove_dir_all(&dir);
    let _ = std::fs::remove_dir_all(&dir2);
}

#[test]
fn tampered_reveal_is_refused_and_recorded_while_the_rest_score() {
    let dir = temp_arena_dir("tampered-reveal");
    let mut arena = Arena::init(&dir).unwrap();
    arena
        .open_window("w", 10, 20, ScoreConfig::default())
        .unwrap();
    let digest = content_digest(b"honest-artifact");
    arena
        .register_entry("w", make_commitment("honest", "w", &digest, "salt-h"))
        .unwrap();
    arena
        .register_entry("w", make_commitment("tamperer", "w", &digest, "salt-t"))
        .unwrap();
    arena.advance(20).unwrap();
    let dataset = write_dataset(&dir, 7);
    let entries = vec![
        entry("honest", 0, 7, &digest, "salt-h"),
        // The tamperer reveals a different artifact than it committed to.
        entry(
            "tamperer",
            1,
            7,
            &content_digest(b"swapped-after-the-fact"),
            "salt-t",
        ),
        // And someone who never committed at all shows up.
        entry("gatecrasher", 2, 7, &digest, "salt-g"),
    ];
    let scores = arena.reveal_and_score("w", &dataset, &entries).unwrap();
    assert_eq!(scores.len(), 1);
    assert_eq!(scores[0].agent_id, "honest");

    let w = arena.window("w").unwrap();
    assert_eq!(w.refusals.len(), 2);
    let tamperer = w
        .refusals
        .iter()
        .find(|r| r.agent_id == "tamperer")
        .unwrap();
    assert!(
        tamperer.reason.contains("does not match"),
        "{}",
        tamperer.reason
    );
    let crasher = w
        .refusals
        .iter()
        .find(|r| r.agent_id == "gatecrasher")
        .unwrap();
    assert!(
        crasher.reason.contains("no such commitment"),
        "{}",
        crasher.reason
    );

    // The refusals are part of the signed board header, permanently.
    let board_path = arena.publish("w", &signing_key()).unwrap();
    let board: PublicChain =
        serde_json::from_str(&std::fs::read_to_string(&board_path).unwrap()).unwrap();
    assert!(board.chain[0].payload.contains("tamperer"));
    assert!(board.chain[0].payload.contains("gatecrasher"));

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn published_boards_verify_from_the_documents_alone_and_chain_across_windows() {
    let dir = temp_arena_dir("chain");
    let mut arena = Arena::init(&dir).unwrap();
    run_window(&mut arena, &dir, "w1", 0, 1);
    run_window(&mut arena, &dir, "w2", 20, 2);
    run_window(&mut arena, &dir, "w3", 40, 3);
    drop(arena); // only the documents remain

    // Self-verification: embedded keys only.
    let report = verify_arena(&dir, None).unwrap();
    assert!(report.ok, "{report:?}");
    assert_eq!(report.windows.len(), 3);
    assert!(report
        .windows
        .iter()
        .all(|w| w.chain_ok && w.anchor_ok && w.key_ok));

    // Pinned verification: the host's public key, nothing secret.
    let vk = signing_key().verifying_key();
    assert!(verify_arena(&dir, Some(&vk)).unwrap().ok);

    // A wrong pinned key rejects everything even though the documents self-verify.
    let wrong = SigningKey::derive(b"impostor").verifying_key();
    let report = verify_arena(&dir, Some(&wrong)).unwrap();
    assert!(!report.ok);

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn altering_a_published_board_breaks_the_cross_window_chain() {
    let dir = temp_arena_dir("tamper-board");
    let mut arena = Arena::init(&dir).unwrap();
    let board1 = run_window(&mut arena, &dir, "w1", 0, 1);
    run_window(&mut arena, &dir, "w2", 20, 2);
    drop(arena);
    assert!(verify_arena(&dir, None).unwrap().ok);

    // Tamper mode 1: edit a payload in window 1's board without re-signing.
    let original = std::fs::read_to_string(&board1).unwrap();
    let mut board: PublicChain = serde_json::from_str(&original).unwrap();
    let last = board.chain.len() - 1;
    board.chain[last].payload = board.chain[last]
        .payload
        .replace("\"deflated_sharpe\":", "\"deflated_sharpe_forged\":");
    std::fs::write(&board1, serde_json::to_string_pretty(&board).unwrap()).unwrap();
    let report = verify_arena(&dir, None).unwrap();
    assert!(!report.ok);
    assert!(!report.windows[0].chain_ok, "{report:?}");

    // Tamper mode 2: replace window 1's board with a fresh, self-consistent
    // board signed by the same key. It self-verifies, but its final signature
    // changes, so window 2's recorded anchor no longer matches: the history is
    // one chain, not per-window islands.
    let forged = publish_public_chain(
        &[serde_json::from_str::<PublicChain>(&original)
            .unwrap()
            .chain[0]
            .payload
            .clone()],
        &signing_key(),
    );
    std::fs::write(&board1, serde_json::to_string_pretty(&forged).unwrap()).unwrap();
    let report = verify_arena(&dir, None).unwrap();
    assert!(!report.ok);
    assert!(report.windows[0].chain_ok, "the forged board self-verifies");
    assert!(
        !report.windows[1].anchor_ok,
        "but the next window's anchor exposes it: {report:?}"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn state_survives_reload_at_every_stage() {
    let dir = temp_arena_dir("reload");
    {
        let mut arena = Arena::init(&dir).unwrap();
        arena
            .open_window("w", 10, 20, ScoreConfig::default())
            .unwrap();
    }
    let digest = content_digest(b"artifact");
    {
        let mut arena = Arena::load(&dir).unwrap();
        assert_eq!(arena.window("w").unwrap().status, WindowStatus::Open);
        arena
            .register_entry("w", make_commitment("alpha", "w", &digest, "s"))
            .unwrap();
    }
    {
        let mut arena = Arena::load(&dir).unwrap();
        assert_eq!(arena.window("w").unwrap().commitments.len(), 1);
        arena.advance(20).unwrap();
    }
    let dataset = write_dataset(&dir, 9);
    {
        let mut arena = Arena::load(&dir).unwrap();
        assert_eq!(arena.current_epoch(), 20);
        assert_eq!(arena.window("w").unwrap().status, WindowStatus::Committed);
        arena
            .reveal_and_score("w", &dataset, &[entry("alpha", 0, 9, &digest, "s")])
            .unwrap();
    }
    {
        let mut arena = Arena::load(&dir).unwrap();
        assert_eq!(arena.window("w").unwrap().status, WindowStatus::Scoring);
        assert_eq!(arena.window("w").unwrap().scores.len(), 1);
        arena.publish("w", &signing_key()).unwrap();
    }
    {
        let arena = Arena::load(&dir).unwrap();
        assert_eq!(arena.window("w").unwrap().status, WindowStatus::Published);
        assert_eq!(arena.published_ids(), ["w".to_string()]);
    }
    assert!(verify_arena(&dir, None).unwrap().ok);

    // Epochs never move backwards, even across a reload.
    let mut arena = Arena::load(&dir).unwrap();
    assert!(arena.advance(19).unwrap_err().contains("backwards"));

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn sealed_eval_salt_commitment_is_persisted_and_shape_checked() {
    let dir = temp_arena_dir("sealed-eval-commitment");
    let mut arena = Arena::init(&dir).unwrap();
    let hash = "ab".repeat(32);
    arena
        .open_window_with_sealed_eval_commitment(
            "sealed",
            10,
            20,
            ScoreConfig::default(),
            Some(hash.clone()),
        )
        .unwrap();
    assert_eq!(
        arena
            .window("sealed")
            .unwrap()
            .sealed_eval_salt_sha256
            .as_deref(),
        Some(hash.as_str())
    );
    drop(arena);
    assert_eq!(
        Arena::load(&dir)
            .unwrap()
            .window("sealed")
            .unwrap()
            .sealed_eval_salt_sha256
            .as_deref(),
        Some(hash.as_str())
    );

    let mut invalid = Arena::init(&temp_arena_dir("sealed-eval-invalid")).unwrap();
    let err = invalid
        .open_window_with_sealed_eval_commitment(
            "bad",
            10,
            20,
            ScoreConfig::default(),
            Some("not-a-sha256".to_string()),
        )
        .unwrap_err();
    assert!(err.contains("64-character lowercase"), "{err}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn opened_window_rejects_a_config_completed_by_later_serde_defaults() {
    let dir = temp_arena_dir("frozen-score-config-digest");
    let mut arena = Arena::init(&dir).unwrap();
    arena
        .open_window("w", 10, 20, ScoreConfig::default())
        .unwrap();
    drop(arena);

    let path = dir.join(WINDOWS_DIR).join("w").join("window.json");
    let mut value: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    value["score_config"]
        .as_object_mut()
        .unwrap()
        .remove("min_measured_trials_sr_std");
    std::fs::write(&path, serde_json::to_string_pretty(&value).unwrap()).unwrap();

    let err = match Arena::load(&dir) {
        Ok(_) => panic!("tampered config unexpectedly loaded"),
        Err(err) => err,
    };
    assert!(err.contains("score_config is not explicit"), "{err}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn arena_layout_is_state_json_plus_windows_dir() {
    let dir = temp_arena_dir("layout");
    let mut arena = Arena::init(&dir).unwrap();
    run_window(&mut arena, &dir, "w1", 0, 1);
    assert!(dir.join("state.json").is_file());
    assert!(dir
        .join(WINDOWS_DIR)
        .join("w1")
        .join("window.json")
        .is_file());
    assert!(dir.join(WINDOWS_DIR).join("w1").join(BOARD_FILE).is_file());
    assert!(dir.join(WINDOWS_DIR).join("w1").join("board.md").is_file());
    let _ = std::fs::remove_dir_all(&dir);
}
