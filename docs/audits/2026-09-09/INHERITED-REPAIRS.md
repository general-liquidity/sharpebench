# Inherited repairs: call ceiling, dataset selector, unsupported DSR interval

Date: 2026-09-10, with section 4 added 2026-09-11. Scope: defects an
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

Row 4 was found by a later independent review of row 1's repair, and is
recorded here rather than in a new file because it is the same defect class in
the same function: a stated ceiling that the code did not enforce.

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

**What the ceiling now guarantees.** A `LLM_MAX_CALLS` of N permits at most N
HTTP requests to the provider for that model, across every subprocess sharing
the ledger, counting requests that fail. It is an upper bound, not a
prediction: cached decisions and stride holds send nothing.

**Transient failures are explicit, not accidental.** Nothing inside the process
retries. A rate limit, a timeout or a connection fault raises on the first
attempt with its unit already spent, and fails the subprocess. The harness
respawns the shim up to `EXTERNAL_MAX_RETRIES` times, and each respawn reads
the ledger and takes a fresh unit, so a retried window is bounded by the same
allowance as any other call. The module docstring states this as the policy.

**Regression.** `ProviderRequestTests` in `paper/src/test_llm_agent_budget.py`,
two cases. The constructor case records the keyword arguments `build_client`
hands the SDK, so the setting the shim ships is pinned rather than the
constant. The load-bearing case drives `main` with a real
`anthropic.Anthropic` bound to an `httpx.MockTransport`, under a ceiling of
two: the first process is answered 429, the exact status the SDK retries, and
exactly one HTTP request reaches the transport where the old client would have
sent three; the respawn spends exactly one more; the third process is refused
with "budget exhausted" and sends nothing. Two units bought two provider
requests.

**Mutations** (in place, restored from a byte copy of the pre-mutation file and
confirmed with `cmp`): see the section 4 verification table below.

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

**CI.** These regressions ran nowhere: `test_llm_agent_budget.py` and
`test_llm_agent_identity.py` were not in any workflow. A new `llm-shim` job in
`ci.yml` runs both. It installs the SDK, which is why it is its own job rather
than a step in `paper-provenance`, which installs nothing.

**Frozen values.** None moved. The shim makes no provider call in this
repository, and no golden, example or `paper/evidence/` value depends on the
client's retry policy.

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
| `python -m unittest paper/src/test_llm_agent_budget.py paper/src/test_llm_agent_identity.py` (23 tests) | 0 |
| `python -m unittest paper/src/test_provenance.py` | 0 |
| `cargo fmt --all --check` | 0 |
| `python paper/src/check-provenance.py` | 0 |

Mutations, each applied in place to `examples/llm-agent/llm_agent.py`, then
restored from a byte copy taken before the mutation and confirmed identical
with `cmp`:

| Mutation | Result |
|---|---|
| `PROVIDER_MAX_RETRIES` 0 to 2, the SDK default the defect inherited | 2 of 7 fail: the constructor case, and the transport case, which then sees three HTTP requests where the ceiling allowed one |
| ceiling gate back to `len(cache) >= MAX_CALLS` | 3 of 7 fail: the two section 1 cases and the new transport case |
| reservation write made a no-op | 6 of 7 fail (5 failures, 1 error) |
