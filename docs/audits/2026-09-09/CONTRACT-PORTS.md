# Contract ports: Hyper-Tau rows 4, 17, 24 and 37

Implementation record for four accepted rows of
[PORT-RECONCILIATION.md](PORT-RECONCILIATION.md). Each section names what was
built, how it follows the row's decision, where it departs from the row and
why, the tests, and the mutation check. Byte identity, commands and open items
follow at the end.

## Row 17 (take): developer-visible field allowlist seal

**Surfaces found.** Every surface that returns a scored board row to a reader
outside the host returns the same record, `CompositeScore`:

| Surface | Entry point |
|---|---|
| CLI | `score --json` (`emit_board`), `run --json` (`run_board_json`), `arena score --json` (`arena_cmd.rs`), `verify-trajectory --json` (the row nested in `VerificationResult`) |
| WASM | `score_json`, `score_agent_json` and their `wasm_bindgen` exports |
| npm | `score`, `scoreAgent` (parse the WASM output unchanged) |
| MCP | `score`, `score_agent` tools (call the npm functions) |
| Python | `rank_board`, `score_one`, `rank_returns` |

The other entry points on those surfaces (`self_audit`, `greeks`,
`is_my_sharpe_real`, `regime_compare`, `percentile_selection` and the like)
return functions of inputs the caller supplied, so they carry no held-out
detail. `classify_disqualification` already builds its output from three
named fields and is an allowlist by construction. The human table printers
(`print_board`, `sharpebench_leaderboard::render`) print named columns.

**Extend or sibling.** A sibling, `crates/sharpebench-core/src/entrant_visibility.rs`.
`evidence_coverage.rs` answers which digest binds a field; the seal answers
which fields leave the host. The axes are independent (a field can be signed
and withheld, or unsigned and shown), and the coverage inventory is flat by
design (it excludes `role_contributions` because it cannot describe nesting),
while the seal has to reach nested records. The sibling reuses the coverage
module's `InventoryAudit` and drift discipline, and a test requires every
coverage-inventory field to be declared for visibility too.

**What was built.** `seal(value, allowlist)` serializes the record, projects it
onto a `VisibilityAllowlist` and returns an `EntrantView` plus a `SealReport`.
Each field is `Visible` (scalar, string, null or array of those), `Nested`
(an object or array of objects sealed by its own allowlist) or `Withheld`
(with a mandatory reason). Undeclared fields, withheld fields and `Visible`
fields that turn out to contain an object are all removed and reported.
`seal_board` and `seal_score` are the single named seal every surface above
now calls. Order-preserving JSON keeps field order and number bytes, so a
record whose fields are all declared seals to its exact unsealed bytes.
`COMPOSITE_SCORE_VISIBILITY` declares every current field visible, with nested
allowlists for `role_contributions`, `declared_mandate`, `verdict_applied` and
`certification`. `VERIFICATION_RESULT_VISIBILITY` (harness) nests the same
allowlist for `verify-trajectory`. The CLI's `attempt_accounting` and
`artifact_preflight` are attached by name after sealing: they are the
entrant's own operational ledger and image report, attached explicitly rather
than serialized from a struct.

**Tests.** `an_added_field_does_not_change_the_projection` (row test 1: a
record grown by a `held_out_window_dates` field projects to identical bytes)
and `a_planted_secret_in_an_undeclared_field_is_withheld` (row test 2: a secret
planted at the top level, inside a role contribution, inside a certification
gap, and in a scalar that grew into an object never reaches the output). The
drift guard `every_composite_score_field_is_declared` reads the full declared
field list from serde's `deserialize_struct` rather than from one serialized
value, so an optional field hidden by `skip_serializing_if` still fails it
until declared. Surface tests: `sealing_the_golden_boards_reproduces_their_bytes`,
`sealing_a_fully_declared_row_is_byte_identical`,
`crates/sharpebench-wasm/tests/entrant_visibility.rs`, and
`every_verification_result_field_is_declared_and_sealing_keeps_bytes`.

**Mutation.** Keeping an undeclared field instead of withholding it
(`None => Some(value)` in `project`): `an_added_field_does_not_change_the_projection`
and `a_planted_secret_in_an_undeclared_field_is_withheld` fail; restored and
`cmp`-verified against `git show HEAD:`.

**Limit.** The surface tests prove every emitted key is declared and the bytes
are unchanged. They cannot prove at the surface that an undeclared field would
be dropped, because no such field exists on the row today; that property is
proved on the seal itself, and each surface's call to it is a one-line change
visible in commit `b6d887d`.

## Row 37 (take): deterministic re-execution contract

