//! The live window runner end to end: a window opened, run bar by bar from a
//! replay feed, sealed, scored, published and verified. Each entrant is a
//! model-backed process on real OS pipes behind the real gateway entrant and
//! model gateway, with a fake provider and a deterministic clock: no model
//! call, no network, no container, no market data.
//!
//! Most tests run under [`StandInHostClock`], which moves like the rehearsal
//! clock but does not report itself simulated, so the certifying path is
//! exercised. The rehearsal clock itself is tested on its own: a board run
//! under it is `live-rehearsal` and never certifies.

use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use sharpebench_arena::live_capture::{LiveEvent, LoggingTransport, SharedRecorder};
use sharpebench_arena::{
    run_live_window, verify_arena, Arena, HostClock, LaunchedEntrant, LiveBar, LiveEntrant,
    LiveEntrants, LiveFeed, LiveHost, LiveRecorder, LiveWindowReport, LiveWindowRun,
    LiveWindowSpec, ReplayFeed, ReplayWindow, ReturnsProvenance, RevealedEntry, SigningKey,
    SimulatedClock, LIVE_ENTRANTS_SCHEMA, LIVE_WINDOW_SPEC_SCHEMA,
};
use sharpebench_attest::content_digest;
use sharpebench_core::ScoreConfig;
use sharpebench_harness::accounting::RateCard;
use sharpebench_harness::gateway::serve::{EntrantPipes, EntrantProcess, GatewayEntrant};
use sharpebench_harness::gateway::{
    CallPermits, GatewayLimits, ModelGateway, ModelRoute, ProviderBody, ProviderCall,
    ProviderOutcome, ProviderTransport, ProviderUsage, RouteTable, Secret, GATEWAY_PROTOCOL,
};
use sharpebench_harness::gateway_journal::{GatewayBudget, GatewayJournal};
use sharpebench_protocol::{Action, Decision, MarketObservation, Order};
use sharpebench_sim::{Dataset, TransportDiagnostics};

const WINDOW: &str = "live-run-w1";
/// When a bar reaches the host under the simulated clock: 21:00 UTC.
const ARRIVAL_MS: u64 = 21 * 3_600_000;

fn temp_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "sharpebench-live-window-{tag}-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn market() -> Dataset {
    Dataset::synthetic_parameterized(3, 40, 5, 3.0, 0.0, 0.0)
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

fn replay_feed() -> ReplayFeed {
    ReplayFeed::from_csv(&csv(&market())).unwrap()
}

fn image(tag: &str) -> String {
    format!(
        "test/llm@sha256:{}",
        content_digest(format!("live-window-image-{tag}").as_bytes())
    )
}

fn spec() -> LiveWindowSpec {
    LiveWindowSpec {
        schema: LIVE_WINDOW_SPEC_SCHEMA.to_string(),
        window_id: WINDOW.to_string(),
        commit_deadline: 10,
        data_reveal_epoch: 20,
        bars: 40,
        scorer_artifact_sha256: content_digest(b"live-window-scorer"),
        score_config: Some(ScoreConfig {
            execution_seeds_per_window: 2,
            ..ScoreConfig::default()
        }),
    }
}

fn entrants(ids: &[&str]) -> LiveEntrants {
    LiveEntrants {
        schema: LIVE_ENTRANTS_SCHEMA.to_string(),
        entrants: ids
            .iter()
            .map(|id| LiveEntrant {
                agent_id: id.to_string(),
                image: image(id),
                salt: format!("salt-{id}"),
            })
            .collect(),
    }
}

fn key() -> SigningKey {
    SigningKey::derive(b"live-window-arena-key")
}

/// A deterministic stand-in for the host's wall clock: the rehearsal clock's
/// readings, without its rehearsal mark. An operator's real host runs
/// `SystemClock`; these tests replay bars from years ago, so they need a clock
/// that moves with the bars.
struct StandInHostClock(SimulatedClock);

impl HostClock for StandInHostClock {
    fn now_unix_ms(&self) -> u64 {
        self.0.now_unix_ms()
    }

    fn bar_arrived(&self, date: &str) {
        self.0.bar_arrived(date);
    }
}

fn simulated() -> Arc<dyn HostClock> {
    Arc::new(StandInHostClock(SimulatedClock::new(ARRIVAL_MS)))
}

/// The rehearsal clock `--simulated-clock` uses.
fn rehearsal() -> Arc<dyn HostClock> {
    Arc::new(SimulatedClock::new(ARRIVAL_MS))
}

/// A provider that cannot open a socket. Its answers depend on how many calls
/// it has served, so the entrant behind it cannot be re-executed.
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
    static ROUTES: OnceLock<RouteTable> = OnceLock::new();
    ROUTES.get_or_init(|| {
        let card = RateCard::from_json(
            br#"{"schema_version":"sharpebench.token-rate-card.v1","provider":"fake","model":"fake-1","revision":"2026-01-01","input_usd_nanos_per_token":1,"output_usd_nanos_per_token":1}"#,
        )
        .unwrap();
        RouteTable::new(vec![ModelRoute::new(
            "fake.v1",
            "https://provider.invalid/v1/messages",
            Secret::new("sk-live-window-test-not-a-secret-0123456789"),
            card,
            64,
            8,
        )
        .unwrap()])
        .unwrap()
    })
}

