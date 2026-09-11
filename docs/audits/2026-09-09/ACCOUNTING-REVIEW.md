# Adversarial review of the money-accounting repairs

Scope: Bench PR #81 (one writer per money journal, settlement fails closed) and
Bench PR #80 (the call ceiling bounds provider requests), as they stand on main
at `b7d755e`. This is a review of merged work. No production code was changed.
One new test file was added to demonstrate two of the findings:
`crates/sharpebench-harness/tests/journal_ownership_review.rs`.

Every mutation below was applied to an isolated copy of the tree under the
session scratchpad, never to the worktree.

## Dispositions, 2026-09-11

The findings were repaired on `fix/journal-lock-identity`. Each one below now
carries its disposition inline, with the test that pins it and the mutation
that kills that test. The characterization tests in
`journal_ownership_review.rs` were inverted: they assert the repaired behaviour
and are named for it.

| Finding | Disposition |
|---|---|
| A1 | Fixed. A lock document carries the instance that wrote it, and a holder removes only a file still carrying its own |
| A2 | Fixed for aliases that share a directory, by keying ownership on a `journal_id` inside the document, and, from 2026-09-11, for documents written before that field existed, whose identity is derived from their own bytes. Cross-directory aliases remain, deliberately: closing them needs a lock in a namespace the journal's owner does not control, which is a worse trade than the exposure. The reasoning is below and in `HOST-ACCOUNTING.md` |
| A3 | Fixed. The displaced holder's identity is a value this process cannot produce for itself, and the timestamp is asserted |
| A6 | Fixed. One directory per test, unique and removed on drop, leaks included |
| A7 | Fixed. A save whose rename landed no longer rewinds its version, so the sole owner's I/O fault is published as `journal_unwritable` rather than as `journal_ownership_lost` |
| A4 | Fixed, separately from this branch. `main` checks the retry setting of whatever client it will use, so a caller-supplied one is on the same footing as one the run builds, and the ceiling case that drives a real SDK client now runs through the check rather than around it |
| A5 | Fixed, separately from this branch. The case's recording constructor reports a compliant setting whatever it was built with, so removing the keyword it names fails its own assertion instead of erroring inside the driver |
| A8 | Fixed, separately from this branch. An unpriced model refuses the run before the first observation is read, and the rate card is matched by the model-identity rule rather than by a prefix walk. The assembler that publishes the number refuses the same way |
| A9 | Fixed, separately from this branch. A replay is screened by `is_requested_policy`, the one function that also admits a fresh reply, so a record naming a served model this scaffold would refuse is dropped rather than resurrected |

## Findings

### A1. A displaced holder's drop removes the lock of the holder that displaced it

Severity: medium. Two writers on one budget, reachable from the operator action
the repair itself documents.

`crates/sharpebench-harness/src/gateway_journal.rs:452-456`

```rust
impl Drop for JournalLock {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}
```

The drop removes whatever file sits at that path. It does not check that the
file there is still the lock this value created. `JournalLock::take_over`
(`gateway_journal.rs:384-407`) removes the incumbent's lock file and creates a
new one at the same path, but the incumbent is still holding a `JournalLock`
naming that path. When the incumbent exits normally, its drop deletes the
taker's lock, and the journal is then unowned while the taker is live and
spending. Any number of further gateways may then open it.

The type documentation (`gateway_journal.rs:377-383`) warns that a takeover of a
live holder "puts two writers back on one budget". It does not say that the
path afterwards ends up with no lock at all, which is the stronger and longer
lasting consequence: the two-writer window does not close when the displaced
process goes away, it opens wider.

This is reachable without misuse in the case the repair is built for. An
operator takes over what they believe is a dead holder; the holder was in fact
hung rather than dead, or is a process the operator then stops cleanly; its
unwind runs `Drop` and silently unlocks the new owner.

Reproduce:

```
cargo test -p sharpebench-harness --test journal_ownership_review \
  a_displaced_holder_unlocks_the_holder_that_displaced_it
```

The test asserts the current behaviour (`lock_path` gone after the displaced
holder drops, a third `acquire` admitted). It is a record of the defect, not a
guarantee, and must be inverted when the defect is fixed.

**Disposition: fixed, 2026-09-11.** `JournalLockDocument` carries an `instance`,
a token the lock value generates when it writes the file, and `Drop` removes a
path only when the document still there carries this holder's instance. A
takeover writes a new instance, so the displaced holder's drop is a no-op on the
taker's lock. A displaced holder can also find out, rather than discovering it
by writing: `JournalLock::is_still_held` compares the same way. Nothing polls
it, and what stops a displaced holder from spending remains the journal's
compare-and-swap, which refuses the second of the two writers to save. What is
left is a read then an unlink rather than one operation: a takeover landing
between them can still lose the taker's lock, which is microseconds against an
operator action taken by hand, where the old behaviour was certain.

Both tests now assert the repair:
`a_displaced_holder_leaves_the_lock_of_the_holder_that_displaced_it`, in
`journal_ownership_review.rs` and in the library suite. A drop that removes
nothing at all would also leave the taker's lock in place, so both tests require
the taker's own drop to release it. Mutation: `Drop` back to
`let _ = std::fs::remove_file(path);` for each held path. Observed, both tests
failing and nothing else, `149 passed; 1 failed` in the library suite and
`1 passed; 1 failed` in the integration file:

