# The arena (forward league)

SharpeBench's tagline is "proves it forward", and every primitive that claim
needs already exists in the workspace: pre-registration commitments and the
epoch-locked registry (`sharpebench-attest`), sealed held-out datasets, HMAC and
Ed25519 result chains, and signed board publication (`sharpebench-leaderboard`).
The arena (`sharpebench-arena`) is the driver that walks those primitives
through an actual forward season.

## The lifecycle

An arena is a plain directory: `state.json` plus one subdirectory per window
under `windows/`. Every state transition is a file write, so the whole league is
inspectable with `cat` and survives any crash between steps.

Each evaluation window moves through four states:

```
open -> committed -> scoring -> published
```

1. **`arena init <dir>`** creates the arena.
2. **`arena open <dir> <window> <commit_deadline> <reveal_epoch>`** opens a
   window. Nothing is sealed yet, but the `ScoreConfig` the window will be
   scored under is recorded now, so the rules are fixed before any entry
   exists. Pass `--config <score_config.json>` to override the default.
3. **`arena commit <dir> <window> <commitment.json>`** registers an entrant's
   commitment (the JSON that `sharpebench commit` prints). Late commitments,
   at or after the deadline epoch, are refused; so are duplicates. These are
   the attest registry's own semantics, wrapped rather than re-derived.
4. **`arena advance <dir> <epoch>`** advances the clock. See below.
5. **`arena score <dir> <window> <dataset> <entries.json>`** runs after the
   data-reveal epoch. Each entry reveals its pre-image (artifact digest plus
   salt) alongside its scored submission; a reveal that does not match its
   registered commitment, or that never committed at all, is **refused and
   recorded**, and the rest of the field is ranked by `sharpebench-core`'s
   luck-robust `rank` under the config recorded at open time. The dataset
   bytes are hashed into the window record.
6. **`arena publish <dir> <window> <key>`** signs the board and writes
   `board.json` (the document of record) and `board.md` (human-readable) into
   the window directory.
7. **`arena verify <dir> [--pubkey <hex>]`** re-checks every published board
   and the cross-window chain from the documents alone. With `--pubkey` the
   host's advertised key is pinned; without it each board is checked under its
   embedded key (consistency, not identity; see the attestation chapter).

All subcommands honor the global `--json` flag and the `env:NAME` /
`file:PATH` key convention.

## Time: integer epochs, no wall clock

The attestation kernel is deliberately clock-free: time is an explicit integer
epoch, which is what makes every refusal reproducible and testable. The arena
keeps that property. Epochs map to wall time **only** at the operator boundary:
somebody (you, cron, CI) decides what epoch "now" is and calls
`arena advance <dir> <epoch>`. Epochs are monotonic; moving backwards is
refused. An hourly cron job that computes `epoch = unix_time / 3600` and calls
`advance` is a perfectly good scheduler. Nothing inside the crate ever reads a
clock.

## Cross-window chaining

Each published window is an Ed25519 public chain: a signed window header
followed by one signed link per ranked entry, verifiable with only the
published verifying key. Windows do not stand alone: the header of window N+1
carries the **final signature of window N's board**, so the entire arena
history is one verifiable chain.

The existing `PublicChain` API is genesis-anchored per document and cannot
express cross-document chaining without modification, so the chaining is
implemented at the arena layer, by embedding the prior board's final signature
in the first signed payload of the next board. The effect is the same: if
window N's board is altered in place its own chain breaks; if it is replaced
wholesale with a re-signed forgery, its final signature changes and window
N+1's recorded anchor exposes it. `arena verify` checks both.

The header also binds the window's rules (`ScoreConfig`), the revealed
dataset's SHA-256, and the list of refused entries, so none of those can be
quietly rewritten after publication either.

## Sandboxed entrants

Untrusted agent code is launched under Docker with every hardening flag the
runtime offers:

