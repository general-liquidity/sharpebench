//! Live forward capture certifies a model-backed entrant on a sealed log of
//! what the host ran and told it, not on re-execution. Everything here runs
//! with a fake provider behind the real model gateway, a fake image digest and
//! a fake clock: no model call, no network, no container.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use sharpebench_arena::live_capture::{
    bar_day_start_ms, entry_sha256, header_sha256, LiveClock, LiveEntry, LiveEvent,
    LiveLoggedAgent, LiveWindow, LoggingTransport, SharedRecorder, LIVE_LOG_SCHEMA,
    MAX_CLOCK_SKEW_MS,
};
use sharpebench_arena::{
    certify_live_log, seal_live_log, verify_arena, Arena, BoardRow, LiveLogHeader, LiveLogRefusal,
    LiveRecorder, ReplayWindow, ReturnsProvenance, RevealedEntry, SealedLiveLog, SigningKey,
    BOARD_FILE, BOARD_MD_FILE, LIVE_CAPTURE_WINDOW_SCHEMA_VERSION, WINDOWS_DIR,
};
use sharpebench_attest::{content_digest, make_commitment, publish_public_chain, PublicChain};
use sharpebench_core::ScoreConfig;
use sharpebench_harness::accounting::RateCard;
use sharpebench_harness::gateway::{
    CallPermits, GatewayLimits, ModelGateway, ModelRoute, ProviderBody, ProviderCall,
    ProviderOutcome, ProviderTransport, ProviderUsage, RouteTable, Secret, GATEWAY_PROTOCOL,
};
use sharpebench_harness::gateway_journal::GatewayBudget;
use sharpebench_protocol::{Action, Decision, MarketObservation, Order};
use sharpebench_sim::{replay_submission, Agent, Dataset};

const WINDOW: &str = "live-w1";
const AGENT: &str = "llm-entrant";
/// When the host hands a bar to the entrant: 21:00 UTC on the bar's day.
const BAR_ARRIVAL_MS: u64 = 21 * 3_600_000;

fn temp_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "sharpebench-arena-live-{tag}-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn scorer() -> String {
    content_digest(b"live-capture-scorer")
}

fn config() -> ScoreConfig {
    ScoreConfig {
        execution_seeds_per_window: 2,
        ..ScoreConfig::default()
    }
}

fn market(seed: u64) -> Dataset {
    Dataset::synthetic_parameterized(3, 40, seed, 3.0, 0.0, 0.0)
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

fn replay_of(data: &Dataset) -> ReplayWindow {
    ReplayWindow::parse(csv(data).as_bytes(), &config()).unwrap()
}

fn image_digest(tag: &str) -> String {
    content_digest(format!("live-image-{tag}").as_bytes())
}

fn image(tag: &str) -> String {
    format!("test/llm@sha256:{}", image_digest(tag))
}

fn key() -> SigningKey {
    SigningKey::derive(b"live-capture-arena-key")
}

/// A host clock the test sets per bar. Every reading advances it by one
/// millisecond, so entries within a bar are strictly ordered.
#[derive(Clone, Default)]
struct FakeClock(Arc<AtomicU64>);

impl FakeClock {
    fn set(&self, unix_ms: u64) {
        self.0.store(unix_ms, Ordering::SeqCst);
    }
}

impl LiveClock for FakeClock {
    fn now_unix_ms(&self) -> u64 {
        self.0.fetch_add(1, Ordering::SeqCst)
    }
}

/// A provider that cannot open a socket. Its answers depend on how many calls
/// this instance has served, so the entrant behind it is not a function of its
/// observations: re-execution could not certify it.
struct FakeProvider {
    calls: u64,
}

impl ProviderTransport for FakeProvider {
    fn call(&mut self, _call: ProviderCall<'_>) -> ProviderOutcome {
        self.calls += 1;
        let text = if self.calls.is_multiple_of(3) {
            "buy"
        } else {
            "hold"
        };
        ProviderOutcome::Answered {
            status: 200,
            body: serde_json::to_vec(&ProviderBody {
                text: text.to_string(),
                finish_reason: Some("stop".to_string()),
                usage: Some(ProviderUsage {
                    input_tokens: 12,
                    output_tokens: 1,
                    cache_read_input_tokens: None,
                    cache_write_input_tokens: None,
                }),
            })
            .unwrap(),
        }
    }
}

fn routes() -> &'static RouteTable {
    let card = RateCard::from_json(
        br#"{"schema_version":"sharpebench.token-rate-card.v1","provider":"fake","model":"fake-1","revision":"2026-01-01","input_usd_nanos_per_token":1,"output_usd_nanos_per_token":1}"#,
    )
    .unwrap();
    Box::leak(Box::new(
        RouteTable::new(vec![ModelRoute::new(
            "fake.v1",
            "https://provider.invalid/v1/messages",
            Secret::new("sk-live-capture-test-not-a-secret-0123456789"),
            card,
            64,
            8,
        )
        .unwrap()])
        .unwrap(),
    ))
}