/// Call permits for one instance. A process-wide set would be shared by the
/// tests running in parallel here, and `try_acquire` refuses rather than waits,
/// so a busy moment in one test could refuse another test's call and change
/// its model-call count. Each instance makes at most one call at a time, so its
/// own small set never refuses. Leaked because the gateway borrows it for the
/// worker thread's life; a test process leaks a few bytes per run.
fn permits() -> &'static CallPermits {
    Box::leak(Box::new(CallPermits::new(4)))
}

/// The entrant process: an in-process thread on a real OS pipe pair. For each
/// observation it asks the model about the bar and buys the first symbol
/// when the model says so. At decision `stall_at` it asks, reads the answer,
/// and then never writes again.
struct ThreadEntrant(std::thread::JoinHandle<()>);

impl EntrantProcess for ThreadEntrant {
    fn exited(&mut self) -> Option<Option<i32>> {
        self.0.is_finished().then_some(Some(0))
    }

    fn terminate(&mut self) {}
}

fn entrant_process(stall_at: Option<usize>) -> EntrantPipes {
    let (host_reads, mut entrant_writes) = std::io::pipe().unwrap();
    let (entrant_reads, host_writes) = std::io::pipe().unwrap();
    let handle = std::thread::spawn(move || {
        let mut input = BufReader::new(entrant_reads);
        // inv: `n` decisions were asked for before this iteration.
        for n in 0.. {
            let mut line = String::new();
            if input.read_line(&mut line).unwrap_or(0) == 0 {
                return;
            }
            let observation: MarketObservation = serde_json::from_str(&line).unwrap();
            let request = serde_json::json!({
                "protocol": GATEWAY_PROTOCOL,
                "model_alias": "fake.v1",
                "messages": [{"role": "user", "content": format!("bar {}", observation.date)}],
                "max_output_tokens": 4,
            });
            if writeln!(entrant_writes, "{request}").is_err() {
                return;
            }
            let mut answer = String::new();
            if input.read_line(&mut answer).unwrap_or(0) == 0 {
                return;
            }
            if stall_at == Some(n) {
                // Stalled: hold the pipe open and answer nothing until the
                // host closes it.
                let mut rest = String::new();
                while input.read_line(&mut rest).unwrap_or(0) > 0 {}
                return;
            }
            let answer: serde_json::Value = serde_json::from_str(&answer).unwrap();
            let buy = answer["text"] == "buy";
            let decision = Decision {
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
            };
            if writeln!(
                entrant_writes,
                "{}",
                serde_json::to_string(&decision).unwrap()
            )
            .is_err()
            {
                return;
            }
        }
    });
    EntrantPipes::new(
        Box::new(host_writes),
        Box::new(host_reads),
        Box::new(ThreadEntrant(handle)),
    )
}

type Journals = Arc<Mutex<Vec<(usize, GatewayJournal)>>>;

/// One run's instance: a worker thread owns the gateway and the gateway
/// entrant (which borrows it), and this end passes observations and decisions.
struct GatewayInstance {
    run: usize,
    observations: Option<mpsc::Sender<MarketObservation>>,
    decisions: mpsc::Receiver<(Decision, Option<String>)>,
    fault: Option<String>,
    worker: Option<std::thread::JoinHandle<GatewayJournal>>,
    journals: Journals,
    crash: Option<Arc<AtomicUsize>>,
}

impl GatewayInstance {
    fn spawn(
        run: usize,
        recorder: SharedRecorder,
        stall_at: Option<usize>,
        journals: Journals,
        crash: Option<Arc<AtomicUsize>>,
    ) -> Self {
        let (observations, inbox) = mpsc::channel::<MarketObservation>();
        let (outbox, decisions) = mpsc::channel();
        let worker = std::thread::spawn(move || {
            let mut gateway = ModelGateway::new(
                routes(),
                permits(),
                LoggingTransport::new(FakeProvider { calls: 0 }, recorder),
                GatewayBudget {
                    max_usd_nanos: 10_000_000,
                    max_calls: 1_000,
                    per_cell: None,
                },
                GatewayLimits::default(),
            );
            // Only the instance meant to stall gets a short budget, so the
            // test waits for its stall briefly; every other instance keeps
            // the transport's default and cannot time out under load.
            let budget = match stall_at {
                Some(_) => Duration::from_secs(2),
                None => Duration::from_secs(30),
            };
            {
                let mut entrant = GatewayEntrant::new(entrant_process(stall_at), &mut gateway)
                    .with_decide_timeout(budget);
                while let Ok(observation) = inbox.recv() {
                    let decision = sharpebench_sim::Agent::decide(&mut entrant, &observation);
                    let fault = entrant.health().last_error.map(|e| format!("{e:?}"));
                    if outbox.send((decision, fault)).is_err() {
                        break;
                    }
                }
            }
            gateway.into_journal()
        });
        Self {
            run,
            observations: Some(observations),
            decisions,
            fault: None,
            worker: Some(worker),
            journals,
            crash,
        }
    }
}

