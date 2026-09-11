# False-pass sweep, 2026-09-11

Five instances of one pattern were found in this repository over two days, every
one of them by accident while doing something else and none by a gate. They are
recorded in the [verification record](VERIFICATION.md) under "Three tests that
would have passed while the thing they name was not what refused", plus the
fourth and fifth in the rows above it.

The shape is one thing: an assertion on an outcome that several independent
causes can produce, so it says nothing about the cause the test is named for. A
gateway with no latch refused by a broken fixture. A client setting asserted on
a client the test built itself. A guard-deletion mutant caught by an incidental
attribute error. A takeover asserted against this process's own pid. A ledger
exclusivity case that three causes could satisfy.

This is the systematic search for the rest of them. It is a test-quality sweep:
no production behaviour changed, and no golden, example or `paper/evidence/`
value moved.

## Method, and why this one

A false pass is invisible from the test's own text: the test passes, the suite
is green, and the assertion reads as if it were about the named cause. The only
thing that distinguishes it is what happens when the named cause is removed. So
the sweep has three legs, and only the third is evidence.

**Leg 1, a name-and-assertion scan.** A script walked every `.rs` file outside
`target/`, collected each `#[test]` function with its body by brace counting,
and flagged the ones that assert an error or a success outcome
(`is_err()`, `unwrap_err()`, `expect_err(`, `.is_ok()`) without pinning which
one (no `matches!`, no `Err(Variant::..)`, no `contains(`, no `assert_eq!` on
the error). On main at `afdccf7`, 1304 test functions were collected and 76
flagged, 43 of them with a name in the refusal vocabulary ("refused",
"rejects", "cannot", "is not", "stops", "exclusive",
"only"). A second script did the Python analogue over
`paper/src`, `scripts/` and the package test directories: a `test_*` function
whose failure-shaped assertion (`assertRaises`, `assertFalse`, `assertNotEqual`,
`assertTrue` on a problem list) carries no `assertRaisesRegex`, `assertIn`, or
message comparison. 18 flagged.

This leg produces candidates, never findings. Most of its output is fine: a
fixture with exactly one defect and a positive control beside it isolates its
cause whether or not the assertion names the refusal.

**Leg 2, reading each candidate for cause multiplicity.** For each flagged test,
the question is not "does it pin the error" but "how many independent rules in
the code under test would refuse this fixture". That is answered by reading the
validator, not the test. The productive sub-shape turned out to be a *global*
consistency rule sitting beside *local* per-item rules: a fixture built to
violate one local rule often violates the global one as well, and then the local
rule can be deleted without the test noticing.

**Leg 3, mutation.** The standing mutation gate (`.github/workflows/mutation.yml`)
runs cargo-mutants only over a pull request's own diff and only in the four pure
crates, `sharpebench-core`, `-stats`, `-protocol` and `-attest`. Everything
outside that, and everything inside it that predates the gate, has never been
mutated. All five recorded instances lived in code the gate does not reach. So
the sweep ran cargo-mutants 27.1.0 over whole files rather than diffs, choosing
the surfaces by what a false pass would cost: the money journal and the gateway,
the fault plan and the token rate card, the artifact scan, and the attestation
and board crates that produce published evidence.

One trap is worth recording, because the first run was discarded for it. With
`CARGO_TARGET_DIR` exported to an absolute path, every mutant build writes into
one shared target directory and the test binary that runs is whichever mutant
built last, so the outcomes are meaningless. cargo-mutants must be left to use
its own per-build directories. Its scratch directories are named after this
worktree, so they stay isolated from the sibling repository's agent without the
variable.

A candidate becomes a finding only when the cause the test is named for is
deleted, the test is observed to pass anyway, and the file is restored from
`git show HEAD:<path>` and confirmed with `cmp`. Each fix is then checked by
re-running the same mutation and observing the test fail.

## What was searched

| Surface | How |
|---|---|
| Every `#[test]` in the workspace, 1304 collected | Leg 1 scan, 76 flagged |
| Every `test_*` in `paper/src`, `scripts/`, `crates/sharpebench-py/tests` | Leg 1 scan, 18 flagged |
| `crates/sharpebench-harness/src/gateway.rs`, `gateway_journal.rs` | cargo-mutants, whole file |
| `crates/sharpebench-harness/src/fault_plan.rs`, `accounting.rs`, `artifact_scan.rs`, `artifact_tar.rs` | cargo-mutants, whole file |
| `crates/sharpebench-attest`, `crates/sharpebench-leaderboard` | cargo-mutants, whole package |
| The gateway CLI, the image allowlist, the hardened launch, the decision contract, the run-identity grid, the WASM surface, the provenance manifest, the evidence figures, the release script, the LLM shim and its pricing | Leg 2, read for cause multiplicity |

