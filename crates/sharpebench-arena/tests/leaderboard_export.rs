//! The leaderboard export over a real arena: rows only from boards that
//! verify, byte-identical output for the same signed boards, and refusal of
//! anything that does not verify.
//!
//! The arena built here is also the committed fixture the CLI test reads
//! (`crates/sharpebench-cli/tests/fixtures/leaderboard-arena`). Run
//! `SHARPEBENCH_WRITE_LEADERBOARD_FIXTURE=1 cargo test -p sharpebench-arena
//! --test leaderboard_export -- --ignored` to rewrite it and its expected
//! export after a deliberate change.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use sharpebench_arena::leaderboard_export::{export_arena, BOARD_NOT_CERTIFYING_REASON};
use sharpebench_arena::{
    reference_entrant_digest, Arena, DockerLauncher, ExpectedHead, IntakeOptions, ReplayWindow,
    RevealedEntry, SigningKey, BOARD_FILE, HEAD_FILE, STATE_FILE, WINDOWS_DIR, WINDOW_FILE,
};
use sharpebench_attest::{content_digest, make_commitment, PublicChain};
use sharpebench_core::headline::{Evidence, Verdict};
use sharpebench_core::leaderboard_export::{render_markdown, to_canonical_json, SourceKind};
use sharpebench_core::{AgentSubmission, Run, ScoreConfig};
use sharpebench_sim::{Agent, BuyAndHold, Dataset, Momentum};

static NEXT: AtomicU64 = AtomicU64::new(0);

struct TempDir(PathBuf);