impl sharpebench_sim::Agent for GatewayInstance {
    fn decide(&mut self, observation: &MarketObservation) -> Decision {
        if let Some(countdown) = &self.crash {
            if countdown.fetch_sub(1, Ordering::SeqCst) == 1 {
                panic!("the host stopped mid-bar");
            }
        }
        self.observations
            .as_ref()
            .unwrap()
            .send(observation.clone())
            .unwrap();
        let (decision, fault) = self.decisions.recv().unwrap();
        self.fault = self.fault.take().or(fault);
        decision
    }
}

impl LaunchedEntrant for GatewayInstance {
    fn fault(&self) -> Option<String> {
        self.fault.clone()
    }

    fn finish(mut self: Box<Self>) -> Result<(), String> {
        drop(self.observations.take());
        let journal = self.worker.take().unwrap().join().unwrap();
        self.journals.lock().unwrap().push((self.run, journal));
        Ok(())
    }
}

/// Starts a gateway-backed instance per run. `stall` makes the instance of
/// one run stall at one decision; `crash` panics the host after that many
/// decisions in all.
#[derive(Default)]
struct GatewayHost {
    stall: Option<(usize, usize)>,
    crash: Option<Arc<AtomicUsize>>,
    refuse: Option<String>,
    journals: Journals,
    launches: usize,
}

impl LiveHost for GatewayHost {
    fn admit(&mut self, image: &str) -> Result<(), String> {
        match &self.refuse {
            Some(refused) if refused == image => Err(format!("image `{image}` is not available")),
            _ => Ok(()),
        }
    }

    fn launch(
        &mut self,
        _image: &str,
        run: usize,
        recorder: &SharedRecorder,
    ) -> Result<Box<dyn LaunchedEntrant>, String> {
        self.launches += 1;
        let stall_at = self.stall.filter(|(r, _)| *r == run).map(|(_, at)| at);
        Ok(Box::new(GatewayInstance::spawn(
            run,
            Arc::clone(recorder),
            stall_at,
            Arc::clone(&self.journals),
            self.crash.clone(),
        )))
    }
}

/// A feed that must not be asked for anything.
struct NoFeed;

impl LiveFeed for NoFeed {
    fn bar(&mut self, index: usize) -> Result<Option<LiveBar>, String> {
        panic!("bar {index} was requested from a window that needs none")
    }
}

fn run(
    dir: &Path,
    spec: &LiveWindowSpec,
    field: &LiveEntrants,
    feed: &mut dyn LiveFeed,
    host: &mut dyn LiveHost,
    clock: Arc<dyn HostClock>,
) -> Result<LiveWindowReport, String> {
    run_live_window(LiveWindowRun {
        arena_dir: dir,
        spec,
        entrants: field,
        feed,
        host,
        clock,
        key: &key(),
    })
}

fn revealed(dir: &Path) -> Vec<RevealedEntry> {
    let path = dir.join("live").join(WINDOW).join("entries.json");
    serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
}

fn live_events(entry: &RevealedEntry) -> &[sharpebench_arena::live_capture::LiveEntry] {
    &entry.live_log.as_ref().unwrap().entries
}

#[test]
fn a_complete_live_window_certifies_every_entrant_and_its_board_verifies() {
    let dir = temp_dir("complete");
    let field = entrants(&["llm-a", "llm-b"]);
    let mut host = GatewayHost::default();
    let report = run(
        &dir,
        &spec(),
        &field,
        &mut replay_feed(),
        &mut host,
        simulated(),
    )
    .unwrap();

    assert!(report.refusals.is_empty(), "{:?}", report.refusals);
    assert!(report.certifying);
    assert_eq!(report.scored, 2);
    assert_eq!(report.verifying_key, key().verifying_key().to_hex());
    for entrant in &report.entrants {
        assert_eq!(
            report.returns_provenance.get(&entrant.agent_id),
            Some(&ReturnsProvenance::LiveCaptured)
        );
        // Two windows of 15 bars, two seeds each; one model call per bar.
        assert_eq!(
            (entrant.bars, entrant.model_calls, entrant.restarts),
            (Some(60), Some(60), Some(0))
        );
        assert!(entrant.faults.is_empty(), "{:?}", entrant.faults);
    }
    // One instance per run per entrant, each finished at its run's end with
    // every call it dispatched priced in its journal.
    assert_eq!(host.launches, 8);
    let journals = host.journals.lock().unwrap();
    assert_eq!(journals.len(), 8);
    for (_, journal) in journals.iter() {
        let spend = journal.spend();
        assert_eq!((spend.calls_started, spend.priced_calls), (15, 15));
    }
    drop(journals);

    // The revealed dataset is the feed's data, bar for bar.
    let live = dir.join("live").join(WINDOW);
    let dataset = std::fs::read(live.join("dataset.csv")).unwrap();
    let replay = ReplayWindow::parse(&dataset, spec().score_config.as_ref().unwrap()).unwrap();
    assert_eq!(
        (replay.data.dates, replay.data.closes),
        (market().dates, market().closes)
    );
    let feed_lines = std::fs::read_to_string(live.join("feed.jsonl")).unwrap();
    assert_eq!(feed_lines.lines().count(), 40);
    assert!(verify_arena(&dir, Some(&key().verifying_key())).unwrap().ok);

    // Running it again only verifies: no bar, no launch, no new byte.
    let board = std::fs::read(&report.board).unwrap();
    let mut idle = GatewayHost::default();
    let again = run(&dir, &spec(), &field, &mut NoFeed, &mut idle, simulated()).unwrap();
    assert_eq!(idle.launches, 0);
    assert_eq!(again.scored, 2);
    assert_eq!(std::fs::read(&again.board).unwrap(), board);
}