**Where the contract lives.** The protocol crate docs
(`crates/sharpebench-protocol/src/lib.rs`, section "Decisions must be
deterministic under re-execution"), the `description` of
`schema/decision.schema.json`, and the book page `docs/book/src/submitting.md`.

**What an entrant must guarantee.** Each decision is a deterministic function
of the observations of the same run up to and including the one answered and
of the agent's own earlier decisions in that run; no wall clock, no ambient
randomness, no state from another run or from outside the run. The
score-bearing part must repeat: each order's `symbol`, `action`,
`target_weight`, `confidence`, and the `cost` report. `reasoning` and
`rationale` are audit text and are not compared.

**What the harness verifies.** The reconciliation said the property was
already enforced. Reading the code shows it is enforced only for replay of
recorded decisions (`replay_run` is exact by construction), which cannot see a
non-deterministic agent: the recorded decisions replay identically whatever
produced them. So a check was needed to back the published text.
`sharpebench_harness::verify_trajectory_reexecuted` runs
`verify_trajectory_strict` first, then re-executes every captured run with a
fresh agent on the same data, window and seed, and refuses the first
score-bearing difference as a typed `ReexecutionDivergence { run, step,
observation_id, recorded, reexecuted }`. The published text also says what is
not checked: a sweep's own retries and resumes do not compare a rerun against
the attempt it replaced, and the CLI does not yet expose re-execution.

**Tests.** `a_clock_reading_agent_fails_reexecution_with_a_typed_divergence`
(the row's test: the clock is a counter that advances per read, so the test
does not depend on timer resolution; it also shows strict replay alone accepts
the artifact), `state_carried_across_runs_fails_reexecution`,
`a_deterministic_agent_passes_reexecution`,
`free_text_is_not_part_of_the_reexecution_contract`.

**Mutation.** Disabling the comparison (`was_bytes != now_bytes && false`):
the clock and carried-state tests fail; restored and `cmp`-verified.

## Row 24 (adapt): published operation metadata triple

**What was built.** `crates/sharpebench-protocol/src/operations.rs` declares
the operations of the wire contract and derives the triple by one rule:
an operation that does not mutate benchmark state is `safe`, and only a safe
operation has `automatic_retries: allowed`. The decision schema carries the
triple as `x-operation`, `x-mutates-state`, `x-idempotency`,
`x-automatic-retries` annotations at the schema root (`decide`) and at
`$defs/Order` (`rebalance_to_target`).

| Operation | mutates_state | idempotency | automatic_retries | Why |
|---|---|---|---|---|
| `decide` | false | safe | allowed | Answering changes no state; the harness applies the decision separately. The HTTP transport already retries it (`decide_with_retry`), so the entrant must answer a repeated request for the same step identically. |
| `rebalance_to_target` | true | not_guaranteed | forbidden | The engine applies each accepted decision's orders once per step. |

**Departures from the row.** The row allows "one audited exception" as in the
archive. Target-weight orders were the candidate (a target cannot double
exposure), and the exception was declined: under the partial-fill and
participation-cap cost models a second application fills more and pays more,
which is not the same effect as one. With no exception the derivation is total.
The row also says to fold the triple into "the existing contract digest". No
digest over the entrant wire contract exists (searched: `schema.json`,
`protocol_sha`, `contract_sha` across `crates/`; the only schema digest is the
repository provenance snapshot, whose scope includes `crates/**/schema/*.json`).
Adding a field to `TrajectoryContract` would change the bytes of every newly
captured trajectory, which the brief forbids. Instead the table has its own
content identity, `operation_contract_preimage` (canonical JSON v1, framed) and
`sharpebench_harness::operation_contract_sha256`, pinned by
`the_operation_contract_digest_is_pinned` to
`1ed1a71b1a77d69c2c043c73c57cdfaa69cf6a91c791157f1fd818c6116fa99b`. The value
was reproduced independently in Python from the published canonical form.
No schema version was bumped: the annotations are keywords a draft 2020-12
validator ignores, the wire shape is unchanged, and a message that carries an
annotation key is still rejected by `deny_unknown_fields`.

**Tests.** `published_operation_metadata_matches_the_declared_table` (the
annotations sit exactly where the table says, with the derived values, and on
no other schema object), `operation_metadata_is_absent_from_wire_messages`
(absence when unset: the triple never appears on a wire `Decision`, and a
decision carrying `x-idempotency` is rejected),
`the_triple_round_trips_under_its_published_spelling`,
`only_a_safe_operation_may_be_retried_automatically`.

**Mutation.** Publishing `"x-idempotency": "not_guaranteed"` for `decide`:
`published_operation_metadata_matches_the_declared_table` fails; restored and
`cmp`-verified.

## Row 4 (adapt): recorded backoff for provider and transport retries

**Scope.** The row names `decide_with_retry` in `crates/sharpebench-sim/src/transport.rs`.
That file is outside this change's ownership (and `external.rs` beside it is
being edited concurrently), so the schedule is built at the run-level retry
driver in `failure.rs`, which is where the attempt ledger and `AttemptDuration`
the row refers to live. Per-decision HTTP retries in the simulator remain
immediate.

