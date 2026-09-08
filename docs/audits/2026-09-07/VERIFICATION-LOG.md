# Sharpe suite audit verification log

Chronological repair diary moved out of [IMPLEMENTATION.md](IMPLEMENTATION.md) on 2026-09-08, unchanged. Latest closure notes come first; older entries that say "pending" or "in progress" are historical and are superseded by the checklist. Row IDs are defined in IMPLEMENTATION.md.

## Verification log

- Batch A and B, 2026-09-08. Work is on `fix/audit-batch-a-b-2026-09-08` in
  each repository, Bench PR #25 and Arena PR #26, both opened as drafts. Eight
  checklist rows close. At the time of writing every CI check on both pull
  requests passes except the provenance gate, which fails by construction until
  the manifest is rebound on a clean tree after the last commit.

  R13. The false containment claim was corrected in the paper, in
  `Mandate::max_run_drawdown` and in `CompositeScore::worst_run_drawdown`. A
  repository-wide grep for the same assertion found no other site. The new
  regression `worst_run_drawdown_can_exceed_pooled_under_seed_averaging` runs
  the real `score_agent` path with `execution_seeds_per_window: 2` and asserts
  pooled 0.0 against worst-run 0.20, plus the gate consequence that a 10 percent
  per-run cap refuses what the same pooled cap admits. Two mutations were run in
  an isolated copy: replacing the per-run fold with the pooled figure fails the
  test, and weakening the assertion to compare pooled against pooled also fails
  it, so the assertion is not vacuous. `cargo test -p sharpebench-core`,
  `clippy` and `fmt --check` all exit 0.

  R11. Corrected twice. The first rewrite said the endogenous market fills every
  order at one cleared price. Reading `market.rs` showed that is false: the fill
  is `cleared_mid * (1 + f * (lambda * Q + eta * q_i) / V)`, so each agent pays
  for its own size and no two agents transact at the same price. The committed
  text says neither model is uniform-price, gives the book its price-time
  priority and queue-position semantics, and grounds the latency-race exclusion
  in discrete time rather than in single-price clearing.

  R14. Arithmetic checked against the declared field of six policies, three
  tiers and 256 seeds. A single agent on one tier is 256 of 4,608, one
  eighteenth; one sixth is one agent across three tiers.

  Shard order. Verified in source rather than from the log: `validate_grid`
  returns `[found[key] for key in expected_keys()]` and writes the original
  decoded line, and `test_shuffling_input_has_canonical_output` asserts reversed
  input yields byte-identical output, so the order is a function of the grid key
  alone and differs from the serial producer's score-rank order.

  Baseline scope. The four literature rules are scored in the paper on nine
  datasets under three cost profiles, 351 records, no rank-eligible cell and no
  pass^k pass; those rule names and that count were checked against
  `05-experiments.tex`. The seven primitives in `sharpebench_core::entrants`
  have no field evaluation, and the four rules are implemented in the harness
  example rather than in that module.

  Publish gate. `require_green_ci` queries `ci.yml` runs at
  `validate_tag.outputs.validated_commit` and passes only on a completed
  success. The workflow parses under `yaml.safe_load` and the job graph was
  dumped to confirm that `binary`, `crates`, `npm`, `pypi` and `verify` all
  depend on it. Both crate publish lists carry the same 12 crates as the
  workspace, `sharpebench-memory` included. A second defect was found while
  fixing this: `verify` runs with `if: always()` and its result loop did not
  inspect the gate, so a blocked release would have reported success.

  Updater. The checksum is fetched from the same release over the same
  connection as the asset, so it establishes integrity and not authenticity.
  The documentation now says so and names the SLSA attestation as the
  out-of-band route. No signature verification was implemented.

  Snapshot cost. `clone_state` copies a `Book` holding shares, cash, RNG, an
  accumulating trace and pending orders, so the cost grows with the run. The
  claim of constant time was corrected in the mdBook page, both changelogs, four
  Bench rustdoc sites and one Arena comment. No optimization was implemented;
  shared immutable trace prefixes would require a measurement that was not run.

  AP5 and AP3. The sealed reveal compared only each symbol's opening close in a
  two-day calm environment while the declared evaluation is 120 days at the hard
  tier. The full replay was measured at 0.14 s for all 16 slots before choosing
  to implement it rather than weaken the claim. Four mutations were run in an
  isolated copy and each failed the expected tests; restoring the pristine copy
  passed all seven. No committed artifact was rewritten, and the deltas a future
  run would produce are recorded. The frozen `reveal_replay_verified` count of
  16 was measured under the weaker check and is not evidence for the stronger
  one; the Arena paper now says so.

  Propagation. Confirmed by ancestry rather than assumed: neither `0cd7d37`
  (moments) nor `4378ad4` (inventory and cash) is an ancestor of `v0.15.0` or of
  `v0.18.4`, so both repairs are unreleased, and Arena's pin of `=0.15.0` cannot
  carry them. Both changelogs record that.

  Scope note. An em dash sweep initially reached archived peer-review records
  under `paper/review/` and the internal assessment documents. Those were
  restored file by file. Rewriting a historical record is out of scope; the row
  covers user-facing documentation only.

