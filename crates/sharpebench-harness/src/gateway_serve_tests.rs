//! End to end through the serving loop: a scripted entrant on real OS pipes, a
//! scripted provider behind the transport seam, a real sweep in between. No
//! provider, no key and no network anywhere.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use sharpebench_protocol::{Action, Order};

use super::*;
use crate::gateway::{
    GatewayRequest, GatewayResponse, Message, MessageRole, ModelRoute, ProviderBody, ProviderCall,
    ProviderOutcome, ProviderUsage, Secret, GATEWAY_PROTOCOL,
};
use crate::scratch::ScratchDir;

const KEY: &str = "sk-live-serve-test-do-not-log-0123456789";
const ALIAS: &str = "fake.v1";

fn card() -> RateCard {
    RateCard::from_json(
        br#"{"schema_version":"sharpebench.token-rate-card.v1","provider":"fake","model":"fake-1","revision":"2026-01-01","input_usd_nanos_per_token":1,"output_usd_nanos_per_token":1}"#,
    )
    .expect("a valid test rate card")
}

fn table() -> RouteTable {
    RouteTable::new(vec![ModelRoute::new(
        ALIAS,
        "https://provider.invalid/v1/messages",
        Secret::new(KEY),
        card(),
        4096,
        8,
    )
    .expect("a valid route")])
    .expect("a valid table")
}

fn budget(max_usd_nanos: u128, max_calls: u32) -> GatewayBudget {
    GatewayBudget {
        max_usd_nanos,
        max_calls,
    }
}

fn temp_dir(tag: &str) -> ScratchDir {
    ScratchDir::new(&format!("gateway-serve-{tag}"))
}

/// A scripted provider. It cannot open a socket; it records the credential it
/// was handed and answers from its script.
#[derive(Clone)]
struct ScriptedProvider {
    text: String,
    usage: Option<(u64, u64)>,
    delay: Duration,
    calls: Arc<Mutex<Vec<String>>>,
}

impl ScriptedProvider {
    fn answering(text: &str) -> Self {
        Self {
            text: text.to_string(),
            usage: Some((10, 5)),
            delay: Duration::ZERO,
            calls: Arc::default(),
        }
    }

    fn calls(&self) -> usize {
        self.calls.lock().expect("calls lock").len()
    }
}

impl ProviderTransport for ScriptedProvider {
    fn call(&mut self, call: ProviderCall<'_>) -> ProviderOutcome {
        self.calls
            .lock()
            .expect("calls lock")
            .push(call.credential.expose().to_string());
        std::thread::sleep(self.delay);
        ProviderOutcome::Answered {
            status: 200,
            body: serde_json::to_vec(&ProviderBody {
                text: self.text.clone(),
                finish_reason: Some("stop".into()),
                usage: self
                    .usage
                    .map(|(input_tokens, output_tokens)| ProviderUsage {
                        input_tokens,
                        output_tokens,
                    }),
            })
            .expect("a provider body serializes"),
        }
    }
}

/// An in-process entrant on a real OS pipe pair. The host sees exactly what it
/// sees from a process: bytes on stdin and stdout.
struct ThreadEntrant(std::thread::JoinHandle<()>);

impl EntrantProcess for ThreadEntrant {
    fn exited(&mut self) -> Option<Option<i32>> {
        self.0.is_finished().then_some(Some(0))
    }

    fn terminate(&mut self) {}
}

fn in_thread(
    script: impl FnOnce(&mut dyn BufRead, &mut dyn Write) + Send + 'static,
) -> EntrantPipes {
    let (host_reads, entrant_writes) = std::io::pipe().expect("a pipe");
    let (entrant_reads, host_writes) = std::io::pipe().expect("a pipe");
    let handle = std::thread::spawn(move || {
        let mut input = BufReader::new(entrant_reads);
        let mut output = entrant_writes;
        script(&mut input, &mut output);
    });
    EntrantPipes::new(
        Box::new(host_writes),
        Box::new(host_reads),
        Box::new(ThreadEntrant(handle)),
    )
}

fn request_line(content: &str) -> String {
    serde_json::to_string(&GatewayRequest {
        protocol: GATEWAY_PROTOCOL.to_string(),
        model_alias: ALIAS.into(),
        messages: vec![Message {
            role: MessageRole::User,
            content: content.into(),
        }],
        max_output_tokens: 16,
        tools: Vec::new(),
    })
    .expect("a request serializes")
}

