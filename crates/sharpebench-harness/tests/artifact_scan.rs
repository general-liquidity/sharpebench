use std::io::{self, Read};

use serde_json::{json, Value};
use sharpebench_harness::artifact_scan::{
    IncompleteReason, MatchKind, RawScanPolicy, RawScanner, POLICY_VERSION,
};

fn policy_value() -> Value {
    json!({"schema_version": POLICY_VERSION, "utf8_sequences": ["protected-needle"]})
}

fn policy(value: &Value) -> RawScanPolicy {
    RawScanPolicy::from_json(&serde_json::to_vec(value).unwrap()).unwrap()
}

fn small_limits() -> Value {
    json!({"max_files": 2, "max_file_bytes": 20, "max_total_bytes": 25, "max_seconds": 60})
}

#[test]
fn closed_nonempty_policy_has_stable_validated_identity() {
    let value = policy_value();
    let original = policy(&value);
    let pretty = RawScanPolicy::from_json(&serde_json::to_vec_pretty(&value).unwrap()).unwrap();
    assert_eq!(original.digest(), pretty.digest());
    let mut changed = value.clone();
    changed["limits"] = small_limits();
    assert_ne!(original.digest(), policy(&changed).digest());
    for bad in [
        json!({"schema_version": POLICY_VERSION}),
        json!({"schema_version": "v0", "utf8_sequences": ["protected-needle"]}),
        json!({"schema_version": POLICY_VERSION, "utf8_sequences": [""]}),
        json!({"schema_version": POLICY_VERSION, "utf8_sequences": ["protected-needle", "protected-needle"]}),
        json!({"schema_version": POLICY_VERSION, "file_sha256": ["A".repeat(64)]}),
        json!({"schema_version": POLICY_VERSION, "file_sha256": ["0".repeat(64), "0".repeat(64)]}),
        json!({"schema_version": POLICY_VERSION, "utf8_sequences": ["protected-needle"], "unknown": true}),
    ] {
        assert!(RawScanPolicy::from_json(&serde_json::to_vec(&bad).unwrap()).is_err());
    }
    let mut bad = value;
    bad["limits"] = small_limits();
    for (key, value) in [
        ("max_files", json!(0)),
        ("max_file_bytes", json!(26)),
        ("max_total_bytes", json!(0)),
        ("max_seconds", json!(121)),
    ] {
        let mut invalid = bad.clone();
        invalid["limits"][key] = value;
        assert!(RawScanPolicy::from_json(&serde_json::to_vec(&invalid).unwrap()).is_err());
    }
    assert!(RawScanPolicy::from_json(&vec![b' '; 65537]).is_err());
}

struct Fragments<'a>(&'a [u8], usize);
impl Read for Fragments<'_> {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        let n = buffer.len().min(self.1).min(self.0.len());
        buffer[..n].copy_from_slice(&self.0[..n]);
        self.0 = &self.0[n..];
        Ok(n)
    }
}

#[test]
fn matches_span_reads_and_reports_do_not_echo_protected_content() {
    let mut scanner = RawScanner::new(policy(&policy_value()));
    let mut bytes = vec![b'x'; 8187];
    bytes.extend_from_slice(b"protected-needle");
    bytes.extend_from_slice(b"suffix");
    assert!(scanner.scan_file(b"private-name", bytes.len() as u64, Fragments(&bytes, 7)));
    let report = scanner.finish();
    assert!(report.completed);
    assert!(!report.no_known_matches());
    assert_eq!(report.matches.len(), 1);
    assert_eq!(report.matches[0].kind, MatchKind::Utf8Sequence);
    assert_eq!(report.matches[0].rule_index, 0);
    let json = serde_json::to_string(&report).unwrap();
    assert!(!json.contains("protected-needle"));
    assert!(!json.contains("private-name"));
}

#[test]
fn repeated_prefixes_and_binary_data_match_without_resetting_across_reads() {
    let value = json!({"schema_version":POLICY_VERSION, "utf8_sequences":["aaaaaaaaaaaaaaab"]});
    let mut bytes = vec![b'a'; 20_000];
    bytes.push(b'b');
    bytes.push(255);
    let mut scanner = RawScanner::new(policy(&value));
    assert!(scanner.scan_file(b"file", bytes.len() as u64, Fragments(&bytes, 1)));
    assert_eq!(scanner.finish().matches.len(), 1);
}

#[test]
fn digest_rules_match_entire_files_not_fragments() {
    let data = b"exact protected file";
    let digest = sharpebench_attest::content_digest(data);
    let value = json!({"schema_version":POLICY_VERSION, "file_sha256":[digest]});
    let mut scanner = RawScanner::new(policy(&value));
    assert!(scanner.scan_file(b"one", data.len() as u64, &data[..]));
    let mut different = data.to_vec();
    different.push(b'!');
    assert!(scanner.scan_file(b"two", different.len() as u64, &different[..]));
    let report = scanner.finish();
    assert_eq!(report.matches.len(), 1);
    assert_eq!(report.matches[0].kind, MatchKind::FileSha256);
    assert_eq!(report.matches[0].entry_index, 0);
    assert_eq!(report.files_started, 2);
    assert_eq!(report.bytes_read, 2 * data.len() as u64 + 1);
}