/// A model-backed entrant: it asks the gateway about each bar and buys the
/// first symbol when the model says so.
struct ModelEntrant {
    gateway: ModelGateway<'static, LoggingTransport<FakeProvider>>,
}

impl ModelEntrant {
    fn new(recorder: &SharedRecorder) -> Self {
        let permits: &'static CallPermits = Box::leak(Box::new(CallPermits::new(1)));
        Self {
            gateway: ModelGateway::new(
                routes(),
                permits,
                LoggingTransport::new(FakeProvider { calls: 0 }, Arc::clone(recorder)),
                GatewayBudget {
                    max_usd_nanos: 10_000_000,
                    max_calls: 1_000,
                },
                GatewayLimits::default(),
            ),
        }
    }
}

impl Agent for ModelEntrant {
    fn decide(&mut self, observation: &MarketObservation) -> Decision {
        let request = serde_json::json!({
            "protocol": GATEWAY_PROTOCOL,
            "model_alias": "fake.v1",
            "messages": [{"role": "user", "content": format!("bar {}", observation.date)}],
            "max_output_tokens": 4,
        });
        let response: serde_json::Value =
            serde_json::from_str(&self.gateway.serve_line(&request.to_string())).unwrap();
        let buy = response["text"] == "buy";
        Decision {
            orders: observation
                .symbols
                .iter()
                .take(1)
                .map(|s| Order {
                    symbol: s.symbol.clone(),
                    action: if buy { Action::Buy } else { Action::Hold },
                    target_weight: if buy { 0.5 } else { 0.0 },
                    confidence: None,
                    rationale: String::new(),
                })
                .collect(),
            reasoning: String::new(),
            cost: None,
        }
    }
}

/// Sets the host clock to each bar's arrival before the logged entrant sees
/// it, as an always-on host would read it off its own clock.
struct BarClocked<A> {
    clock: FakeClock,
    inner: A,
}

impl<A: Agent> Agent for BarClocked<A> {
    fn decide(&mut self, observation: &MarketObservation) -> Decision {
        self.clock
            .set(bar_day_start_ms(&observation.date).unwrap() + BAR_ARRIVAL_MS);
        self.inner.decide(observation)
    }
}

fn header(replay: &ReplayWindow, tag: &str) -> LiveLogHeader {
    LiveLogHeader {
        schema: LIVE_LOG_SCHEMA.to_string(),
        window_id: WINDOW.to_string(),
        agent_id: AGENT.to_string(),
        image: image(tag),
        runner_artifact_sha256: scorer(),
        windows: replay
            .windows
            .iter()
            .map(|w| LiveWindow {
                start: w.start,
                end: w.end,
            })
            .collect(),
        seeds: replay.seeds.clone(),
    }
}

