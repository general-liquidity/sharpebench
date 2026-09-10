# Ambiguous-write replay safety in the lifecycle checks

The one mechanism the Hyper-Tau reconciliation accepted as trading-native and
genuinely absent from SharpeBench: a submission whose acknowledgment was never
observed, retried without a stable client-side key. This records the source, the
gap search, the design decision and the evidence.

## Source

`PORT-RECONCILIATION.md` on `origin/docs/hyper-tau-reconciliation-2026-09-10`,
rows 25 and 26.

- Row 25, post-commit timeout with idempotency-key replay:
  `hyper/client_api/defects.py:240-251` (`PostCommitTimeoutDefect`, a 504 after
  the canonical commit, `idempotency_header` defaulting to `Idempotency-Key`),
  enforced at `hyper/client_api/runtime.py:2191-2238` where the record key is
  `(defect.id, operation_id, path, idempotency_key)` and a reused key with a
  different body returns `409 idempotency_key_reused`.
- Row 26, the violation record: `hyper/client_api/runtime.py:2254-2260` appends
  to `retry_safety_violations` with reason
  `ambiguous_write_retried_without_idempotency`.

Row 26 is the part that belongs in the scoring kernel. Row 25's fault injection
is a simulator change and is out of scope here.

## The gap, as searched

`rg -n -i -e idempot -e client_order_id crates/` finds `idempotency_key` only on
forecast contract revisions (`crates/sharpebench-core/src/forecast.rs:240`,
uniqueness at `:547`). There is no `client_order_id` and no submission dedup.
The nearest order-side control was warn severity:
`crates/sharpebench-core/src/process.rs` `DuplicateTransition { order, phase }`,
whose own comment calls it "duplicated bookkeeping, not a bypassed control", and
which in any case only fires when the same order id repeats a stage. An agent
that retries with a fresh order id after a lost acknowledgment produced no
finding at all.

In trading that silence is expensive. If the venue committed the first write,
the retry is a second live order and the position doubled. Nothing downstream
can separate that from an agent legitimately sending two orders, because before
this change the protocol gave an agent no way to say "this is the same order
again".

## Design

Three additions to `crates/sharpebench-core/src/process.rs`.

**`ClientOrderKey`.** An opaque, equality-only newtype, the agent's own name for
one intent. Distinct from `OrderId`, which is per accepted order: two
submissions of one intent carry two order ids and one key.

**`LifecycleStep::client_key: Option<ClientOrderKey>`.** Additive and optional,
`#[serde(default, skip_serializing_if = "Option::is_none")]`, so a record written
before the field existed still parses and a step without a key still serializes
byte-identically. The field sits on the step rather than inside
`Phase::Submission` for a compatibility reason worth stating: `Phase` variants
are struct variants, so adding a field to `Submission` would break every
brace-construction of it across the workspace, including test files owned by
other work in flight. `LifecycleStep::new` keeps its two-argument shape and a
`with_client_key` builder attaches the key. It is read only on submissions.

**`Phase::AcknowledgmentUnobserved { order }`.** The typed record that a write's
outcome is unknown.

No schema version was bumped. Nothing versions `Trace`: it has no
`schema_version` field, and the workspace's versioned envelopes
(`TrajectoryContract::SCHEMA_VERSION`, `WINDOW_SCHEMA_VERSION`,
`Checkpoint::SCHEMA_VERSION`) describe trajectory identity, evaluation windows
and harness checkpoints rather than the trace event vocabulary. The repository's
own convention for an additive optional field is the one followed here:
`TrajectoryContract::runner_artifact_sha256` and the `windows` / `seeds` fields
are all `#[serde(default)]` additions carrying no bump. A new enum variant under
an existing internal tag is likewise additive, since no record written before it
existed can carry the tag.

### Reading ambiguity without a clock

`sharpebench-core` is pure: no I/O, no system clock, no ambient randomness. A
timeout is therefore not observable to it, and "the acknowledgment has not
arrived yet" is not a fact the kernel can derive. Silence of any duration is not
evidence.

So ambiguity is a fact the trace states, not one the kernel infers. The harness
that saw the transport failure records `Phase::AcknowledgmentUnobserved` for the
order. The check reads exactly that. The only other positional quantity used is
the index of a submission within the recorded event sequence, which picks the
newest outstanding write for a subject when more than one is open; it is an
index into an ordered list, not a time.

### The rule

On a submission that opens a new order for subject `S`, the check looks at the
newest still-outstanding submission for `S`, meaning one that has not advanced
past `Submission` and has not already answered an earlier retry.

