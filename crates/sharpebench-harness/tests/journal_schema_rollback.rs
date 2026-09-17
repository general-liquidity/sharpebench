//! Rollback safety for the journal's finish-reason field (review finding RD-1).
//!
//! A binary built before `finish_reason` existed reads it as an unknown key,
//! drops it, and writes the journal back without it. These tests hold the
//! repair: a journal that records a class is written as schema v2, which such
//! a binary refuses, while a journal with no class stays v1, which it still
//! resumes. The older binary's admission check is reproduced from its source:
//! exact identity equality with a v1 identity in `load_bound`, and an exact v1
//! version match in `load_for_routes`.
//!
//! Nothing here opens a socket: the provider is a local script.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use serde::Deserialize;
use sharpebench_harness::accounting::RateCard;
use sharpebench_harness::gateway::{
    CallPermits, GatewayLimits, ModelGateway, ModelRoute, ProviderCall, ProviderOutcome,
    ProviderTransport, RouteTable, Secret, GATEWAY_PROTOCOL,
};
use sharpebench_harness::gateway_journal::{
    FinishClass, GatewayBudget, GatewayJournal, JournalIdentity, Settlement,
    JOURNAL_SCHEMA_VERSION, JOURNAL_SCHEMA_VERSION_V2,
};

const KEY: &str = "sk-live-rollback-test-do-not-log-0123456789";
const ALIAS: &str = "fake.v1";
const V1: &str = "sharpebench.gateway-journal.v1";
const V2: &str = "sharpebench.gateway-journal.v2";

#[derive(Clone, Copy)]
enum Answer {
    Length,
    RateLimited,
}

/// Answers from a fixed script, one entry per call.
#[derive(Clone)]
struct Scripted(Arc<Mutex<Vec<Answer>>>);

impl ProviderTransport for Scripted {
    fn call(&mut self, _call: ProviderCall<'_>) -> ProviderOutcome {
        match self.0.lock().expect("script lock").remove(0) {
            Answer::RateLimited => ProviderOutcome::Answered {
                status: 429,
                body: Vec::new(),
            },
            Answer::Length => ProviderOutcome::Answered {
                status: 200,
                body: br#"{"text":"0","finish_reason":"length","usage":{"input_tokens":10,"output_tokens":5}}"#
                    .to_vec(),
            },
        }
    }
}

