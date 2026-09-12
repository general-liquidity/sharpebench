# CLI reference

The `sharpebench` binary (crate `sharpebench-cli`) is the command-line entry point.

```text
sharpebench run                       run reference agents through the sim and rank them
sharpebench score <submissions.json>  rank a JSON field of pre-computed submissions
sharpebench check <returns.csv> --trials N [--periods-per-year N]   test one return series for backtest honesty
sharpebench realism [--data <csv>]    run the stylized-facts dataset gate
sharpebench commit <agent> <window> <digest> <salt> [--fault-plan <plan.json>]   forward-attestation pre-registration
sharpebench stress                    run the adversarial stress suite (contamination-masked)
sharpebench audit                     self-audit: prove the scorer resists gaming
sharpebench sign <subs.json> <key> <out.json>         score + sign a board to a file
sharpebench verify <board.json> <key> verify a signed board's chain
sharpebench capture <agent> <out.json>                capture an agent's raw-decision trajectory
sharpebench verify-trajectory <traj.json>             replay a trajectory → recompute its score
sharpebench rescore <bundle.json>                     recompute a declared submission bundle from its frozen files
sharpebench audit-briefing <briefing.json>            audit a shared briefing for salience bias
sharpebench canary <seed>                             derive a do-not-train contamination tripwire
sharpebench sandbox-check <image@sha256:digest>       run the live Docker-boundary acceptance checks
sharpebench gateway --routes <routes.json> ...        report the host-observed model gateway configuration
sharpebench score-allocation <alloc.json>             score a weight-vector trajectory (turnover)
sharpebench greeks <spot> <strike> <t> <r> <vol> <call|put>   Black-Scholes price + Greeks + local exposure
sharpebench self-update                               update an update-enabled binary in place
```

Use `sharpebench --help` for the complete command and flag inventory. Commands
that render a human report accept the global `--json` flag for structured
output; file-producing commands already write their documented JSON artifact.

## `run`

Runs the reference agents (buy-and-hold, momentum) through the point-in-time
simulator over multiple windows × seeds with costs on, and prints the ranked
board. The teaching demo: watch deflation and pass^k in action.

The built-in momentum agent uses a 10-return-interval lookback, requiring 11
observed closes per symbol. It equal-weights symbols with a positive return over
that exact trailing window. Shorter histories, nonpositive or nonfinite trailing
prices, or nonfinite returns receive explicit zero targets, not a shorter-window signal.
Rust callers can set `Momentum { lookback: L }`; zero or overflowing lookbacks
also leave the signal unavailable. The observation's history budget is separate:
requesting a lookback beyond it does not expose additional bars. Older versions
ignored this setting and used all supplied history, so their reference-agent
results must not be presented as measurements of the repaired strategy.

Three external-agent transports are explicit rather than interchangeable:

- `--image <repository@sha256:...>` launches an already-present, digest-pinned
  image through the fail-closed Docker boundary. No daemon, mutable reference,
  absent image, failed readiness check, indeterminate OOM verdict, or failed
  cleanup becomes host execution. Add the opt-in
  `--scan-policy <policy.json>` to scan the image's configuration and container
  export for operator-declared protected content and refuse before the entrant
  is launched; see [entrant image preflight](image-preflight.md).
- `--cmd "<program>"` executes a trusted program on the host and prints an
  unsandboxed warning on every run. Its environment is cleared to a small
  platform allowlist; opt named variables in with
  `SHARPEBENCH_AGENT_ENV=NAME1,NAME2`.
- `--http <addr>` posts to an endpoint whose isolation the operator owns.

Add `--checkpoint <path>` to resume an external sweep. The checkpoint contract
binds the dataset, costs, score configuration, running CLI binary, entrant,
ordered windows, ordered seeds, and retry policy. A checkpointed `--cmd` or
`--http` run also requires `--entrant-sha256 <digest>` because a command line or
endpoint address does not identify the artifact that served it. A mismatched or
legacy checkpoint is refused rather than overwritten.

The checkpoint schema version is 4. Schema 3 predates the persisted per-round
attempt budget, so a schema-3 checkpoint carries no evidence of what an
interrupted round already spent and would read that spend as zero. Resuming one
is refused by version, naming the schema, rather than continued with a fresh
round granted on top of work the writing binary had already done.

Default resume skips all terminal cells, including exhausted runtime failures.
Add `--retry-runtime-failures` to explicitly recover every runtime-failed cell
under the same contract. Completed results, protocol violations and resource-
limit failures are never requeued. Each invocation authorizes one additional
round, with at most three recovery rounds per cell over the checkpoint's
lifetime. The per-round retry limit is unchanged. Exhausting that ceiling
refuses before executing any cell, rather than resetting the budget.