impl TempDir {
    fn new(tag: &str) -> Self {
        let dir = std::env::temp_dir().join(format!(
            "sharpebench-leaderboard-export-{tag}-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn signing_key() -> SigningKey {
    SigningKey::derive(b"leaderboard-export-fixture-key")
}

fn dataset_csv() -> String {
    let data = Dataset::synthetic(4, 60, 7);
    let mut out = String::from("date,symbol,close\n");
    for (symbol, closes) in &data.closes {
        for (date, close) in data.dates.iter().zip(closes) {
            out.push_str(&format!("{date},{symbol},{close}\n"));
        }
    }
    out
}

/// A reference agent's strict capture over the window's execution matrix,
/// revealed under the name-bound reference digest.
fn reference_entry(agent_id: &str, name: &str, scorer: &str, salt: &str) -> RevealedEntry {
    let replay = ReplayWindow::parse(dataset_csv().as_bytes(), &ScoreConfig::default()).unwrap();
    let (_, mut capture) = sharpebench_harness::run_agent_capture(
        name,
        &replay.data,
        &replay.windows,
        &replay.seeds,
        replay.costs,
        || {
            if name == "momentum" {
                Box::new(Momentum::default()) as Box<dyn Agent>
            } else {
                Box::new(BuyAndHold)
            }
        },
    );
    capture.contract.as_mut().unwrap().runner_artifact_sha256 = Some(scorer.to_string());
    RevealedEntry {
        live_log: None,
        agent_id: Some(agent_id.to_string()),
        submission: None,
        capture: Some(capture),
        artifact_digest: reference_entrant_digest(name, scorer),
        salt: salt.to_string(),
        fault_plan_sha256: None,
    }
}

fn supplied_entry(agent_id: &str, returns: Vec<f64>, cost: f64, artifact: &str) -> RevealedEntry {
    RevealedEntry {
        live_log: None,
        agent_id: None,
        submission: Some(AgentSubmission {
            agent_id: agent_id.to_string(),
            runs: (0..3)
                .map(|_| Run {
                    returns: returns.clone(),
                    cost,
                    ..Run::default()
                })
                .collect(),
            in_sample_trials: 0,
            candidates: Vec::new(),
        }),
        capture: None,
        artifact_digest: artifact.to_string(),
        salt: format!("salt-{agent_id}"),
        fault_plan_sha256: None,
    }
}

/// Two published windows. `w1` re-executes two reference agents in process,
/// so its board certifies, and refuses a third entrant whose reveal does not
/// match its commitment. `w2` ranks supplied returns, so it does not certify:
/// one entrant reports a cost, the other is flat and has no Sharpe ratio.
fn build_fixture(dir: &Path) {
    let mut arena = Arena::init(dir).unwrap();
    for id in ["w1", "w2"] {
        arena
            .open_window(id, 10, 20, ScoreConfig::default())
            .unwrap();
    }
    let scorer = arena.window("w1").unwrap().scorer_artifact_sha256.clone();
    for (agent, name) in [("alpha", "momentum"), ("beta", "buy-and-hold")] {
        let digest = reference_entrant_digest(name, &scorer);
        arena
            .register_entry(
                "w1",
                make_commitment(agent, "w1", &digest, &format!("salt-{agent}")),
            )
            .unwrap();
    }
    arena
        .register_entry(
            "w1",
            make_commitment(
                "gamma",
                "w1",
                &reference_entrant_digest("momentum", &scorer),
                "salt-gamma",
            ),
        )
        .unwrap();
    let artifact = content_digest(b"leaderboard-export-supplied-entrant");
    for agent in ["delta", "flat"] {
        arena
            .register_entry(
                "w2",
                make_commitment(agent, "w2", &artifact, &format!("salt-{agent}")),
            )
            .unwrap();
    }
    arena.advance(20).unwrap();
    let dataset = dir.join("dataset.csv");
    std::fs::write(&dataset, dataset_csv()).unwrap();

    let w1 = vec![
        reference_entry("alpha", "momentum", &scorer, "salt-alpha"),
        reference_entry("beta", "buy-and-hold", &scorer, "salt-beta"),
        reference_entry("gamma", "momentum", &scorer, "not-the-committed-salt"),
    ];
    let mut launcher = DockerLauncher;
    arena
        .reveal_and_score_with(
            "w1",
            &dataset,
            &w1,
            IntakeOptions {
                allow_supplied_returns: false,
                reexecute: Some(&mut launcher),
            },
        )
        .unwrap();
    arena.publish("w1", &signing_key()).unwrap();

    let skilled: Vec<f64> = (0..60)
        .map(|i| 0.002 + 0.004 * (i as f64 * 0.7).sin())
        .collect();
    let w2 = vec![
        supplied_entry("delta", skilled, 0.25, &artifact),
        supplied_entry("flat", vec![0.001; 60], 0.0, &artifact),
    ];
    arena
        .reveal_and_score_with(
            "w2",
            &dataset,
            &w2,
            IntakeOptions {
                allow_supplied_returns: true,
                reexecute: None,
            },
        )
        .unwrap();
    arena.publish("w2", &signing_key()).unwrap();
    std::fs::remove_file(&dataset).unwrap();
}

fn board_header_signature(dir: &Path, window: &str) -> String {
    let board: PublicChain = serde_json::from_slice(
        &std::fs::read(dir.join(WINDOWS_DIR).join(window).join(BOARD_FILE)).unwrap(),
    )
    .unwrap();
    board.chain[0].signature.clone()
}

fn export_bytes(dir: &Path) -> (String, String) {
    let export = export_arena(dir, Some(&signing_key().verifying_key()), None).unwrap();
    (to_canonical_json(&export), render_markdown(&export))
}

#[test]
fn the_export_reads_every_signed_row_and_refusal_in_chain_order() {
    let dir = TempDir::new("rows");
    build_fixture(&dir.0);
    let export = export_arena(&dir.0, Some(&signing_key().verifying_key()), None).unwrap();
    assert_eq!(export.source.kind, SourceKind::Arena);
    assert!(export.source.key_pinned);
    assert_eq!(export.source.completeness, "anchored");
    assert!(export.source.head.as_deref().unwrap().starts_with("2:"));
    let ids: Vec<&str> = export.boards.iter().map(|b| b.board_id.as_str()).collect();
    assert_eq!(ids, ["w1", "w2"]);
    assert_eq!(export.boards[0].certifying, Some(true));
    assert_eq!((export.boards[0].rows, export.boards[0].refusals), (2, 1));
    assert_eq!(export.boards[1].certifying, Some(false));
    assert!(export.boards[1].supplied_returns_accepted);

    let rows: Vec<(&str, &str, Verdict)> = export
        .rows
        .iter()
        .map(|r| {
            (
                r.board_id.as_str(),
                r.headline.agent_id.as_str(),
                r.headline.verdict,
            )
        })
        .collect();
    assert_eq!(rows.len(), 5, "{rows:?}");
    assert_eq!(rows[2], ("w1", "gamma", Verdict::Refused));
    // A refusal measured nothing, so it carries no evidence mode even on a
    // certifying board.
    assert_eq!(
        export.rows[2].headline.qualifiers.evidence,
        Evidence::Unknown
    );
    assert_eq!(export.rows[2].record.link, 0);
    assert_eq!(
        export.rows[2].record.link_signature,
        board_header_signature(&dir.0, "w1")
    );
    assert_eq!(rows[4].1, "flat");
    assert_eq!(rows[4].2, Verdict::DegenerateTrack);
    assert_eq!(export.rows[4].dsr, None);
    assert!(matches!(
        &export.rows[0].headline.qualifiers.evidence,
        Evidence::Certifying { mode } if mode == "forward"
    ));
    assert_eq!(
        export.rows[0].returns_provenance.as_deref(),
        Some("re-executed")
    );
    assert_eq!(
        export.rows[3].returns_provenance.as_deref(),
        Some("supplied")
    );
    assert_eq!(export.rows[2].returns_provenance, None);
    assert!(matches!(
        &export.rows[3].headline.qualifiers.evidence,
        Evidence::Noncertifying { mode, reasons }
            if mode == "forward" && reasons.first().map(String::as_str) == Some(BOARD_NOT_CERTIFYING_REASON)
    ));
    // Each scored row names the signed link it came from, and that link is
    // the one the board carries at that position.
    let board: PublicChain = serde_json::from_slice(
        &std::fs::read(dir.0.join(WINDOWS_DIR).join("w1").join(BOARD_FILE)).unwrap(),
    )
    .unwrap();
    for row in &export.rows[..2] {
        let link = row.record.link;
        assert_eq!(row.record.link_signature, board.chain[link].signature);
        assert!(board.chain[link]
            .payload
            .contains(&format!("\"agent_id\":\"{}\"", row.headline.agent_id)));
    }
    let delta = &export.rows[3];
    assert_eq!(delta.cost.as_ref().unwrap().total, Some(0.75));
}

#[test]
fn the_same_signed_boards_export_to_the_same_bytes() {
    let a = TempDir::new("det-a");
    let b = TempDir::new("det-b");
    build_fixture(&a.0);
    build_fixture(&b.0);
    let first = export_bytes(&a.0);
    assert_eq!(
        first,
        export_bytes(&a.0),
        "a second run reads the same bytes"
    );
    assert_eq!(
        first,
        export_bytes(&b.0),
        "another directory reads the same bytes"
    );
}

#[test]
fn an_arena_that_does_not_verify_exports_nothing() {
    let dir = TempDir::new("tamper");
    build_fixture(&dir.0);
    // A wrong pinned key.
    let wrong = SigningKey::derive(b"impostor").verifying_key();
    let error = export_arena(&dir.0, Some(&wrong), None).unwrap_err();
    assert!(error.starts_with("the arena does not verify"), "{error}");
    // A head the history does not pass through.
    let expected = ExpectedHead::parse(&format!("1:{}", "0".repeat(64))).unwrap();
    let error = export_arena(&dir.0, None, Some(&expected)).unwrap_err();
    assert!(error.contains("completeness broken"), "{error}");
    // An edited score link.
    let path = dir.0.join(WINDOWS_DIR).join("w2").join(BOARD_FILE);
    let original = std::fs::read_to_string(&path).unwrap();
    let mut board: PublicChain = serde_json::from_str(&original).unwrap();
    board.chain[1].payload = board.chain[1]
        .payload
        .replace("\"agent_id\":\"delta\"", "\"agent_id\":\"delt4\"");
    assert_ne!(serde_json::to_string_pretty(&board).unwrap(), original);
    std::fs::write(&path, serde_json::to_string_pretty(&board).unwrap()).unwrap();
    let error = export_arena(&dir.0, None, None).unwrap_err();
    assert!(error.contains("w2"), "{error}");
    std::fs::write(&path, original).unwrap();
    assert!(export_arena(&dir.0, None, None).is_ok());
}

#[test]
fn an_arena_with_nothing_published_exports_no_rows() {
    let dir = TempDir::new("empty");
    Arena::init(&dir.0).unwrap();
    let export = export_arena(&dir.0, None, None).unwrap();
    assert!(export.boards.is_empty() && export.rows.is_empty());
    assert_eq!(export.source.completeness, "no_published_windows");
    assert_eq!(export.source.head, None);
}

/// Rewrites the committed CLI fixture. Ignored: it writes into the source
/// tree, and only when the variable is set.
#[test]
#[ignore = "rewrites the committed fixture; set SHARPEBENCH_WRITE_LEADERBOARD_FIXTURE=1"]
fn write_the_cli_fixture() {
    if std::env::var_os("SHARPEBENCH_WRITE_LEADERBOARD_FIXTURE").is_none() {
        return;
    }
    let target = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../sharpebench-cli/tests/fixtures/leaderboard-arena");
    let dir = TempDir::new("fixture");
    build_fixture(&dir.0);
    let _ = std::fs::remove_dir_all(&target);
    for file in [STATE_FILE, HEAD_FILE] {
        copy(&dir.0.join(file), &target.join("arena").join(file));
    }
    for window in ["w1", "w2"] {
        for file in [WINDOW_FILE, BOARD_FILE] {
            let rel = Path::new(WINDOWS_DIR).join(window).join(file);
            copy(&dir.0.join(&rel), &target.join("arena").join(&rel));
        }
    }
    let (json, markdown) = export_bytes(&dir.0);
    std::fs::write(target.join("leaderboard.json"), json).unwrap();
    std::fs::write(target.join("leaderboard.md"), markdown).unwrap();
}

fn copy(from: &Path, to: &Path) {
    std::fs::create_dir_all(to.parent().unwrap()).unwrap();
    std::fs::copy(from, to).unwrap();
}
