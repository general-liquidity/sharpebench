//! The forward evidence ledger: what a scored window records, how the ledger
//! folds a window, how the signed headers carry it from board to board, and
//! what `verify_arena` refuses. Boards signed before the ledger existed must
//! still verify.

use std::path::{Path, PathBuf};

use serde_json::Value;

use sharpebench_arena::{
    fold_window, head_digest, reference_entrant_digest, verify_arena, Arena, ArenaHead,
    EntrantLauncher, ForwardLedger, ForwardObservation, ForwardRecord, IdentityField,
    IntakeOptions, LaunchedEntrant, ReturnsProvenance, RevealedEntry, SigningKey, ARENA_HEAD_KIND,
    BOARD_FILE, BOARD_MD_FILE, EXECUTED_WINDOW_SCHEMA_VERSION, FORWARD_EVIDENCE_LEVEL,
    FORWARD_EVIDENCE_SCHEMA, FORWARD_WINDOW_SCHEMA_VERSION, HEAD_FILE, WINDOWS_DIR, WINDOW_FILE,
};
use sharpebench_attest::{
    content_digest, make_commitment, publish_public_chain, Commitment, PublicChain,
};
use sharpebench_core::stats::mean;
use sharpebench_core::ScoreConfig;
use sharpebench_protocol::{AgentTrajectory, DeclaredMandate};
use sharpebench_sim::{Agent, BuyAndHold, Dataset, Momentum};
use sharpebench_stats::forward_eprocess::{BettorState, DEFAULT_LOSS_BOUND};

// ------------------------------------------------------------ fold_window

fn digest(tag: &str) -> String {
    content_digest(tag.as_bytes())
}

fn commitment(agent_id: &str, window: &str) -> Commitment {
    make_commitment(agent_id, window, &digest(agent_id), "salt")
}

fn observation(agent_id: &str, artifact: &str, returns: Vec<f64>) -> ForwardObservation {
    ForwardObservation {
        agent_id: agent_id.to_string(),
        artifact_digest: digest(artifact),
        declared_loss_bound: None,
        returns,
    }
}

fn record(first: &str, last: &str, entrants: Vec<ForwardObservation>) -> ForwardRecord {
    ForwardRecord {
        first_bar_date: first.to_string(),
        last_bar_date: last.to_string(),
        entrants,
    }
}

/// `bars` returns alternating between `up` and `down`.
fn track(bars: usize, up: f64, down: f64) -> Vec<f64> {
    (0..bars)
        .map(|i| if i % 2 == 0 { up } else { down })
        .collect()
}

fn bettor_over(returns: &[f64], bound: f64, mut state: BettorState) -> BettorState {
    for x in returns {
        state.step(*x, bound).unwrap();
    }
    state
}

#[test]
fn the_genesis_ledger_is_empty_and_declares_its_schema_and_level() {
    let genesis = ForwardLedger::genesis();
    assert_eq!(genesis.schema, FORWARD_EVIDENCE_SCHEMA);
    assert_eq!(genesis.level, FORWARD_EVIDENCE_LEVEL);
    assert_eq!(FORWARD_EVIDENCE_LEVEL, 0.05);
    assert_eq!((genesis.windows, genesis.family_size), (0, 0));
    assert!(genesis.artifacts.is_empty() && genesis.discoveries.is_empty());
}

#[test]
fn a_fold_runs_the_stats_bettor_over_each_entrant_and_counts_every_commitment() {
    let returns = track(60, 0.02, -0.01);
    let r = record(
        "2030-001",
        "2030-060",
        vec![observation("a", "art-a", returns.clone())],
    );
    let commitments = [commitment("a", "w1"), commitment("ghost", "w1")];
    let evidence = fold_window(&ForwardLedger::genesis(), "w1", &commitments, Some(&r)).unwrap();
    assert_eq!(evidence.schema, FORWARD_EVIDENCE_SCHEMA);
    assert_eq!(evidence.prior, ForwardLedger::genesis());
    assert_eq!(evidence.record.as_ref(), Some(&r));
    assert_eq!(evidence.commitments, commitments);
    assert!(evidence.not_applied.is_none());
    let ledger = &evidence.posterior;
    let a = &ledger.artifacts[&digest("art-a")];
    let expected = bettor_over(&returns, DEFAULT_LOSS_BOUND, BettorState::new());
    assert_eq!(
        (
            a.bars,
            a.sum,
            a.sum_sq,
            a.e_value,
            a.max_e_value,
            a.breached
        ),
        (
            expected.bars,
            expected.sum,
            expected.sum_sq,
            expected.wealth,
            expected.max_wealth,
            expected.breached
        )
    );
    assert!(a.e_value > 1.0, "a winning track grows its wealth");
    assert_eq!(a.anytime_p_value, 1.0 / a.max_e_value);
    assert_eq!((a.windows, a.last_window_id.as_str()), (1, "w1"));
    assert_eq!(a.loss_bound_in_force, DEFAULT_LOSS_BOUND);
    // The committed agent that produced no observation is a family member.
    assert_eq!(
        ledger.unresolved_commitments.iter().collect::<Vec<_>>(),
        [&commitments[1].commit_hash]
    );
    assert_eq!(ledger.family_size, 2);
    assert_eq!(
        (ledger.windows, ledger.last_window_id.as_deref()),
        (1, Some("w1"))
    );
    assert_eq!(ledger.last_bar_date.as_deref(), Some("2030-060"));
}

