//! Forward-arena intake ranks returns the arena derives, not returns an entrant
//! hands over after the reveal: each entry reveals a strict capture that names
//! the committed artifact, the revealed dataset and the window's execution
//! matrix, and its returns are replayed from it. Re-execution then refuses a
//! capture whose decisions the committed entrant does not make.

use std::cell::Cell;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use sharpebench_arena::{
    execution_matrix, forward_hindsight_oracle_case, reference_entrant_digest, verify_arena, Arena,
    BoardRow, EntrantLauncher, IdentityField, IntakeOptions, LaunchedEntrant, ReplayWindow,
    ReturnsProvenance, RevealedEntry, SigningKey, WindowStatus, BOARD_FILE, BOARD_MD_FILE,
    FORWARD_HINDSIGHT_ORACLE, SUPPLIED_RETURNS_REFUSAL, WINDOWS_DIR, WINDOW_FILE,
};
use sharpebench_attest::{content_digest, make_commitment_under_fault_plan, PublicChain};
use sharpebench_core::{rank, ScoreConfig};
use sharpebench_protocol::{Action, AgentTrajectory, Decision, MarketObservation, Order};
use sharpebench_sim::{replay_submission, Agent, Dataset, Momentum, Window};

const WINDOW: &str = "w1";

fn temp_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "sharpebench-arena-bound-intake-{tag}-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn scorer() -> String {
    content_digest(b"bound-intake-scorer")
}

fn config() -> ScoreConfig {
    ScoreConfig {
        execution_seeds_per_window: 2,
        ..ScoreConfig::default()
    }
}

/// A revealed market with daily moves of a few percent.
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

/// `(reference, digest)` of a test image.
fn image(tag: &str) -> (String, String) {
    let digest = content_digest(format!("image-{tag}").as_bytes());
    (format!("test/{tag}@sha256:{digest}"), digest)
}

fn salt(agent_id: &str) -> String {
    format!("salt-{agent_id}")
}

/// Holds the revealed data and puts half its book into the symbol whose next
/// close rises most.
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

/// A capture of `make` naming `entrant`, over `windows` and `seeds` of `data`,
/// made by the test scorer.
fn capture_over(
    entrant: &str,
    data: &Dataset,
    windows: &[Window],
    seeds: &[u64],
    make: impl Fn() -> Box<dyn Agent>,
) -> AgentTrajectory {
    let (_, mut capture) = sharpebench_harness::run_agent_capture(
        entrant,
        data,
        windows,
        seeds,
        sharpebench_sim::CostModel::default(),
        make,
    );
    capture.contract.as_mut().unwrap().runner_artifact_sha256 = Some(scorer());
    capture
}

/// A capture over the forward window's own execution matrix for `data`.
fn capture(entrant: &str, data: &Dataset, make: impl Fn() -> Box<dyn Agent>) -> AgentTrajectory {
    let replay = ReplayWindow::parse(csv(data).as_bytes(), &config()).unwrap();
    capture_over(entrant, &replay.data, &replay.windows, &replay.seeds, make)
}

