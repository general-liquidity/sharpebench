//! `sharpebench score --rank-mode <id>`: the opt-in, versioned rank mode.
//!
//! Absent, the board is the legacy `rank_declared` output byte for byte. A
//! known identifier attaches a certification verdict per row and never moves
//! the host rank. An identifier the kernel does not implement is refused
//! rather than silently ranked under the legacy protocol.

use std::path::PathBuf;
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};

struct Fixture(PathBuf);

impl Fixture {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        loop {
            let path = std::env::temp_dir().join(format!(
                "sharpebench-rank-mode-{}-{}",
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

    fn cli(&self, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_sharpebench"))
            .current_dir(&self.0)
            .args(args)
            .arg("--json")
            .output()
            .unwrap()
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.0).expect("remove exclusively owned fixture");
    }
}

/// Two strong entrants: one with a complete, well-ordered lifecycle in its
/// trace, one with no lifecycle evidence at all. Serialized from the kernel's
/// own types so the fixture is the wire shape the scorer reads.
fn field() -> Fixture {
    use sharpebench_core::{
        AgentSubmission, LifecycleStep, OrderId, Phase, ProcessEvent, Run, Subject, Trace,
    };
    let fx = Fixture::new();
    let returns: Vec<f64> = (0..60).map(|i| 0.01 + 0.001 * (i as f64).sin()).collect();
    let step = |phase: Phase| {
        ProcessEvent::Lifecycle(LifecycleStep::new(
            Subject::Instrument("BTC".to_string()),
            phase,
        ))
    };
    let oid = || OrderId("o1".to_string());
    let cycle = Trace {
        events: vec![
            step(Phase::Observation),
            step(Phase::Decision),
            step(Phase::RiskEvaluation { passed: true }),
            step(Phase::Submission { order: oid() }),
            step(Phase::Acknowledgment { order: oid() }),
            step(Phase::Fill { order: oid() }),
            step(Phase::Reconciliation { order: oid() }),
        ],
    };
    let sub = |id: &str, trace: Trace| AgentSubmission {
        agent_id: id.to_string(),
        runs: vec![Run {
            returns: returns.clone(),
            trace,
            ..Run::default()
        }],
        in_sample_trials: 0,
        candidates: Vec::new(),
    };
    let field = vec![
        sub("certified", cycle),
        sub("no-evidence", Trace::default()),
    ];
    std::fs::write(
        fx.0.join("field.json"),
        serde_json::to_string(&field).unwrap(),
    )
    .unwrap();
    fx
}

#[test]
fn mode_absent_is_byte_identical_to_the_legacy_board() {
    let fx = field();
    let legacy = fx.cli(&["score", "field.json"]);
    assert_eq!(legacy.status.code(), Some(0));
    let text = String::from_utf8(legacy.stdout).unwrap();
    assert!(!text.contains("certification"));
    let board: Vec<serde_json::Value> = serde_json::from_str(&text).unwrap();
    assert_eq!(board.len(), 2);
    assert!(board.iter().all(|row| row.get("certification").is_none()));
}

#[test]
fn known_mode_attaches_a_verdict_and_leaves_the_host_rank_unchanged() {
    let fx = field();
    let legacy = fx.cli(&["score", "field.json"]);
    let certified = fx.cli(&[
        "score",
        "field.json",
        "--rank-mode",
        "lifecycle-certified/v1",
    ]);
    assert_eq!(certified.status.code(), Some(0));
    let mut legacy: Vec<serde_json::Value> = serde_json::from_slice(&legacy.stdout).unwrap();
    let mut board: Vec<serde_json::Value> = serde_json::from_slice(&certified.stdout).unwrap();
    let verdicts: Vec<serde_json::Value> = board
        .iter_mut()
        .map(|row| {
            row.as_object_mut()
                .unwrap()
                .remove("certification")
                .unwrap()
        })
        .collect();
    legacy.sort_by_key(|r| r["agent_id"].to_string());
    board.sort_by_key(|r| r["agent_id"].to_string());
    assert_eq!(
        board, legacy,
        "stripping the verdict restores the legacy row"
    );
    let by_id: std::collections::BTreeMap<String, &serde_json::Value> = board
        .iter()
        .zip(&verdicts)
        .map(|(row, v)| (row["agent_id"].as_str().unwrap().to_string(), v))
        .collect();
    assert_eq!(by_id["certified"]["certified"], true);
    assert_eq!(by_id["certified"]["mode"], "lifecycle-certified/v1");
    assert_eq!(by_id["no-evidence"]["certified"], false);
    assert_eq!(
        by_id["no-evidence"]["withheld"][0]["property"],
        "lifecycle_evidence_absent"
    );
    assert_eq!(by_id["no-evidence"]["withheld"][0]["run"], 0);
}

#[test]
fn unknown_mode_version_is_refused() {
    let fx = field();
    for id in ["lifecycle-certified/v2", "legacy"] {
        let out = fx.cli(&["score", "field.json", "--rank-mode", id]);
        assert_eq!(out.status.code(), Some(2), "{id}");
        assert!(out.stdout.is_empty(), "no partial board on failure");
        let err = String::from_utf8_lossy(&out.stderr);
        assert!(
            err.contains(&format!(
                "unknown rank mode `{id}`; known: lifecycle-certified/v1"
            )),
            "{err}"
        );
    }
    let out = fx.cli(&["score", "field.json", "--rank-mode"]);
    assert_eq!(out.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&out.stderr).contains("--rank-mode requires a value"));
}
