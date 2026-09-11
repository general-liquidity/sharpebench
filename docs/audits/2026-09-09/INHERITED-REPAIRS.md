# Inherited repairs: call ceiling, dataset selector, unsupported DSR interval

Date: 2026-09-10, with sections 4, 5, 6 and 7 added 2026-09-11. Scope: defects an
independent verification confirmed. None of the first three was introduced by
the 2026-09-09 work, but that work leans on all of them: the readiness
preflight made the LLM call ceiling a required explicit setting, added the
local producer's preflight, and the checklist now states that an interval which
cannot be estimated is reported unavailable. Section 4 is the exception: it is
a shortfall in section 1's own repair, found by a later review. Each was
verified against source before it was repaired, and each repair has a
regression that fails without it.

| # | Defect | Repair commit | Regression |
|---|---|---|---|
| 1 | LLM call ceiling counted cached successes, not dispatches | `84643b6` | `paper/src/test_llm_agent_budget.py` |
| 2 | Unknown local dataset selector published an empty field as complete | `a8a9fd6` | `local_open_weight_field_eval.rs` tests |
| 3 | Zero bootstrap support reported as a zero-width DSR interval | `39f98c2` | `significance.rs`, `composite.rs` tests |
| 4 | The repaired ceiling still counted dispatches, not provider requests | this commit | `paper/src/test_llm_agent_budget.py::ProviderRequestTests` |
| 5a | The model-identity check accepted a different policy under the requested name | `a7be8d7` | `test_llm_agent_identity.py::ModelIdentityTests` |
| 5b | An unnamed model was recorded as the requested one | `a7be8d7` | `test_llm_agent_identity.py::ModelIdentityTests` |
| 5c | The ledger count was read once, at process start | `a7be8d7` | `test_llm_agent_budget.py::LedgerOwnershipTests` |
| 5d | A malformed reply's cost was not stored on its record | `a7be8d7` | `test_llm_agent_budget.py::MalformedCostTests` |
| 6a | The retry check was not on the caller-supplied client path | `b91f19a` | `test_llm_agent_budget.py::ProviderRequestTests` |
| 6b | The constructor case errored inside the driver instead of failing its own assertion | `b91f19a` | `test_llm_agent_budget.py::ProviderRequestTests` |
| 7a | An unpriced model reported its calls as free | section 7 | `test_llm_agent_budget.py::ModelPricingTests` |
| 7b | A prefix match billed a model at another model's rate card | section 7 | `test_llm_agent_budget.py::ModelPricingTests` |
| 7c | The assembler that publishes the number carried both | section 7 | `test_llm_agent_budget.py::AssemblerPricingTests` |
| 7d | A replay was screened by a shorter rule than a fresh answer | section 7 | `test_llm_agent_identity.py::CacheIdentityTests` |

Row 4 was found by a later independent review of row 1's repair, and is
recorded here rather than in a new file because it is the same defect class in
the same function: a stated ceiling that the code did not enforce. Section 5
holds three findings reported during that repair and left unfixed, plus one
defect found while fixing them.

## 1. The hosted field's call ceiling

**Confirmed.** `examples/llm-agent/llm_agent.py` guarded fresh calls with
`elif len(cache) >= MAX_CALLS:` while the billable `client.messages.create`
ran further down and the cache was appended only after a usable reply came
back. A provider failure raised before `record_decision`, so the cache stayed
the same size; the harness retries the subprocess (`EXTERNAL_MAX_RETRIES`), and
the retry dispatched again under the same allowance. A field could therefore
spend an unbounded multiple of its declared ceiling.

**Repair.** Each fresh call reserves one unit in a per-model ledger,
`llm-attempts-<model>.jsonl` beside the response cache, written and fsynced
before the request is sent. The ceiling is `attempts >= MAX_CALLS`, where
`attempts` is the ledger line count read at startup plus the reservations this
process made. The ledger survives the process the way the cache does, so the
cap holds across the many subprocesses the harness spawns. Each reservation
records the request key, the requested model and the scaffold version so it can
be read against the cache afterwards. A cache hit reserves nothing. Stats carry
`calls_reserved` next to `llm_calls`.

**Stated limit, since closed.** This repair left retries made inside the
provider client uncounted, and said so: the cap bounded dispatches from this
process, not the HTTP requests the SDK ultimately makes. Section 4 closes that
gap and the admission is gone from the module docstring and `reserve_call`.

**Design decisions.** A separate ledger rather than failed-call records in the
cache, because the cache is the replay record and a failed call has no
decision to replay. `main` takes an optional `client` so the decision loop can
be driven against a stand-in; no provider was called.

**Regression.** `paper/src/test_llm_agent_budget.py`, five cases, driven through
`main` with a stand-in client and a module re-import per simulated process.
The load-bearing one: with a ceiling of 1, a first process whose call fails
spends the unit, and the retried process refuses with "budget exhausted"
without sending anything.

**Mutations** (in place, restored from `git show HEAD:` and verified with `cmp`):

- gate reverted to `len(cache) >= MAX_CALLS`: 2 of 5 fail (the retry and the
  dispatches-not-results cases);
- reservation write made a no-op: 4 of 5 fail (3 failures, 1 error).

## 2. The local dataset selector

**Confirmed.** `crates/sharpebench-harness/examples/local_open_weight_field_eval.rs`
skipped every entry of `DATASETS` the selector did not match, then renamed the
`.partial` output into place unconditionally and printed its record count as
complete. With otherwise valid configuration a misspelled dataset name produced
a successful zero-record artifact.

**Repair.** Two gates, because they fail differently.

- `check_dataset_selector` validates the name against `DATASETS`, the table the
  loop walks, immediately after the configuration preflight: before the dry-run
  report, the shim probe, the output file and the identity directory. The
  diagnostic quotes the refused name and lists the known ones. Exit code 2, as
  for every other configuration refusal.
- `check_records_written` refuses publication when the planned support produced
  no records, which catches a dataset that legitimately scores nothing. The
  empty partial is left in place and named in the diagnostic.