#[test]
fn a_window_without_observations_adds_its_commitments_and_moves_nothing_else() {
    let first = fold_window(
        &ForwardLedger::genesis(),
        "w1",
        &[commitment("a", "w1")],
        Some(&record(
            "2030-001",
            "2030-060",
            vec![observation("a", "art-a", track(40, 0.02, -0.01))],
        )),
    )
    .unwrap()
    .posterior;
    let second = fold_window(&first, "w2", &[commitment("b", "w2")], None).unwrap();
    let ledger = &second.posterior;
    assert_eq!(ledger.artifacts, first.artifacts);
    assert_eq!(ledger.last_bar_date, first.last_bar_date);
    assert_eq!(ledger.unresolved_commitments.len(), 1);
    assert_eq!(ledger.family_size, 2);
    assert_eq!(
        (ledger.windows, ledger.last_window_id.as_deref()),
        (2, Some("w2"))
    );
}

#[test]
fn processes_continue_across_windows_and_a_restarted_artifact_is_still_charged() {
    let w1 = track(60, 0.03, -0.01);
    let w2 = track(60, 0.03, -0.01);
    let first = fold_window(
        &ForwardLedger::genesis(),
        "w1",
        &[commitment("a", "w1")],
        Some(&record(
            "2030-001",
            "2030-060",
            vec![observation("a", "art-a", w1.clone())],
        )),
    )
    .unwrap()
    .posterior;
    // The operator abandons `art-a` and enters a fresh artifact under the same
    // agent id; the abandoned one stays in the family at its last wealth.
    let second = fold_window(
        &first,
        "w2",
        &[commitment("a", "w2")],
        Some(&record(
            "2030-061",
            "2030-120",
            vec![observation("a", "art-b", w2.clone())],
        )),
    )
    .unwrap()
    .posterior;
    assert_eq!(
        second.artifacts[&digest("art-a")],
        first.artifacts[&digest("art-a")]
    );
    assert_eq!(second.family_size, 2);
    let b = &second.artifacts[&digest("art-b")];
    assert_eq!(b.e_value, bettor_over(&w2, 1.0, BettorState::new()).wealth);
    // The same artifact entered again continues its process.
    let third = fold_window(
        &second,
        "w3",
        &[commitment("a", "w3")],
        Some(&record(
            "2030-121",
            "2030-180",
            vec![observation("a", "art-b", w1.clone())],
        )),
    )
    .unwrap()
    .posterior;
    let continued = bettor_over(&w1, 1.0, bettor_over(&w2, 1.0, BettorState::new()));
    let b = &third.artifacts[&digest("art-b")];
    assert_eq!((b.e_value, b.bars, b.windows), (continued.wealth, 120, 2));
    assert_eq!(third.family_size, 2);
}

#[test]
fn e_bh_decides_over_the_whole_family_so_restarts_raise_the_bar() {
    // One artifact with overwhelming evidence, alone, is a discovery.
    let strong = track(400, 0.05, 0.01);
    let alone = fold_window(
        &ForwardLedger::genesis(),
        "w1",
        &[commitment("a", "w1")],
        Some(&record(
            "2030-001",
            "2030-400",
            vec![observation("a", "art-a", strong.clone())],
        )),
    )
    .unwrap()
    .posterior;
    let e = alone.artifacts[&digest("art-a")].e_value;
    assert!(e >= 1.0 / FORWARD_EVIDENCE_LEVEL);
    assert_eq!(alone.discoveries, [digest("art-a")]);
    // The same wealth in a family large enough that K / (level k) exceeds it
    // is not: every committed artifact counts, evaluated or not.
    let members = (e * FORWARD_EVIDENCE_LEVEL).ceil() as usize + 1;
    let crowd: Vec<Commitment> = (0..members)
        .map(|i| commitment(&format!("c{i}"), "w1"))
        .collect();
    let mut commitments = vec![commitment("a", "w1")];
    commitments.extend(crowd);
    let crowded = fold_window(
        &ForwardLedger::genesis(),
        "w1",
        &commitments,
        Some(&record(
            "2030-001",
            "2030-400",
            vec![observation("a", "art-a", strong)],
        )),
    )
    .unwrap()
    .posterior;
    assert_eq!(crowded.artifacts[&digest("art-a")].e_value, e);
    assert_eq!(crowded.family_size as usize, members + 1);
    assert!(crowded.discoveries.is_empty());
}

