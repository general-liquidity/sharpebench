use std::io::{self, Read};

use serde_json::json;
use sharpebench_harness::artifact_scan::{IncompleteReason, RawScanPolicy, POLICY_VERSION};
use sharpebench_harness::artifact_tar::scan_tar_snapshot;

fn policy() -> RawScanPolicy {
    RawScanPolicy::from_json(
        &serde_json::to_vec(&json!({
            "schema_version": POLICY_VERSION,
            "utf8_sequences": ["protected-needle"],
            "file_sha256": [sharpebench_attest::content_digest(b"whole protected file")]
        }))
        .unwrap(),
    )
    .unwrap()
}

fn member(kind: u8, name: &[u8], body: &[u8]) -> Vec<u8> {
    let mut header = tar::Header::new_ustar();
    header.set_entry_type(tar::EntryType::new(kind));
    header.set_size(body.len() as u64);
    header.set_mode(0o644);
    header.as_mut_bytes()[..name.len()].copy_from_slice(name);
    header.set_cksum();
    let mut bytes = header.as_bytes().to_vec();
    bytes.extend_from_slice(body);
    bytes.resize(bytes.len().div_ceil(512) * 512, 0);
    bytes
}

fn pax(key: &str, value: &str) -> Vec<u8> {
    let suffix = format!(" {key}={value}\n");
    let mut length = suffix.len() + 1;
    while length != suffix.len() + length.to_string().len() {
        length = suffix.len() + length.to_string().len();
    }
    format!("{length}{suffix}").into_bytes()
}

#[test]
fn scans_repeated_paths_and_concatenated_archives_and_binds_all_bytes() {
    let mut bytes = member(b'0', b"same", b"benign");
    bytes.extend_from_slice(&[0; 1024]);
    bytes.extend(member(b'0', b"same", b"whole protected file"));
    bytes.extend(member(b'0', b"same", b"protected-needle"));
    bytes.extend_from_slice(&[0; 1024]);
    let report = scan_tar_snapshot(&bytes[..], policy());
    assert!(report.scan.completed);
    assert!(!report.no_known_matches());
    assert_eq!(report.scan.matches.len(), 2);
    assert_eq!(report.archive_bytes, bytes.len() as u64);
    assert_eq!(
        report.archive_sha256,
        Some(sharpebench_attest::content_digest(&bytes))
    );
    let benign = member(b'0', b"same", b"benign");
    assert!(scan_tar_snapshot(&benign[..], policy()).no_known_matches());
}

#[test]
fn paths_and_link_targets_are_scanned_bytes_not_host_operations() {
    let mut bytes = member(b'2', b"../../protected-needle", b"");
    let header = tar::Header::from_byte_slice(&bytes[..512]);
    let mut header = header.clone();
    header.set_link_name("/host/protected-needle").unwrap();
    header.set_cksum();
    bytes[..512].copy_from_slice(header.as_bytes());
    bytes.extend(member(b'1', b"absolute-link", b""));
    bytes.extend(member(b'5', b"/absolute/directory", b""));
    let report = scan_tar_snapshot(&bytes[..], policy());
    assert!(report.scan.completed);
    assert_eq!(report.scan.matches.len(), 1);
    assert!(!report.no_known_matches());
    let wire = serde_json::to_string(&report).unwrap();
    assert!(!wire.contains("protected-needle"));
    assert!(!wire.contains("/host/"));
}

#[test]
fn long_names_and_pax_metadata_are_scanned_without_interpreting_paths() {
    let mut bytes = member(b'L', b"long-name", b"very/long/protected-needle\0");
    bytes.extend(member(b'0', b"placeholder", b"benign"));
    bytes.extend(member(b'x', b"pax", &pax("path", "other/protected-needle")));
    bytes.extend(member(b'0', b"placeholder", b"benign"));
    let report = scan_tar_snapshot(&bytes[..], policy());
    assert!(report.scan.completed);
    assert_eq!(report.scan.matches.len(), 2);
}

