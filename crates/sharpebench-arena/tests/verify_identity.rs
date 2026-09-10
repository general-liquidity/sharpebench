//! `verify_arena` cross-checks each published board's signed header against
//! the window file it publishes. The window file is not signed, so without the
//! check a window edited after publication (another config digest, another
//! fault plan, or a plan added or dropped) would still verify beside a board
//! signed over the original identity.

use std::path::{Path, PathBuf};

use sharpebench_arena::{
    verify_arena, Arena, IdentityField, IdentityMismatch, RevealedEntry, SigningKey, WINDOWS_DIR,
    WINDOW_FILE,
};
use sharpebench_attest::{content_digest, make_commitment_under_fault_plan};
use sharpebench_core::{AgentSubmission, Run, ScoreConfig};

fn temp_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "sharpebench-arena-verify-identity-{tag}-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn plan_digest(tag: &str) -> String {
    content_digest(format!("fault-plan-{tag}").as_bytes())
}

fn window_path(dir: &Path, id: &str) -> PathBuf {
    dir.join(WINDOWS_DIR).join(id).join(WINDOW_FILE)
}

/// Publish `w1` (under `plan` when given) and then an unfaulted `w2` after it.
fn published(dir: &Path, plan: Option<String>) {
    let mut arena = Arena::init(dir).unwrap();
    let dataset = dir.join("dataset.csv");
    std::fs::write(&dataset, b"sym,close\nA,1.0\nA,1.01\n").unwrap();
    for (id, window_plan) in [("w1", plan), ("w2", None)] {
        let deadline = arena.current_epoch() + 10;
        arena
            .open_window_with_fault_plan(
                id,
                deadline,
                deadline + 10,
                ScoreConfig::default(),
                Some(content_digest(b"sealed-salt")),
                content_digest(b"verify-identity-scorer"),
                window_plan.clone(),
            )
            .unwrap();
        let artifact = content_digest(b"verify-identity-entrant");
        arena
            .register_entry(
                id,
                make_commitment_under_fault_plan(
                    "alpha",
                    id,
                    &artifact,
                    "salt",
                    window_plan.as_deref(),
                ),
            )
            .unwrap();
        arena.advance(deadline + 10).unwrap();
        let entry = RevealedEntry {
            submission: AgentSubmission {
                agent_id: "alpha".to_string(),
                runs: vec![Run {
                    returns: (0..40).map(|i| 0.001 * (i as f64 + 1.0).sin()).collect(),
                    ..Run::default()
                }],
                in_sample_trials: 0,
                candidates: Vec::new(),
            },
            artifact_digest: artifact,
            salt: "salt".to_string(),
            fault_plan_sha256: window_plan,
        };
        assert_eq!(
            arena
                .reveal_and_score(id, &dataset, &[entry])
                .unwrap()
                .len(),
            1
        );
        arena
            .publish(id, &SigningKey::derive(b"verify-identity-key"))
            .unwrap();
    }
}

fn edit_window(dir: &Path, edit: impl FnOnce(&mut serde_json::Map<String, serde_json::Value>)) {
    let path = window_path(dir, "w1");
    let mut value: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    edit(value.as_object_mut().unwrap());
    std::fs::write(&path, serde_json::to_string_pretty(&value).unwrap()).unwrap();
}

fn mismatch(field: IdentityField, header: Option<&str>, window: Option<&str>) -> IdentityMismatch {
    IdentityMismatch {
        field,
        header: header.map(str::to_string),
        window: window.map(str::to_string),
    }
}

#[test]
fn a_clean_arena_reports_no_identity_field() {
    for (tag, plan) in [("plain", None), ("faulted", Some(plan_digest("a")))] {
        let dir = temp_dir(&format!("clean-{tag}"));
        published(&dir, plan);
        let report = verify_arena(&dir, None).unwrap();
        assert!(report.ok, "{tag}: {report:?}");
        assert!(report
            .windows
            .iter()
            .all(|w| w.identity_mismatches.is_empty() && w.detail == "ok"));
        let json = serde_json::to_string(&report).unwrap();
        assert!(!json.contains("identity_mismatches"), "{tag}: {json}");
        let _ = std::fs::remove_dir_all(&dir);
    }
}