#[test]
fn a_declared_bound_takes_effect_from_the_next_window_and_is_then_frozen() {
    let returns = track(60, 0.03, -0.01);
    let mut declared = observation("a", "art-a", returns.clone());
    declared.declared_loss_bound = Some(0.25);
    let first = fold_window(
        &ForwardLedger::genesis(),
        "w1",
        &[commitment("a", "w1")],
        Some(&record("2030-001", "2030-060", vec![declared])),
    )
    .unwrap()
    .posterior;
    let a = &first.artifacts[&digest("art-a")];
    // Window 1 was capped by the default; the declaration is recorded for later.
    assert_eq!(
        a.e_value,
        bettor_over(&returns, DEFAULT_LOSS_BOUND, BettorState::new()).wealth
    );
    assert_eq!(
        (a.declared_loss_bound, a.loss_bound_in_force),
        (Some(0.25), 0.25)
    );
    // Window 2 is capped by 0.25 although it declares 0.5.
    let mut redeclared = observation("a", "art-a", returns.clone());
    redeclared.declared_loss_bound = Some(0.5);
    let second = fold_window(
        &first,
        "w2",
        &[commitment("a", "w2")],
        Some(&record("2030-061", "2030-120", vec![redeclared])),
    )
    .unwrap()
    .posterior;
    let a2 = &second.artifacts[&digest("art-a")];
    let expected = bettor_over(
        &returns,
        0.25,
        bettor_over(&returns, 1.0, BettorState::new()),
    );
    assert_eq!(a2.e_value, expected.wealth);
    assert_ne!(
        a2.e_value,
        bettor_over(
            &returns,
            0.5,
            bettor_over(&returns, 1.0, BettorState::new())
        )
        .wealth
    );
    assert_eq!(a2.declared_loss_bound, Some(0.25));
}

#[test]
fn a_bar_below_the_bound_in_force_zeroes_the_artifact_for_good() {
    let mut declared = observation("a", "art-a", track(40, 0.03, -0.01));
    declared.declared_loss_bound = Some(0.2);
    let first = fold_window(
        &ForwardLedger::genesis(),
        "w1",
        &[commitment("a", "w1")],
        Some(&record("2030-001", "2030-040", vec![declared])),
    )
    .unwrap()
    .posterior;
    let mut crash = track(40, 0.03, -0.01);
    crash[5] = -0.3;
    let second = fold_window(
        &first,
        "w2",
        &[commitment("a", "w2")],
        Some(&record(
            "2030-041",
            "2030-080",
            vec![observation("a", "art-a", crash)],
        )),
    )
    .unwrap()
    .posterior;
    let a = &second.artifacts[&digest("art-a")];
    assert!(a.breached);
    assert_eq!(a.e_value, 0.0);
    assert!(a.max_e_value >= 1.0);
    // Under the default bound the same bar is inside it.
    let undeclared = fold_window(
        &ForwardLedger::genesis(),
        "w1",
        &[commitment("a", "w1")],
        Some(&record(
            "2030-001",
            "2030-040",
            vec![observation("a", "art-a", vec![-0.3; 3])],
        )),
    )
    .unwrap()
    .posterior;
    assert!(!undeclared.artifacts[&digest("art-a")].breached);
}

#[test]
fn a_window_whose_bars_overlap_the_ledger_is_not_applied() {
    let first = fold_window(
        &ForwardLedger::genesis(),
        "w1",
        &[commitment("a", "w1")],
        Some(&record(
            "2030-001",
            "2030-060",
            vec![observation("a", "art-a", track(60, 0.03, -0.01))],
        )),
    )
    .unwrap()
    .posterior;
    for first_date in ["2030-060", "2030-030"] {
        let evidence = fold_window(
            &first,
            "w2",
            &[commitment("a", "w2")],
            Some(&record(
                first_date,
                "2030-120",
                vec![observation("a", "art-a", track(60, 0.03, -0.01))],
            )),
        )
        .unwrap();
        let reason = evidence.not_applied.as_deref().unwrap();
        assert!(
            reason.contains(first_date) && reason.contains("2030-060"),
            "{reason}"
        );
        let ledger = &evidence.posterior;
        assert_eq!(ledger.artifacts, first.artifacts);
        assert_eq!(ledger.last_bar_date, first.last_bar_date);
        // Its commitment is unresolved, not lost.
        assert_eq!(ledger.family_size, 2);
    }
    // The first bar after the last observed one is applied.
    let next = fold_window(
        &first,
        "w2",
        &[commitment("a", "w2")],
        Some(&record(
            "2030-061",
            "2030-120",
            vec![observation("a", "art-a", track(60, 0.03, -0.01))],
        )),
    )
    .unwrap();
    assert!(next.not_applied.is_none());
    assert_eq!(next.posterior.artifacts[&digest("art-a")].bars, 120);
}