**What was built.** `BackoffSchedule { delays_ns }` (entry `i` precedes retry
`i + 1`, the last entry holds, empty means immediate), a `Sleeper` trait with
`ThreadSleeper`, and `run_with_backoff`. Before each retry of a runtime error
the driver records `Backoff { retry, delay_ns }` on the failed attempt's
`backoff_after` and then waits through the sleeper. The wait is outside the
attempt's `duration`, as the row's threat note requires, and is totalled
separately as `AttemptSummary.backoff_ns_total`. Agent faults never wait.
`run_agent_resilient_with_backoff` exposes it for a sweep. The runtime-versus-
agent classification is unchanged. Nothing in the kernel reads a clock.

**Identity.** A wait can change results against a transiently degraded
endpoint (whether a retry lands after recovery decides which cells complete),
so `BackoffSchedule::bind_invocation` folds a waiting schedule into
`invocation_sha256`. The immediate schedule returns the digest unchanged, so
every existing checkpoint contract stays valid.

**Schema handling.** Following the existing additive pattern
(`AttemptRecord.usage`): `backoff_after` is `#[serde(default,
skip_serializing_if = "Option::is_none")]` and `backoff_ns_total` is skipped
while zero. `SweepContract::SCHEMA_VERSION` is not bumped: a checkpoint written
before the field existed has no backoff, which its absence states correctly.

**Tests.** `a_backoff_schedule_is_followed_exactly_and_recorded` (fake sleeper,
the archive's 5 s then 15 s over four retries: asked for exactly 5, 15, 15, 15;
recorded on attempts 1 to 4, none on the exhausted fifth; total 50 s; attempt
durations exclude it), `a_recovery_stops_the_schedule_and_an_agent_fault_never_waits`,
`the_immediate_schedule_records_nothing_and_keeps_ledger_bytes` (including a
legacy ledger read), `only_a_waiting_schedule_changes_the_invocation_identity`,
`a_resilient_sweep_records_its_backoff_in_the_accounting`.

**Mutation.** Always using the first delay (`delay_before(1)`): both schedule
tests fail; restored and `cmp`-verified.

## Byte identity

No golden, example or `paper/evidence/` file changed. Checks:

- `crates/sharpebench-core/golden/*.scores.json`: `golden_scores` and the WASM
  `native_parity` tests pass unchanged; `sealing_the_golden_boards_reproduces_their_bytes`
  seals both goldens to their exact bytes.
- CLI: `sharpebench score suites/example_submissions.json --json` built from
  this tree is `cmp`-identical to `golden/example_submissions.scores.json`.
  The existing `attempt_metadata_preserves_failed_work_without_changing_the_board`
  test asserts `run_board_json(board, None) == to_value(board)` through the seal.
- Python: the rebuilt wheel passes all 81 binding tests, including the golden
  board comparison.
- npm: `npm/pkg/sharpebench_bg.wasm` was rebuilt with `wasm-pack` and the 20
  npm tests pass against it. The rebuilt module embeds no worktree path.
- Ledgers and accounting: unchanged bytes without a schedule (tested).

## Commands

All run in the worktree with `CARGO_TARGET_DIR` inside it.

| Command | Result |
|---|---|
| `cargo fmt --all --check` | exit 0 |
| `cargo clippy --all-targets --all-features -- -D warnings` | exit 0 |
| `RUSTDOCFLAGS=-Dwarnings cargo doc --workspace --exclude xtask --no-deps` | exit 0 |
| `cargo nextest run --workspace --exclude xtask` | exit 0, 1160 passed, 15 skipped |
| `cargo clippy` and `cargo fmt --check` on `crates/sharpebench-py` | exit 0 (needs a temporary `[workspace]` table in its manifest because this worktree is nested inside another checkout; restored and `cmp`-verified) |
| `maturin build`, install into a venv, `pytest crates/sharpebench-py/tests` | 81 passed |
| `wasm-pack build crates/sharpebench-wasm --target nodejs --out-dir ../../npm/pkg --out-name sharpebench` | exit 0 |
| `npm ci && npm run build && npm test` in `npm/` | exit 0, 20 passed |

## Open items and findings

1. **Evidence inventory gap.** `COMPOSITE_SCORE_INVENTORY` in
   `evidence_coverage.rs` does not declare `selection_error` or
   `certification`, both real serialized fields (`score --rank-mode` emits
   `certification`). Its drift test misses them because its probe never sets
   those options. The visibility drift test does not have that blind spot. Not
   fixed here: whether each is covered or excluded is a digest-policy decision.
2. **Not yet wired.** The CLI exposes neither re-execution verification nor a
   backoff schedule. `run_resumable_sweep_observed` in `checkpoint.rs` owns its
   own retry loop and still retries immediately; a CLI flag for a schedule must
   also pass it through `BackoffSchedule::bind_invocation`.
3. **Unsealed publications.** The signed board written by `sharpebench sign`
   (`sharpebench_leaderboard::publish`) and the scores persisted in arena window
   state are publications governed by the evidence-coverage digests. They were
   left unsealed: sealing a signed payload changes what the signature covers,
   and the arena crate is outside this change.
4. **MCP.** The server code is unchanged and forwards the npm result; its own
   test suite was not run locally because it needs the MCP SDK installed from
   the registry. CI runs it against the locally built package.