#[test]
fn a_host_that_stops_mid_window_resumes_and_the_window_still_certifies() {
    let dir = temp_dir("crash");
    let field = entrants(&["llm-a"]);
    // Bar 10 opens runs 0 and 1; the 7th decision is bar 13 of run 0.
    let mut crashing = GatewayHost {
        crash: Some(Arc::new(AtomicUsize::new(7))),
        ..GatewayHost::default()
    };
    let stopped = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        run(
            &dir,
            &spec(),
            &field,
            &mut replay_feed(),
            &mut crashing,
            simulated(),
        )
    }));
    assert!(stopped.is_err());
    let live = dir.join("live").join(WINDOW);
    let log = std::fs::read_dir(&live)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .find(|path| {
            path.file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with("log-")
        })
        .unwrap();
    let (_, entries) = LiveRecorder::read(&log).unwrap();
    assert!(matches!(
        entries.last().unwrap().event,
        LiveEvent::Observation { .. }
    ));
    // The feed line being written when the host stopped is cut short.
    let mut feed = std::fs::OpenOptions::new()
        .append(true)
        .open(live.join("feed.jsonl"))
        .unwrap();
    write!(feed, "{{\"date\":\"2025-0").unwrap();
    drop(feed);

    // A journal that does not explain the log is refused, and the refused
    // resume writes nothing to the log.
    let journal = std::fs::read(live.join("feed.jsonl")).unwrap();
    std::fs::write(live.join("feed.jsonl"), b"").unwrap();
    let error = run(
        &dir,
        &spec(),
        &field,
        &mut replay_feed(),
        &mut GatewayHost::default(),
        simulated(),
    )
    .unwrap_err();
    assert!(error.contains("do not explain"), "{error}");
    assert_eq!(LiveRecorder::read(&log).unwrap().1, entries);
    std::fs::write(live.join("feed.jsonl"), journal).unwrap();

    let mut host = GatewayHost::default();
    let report = run(
        &dir,
        &spec(),
        &field,
        &mut replay_feed(),
        &mut host,
        simulated(),
    )
    .unwrap();
    assert!(report.certifying, "{:?}", report.refusals);
    assert_eq!(
        report.returns_provenance.get("llm-a"),
        Some(&ReturnsProvenance::LiveCaptured)
    );
    assert_eq!(
        (report.entrants[0].bars, report.entrants[0].restarts),
        (Some(60), Some(1))
    );
    // The resumed host started fresh instances only for the runs still open.
    assert_eq!(host.launches, 4);
    let entries = revealed(&dir);
    let events = live_events(&entries[0]);
    let restart = events
        .iter()
        .position(|e| e.event == LiveEvent::Restart)
        .unwrap();
    // The bar that was open is shown to the fresh instance again first.
    assert_eq!((events[restart - 1].run, events[restart - 1].step), (0, 3));
    assert_eq!((events[restart + 1].run, events[restart + 1].step), (0, 3));
    assert!(matches!(
        events[restart + 1].event,
        LiveEvent::Observation { .. }
    ));
}

