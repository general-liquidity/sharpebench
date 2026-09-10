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

## Runtime allowlist and functional probe

`--runtime-allowlist <allowlist.json>` adds the other polarity. The scan policy
refuses content it was told to look for; the allowlist refuses every entry of
the container export whose path it was not told to expect. It is opt in and is
a leg of the preflight, not a policy of its own: given without `--scan-policy`
and `--image` it is refused, rather than silently ignored.

```bash
sharpebench run --image <repository@sha256:...> --scan-policy policy.json \
  --runtime-allowlist allowlist.json ...
```

```json
{
  "schema_version": "sharpebench.runtime-allowlist.v1",
  "paths": ["app/", "etc/passwd"]
}
```

The file is read once and capped at 64 KiB, unknown fields are refused, and it
declares 1 to 4096 paths of at most 1024 bytes each. A path is relative
printable ASCII with no empty, `.` or `..` segment, and is declared once. A path
ending in `/` admits that directory and everything below it; any other path
admits exactly that entry. A directory that is an ancestor of an admitted path
is admitted itself, because an archive lists the directories it descends
through, and admits nothing else below it.

The allowlist is applied only after the filesystem scan enumerated the whole
export and found it clean, so the listing walks an archive whose structure the
scan already validated. Every entry, directories and links included, must be
admitted. An unreadable listing, an unreadable name or an expired policy
deadline leaves the allowlist report incomplete, and an incomplete report never
admits. Refused entries are reported by count and by archive-order index, at
most 16 indices, never by name: the report can be published, and a name can be
exactly what a policy protects.

**What the daemon adds.** Every container export also holds entries Docker's
init layer puts in each container it creates, whatever the image holds. The
live CI job measured them on Docker 28.0.4 (overlay2, cgroup v2) against the
pinned Alpine fixture, by comparing the export with the image's own layers from
`docker save`:

| Entry | In the export | What the image held |
|---|---|---|
| `.dockerenv` | empty file | nothing |
| `dev/console` | empty file | nothing |
| `dev/pts/`, `dev/shm/` | directories | nothing |
| `etc/resolv.conf` | empty file | nothing |
| `etc/hostname` | empty file | a 10-byte file |
| `etc/hosts` | empty file | a 79-byte file |
| `etc/mtab` | link to `/proc/mounts` | a link to `../proc/mounts` |

The daemon's init-layer table also creates `dev/`, `etc/`, `proc/` and `sys/`
when an image lacks them; the fixture has all four, so that part is read from
the table rather than observed. These entries are admitted without
being listed, and only in that shape: an empty regular file, a directory, or
`etc/mtab` linking to `/proc/mounts`. The same path carrying bytes, of another
type or linking elsewhere is image content and still refuses by index, and
nothing below `proc/`, `sys/`, `dev/pts/` or `dev/shm/` is admitted by this
rule. `runtime_allowlist.docker_init_entries` counts the entries admitted this
way. An allowlist therefore names the image's own paths only. The image's own
bytes at `etc/hostname` and `etc/hosts` are replaced in the export, so the scan
does not read them; a started container sees the daemon's bind-mounted files at
those paths, not the image's.

**The functional probe.** An image that passes every scan leg and the
allowlist, with its snapshot container removal verified, is then run once, from
its configuration ID and under the hardened launch a
[gateway sweep](model-gateway.md) uses (`--network none` included), against one
fixed synthetic observation: an instrument named `PROBE` on `1970-01-01`, which
tells the image nothing about the evaluation it is entering. The first line it
writes must be a decision valid for that observation, within 60 seconds and
8 MiB of output, and removing its container by name must be verified. Otherwise
the probe fails with one of `launch_refused`, `probe_did_not_complete`,
`no_decision`, `invalid_decision`, `probe_output_exceeded` or
`probe_output_unreadable`, or with `cleanup_verified: false`. The probe runs only
after every scan leg authorized the image, so a refused image is still never
started.

With an allowlist, the report gains three fields: `scan_policy_sha256` (the scan
policy's own digest), `runtime_allowlist` (`allowlist_sha256`, `entries`,
`docker_init_entries`, `outside_allowlist`, `outside_indices`, `complete`) and
`functional_probe`
(`observation_sha256`, `passed`, `refusal`, `cleanup_verified`), and a launch
additionally requires the allowlist to admit every entry and the probe to pass
with its cleanup verified. `policy_sha256` becomes a digest over the scan policy
digest and the allowlist digest (`sharpebench.image-preflight-policy.v2`), so a
changed allowlist is a changed experiment for the checkpoint below. Without an
allowlist, `policy_sha256` is the scan policy digest exactly as before and the
three fields are absent.

What this does not prove: an allowlist result says which paths the export
holds. It says nothing about the bytes under an admitted path, which remain the
scan policy's business, and it does not make the image reproducible. A passing
probe says the admitted image answered one synthetic observation validly; it is
not a behavioural test of the entrant. The daemon-added entries above were
measured on one daemon version with one storage driver; a daemon that adds an
entry outside that table refuses it by index rather than admitting it, and the
live test fails, which is the signal to measure again.

**Live verification.** Both legs ran against the CI daemon with the pinned
fixture. An allowlist naming exactly the fixture's 519 own paths admitted all
524 export entries (5 as daemon entries). The fixture's own entrypoint is
`/bin/sh` reading the observation as a script, and its probe refused with
`no_decision`, cleanup verified: the probe runs the image rather than assuming
it works. The same filesystem committed with an entrypoint that answers one
observation with a hold passed the probe and authorized the launch. An
allowlist missing only `etc/alpine-release` refused with one entry outside at
index 89, the position of that entry in an independent export of the same
image, did not start the image and did not name the entry in the report.

## Checkpoint identity

A scanned run frames its checkpoint identity as
`("sharpebench.scanned-image-invocation.v1", sandbox_label, image_id,
policy_sha, scope)` and composes with the [rate-card
binding](cli.md#frozen-token-rates). `policy_sha` is the report's
`policy_sha256`, so it binds the runtime allowlist whenever one was applied.
Unscanned runs keep their legacy identity unchanged. No export timestamp is bound, so a resume under the same policy is
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
emits an array with preflight metadata on the entrant row only. Nine further
unit tests cover the runtime allowlist, the daemon's init-layer entries and the
functional probe against the same injected transport. Nine integration tests, a
deadline unit test and thirteen isolated mutations cover the TAR reader. Three
live legs run by exact name in the live-container CI job against a
digest-pinned Alpine fixture:
`artifact_preflight::tests::live_docker_image_preflight` (scan only),
`live_runtime_allowlist_admits_the_fixture_and_its_probe_passes` (the
daemon-added measurement, the allowlist and the probe) and
`live_runtime_allowlist_refuses_an_omitted_path_by_index`.

That is evidence for the named paths on one runner and one benign fixture. It
is not a general containment or contamination result.
