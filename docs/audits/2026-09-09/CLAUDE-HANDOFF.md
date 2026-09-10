# Claude handoff: Sharpe suite verification and completion

Snapshot date: 2026-09-10. This document is mirrored in both repositories.

The user requested a detailed handoff so Claude can finish the remaining work.
This is not a completion report. Stop relying on the long conversation as the
task ledger: start here, then read the linked findings and verification records.
Repository state below was checked before this handoff commit; fetch and inspect
again before acting. A green draft branch does not mean its feature is finished.

## 1. Objective and governing records

Finish the independent review of Claude's work, the end-to-end Hyper-Tau review,
the accepted recoverable implementations, and the resulting product and paper
documentation. Implement confirmed, scoped defects with meaningful regressions.
Do not expand indefinitely into speculative features in pursuit of perfection.

Read these records in order:

1. The repository's `AGENTS.md` and `CLAUDE.md`.
2. [Active implementation ledger](IMPLEMENTATION.md).
3. [Independent findings](AUDIT.md).
4. [Verification diary](VERIFICATION.md).
5. [Hyper-Tau coverage and assessment](HYPER-TAU-REVIEW.md).
6. [Artifact preflight checklist](ARTIFACT-PREFLIGHT.md).

The [2026-09-07 audit](../2026-09-07/IMPLEMENTATION.md) is historical and complete.
Do not reopen its old checklist or rewrite its historical evidence.

### Hard boundaries

- No releases, tags, force pushes, history rewrites, stashing or destructive cleanup.
- No local model downloads, provider calls, new fields or market-data acquisition.
- Synthetic fixtures, finite diagnostics and installed-package checks are allowed.
- Published numerical paper evidence stays frozen. Changed producer behavior must
  be explained, not silently substituted into an old experiment.
- No provider credentials or personal operational context in product docs or papers.
- Commit author: `Tiberiu Toca <tibi.toca@gmail.com>`; conventional prefixes;
  no `Co-Authored-By` trailers. Stage explicit paths, never `git add -A`.
- No em dashes in new Markdown or paper prose.
- Preserve unrelated branches, worktrees and user edits. Grep before declaring gaps.
- Kernels remain deterministic; no I/O, ambient randomness or system clock.
- A regression needs an isolated mutation check, not merely a green control.
- Source tests do not establish installed wheel or WASM/npm behavior.
- Only merge normally after relevant checks pass on the exact pushed head.
- Keep this handoff and the shared audit records mirrored as work proceeds.

## 2. Recoverable repository state

These are pre-handoff snapshots, not promises about a later checkout.

