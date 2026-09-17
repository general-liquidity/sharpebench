//! Finish-reason accounting for the host-observed model gateway.
//!
//! A completion the output bound cut off can still parse into a decision, or be
//! read by the entrant as an abstention. The gateway always carried the
//! provider's `finish_reason` back to the entrant, but nothing totalled it, so a
//! heavily truncated sweep looked operationally clean. These tests hold the
//! journal fold, its persistence, its legacy reading and the published
//! host-observed usage to the four classes (stop, length, other, absent).
//!
//! Nothing here opens a socket: the provider is a local script.

use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use sharpebench_harness::accounting::RateCard;
use sharpebench_harness::gateway::serve::{
    attach_host_observed_usage, gateway_backtest, run_gateway_sweep, EntrantPipes, EntrantProcess,
    GatewayHost, GatewaySweep,
};
use sharpebench_harness::gateway::{
    CallPermits, GatewayLimits, ModelGateway, ModelRoute, ProviderCall, ProviderOutcome,
    ProviderTransport, RouteTable, Secret, GATEWAY_PROTOCOL,
};
use sharpebench_harness::gateway_journal::{
    FinishClass, FinishReasonCounts, GatewayBudget, GatewayJournal,
};
use sharpebench_harness::{ResumePolicy, SweepIdentity};
use sharpebench_protocol::MarketObservation;
use sharpebench_sim::{CostModel, Dataset, Window};

const KEY: &str = "sk-live-finish-test-do-not-log-0123456789";
const ALIAS: &str = "fake.v1";

/// One scripted answer: an HTTP status and the normalized body's stop reason.
#[derive(Clone)]
enum Answer {
    Body {
        finish_reason: Option<&'static str>,
        usage: bool,
    },
    RateLimited,
}

/// Answers from a fixed script, then repeats its last entry.
#[derive(Clone)]
struct Scripted(Arc<Mutex<Vec<Answer>>>);

impl Scripted {
    fn new(script: Vec<Answer>) -> Self {
        Self(Arc::new(Mutex::new(script)))
    }
}

impl ProviderTransport for Scripted {
    fn call(&mut self, _call: ProviderCall<'_>) -> ProviderOutcome {
        let mut script = self.0.lock().expect("script lock");
        let answer = if script.len() > 1 {
            script.remove(0)
        } else {
            script[0].clone()
        };
        match answer {
            Answer::RateLimited => ProviderOutcome::Answered {
                status: 429,
                body: Vec::new(),
            },
            Answer::Body {
                finish_reason,
                usage,
            } => {
                let mut body = serde_json::json!({ "text": "0" });
                if let Some(reason) = finish_reason {
                    body["finish_reason"] = reason.into();
                }
                if usage {
                    body["usage"] = serde_json::json!({"input_tokens": 10, "output_tokens": 5});
                }
                ProviderOutcome::Answered {
                    status: 200,
                    body: serde_json::to_vec(&body).expect("a body serializes"),
                }
            }
        }
    }
}

fn answer(finish_reason: Option<&'static str>) -> Answer {
    Answer::Body {
        finish_reason,
        usage: true,
    }
}

fn table() -> RouteTable {
    let card = RateCard::from_json(
        br#"{"schema_version":"sharpebench.token-rate-card.v1","provider":"fake","model":"fake-1","revision":"2026-01-01","input_usd_nanos_per_token":1,"output_usd_nanos_per_token":1}"#,
    )
    .expect("a valid rate card");
    RouteTable::new(vec![ModelRoute::new(
        ALIAS,
        "https://provider.invalid/v1/messages",
        Secret::new(KEY),
        card,
        4096,
        8,
    )
    .expect("a valid route")])
    .expect("a valid table")
}

/// Within `u64`, so a test can round-trip the journal through
/// `serde_json::Value` without losing the budget.
fn budget() -> GatewayBudget {
    GatewayBudget {
        max_usd_nanos: 1_000_000_000_000,
        max_calls: 1_000,
    }
}

fn limits() -> GatewayLimits {
    GatewayLimits {
        max_retries_per_request: 0,
        ..GatewayLimits::default()
    }
}

fn request() -> String {
    format!(
        r#"{{"protocol":"{GATEWAY_PROTOCOL}","model_alias":"{ALIAS}","messages":[{{"role":"user","content":"size it"}}],"max_output_tokens":16,"tools":[]}}"#
    )
}