#[test]
fn a_malformed_record_or_prior_is_refused() {
    let good = || observation("a", "art-a", vec![0.01]);
    let g = ForwardLedger::genesis();
    let fails = |r: ForwardRecord| fold_window(&g, "w1", &[], Some(&r)).unwrap_err();
    assert!(fails(record("2030-001", "2030-002", vec![])).contains("no entrant"));
    assert!(fails(record("2030-002", "2030-001", vec![good()])).contains("dated after"));
    assert!(fails(record("x", "y", vec![good(), good()])).contains("strict agent id order"));
    let mut b = observation("b", "art-a", vec![0.01]);
    assert!(fails(record("x", "y", vec![good(), b.clone()])).contains("repeats artifact"));
    b.artifact_digest = "not-a-digest".to_string();
    assert!(fails(record("x", "y", vec![good(), b])).contains("SHA-256"));
    assert!(
        fails(record("x", "y", vec![observation("a", "art-a", vec![])]))
            .contains("no finite track")
    );
    assert!(fails(record(
        "x",
        "y",
        vec![observation("a", "art-a", vec![f64::NAN])]
    ))
    .contains("no finite track"));
    for bound in [0.0, 1.5, f64::NAN] {
        let mut o = good();
        o.declared_loss_bound = Some(bound);
        assert!(
            fails(record("x", "y", vec![o])).contains("declares bound"),
            "{bound}"
        );
    }
    // A prior this binary did not write.
    let mut prior = ForwardLedger::genesis();
    prior.schema = "other".to_string();
    assert!(fold_window(&prior, "w1", &[], None)
        .unwrap_err()
        .contains("schema"));
    let mut prior = ForwardLedger::genesis();
    prior.level = 0.1;
    assert!(fold_window(&prior, "w1", &[], None)
        .unwrap_err()
        .contains("level"));
    let mut prior = fold_window(&g, "w1", &[], Some(&record("x", "y", vec![good()])))
        .unwrap()
        .posterior;
    prior.artifacts.values_mut().next().unwrap().e_value = 2.0;
    assert!(fold_window(&prior, "w2", &[], None)
        .unwrap_err()
        .contains("bettor state"));
    let mut prior = fold_window(&g, "w1", &[], Some(&record("x", "y", vec![good()])))
        .unwrap()
        .posterior;
    prior
        .artifacts
        .values_mut()
        .next()
        .unwrap()
        .declared_loss_bound = Some(2.0);
    assert!(fold_window(&prior, "w2", &[], None)
        .unwrap_err()
        .contains("declares bound"));
}

#[test]
fn a_ledger_round_trips_through_json_bit_for_bit() {
    let evidence = fold_window(
        &ForwardLedger::genesis(),
        "w1",
        &[commitment("a", "w1"), commitment("b", "w1")],
        Some(&record(
            "2030-001",
            "2030-090",
            vec![observation(
                "a",
                "art-a",
                track(90, 0.0123456789, -0.00987654321),
            )],
        )),
    )
    .unwrap();
    let text = serde_json::to_string(&evidence).unwrap();
    let back: sharpebench_arena::ForwardEvidence = serde_json::from_str(&text).unwrap();
    assert_eq!(back, evidence);
    // And the read-back prior folds to the same posterior.
    let again = fold_window(&back.prior, "w1", &back.commitments, back.record.as_ref()).unwrap();
    assert_eq!(again, evidence);
    // Unknown keys are refused rather than dropped.
    let mut value: Value = serde_json::from_str(&text).unwrap();
    value["posterior"]["extra"] = Value::Bool(true);
    assert!(serde_json::from_value::<sharpebench_arena::ForwardEvidence>(value).is_err());
}

// ------------------------------------------------------------ the arena

fn temp_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "sharpebench-arena-forward-evidence-{tag}-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn scorer() -> String {
    content_digest(b"forward-evidence-scorer")
}

fn config() -> ScoreConfig {
    ScoreConfig {
        execution_seeds_per_window: 2,
        ..ScoreConfig::default()
    }
}