type Log = Arc<Mutex<Vec<GatewayResponse>>>;

/// The scripted entrant: for each observation it asks the model `requests`
/// times, records every answer it got back, and sizes a position in the first
/// symbol from the last successful answer. A refusal leaves it flat.
fn model_entrant(
    log: Log,
    requests: usize,
) -> impl FnOnce(&mut dyn BufRead, &mut dyn Write) + Send + 'static {
    move |input, output| loop {
        let mut line = String::new();
        if input.read_line(&mut line).unwrap_or(0) == 0 {
            return;
        }
        let observation: MarketObservation =
            serde_json::from_str(&line).expect("the host writes observations");
        let mut weight = 0.0;
        for _ in 0..requests {
            if writeln!(output, "{}", request_line(&observation.date)).is_err()
                || output.flush().is_err()
            {
                return;
            }
            let mut answer = String::new();
            if input.read_line(&mut answer).unwrap_or(0) == 0 {
                return;
            }
            let answer: GatewayResponse =
                serde_json::from_str(&answer).expect("the host writes gateway responses");
            weight = match (answer.ok, &answer.text) {
                (true, Some(text)) => text.parse().unwrap_or(0.0),
                _ => 0.0,
            };
            log.lock().expect("log lock").push(answer);
        }
        let orders = if weight == 0.0 {
            Vec::new()
        } else {
            vec![Order {
                symbol: observation.symbols[0].symbol.clone(),
                action: Action::Buy,
                target_weight: weight,
                confidence: 0.5,
                rationale: String::new(),
            }]
        };
        let decision = Decision {
            orders,
            reasoning: String::new(),
            cost: None,
        };
        if writeln!(
            output,
            "{}",
            serde_json::to_string(&decision).expect("a decision serializes")
        )
        .is_err()
            || output.flush().is_err()
        {
            return;
        }
    }
}

fn observation() -> MarketObservation {
    serde_json::from_str(
        r#"{"date":"2026-01-02","cash":1000.0,"symbols":[{"symbol":"AAA","close_history":[1.0,1.1]}],"portfolio":[]}"#,
    )
    .expect("a valid observation")
}

fn identity() -> SweepIdentity {
    SweepIdentity {
        dataset_sha256: "a".repeat(64),
        cost_model_sha256: "b".repeat(64),
        score_config_sha256: "c".repeat(64),
        runner_artifact_sha256: "d".repeat(64),
        entrant_sha256: "e".repeat(64),
        invocation_sha256: "f".repeat(64),
    }
}

const WINDOWS: [Window; 1] = [Window { start: 20, end: 26 }];
const SEEDS: [u64; 2] = [0, 1];
/// Decisions in one cell, so model calls per cell at one request each.
const STEPS: usize = 6;

struct Fixture {
    dir: ScratchDir,
    data: Dataset,
    routes: RouteTable,
    permits: CallPermits,
}

impl Fixture {
    fn new(tag: &str) -> Self {
        Self {
            dir: temp_dir(tag),
            data: Dataset::synthetic(4, 60, 7),
            routes: table(),
            permits: CallPermits::new(4),
        }
    }

    fn checkpoint(&self) -> PathBuf {
        self.dir.join("checkpoint.json")
    }

    fn journal(&self) -> PathBuf {
        self.dir.join("journal.json")
    }

    fn sweep<'a>(&'a self, checkpoint: &'a Path, journal: &'a Path) -> GatewaySweep<'a> {
        GatewaySweep {
            checkpoint,
            journal,
            agent_id: "gateway:scripted",
            identity: identity(),
            windows: &WINDOWS,
            seeds: &SEEDS,
            max_retries: 0,
            policy: ResumePolicy::UnfinishedOnly,
        }
    }

    fn host(
        &self,
        provider: ScriptedProvider,
        budget: GatewayBudget,
    ) -> GatewayHost<'_, ScriptedProvider> {
        GatewayHost {
            routes: &self.routes,
            permits: &self.permits,
            transport: provider,
            budget,
            limits: GatewayLimits::default(),
        }
    }

    /// Run the sweep with the scripted entrant, one model request per decision.
    fn run(
        &self,
        provider: ScriptedProvider,
        budget: GatewayBudget,
        log: &Log,
    ) -> std::io::Result<GatewaySweepOutcome> {
        let checkpoint = self.checkpoint();
        let journal = self.journal();
        run_gateway_sweep(
            self.sweep(&checkpoint, &journal),
            self.host(provider, budget),
            |window, seed, gateway| {
                gateway_backtest(
                    &self.data,
                    in_thread(model_entrant(log.clone(), 1)),
                    gateway,
                    WINDOWS[window],
                    seed,
                    CostModel::default(),
                    None,
                )
            },
        )
    }
}

