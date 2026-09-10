# Artifact preflight integration checklist

G07 now covers the byte engine, the non-extracting TAR reader and the Docker
capture with its CLI refusal layer. What remains is delivery verification:
packaging, CI, the normal merge and the post-main checks.

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
- [x] Trusted artifact readers enumerate the declared scope and surface every
  read/enumeration failure. They must not follow host symlinks or extract
  untrusted archives into the host filesystem.
- [x] A Docker reader captures the exact locally resolved, pinned image without
  starting its entrypoint. Capture has byte/time limits and checked cleanup,
  including failure paths. Image volumes and other omitted export content
  must be explicitly handled or refused, not silently called scanned.
- [x] Include executable image configuration in the declared scope; a filesystem
  export alone does not cover embedded environment/command configuration.
- [x] CLI preflight refuses before entrant execution on known matches or
  incomplete scope. Reports bind policy, artifact identity and scanned scope;
  resumable invocation identity cannot silently change the scan policy.
- [x] Synthetic CLI fixtures prove that rejected entrants never launch.
  Docker-specific behavior needs its own labelled live CI test.
- [x] Document limits and unsupported encodings. A raw-byte negative result
  must never be described as proving an agent has not memorized held-out data.
- [ ] Verify packaging, CI, normal merge and post-main checks.

## Implemented TAR reader

`artifact_tar::scan_tar_snapshot` inspects uncompressed snapshots without
extracting them. It uses the `tar` crate's
[raw entry iterator](https://docs.rs/tar/0.4.46/tar/struct.Entries.html#method.raw)
so extended headers cannot trigger unbounded preprocessing or sparse expansion.
Headers and entry bodies are scanned separately, including long-name metadata
and link targets. Neither paths nor links become host filesystem operations.
Repeated paths are all scanned. Iteration continues past zero blocks so a
concatenated archive is not silently omitted.

The policy's stream count includes headers and bodies; the total-byte bound
also covers archive padding. A separate SHA-256 binds all consumed archive
bytes and is withheld on incomplete enumeration. Metadata bodies are capped
at 64 KiB before allocation. Sparse formats, global PAX headers, PAX size
overrides, malformed records and dangling or duplicate extensions are refused.
This is a raw TAR scope, not a general archive decoder or a reconstructed
filesystem certificate. Nine integration tests, a deadline unit test and
thirteen isolated mutations cover this reader.

## Implemented Docker capture and CLI refusal

`sharpebench run --image <repository@sha256:...> --scan-policy <policy.json>`
is opt in. Without `--scan-policy` the image path is byte for byte what it was.

Arguments and the policy are validated before Docker is invoked at all: a
policy without an image, more than one transport, an unpinned reference, an
unreadable, malformed or oversized policy each cost zero Docker commands.

The pinned reference is resolved with `docker image inspect`. The run continues
only for a full lowercase `sha256:` configuration ID, a Linux image and a
configuration object. Docker and its daemon are trusted infrastructure: no flag,
policy field or environment value selects the Docker executable or endpoint.

Declared image volumes refuse before anything is created, because
[container export omits volume contents](https://docs.docker.com/reference/cli/docker/container/export/)
and a filesystem scan would then be reported over a scope it did not cover.
Measured against Docker 29.7.2 (API 1.55), `Config.Volumes` is **omitted
entirely** for an image with no `VOLUME`, and is `{"/data":{}}` when one is
declared; older daemons send `null`. Omitted, null and empty all mean no
declared volumes, and each is accepted. Anything else refuses. The same daemon
trims empty configuration fields, so the shape check is "an object", not a list
of required keys.

The configuration is scanned twice over: the serialized document, and every
decoded string and object key inside it. A protected sequence containing a
newline appears in serialized JSON as two bytes, so a raw search of the
serialized form alone would miss it.

A uniquely named container is then created from the configuration ID, never
started, with `--pull never --network none --ipc none`, a read-only root, all
capabilities dropped, no new privileges, an unprivileged user and an entrypoint
that does not exist inside the image. The created container is inspected for the
same image ID, a created and non-running state and no mounts before it is
exported. The export goes to an owned capture file and is read by
`scan_tar_snapshot_until`, so the capture and the scan share one policy deadline.

Removal is attempted by the reserved name on every exit path, including a
creation whose outcome is unknown, under a separate ten-second allowance. A
nonzero removal is not read as already clean: Docker returns nonzero both for a
container that never existed and for one it could not remove, and the exit
status cannot tell them apart. Uncertainty refuses.

`sharpebench.image-preflight.v1` carries the image ID, the policy digest, the
configuration `RawScanReport`, an optional filesystem `TarScanReport` and
`cleanup_verified`. A launch is authorized only when the configuration scan and
the filesystem scan both completed with no matches and the removal was verified.
A configuration-only negative or any partial report is not sufficient. Only the
validated configuration ID is launched, never the mutable reference the operator
typed. A scanned run frames its checkpoint identity as
`("sharpebench.scanned-image-invocation.v1", sandbox_label, image_id,
policy_sha, scope)` and composes with the existing rate-card binding; unscanned
runs keep their legacy identity unchanged. No export timestamp is bound, so a
resume under the same policy is still the same experiment. A checkpoint bound to
a different scanned invocation is refused before any sweep call and its bytes
are left alone.

### Capture limits, stated honestly

Accepted output: 2 MiB per inspect document, 1024 bytes per create or remove
response, the policy's total-byte limit for the export, 64 KiB of diagnostics.
Inspect output is read under its cap before it is allocated or parsed, exit
status and final sizes are both checked, failed captures are killed and reaped,
and captures are rewound explicitly before parsing. Diagnostics are sanitized to
printable ASCII, bounded and shown only on the console; they never enter a
report, which may be published.

Spool sizes are polled, so they are an **accepted-output bound, not a disk
quota**: a child can overshoot by whatever it writes between two polls, and the
capture is ended once the overshoot is seen. The wall-clock deadline ends the
client this process spawned. It cannot interrupt a blocked OS read, it does not
reach Docker CLI descendants, and it does not stop daemon-side work that
continues after the client is killed, which is why removal is attempted on every
exit path and why an unverified removal refuses. A remote or rootless Docker
context is operator configuration and is trusted as infrastructure: the
environment this process inherits reaches the client unchanged, so an operator
who points `DOCKER_HOST` elsewhere has scanned an image on that host, not this
one.

### What a negative result is not

A completed negative report says that the named streams inside
`image-config-and-container-export/v1` did not contain the policy's protected
bytes. It is not evidence that an agent has not memorized held-out data.
Compressed, encoded, encrypted, chunked and model-internalized copies are all
outside raw-byte scope, and so is anything the daemon does not place in an
export.

Sixteen preflight unit tests cover the refusal, cleanup and capture paths
against an injected transport; eight CLI tests cover the shipped binary's
argument surface; one board regression proves a successful scanned run emits an
array with metadata on the entrant row only. The live leg,
`artifact_preflight::tests::live_docker_image_preflight`, runs by exact name in
the live-container CI job against the digest-pinned Alpine fixture.

Host-command and remote-HTTP execution do not prove deployed artifact identity
from an operator-supplied path alone. Any such audit must state this limitation
instead of presenting a local file scan as verification of a remote process.

This checklist implements the existing G07 scope. It adds no model experiment,
dataset acquisition or new ranking criterion.
