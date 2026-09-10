//! A window scored under a fault plan binds the plan's digest into its
//! identity, beside `score_config_sha256`: the window record, the revealed
//! entries, the supersession ledger and the signed header. A window without a
//! plan serializes exactly as it did before the field existed.

use std::path::{Path, PathBuf};

use sharpebench_arena::{
    verify_arena, Arena, RevealedEntry, SigningKey, WindowState, WindowStatus, BOARD_FILE,
    BOARD_MD_FILE, FAULTED_WINDOW_SCHEMA_VERSION, STATE_FILE, WINDOWS_DIR, WINDOW_FILE,
    WINDOW_SCHEMA_VERSION,
};
use sharpebench_attest::{
    content_digest, make_commitment, make_commitment_under_fault_plan, PublicChain,
};
use sharpebench_core::{AgentSubmission, Run, ScoreConfig};

fn temp_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "sharpebench-arena-fault-plan-{tag}-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn plan_digest(tag: &str) -> String {
    content_digest(format!("fault-plan-{tag}").as_bytes())
}

fn scorer() -> String {
    content_digest(b"fault-plan-identity-scorer")
}

fn window_path(dir: &Path, id: &str) -> PathBuf {
    dir.join(WINDOWS_DIR).join(id).join(WINDOW_FILE)
}

fn open(arena: &mut Arena, id: &str, plan: Option<String>) -> Result<(), String> {
    let deadline = arena.current_epoch() + 10;
    arena.open_window_with_fault_plan(
        id,
        deadline,
        deadline + 10,
        ScoreConfig::default(),
        None,
        scorer(),
        plan,
    )
}

fn entry(agent_id: &str, artifact: &str, plan: Option<String>) -> RevealedEntry {
    RevealedEntry {
        submission: AgentSubmission {
            agent_id: agent_id.to_string(),
            runs: vec![Run {
                returns: (0..40).map(|i| 0.001 * (i as f64 + 1.0).sin()).collect(),
                ..Run::default()
            }],
            in_sample_trials: 0,
            candidates: Vec::new(),
        },
        artifact_digest: artifact.to_string(),
        salt: format!("salt-{agent_id}"),
        fault_plan_sha256: plan,
    }
}

/// Open `id` under `plan`, commit one entrant (binding the plan, as a faulted
/// window's commitment must) and advance to the reveal epoch.
fn committed_window(dir: &Path, id: &str, plan: Option<String>) -> (Arena, String) {
    let mut arena = Arena::init(dir).unwrap();
    open(&mut arena, id, plan.clone()).unwrap();
    let artifact = content_digest(b"fault-plan-entrant");
    arena
        .register_entry(
            id,
            make_commitment_under_fault_plan("alpha", id, &artifact, "salt-alpha", plan.as_deref()),
        )
        .unwrap();
    let reveal = arena.window(id).unwrap().data_reveal_epoch;
    arena.advance(reveal).unwrap();
    (arena, artifact)
}

fn write_dataset(dir: &Path) -> PathBuf {
    let path = dir.join("dataset.csv");
    std::fs::write(&path, b"sym,close\nA,1.0\nA,1.01\n").unwrap();
    path
}

fn edit_json(path: &Path, edit: impl FnOnce(&mut serde_json::Value)) {
    let mut value: serde_json::Value =
        serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    edit(&mut value);
    std::fs::write(path, serde_json::to_string_pretty(&value).unwrap()).unwrap();
}

fn copy_tree(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for item in std::fs::read_dir(from).unwrap() {
        let item = item.unwrap();
        let target = to.join(item.file_name());
        if item.file_type().unwrap().is_dir() {
            copy_tree(&item.path(), &target);
        } else {
            std::fs::copy(item.path(), target).unwrap();
        }
    }
}

fn first_payload(board_path: &Path) -> serde_json::Value {
    let board: PublicChain = serde_json::from_slice(&std::fs::read(board_path).unwrap()).unwrap();
    serde_json::from_str(&board.chain[0].payload).unwrap()
}