- CSV/board-context batch, Bench `7f80fee`: five CSV module tests, four actual
  CLI integration tests, three WASM board-context tests, 62 Python tests and
  18 npm tests pass. The complete product workspace (`cargo test --workspace
  --exclude xtask`) passes, including four frozen-evidence regressions and
  three native/WASM golden-parity tests. Product-workspace and standalone PyO3
  Clippy pass; all 28 `paper/src` unit tests pass. Local `xtask` still lacks
  OpenSSL development files; publication also requires the full CI workspace.
  The expanded offline npm tarball smoke passes, including a declared relative
  mandate through the installed tarball. The rebuilt binary, types and executing
  npm tests are committed in `b22ecdd`.
  In isolated temporary copies, bypassing period-ID agreement fails one test
  and reintroducing missing-cell compaction fails one test. All three new npm
  regressions fail against the old committed main WASM and pass against the
  rebuilt WASM, including actual host eligibility/reason differences rather
  than only changed array order. No mutation touched production source.
  CSV readers refuse blank/ragged input, invalid selected numbers and missing
  selected data. Regime comparison requires equal complete row counts, with
  identical unique ordered IDs when `--period-col` is supplied. These checks
  do not establish temporal support without IDs or repair the separate import
  path. Board declarations retain their second verdict, host ranking is
  unchanged by them, and reasons explain the host score only. No model call,
  new benchmark experiment or historical-artifact rewrite ran.
  The batch must pass exact-head Actions checks and normal main merges before
  the requested break; the final PR records establish those publication results.
  No additional goal entry is to be started.

- R15 evidence-producer closure: Bench `c9dc85f` adds
  declared 4 × 4 × 4 × 8 coverage, strict JSON, dataset/configuration agreement
  and required table-input validation. Assembly preserves original JSON line
  contents in fixed key order with LF endings; the reducer validates all nine
  required datasets before printing tables. Operator documentation is `e60e1f7`.
  All 16 focused tests and all 28 tests discovered under `paper/src` pass.
  The nine frozen principal datasets each contain 512 unique declared cells;
  this validates STRUCTURE ONLY, without rescoring or establishing numerical
  parity with the current engine. The new paper CI command is
  `python -m unittest paper/src/test_sweep_grid.py`, which executes the actual
  16-test suite. CI `34165122503` and npm `34165122442` passed all 17 Actions
  jobs on exact head `7ccc6acfcbe4a05d8d63751d5169eafd471d08e5`. PR #22
  merged normally at `7cfc954881437013d6c6f9457143b104a546549f`; both trees
  equal `c4215bdc061e0d42ae011f7b30b643640d6ac799`. Provenance validates
  171 sources and 40 artifacts.
  A newline-boundary follow-up replaces `splitlines()` with `split('\n')` so
  legal U+2028 inside a JSON string stays within its record. The new regression
  fails against the old temporary reader and passes with the repair in `4ff3637`.
  Three isolated temporary mutations fail as expected: bypassing grid validation
  fails five tests, disabling duplicate-JSON-key rejection fails one subcase,
  and bypassing renderer-field validation fails seven subcases. Restoring the
  temporary copy passes all 15 tests. No mutation touched production source.
  Before repair, the initial 10 tests produced 22 failures, including changed
  diagnostics; those are not 22 independent accepted defects. No benchmark
  experiment or historical-artifact rewrite ran. The section 2 combined R15/BI3
  row remains open because CSV/ranking keyed support is separate from this
  assembler repair.