#[test]
fn a_stalled_entrant_holds_from_the_stall_and_its_calls_are_charged() {
    let dir = temp_dir("stall");
    let field = entrants(&["llm-a"]);
    let mut host = GatewayHost {
        stall: Some((0, 3)),
        ..GatewayHost::default()
    };
    let report = run(
        &dir,
        &spec(),
        &field,
        &mut replay_feed(),
        &mut host,
        simulated(),
    )
    .unwrap();
    assert!(report.certifying, "{:?}", report.refusals);
    assert_eq!(
        report.returns_provenance.get("llm-a"),
        Some(&ReturnsProvenance::LiveCaptured)
    );
    assert_eq!(report.entrants[0].faults, ["run 0: BudgetOverrun"]);
    // Run 0 asked the model four times, the fourth before it stalled; every
    // later bar of the run holds without a call.
    assert_eq!(report.entrants[0].model_calls, Some(4 + 3 * 15));
    // The holds from the stall on (steps 3 to 14 of run 0) are the host's,
    // sealed into the root and surfaced on the signed board's row.
    assert_eq!(report.entrants[0].stalled_holds, Some(12));
    let arena = Arena::load(&dir).unwrap();
    let window = arena.window(WINDOW).unwrap();
    assert_eq!(window.live_capture["llm-a"].stalled_holds, 12);
    let board_md =
        std::fs::read_to_string(dir.join("windows").join(WINDOW).join("board.md")).unwrap();
    assert!(
        board_md.contains("- `llm-a`: live-captured, stalled_holds 12\n"),
        "{board_md}"
    );
    assert!(board_md.contains(", 12 stalled holds\n"), "{board_md}");
    let export = sharpebench_arena::leaderboard_export::export_arena(
        &dir,
        Some(&key().verifying_key()),
        None,
    )
    .unwrap();
    assert_eq!(export.rows[0].headline.qualifiers.stalled_holds, Some(12));
    assert!(
        sharpebench_core::leaderboard_export::render_markdown(&export).contains("stalled_holds 12")
    );
    let entries = revealed(&dir);
    let run0: Vec<&Decision> = live_events(&entries[0])
        .iter()
        .filter(|e| e.run == 0)
        .filter_map(|e| match &e.event {
            LiveEvent::Decision { decision, .. } => Some(decision),
            _ => None,
        })
        .collect();
    assert_eq!(run0.len(), 15);
    assert!(run0[3..].iter().all(|d| d.orders.is_empty()));
    assert!(run0[..3].iter().all(|d| d.orders.len() == 1));
    let held: Vec<bool> = live_events(&entries[0])
        .iter()
        .filter(|e| e.run == 0)
        .filter_map(|e| match &e.event {
            LiveEvent::Decision { host_hold, .. } => Some(*host_hold),
            _ => None,
        })
        .collect();
    assert_eq!(held, [[false; 3].as_slice(), &[true; 12]].concat());
    // No other run stalled, so no other decision is a host hold.
    assert!(live_events(&entries[0])
        .iter()
        .filter(|e| e.run != 0)
        .all(|e| !matches!(
            e.event,
            LiveEvent::Decision {
                host_hold: true,
                ..
            }
        )));
    // The stalled call was dispatched and answered, so it is charged.
    let journals = host.journals.lock().unwrap();
    let (_, stalled) = journals.iter().find(|(run, _)| *run == 0).unwrap();
    let spend = stalled.spend();
    assert_eq!((spend.calls_started, spend.priced_calls), (4, 4));
    assert!(spend.priced_usd_nanos > 0);
}

/// A clock frozen at one reading.
struct Frozen(u64);

impl HostClock for Frozen {
    fn now_unix_ms(&self) -> u64 {
        self.0
    }
}

#[test]
fn a_host_clock_outside_the_bar_calendar_stops_the_window_before_more_spend() {
    let late = SimulatedClock::new(2 * 86_400_000);
    let cases: [(&str, Arc<dyn HostClock>, &str, usize); 2] = [
        ("early", Arc::new(Frozen(0)), "before its UTC day began", 0),
        (
            "late",
            Arc::new(late),
            "no live log of this window can certify",
            4,
        ),
    ];
    for (tag, clock, reason, entries_logged) in cases {
        let dir = temp_dir(&format!("clock-{tag}"));
        let mut host = GatewayHost::default();
        let error = run(
            &dir,
            &spec(),
            &entrants(&["llm-a"]),
            &mut replay_feed(),
            &mut host,
            clock,
        )
        .unwrap_err();
        assert!(error.contains(reason), "{tag}: {error}");
        // Early: nothing was observed. Late: bar 10 was decided in both of
        // its runs, and the stop came when bar 11 arrived.
        let live = dir.join("live").join(WINDOW);
        let log = std::fs::read_dir(&live)
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .find(|path| {
                path.file_name()
                    .unwrap()
                    .to_string_lossy()
                    .starts_with("log-")
            })
            .unwrap();
        let decisions = LiveRecorder::read(&log)
            .unwrap()
            .1
            .iter()
            .filter(|e| matches!(e.event, LiveEvent::Decision { .. }))
            .count();
        assert_eq!(decisions * 2, entries_logged, "{tag}");
    }
}