fn outcome(result: std::io::Result<GatewaySweepOutcome>) -> GatewaySweepOutcome {
    match result {
        Ok(outcome) => outcome,
        Err(error) => panic!("the gateway sweep failed: {error}"),
    }
}

fn refusal(result: std::io::Result<GatewaySweepOutcome>) -> std::io::Error {
    match result {
        Ok(_) => panic!("the gateway sweep was admitted"),
        Err(error) => error,
    }
}

fn returns(outcome: &GatewaySweepOutcome) -> Vec<Vec<u64>> {
    outcome
        .result
        .submission
        .runs
        .iter()
        .map(|run| run.returns.iter().map(|value| value.to_bits()).collect())
        .collect()
}

/// One observation, one model call through the entrant's own pipe, one
/// decision sized from the model's answer. The provider saw the host's
/// credential; the entrant saw an ordinal and the text, nothing else.
#[test]
fn an_entrant_reaches_the_model_through_its_own_pipe() {
    let routes = table();
    let permits = CallPermits::new(4);
    let provider = ScriptedProvider::answering("0.25");
    let mut gateway = ModelGateway::new(
        &routes,
        &permits,
        provider.clone(),
        budget(1_000_000, 10),
        GatewayLimits::default(),
    );
    let log = Log::default();
    let decision = {
        let mut entrant =
            GatewayEntrant::new(in_thread(model_entrant(log.clone(), 1)), &mut gateway);
        let decision = entrant.decide(&observation());
        assert!(!entrant.health().degraded(), "the exchange was clean");
        assert_eq!(entrant.requests_served(), 1);
        decision
    };
    assert_eq!(decision.orders.len(), 1);
    assert_eq!(decision.orders[0].target_weight, 0.25);
    assert_eq!(provider.calls(), 1);
    assert_eq!(provider.calls.lock().expect("calls lock")[0], KEY);
    assert_eq!(gateway.journal().spend().priced_calls, 1);
    let answers = log.lock().expect("log lock");
    assert_eq!(answers.len(), 1);
    assert!(answers[0].ok);
    assert_eq!(answers[0].ordinal, Some(0));
    let wire = serde_json::to_string(&answers[0]).expect("serializes");
    assert!(!wire.contains(KEY) && !wire.contains("provider.invalid"));
}

/// The delivered-artifact leak gate. A provider (or a proxy in front of it)
/// that echoes a credential, a destination or the journal's location into the
/// model text does not get it to the entrant: the answer is withheld with a
/// typed refusal. The call still happened, so it is still charged.
#[test]
fn host_material_in_an_answer_never_reaches_the_entrant() {
    let dir = temp_dir("leak");
    let journal = dir.join("journal.json");
    for echoed in [
        format!("the key is {KEY}"),
        "see https://provider.invalid/v1/messages".to_string(),
        format!("spend is kept at {}", journal.display()),
    ] {
        std::fs::remove_file(&journal).ok();
        let routes = table();
        let permits = CallPermits::new(4);
        let provider = ScriptedProvider::answering(&echoed);
        let mut gateway = ModelGateway::open(
            &routes,
            &permits,
            provider.clone(),
            budget(1_000_000, 10),
            GatewayLimits::default(),
            &journal,
        )
        .expect("a fresh journal opens");
        let log = Log::default();
        {
            let mut entrant =
                GatewayEntrant::new(in_thread(model_entrant(log.clone(), 1)), &mut gateway);
            entrant.decide(&observation());
            assert!(!entrant.health().degraded());
        }
        let answers = log.lock().expect("log lock");
        assert_eq!(
            answers[0].error.as_ref().map(|error| error.kind),
            Some(GatewayErrorKind::ResponseWithheld),
            "{echoed}"
        );
        assert!(answers[0].text.is_none());
        let wire = serde_json::to_string(&answers[0]).expect("serializes");
        assert!(!wire.contains(&echoed), "{wire}");
        assert_eq!(provider.calls(), 1);
        assert_eq!(gateway.journal().spend().priced_calls, 1, "still charged");
    }
}

