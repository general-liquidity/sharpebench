//! Bounded known-content matching over raw file contents.
//!
//! This is the byte engine, not an artifact enumerator or a contamination
//! certificate. Trusted callers must enumerate the entire declared scope, use
//! bounded/deadline-aware readers, and call `invalidate` on enumeration errors.
//! Encoded, compressed, transformed and previously memorized data are not
//! excluded by a negative result. No input is executed or extracted here.

use std::collections::BTreeSet;
use std::io::{ErrorKind, Read};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub const POLICY_VERSION: &str = "sharpebench.raw-scan-policy.v1";
pub const MAX_POLICY_BYTES: usize = 64 * 1024;
const MAX_MATCHES: usize = 256;

/// Resource bounds included in the frozen policy identity. Reader blocking
/// deadlines remain the caller's responsibility; the clock here bounds work
/// between reads, not an indefinitely blocked `Read` implementation.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ScanLimits {
    pub max_files: u64,
    pub max_file_bytes: u64,
    pub max_total_bytes: u64,
    pub max_seconds: u64,
}

impl Default for ScanLimits {
    fn default() -> Self {
        Self {
            max_files: 32_768,
            max_file_bytes: 64 * 1024 * 1024,
            max_total_bytes: 512 * 1024 * 1024,
            max_seconds: 60,
        }
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PolicyWire {
    schema_version: String,
    #[serde(default)]
    file_sha256: Vec<String>,
    #[serde(default)]
    utf8_sequences: Vec<String>,
    #[serde(default)]
    limits: ScanLimits,
}

/// Validated policy. Sequence strings are sensitive: reports refer to their
/// zero-based indices, never echo protected data or canary values.
#[derive(Clone, Serialize)]
pub struct RawScanPolicy {
    schema_version: String,
    file_sha256: Vec<String>,
    utf8_sequences: Vec<String>,
    limits: ScanLimits,
}

impl RawScanPolicy {
    pub fn from_json(bytes: &[u8]) -> Result<Self, String> {
        if bytes.len() > MAX_POLICY_BYTES {
            return Err("scan policy exceeds 64 KiB".into());
        }
        let wire: PolicyWire = serde_json::from_slice(bytes)
            .map_err(|_| "scan policy is not valid under its closed schema")?;
        if wire.schema_version != POLICY_VERSION {
            return Err("unsupported scan policy version".into());
        }
        if wire.file_sha256.is_empty() && wire.utf8_sequences.is_empty() {
            return Err("scan policy must contain at least one protected-content rule".into());
        }
        if wire.file_sha256.len() > 256 || wire.utf8_sequences.len() > 32 {
            return Err("scan policy exceeds the rule-count limit".into());
        }
        let mut digests = BTreeSet::new();
        for digest in &wire.file_sha256 {
            if digest.len() != 64
                || !digest
                    .bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
                || !digests.insert(digest)
            {
                return Err("file digests must be unique lowercase SHA-256 values".into());
            }
        }
        let mut sequences = BTreeSet::new();
        for sequence in &wire.utf8_sequences {
            if !(16..=4096).contains(&sequence.len()) || !sequences.insert(sequence) {
                return Err("protected sequences must be unique and 16 to 4096 UTF-8 bytes".into());
            }
        }
        let limits = &wire.limits;
        if !(1..=100_000).contains(&limits.max_files)
            || !(1..=512 * 1024 * 1024).contains(&limits.max_file_bytes)
            || !(1..=4 * 1024 * 1024 * 1024).contains(&limits.max_total_bytes)
            || !(1..=120).contains(&limits.max_seconds)
            || limits.max_file_bytes > limits.max_total_bytes
        {
            return Err("scan limits are outside the supported bounded domain".into());
        }
        Ok(Self {
            schema_version: wire.schema_version,
            file_sha256: wire.file_sha256,
            utf8_sequences: wire.utf8_sequences,
            limits: wire.limits,
        })
    }

    pub fn digest(&self) -> String {
        sharpebench_attest::content_digest(
            &serde_json::to_vec(self).expect("validated scan policy serializes"),
        )
    }

    pub fn limits(&self) -> &ScanLimits {
        &self.limits
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum IncompleteReason {
    EmptyScope,
    FileLimit,
    ByteLimit,
    Deadline,
    ReadError,
    SizeMismatch,
    DuplicateEntry,
    InvalidEntryName,
    MatchLimit,
    EnumerationError,
    UnsupportedArtifact,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum MatchKind {
    FileSha256,
    Utf8Sequence,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ContentMatch {
    pub entry_index: u64,
    pub entry_name_sha256: String,
    pub kind: MatchKind,
    pub rule_index: usize,
}

/// Evidence about exactly the raw file streams supplied to the engine.
/// No Deserialize implementation: an entrant-supplied report is not a scan.
#[derive(Debug, Serialize)]
pub struct RawScanReport {
    pub schema_version: &'static str,
    pub scope: &'static str,
    pub policy_sha256: String,
    pub completed: bool,
    pub incomplete_reason: Option<IncompleteReason>,
    pub files_started: u64,
    pub bytes_read: u64,
    pub matches: Vec<ContentMatch>,
    /// Framed ordered entry-name/content identities; absent for partial scans.
    pub inventory_sha256: Option<String>,
}

impl RawScanReport {
    /// A negative result within the stated raw-byte scope, not proof of
    /// contamination freedom. Callers must separately establish artifact scope.
    pub fn no_known_matches(&self) -> bool {
        self.completed && self.matches.is_empty()
    }
}

// Knuth-Morris-Pratt matching preserves state across read boundaries and has
// linear work per rule, including repeated-prefix inputs.
struct Needle {
    bytes: Vec<u8>,
    prefix: Vec<usize>,
    position: usize,
    found: bool,
}

impl Needle {
    fn new(bytes: &[u8]) -> Self {
        let mut prefix = vec![0; bytes.len()];
        let mut j = 0;
        for i in 1..bytes.len() {
            while j > 0 && bytes[i] != bytes[j] {
                j = prefix[j - 1];
            }
            if bytes[i] == bytes[j] {
                j += 1;
            }
            prefix[i] = j;
        }
        Self {
            bytes: bytes.to_vec(),
            prefix,
            position: 0,
            found: false,
        }
    }

    fn feed(&mut self, bytes: &[u8]) {
        if self.found {
            return;
        }
        for &byte in bytes {
            while self.position > 0 && self.bytes[self.position] != byte {
                self.position = self.prefix[self.position - 1];
            }
            if self.bytes[self.position] == byte {
                self.position += 1;
            }
            if self.position == self.bytes.len() {
                self.found = true;
                return;
            }
        }
    }
}

pub struct RawScanner {
    policy: RawScanPolicy,
    started: Instant,
    report: RawScanReport,
    names: BTreeSet<[u8; 32]>,
    inventory: Sha256,
}

impl RawScanner {
    pub fn new(policy: RawScanPolicy) -> Self {
        let report = RawScanReport {
            schema_version: "sharpebench.raw-scan-report.v1",
            scope: "raw-file-contents/v1",
            policy_sha256: policy.digest(),
            completed: false,
            incomplete_reason: None,
            files_started: 0,
            bytes_read: 0,
            matches: Vec::new(),
            inventory_sha256: None,
        };
        let mut inventory = Sha256::new();
        inventory.update(b"sharpebench.raw-file-inventory.v1\0");
        Self {
            policy,
            started: Instant::now(),
            report,
            names: BTreeSet::new(),
            inventory,
        }
    }

    pub fn invalidate(&mut self, reason: IncompleteReason) {
        self.report.incomplete_reason.get_or_insert(reason);
    }

    fn fail(&mut self, reason: IncompleteReason) -> bool {
        self.invalidate(reason);
        false
    }

    /// Scan one regular file stream through EOF. Return false to stop: the
    /// report is permanently incomplete and cannot become a passing preflight.
    /// Entry names are identity bytes, never interpreted as host paths here.
    pub fn scan_file(&mut self, name: &[u8], size: u64, mut reader: impl Read) -> bool {
        if self.report.incomplete_reason.is_some() {
            return false;
        }
        if name.is_empty() || name.len() > 4096 {
            return self.fail(IncompleteReason::InvalidEntryName);
        }
        if self.report.files_started >= self.policy.limits.max_files {
            return self.fail(IncompleteReason::FileLimit);
        }
        if size > self.policy.limits.max_file_bytes
            || size > self.policy.limits.max_total_bytes - self.report.bytes_read
        {
            return self.fail(IncompleteReason::ByteLimit);
        }
        let name_digest: [u8; 32] = Sha256::digest(name).into();
        if !self.names.insert(name_digest) {
            return self.fail(IncompleteReason::DuplicateEntry);
        }
        let index = self.report.files_started;
        self.report.files_started += 1;
        let mut needles: Vec<_> = self
            .policy
            .utf8_sequences
            .iter()
            .map(|s| Needle::new(s.as_bytes()))
            .collect();
        let mut hash = Sha256::new();
        let mut read = 0;
        let mut buffer = [0_u8; 8192];
        loop {
            if self.started.elapsed() >= Duration::from_secs(self.policy.limits.max_seconds) {
                return self.fail(IncompleteReason::Deadline);
            }
            // One extra byte detects a stream larger than its declaration.
            let cap = buffer.len().min((size - read + 1) as usize);
            let result = reader.read(&mut buffer[..cap]);
            if self.started.elapsed() >= Duration::from_secs(self.policy.limits.max_seconds) {
                return self.fail(IncompleteReason::Deadline);
            }
            let count = match result {
                Ok(0) => break,
                Ok(count) => count,
                Err(error) if error.kind() == ErrorKind::Interrupted => continue,
                Err(_) => return self.fail(IncompleteReason::ReadError),
            };
            self.report.bytes_read += count as u64;
            read += count as u64;
            if read > size {
                return self.fail(IncompleteReason::SizeMismatch);
            }
            hash.update(&buffer[..count]);
            for needle in &mut needles {
                needle.feed(&buffer[..count]);
            }
        }
        if read != size {
            return self.fail(IncompleteReason::SizeMismatch);
        }
        let digest = format!("{:x}", hash.finalize());
        let mut hits = Vec::new();
        for (rule_index, known) in self.policy.file_sha256.iter().enumerate() {
            if known == &digest {
                hits.push((MatchKind::FileSha256, rule_index));
            }
        }
        for (rule_index, needle) in needles.iter().enumerate() {
            if needle.found {
                hits.push((MatchKind::Utf8Sequence, rule_index));
            }
        }
        for (kind, rule_index) in hits {
            if self.report.matches.len() >= MAX_MATCHES {
                return self.fail(IncompleteReason::MatchLimit);
            }
            self.report.matches.push(ContentMatch {
                entry_index: index,
                entry_name_sha256: sharpebench_attest::content_digest(name),
                kind,
                rule_index,
            });
        }
        self.inventory.update((name.len() as u64).to_be_bytes());
        self.inventory.update(name);
        self.inventory.update(size.to_be_bytes());
        self.inventory.update(digest.as_bytes());
        true
    }

    pub fn finish(mut self) -> RawScanReport {
        if self.started.elapsed() >= Duration::from_secs(self.policy.limits.max_seconds) {
            self.invalidate(IncompleteReason::Deadline);
        }
        if self.report.files_started == 0 || self.report.bytes_read == 0 {
            self.invalidate(IncompleteReason::EmptyScope);
        }
        if self.report.incomplete_reason.is_none() {
            self.report.completed = true;
            self.report.inventory_sha256 = Some(format!("{:x}", self.inventory.finalize()));
        }
        self.report
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expired_scans_cannot_pass_even_when_no_further_read_occurs() {
        let policy = RawScanPolicy::from_json(
            br#"{"schema_version":"sharpebench.raw-scan-policy.v1","utf8_sequences":["protected-needle"]}"#,
        ).unwrap();
        let mut scanner = RawScanner::new(policy);
        assert!(scanner.scan_file(b"file", 1, &b"a"[..]));
        scanner.started = Instant::now() - Duration::from_secs(121);
        assert_eq!(
            scanner.finish().incomplete_reason,
            Some(IncompleteReason::Deadline)
        );
    }
}
