# Sharpe suite audit implementation

Goal started 2026-09-07. Plan restructured 2026-09-08 after an independent
review of progress against both repositories. The overall goal remains
unfinished. This file is mirrored byte-for-byte in the Bench and Arena
repositories; edit both or neither.

Status: 100 checklist rows, 69 closed and 31 open. The restructured plan
opened at 39 closed. Batch A and Batch B are now complete except for the final
paper rebuild and provenance rebind, and Batch D's two Arena producer rows and
Batch E's two propagation rows also close. Their work is on the
`fix/audit-batch-a-b-2026-09-08` branch in each repository and is not merged.
Every commit cited on a closed row was verified to be reachable in the named
repository. Open rows are ordered into batches below; the count is a checklist
disposition, not a count of confirmed defects. Five deferred items are listed
without checkboxes.

Two defects were found while repairing, neither in the original audit. The
release workflow's `verify` job runs with `if: always()` and did not inspect
the new publish gate, so a blocked release would have reported success. The
paper's description of the endogenous market as single-price was wrong in its
own right: each agent pays a size-dependent execution price, so that model is
not uniform-price either. Both are fixed in the commits cited on their rows.

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
- [ ] Rebuilt papers with checked references and layout, fresh provenance, granular verified commits pushed; no release tags.

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

- [ ] R07, BM10: versioned canonical numeric JSON and unambiguous digest framing. `float_roundtrip` is now explicit in both products; the remaining work is the versioned spec and digest framing.
- [ ] R06, AI1: exact frozen contracts and canonical settlements across producers and consumers.
- [x] R03 propagation recorded rather than performed. Verified by ancestry: `0cd7d37` is an ancestor of neither `v0.15.0` nor `v0.18.4`, so the corrected moments are unreleased, and Arena's pin of `=0.15.0` cannot carry them. Both changelogs state this and that no artifact was rescored. Bench `a81c5ed`, Arena `6823c60`.
- [x] R09 propagation recorded on the same evidence: `4378ad4` is an ancestor of neither `v0.15.0` nor `v0.18.4`. Same changelog entries as R03.
- [ ] R02 partially closed, and the open part is worse than the row implied. Closed here: `spearman_rho` returned 0.9999999999999998 for a NaN severity series and `gate_vs_human` reported a kappa beside it, `kendall_tau_b` returned 0.0 for infinite scores and `dissent` published a rank dissent from them, and `sortino_ratio` returned a value for negative infinity and never checked its target. Bench `70229a1`, with paired tests pinning that valid inputs are unchanged. Still open and reproduced: a field of 20 NaN returns **p = 0.001996** from `reality_check_pvalue`, `spa_pvalue` and `spa_consistent_pvalue`, because the observed statistic is NaN so the early exit is skipped and no draw exceeds it, leaving the smallest attainable p. `step_down_significant` rejects every hypothesis for a NaN or out-of-range alpha, the `benjamini_hochberg` defect one module over. `expected_max_sharpe` folds a negative dispersion into the no-trials branch, so a deflated Sharpe of 1.0 comes back for `trials_sr_std = -1.0`. `bootstrap_dsr_ci` turns a NaN confidence level into a zero-width interval, reading invalid input as perfect precision. `percentile_selection` and `selection_robustness` propagate NaN. Closing these widens eight return types across `sharpebench-core`, `sharpebench-edge`, `sharpebench-cli` and `sharpebench-py`; the callers are enumerated in the verification log.
- [ ] R05: independent-block requirements; insufficient-support status for forecast inference.
- [x] R12: an optional default lets a reader accept an older message, not a newer one, because the published schemas set `additionalProperties: false`. Compatibility is now stated by direction: decisions travel to the harness, always the newer reader, so an added decision field needs no coordination, while an added observation field reaches an older reader and needs version negotiation, a parallel namespace or a declared-unvalidated envelope. The minor-bump rule is split the same way. Arena `118ddc4`.
- [ ] BR2, AR2: append-only attempt ledger preserving failed/retried cost and timing.

### Batch F: remaining Bench diagnostics

- [ ] BM1: observed search-footprint floors and valid/unavailable PBO status.
- [ ] BM2, BS6: displayed board content/count/order and trusted terminal receipt anchor.
- [ ] BM3: dated role/durability support; correct IC versus return-trend descriptions.
- [ ] BM7: raw-candidate lineage/rediscovery validation and identity binding.
- [ ] BI6: team-member resource accounting and concurrency semantics.
- [x] Memory: the oracle series was never length-checked against the paired arms, so `fraction_of_ceiling` could divide a lift from one task mix by a ceiling gap from another. The documented zero floor was implemented as a near-zero guard on the absolute gap, so an oracle below baseline kept the ratio's sign and a retrieval arm that also lost ground reported a favourable positive fraction. Scores, costs and alpha are now finite-checked at the boundary on the poisoning leg too. Bench `50e147b`. Equal length cannot establish identical task identities; that stays an explicit caller contract.
- [ ] Budget support and search population semantics.
- [ ] Plateau terminology.
- [ ] Zero-return versus no-trade distinction.
- [ ] Regime reversals.
- [ ] Aligned noncausal attribution.
- [ ] Unknown process checks.
- [ ] Turnover semantics.
- [ ] Configured disqualification rollups.
- [ ] Dated rediscovery.
- [ ] Explicit transitive clone-cluster semantics.

