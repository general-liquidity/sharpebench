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

- The CLI does not yet expose a fault plan: a `--fault-plan <json>` flag that
  loads `FaultPlan::from_json`, calls `bind_invocation` on the invocation
  digest and drives `run_resumable_sweep_faulted` is CLI work.
- Window identity (`arena/windows/*/window.json`) should carry the plan digest
  alongside `score_config_sha256` for a faulted window, per row 32.
- The protocol schema prose should state each declared relaxation, per row 27.
- Row 29 becomes due when a paged read exists; the tripwire test says when.
