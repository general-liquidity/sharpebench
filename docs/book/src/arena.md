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
   or `sharpebench commit ... --fault-plan <plan.json>`, which prints the same
   commitment, so that it binds the plan (see [faulted windows](#faulted-windows));
   without `--fault-plan` both print the same plan-less commitment.
4. **`arena advance <dir> <epoch>`** advances the clock. See below.
5. **`arena score <dir> <window> <dataset> <entries.json> [--replay-only]
   [--allow-supplied-returns]`** runs after the data-reveal epoch. Each entry
   reveals its pre-image (artifact digest plus salt) and a strict trajectory
   capture of its decisions over the revealed dataset. A reveal that does not
   match its registered commitment, or that never committed at all, is
   **refused and recorded**. So is a capture that does not bind the committed
   artifact, the revealed dataset and the window's execution matrix, or whose
   decisions the committed entrant does not repeat when the arena runs it
   again (see [returns intake](#returns-intake)). The arena replays each
   accepted capture into the returns it ranks, with `sharpebench-core`'s
   luck-robust `rank` under the config recorded at open time. It records how
   each row's returns were obtained and whether the board certifies its rows.
   The dataset bytes are hashed into the window record.
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

## Returns intake

Entries arrive after the data-reveal epoch, so returns computed then can use
the revealed data. No statistic separates such returns from skill: Gençay's
planted look-ahead oracle clears deflation with DSR 1.00 ("What survives
honest evaluation?", arXiv 2608.27734, p. 7), and a next-bar oracle over a
revealed window clears every gate SharpeBench applies (see
[the self-audit case](#the-self-audit-case)). The arena therefore ranks
returns it derives itself, and signs a board as certifying only after it has
run the committed entrant again.

### What an entry reveals

```json
{"agent_id": "alpha", "capture": {"agent_id": "sandbox:...", "contract": {}, "runs": []},
 "artifact_digest": "<64 lowercase hex>", "salt": "..."}
```

`capture` is the trajectory `sharpebench capture ... --data <revealed dataset>`
writes. It is accepted only when all of the following hold:

- **It names the committed artifact.** For `sandbox:<repository>@sha256:<digest>`
  (a `capture --image` trajectory) the artifact is the image digest, the 64 hex
  characters after `@sha256:`, and the reference must be one the sandbox would
  launch, so an option-like repository such as `--privileged` is refused. For
  `buy-and-hold` or `momentum`, reference agents compiled into the scorer, the
  artifact is a digest of the agent's name and of the runner binary the
  capture's contract records; `arena reference-artifact <name> <runner_sha256>`
  prints it. One commitment therefore admits one reference agent, and an
  entrant cannot pick the better one after the reveal. The artifact must equal
  the committed `artifact_digest`. A `cmd:` or `http:` capture names no
  artifact, because a command line or an address does not identify the bytes
  behind it, and is refused.
- **It names the revealed dataset.** Its contract's `dataset_sha256` must equal
  the identity of the revealed dataset as the simulator parses it, which the
  window records as `replay_dataset_sha256` beside the byte hash
  `dataset_hash`.
- **It runs the window's execution matrix.** The market windows are the two
  that `capture --data` runs over a revealed dataset of `n` bars, `[w, m)` and
  `[m, n)` with `w = clamp(n / 10, 10, 30)` and `m = (w + n) / 2`; the seeds are
  `0..k` for the frozen config's `execution_seeds_per_window` `k`. No entrant
  chooses which part of the revealed data it is scored on. `sharpebench
  capture` runs eight seeds, so a window that accepts CLI captures is opened
  with `execution_seeds_per_window: 8`.
- **It was made by the window's scorer and verifies strictly.** Its runner must
  be the window's `scorer_artifact_sha256`, and
  `sharpebench_harness::verify_trajectory_strict` must accept it (cost model,
  engine, one aligned decision per bar).

The ranked submission is the capture's replay under the entry's `agent_id`.
Anything else is refused and recorded against that agent: a capture naming
another artifact, dataset, matrix or runner; an entry carrying both a capture
and returns, or neither; and an entry whose `agent_id` differs from its
supplied submission's. An entry that reveals a capture must set `agent_id`; one
that names no agent is refused and recorded as `(unnamed entry <index>)`, and
the rest of the field is scored. A dataset the simulator cannot parse, while
any entry carries a capture, fails the whole call and records nothing.

**One commitment opens once.** The arena judges each entry on its own first, so
an entry that does not open its agent's commitment is refused alone and the
agent's honest reveal is still ranked. Once a salt is revealed, anyone who sees
it can attach it to a second entry, so the registry counts reveals rather than
trusting that only the entrant holds one: the first entry that both opens the
commitment and passes every other check spends it, and every later reveal of
that commitment is refused and recorded. The refusal falls on the copy, never
on the entry it copies. Matching the pre-image alone does not spend the
commitment; an entry refused further down leaves it open for the one that is
not.

A capture is also refused when it declares `in_sample_trials`. That field is
folded into the deflation bar, and no commitment binds it, so a stranger who
read the public reveal could otherwise author the entrant's deflation
footprint. A capture is ranked only on what re-execution derives.

### What replay proves, and what re-execution adds

Replay establishes that the ranked returns follow from the recorded decisions
on the revealed data, under the committed artifact identity. It does **not**
establish that running the committed artifact produced those decisions.
Anyone holding the revealed data can write a capture of hindsight decisions
that names the committed image and replays exactly.

`arena score` therefore re-executes by default. For every capture it runs
`sharpebench_harness::verify_trajectory_reexecuted`: the committed entrant runs
again on the revealed data, window and seed, one fresh instance per run, and
the first score-bearing decision it does not repeat refuses the entry. A
reference agent re-executes in process. An image re-executes through the
hardened launch of [sandboxed entrants](#sandboxed-entrants): `--pull never`,
`--network none` and a fresh container per run. An image that is not present
locally is refused for that entry. A failed start, a transport or protocol
fault, a memory-budget breach or a failed cleanup refuses the entry as well,
with the failure as the reason. Without a running Docker daemon, a call that
would re-execute an image fails before anything is recorded. A row that passes
is `re-executed`.

`arena score --replay-only` skips re-execution. Its rows are `replayed` and its
board is signed noncertifying (see [certifying boards](#certifying-boards)).
In the library, `Arena::reveal_and_score` replays only, and
`Arena::reveal_and_score_with` re-executes when it is given a launcher.

A re-executed entrant sees only the point-in-time observations the harness
gives it, so hindsight would have to be inside the artifact, fixed before the
commit deadline. That rests on the operator's custody of the target data until
the deadline, which nothing in the files proves. To check one capture outside
scoring, run `sharpebench verify-trajectory <capture.json> --data <revealed
dataset> --reexecute --image <repository@sha256:...>`.

**Scope.** The arena can re-execute only what it can run again: the reference
agents, and images that decide deterministically without network access. The
sandbox runs an image with `--network none`, and re-execution requires every
score-bearing decision to repeat. A model-backed entrant therefore has no
certifying route today. A `cmd:` or `http:` capture is refused, an image
capture cannot reach a model from inside the sandbox, and supplied returns are
noncertifying, so its forward rows can only be `replayed` or `supplied`, on a
noncertifying board.

### Certifying boards

A board certifies its rows only when supplied returns were not accepted and
every ranked row is `re-executed`. The arena computes this from the rows when it
scores; no option sets it. The window records it as `certifying`, the signed
header carries it, and `arena score --json` reports it. A `replayed` or
`supplied` row makes the board noncertifying, and `board.md` then opens with a
notice that names the reason. A reader treats a board as certifying only when
its header says `"certifying": true`; a header without the field, as signed
before the field existed, certifies nothing. A window that ranks no row meets
the rule vacuously unless it accepted supplied returns.

### Provenance on the board

Each ranked row's `returns_provenance`, one of `supplied`, `replayed` or
`re-executed`, is recorded in the window file and published on the row's
signed link beside the kernel's fields. It is rank-neutral: it changes no
score and no ordering. `board.md` lists it per agent, and `arena score --json`
returns it as a map from agent id. A row without it has the bytes of the plain
`CompositeScore`.

### Supplied returns are noncertifying

`--allow-supplied-returns` also ranks an entry that reveals `submission`, a
set of returns, in place of a capture. The returns are ranked as given, with
provenance `supplied`. The window then records `supplied_returns_accepted:
true` and `certifying: false`, the signed header carries both, `board.md`
opens with the noncertifying notice, and `arena score --json` reports them.
Without the flag such an entry is refused and recorded. The flag exists for
evidence the simulator cannot replay, such as broker returns from a forward
paper-trading arm, and for [faulted windows](#faulted-windows).

### The self-audit case

`sharpebench audit` runs this as its tenth attack, `forward-hindsight-oracle`.
It first ranks a next-bar oracle's returns directly and requires them to be
rank-eligible (DSR 1.000 on its fixture), so the case never credits the
statistics. It passes only when the oracle reaches no certifying board. Its
supplied returns are refused. A capture of its decisions naming the committed
image is refused under re-execution, the default intake, while that board,
which ranks the committed image's own re-executed capture, certifies. The
replay-only intake ranks the oracle's capture as `replayed` and signs its board
noncertifying. An in-process momentum agent stands in for the image's
container. The case needs the simulator and the arena, so only the CLI runs
it; the WASM, npm and MCP `self_audit` surfaces report the nine kernel cases.

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
dataset's SHA-256, the parsed dataset identity captures were replayed against,
whether supplied returns were accepted, and the list of refused entries, so
none of those can be quietly rewritten after publication either.

### The header and the window file

The signed header is the document of record; `window.json` beside it is not
signed. `arena verify` therefore reads each published window's file and
requires the header to record the same identity, field by field:
`window_id`, `schema_version`, `commit_deadline`, `data_reveal_epoch`,
`score_config` (compared by the digest recomputed on each side, so a config
edited under its old digest is caught), `score_config_sha256`,
`scorer_artifact_sha256`, `sealed_eval_salt_sha256`, `fault_plan_sha256`,
`dataset_hash`, `replay_dataset_sha256`, `supplied_returns_accepted` and
`certifying`. An optional field present on one side and absent on the other is
a disagreement like two different values, so a window file that drops or
invents a fault plan fails, and so does one that changes, adds or drops a
certification mark. Refusals and scores are outcomes rather than
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
- No capture path applies a fault plan (`sharpebench capture` takes no
  `--fault-plan`), so a capture cannot be replayed as a faulted window's
  experiment. On a faulted window a capture is refused and recorded, nothing
  is re-executed, and the window can be scored only from supplied returns
  under `--allow-supplied-returns`, which marks it noncertifying (see
  [returns intake](#supplied-returns-are-noncertifying)).
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
- **No certifying row for an entrant the arena cannot run again.** A
  `replayed` row proves its returns follow from its recorded decisions, not
  that the committed artifact made them without seeing the revealed data, so
  its board is noncertifying. Re-execution covers the reference agents and
  deterministic images without network access; a model-backed entrant has no
  certifying route (see [returns intake](#what-replay-proves-and-what-re-execution-adds)).

A complete forward league is therefore: a cron job that advances the epoch, a
repository that collects commitments before each deadline, one `score` and one
`publish` run per window, and a published verifying key. Everything
cryptographic is in the documents; everything operational is a cron line.