#[test]
fn a_window_whose_spec_field_or_feed_is_wrong_is_refused_with_its_reason() {
    type Edit = fn(&mut LiveWindowSpec, &mut LiveEntrants, &mut GatewayHost);
    let cases: [(&str, Edit, &str); 9] = [
        (
            "schema",
            |s, _, _| s.schema = "v0".into(),
            "live window spec schema",
        ),
        ("short", |s, _, _| s.bars = 39, "replay needs at least 40"),
        (
            "deadline",
            |s, _, _| s.commit_deadline = 30,
            "after the data reveal",
        ),
        (
            "field schema",
            |_, f, _| f.schema = "v0".into(),
            "live entrants schema",
        ),
        (
            "no entrant",
            |_, f, _| f.entrants.clear(),
            "names no entrant",
        ),
        (
            "twice",
            |_, f, _| f.entrants.push(f.entrants[0].clone()),
            "listed twice",
        ),
        ("salt", |_, f, _| f.entrants[0].salt.clear(), "empty salt"),
        (
            "unpinned",
            |_, f, _| f.entrants[0].image = "test/llm:latest".into(),
            "not a digest-pinned image",
        ),
        (
            "unavailable",
            |_, f, h| h.refuse = Some(f.entrants[0].image.clone()),
            "is not available",
        ),
    ];
    for (tag, edit, reason) in cases {
        let dir = temp_dir(&format!("refuse-{}", tag.replace(' ', "-")));
        let (mut spec, mut field, mut host) =
            (spec(), entrants(&["llm-a"]), GatewayHost::default());
        edit(&mut spec, &mut field, &mut host);
        let error = run(&dir, &spec, &field, &mut NoFeed, &mut host, simulated()).unwrap_err();
        assert!(error.contains(reason), "{tag}: {error}");
        // Refused before anything was written.
        assert!(!dir.join("state.json").exists(), "{tag}");
    }
}

#[test]
fn a_resume_keeps_the_spec_field_and_commitments_the_window_opened_with() {
    let dir = temp_dir("resume-fixed");
    let field = entrants(&["llm-a"]);
    // The feed ends before the window's bars: the run stops at bar 20.
    let short = market();
    let short = Dataset {
        dates: short.dates[..20].to_vec(),
        closes: short
            .closes
            .iter()
            .map(|(s, c)| (s.clone(), c[..20].to_vec()))
            .collect(),
        ..short
    };
    let mut feed = ReplayFeed::from_csv(&csv(&short)).unwrap();
    let error = run(
        &dir,
        &spec(),
        &field,
        &mut feed,
        &mut GatewayHost::default(),
        simulated(),
    )
    .unwrap_err();
    assert!(
        error.contains("the feed ended at bar 20, before the window's 40 bars"),
        "{error}"
    );

    let mut other_scorer = spec();
    other_scorer.scorer_artifact_sha256 = content_digest(b"another scorer");
    let mut other_bars = spec();
    other_bars.bars = 41;
    let mut other_salt = field.clone();
    other_salt.entrants[0].salt = "another".into();
    let more = entrants(&["llm-a", "llm-b"]);
    let cases: [(&str, &LiveWindowSpec, &LiveEntrants, &str); 4] = [
        ("scorer", &other_scorer, &field, "exists with another"),
        ("bars", &other_bars, &field, "live window spec differs"),
        ("salt", &spec(), &other_salt, "manifest differs"),
        ("field", &spec(), &more, "manifest differs"),
    ];
    for (tag, spec, field, reason) in cases {
        let error = run(
            &dir,
            spec,
            field,
            &mut replay_feed(),
            &mut GatewayHost::default(),
            simulated(),
        )
        .unwrap_err();
        assert!(error.contains(reason), "{tag}: {error}");
    }
    let report = run(
        &dir,
        &spec(),
        &field,
        &mut replay_feed(),
        &mut GatewayHost::default(),
        simulated(),
    )
    .unwrap();
    assert!(report.certifying);
    assert_eq!(report.entrants[0].restarts, Some(1));
    let arena = Arena::load(&dir).unwrap();
    assert_eq!(arena.current_epoch(), 20);
}

#[test]
fn a_window_committed_to_another_field_is_not_run() {
    let field = entrants(&["llm-a"]);
    let digest = content_digest(b"live-window-image-llm-a");
    let cases: [(&str, &str, &str, &str); 2] = [
        (
            "salt",
            "llm-a",
            "another salt",
            "holds another commitment for `llm-a`",
        ),
        (
            "stranger",
            "stranger",
            "salt",
            "the live entrants manifest lists",
        ),
    ];
    for (tag, agent, salt, reason) in cases {
        let dir = temp_dir(&format!("committed-{tag}"));
        let spec = spec();
        let mut arena = Arena::init(&dir).unwrap();
        arena
            .open_window_with_fault_plan(
                WINDOW,
                spec.commit_deadline,
                spec.data_reveal_epoch,
                spec.score_config.clone().unwrap(),
                None,
                spec.scorer_artifact_sha256.clone(),
                None,
            )
            .unwrap();
        arena
            .register_entry(
                WINDOW,
                sharpebench_attest::make_commitment(agent, WINDOW, &digest, salt),
            )
            .unwrap();
        let mut host = GatewayHost::default();
        let error = run(&dir, &spec, &field, &mut NoFeed, &mut host, simulated()).unwrap_err();
        assert!(error.contains(reason), "{tag}: {error}");
        assert_eq!(host.launches, 0, "{tag}");
    }
}

