//! Non-extracting scan of an uncompressed TAR snapshot.
//!
//! Raw iteration deliberately avoids the TAR library's extended-header
//! allocation and sparse-file expansion. Each header and entry body is a
//! separate scanned stream, identified by archive order, not a host path.
//! Thus repeated paths, link targets and long-name metadata are all inspected
//! without following links or interpreting names as filesystem operations.
//! Sparse formats and size-override extensions are refused, not partially
//! reconstructed. Encoded content within regular files remains raw content.
//!
//! The caller must provide a non-blocking or deadline-bounded snapshot reader.
//! The checks here bound bytes and time between reads, not a blocked OS read.

use std::io::{self, Read};
use std::time::{Duration, Instant};

use serde::Serialize;
use sha2::{Digest, Sha256};

use crate::artifact_scan::{IncompleteReason, RawScanPolicy, RawScanReport, RawScanner};

const MAX_EXTENSION_BYTES: u64 = 64 * 1024;

#[derive(Debug, Serialize)]
pub struct TarScanReport {
    pub scope: &'static str,
    pub archive_bytes: u64,
    /// Present only after complete enumeration and EOF, including padding.
    pub archive_sha256: Option<String>,
    pub scan: RawScanReport,
}

impl TarScanReport {
    pub fn no_known_matches(&self) -> bool {
        self.archive_sha256.is_some() && self.scan.no_known_matches()
    }
}

struct Bounded<R> {
    inner: R,
    bytes: u64,
    limit: u64,
    deadline: Instant,
    hash: Sha256,
    failure: Option<IncompleteReason>,
}

impl<R: Read> Read for Bounded<R> {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        if buffer.is_empty() {
            return Ok(0);
        }
        if self.failure.is_some() {
            return Err(io::Error::other("archive reader already failed"));
        }
        if Instant::now() >= self.deadline {
            self.failure = Some(IncompleteReason::Deadline);
            return Err(io::Error::new(io::ErrorKind::TimedOut, "archive deadline"));
        }
        // An extra byte distinguishes exact-limit EOF from an oversized input.
        let cap = buffer.len().min((self.limit - self.bytes + 1) as usize);
        let count = match self.inner.read(&mut buffer[..cap]) {
            Ok(count) => count,
            Err(error) => {
                if error.kind() != io::ErrorKind::Interrupted {
                    self.failure = Some(IncompleteReason::ReadError);
                }
                return Err(error);
            }
        };
        self.bytes += count as u64;
        if self.bytes > self.limit {
            self.failure = Some(IncompleteReason::ByteLimit);
            return Err(io::Error::other("archive byte limit"));
        }
        self.hash.update(&buffer[..count]);
        Ok(count)
    }
}

/// Scan every raw header and payload, including entries after TAR zero blocks.
/// Policy `max_files` counts scanned streams (header and body separately).
/// `max_total_bytes` also caps the full archive, including padding. Malformed
/// archives, unsupported formats and interrupted enumeration cannot pass.
pub fn scan_tar_snapshot(reader: impl Read, policy: RawScanPolicy) -> TarScanReport {
    let bounded = Bounded {
        inner: reader,
        bytes: 0,
        limit: policy.limits().max_total_bytes,
        deadline: Instant::now() + Duration::from_secs(policy.limits().max_seconds),
        hash: Sha256::new(),
        failure: None,
    };
    let mut scanner = RawScanner::new(policy);
    let mut archive = tar::Archive::new(bounded);
    // Do not silently omit a concatenated archive or data after an EOF marker.
    archive.set_ignore_zeros(true);
    let result = scan_entries(&mut archive, &mut scanner);
    let bounded = archive.into_inner();
    if let Some(reason) = bounded.failure {
        scanner.invalidate(reason);
    } else if result.is_err() {
        scanner.invalidate(IncompleteReason::EnumerationError);
    }
    let scan = scanner.finish();
    TarScanReport {
        scope: "raw-tar-headers-and-entry-payloads/v1",
        archive_bytes: bounded.bytes,
        archive_sha256: scan
            .completed
            .then(|| format!("{:x}", bounded.hash.finalize())),
        scan,
    }
}

fn scan_entries<R: Read>(
    archive: &mut tar::Archive<R>,
    scanner: &mut RawScanner,
) -> io::Result<()> {
    let mut pending_extensions = 0_u8;
    for (index, entry) in archive.entries()?.raw(true).enumerate() {
        let mut entry = entry?;
        let kind = entry.header().entry_type();
        let size = entry.size();
        let extension =
            kind.is_gnu_longname() || kind.is_gnu_longlink() || kind.is_pax_local_extensions();
        let extension_bit = if kind.is_gnu_longname() {
            1
        } else if kind.is_gnu_longlink() {
            2
        } else if kind.is_pax_local_extensions() {
            4
        } else {
            0
        };
        if extension {
            if pending_extensions & extension_bit != 0 {
                return Err(io::Error::other("duplicate pending TAR extension"));
            }
            pending_extensions |= extension_bit;
        } else {
            pending_extensions = 0;
        }
        if !(kind.is_file()
            || kind.is_contiguous()
            || kind.is_dir()
            || kind.is_hard_link()
            || kind.is_symlink()
            || kind.is_character_special()
            || kind.is_block_special()
            || kind.is_fifo()
            || extension)
            || (extension && size > MAX_EXTENSION_BYTES)
        {
            scanner.invalidate(IncompleteReason::UnsupportedArtifact);
            return Ok(());
        }
        if !scanner.scan_file(
            format!("tar/{index}/header").as_bytes(),
            512,
            &entry.header().as_bytes()[..],
        ) {
            return Ok(());
        }
        let name = format!("tar/{index}/body");
        if kind.is_pax_local_extensions() {
            // At most 64 KiB, checked before allocation. Raw iteration does not
            // apply PAX sizes; refuse those overrides instead of misparsing the
            // next header or claiming to hash a reconstructed sparse file.
            let mut metadata = Vec::new();
            entry.read_to_end(&mut metadata)?;
            // The dependency iterator ends at an empty line. Refuse one rather
            // than allowing unexamined size/sparse keys after it.
            if metadata.first() == Some(&b'\n') || metadata.windows(2).any(|pair| pair == b"\n\n") {
                return Err(io::Error::other("empty PAX record"));
            }
            for item in tar::PaxExtensions::new(&metadata) {
                let item = item?;
                let key = item.key_bytes();
                if key == b"size"
                    || key.starts_with(b"GNU.sparse")
                    || key == b"SCHILY.realsize"
                    || key == b"SCHILY.filetype"
                {
                    scanner.invalidate(IncompleteReason::UnsupportedArtifact);
                    return Ok(());
                }
            }
            if !scanner.scan_file(name.as_bytes(), size, &metadata[..]) {
                return Ok(());
            }
        } else if !scanner.scan_file(name.as_bytes(), size, &mut entry) {
            return Ok(());
        }
    }
    if pending_extensions != 0 {
        return Err(io::Error::other("TAR extension has no following entry"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expired_reader_refuses_before_reading_input() {
        let mut reader = Bounded {
            inner: &b"byte"[..],
            bytes: 0,
            limit: 16,
            deadline: Instant::now() - Duration::from_secs(1),
            hash: Sha256::new(),
            failure: None,
        };
        assert!(reader.read(&mut [0; 1]).is_err());
        assert_eq!(reader.failure, Some(IncompleteReason::Deadline));
        assert_eq!(reader.inner, b"byte");
    }
}
