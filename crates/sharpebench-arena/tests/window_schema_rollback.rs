//! S3: the fields that decide what a board means must not be strippable by a
//! binary that predates them.
//!
//! This round added `certifying`, `supplied_returns_accepted`,
//! `replay_dataset_sha256` and `returns_provenance` to `WindowState` while
//! `WINDOW_SCHEMA_VERSION` stayed at 2 and nothing denied unknown fields.
//! `save()` rewrites every `window.json`, published ones included, so a single
//! rolled-back `arena advance` loaded each file, dropped the four fields it did
//! not know, and wrote it back without them. `identity_mismatches` binds three
//! of them, so `verify_arena` would then fail on every published window with no
//! recovery path.
//!
//! The fix is the shape the gateway journal already uses (#150): the version is
//! derived from the content, so a record that carries the fields carries a
//! version the older binary refuses, and a record without them keeps the
//! version and the bytes it always had.

use std::path::{Path, PathBuf};

use sharpebench_arena::{
    Arena, EntrantLauncher, IntakeOptions, LaunchedEntrant, ReplayWindow, RevealedEntry,
    FAULTED_WINDOW_SCHEMA_VERSION, STATE_FILE, WINDOWS_DIR, WINDOW_FILE, WINDOW_SCHEMA_VERSION,
};
use sharpebench_attest::{content_digest, make_commitment};
use sharpebench_core::ScoreConfig;
use sharpebench_protocol::{AgentTrajectory, Decision, MarketObservation};
use sharpebench_sim::{Agent, Dataset, Momentum};

const WINDOW: &str = "w1";
const ENTRANT: &str = "alice";

/// The four fields this round added, and that decide what a board means.
const SCORE_MEANING_FIELDS: [&str; 4] = [
    "certifying",
    "supplied_returns_accepted",
    "replay_dataset_sha256",
    "returns_provenance",
];

/// The three a certifying re-executed window carries. The fourth,
/// `supplied_returns_accepted`, is false on such a board and is skipped, which
/// is what keeps an unscored record's bytes unchanged.
const CERTIFYING_FIELDS: [&str; 3] = ["certifying", "replay_dataset_sha256", "returns_provenance"];

fn temp_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "sharpebench-arena-window-schema-{tag}-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn scorer() -> String {
    content_digest(b"window-schema-scorer")
}

fn config() -> ScoreConfig {
    ScoreConfig {
        execution_seeds_per_window: 2,
        ..ScoreConfig::default()
    }
}

fn market(seed: u64) -> Dataset {
    Dataset::synthetic_parameterized(4, 160, seed, 3.0, 0.0, 0.0)
}

fn csv(data: &Dataset) -> String {
    let mut out = String::from("date,symbol,close\n");
    for (symbol, closes) in &data.closes {
        for (date, close) in data.dates.iter().zip(closes) {
            out.push_str(&format!("{date},{symbol},{close}\n"));
        }
    }
    out
}

fn image(tag: &str) -> (String, String) {
    let digest = content_digest(format!("image-{tag}").as_bytes());
    (format!("test/{tag}@sha256:{digest}"), digest)
}

fn salt(agent_id: &str) -> String {
    format!("salt-{agent_id}")
}

fn capture(entrant: &str, data: &Dataset) -> AgentTrajectory {
    let replay = ReplayWindow::parse(csv(data).as_bytes(), &config()).unwrap();
    let (_, mut capture) = sharpebench_harness::run_agent_capture(
        entrant,
        &replay.data,
        &replay.windows,
        &replay.seeds,
        replay.costs,
        || Box::new(Momentum::default()) as Box<dyn Agent>,
    );
    capture.contract.as_mut().unwrap().runner_artifact_sha256 = Some(scorer());
    capture
}

fn committed(dir: &Path, data: &Dataset) -> (Arena, PathBuf) {
    let mut arena = Arena::init(dir).unwrap();
    arena
        .open_window_with_provenance(WINDOW, 10, 20, config(), None, scorer())
        .unwrap();
    arena
        .register_entry(
            WINDOW,
            make_commitment(ENTRANT, WINDOW, &image("committed").1, &salt(ENTRANT)),
        )
        .unwrap();
    arena.advance(20).unwrap();
    let dataset = dir.join("dataset.csv");
    std::fs::write(&dataset, csv(data)).unwrap();
    (arena, dataset)
}

fn window_json(dir: &Path) -> serde_json::Value {
    serde_json::from_slice(
        &std::fs::read(dir.join(WINDOWS_DIR).join(WINDOW).join(WINDOW_FILE)).unwrap(),
    )
    .unwrap()
}

fn write_window_json(dir: &Path, value: &serde_json::Value) {
    std::fs::write(
        dir.join(WINDOWS_DIR).join(WINDOW).join(WINDOW_FILE),
        serde_json::to_string_pretty(value).unwrap(),
    )
    .unwrap();
}

struct Instance(Momentum);

impl Agent for Instance {
    fn decide(&mut self, observation: &MarketObservation) -> Decision {
        self.0.decide(observation)
    }
}

impl LaunchedEntrant for Instance {
    fn fault(&self) -> Option<String> {
        None
    }