- Final-paper follow-up: [A-commands.tex](https://github.com/general-liquidity/sharpebench/blob/7ccc6acfcbe4a05d8d63751d5169eafd471d08e5/paper/sections/A-commands.tex#L55)
  still connects shard assembly with output identical to the serial producer.
  Revise that claim during the final paper pass: the new fixed grid-key order
  differs from the serial producer's score-rank order, even though original JSON
  lines are preserved. No paper edit or evidence regeneration has been made.

- Arena producer replay: 51 focused promotion cases and 4 Node-forwarding cases
  pass. The initial two output-only/changed-identity regressions failed before
  repair. The unchanged gold payload fails with an injected observation leak,
  then passes after restoring the actual producer; the test asserts all 11
  observation calls reached it. Disabling the gold identity guard fails six tests;
  disabling source comparison fails two. Mutations use the temporary package;
  restoration passes. The final complete local Python suite passed 1,390 tests
  with 9 optional skips; all 55 focused tests pass. CI passed 1,397 with 2 skips.
  Provenance validates 141 sources and 52 artifacts. Historical artifacts remain
  untouched. R21's real Node probe executes
  only a tiny argument-reporting fixture, never the throughput experiment.

- BM9 closure: PR #20 merged at `0b04a13e39caa9e23979019571ba679131f6c5b5`.
  CI `34161858383` and npm `34161858390` passed all 17 Actions jobs on
  `4d553666b16b6ccd66b37f5fce70977061a1817a`; both trees equal
  `635bdfe5f12fcb80630e6573083f05531bc2783a`. Locally, 14 briefing tests,
  affected-crate Clippy, product workspace excluding xtask, 15 npm tests and the
  offline installed-package smoke passed. Seven new area/ordering/threshold cases
  failed against the old implementation. The actual committed WASM was rebuilt
  and exercised. Provenance validates 169 sources and 40 artifacts.

- Accelerated Arena reward/diagnostic batch: 41 episode-outcome cases failed against
  the prior implementation, including favorable-prefix reward through the real rubric.
  The expanded 48-case suite covers all eight schemes, native window endpoints,
  process failures, inconsistent accounting, and the actual verifiers framework loop
  with synthetic responses (no model calls). Seven proxy regressions also failed before
  the repair. Latest complete Python suite: 1,357 passed, 9 skipped; 7 skips concern
  absent optional Minari paths and 2 concern unavailable-dependency branches while
  PettingZoo is installed. No skipped test is treated as passing. The test environment
  uses the project's pinned verifiers 0.1.14 and MCP 1.28.1. Historical results and papers
  are untouched. AI9/AD4 then passed all 11 Actions jobs in run
  [34161346359](https://github.com/general-liquidity/sharpearena/actions/runs/34161346359)
  on `986444f01b30eb5bb16fafb1c4ffe58e9181cb94`; PR #21 merged at
  `9d4e246e60f0ff39490550c09a253706d063c36e`. Both trees equal
  `f0d2149a5c06dd9d11a163c505bf5b2dfeb4fdfb`. Replacing the failure floor
  with zero fails all 16 abort-reward cases; restoration passes all 16.
  The actual CI Python job passed 1,364 tests with 2 skips. Replacing the indicator
  proxy's rolling history with only its last return fails 2 targeted cases; restoring
  the implementation passes all 3 targeted cases. Both mutations used a temporary
  installed package, not the production worktree.

The latest closure notes come first. The subsequent log preserves work in progress
as recorded at the time; older phrases such as "pending" or "in progress" are
historical and are superseded by the current checklist and status above.

- BM6 closure: PR #18 merged normally at `e6f2ab1` after CI `34155625798`
  and npm `34155625802` passed all 17 Actions jobs on `ffedd3a`. The merge tree
  equals the tested tree `73e73f82ac6f382f51f50087faf593067d3ff196`.

- BM4/BM5 closure: options PR #17 merged normally at `77ea0b5`; tree equality
  against tested `b75ccb3` verified. Exact-head CI `34153881621` and npm
  `34153881725` passed all 17 Actions jobs. Post-merge main CI `34154795075`
  and npm `34154795093` also passed. Local main subsequently fast-forwarded
  through the memory merge `e6f2ab1`.
- BM6 implementation: five initial regressions failed before the change, including
  the original three-node broken-chain defect, distinct-node cycles, duplicate
  edges, invalid alpha, and a single-task descriptive report incorrectly requiring
  inferential support. Revised memory suite: 40 unit + 18 integration + 2 doc tests;
  stats suite: 87 unit + 1 doc test. Clippy on affected crates and workspace excluding
  xtask and Rustdoc with warnings denied passed. The full product workspace tests
  excluding xtask subsequently completed successfully; full CI also passed.
  Inference assumes independent complete paired chains with exchangeable arm labels;
  it reevaluates qualification in both orientations, then samples/enumerates joint
  assignments. Tasks/sessions are not promoted to independent observations. Exact
  geometry checks do not establish actual task alignment or replicate independence.
  Four isolated mutants failed (2, 3, 1 and 1 tests respectively): raw-only
  prerequisite checks, frozen credit masks during swaps, raw-significance substitution,
  and unsmoothed Monte Carlo tails. The 16-assignment synthetic null check also
  catches excess rejections from frozen masks. No agent/model experiment or historical
  artifact regeneration was performed. Matching oracle/task populations remain a
  separate open supplementary item. Provenance: 169 sources, 40 artifacts.
- Initialization: read audit plan and source invariants, confirmed clean trees, created repair branches.
- Skills: executing-plans, code-quality-review, statistical-analysis, scientific-critical-thinking.
  Discovery gateways reference absent math/testing files; no execution of those missing resources is claimed.
- R01: two new regressions failed before the repair (erased block and erased warning).
  After repair, `cargo test -p sharpebench-core --quiet` passed 276 unit, 4 integration,
  and 1 doc test; Clippy all targets passed. Declared eligibility, ordinal, warning
  count, process scalar and return floor are checked, including reversed field order.
- R03: exact moment fixtures fail in both pre-fix implementations. Chosen convention:
  empirical population standardized moments, n-normalized second/third/fourth central
  moments, no bias adjustment. Reference: NIST e-Handbook 1.3.5.11.
- R03 verification: Bench stats 79 unit + 1 doc passed; Arena core 155 unit + 9
  integration passed; both affected crates Clippy clean. Two Bench golden-score tests
  correctly detected arithmetic drift. Refreshed only the two deterministic score
  fixtures using the repository's explicit update command, reviewed every diff:
  changes are PSR/DSR/CI/SE only; fixture eligibility and order unchanged. All 4
  golden tests then passed. Historical paper evidence untouched.
- R02 in progress: bootstrap/BH/FDR now return typed errors; memory and budget
  consumers propagate them, Python maps them to ValueError, and composite scores
  expose bootstrap_error with independent fail-closed eligibility. Broader statistical
  family/input validation and all downstream presentation checks are still pending.
- R02 checked bootstrap/FDR batch: Bench `895a623`. Coverage probe now emits a real
  invalid-bootstrap score rather than weakening the inventory. Stats 81, core 277
  unit + 4 integration, memory 40 and their 3 doc tests pass; affected-crate Clippy
  passes. Rebuilt Python extension passes all 47 statistics cases. Removing the BH
  validator makes its malformed-probability regression fail; restored and green.
  Full product workspace (`--exclude xtask`) passes, with Docker/manual tests still
  explicitly ignored. The environment's missing OpenSSL development dependency
  still prevents building xtask; no complete-workspace claim is made.
- R19/R20: all three new regressions fail before the repair. Afterward simulator
  80 unit + 1 integration pass, including a full original/masked dividend-bearing
  BuyAndHold replay equality. Numeric/header/key errors are exact refusals; absent
  dividend columns are intentionally distinct from invalid cells. Clippy clean.
- Arena diagnostic batch in progress: 9 failing cases independently reproduce lost
  initial NAV, centered-loss denominator, aliased actions and reversed signed-cost
  ordering. Repairs pass 23 metrics/trace tests (2 existing native skips). Fresh
  Linux native extension builds successfully; installed-package checks pending.
- Arena fresh-source/native diagnostic checks: 70 metrics/trace/failure/inversion
  tests pass without skips. Exact-grid tests reject three previously accepted
  off-grid observations and reproduce all 2,048 candidates at each grid boundary.
  Failure tests reproduce 13 prior failures and preserve bad episodes in the
  denominator. Sortino uses full-sample target RMS, agreeing with Bench and the
  PerformanceAnalytics full convention (github.com/braverock/PerformanceAnalytics,
  R/DownsideDeviation.R); no conditional-on-loss or centered-loss estimator is implied.
- R18: six new cases fail beforehand, all 10 effective-config tests pass afterward
  against the rebuilt native extension. Real generated panels verify a misconfigured
  second consumer is refused before its rollout, for absent/empty/prefilled collectors.
- R09: target/flat regression fails beforehand; simulator 82 unit + 1 golden pass
  afterward. Exact cash arithmetic for signed trades has a separate fixture. The
  official test-fixture update changes trajectories, no historical paper artifact.
  Fixture support remains 4 agents x 6 runs x 40 bars, and all eligibility/order
  unchanged. Momentum no-op events decrease when exact closes remove residual trading.
- R07-related new finding: isolated core lacked serde_json float_roundtrip while
  workspace harness enabled it. Correctly-rounded parsing was therefore dependent
  on Cargo feature unification. A 4,096-value numeric roundtrip regression fails
  in isolation before explicit core/protocol feature declarations. Repair underway;
  one synthetic crowdedness value changes by 1 ULP after the corrected parse.
- Dependency coordination: Arena currently consumes exact-pinned registry Bench 0.15.0.
  Local Arena confidence correction alone does NOT update its pinned scoring/simulator
  dependencies. Resolve cross-product implementation/package strategy before claiming
  parity or completion; no automatic registry publication is authorized.
- Arena wrappers: all 36 new action-boundary/snapshot cases fail before the repair;
  60 action/Gym/vector tests pass afterward. LOB partial/terminal reset regressions
  reproduce stale reward subtraction; both pass after `53e963a`. PettingZoo stress
  tests compare actual five-bar native sequences, not mode labels; both fail before
  and pass after `e7ecdb8`. Combined competition/LOB suite: 20 pass, 1 optional-import
  skip (dependency is installed). Native-code reset and reconstruction knobs remain.
- Bench full-suite follow-up: the initial positive-price CSV restriction contradicted
  the documented raw WTI dataset. `d882803` retains finite signed prices and the
  explicit percentage-return limitation. No data row changed. The full suite then
  reached a current-engine/historical characterization mismatch: commodities has 2
  finite-Sharpe votes on fresh pooled trajectories versus a historical measured
  dispersion stamp. Three other harness historical-field tests pass. This remains
  under investigation; no full-workspace pass or unchanged paper evidence is claimed.
- Current/historical commodities characterization is now explicit: the unpinned
  frozen default has 8 `measured_floored` rows; the current simulator reconstructs
  only 2 finite-Sharpe streams and therefore insufficient measured-dispersion support.
  The regression checks both the frozen stamp and current support, not a fabricated
  parity guarantee. Current data/domain/terminal behavior still needs audit disposition;
  no historical artifact or paper statistic was rewritten.
- R04: source/package semantic-manifest equivalence and version/feature divergence
  tests pass. The real normalized Cargo package executes the exact committed-pin
  assertion successfully; the new CI script additionally refuses zero executed tests.
  Source core: 155 unit + 11 integration tests; npm: 12; Python handshake: 5.
  Rebuilt wasm and pyo3 against epoch 2 / `24a915041c3f86ea`, Clippy clean.
  Build-only TOML parser dependency added, exact wasm-bindgen CLI 0.2.127 installed
  in an isolated temporary tooling directory. No model files downloaded.
- Bench full product workspace (`--exclude xtask`) passes after the explicit
  current/historical characterization. Checkpoint regressions pass 12 cases,
  affected-crate Clippy passes. Checked repair branch pushed through `e322b17`;
  provenance reports 164 sources and 40 artifacts. No main merge or release.
- Native market reset: all 3 Python regressions fail before repair. Afterward,
  market/uncertainty/concavity/handshake tests pass 34 with 1 optional-import skip;
  Rust workspace passes 156 unit + 24 integration, npm 12, Clippy clean. The reset
  parity test also checks unchanged allocation of the immutable exogenous tape.
- Checkpoint continuation tests expose lost clustered/jump controls on both restore
  paths and discarded action prefixes after native restoration (3 failures). The
  replay repair passes; the native test additionally exposes one-bit JSON float
  drift in cash and positions. Explicit float_roundtrip is being propagated to
  Arena core/Python (wasm already opted in), with epoch 3 and rebuilt wrappers.
- Arena reconstruction and JSON batch: `4141786`, `d285189`. All 17 checkpoint/
  handshake cases pass against the fresh native build; workspace 156 unit + 25
  integration, npm 12, real normalized Cargo pin, workspace and pyo3 Clippy pass.
  Pushed through provenance `a55dde9`, 137 sources and 52 artifacts. PR #14 opened;
  cross-platform Rust, installed wheel, offline npm and provenance green, full
  Python pending. Bench PR #11 caught a Rustdoc link warning for `[0,1]`; fixed
  and pushed `f75d196` + rebind `d7c634d`. Remaining CI gates pending before merge.
- Both initial batches merged normally, preserving individual commits: Bench PR #11
  at `aefd5b1` (all 17 actual Actions checks passed on `d7c634d`), Arena PR #14 at
  `d7ed89f` (all 11 actual Actions checks passed on `2fc0a94`). Merge trees equal
  tested-head trees. Arena post-merge main CI `34143420345` also passed. CodeRabbit
  reported skipped/manual-review-required, not an independent review.
- Arena confidence compatibility: `96e8782` withholds `deflated_sharpe_ci` while
  exact registry Bench 0.15.0 retains different moments. Corrected diagnostics stay
  separately named; no replacement of official score or saturation-as-parity claim.
  `382659a` makes fresh F1/figure production reject missing intervals rather than
  manufacture zero-width bars. Shared-engine update still awaits release authority.
- Bench transport follow-up: `b60890f` includes blocked stdin in the absolute deadline
  and poisons timed-out streams. A finite 3-second nonreader with >128 KiB input
  reproduces pre-fix `Exited(0)` rather than timely Timeout. `abdabef` caps the stdout
  queue at one line and accepted stdout at 64 MiB per attempt. Queue-capacity mutation
  consumes 768 vs allowed 6 fixture bytes; budget mutation admits the forbidden third
  line. Both regressions fail and are restored. Simulator 86 unit + 1 integration,
  Clippy, Rustdoc and fmt pass locally. PR #12 has Linux/macOS passing; Windows pending.
- Arena checkpoint follow-up: `2fcf7ed` default export removes private params/CSV/seeds/
  native state; an actual CSV fixture carries a future-only price needle. Explicit
  private JSON roundtrips restore native/replay continuations. `dfd8891` preserves
  float64 actions, validates metadata and action prefix, and restores transactionally.
  19 regressions reproduce prior coercion or partial replacement; 110 checkpoint,
  Gym, vector, action-validation and functional tests pass afterward. No model or
  empirical field executed. Provenance is checked after committing the binding,
  since Arena's clean-tree check intentionally counts the modified manifest itself.
- Confidence display `efb97be`: actual a/b direction and each requested confidence
  level are rendered; inconsistent, absent or nonfinite records are refused.
  Legacy `tied` remains a wire label only, rendered as difference not established.
  No equivalence, complete luck cancellation, skill isolation or simultaneous ranking
  guarantee follows. Scientific-critical-thinking skill used, including statistical
  pitfalls reference; no Wolfram tool is exposed in this session and none is claimed.
  32 cases fail before repair; 56 Python and 8 Rust confidence tests pass afterward.
- Bench input boundary batch: Rust LITE/FULL reject invalid trial counts rather than
  narrow u64 into u32. JS independently validates safe integer range; MCP publishes and
  enforces the same schema. Exact max-u32 accepted. Binary outcomes require exact zero/
  one or bool (WASM) rather than nonzero truthiness; CLI refuses NaN/infinity too.
  19 wasm unit + 3 integration, 18 CLI analysis, 13 actual-wasm npm and 8 MCP tests pass,
  plus offline packed-install smoke, Clippy and formatting. Raw wasm exports return
  `{error}` JSON (not JS exceptions), and the test asserts that exact contract so the
  JS guard cannot mask a stale unsafe binary. Temporary wasm-bindgen CLI 0.2.126
  matches this workspace lock; no model weights or inference runtime installed.
  The BI5 commit includes test-first binary cases before their following BI9 repair;
  only the verified final tree will be merged, no history rewrite or force-push.
- R10: three Rust geometry regressions fail before repair; the Python balanced-flow
  case independently exposes eta 0.15 versus the feasible maximum 0.13274917217635376.
  The repair solves the active-axis cross-section and retains the ellipse coupling,
  including degenerate correlation. 64 Rust market tests, 160 core + 25 integration
  workspace tests, 41 Python market/handshake cases (one optional skip), 12 npm cases,
  Rustdoc and Clippy pass. SPEC_HASH rebuilt to `5966ec7ce3cb6a50`; actual normalized
  Cargo package executes the committed-pin assertion. No historical data regenerated.
- BS3 underway: same-key synthetic plaintext recovery and unauthenticated canary
  replacement both reproduce in the old cipher. The cryptography discovery gateway
  references absent files; RFC 8452 and RustCrypto docs are the primary fallback.
  No external security audit of the integration or selected whole crate is implied.
- BS3 implementation pushed through `f0f0eb9`: code `d3bda00`, rebuilt wasm
  `34e88a8`, migration documentation `432903e`. 39 attestation tests, workspace
  excluding xtask, affected Clippy/Rustdoc/fmt, real wasm and Python compilation,
  npm 13 and MCP 8 pass. Canary-AAD removal fails the regression; restored.
  RFC 8452 vectors pass. Supply-chain gate requires real CI (cargo-deny absent
  locally). Fresh nonces plus AES-256-GCM-SIV, not a hand-written cipher; public
  commitments still expose equality, length and offline plaintext guesses.
- Confirmed post-merge main checks: Bench `0bac8f9` CI `34146697613` and npm
  `34146697646` pass. Arena R10 CI `34147229965` completed successfully with all
  job conclusions green; one npm job retained a stale in-progress status label
  despite completed_at and every step completed/success. Normal merge accepted
  without bypass; post-merge CI independently completed/success as above.
- AI2: 13 new tests validate strict JSON envelopes, native schema, sparse hold vs
  flatten vectors, actual symbol names and escaped XML delimiters. Combined local
  prompt/action suite passes 49 with one optional-verifiers module skipped; real
  CI `34148375481` passes the full Python job and fresh wheel/npm consumers.
  A hold-to-close mutation fails 8 cases. Merge tree equals tested head. No model.
  Post-merge main CI `34148604519` passed on `0e6a7a1`.
- BI7 underway: Momentum ignored its public lookback. Five pre-fix regressions
  fail; repaired behavior uses L return intervals / L+1 observed closes, with
  explicit zero targets for unavailable/invalid windows and no prior-history
  dependence. Unit checks and simulator Clippy pass; deterministic fixture and
  full-workspace impact verification pending. BM8 delayed-order semantics remain.
- BS3 final CI: `34148296186` and npm `34148296249` completed/success, including
  cargo-deny, live Docker and all three OS workspace jobs. Merge tree equals
  tested head. Momentum work continues on a separate branch descended from this
  tested repair; no dirty checkout was reset or overwritten for the merge.
- BI7 fixture review: only Momentum changes in the 4-agent x 6-run x 40-return
  synthetic input; the other three submissions are byte-identical as parsed.
  Rescoring reorders two ineligible display rows and changes field-relative
  diagnostics; all four rank eligibility/ordinal values remain false/zero. The
  independent README teaching fixture is unchanged. Historical paper artifacts
  are untouched. Two harness assumptions were exposed: the two-close dominance
  probe now explicitly uses a one-interval agent, and cheat-ordering assertions
  require a qualifying synthetic scoring control rather than assuming Momentum
  qualifies. Both harness tests and all 44 harness unit tests pass. Full gates pending.
- Next AI5 repair has been scoped, not implemented: counterfactual.py records an
  allowed preflight prefix as executed before the atomic batch check in
  PaperTradingSession.execute_decision. The repair must consume actual lifecycle/
  broker receipts, distinguish acknowledgement/unknown/partial fills, retain
  refused and exception paths, and avoid treating unknown or unsettled P&L as
  zero. Existing paper-lifecycle tests provide ScriptedBroker and in-memory
  broker seams. Schema/consumer compatibility and receipt identity need review;
  a patch that only relabels the refused prefix would not close AI5.
- BI7 complete: local workspace excluding xtask passed; CI `34149785193` and
  npm `34149785170` completed/success on `9c74ec6`, including all three OS
  workspace jobs and cargo-deny. Normal merge #15 preserves the three commits;
  resulting main tree equals the tested head, local main fast-forwarded clean.
- AI5 now in implementation on `fix/execution-receipts-2026-09-07`: five new
  regressions reproduce invented approved-prefix fills, full fills inferred from
  acknowledgements or partial fills, success inferred from unknown submissions,
  and duplicated cumulative fills on retry. Contract-first V2 design uses explicit
  availability, cumulative receipt snapshots and unique-decision aggregation.
  Initial targeted paper/ledger suite passes 82 cases. Persistence, identity,
  failure-path and compatibility review continues; not yet committed or complete.
- AI5 verification expanded: receipt-driven partial/cancel/unknown outcomes,
  failed second submissions, account-updated retries, explicit refresh and restart
  reconciliation, legacy-ID refusal, missing-state refusal, strict persistent
  snapshot validation and finite availability. Two isolated mutations (unknown
  aggregate -> zero; count every retry snapshot as a new decision) each fail two
  regressions. Workspace 160 unit + 25 integration tests, Clippy and fmt pass;
  full local Python suite passes 1,250 with 11 optional skips before three final
  terminal-zero-fill cases were added. Final rerun and CI pending. Neither mutated
  package was used as the working implementation. No broker network or model call.
- AI5 final local verification: 1,255 Python tests passed, 11 optional skips,
  two existing PettingZoo warnings. Committed code/tests `2e6b41b`, contract and
  migration `3b8ccb2`, provenance `9e65e49`; PR #19 is checking that exact head.
  Initial probe mistakenly refused its first order at the notional cap; raising
  only that fixture cap reproduced the intended approved-prefix bug before repair.
  Broker identity scopes client IDs but is not an account identity; separate
  per-account stores remain an explicit precondition, not a whole-suite identity fix.
- BM8 implementation: four regressions reproduce reversal losing to simultaneous
  old-direction confirmation, expiry losing at the exact wait boundary, illegal
  zero/one-limit parking, and u32 wait overflow. Reversal/expiry now precede
  confirmation; the clock saturates and never resets on same-direction signals.
  Core 281 unit + 5 integration + 1 doc test and Clippy pass. Full workspace pending.
- Documentation follow-up: Bench `docs/book/src/submitting.md` lists both the
  four literature baselines and seven extra primitives, then broadly says none
  has a field evaluation. Reconcile that scope with the paper's external-rules
  evidence during the final documentation audit, rather than carrying the claim
  forward unchanged.
- AI5 merged: final local Python 1,255 passed / 11 skipped; CI `34151564579`
  completed/success on exact head `9e65e49`. Provenance job API header retained
  in-progress/null while every step, including Complete job, was completed/success.
  Normal exact-head merge accepted with no override; resulting tree matches the
  tested head. Local main fast-forwarded clean. Post-merge main CI pending.
- Bench momentum post-merge CI `34150603090` and npm `34150603099` both passed
  on `09e4d31`. BM8 repair runs independently on its own branch from that main.
- Arena receipt-ledger post-merge main CI `34151833901` completed/success on
  `a205289`. Both merged batches now have successful post-merge workflow evidence.
- BM8 full local workspace excluding xtask completed successfully, including all
  four historical-field regressions (217.93 seconds). Rustdoc with warnings denied,
  formatting and diff checks pass. No scoring fixture or historical artifact changed.
- BM8 pushed in three granular commits: `4077551` code/tests, `28a218a` changelog,
  `ad2fce3` provenance. PR #16 targets main at exact head
  `ad2fce329f9f71de07aac7170db639ea08337e01`; CI discovery and merge are pending.
  Branch is clean and pushed. Do not close the combined BI7/BM8 item until this
  final repair passes applicable CI and is merged.
- PR #16 CI `34152086490` is running; all three OS workspace jobs and Windows
  reproducibility are pending. The other checks pass, including npm workflow
  `34152086702`, live Docker, Python, provenance, supply chain and scoped Lean.
  Continue polling these exact runs; no rerun or replacement is needed.
- BM8 merged: CI `34152086490` and npm `34152086702` completed/success on
  `ad2fce3`; all 17 actual Actions jobs passed. Exact-head normal merge #16 at
  `f2439fb` preserves commits and has the identical tree. Options repairs are on
  a separate branch; local main fast-forward awaits a clean checkpoint.
- BM4/BM5 in progress: three fresh regressions fail on the old implementation:
  discounted zero-volatility price/parity, Greek continuity, and false payoff
  boundedness inferred from local gamma. Checked Result APIs and an independent
  same-expiry payoff-tail classifier now pass 11 focused integration cases and
  8 existing options unit cases. CLI/WASM consumers and package checks underway.
  Math discovery references remain absent; derivation checked against QuantLib's
  zero-variance Black formula and OCC/OIC payoff descriptions. No Wolfram/Lean MCP
  is available or claimed; no statistical field or historical artifact was run.
- BM4/BM5 verification: full workspace excluding xtask passed before the final
  slope-residual refinement; all affected crates, Clippy and npm/packed consumers
  passed after it. 12 core boundary regressions, 2 CLI integration cases, 2 focused
  WASM host cases, npm 14 and MCP 9 pass. Source checked against actual raw wasm.
  Three isolated mutants fail: all tails bounded (2), lost rounding residual (1),
  discounting removed (3). The temporary copy is restored and all 12 pass again.
  No mutant touched the production repository. Rustdoc/fmt/diff checks pass; local
  mdbook is absent, so the documentation build still requires CI.
- BM4/BM5 pushed in four granular commits: `36617b2` code/tests, `14a36f4` rebuilt
  wasm, `a00daf8` documentation, `b75ccb3` provenance (166 sources / 40 artifacts).
  Boundary and availability changes are explicit Rust/API migrations, not silent
  zero-risk fallbacks. No historical numerical artifact or release version changed.
- BM6 next: multisession currently permits cycles, checks dependencies against raw
  positive lift rather than qualified retention, and labels a pooled raw-task test
  as the overall significance. Full file and all callers inspected; only the Rust
  library exports it. Require a validated DAG and qualified propagation, and do not
  relabel the raw pooled p-value as inference for the data-selected credited effect.
- Options PR #17 opened on exact head `b75ccb3bc82d3a62365567808559a793f8b34a87`.
  CI `34153881621` and npm `34153881725` are confirmed in progress. Poll these runs;
  do not merge or close BM4/BM5 until applicable exact-head gates pass. Branch clean.
  Local main fast-forwarded to `f2439fb`; its post-merge CI `34153081853` and npm
  `34153081865` both completed/success. Arena remains clean on main `a205289`.