fn momentum() -> Box<dyn Agent> {
    Box::new(Momentum::default())
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

/// Open `w1` under `config()` and the test scorer (optionally under a fault
/// plan), commit each `(agent, artifact)`, advance to the reveal and write the
/// revealed `data`.
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

fn refusal(arena: &Arena, agent_id: &str) -> String {
    arena
        .window(WINDOW)
        .unwrap()
        .refusals
        .iter()
        .find(|r| r.agent_id == agent_id)
        .unwrap_or_else(|| panic!("`{agent_id}` was not refused"))
        .reason
        .clone()
}

fn provenance(arena: &Arena, agent_id: &str) -> Option<ReturnsProvenance> {
    arena
        .window(WINDOW)
        .unwrap()
        .returns_provenance
        .get(agent_id)
        .copied()
}

#[test]
fn the_forward_execution_matrix_is_the_one_capture_data_runs() {
    let (windows, seeds) = execution_matrix(160, &config()).unwrap();
    let windows: Vec<(usize, usize)> = windows.iter().map(|w| (w.start, w.end)).collect();
    assert_eq!(windows, [(16, 88), (88, 160)]);
    assert_eq!(seeds, [0, 1]);
    let (windows, seeds) = execution_matrix(60, &ScoreConfig::default()).unwrap();
    let windows: Vec<(usize, usize)> = windows.iter().map(|w| (w.start, w.end)).collect();
    assert_eq!(windows, [(10, 35), (35, 60)]);
    assert_eq!(seeds, [0]);
    let (windows, _) = execution_matrix(1000, &ScoreConfig::default()).unwrap();
    assert_eq!((windows[0].start, windows[0].end), (30, 515));
    assert!(execution_matrix(39, &ScoreConfig::default())
        .unwrap_err()
        .contains("at least 40"));
    assert!(execution_matrix(40, &ScoreConfig::default()).is_ok());
}

#[test]
fn a_valid_capture_is_scored_identically_to_the_direct_replay() {
    let dir = temp_dir("valid");
    let data = market(1);
    let (reference, digest) = image("alpha");
    let (mut arena, dataset) = committed(&dir, &data, &[("alpha", &digest)], None);
    let capture = capture(&format!("sandbox:{reference}"), &data, momentum);
    let entry = reveal("alpha", &digest, capture.clone());

    let scores = arena.reveal_and_score(WINDOW, &dataset, &[entry]).unwrap();

    let mut direct = replay_submission(&data, &capture, sharpebench_sim::CostModel::default());
    direct.agent_id = "alpha".to_string();
    let expected = rank(&[direct], &config());
    assert_eq!(
        serde_json::to_string(&scores).unwrap(),
        serde_json::to_string(&expected).unwrap(),
        "the arena ranks exactly the replay of the capture"
    );
    let w = arena.window(WINDOW).unwrap();
    assert_eq!(w.status, WindowStatus::Scoring);
    assert!(w.refusals.is_empty(), "{:?}", w.refusals);
    assert_eq!(
        provenance(&arena, "alpha"),
        Some(ReturnsProvenance::Replayed)
    );
    assert_eq!(
        w.replay_dataset_sha256.as_deref(),
        Some(capture.contract.as_ref().unwrap().dataset_sha256.as_str())
    );
    assert!(!w.supplied_returns_accepted);
    // Nothing re-executed the capture, so the board does not certify it.
    assert_eq!(w.certifying, Some(false));

    let board_path = arena.publish(WINDOW, &SigningKey::derive(b"k")).unwrap();
    let board: PublicChain = serde_json::from_slice(&std::fs::read(&board_path).unwrap()).unwrap();
    let header: serde_json::Value = serde_json::from_str(&board.chain[0].payload).unwrap();
    assert_eq!(
        header["replay_dataset_sha256"],
        capture.contract.as_ref().unwrap().dataset_sha256.as_str()
    );
    assert!(
        header.get("supplied_returns_accepted").is_none(),
        "{header}"
    );
    assert_eq!(header["certifying"], false, "{header}");
    let row: BoardRow = serde_json::from_str(&board.chain[1].payload).unwrap();
    assert_eq!(row.score, expected[0]);
    assert_eq!(row.returns_provenance, Some(ReturnsProvenance::Replayed));
    let md = std::fs::read_to_string(board_path.with_file_name(BOARD_MD_FILE)).unwrap();
    assert!(md.contains("- `alpha`: replayed"), "{md}");
    assert!(md.contains("**Noncertifying board.**"), "{md}");
    assert!(md.contains("A row marked `replayed`"), "{md}");
    assert!(verify_arena(&dir, None).unwrap().ok);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_board_row_without_provenance_has_the_bytes_of_the_score() {
    let data = market(1);
    let mut submission = replay_submission(
        &data,
        &capture("momentum", &data, momentum),
        sharpebench_sim::CostModel::default(),
    );
    submission.agent_id = "alpha".to_string();
    let score = rank(&[submission], &config()).remove(0);
    let row = BoardRow {
        score: score.clone(),
        returns_provenance: None,
    };
    assert_eq!(
        serde_json::to_string(&row).unwrap(),
        serde_json::to_string(&score).unwrap()
    );
    let tagged = BoardRow {
        returns_provenance: Some(ReturnsProvenance::ReExecuted),
        ..row
    };
    let json = serde_json::to_string(&tagged).unwrap();
    assert!(
        json.ends_with(r#","returns_provenance":"re-executed"}"#),
        "{json}"
    );
    assert_eq!(serde_json::from_str::<BoardRow>(&json).unwrap(), tagged);
}

#[test]
fn a_capture_whose_artifact_differs_from_the_commitment_is_refused() {
    let dir = temp_dir("artifact");
    let data = market(1);
    let (committed_ref, committed_digest) = image("committed");
    let (other_ref, other_digest) = image("other");
    let entrants = [
        ("swapped", committed_digest.as_str()),
        ("honest", committed_digest.as_str()),
        ("cmd", committed_digest.as_str()),
        ("unpinned", committed_digest.as_str()),
        ("reference", committed_digest.as_str()),
    ];
    let (mut arena, dataset) = committed(&dir, &data, &entrants, None);
    let entries = [
        // A valid commitment and reveal, but the capture records another image.
        reveal(
            "swapped",
            &committed_digest,
            capture(&format!("sandbox:{other_ref}"), &data, momentum),
        ),
        reveal(
            "honest",
            &committed_digest,
            capture(&format!("sandbox:{committed_ref}"), &data, momentum),
        ),
        // A command line does not identify the bytes behind it.
        reveal(
            "cmd",
            &committed_digest,
            capture("cmd:python agent.py", &data, momentum),
        ),
        reveal(
            "unpinned",
            &committed_digest,
            capture("sandbox:test/committed:latest", &data, momentum),
        ),
        // A reference agent's artifact is the runner, not the committed image.
        reveal(
            "reference",
            &committed_digest,
            capture("momentum", &data, momentum),
        ),
    ];
    let scores = arena.reveal_and_score(WINDOW, &dataset, &entries).unwrap();
    assert_eq!(scores.len(), 1);
    assert_eq!(scores[0].agent_id, "honest");
    assert_eq!(
        refusal(&arena, "swapped"),
        format!(
            "the capture records entrant artifact {other_digest}, but the committed artifact digest is {committed_digest}"
        )
    );
    assert!(refusal(&arena, "cmd").contains("`cmd:python agent.py` names no artifact"));
    assert!(refusal(&arena, "unpinned").contains("is not a digest-pinned image"));
    assert_eq!(
        refusal(&arena, "reference"),
        format!(
            "the capture records entrant artifact {}, but the committed artifact digest is {committed_digest}",
            reference_entrant_digest("momentum", &scorer())
        )
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_capture_over_a_different_dataset_is_refused() {
    let dir = temp_dir("dataset");
    let revealed = market(1);
    let other = market(2);
    let (reference, digest) = image("alpha");
    let (mut arena, dataset) = committed(
        &dir,
        &revealed,
        &[("stale", &digest), ("current", &digest)],
        None,
    );
    let stale = capture(&format!("sandbox:{reference}"), &other, momentum);
    let stale_dataset = stale.contract.as_ref().unwrap().dataset_sha256.clone();
    let current = capture(&format!("sandbox:{reference}"), &revealed, momentum);
    let revealed_dataset = current.contract.as_ref().unwrap().dataset_sha256.clone();
    assert_ne!(stale_dataset, revealed_dataset);

    let scores = arena
        .reveal_and_score(
            WINDOW,
            &dataset,
            &[
                reveal("stale", &digest, stale),
                reveal("current", &digest, current),
            ],
        )
        .unwrap();
    assert_eq!(scores.len(), 1);
    assert_eq!(scores[0].agent_id, "current");
    assert_eq!(
        refusal(&arena, "stale"),
        format!(
            "the capture was made over dataset {stale_dataset}, but the window revealed dataset {revealed_dataset}"
        )
    );
    assert_eq!(
        arena
            .window(WINDOW)
            .unwrap()
            .replay_dataset_sha256
            .as_deref(),
        Some(revealed_dataset.as_str())
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_capture_must_run_the_forward_execution_matrix_under_the_frozen_scorer() {
    let dir = temp_dir("matrix");
    let data = market(1);
    let (reference, digest) = image("alpha");
    let entrant = format!("sandbox:{reference}");
    let agents = ["slice", "seeds", "runner", "unbound"];
    let entrants: Vec<(&str, &str)> = agents.iter().map(|a| (*a, digest.as_str())).collect();
    let (mut arena, dataset) = committed(&dir, &data, &entrants, None);

    // Only the favorable second half of the revealed data.
    let slice = capture_over(
        &entrant,
        &data,
        &[Window {
            start: 88,
            end: 160,
        }],
        &[0, 1],
        momentum,
    );
    let seeds = capture_over(
        &entrant,
        &data,
        &[
            Window { start: 16, end: 88 },
            Window {
                start: 88,
                end: 160,
            },
        ],
        &[0],
        momentum,
    );
    let mut runner = capture(&entrant, &data, momentum);
    let other_runner = content_digest(b"another binary");
    runner.contract.as_mut().unwrap().runner_artifact_sha256 = Some(other_runner.clone());
    let mut unbound = capture(&entrant, &data, momentum);
    unbound.contract = None;

    let scores = arena
        .reveal_and_score(
            WINDOW,
            &dataset,
            &[
                reveal("slice", &digest, slice),
                reveal("seeds", &digest, seeds),
                reveal("runner", &digest, runner),
                reveal("unbound", &digest, unbound),
            ],
        )
        .unwrap();
    assert!(scores.is_empty(), "{scores:?}");
    assert_eq!(
        refusal(&arena, "slice"),
        "the capture runs market windows [(88, 160)], but the forward window scores [(16, 88), (88, 160)]"
    );
    assert_eq!(
        refusal(&arena, "seeds"),
        "the capture runs execution seeds [0], but the forward window scores [0, 1]"
    );
    assert_eq!(
        refusal(&arena, "runner"),
        format!(
            "trajectory runner {other_runner} does not match verifier runner {}",
            scorer()
        )
    );
    assert_eq!(
        refusal(&arena, "unbound"),
        "the capture has no execution contract"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// What a fake container does. Every launch runs momentum.
#[derive(Clone, Copy, Default)]
struct Script {
    not_ready: bool,
    refuse_admit: bool,
    refuse_launch: bool,
    transport_fault: bool,
    finish_error: bool,
}

struct FakeLauncher {
    reference: String,
    script: Script,
    launches: Rc<Cell<usize>>,
}

impl FakeLauncher {
    fn new(reference: &str, script: Script) -> Self {
        Self {
            reference: reference.to_string(),
            script,
            launches: Rc::new(Cell::new(0)),
        }
    }
}

struct FakeInstance {
    inner: Momentum,
    script: Script,
    decided: bool,
}

impl Agent for FakeInstance {
    fn decide(&mut self, observation: &MarketObservation) -> Decision {
        self.decided = true;
        self.inner.decide(observation)
    }
}

impl LaunchedEntrant for FakeInstance {
    fn fault(&self) -> Option<String> {
        (self.script.transport_fault && self.decided).then(|| "TransportError".to_string())
    }

    fn finish(self: Box<Self>) -> Result<(), String> {
        if self.script.finish_error {
            Err("the container breached its memory budget".to_string())
        } else {
            Ok(())
        }
    }
}

impl EntrantLauncher for FakeLauncher {
    fn ready(&mut self) -> Result<(), String> {
        if self.script.not_ready {
            Err("no container runtime".to_string())
        } else {
            Ok(())
        }
    }

    fn admit(&mut self, image: &str) -> Result<(), String> {
        if self.script.refuse_admit || image != self.reference {
            Err(format!("image `{image}` is not available"))
        } else {
            Ok(())
        }
    }

    fn launch(&mut self, _image: &str) -> Result<Box<dyn LaunchedEntrant>, String> {
        self.launches.set(self.launches.get() + 1);
        if self.script.refuse_launch {
            return Err("the daemon refused to start it".to_string());
        }
        Ok(Box::new(FakeInstance {
            inner: Momentum::default(),
            script: self.script,
            decided: false,
        }))
    }
}

#[test]
fn reexecution_refuses_hindsight_decisions_and_ranks_the_committed_entrant() {
    let dir = temp_dir("reexecute");
    let data = market(1);
    let (reference, digest) = image("committed");
    let entrant = format!("sandbox:{reference}");
    let momentum_digest = reference_entrant_digest("momentum", &scorer());
    let buy_and_hold_digest = reference_entrant_digest("buy-and-hold", &scorer());
    let entrants = [
        ("oracle", digest.as_str()),
        ("honest", digest.as_str()),
        ("reference", momentum_digest.as_str()),
        ("forged-reference", buy_and_hold_digest.as_str()),
    ];
    let (mut arena, dataset) = committed(&dir, &data, &entrants, None);

    let oracle = capture(&entrant, &data, || Box::new(Oracle { data: data.clone() }));
    let honest = capture(&entrant, &data, momentum);
    // A reference agent compiled into the scorer re-executes in process.
    let reference_capture = capture("momentum", &data, momentum);
    // A capture that says `buy-and-hold` but records momentum's decisions.
    let forged_reference = capture("buy-and-hold", &data, momentum);
    let entries = [
        reveal("oracle", &digest, oracle),
        reveal("honest", &digest, honest),
        reveal("reference", &momentum_digest, reference_capture),
        reveal("forged-reference", &buy_and_hold_digest, forged_reference),
    ];

    // Replay alone ranks the oracle's capture: its returns do follow from its
    // recorded decisions. That is the boundary re-execution exists for.
    let replayed_dir = temp_dir("reexecute-replay-only");
    let (mut replay_only, replay_dataset) = committed(&replayed_dir, &data, &entrants, None);
    replay_only
        .reveal_and_score(WINDOW, &replay_dataset, &entries)
        .unwrap();
    assert_eq!(
        provenance(&replay_only, "oracle"),
        Some(ReturnsProvenance::Replayed)
    );
    assert_eq!(replay_only.window(WINDOW).unwrap().certifying, Some(false));
    let _ = std::fs::remove_dir_all(&replayed_dir);

    let mut launcher = FakeLauncher::new(&reference, Script::default());
    let launches = Rc::clone(&launcher.launches);
    let scores = arena
        .reveal_and_score_with(
            WINDOW,
            &dataset,
            &entries,
            IntakeOptions {
                allow_supplied_returns: false,
                reexecute: Some(&mut launcher),
            },
        )
        .unwrap();
    let ranked: Vec<&str> = scores.iter().map(|s| s.agent_id.as_str()).collect();
    assert_eq!(ranked.len(), 2, "{ranked:?}");
    assert!(ranked.contains(&"honest") && ranked.contains(&"reference"));
    assert_eq!(arena.window(WINDOW).unwrap().certifying, Some(true));
    assert_eq!(
        provenance(&arena, "honest"),
        Some(ReturnsProvenance::ReExecuted)
    );
    assert_eq!(
        provenance(&arena, "reference"),
        Some(ReturnsProvenance::ReExecuted)
    );
    assert!(
        refusal(&arena, "oracle")
            .starts_with("re-execution diverged from the capture at run 0 step 0"),
        "{}",
        refusal(&arena, "oracle")
    );
    assert!(refusal(&arena, "forged-reference").starts_with("re-execution diverged"));
    // A fresh container per re-executed run: the oracle stops at its first
    // run, the honest image runs all four.
    assert_eq!(launches.get(), 1 + 4);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_reexecution_that_cannot_run_the_committed_image_is_a_refusal() {
    let data = market(1);
    let (reference, digest) = image("committed");
    let entrant = format!("sandbox:{reference}");
    let cases: [(&str, Script, &str); 4] = [
        (
            "admit",
            Script {
                refuse_admit: true,
                ..Script::default()
            },
            "cannot be re-executed: image",
        ),
        (
            "launch",
            Script {
                refuse_launch: true,
                ..Script::default()
            },
            "failed: an instance did not start: the daemon refused to start it",
        ),
        (
            "transport",
            Script {
                transport_fault: true,
                ..Script::default()
            },
            "failed: TransportError",
        ),
        (
            "finish",
            Script {
                finish_error: true,
                ..Script::default()
            },
            "failed: the container breached its memory budget",
        ),
    ];
    for (tag, script, expected) in cases {
        let dir = temp_dir(&format!("reexecute-{tag}"));
        let (mut arena, dataset) = committed(&dir, &data, &[("alpha", &digest)], None);
        let mut launcher = FakeLauncher::new(&reference, script);
        let launches = Rc::clone(&launcher.launches);
        let scores = arena
            .reveal_and_score_with(
                WINDOW,
                &dataset,
                &[reveal("alpha", &digest, capture(&entrant, &data, momentum))],
                IntakeOptions {
                    allow_supplied_returns: false,
                    reexecute: Some(&mut launcher),
                },
            )
            .unwrap();
        assert!(scores.is_empty(), "{tag}");
        let reason = refusal(&arena, "alpha");
        assert!(reason.contains(expected), "{tag}: {reason}");
        // Once an instance fails, no further container is started: the next
        // run holds and diverges at its first step.
        let expected_launches = usize::from(tag != "admit");
        assert_eq!(launches.get(), expected_launches, "{tag}");
        let _ = std::fs::remove_dir_all(&dir);
    }
}

#[test]
fn a_launcher_that_cannot_run_anything_is_an_error_and_records_nothing() {
    let dir = temp_dir("not-ready");
    let data = market(1);
    let (reference, digest) = image("committed");
    let (mut arena, dataset) = committed(&dir, &data, &[("alpha", &digest)], None);
    let before = std::fs::read(dir.join(WINDOWS_DIR).join(WINDOW).join(WINDOW_FILE)).unwrap();
    let mut launcher = FakeLauncher::new(
        &reference,
        Script {
            not_ready: true,
            ..Script::default()
        },
    );
    let error = arena
        .reveal_and_score_with(
            WINDOW,
            &dataset,
            &[reveal(
                "alpha",
                &digest,
                capture(&format!("sandbox:{reference}"), &data, momentum),
            )],
            IntakeOptions {
                allow_supplied_returns: false,
                reexecute: Some(&mut launcher),
            },
        )
        .unwrap_err();
    assert_eq!(error, "no container runtime");
    assert_eq!(
        std::fs::read(dir.join(WINDOWS_DIR).join(WINDOW).join(WINDOW_FILE)).unwrap(),
        before
    );
    assert_eq!(
        arena.window(WINDOW).unwrap().status,
        WindowStatus::Committed
    );

    // A reference entrant needs no launcher to be ready.
    let reference_capture = capture("momentum", &data, momentum);
    let dir2 = temp_dir("not-ready-reference");
    let committed_momentum = reference_entrant_digest("momentum", &scorer());
    let (mut arena, dataset) = committed(&dir2, &data, &[("alpha", &committed_momentum)], None);
    let scores = arena
        .reveal_and_score_with(
            WINDOW,
            &dataset,
            &[reveal("alpha", &committed_momentum, reference_capture)],
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
    let _ = std::fs::remove_dir_all(&dir);
    let _ = std::fs::remove_dir_all(&dir2);
}

#[test]
fn a_faulted_window_refuses_captures_and_ranks_supplied_returns_only_as_noncertifying() {
    let dir = temp_dir("faulted");
    let data = market(1);
    let plan = content_digest(b"fault plan");
    let (reference, digest) = image("committed");
    let (mut arena, dataset) = committed(
        &dir,
        &data,
        &[("captured", &digest), ("supplied", &digest)],
        Some(&plan),
    );
    let mut captured = reveal(
        "captured",
        &digest,
        capture(&format!("sandbox:{reference}"), &data, momentum),
    );
    captured.fault_plan_sha256 = Some(plan.clone());
    let mut returns = replay_submission(
        &data,
        &capture("momentum", &data, momentum),
        sharpebench_sim::CostModel::default(),
    );
    returns.agent_id = "supplied".to_string();
    let supplied = RevealedEntry {
        agent_id: None,
        submission: Some(returns),
        capture: None,
        artifact_digest: digest.clone(),
        salt: salt("supplied"),
        fault_plan_sha256: Some(plan.clone()),
    };
    let scores = arena
        .reveal_and_score_with(
            WINDOW,
            &dataset,
            &[captured, supplied],
            IntakeOptions {
                allow_supplied_returns: true,
                reexecute: None,
            },
        )
        .unwrap();
    assert_eq!(scores.len(), 1);
    assert_eq!(scores[0].agent_id, "supplied");
    assert!(refusal(&arena, "captured").starts_with("no capture path applies a fault plan"));
    assert_eq!(
        provenance(&arena, "supplied"),
        Some(ReturnsProvenance::Supplied)
    );
    assert!(arena.window(WINDOW).unwrap().supplied_returns_accepted);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn entries_that_cannot_be_ranked_are_refused_and_recorded() {
    let dir = temp_dir("shapes");
    let data = market(1);
    let (reference, digest) = image("committed");
    let entrant = format!("sandbox:{reference}");
    let agents = ["both", "neither", "renamed", "twice", "supplied"];
    let entrants: Vec<(&str, &str)> = agents.iter().map(|a| (*a, digest.as_str())).collect();
    let (mut arena, dataset) = committed(&dir, &data, &entrants, None);
    let returns = |agent_id: &str| {
        let mut s = replay_submission(
            &data,
            &capture("momentum", &data, momentum),
            sharpebench_sim::CostModel::default(),
        );
        s.agent_id = agent_id.to_string();
        s
    };
    let mut both = reveal("both", &digest, capture(&entrant, &data, momentum));
    both.submission = Some(returns("both"));
    let neither = RevealedEntry {
        capture: None,
        ..reveal("neither", &digest, capture(&entrant, &data, momentum))
    };
    let renamed = RevealedEntry {
        agent_id: Some("renamed".to_string()),
        submission: Some(returns("someone-else")),
        capture: None,
        artifact_digest: digest.clone(),
        salt: salt("renamed"),
        fault_plan_sha256: None,
    };
    let twice = reveal("twice", &digest, capture(&entrant, &data, momentum));
    let supplied = RevealedEntry {
        agent_id: None,
        submission: Some(returns("supplied")),
        capture: None,
        artifact_digest: digest.clone(),
        salt: salt("supplied"),
        fault_plan_sha256: None,
    };
    let scores = arena
        .reveal_and_score(
            WINDOW,
            &dataset,
            &[both, neither, renamed, twice.clone(), twice, supplied],
        )
        .unwrap();
    // The two identical copies of `twice` rank once.
    let ranked: Vec<&str> = scores.iter().map(|s| s.agent_id.as_str()).collect();
    assert_eq!(ranked, ["twice"]);
    let w = arena.window(WINDOW).unwrap();
    assert_eq!(w.refusals.len(), 5, "{:?}", w.refusals);
    assert_eq!(w.returns_provenance.len(), 1);
    assert!(refusal(&arena, "both").contains("both supplied returns and a capture"));
    assert_eq!(
        refusal(&arena, "neither"),
        "the entry carries neither a capture nor returns"
    );
    assert_eq!(
        refusal(&arena, "renamed"),
        "the entry names agent `renamed` but its supplied submission names `someone-else`"
    );
    assert_eq!(
        refusal(&arena, "twice"),
        "an identical copy of this agent's admitted entry; ranked once"
    );
    assert_eq!(refusal(&arena, "supplied"), SUPPLIED_RETURNS_REFUSAL);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn an_unreplayable_dataset_records_nothing() {
    let data = market(1);
    let (reference, digest) = image("committed");
    let entrant = format!("sandbox:{reference}");

    let dir = temp_dir("unreplayable");
    let (mut arena, dataset) = committed(&dir, &data, &[("alpha", &digest)], None);
    let window_file = dir.join(WINDOWS_DIR).join(WINDOW).join(WINDOW_FILE);
    let before = std::fs::read(&window_file).unwrap();
    // A dataset the simulator cannot parse cannot be replayed at all.
    std::fs::write(&dataset, "sym,close\nA,1.0\nA,1.01\n").unwrap();
    let error = arena
        .reveal_and_score(
            WINDOW,
            &dataset,
            &[reveal("alpha", &digest, capture(&entrant, &data, momentum))],
        )
        .unwrap_err();
    assert!(
        error.starts_with("the revealed dataset cannot be replayed"),
        "{error}"
    );
    assert_eq!(std::fs::read(&window_file).unwrap(), before);
    assert_eq!(
        arena.window(WINDOW).unwrap().status,
        WindowStatus::Committed
    );
    let _ = std::fs::remove_dir_all(&dir);
}

type WindowEdit = (
    IdentityField,
    fn(&mut serde_json::Map<String, serde_json::Value>),
);

#[test]
fn verify_cross_checks_the_intake_identity() {
    let data = market(1);
    let (reference, digest) = image("committed");
    let edits: [WindowEdit; 5] = [
        (IdentityField::ReplayDatasetSha256, |w| {
            w.insert(
                "replay_dataset_sha256".into(),
                serde_json::json!(content_digest(b"another parsed dataset")),
            );
        }),
        (IdentityField::ReplayDatasetSha256, |w| {
            w.remove("replay_dataset_sha256");
        }),
        (IdentityField::SuppliedReturnsAccepted, |w| {
            w.insert("supplied_returns_accepted".into(), serde_json::json!(true));
        }),
        (IdentityField::Certifying, |w| {
            w.insert("certifying".into(), serde_json::json!(true));
        }),
        (IdentityField::Certifying, |w| {
            w.remove("certifying");
        }),
    ];
    for (index, (field, edit)) in edits.into_iter().enumerate() {
        let dir = temp_dir(&format!("verify-{index}"));
        let (mut arena, dataset) = committed(&dir, &data, &[("alpha", &digest)], None);
        arena
            .reveal_and_score(
                WINDOW,
                &dataset,
                &[reveal(
                    "alpha",
                    &digest,
                    capture(&format!("sandbox:{reference}"), &data, momentum),
                )],
            )
            .unwrap();
        arena.publish(WINDOW, &SigningKey::derive(b"k")).unwrap();
        assert!(dir.join(WINDOWS_DIR).join(WINDOW).join(BOARD_FILE).exists());
        assert!(verify_arena(&dir, None).unwrap().ok);

        let path = dir.join(WINDOWS_DIR).join(WINDOW).join(WINDOW_FILE);
        let mut value: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        edit(value.as_object_mut().unwrap());
        std::fs::write(&path, serde_json::to_string_pretty(&value).unwrap()).unwrap();
        let report = verify_arena(&dir, None).unwrap();
        assert!(!report.ok, "{index}");
        let fields: Vec<_> = report.windows[0]
            .identity_mismatches
            .iter()
            .map(|m| m.field)
            .collect();
        assert_eq!(fields, [field], "{index}");
        let _ = std::fs::remove_dir_all(&dir);
    }
}

/// The tenth self-audit attack: a next-bar oracle delivered through the arena.
#[test]
fn the_forward_hindsight_oracle_case_is_refused() {
    let case = forward_hindsight_oracle_case();
    assert_eq!(case.name, FORWARD_HINDSIGHT_ORACLE);
    assert!(!case.expected_vulnerable);
    assert!(case.defended, "{}", case.detail);
    // The case shows the statistics admit the oracle, so the defense is intake.
    assert!(
        case.detail
            .starts_with("oracle returns ranked directly: DSR 1.000, eligible true"),
        "{}",
        case.detail
    );
    // Under the default intake the capture is refused, and the board certifies.
    assert!(
        case.detail
            .contains("its capture naming the committed image is refused (re-execution diverged"),
        "{}",
        case.detail
    );
    assert!(
        case.detail
            .contains("under the default re-executing intake, whose board certifies: true"),
        "{}",
        case.detail
    );
    // Replay alone ranks it, and that board says it certifies nothing.
    assert!(
        case.detail.contains(
            "the replay-only intake has it ranked as replayed on a board that certifies: false"
        ),
        "{}",
        case.detail
    );
    assert!(case
        .detail
        .ends_with("the committed image's own capture is ranked as re-executed; oracle on a certifying board: false"));
}