#[test]
fn negative_results_bind_names_and_bytes_and_refuse_empty_scopes() {
    let scan = |name: &[u8], bytes: &[u8]| {
        let mut scanner = RawScanner::new(policy(&policy_value()));
        assert!(scanner.scan_file(name, bytes.len() as u64, bytes));
        scanner.finish()
    };
    let a = scan(b"one", b"abc");
    assert!(a.no_known_matches());
    assert_eq!(a.inventory_sha256, scan(b"one", b"abc").inventory_sha256);
    assert_ne!(a.inventory_sha256, scan(b"two", b"abc").inventory_sha256);
    assert_ne!(a.inventory_sha256, scan(b"one", b"abd").inventory_sha256);
    for report in [
        RawScanner::new(policy(&policy_value())).finish(),
        scan(b"empty", b""),
    ] {
        assert!(!report.no_known_matches());
        assert_eq!(report.incomplete_reason, Some(IncompleteReason::EmptyScope));
        assert!(report.inventory_sha256.is_none());
    }
}

#[test]
fn limits_apply_before_reads_and_across_files() {
    struct MustNotRead;
    impl Read for MustNotRead {
        fn read(&mut self, _: &mut [u8]) -> io::Result<usize> {
            panic!("limit must refuse before read")
        }
    }
    let mut value = policy_value();
    value["limits"] = small_limits();
    let mut scanner = RawScanner::new(policy(&value));
    assert!(!scanner.scan_file(b"large", 21, MustNotRead));
    assert_eq!(
        scanner.finish().incomplete_reason,
        Some(IncompleteReason::ByteLimit)
    );
    let mut scanner = RawScanner::new(policy(&value));
    assert!(scanner.scan_file(b"one", 20, &[b'x'; 20][..]));
    assert!(!scanner.scan_file(b"two", 6, MustNotRead));
    assert_eq!(
        scanner.finish().incomplete_reason,
        Some(IncompleteReason::ByteLimit)
    );
    let mut scanner = RawScanner::new(policy(&value));
    assert!(scanner.scan_file(b"one", 1, &b"x"[..]));
    assert!(scanner.scan_file(b"two", 1, &b"x"[..]));
    assert!(!scanner.scan_file(b"three", 1, MustNotRead));
    assert_eq!(
        scanner.finish().incomplete_reason,
        Some(IncompleteReason::FileLimit)
    );
}

#[test]
fn changing_sizes_and_reader_errors_cannot_produce_complete_reports() {
    for (size, bytes) in [(1, &b"xx"[..]), (3, &b"xx"[..])] {
        let mut scanner = RawScanner::new(policy(&policy_value()));
        assert!(!scanner.scan_file(b"one", size, bytes));
        assert_eq!(
            scanner.finish().incomplete_reason,
            Some(IncompleteReason::SizeMismatch)
        );
    }
    struct Broken;
    impl Read for Broken {
        fn read(&mut self, _: &mut [u8]) -> io::Result<usize> {
            Err(io::Error::other("fixture"))
        }
    }
    let mut scanner = RawScanner::new(policy(&policy_value()));
    assert!(!scanner.scan_file(b"one", 1, Broken));
    assert!(!scanner.scan_file(b"another", 1, &b"x"[..]));
    let report = scanner.finish();
    assert!(!report.no_known_matches());
    assert_eq!(report.incomplete_reason, Some(IncompleteReason::ReadError));
}

#[test]
fn duplicate_entries_and_external_enumeration_failure_are_sticky() {
    let mut scanner = RawScanner::new(policy(&policy_value()));
    assert!(scanner.scan_file(b"one", 1, &b"x"[..]));
    assert!(!scanner.scan_file(b"one", 1, &b"x"[..]));
    assert_eq!(
        scanner.finish().incomplete_reason,
        Some(IncompleteReason::DuplicateEntry)
    );
    let mut scanner = RawScanner::new(policy(&policy_value()));
    assert!(scanner.scan_file(b"one", 1, &b"x"[..]));
    scanner.invalidate(IncompleteReason::EnumerationError);
    assert!(!scanner.scan_file(b"two", 1, &b"x"[..]));
    assert!(!scanner.finish().no_known_matches());
}

#[test]
fn match_list_is_bounded_and_entry_identity_is_bounded() {
    let mut scanner = RawScanner::new(policy(&policy_value()));
    for i in 0..256 {
        assert!(scanner.scan_file(i.to_string().as_bytes(), 16, &b"protected-needle"[..]));
    }
    assert!(!scanner.scan_file(b"overflow", 16, &b"protected-needle"[..]));
    let report = scanner.finish();
    assert_eq!(report.matches.len(), 256);
    assert_eq!(report.incomplete_reason, Some(IncompleteReason::MatchLimit));
    for name in [vec![], vec![b'a'; 4097]] {
        let mut scanner = RawScanner::new(policy(&policy_value()));
        assert!(!scanner.scan_file(&name, 1, &b"x"[..]));
        assert_eq!(
            scanner.finish().incomplete_reason,
            Some(IncompleteReason::InvalidEntryName)
        );
    }
}