#[test]
fn the_clock_checks_allow_exactly_the_two_minute_tolerance() {
    use sharpebench_arena::live_capture::{bar_day_start_ms, MAX_CLOCK_SKEW_MS};
    // A host clock frozen two minutes before the first decided bar's day
    // (bar 10, `2025-011`) may hand that bar out, and stops at the next one.
    let day = bar_day_start_ms("2025-011").unwrap();
    let dir = temp_dir("clock-edge");
    let error = run(
        &dir,
        &spec(),
        &entrants(&["llm-a"]),
        &mut replay_feed(),
        &mut GatewayHost::default(),
        Arc::new(Frozen(day - MAX_CLOCK_SKEW_MS)),
    )
    .unwrap_err();
    assert!(error.contains("bar `2025-012` arrived"), "{error}");

    // Bars handed out at 23:59 UTC are decided within the tolerance of the
    // next bar's day, and the window certifies.
    let dir = temp_dir("clock-late-in-day");
    let report = run(
        &dir,
        &spec(),
        &entrants(&["llm-a"]),
        &mut replay_feed(),
        &mut GatewayHost::default(),
        Arc::new(StandInHostClock(SimulatedClock::new(86_400_000 - 60_000))),
    )
    .unwrap();
    assert!(report.certifying, "{:?}", report.refusals);
}

/// The window's bars as feed journal lines, `n` of them.
fn journal_lines(n: usize) -> String {
    let data = Dataset::synthetic_parameterized(3, n, 5, 3.0, 0.0, 0.0);
    let mut feed = ReplayFeed::from_csv(&csv(&data)).unwrap();
    (0..n)
        .map(|t| serde_json::to_string(&feed.bar(t).unwrap().unwrap()).unwrap() + "\n")
        .collect()
}

#[test]
fn a_host_stopped_on_the_last_bar_resumes_without_the_feed_and_an_overlong_journal_is_refused() {
    let field = entrants(&["llm-a"]);
    // The 60th decision is the last bar of the last run: every bar is in
    // the journal when the host stops.
    let dir = temp_dir("journal-full");
    let mut crashing = GatewayHost {
        crash: Some(Arc::new(AtomicUsize::new(60))),
        ..GatewayHost::default()
    };
    let stopped = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        run(
            &dir,
            &spec(),
            &field,
            &mut replay_feed(),
            &mut crashing,
            simulated(),
        )
    }));
    assert!(stopped.is_err());
    let report = run(
        &dir,
        &spec(),
        &field,
        &mut NoFeed,
        &mut GatewayHost::default(),
        simulated(),
    )
    .unwrap();
    assert!(report.certifying, "{:?}", report.refusals);
    assert_eq!(report.entrants[0].restarts, Some(1));

    let dir = temp_dir("journal-over");
    let live = dir.join("live").join(WINDOW);
    std::fs::create_dir_all(&live).unwrap();
    std::fs::write(live.join("feed.jsonl"), journal_lines(41)).unwrap();
    let error = run(
        &dir,
        &spec(),
        &field,
        &mut NoFeed,
        &mut GatewayHost::default(),
        simulated(),
    )
    .unwrap_err();
    assert!(
        error.contains("holds 41 bars, the window runs 40"),
        "{error}"
    );
}

/// Re-sign a published board after editing its signed payloads, as a key
/// holder could, with the window record and head rewritten to match, so a
/// verifier fails it on what the edit means and not on a broken signature.
fn resign_board(
    dir: &Path,
    edit: impl FnOnce(&mut Vec<serde_json::Value>, &mut serde_json::Value),
) {
    use sharpebench_arena::{head_digest, ArenaHead, ARENA_HEAD_KIND, HEAD_FILE};
    use sharpebench_attest::{publish_public_chain, PublicChain};
    let base = dir.join("windows").join(WINDOW);
    let board: PublicChain =
        serde_json::from_slice(&std::fs::read(base.join("board.json")).unwrap()).unwrap();
    let mut payloads: Vec<serde_json::Value> = board
        .chain
        .iter()
        .map(|link| serde_json::from_str(&link.payload).unwrap())
        .collect();
    let mut window: serde_json::Value =
        serde_json::from_slice(&std::fs::read(base.join("window.json")).unwrap()).unwrap();
    edit(&mut payloads, &mut window);
    std::fs::write(
        base.join("window.json"),
        serde_json::to_vec(&window).unwrap(),
    )
    .unwrap();
    let strings: Vec<String> = payloads
        .iter()
        .map(|value| serde_json::to_string(value).unwrap())
        .collect();
    let board = publish_public_chain(&strings, &key());
    std::fs::write(base.join("board.json"), serde_json::to_vec(&board).unwrap()).unwrap();
    let head = ArenaHead {
        kind: ARENA_HEAD_KIND.to_string(),
        published_count: 1,
        head_window_id: WINDOW.to_string(),
        head_digest: head_digest(&board.chain.last().unwrap().signature),
    };
    let document = publish_public_chain(&[serde_json::to_string(&head).unwrap()], &key());
    std::fs::write(dir.join(HEAD_FILE), serde_json::to_vec(&document).unwrap()).unwrap();
}