```
panicked at crates\sharpebench-harness\src\gateway_journal.rs:1479:9:
the taker's lock survives the displaced holder's drop
panicked at crates\sharpebench-harness\tests\journal_ownership_review.rs:164:5:
the taker's lock is still there once the holder it displaced has gone
```

### A2. Ownership is keyed on the path spelling, so one journal under two names is two locks, and the compare-and-swap does not catch it

Severity: medium. Both gateways spend the full declared budget and neither ever
learns about the other.

`crates/sharpebench-harness/src/gateway_journal.rs:356-375` derives the lock
path textually from the journal path (`<name>.lock` beside it) and
`gateway.rs:776` / `gateway_serve.rs:842` acquire it as given. Nothing
canonicalizes or resolves the journal path, and nothing binds the lock to the
file's identity.

Two directory entries for one journal file therefore yield two distinct lock
names and both gateways open. The version compare-and-swap does not recover the
situation either, because `GatewayJournal::save`
(`gateway_journal.rs:811-864`) persists through a temporary file and a rename.
The rename replaces one directory entry only, so after the first save the two
names are two separate files, each at version 1, and no writer ever observes a
version it did not write.

Measured, with a hard link:

```
cargo test -p sharpebench-harness --test journal_ownership_review \
  two_names_for_one_journal_file_are_two_locks_and_both_gateways_spend
```

Both gateways open, both dispatch once, `journal_conflict()` is false on both.

What I measured about the neighbouring shapes, so the finding is not stated
wider than the evidence:

- A **symlink to the journal file** also defeats the lock (the second gateway
  opens), but the compare-and-swap does catch it sequentially: the second
  gateway's reserve save is refused, it latches `journal_conflict` and it
  dispatches nothing. Observed spends 15 and 0.
- A **symlinked directory** (`current -> run7`, the common deployment shape)
  does **not** defeat the lock. The lock path aliases exactly as the journal
  path does, so `create_new` refuses the second gateway. Observed
  `second_open_ok=false`. This is worth recording because it is the case an
  operator is most likely to have, and the repair holds there.

So the exposure is specifically two directory entries for one inode, or any
other arrangement where the journal aliases but its `.lock` sibling does not.

Note also that even where the compare-and-swap does fire, it is a read then
write, not an atomic swap: `save` reads `version_on_disk` at line 813 and only
renames at line 849, with a file create, write and `sync_all` in between. Two
writers that both read the same version inside that window both proceed. The
code calls it "a second line of defence", which is accurate, but it is not a
concurrency control and should not be read as one. I did not build a racing
repro for this; it is a code reading.

**Disposition: fixed for aliases that share a directory, 2026-09-11.** A journal
document now carries a `journal_id`, assigned once and carried through every
save. It is a property of the document rather than of the entry it was reached
through, so it survives the rename a save persists through and every name for
one document reports it alike. Ownership is a second lock file keyed on it,
`sb-gateway-journal-<id>.lock`, taken beside the spelling lock and held with it.
A journal that does not exist yet names no document, so `ModelGateway::open` and
`run_gateway_sweep` write one before binding: opening a persisted journal now
creates the file rather than waiting for the first call.

Canonicalizing the path, the obvious approach, was measured and does not do the
job. It does not resolve hard links at all, which is the case this finding is
about; on Windows it returns verbatim paths
(`\\?\C:\Users\...\a\journal.json`), which would leak into
`ModelGateway::journal_lock_path` and into the operator-facing refusal message;
and for a journal that does not exist yet, the normal first-run case, it fails
with `NotFound`, so there would be nothing to key the first lock on. Every
spelling of one path already resolves to one lock file through the file system,
which is what this review measured for `.`, trailing separators, relative paths
and symlinked parents.

What remains open, measured rather than assumed:

- Two directory entries for one document in **different** directories. The lock
  is a sibling of the journal, so two parents give two lock files. Observed with
  a hard link across `a/` and `b/`: `ids equal: true`, and
  `cross-directory alias: first=true second=true`.
- A journal document written before `journal_id` existed names none. Its opener
  assigns one and saves before binding, so two gateways opening such a document
  under two names would each assign one. Only documents from before this change
  are in that state, and only until their next save.
- The compare-and-swap is still a read then a write, not an atomic swap. That is
  unchanged, and it is still not a concurrency control.

**Disposition of the pre-identity half: fixed, 2026-09-11.** A document that
names no identity is no longer given a fresh one. Its identity is a digest of
its own bytes, `sha256("sharpebench.gateway-journal-derived-identity.v1" ||
document)` truncated to the same 32 hexadecimal characters a fresh identity
uses, so every name for one such document derives the same value, `acquire`
takes its lock before anything is written, and exactly one of two gateways
opening it concurrently proceeds. The winner writes that derived value into the
document, which is what keeps the lock's name correct once the save changes the
bytes it was derived from. The direction this can be wrong in is over-refusal:
two byte-identical documents that are genuinely separate journals, sharing one
directory, derive one identity and the second gateway is refused.