/// Run the whole window live through the gateway, `before_decide` called with
/// `(run, step)` before every logged decision, and return the recorder.
fn run_live(
    dir: &Path,
    data: &Dataset,
    mut before_decide: impl FnMut(&SharedRecorder, &FakeClock, usize, usize, &MarketObservation)
        + 'static,
) -> (SharedRecorder, FakeClock) {
    let replay = replay_of(data);
    let clock = FakeClock::default();
    let recorder: SharedRecorder = Arc::new(Mutex::new(
        LiveRecorder::create(
            &dir.join("live.jsonl"),
            header(&replay, "a"),
            Box::new(clock.clone()),
        )
        .unwrap(),
    ));
    let hook: Hook = Arc::new(Mutex::new(
        move |r: &SharedRecorder, c: &FakeClock, run, step, o: &MarketObservation| {
            before_decide(r, c, run, step, o)
        },
    ));
    let mut next_run = 0;
    let (shared, host_clock) = (Arc::clone(&recorder), clock.clone());
    sharpebench_harness::run_agent_capture(
        AGENT,
        &replay.data,
        &replay.windows,
        &replay.seeds,
        replay.costs,
        || {
            let run = next_run;
            next_run += 1;
            let entrant = Hooked {
                inner: ModelEntrant::new(&shared),
                hook: Arc::clone(&hook),
                recorder: Arc::clone(&shared),
                clock: host_clock.clone(),
                run,
                step: 0,
            };
            Box::new(BarClocked {
                clock: host_clock.clone(),
                inner: LiveLoggedAgent::new(entrant, Arc::clone(&shared), run),
            })
        },
    );
    (recorder, clock)
}

type Hook = Arc<Mutex<dyn FnMut(&SharedRecorder, &FakeClock, usize, usize, &MarketObservation)>>;

/// Calls the test hook between the logged observation and the decision.
struct Hooked<A> {
    inner: A,
    hook: Hook,
    recorder: SharedRecorder,
    clock: FakeClock,
    run: usize,
    step: usize,
}

impl<A: Agent> Agent for Hooked<A> {
    fn decide(&mut self, observation: &MarketObservation) -> Decision {
        (self.hook.lock().unwrap())(
            &self.recorder,
            &self.clock,
            self.run,
            self.step,
            observation,
        );
        self.step += 1;
        self.inner.decide(observation)
    }
}

fn sealed(recorder: &SharedRecorder) -> SealedLiveLog {
    recorder.lock().unwrap().seal(&key()).unwrap()
}

fn live_log(dir: &Path, data: &Dataset) -> SealedLiveLog {
    sealed(&run_live(dir, data, |_, _, _, _, _| {}).0)
}

fn certify(data: &Dataset, artifact: &str, log: &SealedLiveLog) -> Result<u64, LiveLogRefusal> {
    certify_live_log(
        WINDOW,
        &scorer(),
        &config(),
        &replay_of(data),
        AGENT,
        artifact,
        log,
    )
    .map(|certified| certified.root.bars)
}

/// Re-link every entry after `edit` and seal again under the arena key: what
/// an operator holding the key could produce, so only the content rules stand
/// between it and a certified row.
fn resealed(log: &SealedLiveLog, edit: impl FnOnce(&mut Vec<LiveEntry>)) -> SealedLiveLog {
    let mut entries = log.entries.clone();
    edit(&mut entries);
    let mut prev = header_sha256(&log.header);
    for (seq, entry) in entries.iter_mut().enumerate() {
        entry.seq = seq as u64;
        entry.prev_sha256 = prev;
        prev = entry_sha256(entry);
    }
    seal_live_log(log.header.clone(), entries, &key(), 1).unwrap()
}

fn position(log: &SealedLiveLog, run: usize, step: usize, decision: bool) -> usize {
    log.entries
        .iter()
        .position(|e| {
            e.run == run
                && e.step == step
                && matches!(
                    (&e.event, decision),
                    (LiveEvent::Decision { .. }, true) | (LiveEvent::Observation { .. }, false)
                )
        })
        .unwrap()
}

