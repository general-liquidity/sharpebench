# Sharpe suite audit implementation

Goal started 2026-09-07. Plan restructured 2026-09-08 after an independent
review of progress against both repositories. This file is mirrored
byte-for-byte in the Bench and Arena repositories; edit both or neither.

Status: 100 checklist rows, all 100 closed. The restructured plan opened at 39.
All of it is on the `fix/audit-batch-a-b-2026-09-08` branch in each repository,
Bench PR #25 and Arena PR #26, and is **not merged**. Every commit cited on a
closed row was verified to be reachable in the named repository, and every new
regression was mutation-checked in an isolated copy before its row was closed.
Five deferred items are listed at the end without checkboxes; they were never
part of the 100.

**Closed is not the same as finished.** Each row records what its repair does
and does not establish, and several close on a disposition rather than a code
change: a finding shown to be already repaired, a description corrected where
the arithmetic was right, or a probe run on a real Docker daemon and found not
to reproduce. Reading a row's own text matters more than reading this count.
Three consequences are deliberately left as decisions rather than folded into a
fix: migrating contract digests to the versioned canonical form, which would
invalidate 24 committed digests; renaming the serialized zero-mass and
budget-onset keys, which moves a published Python key and needs rebuilt npm and
WebAssembly artifacts; and merging these branches at all.

Published numerical evidence stays frozen. Where a repair changes what a
producer would now compute, both papers say so rather than regenerating the
number. Two committed artifacts will not reproduce and are recorded in the
papers: the synthetic witness, whose seed expression collided, and the
forecast-quality tutorial fixture, which was itself an instance of R05.

Defects found while repairing that the original audit did not report: the
release workflow's `verify` job ran with `if: always()` and did not inspect the
new publish gate, so a blocked release would have reported success; the paper
described the endogenous market as single-price when each agent pays a
size-dependent execution price; a commitment preimage could have a separator
moved between the artifact digest and the salt, letting an entrant reveal a
different frozen artifact against the same published hash; and the in-memory
sweep driver hardcoded a single attempt exactly as the checkpoint did. Each is
fixed in the commits cited on its row.