Three tests pin it, and they separate the two causes involved.
`a_second_name_for_one_legacy_journal_document_is_refused_the_lock` covers the
concurrent window, where neither gateway has written yet: the alias's spelling
lock is shown free first and the refusal is required to name
`sb-gateway-journal-<derived>.lock`, so the spelling lock is not what refused,
and nothing has been written, so no binding or version cause is available.
`a_legacy_journal_document_is_bound_to_the_identity_derived_from_its_bytes`
covers the assignment: a fresh identity cannot equal a digest of the bytes, and
the identity is asserted again after a save that changes those bytes, so it
cannot be being re-derived rather than stored. The gateway-level
`a_legacy_journal_document_admits_one_gateway_under_two_names` requires the
identity the document ends up carrying to be the value derived before either
gateway opened, which separates "the first gateway wrote some identity" from
"the first gateway wrote the derived one".

Mutation, the identity no longer derived (`document_identity` back to
`journal_id_on_disk` alone). Observed `151 passed; 2 failed` in the library
suite and `2 passed; 1 failed` in the integration file:

```
panicked at crates\sharpebench-harness\src\gateway_journal.rs:1749:9:
a document that names no identity is still a document to own
panicked at crates\sharpebench-harness\src\gateway_journal.rs:1795:9:
assertion `left == right` failed: the document is given the identity its lock was taken on, not a fresh one
  left: Some("db94182c87f062fe5f26c241bd4f8626")
 right: Some("700780fe3b3063a7d5039cc835a46242")
panicked at crates\sharpebench-harness\tests\journal_ownership_review.rs:280:5:
assertion `left == right` failed: the document is given the identity derived from it, not a fresh one
  left: Some("9e57acf4cf936ea955ef312cfcfa8805")
 right: Some("29b7fefac91feda8504d0ebde51779c0")
```

A second mutation, the lock still taken on the derived identity but a fresh one
written into the document, kills the two assignment tests alone and leaves the
lock test green (`152 passed; 1 failed`):

```
panicked at crates\sharpebench-harness\src\gateway_journal.rs:1795:9:
assertion `left == right` failed: the document is given the identity its lock was taken on, not a fresh one
  left: Some("ec934b06a28834995684127934a54895")
 right: Some("700780fe3b3063a7d5039cc835a46242")
```

**Disposition of the cross-directory half: not fixed, and not intended to be.**
The exposure is real and was re-measured after the change above:
`ids equal: true`, `cross-directory alias: first=true second=true`, with the two
lock paths differing only in their parent (`.../a/sb-gateway-journal-<id>.lock`
against `.../b/sb-gateway-journal-<id>.lock`). Closing it needs one lock per
document wherever the document is reached from, which means either a lock
outside the journal's directory or a lock on the file itself. Both were
evaluated:

- **A lock on the journal file itself**, through `std::fs::File::lock`, is the
  only option that is keyed on the document with no namespace at all. It is
  incompatible with the way the journal persists, measured on this host with a
  standalone probe against the same operations `save` performs: with the lock
  held, `save`'s own read of the on-disk version fails, `The process cannot
  access the file because another process has locked a portion of the file. (os
  error 33)`; the rename over the locked journal then succeeds anyway, and a
  handle opened on the journal's name immediately afterwards takes the lock,
  because the rename replaced the entry with a different file. So it breaks the
  sole owner's own save path and protects nothing past the first save.
- **A lock in a shared namespace**, a system temp or a per-user state
  directory, named `sb-gateway-journal-<id>.lock` from the identity alone. The
  name leaks nothing: the identity is a token, not a path, and carries no
  journal content. The cost is what it would do to the common case. A system
  temp is swept by age on many hosts, so a long sweep's lock could be removed
  while it is held, silently readmitting a second gateway for every journal, to
  close an exotic one. A per-user directory is not swept, but it is per user,
  so it does not separate the two users a cross-directory hard link on a shared
  host would need separating, and it puts durable state outside the journal's
  own directory that an operator has to know about to take a lock over. Both
  need a fallback where the directory is absent or unwritable, and a fallback
  means the guarantee is conditional and silently missing rather than stated.

The exposure this would buy is an operator deliberately hard linking a money
journal into a second directory and running two gateways over it. The remedy
weakens ownership for every journal to close that. It is not built, and the
limit is stated in `JournalLock`'s own documentation, in `HOST-ACCOUNTING.md`
and in the book instead.

The test is inverted and renamed
`two_names_for_one_journal_document_admit_one_gateway`. Two causes could refuse
the second open, the spelling lock and the document lock, so the test shows the
alias's spelling lock free first and requires the refusal to name the document's
lock; a third cause, a journal bound to another experiment, would refuse with
`InvalidData` rather than with a lock error. Mutation: put the journal's file
name back into the identity lock's name, which is keying on the spelling again.
Observed, `1 passed; 1 failed`:

```
panicked at crates\sharpebench-harness\tests\journal_ownership_review.rs:220:10:
the second gateway is refused the document the first owns
```

and in the library suite `148 passed; 2 failed`, the two being
`a_second_name_for_one_journal_document_is_refused_the_lock` and
`a_journal_identity_is_the_documents_and_survives_every_save`. A second
mutation, `JournalLock::acquire` not binding the document at all, kills
`a_second_name_for_one_journal_document_is_refused_the_lock` alone
(`149 passed; 1 failed`) and leaves the gateway-level test green, because
`ModelGateway::open` binds the document itself. A third, assigning a fresh
`journal_id` on every save, kills
`a_journal_identity_is_the_documents_and_survives_every_save` alone.

### A3. `a_takeover_is_explicit_and_records_who_it_displaced` passes for a cause other than the one it names

Severity: medium as an evidence defect. The named property, that a takeover
records *who* it displaced, is not established by any test.

`crates/sharpebench-harness/src/gateway_journal.rs:1101-1128`

The test creates the displaced lock with `std::mem::forget` in the same process,
so the displaced holder's pid is this process's pid. It then asserts

```rust
assert_eq!(superseded.pid, std::process::id());
```

which is satisfied by the taker writing its own pid just as well as by the taker
reading the displaced document. `superseded.acquired_unix_ms` is not asserted at
all, so the timestamp half of the record is unpinned entirely.

Demonstrated by mutating the named cause at `gateway_journal.rs:396`:

```rust
-                pid: document.pid,
+                pid: std::process::id(),
```

`cargo test -p sharpebench-harness --lib` then reports
`144 passed; 0 failed`. The whole harness suite stays green while the takeover
records the wrong holder. The `SupersededHolder` fallback for an unparseable
lock document (`gateway_journal.rs:400-404`, pid 0) is likewise untested.

Fixing the test needs a displaced document whose pid is not this process's: write
a lock document with a chosen pid directly, then take it over and assert that
pid and that timestamp come back.

**Disposition: fixed, 2026-09-11.** The test writes the displaced holder's lock
document itself, with pid 424242 and `acquired_unix_ms` 1111111111111, and
asserts on both. Neither is a value this process can produce for itself, so only
the takeover having read the displaced document can satisfy the assertion. The
`SupersededHolder` fallback for an unparseable document is covered too, by
`a_takeover_of_an_illegible_lock_records_that_it_learned_nothing`.

Mutation, the one this finding names, at the same line:

```rust
-                pid: document.pid,
+                pid: std::process::id(),
```

Observed, `149 passed; 1 failed`, where the whole harness suite used to stay
green:

```
panicked at crates\sharpebench-harness\src\gateway_journal.rs:1413:9:
  left: 16104
 right: 424242
