//! S1: a verifying duplicate must not knock an honest entrant off a certifying
//! board.
//!
//! PR #153 closed the wrong-salt griefing path by verifying every entry's
//! pre-image on its own. The correct-salt path stayed open: once an honest
//! entrant reveals, its `agent_id`, `artifact_digest`, `salt` and capture are
//! public, so a rival can append a copy that opens the same commitment. The
//! only entrant-authored field that reached the compared serialization unbound
//! on the certifying path was `in_sample_trials`, and one changed byte made
//! both entries differ, which refused both.
//!
//! These tests drive the whole arena: commit, advance, reveal and score under
//! the re-executing intake, then read the ranked rows and the window record.

use std::path::{Path, PathBuf};

use sharpebench_arena::{
    Arena, EntrantLauncher, IntakeOptions, LaunchedEntrant, ReplayWindow, ReturnsProvenance,
    RevealedEntry,
};
use sharpebench_attest::{content_digest, make_commitment};
use sharpebench_core::ScoreConfig;
use sharpebench_protocol::{AgentTrajectory, Decision, MarketObservation};
use sharpebench_sim::{Agent, Dataset, Momentum};

const WINDOW: &str = "w1";
const HONEST: &str = "alice";

fn temp_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "sharpebench-arena-knockout-{tag}-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn scorer() -> String {
    content_digest(b"knockout-scorer")
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

fn reveal(agent_id: &str, artifact: &str, capture: AgentTrajectory) -> RevealedEntry {
    RevealedEntry {
        agent_id: Some(agent_id.to_string()),
        submission: None,
        capture: Some(capture),
        artifact_digest: artifact.to_string(),
        salt: salt(agent_id),
        fault_plan_sha256: None,
    }
}

fn committed(dir: &Path, data: &Dataset, entrants: &[(&str, &str)]) -> (Arena, PathBuf) {
    let mut arena = Arena::init(dir).unwrap();
    arena
        .open_window_with_provenance(WINDOW, 10, 20, config(), None, scorer())
        .unwrap();
    for (agent_id, artifact) in entrants {
        arena
            .register_entry(
                WINDOW,
                make_commitment(agent_id, WINDOW, artifact, &salt(agent_id)),
            )
            .unwrap();
    }
    arena.advance(20).unwrap();
    let dataset = dir.join("dataset.csv");
    std::fs::write(&dataset, csv(data)).unwrap();
    (arena, dataset)
}