/// The whole path: a real sweep, an entrant making a model call on every
/// decision, the journal recording each one, and the scored output carrying
/// the host-observed usage on the entrant's row, beside the rank.
#[test]
fn a_gateway_sweep_publishes_host_observed_usage_beside_the_rank() {
    let fixture = Fixture::new("e2e");
    let provider = ScriptedProvider::answering("0.25");
    let log = Log::default();
    let outcome = outcome(fixture.run(provider.clone(), budget(1_000_000, 100), &log));

    let cells = WINDOWS.len() * SEEDS.len();
    assert_eq!(outcome.result.submission.runs.len(), cells);
    assert!(outcome.result.failures.is_empty());
    assert_eq!(provider.calls(), cells * STEPS, "one call per decision");
    assert_eq!(log.lock().expect("log lock").len(), cells * STEPS);

    let on_disk = GatewayJournal::load_bound(
        &fixture.journal(),
        &JournalIdentity::new(fixture.routes.identity_digest(), budget(1_000_000, 100))
            .for_sweep(outcome.host_observed.sweep_sha256.clone()),
    )
    .expect("the journal on disk is bound to this sweep");
    assert_eq!(on_disk.spend().calls_started as usize, cells * STEPS);

    let usage = &outcome.host_observed;
    assert_eq!(usage.calls_started as usize, cells * STEPS);
    assert_eq!(usage.priced_calls, usage.calls_started);
    assert_eq!(
        usage.monetary_cost,
        MonetarySummary::Estimated {
            rate_card: card(),
            rate_card_sha256: card().digest(),
            usage_source: "host_observed",
            usd_nanos: (15 * cells * STEPS).to_string(),
        }
    );
    assert!(usage.rank_neutral && !usage.ceiling_breached && !usage.journal_ownership_lost);
    // The entrant-reported estimate is a separate record and stays empty here.
    assert!(matches!(
        outcome.result.monetary_cost,
        MonetarySummary::Unavailable { .. }
    ));

    let config = sharpebench_core::ScoreConfig {
        execution_seeds_per_window: SEEDS.len(),
        ..sharpebench_core::ScoreConfig::default()
    };
    let field = vec![
        outcome.result.submission.clone(),
        crate::run_agent(
            "buy-and-hold",
            &fixture.data,
            &WINDOWS,
            &SEEDS,
            CostModel::default(),
            || Box::new(sharpebench_sim::BuyAndHold) as Box<dyn Agent>,
        ),
    ];
    let board = sharpebench_core::rank(&field, &config);
    let plain = serde_json::to_value(&board).expect("a board serializes");
    let mut published = plain.clone();
    attach_host_observed_usage(&mut published, "gateway:scripted", usage);
    let rows = published.as_array().expect("the board stays an array");
    let row = rows
        .iter()
        .find(|row| row["agent_id"] == "gateway:scripted")
        .expect("the entrant is ranked");
    assert_eq!(row["host_observed_usage"]["calls_started"], 12);
    assert_eq!(
        row["host_observed_usage"]["monetary_cost"]["usage_source"],
        "host_observed"
    );
    let mut stripped = published.clone();
    for row in stripped.as_array_mut().expect("an array") {
        row.as_object_mut()
            .expect("a row")
            .remove("host_observed_usage");
    }
    assert_eq!(
        stripped, plain,
        "the usage sits beside the rank, never in it"
    );
}

/// Resuming a finished sweep makes no call at all, rewrites nothing and
/// reports exactly what the first run reported.
#[test]
fn a_resumed_gateway_sweep_makes_no_new_calls() {
    let fixture = Fixture::new("resume");
    let log = Log::default();
    let first = outcome(fixture.run(
        ScriptedProvider::answering("0.25"),
        budget(1_000_000, 100),
        &log,
    ));
    let journal_bytes = std::fs::read(fixture.journal()).expect("journal");

    let idle = ScriptedProvider::answering("0.99");
    let again = Log::default();
    let second = outcome(fixture.run(idle.clone(), budget(1_000_000, 100), &again));
    assert_eq!(idle.calls(), 0, "a resume starts no provider call");
    assert!(again.lock().expect("log lock").is_empty());
    assert_eq!(
        std::fs::read(fixture.journal()).expect("journal"),
        journal_bytes
    );
    assert_eq!(second.host_observed, first.host_observed);
    assert_eq!(returns(&second), returns(&first));
}