```

And the timestamp half, `acquired_unix_ms: document.acquired_unix_ms` to
`acquired_unix_ms: 0`, also `149 passed; 1 failed`:

```
panicked at crates\sharpebench-harness\src\gateway_journal.rs:1417:9:
  left: 0
 right: 1111111111111
```

### A4. The runtime retry guard is not on the path a caller-supplied client takes

Severity: low. Not reachable from the production entry point, but it is the one
place the ceiling's stated guarantee does not hold.

`examples/llm-agent/llm_agent.py:488-491`

```python
def main(client=None):
    client = client if client is not None else build_client()
```

`assert_no_provider_retries` is called inside `build_client`
(`llm_agent.py:484`), not in `main`. A client handed to `main` reaches
`client.messages.create` (`llm_agent.py:546`, via `call_model` at 431) with its
retry policy never read. The documented limit is that "the ceiling guarantee is
conditional on the runtime assertion"; on this path there is no assertion.

Reproduce (the stand-in reports an effective `max_retries` of 2):

```python
import sys, tempfile; sys.path.insert(0, "paper/src")
import test_llm_agent_budget as T
shim = T.load_shim(tempfile.mkdtemp(), max_calls="1")
retrying = T.Ignoring()
T.drive(shim, retrying)
print(len(retrying.requests))   # 1, dispatched under an unchecked client
```

Observed: `client.max_retries = 2`, one request dispatched.

The production producer is unaffected: `crates/sharpebench-harness/examples/llm_field_eval.rs:438-442`
spawns `python llm_agent.py <model>`, which enters through `if __name__ ==
"__main__": main()` with no client. The suite's own
`test_n_units_allow_exactly_n_provider_requests_across_a_retryable_failure`
uses the bypass deliberately (`test_llm_agent_budget.py:183-193`). Moving the
assertion into `main` would close the hole without changing that test's shape.

**Disposition: fixed, 2026-09-11.** `main` calls `assert_no_provider_retries`
on whatever client it will use, supplied or built, so there is no longer a path
to `client.messages.create` with the retry policy unread. The guarantee, either
the ceiling bounds provider requests or the run does not start, now covers the
caller-supplied path as well as the production one.

A supplied client is checked rather than refused. The guarantee is about how the
client behaves and not about who constructed it, and the case that observes the
SDK's own retry behaviour has to hand in a real `anthropic.Anthropic` bound to a
stand-in transport; refusing every supplied client would push that case back
onto a path with no check on it, which is the shape being removed. Because the
client it supplies is built with the shim's own setting, that case now passes
through the check instead of around it, which is what this finding asked for.
`build_client` keeps its own check: it is reachable on its own, and a new case
pins it so it cannot be deleted with the suite green.

A new case drives a caller-supplied client that accepts `max_retries` and
reports two. Four causes could produce a refusal there without the check on that
path, and each is excluded: a stand-in too thin to dispatch (`Ignoring` answers
normally, and a compliant client of the same shape completes a run and
dispatches in the same case), a client the run built for itself (the constructor
is replaced by one that fails the test if it is called), an exhausted allowance
(the ledger is asserted empty and the message is the check's), and the supplied
path refusing anything at all (the compliant control is supplied the same way).
Deleting the call in `main` fails that case and nothing else, on its own
`assertRaises`; deleting the call in `build_client` fails the helper's case and
nothing else. See section 6 of [inherited repairs](INHERITED-REPAIRS.md).

### A5. `test_the_client_the_run_uses_disables_the_sdk_automatic_retries` does not reach its own assertion when its named cause is broken

Severity: low. The regression gate still holds; the test's stated reason does
not.

`paper/src/test_llm_agent_budget.py:322-343`

Mutating the named cause in the isolated copy:

```python
-    client = anthropic.Anthropic(max_retries=PROVIDER_MAX_RETRIES)
+    client = anthropic.Anthropic()
```

gives `FAILED (errors=1)`, and the one failure is an `ERROR` raised inside
`drive(...)` at line 341: the `Honouring` stand-in reports `max_retries=None`,
`assert_no_provider_retries` refuses the run, and the test never evaluates its
own assertion at line 343. This is the same shape the verification record
already names for "the runtime guard", recurring in the other direction: the
failure is incidental to the guard rather than the constructor argument the test
is about. Asserting on `seen` outside the `with` block, or building the
recording constructor so it reports zero regardless, would isolate it.

Coverage in aggregate is not affected: mutating `PROVIDER_MAX_RETRIES` to 2
fails three tests, and deleting the guard call fails two others, both for their
named reasons.

**Disposition: fixed, 2026-09-11.** The case's recording constructor now sets
the returned stand-in's effective setting to `PROVIDER_MAX_RETRIES` whatever
keyword it was built with, so the runtime check cannot be what refuses and the
only thing left to answer the assertion is the keyword the run passed. Removing
`max_retries=` from `build_client` gives `FAILED (failures=1)` at
`self.assertEqual(seen[0].get("max_retries"), 0)` with `AssertionError: None !=
0`, in place of the `ERROR` raised inside `drive` this finding reports. Reading
the stand-in's setting from the constant rather than writing a literal keeps the
isolation under a mutation of the constant itself: with `PROVIDER_MAX_RETRIES`
at 2 the check still passes and the case fails on its own assertion, 2 against
the literal 0 it asserts.

The suite's default stand-in client now reports the compliant setting, because
A4's repair means every client `main` is handed is checked; the three classes
that vary the setting do so deliberately. The same one-line change was needed in
`test_llm_agent_identity.py`, whose cases are about the model-identity rule and
would otherwise be refused before reaching it.

### A6. The gateway regressions are not hermetic, and an inherited journal already produced a misleading failure

Severity: low. Test reliability, not money.

`crates/sharpebench-harness/src/gateway.rs:1422-1430` and
`gateway_journal.rs:1048-1052` build their working directories under the shared
system temp keyed on `std::process::id()` (plus a `ThreadId` in the gateway
case), and the `remove_dir_all` cleanup is the last statement of each test, so a
panicking run leaves the directory behind. Operating system pids are recycled.
`ModelGateway::open` resumes any journal it finds, so a re-run that lands on a
recycled pid resumes a previous run's spend record.

Observed during this review. A mutation run of
`a_settlement_that_cannot_be_persisted_stops_the_gateway` failed with

```
assertion `left == right` failed: the settlement the file is missing is still in memory
  left: 45
 right: 15
