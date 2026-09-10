# Entrant image preflight

`sharpebench run --image <repository@sha256:...> --scan-policy <policy.json>`
scans a digest-pinned container image for operator-declared protected content
**before** the entrant is launched. It is opt in. Without `--scan-policy` the
`--image` path is byte for byte what it was.

The question the preflight answers is narrow and worth stating first: did the
named streams inside the declared scope contain the exact bytes the policy
protects? A negative result is not evidence that an agent has not memorized
held-out data. Compressed, encoded, encrypted, chunked and model-internalized
copies are all outside raw-byte scope, and so is anything the daemon does not
place in a container export.

## The policy

The policy is a closed JSON schema, `sharpebench.raw-scan-policy.v1`, read once
and capped at 64 KiB. Unknown fields are refused.

```json
{
  "schema_version": "sharpebench.raw-scan-policy.v1",
  "file_sha256": ["<64 lowercase hex characters>"],
  "utf8_sequences": ["a-protected-sequence-of-at-least-16-bytes"],
  "limits": {
    "max_files": 32768,
    "max_file_bytes": 67108864,
    "max_total_bytes": 536870912,
    "max_seconds": 60
  }
}
```

| Rule | Meaning | Bounds |
|---|---|---|
| `file_sha256` | Whole-file SHA-256 of a protected artifact | Unique lowercase hex, at most 256 rules |
| `utf8_sequences` | An exact UTF-8 byte sequence, such as a held-out series prefix or a [canary](cli.md) token | Unique, 16 to 4096 bytes, at most 32 rules |
| `limits` | Bounded file count, per-file bytes, aggregate bytes and elapsed seconds | Each within a validated domain; `max_file_bytes` cannot exceed `max_total_bytes` |

A policy with no rule at all is refused: an empty policy would report a clean
scan while checking nothing. Protected values are sensitive, so reports refer to
rules by zero-based index and never echo the protected string or the entry name.

## Declared scope

The scan scope is `image-config-and-container-export/v1`. It has two legs and a
launch needs both.

**Image configuration.** A filesystem export alone does not cover embedded
environment variables, entrypoint or command configuration, so the resolved
configuration document is scanned in its own right: once over the serialized
JSON, and again over every decoded string and object key inside it. Both passes
are needed. A protected sequence containing a newline appears in serialized JSON
as the two bytes `\n`, so a raw search of the serialized form alone would miss
it.

**Container export.** A uniquely named container is created from the resolved
configuration ID and never started, then exported and read by the non-extracting
TAR reader. The reader inspects headers and bodies separately, including
long-name metadata and link targets, and turns neither paths nor links into host
filesystem operations. Sparse formats, global PAX headers, PAX size overrides,
malformed records and dangling or duplicate extensions are refused rather than
skipped. This is a raw TAR scope, not a general archive decoder and not a
reconstructed filesystem certificate.

### Declared volumes refuse

