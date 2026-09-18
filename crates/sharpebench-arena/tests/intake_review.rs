//! Regressions for the 2026-09-17 pre-merge review of bound forward intake
//! (`RETRO-CR-PENDING.md`, rows F1-1 to F1-9).
//!
//! A board that ranks a replayed capture is signed noncertifying, so a capture
//! of hindsight decisions never reaches a certifying board. A reference-agent
//! commitment names one agent. A forged entry is refused alone. This file reads
//! the new marks from the window file and the signed header as JSON, so it
//! builds against the intake that predates them, and fails there.

use std::path::{Path, PathBuf};

use sharpebench_arena::{
    Arena, EntrantLauncher, IntakeOptions, LaunchedEntrant, ReplayWindow, ReturnsProvenance,
    RevealedEntry, SigningKey, BOARD_FILE, BOARD_MD_FILE, WINDOWS_DIR, WINDOW_FILE,
};
use sharpebench_attest::{content_digest, make_commitment_under_fault_plan, PublicChain};
use sharpebench_core::ScoreConfig;
use sharpebench_protocol::{Action, AgentTrajectory, Decision, MarketObservation, Order};
use sharpebench_sim::{Agent, BuyAndHold, Dataset, Momentum};

const WINDOW: &str = "w1";

fn temp_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "sharpebench-arena-intake-review-{tag}-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn scorer() -> String {
    content_digest(b"intake-review-scorer")
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

/// The documented pre-image of a reference entrant's artifact digest,
/// restated here so the test pins the published formula.
fn reference_digest(name: &str, runner: &str) -> String {
    content_digest(format!("sharpebench-arena/reference-entrant/v1\n{name}\n{runner}").as_bytes())
}

fn salt(agent_id: &str) -> String {
    format!("salt-{agent_id}")
}

struct Oracle {
    data: Dataset,
}

impl Agent for Oracle {
    fn decide(&mut self, observation: &MarketObservation) -> Decision {
        let now = self.data.dates.iter().position(|d| *d == observation.date);
        let best = now.and_then(|t| {
            self.data
                .closes
                .iter()
                .filter_map(|(symbol, closes)| {
                    let gain = closes.get(t + 1)? / closes[t] - 1.0;
                    (gain > 0.0).then_some((symbol, gain))
                })
                .max_by(|a, b| a.1.total_cmp(&b.1))
                .map(|(symbol, _)| symbol.clone())
        });
        Decision {
            orders: observation
                .symbols
                .iter()
                .map(|s| {
                    let long = best.as_deref() == Some(s.symbol.as_str());
                    Order {
                        symbol: s.symbol.clone(),
                        action: if long { Action::Buy } else { Action::Close },
                        target_weight: if long { 0.5 } else { 0.0 },
                        confidence: None,
                        rationale: String::new(),
                    }
                })
                .collect(),
            reasoning: String::new(),
            cost: None,
        }
    }
}

fn capture(entrant: &str, data: &Dataset, make: impl Fn() -> Box<dyn Agent>) -> AgentTrajectory {
    let replay = ReplayWindow::parse(csv(data).as_bytes(), &config()).unwrap();
    let (_, mut capture) = sharpebench_harness::run_agent_capture(
        entrant,
        &replay.data,
        &replay.windows,
        &replay.seeds,
        replay.costs,
        make,
    );
    capture.contract.as_mut().unwrap().runner_artifact_sha256 = Some(scorer());
    capture
}

fn momentum() -> Box<dyn Agent> {
    Box::new(Momentum::default())
}

fn oracle(data: &Dataset) -> impl Fn() -> Box<dyn Agent> + '_ {
    move || Box::new(Oracle { data: data.clone() })
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