```
docker run --name sharpebench-agent-... --init --pull never \
  --network none --ipc none --read-only \
  --cap-drop ALL --security-opt no-new-privileges=true \
  --user 65532:65532 \
  --memory 1g --memory-swap 1g --cpus 1 \
  --pids-limit 128 --ulimit nofile=256:256 \
  --tmpfs /tmp:rw,noexec,nosuid,nodev,size=64m,mode=1777 \
  --tmpfs /run:rw,noexec,nosuid,nodev,size=16m,mode=1777 \
  --log-driver none -i <image>
```

`--pull never` means a missing image is a refusal rather than an implicit
pull, and the image is pinned by digest. Startup and execution carry explicit
timeouts. The container speaks the same stdin/stdout observation/decision
protocol as any external agent (`sharpebench-sim`'s process transport is
wrapped, not reimplemented), so an arena entrant is just an image whose
entrypoint reads one observation per line and writes one decision per line.

Entrant containers are named and retained only long enough to read Docker's
post-exit `State.OOMKilled` fact. The harness then removes them explicitly on
the normal finish path and in `Drop`; a memory-budget kill becomes the typed,
non-retryable `ResourceLimitExceeded` agent fault. A harness itself killed with
SIGKILL between spawn and cleanup can still leave a stopped container, whose
deterministic `sharpebench-agent-*` name makes it discoverable. Short-lived
readiness probes continue to use `--rm`.

Be clear about the boundary: **container isolation is the security boundary.**
The flags above remove network and IPC access, drop every capability, refuse
privilege escalation, run as a non-root user on a read-only root with
`noexec` scratch space, and bound memory, CPU, processes and file descriptors.
The arena adds no hardening beyond what the container runtime provides. When
Docker is absent the sandbox helper returns an explicit error, never a silent
unsandboxed fallback. An `allow_unsandboxed` opt-in exists for local
development against your own agent; it defaults to false and additionally
requires an explicit host command. Host execution of untrusted third-party
code remains unsupported.

### What the acceptance evidence covers

The Docker-enabled CI job runs the ignored production-boundary suite against an
Alpine fixture pinned by repository digest. It proves that, on that runner and
fixture:

- the entrant executes as uid 65532 with no effective capabilities and
  `NoNewPrivs` set;
- the root is read-only, `/etc` rejects writes, and the writable scratch mounts
  reject execution;
- the network namespace exposes only loopback;
- public internet, cloud metadata, the wider link-local range, all three
  RFC1918 ranges, and a live host-loopback listener classify as immediate
  policy denials rather than timeouts or missing-client false passes;
- the production spawn reaches a live container rather than treating a failed
  start as a deliberate hold; and
- an actual 32 MiB cgroup overrun produces `OOMKilled=true`, after which the
  container is removed.

The elapsed-time classification is load-bearing: a bare non-zero connection
status would also pass when the runner's network is merely broken or the image
lacks the probe client. The host-loopback test likewise holds a real listener
open and proves it is reachable from the host before asking the container to
fail.

This is acceptance evidence for one daemon configuration and one benign fixture,
not a proof against a Docker or kernel escape. No hostile third-party entrant has
been operated as a tenant, and multi-tenant hosting remains outside this crate.
The development machine has no running Docker daemon, so these live legs are CI
evidence rather than local evidence.

## What the arena does NOT provide

Honestly, quite a lot; deliberately so:

- **No hosting or HTTP intake.** There is no server. Entrants deliver
  commitment files and reveal files out of band (a PR, an upload, an email);
  the operator feeds them to the CLI.
- **No wall-clock scheduler.** The arena never advances itself. Drive it with
  cron, CI, or by hand; the `advance` call is the entire integration surface.
- **No dataset feed.** Producing the frozen forward dataset (and optionally
  sealing it with `sharpebench-attest`'s `seal_dataset` until reveal time) is
  the host's job.
- **No identity layer.** An agent id is a string. Binding it to a real entity
  is out of band, as is publishing the host's verifying key somewhere
  tamper-resistant so `--pubkey` pinning means something.

A complete forward league is therefore: a cron job that advances the epoch, a
repository that collects commitments before each deadline, one `score` and one
`publish` run per window, and a published verifying key. Everything
cryptographic is in the documents; everything operational is a cron line.
