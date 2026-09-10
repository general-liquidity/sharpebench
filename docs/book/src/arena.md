# The arena (forward league)

SharpeBench supplies the cryptographic primitives for an operator-declared
forward protocol: pre-registration commitments and the
epoch-locked registry (`sharpebench-attest`), sealed held-out datasets, HMAC and
Ed25519 result chains, and signed board publication (`sharpebench-leaderboard`).
The arena (`sharpebench-arena`) is the driver that walks those primitives
through a season. Calling that season forward additionally assumes honest epoch
advancement, held-out-data custody, and non-observation before commitment.

## The lifecycle

An arena is a plain directory: `state.json` plus one subdirectory per window
under `windows/`. Every state transition is a file write, so the whole league is
inspectable with `cat` and survives any crash between steps.

Each evaluation window moves through four states:

```
open -> committed -> scoring -> published
```

1. **`arena init <dir>`** creates the arena.
2. **`arena open <dir> <window> <commit_deadline> <reveal_epoch>
   --scorer-artifact-sha256 <hex>`** opens a
   window. Nothing is sealed yet, but the `ScoreConfig` the window will be
   scored under is recorded now, so the rules are fixed before any entry
   exists. The required scorer digest freezes the exact scoring artifact before
   entrants commit. Pass `--config <score_config.json>` to override the default
   and `--sealed-eval-salt-sha256 <hex>` when the window uses sealed evaluation.
   Pass `--fault-plan <plan.json>` when the window's entrants run under a
   seeded fault plan (see [faulted windows](#faulted-windows)).
3. **`arena commit <dir> <window> <commitment.json>`** registers an entrant's
   commitment (the JSON that `sharpebench commit` prints). Late commitments,
   at or after the deadline epoch, are refused; so are duplicates. These are
   the attest registry's own semantics, wrapped rather than re-derived. An
   entrant to a faulted window makes its commitment with `arena commitment
   <agent_id> <window> <artifact_digest> <salt> --fault-plan <plan.json>`
   instead, so that it binds the plan (see [faulted windows](#faulted-windows));
   without `--fault-plan` it prints exactly what `sharpebench commit` prints.
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
   embedded key (consistency, not identity; see the attestation chapter). Each
   board's header is also checked against its window file (see
   [the header and the window file](#the-header-and-the-window-file)).

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

That design makes the state machine deterministic, not self-dating. The
operator can advance an epoch early or reveal data out of band, and the files
alone cannot disprove it. A credible deployment therefore needs an external
wall-time record, controlled custody of the target data, and evidence that
entrants could not observe it before their commitments were accepted.

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

### The header and the window file

The signed header is the document of record; `window.json` beside it is not
signed. `arena verify` therefore reads each published window's file and
requires the header to record the same identity, field by field:
`window_id`, `schema_version`, `commit_deadline`, `data_reveal_epoch`,
`score_config` (compared by the digest recomputed on each side, so a config
edited under its old digest is caught), `score_config_sha256`,
`scorer_artifact_sha256`, `sealed_eval_salt_sha256`, `fault_plan_sha256` and
`dataset_hash`. An optional field present on one side and absent on the other
is a disagreement like two different values, so a window file that drops or
invents a fault plan fails. Refusals and scores are outcomes rather than
identity, and the scores are the signed links themselves; they are not
compared.

A disagreement fails that window. The `--json` report lists each one under the
window's `identity_mismatches` as `{"field", "header", "window"}`, with `null`
for an absent value; the text report names the field and both values; the exit
code is 1. A published window whose file cannot be read is an error, also
exit 1. When every field agrees the report carries no `identity_mismatches`
key and is byte-identical to the report before the check existed.

## Faulted windows

A window can be scored under a frozen fault plan, the one `sharpebench run
--fault-plan <plan.json>` injects at the entrant boundary. `arena open ...
--fault-plan <plan.json>` reads the plan once, validates it exactly as `run`
does (a plan `run` would refuse is refused here, before the window exists) and
records its SHA-256 as `fault_plan_sha256` beside `score_config_sha256`. The
digest is taken over the validated plan, so a reformatted copy of the same
plan records the same digest.

The digest is part of the window's identity from then on, so a faulted and an
unfaulted run of the same window are never scored, published or superseded as
the same thing:

- A faulted window is written with schema 3; an unfaulted window keeps schema 2.
  Loading refuses a window whose digest disagrees with its schema (present on
  schema 2, absent on schema 3) or is not a lowercase SHA-256, so an edited
  record, or a scorer that predates the field, cannot treat a faulted window as
  unfaulted.
- Each entry in `entries.json` declares the plan its submission ran under as
  `fault_plan_sha256`, the `fault_injection.plan_sha256` of its `run` row. If
  any entry's declaration differs from the window's, including a declaration
  on an unfaulted window or none on a faulted one, `arena score` refuses the
  whole call and records nothing: the window stays `committed`.
- Each entrant's pre-deadline commitment binds the plan too, so an entrant
  cannot commit under one plan and be scored under another. The plan digest
  is a fifth framed field of the commitment pre-image, after `agent_id`,
  `target_window`, `artifact_digest` and `salt`, present only when the window
  has a plan; `arena commitment ... --fault-plan <plan.json>` computes it. At
  score time every entry is revealed under the window's plan, and a
  commitment made for another plan, for no plan on a faulted window, or for a
  plan on an unfaulted one does not match: it is refused and recorded like
  any failed reveal, and the rest of the field is ranked.
- `arena link-supersession` records the replacement's plan digest as
  `replacement_fault_plan_sha256`, and loading refuses a ledger whose recorded
  digest disagrees with the replacement window.
- The signed header carries `fault_plan_sha256`, and `board.md` names it.
  `arena verify` requires it to equal the window file's, absent versus
  present included.

Without `--fault-plan` none of these fields is written: an unfaulted window,
its entries, its commitments, the supersession ledger and the signed header
have the same bytes as before the field existed.

## Sandboxed entrants

Untrusted agent code is launched under Docker with every hardening flag the
runtime offers:

```
docker run --name sharpebench-agent-... --pull never \
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
post-exit state: status, `State.OOMKilled` and exit code. The harness then
removes them explicitly on the normal finish path and in `Drop`; a
memory-budget kill becomes the typed, non-retryable `ResourceLimitExceeded`
agent fault. Docker sets `State.OOMKilled` from an asynchronous containerd
event that can arrive late or not at all, so an exited entrant with exit code
137 (SIGKILL, which nothing inside the launch can send to namespace PID 1, and
which the harness sends only after the read) is also a breach; see
[OOM-VERDICT.md](../../audits/2026-09-09/OOM-VERDICT.md).
A harness itself killed with
SIGKILL between spawn and cleanup can still leave a stopped container, whose
deterministic `sharpebench-agent-*` name makes it discoverable. Short-lived
readiness probes continue to use `--rm` and `--init`. Entrant launches omit
`--init` so the image entrypoint is namespace PID 1: otherwise an allocating
child can be killed while Docker's init survives and the post-exit
`State.OOMKilled` fact remains false. The 128-process limit bounds zombie
accumulation, and forced container removal still tears down the namespace.

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
- an actual 32 MiB cgroup overrun exits 137 and is classified as a budget
  breach by the production classification, after which the container is
  removed.

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
  sealing it with `sharpebench-attest`'s [authenticated dataset seal](sealed-datasets.md) until reveal time) is
  the host's job.
- **No identity layer.** An agent id is a string. Binding it to a real entity
  is out of band, as is publishing the host's verifying key somewhere
  tamper-resistant so `--pubkey` pinning means something.

A complete forward league is therefore: a cron job that advances the epoch, a
repository that collects commitments before each deadline, one `score` and one
`publish` run per window, and a published verifying key. Everything
cryptographic is in the documents; everything operational is a cron line.