## Demonstrated findings

### 1. The empty fault plan was refused by the wrong rule

`plan_validation_refuses_malformed_plans`
(`crates/sharpebench-harness/src/fault_plan.rs`) opens with

```rust
let ryw = vec![ContractRelaxation::ReadYourWrites];
assert!(FaultPlan::new(1, ryw.clone(), vec![]).is_err());
```

Two rules refuse that plan. `FaultPlan::new` requires `1..=MAX_FAULTS` faults,
which is the rule the row exists for, and it separately requires the declared
relaxations to be exactly the ones the armed faults use. A plan with no faults
arms nothing, so declaring `ReadYourWrites` violates the second rule too. The
fault plan's digest is folded into the checkpoint invocation identity, so this
is an identity input, not a convenience.

Mutation: delete `faults.is_empty() ||` from the fault-count guard.

```
$ cargo nextest run -p sharpebench-harness plan_validation_refuses_malformed_plans
        PASS [   0.052s] (1/1) sharpebench-harness fault_plan::tests::plan_validation_refuses_malformed_plans
     Summary [   0.053s] 1 test run: 1 passed, 241 skipped

$ cargo nextest run --workspace --exclude xtask
     Summary [  83.349s] 1319 tests run: 1319 passed, 19 skipped
```

The whole workspace is green with a build that accepts a fault plan arming
nothing.

Fix: declare no relaxations, since none are armed, so the declared-versus-armed
rule cannot fire, and assert the diagnostic. After the fix, the same mutation:

```
$ cargo nextest run -p sharpebench-harness plan_validation_refuses_malformed_plans
    thread 'fault_plan::tests::plan_validation_refuses_malformed_plans' panicked at
    crates\sharpebench-harness\src\fault_plan.rs:869:47:
    a plan carries at least one fault: FaultPlan { schema_version:
    "sharpebench.fault-plan.v1", seed: 1, declared_relaxations: [], faults: [] }
     Summary [   0.086s] 1 test run: 0 passed, 1 failed, 241 skipped
```

The other half of the same rule, a plan longer than `MAX_FAULTS`, has no case at
all. That is a coverage gap rather than a false pass and is listed below.

### 2. The published board's terminal receipt could stop being signed

`public_receipt_rejects_terminal_deletion_that_the_chain_accepts`
(`crates/sharpebench-attest/tests/terminal_anchor.rs`) truncates a published
Ed25519 chain and asserts the receipt refuses it.
`verify_chain_receipt_public` refuses on three clauses: the receipt's signature
under the reader's key, the record count, and the terminal signature. A
truncation violates all three at once. The function's own doc comment says as
much: "A truncated chain fails on both counts."

The HMAC twin, `hmac_receipt_cannot_be_restated_for_a_shorter_chain_without_the_key`,
carries the forged-receipt cases that isolate the signature. The Ed25519 half,
which is the one a third party runs against a published board, had none.

Mutation: replace the whole signature check with `true`, so the receipt is no
longer signed at all.

```
$ cargo nextest run -p sharpebench-attest
     Summary [   1.696s] 58 tests run: 58 passed, 0 skipped

$ cargo nextest run --workspace --exclude xtask
     Summary [  80.765s] 1319 tests run: 1319 passed, 19 skipped
```

cargo-mutants found the same hole independently and from the other side: the one
surviving mutant in `sharpebench-attest` was
`public.rs:216:9: replace && with || in verify_chain_receipt_public`, which makes
a receipt verify on its terminal signature alone.

Isolating the terminal clause instead, by replacing
`receipt.terminal_signature == terminal_signature(results)` with `true`, the
named test also passes:

```
$ cargo nextest run -p sharpebench-attest public_receipt_rejects_terminal_deletion_that_the_chain_accepts
        PASS [   0.105s] (1/1) sharpebench-attest::terminal_anchor public_receipt_rejects_terminal_deletion_that_the_chain_accepts
     Summary [   0.105s] 1 test run: 1 passed, 58 skipped
```