#[test]
fn sparse_global_and_size_overrides_are_refused_instead_of_partially_scanned() {
    for kind in [b'S', b'g', b'?'] {
        let bytes = member(kind, b"unsupported", b"");
        assert_eq!(
            scan_tar_snapshot(&bytes[..], policy())
                .scan
                .incomplete_reason,
            Some(IncompleteReason::UnsupportedArtifact)
        );
    }
    for key in [
        "size",
        "GNU.sparse.map",
        "SCHILY.realsize",
        "SCHILY.filetype",
    ] {
        let bytes = member(b'x', b"pax", &pax(key, "1"));
        let report = scan_tar_snapshot(&bytes[..], policy());
        assert!(!report.no_known_matches());
        assert_eq!(
            report.scan.incomplete_reason,
            Some(IncompleteReason::UnsupportedArtifact)
        );
    }
}

#[test]
fn malformed_pax_records_cannot_hide_later_size_overrides() {
    for body in [
        b"not a record\n".to_vec(),
        [b"\n".to_vec(), pax("size", "1")].concat(),
        [pax("mtime", "1"), b"\n".to_vec(), pax("size", "1")].concat(),
    ] {
        let bytes = member(b'x', b"pax", &body);
        let report = scan_tar_snapshot(&bytes[..], policy());
        assert!(!report.no_known_matches());
        assert_eq!(
            report.scan.incomplete_reason,
            Some(IncompleteReason::EnumerationError)
        );
    }
}

#[test]
fn dangling_and_duplicate_extended_headers_are_not_complete_archives() {
    let extension = member(b'L', b"long-name", b"a long file name\0");
    let mut duplicate = extension.clone();
    duplicate.extend(&extension);
    duplicate.extend(member(b'0', b"file", b"benign"));
    for bytes in [extension, duplicate] {
        let report = scan_tar_snapshot(&bytes[..], policy());
        assert_eq!(
            report.scan.incomplete_reason,
            Some(IncompleteReason::EnumerationError)
        );
        assert!(!report.no_known_matches());
    }
}

#[test]
fn large_extension_is_refused_before_body_read() {
    let mut header = tar::Header::new_ustar();
    header.set_entry_type(tar::EntryType::GNULongName);
    header.set_size(65537);
    header.set_cksum();
    struct HeaderOnly<'a>(&'a [u8]);
    impl Read for HeaderOnly<'_> {
        fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
            assert!(
                !self.0.is_empty(),
                "oversized metadata body must not be read"
            );
            self.0.read(out)
        }
    }
    let report = scan_tar_snapshot(HeaderOnly(header.as_bytes()), policy());
    assert_eq!(
        report.scan.incomplete_reason,
        Some(IncompleteReason::UnsupportedArtifact)
    );
    assert_eq!(report.archive_bytes, 512);
}

#[test]
fn limits_cover_padding_and_header_streams_not_only_regular_file_bytes() {
    let bytes = member(b'0', b"file", b"benign");
    for (files, total, reason) in [
        (10, 1023, IncompleteReason::ByteLimit),
        (1, 1024, IncompleteReason::FileLimit),
    ] {
        let policy = RawScanPolicy::from_json(
            &serde_json::to_vec(&json!({
                "schema_version":POLICY_VERSION, "utf8_sequences":["protected-needle"],
                "limits":{"max_files":files, "max_file_bytes":512,
                    "max_total_bytes":total,"max_seconds":60}
            }))
            .unwrap(),
        )
        .unwrap();
        let report = scan_tar_snapshot(&bytes[..], policy);
        assert!(!report.no_known_matches());
        assert_eq!(report.scan.incomplete_reason, Some(reason));
        assert!(report.archive_sha256.is_none());
    }
}

#[test]
fn corrupt_truncated_empty_and_read_failed_archives_never_pass() {
    let bytes = member(b'0', b"file", b"benign");
    let mut corrupt = bytes.clone();
    corrupt[0] ^= 1;
    for bad in [
        corrupt,
        bytes[..514].to_vec(),
        bytes[..1023].to_vec(),
        vec![],
        vec![0; 1024],
    ] {
        let report = scan_tar_snapshot(&bad[..], policy());
        assert!(!report.no_known_matches());
        assert!(report.archive_sha256.is_none());
    }
    struct Broken;
    impl Read for Broken {
        fn read(&mut self, _: &mut [u8]) -> io::Result<usize> {
            Err(io::Error::other("private error not for output"))
        }
    }
    let report = scan_tar_snapshot(Broken, policy());
    assert_eq!(
        report.scan.incomplete_reason,
        Some(IncompleteReason::ReadError)
    );
}
