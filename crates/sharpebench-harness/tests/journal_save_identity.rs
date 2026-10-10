//! Public saves must preserve document ownership, not only version equality.

use sharpebench_harness::accounting::RateCard;
use sharpebench_harness::gateway_journal::{
    GatewayBudget, GatewayJournal, JournalIdentity, JournalLock,
};
use std::path::PathBuf;

struct Scratch(PathBuf);

impl Scratch {
    fn new() -> Self {
        use std::sync::atomic::{AtomicUsize, Ordering};
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "sb-journal-save-identity-{}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&path).unwrap();
        Self(path)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn identity() -> JournalIdentity {
    JournalIdentity::new(
        "a".repeat(64),
        GatewayBudget {
            max_usd_nanos: 1000,
            max_calls: 8,
        },
    )
}

fn card() -> RateCard {
    RateCard::from_json(br#"{"schema_version":"sharpebench.token-rate-card.v1","provider":"fake","model":"fake-1","revision":"2026-01-01","input_usd_nanos_per_token":1,"output_usd_nanos_per_token":1}"#).unwrap()
}

#[test]
fn a_same_version_replacement_is_not_overwritten() {
    let dir = Scratch::new();
    let path = dir.0.join("journal.json");
    let replacement = dir.0.join("replacement.json");
    let mut journal = GatewayJournal::new(identity());
    journal.save(&path).unwrap();
    let mut other = GatewayJournal::new(identity());
    other.save(&replacement).unwrap();
    assert_eq!(journal.version(), other.version());
    assert_ne!(journal.journal_id(), other.journal_id());
    std::fs::copy(&replacement, &path).unwrap();
    let replaced_bytes = std::fs::read(&path).unwrap();
    journal.reserve("alias", &card(), 10);
    let result = journal.save(&path);
    assert!(
        result.is_err(),
        "another document at the same version must be refused"
    );
    assert!(result
        .unwrap_err()
        .to_string()
        .contains("identity or binding differs"));
    assert_eq!(std::fs::read(&path).unwrap(), replaced_bytes);
    assert_eq!(journal.version(), 1);
    assert_eq!(journal.records().len(), 1);
}

#[test]
fn a_same_version_and_same_id_changed_binding_is_not_overwritten() {
    let dir = Scratch::new();
    let path = dir.0.join("journal.json");
    let mut journal = GatewayJournal::new(identity());
    journal.save(&path).unwrap();
    let original = std::fs::read(&path).unwrap();
    let mut document: serde_json::Value = serde_json::from_slice(&original).unwrap();
    document["identity"]["route_table_sha256"] = serde_json::Value::String("b".repeat(64));
    let replacement = serde_json::to_vec_pretty(&document).unwrap();
    std::fs::write(&path, &replacement).unwrap();
    assert!(
        journal.save(&path).is_err(),
        "a matching version and id do not authorize a different binding"
    );
    assert_eq!(std::fs::read(&path).unwrap(), replacement);
    assert_eq!(journal.version(), 1);
    std::fs::write(&path, original).unwrap();
    journal.save(&path).unwrap();
    assert_eq!(journal.version(), 2);
}

#[test]
fn a_same_version_legacy_replacement_is_not_overwritten() {
    let dir = Scratch::new();
    let path = dir.0.join("journal.json");
    let mut legacy = serde_json::to_value(GatewayJournal::new(identity())).unwrap();
    legacy.as_object_mut().unwrap().remove("journal_id");
    std::fs::write(&path, serde_json::to_vec(&legacy).unwrap()).unwrap();
    let mut journal = GatewayJournal::load_bound(&path, &identity()).unwrap();
    let mut other = GatewayJournal::new(identity());
    other.reserve("alias", &card(), 10);
    let mut replacement = serde_json::to_value(other).unwrap();
    replacement.as_object_mut().unwrap().remove("journal_id");
    let replacement = serde_json::to_vec(&replacement).unwrap();
    std::fs::write(&path, &replacement).unwrap();
    assert!(
        journal.save(&path).is_err(),
        "legacy ownership comes from the loaded bytes, not version zero"
    );
    assert_eq!(std::fs::read(&path).unwrap(), replacement);
    assert_eq!(journal.version(), 0);
}

#[test]
fn legacy_binding_and_direct_repeated_saves_preserve_the_owner() {
    let dir = Scratch::new();
    for bind in [false, true] {
        let path = dir.0.join(format!("legacy-{bind}.json"));
        let mut document = serde_json::to_value(GatewayJournal::new(identity())).unwrap();
        document.as_object_mut().unwrap().remove("journal_id");
        std::fs::write(&path, serde_json::to_vec(&document).unwrap()).unwrap();
        let derived = JournalLock::document_id(&path).unwrap();
        let mut journal = GatewayJournal::load_bound(&path, &identity()).unwrap();
        let mut lock = JournalLock::acquire(&path).unwrap();
        if bind {
            lock.bind_document(&path, &mut journal).unwrap();
            assert_eq!(journal.journal_id(), Some(derived.as_str()));
        }
        journal.save(&path).unwrap();
        journal.save(&path).unwrap();
        assert_eq!(
            GatewayJournal::load_bound(&path, &identity())
                .unwrap()
                .version(),
            journal.version()
        );
    }
}