fn committed(
    dir: &Path,
    data: &Dataset,
    entrants: &[(&str, &str)],
    plan: Option<&str>,
) -> (Arena, PathBuf) {
    let mut arena = Arena::init(dir).unwrap();
    arena
        .open_window_with_fault_plan(
            WINDOW,
            10,
            20,
            config(),
            None,
            scorer(),
            plan.map(str::to_string),
        )
        .unwrap();
    for (agent_id, artifact) in entrants {
        arena
            .register_entry(
                WINDOW,
                make_commitment_under_fault_plan(agent_id, WINDOW, artifact, &salt(agent_id), plan),
            )
            .unwrap();
    }
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

fn header_json(dir: &Path) -> serde_json::Value {
    let board: PublicChain = serde_json::from_slice(
        &std::fs::read(dir.join(WINDOWS_DIR).join(WINDOW).join(BOARD_FILE)).unwrap(),
    )
    .unwrap();
    serde_json::from_str(&board.chain[0].payload).unwrap()
}

fn board_md(dir: &Path) -> String {
    std::fs::read_to_string(dir.join(WINDOWS_DIR).join(WINDOW).join(BOARD_MD_FILE)).unwrap()
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

fn provenance(arena: &Arena, agent_id: &str) -> Option<ReturnsProvenance> {
    arena
        .window(WINDOW)
        .unwrap()
        .returns_provenance
        .get(agent_id)
        .copied()
}

/// Runs the committed image as momentum; `ready` fails when `down`.
struct Launcher {
    reference: String,
    down: bool,
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

impl EntrantLauncher for Launcher {
    fn ready(&mut self) -> Result<(), String> {
        if self.down {
            Err("no container runtime".to_string())
        } else {
            Ok(())
        }
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

/// F1-1: replay alone ranks a capture of hindsight decisions that names the
/// committed image, so the board it lands on is signed noncertifying.
#[test]
fn a_board_that_ranks_a_replayed_hindsight_capture_is_signed_noncertifying() {
    let dir = temp_dir("replayed");
    let data = market(1);
    let (reference, digest) = image("committed");
    let (mut arena, dataset) = committed(&dir, &data, &[("oracle", &digest)], None);
    let entry = reveal(
        "oracle",
        &digest,
        capture(&format!("sandbox:{reference}"), &data, oracle(&data)),
    );
    let scores = arena.reveal_and_score(WINDOW, &dataset, &[entry]).unwrap();
    assert_eq!(scores.len(), 1);
    assert!(scores[0].rank_eligible, "{:?}", scores[0]);
    assert_eq!(
        provenance(&arena, "oracle"),
        Some(ReturnsProvenance::Replayed)
    );
    assert_eq!(window_json(&dir)["certifying"], false);

    arena.publish(WINDOW, &SigningKey::derive(b"k")).unwrap();
    assert_eq!(header_json(&dir)["certifying"], false);
    let md = board_md(&dir);
    assert!(
        md.starts_with("# Arena window `w1`\n\n**Noncertifying board.**"),
        "{md}"
    );
    assert!(
        md.contains("- A row marked `replayed` ranks recorded decisions that were not re-executed"),
        "{md}"
    );
    assert!(!md.contains("supplied returns allowed"), "{md}");
    let _ = std::fs::remove_dir_all(&dir);
}

/// F1-1: under re-execution the hindsight capture is refused, and a board of
/// re-executed rows is signed certifying.
#[test]
fn a_reexecuted_board_refuses_the_hindsight_capture_and_certifies() {
    let dir = temp_dir("reexecuted");
    let data = market(1);
    let (reference, digest) = image("committed");
    let entrant = format!("sandbox:{reference}");
    let (mut arena, dataset) = committed(
        &dir,
        &data,
        &[("oracle", &digest), ("honest", &digest)],
        None,
    );
    let mut launcher = Launcher {
        reference: reference.clone(),
        down: false,
    };
    let scores = arena
        .reveal_and_score_with(
            WINDOW,
            &dataset,
            &[
                reveal("oracle", &digest, capture(&entrant, &data, oracle(&data))),
                reveal("honest", &digest, capture(&entrant, &data, momentum)),
            ],
            IntakeOptions {
                allow_supplied_returns: false,
                reexecute: Some(&mut launcher),
            },
        )
        .unwrap();
    assert_eq!(scores.len(), 1);
    assert_eq!(scores[0].agent_id, "honest");
    assert!(refusals(&arena, "oracle")[0].starts_with("re-execution diverged"));
    assert_eq!(window_json(&dir)["certifying"], true);
    arena.publish(WINDOW, &SigningKey::derive(b"k")).unwrap();
    assert_eq!(header_json(&dir)["certifying"], true);
    assert!(!board_md(&dir).contains("Noncertifying"));
    let _ = std::fs::remove_dir_all(&dir);
}

/// F1-3: a commitment names one reference agent, so the entrant cannot choose
/// between them after the reveal, and a bare runner digest names neither.
#[test]
fn a_reference_commitment_admits_only_the_named_reference_agent() {
    let dir = temp_dir("reference");
    let data = market(7);
    let committed_momentum = reference_digest("momentum", &scorer());
    let (mut arena, dataset) = committed(
        &dir,
        &data,
        &[
            ("switched", committed_momentum.as_str()),
            ("named", committed_momentum.as_str()),
            ("bare-runner", scorer().as_str()),
        ],
        None,
    );
    let mut launcher = Launcher {
        reference: String::new(),
        down: true,
    };
    let scores = arena
        .reveal_and_score_with(
            WINDOW,
            &dataset,
            &[
                reveal(
                    "switched",
                    &committed_momentum,
                    capture("buy-and-hold", &data, || Box::new(BuyAndHold)),
                ),
                reveal(
                    "named",
                    &committed_momentum,
                    capture("momentum", &data, momentum),
                ),
                reveal(
                    "bare-runner",
                    &scorer(),
                    capture("momentum", &data, momentum),
                ),
            ],
            IntakeOptions {
                allow_supplied_returns: false,
                reexecute: Some(&mut launcher),
            },
        )
        .unwrap();
    assert_eq!(scores.len(), 1);
    assert_eq!(scores[0].agent_id, "named");
    assert_eq!(
        provenance(&arena, "named"),
        Some(ReturnsProvenance::ReExecuted)
    );
    assert_eq!(
        refusals(&arena, "switched"),
        [format!(
            "the capture records entrant artifact {}, but the committed artifact digest is {committed_momentum}",
            reference_digest("buy-and-hold", &scorer())
        )]
    );
    assert_eq!(
        refusals(&arena, "bare-runner"),
        [format!(
            "the capture records entrant artifact {committed_momentum}, but the committed artifact digest is {}",
            scorer()
        )]
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// F1-4: an entry that does not open the agent's commitment is refused alone;
/// the honest reveal beside it is ranked. A copy carrying the honest salt and
/// other decisions arrives after the commitment is already open, so it is
/// refused there, also alone.
#[test]
fn a_forged_entry_is_refused_alone() {
    let dir = temp_dir("forged");
    let data = market(1);
    let (reference, digest) = image("committed");
    let entrant = format!("sandbox:{reference}");
    let (mut arena, dataset) = committed(&dir, &data, &[("alpha", &digest)], None);
    let honest = reveal("alpha", &digest, capture(&entrant, &data, momentum));
    let wrong_salt = RevealedEntry {
        salt: "not-alpha-salt".to_string(),
        ..reveal("alpha", &digest, capture(&entrant, &data, oracle(&data)))
    };
    let copied_salt = reveal("alpha", &digest, capture(&entrant, &data, oracle(&data)));
    let mut launcher = Launcher {
        reference,
        down: false,
    };
    let scores = arena
        .reveal_and_score_with(
            WINDOW,
            &dataset,
            &[wrong_salt, honest, copied_salt],
            IntakeOptions {
                allow_supplied_returns: false,
                reexecute: Some(&mut launcher),
            },
        )
        .unwrap();
    assert_eq!(scores.len(), 1);
    assert_eq!(
        provenance(&arena, "alpha"),
        Some(ReturnsProvenance::ReExecuted)
    );
    let reasons = refusals(&arena, "alpha");
    assert_eq!(reasons.len(), 2, "{reasons:?}");
    assert_eq!(reasons[0], "reveal does not match commitment");
    assert_eq!(
        reasons[1],
        "commitment already revealed; one commitment opens once"
    );
    assert_eq!(window_json(&dir)["certifying"], true);
    let _ = std::fs::remove_dir_all(&dir);
}

/// F1-4, as S1 amended it: one commitment admits one entry, and the refusal
/// falls on the reveal that arrives second rather than on both. A pre-image is
/// public once it is revealed, so refusing both let a rival who committed
/// nothing delete the entrant's row by appending a copy.
#[test]
fn a_second_reveal_of_one_commitment_is_refused_and_the_first_still_ranks() {
    let data = market(1);
    let (reference, digest) = image("committed");
    let entrant = format!("sandbox:{reference}");

    let dir = temp_dir("differ");
    let (mut arena, dataset) = committed(&dir, &data, &[("alpha", &digest)], None);
    let scores = arena
        .reveal_and_score(
            WINDOW,
            &dataset,
            &[
                reveal("alpha", &digest, capture(&entrant, &data, momentum)),
                reveal("alpha", &digest, capture(&entrant, &data, oracle(&data))),
            ],
        )
        .unwrap();
    assert_eq!(scores.len(), 1);
    assert_eq!(scores[0].agent_id, "alpha");
    assert_eq!(
        refusals(&arena, "alpha"),
        ["commitment already revealed; one commitment opens once"]
    );
    let _ = std::fs::remove_dir_all(&dir);

    let dir = temp_dir("identical");
    let (mut arena, dataset) = committed(&dir, &data, &[("alpha", &digest)], None);
    let copy = reveal("alpha", &digest, capture(&entrant, &data, momentum));
    let scores = arena
        .reveal_and_score(WINDOW, &dataset, &[copy.clone(), copy])
        .unwrap();
    assert_eq!(scores.len(), 1);
    assert_eq!(
        refusals(&arena, "alpha"),
        ["commitment already revealed; one commitment opens once"]
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// F1-6: an image reference the sandbox would not launch is not ranked.
#[test]
fn an_option_like_image_reference_is_refused() {
    let dir = temp_dir("option-like");
    let data = market(1);
    let (_, digest) = image("committed");
    let (mut arena, dataset) = committed(&dir, &data, &[("alpha", &digest)], None);
    let entry = reveal(
        "alpha",
        &digest,
        capture(
            &format!("sandbox:--privileged@sha256:{digest}"),
            &data,
            momentum,
        ),
    );
    let scores = arena.reveal_and_score(WINDOW, &dataset, &[entry]).unwrap();
    assert!(scores.is_empty());
    assert!(
        refusals(&arena, "alpha")[0].contains("is not a launchable image"),
        "{:?}",
        refusals(&arena, "alpha")
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// F1-5: an entry that names no agent is recorded, and the rest are scored.
#[test]
fn an_unnamed_entry_is_recorded_and_the_rest_are_scored() {
    let dir = temp_dir("unnamed");
    let data = market(1);
    let (reference, digest) = image("committed");
    let entrant = format!("sandbox:{reference}");
    let (mut arena, dataset) = committed(&dir, &data, &[("alpha", &digest)], None);
    let unnamed = RevealedEntry {
        agent_id: None,
        ..reveal("alpha", &digest, capture(&entrant, &data, momentum))
    };
    let scores = arena
        .reveal_and_score(
            WINDOW,
            &dataset,
            &[
                unnamed,
                reveal("alpha", &digest, capture(&entrant, &data, momentum)),
            ],
        )
        .unwrap();
    assert_eq!(scores.len(), 1);
    assert_eq!(
        refusals(&arena, "(unnamed entry 0)"),
        ["the entry names no agent: an entry that reveals a capture must set `agent_id`"]
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// F1-9: a faulted window refuses every capture, so it does not need a
/// launcher that can run anything.
#[test]
fn a_faulted_window_does_not_ask_the_launcher_to_be_ready() {
    let dir = temp_dir("faulted-ready");
    let data = market(1);
    let plan = content_digest(b"fault plan");
    let (reference, digest) = image("committed");
    let (mut arena, dataset) = committed(&dir, &data, &[("alpha", &digest)], Some(&plan));
    let mut entry = reveal(
        "alpha",
        &digest,
        capture(&format!("sandbox:{reference}"), &data, momentum),
    );
    entry.fault_plan_sha256 = Some(plan);
    let mut launcher = Launcher {
        reference,
        down: true,
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
    assert!(scores.is_empty());
    assert!(refusals(&arena, "alpha")[0].starts_with("no capture path applies a fault plan"));
    let _ = std::fs::remove_dir_all(&dir);
}

/// A board that ranked nothing must not carry the mark that says its rows were
/// re-executed. `board_certifies` was vacuously true on an empty field, so a
/// window where every entry was refused signed `certifying: true` with zero
/// rows and `board.md` opened with no notice. Found from the SharpeArena side,
/// where every forward entry is refused under the default intake, which makes
/// the empty board the expected outcome of a documented workflow rather than a
/// corner case.
#[test]
fn a_board_that_ranked_nothing_does_not_certify() {
    let dir = temp_dir("ranked-nothing");
    let data = market(1);
    let (_, digest) = image("committed");
    let (mut arena, dataset) = committed(&dir, &data, &[("alpha", &digest)], None);
    // Supplied returns are refused under the default intake, so the field is
    // empty and every entry is on the refusals list.
    let entry = RevealedEntry {
        agent_id: Some("alpha".to_string()),
        submission: Some(sharpebench_core::AgentSubmission {
            agent_id: "alpha".to_string(),
            runs: Vec::new(),
            in_sample_trials: 0,
            candidates: Vec::new(),
        }),
        capture: None,
        artifact_digest: digest,
        salt: salt("alpha"),
        fault_plan_sha256: None,
    };
    let scores = arena.reveal_and_score(WINDOW, &dataset, &[entry]).unwrap();
    assert!(scores.is_empty());
    assert_eq!(refusals(&arena, "alpha").len(), 1);

    assert_eq!(arena.window(WINDOW).unwrap().certifying, Some(false));
    assert_eq!(window_json(&dir)["certifying"], false);
    arena.publish(WINDOW, &SigningKey::derive(b"k")).unwrap();
    assert_eq!(header_json(&dir)["certifying"], false);
    let md = board_md(&dir);
    assert!(
        md.contains("**Noncertifying board.**"),
        "a board with no rows must say so: {md}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}