/// Open, commit, advance and reveal through the arena; return the scored
/// arena and its directory.
fn scored(dir: &Path, data: &Dataset, artifact: &str, log: SealedLiveLog) -> Arena {
    let mut arena = Arena::init(dir).unwrap();
    arena
        .open_window_with_fault_plan(WINDOW, 10, 20, config(), None, scorer(), None)
        .unwrap();
    arena
        .register_entry(WINDOW, make_commitment(AGENT, WINDOW, artifact, "salt"))
        .unwrap();
    arena.advance(20).unwrap();
    let dataset = dir.join("dataset.csv");
    std::fs::write(&dataset, csv(data)).unwrap();
    let entry = RevealedEntry {
        agent_id: Some(AGENT.to_string()),
        submission: None,
        capture: None,
        artifact_digest: artifact.to_string(),
        salt: "salt".to_string(),
        fault_plan_sha256: None,
        live_log: Some(log),
    };
    arena.reveal_and_score(WINDOW, &dataset, &[entry]).unwrap();
    arena
}

#[test]
fn a_complete_live_log_certifies_a_model_backed_entrant_and_its_board_verifies() {
    let dir = temp_dir("complete");
    let data = market(5);
    let log = live_log(&dir, &data);
    let model_calls = log
        .entries
        .iter()
        .filter(|e| matches!(e.event, LiveEvent::ModelCall(_)))
        .count();
    let arena_dir = dir.join("arena");
    let mut arena = scored(&arena_dir, &data, &image_digest("a"), log.clone());

    let window = arena.window(WINDOW).unwrap();
    assert!(window.refusals.is_empty(), "{:?}", window.refusals);
    assert_eq!(
        window.returns_provenance.get(AGENT),
        Some(&ReturnsProvenance::LiveCaptured)
    );
    assert_eq!(window.certifying, Some(true));
    assert_eq!(window.schema_version, LIVE_CAPTURE_WINDOW_SCHEMA_VERSION);
    let root = &window.live_capture[AGENT];
    // Two windows of 15 bars, two seeds each; one model call per bar.
    assert_eq!((root.bars, root.model_calls, root.restarts), (60, 60, 0));
    assert_eq!(model_calls, 60);
    assert_eq!(root.entries, 180);
    assert_eq!(root.image, image("a"));

    // The ranked returns are the logged decisions replayed over the data.
    let replay = replay_of(&data);
    let certified = certify_live_log(
        WINDOW,
        &scorer(),
        &config(),
        &replay,
        AGENT,
        &image_digest("a"),
        &log,
    )
    .unwrap();
    let replayed = replay_submission(&replay.data, &certified.trajectory, replay.costs);
    let returns = |runs: &[sharpebench_core::Run]| -> Vec<Vec<f64>> {
        runs.iter().map(|run| run.returns.clone()).collect()
    };
    assert_eq!(window.scores.len(), 1);
    assert_eq!(window.scores[0].agent_id, AGENT);
    assert_eq!(returns(&replayed.runs).len(), 4);

    arena.publish(WINDOW, &key()).unwrap();
    let report = verify_arena(&arena_dir, Some(&key().verifying_key())).unwrap();
    assert!(report.ok, "{:?}", report.windows);
    let window_dir = arena_dir.join(WINDOWS_DIR).join(WINDOW);
    let markdown = std::fs::read_to_string(window_dir.join(BOARD_MD_FILE)).unwrap();
    assert!(markdown.contains("`llm-entrant`: live-captured"));
    assert!(markdown.contains("60 bars, 60 model calls, 0 restarts"));
    assert!(!markdown.contains("Noncertifying"));
    // Reloading the arena checks the recorded root against its seal.
    Arena::load(&arena_dir).unwrap();
}