- Nothing outstanding: clean. The submission stands alone.
- Outstanding and marked `AcknowledgmentUnobserved`: the two attempts must share
  a key. `Some(k) == Some(k)` is clean, one intent sent twice. Anything else,
  an absent key on the retry, a key the ambiguous write never carried, or an
  absent key on the ambiguous write, is
  `AmbiguousWriteRetriedWithoutKey`, block severity. An absent key matches
  nothing, including another absent key.
- Outstanding with no recorded outcome at all: `AmbiguityUnavailable`, warn
  severity. The trace does not establish whether the first write was ambiguous,
  so the check reports the missing evidence rather than guessing a verdict in
  either direction.

An acknowledgment, fill or reconciliation clears ambiguity: a response that
arrives late still arrives, so the outcome is known and a later submission is a
fresh intent needing no key. Each outstanding write answers at most one retry,
so a chain of blind retries reports once per retry rather than once per pair.

Two distinct legitimate submissions do not trip anything, because the first one
progressed and is no longer outstanding. Two subjects never interact, since the
scan is subject-scoped. A keyed retry produces no violation and is not counted
as a second intent: the prior write stays at `Submission` and is marked answered,
so it raises no dangling finding of its own.

## The gating decision

`AmbiguousWriteRetriedWithoutKey` is **block severity and gates the process leg**
like every other block variant. `is_block` returns true for it, so it zeroes
`process_score_with_ordering` and withholds a lifecycle certification through
`certification.rs`.

The reconciliation's row 26 suggested shipping it warn-severity until
calibrated. That caution is about false positives, and the design removes the
false-positive surface rather than discounting the finding:

- The check cannot fire on a trace that does not carry
  `Phase::AcknowledgmentUnobserved`, a variant that did not exist until now, so
  no recorded trace anywhere can trip it.
- The unknown case is separated out as its own warn-severity variant instead of
  being folded into the block, which is where a miscalibrated version of this
  check would have done its damage.

What remains is a real bypass of a real control. The existing severity model
says block is for "the ones that let an agent move capital without the control
that was supposed to gate it" and warn is for "the control ran, the record is
incomplete". A blind retry of a possibly-committed write moves capital: it is
the same class as `UnauthorizedSubmission`, not the same class as
`DuplicateTransition`. Warn severity would cost 0.1 for having possibly doubled
a position, which understates it by the width of the position.

`AmbiguityUnavailable` is warn, and that is the refusal-over-coercion half. It
is typed unavailability, not a pass: the report names it and the process score
docks it, so an unevidenced resubmission never scores as a clean trace, but the
kernel does not assert a verdict the trace cannot support.

## Rank neutrality

No published board moves. Zero traces in the repository carry the new phase or
the new field, so no existing trace can produce either new finding, and the
serialization of a step without a key is byte-identical to what it was.
`git status` after the change shows two modified files and nothing under
`crates/sharpebench-core/golden/**`, `examples/**` or `paper/evidence/**`.

## Evidence

Red before green. The ten new tests were written against the data model with the
detection rule absent: 9 of 32 `process::` tests failed, and the block case
failed with

```
expected an ambiguous-write violation, got OutOfOrderTransition {
  order: OrderId("o1"), attempted: AcknowledgmentUnobserved, current: Some(Submission) }
```

After implementing the rule, 32 of 32 pass.

Tests, all in `crates/sharpebench-core/src/process.rs`:

| Test | Asserts |
|---|---|
| `keyed_retry_after_an_unobserved_acknowledgment_is_clean` | shared key, no violations |
| `blind_retry_after_an_unobserved_acknowledgment_blocks` | one block, score 0.0, payload names both orders |
| `a_retry_under_a_different_key_blocks` | a borrowed name is not the same intent |
| `an_observed_acknowledgment_resolves_the_ambiguity` | a late response clears it |
| `two_distinct_submissions_are_not_a_retry` | two completed cycles score 1.0 |
| `distinct_subjects_never_interact` | subject scoping |
| `unobservable_ambiguity_is_reported_rather_than_passed` | warn, not block, and score below 1.0 |
| `one_finding_per_retry_however_many_priors_are_outstanding` | no quadratic reporting |
| `a_step_without_a_key_serializes_exactly_as_before` | frozen-trace regression, encode and decode |
| `an_ambiguity_marker_does_not_disturb_the_state_machine` | marker is score-neutral on a clean cycle |

### Mutation check

The block condition was reverted in isolation, one line at a time, in this
worktree, with the source restored from `git show HEAD:<path>` and verified with
`cmp`. Results are in the commit's verification record.

### Commands

| Command | Exit |
|---|---|
| `cargo fmt --all --check` | 0 |
| `cargo clippy --all-targets --all-features -- -D warnings` | 0 |
| `RUSTDOCFLAGS=-Dwarnings cargo doc --workspace --exclude xtask --no-deps` | 0 |
| `cargo nextest run -p sharpebench-core -p sharpebench` | 0, 447 passed |