#[test]
fn the_committed_arena_round_trips_byte_identically() {
    let committed = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../arena");
    for id in ["window-002", "window-003"] {
        let bytes = std::fs::read(window_path(&committed, id)).unwrap();
        let window: WindowState = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(window.fault_plan_sha256, None);
        assert_eq!(
            serde_json::to_string_pretty(&window).unwrap().as_bytes(),
            bytes.as_slice(),
            "{id} no longer round-trips to its committed bytes"
        );
    }

    // Load the whole committed arena (supersession ledger, replacement link and
    // active window) and persist it again through `advance` at its own epoch.
    let dir = temp_dir("committed");
    copy_tree(&committed, &dir);
    let mut arena = Arena::load(&dir).unwrap();
    arena.advance(arena.current_epoch()).unwrap();
    for relative in [
        PathBuf::from(STATE_FILE),
        window_path(Path::new(""), "window-001"),
        window_path(Path::new(""), "window-002"),
        window_path(Path::new(""), "window-003"),
    ] {
        assert_eq!(
            std::fs::read(dir.join(&relative)).unwrap(),
            std::fs::read(committed.join(&relative)).unwrap(),
            "{} changed on a load and save",
            relative.display()
        );
    }
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_plan_less_window_entry_and_header_carry_no_fault_field() {
    let dir = temp_dir("plan-less");
    let (mut arena, artifact) = committed_window(&dir, "w1", None);
    let window = std::fs::read_to_string(window_path(&dir, "w1")).unwrap();
    assert!(!window.contains("fault_plan"), "{window}");
    assert_eq!(
        arena.window("w1").unwrap().schema_version,
        WINDOW_SCHEMA_VERSION
    );
    let plan_less = entry("alpha", &artifact, None);
    assert!(!serde_json::to_string(&plan_less)
        .unwrap()
        .contains("fault_plan"));

    arena
        .reveal_and_score("w1", &write_dataset(&dir), &[plan_less])
        .unwrap();
    let board = arena
        .publish("w1", &SigningKey::derive(b"fault-plan-key"))
        .unwrap();
    let header = first_payload(&board);
    assert!(header.get("fault_plan_sha256").is_none(), "{header}");
    let md = std::fs::read_to_string(dir.join(WINDOWS_DIR).join("w1").join(BOARD_MD_FILE)).unwrap();
    assert!(!md.contains("fault plan"), "{md}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_faulted_window_binds_its_plan_through_to_the_signed_header() {
    let dir = temp_dir("faulted");
    let plan = plan_digest("a");
    let (arena, artifact) = committed_window(&dir, "w1", Some(plan.clone()));
    let window = arena.window("w1").unwrap();
    assert_eq!(window.fault_plan_sha256.as_deref(), Some(plan.as_str()));
    assert_eq!(window.schema_version, FAULTED_WINDOW_SCHEMA_VERSION);

    // The record survives a reload, and scores entries declaring the same plan.
    let mut arena = Arena::load(&dir).unwrap();
    assert_eq!(
        arena.window("w1").unwrap().fault_plan_sha256.as_deref(),
        Some(plan.as_str())
    );
    let scores = arena
        .reveal_and_score(
            "w1",
            &write_dataset(&dir),
            &[entry("alpha", &artifact, Some(plan.clone()))],
        )
        .unwrap();
    assert_eq!(scores.len(), 1);
    let board = arena
        .publish("w1", &SigningKey::derive(b"fault-plan-key"))
        .unwrap();
    let header = first_payload(&board);
    assert_eq!(header["fault_plan_sha256"], plan.as_str());
    assert_eq!(header["schema_version"], FAULTED_WINDOW_SCHEMA_VERSION);
    let md = std::fs::read_to_string(dir.join(WINDOWS_DIR).join("w1").join(BOARD_MD_FILE)).unwrap();
    assert!(md.contains(&plan), "{md}");
    assert!(verify_arena(&dir, None).unwrap().ok);
    assert!(dir.join(WINDOWS_DIR).join("w1").join(BOARD_FILE).exists());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn opening_refuses_a_malformed_plan_digest() {
    let dir = temp_dir("open-malformed");
    let mut arena = Arena::init(&dir).unwrap();
    for bad in ["", "ABC", &"A".repeat(64), &"g".repeat(64), &"a".repeat(63)] {
        let error = open(&mut arena, "w1", Some(bad.to_string())).unwrap_err();
        assert!(error.contains("fault plan digest"), "{error}");
    }
    assert!(arena.window("w1").is_none());
    assert!(!window_path(&dir, "w1").exists());
    let _ = std::fs::remove_dir_all(&dir);
}

/// (tag, plan opened under, edit to the window file, expected refusal).
type LoadCase = (
    &'static str,
    Option<String>,
    fn(&mut serde_json::Value),
    &'static str,
);

#[test]
fn loading_refuses_a_plan_digest_that_disagrees_with_the_schema() {
    // Present on a plan-less schema, absent on the faulted schema, and a
    // malformed digest: each is refused on load, as a config digest mismatch is.
    let cases: [LoadCase; 3] = [
        (
            "added",
            None,
            |w| w["fault_plan_sha256"] = serde_json::json!(plan_digest("added")),
            "expected 3 for a window with fault plan",
        ),
        (
            "removed",
            Some(plan_digest("removed")),
            |w| {
                w.as_object_mut().unwrap().remove("fault_plan_sha256");
            },
            "expected 2 for a window with no fault plan",
        ),
        (
            "malformed",
            Some(plan_digest("malformed")),
            |w| w["fault_plan_sha256"] = serde_json::json!("not-a-digest"),
            "fault plan digest must be",
        ),
    ];
    for (tag, plan, edit, expected) in cases {
        let dir = temp_dir(&format!("load-{tag}"));
        let mut arena = Arena::init(&dir).unwrap();
        open(&mut arena, "w1", plan).unwrap();
        edit_json(&window_path(&dir, "w1"), edit);
        let error = Arena::load(&dir).err().expect("the edited window loaded");
        assert!(error.contains(expected), "{tag}: {error}");
        let _ = std::fs::remove_dir_all(&dir);
    }
}

#[test]
fn scoring_refuses_an_entry_run_under_another_plan_and_records_nothing() {
    let window_plan = plan_digest("window");
    let cases = [
        (
            "different",
            Some(window_plan.clone()),
            Some(plan_digest("other")),
        ),
        ("absent", Some(window_plan.clone()), None),
        ("present", None, Some(window_plan.clone())),
    ];
    for (tag, window, declared) in cases {
        let dir = temp_dir(&format!("score-{tag}"));
        let (mut arena, artifact) = committed_window(&dir, "w1", window.clone());
        let before = std::fs::read(window_path(&dir, "w1")).unwrap();
        let entries = [
            entry("alpha", &artifact, window.clone()),
            entry("beta", &artifact, declared),
        ];
        let error = arena
            .reveal_and_score("w1", &write_dataset(&dir), &entries)
            .unwrap_err();
        assert!(
            error.contains("entry `beta` was produced under"),
            "{tag}: {error}"
        );
        assert_eq!(
            std::fs::read(window_path(&dir, "w1")).unwrap(),
            before,
            "{tag}: a refused score wrote the window"
        );
        let reloaded = Arena::load(&dir).unwrap();
        let w = reloaded.window("w1").unwrap();
        assert_eq!(w.status, WindowStatus::Committed, "{tag}");
        assert!(w.scores.is_empty() && w.refusals.is_empty() && w.dataset_hash.is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }
}

/// Supersede empty `old`, open `new` under `plan` and link it.
fn superseded_and_linked(dir: &Path, plan: Option<String>) {
    let mut arena = Arena::init(dir).unwrap();
    open(&mut arena, "old", None).unwrap();
    Arena::supersede_empty_window(dir, "old", "replaced under a fault plan").unwrap();
    let mut arena = Arena::load(dir).unwrap();
    open(&mut arena, "new", plan).unwrap();
    Arena::link_supersession_replacement(dir, "old", "new").unwrap();
}

#[test]
fn a_supersession_records_the_replacement_plan_and_refuses_a_mismatch() {
    let plan = plan_digest("replacement");

    // The link records the replacement's plan digest; without one, the ledger
    // field is omitted and the state file carries no fault key at all.
    let dir = temp_dir("supersede-faulted");
    superseded_and_linked(&dir, Some(plan.clone()));
    let state: serde_json::Value =
        serde_json::from_slice(&std::fs::read(dir.join(STATE_FILE)).unwrap()).unwrap();
    assert_eq!(
        state["superseded"][0]["replacement_fault_plan_sha256"],
        plan.as_str()
    );
    Arena::load(&dir).unwrap();
    let _ = std::fs::remove_dir_all(&dir);

    let dir = temp_dir("supersede-plan-less");
    superseded_and_linked(&dir, None);
    let state = std::fs::read_to_string(dir.join(STATE_FILE)).unwrap();
    assert!(!state.contains("fault_plan"), "{state}");
    Arena::load(&dir).unwrap();
    let _ = std::fs::remove_dir_all(&dir);

    // A ledger that disagrees with its replacement is refused on load: another
    // digest, the digest dropped, or a digest recorded for a plan-less window.
    let cases: [(&str, Option<String>, Option<String>); 3] = [
        ("different", Some(plan.clone()), Some(plan_digest("other"))),
        ("dropped", Some(plan.clone()), None),
        ("invented", None, Some(plan.clone())),
    ];
    for (tag, replacement_plan, recorded) in cases {
        let dir = temp_dir(&format!("supersede-{tag}"));
        superseded_and_linked(&dir, replacement_plan);
        edit_json(&dir.join(STATE_FILE), |state| {
            let record = state["superseded"][0].as_object_mut().unwrap();
            match &recorded {
                Some(digest) => {
                    record.insert(
                        "replacement_fault_plan_sha256".into(),
                        serde_json::json!(digest),
                    );
                }
                None => {
                    record.remove("replacement_fault_plan_sha256");
                }
            }
        });
        let error = Arena::load(&dir).err().expect("the edited ledger loaded");
        assert!(
            error.contains("replacement `new` fault plan digest mismatch"),
            "{tag}: {error}"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}

#[test]
fn a_commitment_for_another_plan_is_refused_at_reveal() {
    // The entrant's pre-deadline commitment binds the plan, so an entry that
    // declares the window's plan at score time but committed under another
    // plan, or under none, is refused and recorded like any failed reveal. An
    // honest entrant committed under the window's plan scores beside it.
    let window_plan = plan_digest("window");
    let cases = [
        (
            "different",
            Some(window_plan.clone()),
            Some(plan_digest("other")),
        ),
        ("none-on-faulted", Some(window_plan.clone()), None),
        ("plan-on-unfaulted", None, Some(window_plan.clone())),
    ];
    for (tag, window, committed) in cases {
        let dir = temp_dir(&format!("commit-{tag}"));
        let mut arena = Arena::init(&dir).unwrap();
        open(&mut arena, "w1", window.clone()).unwrap();
        let artifact = content_digest(b"fault-plan-entrant");
        let honest = make_commitment_under_fault_plan(
            "alpha",
            "w1",
            &artifact,
            "salt-alpha",
            window.as_deref(),
        );
        let mismatched = make_commitment_under_fault_plan(
            "beta",
            "w1",
            &artifact,
            "salt-beta",
            committed.as_deref(),
        );
        if committed.is_none() {
            assert_eq!(
                mismatched,
                make_commitment("beta", "w1", &artifact, "salt-beta")
            );
        }
        arena.register_entry("w1", honest).unwrap();
        arena.register_entry("w1", mismatched).unwrap();
        let reveal = arena.window("w1").unwrap().data_reveal_epoch;
        arena.advance(reveal).unwrap();

        let scores = arena
            .reveal_and_score(
                "w1",
                &write_dataset(&dir),
                &[
                    entry("alpha", &artifact, window.clone()),
                    entry("beta", &artifact, window.clone()),
                ],
            )
            .unwrap();
        assert_eq!(scores.len(), 1, "{tag}");
        assert_eq!(scores[0].agent_id, "alpha", "{tag}");
        let reloaded = Arena::load(&dir).unwrap();
        let refusals = &reloaded.window("w1").unwrap().refusals;
        assert_eq!(refusals.len(), 1, "{tag}");
        assert_eq!(refusals[0].agent_id, "beta", "{tag}");
        assert_eq!(
            refusals[0].reason, "reveal does not match commitment",
            "{tag}"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