    fn finish(self: Box<Self>) -> Result<(), String> {
        Ok(())
    }
}

struct Launcher {
    reference: String,
}

impl EntrantLauncher for Launcher {
    fn ready(&mut self) -> Result<(), String> {
        Ok(())
    }

    fn admit(&mut self, image: &str) -> Result<(), String> {
        if image == self.reference {
            Ok(())
        } else {
            Err(format!("image `{image}` is not available"))
        }
    }

    fn launch(&mut self, _image: &str) -> Result<Box<dyn LaunchedEntrant>, String> {
        Ok(Box::new(Instance(Momentum::default())))
    }
}

fn score(dir: &Path, data: &Dataset) -> Arena {
    let (mut arena, dataset) = committed(dir, data);
    let (reference, digest) = image("committed");
    let entry = RevealedEntry {
        agent_id: Some(ENTRANT.to_string()),
        submission: None,
        capture: Some(capture(&format!("sandbox:{reference}"), data)),
        artifact_digest: digest,
        salt: salt(ENTRANT),
        fault_plan_sha256: None,
    };
    let mut launcher = Launcher { reference };
    arena
        .reveal_and_score_with(
            WINDOW,
            &dataset,
            &[entry],
            IntakeOptions {
                allow_supplied_returns: false,
                reexecute: Some(&mut launcher),
            },
        )
        .unwrap();
    arena
}

/// A record that carries the score-meaning fields carries a schema version the
/// binaries that predate them do not accept, so a rollback refuses the window
/// rather than loading it, dropping the fields and writing it back.
#[test]
fn a_scored_window_carries_a_schema_version_an_older_binary_refuses() {
    let dir = temp_dir("scored");
    let data = market(1);
    let _arena = score(&dir, &data);

    // Stated as the property rather than the number: whatever the version is,
    // it must not be one the binaries that predate these fields accept, since
    // those two are the whole set such a binary will load.
    let window = window_json(&dir);
    for known in [WINDOW_SCHEMA_VERSION, FAULTED_WINDOW_SCHEMA_VERSION] {
        assert_ne!(
            window["schema_version"], known,
            "a binary that reads schema {known} would load this record, drop the fields it \
             does not know, and write it back without them"
        );
    }
    for field in CERTIFYING_FIELDS {
        assert!(
            window.get(field).is_some(),
            "`{field}` must be on the scored record"
        );
    }
    let _ = std::fs::remove_dir_all(&dir);
}

/// The other half of the same rule: a record that carries none of the fields is
/// written exactly as it was before they existed, so an older binary still
/// reads it.
#[test]
fn an_unscored_window_keeps_the_schema_version_and_the_bytes_it_always_had() {
    let dir = temp_dir("unscored");
    let data = market(1);
    let (_arena, _dataset) = committed(&dir, &data);

    let window = window_json(&dir);
    assert_eq!(window["schema_version"], WINDOW_SCHEMA_VERSION);
    for field in SCORE_MEANING_FIELDS {
        assert!(
            window.get(field).is_none(),
            "`{field}` is at its default and must not be serialized"
        );
    }
    let _ = std::fs::remove_dir_all(&dir);
}

/// Refuse rather than strip, in the forward direction too: a record written by
/// a later release carries keys this binary does not know, and loading it
/// would drop them on the next save.
#[test]
fn a_window_carrying_an_unknown_field_is_refused_rather_than_loaded_and_stripped() {
    let dir = temp_dir("unknown");
    let data = market(1);
    let (_arena, _dataset) = committed(&dir, &data);

    let mut window = window_json(&dir);
    window
        .as_object_mut()
        .unwrap()
        .insert("a_later_release_field".to_string(), serde_json::json!(true));
    write_window_json(&dir, &window);

    let error = Arena::load(&dir)
        .err()
        .expect("an unknown field must not load");
    assert!(
        error.contains("a_later_release_field"),
        "the refusal must name the field it will not drop: {error}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// The rewrite path itself: `save()` touches every window on every `advance`,
/// so a reload followed by an epoch advance must return the record unchanged.
#[test]
fn reloading_and_advancing_preserves_a_scored_window_byte_for_byte() {
    let dir = temp_dir("roundtrip");
    let data = market(1);
    let arena = score(&dir, &data);
    drop(arena);

    let before = std::fs::read(dir.join(WINDOWS_DIR).join(WINDOW).join(WINDOW_FILE)).unwrap();
    let state_before = std::fs::read(dir.join(STATE_FILE)).unwrap();

    let mut arena = match Arena::load(&dir) {
        Ok(arena) => arena,
        Err(error) => panic!("a scored window must reload under its own binary: {error}"),
    };
    arena.advance(21).unwrap();

    let after = std::fs::read(dir.join(WINDOWS_DIR).join(WINDOW).join(WINDOW_FILE)).unwrap();
    assert_eq!(
        before, after,
        "an epoch advance must not rewrite a scored window's record"
    );
    assert_ne!(
        state_before,
        std::fs::read(dir.join(STATE_FILE)).unwrap(),
        "the advance did happen"
    );
    let _ = std::fs::remove_dir_all(&dir);
}