Claims, per-round attempt counts and recovery counts are persisted. Every
observed attempt and any terminal outcome are saved together before a later
attempt starts. An interrupted claim retains its consumed per-round budget;
its in-flight, unobserved work can still be absent from the ledger. Earlier
attempts stay in the accounting even after recovery succeeds. The checkpoint
is an operator-controlled record, not tamper-proof evidence, and a transport
failure label alone does not prove that infrastructure caused the failure.
Do not use recovery to select among completed outcomes. The running binary is
part of the contract, so this flag does not authorize resuming an old binary's
checkpoint under a new binary.

Exhausted runtime failures make the external sweep noncertifying: the CLI emits
expected, completed, runtime-failed, and agent-failed cell counts, then exits
without a board. Agent-caused protocol faults remain in the pass^k denominator
as failing sentinels.

External sweeps publish rank-neutral `attempt_accounting` using schema
`sharpebench.attempt-accounting.v1`. On success it is an additional field on
the external agent's JSON board row; the board remains an array and reference
rows are unchanged. An incomplete-sweep error carries the same field. Human
output prints the totals on stderr.

The summary counts completed and failed attempts, including retries, and
reports observed host duration with `host_clock`, `mixed`, or `unavailable`
provenance. Duration totals saturate at `u64::MAX`; they are not billing data.
Without a rate card, `monetary_cost.status` is `unavailable`: the legacy scalar
does not establish a monetary unit. A failed attempt is not free, and a missing
cost is not zero. These observations never enter ranking or the pass^k denominator.
Checkpoint totals cover persisted records only; a process killed before saving
can leave unrecorded work.

### Opt-in image preflight

`--scan-policy <policy.json>` applies only to `--image`. It scans the pinned
image's executable configuration and its container export for the exact bytes
the policy protects, and refuses before the entrant is launched when either leg
matches or when either leg could not complete. On a refusal no entrant runs and
no board is emitted.

The declared scope is `image-config-and-container-export/v1`. Container export
omits volume contents, so an image that declares a volume refuses rather than
being reported as scanned over a scope the scan did not cover. The accepted
output caps are bounds on what the CLI will read, and the export spool size is
polled, which makes it an accepted-output bound and **not a disk quota**.

A completed negative report says the named streams did not contain the
protected bytes. It never establishes that an agent has not memorized held-out
data: compressed, encoded, encrypted, chunked and model-internalized copies are
all outside raw-byte scope. The policy schema, the refusal order, the capture
limits and the checkpoint identity are in
[entrant image preflight](image-preflight.md).

`--runtime-allowlist <allowlist.json>` adds the other polarity: a leg of the
same preflight that refuses every entry of the container export whose path the
allowlist (`sharpebench.runtime-allowlist.v1`, a list of relative `paths`) does
not admit. It requires `--scan-policy` and `--image` and is refused without
them. It runs only after the scan found the export clean, and an image it admits
must then pass a one-observation functional probe, with its container removal
verified, before the entrant launches. The allowlist digest is folded into the
report's `policy_sha256`, so a changed allowlist is a different checkpoint
identity. An allowlist result says which paths the export holds, not what the
bytes under an admitted path are; see
[entrant image preflight](image-preflight.md) for the path rules and the probe.

### Frozen token rates

Add `--rate-card <json>` to an external `run` to quote token usage under one
operator-declared provider, model and revision. The example at
`examples/reference-agent/rate-card.example.json` contains synthetic rates,
not current provider prices:

```json
{
  "schema_version": "sharpebench.token-rate-card.v1",
  "provider": "example",
  "model": "example-model",
  "revision": "example-1",
  "input_usd_nanos_per_token": 125,
  "output_usd_nanos_per_token": 500
}
```

Rates are nonnegative integer nanodollars per token (one USD is 1,000,000,000
nanodollars). The quote is input tokens times the input rate, plus output
tokens times the output rate. Reasoning tokens are a subset of output, not an
additional charge. Arithmetic is checked integer arithmetic; exact output
amounts are decimal strings to avoid JavaScript number rounding.

The card is read once, capped at 64 KiB, rejects unknown or missing fields,
and binds its validated model/rate identity into the checkpoint invocation.
Changing its rates, model or revision refuses an existing checkpoint before
executing cells. Reformatting the same JSON does not change the identity.

The separate `attempt_accounting.monetary_cost` has status `estimated` only
when every recorded attempt has usable usage. It carries `usage_source:
"entrant_reported"`, the full card, its SHA-256 identity and `usd_nanos`.
Any missing or failed-attempt usage produces `unavailable`, with a separately
named `known_subtotal_usd_nanos` when possible. Failed attempts retain their
known tokens through retries and resume; a later success cannot erase them.
Mixed cards and overflowing arithmetic cannot produce a total.