Fix: `a_public_receipt_is_refused_clause_by_clause` presents three receipts that
are each wrong in one way. A receipt honestly signed for this chain under
another key, so only the signature can refuse. A receipt whose stated count and
terminal signature were edited after signing to match the chain supplied, so
again only the signature can refuse. And an honestly signed receipt for a
different chain of the same length, so the count agrees, the signature verifies
over exactly what the receipt states, and only the terminal clause can refuse.
Each case carries a positive control: the foreign-key receipt verifies under its
own key, and the sibling receipt verifies against its own chain.

After the fix, all three mutations fail the suite:

```
$ # signature clause replaced with `true`
        FAIL [   1.161s] (51/59) sharpebench-attest::terminal_anchor a_public_receipt_is_refused_clause_by_clause
     Summary [   1.984s] 59 tests run: 58 passed, 1 failed, 0 skipped

$ # `&&` replaced with `||`, the mutant cargo-mutants reported surviving
        FAIL [   2.840s] (55/59) sharpebench-attest::terminal_anchor a_public_receipt_is_refused_clause_by_clause
     Summary [   3.007s] 59 tests run: 58 passed, 1 failed, 0 skipped

$ # terminal-signature clause replaced with `true`
        FAIL [   0.533s] (29/59) sharpebench-attest::terminal_anchor a_public_receipt_is_refused_clause_by_clause
     Summary [   1.460s] 59 tests run: 58 passed, 1 failed, 0 skipped
```

The count clause is deliberately left unisolated, and the test says so. The
signature commits to the pair, and a chain carrying the committed terminal
signature has the committed length, so no honestly signed receipt can state the
right terminal signature and the wrong count. Writing a case that appeared to
cover it would really be testing something else.

### 3. The board's tie marker was asserted on a board where both conditions held

`render_marks_tied_entries_with_a_shared_band`
(`crates/sharpebench-leaderboard/src/lib.rs`) ranks two identical strong agents,
asserts `board.iter().all(|s| s.rank_eligible && s.dsr_tied)`, and then counts
two `=` markers in the rendered board. The marker is printed when
`s.rank_eligible && s.dsr_tied`. Both rows satisfy both conditions, so either
one alone produces the same two markers. The `=` is what tells a reader of a
published board that a row is in a tie band rather than ranked ahead of the
next one.

Mutation: `s.rank_eligible && s.dsr_tied` becomes `s.rank_eligible || s.dsr_tied`.

```
$ cargo nextest run -p sharpebench-leaderboard
     Summary [   0.401s] 13 tests run: 13 passed, 0 skipped

$ cargo nextest run --workspace --exclude xtask
     Summary [  81.576s] 1320 tests run: 1320 passed, 19 skipped
```

Fix: two further boards, each with one row carrying one condition and not the
other, where exactly one marker may appear. After the fix, the same mutation:

```
$ cargo nextest run -p sharpebench-leaderboard
    #    agent                    DSR              DSR CI  tie    elig   raw_ret
    1    a                     1.0000     [1.0000,1.0000]    =    true   0.01003
    -    b                     1.0000     [1.0000,1.0000]    =   false   0.01003
      left: 2
     right: 1
        FAIL [   0.629s] (12/13) sharpebench-leaderboard tests::render_marks_tied_entries_with_a_shared_band
     Summary [   0.633s] 13 tests run: 12 passed, 1 failed, 0 skipped
```

### 4. "The DSR CI column is rendered" was an assertion about the header

`render_and_sign_roundtrip` asserts
`text.contains("DSR CI")` under the comment "the DSR CI column is rendered".
That string is the column header, written by the header `writeln!` whatever the
rows hold. Every row could read `unavailable` and the assertion would still
hold. The interval is the board's statement of how much of its own ranking the
sampling noise supports.

Mutation: delete the `(Some(low), Some(high))` arm, so every row prints the
absence.

```
$ cargo nextest run -p sharpebench-leaderboard
     Summary [   0.782s] 13 tests run: 13 passed, 0 skipped

$ cargo nextest run --workspace --exclude xtask
     Summary [  86.535s] 1320 tests run: 1320 passed, 19 skipped
```

Fix: each row is asserted against the interval its own score carries, and the
absence spelling is asserted against a score whose interval was removed, so each
arm is what its own case turns on. After the fix:

```
$ cargo nextest run -p sharpebench-leaderboard
        FAIL [   0.259s] ( 3/13) sharpebench-leaderboard tests::render_and_sign_roundtrip
     Summary [   0.632s] 13 tests run: 12 passed, 1 failed, 0 skipped
```

