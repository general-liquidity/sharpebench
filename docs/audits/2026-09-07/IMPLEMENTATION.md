# Sharpe suite audit implementation

Goal started 2026-09-07. Status: paused; repairs incomplete. Checkpoint saved on 2026-09-07.

All nine checklist sections are preserved: 93 rows, 29 closed and 64 open. The
previous checkpoint had 28 closed and 65 open; BM6 is the only newly closed row.
Unchecked rows include partially implemented work and unconfirmed probes, so these
counts are checklist dispositions, not a count of independent confirmed defects.

Source baselines: [Bench `933e0c1` (0.18.4)](https://github.com/general-liquidity/sharpebench/tree/933e0c1056a2e4707b28762c294323bf05bdab65)
and [Arena `1be915f` (0.24.1)](https://github.com/general-liquidity/sharpearena/tree/1be915f330acabacd171cc350bec0def58d9e134).
Both repair efforts started on `fix/audit-2026-09-07` from clean worktrees.
The archived [audit report](README.md#original-audit-report-historical) and its
appendices describe those baselines; their findings are not a current defect list.
This checklist records the later repair dispositions.
No model downloads, model calls, new benchmark experiments, market-data acquisition,
or release publication are authorized by this implementation goal. Synthetic
regressions, existing-fixture tests, package checks and finite diagnostics are in scope.
Existing published numerical evidence stays frozen until its validity is assessed.

## Status at this checkpoint

Bench main is [`e6f2ab189622a8e27d78b0ae8183fdd7cc5e84f4`](https://github.com/general-liquidity/sharpebench/commit/e6f2ab189622a8e27d78b0ae8183fdd7cc5e84f4).
[Memory PR #18](https://github.com/general-liquidity/sharpebench/pull/18) merged normally after
[CI `34155625798`](https://github.com/general-liquidity/sharpebench/actions/runs/34155625798) and
[npm `34155625802`](https://github.com/general-liquidity/sharpebench/actions/runs/34155625802) passed
all 17 Actions jobs on tested head `ffedd3a6d505c2a3fe77bdc126d0e22877319619`.
The merged and tested trees both equal `73e73f82ac6f382f51f50087faf593067d3ff196`.

Options PR #17 was already merged at `77ea0b5`; its post-merge
[CI `34154795075`](https://github.com/general-liquidity/sharpebench/actions/runs/34154795075) and
[npm `34154795093`](https://github.com/general-liquidity/sharpebench/actions/runs/34154795093) passed.
Arena main is [`a205289cc4fa24cf788f7199480b4f44c004099b`](https://github.com/general-liquidity/sharpearena/commit/a205289cc4fa24cf788f7199480b4f44c004099b),
with successful post-merge [CI `34151833901`](https://github.com/general-liquidity/sharpearena/actions/runs/34151833901).

The final local Bench product workspace test run, excluding `xtask`, completed
successfully. Local `xtask` remains unavailable because OpenSSL development files
are absent; the actual full CI jobs passed. BM6's revised memory tests, statistical
helper tests, qualification-recomputing randomization checks and four isolated
failing mutants are recorded below. They validate the implementation under its
stated assumptions; they do not prove that supplied replicates are independent.

The full repair goal remains paused and unfinished. No model download, model
execution, new benchmark experiment, historical evidence regeneration or release
publication was performed for this documentation checkpoint.

## Completion rules

Every item needs an implementation/test reference or a specific evidence-backed
design disposition. A suspected issue is not a confirmed bug. A passing source-tree
test does not establish installed-package parity. Formal models do not establish
assumption-free empirical validity. Documentation follows verified code.
Commit explicit paths in small conventional commits, preserve unrelated edits,
and bind provenance on a clean candidate before pushing checked repair batches.
User confirmed repair branches must be merged into each product's main after
verification. Keep commit granularity through a normal merge, require all relevant
PR checks on the exact pushed head, and verify the resulting main tree. No force
push, tag or release is authorized. The original coordination ledger lives outside the
product repositories. The user requested this mirrored documentation checkpoint;
it does not authorize publishing unrelated parent-repo work.

IDs refer to the [audit README](README.md#root-reviewed-findings) (`R`),
[Bench recent/safety/methodology slices](bench-reviewer.md) (`BR`, `BS`, `BM`),
[Bench interfaces](bench-interfaces.md)/[producers](bench-producers.md) (`BI`, `BP`),
[Arena recent/interfaces](arena-reviewer.md) (`AR`, `AI`), and
[Arena diagnostics](arena-diagnostics.md)/[producers](arena-producers.md) (`AD`, `AP`). Numbering in each
appendix restarts within its named slice. Supplementary recommendations remain
listed below, not silently omitted from the goal.

## 1. Shared evidence and mathematics

- [ ] Typed run/window/seed/configuration/outcome identity and complete-grid validation.
- [ ] R07, BM10: versioned canonical numeric JSON and unambiguous digest framing.
- [ ] R06, AI1: exact frozen contracts and canonical settlements across producers/consumers.
- [ ] R03: arithmetic fixed in Bench `0cd7d37` and Arena `cdd877e`; dependency propagation and historical impact assessment remain.
- [ ] R02: checked finite/domain statistical boundaries and typed availability.
- [ ] R05: independent-block requirements; insufficient-support status for forecast inference.
- [ ] R08: unit/rule/target strata or precommitted dimensionless aggregation.
- [ ] BR2, AR2: append-only attempt ledger preserving failed/retried cost and timing.
- [ ] R12: directional compatibility and versioned extension policy.

## 2. Bench ranking and verification

- [x] R01: original process evidence survives common-support restriction. Bench `dff0d84`.
- [ ] R15, BI3: complete expected geometry, keyed support rather than positional ambiguity.
- [ ] BM1: observed search-footprint floors and valid/unavailable PBO.
- [ ] BM2, BS6: displayed board content/count/order and trusted terminal receipt anchor.
- [ ] BI2, BI8: identical declared mandates and rank-context explanations on all surfaces.
- [ ] BR1: effective nonsecret configuration identity on resume; separate credential handling.

## 3. Bench simulation and diagnostics

- [ ] R09: Bench inventory/cash fix in `4378ad4`; propagation to Arena remains.
- [x] R19: masking preserves dividends and total-return economics. Bench `1bd06e2`.
- [x] R20: finite/domain CSV validation, duplicates, dividend missingness. Bench `1bd06e2`, `d882803`; signed raw closes remain permitted, consistent with the archived WTI data contract.
- [ ] BI3, BI4: shared CSV support and explicit columns/run identities.
- [ ] BM3: dated role/durability support and correct IC versus return-trend descriptions.
- [x] BM6: validated DAG and qualified transitive prerequisites in Bench `7719f53`; complete-chain inference uses the checked paired-randomization helper `e68d1d8`, recomputing qualification under whole-arm swaps with explicit unavailable single-chain inference. Migration `1f64ad5`, provenance `ffedd3a`. PR #18 merged normally at `e6f2ab1` after all 17 Actions jobs passed on tested `ffedd3a`; merged tree equals tested tree. Replicate independence and arm exchangeability remain assumptions.
- [ ] Memory supplementary: matching oracle/task populations, finite parameters, oracle floor.
- [ ] BM7: raw-candidate lineage/rediscovery validation and identity binding.
- [x] BM4, BM5: checked options pricing and separate payoff-tail classification in Bench `36617b2`; rebuilt wasm, migration docs and provenance through `b75ccb3`. PR #17 merged at `77ea0b5` after all 17 Actions jobs passed on that exact head; merged tree equals tested tree. This classifies the supplied same-expiry payoff, not intermediate margin or assignment risk.
- [x] BI7, BM8: Momentum lookback fixed in Bench `259555e`, PR #15 merged at `09e4d31`. Pending-trade precedence fixed in `4077551`, PR #16 merged at `f2439fb` after all 17 Actions jobs passed on `ad2fce3`.
- [ ] BI6: team-member resource accounting and concurrency semantics.
- [ ] BM9: normalized area aggregation and duplicate handling in briefings.
- [ ] Supplementary: budget support/search population, plateau terminology, zero-return versus no-trade, regime reversals.
- [ ] Supplementary: aligned noncausal attribution, unknown process checks, turnover semantics, configured disqualification rollups.
- [ ] Supplementary: dated rediscovery and explicit transitive clone-cluster semantics.

## 4. Bench transport/security

- [x] BS1: bounded stdout queue and total accepted-output budget, finite flood regression. Bench `abdabef`, PR #12 merged at `7a7d155` after all 17 Actions checks passed.
- [x] BS2: absolute request deadline includes blocking stdin write/flush. Bench `b60890f`, PR #12 merged.
- [x] BS3: authenticated V2 sealing and versioned migration. Bench `d3bda00`, PR #14 merged at `de8ec73` after all 17 Actions jobs passed on `f0f0eb9`; independent security audit is not claimed.
- [x] BS4, BI10: malformed hex and Unicode IDs cannot panic. Bench `6d1becc`.
- [x] BI5, BI9: checked integer conversion and strict boolean/binary inputs. Bench `72afdfa`, `a1bee0b`, rebuilt wasm `b9c1af4`; PR #13 merged at `0bac8f9` after all 17 Actions checks passed. This closes trial-count narrowing and nonbinary outcome coercion, not the separate CSV alignment/empty-row issues.
- [x] BS5: relative checkpoint parent, exclusively owned temporary siblings and bounded collision retries; file sync and Unix directory sync scope explicit. Bench `e5ae7a8`.

## 5. Arena state and execution

- [x] R04: distribution-stable SPEC_HASH and fresh package-consumer checks. Canonical suite dependency record, epoch 2, actual normalized Cargo package test, Python/npm pins and rebuilt wasm verified. Broader package-surface verification remains in section 8.
- [x] AI4, AI7: complete native/book/cursor/terminal/reward reset. Arena `fae8e1b`, `53e963a`.
- [x] AI6, AI8: every scenario knob survives reconstruction; PettingZoo honors difficulty. Arena `4141786`, `e7ecdb8`. Native restore also retains the full action prefix.
- [x] R18: unconditional per-environment readback, reporting deduplication separate. Arena `6e57c3c`.
- [x] AI2: executable prompt examples generated from the actual action contract. Arena `8f9a2a8`, PR #18 merged at `0e6a7a1` after all 11 Actions jobs passed on `b786088`.
- [x] AI10: exact Gym vector shape, finiteness and policy/bounds validation. Arena `e51937c`.
- [ ] AI9: failure/horizon accounting prevents profitable-prefix abort selection.
- [x] AI3: trusted full snapshot separated from default step/action-only export. Arena `2fcf7ed`; private snapshots and pickles remain operator-only. This omits private reconstruction data, not information deliberately encoded in actions. PR #15 merged at `da47780` after all 11 Actions checks passed. `dfd8891` additionally makes exact-action restoration transactional.
- [x] AI5: receipt-backed V2 execution evidence in Arena `2e6b41b`, PR #19 merged at `a205289`. Cumulative fill snapshots preserve unknown outcomes and avoid retry double-counting; local reference-price marks are not realized P&L or independent broker attestations.
- [ ] R16: content-bound promoted gold inputs rerun the producer.
- [x] R10: constrained ellipse fix in Arena `05078a7`; PR #17 merged at `d3f457a`, post-merge main CI `34147591625` passed. Finite nonnegative centre/cost and representable floating-point intermediates remain documented preconditions, not an assumption-free proof.

## 6. Arena metrics and telemetry

- [x] AD2: initial NAV contributes to full-episode metrics. Arena `919eb32`.
- [x] AD1: risk-radar drawdown direction and real anchors. Arena `ed40d76`.
- [x] AD5: target downside RMS/Sortino agreement and constant-loss boundaries. Arena `919eb32`.
- [x] AD3: failed/missing/nonfinite episodes cannot default to clean. Arena `fc01d5f`.
- [x] R17: actual winner/confidence rendering and no false equivalence language. Arena `efb97be`, PR #16 merged at `2d85aac` after all 11 Actions checks passed; post-merge main CI `34145481182` passed.
- [x] AD7: resource penalties monotonic for signed base scores. Arena `919eb32`.
- [x] AD6: prior-action snapshot does not alias caller arrays. Arena `919eb32`.
- [ ] AR1, AR3: strict optional telemetry and reconciled counts/reasoning/steps/cadence.
- [ ] AR4: ordered per-measurement duration value/unit/source provenance.
- [ ] AD4: negative controls match claimed horizon/objective and are distinct.
- [x] AD8: exact representable PRNG output grid and inversion round trips. Arena `3d0aa82`.

## 7. Evidence producers

- [ ] R15: declared unique Cartesian sweep support, not just row count.
- [ ] BP1: requested/effective model identity and no silent substitution.
- [ ] BP2: collision-free calibration seed tuples with explicit CRN policy.
- [ ] BP3: full effective request/scaffold/parser cache identity.
- [ ] BP4: explicit hermetic passthrough/readback of supported nonsecret controls.
- [ ] BP5: collision-resistant model artifact identifiers.
- [ ] BP6: invalid selections/missing datasets cannot publish empty-complete outputs.
- [ ] BP7: strict JSONL and complete figure/summary support.
- [ ] BP8: eligibility union by identity, not overlapping-count sum.
- [ ] AP1: oracle/causal equal bars, warmup and costs.
- [ ] AP2: provenance includes actual JS producer and manifest dependencies.
- [ ] AP3: frozen historical references versus fresh-path parity named and tested accurately.
- [ ] AP4: persist pre-execution commitment separately from reveal; witness limits explicit.
- [ ] AP5: full actual scenario/trajectory replay, not a different first-bar proxy.
- [ ] AP6: complete frozen-input figure renderer registry.
- [ ] R21: direct POSIX/Windows Node argument forwarding.

## 8. Verification and publication documents

- [ ] Single checked Bench publish graph for CI/manual routes (including memory crate).
- [ ] Fresh Rust/Python/npm/WASM package consumers and cross-language conformance fixtures.
- [ ] Discriminating regressions, paired boundaries, targeted mutations, full product gates.
- [ ] Historical numerical impact assessment; flag/withdraw claims not verifiable without new experiments.
- [ ] R11: actual LOB price-time/multilevel mechanism and separate auction query.
- [ ] R13: retain per-run/pooled drawdown caps; correct seed-averaging inequality claim.
- [ ] R14: correct episode-count denominator, no unverified wall-time promise.
- [ ] Correct snapshot complexity claim; optimization only if justified.
- [ ] Readmes/API docs/changelogs/papers updated after implementation, no README em dashes or smolvm sections.
- [ ] Rebuilt papers, checked references/layout and fresh provenance; granular verified commits pushed, no release tags.

## 9. Optional architecture and unconfirmed probes

- [ ] Cross-suite versioned conformance fixtures without a cyclic whole-package dependency.
- [ ] Versioned opt-in lifecycle-certified rank mode; do not silently change legacy protocol.
- [ ] Production-linked reset/step/terminal/fill/accounting properties and scoped Lean models.
- [ ] Assess and implement justified updater signature verification or explicitly narrow unsupported claims.
- [ ] Snapshot sharing decision grounded in actual complexity/use, not invented benchmark performance.
- [ ] Probe HTTP connect/slow-trickle absolute deadline before classification.
- [ ] Probe checked token accounting overflow.
- [ ] Probe Docker ENTRYPOINT versus effective readiness command.
- [ ] Probe child OOM versus surviving wrapper classification.

## Verification log

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