/// A crash mid-sweep: the finished cell is not re-run, the interrupted cell is,
/// and the calls the interrupted cell already made stay charged.
#[test]
fn an_interrupted_gateway_sweep_reruns_only_unfinished_cells_and_keeps_their_spend() {
    let fixture = Fixture::new("crash");
    let log = Log::default();
    let checkpoint = fixture.checkpoint();
    let journal = fixture.journal();
    let crashed = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let mut cells = 0;
        run_gateway_sweep(
            fixture.sweep(&checkpoint, &journal),
            fixture.host(ScriptedProvider::answering("0.25"), budget(1_000_000, 100)),
            |window, seed, gateway| {
                let observed = gateway_backtest(
                    &fixture.data,
                    in_thread(model_entrant(log.clone(), 1)),
                    gateway,
                    WINDOWS[window],
                    seed,
                    CostModel::default(),
                    None,
                );
                cells += 1;
                assert!(cells < 2, "the process dies after the second cell's calls");
                observed
            },
        )
    }));
    assert!(crashed.is_err());

    let resumed_provider = ScriptedProvider::answering("0.25");
    let resumed = outcome(fixture.run(resumed_provider.clone(), budget(1_000_000, 100), &log));
    assert_eq!(
        resumed_provider.calls(),
        STEPS,
        "only the interrupted cell runs"
    );
    assert_eq!(
        resumed.host_observed.calls_started as usize,
        3 * STEPS,
        "the interrupted cell's first calls stay charged"
    );

    let clean = Fixture::new("crash-baseline");
    let baseline = outcome(clean.run(
        ScriptedProvider::answering("0.25"),
        budget(1_000_000, 100),
        &Log::default(),
    ));
    assert_eq!(returns(&resumed), returns(&baseline));
}

/// A refused call is not a silent zero: the entrant receives the refusal as a
/// typed error on its own pipe, and nothing past the ceiling reaches the wire.
#[test]
fn a_budget_refusal_reaches_the_entrant_as_a_typed_error() {
    let fixture = Fixture::new("budget");
    // Each call reserves 8 content bytes (the date) + 8 overhead + 16 output
    // = 32 and settles at 15, so under 60 two calls fit and the third, with 30
    // left, is refused.
    let provider = ScriptedProvider::answering("0.25");
    let log = Log::default();
    let refused = outcome(fixture.run(provider.clone(), budget(60, 100), &log));
    let answers = log.lock().expect("log lock");
    assert_eq!(answers.len(), WINDOWS.len() * SEEDS.len() * STEPS);
    assert!(answers[..2].iter().all(|answer| answer.ok));
    for answer in &answers[2..] {
        assert!(!answer.ok);
        let error = answer.error.as_ref().expect("a refusal carries its kind");
        assert_eq!(error.kind, GatewayErrorKind::BudgetExhausted);
        assert_eq!(error.detail, GatewayErrorKind::BudgetExhausted.detail());
    }
    assert_eq!(provider.calls(), 2);
    assert_eq!(refused.host_observed.calls_started, 2);
    assert!(
        refused.result.failures.is_empty(),
        "the entrant chose to hold"
    );

    let calls = Fixture::new("calls");
    let provider = ScriptedProvider::answering("0.25");
    let log = Log::default();
    outcome(calls.run(provider.clone(), budget(1_000_000, 3), &log));
    let answers = log.lock().expect("log lock");
    assert!(answers[3..].iter().all(|answer| answer
        .error
        .as_ref()
        .is_some_and(|error| error.kind == GatewayErrorKind::CallLimitExhausted)));
    assert_eq!(provider.calls(), 3);
}