**Regression.** Three tests in the example (a test target via `test = true`):
an unknown selector is refused and names every known dataset; every known
selector and the no-selector form are accepted; a zero-record field is refused
and a one-record field publishes.

**Mutations:** both gate functions forced to `Ok(())`: the unknown-selector and
empty-field tests fail (2 of 15 run). The call sites in `main` are not
exercised by a test, because `main` exits the process and needs a dataset and an
interpreter; the gates are pure functions so the refusal logic itself is pinned.

## 3. The unsupported DSR interval

**Confirmed.** `bootstrap_dsr_ci_against_null` returned
`Ok(DsrConfidence { se: 0.0, lower: point, upper: point })` when `n < 2` or
`n_boot == 0`. Its only caller, `composite.rs`, treats only an error as
unavailability, so this configuration was published as the narrowest interval
the schema can express. That contradicts the checklist claim in
`paper/sections/B-checklist.tex` that an interval which cannot be estimated is
reported unavailable rather than as a numeric no-skill value.

The report layer had the same shape on the existing error path: an interval
error was recorded in `deflation_error`, but `dsr_ci_low`, `dsr_ci_high` and
`dsr_se` were still serialized as numbers pinned to `deflated_sharpe` with zero
width. Checking the serialized report, not only the in-memory value, is what
exposed this.

**Repair.**

- The estimator now calls `bootstrap_inputs`, the same support check
  `bootstrap_pvalue` applies on this resampler, and returns the typed
  `InsufficientObservations { required: 2, .. }` or
  `InvalidParameter { name: "n_boot", .. }`.
- `CompositeScore.dsr_ci_low`, `dsr_ci_high` and `dsr_se` are `Option<f64>`,
  serialized only when estimated, following the withheld-inference shape
  `PairwiseForecastComparison` already uses. `deflation_error` carries the
  reason. `ci_overlap` reports no overlap for an entry without an interval, and
  the leaderboard prints `unavailable` in the DSR CI column.
- `evidence_coverage.rs` declares `deflation_error` (covered by the score
  digest, like `bootstrap_error`). It was already a field of the score, but no
  inventory probe exercised it until the `n_boot = 0` probe began producing it.
- The PyO3 test that pinned the old zero-width behavior now expects the refusal.

**No ranking admission changes.** `bootstrap_pvalue` already refuses both
configurations, and a `bootstrap_error` disqualifies the entry on its own
(`unavailable_bootstrap_is_reported_and_cannot_admit_an_agent`). The new
regressions assert both refusals side by side.

**Regression.** `dsr_ci_without_bootstrap_support_is_unavailable_not_zero_width`
(stats) and `an_unsupported_dsr_interval_is_absent_from_the_report` (core), the
latter asserting on `serde_json::to_value` that the three interval fields are
absent, that `deflation_error` states the reason, and that a supported
configuration still serializes all three as numbers.

**Mutations:** restoring the old early return fails both tests.

**Frozen values.** None moved. Every committed score document carries the
interval fields as numbers, and the golden parity tests (native, WASM and the
installed Python wheel) are byte-identical. Three golden records
(`example_submissions.scores.json` `skilled-momentum` and `ungated-bot`,
`synthetic_field.scores.json` `random`) have `dsr_se = 0` with equal bounds;
those come from a real bootstrap over a valid track whose resampled DSR
saturates at 1.0 or 0.0, carry no error, and are unchanged. The repair covers
only the configuration from which nothing was resampled.

## 4. The ceiling counted dispatches, not provider requests

**Confirmed.** Section 1 made `reserve_call` spend one durable unit before each
`client.messages.create`, which is one dispatch from this process. The client
was a bare `anthropic.Anthropic()`, so `max_retries` took the SDK default. In
the installed SDK (`anthropic` 0.112.0) that default is
`_constants.DEFAULT_MAX_RETRIES = 2`, and `_base_client._should_retry` retries
408, 409, 429, any 5xx, and, through `_should_retry_exception`,
`APIConnectionError` and `APITimeoutError`. One reservation therefore covered
up to three billable HTTP requests, and a run could exceed the ceiling its
operator set by up to a factor of three. The `reserve_call` docstring admitted
it rather than enforcing it. No provider call was made to confirm this: the
SDK source at the installed version is the evidence, and the regression below
observes the behaviour through a stand-in transport.

