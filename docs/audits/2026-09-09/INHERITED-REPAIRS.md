# Inherited repairs: call ceiling, dataset selector, unsupported DSR interval

Date: 2026-09-10. Scope: three defects an independent verification confirmed.
None was introduced by the 2026-09-09 work, but that work leans on all three:
the readiness preflight made the LLM call ceiling a required explicit setting,
added the local producer's preflight, and the checklist now states that an
interval which cannot be estimated is reported unavailable. Each was verified
against source before it was repaired, and each repair has a regression that
fails without it.

| # | Defect | Repair commit | Regression |
|---|---|---|---|
| 1 | LLM call ceiling counted cached successes, not dispatches | `84643b6` | `paper/src/test_llm_agent_budget.py` |
| 2 | Unknown local dataset selector published an empty field as complete | `a8a9fd6` | `local_open_weight_field_eval.rs` tests |
| 3 | Zero bootstrap support reported as a zero-width DSR interval | `39f98c2` | `significance.rs`, `composite.rs` tests |

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

**Stated limit.** Retries made inside the provider client are extra billable
requests under one reservation and cannot be counted from the shim. The module
docstring and `reserve_call` say so: the cap bounds dispatches from this
process, not the HTTP requests the SDK ultimately makes.

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