fn refusals(arena: &Arena, agent_id: &str) -> Vec<String> {
    arena
        .window(WINDOW)
        .unwrap()
        .refusals
        .iter()
        .filter(|r| r.agent_id == agent_id)
        .map(|r| r.reason.clone())
        .collect()
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

/// Runs the committed image as momentum, so a capture of it re-executes.
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

/// The attack, end to end: the honest reveal, then an appended copy of it whose
/// only difference is the one field the commitment never bound. The honest row
/// must survive, once, re-executed, on a board that still certifies.
#[test]
fn a_copy_that_edits_only_the_declared_search_budget_does_not_delete_the_honest_row() {
    let dir = temp_dir("append");
    let data = market(1);
    let (reference, digest) = image("committed");
    let (mut arena, dataset) = committed(&dir, &data, &[(HONEST, &digest)]);

    let honest = reveal(
        HONEST,
        &digest,
        capture(&format!("sandbox:{reference}"), &data),
    );
    // The rival commits nothing. It copies the public reveal and edits the one
    // field re-execution does not pin.
    let mut forged = honest.clone();
    forged.capture.as_mut().unwrap().in_sample_trials = 1;

    let mut launcher = Launcher {
        reference: reference.clone(),
    };
    let scores = arena
        .reveal_and_score_with(
            WINDOW,
            &dataset,
            &[honest, forged],
            IntakeOptions {
                allow_supplied_returns: false,
                reexecute: Some(&mut launcher),
            },
        )
        .unwrap();

    let ranked: Vec<&str> = scores.iter().map(|s| s.agent_id.as_str()).collect();
    assert_eq!(ranked, [HONEST], "the honest entrant must keep its row");
    let window = arena.window(WINDOW).unwrap();
    assert_eq!(
        window.returns_provenance.get(HONEST),
        Some(&ReturnsProvenance::ReExecuted)
    );
    assert_eq!(window.certifying, Some(true));
    let refused = refusals(&arena, HONEST);
    assert_eq!(refused.len(), 1, "only the copy is refused: {refused:?}");
    assert!(
        !refused[0].contains("admissible entries that differ"),
        "the refusal must fall on the copy, not on both entries: {refused:?}"
    );
}

/// The same attack with the copy placed first. Order in `entries.json` is the
/// rival's to choose, so the honest row must not depend on it.
#[test]
fn a_prepended_copy_does_not_take_the_honest_entrant_s_row() {
    let dir = temp_dir("prepend");
    let data = market(1);
    let (reference, digest) = image("committed");
    let (mut arena, dataset) = committed(&dir, &data, &[(HONEST, &digest)]);

    let honest = reveal(
        HONEST,
        &digest,
        capture(&format!("sandbox:{reference}"), &data),
    );
    let mut forged = honest.clone();
    forged.capture.as_mut().unwrap().in_sample_trials = 5000;

    let mut launcher = Launcher {
        reference: reference.clone(),
    };
    let scores = arena
        .reveal_and_score_with(
            WINDOW,
            &dataset,
            &[forged, honest],
            IntakeOptions {
                allow_supplied_returns: false,
                reexecute: Some(&mut launcher),
            },
        )
        .unwrap();

    let ranked: Vec<&str> = scores.iter().map(|s| s.agent_id.as_str()).collect();
    assert_eq!(ranked, [HONEST]);
    assert_eq!(
        scores[0].in_sample_trials, 0,
        "the rival must not choose the honest entrant's deflation footprint"
    );
    assert_eq!(arena.window(WINDOW).unwrap().certifying, Some(true));
}

/// One commitment admits one entry even when the copy is byte for byte the
/// honest reveal: the second reveal never opens the commitment.
#[test]
fn a_byte_identical_copy_is_refused_as_a_second_reveal_and_the_honest_row_ranks() {
    let dir = temp_dir("identical");
    let data = market(1);
    let (reference, digest) = image("committed");
    let (mut arena, dataset) = committed(&dir, &data, &[(HONEST, &digest)]);

    let honest = reveal(
        HONEST,
        &digest,
        capture(&format!("sandbox:{reference}"), &data),
    );
    let mut launcher = Launcher {
        reference: reference.clone(),
    };
    let scores = arena
        .reveal_and_score_with(
            WINDOW,
            &dataset,
            &[honest.clone(), honest],
            IntakeOptions {
                allow_supplied_returns: false,
                reexecute: Some(&mut launcher),
            },
        )
        .unwrap();

    let ranked: Vec<&str> = scores.iter().map(|s| s.agent_id.as_str()).collect();
    assert_eq!(ranked, [HONEST]);
    let refused = refusals(&arena, HONEST);
    assert_eq!(refused.len(), 1, "{refused:?}");
    assert!(
        refused[0].contains("already revealed"),
        "a second reveal of one commitment is refused where a commitment is opened: {refused:?}"
    );
}

/// A declared in-sample search budget nothing binds does not reach a scored
/// board: it is folded into the deflation bar, and a rival who commits nothing
/// could otherwise author it.
#[test]
fn a_declared_search_budget_the_commitment_does_not_bind_is_refused() {
    let dir = temp_dir("unbindable");
    let data = market(1);
    let (reference, digest) = image("committed");
    let (mut arena, dataset) = committed(&dir, &data, &[(HONEST, &digest)]);

    let mut entry = reveal(
        HONEST,
        &digest,
        capture(&format!("sandbox:{reference}"), &data),
    );
    entry.capture.as_mut().unwrap().in_sample_trials = 42;

    let mut launcher = Launcher {
        reference: reference.clone(),
    };
    let scores = arena
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

    assert!(scores.is_empty(), "{scores:?}");
    let refused = refusals(&arena, HONEST);
    assert_eq!(refused.len(), 1, "{refused:?}");
    assert!(
        refused[0].contains("in_sample_trials"),
        "the refusal must name the unbound field: {refused:?}"
    );
}