#[test]
fn a_board_run_under_the_simulated_clock_is_a_rehearsal_and_never_certifies() {
    let dir = temp_dir("rehearsal");
    let field = entrants(&["llm-a", "llm-b"]);
    let mut host = GatewayHost::default();
    let report = run(
        &dir,
        &spec(),
        &field,
        &mut replay_feed(),
        &mut host,
        rehearsal(),
    )
    .unwrap();
    assert!(report.simulated_clock);
    assert!(!report.certifying);
    assert_eq!(report.scored, 2);
    for entrant in &report.entrants {
        assert_eq!(
            report.returns_provenance.get(&entrant.agent_id),
            Some(&ReturnsProvenance::LiveRehearsal)
        );
    }
    // Every log header carries the mark, and every root records it.
    for entry in revealed(&dir) {
        assert!(entry.live_log.as_ref().unwrap().header.simulated_clock);
    }
    let arena = Arena::load(&dir).unwrap();
    let window = arena.window(WINDOW).unwrap();
    assert_eq!(window.certifying, Some(false));
    assert!(window
        .live_capture
        .values()
        .all(|root| root.simulated_clock));
    // The board verifies as what it is: a noncertifying rehearsal.
    assert!(verify_arena(&dir, Some(&key().verifying_key())).unwrap().ok);
    let board_md =
        std::fs::read_to_string(dir.join("windows").join(WINDOW).join("board.md")).unwrap();
    assert!(
        board_md.contains("Evaluation tier: `historical-advisory`"),
        "{board_md}"
    );
    assert!(board_md.contains("**Noncertifying board.**"), "{board_md}");
    assert!(
        board_md.contains("A row marked `live-rehearsal`"),
        "{board_md}"
    );
    assert!(board_md.contains(", simulated clock\n"), "{board_md}");
    let export = sharpebench_arena::leaderboard_export::export_arena(
        &dir,
        Some(&key().verifying_key()),
        None,
    )
    .unwrap();
    for row in &export.rows {
        assert_eq!(
            row.tier,
            sharpebench_core::EvaluationTier::HistoricalAdvisory
        );
        assert_eq!(row.returns_provenance.as_deref(), Some("live-rehearsal"));
    }

    // Publishing the rehearsal as a certifying board is refused: a re-signed
    // header that claims certification fails verification on what its rows
    // derive.
    let claim = temp_dir("rehearsal-claim");
    copy_tree(&dir, &claim);
    resign_board(&claim, |payloads, window| {
        payloads[0]["certifying"] = serde_json::json!(true);
        window["certifying"] = serde_json::json!(true);
    });
    let verdict = verify_arena(&claim, Some(&key().verifying_key())).unwrap();
    assert!(!verdict.ok);
    assert!(
        verdict.windows[0]
            .detail
            .contains("signed rows derive false"),
        "{verdict:?}"
    );
    // Relabelling the rows as live-captured custody is refused too: each
    // root records the simulated clock its log was run under.
    let relabel = temp_dir("rehearsal-relabel");
    copy_tree(&dir, &relabel);
    resign_board(&relabel, |payloads, window| {
        payloads[0]["certifying"] = serde_json::json!(true);
        window["certifying"] = serde_json::json!(true);
        for row in payloads.iter_mut().skip(1) {
            row["returns_provenance"] = serde_json::json!("live-captured");
        }
        for (_, provenance) in window["returns_provenance"].as_object_mut().unwrap() {
            *provenance = serde_json::json!("live-captured");
        }
    });
    let verdict = verify_arena(&relabel, Some(&key().verifying_key())).unwrap();
    assert!(!verdict.ok);
    assert!(
        verdict.windows[0]
            .detail
            .contains("records simulated_clock true"),
        "{verdict:?}"
    );

    // A window opened under the rehearsal clock cannot be resumed under a
    // host clock: the logs' headers would change.
    let mixed = temp_dir("rehearsal-mixed");
    let mut stop = GatewayHost {
        crash: Some(Arc::new(AtomicUsize::new(7))),
        ..GatewayHost::default()
    };
    let crashed = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        run(
            &mixed,
            &spec(),
            &field,
            &mut replay_feed(),
            &mut stop,
            rehearsal(),
        )
    }));
    assert!(crashed.is_err());
    let error = run(
        &mixed,
        &spec(),
        &field,
        &mut replay_feed(),
        &mut GatewayHost::default(),
        simulated(),
    )
    .unwrap_err();
    assert!(error.contains("has another header"), "{error}");
}

fn copy_tree(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for entry in std::fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let target = to.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_tree(&entry.path(), &target);
        } else {
            std::fs::copy(entry.path(), target).unwrap();
        }
    }
}