These are estimates from the legacy decision protocol, not host-observed
provider receipts. For usage the host itself observed, see
[`gateway`](#gateway) and the
[host-observed model gateway](model-gateway.md); that record is host-observed,
which is still not verified billing. That protocol defaults omitted individual token counts to
zero, so their completeness is not independently established. Entirely absent,
dollar-only or all-zero usage is unpriced rather than assumed free. Invalid
reasoning counts also withhold the estimate. The model identity is an operator
declaration; this mechanism does not verify which model an entrant called.
Version 1 excludes caching, batch discounts, tool fees and taxes. Explicitly
zero rates can price a nonzero reported count as zero.

The old `cost`, `return_per_cost` and `dsr_per_cost` columns are unchanged.
Their legacy entrant-selected scalar can mean USD or tokens; the accounting
record labels it `entrant_selected_usd_or_tokens_not_rate_card_priced`. Do not
interpret those columns as the new USD quote. Token pricing does not change
decisions, returns, scores, eligibility or ordering.

See [The arena](arena.md#sandboxed-entrants) for the boundary and acceptance
evidence.

### Seeded fault injection

Add `--fault-plan <plan.json>` to an external `run` (`--http`, `--image` or
`--cmd`, with or without `--checkpoint`) to run the entrant under a frozen,
seeded fault plan. The injector sits at the entrant boundary: it changes what
the entrant is shown and which of its submissions is accepted, never the book
the engine executes or the returns the scorer reads.

```json
{
  "schema_version": "sharpebench.fault-plan.v1",
  "seed": 11,
  "declared_relaxations": ["read_your_writes", "submission_acceptance"],
  "faults": [
    {"id": "lag", "cohort_ppm": 500000,
     "fault": {"mode": "projection_lag", "max_lag_steps": 3}},
    {"id": "limit", "cohort_ppm": 1000000,
     "fault": {"mode": "rate_limit", "max_rejected_presentations": 4}}
  ]
}
```

The armable modes are `projection_lag`, `amount_sign` and `rate_limit`.
`limit_before_sort` is recorded but refused, because the observation contract
has no paged read. Each fault is assigned to a share of cells (`cohort_ppm`,
parts per million) by a draw over the plan digest, and every per-mode parameter
within its bound (at most 64) is drawn the same way, so the whole schedule is
reproducible from the published plan. `declared_relaxations` must list exactly
the relaxations the faults use; the text an entrant is owed for each one is in
[Submitting an agent](submitting.md#faulted-observations-under-a-declared-plan).

The plan is read once, capped at 64 KiB and validated before anything
launches. Malformed JSON, an unknown field, a bound out of range, a relaxation
declared but unused or used but undeclared, and an unarmable mode are refused
with exit code 2. Its digest is bound into the checkpoint invocation identity,
so a checkpoint written under one plan is refused under a changed plan, or
under none, without being overwritten. Reformatting the same plan does not
change the digest.

On success the external row carries a rank-neutral `fault_injection` object
beside `attempt_accounting`: the plan digest, the declared relaxations, the
declaration text, per-fault denominators (`cells`, `assigned`, and `fired`,
the distinct cells whose evidence shows the fault firing) and every attempt's
evidence with its process grades. Under `--checkpoint` the same evidence is
persisted on each attempt record of the ledger. Human output prints the
declaration before the sweep and the denominators on stderr. No grade is an
input to a return, score, rank or pass^k pool, and an entrant whose decisions
do not depend on the perturbed fields scores exactly as it does unfaulted.

A sweep that ends incomplete (a cell exhausts its retries) emits no board, but
its `incomplete_external_sweep` error carries the same `fault_injection`
object beside `attempt_accounting`, built from the same attempt ledger (read
back from the checkpoint when there is one): the plan digest, the declaration,
the denominators over the swept cells, and the evidence of every attempt that
ran, failed attempts included. `fired` counts only the cells whose evidence
shows the fault, so an exhausted cell counts in `cells` and `assigned` and
fires only if it did before failing. Human output prints the denominators
after the attempt accounting. The error is still rank-neutral: no score,
board or rank is emitted. Without the flag nothing changes: no field is added
and every output is byte-identical.

### Retry backoff

Add `--retry-backoff <ms,ms,...>` to an external `run` to wait between runtime
retries of a cell instead of retrying at once. Entry `i` is the wait in whole
milliseconds before retry `i`, and the last entry holds. A cell retries at most
twice per round, so at most two entries are accepted; each is at most 600000
(ten minutes). An empty entry, a sign, a fraction or an out-of-range value is
refused with exit code 2 before launch.

Each scheduled wait is recorded on the failed attempt it follows
(`backoff_after`), kept out of that attempt's duration, and totalled as
`attempt_accounting.attempts.backoff_ns_total`. Under `--checkpoint` the wait
is saved before the harness sleeps, retries are numbered within the cell's
round (a `--retry-runtime-failures` round starts the schedule again), and the
schedule is bound into the checkpoint invocation identity: a checkpoint is
refused under a changed schedule or under none. Agent faults never wait.
Without the flag, or with an all-zero schedule, retries are immediate, nothing
is recorded and every output is byte-identical.

### Suite evidence

`--suite-evidence` with `--json` wraps the board as `{board, suite_evidence}`.
The board under the envelope is byte-identical to the board the plain
invocation emits; the plain table always prints the evidence.

`suite_evidence.trials` is the census against the roster the run declared
before the first entrant ran, and `suite_evidence.controls` is one verdict per
declared control. `suite_evidence.control_binding` is the digest that binds
those verdicts. It carries the `run_provenance` digest over the ordered
per-control preimages, the list of verdict fields whose values entered it, and
the list of fields that entered nothing with the reason each is excluded. One
digest per control is published alongside the suite digest, so a reader can see
which row moved rather than only that one did.

The `detail` line is excluded by declaration: it is a rendering of fields that
are already bound, and binding the prose would let a wording edit break a digest
over unchanged evidence. Everything here carries `used_by_gate: false`. The
census does not move a score, the controls carry no score field, and the binding
is provenance beside a result; none of them reaches the gate, eligibility or the
rank.

## `score`

Ranks a JSON field of pre-computed submissions (see
[Submitting an agent](submitting.md)). The board shows DSR, PSR, pass^k, process,
bootstrap p, and raw return, with a footer naming how many of the submitted agents
are eligible.

`score` and [`disqualify`](#disqualify) share these host scoring controls:

| Flag | Meaning |
|---|---|
| `--periods-per-year N` | Positive finite annualization frequency |
| `--execution-seeds-per-window N` | Positive integer number of adjacent execution replicates per window |
| `--pass-mode MODE` | `all`, `any`, `at-least:N`, or `relative-to-benchmark` |
| `--benchmark-agent ID` | Field member used for the relative verdict; defaults to `buy-and-hold` |

Each supplied flag requires a value; an omitted value is a usage error in both
commands.

Each submission may carry `declared_mandate`, as described under
[declaring a mandate](methodology-pass-k.md#declaring-a-mandate-at-submission).
The declaration adds `declared_passed_k` and `declared_mandate_eligible` beside
the host verdict; it does not change host eligibility, rank order or ordinal.
Unknown declaration kinds, duplicate agent IDs and whitespace-only IDs are
refused. IDs are compared exactly, without trimming or case normalization.

`--rank-mode <id>` opts into a versioned rank mode; the only one is
`lifecycle-certified/v1`, described under
[lifecycle-certified rank mode](lifecycle-certified.md). It adds a
`certification` verdict to every row and never changes host eligibility, rank
order or ordinal. An identifier the kernel does not implement is refused with
exit code 2. Without the flag the board is unchanged.

`--diagnostics <list>` also reports opt-in Sharpe diagnostics that the gate,
eligibility and the rank do not use: `autocorrelated-psr`, `null-se-psr` and
`mppm`, comma-separated, described under
[opt-in diagnostics](methodology-deflated-sharpe.md#opt-in-diagnostics-the-gate-does-not-use).
The human table gains a separate block after the unchanged board; `--json`
prints `{"board": ..., "sharpe_diagnostics": [...]}`, where `board` is the
board-only output. An unknown identifier or a missing value exits 2 before any
output. Without the flag the output is byte-identical to a build without it.

These identity checks do not verify temporal alignment of legacy JSON `runs`.
The caller still supplies consistent window, seed and period order across the
field; use [captured trajectory contracts](evidence-contracts.md#captured-trajectories)
when the task requires their stronger identity checks.

## Analysis CSV input

`check`, `regime`, `select`, `rediscover`, `uncertainty` and `decay-prior` share
readers for unquoted comma-separated analysis tables. They refuse quoted fields,
blank observations, ragged rows, missing selected cells and nonfinite or invalid
selected numbers. Headers must contain distinct, nonempty names. A multi-column
`select` file requires every candidate column to contain complete finite data.
Missing cells are never dropped independently to make shorter vectors.

Header detection is heuristic when no column is named: numeric series inspect
the first cell, while multi-column `select` inspects the whole first row. Prefer
explicit names where `--col` is offered. Other columns may hold text when only
one numeric column is selected. These checks apply to the numerical analysis
readers; the separate `import` command is unchanged.

## `stress`

Runs the adversarial stress suite (flash-crash, whipsaw, …) with
contamination-masking so an agent can't fingerprint the scenario.

## `audit`

Runs the [benchmark self-audit](integrity.md). Exits non-zero if any claimed defense
is not demoted.

## `commit` / `sign` / `verify`

The [forward-attestation](attestation.md) surface: pre-register a strategy digest,
sign a published board, and verify a board's chain. HMAC verification requires a
shared secret whose holders can also forge. Public verification uses the
Ed25519 chain and a verifying key obtained through an independent channel.

## `capture` / `verify-trajectory`

Capture an agent's raw per-seed×window decision trajectory to JSON, then have a
separate verifier replay it through the simulator and recompute the score from
the raw decisions. New captures bind the data, costs, engine, runner, exact
ordered windows, and exact ordered seeds. Strict verification requires every
declared cell and every decision step, validates step and observation identity,
and derives replicate grouping from the contract. Missing, duplicated,
reordered, shortened, or cross-environment evidence is refused.

`--allow-unbound-trajectory` is an explicit legacy or cross-version regrade. It
does not claim that the artifact reproduces its original execution conditions.
See [Evidence contracts](evidence-contracts.md).

Replaying recorded decisions cannot tell whether the agent that made them is
deterministic. `--reexecute` adds that check: after the strict checks pass,
every captured run is executed again with a fresh agent on the same data,
window and seed, and each score-bearing decision (orders and cost; not
`reasoning` or `rationale`) is compared with the recorded one. The agent is
`--cmd "<prog>"` (host execution, with a warning), `--http <addr>`, or, with
neither, the reference agent the trajectory names (`buy-and-hold` or
`momentum`).

```bash
sharpebench verify-trajectory traj.json --data data.csv --reexecute --http 127.0.0.1:8080 --json
```

A pass prints the usual verification plus a `reexecution` object (`agent`,
`runs_reexecuted`, `decisions_compared`). The first divergence exits 1 with
`"error": "reexecution_diverged"` and the typed divergence (`run`, `step`,
`observation_id`, and the `recorded` and `reexecuted` decisions). A transport,
protocol or spawn failure during re-execution exits 1 as
`reexecution_transport_failure`, not as a divergence, because a degraded
transport says nothing about determinism. `--reexecute` refuses
`--allow-unbound-trajectory` and a trajectory whose agent is not a reference
agent unless `--cmd`, `--http` or `--image` names it; `--cmd`, `--http` and
`--image` are refused without `--reexecute`, and more than one agent flag with
`--image` is refused. Each is exit code 2.

`--reexecute --image <repository@sha256:...>` re-runs a digest-pinned image
through the hardened launch `run --image` uses: the same refusal of an absent
daemon, a mutable reference or an image that is not present locally, before
anything starts, and then a fresh named container for every captured run,
finished after its run with the post-exit resource verdict and removed. An
out-of-memory verdict is reported as `reexecution_transport_failure` with
`resource_limit_exceeded`, an indeterminate verdict or failed cleanup as
`transport_error`, and a refused launch as `spawn_error`; no further container
is started after the first failure. A divergence is `reexecution_diverged`,
as for the other agents. There is no host fallback.

`capture` also records an external entrant, over the same transports as `run`:

```bash
sharpebench capture traj.json --http 127.0.0.1:8080 --data data.csv
sharpebench capture traj.json --cmd "./my-agent --flag" --data data.csv
sharpebench capture traj.json --image registry/agent@sha256:<digest> --data data.csv
```

The first argument is the output file; a transport flag replaces the
reference agent name, and passing both, or more than one transport, exits 2.
Each run gets a fresh agent (a fresh process, connection or container), with
the same launch checks as `run` (`--cmd` prints the unsandboxed warning; `--image`
refuses what `run --image` refuses; `--scan-policy` is not accepted). The
trajectory carries the same contract as a reference capture, and its
`agent_id` names the entrant by the flag that re-runs it:

| `agent_id` | Re-execute with |
|---|---|
| `cmd:<command line>` | `--reexecute --cmd "<command line>"` |
| `http:<addr>` | `--reexecute --http <addr>` |
| `sandbox:<repository@sha256:...>` | `--reexecute --image <repository@sha256:...>` |

The CLI prints that command after a capture, and `--json` gives it as
`reexecute_with`. Nothing is launched from the file itself: re-execution still
needs the flag. The pinned image reference identifies the artifact; a command
line or an address does not, and the environment a `--cmd` entrant receives
(`SHARPEBENCH_AGENT_ENV`) is not recorded, so the operator re-runs those under
the same conditions. A spawn, transport, protocol or resource failure during a
capture exits 1 with `capture_transport_failure` and writes nothing, because a
degraded transport would otherwise put the harness's holds into the trajectory
as the entrant's decisions.

## `rescore`

`verify-trajectory` recomputes a score from one artifact the operator points it
at. `rescore` recomputes it from a **declared submission bundle**: a JSON
document that names, by content digest, every file the evaluator is allowed to
read.

```json
{
  "schema_version": "sharpebench.submission-bundle.v1",
  "agent_id": "momentum",
  "trajectory": "trajectory.json",
  "dataset": "prices.csv",
  "costs": "costs.json",
  "runner_artifact_sha256": "<the capture binary's digest>",
  "image": "registry/agent@sha256:<digest>",
  "frozen_files": [
    { "path": "prices.csv", "sha256": "..." },
    { "path": "costs.json", "sha256": "..." },
    { "path": "trajectory.json", "sha256": "..." }
  ],
  "resources": {
    "cpu_millis": 2000,
    "memory_bytes": 2147483648,
    "wall_clock_seconds": 900,
    "disclosed": { "host_kernel": "6.8.0-generic" }
  },
  "claimed": { "deflated_sharpe": 0.41 }
}
```

`frozen_files` is the whole read set. Paths are relative to the bundle
document's own directory and may not be absolute or reach upward. Nothing
outside the manifest is opened, so agent state sitting beside the bundle, a
workspace, a cache or a results file the entrant wrote about itself, is not an
input and cannot move the recomputed score. Every declared file is read once
and held to its declared digest; bytes that do not hash to it refuse with the
path, the declared digest and the digest on disk. `claimed` is published beside
the recomputation as `claim_matches_recomputation` and is never scored.

An absent `frozen_files` manifest is a refusal, not a warning. A verifier that
imports its scoring code from a separate protected copy can afford to warn when
a manifest is missing, because its scorer is out of the entrant's reach either
way. This command has no second copy, so the manifest is the only thing between
the recompute and the entrant's disk and it is required.

```bash
sharpebench rescore bundle.json --envelope field-envelope.json --json
sharpebench rescore bundle.json --reexecute --scan-policy policy.json
```

`--reexecute` runs the `run --image` image preflight (`--scan-policy` and
`--runtime-allowlist`, documented above) on the bundle's own pinned image, refuses unless every scan leg, the runtime allowlist, the
functional probe and the cleanup completed clean, and then re-executes every
captured run in the hardened, network-disabled container through
`verify_trajectory_reexecuted`. The image comes from the bundle and never from
a flag: an operator flag that could name a different image would let the
rescore verify something other than what was submitted.

What `--reexecute` offers is deterministic policy re-execution, not recorded
provider-response replay. Every report says so under `not_established`: for an
entrant whose decisions depend on a sampled model response, a divergence is not
evidence of tampering and agreement is not evidence that the recorded responses
were the ones the entrant received.

`--envelope` supplies the field's declared compute budget. A difference in
`cpu_millis`, `memory_bytes` or `wall_clock_seconds` refuses and names the
field, the declared value and the envelope's: those bound what the agent could
have computed, so two scores under different budgets answer different
questions. Everything in `resources.disclosed` is environment description that
the envelope does not constrain, and is published beside the score as
`comparability.disclosed` rather than refused. Without `--envelope` the budget
is recorded and not judged, and the report says that too.

The report carries the bundle digest, the frozen-manifest digest, every
verified file with its role, the runner artifact, the semantic dataset and
cost-model digests, the recomputed score, and two prose lists: `verified` and
`not_established`.

## `compare`

```bash
sharpebench compare --axis <entrant|invocation|score-config> --baseline <checkpoint.json> --treatment <checkpoint.json> [--json]
```

Declares that two sweep arms are comparable and on which axis, or refuses and
names the field that decided it.

Every checkpoint the resumable sweep writes binds six identities: dataset, cost
model, score configuration, runner artifact, entrant and invocation. Binding
them never said which one the experiment varies on purpose. Two arms that
differ in the entrant are a model comparison; two that differ in the dataset are
two experiments printed in one table, and the checkpoint files do not
distinguish them for a reader.

The axis is the one identity the arms are allowed to differ on. Only three are
declarable. The dataset, the cost model and the runner artifact are not, and
neither is the execution matrix: an arm that moves one of those is measuring
something else, so naming one as the axis is a usage error and nothing is
emitted. What the contract deliberately does not bind stays unbound here too: a
rotated credential never reaches `invocation_sha256`, so it neither refuses a
comparison nor invalidates a resume.

A declared comparison exits 0 and names the axis field, the digest each arm
carries on it, and every identity checked equal to get there, so a reader who
disagrees with the declared axis can see what was held fixed instead of assuming
anything was. A refusal exits 1 and names the off-axis identity, the differing
element of the execution matrix, or the arm that carries no contract. Under
`--json` the refusal is emitted as a document rather than written to stderr.

The command reads both checkpoints and writes to neither. The receipt carries
`used_by_gate: false`: it is reporting surface beside a result and no score,
gate, eligibility or rank sees it.

## `regime`

```bash
sharpebench regime returns_a.csv returns_b.csv regimes.csv [--col NAME] [--regime-col NAME] [--period-col NAME] [--json]
```

Compares two strategies' per-period returns *within* each market regime instead
of pooled. See [Regime-conditional comparison](methodology-regime.md).
`--col` selects the return column in both strategy files; `--regime-col` selects
the label column independently. Without those flags, each reader uses its first
column. Regime labels are supplied by the caller.

All three files must have the same number of complete observations; no series
is truncated. `--period-col NAME` additionally requires a nonempty, unique period
ID on every row and the identical ordered ID sequence in all three files. The
period column must differ from the selected value column. It compares trimmed
ID strings, without parsing dates, sorting rows or joining an intersection:

```sh
sharpebench regime a.csv b.csv labels.csv \
  --col return --regime-col state --period-col period --json
```

Here the strategy files have `period,return` headers and the label file has
`period,state`. Without `--period-col`, row alignment is the caller's assertion;
equal lengths do not establish common dates or temporal support. Invalid input
produces no report and a nonzero exit. A produced report exits 0; read
`pooled_hides_reversal` for its verdict.

## `lineage`

```bash
sharpebench lineage strategy-evidence.json [--json]
```

Verifies one SharpeArena generated-strategy ledger and reports its observed
trial count, candidate ancestry, cited idea sources, and best-versus-median
robustness within each host-derived strategy family. It recomputes the ledger
and family bindings and requires validation scores for every selectable
candidate. The report is diagnostic only and cannot alter eligibility, rank, or
the trial denominator. See [Candidate lineage diagnostics](candidate-lineage.md).

## `audit-briefing` / `canary` / `score-allocation` / `greeks`

Standalone analysis surfaces over the kernel: lint a shared briefing for
input-side salience bias, derive a do-not-train contamination tripwire, score a
target-allocation weight-vector trajectory (validity + L1 turnover), and price an
one long European option with its Greeks and local gamma/vega exposure flags.
Invalid inputs and undefined Greek vectors are refused. Local Greeks do not
establish payoff boundedness; see [Options pricing and payoff risk](options-risk.md).

## `gateway`

```bash
sharpebench gateway --routes <routes.json> --budget-usd-nanos <n> --max-calls <n> [--journal <journal.json>] [--json]
```

Reports and preflights the host-observed model gateway. It never calls a
provider. It resolves the route manifest, binds each alias's credential from the
named environment variable, and prints the frozen route-table identity, the
bounds a sweep would enforce, and what the money journal has already committed.
Credential values are read but never printed; the report names the variable and
marks the value redacted.

Both `--budget-usd-nanos` and `--max-calls` are required, and neither may be
zero: a paid run without a stated ceiling has no bound on what it spends before
anyone notices, and a zero ceiling would refuse every call. A missing
credential, a malformed manifest, inline key material and a journal bound to
another route table each refuse with a nonzero exit code.

The report labels its own provenance: `usage_provenance` is
`host_observed_not_verified_billing`, and `provider_transport` is
`operator_supplied_none_ships_in_this_build` because no provider transport
ships in this build. See
[the host-observed model gateway](model-gateway.md) for the protocol, the bound
table and the reservation and settlement rules.

## `select`

```bash
sharpebench select <candidates.csv...> [--alpha A] [--utility mean_return|sharpe] [--seed N] [--boot N] [--block-prob P] [--json]
```

Ranks candidate strategies on a percentile of their bootstrapped utility instead
of the point-estimate argmax, so the winner has to be good on most resampled
histories rather than on the one that happened to be observed. Pass one CSV per
candidate (first column read), or a single CSV whose columns are the candidates.

The output names both the point winner and the percentile winner, whether they
agree (disagreement is the whole reason to run this), and each candidate's
optimism gap: how much of its headline utility fails to survive resampling. The
point winner's gap is the number to report next to any headline result.

`--alpha` defaults to 0.5, the middle of the band. An alpha below 0.3 still
computes but prints a warning: the extreme lower tail of a bootstrap
distribution is decided by a handful of unlucky resamples nobody has real data
for. The warning flags a choice; it does not veto one. Deterministic given
(data, `--seed`).

The flag reports where alpha sits and nothing else. A refusal that had nothing
to do with alpha, such as too few observations or a rejected block probability,
leaves it false rather than blaming an argument that was not the problem.

## `disqualify`

```bash
sharpebench disqualify <submissions.json> [--periods-per-year N] [--execution-seeds-per-window N] [--pass-mode MODE] [--benchmark-agent ID] [--json]
```

Scores a JSON field of submissions (same format as `score`) and names every
disqualification/quality signal that fired for each agent, instead of the
single rank-eligible verdict. Pass the same field and [host controls](#score) as
`score`: explanations come from the ranked field, including its benchmark and
field-dependent deflation, rather than separately scoring each submission.
The taxonomy has eleven reasons in three groups. Five mirror the scorer's hard
eligibility gates (`FailedPassK`, `DsrBelowBar`, `ProcessViolation`,
`BootstrapInsignificant`, `MandateBreached`). Two name the unavailability of a
statistic the scorer really does gate on, so they are hard as well:
`DeflationUnavailable` and `BootstrapUnavailable`. The remaining four are
advisory and never gate: `SelectionUnavailable`, `HighSelectionGap`,
`IsRediscovery` and `OosDecay`.

`SelectionUnavailable` is advisory because the whole selection axis is. The
scorer reports `selection_gap` and never consults it in `rank_eligible`, so the
unavailability of that same diagnostic cannot demote an agent either. A
submission whose candidate set refuses therefore keeps `rank_eligible: true` and
carries the reason marked `(advisory)`. The invariant the classifier holds is
`rank_eligible == reasons.iter().all(FailReason::is_advisory)`, and
`is_advisory` lives in the core taxonomy next to the enum so the CLI cannot
drift from it.
JSON rows contain `agent_id`, host `rank_eligible` and `reasons`. These reasons
explain the host score only, including its drawdown mandate; they do not explain
the separate declared-mandate verdict. To inspect that verdict, use the
`declared_*` fields returned by `score`. Explanation generation changes neither
eligibility nor rank.

## `rediscover`

```bash
sharpebench rediscover <submitted.csv> <known.csv...> [--threshold T] [--center] [--json]
```

Screens a submitted pooled return stream against a library of known prior
strategy streams and flags near-duplicates on `|cosine|` similarity. A stream
must be all but collinear with a known one to flag (default threshold 0.97);
leveraged and inverted variants of a known stream flag too, while
correlated-but-distinct strategies do not. `--center` de-means first (Pearson);
the default compares raw direction, because for return streams the direction is
the strategy. Novelty screening only: it says nothing about skill.

## `uncertainty`

```bash
sharpebench uncertainty <returns.csv> [--reference <csv>] [--outcomes <csv>] [--confidences <csv>]... [--json]
```

Decomposes the uncertainty behind one scored case into three legs, printed side
by side and never summed:

- **aleatoric** (from `--outcomes`, 0/1 per decision): irreducible outcome
  noise; more evidence will not reduce it. High reading: stop looking.
- **epistemic** (from repeatable `--confidences` streams): reducible ignorance,
  read off disagreement between independent confidence streams plus how thin
  the evidence behind them is. High reading: keep looking.
- **distributional** (case returns vs `--reference`): unlikeness to the
  reference series, as a location or dispersion shift. High reading: the
  reference cannot vouch for this case.

The epistemic leg is a lower bound, never an upper one: unanimous or correlated
signals understate it, so a low reading is weak evidence of knowledge and only
high readings are informative. The command prints this caveat with every
result. Inputs you omit are reported as not measured, not as zero risk.

## `decay-prior`

```bash
sharpebench decay-prior --measured-ic <ic.csv> --adoption X --theta Y --delta-max Z [--curvature C] [--anomaly-ratio R] [--json]
```

Measures the edge's half-life from its IC series (regressing `ln|IC|` on time)
and sets it against the expected half-life from a crowding model,
`ln2 / (theta + delta_max * adoption^curvature)`. The expected half-life is a
model prior, reported never gating: it comes out of a crowding model, not out
of a dataset, and nothing ranks on it. All rates are per period of the supplied
IC series, and there is deliberately no default calibration; the caller owns
every rate.

A measured/expected ratio below `--anomaly-ratio` (default 0.5) flags the decay
as too fast for crowding to be the whole story, which usually points at
overfitting, a broken data pipeline, or a regime the strategy was never fit
for. The flag is a diagnostic, not a verdict.