```

45 is three priced calls at 15. The run had inherited a journal with two calls
already in it from an earlier crashed run under the same pid. The same mutation
re-run in a clean directory failed at the intended assertion, the
`next.error.expect("error")` on the second request (`gateway.rs:2814-2819`),
which is the correct signal. The leftovers are
still on this machine, including
`sb-gateway-unwritable-17128-ThreadId(25)/journal.json` left behind as a
*directory*, which is the sabotaged state a later run would inherit.

`a_stale_lock_is_refused_rather_than_broken`
(`gateway_journal.rs:1081-1097`) is the sharpest case: it deliberately leaks a
lock file with `std::mem::forget` and only removes it on the success path, and
its directory has no thread component at all, so a panic there poisons that
pid's directory for `a_second_holder_of_one_journal_is_refused` as well.

A `TempDir` with a random component, or a cleanup guard, removes the class.

**Disposition: fixed, 2026-09-11.** `crate::scratch::ScratchDir` gives every test
its own directory, named with the pid, a counter within the process and a
nanosecond stamp, and removes it in `Drop`, which a panicking test still runs.
The gateway, gateway-journal and gateway-serve regressions all take their
directories from it, as does the integration file through a local copy of the
same shape. `a_stale_lock_is_refused_rather_than_broken` still leaks a lock
deliberately, and now leaks it into a directory nothing else will ever name.

Observed: 30 directories under the system temp matched the old naming before a
full `cargo test -p sharpebench-harness` run and 30 after it, so the run left
nothing behind. The 30 are leftovers from runs before this change, including the
`journal.json` left as a *directory* that this finding names; they are outside
the worktree and were not touched. No new-scheme directory survives a run, and
an old-scheme leftover cannot be inherited by one, because the names no longer
have the same shape.

### A7. A reservation write that fails with I/O does not latch, and a partly landed save is later reported as another writer

Severity: low. No double spend; a wrong diagnosis in the published record.

`crates/sharpebench-harness/src/gateway.rs:989-1006`. The reserve-before-dispatch
save latches `journal_conflict` on a version conflict but returns
`JournalUnwritable` on `JournalSaveError::Io` without latching. That asymmetry
with `settle_and_persist` (`gateway.rs:1203-1219`) is defensible on its own
terms: nothing was dispatched, the reservation is released in memory, and the
snapshot's version still matches the disk, so the next call is consistent.

The case where it is visibly wrong is a save that partly landed.
`GatewayJournal::save` decrements `self.version` on any error
(`gateway_journal.rs:860`), including a failure of the post-rename parent
directory `sync_all` at line 851, by which point the rename has already moved
the disk to `version + 1`. The gateway is then a version behind the file it
owns, does not latch, and the next reserve save is refused as
`Conflict`. That latches `journal_conflict`, and the sweep publishes
`journal_ownership_lost: true` (`gateway_serve.rs:752`) for what was an I/O
fault by the sole owner. Money is still safe: the gateway stops before
dispatching. I did not reproduce this; it needs a directory fsync to fail after
a successful rename, which is a Unix-only path I cannot force here.

**Disposition: fixed, 2026-09-11.** `save` now knows whether the rename landed.
When it did, the version bump stands, because the disk really is at the new
version, and the failure comes back as a new `JournalSaveError::Unsynced` rather
than as `Io`. The snapshot therefore stays level with the file it owns and its
next save is not refused as another writer's. The reservation path treats
`Unsynced` as it treats a settlement that could not be written: it latches
`journal_unwritable`, because the file holds a reservation this gateway will
never settle, and the sweep publishes `journal_unwritable` rather than
`journal_ownership_lost`. A plain `Io`, where nothing landed, still refuses per
call without latching, which the review found defensible and which is unchanged.

The partly landed save is now reachable on every platform. The post-rename
durability step is its own function with a thread-local one-shot fault behind
`cfg(test)`: forcing a real directory fsync to fail is Unix-only, and what the
finding is about is what the caller does with a half-landed write, so the fault
is injected rather than provoked.

Two tests pin it.
`a_save_whose_rename_landed_is_unsynced_rather_than_a_later_conflict` reads the
version off the disk, so a save that never bumped at all cannot satisfy it, and
then requires the next save to succeed.
`a_reservation_whose_durability_is_unconfirmed_is_not_published_as_lost_ownership`
asserts the label: three causes could leave `journal_conflict` false, the save
not failing, the save failing before the rename, and the repair, and the first
two are ruled out by the refusal itself and by the file being a version ahead.

Mutation, decrementing the version on the landed path as before and reporting
`Io`. Observed `148 passed; 2 failed`, both of them these:

```
panicked at crates\sharpebench-harness\src\gateway.rs:2894:9:
the fault latches
panicked at crates\sharpebench-harness\src\gateway_journal.rs:1637:9
```

A second mutation, latching `journal_conflict` on `Unsynced` instead, which is
the misdiagnosis itself, kills the gateway test alone (`149 passed; 1 failed`):

```
panicked at crates\sharpebench-harness\src\gateway.rs:2885:9:
  left: JournalOwnershipLost
 right: JournalUnwritable
