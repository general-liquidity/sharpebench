//! The two journal-ownership findings recorded in
//! `docs/audits/2026-09-09/ACCOUNTING-REVIEW.md` (A1 and A2), as guarantees.
//!
//! These began as characterization tests asserting the defects. They now assert
//! the repaired behaviour through the public surface only: a holder releases
//! the lock it holds and no other, and one journal document admits one gateway
//! whatever name it is reached under.
//!
//! What A2's repair does not reach, and what no test here claims: the lock is a
//! sibling of the journal, so two directory entries for one document in
//! *different* directories still derive two lock files; and a journal document
//! written before it carried an identity names none, so two gateways opening
//! such a document under two names each assign one.
//!
//! Nothing here opens a socket: the provider is a local stand-in.

use std::path::Path;

use sharpebench_harness::accounting::RateCard;
use sharpebench_harness::gateway::{
    CallPermits, GatewayLimits, ModelGateway, ModelRoute, ProviderCall, ProviderOutcome,
    ProviderTransport, RouteTable, Secret, GATEWAY_PROTOCOL,
};
use sharpebench_harness::gateway_journal::{
    GatewayBudget, GatewayJournal, JournalIdentity, JournalLock, JournalLockError,
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

/// One directory per test, removed when the test ends: a leak here can never
/// be inherited by a later run, whatever the operating system does with pids.
struct ScratchDir {
    path: std::path::PathBuf,
}

impl ScratchDir {
    fn new(tag: &str) -> Self {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|since| since.as_nanos())
            .unwrap_or(0);
        let path = std::env::temp_dir().join(format!(
            "sb-review-{tag}-{}-{}-{stamp:x}",
            std::process::id(),
            NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&path).expect("a scratch directory");
        Self { path }
    }

    fn join(&self, name: &str) -> std::path::PathBuf {
        self.path.join(name)
    }
}

impl Drop for ScratchDir {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.path).ok();
    }
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

fn held_lock_path(error: &std::io::Error) -> std::path::PathBuf {
    match error
        .get_ref()
        .and_then(|source| source.downcast_ref::<JournalLockError>())
    {
        Some(JournalLockError::Held { lock_path, .. }) => lock_path.clone(),
        other => panic!("expected a refusal naming a held lock, got {other:?}"),
    }
}

/// A1. A holder removes the lock it holds and no other. After a takeover the
/// file at the path is the taker's, and the displaced holder, which may still
/// be alive and may still be spending, must leave it alone: otherwise its exit
/// unlocks the journal under the taker and any number of further gateways may
/// open it.
///
/// What could satisfy the middle assertion other than the repair: a drop that
/// removes nothing at all. The last assertion rules that out by requiring the
/// holder that does own the lock to release it.
#[test]
fn a_displaced_holder_leaves_the_lock_of_the_holder_that_displaced_it() {
    let dir = ScratchDir::new("takeover-drop");
    let path = dir.join("journal.json");
    let lock_path = JournalLock::lock_path(&path).expect("a lock path");

    let displaced = JournalLock::acquire(&path).expect("the first holder takes the path");
    let taker = JournalLock::take_over(&path, "believed gone, in fact still running")
        .expect("an operator displaces it");
    assert!(lock_path.exists(), "the taker holds the path");
    assert!(
        !displaced.is_still_held(),
        "the displaced holder can tell that the lock at its path is not its own"
    );

    // The displaced process was not dead after all, and now exits normally.
    drop(displaced);

    assert!(
        lock_path.exists(),
        "the taker's lock is still there once the holder it displaced has gone"
    );
    assert!(
        matches!(
            JournalLock::acquire(&path),
            Err(JournalLockError::Held { .. })
        ),
        "so no third holder is admitted while the taker is live"
    );

    drop(taker);
    assert!(
        !lock_path.exists(),
        "and the holder that does own the lock still releases it"
    );
}

/// A2. Ownership is keyed on the journal document, not on the directory entry
/// it was reached through. Two names for one journal (here a hard link; a
/// symlink to the file has the same shape) name one document identity, so the
/// second gateway is refused at open and never reaches the budget.
///
/// Two causes could refuse that second open: the spelling lock, and the
/// document lock. The spelling lock is shown free first, and the refusal is
/// required to name the document lock, so only the document lock can be what
/// refused. A third cause, the journal being bound to a different experiment,
/// would refuse with `InvalidData` rather than with a lock error.
#[test]
fn two_names_for_one_journal_document_admit_one_gateway() {
    let dir = ScratchDir::new("aliased-journal");
    let real = dir.join("journal.json");
    let alias = dir.join("journal-copy.json");
    let routes = table();
    let permits = CallPermits::new(4);

    GatewayJournal::new(JournalIdentity::new(routes.identity_digest(), budget()))
        .save(&real)
        .expect("seed the journal the operator owns");
    std::fs::hard_link(&real, &alias).expect("a second name for one file");
    assert_eq!(
        JournalLock::document_id(&real),
        JournalLock::document_id(&alias),
        "one document, reached under two names"
    );

    let mut first = open(&routes, &permits, &real).expect("the first gateway opens");
    let alias_spelling_lock = JournalLock::lock_path(&alias).expect("a lock path");
    assert!(
        !alias_spelling_lock.exists(),
        "the other name's spelling lock is free, so only the document lock can refuse"
    );

    let refused = open(&routes, &permits, &alias)
        .err()
        .expect("the second gateway is refused the document the first owns");
    assert_eq!(refused.kind(), std::io::ErrorKind::AlreadyExists);
    assert_eq!(
        held_lock_path(&refused),
        JournalLock::identity_lock_path(
            &real,
            &JournalLock::document_id(&real).expect("an identity")
        ),
        "the refusal names the document's lock, not a spelling's"
    );
    assert!(
        !alias_spelling_lock.exists(),
        "a refused gateway leaves no lock of its own behind"
    );

    assert!(answered(&first.serve_line(&request("hello"))));
    assert_eq!(first.dispatches(), 1, "one writer, one spend");
}