fn scratch(tag: &str) -> PathBuf {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let dir = std::env::temp_dir().join(format!(
        "sb-finish-{tag}-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::create_dir_all(&dir).expect("scratch dir");
    dir
}

fn counts(stop: u32, length: u32, other: u32, absent: u32, unrecorded: u32) -> FinishReasonCounts {
    FinishReasonCounts {
        stop,
        length,
        other,
        absent,
        unrecorded,
    }
}

/// The regression: a completion the output bound truncated is counted as
/// `length`, and every other answer lands in exactly one class. A refused call
/// produced no answer and is in none of them.
#[test]
fn a_length_stopped_call_is_counted_as_length() {
    let routes = table();
    let permits = CallPermits::new(1);
    let provider = Scripted::new(vec![
        answer(Some("length")),
        answer(Some("stop")),
        answer(Some("content_filter")),
        answer(None),
        // A provider spelling the adapter did not normalize is not guessed at.
        answer(Some("max_tokens")),
        Answer::RateLimited,
        // An answer with no reported usage is still an answer.
        Answer::Body {
            finish_reason: Some("length"),
            usage: false,
        },
    ]);
    let mut gateway = ModelGateway::new(&routes, &permits, provider, budget(), limits());
    let mut answers = Vec::new();
    for _ in 0..7 {
        let line = gateway.serve_line(&request());
        answers.push(serde_json::from_str::<serde_json::Value>(&line).expect("json"));
    }
    // What the entrant is handed is unchanged: the reason still reaches it.
    assert_eq!(answers[0]["finish_reason"], "length");
    assert_eq!(answers[5]["ok"], false);
    let spend = gateway.journal().spend();
    assert_eq!(spend.calls_started, 7);
    assert_eq!((spend.priced_calls, spend.unknown_calls), (5, 1));
    assert_eq!(gateway.journal().finish_reasons(), counts(1, 2, 2, 1, 0));
}

#[test]
fn the_class_vocabulary_is_exact() {
    assert_eq!(FinishClass::of(Some("stop")), FinishClass::Stop);
    assert_eq!(FinishClass::of(Some("length")), FinishClass::Length);
    assert_eq!(FinishClass::of(Some("Stop")), FinishClass::Other);
    assert_eq!(FinishClass::of(Some("")), FinishClass::Other);
    assert_eq!(FinishClass::of(None), FinishClass::Absent);
}

fn persisted_journal(dir: &Path) -> PathBuf {
    let path = dir.join("journal.json");
    let routes = table();
    let permits = CallPermits::new(1);
    let provider = Scripted::new(vec![
        answer(Some("length")),
        Answer::RateLimited,
        Answer::Body {
            finish_reason: Some("stop"),
            usage: false,
        },
    ]);
    let mut gateway = ModelGateway::open(&routes, &permits, provider, budget(), limits(), &path)
        .expect("a fresh journal opens");
    for _ in 0..3 {
        gateway.serve_line(&request());
    }
    drop(gateway);
    path
}

fn load(path: &Path) -> std::io::Result<GatewayJournal> {
    GatewayJournal::load_for_routes(path, &table().identity_digest(), budget())
}

/// The counts are a fold over durable records, so a resumed journal reports
/// what its earlier process saw rather than starting again from zero.
#[test]
fn the_counts_survive_the_journal_file() {
    let dir = scratch("persist");
    let path = persisted_journal(&dir);
    let reloaded = load(&path).expect("the journal reloads");
    assert_eq!(reloaded.finish_reasons(), counts(1, 1, 0, 0, 0));
    let text = std::fs::read_to_string(&path).expect("journal text");
    assert!(text.contains(r#""finish_reason": "length""#), "{text}");
    std::fs::remove_dir_all(&dir).ok();
}

/// A journal written before the field existed still loads. Its parsed answers
/// are reported as unrecorded, not as clean stops and not as absent reasons;
/// its released call stays out of every class.
#[test]
fn a_journal_written_before_the_field_reports_its_answers_as_unrecorded() {
    let dir = scratch("legacy");
    let path = persisted_journal(&dir);
    let mut document: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&path).expect("journal bytes")).expect("json");
    for record in document["records"].as_array_mut().expect("records") {
        record
            .as_object_mut()
            .expect("a record")
            .remove("finish_reason");
    }
    std::fs::write(&path, serde_json::to_vec_pretty(&document).expect("json")).expect("write");
    let legacy = load(&path).expect("a legacy journal still loads");
    assert_eq!(legacy.finish_reasons(), counts(0, 0, 0, 0, 2));
    assert_eq!(legacy.spend().released_calls, 1);
    std::fs::remove_dir_all(&dir).ok();
}

/// The class is a closed vocabulary on disk too: a record naming anything else
/// is a malformed journal, refused rather than read.
#[test]
fn a_journal_naming_an_unknown_class_is_refused() {
    let dir = scratch("unknown");
    let path = persisted_journal(&dir);
    let text = std::fs::read_to_string(&path).expect("journal text");
    std::fs::write(
        &path,
        text.replace(
            r#""finish_reason": "length""#,
            r#""finish_reason": "truncated""#,
        ),
    )
    .expect("write");
    assert!(load(&path).is_err());
    std::fs::remove_dir_all(&dir).ok();
}

/// An in-process entrant on a real OS pipe pair: one model request per
/// observation, then a flat decision.
struct ThreadEntrant(std::thread::JoinHandle<()>);

impl EntrantProcess for ThreadEntrant {
    fn exited(&mut self) -> Option<Option<i32>> {
        self.0.is_finished().then_some(Some(0))
    }

    fn terminate(&mut self) {}
}

fn asking_entrant() -> EntrantPipes {
    let (host_reads, mut entrant_writes) = std::io::pipe().expect("a pipe");
    let (entrant_reads, host_writes) = std::io::pipe().expect("a pipe");
    let handle = std::thread::spawn(move || {
        let mut input = BufReader::new(entrant_reads);
        loop {
            let mut line = String::new();
            if input.read_line(&mut line).unwrap_or(0) == 0 {
                return;
            }
            let _: MarketObservation =
                serde_json::from_str(&line).expect("the host writes observations");
            if writeln!(entrant_writes, "{}", request()).is_err() {
                return;
            }
            let mut answer = String::new();
            if input.read_line(&mut answer).unwrap_or(0) == 0 {
                return;
            }
            if writeln!(entrant_writes, r#"{{"orders":[]}}"#).is_err() {
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

/// The published record: a sweep whose every answer was truncated says so in
/// its host-observed usage, on the entrant's row, beside the rank.
#[test]
fn a_truncated_sweep_publishes_its_length_count_beside_the_rank() {
    const WINDOWS: [Window; 1] = [Window { start: 20, end: 26 }];
    const SEEDS: [u64; 2] = [0, 1];
    let dir = scratch("sweep");
    let (checkpoint, journal) = (dir.join("checkpoint.json"), dir.join("journal.json"));
    let data = Dataset::synthetic(4, 60, 7);
    let routes = table();
    let permits = CallPermits::new(1);
    let outcome = run_gateway_sweep(
        GatewaySweep {
            checkpoint: &checkpoint,
            journal: &journal,
            agent_id: "gateway:truncated",
            identity: SweepIdentity {
                dataset_sha256: "a".repeat(64),
                cost_model_sha256: "b".repeat(64),
                score_config_sha256: "c".repeat(64),
                runner_artifact_sha256: "d".repeat(64),
                entrant_sha256: "e".repeat(64),
                invocation_sha256: "f".repeat(64),
            },
            windows: &WINDOWS,
            seeds: &SEEDS,
            max_retries: 0,
            policy: ResumePolicy::UnfinishedOnly,
        },
        GatewayHost {
            routes: &routes,
            permits: &permits,
            transport: Scripted::new(vec![answer(Some("length"))]),
            budget: budget(),
            limits: limits(),
        },
        |window, seed, gateway| {
            gateway_backtest(
                &data,
                asking_entrant(),
                gateway,
                WINDOWS[window],
                seed,
                CostModel::default(),
                None,
            )
        },
    )
    .expect("the sweep runs");
    let calls = outcome.host_observed.calls_started;
    assert_eq!(calls, 12, "six decisions in each of two cells");
    assert_eq!(
        outcome.host_observed.finish_reasons,
        counts(0, calls, 0, 0, 0)
    );
    let mut board = serde_json::json!([{ "agent_id": "gateway:truncated" }]);
    attach_host_observed_usage(&mut board, "gateway:truncated", &outcome.host_observed);
    let published = &board[0]["host_observed_usage"];
    assert_eq!(published["finish_reasons"]["length"], 12);
    assert_eq!(published["finish_reasons"]["stop"], 0);
    assert_eq!(published["rank_neutral"], true);
    std::fs::remove_dir_all(&dir).ok();
}