### 5. The host's own credential table was not what redacted the credential

`credentials_and_request_bodies_are_redacted`
(`crates/sharpebench-harness/src/gateway.rs`) builds a leak string containing
the configured credential and asserts it is gone. `redact` has two passes: the
exact values the host holds, which is the pass this test is named for, and a
generic scrub of bearer-token-shaped words. The test's credential is
`sk-live-test-do-not-log-0123456789`, which is both, so either pass alone
satisfies the assertion. A credential that does not look like a token would
have leaked and no test would have noticed.

Mutation: `if secret.len() >= 4` becomes `if secret.len() < 4`, which turns the
exact-value pass off for every realistic credential.

```
$ cargo nextest run -p sharpebench-harness credentials_and_request_bodies_are_redacted
        PASS [   0.035s] (1/1) sharpebench-harness gateway::tests::credentials_and_request_bodies_are_redacted
     Summary [   0.036s] 1 test run: 1 passed, 241 skipped

$ cargo nextest run --workspace --exclude xtask
     Summary [  63.940s] 1320 tests run: 1320 passed, 19 skipped
```

Fix: a second route whose credential is `opaque-host-credential-0123456789`,
which no token-shape rule matches, plus the control that the same text under a
gateway not holding that credential comes back unchanged. After the fix:

```
$ cargo nextest run -p sharpebench-harness credentials_and_request_bodies_are_redacted
    thread 'gateway::tests::credentials_and_request_bodies_are_redacted' panicked at
    crates\sharpebench-harness\src\gateway.rs:2265:9:
    provider said: 401 for opaque-host-credential-0123456789
     Summary [   0.055s] 1 test run: 0 passed, 1 failed, 241 skipped
```

### 6. The entrant-facing error text was compared against itself

Two tests assert `error.detail` against `GatewayErrorKind::<Kind>.detail()`,
which is the function that filled the field:
`an_error_to_the_entrant_carries_no_provider_material`
(`crates/sharpebench-harness/src/gateway.rs`) and
`a_budget_refusal_reaches_the_entrant_as_a_typed_error`
(`gateway_serve_tests.rs`). The same value on both sides pins nothing. That
text is the whole of what an entrant learns about a refusal.

Mutation: `GatewayErrorKind::detail` returns `""` for every kind.

```
$ cargo nextest run --workspace --exclude xtask
     Summary [  71.630s] 1320 tests run: 1320 passed, 19 skipped
```

Fix: both comparisons take the literal string. After the fix:

```
$ cargo nextest run -p sharpebench-harness
        FAIL [   1.321s] ( 69/240) sharpebench-harness gateway::tests::an_error_to_the_entrant_carries_no_provider_material
        FAIL [   2.433s] ( 84/240) sharpebench-harness gateway::serve::tests::a_budget_refusal_reaches_the_entrant_as_a_typed_error
```

The other twenty kinds' explanations are still unpinned. That is a coverage gap
and is listed below, not closed here.

### 7. A rate-limit rejection was indistinguishable from any other rejection

`a_rejection_before_work_releases_money_but_consumes_a_call`
(`crates/sharpebench-harness/src/gateway.rs`) answers 429 and then a transport
refusal, and asserts the money outcome plus the final error kind, which comes
from the second attempt. `settle_answer`'s `429` arm and its `400..=499` arm
release the reservation identically and differ only in the kind they return, so
the 429 arm can be deleted and the rejection falls through to the generic one.
`GatewayErrorKind::ProviderRateLimited` was asserted nowhere in the workspace.

Mutation: delete the `429` arm.

```
$ cargo nextest run --workspace --exclude xtask
     Summary [  74.877s] 1320 tests run: 1320 passed, 19 skipped
```

Fix: one more gateway in the same case, a single 429 with no retry, so the kind
the entrant reads is the 429's own. After the fix:

```
$ cargo nextest run -p sharpebench-harness a_rejection_before_work_releases_money_but_consumes_a_call
    assertion `left == right` failed
      left: ProviderUnavailable
     right: ProviderRateLimited
     Summary [   0.028s] 1 test run: 0 passed, 1 failed, 241 skipped
```

## Suspected, not demonstrated

These are cause multiplicities that reading found and mutation did not turn into
findings, either because the redundant clause cannot be isolated at all or
because the test as a whole still kills the mutation.