/// A market whose date labels start in `year`, so windows can be disjoint.
fn market(seed: u64, year: u32) -> Dataset {
    let mut data = Dataset::synthetic_parameterized(4, 160, seed, 3.0, 0.0, 0.0);
    data.dates = (0..data.dates.len())
        .map(|d| format!("{year}-{:03}", d + 1))
        .collect();
    data
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

fn capture(name: &str, data: &Dataset) -> AgentTrajectory {
    let replay = sharpebench_arena::ReplayWindow::parse(csv(data).as_bytes(), &config()).unwrap();
    let momentum = name == "momentum";
    let (_, mut capture) = sharpebench_harness::run_agent_capture(
        name,
        &replay.data,
        &replay.windows,
        &replay.seeds,
        sharpebench_sim::CostModel::default(),
        || {
            if momentum {
                Box::new(Momentum::default()) as Box<dyn Agent>
            } else {
                Box::new(BuyAndHold)
            }
        },
    );
    capture.contract.as_mut().unwrap().runner_artifact_sha256 = Some(scorer());
    capture
}

/// Re-executes the reference agents compiled into the scorer; every image
/// is unavailable.
struct ReferencesOnly;

impl EntrantLauncher for ReferencesOnly {
    fn admit(&mut self, image: &str) -> Result<(), String> {
        Err(format!("image `{image}` is not available"))
    }
    fn launch(&mut self, image: &str) -> Result<Box<dyn LaunchedEntrant>, String> {
        Err(format!("image `{image}` is not available"))
    }
}

/// An entrant of one window: its agent id, the reference agent it runs, and
/// whether it reveals.
struct Entrant {
    agent_id: &'static str,
    reference: &'static str,
    reveals: bool,
    mandate: Option<DeclaredMandate>,
}

fn entrant(agent_id: &'static str, reference: &'static str) -> Entrant {
    Entrant {
        agent_id,
        reference,
        reveals: true,
        mandate: None,
    }
}

/// Open, commit, score with re-execution, and publish window `id` (the
/// `index`-th) over `data`.
fn run_window(
    arena: &mut Arena,
    dir: &Path,
    index: u64,
    id: &str,
    data: &Dataset,
    entrants: &[Entrant],
    key: &SigningKey,
) {
    let base = index * 20;
    arena
        .open_window_with_provenance(id, base + 10, base + 20, config(), None, scorer())
        .unwrap();
    let mut entries = Vec::new();
    for e in entrants {
        let artifact = reference_entrant_digest(e.reference, &scorer());
        let salt = format!("salt-{}-{id}", e.agent_id);
        arena
            .register_entry(id, make_commitment(e.agent_id, id, &artifact, &salt))
            .unwrap();
        if e.reveals {
            let mut c = capture(e.reference, data);
            c.declared_mandate = e.mandate.clone();
            entries.push(RevealedEntry {
                agent_id: Some(e.agent_id.to_string()),
                submission: None,
                capture: Some(c),
                artifact_digest: artifact,
                salt,
                fault_plan_sha256: None,
            });
        }
    }
    arena.advance(base + 20).unwrap();
    let dataset = dir.join(format!("{id}.csv"));
    std::fs::write(&dataset, csv(data)).unwrap();
    let mut launcher = ReferencesOnly;
    arena
        .reveal_and_score_with(
            id,
            &dataset,
            &entries,
            IntakeOptions {
                allow_supplied_returns: false,
                reexecute: Some(&mut launcher),
            },
        )
        .unwrap();
    arena.publish(id, key).unwrap();
}

fn board(dir: &Path, id: &str) -> PublicChain {
    serde_json::from_slice(&std::fs::read(dir.join(WINDOWS_DIR).join(id).join(BOARD_FILE)).unwrap())
        .unwrap()
}

fn header(dir: &Path, id: &str) -> Value {
    serde_json::from_str(&board(dir, id).chain[0].payload).unwrap()
}

fn evidence(dir: &Path, id: &str) -> sharpebench_arena::ForwardEvidence {
    serde_json::from_value(header(dir, id)["forward_evidence"].clone()).unwrap()
}

/// Re-sign window `id`'s board with `header` as its first payload, and the
/// head over it when `id` is the newest of `published` windows.
fn resign(dir: &Path, id: &str, header: &Value, key: &SigningKey, published: u64) {
    let old = board(dir, id);
    let mut payloads = vec![serde_json::to_string(header).unwrap()];
    payloads.extend(old.chain.iter().skip(1).map(|link| link.payload.clone()));
    let board = publish_public_chain(&payloads, key);
    std::fs::write(
        dir.join(WINDOWS_DIR).join(id).join(BOARD_FILE),
        serde_json::to_vec(&board).unwrap(),
    )
    .unwrap();
    let head = ArenaHead {
        kind: ARENA_HEAD_KIND.to_string(),
        published_count: published,
        head_window_id: id.to_string(),
        head_digest: head_digest(&board.chain.last().unwrap().signature),
    };
    let document = publish_public_chain(&[serde_json::to_string(&head).unwrap()], key);
    std::fs::write(dir.join(HEAD_FILE), serde_json::to_vec(&document).unwrap()).unwrap();
}

#[test]
fn a_re_executed_window_records_its_tracks_and_signs_the_ledger_into_its_header() {
    let dir = temp_dir("record");
    let key = SigningKey::derive(b"forward-evidence-key");
    let mut arena = Arena::init(&dir).unwrap();
    let data = market(1, 2030);
    let silent = Entrant {
        reveals: false,
        ..entrant("silent", "buy-and-hold")
    };
    run_window(
        &mut arena,
        &dir,
        0,
        "w1",
        &data,
        &[
            entrant("mom", "momentum"),
            entrant("bah", "buy-and-hold"),
            silent,
        ],
        &key,
    );
    let w = arena.window("w1").unwrap();
    assert_eq!(w.schema_version, FORWARD_WINDOW_SCHEMA_VERSION);
    let record = w.forward.as_ref().expect("re-executed rows are recorded");
    let ids: Vec<&str> = record
        .entrants
        .iter()
        .map(|e| e.agent_id.as_str())
        .collect();
    assert_eq!(ids, ["bah", "mom"]);
    // Each observation is the pooled track the row was scored on.
    for e in &record.entrants {
        let score = w.scores.iter().find(|s| s.agent_id == e.agent_id).unwrap();
        assert_eq!(
            w.returns_provenance[&e.agent_id],
            ReturnsProvenance::ReExecuted
        );
        assert_eq!(e.returns.len(), score.pooled_observations);
        assert_eq!(mean(&e.returns), score.raw_mean_return);
        assert_eq!(
            e.artifact_digest,
            reference_entrant_digest(
                if e.agent_id == "mom" {
                    "momentum"
                } else {
                    "buy-and-hold"
                },
                &scorer()
            )
        );
    }
    // The first scored bar is the first bar of the execution matrix.
    let replay = sharpebench_arena::ReplayWindow::parse(csv(&data).as_bytes(), &config()).unwrap();
    assert_eq!(
        record.first_bar_date,
        replay.data.dates[replay.windows[0].start]
    );
    assert_eq!(record.last_bar_date, "2030-160");

    let signed = evidence(&dir, "w1");
    assert_eq!(signed.prior, ForwardLedger::genesis());
    assert_eq!(signed.record.as_ref(), Some(record));
    assert_eq!(signed.commitments, w.commitments);
    // The posterior is the ledger fold of the window file, recomputed here.
    assert_eq!(
        signed,
        fold_window(
            &ForwardLedger::genesis(),
            "w1",
            &w.commitments,
            Some(record)
        )
        .unwrap()
    );
    // The unrevealed commitment is in the family.
    assert_eq!(signed.posterior.family_size, 3);
    let report = verify_arena(&dir, None).unwrap();
    assert!(report.ok, "{report:?}");
    assert_eq!(report.forward_ledger.as_ref(), Some(&signed.posterior));
    let md = std::fs::read_to_string(dir.join(WINDOWS_DIR).join("w1").join(BOARD_MD_FILE)).unwrap();
    assert!(md.contains("## Forward evidence"), "{md}");
    assert!(
        md.contains("`mom` (artifact `") && md.contains("Family: 3 committed artifact(s)"),
        "{md}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn the_next_header_carries_the_running_e_values_and_verify_recomputes_them() {
    let dir = temp_dir("chain");
    let key = SigningKey::derive(b"forward-evidence-key");
    let mut arena = Arena::init(&dir).unwrap();
    let mut declared = entrant("mom", "momentum");
    declared.mandate = Some(DeclaredMandate::DrawdownCapped {
        max_per_run_drawdown: 0.5,
    });
    run_window(
        &mut arena,
        &dir,
        0,
        "w1",
        &market(1, 2030),
        &[declared],
        &key,
    );
    run_window(
        &mut arena,
        &dir,
        1,
        "w2",
        &market(2, 2031),
        &[entrant("mom", "momentum")],
        &key,
    );
    let first = evidence(&dir, "w1");
    let second = evidence(&dir, "w2");
    assert_eq!(second.prior, first.posterior);
    let digest = reference_entrant_digest("momentum", &scorer());
    let a1 = &first.posterior.artifacts[&digest];
    let a2 = &second.posterior.artifacts[&digest];
    assert_eq!(a1.loss_bound_in_force, 0.5);
    assert_eq!(a2.windows, 2);
    let w1 = &first.record.as_ref().unwrap().entrants[0].returns;
    let w2 = &second.record.as_ref().unwrap().entrants[0].returns;
    assert_eq!(a2.bars as usize, w1.len() + w2.len());
    let expected = bettor_over(
        w2,
        0.5,
        bettor_over(w1, DEFAULT_LOSS_BOUND, BettorState::new()),
    );
    assert_eq!(a2.e_value, expected.wealth);
    let report = verify_arena(&dir, None).unwrap();
    assert!(report.ok, "{report:?}");
    assert_eq!(report.forward_ledger.as_ref(), Some(&second.posterior));

    // A posterior that does not recompute fails its board, and only it.
    let mut forged = header(&dir, "w2");
    forged["forward_evidence"]["posterior"]["artifacts"][&digest]["e_value"] = Value::from(1e6);
    forged["forward_evidence"]["posterior"]["artifacts"][&digest]["max_e_value"] = Value::from(1e6);
    resign(&dir, "w2", &forged, &key, 2);
    let report = verify_arena(&dir, None).unwrap();
    assert!(!report.ok);
    assert!(report.windows[0].forward_evidence_error.is_none());
    let error = report.windows[1].forward_evidence_error.as_deref().unwrap();
    assert!(error.contains("does not recompute"), "{error}");
    assert!(
        report.windows[1].detail.contains("forward evidence: "),
        "{}",
        report.windows[1].detail
    );

    // A prior that is not the previous posterior.
    let mut forged = header(&dir, "w2");
    forged["forward_evidence"] = serde_json::to_value(
        fold_window(
            &ForwardLedger::genesis(),
            "w2",
            &second.commitments,
            second.record.as_ref(),
        )
        .unwrap(),
    )
    .unwrap();
    resign(&dir, "w2", &forged, &key, 2);
    let error = verify_arena(&dir, None).unwrap().windows[1]
        .forward_evidence_error
        .clone()
        .unwrap();
    assert!(
        error.contains("not the previous board's posterior"),
        "{error}"
    );

    // Dropping the ledger after it began.
    let mut forged = header(&dir, "w2");
    forged.as_object_mut().unwrap().remove("forward_evidence");
    resign(&dir, "w2", &forged, &key, 2);
    let report = verify_arena(&dir, None).unwrap();
    let error = report.windows[1].forward_evidence_error.as_deref().unwrap();
    assert!(
        error.contains("an earlier board began the ledger"),
        "{error}"
    );
    // The window file still records observations the header no longer signs.
    assert!(report.windows[1]
        .identity_mismatches
        .iter()
        .any(|m| m.field == IdentityField::ForwardRecord));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn observations_and_commitments_must_be_the_ones_the_window_records() {
    let dir = temp_dir("identity");
    let key = SigningKey::derive(b"forward-evidence-key");
    let mut arena = Arena::init(&dir).unwrap();
    run_window(
        &mut arena,
        &dir,
        0,
        "w1",
        &market(1, 2030),
        &[entrant("mom", "momentum"), entrant("bah", "buy-and-hold")],
        &key,
    );
    let original = header(&dir, "w1");

    // Signed observations that differ from the window file, with a posterior
    // that follows from them: the identity check catches it.
    let mut ev = evidence(&dir, "w1");
    ev.record.as_mut().unwrap().entrants[0].returns[0] += 0.5;
    let refolded = fold_window(&ev.prior, "w1", &ev.commitments, ev.record.as_ref()).unwrap();
    let mut forged = original.clone();
    forged["forward_evidence"] = serde_json::to_value(&refolded).unwrap();
    resign(&dir, "w1", &forged, &key, 1);
    let report = verify_arena(&dir, None).unwrap();
    assert!(!report.ok);
    let fields: Vec<IdentityField> = report.windows[0]
        .identity_mismatches
        .iter()
        .map(|m| m.field)
        .collect();
    assert_eq!(fields, [IdentityField::ForwardRecord]);
    assert!(report.windows[0].forward_evidence_error.is_none());

    // A family that leaves a commitment out.
    let mut ev = evidence(&dir, "w1");
    ev.record = serde_json::from_value(original["forward_evidence"]["record"].clone()).unwrap();
    ev.commitments.pop();
    let refolded = fold_window(&ev.prior, "w1", &ev.commitments, ev.record.as_ref()).unwrap();
    let mut forged = original.clone();
    forged["forward_evidence"] = serde_json::to_value(&refolded).unwrap();
    resign(&dir, "w1", &forged, &key, 1);
    let report = verify_arena(&dir, None).unwrap();
    let fields: Vec<IdentityField> = report.windows[0]
        .identity_mismatches
        .iter()
        .map(|m| m.field)
        .collect();
    assert_eq!(fields, [IdentityField::ForwardCommitments]);
    assert_eq!(
        IdentityField::ForwardCommitments.as_str(),
        "forward_commitments"
    );
    assert_eq!(IdentityField::ForwardRecord.as_str(), "forward_record");

    // An observation of a row the board did not re-execute.
    let mut ev = evidence(&dir, "w1");
    ev.record = serde_json::from_value(original["forward_evidence"]["record"].clone()).unwrap();
    let mut forged = original.clone();
    let rows: Vec<String> = board(&dir, "w1")
        .chain
        .iter()
        .skip(1)
        .map(|l| l.payload.clone())
        .collect();
    let mut payloads = vec![serde_json::to_string(&forged).unwrap()];
    for row in rows {
        let mut value: Value = serde_json::from_str(&row).unwrap();
        if value["agent_id"] == "bah" {
            value["returns_provenance"] = Value::from("replayed");
        }
        payloads.push(serde_json::to_string(&value).unwrap());
    }
    forged["certifying"] = Value::Bool(false);
    payloads[0] = serde_json::to_string(&forged).unwrap();
    let chain = publish_public_chain(&payloads, &key);
    std::fs::write(
        dir.join(WINDOWS_DIR).join("w1").join(BOARD_FILE),
        serde_json::to_vec(&chain).unwrap(),
    )
    .unwrap();
    resign(&dir, "w1", &forged, &key, 1);
    let error = verify_arena(&dir, None).unwrap().windows[0]
        .forward_evidence_error
        .clone()
        .unwrap();
    assert!(
        error.contains("`bah` is observed but has no re-executed row"),
        "{error}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn boards_signed_before_the_ledger_still_verify_and_the_ledger_starts_after_them() {
    let dir = temp_dir("legacy");
    let key = SigningKey::derive(b"forward-evidence-key");
    let mut arena = Arena::init(&dir).unwrap();
    // A window with no re-executed row keeps its earlier schema and bytes.
    let data = market(1, 2030);
    arena
        .open_window_with_provenance("w1", 10, 20, config(), None, scorer())
        .unwrap();
    let artifact = reference_entrant_digest("momentum", &scorer());
    arena
        .register_entry("w1", make_commitment("mom", "w1", &artifact, "salt"))
        .unwrap();
    arena.advance(20).unwrap();
    let dataset = dir.join("w1.csv");
    std::fs::write(&dataset, csv(&data)).unwrap();
    arena
        .reveal_and_score(
            "w1",
            &dataset,
            &[RevealedEntry {
                agent_id: Some("mom".to_string()),
                submission: None,
                capture: Some(capture("momentum", &data)),
                artifact_digest: artifact,
                salt: "salt".to_string(),
                fault_plan_sha256: None,
            }],
        )
        .unwrap();
    let w = arena.window("w1").unwrap();
    assert_eq!(w.returns_provenance["mom"], ReturnsProvenance::Replayed);
    assert!(w.forward.is_none());
    assert_eq!(w.schema_version, EXECUTED_WINDOW_SCHEMA_VERSION);
    let window_text =
        std::fs::read_to_string(dir.join(WINDOWS_DIR).join("w1").join(WINDOW_FILE)).unwrap();
    assert!(!window_text.contains("forward"));
    arena.publish("w1", &key).unwrap();
    // The board carries a ledger with the commitment unresolved and nothing
    // observed: replayed rows are never evidence.
    let ev = evidence(&dir, "w1");
    assert!(ev.record.is_none() && ev.posterior.artifacts.is_empty());
    assert_eq!(ev.posterior.family_size, 1);

    // Rewrite it as a board signed before the ledger existed.
    let mut legacy = header(&dir, "w1");
    legacy.as_object_mut().unwrap().remove("forward_evidence");
    resign(&dir, "w1", &legacy, &key, 1);
    let report = verify_arena(&dir, None).unwrap();
    assert!(report.ok, "{report:?}");
    assert!(report.forward_ledger.is_none());
    assert!(report.windows[0].forward_evidence_error.is_none());
    let json = serde_json::to_value(&report).unwrap();
    assert!(json.get("forward_ledger").is_none());
    assert!(json["windows"][0].get("forward_evidence_error").is_none());

    // A new board after it starts the ledger from genesis.
    let mut arena = Arena::load(&dir).unwrap();
    run_window(
        &mut arena,
        &dir,
        1,
        "w2",
        &market(2, 2031),
        &[entrant("mom", "momentum")],
        &key,
    );
    let ev = evidence(&dir, "w2");
    assert_eq!(ev.prior, ForwardLedger::genesis());
    let report = verify_arena(&dir, None).unwrap();
    assert!(report.ok, "{report:?}");
    assert_eq!(report.forward_ledger.as_ref(), Some(&ev.posterior));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_window_file_whose_forward_record_names_an_unexecuted_row_does_not_load() {
    let dir = temp_dir("load");
    let key = SigningKey::derive(b"forward-evidence-key");
    let mut arena = Arena::init(&dir).unwrap();
    run_window(
        &mut arena,
        &dir,
        0,
        "w1",
        &market(1, 2030),
        &[entrant("mom", "momentum")],
        &key,
    );
    assert!(Arena::load(&dir).is_ok());
    let path = dir.join(WINDOWS_DIR).join("w1").join(WINDOW_FILE);
    let mut window: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    window["returns_provenance"]["mom"] = Value::from("replayed");
    std::fs::write(&path, serde_json::to_vec(&window).unwrap()).unwrap();
    let error = Arena::load(&dir).err().unwrap();
    assert!(
        error.contains("records forward evidence for `mom`, which it did not re-execute"),
        "{error}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}