| Item | SharpeBench | SharpeArena |
|---|---|---|
| Fetched origin/main | `855afbc1d5f31514960b7f107ab4cfc07d98f8c5` | `00a3d569c180e6e620dda2049420415efd469a58` |
| Recoverable draft head | `b61f8fb331a096be2708fc48014bde3f5277726c` | `416f78d6c5fd91194e62fb60e380c158c3866a6c` |
| Draft remote branch | `feat/artifact-preflight-2026-09-10` | `docs/scanner-progress-2026-09-10` |
| Open draft PR | [#44](https://github.com/general-liquidity/sharpebench/pull/44) | [#40](https://github.com/general-liquidity/sharpearena/pull/40) |
| Local handoff branch | `docs/claude-goal-handoff-2026-09-10` | `docs/claude-goal-handoff-2026-09-10` |
| Last checked draft CI | CI, mutation and npm successful | CI successful |

Bench draft workflows: `34410125816`, `34410125822`, `34410125846`.
Arena draft workflow: `34410127055`. These runs cover the pushed TAR-reader
stage and its documentation, not the lost Docker integration described below.
The handoff commits require their own checks after being pushed.

The local handoff branch tracks the corresponding draft remote branch. The
original local main branches were stale; do not confuse them with origin/main.
Both handoff branches started clean from the recoverable draft heads.

### Temporary worktree loss: critical recovery fact

The previous temporary directory `/tmp/sharpe-suite-goal.uWKPc7` disappeared
during an environment reset. Its registrations can remain in Git even when the
directories no longer exist. Temporary tool installations, venvs, build targets,
test sessions and in-memory source snapshots are unavailable too.

The Docker/CLI preflight integration was NEVER COMMITTED OR PUSHED. Reconstruct
it from section 6; do not claim it exists because prototype test output appeared
in the conversation. The raw scanner and TAR reader ARE pushed and recoverable.

Missing registrations included Bench `bench`, `scanner`, `verification` and
Arena `arena` under that temporary root. Bench also has an older locked Claude
worktree associated with `bedbfb8`. Do not remove or unlock it without checking
ownership and work. A stale lock is not evidence of a currently running agent.
No implementation needs to be recovered by deleting branches or worktrees.

## 3. Completed work: inspect, but do not rebuild from scratch

### Merged independent repairs

F01 through F13 are merged. Details and mutation evidence remain in AUDIT.md
and VERIFICATION.md; the following is an orientation, not substitute evidence.

| Finding | Delivered behavior |
|---|---|
| F01 | Non-finite computed field statistics withhold the whole snooping family. |
| F02 | Both forecast verifiers join sealed identities, contracts and revisions. |
| F03 | Effective seed width comes from validated keys; contradictory flags fail. |
| F04 | CSV columns retain their own observed date axes rather than false pairing. |
| F05 | Unsupported seed-bootstrap intervals fail at Rust and Python boundaries. |
| F06 | Unavailable baseline scores remain unavailable, without numeric ranking. |
| F07 | Missing or overflowing candidate utilities withhold the field. |
| F08 | Prospective v2 imports enforce digest encoding labels. |
| F09 | Internal numeric settlement identity is canonicalized. |
| F10 | Statistical refusal reasons and rollup labels survive reporting. |
| F11 | npm retains error fields and nullable statistical diagnostics. |
| F12 | Stale committed WASM was rebuilt and installed-package behavior pinned. |
| F13 | Checked deflation rejects invalid intermediate quantities before floors or saturation. |

Bench PR #40 merged at `4b3cc0d`, retaining PR #39's compatibility-pin history.
Its final mutation run: 128 tested, 125 caught, three unviable, no misses or
timeouts. This is finite mutation evidence, not a formal or coverage proof.
Arena repairs merged through #35 at `f7614dc`; later documentation mirrors
merged through #39 at the origin/main snapshot in section 2.

Independent SPA fixtures pin ordinary and consistent variants separately. A
three-column, 16-observation dyadic fixture with seed 7 and 127 draws gives
35/128 and 27/128; scaling a column by 2^-27 gives 75/128 and 58/128. A seed-2,
seven-draw case gives 7/8 and 6/8. These are implementation-reference checks;
the floating log/square-root threshold still is not a nominal coverage proof.

### G04: special-function compatibility pins

PR #39 is merged by ancestry. It did not add statrs. The existing 93 exact bit
pins and NaN contract protect the current approximation's outputs. Compatibility
and mathematical accuracy are different properties. G10 remains open.

### G06 and G09: recovery and attempt visibility

Bench PR #41 merged at `4a453d0`, with exact-head and post-main CI successful.
`ResumePolicy::UnfinishedOnly` is the default; `RetryRuntimeFailures` is explicit.
There are at most three additional recovery rounds per cell, persisted budgets,
and append-only attempts. Completed and agent-fault cells remain untouched.
CLI `--retry-runtime-failures` requires external execution and a checkpoint.
Actual CLI fixtures exercised 48 initial attempts, unchanged default resume,
and 96 after explicit recovery; incomplete support still produces no board.
Six isolated mutations were caught.

Attempt summaries are exposed on success and incomplete-sweep errors. Duration
is host-observed; the summaries do not change scoring or eligibility.

### G08: frozen rate cards

Bench PR #42 merged at `630183a`, exact-head and post-main CI successful.
Primary implementation: `crates/sharpebench-harness/src/accounting.rs`, the
checkpoint/failure modules, and `crates/sharpebench-cli/src/main.rs`.
Tests: harness `frozen_rate_card.rs` and CLI `frozen_rate_card_cli.rs`.

Schema: `sharpebench.token-rate-card.v1`. Provider/model/revision and integer
USD-nanodollar rates are validated from bounded 64 KiB JSON. Quoting uses checked
u128 arithmetic; integer monetary values are serialized as strings. Reasoning
tokens are a subset of output tokens, not charged a second time.
Missing, all-zero, dollar-only or inconsistent usage cannot become complete cost.
Failed and recovered attempts retain observations. The checkpoint binds the card.
Legacy ranking cost is unchanged and explicitly distinct from rate-priced cost.

A fixture with 240 decisions, three input/five output/four reasoning tokens
quotes 690000 nanodollars, ignores an entrant's claimed USD 999, and resumes
without new calls. A changed card fails before calls. Board values other than
telemetry remain equal. Seven harness tests, three CLI tests and isolated
mutations passed. A macOS socket fixture was corrected to disable inherited
nonblocking mode while retaining its read timeout.

These estimates are still entrant-reported. Legacy omitted counts defaulting to
zero cannot independently prove completeness or model identity. G11 owns that gap.

### G13: installed checks already completed

The fresh Arena wheel passed 91 affected tests. The rebuilt Bench WASM passed
20 npm tests, offline tarball checks and nine MCP checks. Exact-head CI included
packaged consumers. Do not rerun this old verification as a substitute for testing
new changes; it establishes only the repaired state recorded in the diary.

Arena still consumes exact registry Bench dependencies at `=0.19.0`. Later
Bench main repairs do not reach that dependency without a future authorized
release and Arena pin bump. Do not change the pin to an unpublished version or
leave a local Cargo patch while claiming registry parity.

## 4. Remaining ledger and dependencies

| ID | Remaining acceptance | Depends on |
|---|---|---|
| G01 | Complete independent review of changed source, tests, APIs and claims. | Existing findings plus current diff |
| G02 | Finish documented end-to-end Hyper-Tau coverage, not just inventory. | Section 7 |
| G03 | Reconcile every proposed port with existing consumers and justified disposition. | G02 and code searches |
| G04 | Already merged; preserve compatibility evidence. | None |
| G05 | Close only confirmed remaining defects with red/green and mutation evidence. | G01, G03 |
| G06 | Already merged; preserve recovery invariants. | None |
| G07 | Finish exact-image/config capture, CLI refusal and live/installed verification. | Section 6 |
| G08 | Already merged; keep self-reported cost labelled. | None |
| G09 | Already merged; preserve rank-neutral attempt records. | None |
| G10 | Resolve statrs migration with independent accuracy and evidence-impact treatment. | Section 8 |
| G11 | Implement bounded host-mediated provider accounting, hermetically tested. | G08, section 9 |
| G12 | Validate field readiness and refusal only; no empirical run. | G11 where applicable |
| G13 | Repeat affected installed/parity checks after new changes. | G05, G07, G10, G11 |
| G14 | Finish product/onboarding docs against final implemented behavior. | Code acceptance |
| G15 | Update papers without replacing frozen numerical evidence. | Code and claim audit |
| G16 | Push, verify, merge normally, verify main; reconcile all remaining rows. | Each delivered change |

Do G07 recovery first, while G02/G03 can be read-only independent work.
Do not create competing writers in the same files. The user should not have to
wait for an entire paper rewrite before recoverable code is safely committed.

## 5. G07: the code that actually exists

Recoverable Bench commits include `e11b31a` (byte engine), `e24bcf5` (TAR
reader), documentation, and provenance through `b61f8fb`.

Existing files:

- `crates/sharpebench-harness/src/artifact_scan.rs`
- `crates/sharpebench-harness/src/artifact_tar.rs`
- `crates/sharpebench-harness/tests/artifact_scan.rs`
- `crates/sharpebench-harness/tests/artifact_tar.rs`

`RawScanPolicy` uses schema `sharpebench.raw-scan-policy.v1`, private validated
fields and a 64 KiB policy bound. Whole-file lowercase SHA-256 rules are bounded
at 256; exact UTF-8 sequences at 32, each 16..4096 bytes. No debug representation
should disclose protected values. Policy identity hashes validated serialization.

Default limits: 32768 streams, 64 MiB/file, 512 MiB total, 60 seconds.
Maximum permitted limits: 100000 streams, 512 MiB/file, 4 GiB total, 120 seconds.
Per-file limit cannot exceed total. Match storage is bounded at 256.
The KMP matcher spans 8192-byte read boundaries. One extra byte detects growth.
Read errors, duplicate stream names, empty scope, size mismatches and expired
limits cannot yield a clean result. Reports disclose rule index and name hash,
not filenames or needles. Inventory identity is withheld on incomplete scans.
The caller must still bound blocking reads and enumerate the actual scope.

`scan_tar_snapshot` returns `TarScanReport`, with scope
`raw-tar-headers-and-entry-payloads/v1`, archive bytes/hash and raw scan report.
It never extracts. Headers and bodies are separate synthetic named streams.
Repeated paths and concatenated archives are scanned, including padding in
aggregate size/hash. Links are bytes, not host filesystem operations.
GNU long-name/link and local PAX metadata are bounded before allocation.
Global PAX, sparse formats, size overrides, unsupported entry types, malformed
records, duplicate or dangling extensions are refused.

Each component has nine integration tests, one unit test and thirteen caught
isolated mutations. Final recorded harness result was 108 passes and two ignored
tests. Formatting, clippy, rustdoc and exact-head CI passed.
This establishes archive-reader behavior, NOT deployed image preflight.

## 6. G07: reconstruct the lost Docker/CLI integration

All paths in this section are proposed unless already present after a fresh
inspection. None of this section's prototype code is in the snapshot above.

Proposed files: `crates/sharpebench-cli/src/artifact_preflight.rs`,
`crates/sharpebench-cli/tests/artifact_preflight_cli.rs`, CLI main wiring,
the CLI manifest/lockfile edge for tempfile, and a live-container workflow step.

### Required launch contract

1. Add opt-in `run --image <repo@sha256:...> --scan-policy <policy.json>`.
2. Reject policy-without-image, multiple transport choices, malformed or oversized
   policy, and unsupported transport combinations before Docker calls.
3. Resolve the pinned local image using Docker image inspect. Validate full
   lowercase sha256 configuration ID, Linux OS and expected Config shape.
4. Treat Docker and its daemon as trusted infrastructure. Do not let an entrant
   supply an arbitrary Docker executable or provider endpoint.
5. Reject declared image volumes because container export omits their contents.
   Check real Docker behavior for omitted versus null versus empty Volumes;
   the prototype's assumptions need validation, not blind reproduction.
6. Scan serialized Config AND decoded strings/keys. Escaped newlines or Unicode
   must not hide protected content from a raw serialized-JSON search.
7. Create a uniquely named stopped container from the validated image ID. Never
   start its entrypoint to obtain the snapshot.
8. Inspect the created container: same Image ID, created/non-running state, no
   mounts. Refuse any contradictory or incomplete scope.
9. Export to an owned bounded capture, then call the non-extracting TAR reader.
10. Attempt checked removal on every path, including uncertain creation and
    export failure. Cleanup failure or uncertainty cannot authorize launch.
11. Launch only the exact validated configuration ID, not a mutable alias.
12. Bind policy digest, image identity and scan scope into checkpoint identity.

A candidate create invocation preserves `--pull=never --network=none --ipc=none`,
read-only root, dropped capabilities and no-new-privileges; its entrypoint can
be an intentionally absent preflight marker since it must never start.
Review existing hardened launch helpers before duplicating configuration.

The existing sandbox launcher accepts strict repository digests. The prototype
internally enabled its unpinned-image option only after validating a full config
ID. Prefer a typed trusted-ID boundary if possible; never allow that exception
to turn arbitrary user input into an unpinned launch.

### Capture, deadlines and cleanup

The prototype used null stdin, anonymous temporary stdout/stderr files, child
polling and explicit seek-to-start before parsing. Proposed accepted output caps:
2 MiB inspect JSON, 1024 bytes create/remove response, policy total for export,
64 KiB stderr. Read inspect output with a cap before allocating/parsing JSON.
Check exit status and final sizes, kill and reap failed captures, and redact
untrusted stderr rather than printing it wholesale.

Polling spool sizes is an accepted-output bound, NOT a hard disk quota. Output
can overshoot between polls. A blocked OS read and Docker CLI descendants also
need explicit treatment; a wall-clock check cannot magically interrupt them.
Review remote Docker contexts, inherited environment, late descendant writes
and daemon-side operations after client termination. State limitations honestly.

Use one overall policy deadline for capture and scan, with a separate bounded
cleanup allowance (the prototype used ten seconds). Attempt removal by the
reserved unique name even if creation returns an uncertain failure. Do not
reinterpret every nonzero remove result as already-clean success.

### Report, invocation binding and a known unfixed bug

Proposed report schema: `sharpebench.image-preflight.v1`, image_id,
configuration RawScanReport, optional filesystem TarScanReport, cleanup_verified.
Authorization requires completed negative config and filesystem scans AND
verified cleanup. A partial report or config-only negative is not sufficient.

Candidate invocation framing:
`("sharpebench.scanned-image-invocation.v1", sandbox_label, image_id, policy_sha, scope)`.
Apply existing rate-card binding too. Keep legacy unscanned identities unchanged.
Do not bind unstable export timestamps without examining resume implications;
the immutable image configuration ID is the intended deployment identity.

Emit a structured redacted report even on later failure. Refusal produces no
leaderboard. A successful JSON board should attach metadata only to the matching
external entrant row, preserving reference rows and the existing array schema.

**Known prototype bug:** `run_board_json` returns an ARRAY. The lost prototype
indexed it as an object using `output["artifact_preflight"]`, which would panic
on a successful scanned run. Fix the integration design and add an actual
successful-board regression. The old failure-only fake-Docker tests missed this.

### Required synthetic and live tests

- Known TAR match: no entrant launch; removal attempted and checked.
- Config match, including decoded escaped text: no create or entrant launch.
- Declared volumes: refused before snapshot creation.
- Malformed archive, create/export error or cleanup error: fail closed.
- Wrong container image, running state or mounts: refuse before export.
- Invalid policy or transport conflict: no Docker commands.
- Negative scan: actual successful entrant board JSON, correct row metadata,
  unchanged references, correct image ID and no array/object panic.
- Resume under same policy makes no new calls; changed policy refuses before
  calls and does not mutate checkpoint bytes.
- Caps, exit status, rewinding, deadline, redaction and child cleanup unit tests.
- Real daemon: a clean pinned fixture passes and a known header needle such as
  `etc/alpine-release` fails, both with verified cleanup and equal image ID.
- Validate supported TAR forms emitted by real Docker, including metadata.
- Verify installed CLI behavior where packaging affects the feature.

A proposed ignored test name was
`artifact_preflight::tests::live_docker_image_preflight`, using the existing
`SHARPEBENCH_SANDBOX_FIXTURE` digest-pinned Alpine job. Add its exact invocation
to that CI job; never run all ignored CLI tests because a subprocess fixture may
require an environment mode.

Historical prototype output reported six units, six fake-Docker CLI tests and
15 caught mutations. A later full CLI run and restored control had no recovered
final result. The live test was never pushed or executed. ALL reconstructed
code requires fresh verification; these numbers are not current acceptance.

## 7. G02/G03: complete Hyper-Tau study and port reconciliation

Local archive root: `hyper-tau-bench-main/hyper-tau-bench-main`; source paths
below are relative to its `src/tau2`. The inventory is 8484 files: 7818 data,
323 source, 172 tests, 151 web, three Docker files and remaining root assets.
Counting, grepping or listing these files did not constitute end-to-end reading.

Complete reads already recorded in HYPER-TAU-REVIEW.md:

- Root README, AGENTS, LICENSE, pyproject and tests/AGENTS.
- hyper/runtime_contract.py, performance.py, agent_context.py, _inner.py.
- hyper/sandbox/model_gateway.py, native_runtime.py, builder.py,
  callback_broker.py, callback_mcp.py, starting_workspace.py,
  result_serialization.py, orchestrator.py, sealed_runner.py,
  candidate_server.py, kit.py, native_builder.py, local_test.py.
- hyper/client_api/defects.py, development.py, runtime.py, __init__.py,
  capabilities.py, catalog.py, catalogs/__init__.py.
- hyper/client.py, grounding.py, task_loader.py, run_defaults.py,
  performance_profiles.py; utils/llm_utils.py, model_routing.py.
- docker/hyper-construction/Dockerfile and strip_runtime_src.py.
- tests/test_hyper/test_runtime_image_surface.py, tests/test_llm_utils.py,
  tests/plus_support/leakage.py.

Remaining named areas include domain catalogs (retail, airline, telecom, banking),
action_catalog, data_model, recording, live_experiment, other provider paths,
domain tools, task corpus, remaining tests, web and configuration.
Maintain a file/group coverage register: complete read, generated/duplicate with
reason, binary, or unread. Do not call an inventory a full scan. Account for the
actual corpus and schemas, not just Python entrypoints.
Do not run the foreign full test suite: some llm_utils tests call providers.

The earlier porting assessment was NOT spot on:

- Its scanner runs after scoring and skips read errors; it is not prelaunch refusal.
- There are two gateways. Sealed scored candidates use network-disabled stdout RPC
  to a host caller; construction uses an HTTP sidecar with two networks.
- Sealed queues bound lines, not bytes; outer timeouts do not interrupt a blocking
  provider call; quotas are per request, not a persisted sweep budget.
- HTTP gateway response/spend bounds and ownership checks need strengthening.
- Empty-completion retries account only final usage; unknown/Responses cost can
  become zero. Do not import that behavior.
- Runtime compatibility integers are not immutable artifact identities.
- Cleanup ignores some nonzero exits; event accumulation and descendant handling
  are weaker than Bench's existing protections.
- Missing or malformed tasks can silently shrink support; keep explicit denominators.
- Keyed fault plans and backoff are meaningful diagnostics, not just cosmetic.
- Async mutation versus response projection is useful, but polling simulation is
  not real exchange behavior or a general exactly-once guarantee.
- Source stripping and public-kit separation help but do not prove absence of
  equivalent private data or make mutable Docker inputs reproducible.
- Capability freezing, catalog sealing and contract identity are distinct.
- Grounding set checks ignore arguments/multiplicity; they are not trajectory proof.

For every proposed port, record source, existing Sharpe consumer search, concrete
benefit, threat/assumption changes, tests and take/adapt/reject/defer decision.
Keep customer-service semantics and an outer agent-building product out unless
separately justified. No integration with the reference arena is required.

## 8. G10: numerical migration without rewriting history

Do not close the accepted migration task by saying the old bit pins suffice.
Independently reassess statrs and record a justified implementation or an explicit
authority-dependent disposition. Claude's large-grid bit-difference figures are
historical claims, not newly independently established measurements.

Review the special-function implementation in `crates/sharpebench-stats/`,
its pins, Cargo feature/dependency impact, supported Rust/WASM targets and licenses.
Check erf, normal CDF and inverse CDF across signed zero, NaN, infinities, tails,
branch boundaries and actual kernel quantiles. Use an independent accuracy
reference with a stated precision and error metric. A better approximation need
not have identical bits; identical bits need not mean a correct formula.

Keep the audited moment-normalization convention explicit. Do not assume a
generic crate's skewness/kurtosis convention matches the required population
moments. Preserve valid arithmetic ordering where compatibility is promised.

Build an impact ledger: old/new functions, methodology identity, affected code
goldens, reports, wrappers and paper producer outputs. Historical paper artifacts
remain frozen. Versioned new behavior may require separate fixtures and explicit
legacy replay support, not silent replacement of archived evidence.
If the proposed migration requires regenerating published numerical evidence,
stop that portion and request the missing authority; continue independent work.
Never claim statrs was shipped when only compatibility tests were added.

## 9. G11 and G12: host accounting and readiness

Read existing external transport, invocation identity, attempt ledger, accounting
and field examples before selecting a protocol. Prefer host-mediated model access
compatible with network-disabled entrants; do not add broad egress to make an
HTTP sidecar convenient. No third-party arena integration is needed.

Required properties:

- Host owns credentials, provider destinations, routing and allowed model revisions.
- Entrants cannot supply arbitrary URLs, credential headers or shared response IDs.
- Bound complete wire envelopes before allocation, responses, lines, tool payloads,
  concurrency, calls and durations, including provider client read timeouts.
- Enforce persisted sweep-level money/token budgets with reservations before calls.
- Reconcile every attempt, including empty responses, retries and partial failures.
- Unknown completion/cancellation cost is not a free retry or an automatic refund.
- Freeze model/revision/rate-card identity into invocation/checkpoint identity.
- Persist append-only attempt usage; recovery cannot selectively erase spent attempts.
- Missing usage is unavailable, not zero; partial totals remain labelled partial.
- Host-observed provider usage is not independently verified billing or invoice cost.
- Preserve score/rank neutrality and existing support/certification requirements.
- Redact credentials and sensitive request bodies in errors, logs and evidence.
- Ensure cancellation, shutdown and descendant lifecycle remain bounded.

Use hermetic fake providers to test success, 429/5xx, timeouts, invalid/truncated
or oversized JSON, absent usage, empty responses, retries, budget exhaustion,
ambiguous post-commit failure and resume. Do not use a real API key for tests.
Check that all retries are counted, no cost is repriced on resume, and no request
starts after a hard budget refusal. Mutation-check these invariants.

For G12 inspect `llm_field_eval.rs` and `local_open_weight_field_eval.rs`
under the harness examples. Test effective config, missing credentials, missing
budget, unsupported model, dry-run and refusal paths without network calls.
No experiment is required to deliver readiness. No model installation is allowed.
Paper prose must describe absent empirical evidence without personal setup details.

## 10. G01/G05/G13/G14/G15: final correctness and documentation

Finish source/API review with greps and actual consumer traces. A suspected gap
becomes a finding only after reproduction or a concrete violated invariant.
Each repair needs valid and invalid controls, an isolated mutation, and affected
installed-package checks. Do not claim every possible bug has been eliminated.

Rebuild packages after relevant changes. Inspect pinned tool versions first:
the earlier wasm-bindgen lock required 0.2.126 while an old system binary was
0.2.105. Temporary matching tools and venvs are gone. Recreate safely from the
current lockfiles rather than assuming the old commands still resolve.
Check Rust, CLI, WASM/npm, Python and MCP at the actual exposed boundaries.
Preserve typed errors/nullability; never coerce unavailable values to zero.
Run Arena packaged SPEC_HASH checks after manifest/build-input changes.
The exact registry pin propagation limit in section 3 remains a separate issue.

Update READMEs after code acceptance, retaining supported capabilities and moving
deep detail into discoverable subdocuments. Bench positioning: a luck-robust
benchmark for quantitative trading agents. Arena retains sandbox and RL-environment
terminology. Do not remove the term sandbox. No Smolvm sections in either README,
no em dashes, no unsupported claims of being first or universally secure.
Document new flags, cost provenance, recovery limits, scan scope and compatibility.

For papers, audit definitions, inference assumptions, support counts, eligibility,
interval withholding, pass^k and historical-versus-current producer behavior.
Rebuild PDFs without regenerating frozen numeric evidence. Check actual exit codes,
undefined references/citations and overfull boxes. Avoid quoted self-referential
manifest digests. No invented LLM leaderboard or new numerical experiment.

Lean models prove only their declared Covers/Assumes scope, not the whole Rust
implementation. Seeded property tests are not formal verification. No successful
Wolfram result was established in this work; do not imply one.
A raw scan does not prove no memorization. Containment is not kernel-escape or
multi-tenant assurance. Rate-card estimates are not invoices.

## 11. Safe commands and delivery procedure

Run commands from the intended repo explicitly. These paths describe this
checkout; substitute the verified clone location on another machine.

```bash
git status --short
git branch --show-current
git remote -v
git worktree list
git fetch origin
git log -6 --oneline
git rev-parse HEAD origin/main
git diff --check
```

Read the current workflows and CONTRIBUTING.md before choosing broad commands.
Focused recoverable tests, from Bench:

```bash
cargo test -p sharpebench-harness --test artifact_scan
cargo test -p sharpebench-harness --test artifact_tar
cargo test -p sharpebench-harness --test runtime_recovery
cargo test -p sharpebench-harness --test frozen_rate_card
cargo test -p sharpebench --test frozen_rate_card_cli
cargo test -p sharpebench --test attempt_accounting_cli
cargo fmt --all --check
cargo clippy --all-targets --all-features -- -D warnings
RUSTDOCFLAGS=-Dwarnings cargo doc --workspace --exclude xtask --no-deps
python paper/src/check-provenance.py
```

For Arena use its AGENTS commands: workspace Rust tests, packaged-spec check,
Lean scope check and installed-venv pytest. A fresh wheel test must import the
installed extension, not accidentally import a stale source-tree package.
Use nextest/workflow settings where appropriate; don't accidentally skip doctests
or run environment-dependent ignored subprocess fixtures wholesale.

Before each commit inspect the full diff, verify no frozen numerical artifacts
changed, and stage only owned explicit paths. Commit code/tests separately from
documentation and any required manifest rebind. Check provenance after committing:
dirty working state can intentionally fail honesty checks. If rebinding is needed,
run the repository producer on the appropriate clean candidate, commit the
manifest explicitly, then run the checker. Do not hand-edit honesty fields.

These explicit pushes update the existing draft PRs from the handoff branches:

```bash
# From the SharpeBench checkout only:
git push origin HEAD:refs/heads/feat/artifact-preflight-2026-09-10
# From the SharpeArena checkout only:
git push origin HEAD:refs/heads/docs/scanner-progress-2026-09-10
```

Keep both draft until their scoped work is ready. Do not merge unfinished G07
just to make a handoff document appear on main. If a later independent docs-only
PR is needed, build it explicitly from current main without importing feature code.

Inspect exact PR head and all relevant checks, not an old watcher result:

```bash
gh pr view 44 --repo general-liquidity/sharpebench --json headRefOid,isDraft,state
gh pr checks 44 --repo general-liquidity/sharpebench
gh pr view 40 --repo general-liquidity/sharpearena --json headRefOid,isDraft,state
gh pr checks 40 --repo general-liquidity/sharpearena
```

On readiness, use a normal merge matched to the verified full head SHA. Fetch
and compare the merged tree to the tested tree; wait for post-main checks.
If main moves, merge it normally, resolve only understood conflicts, reverify
generated non-frozen fixtures and rebind provenance. A conflict-free textual
merge can still combine incompatible generated reports. Never overwrite frozen
paper artifacts as an automatic merge-resolution strategy.

Update shared docs identically and verify with cmp. Record command, exit status,
tested SHA, package source and CI URL for each gate. A rerun is not a diagnosis
of a flaky sandbox test; distinguish network timeout from an observed denial.

## 12. Completion and continuation template

Before calling the whole goal finished, every open ledger row must have a tested
implementation or explicit evidence-backed disposition, installed checks where
needed, documentation and delivery status. External authority blockers remain
visible. No active draft, unpublished local change or unobserved CI run may be
silently described as merged and green. Preserve unrelated branches regardless.

For each task append:

- ID, source paths and concrete invariant.
- Initial reproduction or why the proposal is rejected.
- Changed files and compatibility/evidence impact.
- Positive/negative tests and isolated mutation result.
- Installed-artifact and live versus synthetic verification.
- Exact commit, PR, tested head, merge head and post-main result.
- Remaining limitations, missing authority and next executable step.

Suggested opening instruction for Claude:

> Read AGENTS.md and docs/audits/2026-09-09/CLAUDE-HANDOFF.md in both repos.
> Verify current refs and preserve all user work. The raw scanner/TAR code is
> pushed; the Docker prototype was lost and must be reconstructed, including its
> known JSON-array output bug. Complete the remaining G01-G16 ledger under the
> no-experiments/no-releases/frozen-evidence rules. Use granular commits and
> exact-head checks; keep audit docs mirrored. Do not restart completed repairs
> or treat historical prototype test output as verification of current code.