- **The runtime allowlist's absolute-path clause.**
  `RuntimeAllowlist::from_json` (`crates/sharpebench-cli/src/artifact_preflight.rs`)
  refuses a path on one `if` with several disjuncts, among them
  `path.starts_with('/')` and an empty-segment test over `body.split('/')`. Every
  path that begins with `/` has an empty first segment, so no fixture can make
  the leading-slash disjunct the only thing that refuses, and
  `a_malformed_allowlist_is_refused` cannot isolate it. Recorded as subsumed
  rather than untested. Removing the clause is a production change and is out of
  scope here.
- **The regime label length rule.**
  `regime_rejects_missing_duplicate_and_reordered_periods`
  (`crates/sharpebench-cli/src/csv_columns.rs`) opens with a label file that has
  one row where the returns have two. The row count and the period identities
  both disagree, so the length rule is not isolated. Its three other fixtures
  have the right row count and do isolate the identity rule, which is what the
  test is named for, so the test still kills the mutation that matters.
- **`ModelRoute::alias` in the money journal.** cargo-mutants reports
  `replace ModelRoute::alias -> &str with ""` and `with "xyzzy"` surviving the
  whole harness suite. The accessor's only production caller is
  `journal.reserve(route.alias(), ...)`, which writes the alias into the
  append-only money journal's `Reserved` record. Nothing reads that field back,
  in production or in a test, so every reservation could name the wrong route
  and the audit record would be wrong with no assertion to notice. No test is
  named for it, so this is a coverage gap rather than a false pass, but it is the
  gap with the highest stakes that this sweep found and did not close.

## Coverage gaps the mutation leg surfaced

Not false passes: no test claims these, they are simply unmutated. Listed so the
next round does not have to rediscover them.

- `crates/sharpebench-harness/src/gateway.rs`: `ModelRoute::max_output_tokens`
  has no caller anywhere in the workspace. Dead public accessor.
- `crates/sharpebench-harness/src/accounting.rs`: `MAX_RATE_CARD_BYTES`
  (`64 * 1024`) is never exercised at its boundary, and the
  `TransportDiagnostics::health` accessor on `UsageObservedAgent` is unasserted.
- `crates/sharpebench-harness/src/artifact_scan.rs`: three `>` comparisons in
  `RawScanPolicy::from_json` survive being turned into `==`, so the 64 KiB policy
  size bound and the 256 / 32 rule-count bounds have no case at or over the
  limit.
- `crates/sharpebench-attest/src/registry.rs`: `Registry::key` can be replaced by
  a constant and the crate suite stays green, so nothing establishes that two
  agents, or two windows, key to different registrations. `Registry::current_epoch`
  survives too, but only because its callers are in `sharpebench-arena` and the
  run was scoped to the attest package.
- `crates/sharpebench-leaderboard/src/lib.rs`: `save` and `save_self_describing`
  can both be replaced by `Ok(())` with nothing written to disk.
- `crates/sharpebench-harness/src/fault_plan.rs`: the `faults.len() > MAX_FAULTS`
  half of the fault-count rule has no case, the 64 KiB input bound in
  `from_json` has no case at the limit, and one disjunct of `valid_name` can
  become a conjunction unnoticed.
- `crates/sharpebench-harness/src/gateway.rs`: `GatewayErrorKind::detail` is now
  pinned for two of the twenty-two kinds. The other twenty can go to any text,
  or to the wrong one, with nothing to notice. The gateway chapter publishes
  that vocabulary, so a drift gate against the chapter, of the kind
  `scripts/check-gateway-test-evidence.py` already applies to the test counts,
  would be the right shape rather than twenty more assertions.
- `crates/sharpebench-harness/src/gateway.rs`: `route_index` can return `0` for
  every alias, and `CallPermits::max`, `RouteTable::is_empty` and
  `RouteTable::aliases` can all return constants, with the harness suite green.
  Every fixture in the suite has one route, which is why.
- cargo-mutants also reports the `#[cfg(target_arch = "wasm32")]` arm of
  `fresh_nonce` in `crates/sharpebench-attest/src/sealed.rs` surviving. That code
  is not compiled on this target; the live arm is covered by
  `os_nonce_seals_repeat_plaintext_differently_and_both_open`. It is an artifact
  of mutating both `cfg` arms, not a gap.

## Product defects

None. Every finding here is a test that fails to constrain correct production
code. `FaultPlan::new`, `verify_chain_receipt_public`, `render`, `redact`,
`GatewayErrorKind::detail`, `settle_answer` and `RuntimeAllowlist::from_json`
all behave as documented; what was missing was any case that would notice if
they stopped.

