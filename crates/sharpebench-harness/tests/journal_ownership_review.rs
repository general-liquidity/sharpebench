//! Demonstrations for the two journal-ownership defects recorded in
//! `docs/audits/2026-09-09/ACCOUNTING-REVIEW.md` (findings A1 and A2).
//!
//! These assert the behaviour as it stands today, so they are records of a
//! defect rather than guarantees. When either defect is repaired the assertion
//! it pins will flip, and the test must be rewritten as the guarantee instead
//! of relaxed.
//!
//! Nothing here opens a socket: the provider is a local stand-in.

use std::path::Path;

use sharpebench_harness::accounting::RateCard;
use sharpebench_harness::gateway::{
    CallPermits, GatewayLimits, ModelGateway, ModelRoute, ProviderCall, ProviderOutcome,
    ProviderTransport, RouteTable, Secret, GATEWAY_PROTOCOL,
};
use sharpebench_harness::gateway_journal::{
    GatewayBudget, GatewayJournal, JournalIdentity, JournalLock,
};

const KEY: &str = "sk-live-test-do-not-log-0123456789";
const OVERHEAD: u64 = 8;

struct Answering;

impl ProviderTransport for Answering {
    fn call(&mut self, _call: ProviderCall<'_>) -> ProviderOutcome {
        ProviderOutcome::Answered {
            status: 200,
            body: br#"{"text":"ok","finish_reason":"stop","usage":{"input_tokens":10,"output_tokens":5}}"#
                .to_vec(),
        }
    }
}

fn table() -> RouteTable {
    let card = RateCard::from_json(
        br#"{"schema_version":"sharpebench.token-rate-card.v1","provider":"fake","model":"fake-1","revision":"2026-01-01","input_usd_nanos_per_token":1,"output_usd_nanos_per_token":1}"#,
    )
    .expect("a valid rate card");
    RouteTable::new(vec![ModelRoute::new(
        "fake.v1",
        "https://provider.invalid/v1/messages",
        Secret::new(KEY),
        card,
        4096,
        OVERHEAD,
    )
    .expect("a valid route")])
    .expect("a valid table")
}

fn budget() -> GatewayBudget {
    GatewayBudget {
        max_usd_nanos: u128::MAX,
        max_calls: 8,
    }
}

fn request(content: &str) -> String {
    format!(
        r#"{{"protocol":"{GATEWAY_PROTOCOL}","model_alias":"fake.v1","messages":[{{"role":"user","content":"{content}"}}],"max_output_tokens":16,"tools":[]}}"#
    )
}

fn answered(line: &str) -> bool {
    let response: serde_json::Value =
        serde_json::from_str(line).expect("the gateway answers in its own schema");
    response["ok"].as_bool().expect("ok is a boolean")
}

fn temp_dir(tag: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "sb-review-{tag}-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    std::fs::remove_dir_all(&dir).ok();
    std::fs::create_dir_all(&dir).expect("temp dir");
    dir
}

fn open<'a>(
    routes: &'a RouteTable,
    permits: &'a CallPermits,
    path: &Path,
) -> std::io::Result<ModelGateway<'a, Answering>> {
    ModelGateway::open(
        routes,
        permits,
        Answering,
        budget(),
        GatewayLimits::default(),
        path,
    )
}

/// A1. `Drop for JournalLock` removes the lock file by path without checking
/// that the file there is still the lock this value created. After a takeover
/// the displaced holder is still alive and still owns a `JournalLock` naming
/// that path, so its drop deletes the *taker's* lock and leaves the journal
/// unowned while the taker is live and spending.
#[test]
fn a_displaced_holder_unlocks_the_holder_that_displaced_it() {
    let dir = temp_dir("takeover-drop");
    let path = dir.join("journal.json");
    let lock_path = JournalLock::lock_path(&path).expect("a lock path");

    let displaced = JournalLock::acquire(&path).expect("the first holder takes the path");
    let taker = JournalLock::take_over(&path, "believed gone, in fact still running")
        .expect("an operator displaces it");
    assert!(lock_path.exists(), "the taker holds the path");

    // The displaced process was not dead after all, and now exits normally.
    drop(displaced);

    assert!(
        !lock_path.exists(),
        "DEFECT A1: the displaced holder's drop removed the taker's lock file"
    );
    let third = JournalLock::acquire(&path);
    assert!(
        third.is_ok(),
        "DEFECT A1: a third holder is admitted while the taker is still live"
    );

    drop(third);
    drop(taker);
    std::fs::remove_dir_all(&dir).ok();
}

/// A2. Ownership is keyed on the journal's path spelling, not on the file it
/// names. Two directory entries for one journal (here a hard link; a
/// symlink-to-file has the same shape) produce two different `.lock` names, so
/// both gateways open. The version compare-and-swap does not catch it either:
/// `save` persists through a temporary file and a rename, which replaces the
/// directory entry, so after the first save the two names are two files and
/// neither writer ever sees the other's version.
///
/// Both gateways then spend the same declared budget in full.
#[test]
fn two_names_for_one_journal_file_are_two_locks_and_both_gateways_spend() {
    let dir = temp_dir("aliased-journal");
    let real = dir.join("journal.json");
    let alias = dir.join("journal-copy.json");
    let routes = table();
    let permits = CallPermits::new(4);

    GatewayJournal::new(JournalIdentity::new(routes.identity_digest(), budget()))
        .save(&real)
        .expect("seed the journal the operator owns");
    std::fs::hard_link(&real, &alias).expect("a second name for one file");

    let mut first = open(&routes, &permits, &real).expect("the first gateway opens");
    let mut second = open(&routes, &permits, &alias)
        .expect("DEFECT A2: a second gateway opens the same journal under its other name");
    assert_ne!(
        first.journal_lock_path().expect("a lock"),
        second.journal_lock_path().expect("a lock"),
        "one journal, two lock files"
    );

    assert!(answered(&first.serve_line(&request("hello"))));
    assert!(
        answered(&second.serve_line(&request("world"))),
        "DEFECT A2: the second gateway spends too; the compare-and-swap never fires"
    );
    assert_eq!(first.dispatches(), 1);
    assert_eq!(second.dispatches(), 1);
    assert!(
        !first.journal_conflict() && !second.journal_conflict(),
        "neither writer ever learns about the other"
    );

    std::fs::remove_dir_all(&dir).ok();
}
