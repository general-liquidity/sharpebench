# Artifact preflight integration checklist

G07 remains in progress. A tested byte matcher is not a completed preflight.

## Implemented engine

Bench's `sharpebench_harness::artifact_scan` reads named regular-file streams
without executing or extracting them. The closed policy contains whole-file
SHA-256 rules and exact UTF-8 sequences, such as a series prefix or a canary.
Rule indices appear in findings; protected strings and entry names do not.

Limits bind the file count, per-file bytes, aggregate bytes and elapsed time.
The reader consumes at most one byte beyond a declared size to detect a
mismatch. Read errors, truncation, growth, duplicate entries, expired scans,
empty scope and exceeded match storage cannot produce a passing report.
Sequence state survives read boundaries, with linear work per rule.

The engine does not enumerate artifacts. Its clock checks cannot interrupt an
arbitrary blocking reader. Its ordered inventory digest identifies the named
streams supplied by its caller, not an independently authenticated deployment.

## Required before G07 closes

- [x] Validated bounded policy, whole-file hashing and streaming sequence matching.
- [x] Explicit partial-scan reasons and non-disclosing, bounded match reports.
- [x] Negative and positive fixtures, boundary cases and isolated mutation checks.
- [ ] Trusted artifact readers enumerate the declared scope and surface every
  read/enumeration failure. They must not follow host symlinks or extract
  untrusted archives into the host filesystem.
- [ ] A Docker reader captures the exact locally resolved, pinned image without
  starting its entrypoint. Capture has byte/time limits and checked cleanup,
  including failure paths. Image volumes and other omitted export content
  must be explicitly handled or refused, not silently called scanned.
- [ ] Include executable image configuration in the declared scope; a filesystem
  export alone does not cover embedded environment/command configuration.
- [ ] CLI preflight refuses before entrant execution on known matches or
  incomplete scope. Reports bind policy, artifact identity and scanned scope;
  resumable invocation identity cannot silently change the scan policy.
- [ ] Synthetic CLI fixtures prove that rejected entrants never launch.
  Docker-specific behavior needs its own labelled live CI test.
- [ ] Document limits and unsupported encodings. A raw-byte negative result
  must never be described as proving an agent has not memorized held-out data.
- [ ] Verify packaging, CI, normal merge and post-main checks.

Host-command and remote-HTTP execution do not prove deployed artifact identity
from an operator-supplied path alone. Any such audit must state this limitation
instead of presenting a local file scan as verification of a remote process.

This checklist implements the existing G07 scope. It adds no model experiment,
dataset acquisition or new ranking criterion.
