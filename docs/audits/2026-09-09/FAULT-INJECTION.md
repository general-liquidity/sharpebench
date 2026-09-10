# Seeded fault injection at the entrant boundary

The Hyper-Tau reconciliation accepted six fault-injection mechanisms, each with a
decision attached: rows 27, 29, 30, 31, 32 and 33 of
[PORT-RECONCILIATION.md](PORT-RECONCILIATION.md). This records, per row, what
was built and how it follows that decision, the byte-identity evidence for the
no-plan path, and the mutation checks.

## Where it attaches

An external entrant sees exactly one surface: each decision step the harness
sends a `MarketObservation` and accepts one `Decision`
(`crates/sharpebench-protocol/src/lib.rs`). The engine builds the observation
from the canonical book and executes the accepted decision
(`crates/sharpebench-sim/src/engine.rs`, `run_backtest`). The injector is a
wrapper at that seam, `FaultInjectingAgent`, in the same position as the
existing `UsageObservedAgent` (`crates/sharpebench-harness/src/accounting.rs`):
it implements `Agent` and `TransportDiagnostics`, passes the entrant's own
transport health through unchanged, and alters only what the entrant is shown
and which of its submissions is accepted. The book is never touched, so a fault
is something the entrant must handle, never a corruption of the returns the
scorer reads. An entrant whose decisions do not depend on the perturbed fields
produces the byte-identical run with and without a plan; the tests pin this for
each mode.

New code:

- `crates/sharpebench-harness/src/fault_plan.rs`: the frozen plan, digest,
  cohort draw, invocation binding, per-fault denominators and the evidence
  types.
- `crates/sharpebench-harness/src/fault_modes.rs` (compiled as
  `fault_plan::modes`): per-trial state, the three armable modes, the wrapper
  and `run_faulted_backtest_observed`.
- `crates/sharpebench-harness/tests/fault_injection.rs`: checkpoint and ledger
  behaviour.

Wiring elsewhere, kept minimal:

- `crates/sharpebench-harness/src/lib.rs`: one line, `pub mod fault_plan;`.
- `crates/sharpebench-harness/src/failure.rs`: `AttemptRecord.injected_faults`
  (`#[serde(default, skip_serializing_if = "Option::is_none")]`), and the body
  of `run_with_observed_retries` now lives in `run_with_faulted_retries`, which
  carries each attempt's evidence onto its record. `run_with_observed_retries`
  keeps its signature and delegates with no evidence.
- `crates/sharpebench-harness/src/checkpoint.rs`: likewise,
  `run_resumable_sweep_faulted` carries the body and
  `run_resumable_sweep_observed` keeps its signature and delegates.
- `crates/sharpebench-sim/src/external.rs`: no change was needed. The wrapper
  composes over any `Agent + TransportDiagnostics`, so `ExternalAgent` and
  `HttpAgent` are wrapped as they are.

No existing public signature changed, so every current caller, the CLI
included, compiles and behaves as before.

## Row 32, frozen plan and mutable trial state (take)

`FaultPlan` is validated once through `TryFrom<FaultPlanWire>`
(`deny_unknown_fields` on every wire type), has private fields and no mutating
method. Its digest is SHA-256 over the validated, fixed-order serialization,
independent of input JSON formatting, with the declared relaxations stored
sorted and deduplicated. Everything that varies while a cell runs (step
counter, the entrant's stated targets, the previous projection, active lag or
limit, which faults fired, the events) lives in `TrialFaultState`, built from
the plan for one cell and holding only copies of what the plan armed, so it has
no path back into the plan.

The row asks for the digest to enter window identity "alongside
`score_config_sha256`". The concrete identity this work owns is the checkpoint
contract: `bind_invocation(invocation_sha256, plan)` returns the digest
unchanged without a plan and a domain-separated SHA-256 of
(invocation, plan digest) with one. A resume under a changed plan therefore
fails the existing `matches_bound` comparison and is refused without the
checkpoint being overwritten. Window identity in `arena/` and the CLI flag that
calls `bind_invocation` belong to crates other agents own under this goal, and
are left for them; `run_resumable_sweep_faulted` documents that the plan must
already be bound.

Tests, as the row specifies:

- same digest and seed give byte-identical traces:
  `every_mode_fires_reproducibly_from_the_plan` compares the `Run`, the
  `RunTrajectory`, every presentation the entrant saw and the deterministic
  evidence across two runs, for all three modes;