/// The journal and the checkpoint are one record. Either one without the
/// other, a journal from another sweep, and a changed route budget are all
/// refused before any entrant runs.
#[test]
fn the_journal_and_the_checkpoint_are_bound_together() {
    let fixture = Fixture::new("pair");
    let log = Log::default();
    let bound = outcome(fixture.run(
        ScriptedProvider::answering("0.25"),
        budget(1_000_000, 100),
        &log,
    ));

    // The checkpoint carries the gateway binding in its invocation identity:
    // the same sweep without the gateway is a different experiment.
    let unbound = SweepContract::new(identity(), &WINDOWS, &SEEDS, 0);
    assert_ne!(unbound, bound.contract);
    let resume = |contract: &SweepContract| {
        crate::run_resumable_sweep_observed(
            &fixture.checkpoint(),
            "gateway:scripted",
            contract,
            &WINDOWS,
            ResumePolicy::UnfinishedOnly,
            |_, _| unreachable!("a finished checkpoint runs no cell"),
        )
        .map(|_| ())
    };
    assert_eq!(
        resume(&unbound).map_err(|error| error.kind()),
        Err(std::io::ErrorKind::InvalidData)
    );
    assert!(resume(&bound.contract).is_ok());
    let checkpoint_bytes = std::fs::read(fixture.checkpoint()).expect("checkpoint");
    let journal_bytes = std::fs::read(fixture.journal()).expect("journal");

    // A changed budget changes the invocation identity: the checkpoint refuses.
    let changed = ScriptedProvider::answering("0.25");
    let error = refusal(fixture.run(changed.clone(), budget(2_000_000, 100), &log));
    assert_eq!(error.kind(), std::io::ErrorKind::InvalidData);
    assert_eq!(changed.calls(), 0);

    // A checkpoint whose journal is gone.
    std::fs::remove_file(fixture.journal()).expect("remove journal");
    let error = refusal(fixture.run(
        ScriptedProvider::answering("0.25"),
        budget(1_000_000, 100),
        &log,
    ));
    assert!(error.to_string().contains("journal is missing"), "{error}");
    assert!(!fixture.journal().exists(), "the refusal wrote no journal");

    // A journal with calls whose checkpoint is gone.
    std::fs::write(fixture.journal(), &journal_bytes).expect("restore journal");
    std::fs::remove_file(fixture.checkpoint()).expect("remove checkpoint");
    let error = refusal(fixture.run(
        ScriptedProvider::answering("0.25"),
        budget(1_000_000, 100),
        &log,
    ));
    assert!(
        error.to_string().contains("checkpoint is missing"),
        "{error}"
    );

    // A journal that belongs to a different sweep with the same routes.
    std::fs::write(fixture.checkpoint(), &checkpoint_bytes).expect("restore checkpoint");
    let checkpoint = fixture.checkpoint();
    let journal = fixture.journal();
    let mut other = fixture.sweep(&checkpoint, &journal);
    other.agent_id = "gateway:someone-else";
    let error = refusal(run_gateway_sweep(
        other,
        fixture.host(ScriptedProvider::answering("0.25"), budget(1_000_000, 100)),
        |_, _, _| unreachable!("no cell runs under a foreign journal"),
    ));
    assert!(
        error
            .to_string()
            .contains("different route table, budget or sweep"),
        "{error}"
    );
    assert_eq!(
        std::fs::read(fixture.journal()).expect("journal"),
        journal_bytes
    );
}