```

### A8. An unpriced model reports its calls as free

Severity: medium. A plausible wrong number published as what a run spent, on the
producer behind the money column.

Found while repairing A4, reported in section 6 of
[inherited repairs](INHERITED-REPAIRS.md) and left for a decision rather than
fixed silently.

`examples/llm-agent/llm_agent.py:153-157`

```python
def price_for(model):
    for prefix, p in PRICING.items():
        if model.startswith(prefix):
            return p
    return (0.0, 0.0)
```

Two fail-open behaviours in five lines.

The fallback answers a model the table does not name with a zero rate card, so
every call of such a run priced at nothing, `STATS["cost_usd"]` stayed 0.0 and
that zero was what the field reported as spend. For a benchmark that prices what
an agent spent, a plausible zero is worse than an absence: a reader has no way
to tell it from a run that really cost nothing.

The prefix walk is the model-identity defect in the accounting. It takes any
continuation, so a model whose name extends a priced one is billed at the other
model's card: `claude-opus-5-1` prices as `claude-opus-5`, at half the input
rate, and a table gaining a `claude-haiku-4` would price every
`claude-haiku-4-5` at whichever key `dict` iteration reached first. That is the
same shape as the served-id prefix acceptance already repaired in
`effective_model`, in the file that repaired it.

`paper/evidence/assemble_llm_field.py:39-44` carried an independent copy of both,
and it is the script that writes the published `cost_usd`.

**Disposition: fixed, 2026-09-11.** Refusal, not a recorded unavailability, and
the reasoning is in section 7 of [inherited repairs](INHERITED-REPAIRS.md). The
rate card is matched by `is_dated_snapshot_of`, the rule that decides model
identity, and an unpriced model raises `UnpricedModel`. `main` establishes it
through `assert_model_is_priced` before the first observation is read, beside
the retry guard, so the refusal costs nothing rather than arriving after a field
has been billed. The assembler refuses the same way, as `SystemExit`, which is
how every other incompleteness in that script refuses.

### A9. A replay is screened by a shorter rule than a fresh answer

Severity: low, and bounded. Reachable only through a cache file this scaffold
did not write.

Also found while repairing A4 and reported with it.
`effective_model` refuses a served id that is not the requested policy, and
`record_decision` stamps `model_effective` on every record, but `load_cache`
screened a record on `scaffold_version`, `request_sha256` equal to its own key
and `model_requested`, and not on `model_effective`. A record naming a served
model this scaffold would refuse today was replayed rather than dropped.

The bound is real: this scaffold cannot write such a record, so it takes a
foreign or hand-edited cache file, and the request digest must still match a
request for the requested model. It is the same shape as A4 either way, a check
on the writing path and not on the reading one, and a replayed decision is
published exactly as a fresh one is.

**Disposition: fixed, 2026-09-11.** The rule moved into `is_requested_policy`,
which `effective_model` and `load_cache` both call, so the replay screen is the
rule rather than a copy of it that can drift. A record whose `model_effective`
is absent or null is dropped too, the same absence `effective_model` refuses.

## Claims checked and found sound

**The lock's exclusivity is real and well pinned.** Mutating
`gateway_journal.rs:431` from `.create_new(true)` to `.create(true)` fails four
tests: `a_second_gateway_cannot_open_the_journal_the_first_owns`,
`only_one_of_many_racing_gateways_owns_the_journal`,
`a_second_holder_of_one_journal_is_refused` and
`a_stale_lock_is_refused_rather_than_broken`. The racing test's assertion is
winner-independent and would read 8 admitted rather than 1 if the lock did
nothing.

**The same process opening two gateways on one path is refused.**
`a_second_gateway_cannot_open_the_journal_the_first_owns` (`gateway.rs:2622`)
opens both in one process and the second is refused with a typed
`JournalLockError::Held` naming the lock file. `create_new` does not exempt the
creating process.

**Relative against absolute paths, and `.` or trailing-separator spellings, do
not defeat the lock.** `lock_path` joins the lock name onto the journal's own
parent, so every spelling of one path produces a spelling of one lock path, and
`create_new` is resolved by the file system. Case-insensitive file systems
resolve `Journal.json.lock` and `journal.json.lock` to the same file for the
same reason.

**A symlinked parent directory does not defeat the lock.** Measured:
`second_open_ok=false` with `current -> run7` and the journal named through both.

**The lock is released on drop and the path opens again.** Asserted in
`a_second_gateway_cannot_open_the_journal_the_first_owns` and exercised in
`gateway_serve`. There is no window in which the gateway can still spend after
the lock is released: the lock is a field of `ModelGateway`
(`gateway.rs:715`), so it outlives every method that can reserve or dispatch and
is dropped only with the gateway itself. `into_journal` consumes the gateway.

**A stale lock is refused rather than broken, and a takeover is required.**
Confirmed by reading: there is no liveness probe, no timeout and no automatic
removal anywhere in `JournalLock`. `take_over` is the only path that removes a
lock it did not create, and it refuses a path that holds none
(`JournalLockError::NotHeld`). This is the documented limit and it is accurate.
Finding A1 is about what happens after a takeover, not about the takeover gate
itself.

**The latch is consulted before anything reaches a provider.** There is exactly
one provider call site in the crate, `gateway.rs:1022`, and both latch checks
(`gateway.rs:948` and `954`) precede it in the same function, ahead of the
reservation, the permit and the wire. Every path that can reserve, dispatch or
settle goes through `dispatch_once`: `serve_line` to `serve` to
`dispatch_with_retries` to `dispatch_once`, and `gateway_serve.rs:585` is the
only external driver. The latches are write-once; nothing clears them.

**The retry loop cannot get past a latch.** `dispatch_with_retries`
(`gateway.rs:910-930`) retries only `ProviderRateLimited`,
`ProviderUnavailable`, `ProviderTimeout` and `ProviderResponseInvalid`. Several
settlement sites ignore the boolean `settle_and_persist` returns and hand back a
retryable kind, but the retry re-enters `dispatch_once`, which reads the latch
first, so the refusal still lands before a reservation and before the wire.

**The settlement latch is load-bearing and its test is isolated.** Deleting the
`journal_unwritable` check at `gateway.rs:954-956` in the isolated copy makes
`a_settlement_that_cannot_be_persisted_stops_the_gateway` fail at the assertion
that the *second* request is refused (`gateway.rs:2814-2819`), which is precisely the
isolation the verification record claims it added. The gateway's version and the
disk's agree again at that point, so nothing but the latch can refuse it.

**A refusal after latching leaves the journal consistent.** The records the
gateway appended stay in memory and are reachable through `into_journal`; the
file keeps the last state the gateway owned; and an unsettled reservation folds
as consumed at its reserved amount (`gateway_journal.rs:683-689`), so a restart
over-reports rather than under-reports. The sweep publishes the condition as
`journal_unwritable` / `journal_ownership_lost` beside the pool
(`gateway_serve.rs:725-732`).

**The compare-and-swap catches a journal that moved under a single writer.**
Removing the version check at `gateway_journal.rs:816-821` fails exactly
`a_second_gateway_cannot_spend_the_journal_the_first_owns`, and nothing else, so
that test does pin its named cause.

**A removed parent directory does not let a second writer in behind the first.**
If the journal's directory goes, saves fail with I/O and the gateway refuses
before dispatching; if it comes back, the first gateway's next save sees a
version that is not its own and latches. I traced this rather than reproducing
it.

**The call ceiling counts reservations written before the request, and survives
the process.** `reserve_call` (`llm_agent.py:284-314`) appends, flushes and
`fsync`s before `call_model`, and `load_attempt_count` reads the line count at
startup. Mutating `PROVIDER_MAX_RETRIES` to 2 fails three tests; deleting the
`assert_no_provider_retries` call fails the two refusal tests with "RuntimeError
not raised", which is their named reason.

**A cached decision reaches no provider and spends no unit.** The cache branch
(`llm_agent.py:509-526`) returns before `reserve_call` and before
`call_model`, and `test_a_cached_decision_spends_nothing` pins it through
`STATS["cache_hits"]` rather than through the request count alone.

**The harness respawn after a mid-call failure cannot respend the unit.** The
ledger line is durable before the request, and the respawn re-reads it. This is
what `test_a_failed_dispatch_spends_its_unit_and_a_retry_cannot_respend_it`
exercises, and the ledger is a separate file from the cache so deleting one does
not restore the other's allowance.

**There is no retry inside the Python scaffold.** The only provider call is
`client.messages.create` at `llm_agent.py:431`; `anthropic.APIError` is caught
once, at `llm_agent.py:593`, and re-raised as a `RuntimeError` that ends the
subprocess. No loop re-enters the call.

**The SDK pin and the evidenced version agree.** `.github/workflows/ci.yml:446`
installs `anthropic==0.112.0` and `EVIDENCED_SDK_VERSION`
(`llm_agent.py:119`) names the same version. Locally installed: anthropic
0.112.0, httpx 0.27.2. `test_llm_agent_budget.py` imports `httpx` directly at
line 51 and gets it transitively from the pinned SDK; that is an undeclared
dependency, but it cannot drift while the pin stands.

**The producer spawns sequentially.** `llm_field_eval.rs:402-457` iterates
datasets and models in plain `for` loops with no threading, so the ledger's
documented assumption holds for the shipped producer.

## Limits: what the documentation claims, and what else is unprevented

The three stated limits are accurate as far as they go:

- Two hosts over a network file system is unprevented. Correct, and it is the
  right characterisation: `create_new` is only as exclusive as the remote server
  makes it.
- A crashed holder needs an explicit take-over. Correct; there is no automatic
  break anywhere in `JournalLock`.
- The ceiling's guarantee is conditional on the runtime assertion. Correct for
  the production entry point. It was absent on the caller-supplied client path,
  which A4 records and which is now fixed: `main` checks whatever client it will
  use, so the assertion covers every path that can reach a provider request from
  this module's entry point.

Silently unprevented and undocumented:

1. One journal reached through two directory entries, defeating both the lock
   and the compare-and-swap (A2). **Closed for aliases sharing a directory on
   2026-09-11, pre-identity documents included. Cross-directory aliases remain
   open deliberately, with the reasoning and the measurements above, and are
   stated in `JournalLock`'s own documentation, in `HOST-ACCOUNTING.md` and in
   the book.**
2. A takeover leaving the path unlocked once the displaced process exits
   normally (A1). **Closed 2026-09-11.**
3. The compare-and-swap being a read then a write rather than an atomic swap,
   so it is not a concurrency control even where it does fire (A2, last
   paragraph). **Still true. It is not redundant either: a takeover deliberately
   adds a second writer to a journal whose first holder may be alive, and
   nothing in `save` consults a lock, so the version check is what refuses that
   holder's next write. Pinned on 2026-09-11 by
   `a_displaced_holder_is_refused_by_the_version_check_once_the_taker_writes`,
   which rules out an I/O fault by requiring a `Conflict` carrying both
   versions, and rules out the lock by having the same displaced holder's
   earlier save land while its lock is already displaced: the only thing that
   changes between the two is the taker having written in between. Mutation, the
   version comparison removed: `151 passed; 2 failed`, this test and
   `a_second_gateway_cannot_spend_the_journal_the_first_owns`, where removing it
   used to fail the second alone.**

   ```
   panicked at crates\sharpebench-harness\src\gateway_journal.rs:1869:14:
   the displaced holder's next write is refused: ()
   ```

   Making it atomic was considered and not done. No portable file-system
   operation renames a file only if its target still carries a given version, so
   an atomic swap would need a per-version claim file taken with `create_new`,
   which a crashed writer leaves behind: a benign fault turned into a journal
   its own owner cannot advance without an operator, to narrow a window the lock
   already covers.
4. The ceiling's dependence on sequential spawning is documented in the shim's
   own docstring (`llm_agent.py:298-301`) but not in the verification record's
   limits, where the other conditional guarantees are stated.

## Not established

- **No real provider was involved.** Nothing here says what an actual Anthropic
  endpoint does under any of these paths, and the retry reading remains a
  reading of anthropic 0.112.0's source plus the observed behaviour of a real
  SDK client over a stand-in transport.
- **No second host.** The network-file-system limit could not be tested and is
  taken on the documentation's own terms.
- **No real crash.** Every "crashed holder" in the suite and in this review is
  `std::mem::forget` in a live process. Whether a real kill on this platform
  leaves the lock file behind as assumed was not exercised.
- **No concurrent double-save race was produced.** The compare-and-swap's
  read-then-write window is established by code reading only. Whether it is wide
  enough to hit in practice was not measured.
- **A7's partly landed save was not reproduced.** Forcing a parent directory
  `sync_all` to fail after a successful rename is a Unix-only path and was not
  available here. The repair of 2026-09-11 exercises the case with an injected
  failure at that step, which is not the same as a real one: what is pinned is
  what the caller does with a half-landed write, not that a real directory sync
  fails in the way assumed.
- **The mutation evidence is local.** Every mutation was run against
  `cargo test -p sharpebench-harness` and `python -m unittest
  paper/src/test_llm_agent_budget.py` on Windows with anthropic 0.112.0. The
  three operating system matrices, `cargo deny` and the packaged consumers were
  not run.