fn card() -> RateCard {
    RateCard::from_json(
        br#"{"schema_version":"sharpebench.token-rate-card.v1","provider":"fake","model":"fake-1","revision":"2026-01-01","input_usd_nanos_per_token":1,"output_usd_nanos_per_token":1}"#,
    )
    .expect("a valid rate card")
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

fn budget() -> GatewayBudget {
    GatewayBudget {
        max_usd_nanos: 1_000_000_000_000,
        max_calls: 100,
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
        "sb-rollback-{tag}-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::create_dir_all(&dir).expect("scratch dir");
    dir
}

/// Open a gateway on `path`, serve one call per scripted answer, and close it.
fn serve(path: &Path, answers: &[Answer]) {
    let routes = table();
    let permits = CallPermits::new(1);
    let provider = Scripted(Arc::new(Mutex::new(answers.to_vec())));
    let mut gateway = ModelGateway::open(&routes, &permits, provider, budget(), limits(), path)
        .expect("the journal opens");
    for _ in answers {
        gateway.serve_line(&request());
    }
}

fn document(path: &Path) -> serde_json::Value {
    serde_json::from_slice(&std::fs::read(path).expect("journal bytes")).expect("journal json")
}

fn schema_version(path: &Path) -> String {
    document(path)["identity"]["schema_version"]
        .as_str()
        .expect("a schema version")
        .to_string()
}

/// The journal identity as a binary from before this repair declares it.
#[derive(Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
struct OlderIdentity {
    schema_version: String,
    route_table_sha256: String,
    budget: GatewayBudget,
    #[serde(default)]
    sweep_sha256: Option<String>,
}

/// Whether an older binary's `load_bound` and `load_for_routes` admit the
/// journal at `path`. Both compared the schema version exactly against v1.
fn older_binary_admits(path: &Path, sweep: Option<&str>) -> (bool, bool) {
    let found: OlderIdentity =
        serde_json::from_value(document(path)["identity"].clone()).expect("an identity");
    let own = OlderIdentity {
        schema_version: V1.to_string(),
        route_table_sha256: table().identity_digest(),
        budget: budget(),
        sweep_sha256: sweep.map(str::to_string),
    };
    let load_bound = found == own;
    let load_for_routes = found.schema_version == V1
        && found.route_table_sha256 == own.route_table_sha256
        && found.budget == own.budget;
    (load_bound, load_for_routes)
}

fn load(path: &Path) -> std::io::Result<GatewayJournal> {
    GatewayJournal::load_bound(
        path,
        &JournalIdentity::new(table().identity_digest(), budget()),
    )
}

fn inspect(path: &Path) -> std::io::Result<GatewayJournal> {
    GatewayJournal::load_for_routes(path, &table().identity_digest(), budget())
}

#[test]
fn the_published_versions_are_the_documented_strings() {
    assert_eq!(JOURNAL_SCHEMA_VERSION, V1);
    assert_eq!(JOURNAL_SCHEMA_VERSION_V2, V2);
    assert_eq!(
        JournalIdentity::new("a".repeat(64), budget()).schema_version,
        V1,
        "a journal starts at v1"
    );
}

/// With no class to lose, nothing changes: the journal stays v1 and an older
/// binary can still resume it.
#[test]
fn a_journal_without_a_finish_class_stays_v1_and_older_binaries_resume_it() {
    let dir = scratch("v1");
    let path = dir.join("journal.json");
    serve(&path, &[Answer::RateLimited, Answer::RateLimited]);
    let text = std::fs::read_to_string(&path).expect("journal text");
    assert_eq!(schema_version(&path), V1);
    assert!(!text.contains("finish_reason"), "{text}");
    assert_eq!(older_binary_admits(&path, None), (true, true));
    let reloaded = load(&path).expect("this binary resumes it");
    assert_eq!(reloaded.required_schema_version(), V1);
    assert_eq!(reloaded.spend().released_calls, 2);
    std::fs::remove_dir_all(&dir).ok();
}

/// The regression: the save that adds the first class writes v2, which an
/// older binary refuses rather than rewrites, and this binary still resumes.
#[test]
fn the_first_recorded_class_moves_the_journal_to_v2_which_older_binaries_refuse() {
    let dir = scratch("v2");
    let path = dir.join("journal.json");
    serve(&path, &[Answer::RateLimited]);
    assert_eq!(schema_version(&path), V1);
    serve(&path, &[Answer::Length, Answer::RateLimited]);
    assert_eq!(schema_version(&path), V2);
    assert_eq!(older_binary_admits(&path, None), (false, false));

    let reloaded = load(&path).expect("this binary resumes a v2 journal");
    assert_eq!(reloaded.finish_reasons().length, 1);
    assert_eq!(reloaded.spend().calls_started, 3);
    assert_eq!(
        inspect(&path)
            .expect("the report reads v2")
            .finish_reasons()
            .length,
        1
    );

    // Append-only records keep their class, so later saves stay v2.
    serve(&path, &[Answer::RateLimited]);
    assert_eq!(schema_version(&path), V2);
    std::fs::remove_dir_all(&dir).ok();
}

/// A journal written between the class landing and this repair holds classes
/// under v1. This binary reads it without loss and writes it as v2 on its next
/// save; loading alone does not rewrite it.
#[test]
fn a_v1_journal_already_holding_classes_is_written_as_v2_on_its_next_save() {
    let dir = scratch("upgrade");
    let path = dir.join("journal.json");
    serve(&path, &[Answer::Length]);
    let mut legacy = document(&path);
    legacy["identity"]["schema_version"] = V1.into();
    std::fs::write(&path, serde_json::to_vec_pretty(&legacy).expect("json")).expect("write");

    let loaded = load(&path).expect("a v1 journal with classes loads");
    assert_eq!(loaded.identity.schema_version, V1);
    assert_eq!(loaded.required_schema_version(), V2);
    assert_eq!(loaded.finish_reasons().length, 1);
    assert_eq!(
        inspect(&path)
            .expect("the report reads it")
            .finish_reasons()
            .length,
        1
    );
    assert_eq!(schema_version(&path), V1, "reading does not rewrite");

    serve(&path, &[Answer::RateLimited]);
    assert_eq!(schema_version(&path), V2);
    let upgraded = load(&path).expect("the upgraded journal loads");
    assert_eq!(upgraded.finish_reasons().length, 1);
    assert_eq!(upgraded.spend().released_calls, 1);
    std::fs::remove_dir_all(&dir).ok();
}

/// A version this binary does not write is refused by both loaders, whatever
/// the rest of the identity says.
#[test]
fn a_journal_under_an_unknown_schema_version_is_refused() {
    let dir = scratch("unknown");
    let path = dir.join("journal.json");
    serve(&path, &[Answer::Length]);
    for version in [
        "sharpebench.gateway-journal.v3",
        "sharpebench.gateway-journal.v0",
        "",
    ] {
        let mut foreign = document(&path);
        foreign["identity"]["schema_version"] = version.into();
        std::fs::write(&path, serde_json::to_vec_pretty(&foreign).expect("json")).expect("write");
        assert!(load(&path).is_err(), "{version}");
        assert!(inspect(&path).is_err(), "{version}");
    }
    std::fs::remove_dir_all(&dir).ok();
}

/// Accepting two schema versions does not loosen the rest of the binding: a v2
/// journal bound to a sweep resumes only under that sweep, routes and budget.
#[test]
fn a_v2_journal_keeps_its_route_budget_and_sweep_binding() {
    let dir = scratch("binding");
    let path = dir.join("journal.json");
    let sweep = "9".repeat(64);
    let routes = table().identity_digest();
    let mut journal = GatewayJournal::new(
        JournalIdentity::new(routes.clone(), budget()).for_sweep(sweep.clone()),
    );
    let ordinal = journal.reserve(ALIAS, &card(), 40);
    journal.settle_answered(
        ordinal,
        Settlement::Priced {
            input_tokens: 10,
            output_tokens: 5,
            usd_nanos: "15".into(),
        },
        FinishClass::Length,
    );
    journal.save(&path).expect("save");
    assert_eq!(schema_version(&path), V2);
    assert_eq!(journal.identity.schema_version, V2);
    assert_eq!(older_binary_admits(&path, Some(&sweep)), (false, false));

    let mine = JournalIdentity::new(routes.clone(), budget()).for_sweep(sweep.clone());
    assert!(GatewayJournal::load_bound(&path, &mine).is_ok());
    for other in [
        JournalIdentity::new(routes.clone(), budget()),
        JournalIdentity::new(routes.clone(), budget()).for_sweep("8".repeat(64)),
        JournalIdentity::new("b".repeat(64), budget()).for_sweep(sweep.clone()),
        JournalIdentity::new(
            routes.clone(),
            GatewayBudget {
                max_calls: 99,
                ..budget()
            },
        )
        .for_sweep(sweep.clone()),
        JournalIdentity {
            schema_version: "sharpebench.gateway-journal.v3".into(),
            ..mine.clone()
        },
    ] {
        assert!(GatewayJournal::load_bound(&path, &other).is_err());
    }
    std::fs::remove_dir_all(&dir).ok();
}