**Option chosen: disable the client's internal retries.** `PROVIDER_MAX_RETRIES
= 0`, applied in a new `build_client()` that `main` uses. The alternative,
hooking the transport so each underlying HTTP request takes a reservation, was
rejected on two grounds. It accounts for overspend instead of preventing it: a
transport hook learns of the second request as it goes out, and refusing there
raises from inside the SDK's retry loop, which the scaffold would then have to
classify. And it buys nothing, because the reservation exists to bound what the
provider is asked to do, and with retries off the two quantities are already
the same number. The SDK supports the setting directly and documents it: its
own constructor error says "If you want to disable retries, pass `0`".

**What the ceiling now guarantees, stated with its condition.** Provided the
constructed client reports an effective retry setting of zero, a
`LLM_MAX_CALLS` of N permits at most N HTTP requests to the provider for that
model, across every subprocess sharing the ledger, counting requests that fail.
It is an upper bound, not a prediction: cached decisions and stride holds send
nothing.

The condition is not decoration, and it is not a claim about an SDK version.
`assert_no_provider_retries` reads the setting back off the object the run will
use, before the first observation is read. If it is present and not zero, or
cannot be read at all, the run raises and the subprocess fails, and the harness
records a transport failure rather than publishing a field. So the honest form
of the guarantee is: either the ceiling bounds provider requests, or the run
does not start. It is never the case that the run proceeds with the property
unverified. `EVIDENCED_SDK_VERSION` in the shim records which SDK the property
was established against, for a reader, not as a runtime requirement: a later
version that still honours the knob passes on its behaviour.

**Transient failures are explicit, not accidental.** Nothing inside the process
retries. A rate limit, a timeout or a connection fault raises on the first
attempt with its unit already spent, and fails the subprocess. The harness
respawns the shim up to `EXTERNAL_MAX_RETRIES` times, and each respawn reads
the ledger and takes a fresh unit, so a retried window is bounded by the same
allowance as any other call. The module docstring states this as the policy.

**Regression.** `ProviderRequestTests` in `paper/src/test_llm_agent_budget.py`,
two cases. The constructor case drives `main` with no client of its own, the
way the harness runs it, and records the keyword arguments the SDK constructor
receives, so what is pinned is the client the run actually builds rather than
the constant or `build_client` in isolation. The load-bearing case drives
`main` with a real
`anthropic.Anthropic` bound to an `httpx.MockTransport`, under a ceiling of
two: the first process is answered 429, the exact status the SDK retries, and
exactly one HTTP request reaches the transport where the old client would have
sent three; the respawn spends exactly one more; the third process is refused
with "budget exhausted" and sends nothing. Two units bought two provider
requests. What that case asserts is `anthropic` 0.112.0's behaviour, which is
why the job pins it.

Two further cases drive the runtime check's refusal rather than its happy path,
because a check that is only ever shown passing cannot be told apart from one
sitting in a helper nothing calls. A stand-in that accepts `max_retries` and
reports two, the shape of a future SDK that keeps the keyword and drops its
effect, must refuse before anything is reserved; so must one that exposes no
readable setting. Both stand-ins answer requests normally, so a run that is not
refused completes and leaves a reservation on the ledger, which is what makes
the deletion mutant below fail for the right reason.

**Mutations.** Six, in the section 4 verification table below, each run against
the nine cases the file now holds. Two are worth naming here, because both
survived a first version of these tests and the tests were changed, not the
prose. Reverting `main` to a bare `anthropic.Anthropic()` survived because every
case either called `build_client()` directly or supplied its own client, so the
repair was pinned everywhere except on the path the harness takes. And deleting
the runtime check was at first caught only by an incidental `AttributeError`,
because the stand-in clients could not dispatch; giving them a working
`messages.create` turned that into the real signal, a run that completed under
a retrying client with nothing refusing it.

**Also audited, not changed.** Three claims in the same file were checked and
hold as written: the cache-identity gate (`load_cache`) drops a record whose
stored digest is not its own key, which is what its docstring claims; a cache
hit reserves nothing; `llm_calls` and `calls_reserved` now describe their real
relationship (they are equal within one process, because the unit is reserved
immediately before each dispatch). Two are reported rather than repaired.
`effective_model` accepts any served id with the requested id as a prefix,
which is looser than the "versioned expansion" its docstring describes. And the
ledger count is read at startup and advanced in memory, so concurrent
subprocesses sharing one ledger would each start from the same base; the field
producer (`crates/sharpebench-harness/examples/llm_field_eval.rs`) walks
datasets and models in a sequential `for` loop and `run_external_agent` spawns
one shim at a time, so the ceiling holds for the only caller, and
`reserve_call` now names the assumption instead of leaving it implicit.

**CI, and why the SDK is pinned.** These regressions ran nowhere:
`test_llm_agent_budget.py` and `test_llm_agent_identity.py` were not in any
workflow. A new `llm-shim` job in `ci.yml` runs both. It installs the SDK,
which is why it is its own job rather than a step in `paper-provenance`, which
installs nothing.

The install is `anthropic==0.112.0`, not a bare `pip install anthropic`. The
first version of this job floated, and on PR #80 it resolved to anthropic
1.5.0, which depends on httpx2 rather than httpx; the load-bearing test then
failed at import with `ModuleNotFoundError: No module named 'httpx'`. That is
the shallow symptom. The real problem is that the test asserts a specific SDK's
retry policy, so a floating install exercises whichever major version is newest
that day rather than the one the reading in this section was taken from, and
the claim and the dependency have to travel together. Nothing else in the
repository pins `anthropic`; this job is the only pin, and the shim's README
still tells an operator to `pip install anthropic` unpinned, which the runtime
check above is what makes safe. anthropic 1.x is a different SDK and its retry
semantics are **not** assumed from the 0.x reading. Establishing them, and
deciding whether the shim should support 1.x, is separate work with its own
evidence.

**Frozen values.** None moved. The shim makes no provider call in this
repository, and no golden, example or `paper/evidence/` value depends on the
client's retry policy.

## 5. Three reported findings, and one found while repairing them

Section 4 reported two of these as audited and not changed, and a third came
with them. All three were verified against source before anything changed.

### 5a. The identity check accepted a different policy

**Confirmed.** `effective_model` returned the served id whenever
`served.startswith(REQUESTED_MODEL)`, so a run requesting `claude-opus-5`
accepted `claude-opus-5-mini`: a different policy answering under the requested
name, which the function's own docstring calls the one thing this benchmark
must not publish. The intent stated there is narrower, a provider expanding an
alias into the pinned version it served, and prefix acceptance is not that.

**The rule, and where it comes from.** The served id is the requested policy
when it is the requested id exactly, or the requested id followed by one hyphen
and a dated snapshot of exactly eight ASCII digits. That is read off what the
provider returns rather than invented: the `Message.model` literal in the
pinned SDK enumerates the aliases and their pinned ids, and every pair in it is
the alias plus `-` plus eight digits.

```
$ python -c "import anthropic; from anthropic.types import Message; \
    print(anthropic.__version__); print(Message.model_fields['model'])"
0.112.0
annotation=Union[Literal['claude-fable-5', ..., 'claude-haiku-4-5',
 'claude-haiku-4-5-20251001', 'claude-opus-4-5', 'claude-opus-4-5-20251101',
 'claude-sonnet-4-5', 'claude-sonnet-4-5-20250929', 'claude-opus-4-1',
 'claude-opus-4-1-20250805'], str] required=True