## What this did not cover

- **Two of the three mutation runs were cut short.** `gateway.rs` and
  `gateway_journal.rs` reached 427 of 559 mutants, and the fault-plan group 250
  of 300. The pass was ended on the operator's schedule, not because the runs
  had converged, so the remaining 132 and 50 mutants are unexamined and a
  further false pass could be among them. Both runs are cheap to resume: the
  commands are recorded above.
- **The single-route fixture shape was found and not chased.** Every gateway
  fixture in the suite publishes one route, which is why the alias, the route
  index and the alias list can all be constants. A two-route fixture would
  close several survivors at once and is the obvious next piece of work. It was
  not attempted here.
- **The five surfaces the recorded instances came from were read, not mutated
  end to end.** `gateway.rs` and `gateway_journal.rs` were mutated as far as the
  table above says; the Python shim (`examples/llm-agent/llm_agent.py`
  with `paper/src/test_llm_agent_budget.py` and `test_llm_agent_identity.py`) was
  read for cause multiplicity and not mutated. Those suites already carry
  written-out isolation arguments on the cases that were repaired, and the four
  fixtures spot-checked by reading each vary one thing from a sound control.
- **`crates/sharpebench-core`, `-stats`, `-sim`, `-memory`, `-edge`, `-py`,
  `-wasm` and `-cli` were not mutated.** The pure crates among them are inside
  the standing gate's blast radius for anything a future pull request touches;
  the others are not. `-cli` in particular holds the image allowlist and the
  preflight, which were read but not mutated.
- **`crates/sharpebench-arena/src/sandbox.rs` was read, not mutated.** The
  hardened launch's flag list is pinned literally by
  `docker_present_always_launches_the_exact_container_command`, which is what the
  gateway launch test leans on when it compares against the shared helper, so the
  obvious false-pass shape there is already closed. The live, `#[ignore]` probes
  were not run at all.
- **No Python mutation.** Leg 1 and leg 2 covered the Python tests; no Python
  cause was deleted and re-run. The provenance checker, the evidence figures and
  the pricing tests were read and each of their negative fixtures was traced to a
  single rule, with the diagnostic asserted.
- **No packaged-artifact parity.** The WASM module and the Python wheel were not
  rebuilt. A source-tree test does not establish installed-package parity, and
  nothing in this sweep touches either surface.
- **The 19 `#[ignore]` tests were not considered.** They do not run in the suite
  a false pass would hide in.

## Mutation runs

cargo-mutants 27.1.0, `--test-tool nextest`, `--timeout-multiplier 4`,
`--minimum-test-timeout 120`. The harness runs excluded the
`evidence_fields_no_clone_merges` integration binary, whose two cases take about
a minute and a half between them and concern seed-averaged dispersion rather
than anything under mutation here; that exclusion could in principle hide a
mutant those two cases alone would kill, and every survivor listed above was
confirmed by hand against the full workspace suite.

| Scope | Mutants | Run | Caught | Missed | Unviable |
|---|---|---|---|---|---|
| `sharpebench-harness` `gateway.rs` + `gateway_journal.rs` | 559 | 427 | 363 | 46 | 18 |
| `sharpebench-harness` `fault_plan.rs` + `accounting.rs` + `artifact_scan.rs` + `artifact_tar.rs` | 300 | 250 | 221 | 13 | 16 |
| `sharpebench-attest` + `sharpebench-leaderboard` | 170 | 170 | 136 | 11 | 23 |

The first two runs were stopped before they finished, at 427 of 559 and 250 of
300 mutants, so their survivor lists are a lower bound and the unrun remainder
of both files is uncovered work rather than clean ground. The attest and
leaderboard run completed.

The missed mutants divide as follows. Findings 2, 3, 4, 5 and 7 came from this
leg. Two are suspected items below. The rest are coverage gaps of one of two
kinds: an accessor or constant that nothing asserts, and off-by-one boundary
mutants, `>` turned into `>=` or `==`, in `gateway.rs`'s request and response
bounds, `artifact_scan.rs`'s policy limits, `artifact_tar.rs`'s scan bounds and
`fault_plan.rs`'s input-size bound. Those are all the same missing thing, a case
sitting exactly on the limit, and the paired-boundary gate that would catch them
covers only the kernel crates. None of them is a test passing for the wrong
reason, so none is fixed here.