[`docker container export` omits the contents of
volumes](https://docs.docker.com/reference/cli/docker/container/export/). An
image that declares a `VOLUME` therefore has content the export would not carry,
and reporting a filesystem scan over it would name a scope the scan did not
cover. Such an image refuses before anything is created.

Omitted, `null` and empty `Config.Volumes` all mean no declared volumes and are
each accepted; anything else refuses. The shape check is "an object", because
daemons trim empty configuration fields differently across versions.

## What runs, and what refuses first

Arguments and the policy are validated before Docker is invoked at all. A policy
without an image, more than one transport, an unpinned reference, and an
unreadable, malformed or oversized policy each cost zero Docker commands.

The pinned reference is then resolved with `docker image inspect`, and the run
continues only for a full lowercase `sha256:` configuration ID, a Linux image
and a configuration object. Docker and its daemon are trusted infrastructure: no
flag, policy field or environment value selects the Docker executable or
endpoint. An operator who points `DOCKER_HOST` elsewhere has scanned an image on
that host, not this one.

The container is created with `--pull never --network none --ipc none`, a
read-only root, all capabilities dropped, no new privileges, an unprivileged
user and an entrypoint that does not exist inside the image. It is inspected for
the same image ID, a created and non-running state and no mounts before it is
exported.

Removal is attempted by the reserved name on every exit path, including a
creation whose outcome is unknown, under a separate ten-second allowance. A
nonzero removal status is not read as already clean: Docker returns nonzero both
for a container that never existed and for one it could not remove, and the exit
status cannot tell them apart. Uncertainty refuses.

## The report and the launch decision

A completed preflight emits `sharpebench.image-preflight.v1`, carrying the image
ID, the policy digest, the configuration `RawScanReport`, an optional filesystem
`TarScanReport` and `cleanup_verified`. A preflight that could not produce a
scan emits `sharpebench.image-preflight-failure.v1` instead, with a composed,
redacted message and the stage it failed at.

A launch is authorized only when the configuration scan and the filesystem scan
**both** completed with no matches and the removal was verified. A
configuration-only negative is not sufficient, and neither is any partial
report. On a refusal the CLI emits no board, launches no entrant and exits
unsuccessfully.

Only the validated configuration ID is launched, never the mutable reference the
operator typed.

Read errors, truncation, growth, duplicate entries, expired scans, empty scope
and exceeded match storage each produce an explicit `incomplete_reason` and
cannot produce a passing report. The ordered inventory digest identifies the
named streams the engine was handed; it is withheld on incomplete enumeration
and is not an independently authenticated deployment identity.

## Checkpoint identity

A scanned run frames its checkpoint identity as
`("sharpebench.scanned-image-invocation.v1", sandbox_label, image_id,
policy_sha, scope)` and composes with the [rate-card
binding](cli.md#frozen-token-rates). Unscanned runs keep their legacy identity
unchanged. No export timestamp is bound, so a resume under the same policy is
still the same experiment. A checkpoint bound to a different scanned invocation
is refused before any sweep call, and its bytes are left alone. A resumable
invocation identity cannot silently change the scan policy.

## Capture limits, stated honestly

| Accepted output | Cap |
|---|---|
| One inspect document | 2 MiB |
| One create or remove response | 1024 bytes |
| The container export | The policy's `max_total_bytes` |
| Diagnostics | 64 KiB |

Inspect output is read under its cap before it is allocated or parsed, exit
status and final sizes are both checked, failed captures are killed and reaped,
and captures are rewound explicitly before parsing. Diagnostics are sanitized to
printable ASCII, bounded, and shown only on the console. They never enter a
report, because a report can be published and Docker's diagnostics can quote
image-controlled metadata that the policy exists to keep out of the open.

Two limits are weaker than they look, and the implementation says so rather than
rounding up:

- **The spool size is polled, so it is an accepted-output bound and not a disk
  quota.** A child can overshoot by whatever it writes between two polls; the
  capture is ended once the overshoot is seen.
- **The wall-clock deadline ends the client this process spawned.** It cannot
  interrupt a blocked OS read, it does not reach Docker CLI descendants, and it
  does not stop daemon-side work that continues after the client is killed.
  That is why removal is attempted on every exit path and why an unverified
  removal refuses.

The scan engine itself does not enumerate artifacts, and its clock checks cannot
interrupt an arbitrary blocking reader.

## What a negative result is not

A completed negative report says that the named streams inside
`image-config-and-container-export/v1` did not contain the policy's protected
bytes, under the limits the policy declared. It is not:

- evidence that an agent has not memorized held-out data;
- a contamination-free certificate for the entrant;
- proof against a Docker or kernel escape, which is the separate and equally
  bounded subject of [the arena's sandbox
  evidence](arena.md#what-the-acceptance-evidence-covers); or
- an identity proof for a remote process. Host-command and remote-HTTP
  execution do not establish deployed artifact identity from an
  operator-supplied path, and a local file scan must not be presented as
  verification of a remote entrant.

## Test evidence

Sixteen preflight unit tests cover the refusal, cleanup and capture paths
against an injected transport. Eight CLI tests cover the shipped binary's
argument surface, and one board regression proves that a successful scanned run
emits an array with preflight metadata on the entrant row only. Nine
integration tests, a deadline unit test and thirteen isolated mutations cover
the TAR reader. The live leg,
`artifact_preflight::tests::live_docker_image_preflight`, runs by exact name in
the live-container CI job against a digest-pinned Alpine fixture.

That is evidence for the named paths on one runner and one benign fixture. It
is not a general containment or contamination result.