### Batch G: remaining Arena telemetry

- [x] AR1, AR3: missing and null provider fields were coerced to zero before validation, so an absent count and a real zero were indistinguishable and a negative fraction truncated to zero; a null reasoning count read as provider-reported because availability tested key presence. Strict readers now return not-reported, refuse a present non-integer, and derive availability. Validation reconciles what it claims to: steps must equal the realized return count, and reasoning observations must be one per request and sum to the reported total. Arena `8fa7a54`. No committed artifact carries these fields, so no frozen number changes.
- [x] AR4: durations are recorded per measurement in order with value, unit and source, the `unspecified` default is gone and the source is validated against a closed set. Percentiles are reported per observing clock, so a p95 in a mixed cell is attributable to backend compute time or host elapsed time instead of pooled across both. Arena `8fa7a54`.

### Batch H: producer rows for the next field run

Do these before any field is scheduled; they are not required for the current
papers.

- [ ] BP1: requested/effective model identity, no silent substitution.
- [ ] BP2: collision-free calibration seed tuples with explicit CRN policy.
- [ ] BP3: full effective request/scaffold/parser cache identity.
- [ ] BP4: explicit hermetic passthrough/readback of supported nonsecret controls.
- [ ] BP5: collision-resistant model artifact identifiers.
- [ ] BP7: strict JSONL and complete figure/summary support.
- [ ] AP1: oracle/causal equal bars, warmup and costs.
- [x] AP2: `make-throughput.py` runs `node bench/throughput.js` and folds its JSON into the evidence, but the source scope covered only Rust, Python, TeX and selected root configuration, so the WebAssembly throughput producer could change without moving `source_snapshot_sha256`. That is exactly the not-yet-committed working-tree case the snapshot claims to bind. The script and the package manifest that pins what it runs against are now in scope, taking the manifest from 144 sources to 146. Arena `a42c47c`.
- [ ] AP4: persist pre-execution commitment separately from reveal; witness limits explicit.
- [ ] AP6: complete frozen-input figure renderer registry.

### Batch I: bounded probes (promote or delete)

Each probe is one test attempt. A reproduced failure becomes a defect row in
the matching batch; a non-reproduction deletes the row with a one-line note.

- [x] Confirmed and fixed. `HttpAgent` connected with no connect timeout and used fixed per-operation read timeouts, so an endpoint answering one byte just inside every read timeout extended the exchange to the 8 MiB cap, multiplied by the retry budget, and nothing was classified as a timeout because no single read timed out. One absolute deadline now bounds connect, write and every read, as BS2 did for stdio. Bench `4809747`. The pre-fix run took 6.03 seconds against a 200 ms budget.
- [x] Confirmed and fixed. `billable_units` summed two `u64` token counts before the float conversion and `validate_for` bounds only `cost_usd`, so a decision declaring the maximum `u64` is a valid wire message from an untrusted entrant: it panics with overflow checks on and wraps to a near-zero cost with them off, the favourable direction for the per-cost statistics. The sum now saturates. Bench `2fa068c`.
- [x] Confirmed from source and fixed. An appended `/bin/sh` command replaces `CMD`, not `ENTRYPOINT`, so against an image declaring one the hostile readiness probe, every egress probe and the live OOM fixture were reporting on what that entrypoint did with the script as its argv. The executable now goes to `--entrypoint`. Bench `f422a45`. The production entrant launch is unchanged and a test pins that. No daemon was started and no image pulled.
- [ ] Blocked on a Docker-enabled runner, not unexamined. Source shows the hazard is already reasoned about: `new_with_memory` omits `--init` because docker-init can survive long enough for Docker to record `OOMKilled=false`, and a false negative falls through to a retryable transport classification, so the harness respawns an agent certain to breach the same budget. What source cannot settle is an entrant whose own entrypoint forks the real agent, reinstating the surviving-PID-1 shape. The CI step is an ignored live test under `--memory 32m` with a shell that survives its OOM-killed child, asserting what `DockerCli.oom_killed` returns and recording the runner's `docker version` and cgroup driver.

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