- mutating any field changes the digest:
  `every_plan_field_is_bound_into_the_digest` (seed, id, group added and
  changed, rate, each mode's bound, fault order);
- state after `reset()` equals fresh: `reset_trial_state_equals_fresh_state`,
  after a full window per mode;
- a trial never changes the plan: `a_trial_never_feeds_back_into_the_plan`;
- a changed plan refuses to resume: `a_changed_fault_plan_refuses_to_resume`
  (refused with `InvalidData`, checkpoint bytes unchanged, the unchanged plan
  then resumes and recovers).

## Row 33, digest-derived cohort draw (adapt)

The row takes the draw "if and when Bench cohorts faults". Fault assignment is
that cohort. `cohort_draw` is SHA-256 over length-prefixed fields: a domain
string, the scope label (`group` or `independent`), the group name or fault id,
the plan digest, the plan seed and the cell (`window_start`, `window_end`,
`seed`). The high 64 bits are scaled onto `0..1_000_000`. Rates are integer
parts per million so the digest never depends on float printing. Faults sharing
a group occupy consecutive, disjoint sub-ranges of the one group draw in plan
order, so they are mutually exclusive without a second draw; validation refuses
a group whose rates sum past one. The plan has a single seed, which satisfies
the archive's single-seed rule by construction. Parameters within a mode's
bound (arm step, lag length, sign span, rejected presentations) are drawn the
same way under a `parameter:<name>` label, so a cell's whole fault schedule is
reproducible from the published plan alone.

Denominators are explicit, as the row requires: `FaultPlan::denominators(cells)`
reports `cells` and `assigned` per fault, and `denominators_with_evidence` adds
`fired`, the distinct cells whose recorded evidence under this plan's digest
shows the fault firing. Assigned and fired differ in practice: a rate limit on
an entrant that never trades after its armed step, or a lag with no write after
it, is assigned and never fires.

Tests: `the_cohort_draw_is_a_pure_function_of_the_plan_digest` (a re-parsed
twin assigns 1000 cells identically; the same faults under a different digest
assign differently); `the_draw_encoding_is_pinned` (two draw values, reproduced
independently by a Python `hashlib` rendering of the documented encoding);
`grouped_faults_are_mutually_exclusive_and_rates_hold_roughly`.

## Row 29, `limit_before_sort` (adapt, conditionally)

The condition is that a paged read exists on the Bench data surface. It does
not: `rg -n -i "paginat|cursor" crates/` finds only the simulator's time cursor
(`crates/sharpebench-sim/src/env.rs`), and the observation schema has no
paging field. Honouring "do not add pagination for the fault's sake", no paged
read was added and the fault is not implemented. The mode is recorded in the
plan schema as `limit_before_sort` so a plan naming it gets a typed refusal,
`FaultPlanError::ConditionUnmet`, quoting the row, rather than being silently
accepted or dropped. The cursor-binding discipline is deferred with it.

Tests: `limit_before_sort_is_refused_while_no_paged_read_exists`, and a
tripwire, `the_observation_contract_still_has_no_paged_read`, which reads
`crates/sharpebench-protocol/schema/observation.schema.json` and fails the day
a paging field appears, naming row 29 as now due.

## Row 30, amount-sign normalization (adapt)

A seeded response perturbation on a deep copy of the observation: from a seeded
arm step, for a seeded span of one to `max_span_steps` steps, every nonzero
`portfolio[].shares` is presented with the opposite sign. Zero has no sign and
is left alone. The canonical position is unchanged: the engine keeps its book,
and `amount_sign_inverts_the_projection_only` shows the faulted run is
byte-identical to the unfaulted one for an entrant that does not read the book.

Grading follows the row: whether the entrant's decision is consistent with its
own stated reading. The protocol's stated reading of a position is the target
weight the entrant last set for it, so each inverted symbol is graded
`ConsistentWithOwnStatement` (the new target keeps the sign of its last stated
target), `FollowedPresentation` (it flipped to the inverted sign it was shown),
`Revised` (any other change), `NoOrder` or `NoPriorStatement`. Free-text
`reasoning` is not parsed; the grade is deterministic. It is a process signal,
not a verdict: an entrant whose own policy happens to flip sign on an inverted
step is graded `FollowedPresentation` even if it never read the book, which the
tests observed with a date-driven fixture and which is why the grade stays out
of scoring.

Tests: `amount_sign_grades_the_response_to_the_inverted_report` (each grade on a
hand-built book, and that the span ends), `every_mode_fires_reproducibly_from_the_plan`,
`no_mode_fires_without_a_plan`.

## Row 31, seeded rate limit (adapt)

The seeded-deadline construction, with the deadline on a monotonic clock that
needs no wall time: presentations of the same step. From a seeded arm step, the
first order-bearing decision is rejected, and the harness re-presents the
identical observation until a seeded one to `max_rejected_presentations`
rejected presentations have passed; the decision answering the first
presentation after the deadline is the one executed at that step. A hold is not
a venue call and is never limited. The engine still receives exactly one
decision per step, so an entrant that restates its decision loses nothing and
its return is byte-identical to the unfaulted run: the response is graded as a
process property, not folded into the return, as the row requires. The event
records `rejected_presentations`, `resubmissions_under_limit` (hammering),
`waits_under_limit` (backing off) and `resubmitted_after_deadline`.

Durations are separated as the row requires: the wrapper times every rejected
presentation on the host clock and records the total as
`InjectedFaults.rate_limited`, an `AttemptDuration`, beside the attempt's own
duration, so waiting on the limit is not read as thinking. When a rate card is
set, usage is observed inside the injector, so self-reported spend on rejected
decisions still reaches the rank-neutral monetary ledger, while `Run.cost`, a
scoring input, only ever sees accepted decisions. The limit is bounded at 64
re-presentations by plan validation, so one step can never become an unbounded
number of entrant calls.

Tests: `rate_limit_rejects_until_the_seeded_deadline` (identical
re-presentations, the next step canonical, same return),
`rate_limit_distinguishes_waiting_from_hammering`,
`rate_limit_fires_on_the_first_order_bearing_call_only`.

## Row 27, projection lag with convergence verification (adapt)

Taken: the write-authoritative, read-stale separation. After the first write at
or after a seeded arm step (a change in held quantities between consecutive
presentations), `cash` and `portfolio` are presented at their pre-write values
for a seeded one to `max_lag_steps` steps, while market data stays current and
the order has executed in the book. Taken: convergence observed, not timed. The
lag's end is not recorded when its counter runs out; `ProjectionConverged` is
recorded on the first presentation whose projection is observed to equal the
canonical one. A window that ends first yields `ProjectionUnconverged`, and a
presentation still perturbed by another mode does not count as convergence.
Rejected, as the row directs: the alternate-read-surface machinery.

"Declare the relaxed consistency in the contract first" is enforced
structurally: a plan must list in `declared_relaxations` exactly the
relaxations its faults use (`read_your_writes` here), or it is refused, and
`FaultPlan::entrant_declaration()` renders the text an operator publishes to
entrants with the plan. The protocol crate's own schema prose is owned by
another agent under this goal and is unchanged; adding the relaxation there is
the follow-up that makes the declaration part of the published wire contract.

Grading: for each symbol whose write the stale read hid, `Reaffirmed` (the
entrant restated its last target, the idempotent response), `Escalated` (it
pushed the target further in the direction of the hidden write, the
resubmission failure the row names), `Revised`, `NoOrder` or `NoPriorStatement`.
Rank-neutral like the others.

Tests: `projection_lag_is_stale_on_reads_and_authoritative_on_writes` (stale
steps show the pre-write holdings and current market data, the return is
unchanged, convergence lands on the first canonical read),
`a_lag_the_window_outlasts_is_recorded_unconverged`,
`projection_lag_grades_escalation_against_restatement`,
`projection_lag_waits_for_a_write`.

## Ledger evidence and rank neutrality

Each attempt run under a plan carries `InjectedFaults` on its `AttemptRecord`:
the plan digest, the cell, the assigned fault ids, the events with their grades
and the rate-limited duration. No mode ever produces a `FailureKind` or touches
`TransportHealth`, so a reader separates the two by construction: a failure
kind is genuine, evidence is injected. `injected_faults_are_recorded_on_the_attempt_ledger`
runs a faulted checkpoint sweep with one genuine transport failure and checks
that the failed record has a kind and no evidence while every completed record
has evidence under the plan's digest, then reads the per-fault denominators
back from the persisted ledger. No grade is an input to a return, score, rank
or pass^k pool; no row asked otherwise.

## No plan: byte identity

With no plan, `run_faulted_backtest_observed` is a direct call to
`run_external_backtest_observed`, `AttemptRecord.injected_faults` is `None` and
is not serialized, and `bind_invocation` returns its input unchanged.

- `an_unfaulted_sweep_writes_no_fault_evidence` pins the serialized record to
  its prior bytes and checks an unfaulted checkpoint never mentions faults.
- `no_mode_fires_without_a_plan` checks the no-plan helper returns exactly the
  run, and shows the entrant exactly the observations, of the existing path.
- The CLI built from `origin/main` (`7fe5a06`) and from this branch were run on
  the same commands and their outputs compared byte for byte: `run --json`,
  `run`, `audit --json`, `stress --json`, `run --cmd <agent> --json`, and
  `run --cmd <agent> --json --entrant-sha256 <digest> --checkpoint <file>`
  with the checkpoint file compared too. All identical. Two values were
  masked, and only these: host-clock `nanos` and `duration_ns_total`, which
  differ between two runs of one binary, and the checkpoint's
  `runner_artifact_sha256`, the digest of the running binary itself, which
  must differ between two binaries. Two runs of the baseline binary were
  identical under the same masking, and the unmasked diff between baseline
  and candidate checkpoints was exactly the one `runner_artifact_sha256` line.
- The golden suites (`crates/sharpebench-core/tests/golden_scores.rs`,
  `crates/sharpebench-sim/tests/golden_input.rs`,
  `crates/sharpebench-stats/tests/special_function_bits.rs`,
  `crates/sharpebench-wasm/tests/native_parity.rs`) pass unchanged in the full
  workspace run. Nothing under `paper/evidence/`, `examples/` or `arena/`
  changed, and no producer was touched.

## Mutation checks

Each invariant was broken in place on the committed tree (`7810dde`), one at a
time, the named test run, and the file restored from `git show HEAD:<path>` and
confirmed byte-identical with `cmp` before the next mutation.

| Invariant | Mutation | Killed by |
|---|---|---|
| Row 32: the plan is bound into the resume identity | `bind_invocation` returns the invocation unchanged under a plan | `a_changed_fault_plan_refuses_to_resume` (the changed plan resumed) |
| Row 33: the draw is a function of the plan digest | the digest dropped from the draw encoding | `the_cohort_draw_is_a_pure_function_of_the_plan_digest` |
| No plan: byte identity | `skip_serializing_if` removed from `AttemptRecord.injected_faults` | `an_unfaulted_sweep_writes_no_fault_evidence` (`"injected_faults":null` appeared) |
| Row 27: convergence observed, not timed | `ProjectionConverged` recorded when the lag counter runs out | `projection_lag_is_stale_on_reads_and_authoritative_on_writes`, `projection_lag_grades_escalation_against_restatement` |
| Ledger: injected faults are recorded | evidence dropped from completed records in `run_with_faulted_retries` | `injected_faults_are_recorded_on_the_attempt_ledger` |
| Row 31: the limit is not folded into the return | a rejected presentation executes a hold | `rate_limit_rejects_until_the_seeded_deadline` (return changed), `rate_limit_distinguishes_waiting_from_hammering` |

## Open follow-ups

- Done, see "CLI and protocol follow-up" below: the CLI exposes a fault plan
  (`run --fault-plan <json>`), bound with `bind_invocation` and driven through
  the faulted sweep.
- Done, see "Arena window identity follow-up" below: window identity
  (`arena/windows/*/window.json`) carries the plan digest alongside
  `score_config_sha256` for a faulted window, per row 32.
- Done, see below: the protocol text states each declarable relaxation, per
  row 27.
- Done, see "Incomplete sweeps follow-up" below: an incomplete faulted sweep
  carries its fault report.
- Row 29 becomes due when a paged read exists; the tripwire test says when.

## CLI and protocol follow-up

Built on `a136d14` in four commits: the harness driver (`7c019d6`), the CLI
(`270fd71`), the protocol text (`bd4a86e`) and the book (`f6cdc7d`). The
sections above describe the library as merged and are unchanged.

**`run --fault-plan <plan.json>`** (`crates/sharpebench-cli/src/main.rs`).
`load_fault_plan` runs before any dataset, preflight or launch work, next to
the rate card: it requires an external transport, reads the file once capped at
`MAX_FAULT_PLAN_BYTES + 1` and validates it with `FaultPlan::from_json`. Every
refusal (no transport, no path, unreadable file, malformed JSON, unknown field,
bounds, a declaration that differs from the armed relaxations, the unarmable
`limit_before_sort`) exits 2 with nothing launched. Each transport's attempt
closure (`--http`, `--image`, `--cmd`) now calls
`fault_plan::modes::run_faulted_backtest_observed`, which is the unchanged
observed call with no plan and wraps the entrant in `FaultInjectingAgent` with
one; the sandbox path applies its OOM verdict to the faulted observation's
result as before. `checkpoint_contract` folds the plan into
`invocation_sha256` with `bind_invocation` after the rate card and before the
backoff schedule. A checkpointed sweep runs `run_resumable_sweep_with_backoff`
(the former body of `run_resumable_sweep_faulted`, which now delegates to it
with the immediate schedule), so fault records land on each persisted attempt
record; an unpersisted sweep runs the new `run_agent_resilient_faulted`, which
returns its attempt ledger because no checkpoint holds it. The entrant row
gains a rank-neutral `fault_injection` object built from that ledger (read back
from the checkpoint when there is one): plan digest, declared relaxations,
`entrant_declaration()`, `denominators_with_evidence` over the swept cells and
every attempt's `InjectedFaults`. Human output prints the declaration before
the sweep and the denominators after it. As built, the incomplete-sweep error
carried no fault report and its evidence was only in the checkpoint when one
was used; closed since, see "Incomplete sweeps follow-up" below.

**Row 27 protocol text.** The protocol crate documentation
(`crates/sharpebench-protocol/src/lib.rs`, "Consistency relaxations a fault
plan may declare") states every `ContractRelaxation` by its wire name, what a
faulted observation may violate under it, the bound, and that the book is never
touched. The observation schema states `read_your_writes` on `cash` and
`portfolio` and `position_sign_convention` on `PositionState.shares`; the
decision schema states `submission_acceptance`. Only `description` strings
changed, so the wire shape and `schema_drift.rs` are unchanged, and the text
avoids the markers the row 29 tripwire reads. The new harness test
`every_declarable_relaxation_is_stated_in_the_published_contract` fails if a
relaxation is missing from the crate docs, or an armable one from the schema
text; its `match` is exhaustive, so a new relaxation cannot be added silently.

**Tests** (`crates/sharpebench-cli/tests/fault_backoff_reexecution_cli.rs`,
hermetic loopback HTTP entrants):
`a_fault_plan_is_injected_at_the_entrant_boundary_and_reported_rank_neutral`
(the plan fires in every cell, the entrant is re-presented observations, and
the board with operational metadata removed equals the unfaulted board),
`a_changed_fault_plan_refuses_to_resume_its_checkpoint` (the checkpoint holds
`injected_faults`; the reformatted plan resumes with no new entrant calls and
identical output; a changed plan and no plan are refused with the checkpoint
bytes unchanged and no entrant call),
`a_malformed_or_unusable_fault_plan_refuses_before_launch` (seven refusals,
exit 2, no spawn and no unsandboxed warning).

**Byte identity without the flag.** The CLI built from `origin/main`
(`a136d14`) and from this branch were run on the same commands in one
directory, each baseline command twice: `run --json`, `run`,
`run --data <csv> --json`, `audit --json`, `stress --json`,
`run --http <fixture> --data <csv>` with and without `--json`, the same with
`--entrant-sha256 <digest> --checkpoint <file>`, `run --http <failing fixture>
--json` with and without a checkpoint (the incomplete-sweep path),
`run --cmd <stdio agent> --data <csv> --json` with and without a checkpoint,
and `capture momentum`; stdout, stderr, exit code and every written file were
compared. All identical after masking only host-clock `nanos`,
`duration_ns_total` and "observed host duration" and the runner's own
`runner_artifact_sha256`, and two baseline runs differed in exactly the same
places. `verify-trajectory` over each binary's own capture, JSON and text, was
identical unmasked. `--help` changes by the three new flag lines.

**Mutation checks**, broken in place on the committed tree, the named test run,
the file restored from `git show HEAD:<path>` and confirmed with `cmp`:

| Invariant | Mutation | Killed by |
|---|---|---|
| A changed plan refuses to resume | `checkpoint_contract` binds `None` instead of the plan | `a_changed_fault_plan_refuses_to_resume_its_checkpoint` (the changed plan resumed) |
| A malformed plan refuses before launch | `load_fault_plan` returns `Ok(from_json(..).ok())` | `a_malformed_or_unusable_fault_plan_refuses_before_launch` |
| The plan reaches the entrant | the `--http` attempt passes `None` as the plan | `a_fault_plan_is_injected_at_the_entrant_boundary_and_reported_rank_neutral`, `a_changed_fault_plan_refuses_to_resume_its_checkpoint` |
| No plan: no field is added | the row always carries `fault_injection` (null) | `a_fault_plan_is_injected_at_the_entrant_boundary_and_reported_rank_neutral`, and the binary comparison (4 outputs differ) |
| Row 27 is stated | `declares submission_acceptance` removed from the decision schema | `every_declarable_relaxation_is_stated_in_the_published_contract` (`schema_drift.rs` still passes, as it should for a description change) |

The backoff and re-execution flags built in the same change are recorded in
[CONTRACT-PORTS.md](CONTRACT-PORTS.md).

## Arena window identity follow-up

Built on `fa9525c` in `3196ecc`, then merged with `origin/main` at `23276af`
(PR #67, which touched only the `sandbox` re-export lines of the same
`lib.rs`; the merge was clean). The sections above are unchanged.

**Design.** The arena does not run entrants: `arena score` ranks submissions
produced elsewhere. The plan therefore enters identity where the config does,
at open, and is checked wherever the window binds its scored submissions.
`WindowState`, `WindowHeader` and `RevealedEntry` gain `fault_plan_sha256`, and
`WindowSupersession` gains `replacement_fault_plan_sha256` beside
`replacement_score_config_sha256`, each `#[serde(default,
skip_serializing_if = "Option::is_none")]`. The arena crate stores only the
digest and checks its shape; `arena open ... --fault-plan <plan.json>`
(`crates/sharpebench-cli/src/arena_cmd.rs`, `fault_plan_digest`) reads the
file once capped at `MAX_FAULT_PLAN_BYTES + 1`, validates it with
`FaultPlan::from_json` and passes `FaultPlan::digest` to the new
`Arena::open_window_with_fault_plan`; `open_window_with_provenance` delegates
with `None`. A faulted window is written with schema 3
(`FAULTED_WINDOW_SCHEMA_VERSION`) and an unfaulted one keeps schema 2. The
optional field alone would let a scorer that predates it load a faulted
window as unfaulted and drop the digest on its next save; the version makes
that scorer refuse the window. The arena crate's test that compiles
`arena_cmd.rs` needs a dev-dependency on `sharpebench-harness`; harness does
not depend on arena and publishes before it.

**Checks that refuse a plan mismatch**, each the same kind of refusal the
config digest gets at that point:

| Where | Refusal |
|---|---|
| `Arena::load`, every active window (so every `arena` subcommand but `init`, `verify` and the two supersession commands) | schema 2 with a digest, schema 3 without one, or a digest that is not 64 lowercase hex |
| `Arena::open_window_with_fault_plan` | a malformed digest; the CLI refuses a plan `run --fault-plan` would refuse before the arena is touched |
| `Arena::reveal_and_score` | any entry whose declared `fault_plan_sha256` differs from the window's, absent versus present included; the whole call is an `Err` and nothing is written, as the window stays `committed` |
| `Arena::link_supersession_replacement` | records the replacement's plan digest with its config digest |
| `Arena::load`, each linked supersession | the recorded replacement plan digest differs from the replacement window's, absent versus present included |
| `Arena::publish` | the signed header carries the digest, so a published faulted board cannot be read as unfaulted; `board.md` names it |

`verify_arena` does not compare the header's config digest with the window
file today and does not compare the plan digest either.
`supersede_empty_window` binds the superseded window's whole bytes through
`historical_window_sha256`, which already covers its plan digest.

**Byte identity without a plan.**

- `the_committed_arena_round_trips_byte_identically` deserializes and
  re-serializes the committed `arena/windows/window-002` and `window-003`
  (the schema 2 records) to their exact bytes, and loads a copy of the whole
  committed `arena/` (supersession ledger, replacement link, active window)
  and saves it through `advance` at its own epoch: `state.json` and all three
  `window.json` files are unchanged.
- `a_plan_less_window_entry_and_header_carry_no_fault_field`: an unfaulted
  window, entry, signed header and `board.md` contain no fault key.
- The CLI built from `origin/main` (`23276af`) and from this branch ran the
  same session in two directories: `arena init`, `open` (text, `--json`,
  with `--sealed-eval-salt-sha256`), `commit` for two entrants, `arena commit`,
  `supersede-empty`, `link-supersession`, `advance`, `score` (one entry
  refused for a wrong salt, and a `--json` score of a window with no
  commitments), `publish`, `verify` (text and `--json`), `advance` over a copy
  of the committed `arena/` and a `--json` `open` in another copy. All 19
  commands had identical exit codes, stdout and stderr, and all 22 written
  files, `board.json` and `board.md` included, were identical unmasked. The
  copy of `arena/` advanced at its own epoch was identical to the committed
  one.
- `git diff origin/main -- arena/ paper/evidence/ examples/` is empty apart
  from the provenance manifest rebind, and no golden or arena test fixture
  changed.

**Tests** (`crates/sharpebench-arena/tests/fault_plan_identity.rs`, and
`open_records_the_digest_of_a_validated_fault_plan` in
`crates/sharpebench-arena/tests/cli_arena_cmd.rs`):
`a_faulted_window_binds_its_plan_through_to_the_signed_header`,
`opening_refuses_a_malformed_plan_digest`,
`loading_refuses_a_plan_digest_that_disagrees_with_the_schema` (added,
removed, malformed),
`scoring_refuses_an_entry_run_under_another_plan_and_records_nothing`
(different, absent on a faulted window, present on an unfaulted one; the
window file bytes are unchanged and it reloads `committed`),
`a_supersession_records_the_replacement_plan_and_refuses_a_mismatch`
(recorded and omitted; a different, dropped or invented ledger digest), and
the CLI test (a reformatted plan records the same digest, schema 3 and 2 as
expected, and no path, a missing file, malformed JSON and an unknown field
exit 1 with no window created).

**Mutation checks**, broken in place on the committed tree (`720d5a8`), the
named tests run, the file restored from `git show HEAD:<path>` and confirmed
with `cmp` before the next:

| Invariant | Mutation | Killed by |
|---|---|---|
| Schema agrees with the plan on load | load accepts schema 3 or 2 whatever the digest | `loading_refuses_a_plan_digest_that_disagrees_with_the_schema` |
| A loaded digest is well formed | the load shape check removed | `loading_refuses_a_plan_digest_that_disagrees_with_the_schema` |
| An opened digest is well formed | the open shape check removed | `opening_refuses_a_malformed_plan_digest` |
| A faulted window is schema 3 | open always writes schema 2 | `a_faulted_window_binds_its_plan_through_to_the_signed_header`, `open_records_the_digest_of_a_validated_fault_plan` |
| Score refuses absent versus present | the entry check compares only when both sides have a digest | `scoring_refuses_an_entry_run_under_another_plan_and_records_nothing` |
| The replacement's plan is checked | the supersession plan check removed | `a_supersession_records_the_replacement_plan_and_refuses_a_mismatch` |
| The replacement's plan is recorded | link records `None` | `a_supersession_records_the_replacement_plan_and_refuses_a_mismatch` |
| The header binds the plan | publish writes `None` | `a_faulted_window_binds_its_plan_through_to_the_signed_header` |
| No plan: window bytes | `skip_serializing_if` removed on `WindowState` | `the_committed_arena_round_trips_byte_identically`, `a_plan_less_window_entry_and_header_carry_no_fault_field` |
| No plan: header bytes | `skip_serializing_if` removed on `WindowHeader` | `a_plan_less_window_entry_and_header_carry_no_fault_field` |
| No plan: ledger bytes | `skip_serializing_if` removed on `WindowSupersession` | `the_committed_arena_round_trips_byte_identically`, `a_supersession_records_the_replacement_plan_and_refuses_a_mismatch` |
| No plan: entry bytes | `skip_serializing_if` removed on `RevealedEntry` | `a_plan_less_window_entry_and_header_carry_no_fault_field` |
| The CLI refuses an unusable plan | an invalid plan becomes no plan | `open_records_the_digest_of_a_validated_fault_plan` |
| The CLI records the plan | `arena open` passes `None` | `open_records_the_digest_of_a_validated_fault_plan` |

What this does not do: the `Commitment` an entrant registers before the
deadline does not bind a plan (the attest crate is unchanged), so the plan is
fixed by the window at open and checked against each entry's declaration at
score, not committed to by the entrant.

## Incomplete sweeps follow-up

Built on `0dcc4b8` in `2442d7f`. The sections above are unchanged apart from
the closing sentence of "CLI and protocol follow-up".

**Change.** `report_transport_failures` (`crates/sharpebench-cli/src/main.rs`)
takes the sweep's report as `Option<&serde_json::Value>`, and each transport
(`--http`, `--image`, `--cmd`) now builds `fault_injection_report` from its
attempt ledger before the completeness check rather than after it: the ledger
`run_agent_resilient_faulted` returns, or the one `checkpoint_fault_ledger`
reads back from the checkpoint. The `incomplete_external_sweep` JSON gains
`fault_injection` beside `attempt_accounting`, and human output prints the
denominators after the attempt accounting. It is the same function over the
same ledger as a completed row's report, so it carries the plan digest, the
declaration and the evidence of every attempt that ran, failed attempts
included. Every cell of an incomplete sweep was attempted (both drivers run
every cell; an exhausted cell is recorded, not skipped), so the denominators
stay over the swept cells: an exhausted cell counts in `cells` and `assigned`,
and in `fired` only if its evidence shows the fault. The error still emits no
score, board or rank. Without a plan the report is `None`, nothing is inserted
and nothing is printed.

**Test.** `an_incomplete_faulted_sweep_keeps_its_fault_report`
(`crates/sharpebench-cli/tests/fault_backoff_reexecution_cli.rs`), against a
loopback entrant that serves the first 40 requests and then breaks: the sweep
completes some cells and exhausts the rest (exit 1); the refusal's digest,
declaration and relaxations equal those of a completed faulted run under the
same plan; the limit fired in at least the completed cells and in fewer than
all 16; every piece of evidence is under the plan's digest; human output
prints the digest and the denominators; without a plan neither mode mentions
fault injection; and under `--checkpoint` the refusal's evidence equals the
`injected_faults` persisted in the checkpoint.

**Byte identity without the new flags.** The CLI built from `origin/main`
(`0dcc4b8`, from a clean `git archive`) and from this branch ran the same 35
commands, each in its own directory, the baseline twice: `--help`, `run` and
`run --json`, `run --data <csv> --json`, `run --http <fixture>` with and
without `--json` and with `--entrant-sha256 --checkpoint`, the incomplete-sweep
path (`run --http <unframed fixture>`) in both modes with and without a
checkpoint and with `--retry-backoff 1,2`, `run --cmd <reference-agent>` with
and without a checkpoint, `run --image some/agent:latest`, `capture` with no
argument, one argument, an unknown agent, and `momentum` and `buy-and-hold`
(text and `--json`), and `verify-trajectory` with no argument, strict (text and
JSON), `--allow-unbound-trajectory`, `--reexecute` against the reference agent
(text and JSON), `--http` (a passing and a broken endpoint) and `--cmd
<reference-agent>`, and the three existing refusals. Exit codes, stdout,
stderr and all 10 written files (five checkpoints, three captured
trajectories, a renamed copy and the data file) were identical after masking
only host-clock `nanos`, `duration_ns_total` and "observed host duration", and
each binary's own `runner_artifact_sha256`; the two baseline runs differed
only in the host-clock fields. Unmasked, each trajectory differed from the
baseline's in exactly its one `runner_artifact_sha256` line. Every command
but `--help` matched; `--help` differs by two lines, the new `capture
<out.json> --cmd|--http|--image` line and `|--image <ref>` in the
`--reexecute` line. The usage messages of `capture` and `verify-trajectory`
with too few arguments are unchanged on purpose, because both are output
without the new flags. The five incomplete-sweep commands are the
ones this change touches, and all five matched.

**Mutation checks** are recorded with the capture and re-execution follow-up
in [CONTRACT-PORTS.md](CONTRACT-PORTS.md#external-capture-and-image-re-execution-follow-up),
in one table, since they ran together on the committed tree.
