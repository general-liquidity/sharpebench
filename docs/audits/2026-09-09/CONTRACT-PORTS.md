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
   Closed later as [F16](#f16-evidence-inventory-gap).
2. **Not yet wired.** The CLI exposes neither re-execution verification nor a
   backoff schedule. `run_resumable_sweep_observed` in `checkpoint.rs` owns its
   own retry loop and still retries immediately; a CLI flag for a schedule must
   also pass it through `BackoffSchedule::bind_invocation`. Since done: see
   "CLI wiring follow-up" below.
3. **Unsealed publications.** The signed board written by `sharpebench sign`
   (`sharpebench_leaderboard::publish`) and the scores persisted in arena window
   state are publications governed by the evidence-coverage digests. They were
   left unsealed: sealing a signed payload changes what the signature covers,
   and the arena crate is outside this change.
4. **MCP.** The server code is unchanged and forwards the npm result; its own
   test suite was not run locally because it needs the MCP SDK installed from
   the registry. CI runs it against the locally built package.

## CLI wiring follow-up (rows 4 and 37)

Built on `a136d14`: the harness driver in `7c019d6`, the CLI in `270fd71`, the
book in `f6cdc7d`. The fault-plan flag built in the same change is recorded in
[FAULT-INJECTION.md](FAULT-INJECTION.md).

**Row 4: `run --retry-backoff <ms,ms,...>`.** `load_retry_backoff`
(`crates/sharpebench-cli/src/main.rs`) parses whole milliseconds, digits only,
each at most 600000, and at most as many entries as the per-round retry budget
(two): a longer list could never be used but would still change the identity.
Every refusal, and use without an external transport, exits 2 before launch.
The schedule is folded into `invocation_sha256` with
`BackoffSchedule::bind_invocation`, after the rate card and the fault plan.
The gap item 2 named is closed in the harness, not worked around: the body of
`run_resumable_sweep_faulted` moved into `run_resumable_sweep_with_backoff`
(`crates/sharpebench-harness/src/checkpoint.rs`), which keeps its own retry
loop and applies the schedule inside it with the record `run_with_backoff`
makes. The scheduled wait is written on the failed attempt's `backoff_after`
and saved before the driver sleeps; retries are numbered within the cell's
round, so a resumed round continues the schedule and a
`--retry-runtime-failures` round starts it again; agent faults never wait. The
unpersisted paths use `run_agent_resilient_faulted` (`lib.rs`), onto which
`run_agent_resilient_with_backoff` now delegates. The totals reach the
existing `attempt_accounting.attempts.backoff_ns_total`. Without the flag the
schedule is immediate, the identity is unchanged and nothing is recorded; the
byte-identity comparison in FAULT-INJECTION.md covers these paths, including
the incomplete-sweep and checkpointed failure paths.

**Row 37: `verify-trajectory --reexecute`.** `run_reexecution` calls
`verify_trajectory_reexecuted` with the running binary's digest and a factory
for a fresh agent per captured run: `--cmd "<prog>"` (host execution, with an
unsandboxed warning), `--http <addr>`, or with neither the reference agent the
trajectory names (`buy-and-hold` or `momentum`). External agents are wrapped so
that the first transport, protocol or spawn failure is recorded; any such
failure exits 1 as `reexecution_transport_failure`, because an HttpAgent that
degrades to a hold would otherwise surface as a divergence and be read as
non-determinism. A divergence exits 1 with `reexecution_diverged` and the typed
`ReexecutionDivergence` on stdout (JSON) and its `Display` on stderr; a strict
refusal exits 1 as before. A pass emits the usual sealed verification with a
`reexecution` object (`agent`, `runs_reexecuted`, `decisions_compared`)
appended after the sealed fields. `--reexecute` with
`--allow-unbound-trajectory`, with a non-reference trajectory and no agent
flag, and `--cmd` or `--http` without `--reexecute` exit 2. Plain
`verify-trajectory` output is byte-identical. Limit as built: `capture`
recorded only the reference agents, so a trajectory of an external entrant had
to come from `sharpebench_harness::run_agent_capture`, and `--image` was not a
re-execution agent. Both closed since; see "External capture and image
re-execution follow-up" below.

**Tests.** Harness (`crates/sharpebench-harness/tests/runtime_recovery.rs`):
`a_checkpointed_backoff_is_saved_before_each_wait_and_restarts_per_round`
(a sleeper that reloads the checkpoint at every wait and requires the wait to
be on disk already; 5 s then 15 s for an exhausted cell, 5 s for a cell that
recovers, the schedule again in a recovery round, totals 25 s then 45 s) and
`an_immediate_checkpoint_schedule_writes_no_backoff`. CLI
(`crates/sharpebench-cli/tests/fault_backoff_reexecution_cli.rs`):
`retry_backoff_waits_are_recorded_and_bound_into_the_checkpoint` (48 attempts,
`backoff_ns_total` 48 ms against no field without the flag; 32 recorded waits
in the checkpoint; a changed schedule and no schedule are refused with the
checkpoint unchanged and no entrant call),
`a_malformed_retry_backoff_refuses_before_launch`,
`reexecution_passes_a_deterministic_entrant_and_refuses_a_divergent_one`
(reference and HTTP passes, a call-counting HTTP entrant refused at run 0 step
1, a broken transport reported as a transport failure) and
`reexecution_flags_refuse_an_unlaunchable_or_contradictory_request`.

**Mutation checks**, broken in place on the committed tree, the named test run,
restored from `git show HEAD:<path>` and confirmed with `cmp`:

| Invariant | Mutation | Killed by |
|---|---|---|
| A changed schedule refuses to resume | `checkpoint_contract` binds the immediate schedule | `retry_backoff_waits_are_recorded_and_bound_into_the_checkpoint` |
| A malformed schedule refuses | the digits-only check removed (`+5` accepted) | `a_malformed_retry_backoff_refuses_before_launch` |
| A schedule longer than the budget refuses | the length check disabled | `a_malformed_retry_backoff_refuses_before_launch` |
| No flag: immediate and unrecorded | an absent flag yields a 1 ms schedule | `retry_backoff_waits_are_recorded_and_bound_into_the_checkpoint`, and the binary comparison (4 outputs differ) |
| No schedule: the checkpoint records nothing | the checkpoint driver records a zero wait | `an_immediate_checkpoint_schedule_writes_no_backoff`, and the binary comparison (the failing checkpoint differs) |
| The wait is saved before the sleep | sleep moved before `save` | `a_checkpointed_backoff_is_saved_before_each_wait_and_restarts_per_round` |
| Retries are numbered per round | always the first delay | `a_checkpointed_backoff_is_saved_before_each_wait_and_restarts_per_round` |
| A divergence is a refusal | exit 0 on `Diverged` | `reexecution_passes_a_deterministic_entrant_and_refuses_a_divergent_one` |
| `--reexecute` re-executes | the flag falls through to strict replay | `reexecution_passes_a_deterministic_entrant_and_refuses_a_divergent_one` |
| A transport failure is not a divergence | the transport-failure check disabled | `reexecution_passes_a_deterministic_entrant_and_refuses_a_divergent_one` (`reexecution_diverged` reported) |

## External capture and image re-execution follow-up

Built on `0dcc4b8`: the capture and `--image` re-execution in `8917c2b`, the
live CI leg in `e95ad00`, after the incomplete-sweep fix in `2442d7f`
([FAULT-INJECTION.md](FAULT-INJECTION.md#incomplete-sweeps-follow-up)). The
row 37 text above is unchanged apart from its closing "Limit" sentence.

**`capture <out.json> --cmd "<prog>" | --http <addr> | --image <ref>`**
(`crates/sharpebench-cli/src/external_capture.rs`, `run_capture_external`).
`run_capture` hands any command line naming a transport to it, so the
reference form is untouched. It calls `sharpebench_harness::run_agent_capture`
with a factory that gives each run a fresh agent: a fresh `ExternalAgent`
process (after the same unsandboxed warning and spawn preflight as `run
--cmd`), a fresh `HttpAgent`, or a fresh container. Each is watched: the first
spawn, transport, protocol or resource failure is recorded, no further agent
is started, and the capture exits 1 with `capture_transport_failure` and writes
no file, because a degraded agent's holds would otherwise be recorded as its
decisions. Exactly one transport, only `<out.json>` as a positional argument,
and no `--scan-policy` are accepted; anything else exits 2.

**How the trajectory identifies the entrant.** The trajectory carries the same
contract a reference capture does (data, costs, engine, runner, windows,
seeds), and its `agent_id` names the entrant by the flag that re-runs it:
`cmd:<command line>` for `--cmd`, `http:<addr>` for `--http`, and
`sandbox:<repository@sha256:...>` for `--image`. The CLI prints the
`verify-trajectory ... --reexecute <flag> <target>` command after a capture
(`reexecute_with` under `--json`). Re-execution still needs the flag: nothing
is launched from a file's contents, and a non-reference trajectory without an
agent flag is refused as before. The pinned reference identifies the image's
bytes; a command line or an address does not identify the artifact behind it
(the reason `run --checkpoint` requires `--entrant-sha256` for those), and the
`SHARPEBENCH_AGENT_ENV` values a `--cmd` entrant receives are not recorded, so
those are re-run under the operator's own control. The protocol schema is
unchanged; binding an entrant digest into `TrajectoryContract` would change the
published wire contract and is not done here.

**`verify-trajectory --reexecute --image <ref>`** (`run_reexecution`, the
`SandboxLauncher` seam in `external_capture.rs`). `DockerSandbox` makes the
calls `run --image` makes for an unscanned image: `resolve_launch` with
`docker_available()` and default `SandboxOptions`, then `require_local_image`,
before anything starts; then `run_external_sandboxed` per captured run, so each
run gets a fresh named container. The container is finished when the harness
drops the run's agent at the end of the run: `SandboxedAgent::finish` reads the
post-exit verdict and removes it. The outcomes are the existing ones: a
refused admission or launch is `reexecution_transport_failure` with
`spawn_error`, a transport fault or an indeterminate verdict or failed cleanup
`transport_error`, an out-of-memory verdict `resource_limit_exceeded` (folded
as `apply_oom_verdict` folds it for `run --image`), and a score-bearing
difference `reexecution_diverged`. After the first failure no container is
started and the failed run's container is not asked again. `--image` without
`--reexecute`, and `--image` beside `--cmd` or `--http`, exit 2. There is no
host fallback.

**Tests.** Daemon-free, through a fake `SandboxLauncher` whose containers
record their launch and finish (`external_capture::tests` in the binary):
`a_sandboxed_capture_names_its_image_and_reexecutes_in_fresh_containers` (16
launches and 16 finishes in launch order for the capture and for each
re-execution, a pass through `run_reexecution`, and a typed divergence when a
different policy sits behind the same reference, every started container
finished), `a_sandbox_failure_is_typed_and_stops_launching` (an out-of-memory
verdict, a failed finalization, a mid-run transport fault and a refused launch,
each with its kind and its exact launch count, for re-execution and capture,
with no trajectory written) and `an_image_the_boundary_refuses_launches_nothing`.
Process level (`fault_backoff_reexecution_cli.rs`):
`capture_records_an_http_entrant_that_reexecution_can_rerun` (the `agent_id`,
`reexecute_with`, strict replay, a `--reexecute --http` pass that calls the
entrant again, the exact human output, and a broken endpoint refused with no
file) and
`external_capture_and_image_reexecution_refuse_contradictory_or_unlaunchable_requests`
(nine exit-2 refusals, an unspawnable `--cmd`, and an unpinned and an absent
pinned image refused for both commands without host execution).

**Live leg.** `live_image_capture_and_reexecution_launch_the_hardened_sandbox`,
`#[ignore]`d, runs by exact name in the "live container boundary (hostile
probe)" job against the digest-pinned Alpine fixture the job already pulls.
The fixture's own entrypoint is `/bin/sh`, which does not speak the decision
protocol, and the CLI deliberately launches an image's own entrypoint, as
`run --image` does. So the live test proves the hardened launch against a real
daemon, the typed `transport_error` for a silent entrant from both commands,
no file written, and no `sharpebench-agent-*` container left behind; the
passing comparison needs a protocol-speaking pinned image, which the job does
not have, and is covered by the fake. The test did not run locally: the local
Docker daemon did not answer `docker version` within 15 seconds.

**Byte identity without the new flags.** The CLI built from `origin/main`
(`0dcc4b8`, from a clean `git archive`) and from this branch ran the same 35
commands, each in its own directory, the baseline twice: `--help`, `run` and
`run --json`, `run --data <csv> --json`, `run --http <fixture>` with and
without `--json` and with `--entrant-sha256 --checkpoint`, the incomplete-sweep
path (`run --http <unframed fixture>`) in both modes with and without a
checkpoint and with `--retry-backoff 1,2`, `run --cmd <reference-agent>` with
and without a checkpoint, `run --image some/agent:latest`, `capture` with no
argument, one argument, an unknown agent, and `momentum` and `buy-and-hold`
(text and `--json`), and `verify-trajectory` with no argument, strict (text and
JSON), `--allow-unbound-trajectory`, `--reexecute` against the reference agent
(text and JSON), `--http` (a passing and a broken endpoint) and `--cmd
<reference-agent>`, and the three existing refusals. Exit codes, stdout,
stderr and all 10 written files (five checkpoints, three captured
trajectories, a renamed copy and the data file) were identical after masking
only host-clock `nanos`, `duration_ns_total` and "observed host duration", and
each binary's own `runner_artifact_sha256`; the two baseline runs differed
only in the host-clock fields. Unmasked, each trajectory differed from the
baseline's in exactly its one `runner_artifact_sha256` line. Every command
but `--help` matched; `--help` differs by two lines, the new `capture
<out.json> --cmd|--http|--image` line and `|--image <ref>` in the
`--reexecute` line. The usage messages of `capture` and `verify-trajectory`
with too few arguments are unchanged on purpose, because both are output
without the new flags.

**Mutation checks**, for this follow-up and the incomplete-sweep fix, broken in
place on the committed tree (`e95ad00`), the named tests run, the file restored
from `git show HEAD:<path>` and confirmed with `cmp` before the next. All 15
mutation runs were killed; the capture's failure check was broken twice, once
against each test.

| Invariant | Mutation | Killed by |
|---|---|---|
| An incomplete faulted sweep keeps its report | the JSON error drops `fault_injection` | `an_incomplete_faulted_sweep_keeps_its_fault_report` |
| No plan: the error is unchanged | the error always carries `fault_injection` (null without a plan) | `an_incomplete_faulted_sweep_keeps_its_fault_report` |
| Human output keeps the report | the incomplete human path skips `print_fault_injection` | `an_incomplete_faulted_sweep_keeps_its_fault_report` |
| A failed capture writes nothing | the capture's failure check disabled (two runs) | `a_sandbox_failure_is_typed_and_stops_launching`; `capture_records_an_http_entrant_that_reexecution_can_rerun` |
| The trajectory names the image | `agent_id` is a fixed string for `--image` | `a_sandboxed_capture_names_its_image_and_reexecutes_in_fresh_containers` |
| The trajectory names the address | `agent_id` is a fixed string for `--http` | `capture_records_an_http_entrant_that_reexecution_can_rerun` |
| Every container is finished | the run's agent is dropped without `finish` | `a_sandboxed_capture_names_its_image_and_reexecutes_in_fresh_containers`, `a_sandbox_failure_is_typed_and_stops_launching` |
| An out-of-memory verdict is a failure | `Some(true)` from `finish` ignored | `a_sandbox_failure_is_typed_and_stops_launching` |
| A failed finalization is a failure | the finalization error not recorded | `a_sandbox_failure_is_typed_and_stops_launching` |
| A mid-run transport fault is a failure | the sandboxed run's health check disabled | `a_sandbox_failure_is_typed_and_stops_launching` |
| Nothing launches after a failure | the factory's short-circuit removed | `a_sandbox_failure_is_typed_and_stops_launching` |
| A refused launch is a failure | the launch error not recorded | `a_sandbox_failure_is_typed_and_stops_launching` |
| The boundary admits before launch | `--reexecute --image` ignores the admission refusal | `an_image_the_boundary_refuses_launches_nothing` |
| `--image` needs `--reexecute` | the check disabled | `external_capture_and_image_reexecution_refuse_contradictory_or_unlaunchable_requests` |

## F16. Evidence inventory gap

**Found.** Open item 1 above, confirmed against the code.
`COMPOSITE_SCORE_INVENTORY` did not declare `selection_error` or
`certification`. Both are `Option` fields with
`skip_serializing_if = "Option::is_none"`, and the drift guard
`every_composite_score_field_is_declared_covered_or_excluded` built its field
list from the serialized keys of probe rows. No probe declared candidates or
selected a rank mode, so neither field was ever serialized, and the guard passed
on `a136d14` with both undeclared.

**What each digest actually covers.** Read from the code, not the inventory's
own description:

- The `agent_score` and `field_context` digests are defined by
  `EvidenceInventory::preimage`, the only supported builder of their bytes: a
  field enters a digest's preimage exactly when the inventory declares it
  `Covered` or `Redacted` for that digest. No production code builds a
  `CompositeScore` preimage today (`grep -rn "\.preimage(" crates/` finds only
  this module's tests), so no computed digest exists that a class change could
  move.
- The signing path that does ship for scored rows, the leaderboard HMAC chain
  (`sharpebench_leaderboard::sign_board`, `publish`, `publish_self_describing`),
  signs `serde_json::to_string` of each whole row. It already signs both fields
  whenever they are present, and nothing here changes it. The inventory does not
  describe that chain; it classifies fields for the content digests, which is
  why `role_contributions` can be excluded there while the chain signs it.

**Decisions.**

- `selection_error`: `Covered { agent_score }`. It is the reason the selection
  diagnostic was withheld, computed from the agent's own candidate set,
  effective trial footprint and dispersion: the inputs of
  `selection_median_dsr` and `selection_gap`, which `agent_score` already
  covers. Its analogues `bootstrap_error` and `deflation_error` are covered by
  `agent_score`, the second added the same way when its probe first produced it
  ([INHERITED-REPAIRS.md](INHERITED-REPAIRS.md)). The `agent_score` preimage now
  emits a `selection_error` record, which the new test asserts. That changes the
  declared preimage layout and no computed signature, because none is computed.
- `certification`: `Excluded`, with its reason in the code. It is a nested
  record whose `withheld` list is variable-length and whose elements are tagged
  variants carrying their own fields: the shape the inventory already excludes
  for `role_contributions`, whose reason is that a flat inventory would go stale
  silently when the element type gains a field. Binding it would first need an
  inventory for the nested record, a change to what a digest covers rather than a
  classification, so it is excluded and no signature changes. It is also a
  second, labeled verdict filled only by `rank_certified`. The HMAC chain still
  signs it whenever present.

**Drift test.** `observed_composite_score_fields` now starts from
`entrant_visibility::declared_struct_fields::<CompositeScore>()`, the field list
serde hands to `deserialize_struct`, which includes fields a
`skip_serializing_if` hides, and adds the probe rows' serialized keys on top.
The new `fields_a_skipped_none_hides_are_still_classified` asserts both fields
are absent from a serialized probe row, present in the observed list, classified
as above, and that `selection_error` is really in the `agent_score` preimage.
`the_two_digests_partition_the_covered_fields` now counts four exclusions.
Commit `c82fbba`.

**Mutations.** Applied in place to the committed file, the core
`evidence_coverage` tests run, the file restored from `git show HEAD:<path>` and
confirmed identical with `cmp`.

| Mutant | Result |
|---|---|
| Drop the `selection_error` declaration | killed: the drift guard (`undeclared: ["selection_error"]`), `a_new_undeclared_field_fails_the_audit` and the new test failed |
| Drop the `certification` declaration | killed: the drift guard, the partition count, `a_new_undeclared_field_fails_the_audit` and the new test failed |
| Build the observed list from probe keys alone, as before | killed: the drift guard (`stale: ["certification", "selection_error"]`), `a_removed_field_is_reported_as_stale`, `a_new_undeclared_field_fails_the_audit` and the new test failed |

**No output moved.** The serialized `CompositeScore` is unchanged, the goldens
pass unchanged, and no scoring, ranking or signing path reads the inventory.