#[test]
fn a_broken_or_altered_live_log_is_refused_with_its_reason() {
    let dir = temp_dir("refusals");
    let data = market(5);
    let log = live_log(&dir, &data);
    let committed = image_digest("a");
    assert_eq!(certify(&data, &committed, &log), Ok(60));

    let decision = position(&log, 1, 4, true);
    let observation = position(&log, 1, 4, false);
    let call = observation + 1;
    let last = log.entries.len() - 1;
    type Case = (&'static str, SealedLiveLog, fn(&LiveLogRefusal) -> bool);
    let cases: Vec<Case> = vec![
        (
            "dropped epoch",
            resealed(&log, |entries| {
                entries.drain(observation..=decision);
            }),
            |r| matches!(r, LiveLogRefusal::MissingBar { run: 1, step: 4 }),
        ),
        (
            "reordered entry, chain left as it was",
            {
                let mut swapped = log.clone();
                swapped.entries.swap(observation, decision);
                swapped
            },
            |r| matches!(r, LiveLogRefusal::BrokenChain { .. }),
        ),
        (
            "reordered entry, re-linked and resealed",
            resealed(&log, |entries| entries.swap(observation, decision)),
            |r| matches!(r, LiveLogRefusal::OutOfOrder { .. }),
        ),
        (
            "tampered response",
            {
                let mut tampered = log.clone();
                if let LiveEvent::ModelCall(record) = &mut tampered.entries[call].event {
                    record.response_sha256 = Some(content_digest(b"another answer"));
                }
                tampered
            },
            |r| matches!(r, LiveLogRefusal::BrokenChain { .. }),
        ),
        (
            "tampered last decision",
            {
                let mut tampered = log.clone();
                if let LiveEvent::Decision { decision } = &mut tampered.entries[last].event {
                    decision.reasoning = "rewritten".to_string();
                }
                tampered
            },
            |r| matches!(r, LiveLogRefusal::SealMismatch(_)),
        ),
        (
            "forged seal signature",
            {
                let mut forged = log.clone();
                let signature = &mut forged.seal.chain[0].signature;
                let flipped = if signature.starts_with('0') { "1" } else { "0" };
                signature.replace_range(0..1, flipped);
                forged
            },
            |r| matches!(r, LiveLogRefusal::UnsignedSeal(_)),
        ),
        (
            "observation the revealed data does not produce",
            resealed(&log, |entries| {
                if let LiveEvent::Observation {
                    observation_sha256, ..
                } = &mut entries[observation].event
                {
                    *observation_sha256 = content_digest(b"a different book");
                }
            }),
            |r| {
                matches!(
                    r,
                    LiveLogRefusal::DataMismatch {
                        run: 1,
                        step: 4,
                        ..
                    }
                )
            },
        ),
        (
            "decision logged after the next bar began",
            resealed(&log, |entries| {
                entries[decision].at_unix_ms += 86_400_000;
            }),
            |r| matches!(r, LiveLogRefusal::ClockSkew { .. }),
        ),
        (
            "observation stamped before its bar's day",
            resealed(&log, |entries| {
                entries[observation].at_unix_ms -= BAR_ARRIVAL_MS + 2 * MAX_CLOCK_SKEW_MS;
            }),
            |r| matches!(r, LiveLogRefusal::ClockSkew { .. }),
        ),
    ];
    for (name, altered, expected) in cases {
        let refusal = certify(&data, &committed, &altered).expect_err(name);
        assert!(expected(&refusal), "{name}: {refusal}");
    }

    let wrong = certify(&data, &image_digest("b"), &log).unwrap_err();
    assert_eq!(
        wrong,
        LiveLogRefusal::WrongImageDigest {
            logged: committed.clone(),
            committed: image_digest("b"),
        }
    );
    // The same log against another revealed market: same dates, other closes.
    let other = certify(&market(6), &committed, &log).unwrap_err();
    assert!(
        matches!(other, LiveLogRefusal::DataMismatch { run: 0, .. }),
        "{other}"
    );
}

#[test]
fn a_log_for_another_window_agent_scorer_or_matrix_is_refused() {
    let dir = temp_dir("header");
    let data = market(5);
    let log = live_log(&dir, &data);
    let committed = image_digest("a");
    let with_header = |edit: fn(&mut LiveLogHeader)| {
        let mut header = log.header.clone();
        edit(&mut header);
        let relinked = SealedLiveLog {
            header,
            ..log.clone()
        };
        resealed(&relinked, |_| {})
    };
    let edits: [fn(&mut LiveLogHeader); 6] = [
        |h| h.schema = "sharpebench-arena-live-log/v0".to_string(),
        |h| h.window_id = "another-window".to_string(),
        |h| h.agent_id = "another-agent".to_string(),
        |h| h.runner_artifact_sha256 = content_digest(b"another scorer"),
        |h| h.seeds = vec![0],
        |h| h.windows[0].end += 1,
    ];
    for edit in edits {
        let refusal = certify(&data, &committed, &with_header(edit)).unwrap_err();
        assert!(
            matches!(refusal, LiveLogRefusal::HeaderMismatch(_)),
            "{refusal}"
        );
    }
    let unpinned = with_header(|h| h.image = "test/llm:latest".to_string());
    assert!(matches!(
        certify(&data, &committed, &unpinned),
        Err(LiveLogRefusal::HeaderMismatch(_))
    ));
}

#[test]
fn an_arena_records_a_refused_live_log_and_its_board_does_not_certify() {
    let dir = temp_dir("arena-refusal");
    let data = market(5);
    let log = live_log(&dir, &data);
    let dropped = resealed(&log, |entries| {
        entries.truncate(entries.len() - 2);
    });
    let arena = scored(&dir.join("arena"), &data, &image_digest("a"), dropped);
    let window = arena.window(WINDOW).unwrap();
    assert!(window.returns_provenance.is_empty());
    assert!(window.live_capture.is_empty());
    assert_eq!(window.certifying, Some(false));
    assert_eq!(window.refusals.len(), 1);
    assert!(
        window.refusals[0]
            .reason
            .contains("no complete record of run 3"),
        "{}",
        window.refusals[0].reason
    );
}

#[test]
fn an_entry_carrying_a_live_log_and_returns_is_refused() {
    let dir = temp_dir("both");
    let data = market(5);
    let log = live_log(&dir, &data);
    let mut arena = Arena::init(&dir.join("arena")).unwrap();
    arena
        .open_window_with_fault_plan(WINDOW, 10, 20, config(), None, scorer(), None)
        .unwrap();
    let artifact = image_digest("a");
    arena
        .register_entry(WINDOW, make_commitment(AGENT, WINDOW, &artifact, "salt"))
        .unwrap();
    arena.advance(20).unwrap();
    let dataset = dir.join("dataset.csv");
    std::fs::write(&dataset, csv(&data)).unwrap();
    let replay = replay_of(&data);
    let certified = certify_live_log(
        WINDOW,
        &scorer(),
        &config(),
        &replay,
        AGENT,
        &artifact,
        &log,
    )
    .unwrap();
    let entry = RevealedEntry {
        agent_id: Some(AGENT.to_string()),
        submission: None,
        capture: Some(certified.trajectory),
        artifact_digest: artifact,
        salt: "salt".to_string(),
        fault_plan_sha256: None,
        live_log: Some(log),
    };
    arena.reveal_and_score(WINDOW, &dataset, &[entry]).unwrap();
    let window = arena.window(WINDOW).unwrap();
    assert!(window.returns_provenance.is_empty());
    assert!(window.refusals[0]
        .reason
        .contains("also a capture or returns"));
}

#[test]
fn a_host_restart_mid_bar_resumes_the_chain_and_still_certifies() {
    let dir = temp_dir("restart");
    let data = market(5);
    let path = dir.join("live.jsonl");
    // The host dies after logging run 2 step 7's observation, before the
    // entrant decides. It resumes from the file, which logs the restart, and
    // shows the open bar to the fresh entrant instance again.
    let (recorder, _) = run_live(
        &dir,
        &data,
        move |recorder, clock, run, step, observation| {
            if (run, step) == (2, 7) {
                let mut guard = recorder.lock().unwrap();
                *guard = LiveRecorder::resume(&path, Box::new(clock.clone())).unwrap();
                guard.record_observation(run, step, observation);
            }
        },
    );
    let log = sealed(&recorder);
    let restart = log
        .entries
        .iter()
        .position(|e| matches!(e.event, LiveEvent::Restart))
        .unwrap();
    assert!(matches!(
        log.entries[restart - 1].event,
        LiveEvent::Observation { .. }
    ));
    let root = certify_live_log(
        WINDOW,
        &scorer(),
        &config(),
        &replay_of(&data),
        AGENT,
        &image_digest("a"),
        &log,
    )
    .unwrap()
    .root;
    assert_eq!((root.bars, root.restarts), (60, 1));

    // Without the restart on record, the bar is observed twice while open.
    let unrecorded = resealed(&log, |entries| {
        entries.remove(restart);
    });
    assert!(matches!(
        certify(&data, &image_digest("a"), &unrecorded),
        Err(LiveLogRefusal::OutOfOrder { .. })
    ));
    // A restarted host that decides the open bar without showing it to the
    // fresh instance again is refused.
    let unshown = resealed(&log, |entries| {
        entries.remove(restart + 1);
    });
    assert!(matches!(
        certify(&data, &image_digest("a"), &unshown),
        Err(LiveLogRefusal::OutOfOrder { .. })
    ));
}

#[test]
fn a_resume_refuses_a_log_whose_last_write_was_cut_short() {
    let dir = temp_dir("torn");
    let data = market(5);
    let replay = replay_of(&data);
    let path = dir.join("live.jsonl");
    let clock = FakeClock::default();
    let mut recorder =
        LiveRecorder::create(&path, header(&replay, "a"), Box::new(clock.clone())).unwrap();
    let observation = MarketObservation {
        date: replay.data.dates[replay.windows[0].start].clone(),
        cash: 1.0,
        symbols: Vec::new(),
        portfolio: Vec::new(),
    };
    recorder.record_observation(0, 0, &observation);
    drop(recorder);
    assert_eq!(LiveRecorder::read(&path).unwrap().1.len(), 1);
    let mut bytes = std::fs::read(&path).unwrap();
    bytes.extend_from_slice(b"{\"seq\":1");
    std::fs::write(&path, &bytes).unwrap();
    let error = LiveRecorder::resume(&path, Box::new(clock))
        .err()
        .expect("a torn tail is refused");
    assert!(error.contains("incomplete line"), "{error}");
    // A recorder never overwrites an existing log.
    assert!(
        LiveRecorder::create(&path, header(&replay, "a"), Box::new(FakeClock::default())).is_err()
    );
}

#[test]
fn host_clock_skew_within_the_tolerance_certifies_and_beyond_it_refuses() {
    let dir = temp_dir("skew");
    let data = market(5);
    let log = live_log(&dir, &data);
    let committed = image_digest("a");
    let decision = position(&log, 0, 3, true);
    let next_bar =
        bar_day_start_ms(&replay_of(&data).data.dates[replay_of(&data).windows[0].start + 4])
            .unwrap();
    // A decision stamped just after the next bar's day began, inside the
    // tolerance, certifies; one stamped past it does not.
    for (offset, certifies) in [(MAX_CLOCK_SKEW_MS - 1, true), (MAX_CLOCK_SKEW_MS, false)] {
        let skewed = resealed(&log, |entries| {
            entries[decision].at_unix_ms = next_bar + offset;
        });
        assert_eq!(
            certify(&data, &committed, &skewed).is_ok(),
            certifies,
            "offset {offset}"
        );
    }
    // A run's clock may step back by less than the tolerance, not more.
    for (back, certifies) in [(MAX_CLOCK_SKEW_MS, true), (MAX_CLOCK_SKEW_MS + 1, false)] {
        let call = position(&log, 0, 5, false) + 1;
        let skewed = resealed(&log, |entries| {
            let at = entries[call - 1].at_unix_ms - back;
            entries[call].at_unix_ms = at;
            if let LiveEvent::ModelCall(record) = &mut entries[call].event {
                record.started_unix_ms = at;
            }
        });
        let outcome = certify(&data, &committed, &skewed);
        assert_eq!(outcome.is_ok(), certifies, "back {back}: {outcome:?}");
    }
    // A model call that finished before it started.
    let call = position(&log, 0, 2, false) + 1;
    let backwards = resealed(&log, |entries| {
        let at = entries[call].at_unix_ms;
        if let LiveEvent::ModelCall(record) = &mut entries[call].event {
            record.started_unix_ms = at + 1;
        }
    });
    assert!(matches!(
        certify(&data, &committed, &backwards),
        Err(LiveLogRefusal::ClockSkew { .. })
    ));
}

#[test]
fn publishing_refuses_a_root_sealed_under_another_key_and_verify_needs_the_roots() {
    let dir = temp_dir("keys");
    let data = market(5);
    let log = live_log(&dir, &data);
    let arena_dir = dir.join("arena");
    let mut arena = scored(&arena_dir, &data, &image_digest("a"), log);
    let other = SigningKey::derive(b"not-the-arena-key");
    let error = arena.publish(WINDOW, &other).unwrap_err();
    assert!(error.contains("not the arena key"), "{error}");

    // A board re-signed without its roots fails verification.
    let mut arena = Arena::load(&arena_dir).unwrap();
    arena.publish(WINDOW, &key()).unwrap();
    let board_path = arena_dir.join(WINDOWS_DIR).join(WINDOW).join(BOARD_FILE);
    let board: PublicChain = serde_json::from_slice(&std::fs::read(&board_path).unwrap()).unwrap();
    let mut header: serde_json::Value = serde_json::from_str(&board.chain[0].payload).unwrap();
    header.as_object_mut().unwrap().remove("live_capture");
    let mut payloads = vec![header.to_string()];
    payloads.extend(board.chain[1..].iter().map(|link| link.payload.clone()));
    let row: BoardRow = serde_json::from_str(&payloads[1]).unwrap();
    assert_eq!(
        row.returns_provenance,
        Some(ReturnsProvenance::LiveCaptured)
    );
    std::fs::write(
        &board_path,
        serde_json::to_vec(&publish_public_chain(&payloads, &key())).unwrap(),
    )
    .unwrap();
    let report = verify_arena(&arena_dir, Some(&key().verifying_key())).unwrap();
    assert!(!report.ok);
    let window = &report.windows[0];
    let error = window.certification_error.as_deref().unwrap();
    assert!(error.contains("do not match live capture roots"), "{error}");
    assert!(window
        .identity_mismatches
        .iter()
        .any(|m| m.field.as_str() == "live_capture"));
}

#[test]
fn a_faulted_window_refuses_a_live_log() {
    let dir = temp_dir("faulted");
    let data = market(5);
    let log = live_log(&dir, &data);
    let plan = content_digest(b"a fault plan");
    let artifact = image_digest("a");
    let mut arena = Arena::init(&dir.join("arena")).unwrap();
    arena
        .open_window_with_fault_plan(WINDOW, 10, 20, config(), None, scorer(), Some(plan.clone()))
        .unwrap();
    arena
        .register_entry(
            WINDOW,
            sharpebench_attest::make_commitment_under_fault_plan(
                AGENT,
                WINDOW,
                &artifact,
                "salt",
                Some(&plan),
            ),
        )
        .unwrap();
    arena.advance(20).unwrap();
    let dataset = dir.join("dataset.csv");
    std::fs::write(&dataset, csv(&data)).unwrap();
    let entry = RevealedEntry {
        agent_id: Some(AGENT.to_string()),
        submission: None,
        capture: None,
        artifact_digest: artifact,
        salt: "salt".to_string(),
        fault_plan_sha256: Some(plan),
        live_log: Some(log),
    };
    arena.reveal_and_score(WINDOW, &dataset, &[entry]).unwrap();
    let window = arena.window(WINDOW).unwrap();
    assert!(window.live_capture.is_empty());
    assert!(
        window.refusals[0].reason.contains("faulted experiment"),
        "{}",
        window.refusals[0].reason
    );
}
