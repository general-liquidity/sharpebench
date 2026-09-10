# Ambiguity survives the retry chain

An independent verification of the ambiguous-write check merged in
[`AMBIGUOUS-WRITE.md`](./AMBIGUOUS-WRITE.md) found a safety bypass: a keyed
retry silently resolved the ambiguity, so a later blind write scored as missing
evidence instead of as a bypassed control. This records the reproduction, the
corrected invariant, the representation and the mutation evidence.

## Reproduction

Trace, after an observation, a decision and a passing risk gate for the same
subject:

```
submit o1 with client key K
acknowledgment_unobserved o1
submit o2 with client key K
submit o3 with no client key
```

Against `crates/sharpebench-core/src/process.rs` at
`4e79baf`, `check_lifecycle` returned zero block violations, one warn violation
and `process_score_with_ordering` returned 0.9. The single finding was

```
AmbiguityUnavailable { subject: Instrument("BTC"), prior_order: OrderId("o2"), retry_order: OrderId("o3") }
```

o3 is a blind retry. o1 was never acknowledged, never filled and never
reconciled, so whether that intent committed at the venue is still unknown when
o3 is sent. If it did commit, o3 doubles the position, which is exactly the
failure the check exists to catch.

## Cause

`process.rs:563` and the parallel site around `:585` set `prior.answered = true`
on any retry, including the same-intent branch where the retry carried the
matching key. `answered` removed the write from `outstanding_write`, so the next
submission saw either nothing or a merely unknown prior, and the check degraded
from block to warn.

The comment defending it, "each outstanding write answers at most one retry",
states the wrong invariant. A shared client key lets the venue collapse the two
writes into one intent, which makes the *retry* safe. It observes nothing about
whether that intent ever reached the venue, so it cannot resolve the ambiguity.
Safety of the retry and resolution of the ambiguity are separate facts, and the
old code conflated them.

## Corrected invariant

Unresolved intent-level ambiguity persists through the entire retry chain. It is
closed only by an observed acknowledgment, fill or reconciliation for a member of
the chain, and closing it settles every write in the chain at once. While a chain
is open, a keyless submission for the same subject is a block-severity
`AmbiguousWriteRetriedWithoutKey` naming the write that originally went
ambiguous, however many keyed retries preceded it.

## Representation

The unit of ambiguity is the intent, not the single prior write. `check_lifecycle`
now carries a `Vec<AmbiguousIntent>` in trace order plus a `BTreeSet<OrderId>` of
every order that has ever belonged to a chain:

```rust
struct AmbiguousIntent {
    subject: Subject,
    origin: OrderId,
    key: Option<ClientOrderKey>,
    members: BTreeSet<OrderId>,
    resolved: bool,
}
```

- A chain opens when a submission still at `Submission` stage is marked
  `AcknowledgmentUnobserved`. Its `origin` is that write and its `key` is
  whatever key the submission carried. `None` matches nothing, including another
  absent key, so a keyless chain can never be joined.
- A submission carrying a key equal to an open chain's key joins that chain as a
  member and emits no finding.
- Any other submission for a subject with an open chain is the block-severity
  finding, naming the oldest open chain's `origin`. The chain stays open, so the
  next blind write is caught too, and one finding is emitted per retry no matter
  how many chains are outstanding.
- An `Acknowledgment`, `Fill` or `Reconciliation` naming any member marks the
  chain resolved, even when that transition is itself out of order: the outcome
  was observed either way.
- Chain members are excluded from `outstanding_write`, which now answers only
  `None` or `Unknown`. Ambiguity is no longer a possible answer there, so the
  warn-severity `AmbiguityUnavailable` remains reserved for a resubmission whose
  prior has no recorded outcome at all.

The per-order `ambiguous` flag is gone; `answered` survives only as the
deduplication flag for `AmbiguityUnavailable`. The kernel stays pure: no clock,
no ambient randomness, and ambiguity is read only from the trace-stated
`Phase::AcknowledgmentUnobserved`.

## Evidence

Red before, on the committed tree at `4e79baf` with the new tests added:

```
process::tests::an_unresolved_intent_survives_a_keyed_retry ... FAILED
  assertion `left == right` failed: the blind third write is a bypass, not missing evidence:
  [AmbiguityUnavailable { subject: Instrument("BTC"), prior_order: OrderId("o2"), retry_order: OrderId("o3") }]
  left: 0
 right: 1
process::tests::an_irregular_fill_still_resolves_the_chain ... FAILED
  the fill observed the intent's outcome:
  [FillWithoutAcknowledgment { order: OrderId("o1") }, AmbiguousWriteRetriedWithoutKey { .. }]
test result: FAILED. 35 passed; 2 failed
```

Green after: `test result: ok. 37 passed; 0 failed`.

Tests added to `crates/sharpebench-core/src/process.rs`:

- `an_unresolved_intent_survives_a_keyed_retry`, the reproduction above,
  asserting one block violation, zero warn violations, a finding naming `o1` and
  a score of 0.0.
- `a_keyed_chain_resolved_by_an_acknowledgment_is_clean`, three submissions under
  one key with an acknowledgment at the end, asserting no violations and a score
  of 1.0.
- `a_fill_resolves_the_chain_so_a_later_keyless_submission_is_clean`, a chain
  whose intent is acknowledged, filled and reconciled, after which a keyless
  submission is a fresh order rather than a blind retry.
- `an_irregular_fill_still_resolves_the_chain`, the fill leg of the resolution
  rule on its own.

Every existing behaviour kept: the keyed retry that is then acknowledged, two
genuinely distinct submissions, the marker against an already acknowledged order
reported as out of order, the warn-severity `AmbiguityUnavailable` when the trace
cannot establish ambiguity, and one finding per retry however many priors are
outstanding.

## Mutation evidence

Mutated in place, run, then restored from `git show HEAD:crates/sharpebench-core/src/process.rs`
with `cmp` returning 0 and `git status --short` empty both times.

1. Keyed retry resolves the chain, that is `chains[i].resolved = true;` added to
   the same-intent branch, which is the old semantics of `prior.answered = true`.
   Result: `an_unresolved_intent_survives_a_keyed_retry ... FAILED`,
   `36 passed; 1 failed`, exit 101.
2. Drop the fill leg, that is `PhaseKind::Acknowledgment | PhaseKind::Fill |
   PhaseKind::Reconciliation` narrowed to `PhaseKind::Acknowledgment |
   PhaseKind::Reconciliation`. Result:
   `an_irregular_fill_still_resolves_the_chain ... FAILED`, `36 passed; 1 failed`,
   exit 101.

Each mutation kills exactly the test that isolates it, and the restored tree is
green at `37 passed; 0 failed`.

## Frozen evidence

Nothing under `paper/`, no golden and no example changed. The whole workspace
excluding `xtask` runs `1121 tests run: 1121 passed, 15 skipped`, and
`git status --short` after the run lists only
`crates/sharpebench-core/src/process.rs`. `AmbiguityUnavailable` and
`AmbiguousWriteRetriedWithoutKey` keep their variants and fields, so no recorded
report changes shape.