Source baselines: [Bench `933e0c1` (0.18.4)](https://github.com/general-liquidity/sharpebench/tree/933e0c1056a2e4707b28762c294323bf05bdab65)
and [Arena `1be915f` (0.24.1)](https://github.com/general-liquidity/sharpearena/tree/1be915f330acabacd171cc350bec0def58d9e134).
Current verified checkpoints: Bench `7cfc954` (PR #22) plus PR #24 and the
contract-inventory fix `c5fe201`; Arena `2130fdc` (PR #22) plus PRs #23 to #25
and the logit-validator test `3cc00e5`. The archived
[audit report](README.md#original-audit-report-historical) describes the
baselines; its findings are not a current defect list. The chronological
repair diary that used to live at the bottom of this file is preserved
unchanged in [VERIFICATION-LOG.md](VERIFICATION-LOG.md).

## Scope and rules

No model downloads, model calls, new benchmark experiments, market-data
acquisition, or release publication are authorized by this goal. Synthetic
regressions, existing-fixture tests, package checks and finite diagnostics are
in scope. Existing published numerical evidence stays frozen; where repaired
arithmetic changes what a published number would be, the paper states that
rather than regenerating the number.

Every item needs an implementation/test reference or a specific
evidence-backed design disposition. A suspected issue is not a confirmed bug.
A passing source-tree test does not establish installed-package parity.
Formal models do not establish assumption-free empirical validity.
Documentation follows verified code. Commit explicit paths in small
conventional commits, preserve unrelated edits, and bind provenance on a clean
candidate before pushing. Repair branches merge into main by normal merge after
all relevant PR checks pass on the exact pushed head; verify the resulting main
tree. No force push, tag, release or `Co-Authored-By` trailer.

IDs refer to the [audit README](README.md#root-reviewed-findings) (`R`),
[Bench recent/safety/methodology slices](bench-reviewer.md) (`BR`, `BS`, `BM`),
[Bench interfaces](bench-interfaces.md)/[producers](bench-producers.md) (`BI`, `BP`),
[Arena recent/interfaces](arena-reviewer.md) (`AR`, `AI`), and
[Arena diagnostics](arena-diagnostics.md)/[producers](arena-producers.md) (`AD`, `AP`).

## Corrections from the 2026-09-08 review

These change how open rows are read. They do not reopen closed rows.

1. **Historical impact is a paper caveat, not a rerun.** R03 (moment
   arithmetic), R09 (cash/inventory trajectories) and BI7 (Momentum lookback)
   changed what the shipped producers would compute. Experiments are not
   authorized, so the section 8 impact row resolves by naming, in each paper,
   which published tables were produced under the pre-repair code and which
   direction each repair moves them where that is known. It is the most
   important open row.
2. **R11 is partly stale.** `paper/sections/03-environment.tex` line 104
   already describes price-time priority and the call-auction uncross, and
   `crates/sharpearena/src/lob_market.rs` implements FIFO per level. The
   residual defect is that line 106 says both market models clear once per
   step at a single price, which contradicts line 104. This is a paragraph fix.
3. **The publish graph gap is confirmed.** Bench `release.yml` gates every job
   on `validate_tag` only; nothing depends on `ci.yml`. This is why tags were
   cut with red CI. The fix is one dependency edge, not a redesign.
4. **Grab-bag rows are split.** The three "Supplementary" rows in section 3
   bundled four unrelated items each; they are now single rows. The four
   "Probe" rows in section 9 are hypotheses, not defects: each gets one bounded
   probe and is then either promoted to a defect row or deleted.
5. **Producer rows (section 7) matter before the next field, not now.** They
   harden the evidence pipeline and do nothing for shipped numbers. Three of
   them affect existing claims (BP6, BP8, AP5) and are pulled forward; the rest
   wait until a field run is scheduled.
6. **Updater signature verification resolves by narrowing the claim.**
   `crates/sharpebench-cli/src/update.rs` verifies a SHA-256 published on the
   same channel as the binary, which is integrity, not authenticity. Narrow the
   README now; a signed manifest is a separate feature request.
7. **BI3 appears as three rows** (sections 1, 2 and 3). It is one batch: keyed
   run/window/seed identity through the CSV import path and the legacy `Run`
   array.

## Batch order for open rows

Work the batches in order. Each batch is one or more PRs; a batch is done when
every row in it is closed under the rules above.

### Batch A: paper pass (both products)

- [x] Both papers now record the repairs that landed after their evidence was frozen. Bench `1edcec6` names the moment estimators behind every PSR and DSR value, the zero-target cross through flat, the momentum lookback, and the unresolved commodities dispersion divergence; Arena `2d0a804` names the moment estimators, the episode-metric and failure-disposition changes, the reward floor and the refused infeasible market parameter. BM8 is excluded: its log records no fixture or historical artifact change. Nothing was regenerated.
- [x] R11: each market model's clearing is now described separately. Arena `320cfb4`. The book matches under price-time priority across levels and its uncross is a separate query; the endogenous market shares one reference mid but charges each agent a size-dependent execution price, so neither model is uniform-price. Discrete time, not single-price clearing, is what rules out latency races, and that scope statement is kept on that ground.
- [x] R13: the containment claim is corrected in the paper (Bench `e1db324`) and in the two API doc comments that repeated it, with the anti-correlated-seed case pinned as a regression (Bench `7eaa285`). Both caps are retained and applied; the mutation check confirms the test fails when the per-run fold is replaced by the pooled figure.
- [x] R14: a single agent on one tier is 256 of 4,608 episodes, one eighteenth; one sixth is a single agent across all three tiers. Stated as count ratios without a wall-clock promise. Arena `7492135`.
- [x] Shard assembly is now described as identical record for record rather than byte for byte, because the fixed grid-key order differs from the serial producer's score-rank order. Verified against `assemble_sweep.py` and `test_sweep_grid.py`, not only the log. Bench `02ccbbe`.
- [x] The universal "no field evaluation" claim is narrowed: the four literature rules were scored on all nine frozen datasets under three cost profiles with no rank-eligible cell in 351 records, while the seven further primitives in `sharpebench_core::entrants` have no field evaluation. Note that the four rules are implemented in the harness example, not in `entrants`.
- [x] `clone_state` copies a book holding shares, cash, RNG, an accumulating trace and pending orders, so it is not constant time; what it saves is the replay. Corrected in the mdBook page, both changelogs, four Bench rustdoc sites and one Arena comment. Bench `2c7356d`, Arena `a40b6a3`. No optimization implemented: shared immutable trace prefixes would need a measurement, and none was run.
- [x] No README in either repository contains an em dash or a smolvm section; the two changelog mentions of smolvm are accurate historical entries and were kept. 34 em dashes were removed from user-facing documentation across 11 files. Bench `f2dfdf7`, Arena `ac54362`. Archived review records under `paper/review/` and the internal assessment documents were deliberately left alone: rewriting a historical record is out of scope.
- [x] Both papers rebuilt from the corrected sections with no undefined reference and no layout warning, and both PDFs are committed. Bench `6108306`, Arena `7b13ae2`. Provenance is rebound and clean in both, 181 sources for Bench and 146 for Arena, and the CI provenance gate passes on both branches. Commits are granular and pushed. No release tag was created; the newest tags remain `v0.18.4` and `v0.24.1`.

### Batch B: publication gating

- [x] `require_green_ci` polls the `ci.yml` runs at the exact release commit and passes only on a completed success, and every publishing job plus the final aggregation depends on it. Bench `59d985c`. It waits rather than failing fast because the driver pushes the version commit and its tag together, and fails as soon as every run at that commit is terminal without success. The `workflow_dispatch` recovery route resolves the same commit, so there is no bypass. A second defect was found and fixed in the same pass: `verify` runs with `if: always()` and its result loop did not inspect the gate, so a blocked release would have reported success. Both publish lists carry all 12 crates including `sharpebench-memory`.
- [x] The updater's checksum is described as an integrity check against corruption and mismatched assets, not authenticity: the digest ships in the same release over the same connection. The SLSA attestation is named as the out-of-band route. Bench `d624ef4`. No signature verification was implemented.
- [x] Audited per surface before adding anything. Already covered: the npm tarball offline install and the WASM execution in both products, and Arena's wheel install. Genuinely absent and now added: a packaged-crate consumer in both products, and a wheel-install-import job in Bench. Bench `dfcc163`, Arena `bf1670c`. The Arena consumer replays the conformance kit out of the archive, which also proves the contract fixtures ship. Both consumer scripts were mutation-probed and fail when the expected value or the kit version is perturbed.
- [x] `contract/conformance-kit.v1.json` is the versioned index, naming the fixtures, the schemas and the contract version, and Rust, Python and npm each check it. Arena `fcd5400`. Every surface reads the same files by repository path, so no package depends on another. Before this, only Rust read the fixtures and the set carried no version at all.

### Batch C: run identity (BI3, one batch)

- [x] `run_identity` gives a run a typed `RunKey` of window and seed plus optional period identities, and `parse_keyed_field` requires the complete window by seed product exactly once across every agent. Bench `14150a6`. Completeness is enforced as a safety property, not a convenience: restricting to shared support lets a partial peer rescope every other entrant's evidence and can drop the cell carrying another entrant's process violation, which is R01.
- [x] R15, BI3: the Rust ranking path takes keyed support through `score --require-run-keys`, and accepted fields are reordered into one canonical cell order so the positional reads in `composite` become keyed reads without touching that module. Bench `14150a6`; the assembler portion was closed in `c9dc85f`. Two residuals are deliberate: scoring without the flag keeps the legacy positional path, because making refusal the default would break the committed goldens and frozen evidence, and the WASM, npm and Python surfaces still rank without identity validation, which needs new surface and rebuilt artifacts.
- [x] BI3, BI4: wide import reads header cells as window identities and a leading period column as the period axis; long import reads optional run, seed and period columns. Identity is never manufactured from column position, so a file declaring none produces an unkeyed import that says so and is refused by the keyed scorer. An unkeyed legacy `Run` array is refused rather than aligned. Bench `14150a6`; BI4's strict readers were closed in `7f80fee`.
- [x] BR1: the checkpoint bound the passed-through variable names but not their values, so keeping `SHARPEBENCH_AGENT_ENV=AGENT_MODE` and changing `AGENT_MODE` from conservative to aggressive left `invocation_sha256` unchanged, and completed cells of one policy could be resumed into the other and pooled as one result. Each name now contributes its value, with a credential-shaped or declared-secret name contributing a placeholder so rotating a token does not invalidate a checkpoint and no secret reaches the digest. Bench `14150a6` and `4809747`. The `SweepIdentity` doc no longer claims to bind every condition that can change a sweep's result.

### Batch D: producer rows that touch existing claims

- [x] BP6: closed on both sides. In the Rust producers an optional dataset selector was never validated against the table, so a misspelled one skipped every dataset and reached the normal publication path with zero records under the ordinary filename and a zero exit, and a dataset that failed to load was a warning plus a continue. Selectors are now resolved before any output is opened, each run stages through a `.partial` name, and publication requires the planned support to have been evaluated, with the sweep also checking the full declared grid. Bench `df078c3`. The figure producer's three publish-on-absent-support paths are in `69e9caa`. Failure was chosen over an incompleteness manifest because every consumer keys on file presence and record content.
- [x] BP8: the annotation summed two path marginals, double counting any cell eligible on both and able to exceed its own denominator. It now unions over `(dataset, agent_id)` and cross-checks each marginal against the independently stored summary. Bench `69e9caa`. On the frozen records both marginals are zero, so regenerating all four figures under a fixed `SOURCE_DATE_EPOCH` gives byte-identical PDFs.
- [x] AP5: the reveal compared only each symbol's opening close in a two-day calm environment against a declared evaluation of 120 days at the hard tier. It now regenerates the complete tape for both seeds of every slot and requires equality on every bar and symbol. Arena `dc86f76`. The full check was measured at 0.14 s before choosing it over weakening the claim. The frozen count of 16 was measured under the old check and is not evidence for the new one; the paper says so in `ff057b0`.
- [x] AP3: F6's gate between two current code paths was reported as agreement with the committed vectors. The gate now carries its real name and a separate frozen-reference comparison reads the committed artifact before it is overwritten, reporting rather than raising so parity is neither assumed nor silently enforced. Arena `dc86f76`.

### Batch E: shared mathematics and contracts

- [x] R07, BM10: `sharpebench/canonical-json/v1` specifies the numeric form once, with fixtures at the exponent boundaries, signed zero, integer-looking floats and Unicode. Python rendered `1e-05` where Rust rendered `0.00001`, a fixed-point versus exponential difference that exponent padding could never reconcile. A preimage carries its version, so a digest under one form is never silently comparable to one under another. BM10's commitment preimage pasted four fields between literal separators, so a separator could move from one into the next; the shift between the artifact digest and the salt is exploitable because neither is carried in the clear, letting an entrant reveal a different frozen artifact against the same published hash. Fields are now length-prefixed under a versioned domain. Bench `1ff5a32`. Contract digests are deliberately NOT migrated: the committed forecast field pins 24 of them, and that is a declared migration needing its own decision.
- [x] R06, AI1: both halves closed. Arena binds each document to the frozen contract bytes and settles every agent from one canonical record per contract (Arena `1848f82`). Bench, which accepts documents from any producer, now content-addresses the realized outcome and its availability time, retains them past scoring, and refuses to difference losses when an identical contract digest carries different outcomes. Bench `b66f942`.
- [x] R03 propagation recorded rather than performed. Verified by ancestry: `0cd7d37` is an ancestor of neither `v0.15.0` nor `v0.18.4`, so the corrected moments are unreleased, and Arena's pin of `=0.15.0` cannot carry them. Both changelogs state this and that no artifact was rescored. Bench `a81c5ed`, Arena `6823c60`.
- [x] R09 propagation recorded on the same evidence: `4378ad4` is an ancestor of neither `v0.15.0` nor `v0.18.4`. Same changelog entries as R03.
- [x] R02 closed. The first half landed in `895a623` and the agreement and downside legs in `70229a1`; the data-snooping family closes in Bench `1aa8a14`.
- [x] R05: two one-contract documents produced one settlement block, a zero-width interval and familywise significance at p = 1/401, and raising the replication count only made that look smaller without adding evidence. Resampling B blocks with replacement lands on a single repeated block with probability B^(1-B), so a level finer than that mass is finer than the law can resolve; at the default alpha the bar is met from four blocks. The interval and p-values are withheld with the reason recorded rather than coerced, and Holm keeps a withheld comparison in the family size so it cannot inflate its neighbours. Bench `b66f942`. This is a necessary condition on the reported level, not a certificate of calibration. The shipped tutorial fixture was itself an instance of the defect and is regenerated; the frozen field has six blocks and is unchanged.
- [x] R12: an optional default lets a reader accept an older message, not a newer one, because the published schemas set `additionalProperties: false`. Compatibility is now stated by direction: decisions travel to the harness, always the newer reader, so an added decision field needs no coordination, while an added observation field reaches an older reader and needs version negotiation, a parallel namespace or a declared-unvalidated envelope. The minor-bump rule is split the same way. Arena `118ddc4`.
- [x] BR2, AR2: both halves closed. Arena in `88d054a`, where a failed request reached `continue` before accounting and a resumed completion replaced its failed attempt. Bench in `a2dc1bf`: the retry path returned the completed run and dropped everything before it, the checkpoint had no attempt field at all, and an eventual agent fault was logged as one attempt however many transport failures preceded it, so a cell that failed twice before completing reported the cost of the completion alone. The in-memory driver had the same hardcoded single attempt, which the finding did not name. Every attempt is now timed and recorded, an unobserved duration is typed rather than summed as zero, a completion is appended after the attempts it superseded, and a terminal cell with no attempt evidence is refused. The checkpoint schema goes to 3 so a pre-ledger file is refused rather than resumed with its spend read as zero. Totals sit beside the scored pool and never enter a score, a rank or a pass^k pool. The architecture audit's claim that retries already preserved prior evidence is corrected in `c86267f`.

### Batch F: remaining Bench diagnostics

- [x] BM1: the FULL headline deflated the winner at the caller's declared `n_trials` while the caller selected that winner out of the whole field, and the Python entry point defaults to one trial and picks the highest Sharpe itself, so a search-aware report priced a single trial for a search over every candidate. The floor is now `max(declared, observed field size)`, the rule `rank` already applies in the core, and it only ever raises. The FULL path also goes through the PBO status API, so an unestimable matrix reports why instead of returning 1.0. Bench `5fd1a7a`, with the status API itself in `879f370`.
- [x] BM2, BS6: board verification claimed to prove the integrity of the published scores and compared only the spec payload, so rewriting every displayed score left the verifier's answer unchanged. And a genesis-anchored chain is prefix-closed, so removing its final records left a document that still verified, against a module doc claiming a dropped row breaks it. `ChainReceipt` now signs the record count and terminal signature under the chain's own scheme on both the HMAC and Ed25519 surfaces, a document with no anchor is refused rather than reported complete, and each displayed score is bound to the link that signed it by content, count and order. Bench `879f370`, with the CLI wired in `acf7c54` because `sharpebench verify` was still running the chain-only check. Arena's `state.json` publication-order anchor is a separate arena-crate gap and stays open.
- [x] BM3: role attribution truncated every run to the shortest and averaged by run ordinal while claiming period-by-period alignment, so a 40-period run deleted the final 40 periods of an 80-period run even when that tail carried the result. Streams are now built window by window on the axis the pooled track uses. A class is reported only where it is separable from its window: with one execution per window the class stream is the team stream and every loading is 1.0 by construction, which is exactly what both golden fixtures published, so the diagnostic carried no information and those entries are now empty. Durability had the same defect as a computation error, not a naming one, since permuting seeds inside a window changed the reported half-life with no change in economic history. The quantity is return drift, not information-coefficient decay, and the paper and book now say so. Bench `06a1982`. No score, half-life or eligibility value moves.
- [x] BM7: the declared branch checked the declaration against itself and against earlier records and never read `raw_candidate`, so a record could hash one candidate and display the lineage of another while its digest still verified. The success fixture proved the gap by passing: it hashed a candidate with no lineage field while the record displayed one. The declaration is now bound to the hashed bytes. Bench `f1b4a67`. Counting before dedup, which the paper's honor-system claim rests on, was verified sound and needed no change.
- [x] BI6: a team kept only its members' orders and reported no cost, so a team of paid agents showed zero compute spend even when every member supplied one, and its cost-normalized columns went unavailable rather than reflecting real expenditure. Member costs are now summed onto the consensus decision with the unit recorded, and mixed denominations are refused rather than reduced to a dollar total that would silently drop a token reporter's entire spend. Bench `de7ef82`. The concurrency semantics the sum assumes are documented and pinned; latency is neither summed nor scored.
- [x] Memory: the oracle series was never length-checked against the paired arms, so `fraction_of_ceiling` could divide a lift from one task mix by a ceiling gap from another. The documented zero floor was implemented as a near-zero guard on the absolute gap, so an oracle below baseline kept the ratio's sign and a retrieval arm that also lost ground reported a favourable positive fraction. Scores, costs and alpha are now finite-checked at the boundary on the poisoning leg too. Bench `50e147b`. Equal length cannot establish identical task identities; that stays an explicit caller contract.
- [x] Budget support and search population: the selection footprint was already sound, re-deflating the peak at base trials plus budget points, which only ever raises the bar. Comparable support across budgets was never stated and the module cannot check it, since it receives returns and not dates, so the caller contract and the per-point sample size are now written down. Bench `637852e`. Documentation only.
- [x] Plateau terminology: the onset predicate is non-strict, so an honest plateau set the overfit marker while the field claimed more compute had lowered held-out edge. The pre-existing test hedged on exactly that case, which was the tree admitting its own uncertainty. It is documented as a non-improvement marker that is not uncertainty-tested, and the test now pins the plateau setting it with no point declining. Bench `637852e`. Renaming the published key is a separate decision.
- [x] Zero-return versus no-trade: the split filters on magnitude alone, so it cannot separate sitting out from a position that went nowhere or a period whose gain went to fees. No trade or position flag reaches the module, yet the documentation said it identified inactivity. Core, CLI columns and the book now say near-zero-return mass. Bench `637852e`. The wire field name is unchanged pending the npm and WASM surface decision.
- [x] Regime reversals: a reversal was reported only when the pooled gap had a sign to contradict, so exact cancellation, the case pooling hides best, reported none. The module's own motivating fixture proved it by asserting the two opposite regime signs and pointedly never asserting the verdict. A tied pooled gap now lists every counted regime when both signs are present. Bench `26a7b42`.
- [x] Aligned noncausal attribution: the regression pairs by index and silently truncates, and its two callers differed, with the composite path trimming to a common prefix and the role path not. The pairing is now explicit at the call site and asserted in debug, and the loading is documented as a marginal association from a univariate fit against a non-orthogonal regressor, which does not decompose the return additively and does not identify cause. Bench `637852e` and `14a4729`.
- [x] Unknown process checks: a point-in-time arm that made no recalls scored perfect compliance and counted as fully compliant, indistinguishable from an audited clean arm, and the suite rollup could not see it. A non-finite allocation cap made the leverage comparison false for every input, reporting a check that never ran. Both fail closed now. Bench `26a7b42`. Confabulation already separated unresolved from resolved and needed no change.
- [x] Turnover semantics: the arithmetic is churn in the stated target weights and is correct; the documentation called it realized. No price reaches the module, so drift between rebalances is unmodelled and a repeated target scores zero churn where a real account traded. Bench `637852e`. Documentation only.
- [x] Configured disqualification rollups: the rollup built default thresholds internally, so an explanation of a board scored at other bars could contradict that board. It now takes the board's own thresholds. Bench `26a7b42`. The CLI and WASM paths already passed them, so only the rollup could disagree.
- [x] Dated rediscovery: similarity is positional with unequal lengths truncated rather than intersected on a calendar, and nothing reads a date. The dated-alignment contract and the advisory framing are now explicit. Bench `637852e`.
- [x] Explicit transitive clone-cluster semantics: single linkage means membership is reachability through a chain of near-clone pairs, not similarity between every pair, so a cluster can join endpoints that are not clones of each other. Documented with why that trade-off suits the vote collapse, and pinned by a regression where three directions four degrees apart cluster while the endpoints sit below the threshold. Bench `637852e`.

### Batch G: remaining Arena telemetry

- [x] AR1, AR3: missing and null provider fields were coerced to zero before validation, so an absent count and a real zero were indistinguishable and a negative fraction truncated to zero; a null reasoning count read as provider-reported because availability tested key presence. Strict readers now return not-reported, refuse a present non-integer, and derive availability. Validation reconciles what it claims to: steps must equal the realized return count, and reasoning observations must be one per request and sum to the reported total. Arena `8fa7a54`. No committed artifact carries these fields, so no frozen number changes.
- [x] AR4: durations are recorded per measurement in order with value, unit and source, the `unspecified` default is gone and the source is validated against a closed set. Percentiles are reported per observing clock, so a p95 in a mixed cell is attributable to backend compute time or host elapsed time instead of pooled across both. Arena `8fa7a54`.

### Batch H: producer rows for the next field run

Do these before any field is scheduled; they are not required for the current
papers.

- [x] BP1: the shim fell back to an unversioned alias when the requested model was unavailable, changing the evaluated policy without changing the published identity. It now fails the run, accepts only the requested id or a versioned expansion, and stamps both identities on every cached decision. Bench `f1ac409`.
- [x] BP2: the seed expression overlapped the calibration-member and execution-seed indices, so the five zero-edge calibrators that fix the dispersion bar shared draws, giving 13 distinct streams per window where the design calls for 40. Seeds now come from an unambiguous domain, member, window and execution-seed tuple, with common random numbers across the edge sweep deliberately preserved. Bench `f1ac409`. The committed witness evidence will not reproduce; the paper records that in `b993016`.
- [x] BP3: cache identity hashed only model and prompt, so a decision taken under a different system prompt, temperature, token budget or scaffold version was replayed as the same request. It is now the digest of the whole effective request plus an explicit scaffold version, and a disagreeing record is dropped and counted. Bench `f1ac409`.
- [x] BP4: the example stripped the four cost controls its own instructions tell operators to export. They now pass through the hermetic spawn and their effective values are recorded, with the credential bound by presence and never by value, following the same secret shape as `14150a6`. Bench `f1ac409`.
- [x] BP5: sanitized tags folded non-injectively, so `a:b` and `a-b` shared one agent id, one identity artifact and one first-match metadata join. The encoding is injective and a colliding or repeated tag is refused before any model starts. Bench `f1ac409`.
- [x] BP7: the loader silently discarded truncated and non-object records. Every nonempty line is now parsed strictly, and the thousand-agent field size is derived from the records and their summaries, which must agree with each other and across datasets, rather than asserted in the axis label. Bench `f1ac409`. Verified byte-safe against all 19 committed JSONL files.
- [x] AP1: the predictability oracle was scored on the full tape while the two causal predictors started at the warmup bar, so the reported deflated-Sharpe gap mixed predictive power with 30 extra scored bars and a prefix the others never traded. The oracle is masked to the same window, a run whose adversaries disagree on support fails rather than reporting the gap, and the window and costs are serialized beside the numbers. Arena `8ce15a1`. Replaying the frozen tapes shows the published gap unaffected past the sixth decimal, because the oracle's deflated Sharpe saturates at 1.0 either way.
- [x] AP2: `make-throughput.py` runs `node bench/throughput.js` and folds its JSON into the evidence, but the source scope covered only Rust, Python, TeX and selected root configuration, so the WebAssembly throughput producer could change without moving `source_snapshot_sha256`. That is exactly the not-yet-committed working-tree case the snapshot claims to bind. The script and the package manifest that pins what it runs against are now in scope, taking the manifest from 144 sources to 146. Arena `a42c47c`.
- [x] AP4: the salt commitment was computed before the attacks but only reached disk together with the reveal, so ordering inside one process was not an externally checkable commitment. It now writes its own artifact carrying no reveal field before either attack runs, the reveal is checked against that file rather than the in-memory digest, and the record states what the separation does and does not witness. Arena `8ce15a1`. It becomes an audit guarantee only when published before the reveal, which the record says in its own limits field.
- [x] AP6: the all-figure renderer claimed to rebuild every committed figure and reached 10 of 17. Dispatch is now an explicit registry naming, per entry, the evidence file it reads and every PDF it writes, and it prints an omission notice for any figure no entry claims. Arena `8ce15a1`. It also drops a duplicated F5 layout that drew two committed figures at a different size than their own producer, so the documented rebuild would have silently replaced them.

### Batch I: bounded probes (promote or delete)

Each probe is one test attempt. A reproduced failure becomes a defect row in
the matching batch; a non-reproduction deletes the row with a one-line note.

- [x] Confirmed and fixed. `HttpAgent` connected with no connect timeout and used fixed per-operation read timeouts, so an endpoint answering one byte just inside every read timeout extended the exchange to the 8 MiB cap, multiplied by the retry budget, and nothing was classified as a timeout because no single read timed out. One absolute deadline now bounds connect, write and every read, as BS2 did for stdio. Bench `4809747`. The pre-fix run took 6.03 seconds against a 200 ms budget.
- [x] Confirmed and fixed. `billable_units` summed two `u64` token counts before the float conversion and `validate_for` bounds only `cost_usd`, so a decision declaring the maximum `u64` is a valid wire message from an untrusted entrant: it panics with overflow checks on and wraps to a near-zero cost with them off, the favourable direction for the per-cost statistics. The sum now saturates. Bench `2fa068c`.
- [x] Docker ENTRYPOINT probe: resolved as not observable with the pinned fixture, which is why the first repair failed. The appended-command defect is real in the launch construction: a trailing `/bin/sh` replaces `CMD`, not `ENTRYPOINT`. But the fixture is `alpine`, which declares a `CMD` and no entrypoint, so the appended command lands exactly where an override would have put it and neither the defect nor a fix is visible. That explains the earlier failure: the override changed which process is PID 1 on the one launch whose contract depends on that, while buying nothing observable. `live_fixture_declares_no_entrypoint_to_override` records the fixture's declaration on the Docker runner and fails if a future fixture declares one, which is the signal that the override becomes both testable and required. Bench `bb0d166`.
- [x] Child OOM versus surviving wrapper: **probed on a real daemon and not reproducible**. `live_surviving_wrapper_child_oom_is_recorded` builds the exact hazard, a shell that outlives its OOM-killed child and exits 0 itself, and the CI run reports `docker 28.0.4, cgroup systemd v2, wrapper exit success=true, State.OOMKilled=Ok(true)`. The daemon still attributes the kill, so the existing classification holds and the retryable-transport misclassification the finding feared does not occur on this configuration. Bench `337046a`. The answer is daemon and cgroup dependent, so the probe stays in the suite and records the version and driver on every run rather than asserting the result once.

### Deferred (not scheduled)

- R08: unit/rule/target strata or precommitted dimensionless aggregation. A design decision, not a defect; reopen only with a concrete misranking it would have prevented.
- Versioned opt-in lifecycle-certified rank mode; legacy protocol unchanged.
- Production-linked reset/step/terminal/fill/accounting properties and scoped Lean models.
- Snapshot sharing decision; needs a measured use, not an invented benchmark.
- Full mutation and paired-boundary gates as a standing CI leg (individual repairs already carry mutation evidence in the log).

## Closed rows

Commit references are in the named repository. "PR" means a normal merge into
main after all Actions jobs passed on the exact head; the log has run IDs.

### Bench

- [x] R01: original process evidence survives common-support restriction. `dff0d84`, PR #11.
- [x] BI2, BI8: declared mandates preserved through CLI/Python/WASM board parsing; duplicate/blank agent IDs rejected; disqualification classified from the ranked field. `7f80fee`, npm/WASM `b22ecdd`, PR #24. Declarations add a second verdict without changing host rank.
- [x] R19: masking preserves dividends and total-return economics. `1bd06e2`.
- [x] R20: finite/domain CSV validation, duplicates, dividend missingness. `1bd06e2`, `d882803`; signed raw closes remain permitted per the archived WTI contract.
- [x] BM6: validated DAG, qualified transitive prerequisites, paired-randomization inference. `7719f53`, `e68d1d8`, migration `1f64ad5`, PR #18. Replicate independence and arm exchangeability remain assumptions.
- [x] BM4, BM5: checked options pricing and separate payoff-tail classification. `36617b2`, PR #17. Classifies the supplied same-expiry payoff only.
- [x] BI7, BM8: Momentum lookback `259555e`, PR #15; pending-trade precedence `4077551`, PR #16.
- [x] BM9: repeated normalized areas aggregate all rows; unspecified ordering, empty identities and invalid salience limits fail explicitly. `011154a`, `05522cb`, PR #20. Structural auditing, not semantic truth.
- [x] BS1: bounded stdout queue and accepted-output budget. `abdabef`, PR #12.
- [x] BS2: absolute request deadline includes blocking stdin write/flush. `b60890f`, PR #12.
- [x] BS3: authenticated V2 sealing (AES-256-GCM-SIV, fresh nonces) with versioned migration. `d3bda00`, PR #14. No independent security audit is claimed.
- [x] BS4, BI10: malformed hex and Unicode IDs cannot panic. `6d1becc`.
- [x] BI5, BI9: checked integer conversion; strict boolean/binary inputs. `72afdfa`, `a1bee0b`, `b9c1af4`, PR #13.
- [x] BS5: relative checkpoint parent, owned temporary siblings, bounded collision retries. `e5ae7a8`.
- [x] R15 (evidence producer): declared unique Cartesian sweep support, strict JSON, dataset/configuration agreement. `c9dc85f`, `4ff3637`, `e60e1f7`, PR #22. Structure only; no rescoring.
- [x] R03 (arithmetic): population standardized moments, NIST 1.3.5.11. `0cd7d37`. Propagation is Batch E.
- [x] R09 (Bench side): inventory/cash fix. `4378ad4`. Propagation is Batch E.
- [x] R02 (bootstrap/BH/FDR): typed errors, fail-closed eligibility. `895a623`. Remainder is Batch E.
- [x] Prospective-field import refuses an empty contract inventory. `c5fe201`, provenance `45087ce`.

### Arena

- [x] R04: distribution-stable SPEC_HASH, epoch 2, real normalized Cargo package test, Python/npm pins, rebuilt wasm. Broader package surface is Batch B.
- [x] AI4, AI7: complete native/book/cursor/terminal/reward reset. `fae8e1b`, `53e963a`.
- [x] AI6, AI8: every scenario knob survives reconstruction; PettingZoo honors difficulty; native restore retains the action prefix. `4141786`, `e7ecdb8`.
- [x] R18: unconditional per-environment readback. `6e57c3c`.
- [x] AI2: executable prompt examples generated from the action contract. `8f9a2a8`, PR #18.
- [x] AI10: exact Gym vector shape, finiteness, policy/bounds validation. `e51937c`.
- [x] AI9: full-horizon/process eligibility gates all eight training schemes; failed or incomplete episodes score a composite -1. `da019a2`, `d7189d5`, PR #21.
- [x] AI3: trusted full snapshot separated from default step/action-only export; transactional exact-action restore. `2fcf7ed`, `dfd8891`, PR #15.
- [x] AI5: receipt-backed V2 execution evidence; cumulative fill snapshots; no retry double-counting. `2e6b41b`, PR #19. Local marks are not realized P&L.
- [x] R16: V2 binds complete reconstruction inputs and reruns the fixed producer; output-only V1 refused. `e648b41`, `d9146c6`, `486cca5`, PR #22.
- [x] R10: constrained ellipse fix. `05078a7`, PR #17. Finite nonnegative centre/cost is a documented precondition.
- [x] AD2, AD5, AD7, AD6: initial NAV in full-episode metrics; target downside RMS/Sortino; monotonic resource penalties; no caller-array aliasing. `919eb32`.
- [x] AD1: risk-radar drawdown direction and real anchors. `ed40d76`.
- [x] AD3: failed/missing/nonfinite episodes cannot default to clean. `fc01d5f`.
- [x] R17: actual winner/confidence rendering, no false equivalence language. `efb97be`, PR #16.
- [x] AD4: indicator proxy consumes the same three-bar realized-return signal as its reward. `4ac09d3`, PR #21.
- [x] AD8: exact representable PRNG output grid and inversion round trips. `3d0aa82`.
- [x] R21: Node invoked directly without a shell. `1c6dd70`, PR #22.
- [x] R03 (arithmetic): `cdd877e`. Confidence intervals withheld while the pinned registry Bench retains different moments (`96e8782`).
- [x] Logit-runtime validator test drives asymmetric cases, not only the fixed point. `3cc00e5`.