#[test]
fn a_window_whose_config_digest_differs_from_its_header_fails() {
    let dir = temp_dir("config-digest");
    published(&dir, None);
    let window: serde_json::Value =
        serde_json::from_slice(&std::fs::read(window_path(&dir, "w1")).unwrap()).unwrap();
    let recorded = window["score_config_sha256"].as_str().unwrap().to_string();
    let other = content_digest(b"another config");
    edit_window(&dir, |w| {
        w["score_config_sha256"] = serde_json::json!(other);
    });
    let report = verify_arena(&dir, None).unwrap();
    assert!(!report.ok);
    let w1 = &report.windows[0];
    assert!(w1.chain_ok && w1.anchor_ok && w1.key_ok, "{w1:?}");
    assert_eq!(
        w1.identity_mismatches,
        [mismatch(
            IdentityField::ScoreConfigSha256,
            Some(&recorded),
            Some(&other)
        )]
    );
    assert!(w1.detail.contains("score_config_sha256"), "{}", w1.detail);
    assert!(report.windows[1].identity_mismatches.is_empty());

    // The config itself edited under its old digest is caught as well.
    edit_window(&dir, |w| {
        w["score_config_sha256"] = serde_json::json!(recorded);
        let n = w["score_config"]["n_trials"].as_u64().unwrap();
        w["score_config"]["n_trials"] = serde_json::json!(n + 1);
    });
    let report = verify_arena(&dir, None).unwrap();
    assert!(!report.ok);
    let fields: Vec<_> = report.windows[0]
        .identity_mismatches
        .iter()
        .map(|m| m.field)
        .collect();
    assert_eq!(fields, [IdentityField::ScoreConfig]);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_window_whose_fault_plan_differs_from_its_header_fails() {
    let a = plan_digest("a");
    let b = plan_digest("b");
    // (tag, plan published under, plan the window file is edited to).
    let cases: [(&str, Option<String>, Option<String>); 3] = [
        ("different", Some(a.clone()), Some(b.clone())),
        ("dropped", Some(a.clone()), None),
        ("added", None, Some(b.clone())),
    ];
    for (tag, published_plan, edited) in cases {
        let dir = temp_dir(&format!("plan-{tag}"));
        published(&dir, published_plan.clone());
        edit_window(&dir, |w| match &edited {
            Some(digest) => {
                w.insert("fault_plan_sha256".into(), serde_json::json!(digest));
            }
            None => {
                w.remove("fault_plan_sha256");
            }
        });
        let report = verify_arena(&dir, None).unwrap();
        assert!(!report.ok, "{tag}");
        assert_eq!(
            report.windows[0].identity_mismatches,
            [mismatch(
                IdentityField::FaultPlanSha256,
                published_plan.as_deref(),
                edited.as_deref()
            )],
            "{tag}"
        );
        assert!(
            report.windows[0].detail.contains("fault_plan_sha256 is"),
            "{tag}: {}",
            report.windows[0].detail
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}

type FieldCase = (
    IdentityField,
    fn(&mut serde_json::Map<String, serde_json::Value>),
);

#[test]
fn every_other_identity_field_is_cross_checked() {
    let cases: [FieldCase; 7] = [
        (IdentityField::WindowId, |w| {
            w["id"] = serde_json::json!("w9");
        }),
        (IdentityField::SchemaVersion, |w| {
            w["schema_version"] = serde_json::json!(9);
        }),
        (IdentityField::CommitDeadline, |w| {
            w["commit_deadline"] = serde_json::json!(11);
        }),
        (IdentityField::DataRevealEpoch, |w| {
            w["data_reveal_epoch"] = serde_json::json!(21);
        }),
        (IdentityField::ScorerArtifactSha256, |w| {
            w["scorer_artifact_sha256"] = serde_json::json!(content_digest(b"other scorer"));
        }),
        (IdentityField::SealedEvalSaltSha256, |w| {
            w["sealed_eval_salt_sha256"] = serde_json::Value::Null;
        }),
        (IdentityField::DatasetHash, |w| {
            w["dataset_hash"] = serde_json::json!(content_digest(b"other dataset"));
        }),
    ];
    for (field, edit) in cases {
        let dir = temp_dir(&format!("field-{}", field.as_str()));
        published(&dir, None);
        edit_window(&dir, edit);
        let report = verify_arena(&dir, None).unwrap();
        assert!(!report.ok, "{field:?}");
        let fields: Vec<_> = report.windows[0]
            .identity_mismatches
            .iter()
            .map(|m| m.field)
            .collect();
        assert_eq!(fields, [field]);
        let _ = std::fs::remove_dir_all(&dir);
    }
}

#[test]
fn a_published_board_without_its_window_file_is_an_error() {
    let dir = temp_dir("missing-window");
    published(&dir, None);
    std::fs::remove_file(window_path(&dir, "w1")).unwrap();
    let error = verify_arena(&dir, None).unwrap_err();
    assert!(error.contains("cannot read"), "{error}");
    let _ = std::fs::remove_dir_all(&dir);
}