/// A line in the gateway family with a version this host does not speak is
/// answered with a typed refusal, and an oversized one is refused before it is
/// parsed; neither is misread as a malformed decision.
#[test]
fn a_gateway_line_is_never_read_as_a_decision() {
    let routes = table();
    let permits = CallPermits::new(4);
    let provider = ScriptedProvider::answering("0.25");
    let mut gateway = ModelGateway::new(
        &routes,
        &permits,
        provider.clone(),
        budget(1_000_000, 10),
        GatewayLimits::default(),
    );
    let log = Log::default();
    let seen = log.clone();
    let script = move |input: &mut dyn BufRead, output: &mut dyn Write| {
        let mut line = String::new();
        input.read_line(&mut line).expect("observation");
        let oversized = format!(
            r#"{{"protocol":"{GATEWAY_PROTOCOL}","model_alias":"{ALIAS}","messages":[{{"role":"user","content":"{}"}}],"max_output_tokens":16}}"#,
            "x".repeat(300 * 1024)
        );
        for request in [
            r#"{"protocol":"sharpebench.model-gateway.v9","model_alias":"fake.v1","messages":[],"max_output_tokens":1}"#.to_string(),
            oversized,
        ] {
            writeln!(output, "{request}").expect("write");
            output.flush().expect("flush");
            let mut answer = String::new();
            input.read_line(&mut answer).expect("answer");
            seen.lock()
                .expect("log lock")
                .push(serde_json::from_str(&answer).expect("a gateway response"));
        }
        writeln!(output, r#"{{"orders":[],"reasoning":"flat"}}"#).expect("write");
        output.flush().expect("flush");
    };
    let decision = {
        let mut entrant = GatewayEntrant::new(in_thread(script), &mut gateway);
        let decision = entrant.decide(&observation());
        assert!(!entrant.health().degraded());
        decision
    };
    assert_eq!(decision.reasoning, "flat");
    let kinds: Vec<_> = log
        .lock()
        .expect("log lock")
        .iter()
        .map(|answer| answer.error.as_ref().map(|error| error.kind))
        .collect();
    assert_eq!(
        kinds,
        vec![
            Some(GatewayErrorKind::UnknownProtocol),
            Some(GatewayErrorKind::RequestTooLarge)
        ]
    );
    assert_eq!(provider.calls(), 0);
}

/// The per-decision ceiling is a circuit breaker: the request past it is
/// refused by type and never reaches the provider.
#[test]
fn requests_past_the_per_decision_ceiling_are_refused_by_type() {
    let routes = table();
    let permits = CallPermits::new(4);
    let provider = ScriptedProvider::answering("0.25");
    let mut gateway = ModelGateway::new(
        &routes,
        &permits,
        provider.clone(),
        budget(1_000_000, 100),
        GatewayLimits {
            max_requests_per_decision: 2,
            ..GatewayLimits::default()
        },
    );
    let log = Log::default();
    {
        let mut entrant =
            GatewayEntrant::new(in_thread(model_entrant(log.clone(), 3)), &mut gateway);
        entrant.decide(&observation());
        assert_eq!(entrant.requests_served(), 2);
    }
    let answers = log.lock().expect("log lock");
    assert!(answers[0].ok && answers[1].ok);
    assert_eq!(
        answers[2].error.as_ref().map(|error| error.kind),
        Some(GatewayErrorKind::DecisionRequestLimit)
    );
    assert_eq!(provider.calls(), 2);
}

/// Time the host spends at the provider is not the entrant's: a slow provider
/// does not turn into an entrant timeout.
#[test]
fn host_serving_time_does_not_count_against_the_entrant() {
    let routes = table();
    let permits = CallPermits::new(4);
    let mut provider = ScriptedProvider::answering("0.25");
    provider.delay = Duration::from_millis(400);
    let mut gateway = ModelGateway::new(
        &routes,
        &permits,
        provider.clone(),
        budget(1_000_000, 10),
        GatewayLimits::default(),
    );
    let log = Log::default();
    let mut entrant = GatewayEntrant::new(in_thread(model_entrant(log.clone(), 1)), &mut gateway)
        .with_decide_timeout(Duration::from_millis(200));
    let decision = entrant.decide(&observation());
    assert!(!entrant.health().degraded(), "{:?}", entrant.health());
    assert_eq!(decision.orders.len(), 1);
}

/// No process the host starts for an entrant carries a route credential: not
/// in its program, its arguments or its declared environment, and a launcher
/// that inherits the host environment inherits it without them.
#[test]
fn credentials_never_reach_an_entrant_launch() {
    let routes = table();
    for launch in [
        EntrantLaunch::host("agent", vec![format!("--key={KEY}")], Vec::new()),
        EntrantLaunch::host("agent", Vec::new(), vec![("MODEL_KEY".into(), KEY.into())]),
        EntrantLaunch::host(KEY, Vec::new(), Vec::new()),
        EntrantLaunch::isolating_launcher("docker", vec!["run".into(), format!("-eK={KEY}")]),
    ] {
        let error = match launch.spawn(&routes) {
            Ok(_) => panic!("a credential-bearing launch spawned"),
            Err(error) => error,
        };
        assert_eq!(error.kind(), std::io::ErrorKind::InvalidInput);
        assert!(!error.to_string().contains(KEY));
    }
    let inherited = launcher_environment(
        [
            (
                "DOCKER_HOST".to_string(),
                "unix:///var/run/docker.sock".to_string(),
            ),
            ("PROVIDER_KEY".to_string(), KEY.to_string()),
            ("WRAPPED".to_string(), format!("Bearer {KEY}")),
        ],
        &routes.secrets(),
    );
    assert_eq!(
        inherited,
        vec![(
            "DOCKER_HOST".to_string(),
            "unix:///var/run/docker.sock".to_string()
        )]
    );
}

/// A real child process on real stdio: the scripted entrant asks the model on
/// every decision and holds when refused. The call ceiling proves both paths
/// reached it: the first calls answer, the rest are refused, and the run is
/// still a clean, scorable run.
#[test]
fn a_spawned_entrant_process_reaches_the_model_through_its_stdio() {
    let dir = temp_dir("process");
    let routes = table();
    let permits = CallPermits::new(4);
    let provider = ScriptedProvider::answering("0.25");
    let mut gateway = ModelGateway::new(
        &routes,
        &permits,
        provider.clone(),
        budget(1_000_000, 3),
        GatewayLimits::default(),
    );
    let launch = scripted_process(dir.path());
    let pipes = launch.spawn(&routes).expect("the scripted entrant spawns");
    let observed = gateway_backtest(
        &Dataset::synthetic(4, 60, 7),
        pipes,
        &mut gateway,
        WINDOWS[0],
        0,
        CostModel::default(),
        None,
    );
    let run = match observed.result {
        Ok(run) => run,
        Err(kind) => panic!("the scripted process run failed: {kind:?}"),
    };
    assert_eq!(run.returns.len(), STEPS);
    assert_eq!(provider.calls(), 3);
    let spend = gateway.journal().spend();
    assert_eq!(spend.calls_started, 3);
    assert_eq!(spend.priced_calls, 3);
}

const PROCESS_REQUEST: &str = r#"{"protocol":"sharpebench.model-gateway.v1","model_alias":"fake.v1","messages":[{"role":"user","content":"size it"}],"max_output_tokens":16}"#;
const PROCESS_HOLD: &str = r#"{"orders":[],"reasoning":"flat"}"#;

#[cfg(not(windows))]
fn scripted_process(dir: &Path) -> EntrantLaunch {
    let script = dir.join("entrant.sh");
    std::fs::write(
        &script,
        format!(
            "while IFS= read -r obs; do\n  printf '%s\\n' '{PROCESS_REQUEST}'\n  IFS= read -r answer || exit 0\n  printf '%s\\n' '{PROCESS_HOLD}'\ndone\n"
        ),
    )
    .expect("write the entrant script");
    EntrantLaunch::host(
        "sh",
        vec![script.display().to_string()],
        std::env::vars()
            .filter(|(name, _)| name == "PATH")
            .collect(),
    )
}

#[cfg(windows)]
fn scripted_process(dir: &Path) -> EntrantLaunch {
    let script = dir.join("entrant.ps1");
    std::fs::write(
        &script,
        format!(
            "while ($true) {{\r\n  $obs = [Console]::In.ReadLine()\r\n  if ($null -eq $obs) {{ break }}\r\n  [Console]::Out.WriteLine('{PROCESS_REQUEST}')\r\n  $answer = [Console]::In.ReadLine()\r\n  if ($null -eq $answer) {{ break }}\r\n  [Console]::Out.WriteLine('{PROCESS_HOLD}')\r\n}}\r\n"
        ),
    )
    .expect("write the entrant script");
    const ESSENTIALS: &[&str] = &[
        "PATH",
        "PATHEXT",
        "SYSTEMROOT",
        "WINDIR",
        "COMSPEC",
        "SYSTEMDRIVE",
        "TEMP",
        "TMP",
        "USERPROFILE",
        "APPDATA",
        "LOCALAPPDATA",
    ];
    EntrantLaunch::host(
        "powershell",
        vec![
            "-NoProfile".into(),
            "-NonInteractive".into(),
            "-ExecutionPolicy".into(),
            "Bypass".into(),
            "-File".into(),
            script.display().to_string(),
        ],
        std::env::vars()
            .filter(|(name, _)| {
                ESSENTIALS
                    .iter()
                    .any(|wanted| wanted.eq_ignore_ascii_case(name))
            })
            .collect(),
    )
}
