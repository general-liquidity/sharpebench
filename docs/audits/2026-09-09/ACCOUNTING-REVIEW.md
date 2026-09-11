# Adversarial review of the money-accounting repairs

Scope: Bench PR #81 (one writer per money journal, settlement fails closed) and
Bench PR #80 (the call ceiling bounds provider requests), as they stand on main
at `b7d755e`. This is a review of merged work. No production code was changed.
One new test file was added to demonstrate two of the findings:
`crates/sharpebench-harness/tests/journal_ownership_review.rs`.

Every mutation below was applied to an isolated copy of the tree under the
session scratchpad, never to the worktree.

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
  the production entry point; see A4 for the path where the assertion is absent.

Silently unprevented and undocumented:

1. One journal reached through two directory entries, defeating both the lock
   and the compare-and-swap (A2).
2. A takeover leaving the path unlocked once the displaced process exits
   normally (A1).
3. The compare-and-swap being a read then a write rather than an atomic swap,
   so it is not a concurrency control even where it does fire (A2, last
   paragraph).
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
  available here.
- **The mutation evidence is local.** Every mutation was run against
  `cargo test -p sharpebench-harness` and `python -m unittest
  paper/src/test_llm_agent_budget.py` on Windows with anthropic 0.112.0. The
  three operating system matrices, `cargo deny` and the packaged consumers were
  not run.