```

The four alias/pinned pairs there are `claude-haiku-4-5` ->
`claude-haiku-4-5-20251001`, `claude-opus-4-5` -> `claude-opus-4-5-20251101`,
`claude-sonnet-4-5` -> `claude-sonnet-4-5-20250929` and `claude-opus-4-1` ->
`claude-opus-4-1-20250805`. Nothing else is appended, so `-mini` is refused,
and so is a snapshot of seven or nine digits, one with a trailing word, one
with no separator and one that is not all digits.

The rule is deliberately narrower than the provider's whole namespace. Two
deprecated aliases rebind rather than expand (`claude-sonnet-4-0` is served as
`claude-sonnet-4-20250514`, which is not the alias plus a suffix at all) and
this refuses them; prefix acceptance refused them too. Refusing a policy that
is arguably the requested one costs a run, while accepting one that is not
publishes the wrong identity. The Vertex form of the same expansion uses `@`
rather than `-`; the shim runs against the first-party API, which does not, and
admitting a separator it never returns would only widen the hole.

**Elsewhere.** Nothing else accepts a model id by prefix. The Rust field runner
(`crates/sharpebench-harness/examples/llm_field_eval.rs`) does not compare
served ids at all: it hands the requested id to the shim as `argv[1]` and
records it as both requested and effective, because the shim refuses any gap.
`grep -rn starts_with --include=*.rs crates/` returns no model or policy
comparison anywhere, the Arena included. Two prefix matches on model ids remain
and are not identity checks: `price_for` and `request_kwargs` in the shim, and
`price_for` in `paper/evidence/assemble_llm_field.py`, select a pricing family
and a request shape. They now only ever see an id the identity check admitted.

### 5b. A reply naming no model was recorded as the requested one

**Confirmed, and it now refuses.** `effective_model` returned `REQUESTED_MODEL`
when the response carried no model, which states that the requested policy
answered on the strength of the API not having said so. That is the
accepting-on-absence shape the retry guard refuses.

Absence is not normal for this API: the command above shows `Message.model` as
`required=True` in the pinned SDK, so a parsed response always carries one.
Fail closed, therefore, rather than record an unverified identity: the run
raises and the field is incomplete, which the driver already refuses to
publish. Recording it as unverified was the alternative, and it is the wrong
one here, because the value would still be written into a cached decision that
a replay reports as the policy that answered.

### 5c. The ledger count was read once, at process start

**Confirmed.** `reserve_call` advanced a count read at import. That holds only
while nothing else writes the ledger, which was true of the one caller and
named in the docstring as an assumption rather than enforced. Two shims sharing
a ledger would each start from the same base and each believe the same unit was
free.

**Repair.** Both halves of the suggestion, because neither alone is enough. The
count is re-read from the ledger at reservation time, and the read and the
append happen under exclusive ownership of the ledger: a sibling
`llm-attempts-<model>.jsonl.lock` created with `O_CREAT | O_EXCL`, the shape
`crates/sharpebench-harness/src/gateway_journal.rs` uses for the money journal.
Re-reading alone leaves check-then-act between the read and the append; the
lock alone leaves a stale startup count. A second shim reserving at that moment
is refused with a typed `LedgerBusy` naming the holder and the lock file, and a
lock left behind by a killed shim is refused rather than broken, for the reason
the journal gives: a lock nobody can prove is dead puts two writers back on one
allowance. The ceiling itself moved into the reservation, which is the only
place the count is current; the loop's stale pre-check is gone.

The lock is held across one reservation rather than for the run, so a shim the
harness kills (it kills them at the end of a run, which is why statistics are
rewritten after every decision) can strand a lock for one reservation rather
than for a whole field. Two hosts sharing one directory over a network file
system are still not separated, exactly as recorded for the journal.

### 5d. A malformed reply's cost was not stored

**Confirmed.** The refusal and success records carried a `cost` block; the
malformed record carried `tokens_in` and `tokens_out` but no cost, so replaying
that decision reported a billed call as free. It now carries the same block.

The replayed wire decision still carries no cost, and cannot: a replayed
malformed decision has to stay unparseable so the transport records an agent
protocol fault, so nothing on it would be read as accounting. The cost lives on
the cache record, which is what the accounting reads, and the code says so
where a reader would otherwise wonder. `paper/evidence/assemble_llm_field.py`
priced malformed calls correctly before and after, because it prices the tokens
on the record rather than the cost field, so no published number moves.

### 5e. Releasing the new lock stranded it on Windows

**Found while repairing 5c, by the regression for it.** A refused shim reads
the holder document to name it, and Windows refuses to delete a file another
handle holds. Eight contending threads reproduced it on the first run: the
winner reserved its unit and then failed to release, with `PermissionError:
[WinError 32] The process cannot access the file because it is being used by
another process`. A stranded lock refuses every later reservation, which would
have taken the field down rather than overspent it. The release now retries for
up to a second and raises a typed `LedgerLockStranded` naming the file if it
still cannot, rather than leaving one behind quietly. Acquiring treats
`PermissionError` as busy for the same reason: on Windows a lock whose holder
is deleting it is refused as access denied rather than as already existing.

**Frozen values.** None moved. The shim makes no provider call in this
repository, and no golden, example or `paper/evidence/` value depends on which
ids the identity check admits, on the ledger's locking, or on a field the
assembler does not read.

## 6. The retry check was not on the caller-supplied client path

Two findings from the [accounting review](ACCOUNTING-REVIEW.md), A4 and A5, both
about the section 4 repair rather than about money that moved. Neither is
reachable from the field producer, which spawns `python llm_agent.py` and so
enters through `main()` with no client.

### 6a. A supplied client reached the provider unchecked

**Confirmed.** `assert_no_provider_retries` ran inside `build_client`, and
`main` called `build_client` only when no client was supplied. A client handed
to `main` reached `client.messages.create` with its retry policy never read.
What the project publishes is not "the client the shim builds has retries off"
but "either the ceiling bounds provider requests, or the run does not start",
and on that path the second half was untrue. The suite made it worse by using
that path for
`test_n_units_allow_exactly_n_provider_requests_across_a_retryable_failure`, so
the case that demonstrates the ceiling was the one case running where the check
was not.

**Repair.** `main` calls `assert_no_provider_retries` on whatever client it will
use, supplied or built. `build_client` keeps its own call.

**Checked, not refused, and why.** Refusing a supplied client outright was the
other option the review names. It was rejected because the guarantee is a
property of the client's behaviour, not of its provenance: a client that reports
zero will send one request per `create` whoever constructed it, and one that
reports two will not, which is exactly what the check reads. The concrete cost
of refusing is that the load-bearing case cannot exist. That case observes the
SDK's own retry behaviour by driving a real `anthropic.Anthropic` over an
`httpx.MockTransport`, which has to be supplied, and refusing supplied clients
would force it back onto `build_client` or onto a private path, which is the
shape this repair removes. Checking instead means that case now enters through
the checked path and passes the check, so the ceiling is demonstrated on the
same path the guarantee is stated for.

Keeping the check in `build_client` as well is deliberate rather than
redundancy left in place. The helper is importable and states a property of what
it returns, and `main` cannot cover a caller that uses the returned client
directly. Each call site has one case that fails when that call alone is
deleted, so neither can rot.

**What the guarantee now covers.** Every path from this module to
`client.messages.create` passes the check before the first observation is read:
`main()` with no client, `main(client=...)` with any client, and `build_client`
used on its own. What it does not cover is a caller that imports `call_model` or
constructs its own client and dispatches without going through `main` at all;
nothing in this repository does, and the ceiling's reservation is not on that
path either.

**Regression.** `test_a_client_the_caller_supplies_is_checked_like_one_the_run_builds`
drives `main` with a supplied `Ignoring`, the stand-in that accepts
`max_retries` and reports two. Four causes could produce a refusal there without
the check on that path, and each is excluded rather than assumed:

| Cause that could also refuse | How it is excluded |
|---|---|
| The stand-in is too thin to dispatch | `Ignoring` answers normally, and the second half of the case supplies a compliant client of the same shape, which completes the run and dispatches one request |
| `main` built a client of its own after all | The SDK constructor is replaced by one that fails the test if it is called |
| The allowance was already spent | The ledger is asserted empty, and the message asserted is the check's, naming `max_retries=2` |
| The supplied-client path refuses whatever it is handed | The compliant control is supplied through the same parameter and is not refused |

`test_build_client_refuses_to_return_a_client_that_would_retry` pins the
helper's own call: the stand-in constructor returns normally, so nothing but the
call inside `build_client` can raise.

### 6b. The constructor case did not reach its own assertion

**Confirmed.** `test_the_client_the_run_uses_disables_the_sdk_automatic_retries`
names the keyword `build_client` passes its constructor. Removing that keyword
made the recording stand-in report `max_retries=None`, the runtime check refused
the run, and the case ended as an `ERROR` raised inside `drive` with its
assertion never evaluated. The regression gate held in aggregate; the case did
not demonstrate what it names.

**Repair.** The recording constructor sets the returned stand-in's effective
setting to `PROVIDER_MAX_RETRIES` whatever keyword it was built with, so the
check cannot be what refuses and the recorded keyword is the only thing that can
answer the assertion. Reading the constant rather than writing a literal keeps
that true under a mutation of the constant: at 2 the check still passes and the
case fails on its own assertion, 2 against the literal 0 it asserts.

**Consequence for the other cases.** Because `main` now checks every client it
is handed, a stand-in that reports no setting is refused before the decision
loop runs. The default stand-in in both shim suites therefore reports the
compliant setting, and the three classes that vary it keep doing so
deliberately. Without that, every ceiling, ledger and model-identity case would
have failed on the check rather than on the thing it names, which is the failure
mode this section is about.

**Frozen values.** None moved. The shim makes no provider call in this
repository, and no golden, example or `paper/evidence/` value depends on the
client's retry policy or on where the check is called from.

**Also audited, not changed.** Two claims in the same file were looked at for
the same shape, a published guarantee with a path that reaches the guarded
effect without passing the guard.

- **The model-identity rule is enforced where a decision is written and not
  where one is replayed.** `effective_model` refuses a served id that is not the
  requested policy, and every cached record carries `model_effective`.
  `load_cache` screens a record on three identity fields, `scaffold_version`,
  `request_sha256` equal to its own key, and `model_requested`, and not on
  `model_effective`, so a record naming a served model this scaffold would have
  refused is replayed rather than dropped. This scaffold cannot write such a
  record, so it takes a cache file from somewhere else, and the digest still has
  to match a request for the requested model, which bounds what the replayed
  decision can be. It is reported rather than repaired: it is the same shape as
  A4, a check on the writing path and not on the reading one, and worth a
  decision rather than a silent fix. **Decided and repaired in section 7.**
- **`price_for` falls back to `(0.0, 0.0)` for a model not in `PRICING`.** A run
  on an unpriced model reports its calls as free rather than refusing. This is a
  fail-open default rather than a bypassed check, so it is a different shape, but
  it is the other place in the file where a stated property (recorded cost
  describes the call) does not hold on every input. **Decided and repaired in
  section 7**, which also found the prefix walk above that fallback and the
  same pair in the assembler that publishes the number.

## Verification

Run from the worktree on the committed tree with an isolated
`CARGO_TARGET_DIR`:

| Command | Exit |
|---|---|
| `cargo fmt --all --check` | 0 |
| `cargo clippy --all-targets --all-features -- -D warnings` | 0 |
| `RUSTDOCFLAGS=-Dwarnings cargo doc --workspace --exclude xtask --no-deps` | 0 |
| `cargo nextest run --workspace --exclude xtask` (1122 passed, 15 skipped) | 0 |
| `python -m unittest paper.src.test_llm_agent_budget paper.src.test_llm_agent_identity` (21) | 0 |
| `maturin build --release` of `crates/sharpebench-py` from the committed tree, wheel installed into a venv, `pytest crates/sharpebench-py/tests` (81 passed) | 0 |

## Verification, section 4 (2026-09-11)

Section 4 changes Python and prose only. No Rust file was touched, so the
workspace legs above are unaffected; `cargo fmt --all --check` was run to
confirm it.

| Command | Exit |
|---|---|
| `python -m unittest paper/src/test_llm_agent_budget.py paper/src/test_llm_agent_identity.py` (25 tests) | 0 |
| `python -m unittest paper/src/test_provenance.py` | 0 |
| `cargo fmt --all --check` | 0 |
| `python paper/src/check-provenance.py` | 0 |

Mutations, each applied in place to `examples/llm-agent/llm_agent.py`, then
restored from a byte copy taken before the mutation and confirmed identical
with `cmp`:

| Mutation | Result |
|---|---|
| `PROVIDER_MAX_RETRIES` 0 to 2, the SDK default the defect inherited | 3 of 9 fail. The transport case is the informative one: the SDK retried the 429, the second attempt was answered, and no failure was raised at all, which is the defect exactly. Two billable requests, one reserved unit, a green run |
| `main` reverted to a bare `anthropic.Anthropic()`, `build_client` left defined but unused | 3 of 9 fail. It survived the first version of these tests: the constructor case called `build_client()` directly and the transport case supplied its own client, so nothing pinned the client the run builds. The case now drives `main` with no client of its own |
| ceiling gate back to `len(cache) >= MAX_CALLS` | 3 of 9 fail: the two section 1 cases and the transport case |
| reservation write made a no-op | 6 of 9 fail (5 failures, 1 error) |
| `assert_no_provider_retries(client)` deleted from `build_client`, the function left defined | 2 of 9 fail, both `AssertionError: RuntimeError not raised`. The two stand-in clients answer normally, so with nothing refusing, the run dispatched under a client reporting `max_retries=2` and under one reporting none at all. An earlier version of the stand-ins had no `messages.create`, and the same mutant was caught by an incidental `AttributeError`; that proved only that the stand-in was thin, so they were given a working `create` and the mutant re-run |
| the unreadable branch made fail-open (`getattr(client, "max_retries", PROVIDER_MAX_RETRIES)`) | 1 of 9 fails, the "cannot be read" case. The two branches of the check are pinned separately |

Each mutation was applied to the working file, the suite run, and the file
restored from `git show HEAD:examples/llm-agent/llm_agent.py` written to a
temporary path, then confirmed byte-identical with `cmp` and an empty `git
diff` before the next mutation.

## Verification, section 5 (2026-09-11)

Section 5 changes Python and prose only. No Rust file was touched.

| Command | Exit |
|---|---|
| `python -m unittest paper/src/test_llm_agent_budget.py paper/src/test_llm_agent_identity.py` (38 tests, run ten times) | 0 |
| `python -m unittest paper/src/test_provenance.py` | 0 |
| `python paper/src/check-provenance.py` | 0 |

Mutations, each applied in place to `examples/llm-agent/llm_agent.py`, the
suite run, then the file restored from `git show
HEAD:examples/llm-agent/llm_agent.py` and confirmed byte-identical with `cmp`
before the next one:

| Mutation | Result |
|---|---|
| the snapshot rule back to `served.startswith(REQUESTED_MODEL)` | 8 of 38 fail. The refused-continuation case, six of the seven shapes the rule rejects, and the on-path case where the run is driven with a reply that is usable apart from its identity |
| absence back to `return REQUESTED_MODEL` | 2 of 38 fail, the unit case and the on-path case |
| the ceiling inside the reservation made unreachable | 4 of 38 fail: the two section 1 cases, the transport case, and the new one where another shim spent the last unit after this process read the ledger |
| `O_EXCL` dropped from the lock, everything else intact | 2 of 38 fail, both ledger-ownership cases, in six runs out of six |
| the malformed record's `cost` removed | 1 of 38 fails |
| the release retry removed (first `PermissionError` raises) | 1 of 38 fails, as an error carrying `LedgerLockStranded`, in three runs out of three |

The exclusivity case took two attempts, and the first one is the finding worth
recording. It released eight reservations together against a ceiling of one and
asserted that exactly one came back with a unit. That assertion has three
causes: the lock, the ceiling refusing a thread that arrives after the winner
released, and threads that simply do not interleave. Against a build with
`O_EXCL` removed it failed in four runs out of six and passed in two, and
against the build with the ceiling removed it passed, so it was not pinning
either. It is replaced by a deterministic case with the allowance set to eight,
where nothing is short of budget, a reservation is attempted while the test
holds the lock, and the same reservation then succeeds once the lock is
released. That leaves exclusive ownership as the only thing that can refuse the
first, and it catches the mutant every run.

Dropping the threaded case then left the release retry (5e) unpinned: the
mutant that removes it passed the whole suite, because nothing else contends
for the lock file. The case that pins it holds a reader open across the release
and closes it on a timer, so the retry rather than the timing is what is
exercised. On a platform where an open file can be unlinked, the first attempt
succeeds and the case asserts the same end state.

**Not established.** No provider was called and no shim ran concurrently
outside one process on one file system. The identity rule is evidenced against
`anthropic` 0.112.0's declared response type and the ids it enumerates; a later
SDK or a new alias family that expands some other way would be refused rather
than misread, but the rule would need re-reading. `O_EXCL` over a network file
system is only as exclusive as the remote server makes it.

## Verification, section 6 (2026-09-11)

Section 6 changes Python and prose only. No Rust file was touched, so
`cargo fmt` and the workspace suites are unaffected and were not re-run for it.
The installed SDK is `anthropic` 0.112.0, the version the `llm-shim` job pins.

| Command | Exit |
|---|---|
| `python -m unittest paper/src/test_llm_agent_budget.py paper/src/test_llm_agent_identity.py` (40 tests, run five times) | 0 |
| `python -m unittest paper/src/test_provenance.py` | 0 |
| `python paper/src/check-provenance.py` | 0 |

Mutations, each applied in place, the suite run, then the file restored from
`git show HEAD:<path>` and confirmed byte-identical with `cmp` before the next
one. The suite is the 18 cases of `test_llm_agent_budget.py`; the identity file
was run alongside the first mutation and is unaffected by all of them.

| Mutation | Result |
|---|---|
| `assert_no_provider_retries(client)` deleted from `main` | 1 of 18 fails, the caller-supplied case, as `AssertionError: RuntimeError not raised` on its own `assertRaises`. The 22 identity cases still pass, so nothing incidental is carrying it |
| `assert_no_provider_retries(client)` deleted from `build_client` | 1 of 18 fails, the helper's case, on the same `assertRaises`. A different case from the one above, so neither call site is pinned by the other |
| `max_retries=PROVIDER_MAX_RETRIES` removed from the constructor call | 1 of 18 fails, the constructor case, as `AssertionError: None != 0` at `self.assertEqual(seen[0].get("max_retries"), 0)`. Before 6b it was an `ERROR` raised inside `drive`, with that line never reached |
| the same, with 6b's one line also reverted in the test | 1 of 18 errors, `RuntimeError ... does not report a readable max_retries (got None)` raised at `build_client`, inside `drive`. This is the finding's own observation, reproduced, and it is what 6b changes |

The first two mutations are the point of 6a: the guarantee is now pinned on the
path the caller takes and on the helper separately, and deleting either call is
caught by the case named for that path rather than by a case that happens to run
through it.

**Not established.** No provider was called. What the check reads is an
attribute on the client object, so a future SDK that reports zero and retries
anyway would pass it; that is the same limit the check has always had, and the
load-bearing case is what observes real behaviour, for `anthropic` 0.112.0 only.
A caller that bypasses `main` entirely, by importing `call_model` or dispatching
on a client of its own, is outside what any of this covers.

## 7. Two fail-open paths, both found while repairing the third

A8 and A9 from the [accounting review](ACCOUNTING-REVIEW.md), reported at the
end of section 6 and left for a decision rather than fixed silently. Neither is
a bypassed check: both are stated properties that do not hold on every input,
which is the shape this round has been closing.

### 7a. An unpriced model reported its calls as free

**Confirmed.** `price_for` returned `(0.0, 0.0)` for a model no entry of
`PRICING` matched. A run on such a model priced every call at nothing,
`STATS["cost_usd"]` stayed 0.0, and that zero was what the field published as
its spend. Nothing anywhere said the number was a fallback rather than a
measurement.

**Refusal, and why not an unavailability.** Two candidates were weighed.

The Rust side never answers an unknowable cost with a number: a journal or an
attempt ledger that cannot be priced produces `MonetarySummary::Unavailable`
with a `reason` and, where one exists, a separately labelled `known_subtotal`
(`crates/sharpebench-harness/src/accounting.rs`, `gateway_journal.rs`). The
consistent-looking move is to give the shim the same shape. It was rejected on
what the shim can actually express. Its output is a statistics file with one
`cost_usd` float, summed by `paper/evidence/assemble_llm_field.py` across
processes; there is no status field, no reason and no place for a labelled
subtotal, and adding that vocabulary to a stats file two scripts read is a
larger change than the defect warrants. More to the point, an unavailability is
for a cost that could not be established after the fact. This one is knowable
before any money moves: the operator names the model, the table is a literal in
the same file, and the mismatch is visible at startup. What the Rust convention
says is "never publish a number you cannot establish", and refusing the run
satisfies that more completely than recording an absence would, because no field
is produced at all.

So: refusal, raised as a typed `UnpricedModel`, and established in `main`
through `assert_model_is_priced` beside `assert_no_provider_retries`, before the
first observation is read. `price_for` refuses on its own path too rather than
relying on the startup check having run.

**Consistency with the Rust side.** No third convention is invented. This shim
states one of the two things the Rust side states, "no number without a rate
card", by the strongest available means; it does not emit a differently shaped
unavailability record.

### 7b. A prefix match could select the wrong rate card

**Confirmed, and it could.** The walk accepted any continuation of a table key,
so a model whose name extends a priced one was billed at the other model's card:
`claude-opus-5-1` prices as `claude-opus-5`, at half the input rate and half the
output rate, silently. `claude-opus-5-mini` and `claude-opus-50` do the same.
The table's three keys do not collide with each other today, so nothing is
mispriced right now, but the exposure is to any future model name and to the
table gaining a shorter key: adding `claude-haiku-4` would price every
`claude-haiku-4-5` at whichever key `dict` iteration reached first.

This is the model-identity defect (5a) in the accounting, in the file that
repaired it. The repair is the same rule: a model matches a card when it equals
the alias, or is that alias followed by one hyphen and a dated snapshot of
exactly eight digits, which is what `is_dated_snapshot_of` already states.
`price_for` calls that function rather than restating it, so narrowing the
identity rule narrows the pricing match with it.

### 7c. The assembler carried both

**Confirmed.** `paper/evidence/assemble_llm_field.py` had its own `PRICING`
table, its own prefix walk and its own `(0.0, 0.0)` fallback, and it is the
script that writes the published `cost_usd`. With 7a in place this scaffold can
no longer produce a response cache for an unpriced model, but the assembler
prices by cache file name and does not cross-check those names against the
models it requires, so a stray or hand-placed `llm-cache-<model>.jsonl` was
still assembled at zero.

Repaired the same way, as a `SystemExit`, which is how every other
incompleteness in that script refuses. The rule is restated there rather than
imported: importing the shim would pull the Anthropic SDK into an assembler that
reads only files. The two tables must agree, and that is now a stated
requirement rather than an accident, but it is a duplication and it is recorded
as one.

### 7d. A replay was screened by a shorter rule than a fresh answer

**Confirmed.** `effective_model` refuses a served id that is not the requested
policy, and `record_decision` stamps `model_effective` on every record, but
`load_cache` screened on `scaffold_version`, `request_sha256` equal to its own
key and `model_requested`, and not on `model_effective`. A record naming a
served model this scaffold would refuse today was replayed rather than dropped.

**Bounded, and closed anyway.** This scaffold cannot write such a record, so the
case needs a foreign or hand-edited cache file, and the request digest must
still match a request for the requested model. A replayed decision is published
exactly as a fresh one is, so a replay should be screened by the rule that
governs a fresh answer.

**The same rule, not a second copy.** The acceptance test moved into
`is_requested_policy`, which `effective_model` and `load_cache` both call.
`effective_model` keeps its two distinct refusals, the unverifiable-identity one
for an absent id and the substitution one for a wrong id, and delegates only the
acceptance decision. A record whose `model_effective` is null or absent is
dropped too, which is the same absence `effective_model` refuses. Writing the
rule out a second time inside `load_cache` is a mutation the suite catches.

**Frozen values.** None moved, and none could. The LLM field has never
completed: `git ls-files` tracks no `llm-cache-*.jsonl`, no
`llm-attempts-*.jsonl`, no `stats-*.json` and no `llm-field*` artifact; the
assembler's output `paper/evidence/final/llm-field.jsonl` does not exist;
`.gitignore` excludes every one of those paths and
`paper/evidence/provenance.json` excludes the `llm-cache-` and `llm-field-`
prefixes from result provenance. No committed evidence file, in
`paper/evidence/`, `arena/`, `suites/` or `data/`, contains a `claude-` model id
or a `cost_usd` value, and no `.tex` source states an LLM cost, token count or
field result. `paper/sections/07-limitations.tex` says so directly: "No current
model field has completed or produced an admitted performance artifact." So no
committed evidence or example was produced with a model absent from the table,
because none was produced with any model at all. The two files that mention
pricing are hashed in `provenance.json` as **source**, not as artifacts, so the
manifest rebinds and no artifact digest changes.

## Verification, section 7 (2026-09-11)

Section 7 changes Python and prose only. No Rust file was touched, so the
workspace suites and `cargo fmt` are unaffected and were not re-run for it. The
installed SDK is `anthropic` 0.112.0, the version the `llm-shim` job pins; the
pin is unchanged.

| Command | Exit |
|---|---|
| `python -m unittest paper/src/test_llm_agent_budget.py paper/src/test_llm_agent_identity.py` (52 tests) | 0 |
| `python -m unittest paper/src/test_provenance.py` | 0 |
| `python -m unittest paper/src/test_sweep_grid.py` | 0 |
| `python paper/src/check-provenance.py` | 0 |

Mutations were applied in an isolated copy of the four files under the session
scratchpad, never in the worktree, and each file was restored from the
pre-mutation copy and confirmed byte-identical with `cmp` before the next
mutation. The worktree copies were compared against those originals afterwards
and are identical.

Each refusal names the causes that could also satisfy its assertion, and each is
excluded rather than assumed.

**The unpriced-model refusal.** Four other causes could raise from that run: the
retry guard, a stand-in too thin to dispatch, an exhausted allowance, and the
identity rule. The stand-in reports the compliant retry setting and answers
under the requested id, so neither of those can fire; the allowance is two for
one observation and the ledger is asserted empty afterwards; and a control case
drives the same stand-in class, the same allowance and the same observation
under a priced model to a decision. The isolating assertion is
`client.requests == []`: the refusal lands before any dispatch, which no cause
further down the loop can produce.

| Mutation | Observed |
|---|---|
| `price_for`'s refusal reverted to `return (0.0, 0.0)` | 6 of 26 fail in the budget suite, all in `ModelPricingTests`, each as `AssertionError: UnpricedModel not raised`. Nothing errors, so no case is carried by an incidental failure |
| the match reverted to `model.startswith(alias)`, the refusal kept | 5 of 26 fail: the four extending-name subcases and the shared-rule case. The unpriced-model case still passes, which separates the two defects |
| `assert_model_is_priced()` deleted from `main` | 1 of 26 fails, and on the isolating assertion rather than on the refusal: `Lists differ: [{'model': 'claude-not-a-model-9', ...}] != []`, with the message "the refusal precedes every dispatch". The run still refuses, further down, after a provider request has been made. This is what shows the check is on the path the run takes and at the point claimed |
| `SNAPSHOT_DIGITS` narrowed to 6 (applied by the test to the module, not to the file) | `price_for("claude-haiku-4-5-20251001")` refuses, so the pricing match really is the identity rule and not a copy of it |

**The assembler refusal.** Four gates in that script can exit non-zero on the
same fixture: the empty-records check, the model set, the dataset set and the
incompleteness check. Each states its own reason, so the assertion is on the
pricing refusal's message rather than on the exit code, and a priced control
reaches the later gates.

| Mutation | Observed |
|---|---|
| the assembler's prefix walk and zero fallback restored | 2 of 3 `AssemblerPricingTests` fail. The script still exits 1, for the later gate instead: `AssertionError: 'no rate card for claude-not-a-model-9' not found in "refusing to assemble: models ['claude-not-a-model-9']; required [...]"`. Exit code alone would not have told the two apart |

**The replay screen.** Three other clauses of the screen could empty the cache:
the scaffold version, the digest, and the requested model. The record is built
by `record_decision` itself, so all three are correct by construction and only
the served identity differs. A control record whose served id is a dated
snapshot of the requested alias is still replayed, so the screen is not simply
rejecting everything.

| Mutation | Observed |
|---|---|
| the `model_effective` clause deleted from `load_cache` | 4 of 26 fail in the identity suite: the refused-served-model case, both absence subcases and the shared-rule case, each as the cache being non-empty |
| the clause replaced by a second, inline copy of the same rule | 1 of 26 fails, `test_the_replay_screen_is_the_rule_the_fresh_path_uses`, and only that one. The drift the repair is meant to prevent is caught by the case named for it |

One pre-existing case needed its own isolation restored.
`test_a_record_whose_digest_is_not_its_key_is_not_replayed` wrote a fixture with
no `model_effective`, which the new clause also rejects, so it would have passed
with the digest clause deleted. The fixture now carries a valid served id, and
the digest is again the only clause it can fail.

**Not established.** No provider was called and no field was run, so nothing
here says what a real run costs. The rate card values themselves are unverified
against a price list: this repair changes which card is selected and what
happens when none is, not whether the numbers in the table are right. The two
pricing tables, in the shim and in the assembler, are still separate literals
that a future edit could desynchronize; the duplication is stated rather than
prevented. And the refusal is at startup on the requested model: it rests on
`effective_model` binding the served id to the requested one, which is argued
from that function rather than observed against a provider.
