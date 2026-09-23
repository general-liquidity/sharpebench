# Evidence-freshness and applicability register

Ticket P00-E. This register maps every table, every figure and every empirical
claim in the inventoried manuscript scope of the two products to the artifact
that produced it, the command and version behind that artifact, the changes that
landed afterwards, and what the claim supports today.

The register maps existing evidence. It does not rerun, regenerate or authorize
any experiment, and no frozen evidence file, golden or manuscript number was
changed to produce it. Where an artifact could not be located the row records
`unresolved` rather than a guess, and unknown historical versions stay unknown.

`docs/evidence-register.jsonl` is the machine-readable authority; this file is
generated from it by `scripts/make-evidence-register.py`.
`scripts/check-evidence-register.py` fails on a missing artifact, a claim with no
disposition, a manuscript table or figure with no row, and a status change with
no recorded rationale.

Rows marked with an external repository cover the companion product, whose tree
is not part of this repository. Their artifact paths are recorded relative to
that repository's root and are not opened by the checker, which reports coverage
only for what it can read.

## Changing a row

Edit the JSONL row, then run `python scripts/make-evidence-register.py` to
rewrite this file and `python scripts/check-evidence-register.py` to validate
both. A disposition that changes gets a new `status_history` entry with its own
rationale; repeating the previous entry's text fails the check. An artifact's
`sha256` is over the file's bytes, and the checker prints the digest it computed
when a recorded one does not match.

## Dispositions

| Disposition | Meaning and action |
|---|---|
| historical-only | Validly records an older experiment. The claim is retained and bound to that version. It does not validate changed current behavior. |
| still-applicable | A documented comparison establishes that the computation, the inputs and the claim remain applicable. The check is recorded, not inferred. |
| needs-rescore | Recorded inputs may suffice to compute a current-method result. Sufficiency is verified first; any output is separately versioned and never overwrites the old one. |
| needs-new-experiment | Changed generation, policy, execution or measurement, or missing inputs, prevent a defensible rescore. A new protocol and budget request is required. |
| unresolved | Evidence is insufficient to choose a disposition. What is missing is recorded and no current validation is implied. |

## Counts

| Disposition | Rows |
|---|---|
| historical-only | 24 |
| still-applicable | 18 |
| needs-rescore | 4 |
| needs-new-experiment | 8 |
| unresolved | 5 |
| **total** | **59** |

## Priority queue

The headline claims whose present applicability is not established, worst first. The queue is bounded at 12 rows so that it stays a working list rather than a second copy of the register; every other claim is tracked by its row alone, and no historical audit is reopened for it.

| Claim | Product | Disposition | What it would take |
|---|---|---|---|
| `SB-sec-passk` | SharpeBench | needs-new-experiment | P12-A then P13-A/P14-A: a shipped-method joint-gate producer run on the current engine, reported as a new experiment and never as a refresh of the frozen numbers. |
| `SB-tab-costsens` | SharpeBench | needs-new-experiment | P12-A: re-run the externally specified sweep on the current engine and report it as a separately versioned experiment beside, not in place of, the frozen table. |
| `SB-sec-external` | SharpeBench | needs-new-experiment | P12-A then P13-A/P14-A: re-run the externally specified field on the current engine as a new experiment. |
| `SB-claims-i` | SharpeBench | needs-new-experiment | P12-A then P13-A/P14-A. |
| `SB-claims-iii` | SharpeBench | needs-new-experiment | P12-A then P13-A/P14-A. |
| `SA-predictability` | SharpeArena | needs-new-experiment | P13-R/P14-R: re-run the predictability producer under the corrected pin and report it as a new experiment beside the frozen artifact. |
| `SA-sealed-seeds` | SharpeArena | needs-new-experiment | P13-R/P14-R: re-run the sealed-seed reveal under the full-tape check and under the AP4 commitment ordering, reported as a new measurement. |
| `SA-tab-witness` | SharpeArena | needs-new-experiment | P13-R/P14-R: re-run the rank-eligibility witness under the corrected pin as a declared new experiment. |
| `SB-tab-data` | SharpeBench | needs-rescore | P15: commit a realism report artifact beside the datasets so the table's verdicts have a stored producer output. |
| `SB-sec-power` | SharpeBench | needs-rescore | P12-A supplies repaired bars; then recompute this table as a separately versioned output beside the frozen one. |
| `SA-tab-f1` | SharpeArena | needs-rescore | P12-A/P13-R: rescore the committed per-seed returns under the corrected pin and publish the result beside, not in place of, the frozen table. |
| `SA-tab-f3` | SharpeArena | needs-rescore | P12-A/P13-R: rescore the committed F3 return series under the corrected pin as a separately versioned output. |

## Summary

| Claim | Product | Kind | Source | Disposition | Owner | Follow-up |
|---|---|---|---|---|---|---|
| `SB-tab-design-constraints` | SharpeBench | table | paper/sections/02-principles.tex:22 | still-applicable | SharpeBench paper/evidence owner | none |
| `SB-tab-leakage` | SharpeBench | table | paper/sections/04-integrity.tex:10 | still-applicable | SharpeBench paper/evidence owner | none |
| `SB-tab-attacks` | SharpeBench | table | paper/sections/04-integrity.tex:43 | still-applicable | SharpeBench paper/evidence owner | none |
| `SB-tab-data` | SharpeBench | table | paper/sections/03-benchmark.tex:84 | needs-rescore | SharpeBench paper/evidence owner | P15: commit a realism report artifact beside the datasets so the table's verdicts have a stored producer output. |
| `SB-tab-datasheet` | SharpeBench | table | paper/sections/D-datasheet.tex:18 | still-applicable | SharpeBench paper/evidence owner | none |
| `SB-tab-related` | SharpeBench | table | paper/sections/06-related.tex:19, paper/sections/06-related.tex:44 | still-applicable | SharpeBench paper/evidence owner | P15: recheck the dated cells before submission, since the marks track external publications. |
| `SB-fig-demotion` | SharpeBench | figure | paper/sections/05-experiments.tex:20 | still-applicable | SharpeBench paper/evidence owner | none |
| `SB-fig-deflation` | SharpeBench | figure | paper/sections/05-experiments.tex:28 | still-applicable | SharpeBench paper/evidence owner | none |
| `SB-tab-units` | SharpeBench | table | paper/sections/05-experiments.tex:45 | historical-only | SharpeBench paper/evidence owner | P15: bring paper/evidence/baseline-v0.2.1/ and the FINDING records into a recorded digest scope so the historical artifacts are identified rather than merely present. |
| `SB-sec-units-prefloor` | SharpeBench | claim | paper/sections/05-experiments.tex:33 | unresolved | SharpeBench paper/evidence owner | P15: identify which artifact the 0.984 and 0.000 pair is read from and reconcile the appendix with the frozen note, or drop the pair. |
| `SB-sec-units-worked-example` | SharpeBench | claim | paper/sections/05-experiments.tex:36 | still-applicable | SharpeBench paper/evidence owner | none |
| `SB-tab-eligibility` | SharpeBench | table | paper/sections/05-experiments.tex:77 | historical-only | SharpeBench paper/evidence owner | none |
| `SB-sec-passk` | SharpeBench | claim | paper/sections/05-experiments.tex:61, paper/sections/05-experiments.tex:66 | needs-new-experiment | SharpeBench paper/evidence owner | P12-A then P13-A/P14-A: a shipped-method joint-gate producer run on the current engine, reported as a new experiment and never as a refresh of the frozen numbers. |
| `SB-sec-ablation` | SharpeBench | claim | paper/sections/05-experiments.tex:99, paper/sections/05-experiments.tex:100 | historical-only | SharpeBench paper/evidence owner | P12-A: include the never-catastrophic preset in the joint-gate producer run so the ablation is re-established on the current engine. |
| `SB-sec-riskmanaged` | SharpeBench | claim | paper/sections/05-experiments.tex:107, paper/sections/05-experiments.tex:110 | historical-only | SharpeBench paper/evidence owner | none |
| `SB-fig-drawdowns` | SharpeBench | figure | paper/sections/05-experiments.tex:124 | historical-only | SharpeBench paper/evidence owner | none |
| `SB-fig-luckdeflation` | SharpeBench | figure | paper/sections/05-experiments.tex:132 | historical-only | SharpeBench paper/evidence owner | none |
| `SB-tab-external` | SharpeBench | table | paper/sections/05-experiments.tex:159 | historical-only | SharpeBench paper/evidence owner | none |
| `SB-tab-costsens` | SharpeBench | table | paper/sections/05-experiments.tex:209 | needs-new-experiment | SharpeBench paper/evidence owner | P12-A: re-run the externally specified sweep on the current engine and report it as a separately versioned experiment beside, not in place of, the frozen table. |
| `SB-sec-external` | SharpeBench | claim | paper/sections/05-experiments.tex:137, paper/sections/05-experiments.tex:145 | needs-new-experiment | SharpeBench paper/evidence owner | P12-A then P13-A/P14-A: re-run the externally specified field on the current engine as a new experiment. |
| `SB-tab-relative` | SharpeBench | table | paper/sections/relative-mandate-fragment.tex:24 | historical-only | SharpeBench paper/evidence owner | none |
| `SB-tab-mandate` | SharpeBench | table | paper/sections/mandate-declaration-fragment.tex:25 | historical-only | SharpeBench paper/evidence owner | none |
| `SB-tab-seedleg` | SharpeBench | table | paper/sections/execution-noise-fragment.tex:29 | historical-only | SharpeBench paper/evidence owner | none |
| `SB-sec-perturb` | SharpeBench | claim | paper/sections/05-experiments.tex:237 | historical-only | SharpeBench paper/evidence owner | none |
| `SB-sec-witness` | SharpeBench | claim | paper/sections/05-experiments.tex:241, paper/sections/05-experiments.tex:251 | still-applicable | SharpeBench paper/evidence owner | P13-A: replication over independent noise draws, which the introduction records as planned rather than done (paper/sections/01-introduction.tex:22). |
| `SB-sec-falsify` | SharpeBench | claim | paper/sections/05-experiments.tex:254, paper/sections/05-experiments.tex:257 | historical-only | SharpeBench paper/evidence owner | none |
| `SB-sec-luck1000` | SharpeBench | claim | paper/sections/hardening-fragment.tex:5, paper/sections/hardening-fragment.tex:13 | historical-only | SharpeBench paper/evidence owner | none |
| `SB-sec-sybil` | SharpeBench | claim | paper/sections/sybil-defense-fragment.tex:4 | still-applicable | SharpeBench paper/evidence owner | P15: write the per-field similarity maxima to a committed artifact so the two quoted numbers carry a digest rather than living only in test output. |
| `SB-sec-power` | SharpeBench | claim | paper/sections/power-fragment.tex:7, paper/sections/power-fragment.tex:32, paper/sections/power-fragment.tex:39 | needs-rescore | SharpeBench paper/evidence owner | P12-A supplies repaired bars; then recompute this table as a separately versioned output beside the frozen one. |
| `SB-sec-forecast-report` | SharpeBench | claim | paper/sections/03-benchmark.tex:53 | still-applicable | SharpeBench paper/evidence owner | none |
| `SB-sec-forecast-size` | SharpeBench | claim | paper/sections/03-benchmark.tex:59, paper/sections/01-introduction.tex:24, paper/sections/07-limitations.tex:37 | unresolved | SharpeBench paper/evidence owner | P15: commit the size study's producer and its output, or state in the manuscript that the size table is an uncommitted measurement. |
| `SB-sec-compute` | SharpeBench | claim | paper/sections/05-experiments.tex:5 | unresolved | SharpeBench paper/evidence owner | P15: commit a timing receipt, or state the measurements as uncommitted machine observations. |
| `SB-claims-i` | SharpeBench | claim | paper/sections/05-experiments.tex:266 | needs-new-experiment | SharpeBench paper/evidence owner | P12-A then P13-A/P14-A. |
| `SB-claims-ii` | SharpeBench | claim | paper/sections/05-experiments.tex:266 | historical-only | SharpeBench paper/evidence owner | none |
| `SB-claims-iii` | SharpeBench | claim | paper/sections/05-experiments.tex:266 | needs-new-experiment | SharpeBench paper/evidence owner | P12-A then P13-A/P14-A. |
| `SB-claims-iv` | SharpeBench | claim | paper/sections/05-experiments.tex:266 | still-applicable | SharpeBench paper/evidence owner | none |
| `SB-claims-v` | SharpeBench | claim | paper/sections/05-experiments.tex:266 | still-applicable | SharpeBench paper/evidence owner | none |
| `SB-claims-vi` | SharpeBench | claim | paper/sections/05-experiments.tex:266 | still-applicable | SharpeBench paper/evidence owner | P13-A: replication over independent noise draws. |
| `SA-tab-f4` | SharpeArena | table | paper/sections/07-validation.tex:18, paper/sections/07-validation.tex:42, paper/sections/07-validation.tex:11 | historical-only | SharpeArena paper/evidence owner | P13-R/P14-R: the F4 positive control, which the manuscript names as available and unrun. |
| `SA-calm-calibration` | SharpeArena | claim | paper/sections/07-validation.tex:45, paper/sections/07-validation.tex:55 | historical-only | SharpeArena paper/evidence owner | none |
| `SA-tab-f5` | SharpeArena | table | paper/sections/07-validation.tex:69, paper/sections/07-validation.tex:104, paper/sections/07-validation.tex:110, paper/sections/07-validation.tex:58 | historical-only | SharpeArena paper/evidence owner | P13-R: re-verify the frozen F5 grid against the traded-bar validation rule, or record that the check was run. |
| `SA-f5-concave` | SharpeArena | claim | paper/sections/07-validation.tex:115, paper/sections/07-validation.tex:130 | historical-only | SharpeArena paper/evidence owner | none |
| `SA-f5-positive-control` | SharpeArena | claim | paper/sections/07-validation.tex:135, paper/sections/07-validation.tex:151 | still-applicable | SharpeArena paper/evidence owner | none |
| `SA-f5-extended-sweeps` | SharpeArena | claim | paper/sections/07-validation.tex:156, paper/sections/07-validation.tex:169 | historical-only | SharpeArena paper/evidence owner | none |
| `SA-tab-f6` | SharpeArena | table | paper/sections/07-validation.tex:179, paper/sections/07-validation.tex:204, paper/sections/07-validation.tex:172 | historical-only | SharpeArena paper/evidence owner | none |
| `SA-f6-endogenous` | SharpeArena | claim | paper/sections/07-validation.tex:207, paper/sections/07-validation.tex:222, paper/sections/07-validation.tex:245, paper/sections/07-validation.tex:270 | historical-only | SharpeArena paper/evidence owner | P15: name the test behind the 1e-12 native cross-check, or record that none exists. |
| `SA-predictability` | SharpeArena | claim | paper/sections/07-validation.tex:273, paper/sections/07-validation.tex:285 | needs-new-experiment | SharpeArena paper/evidence owner | P13-R/P14-R: re-run the predictability producer under the corrected pin and report it as a new experiment beside the frozen artifact. |
| `SA-seed-search` | SharpeArena | claim | paper/sections/07-validation.tex:273 | historical-only | SharpeArena paper/evidence owner | none |
| `SA-sealed-seeds` | SharpeArena | claim | paper/sections/07-validation.tex:290 | needs-new-experiment | SharpeArena paper/evidence owner | P13-R/P14-R: re-run the sealed-seed reveal under the full-tape check and under the AP4 commitment ordering, reported as a new measurement. |
| `SA-tab-f1` | SharpeArena | table | paper/sections/08-findings.tex:22, paper/sections/08-findings.tex:63, paper/sections/08-findings.tex:7 | needs-rescore | SharpeArena paper/evidence owner | P12-A/P13-R: rescore the committed per-seed returns under the corrected pin and publish the result beside, not in place of, the frozen table. |
| `SA-tab-witness` | SharpeArena | table | paper/sections/08-findings.tex:78, paper/sections/08-findings.tex:99, paper/sections/08-findings.tex:66 | needs-new-experiment | SharpeArena paper/evidence owner | P13-R/P14-R: re-run the rank-eligibility witness under the corrected pin as a declared new experiment. |
| `SA-f2-regret` | SharpeArena | claim | paper/sections/08-findings.tex:102, paper/sections/08-findings.tex:114 | still-applicable | SharpeArena paper/evidence owner | none |
| `SA-tab-f3` | SharpeArena | table | paper/sections/08-findings.tex:124, paper/sections/08-findings.tex:117 | needs-rescore | SharpeArena paper/evidence owner | P12-A/P13-R: rescore the committed F3 return series under the corrected pin as a separately versioned output. |
| `SA-tab-f7` | SharpeArena | table | paper/sections/08-findings.tex:158, paper/sections/08-findings.tex:180, paper/sections/08-findings.tex:151 | still-applicable | SharpeArena paper/evidence owner | P15: serialize a runtime version in the artifact so the 0.29.0 attribution does not rest on prose. |
| `SA-tab-f8` | SharpeArena | table | paper/sections/08-findings.tex:192, paper/sections/08-findings.tex:224, paper/sections/08-findings.tex:183 | historical-only | SharpeArena paper/evidence owner | none |
| `SA-tab-throughput` | SharpeArena | table | paper/sections/06-protocol.tex:85, paper/sections/06-protocol.tex:78 | historical-only | SharpeArena paper/evidence owner | none |
| `SA-orderbook-mark` | SharpeArena | claim | paper/sections/03-environment.tex:112 | unresolved | SharpeArena paper/evidence owner | P15: commit a producer and artifact for the order-book mark comparison, or state it in the manuscript as an uncommitted measurement. |
| `SA-seeded-shuffle` | SharpeArena | claim | paper/sections/03-environment.tex:110 | unresolved | SharpeArena paper/evidence owner | P15: cite the seat-ordering measurement or commit it as evidence. |
| `SA-nonempirical-tables` | SharpeArena | table | paper/sections/03-environment.tex:20, paper/sections/03-environment.tex:55, paper/sections/06-protocol.tex:36, paper/sections/09-related.tex:12 | still-applicable | SharpeArena paper/evidence owner | P15: consider a per-cell provenance sheet for the related-work table, as the companion product keeps. |

## SharpeArena

### `SA-tab-f4`

Covers the **sharpearena** repository, which is not part of this tree. Paths below are relative to that repository's root.

**Claim.** Stylized-facts pass rates over eight seeds per tier: the canonical generator passes 1 of 24 seeded panels, with gated pass rates 0.000 Calm, 0.000 Hard and 0.125 Extreme.

**Source.** `paper/sections/07-validation.tex:18`, `paper/sections/07-validation.tex:42`, `paper/sections/07-validation.tex:11`

**Labels.** `tab:f4`, `fig:f4`

**Artifacts.**
- `paper/evidence/f4-realism.json`, sha256 `a929cf0e1cae81d399f7ee81f25dec218193834b8c0942cb3f8b565194f44cda`, digest source: paper/evidence/provenance.json in the sharpearena repository, schema 5, generated at commit ddc9239ecb8938a6e75ec0a0d13c4955ef698021; read for this register on 2026-09-23 at that repository's HEAD d33b24919f0ef63fa432b7aef3694ff03e2e4797, not in this repository, tiers.<tier>.mean_facts, per_seed and pass_rate; every table cell matches to displayed precision
- `paper/figures/f4-realism.pdf`, sha256 `33d3a9ef026a84fed48d9248397c1a41a236b0aece8450b7ee9b10aea3e10cd4`, digest source: paper/evidence/provenance.json in the sharpearena repository, schema 5, generated at commit ddc9239ecb8938a6e75ec0a0d13c4955ef698021; read for this register on 2026-09-23 at that repository's HEAD d33b24919f0ef63fa432b7aef3694ff03e2e4797, not in this repository

**Producer command.** python paper/src/make-f4-realism.py (paper/sections/A-commands.tex); re-render only with python paper/src/make-figures.py

**Producing commit.** Unknown. paper/sections/A-commands.tex:5 is the paper's single provenance statement: only f1-baselines.json serializes a runtime version (sharpearena 0.9.0), no evidence file serializes a SharpeBench version, and the paper declines to reconstruct one from creation dates. No artifact carries a producing commit; the manifest records only the commit it was generated at.

**Effective configuration.** config = {n_symbols: 4, n_days: 120, seeds: 0 to 7, max_steps: 512, tiers: calm, hard, extreme, vol_clustering: 0.5}. No effective_config readback block.

**Missing provenance.**
- Runtime version, SharpeBench version, producing commit and host platform are all unserialized.
- No effective-configuration readback block, so the requested configuration is recorded and the realized one is not.

**Relevant later changes.**
- Apart from F7 the committed F1 to F8 artifacts were not rerun after effective-configuration readback, SPEC_HASH, canonical pre-hash fixtures, typed FFI errors, strict input decoding, path-containment checks or the prospective forecast contract were added. The manuscript states those additions strengthen present executions without changing the observations of artifacts that were not rerun (paper/sections/10-limitations.tex:13).
- The Python reductions behind the realism, manipulation, adverse-selection, failure, ecology and predictability evidence are off the golden-hash path, so their floating-point reproducibility is the platform's rather than the paper's (paper/sections/10-limitations.tex:11).
- The computation-changing group at paper/sections/10-limitations.tex:15 concerns deflated Sharpes and episode metrics; F4 reads neither.

**Present applicability.** The reported statistics do not pass through the scorer, so the moment repair does not reach them. The manuscript records that the gate has no positive control and that the control is available and has not been run (paper/sections/07-validation.tex:5), which bounds what the 23 of 24 failure establishes.

**Disposition.** `historical-only`. The artifact validly records the run and no later change moves its numbers, but nothing re-establishes it against the current tree: the producer is off the golden-hash path and the artifact was not rerun.

**Owner.** SharpeArena paper/evidence owner

**Follow-up.** P13-R/P14-R: the F4 positive control, which the manuscript names as available and unrun.

**Status history.**

| Date | Disposition | Rationale |
|---|---|---|
| 2026-09-23 | historical-only | Initial entry: The artifact validly records the run and no later change moves its numbers, but nothing re-establishes it against the current tree: the producer is off the golden-hash path and the artifact was not rerun. |

### `SA-calm-calibration`

Covers the **sharpearena** repository, which is not part of this tree. Paths below are relative to that repository's root.

**Claim.** A 99-configuration Calm calibration sweep qualifies no cell, so there is no certified Calm preset.

**Source.** `paper/sections/07-validation.tex:45`, `paper/sections/07-validation.tex:55`

**Labels.** `fig:calm-calibration`

**Artifacts.**
- `paper/evidence/f4-realism.json`, sha256 `a929cf0e1cae81d399f7ee81f25dec218193834b8c0942cb3f8b565194f44cda`, digest source: paper/evidence/provenance.json in the sharpearena repository, schema 5, generated at commit ddc9239ecb8938a6e75ec0a0d13c4955ef698021; read for this register on 2026-09-23 at that repository's HEAD d33b24919f0ef63fa432b7aef3694ff03e2e4797, not in this repository, key calm_calibration, with 99 cells, n_qualifying 0 and chosen null
- `paper/figures/f4-calm-calibration.pdf`, sha256 `a196cd0593aff98f0ded31cf94ae7375e2e7e86cb7755646aa24acfe78827737`, digest source: paper/evidence/provenance.json in the sharpearena repository, schema 5, generated at commit ddc9239ecb8938a6e75ec0a0d13c4955ef698021; read for this register on 2026-09-23 at that repository's HEAD d33b24919f0ef63fa432b7aef3694ff03e2e4797, not in this repository

**Producer command.** python paper/src/make-f4-realism.py, whose same invocation runs the 99-configuration sweep and serializes it under calm_calibration (paper/sections/A-commands.tex)

**Producing commit.** Unknown. paper/sections/A-commands.tex:5 is the paper's single provenance statement: only f1-baselines.json serializes a runtime version (sharpearena 0.9.0), no evidence file serializes a SharpeBench version, and the paper declines to reconstruct one from creation dates. No artifact carries a producing commit; the manifest records only the commit it was generated at.

**Effective configuration.** As SA-tab-f4; the sweep's grid, seed bands and rule are serialized under calm_calibration.

**Missing provenance.**
- As SA-tab-f4.

**Relevant later changes.**
- Apart from F7 the committed F1 to F8 artifacts were not rerun after effective-configuration readback, SPEC_HASH, canonical pre-hash fixtures, typed FFI errors, strict input decoding, path-containment checks or the prospective forecast contract were added. The manuscript states those additions strengthen present executions without changing the observations of artifacts that were not rerun (paper/sections/10-limitations.tex:13).
- The Python reductions behind the realism, manipulation, adverse-selection, failure, ecology and predictability evidence are off the golden-hash path, so their floating-point reproducibility is the platform's rather than the paper's (paper/sections/10-limitations.tex:11).

**Present applicability.** A negative result recorded in the artifact as n_qualifying 0 and chosen null. No later change bears on it.

**Disposition.** `historical-only`. The artifact validly records the sweep and its negative outcome; nothing re-establishes it against the current tree.

**Owner.** SharpeArena paper/evidence owner

**Follow-up.** none

**Status history.**

| Date | Disposition | Rationale |
|---|---|---|
| 2026-09-23 | historical-only | Initial entry: The artifact validly records the sweep and its negative outcome; nothing re-establishes it against the current tree. |

### `SA-tab-f5`

Covers the **sharpearena** repository, which is not part of this tree. Paths below are relative to that repository's root.

**Claim.** Manipulation boundary sweeps on three axes find no profitability boundary, and impact-attributable PnL is negative and decreasing in push size.

**Source.** `paper/sections/07-validation.tex:69`, `paper/sections/07-validation.tex:104`, `paper/sections/07-validation.tex:110`, `paper/sections/07-validation.tex:58`

**Labels.** `tab:f5`, `fig:f5`, `fig:f5-size`

**Artifacts.**
- `paper/evidence/f5-manipulation.json`, sha256 `19b08f0cb184aeca94020336153c2bf9f2b8ec6d392ada88cde0da04e8ed29e9`, digest source: paper/evidence/provenance.json in the sharpearena repository, schema 5, generated at commit ddc9239ecb8938a6e75ec0a0d13c4955ef698021; read for this register on 2026-09-23 at that repository's HEAD d33b24919f0ef63fa432b7aef3694ff03e2e4797, not in this repository, keys boundaries, size_response, dispersion, ci_convention
- `paper/figures/f5-boundaries.pdf`, sha256 `78d3a1c3a3626febad9edf9aeec19cf9720873769059a8173378a50f00844c04`, digest source: paper/evidence/provenance.json in the sharpearena repository, schema 5, generated at commit ddc9239ecb8938a6e75ec0a0d13c4955ef698021; read for this register on 2026-09-23 at that repository's HEAD d33b24919f0ef63fa432b7aef3694ff03e2e4797, not in this repository
- `paper/figures/f5-size-response.pdf`, sha256 `686649a200c0c12613d8cbfd00db2d4175388a1a9eb8f6a7c29d0768280e65b8`, digest source: paper/evidence/provenance.json in the sharpearena repository, schema 5, generated at commit ddc9239ecb8938a6e75ec0a0d13c4955ef698021; read for this register on 2026-09-23 at that repository's HEAD d33b24919f0ef63fa432b7aef3694ff03e2e4797, not in this repository

**Producer command.** python paper/src/make-f5-manipulation.py (paper/sections/A-commands.tex)

**Producing commit.** Unknown. paper/sections/A-commands.tex:5 is the paper's single provenance statement: only f1-baselines.json serializes a runtime version (sharpearena 0.9.0), no evidence file serializes a SharpeBench version, and the paper declines to reconstruct one from creation dates. No artifact carries a producing commit; the manifest records only the commit it was generated at.

**Effective configuration.** config = {seeds: 0 to 7, base_params: {n_symbols: 1, n_days: 80, n_followers: 3, kyle_lambda: 0.1, eta: 0.05, push_weight: 0.8, follower_gain: 30.0, impact_exponent: 1.0}, concave_exponents: [0.5, 0.7]}; t-based 95 percent intervals over per-seed impact PnL at df 7.

**Missing provenance.**
- As SA-tab-f4, plus no record of which impact-engine revision produced the artifact.

**Relevant later changes.**
- Release 0.31.0 validates the manipulation round trip against traded bars rather than calendar days, so some previously accepted configurations now raise. The committed config uses n_days 80, which is a validity change rather than a value change, and no re-verification against the frozen grid was found.
- The manipulation probe gained per-follower seat-removal externality fields that the committed F5 artifact does not carry (paper/sections/10-limitations.tex:15, paper/sections/03-environment.tex:130).
- The Python reductions behind the realism, manipulation, adverse-selection, failure, ecology and predictability evidence are off the golden-hash path, so their floating-point reproducibility is the platform's rather than the paper's (paper/sections/10-limitations.tex:11).

**Present applicability.** The reported values do not pass through the scorer. The artifact predates the externality fields, so it is a strict subset of what the current probe records rather than a contradiction of it.

**Disposition.** `historical-only`. The artifact validly records the run; the later changes add fields and tighten configuration validation without being shown to move the frozen values, and no re-verification is recorded.

**Owner.** SharpeArena paper/evidence owner

**Follow-up.** P13-R: re-verify the frozen F5 grid against the traded-bar validation rule, or record that the check was run.

**Status history.**

| Date | Disposition | Rationale |
|---|---|---|
| 2026-09-23 | historical-only | Initial entry: The artifact validly records the run; the later changes add fields and tighten configuration validation without being shown to move the frozen values, and no re-verification is recorded. |

### `SA-f5-concave`

Covers the **sharpearena** repository, which is not part of this tree. Paths below are relative to that repository's root.

**Claim.** At the canonical calibration the concave-impact probe attributes negative P&L at both exponents, with seven of 23 stored sweep slots crossing zero at beta 0.5 and none at beta 0.7.

**Source.** `paper/sections/07-validation.tex:115`, `paper/sections/07-validation.tex:130`

**Labels.** `fig:f5-concave`

**Artifacts.**
- `paper/evidence/f5-manipulation.json`, sha256 `19b08f0cb184aeca94020336153c2bf9f2b8ec6d392ada88cde0da04e8ed29e9`, digest source: paper/evidence/provenance.json in the sharpearena repository, schema 5, generated at commit ddc9239ecb8938a6e75ec0a0d13c4955ef698021; read for this register on 2026-09-23 at that repository's HEAD d33b24919f0ef63fa432b7aef3694ff03e2e4797, not in this repository, key concave, one sub-key per exponent
- `paper/figures/f5-concave.pdf`, sha256 `d37f814664a5e5acddc09c01090100eb981b461bbeae1c25ae258d42832a4f6e`, digest source: paper/evidence/provenance.json in the sharpearena repository, schema 5, generated at commit ddc9239ecb8938a6e75ec0a0d13c4955ef698021; read for this register on 2026-09-23 at that repository's HEAD d33b24919f0ef63fa432b7aef3694ff03e2e4797, not in this repository

**Producer command.** python paper/src/make-f5-manipulation.py, which serializes the concave leg under concave (paper/sections/A-commands.tex)

**Producing commit.** Unknown. paper/sections/A-commands.tex:5 is the paper's single provenance statement: only f1-baselines.json serializes a runtime version (sharpearena 0.9.0), no evidence file serializes a SharpeBench version, and the paper declines to reconstruct one from creation dates. No artifact carries a producing commit; the manifest records only the commit it was generated at.

**Effective configuration.** As SA-tab-f5, exponents 0.5 and 0.7.

**Missing provenance.**
- As SA-tab-f5.

**Relevant later changes.**
- The Python reductions behind the realism, manipulation, adverse-selection, failure, ecology and predictability evidence are off the golden-hash path, so their floating-point reproducibility is the platform's rather than the paper's (paper/sections/10-limitations.tex:11).

**Present applicability.** Bounded by the manuscript's own stated limitation that the dimensionless flow sits two to three orders of magnitude below the unit crossover, so exponents below one amplify impact at the tested calibration (paper/sections/10-limitations.tex:19).

**Disposition.** `historical-only`. The artifact validly records the probe and the manuscript already scopes it as a stress setting rather than an estimate.

**Owner.** SharpeArena paper/evidence owner

**Follow-up.** none

**Status history.**

| Date | Disposition | Rationale |
|---|---|---|
| 2026-09-23 | historical-only | Initial entry: The artifact validly records the probe and the manuscript already scopes it as a stress setting rather than an estimate. |

### `SA-f5-positive-control`

Covers the **sharpearena** repository, which is not part of this tree. Paths below are relative to that repository's root.

**Claim.** An asymmetric-schedule positive control over 135 cells finds four familywise-positive cells, the best at +21.8e-4 with familywise interval [10.0, 33.6]e-4, and the selected cell replicates on 32 fresh seeds at +26.3e-4 [23.7, 28.9]e-4.

**Source.** `paper/sections/07-validation.tex:135`, `paper/sections/07-validation.tex:151`

**Labels.** `fig:f5-positive-control`

**Artifacts.**
- `paper/evidence/f5-manipulation.json`, sha256 `19b08f0cb184aeca94020336153c2bf9f2b8ec6d392ada88cde0da04e8ed29e9`, digest source: paper/evidence/provenance.json in the sharpearena repository, schema 5, generated at commit ddc9239ecb8938a6e75ec0a0d13c4955ef698021; read for this register on 2026-09-23 at that repository's HEAD d33b24919f0ef63fa432b7aef3694ff03e2e4797, not in this repository, key positive_control, including the confirmation leg
- `paper/figures/f5-positive-control.pdf`, sha256 `983d1183c1776dee7ebc4e5aec5399a29cc12fc8a309e77eec62825ed75ca10c`, digest source: paper/evidence/provenance.json in the sharpearena repository, schema 5, generated at commit ddc9239ecb8938a6e75ec0a0d13c4955ef698021; read for this register on 2026-09-23 at that repository's HEAD d33b24919f0ef63fa432b7aef3694ff03e2e4797, not in this repository

**Producer command.** python paper/src/make-f5-manipulation.py, positive-control leg (paper/sections/A-commands.tex)

**Producing commit.** Unknown. paper/sections/A-commands.tex:5 is the paper's single provenance statement: only f1-baselines.json serializes a runtime version (sharpearena 0.9.0), no evidence file serializes a SharpeBench version, and the paper declines to reconstruct one from creation dates. No artifact carries a producing commit; the manifest records only the commit it was generated at.

**Effective configuration.** 135 cells as five duration ratios by three block fractions by three exponents by three arms, eight seeds each; fresh-band confirmation on 32 seeds at df 31.

**Missing provenance.**
- As SA-tab-f5.

**Relevant later changes.**
- The Python reductions behind the realism, manipulation, adverse-selection, failure, ecology and predictability evidence are off the golden-hash path, so their floating-point reproducibility is the platform's rather than the paper's (paper/sections/10-limitations.tex:11).

**Present applicability.** Independently recomputed during the August 2026 review round, which reproduced the headline cell, its intervals, the four familywise-positive cells, all 45 negative linear cells and the 32-seed confirmation (paper/review/rereview-2026-08-25.md).

**Disposition.** `still-applicable`. A documented recomputation exists in the review record, which is a comparison rather than an inference; the quantities do not pass through the scorer that later changed.

**Owner.** SharpeArena paper/evidence owner

**Follow-up.** none

**Status history.**

| Date | Disposition | Rationale |
|---|---|---|
| 2026-09-23 | still-applicable | Initial entry: A documented recomputation exists in the review record, which is a comparison rather than an inference; the quantities do not pass through the scorer that later changed. |

### `SA-f5-extended-sweeps`

Covers the **sharpearena** repository, which is not part of this tree. Paths below are relative to that repository's root.

**Claim.** Sixty extension cells give 23 global-familywise positive intervals, 31 negative and six crossing zero, the largest cell at +135.0e-4 with interval [122.1, 148.0]e-4, read as descriptive rather than as simultaneous coverage.

**Source.** `paper/sections/07-validation.tex:156`, `paper/sections/07-validation.tex:169`

**Labels.** `fig:f5-extended-sweeps`

**Artifacts.**
- `paper/evidence/f5-manipulation.json`, sha256 `19b08f0cb184aeca94020336153c2bf9f2b8ec6d392ada88cde0da04e8ed29e9`, digest source: paper/evidence/provenance.json in the sharpearena repository, schema 5, generated at commit ddc9239ecb8938a6e75ec0a0d13c4955ef698021; read for this register on 2026-09-23 at that repository's HEAD d33b24919f0ef63fa432b7aef3694ff03e2e4797, not in this repository, key extended_sweeps
- `paper/figures/f5-extended-sweeps.pdf`, sha256 `91d3b7113c9d54220167c130fb8ed13f093e261225a7ccdea981bd7febbaf260`, digest source: paper/evidence/provenance.json in the sharpearena repository, schema 5, generated at commit ddc9239ecb8938a6e75ec0a0d13c4955ef698021; read for this register on 2026-09-23 at that repository's HEAD d33b24919f0ef63fa432b7aef3694ff03e2e4797, not in this repository

**Producer command.** python paper/src/make-f5-manipulation.py, extended-sweep leg (paper/sections/A-commands.tex)

**Producing commit.** Unknown. paper/sections/A-commands.tex:5 is the paper's single provenance statement: only f1-baselines.json serializes a runtime version (sharpearena 0.9.0), no evidence file serializes a SharpeBench version, and the paper declines to reconstruct one from creation dates. No artifact carries a producing commit; the manifest records only the commit it was generated at.

**Effective configuration.** Six axes by five values by eight common seeds, corrected over the global 195-cell family.

**Missing provenance.**
- As SA-tab-f5.

**Relevant later changes.**
- The Python reductions behind the realism, manipulation, adverse-selection, failure, ecology and predictability evidence are off the golden-hash path, so their floating-point reproducibility is the platform's rather than the paper's (paper/sections/10-limitations.tex:11).

**Present applicability.** The manuscript states the familywise correction is descriptive rather than simultaneous, which bounds the claim independently of freshness.

**Disposition.** `historical-only`. The artifact validly records the sweep; no documented recomputation covers this leg, unlike the positive control.

**Owner.** SharpeArena paper/evidence owner

**Follow-up.** none

**Status history.**

| Date | Disposition | Rationale |
|---|---|---|
| 2026-09-23 | historical-only | Initial entry: The artifact validly records the sweep; no documented recomputation covers this leg, unlike the positive control. |

### `SA-tab-f6`

Covers the **sharpearena** repository, which is not part of this tree. Paths below are relative to that repository's root.

**Claim.** Maker markout per filled unit over 24 paired episodes: the informed-uninformed gap widens from 0.072 at h = 1 to 0.436 at h = 20, with t-based 95 percent intervals at df 23.

**Source.** `paper/sections/07-validation.tex:179`, `paper/sections/07-validation.tex:204`, `paper/sections/07-validation.tex:172`

**Labels.** `tab:f6`, `fig:f6`

**Artifacts.**
- `paper/evidence/f6-adverse-selection.json`, sha256 `2e2e9199803693f6a15884e7461ed09d0a840ff36dc0405b5ac4e12833ee7e2b`, digest source: paper/evidence/provenance.json in the sharpearena repository, schema 5, generated at commit ddc9239ecb8938a6e75ec0a0d13c4955ef698021; read for this register on 2026-09-23 at that repository's HEAD d33b24919f0ef63fa432b7aef3694ff03e2e4797, not in this repository, keys comparison, detail_episode_makers, per_episode_markout_per_unit, gap_stats, ci_convention
- `paper/figures/f6-markouts.pdf`, sha256 `89d9fb5af7b4cd2e80cf0e87b4717d0b946a7879ba9837b5d7e270bd43751dfe`, digest source: paper/evidence/provenance.json in the sharpearena repository, schema 5, generated at commit ddc9239ecb8938a6e75ec0a0d13c4955ef698021; read for this register on 2026-09-23 at that repository's HEAD d33b24919f0ef63fa432b7aef3694ff03e2e4797, not in this repository

**Producer command.** python paper/src/make-f6-adverse-selection.py (paper/sections/A-commands.tex)

**Producing commit.** Unknown. paper/sections/A-commands.tex:5 is the paper's single provenance statement: only f1-baselines.json serializes a runtime version (sharpearena 0.9.0), no evidence file serializes a SharpeBench version, and the paper declines to reconstruct one from creation dates. No artifact carries a producing commit; the manifest records only the commit it was generated at.

**Effective configuration.** config = {n_episodes: 24, seed_base: 0, detail_seed: 0}. This is the thinnest config block of the eight findings: it does not record the maker roster, the alpha or the quote depths that the interpreting prose says drive the level result.

**Missing provenance.**
- As SA-tab-f4.
- The config block omits the maker roster, alpha and quote depths the level claim rests on.

**Relevant later changes.**
- Audit repair AP3 (docs/audits/2026-09-07/IMPLEMENTATION.md): F6's gate between two current code paths was reported as agreement with the committed vectors; the gate now carries its real name and a separate frozen-reference comparison reads the committed artifact before it is overwritten, reporting rather than raising.
- The endogenous arm and its key postdate the original run and share one artifact and one code path with the exogenous arm, pinned by a golden hash asserting the exogenous reports do not move when the endogenous arm runs (paper/sections/07-validation.tex:207).
- The Python reductions behind the realism, manipulation, adverse-selection, failure, ecology and predictability evidence are off the golden-hash path, so their floating-point reproducibility is the platform's rather than the paper's (paper/sections/10-limitations.tex:11).

**Present applicability.** The exogenous reports are pinned against movement when the endogenous arm runs, and AP3 replaced an assumed parity gate with a reported comparison. Neither establishes that the frozen values reproduce on the current tree.

**Disposition.** `historical-only`. The artifact validly records the run; the repairs named change how parity is reported rather than establishing that the frozen numbers still reproduce.

**Owner.** SharpeArena paper/evidence owner

**Follow-up.** none

**Status history.**

| Date | Disposition | Rationale |
|---|---|---|
| 2026-09-23 | historical-only | Initial entry: The artifact validly records the run; the repairs named change how parity is reported rather than establishing that the frozen numbers still reproduce. |

### `SA-f6-endogenous`

Covers the **sharpearena** repository, which is not part of this tree. Paths below are relative to that repository's root.

**Claim.** The endogenous arm's calibration arithmetic predicts a 0.933 percent displacement against a measured 0.81 percent [0.77, 0.85], and the six-value lambda sweep crosses sign in the informed level between lambda 0.2 and 0.3.

**Source.** `paper/sections/07-validation.tex:207`, `paper/sections/07-validation.tex:222`, `paper/sections/07-validation.tex:245`, `paper/sections/07-validation.tex:270`

**Labels.** `tab:f6-endo`, `tab:f6-endo-sweep`, `fig:f6-endo`

**Artifacts.**
- `paper/evidence/f6-adverse-selection.json`, sha256 `2e2e9199803693f6a15884e7461ed09d0a840ff36dc0405b5ac4e12833ee7e2b`, digest source: paper/evidence/provenance.json in the sharpearena repository, schema 5, generated at commit ddc9239ecb8938a6e75ec0a0d13c4955ef698021; read for this register on 2026-09-23 at that repository's HEAD d33b24919f0ef63fa432b7aef3694ff03e2e4797, not in this repository, key endogenous, ten sub-keys
- `paper/figures/f6-endogenous.pdf`, sha256 `cce8ff8cb77299935d2df45f2967848ad8fe4fbb0e5ce81abb7e1ac1ff838cb7`, digest source: paper/evidence/provenance.json in the sharpearena repository, schema 5, generated at commit ddc9239ecb8938a6e75ec0a0d13c4955ef698021; read for this register on 2026-09-23 at that repository's HEAD d33b24919f0ef63fa432b7aef3694ff03e2e4797, not in this repository

**Producer command.** python paper/src/make-f6-adverse-selection.py, whose same invocation runs the endogenous arm and its six-value impact sweep (paper/sections/A-commands.tex)

**Producing commit.** Unknown. paper/sections/A-commands.tex:5 is the paper's single provenance statement: only f1-baselines.json serializes a runtime version (sharpearena 0.9.0), no evidence file serializes a SharpeBench version, and the paper declines to reconstruct one from creation dates. No artifact carries a producing commit; the manifest records only the commit it was generated at.

**Effective configuration.** As SA-tab-f6; the sweep is declared exploratory with no sweep-wide multiplicity claim.

**Missing provenance.**
- As SA-tab-f6.
- The native clearing-engine cross-check quoted at relative 1e-12 names no test, and no test reproducing it was located.

**Relevant later changes.**
- As SA-tab-f6.

**Present applicability.** Recorded in the same artifact as the exogenous arm and subject to the same unestablished reproducibility. The sweep is declared exploratory in the manuscript.

**Disposition.** `historical-only`. The artifact validly records the arm; the one cross-check that would strengthen it names no test that could be located.

**Owner.** SharpeArena paper/evidence owner

**Follow-up.** P15: name the test behind the 1e-12 native cross-check, or record that none exists.

**Status history.**

| Date | Disposition | Rationale |
|---|---|---|
| 2026-09-23 | historical-only | Initial entry: The artifact validly records the arm; the one cross-check that would strengthen it names no test that could be located. |

### `SA-predictability`

Covers the **sharpearena** repository, which is not part of this tree. Paths below are relative to that repository's root.

**Claim.** An honest ridge AR(5) adversary reaches 55.4 percent directional accuracy on Calm against a 54.1 percent baseline, with a paired t over 16 Calm seeds of t(15) = 3.36, p = 0.004, and the known-seed oracle scores 100 percent accuracy and a deflated Sharpe of 1.00 on every tier.

**Source.** `paper/sections/07-validation.tex:273`, `paper/sections/07-validation.tex:285`

**Labels.** `fig:predictability`

**Artifacts.**
- `paper/evidence/predictability.json`, sha256 `bf4c2b3bb4c6311a0795a7b0b65a2fe00b76a06193f3051de0a1f63391782a8a`, digest source: paper/evidence/provenance.json in the sharpearena repository, schema 5, generated at commit ddc9239ecb8938a6e75ec0a0d13c4955ef698021; read for this register on 2026-09-23 at that repository's HEAD d33b24919f0ef63fa432b7aef3694ff03e2e4797, not in this repository, keys config, seed_search, tiers; every per-seed score and the measured search timing
- `paper/figures/predictability.pdf`, sha256 `75655375df8c1cd28c8af1f8dc5c205292a64aa8a5b4ccd24fb3063dc3786e9d`, digest source: paper/evidence/provenance.json in the sharpearena repository, schema 5, generated at commit ddc9239ecb8938a6e75ec0a0d13c4955ef698021; read for this register on 2026-09-23 at that repository's HEAD d33b24919f0ef63fa432b7aef3694ff03e2e4797, not in this repository

**Producer command.** python paper/src/make-predictability.py (paper/sections/A-commands.tex)

**Producing commit.** Unknown. paper/sections/A-commands.tex:5 is the paper's single provenance statement: only f1-baselines.json serializes a runtime version (sharpearena 0.9.0), no evidence file serializes a SharpeBench version, and the paper declines to reconstruct one from creation dates. No artifact carries a producing commit; the manifest records only the commit it was generated at.

**Effective configuration.** config = {tiers: calm, hard, extreme, n_symbols: 4, n_days: 120, seeds: 50000 to 50015, warmup_bars: 30, ar_order: 5, ridge_lambda: 1e-4, search_band_width: 65536, match_tolerance: 1e-9, policy: frictionless unit-gross long/short sign following scored via score_run deflated_sharpe}.

**Missing provenance.**
- The SharpeBench kernel version behind the deflated Sharpes is unserialized, and the limitations name this artifact among those carrying superseded moments.
- The artifact records per-seed scores rather than per-seed return series, so the deflated Sharpes cannot be recomputed from it under the corrected estimator.

**Relevant later changes.**
- Standardized-moment estimators mixed finite-sample normalizations, which moves deflated Sharpes and their intervals. SharpeBench corrected its estimators in 0.19.0 and SharpeArena its own confidence estimator in 0.25.0; the frozen F1, F3, witness and predictability artifacts predate both corrections and have not been rescored (paper/sections/10-limitations.tex:15).
- Audit repair AP1 (docs/audits/2026-09-07/IMPLEMENTATION.md): the oracle was scored on the full tape while the causal predictors started at the warmup bar; the oracle is now masked to the same window, and replaying the frozen tapes shows the published gap unaffected past the sixth decimal.
- The Python reductions behind the realism, manipulation, adverse-selection, failure, ecology and predictability evidence are off the golden-hash path, so their floating-point reproducibility is the platform's rather than the paper's (paper/sections/10-limitations.tex:11).

**Present applicability.** The accuracy and mean-squared-error figures do not pass through the scorer and are unaffected by the moment repair. The deflated Sharpes do, and are named in the limitations as carrying superseded moments and not rescored. AP1's effect is the one part with a documented replay comparison.

**Disposition.** `needs-new-experiment`. The deflated-Sharpe leg cannot be rescored from the artifact, which stores scores rather than return series, so recomputing it means re-running the producer. The producer is deterministic in its seeds, which makes the run cheap, but it is a new execution rather than a correction. P00-E maps evidence and authorizes no rerun. The follow-up names the ticket that would own one.

**Owner.** SharpeArena paper/evidence owner

**Follow-up.** P13-R/P14-R: re-run the predictability producer under the corrected pin and report it as a new experiment beside the frozen artifact.

**Status history.**

| Date | Disposition | Rationale |
|---|---|---|
| 2026-09-23 | needs-new-experiment | Initial entry: The deflated-Sharpe leg cannot be rescored from the artifact, which stores scores rather than return series, so recomputing it means re-running the producer. The producer is deterministic in its seeds, which makes the run cheap, but it is a new execution rather than a correction. P00-E maps evidence and authorizes no rerun. The follow-up names the ticket that would own one. |

### `SA-seed-search`

Covers the **sharpearena** repository, which is not part of this tree. Paths below are relative to that repository's root.

**Claim.** A brute scan over a 2^16 band recovers the true seed in 16 of 16 trials with zero collisions from one observed bar at about 14.7 microseconds per candidate, extrapolating to roughly 8.6 million CPU-years at 2^64.

**Source.** `paper/sections/07-validation.tex:273`

**Artifacts.**
- `paper/evidence/predictability.json`, sha256 `bf4c2b3bb4c6311a0795a7b0b65a2fe00b76a06193f3051de0a1f63391782a8a`, digest source: paper/evidence/provenance.json in the sharpearena repository, schema 5, generated at commit ddc9239ecb8938a6e75ec0a0d13c4955ef698021; read for this register on 2026-09-23 at that repository's HEAD d33b24919f0ef63fa432b7aef3694ff03e2e4797, not in this repository, key seed_search

**Producer command.** python paper/src/make-predictability.py (paper/sections/A-commands.tex)

**Producing commit.** Unknown. paper/sections/A-commands.tex:5 is the paper's single provenance statement: only f1-baselines.json serializes a runtime version (sharpearena 0.9.0), no evidence file serializes a SharpeBench version, and the paper declines to reconstruct one from creation dates. No artifact carries a producing commit; the manifest records only the commit it was generated at.

**Effective configuration.** Scan band width 65536, match tolerance 1e-9, 16 trials.

**Missing provenance.**
- The artifact records no host block, unlike throughput.json, so the per-candidate timing and the extrapolation carry no machine identity.

**Relevant later changes.**
- The Python reductions behind the realism, manipulation, adverse-selection, failure, ecology and predictability evidence are off the golden-hash path, so their floating-point reproducibility is the platform's rather than the paper's (paper/sections/10-limitations.tex:11).

**Present applicability.** The recovery counts are deterministic properties of the recorded run. The timing and the CPU-year extrapolation are hardware-dependent with no recorded host, and the manuscript labels the extrapolation harness-specific.

**Disposition.** `historical-only`. The artifact validly records the scan; the timing leg is bound to an unrecorded machine, which the missing-provenance field states rather than the disposition hiding.

**Owner.** SharpeArena paper/evidence owner

**Follow-up.** none

**Status history.**

| Date | Disposition | Rationale |
|---|---|---|
| 2026-09-23 | historical-only | Initial entry: The artifact validly records the scan; the timing leg is bound to an unrecorded machine, which the missing-provenance field states rather than the disposition hiding. |

### `SA-sealed-seeds`

Covers the **sharpearena** repository, which is not part of this tree. Paths below are relative to that repository's root.

**Claim.** On 16 tested slots the public-band scanner recovers 16 of 16 public seeds and 0 of 16 sealed seeds while the salt is withheld, and revealing the salt replays 16 of 16.

**Source.** `paper/sections/07-validation.tex:290`

**Artifacts.**
- `paper/evidence/sealed-seeds.json`, sha256 `a111ae0114f88f83a128970f60cebd00200d32540328137de927fa7ed6c8851f`, digest source: paper/evidence/provenance.json in the sharpearena repository, schema 5, generated at commit ddc9239ecb8938a6e75ec0a0d13c4955ef698021; read for this register on 2026-09-23 at that repository's HEAD d33b24919f0ef63fa432b7aef3694ff03e2e4797, not in this repository, keys config, table_build_s, public, sealed, note; every per-trial recovery and replay record

**Producer command.** python paper/src/make-sealed-seeds.py (paper/sections/A-commands.tex)

**Producing commit.** Unknown. paper/sections/A-commands.tex:5 is the paper's single provenance statement: only f1-baselines.json serializes a runtime version (sharpearena 0.9.0), no evidence file serializes a SharpeBench version, and the paper declines to reconstruct one from creation dates. No artifact carries a producing commit; the manifest records only the commit it was generated at.

**Effective configuration.** 16 slots, verification tier hard, 120 days, scan band [1000000, 1065536], match tolerance 1e-9.

**Missing provenance.**
- Runtime version unserialized.
- The separate salt-commitment artifact that audit repair AP4 introduced does not exist beside this file, so the frozen record predates the externally checkable commitment separation.
- The manuscript names the AP5 reveal-check repair but not the AP4 commitment-ordering repair, which is a disclosure gap rather than a resolved question.

**Relevant later changes.**
- Computation-changing repair AP5 (paper/sections/10-limitations.tex:15): the committed record's count of sixteen slots replayed was measured by a check that compared only each symbol's opening close in a two-day calm environment, while the declared evaluation is 120 days at the hard tier. The check now regenerates the complete tape and requires equality on every bar and symbol, so the frozen number is not evidence for the stronger property the reveal now tests.
- Audit repair AP4: the salt commitment is now written to its own artifact before the attacks run.

**Present applicability.** The manuscript states outright that the frozen count is not evidence for the property the current check tests. The replay claim as a reader would now read it is therefore unsupported by this artifact.

**Disposition.** `needs-new-experiment`. The measurement instrument changed, not just the scorer, and the artifact records the old check's outcome rather than inputs a new check could be run over. The producer is deterministic, so the new run is cheap, but it measures something the frozen record did not. P00-E maps evidence and authorizes no rerun. The follow-up names the ticket that would own one.

**Owner.** SharpeArena paper/evidence owner

**Follow-up.** P13-R/P14-R: re-run the sealed-seed reveal under the full-tape check and under the AP4 commitment ordering, reported as a new measurement.

**Status history.**

| Date | Disposition | Rationale |
|---|---|---|
| 2026-09-23 | needs-new-experiment | Initial entry: The measurement instrument changed, not just the scorer, and the artifact records the old check's outcome rather than inputs a new check could be run over. The producer is deterministic, so the new run is cheap, but it measures something the frozen record did not. P00-E maps evidence and authorizes no rerun. The follow-up names the ticket that would own one. |

### `SA-tab-f1`

Covers the **sharpearena** repository, which is not part of this tree. Paths below are relative to that repository's root.

**Claim.** Eighteen baseline rows over six policies and three tiers: no policy is rank-eligible on any tier, because eligibility requires a pass^k rate of exactly 1.00 and the best rate is 0.75.

**Source.** `paper/sections/08-findings.tex:22`, `paper/sections/08-findings.tex:63`, `paper/sections/08-findings.tex:7`

**Labels.** `tab:f1`, `fig:f1`

**Artifacts.**
- `paper/evidence/f1-baselines.json`, sha256 `ec61a9a1f4340d58d497545ce401c159bd1e61201cb7c001773dd66b9e381f67`, digest source: paper/evidence/provenance.json in the sharpearena repository, schema 5, generated at commit ddc9239ecb8938a6e75ec0a0d13c4955ef698021; read for this register on 2026-09-23 at that repository's HEAD d33b24919f0ef63fa432b7aef3694ff03e2e4797, not in this repository, the only evidence file carrying a runtime version, package_version 0.9.0; rows carry deflated_sharpe, its bootstrap interval, passed_k_rate, mean_return and per_seed_returns
- `paper/figures/f1-baselines.pdf`, sha256 `07e253fa1d96cdef8b30aff1c1e4cebe7ef673c296ca2fa7b7e3cbad8406b624`, digest source: paper/evidence/provenance.json in the sharpearena repository, schema 5, generated at commit ddc9239ecb8938a6e75ec0a0d13c4955ef698021; read for this register on 2026-09-23 at that repository's HEAD d33b24919f0ef63fa432b7aef3694ff03e2e4797, not in this repository

**Producer command.** python paper/src/make-f1-baselines.py (paper/sections/A-commands.tex)

**Producing commit.** sharpearena 0.9.0 is serialized in the artifact. The SharpeBench version is not; paper/sections/A-commands.tex:5 records that the commits adding the artifact lock SharpeBench 0.5.0. The producing commit is unknown.

**Effective configuration.** config = {n_symbols: 4, n_days: 120, seeds: 0 to 15, tiers: calm, hard, extreme, bootstrap: {n_boot: 2000, resample_seed: 1537679398 (0x5BA72026), alpha: 0.05, seed-paired percentile bootstrap on the deflated Sharpe}}.

**Missing provenance.**
- Producing commit, SharpeBench kernel version, effective-configuration readback and host platform.
- paper/sections/08-findings.tex:55 records that the dispersion-source claim holds by the producer path rather than by readback, because the committed artifacts do not serialize which dispersion source the scorer took.

**Relevant later changes.**
- Standardized-moment estimators mixed finite-sample normalizations, which moves deflated Sharpes and their intervals. SharpeBench corrected its estimators in 0.19.0 and SharpeArena its own confidence estimator in 0.25.0; the frozen F1, F3, witness and predictability artifacts predate both corrections and have not been rescored (paper/sections/10-limitations.tex:15).
- A repair after release 0.29.0 withholds the flat policy's values at SharpeArena's own boundary, and the pinned scorer is now SharpeBench 0.28.0, which refuses a constant track itself. Under the repaired code the flat rows would report a deflation error instead of a deflated Sharpe, a pass^k rate and an interval; the frozen table still prints 0.0007 (paper/sections/08-findings.tex:55).
- Apart from F7 the committed F1 to F8 artifacts were not rerun after effective-configuration readback, SPEC_HASH, canonical pre-hash fixtures, typed FFI errors, strict input decoding, path-containment checks or the prospective forecast contract were added. The manuscript states those additions strengthen present executions without changing the observations of artifacts that were not rerun (paper/sections/10-limitations.tex:13).
- The Python reductions behind the realism, manipulation, adverse-selection, failure, ecology and predictability evidence are off the golden-hash path, so their floating-point reproducibility is the platform's rather than the paper's (paper/sections/10-limitations.tex:11).

**Present applicability.** The table as printed is not what the current code would emit for the flat rows, which the manuscript states. The remaining deflated Sharpes and intervals carry the superseded moments.

**Disposition.** `needs-rescore`. The artifact serializes per_seed_returns, so the deflated Sharpes and their intervals are recomputable under the corrected estimator without re-running the environment. Sufficiency is evidenced by the manuscript's own worked instance, which rescores the committed F3 return series at six trials and reproduces 0.1114 exactly. Any rescore is separately versioned and does not overwrite the frozen artifact. P00-E maps evidence and authorizes no rerun. The follow-up names the ticket that would own one.

**Owner.** SharpeArena paper/evidence owner

**Follow-up.** P12-A/P13-R: rescore the committed per-seed returns under the corrected pin and publish the result beside, not in place of, the frozen table.

**Status history.**

| Date | Disposition | Rationale |
|---|---|---|
| 2026-09-23 | needs-rescore | Initial entry: The artifact serializes per_seed_returns, so the deflated Sharpes and their intervals are recomputable under the corrected estimator without re-running the environment. Sufficiency is evidenced by the manuscript's own worked instance, which rescores the committed F3 return series at six trials and reproduces 0.1114 exactly. Any rescore is separately versioned and does not overwrite the frozen artifact. P00-E maps evidence and authorizes no rerun. The follow-up names the ticket that would own one. |

### `SA-tab-witness`

Covers the **sharpearena** repository, which is not part of this tree. Paths below are relative to that repository's root.

**Claim.** The eligibility set is nonempty: observed crossings in nominal signal strength over five noise paths, and at every one of the 50 attained crossings the last failing gate is the per-run reliability gate.

**Source.** `paper/sections/08-findings.tex:78`, `paper/sections/08-findings.tex:99`, `paper/sections/08-findings.tex:66`

**Labels.** `tab:witness`, `fig:witness`

**Artifacts.**
- `paper/evidence/witness.json`, sha256 `bc80f9f93627cab952b2edf61d93d50a5de86c601aef5b17feab2eb18f16578b`, digest source: paper/evidence/provenance.json in the sharpearena repository, schema 5, generated at commit ddc9239ecb8938a6e75ec0a0d13c4955ef698021; read for this register on 2026-09-23 at that repository's HEAD d33b24919f0ef63fa432b7aef3694ff03e2e4797, not in this repository, keys finding, oracle_disclosure, config, results, boundaries, noise_replicates
- `paper/figures/witness.pdf`, sha256 `cb050f83fa5c7a5cf7dc1092dfe60567baaa42fd680cd26fd3ca3919d6d831e5`, digest source: paper/evidence/provenance.json in the sharpearena repository, schema 5, generated at commit ddc9239ecb8938a6e75ec0a0d13c4955ef698021; read for this register on 2026-09-23 at that repository's HEAD d33b24919f0ef63fa432b7aef3694ff03e2e4797, not in this repository

**Producer command.** python paper/src/make-witness.py (paper/sections/A-commands.tex)

**Producing commit.** Unknown. paper/sections/A-commands.tex:5 is the paper's single provenance statement: only f1-baselines.json serializes a runtime version (sharpearena 0.9.0), no evidence file serializes a SharpeBench version, and the paper declines to reconstruct one from creation dates. No artifact carries a producing commit; the manifest records only the commit it was generated at.

**Effective configuration.** config includes bands {held_out: 10016 to 10031, f1_table: 0 to 15}, seed_gap 10000, n_trials 6 on the F1 footprint, 13 strength points, bisect_resolution 0.005, kernel_gates {per_run_psr_bar: 0.9, dsr_bar: 0.95, alpha: 0.05}.

**Missing provenance.**
- SharpeBench version, which matters directly because the result is a kernel-gate crossing.
- The artifact records crossings and boundaries rather than per-seed return series, so the gates cannot be recomputed from it.

**Relevant later changes.**
- Standardized-moment estimators mixed finite-sample normalizations, which moves deflated Sharpes and their intervals. SharpeBench corrected its estimators in 0.19.0 and SharpeArena its own confidence estimator in 0.25.0; the frozen F1, F3, witness and predictability artifacts predate both corrections and have not been rescored (paper/sections/10-limitations.tex:15). The witness is named in that group.
- Apart from F7 the committed F1 to F8 artifacts were not rerun after effective-configuration readback, SPEC_HASH, canonical pre-hash fixtures, typed FFI errors, strict input decoding, path-containment checks or the prospective forecast contract were added. The manuscript states those additions strengthen present executions without changing the observations of artifacts that were not rerun (paper/sections/10-limitations.tex:13).
- The Python reductions behind the realism, manipulation, adverse-selection, failure, ecology and predictability evidence are off the golden-hash path, so their floating-point reproducibility is the platform's rather than the paper's (paper/sections/10-limitations.tex:11).

**Present applicability.** The crossings are gate outcomes computed by a scorer that has since been corrected, and the artifact does not carry the returns a corrected scorer would need. No record was found that this artifact was rerun.

**Disposition.** `needs-new-experiment`. Missing inputs prevent a rescore: the artifact stores crossings, not the trajectories behind them, so establishing the crossing under the corrected gates means re-running the bisection. The producer is deterministic in its seeds. P00-E maps evidence and authorizes no rerun. The follow-up names the ticket that would own one.

**Owner.** SharpeArena paper/evidence owner

**Follow-up.** P13-R/P14-R: re-run the rank-eligibility witness under the corrected pin as a declared new experiment.

**Status history.**

| Date | Disposition | Rationale |
|---|---|---|
| 2026-09-23 | needs-new-experiment | Initial entry: Missing inputs prevent a rescore: the artifact stores crossings, not the trajectories behind them, so establishing the crossing under the corrected gates means re-running the bisection. The producer is deterministic in its seeds. P00-E maps evidence and authorizes no rerun. The follow-up names the ticket that would own one. |

### `SA-f2-regret`

Covers the **sharpearena** repository, which is not part of this tree. Paths below are relative to that repository's root.

**Claim.** Regret against the closed-form reference over 16 paired episodes is U-shaped in the half-spread, 84.6 [68.2, 101.1] at 0.05, minimal at 0.5 and 58.8 [55.3, 62.3] at 4.0, with 16 episodes detecting about 12 reward units at 80 percent power.

**Source.** `paper/sections/08-findings.tex:102`, `paper/sections/08-findings.tex:114`

**Labels.** `fig:f2`

**Artifacts.**
- `paper/evidence/f2-regret.json`, sha256 `7dcf2e656b4f73e8af7e4dba90721bab63f79b8de4a01c682b299b9f693c80ee`, digest source: paper/evidence/provenance.json in the sharpearena repository, schema 5, generated at commit ddc9239ecb8938a6e75ec0a0d13c4955ef698021; read for this register on 2026-09-23 at that repository's HEAD d33b24919f0ef63fa432b7aef3694ff03e2e4797, not in this repository, keys config, optimal_regret, fixed_spread_regret, per_episode_regret, regret_dispersion
- `paper/figures/f2-regret.pdf`, sha256 `634c38c28a152ebefbc5acbd8620c5820e89269e3486cceb912d8398dc346be6`, digest source: paper/evidence/provenance.json in the sharpearena repository, schema 5, generated at commit ddc9239ecb8938a6e75ec0a0d13c4955ef698021; read for this register on 2026-09-23 at that repository's HEAD d33b24919f0ef63fa432b7aef3694ff03e2e4797, not in this repository

**Producer command.** python paper/src/make-f2-regret.py (paper/sections/A-commands.tex)

**Producing commit.** Unknown. paper/sections/A-commands.tex:5 is the paper's single provenance statement: only f1-baselines.json serializes a runtime version (sharpearena 0.9.0), no evidence file serializes a SharpeBench version, and the paper declines to reconstruct one from creation dates. No artifact carries a producing commit; the manifest records only the commit it was generated at.

**Effective configuration.** config = {params: {sigma: 2.0, gamma: 0.1, kappa: 1.5, arrival_rate: 140.0, n_steps: 200, dt: 0.005, inventory_cap: 50, phi: 0.0015}, n_episodes: 16, seed_base: 0, half_spreads: seven points from 0.05 to 4.0, t-based 95 percent at df 15}.

**Missing provenance.**
- Producing commit, runtime version and host. The manuscript names sigma, gamma, kappa and the step count but not the arrival rate or phi, which the artifact does record.

**Relevant later changes.**
- Release 0.31.0 makes the market-making regret refuse two arms that drew different random streams, and the committed F2 regrets pass that check unchanged (paper/sections/10-limitations.tex:15).

**Present applicability.** Re-established against the current guard: a committed test runs every half-spread of this subsection through the checked function at the default parameters and reproduces the frozen regrets bit for bit (paper/sections/08-findings.tex:108).

**Disposition.** `still-applicable`. This is the one artifact in the manuscript with a recorded comparison against a later guard that reproduces the frozen numbers exactly, which is a check rather than an inference.

**Owner.** SharpeArena paper/evidence owner

**Follow-up.** none

**Status history.**

| Date | Disposition | Rationale |
|---|---|---|
| 2026-09-23 | still-applicable | Initial entry: This is the one artifact in the manuscript with a recorded comparison against a later guard that reproduces the frozen numbers exactly, which is a check rather than an inference. |

### `SA-tab-f3`

Covers the **sharpearena** repository, which is not part of this tree. Paths below are relative to that repository's root.

**Claim.** Within-tier train minus test deflated Sharpe gaps of +0.0012 Calm, -0.3388 Hard and -0.1012 Extreme, and a transfer matrix with Calm to Extreme +0.973.

**Source.** `paper/sections/08-findings.tex:124`, `paper/sections/08-findings.tex:117`

**Labels.** `tab:f3`

**Artifacts.**
- `paper/evidence/f3-generalization.json`, sha256 `3bff445e13dd4d094a45ebe20e713ec0bc63b06e017c30a399d3aa41a8cf50e1`, digest source: paper/evidence/provenance.json in the sharpearena repository, schema 5, generated at commit ddc9239ecb8938a6e75ec0a0d13c4955ef698021; read for this register on 2026-09-23 at that repository's HEAD d33b24919f0ef63fa432b7aef3694ff03e2e4797, not in this repository, keys config, generalization_gap, cross_regime_transfer, per_seed_returns, band_dsr_ci, transfer_gap_ci

**Producer command.** python paper/src/make-f3-generalization.py (paper/sections/A-commands.tex). The appendix records that the transfer matrix is reported as a table only and that the paper carries no F3 figure.

**Producing commit.** Unknown. paper/sections/A-commands.tex:5 is the paper's single provenance statement: only f1-baselines.json serializes a runtime version (sharpearena 0.9.0), no evidence file serializes a SharpeBench version, and the paper declines to reconstruct one from creation dates. No artifact carries a producing commit; the manifest records only the commit it was generated at.

**Effective configuration.** config = {n_symbols: 4, n_days: 120, n_train: 16, n_test: 16, seed_gap: 10000, transfer_seeds: 0 to 15, bootstrap: {n_boot: 2000, resample_seed: 0, alpha: 0.05}}. The resample seed differs from F1's.

**Missing provenance.**
- As SA-tab-f1, plus the SharpeBench version behind the deflated Sharpes.

**Relevant later changes.**
- Standardized-moment estimators mixed finite-sample normalizations, which moves deflated Sharpes and their intervals. SharpeBench corrected its estimators in 0.19.0 and SharpeArena its own confidence estimator in 0.25.0; the frozen F1, F3, witness and predictability artifacts predate both corrections and have not been rescored (paper/sections/10-limitations.tex:15). F3 is named in that group.
- Apart from F7 the committed F1 to F8 artifacts were not rerun after effective-configuration readback, SPEC_HASH, canonical pre-hash fixtures, typed FFI errors, strict input decoding, path-containment checks or the prospective forecast contract were added. The manuscript states those additions strengthen present executions without changing the observations of artifacts that were not rerun (paper/sections/10-limitations.tex:13).
- The Python reductions behind the realism, manipulation, adverse-selection, failure, ecology and predictability evidence are off the golden-hash path, so their floating-point reproducibility is the platform's rather than the paper's (paper/sections/10-limitations.tex:11).

**Present applicability.** The deflated Sharpes carry superseded moments. The artifact serializes per-seed return series, and the manuscript demonstrates rescoring them: rescoring the committed F3 return series at six trials reproduces 0.1114 exactly.

**Disposition.** `needs-rescore`. Recorded inputs suffice: per_seed_returns is committed and the manuscript records a worked rescore of that series, so a corrected-estimator result is a recomputation rather than a new run. P00-E maps evidence and authorizes no rerun. The follow-up names the ticket that would own one.

**Owner.** SharpeArena paper/evidence owner

**Follow-up.** P12-A/P13-R: rescore the committed F3 return series under the corrected pin as a separately versioned output.

**Status history.**

| Date | Disposition | Rationale |
|---|---|---|
| 2026-09-23 | needs-rescore | Initial entry: Recorded inputs suffice: per_seed_returns is committed and the manuscript records a worked rescore of that series, so a corrected-estimator result is a recomputation rather than a new run. P00-E maps evidence and authorizes no rerun. The follow-up names the ticket that would own one. |

### `SA-tab-f7`

Covers the **sharpearena** repository, which is not part of this tree. Paths below are relative to that repository's root.

**Claim.** Failure-mode distributions over 384 episodes: clean rates of 0.40, 0.38 and 0.34 by tier, with 231 of 384 episodes ending in a process failure and 231 of 241 failures non-PnL.

**Source.** `paper/sections/08-findings.tex:158`, `paper/sections/08-findings.tex:180`, `paper/sections/08-findings.tex:151`

**Labels.** `tab:f7`, `fig:f7`

**Artifacts.**
- `paper/evidence/f7-failures.json`, sha256 `e30fc65eafc183df18bc5f7e0b6c80a4194889af2cbd3388786926b167f86bfc`, digest source: paper/evidence/provenance.json in the sharpearena repository, schema 5, generated at commit ddc9239ecb8938a6e75ec0a0d13c4955ef698021; read for this register on 2026-09-23 at that repository's HEAD d33b24919f0ef63fa432b7aef3694ff03e2e4797, not in this repository, keys config, effective_config, a 384-entry episodes list, rollup_by_tier and rollup_overall
- `paper/figures/f7-failures.pdf`, sha256 `a8848234748264a3d3dd489cb763d7238c1a5d6843af0800383e365f575f7a21`, digest source: paper/evidence/provenance.json in the sharpearena repository, schema 5, generated at commit ddc9239ecb8938a6e75ec0a0d13c4955ef698021; read for this register on 2026-09-23 at that repository's HEAD d33b24919f0ef63fa432b7aef3694ff03e2e4797, not in this repository

**Producer command.** python paper/src/make-f7-failures.py (paper/sections/A-commands.tex)

**Producing commit.** Regenerated for SharpeArena 0.29.0 by re-running its producer, which the manuscript states at paper/sections/08-findings.tex:153. The runtime version is not serialized in the artifact, so the attribution rests on the manuscript and the release history.

**Effective configuration.** config = {n_symbols: 4, n_days: 120, seeds: 0 to 15, max_steps: 512, max_drawdown: 0.5, three tiers, eight policies}; effective_config carries per-tier panel dimensions, window, verified true and 16 per-seed dataset fingerprints.

**Missing provenance.**
- Runtime version inside the artifact, SharpeBench version, producing commit and host.

**Relevant later changes.**
- The failure-taxonomy repair, under which an episode that failed, went missing or returned a nonfinite value can no longer default to a clean disposition, is reflected in this artifact because it was regenerated; the mandate draw change is too (paper/sections/10-limitations.tex:15).

**Present applicability.** The only committed artifact that absorbed the post-freeze repairs rather than lagging them, and the only one carrying an effective-configuration readback.

**Disposition.** `still-applicable`. The documented comparison is the recorded regeneration for 0.29.0 after the taxonomy repair and the mandate draw change, which the manuscript states and the artifact's readback block supports.

**Owner.** SharpeArena paper/evidence owner

**Follow-up.** P15: serialize a runtime version in the artifact so the 0.29.0 attribution does not rest on prose.

**Status history.**

| Date | Disposition | Rationale |
|---|---|---|
| 2026-09-23 | still-applicable | Initial entry: The documented comparison is the recorded regeneration for 0.29.0 after the taxonomy repair and the mandate draw change, which the manuscript states and the artifact's readback block supports. |

### `SA-tab-f8`

Covers the **sharpearena** repository, which is not part of this tree. Paths below are relative to that repository's root.

**Claim.** Ecology outcomes over eight replicator seeds per schedule: the root winner differs between the control and shocked schedules on five of eight seeds, and a single-seed reading does not survive replication.

**Source.** `paper/sections/08-findings.tex:192`, `paper/sections/08-findings.tex:224`, `paper/sections/08-findings.tex:183`

**Labels.** `tab:f8`, `fig:f8`

**Artifacts.**
- `paper/evidence/f8-ecology.json`, sha256 `bed6e9e3e5430e076685db9d3bd2b912ea531859f0134cff89aea4082990ff19`, digest source: paper/evidence/provenance.json in the sharpearena repository, schema 5, generated at commit ddc9239ecb8938a6e75ec0a0d13c4955ef698021; read for this register on 2026-09-23 at that repository's HEAD d33b24919f0ef63fa432b7aef3694ff03e2e4797, not in this repository, keys config, control, shocked, control_table, shocked_table, multi_seed
- `paper/figures/f8-ecology-control.pdf`, sha256 `299f46fb1d5032a85fac289948eb0ded8d1bd42234a71e61a8db91622cfa8ae8`, digest source: paper/evidence/provenance.json in the sharpearena repository, schema 5, generated at commit ddc9239ecb8938a6e75ec0a0d13c4955ef698021; read for this register on 2026-09-23 at that repository's HEAD d33b24919f0ef63fa432b7aef3694ff03e2e4797, not in this repository
- `paper/figures/f8-ecology-shocked.pdf`, sha256 `5ed376a14d2fe58951a032793e535921708fddb1aae9e94b8ac393020a403511`, digest source: paper/evidence/provenance.json in the sharpearena repository, schema 5, generated at commit ddc9239ecb8938a6e75ec0a0d13c4955ef698021; read for this register on 2026-09-23 at that repository's HEAD d33b24919f0ef63fa432b7aef3694ff03e2e4797, not in this repository

**Producer command.** python paper/src/make-f8-ecology.py (paper/sections/A-commands.tex)

**Producing commit.** Unknown. paper/sections/A-commands.tex:5 is the paper's single provenance statement: only f1-baselines.json serializes a runtime version (sharpearena 0.9.0), no evidence file serializes a SharpeBench version, and the paper declines to reconstruct one from creation dates. No artifact carries a producing commit; the manifest records only the commit it was generated at.

**Effective configuration.** config = {generations: 12, field_size: 8, seeds: 0 to 7, detail_seed: 0, n_symbols: 4, n_days: 120, max_steps: 256, innovate_every: 4, shock_period: 4}. The figures are seed 0 only, which the caption states.

**Missing provenance.**
- As the other non-F7 artifacts.

**Relevant later changes.**
- Pre-freeze regeneration during the August 2026 review round extended the producer to eight replicator seeds per schedule. No post-freeze change bearing on F8 was found.
- The Python reductions behind the realism, manipulation, adverse-selection, failure, ecology and predictability evidence are off the golden-hash path, so their floating-point reproducibility is the platform's rather than the paper's (paper/sections/10-limitations.tex:11).

**Present applicability.** Bounded by the manuscript's own statement that the replicator is an abstraction and that a comparable simulator runs 128 replications per configuration against this artifact's eight.

**Disposition.** `historical-only`. The artifact validly records the run and no later change reaches it, but nothing re-establishes it against the current tree.

**Owner.** SharpeArena paper/evidence owner

**Follow-up.** none

**Status history.**

| Date | Disposition | Rationale |
|---|---|---|
| 2026-09-23 | historical-only | Initial entry: The artifact validly records the run and no later change reaches it, but nothing re-establishes it against the current tree. |

### `SA-tab-throughput`

Covers the **sharpearena** repository, which is not part of this tree. Paths below are relative to that repository's root.

**Claim.** Native Rust runs 747,468 steps a second, WebAssembly 508,127 and Python 14,240; the full six-policy field over the canonical held-out band takes 302 seconds.

**Source.** `paper/sections/06-protocol.tex:85`, `paper/sections/06-protocol.tex:78`

**Labels.** `tab:throughput`

**Artifacts.**
- `paper/evidence/throughput.json`, sha256 `dae509f677049e673bb9f3040b776e046fd7ee8210ee8f13e8a2922179a99d17`, digest source: paper/evidence/provenance.json in the sharpearena repository, schema 5, generated at commit ddc9239ecb8938a6e75ec0a0d13c4955ef698021; read for this register on 2026-09-23 at that repository's HEAD d33b24919f0ef63fa432b7aef3694ff03e2e4797, not in this repository, the only artifact with a host block: platform, processor and Python version, plus canonical_evaluation with the three measured runs

**Producer command.** python paper/src/make-throughput.py, which shells out to cargo run --release -p sharpearena --example bench-steps and node npm/sharpearena/bench/throughput.js (paper/sections/A-commands.tex)

**Producing commit.** Unknown. paper/sections/A-commands.tex:5 is the paper's single provenance statement: only f1-baselines.json serializes a runtime version (sharpearena 0.9.0), no evidence file serializes a SharpeBench version, and the paper declines to reconstruct one from creation dates. No artifact carries a producing commit; the manifest records only the commit it was generated at. The host block is recorded: Windows 11, an AMD64 family 25 processor and Python 3.12.6.

**Effective configuration.** episodes_per_repeat 500, repeats 3, steps_per_repeat 60000, wasm steps_per_episode 100.

**Missing provenance.**
- Runtime version, producing commit, and the Rust toolchain and Node versions behind the two shelled-out benchmarks.

**Relevant later changes.**
- Audit repair AP2: the WebAssembly throughput producer and the npm package manifest were outside the provenance source scope, so the producer could change without moving source_snapshot_sha256. Both are now in scope.

**Present applicability.** Hardware-dependent by nature, and the appendix names throughput as the one quantity a replicator should expect to differ. The host block makes the measurement identifiable, which no other artifact offers.

**Disposition.** `historical-only`. The artifact validly records a timed run on a named host; a current measurement would be a different machine's number rather than a correction of this one.

**Owner.** SharpeArena paper/evidence owner

**Follow-up.** none

**Status history.**

| Date | Disposition | Rationale |
|---|---|---|
| 2026-09-23 | historical-only | Initial entry: The artifact validly records a timed run on a named host; a current measurement would be a different machine's number rather than a correction of this one. |

### `SA-orderbook-mark`

Covers the **sharpearena** repository, which is not part of this tree. Paths below are relative to that repository's root.

**Claim.** Over seeds 0 to 31 with no inventory penalty a lone quoting agent earns a mean episode reward of 881.4 against 897.1 for cash plus inventory at the final reference mid, where the frozen mark it replaced paid 2360.3.

**Source.** `paper/sections/03-environment.tex:112`

**Artifacts.**
- none located

**Producer command.** none recorded. No producer paragraph in paper/sections/A-commands.tex covers this number, and the manuscript's reproducibility statement scopes its producer guarantee to the validation and findings sections, which this claim sits outside.

**Producing commit.** unknown; the same three numbers appear in CHANGELOG.md under release 0.31.0.

**Effective configuration.** Stated in prose only: seeds 0 to 31, no inventory penalty, one tick below the reference mid on the bid and twenty above on the ask, sixty steps of mid-walking. No seed serialization, no interval.

**Missing provenance.**
- No artifact in paper/evidence and none in the provenance manifest.
- No producer command.
- No interval and no per-seed record.
- Whether a committed test reproduces the three numbers was not established.

**Relevant later changes.**
- The claim is itself the record of a later change: the order-book environment's default inventory mark changed twice, so rewards from earlier runs reproduce only under one mark setting and rewards under the intervening fallback do not reproduce at all; no committed artifact uses the environment (paper/sections/10-limitations.tex:15).

**Present applicability.** Not established. The numbers are corroborated only by a changelog entry that states them in the same words.

**Disposition.** `unresolved`. The artifact cannot be located, so the ticket's rule applies: record unresolved rather than guess.

**Owner.** SharpeArena paper/evidence owner

**Follow-up.** P15: commit a producer and artifact for the order-book mark comparison, or state it in the manuscript as an uncommitted measurement.

**Status history.**

| Date | Disposition | Rationale |
|---|---|---|
| 2026-09-23 | unresolved | Initial entry: The artifact cannot be located, so the ticket's rule applies: record unresolved rather than guess. |

### `SA-seeded-shuffle`

Covers the **sharpearena** repository, which is not part of this tree. Paths below are relative to that repository's root.

**Claim.** Seat ordering under the default agent-index priority advantages the lower index, which the seeded shuffle removes.

**Source.** `paper/sections/03-environment.tex:110`

**Artifacts.**
- none located

**Producer command.** none recorded. The supporting measurement, two identical quoters filling seat 0 more often on 32 of 32 seeds under the default order and 15 of 32 under a seeded shuffle, appears only in CHANGELOG.md under release 0.31.0.

**Producing commit.** unknown

**Effective configuration.** Stated in the changelog only: seeds 0 to 31, two identical three-tick quoters.

**Missing provenance.**
- No artifact in paper/evidence and none in the manifest.
- No producer command, and the manuscript states the property qualitatively without citing the measurement it rests on.

**Relevant later changes.**
- The seeded-shuffle priority is itself the later change; the measurement predates no repair.

**Present applicability.** Not established from the paper's evidence. The manuscript's qualitative claim rests on a measurement it does not cite.

**Disposition.** `unresolved`. The supporting measurement has no artifact in the evidence scope, so no disposition can be chosen without guessing.

**Owner.** SharpeArena paper/evidence owner

**Follow-up.** P15: cite the seat-ordering measurement or commit it as evidence.

**Status history.**

| Date | Disposition | Rationale |
|---|---|---|
| 2026-09-23 | unresolved | Initial entry: The supporting measurement has no artifact in the evidence scope, so no disposition can be chosen without guessing. |

### `SA-nonempirical-tables`

Covers the **sharpearena** repository, which is not part of this tree. Paths below are relative to that repository's root.

**Claim.** The observation and action wire schemas, the leakage-channel summary and the related-work comparison.

**Source.** `paper/sections/03-environment.tex:20`, `paper/sections/03-environment.tex:55`, `paper/sections/06-protocol.tex:36`, `paper/sections/09-related.tex:12`

**Labels.** `tab:obs-space`, `tab:act-space`, `tab:leakage`, `tab:related`

**Artifacts.**
- The leakage table is declared non-additive in its own section: every entry restates a property established in the section it cites, so the table adds no result. The two space tables describe the wire schema, and the related-work table's caption states that entries rest on the authors' reading where the cited authors do not claim the property.

**Producer command.** none; these tables are written from the interface, the protocol and the cited literature.

**Producing commit.** not applicable

**Effective configuration.** not applicable

**Missing provenance.**
- The related-work table's entries rest on the authors' reading, which the caption states, and there is no per-cell provenance sheet of the kind the companion product keeps.

**Relevant later changes.**
- A schema change to the observation or action space would move the two space tables; none was found in scope for this register.

**Present applicability.** Applicable as descriptions rather than measurements.

**Disposition.** `still-applicable`. No cell reports a measurement, so no artifact can go stale under these tables.

**Owner.** SharpeArena paper/evidence owner

**Follow-up.** P15: consider a per-cell provenance sheet for the related-work table, as the companion product keeps.

**Status history.**

| Date | Disposition | Rationale |
|---|---|---|
| 2026-09-23 | still-applicable | Initial entry: No cell reports a measurement, so no artifact can go stale under these tables. |

## SharpeBench

### `SB-tab-design-constraints`

**Claim.** Design constraints: each principle stated as the requirement the kernel or protocol enforces and the failure mode a benchmark violating it would exhibit.

**Source.** `paper/sections/02-principles.tex:22`

**Labels.** `tab:design-constraints`

**Artifacts.**
- The caption states that the requirement column names mechanisms and that whether each is exercised by the frozen evidence is stated in the section describing it (paper/sections/02-principles.tex:21); no cell reports a measurement.

**Producer command.** none; the table is written from the protocol, not computed.

**Producing commit.** not applicable

**Effective configuration.** not applicable

**Missing provenance.**
- none identified

**Relevant later changes.**
- none identified

**Present applicability.** Applicable as a statement of protocol requirements. Each row's empirical support is carried by the section it points to, which has its own register row.

**Disposition.** `still-applicable`. The table reports no measurement, so no artifact can go stale under it; the mechanisms it names are the current tree's, exercised by the test list in paper/sections/A-commands.tex.

**Owner.** SharpeBench paper/evidence owner

**Follow-up.** none

**Status history.**

| Date | Disposition | Rationale |
|---|---|---|
| 2026-09-23 | still-applicable | Initial entry: The table reports no measurement, so no artifact can go stale under it; the mechanisms it names are the current tree's, exercised by the test list in paper/sections/A-commands.tex. |

### `SB-tab-leakage`

**Claim.** Leakage channels, the control SharpeBench applies to each, and the exposure each control leaves open.

**Source.** `paper/sections/04-integrity.tex:10`

**Labels.** `tab:leakage`

**Artifacts.**
- The caption states outright: "No row reports a measurement" (paper/sections/04-integrity.tex:9).

**Producer command.** none; the table is written from the protocol and its known gaps.

**Producing commit.** not applicable

**Effective configuration.** not applicable

**Missing provenance.**
- none identified

**Relevant later changes.**
- none identified

**Present applicability.** Applicable as a statement of controls and residual exposure. Rows that name a mechanism inherit that mechanism's current behaviour, not a frozen measurement.

**Disposition.** `still-applicable`. No measurement is reported, and the controls named are properties of the current tree rather than of the frozen snapshot.

**Owner.** SharpeBench paper/evidence owner

**Follow-up.** none

**Status history.**

| Date | Disposition | Rationale |
|---|---|---|
| 2026-09-23 | still-applicable | Initial entry: No measurement is reported, and the controls named are properties of the current tree rather than of the frozen snapshot. |

### `SB-tab-attacks`

**Claim.** The ten-attack self-audit battery: every attack is demoted on every commit, nine against the scoring kernel and the tenth against the forward arena's intake.

**Source.** `paper/sections/04-integrity.tex:43`

**Labels.** `tab:attacks`

**Artifacts.**
- `crates/sharpebench-core/src/selfaudit.rs`, sha256 `579718f85fe269b33174681aad905a3803da9ff5a6a67278cb31afea2af13a16`, digest source: paper/evidence/provenance.json, carries the battery and the commit-time assertions that every case is defended and no known gap remains
- `crates/sharpebench-arena/src/audit.rs`, sha256 `12c13aadaa1c5e0028e8d4872838ee6fb332b4fd48bb1bd9906185641620eae4`, digest source: paper/evidence/provenance.json, the tenth case, against the forward arena's intake

**Producer command.** sharpebench audit (paper/sections/A-commands.tex); the claim is enforced by the commit-time tests in the two files above.

**Producing commit.** current engineering tree; the exact commit is not recorded in the manuscript.

**Effective configuration.** Fixed synthetic constructions; the per-case detail the command prints is not reported as a result in the paper (paper/sections/04-integrity.tex:35).

**Missing provenance.**
- No audit report artifact is committed; the evidence is the test assertions rather than a stored output.

**Relevant later changes.**
- The ninth case was a measured known gap until ranking gained the clone collapse of sec:sybil; its test now asserts that the exploit reproduces when the defense is switched off (paper/sections/04-integrity.tex:42).

**Present applicability.** Applicable to the current tree: the assertions run on every commit, so the claim is re-established rather than inherited.

**Disposition.** `still-applicable`. The documented comparison is the commit-time test itself, which re-checks the claim against the current kernel on every commit.

**Owner.** SharpeBench paper/evidence owner

**Follow-up.** none

**Status history.**

| Date | Disposition | Rationale |
|---|---|---|
| 2026-09-23 | still-applicable | Initial entry: The documented comparison is the commit-time test itself, which re-checks the claim against the current kernel on every commit. |

### `SB-tab-data`

**Claim.** The nine frozen datasets with bars, periods per year, range and the stylized-facts realism verdict; US indices 1w fails aggregational Gaussianity and FX 1d fails the time-reversal asymmetry test.

**Source.** `paper/sections/03-benchmark.tex:84`

**Labels.** `tab:data`

**Artifacts.**
- `data/us-indices-1d.csv`, sha256 `a153e1d7fb306668509d7d47203b142dc4409d33a8a3ef9e6b9a7f5f88e67ccd`, digest source: paper/evidence/provenance.json
- `data/us-indices-1w.csv`, sha256 `160cef2617097f3b4ebd3655b4be468efde5bcc3a6df9858f7d2b699b7affdf8`, digest source: paper/evidence/provenance.json
- `data/crypto-majors-1h.csv`, sha256 `869f570767b04b520cc95788f17ab2e244048a1bb850128a5abe6520630e8de0`, digest source: paper/evidence/provenance.json
- `data/crypto-majors-4h.csv`, sha256 `bc2728a9409b0e2b66795c81990d670cbbbb332065997ff933d5d2d7f326d2e6`, digest source: paper/evidence/provenance.json
- `data/crypto-majors-1d.csv`, sha256 `1854e76e9811387dd1ff3db64308eceb613ad05c9eee0011762eddc48f538a26`, digest source: paper/evidence/provenance.json
- `data/crypto-majors-1w.csv`, sha256 `506524623af472c9ca314e2efca12a0584a44d52826a4ea6509933fe05519214`, digest source: paper/evidence/provenance.json
- `data/fx-majors-1d.csv`, sha256 `eb7267ce8135662ab48407c8c33c8bfd901546063e4acd8c5f1c18163cb09e99`, digest source: paper/evidence/provenance.json
- `data/commodities-1d.csv`, sha256 `e174f30a79b2d5520124bbd22791b0352fa498c310d218c764993ce6902f9cd5`, digest source: paper/evidence/provenance.json
- `data/rates-1d.csv`, sha256 `2e375b15bf334e3cc604d09a518d97a1cc141d91047c7df641009498e80ed24e`, digest source: paper/evidence/provenance.json

**Producer command.** sharpebench realism --data data/<dataset>.csv (paper/sections/A-commands.tex)

**Producing commit.** unknown; the verdicts carry no producing version.

**Effective configuration.** The shipped stylized-facts battery at its defaults; no configuration is recorded with the verdicts.

**Missing provenance.**
- No realism report artifact is committed, so the pass and fail verdicts in the table have no stored output to compare against.
- The kernel version that produced the verdicts is not recorded.

**Relevant later changes.**
- No repair recorded in paper/sections/E-repairs.tex names the realism battery.

**Present applicability.** The inputs are hash-pinned and present, and the producing command is on the current tree, so the verdicts are recomputable; whether they still read the same is not established here.

**Disposition.** `needs-rescore`. The recorded inputs are sufficient: the battery is a deterministic function of the committed CSVs and the command exists on the current tree, so a current-method verdict is a recomputation rather than a new experiment. P00-E maps evidence and authorizes no rerun. The follow-up names the ticket that would own one.

**Owner.** SharpeBench paper/evidence owner

**Follow-up.** P15: commit a realism report artifact beside the datasets so the table's verdicts have a stored producer output.

**Status history.**

| Date | Disposition | Rationale |
|---|---|---|
| 2026-09-23 | needs-rescore | Initial entry: The recorded inputs are sufficient: the battery is a deterministic function of the committed CSVs and the command exists on the current tree, so a current-method verdict is a recomputation rather than a new experiment. P00-E maps evidence and authorizes no rerun. The follow-up names the ticket that would own one. |

### `SB-tab-datasheet`

**Claim.** Provenance of the nine frozen datasets: upstream source, byte count, data rows and freeze date, totalling 6,455,868 bytes and 198,834 rows.

**Source.** `paper/sections/D-datasheet.tex:18`

**Labels.** `tab:datasheet`

**Artifacts.**
- `data/us-indices-1d.csv`, sha256 `a153e1d7fb306668509d7d47203b142dc4409d33a8a3ef9e6b9a7f5f88e67ccd`, digest source: paper/evidence/provenance.json
- `data/us-indices-1w.csv`, sha256 `160cef2617097f3b4ebd3655b4be468efde5bcc3a6df9858f7d2b699b7affdf8`, digest source: paper/evidence/provenance.json
- `data/crypto-majors-1h.csv`, sha256 `869f570767b04b520cc95788f17ab2e244048a1bb850128a5abe6520630e8de0`, digest source: paper/evidence/provenance.json
- `data/crypto-majors-4h.csv`, sha256 `bc2728a9409b0e2b66795c81990d670cbbbb332065997ff933d5d2d7f326d2e6`, digest source: paper/evidence/provenance.json
- `data/crypto-majors-1d.csv`, sha256 `1854e76e9811387dd1ff3db64308eceb613ad05c9eee0011762eddc48f538a26`, digest source: paper/evidence/provenance.json
- `data/crypto-majors-1w.csv`, sha256 `506524623af472c9ca314e2efca12a0584a44d52826a4ea6509933fe05519214`, digest source: paper/evidence/provenance.json
- `data/fx-majors-1d.csv`, sha256 `eb7267ce8135662ab48407c8c33c8bfd901546063e4acd8c5f1c18163cb09e99`, digest source: paper/evidence/provenance.json
- `data/commodities-1d.csv`, sha256 `e174f30a79b2d5520124bbd22791b0352fa498c310d218c764993ce6902f9cd5`, digest source: paper/evidence/provenance.json
- `data/rates-1d.csv`, sha256 `2e375b15bf334e3cc604d09a518d97a1cc141d91047c7df641009498e80ed24e`, digest source: paper/evidence/provenance.json

**Producer command.** python scripts/data/fetch_crypto.py --interval 1h --check; python scripts/data/fetch_fred.py fx --check; python scripts/data/derive_weekly.py --check (paper/sections/A-commands.tex)

**Producing commit.** Freeze dates are recorded per row (2026-06-22 and 2026-08-23); the producing commit is not recorded.

**Effective configuration.** Each fetch script pins its own date window; every file carries a .sha256 sidecar.

**Missing provenance.**
- none identified

**Relevant later changes.**
- None; the data files are frozen and their digests are in the provenance manifest.

**Present applicability.** Verified on 2026-09-23 for this register: every one of the nine byte counts and row counts in the table matches the file on disk, and every digest matches paper/evidence/provenance.json.

**Disposition.** `still-applicable`. The comparison is recorded rather than inferred: all nine byte and row counts were recomputed from the committed CSVs and agree with the table exactly.

**Owner.** SharpeBench paper/evidence owner

**Follow-up.** none

**Status history.**

| Date | Disposition | Rationale |
|---|---|---|
| 2026-09-23 | still-applicable | Initial entry: The comparison is recorded rather than inferred: all nine byte and row counts were recomputed from the committed CSVs and agree with the table exactly. |

### `SB-tab-related`

**Claim.** Per-cell comparison of six rival trading-agent boards against SharpeBench on gate axes and on rival axes.

**Source.** `paper/sections/06-related.tex:19`, `paper/sections/06-related.tex:44`

**Labels.** `tab:related`, `tab:related-inverse`

**Artifacts.**
- `paper/evidence/table-provenance.md`, sha256 `a57c4defcc16b664ea0aac1e5d98e3f8cbf2e2cadd889678e68b206b55cc593f`, digest source: computed for this register; the file is outside the provenance manifest scope, records the source of every mark and distinguishes exhaustively searched blanks from abstract-only checks

**Producer command.** none; the marks are read from the cited papers and their public boards, recorded per cell in the provenance sheet.

**Producing commit.** not applicable; the sources are external publications.

**Effective configuration.** Checked 2026-08-23, amended 2026-08-24 and 2026-09-17 (paper/evidence/table-provenance.md).

**Missing provenance.**
- Several blanks are recorded as "unchecked" rather than as exhaustive searches, which the sheet states per cell.

**Relevant later changes.**
- None inside this repository; a cited paper could publish a new version, which the sheet's dated checks would then lag.

**Present applicability.** Applicable as of the recorded check dates. A blank means only that the audit found no positive documentation, which the sheet states as its protocol.

**Disposition.** `still-applicable`. The documented comparison is the per-cell provenance sheet with its dated checks; nothing in this repository can move these cells.

**Owner.** SharpeBench paper/evidence owner

**Follow-up.** P15: recheck the dated cells before submission, since the marks track external publications.

**Status history.**

| Date | Disposition | Rationale |
|---|---|---|
| 2026-09-23 | still-applicable | Initial entry: The documented comparison is the per-cell provenance sheet with its dated checks; nothing in this repository can move these cells. |

### `SB-fig-demotion`

**Claim.** On the shipped three-agent demonstration field the agent with the highest raw return is refused and holds no rank.

**Source.** `paper/sections/05-experiments.tex:20`

**Labels.** `fig:demotion`

**Artifacts.**
- `paper/figures/sharpebench-luck-demotion.pdf`, sha256 `1e025ac30099de23ae8455f20ae5441b557e924870d0e1475c7508512908ec0d`, digest source: paper/evidence/provenance.json
- `crates/sharpebench-core/golden/example_submissions.scores.json`, sha256 `00cc54e484c5193e424d46c649d05378a5f5eb3bafc8d5a42a880dac8f7e54f6`, digest source: computed for this register; the file is outside the provenance manifest scope, the golden score record the panel is read from
- `suites/example_submissions.json`, sha256 `909775dd86084682cfee7b723b64ce67529fe9b1111a5054f1ef6c8437c4ba96`, digest source: computed for this register; the file is outside the provenance manifest scope, the field the golden record scores

**Producer command.** sharpebench score suites/example_submissions.json; the figure is drawn by python paper/src/make-figures.py (paper/sections/A-commands.tex)

**Producing commit.** current engineering tree; the exact commit is not recorded in the manuscript. The golden record is regenerated with the kernel and pinned by cargo test -p sharpebench-core --test golden_scores.

**Effective configuration.** Synthetic returns; fixed golden values with no sampling spread (paper/sections/05-experiments.tex:27).

**Missing provenance.**
- none identified

**Relevant later changes.**
- The golden fields changed in the Pareto flag of hold and later in calibration counts, Brier scores and confidence-weighted return; no gate reads those fields (paper/sections/E-repairs.tex:19).
- paper/sections/E-repairs.tex:17 records that the two committed golden fields hold four and three agents and take the configured path, so the dispersion-vote repair does not move them, and that sharpebench score --json on the example submissions is byte-identical.

**Present applicability.** Applicable to the current kernel: the golden record is the current tree's and the repairs that touched it are recorded as leaving the gate outcomes the panel plots unchanged.

**Disposition.** `still-applicable`. The documented comparison is paper/sections/E-repairs.tex:17 together with the golden_scores test, which pins the record the panel reads to the current kernel.

**Owner.** SharpeBench paper/evidence owner

**Follow-up.** none

**Status history.**

| Date | Disposition | Rationale |
|---|---|---|
| 2026-09-23 | still-applicable | Initial entry: The documented comparison is paper/sections/E-repairs.tex:17 together with the golden_scores test, which pins the record the panel reads to the current kernel. |

### `SB-fig-deflation`

**Claim.** One fixed synthetic track, per-period Sharpe 0.85 over 150 periods with a cross-trial dispersion of 0.30 per period, crosses the 0.95 eligibility bar between 32 and 64 trials and collapses past a few hundred.

**Source.** `paper/sections/05-experiments.tex:28`

**Labels.** `fig:deflation`

**Artifacts.**
- `paper/figures/sharpebench-deflation-curve.pdf`, sha256 `3a17ec9552773904e079fdfe933e06e35b791be345b49651e70fe255a2832579`, digest source: paper/evidence/provenance.json
- `paper/src/kernel_stats.py`, sha256 `836a71dc2411869de43960cb85cf8ad000762d7225053092da112973c8539191`, digest source: paper/evidence/provenance.json, the Python transcription of the kernel's formulas that draws the panel

**Producer command.** python paper/src/make-figures.py (paper/sections/A-commands.tex)

**Producing commit.** current engineering tree; the exact commit is not recorded in the manuscript.

**Effective configuration.** Deterministic function of its stated inputs; no sampling spread (paper/sections/05-experiments.tex:27).

**Missing provenance.**
- none identified

**Relevant later changes.**
- The standardized-moment repair changes how moments are estimated from a sample; this panel is computed from stated moments rather than an estimated sample.

**Present applicability.** Verified on 2026-09-23 for this register: python -B -m unittest paper/src/test_kernel_stats.py passes on the current tree, which pins the producer to the kernel's golden PSRs and to the worked example of Bailey and Lopez de Prado.

**Disposition.** `still-applicable`. The producer agrees with the current kernel under a passing pinned test, and the panel's inputs are stated constants rather than frozen measurements.

**Owner.** SharpeBench paper/evidence owner

**Follow-up.** none

**Status history.**

| Date | Disposition | Rationale |
|---|---|---|
| 2026-09-23 | still-applicable | Initial entry: The producer agrees with the current kernel under a passing pinned test, and the panel's inputs are stated constants rather than frozen measurements. |

### `SB-tab-units`

**Claim.** The annualized Sharpe an agent had to exceed at N = 50 under the v0.2.1 unit error: 18.1 on daily bars and 106.5 on hourly bars at the shipped 0.5 prior.

**Source.** `paper/sections/05-experiments.tex:45`

**Labels.** `tab:units`

**Artifacts.**
- `paper/evidence/FINDING-units.md`, sha256 `13c1a0d72d74a18fc04dc28d6f4df80b276cafebc6b797bb1c038468de6a2f07`, digest source: computed for this register; the file is outside the provenance manifest scope, the frozen record of the finding; it reproduces the table's four rows verbatim and names its own source

**Producer command.** cargo run --release -p sharpebench-harness --example evidence_sweep -- out.jsonl [dataset] [dsr_bar], assembled by paper/evidence/assemble_sweep.py and reduced by paper/evidence/analyze.py (paper/sections/A-commands.tex), run against the v0.2.1 kernel.

**Producing commit.** v0.2.1, deliberately: the caption states the table documents that kernel's error. The frozen note records the producing commit as d0ae4f2.

**Effective configuration.** N = 50, per-prior per-period bars annualized by each column's periods per year.

**Missing provenance.**
- The note names its source as paper/evidence/sweep.jsonl, 1,024 records over us-indices-1d and us-indices-1w. That file is not in the tree: paper/evidence holds no top-level .jsonl, and paper/evidence/baseline-v0.2.1/ has no us-indices-1d file. The table therefore cannot be recomputed from committed records.
- paper/evidence/baseline-v0.2.1/ and paper/evidence/FINDING-units.md are outside the artifact scope of paper/evidence/provenance.json, so they carry no recorded digest.
- paper/evidence/FINDING-units.md still carries the superseded attribution of the 0.5 prior to the cited worked example, which paper/sections/05-experiments.tex:36 corrects.

**Relevant later changes.**
- The units fix shipped in v0.3.0 (paper/sections/A-commands.tex); the table is retained as the record of the error, not as current behaviour.

**Present applicability.** Historical by construction. It describes a kernel two minor versions before the frozen evidence and no current behaviour.

**Disposition.** `historical-only`. The frozen note is the located artifact: it carries the table verbatim and a producing commit, so the claim is a valid record of the v0.2.1 kernel, which the caption binds it to. The upstream sweep it reduces is gone, which the missing-provenance field records rather than the disposition hiding.

**Owner.** SharpeBench paper/evidence owner

**Follow-up.** P15: bring paper/evidence/baseline-v0.2.1/ and the FINDING records into a recorded digest scope so the historical artifacts are identified rather than merely present.

**Status history.**

| Date | Disposition | Rationale |
|---|---|---|
| 2026-09-23 | historical-only | Initial entry: The frozen note is the located artifact: it carries the table verbatim and a producing commit, so the claim is a valid record of the v0.2.1 kernel, which the caption binds it to. The upstream sweep it reduces is gone, which the missing-provenance field records rather than the disposition hiding. |

### `SB-sec-units-prefloor`

**Claim.** The pre-floor deflated-Sharpe pair of 0.984 weekly and 0.000 daily for the same asset, quoted as the observation that localized the refusal to the deflation threshold.

**Source.** `paper/sections/05-experiments.tex:33`

**Artifacts.**
- `paper/evidence/after-v0.3.0/us-indices-1w.jsonl`, sha256 `8aa2d151b33085df656342fe9cb299d72d762a7b92a534bb0e2f4b5967394887`, digest source: computed for this register; the file is outside the provenance manifest scope, the file paper/sections/A-commands.tex names as the source of the weekly figure
- `paper/evidence/after-v0.3.0/us-indices-1d.jsonl`, sha256 `8cdf5feacbe7465d110ea3559d8d0febe5e5dd1ca2680ed02def19616ddcc120`, digest source: computed for this register; the file is outside the provenance manifest scope, the file it names as the source of the daily figure
- `paper/evidence/FINDING-units.md`, sha256 `13c1a0d72d74a18fc04dc28d6f4df80b276cafebc6b797bb1c038468de6a2f07`, digest source: computed for this register; the file is outside the provenance manifest scope, carries the same 0.984 and 0.000 pair, attributed instead to the v0.2.1 sweep

**Producer command.** cargo run --release -p sharpebench-harness --example evidence_sweep -- out.jsonl [dataset] [dsr_bar], assembled by paper/evidence/assemble_sweep.py and reduced by paper/evidence/analyze.py (paper/sections/A-commands.tex), run against an intermediate kernel; paper/sections/A-commands.tex states the pair is read from paper/evidence/after-v0.3.0/.

**Producing commit.** unknown for the after-v0.3.0 artifacts. The competing source, paper/evidence/FINDING-units.md, records commit d0ae4f2 at v0.2.1.

**Effective configuration.** Pre-floor dispersion handling; otherwise the default grid.

**Missing provenance.**
- Checked for this register on 2026-09-23: across all 64 buy-and-hold records in each of the two after-v0.3.0 files, no record carries a deflated Sharpe of 0.984. At the default cell the two files give 0.0046 and 0.0021; the maximum buy-and-hold value in each file is 1.0000, at a bar of 0.80 and a host N of 1.
- The two sources disagree on the pair's origin: the appendix names after-v0.3.0, while paper/evidence/FINDING-units.md attributes the same pair to the v0.2.1 sweep and adds a status block stating the contrast came from the small field-measured path and must not be attributed to the configured-prior unit bug.
- The v0.2.1 sweep the note names, paper/evidence/sweep.jsonl, is not in the tree.
- paper/evidence/after-v0.3.0/ is outside the artifact scope of paper/evidence/provenance.json and carries no recorded digest.

**Relevant later changes.**
- The dispersion floor shipped after these artifacts, which is why the appendix states they are not used for any current result.

**Present applicability.** Not established. The register could not reproduce the quoted pair from the artifact the appendix names, and the other artifact carrying it warns against the reading the surrounding prose gives it.

**Disposition.** `unresolved`. The artifact behind the quoted pair could not be located: the named file does not contain it at any cell checked, and the file that does contain it is a superseded note that disclaims the attribution. Recorded as unresolved rather than resolved by picking one of the two.

**Owner.** SharpeBench paper/evidence owner

**Follow-up.** P15: identify which artifact the 0.984 and 0.000 pair is read from and reconcile the appendix with the frozen note, or drop the pair.

**Status history.**

| Date | Disposition | Rationale |
|---|---|---|
| 2026-09-23 | unresolved | Initial entry: The artifact behind the quoted pair could not be located: the named file does not contain it at any cell checked, and the file that does contain it is a superseded note that disclaims the attribution. Recorded as unresolved rather than resolved by picking one of the two. |

### `SB-sec-units-worked-example`

**Claim.** The kernel reproduces the cited worked example: a threshold of 0.1132 per period and a deflated Sharpe of 0.9004 at N = 100, 0.9505 at N = 46, and 0.9505 for normal returns at N = 88.

**Source.** `paper/sections/05-experiments.tex:36`

**Artifacts.**
- `crates/sharpebench-stats/src/deflated_sharpe.rs`, sha256 `d6977b8faabac85311892dfab2509d8203514fd79293466b19cd73cd0281456f`, digest source: paper/evidence/provenance.json, carries the worked-example regression test

**Producer command.** cargo test -p sharpebench-stats worked_example (paper/sections/A-commands.tex)

**Producing commit.** current engineering tree; the exact commit is not recorded in the manuscript.

**Effective configuration.** Constructed return series with the example's moments: N = 100, cross-trial variance 1/2 annualized, T = 1250, skewness -3, kurtosis 10, annualized Sharpe 2.5 at 250 periods a year.

**Missing provenance.**
- No output artifact is committed; the evidence is the regression test.

**Relevant later changes.**
- The standardized-moment repair changed the estimator convention; the test runs on the current tree and reproduces the printed values through the kernel's own entry points.

**Present applicability.** Applicable to the current tree, since the test executes against it.

**Disposition.** `still-applicable`. The documented comparison is the commit-time regression test against the published example.

**Owner.** SharpeBench paper/evidence owner

**Follow-up.** none

**Status history.**

| Date | Disposition | Rationale |
|---|---|---|
| 2026-09-23 | still-applicable | Initial entry: The documented comparison is the commit-time regression test against the published example. |

### `SB-tab-eligibility`

**Claim.** Default configuration on all nine datasets: the leading agent's deflated Sharpe, worst drawdown, bootstrap p, dispersion source and annualized bar, with no dataset eligible.

**Source.** `paper/sections/05-experiments.tex:77`

**Labels.** `tab:eligibility`

**Artifacts.**
- `paper/evidence/final/us-indices-1d.jsonl`, sha256 `d8aeabb1b69e9f9345ec04fea82dd6391652b7ce5e7f8e9a4f9487cf469d4324`, digest source: paper/evidence/provenance.json
- `paper/evidence/final/us-indices-1w.jsonl`, sha256 `aa4d8d46ff69a464c64c2080b1ef4c0eaa1403a02e57daac8a1a47de809c105f`, digest source: paper/evidence/provenance.json
- `paper/evidence/final/crypto-majors-1h.jsonl`, sha256 `0b6c74f29540682efec1aef7d860f0638535903bc4416000793e23d8fac2a8a9`, digest source: paper/evidence/provenance.json
- `paper/evidence/final/crypto-majors-4h.jsonl`, sha256 `be6c3abbd6189f8372693b9370148850b99dbc02fa9a1b017f05db894b13cb85`, digest source: paper/evidence/provenance.json
- `paper/evidence/final/crypto-majors-1d.jsonl`, sha256 `5d522ba86c67d11903acb4187950c706e79ebd6a044d84d784bf0d90f74e9258`, digest source: paper/evidence/provenance.json
- `paper/evidence/final/crypto-majors-1w.jsonl`, sha256 `78ac6179bec6f2a7b474c7a05ab35f827fa5db0a5aa9f980083e54961eaa697e`, digest source: paper/evidence/provenance.json
- `paper/evidence/final/fx-majors-1d.jsonl`, sha256 `dc5a219e44a2cc1e4ca5e19f5de4363cb8226aab300dc47d61c780f4e6333337`, digest source: paper/evidence/provenance.json
- `paper/evidence/final/commodities-1d.jsonl`, sha256 `c2e389e99ebe6baa408f415dcb7e2da0c90672933054903e339fc4e05349500d`, digest source: paper/evidence/provenance.json
- `paper/evidence/final/rates-1d.jsonl`, sha256 `34018ad8511d509fc086dc711bb0acb704a192a41e7979a0d9329165e4bedd65`, digest source: paper/evidence/provenance.json

**Producer command.** cargo run --release -p sharpebench-harness --example evidence_sweep -- out.jsonl [dataset] [dsr_bar], assembled by paper/evidence/assemble_sweep.py and reduced by paper/evidence/analyze.py (paper/sections/A-commands.tex)

**Producing commit.** v0.9.0 evidence snapshot. The commit that cut v0.9.0 is not recorded and stays unknown. paper/evidence/provenance.json records the commit at which the manifest itself was generated, which is rebound whenever a file in its source scope moves and is not the source state these numbers were produced from; the manifest's digest of each artifact below is the identity that does hold.

**Effective configuration.** Default cell beta = 0.95, host N = 50, field size eight; eight execution seeds, six disjoint windows, typical cost profile; 512 records per dataset over 64 host configurations, 4,608 in all.

**Missing provenance.**
- The frozen records carry no DSR interval, so the table prints point estimates only (paper/sections/05-experiments.tex:68).

**Relevant later changes.**
- Standardized-moment estimators mixed finite-sample normalizations, so every probabilistic and deflated Sharpe here was computed under the superseded convention, and how far the correction moves the frozen values is not established (paper/sections/E-repairs.tex:5).
- The simulator let a zero target cross through flat into a short position and the momentum entrant ignored its declared lookback, so the current engine reconstructs different trajectories (paper/sections/E-repairs.tex:5).
- A constant observed track is now refused for PSR and DSR, so the current kernel would refuse all 576 frozen hold records as unavailable rather than scoring them (paper/sections/E-repairs.tex:9).
- A track with no Sharpe ratio no longer votes on another agent's deflation bar. hold voted wherever a record stamps measured or floored: 64 of the 576 default-sweep panels, all 27 panels of the externally specified sweep and four of the nine power-curve panels. How far the repaired measurement would move each bar is not established and is not estimated (paper/sections/E-repairs.tex:15, :17).

**Present applicability.** Valid as the v0.9.0 record. It does not establish what the current engine would compute: three repairs move quantities this table prints, and the manuscript states their effect is not established.

**Disposition.** `historical-only`. The artifact validly records the frozen kernel's run and the manuscript binds the table to that snapshot. The separate question of what the current engine would compute is carried by SB-sec-passk.

**Owner.** SharpeBench paper/evidence owner

**Follow-up.** none

**Status history.**

| Date | Disposition | Rationale |
|---|---|---|
| 2026-09-23 | historical-only | Initial entry: The artifact validly records the frozen kernel's run and the manuscript binds the table to that snapshot. The separate question of what the current engine would compute is carried by SB-sec-passk. |

### `SB-sec-passk`

**Claim.** Under the shipped defaults no agent is eligible in any of the 72 agent-dataset verdicts; 63 are refused by a substantive gate and the other nine are the never-trading control, and deflation and regime robustness refuse independently.

**Source.** `paper/sections/05-experiments.tex:61`, `paper/sections/05-experiments.tex:66`

**Artifacts.**
- `paper/evidence/final/us-indices-1d.jsonl`, sha256 `d8aeabb1b69e9f9345ec04fea82dd6391652b7ce5e7f8e9a4f9487cf469d4324`, digest source: paper/evidence/provenance.json
- `paper/evidence/final/us-indices-1w.jsonl`, sha256 `aa4d8d46ff69a464c64c2080b1ef4c0eaa1403a02e57daac8a1a47de809c105f`, digest source: paper/evidence/provenance.json
- `paper/evidence/final/crypto-majors-1h.jsonl`, sha256 `0b6c74f29540682efec1aef7d860f0638535903bc4416000793e23d8fac2a8a9`, digest source: paper/evidence/provenance.json
- `paper/evidence/final/crypto-majors-4h.jsonl`, sha256 `be6c3abbd6189f8372693b9370148850b99dbc02fa9a1b017f05db894b13cb85`, digest source: paper/evidence/provenance.json
- `paper/evidence/final/crypto-majors-1d.jsonl`, sha256 `5d522ba86c67d11903acb4187950c706e79ebd6a044d84d784bf0d90f74e9258`, digest source: paper/evidence/provenance.json
- `paper/evidence/final/crypto-majors-1w.jsonl`, sha256 `78ac6179bec6f2a7b474c7a05ab35f827fa5db0a5aa9f980083e54961eaa697e`, digest source: paper/evidence/provenance.json
- `paper/evidence/final/fx-majors-1d.jsonl`, sha256 `dc5a219e44a2cc1e4ca5e19f5de4363cb8226aab300dc47d61c780f4e6333337`, digest source: paper/evidence/provenance.json
- `paper/evidence/final/commodities-1d.jsonl`, sha256 `c2e389e99ebe6baa408f415dcb7e2da0c90672933054903e339fc4e05349500d`, digest source: paper/evidence/provenance.json
- `paper/evidence/final/rates-1d.jsonl`, sha256 `34018ad8511d509fc086dc711bb0acb704a192a41e7979a0d9329165e4bedd65`, digest source: paper/evidence/provenance.json

**Producer command.** cargo run --release -p sharpebench-harness --example evidence_sweep -- out.jsonl [dataset] [dsr_bar], assembled by paper/evidence/assemble_sweep.py and reduced by paper/evidence/analyze.py (paper/sections/A-commands.tex)

**Producing commit.** v0.9.0 evidence snapshot. The commit that cut v0.9.0 is not recorded and stays unknown. paper/evidence/provenance.json records the commit at which the manifest itself was generated, which is rebound whenever a file in its source scope moves and is not the source state these numbers were produced from; the manifest's digest of each artifact below is the identity that does hold.

**Effective configuration.** Default cell beta = 0.95, host N = 50, field size eight; eight execution seeds, six disjoint windows, typical cost profile; 512 records per dataset over 64 host configurations, 4,608 in all.

**Missing provenance.**
- The records do not serialize each agent's pooled Sharpe, so a repaired dispersion measurement cannot be recomputed from them.
- No DSR interval is recorded, so a difference between two printed deflated Sharpes is not by itself evidence of a difference.

**Relevant later changes.**
- Standardized-moment estimators mixed finite-sample normalizations, so every probabilistic and deflated Sharpe here was computed under the superseded convention, and how far the correction moves the frozen values is not established (paper/sections/E-repairs.tex:5).
- The simulator let a zero target cross through flat into a short position and the momentum entrant ignored its declared lookback, so the current engine reconstructs different trajectories (paper/sections/E-repairs.tex:5).
- A constant observed track is now refused for PSR and DSR, so the current kernel would refuse all 576 frozen hold records as unavailable rather than scoring them (paper/sections/E-repairs.tex:9).
- A track with no Sharpe ratio no longer votes on another agent's deflation bar. hold voted wherever a record stamps measured or floored: 64 of the 576 default-sweep panels, all 27 panels of the externally specified sweep and four of the nine power-curve panels. How far the repaired measurement would move each bar is not established and is not estimated (paper/sections/E-repairs.tex:15, :17).
- The committed records serialize each agent's deflated and probabilistic Sharpe but not its pooled Sharpe, and the probabilistic Sharpe cannot be inverted without that track's skewness and kurtosis, so the frozen artifacts lack the inputs a repaired dispersion measurement would need (paper/sections/E-repairs.tex:17).

**Present applicability.** The refusal is established for the frozen snapshot. As a statement about the shipped defaults today it is not established: the moment repair moves every DSR by an unquantified amount, the dispersion-vote repair moves the bar on four of the nine panels, and the simulator repair changes the trajectories the field is scored on.

**Disposition.** `needs-new-experiment`. A rescore is not defensible: generation changed (simulator and entrant repairs) and the inputs a repaired dispersion measurement needs are absent from the committed records. Re-running the field would be a new empirical run rather than a correction, which paper/sections/E-repairs.tex:7 states in those terms. P00-E maps evidence and authorizes no rerun. The follow-up names the ticket that would own one.

**Owner.** SharpeBench paper/evidence owner

**Follow-up.** P12-A then P13-A/P14-A: a shipped-method joint-gate producer run on the current engine, reported as a new experiment and never as a refresh of the frozen numbers.

**Status history.**

| Date | Disposition | Rationale |
|---|---|---|
| 2026-09-23 | needs-new-experiment | Initial entry: A rescore is not defensible: generation changed (simulator and entrant repairs) and the inputs a repaired dispersion measurement needs are absent from the committed records. Re-running the field would be a new empirical run rather than a correction, which paper/sections/E-repairs.tex:7 states in those terms. P00-E maps evidence and authorizes no rerun. The follow-up names the ticket that would own one. |

### `SB-sec-ablation`

**Claim.** The never-catastrophic preset admits nobody either: it returns no on all nine rows, seven of nine breach the 20 percent drawdown bound, daily-FX buy-and-hold at 0.199 is the single exception in the grid, and hourly momentum loses 99.3 percent in one window.

**Source.** `paper/sections/05-experiments.tex:99`, `paper/sections/05-experiments.tex:100`

**Artifacts.**
- `paper/evidence/final/us-indices-1d.jsonl`, sha256 `d8aeabb1b69e9f9345ec04fea82dd6391652b7ce5e7f8e9a4f9487cf469d4324`, digest source: paper/evidence/provenance.json
- `paper/evidence/final/us-indices-1w.jsonl`, sha256 `aa4d8d46ff69a464c64c2080b1ef4c0eaa1403a02e57daac8a1a47de809c105f`, digest source: paper/evidence/provenance.json
- `paper/evidence/final/crypto-majors-1h.jsonl`, sha256 `0b6c74f29540682efec1aef7d860f0638535903bc4416000793e23d8fac2a8a9`, digest source: paper/evidence/provenance.json
- `paper/evidence/final/crypto-majors-4h.jsonl`, sha256 `be6c3abbd6189f8372693b9370148850b99dbc02fa9a1b017f05db894b13cb85`, digest source: paper/evidence/provenance.json
- `paper/evidence/final/crypto-majors-1d.jsonl`, sha256 `5d522ba86c67d11903acb4187950c706e79ebd6a044d84d784bf0d90f74e9258`, digest source: paper/evidence/provenance.json
- `paper/evidence/final/crypto-majors-1w.jsonl`, sha256 `78ac6179bec6f2a7b474c7a05ab35f827fa5db0a5aa9f980083e54961eaa697e`, digest source: paper/evidence/provenance.json
- `paper/evidence/final/fx-majors-1d.jsonl`, sha256 `dc5a219e44a2cc1e4ca5e19f5de4363cb8226aab300dc47d61c780f4e6333337`, digest source: paper/evidence/provenance.json
- `paper/evidence/final/commodities-1d.jsonl`, sha256 `c2e389e99ebe6baa408f415dcb7e2da0c90672933054903e339fc4e05349500d`, digest source: paper/evidence/provenance.json
- `paper/evidence/final/rates-1d.jsonl`, sha256 `34018ad8511d509fc086dc711bb0acb704a192a41e7979a0d9329165e4bedd65`, digest source: paper/evidence/provenance.json

**Producer command.** cargo run --release -p sharpebench-harness --example evidence_sweep -- out.jsonl [dataset] [dsr_bar], assembled by paper/evidence/assemble_sweep.py and reduced by paper/evidence/analyze.py (paper/sections/A-commands.tex)

**Producing commit.** v0.9.0 evidence snapshot. The commit that cut v0.9.0 is not recorded and stays unknown. paper/evidence/provenance.json records the commit at which the manifest itself was generated, which is rebound whenever a file in its source scope moves and is not the source state these numbers were produced from; the manifest's digest of each artifact below is the identity that does hold.

**Effective configuration.** Default cell beta = 0.95, host N = 50, field size eight; eight execution seeds, six disjoint windows, typical cost profile; 512 records per dataset over 64 host configurations, 4,608 in all. Preset: pass mode any-run with a per-run drawdown bound of 20 percent, edge tested on the pooled track.

**Missing provenance.**
- none identified

**Relevant later changes.**
- Standardized-moment estimators mixed finite-sample normalizations, so every probabilistic and deflated Sharpe here was computed under the superseded convention, and how far the correction moves the frozen values is not established (paper/sections/E-repairs.tex:5).
- The simulator let a zero target cross through flat into a short position and the momentum entrant ignored its declared lookback, so the current engine reconstructs different trajectories (paper/sections/E-repairs.tex:5).
- A constant observed track is now refused for PSR and DSR, so the current kernel would refuse all 576 frozen hold records as unavailable rather than scoring them (paper/sections/E-repairs.tex:9).

**Present applicability.** Valid for the frozen snapshot. Drawdowns are path quantities that the simulator repair can move, and the statistical legs carry the moment repair, so the current engine's verdicts are not established.

**Disposition.** `historical-only`. The artifact validly records the v0.9.0 ablation. The drawdown figures are read from the same frozen records and the manuscript scopes them to that run; the current-engine question is carried by SB-sec-passk rather than duplicated here.

**Owner.** SharpeBench paper/evidence owner

**Follow-up.** P12-A: include the never-catastrophic preset in the joint-gate producer run so the ablation is re-established on the current engine.

**Status history.**

| Date | Disposition | Rationale |
|---|---|---|
| 2026-09-23 | historical-only | Initial entry: The artifact validly records the v0.9.0 ablation. The drawdown figures are read from the same frozen records and the manuscript scopes them to that run; the current-engine question is carried by SB-sec-passk rather than duplicated here. |

### `SB-sec-riskmanaged`

**Claim.** The risk-managed control is refused by deflation, the bootstrap and the applicable reliability verdict at every recorded effective-N setting, has the smallest worst-window drawdown of any trading agent on seven of nine datasets, and is whipsawed to 87 percent on hourly crypto and 33 percent on daily FX.

**Source.** `paper/sections/05-experiments.tex:107`, `paper/sections/05-experiments.tex:110`

**Artifacts.**
- `paper/evidence/final/risk-managed.jsonl`, sha256 `9909246e803bf0014df61b113f7a3ff22a304d89f5252237611b2b9b7becfb99`, digest source: paper/evidence/provenance.json
- `paper/evidence/final/risk-managed.log`, sha256 `2a5df5ef27ea41b7dc4213de6d055d303784365940261dc6cb53d15234094822`, digest source: computed for this register; the file is outside the provenance manifest scope

**Producer command.** cargo run --release -p sharpebench-harness --example risk_managed_eval -- out.jsonl (paper/sections/A-commands.tex)

**Producing commit.** v0.9.0 evidence snapshot. The commit that cut v0.9.0 is not recorded and stays unknown. paper/evidence/provenance.json records the commit at which the manifest itself was generated, which is rebound whenever a file in its source scope moves and is not the source state these numbers were produced from; the manifest's digest of each artifact below is the identity that does hold.

**Effective configuration.** Risk-managed agent beside buy-and-hold and the luck floor on all nine datasets at documented defaults, both reliability verdicts per row, with n_sensitivity records for weekly US indices at N in {7, 10, 25, 50, 100}.

**Missing provenance.**
- none identified

**Relevant later changes.**
- Standardized-moment estimators mixed finite-sample normalizations, so every probabilistic and deflated Sharpe here was computed under the superseded convention, and how far the correction moves the frozen values is not established (paper/sections/E-repairs.tex:5).
- The simulator let a zero target cross through flat into a short position and the momentum entrant ignored its declared lookback, so the current engine reconstructs different trajectories (paper/sections/E-repairs.tex:5).
- paper/sections/E-repairs.tex:17 records that the risk-managed gate panels are the exception among the measured records: their seven-agent field contains no hold and no other constant track, so their dispersion stamps stand unchanged.

**Present applicability.** Valid for the frozen snapshot, and the one measured family the dispersion-vote repair leaves alone. The moment repair still moves its statistical legs by an unquantified amount.

**Disposition.** `historical-only`. The artifact validly records the v0.9.0 run and the manuscript binds it to that snapshot; the appendix establishes only that the dispersion-vote repair does not touch these panels, not that the statistics are unchanged.

**Owner.** SharpeBench paper/evidence owner

**Follow-up.** none

**Status history.**

| Date | Disposition | Rationale |
|---|---|---|
| 2026-09-23 | historical-only | Initial entry: The artifact validly records the v0.9.0 run and the manuscript binds it to that snapshot; the appendix establishes only that the dispersion-vote repair does not touch these panels, not that the statistics are unchanged. |

### `SB-fig-drawdowns`

**Claim.** The risk-managed agent has the smallest worst-window drawdown of any trading agent on seven of nine datasets.

**Source.** `paper/sections/05-experiments.tex:124`

**Labels.** `fig:drawdowns`

**Artifacts.**
- `paper/figures/evidence-drawdowns.pdf`, sha256 `bab818feea7dfdcb513394b1e99f216f7c51eb604afc60830fffea45114ed246`, digest source: paper/evidence/provenance.json
- `paper/evidence/final/risk-managed.jsonl`, sha256 `9909246e803bf0014df61b113f7a3ff22a304d89f5252237611b2b9b7becfb99`, digest source: paper/evidence/provenance.json

**Producer command.** python paper/src/make-evidence-figures.py (paper/sections/A-commands.tex)

**Producing commit.** v0.9.0 evidence snapshot. The commit that cut v0.9.0 is not recorded and stays unknown. paper/evidence/provenance.json records the commit at which the manifest itself was generated, which is rebound whenever a file in its source scope moves and is not the source state these numbers were produced from; the manifest's digest of each artifact below is the identity that does hold. The figure script and its test run on the current tree.

**Effective configuration.** Maximum over all windows and seeds of a panel; the records store only that maximum, so no spread is shown.

**Missing provenance.**
- none identified

**Relevant later changes.**
- The simulator let a zero target cross through flat into a short position and the momentum entrant ignored its declared lookback, so the current engine reconstructs different trajectories (paper/sections/E-repairs.tex:5).

**Present applicability.** Verified on 2026-09-23 for this register: python -B -m unittest paper/src/test_evidence_figures.py passes, so the committed figure agrees with the committed records. The records themselves remain the frozen snapshot.

**Disposition.** `historical-only`. Figure-to-record agreement is checked on the current tree, but the records are v0.9.0 and the simulator repair can move drawdown paths, so the figure inherits its records' historical scope.

**Owner.** SharpeBench paper/evidence owner

**Follow-up.** none

**Status history.**

| Date | Disposition | Rationale |
|---|---|---|
| 2026-09-23 | historical-only | Initial entry: Figure-to-record agreement is checked on the current tree, but the records are v0.9.0 and the simulator repair can move drawdown paths, so the figure inherits its records' historical scope. |

### `SB-fig-luckdeflation`

**Claim.** The best zero-skill agent's deflated Sharpe stays far below the bar at every effective trial count on weekly crypto, daily rates and weekly US indices.

**Source.** `paper/sections/05-experiments.tex:132`

**Labels.** `fig:luckdeflation`

**Artifacts.**
- `paper/figures/evidence-luck-deflation.pdf`, sha256 `a1cb815ce73b22eda9bcd8910a4997e6b3e9483edee8dec754b0b6d607a807a0`, digest source: paper/evidence/provenance.json
- `paper/evidence/final/crypto-majors-1w.jsonl`, sha256 `78ac6179bec6f2a7b474c7a05ab35f827fa5db0a5aa9f980083e54961eaa697e`, digest source: paper/evidence/provenance.json
- `paper/evidence/final/rates-1d.jsonl`, sha256 `34018ad8511d509fc086dc711bb0acb704a192a41e7979a0d9329165e4bedd65`, digest source: paper/evidence/provenance.json
- `paper/evidence/final/us-indices-1w.jsonl`, sha256 `aa4d8d46ff69a464c64c2080b1ef4c0eaa1403a02e57daac8a1a47de809c105f`, digest source: paper/evidence/provenance.json

**Producer command.** python paper/src/make-evidence-figures.py (paper/sections/A-commands.tex)

**Producing commit.** v0.9.0 evidence snapshot. The commit that cut v0.9.0 is not recorded and stays unknown. paper/evidence/provenance.json records the commit at which the manifest itself was generated, which is rebound whenever a file in its source scope moves and is not the source state these numbers were produced from; the manifest's digest of each artifact below is the identity that does hold.

**Effective configuration.** Operational DSR of the best of five luck-floor agents against the effective trial count; one agent's DSR is a pooled-track statistic with no interval in the frozen records.

**Missing provenance.**
- none identified

**Relevant later changes.**
- Standardized-moment estimators mixed finite-sample normalizations, so every probabilistic and deflated Sharpe here was computed under the superseded convention, and how far the correction moves the frozen values is not established (paper/sections/E-repairs.tex:5).
- A track with no Sharpe ratio no longer votes on another agent's deflation bar. hold voted wherever a record stamps measured or floored: 64 of the 576 default-sweep panels, all 27 panels of the externally specified sweep and four of the nine power-curve panels. How far the repaired measurement would move each bar is not established and is not estimated (paper/sections/E-repairs.tex:15, :17).

**Present applicability.** Verified for figure-to-record agreement by the passing test_evidence_figures.py on 2026-09-23. The plotted deflated Sharpes carry the superseded moment convention, and daily rates is one of the panels whose measured bar the dispersion-vote repair would move.

**Disposition.** `historical-only`. The figure validly reduces the frozen records; the manuscript binds those records to v0.9.0 and the repairs that would move them are recorded rather than applied.

**Owner.** SharpeBench paper/evidence owner

**Follow-up.** none

**Status history.**

| Date | Disposition | Rationale |
|---|---|---|
| 2026-09-23 | historical-only | Initial entry: The figure validly reduces the frozen records; the manuscript binds those records to v0.9.0 and the repairs that would move them are recorded rather than applied. |

### `SB-tab-external`

**Claim.** The four externally specified rules on all nine datasets under the typical cost profile in a thirteen-agent field: no cell passes pass^k and no cell is eligible.

**Source.** `paper/sections/05-experiments.tex:159`

**Labels.** `tab:external`

**Artifacts.**
- `paper/evidence/final/external-rules.jsonl`, sha256 `14fd1306755457d3e16162ec983eca53e7b69f6ed000fc6e9a49875589c2a9af`, digest source: paper/evidence/provenance.json

**Producer command.** cargo run --release -p sharpebench-harness --example external_rules_eval -- paper/evidence/final/external-rules.jsonl [dataset] (paper/sections/A-commands.tex)

**Producing commit.** v0.9.0 evidence snapshot. The commit that cut v0.9.0 is not recorded and stays unknown. paper/evidence/provenance.json records the commit at which the manifest itself was generated, which is rebound whenever a file in its source scope moves and is not the source state these numbers were produced from; the manifest's digest of each artifact below is the identity that does hold.

**Effective configuration.** Thirteen agents on nine datasets under three cost profiles, 351 records, default ScoreConfig per dataset's periods per year; the four rules use their published parameters with none fitted here.

**Missing provenance.**
- none identified

**Relevant later changes.**
- Standardized-moment estimators mixed finite-sample normalizations, so every probabilistic and deflated Sharpe here was computed under the superseded convention, and how far the correction moves the frozen values is not established (paper/sections/E-repairs.tex:5).
- The simulator let a zero target cross through flat into a short position and the momentum entrant ignored its declared lookback, so the current engine reconstructs different trajectories (paper/sections/E-repairs.tex:5).
- A constant observed track is now refused for PSR and DSR, so the current kernel would refuse all 576 frozen hold records as unavailable rather than scoring them (paper/sections/E-repairs.tex:9).
- A track with no Sharpe ratio no longer votes on another agent's deflation bar. hold voted wherever a record stamps measured or floored: 64 of the 576 default-sweep panels, all 27 panels of the externally specified sweep and four of the nine power-curve panels. How far the repaired measurement would move each bar is not established and is not estimated (paper/sections/E-repairs.tex:15, :17).

**Present applicability.** Valid as the v0.9.0 record. paper/sections/E-repairs.tex:17 records that all 27 panels of this sweep had hold voting on the measured dispersion, and on the three crypto panels faber-10m as well, so every bar in this family is one the current engine would measure differently.

**Disposition.** `historical-only`. The artifact validly records the frozen run; the current-engine question is carried by SB-sec-external.

**Owner.** SharpeBench paper/evidence owner

**Follow-up.** none

**Status history.**

| Date | Disposition | Rationale |
|---|---|---|
| 2026-09-23 | historical-only | Initial entry: The artifact validly records the frozen run; the current-engine question is carried by SB-sec-external. |

### `SB-tab-costsens`

**Claim.** Cost sensitivity across the three shipped profiles: every bar in the table is field-measured, and the typical profile lifts five panels well above the precommitted floor, hourly crypto to 27.5397 and daily FX to 6.7466.

**Source.** `paper/sections/05-experiments.tex:209`

**Labels.** `tab:costsens`

**Artifacts.**
- `paper/evidence/final/external-rules.jsonl`, sha256 `14fd1306755457d3e16162ec983eca53e7b69f6ed000fc6e9a49875589c2a9af`, digest source: paper/evidence/provenance.json

**Producer command.** cargo run --release -p sharpebench-harness --example external_rules_eval -- paper/evidence/final/external-rules.jsonl [dataset] (paper/sections/A-commands.tex)

**Producing commit.** v0.9.0 evidence snapshot. The commit that cut v0.9.0 is not recorded and stays unknown. paper/evidence/provenance.json records the commit at which the manifest itself was generated, which is rebound whenever a file in its source scope moves and is not the source state these numbers were produced from; the manifest's digest of each artifact below is the identity that does hold.

**Effective configuration.** Thirteen-agent field, default configuration, three cost profiles; the caption records that the frozen kernel let hold, and on three crypto panels faber-10m, vote on the measurement.

**Missing provenance.**
- none identified

**Relevant later changes.**
- A track with no Sharpe ratio no longer votes on another agent's deflation bar. hold voted wherever a record stamps measured or floored: 64 of the 576 default-sweep panels, all 27 panels of the externally specified sweep and four of the nine power-curve panels. How far the repaired measurement would move each bar is not established and is not estimated (paper/sections/E-repairs.tex:15, :17).
- Standardized-moment estimators mixed finite-sample normalizations, so every probabilistic and deflated Sharpe here was computed under the superseded convention, and how far the correction moves the frozen values is not established (paper/sections/E-repairs.tex:5).

**Present applicability.** Every cell in the Bar columns is a field-measured dispersion that the current engine would take over a different voting set, and the caption already says so. How far each bar would move is not established and is not estimated.

**Disposition.** `needs-new-experiment`. This table's headline quantity is exactly the one the dispersion-vote repair changes, and The committed records serialize each agent's deflated and probabilistic Sharpe but not its pooled Sharpe, and the probabilistic Sharpe cannot be inverted without that track's skewness and kurtosis, so the frozen artifacts lack the inputs a repaired dispersion measurement would need (paper/sections/E-repairs.tex:17). A rescore from the committed records is therefore not sufficient. P00-E maps evidence and authorizes no rerun. The follow-up names the ticket that would own one.

**Owner.** SharpeBench paper/evidence owner

**Follow-up.** P12-A: re-run the externally specified sweep on the current engine and report it as a separately versioned experiment beside, not in place of, the frozen table.

**Status history.**

| Date | Disposition | Rationale |
|---|---|---|
| 2026-09-23 | needs-new-experiment | Initial entry: This table's headline quantity is exactly the one the dispersion-vote repair changes, and The committed records serialize each agent's deflated and probabilistic Sharpe but not its pooled Sharpe, and the probabilistic Sharpe cannot be inverted without that track's skewness and kurtosis, so the frozen artifacts lack the inputs a repaired dispersion measurement would need (paper/sections/E-repairs.tex:17). A rescore from the committed records is therefore not sufficient. P00-E maps evidence and authorizes no rerun. The follow-up names the ticket that would own one. |

### `SB-sec-external`

**Claim.** No record is rank-eligible and none passes pass^k in any of the 351 cells under any of the three cost profiles; the highest deflated Sharpe anywhere in the paper is donchian-20-10 on daily crypto at 0.6534 frictionless, 0.4155 typical and 0.1185 stressed.

**Source.** `paper/sections/05-experiments.tex:137`, `paper/sections/05-experiments.tex:145`

**Artifacts.**
- `paper/evidence/final/external-rules.jsonl`, sha256 `14fd1306755457d3e16162ec983eca53e7b69f6ed000fc6e9a49875589c2a9af`, digest source: paper/evidence/provenance.json

**Producer command.** cargo run --release -p sharpebench-harness --example external_rules_eval -- paper/evidence/final/external-rules.jsonl [dataset] (paper/sections/A-commands.tex)

**Producing commit.** v0.9.0 evidence snapshot. The commit that cut v0.9.0 is not recorded and stays unknown. paper/evidence/provenance.json records the commit at which the manifest itself was generated, which is rebound whenever a file in its source scope moves and is not the source state these numbers were produced from; the manifest's digest of each artifact below is the identity that does hold.

**Effective configuration.** As SB-tab-external.

**Missing provenance.**
- none identified

**Relevant later changes.**
- Standardized-moment estimators mixed finite-sample normalizations, so every probabilistic and deflated Sharpe here was computed under the superseded convention, and how far the correction moves the frozen values is not established (paper/sections/E-repairs.tex:5).
- The simulator let a zero target cross through flat into a short position and the momentum entrant ignored its declared lookback, so the current engine reconstructs different trajectories (paper/sections/E-repairs.tex:5).
- A constant observed track is now refused for PSR and DSR, so the current kernel would refuse all 576 frozen hold records as unavailable rather than scoring them (paper/sections/E-repairs.tex:9).
- A track with no Sharpe ratio no longer votes on another agent's deflation bar. hold voted wherever a record stamps measured or floored: 64 of the 576 default-sweep panels, all 27 panels of the externally specified sweep and four of the nine power-curve panels. How far the repaired measurement would move each bar is not established and is not estimated (paper/sections/E-repairs.tex:15, :17).
- The committed records serialize each agent's deflated and probabilistic Sharpe but not its pooled Sharpe, and the probabilistic Sharpe cannot be inverted without that track's skewness and kurtosis, so the frozen artifacts lack the inputs a repaired dispersion measurement would need (paper/sections/E-repairs.tex:17).

**Present applicability.** Established for the frozen snapshot. The claim that removing every friction still admits nobody rests on bars the current engine would measure over a different voting set, so it is not established for the shipped engine today.

**Disposition.** `needs-new-experiment`. Generation and measurement both changed and the committed records lack the pooled Sharpes a repaired measurement needs, so no defensible rescore exists. P00-E maps evidence and authorizes no rerun. The follow-up names the ticket that would own one.

**Owner.** SharpeBench paper/evidence owner

**Follow-up.** P12-A then P13-A/P14-A: re-run the externally specified field on the current engine as a new experiment.

**Status history.**

| Date | Disposition | Rationale |
|---|---|---|
| 2026-09-23 | needs-new-experiment | Initial entry: Generation and measurement both changed and the committed records lack the pooled Sharpes a repaired measurement needs, so no defensible rescore exists. P00-E maps evidence and authorizes no rerun. The follow-up names the ticket that would own one. |

### `SB-tab-relative`

**Claim.** Per-window reliability under the default and the benchmark-relative verdict on all nine datasets: no agent becomes eligible under the relative verdict and none passes pass^k under it.

**Source.** `paper/sections/relative-mandate-fragment.tex:24`

**Labels.** `tab:relative`

**Artifacts.**
- `paper/evidence/final/relative-mandate.jsonl`, sha256 `dad27a40a6605bf2bd2273682bda03c142594777896f734725a202caa4b6d577`, digest source: paper/evidence/provenance.json
- `paper/evidence/final/relative-mandate.log`, sha256 `d2f95b2e75d9df26ff9e038d642e9687bdcd4eef64f37b8276d12814589e2f68`, digest source: computed for this register; the file is outside the provenance manifest scope

**Producer command.** cargo run --release -p sharpebench-harness --example relative_mandate_eval -- paper/evidence/final/relative-mandate.jsonl (paper/sections/A-commands.tex)

**Producing commit.** v0.9.0 evidence snapshot. The commit that cut v0.9.0 is not recorded and stays unknown. paper/evidence/provenance.json records the commit at which the manifest itself was generated, which is rebound whenever a file in its source scope moves and is not the source state these numbers were produced from; the manifest's digest of each artifact below is the identity that does hold.

**Effective configuration.** Nine-agent risk-managed field, all nine datasets, default verdict and PassMode::RelativeToBenchmark with buy-and-hold as benchmark; 81 records, six windows times eight seeds per agent per dataset.

**Missing provenance.**
- none identified

**Relevant later changes.**
- Standardized-moment estimators mixed finite-sample normalizations, so every probabilistic and deflated Sharpe here was computed under the superseded convention, and how far the correction moves the frozen values is not established (paper/sections/E-repairs.tex:5).
- The simulator let a zero target cross through flat into a short position and the momentum entrant ignored its declared lookback, so the current engine reconstructs different trajectories (paper/sections/E-repairs.tex:5).
- paper/sections/E-repairs.tex:9 records that the benchmark-relative verdict refuses a zero-excess series through its own dispersion clause, independently of the constant-track repair.

**Present applicability.** Valid as the v0.9.0 record. The per-window pass vectors are PSR comparisons that carry the superseded moment convention.

**Disposition.** `historical-only`. The artifact validly records the frozen run under both verdicts, and the manuscript binds it to that snapshot.

**Owner.** SharpeBench paper/evidence owner

**Follow-up.** none

**Status history.**

| Date | Disposition | Rationale |
|---|---|---|
| 2026-09-23 | historical-only | Initial entry: The artifact validly records the frozen run under both verdicts, and the manuscript binds it to that snapshot. |

### `SB-tab-mandate`

**Claim.** No declared agent meets its declared eligibility predicate in any of the 36 declared agent-dataset cases; the one declared reliability pass, daily-crypto risk-managed, is refused by deflation and the bootstrap.

**Source.** `paper/sections/mandate-declaration-fragment.tex:25`

**Labels.** `tab:mandate`

**Artifacts.**
- `paper/evidence/final/mandate-declaration.jsonl`, sha256 `4092bfad1f76ec08a64f86c1040fad36ace7d08f9e99827e58573918ab24a795`, digest source: paper/evidence/provenance.json

**Producer command.** cargo run --release -p sharpebench-harness --example mandate_eval -- paper/evidence/final/mandate-declaration.jsonl (paper/sections/A-commands.tex)

**Producing commit.** v0.9.0 evidence snapshot. The commit that cut v0.9.0 is not recorded and stays unknown. paper/evidence/provenance.json records the commit at which the manifest itself was generated, which is rebound whenever a file in its source scope moves and is not the source state these numbers were produced from; the manifest's digest of each artifact below is the identity that does hold.

**Effective configuration.** Nine-agent field on nine datasets, 81 records, 36 declared and 45 undeclared; declared columns produced by the kernel's rank_declared.

**Missing provenance.**
- none identified

**Relevant later changes.**
- Standardized-moment estimators mixed finite-sample normalizations, so every probabilistic and deflated Sharpe here was computed under the superseded convention, and how far the correction moves the frozen values is not established (paper/sections/E-repairs.tex:5).
- A constant observed track is now refused for PSR and DSR, so the current kernel would refuse all 576 frozen hold records as unavailable rather than scoring them (paper/sections/E-repairs.tex:9).
- A track with no Sharpe ratio no longer votes on another agent's deflation bar. hold voted wherever a record stamps measured or floored: 64 of the 576 default-sweep panels, all 27 panels of the externally specified sweep and four of the nine power-curve panels. How far the repaired measurement would move each bar is not established and is not estimated (paper/sections/E-repairs.tex:15, :17).
- paper/sections/E-repairs.tex:17 records that the reconciliation of tab:eligibility against tab:mandate is a property of the frozen kernel: withdrawing hold's vote takes the three reconciling panels below the five-vote minimum in the nine-agent field as well, so on the current engine the added agent changes the dispersion source on no panel.

**Present applicability.** Valid as the v0.9.0 record. The reconciliation the surrounding section draws between this table and tab:eligibility is explicitly a property of the frozen kernel and does not hold on the current one.

**Disposition.** `historical-only`. The artifact validly records the frozen run, and the one cross-table inference it supports is already bound to the frozen kernel in the manuscript.

**Owner.** SharpeBench paper/evidence owner

**Follow-up.** none

**Status history.**

| Date | Disposition | Rationale |
|---|---|---|
| 2026-09-23 | historical-only | Initial entry: The artifact validly records the frozen run, and the one cross-table inference it supports is already bound to the frozen kernel in the manuscript. |

### `SB-tab-seedleg`

**Claim.** Under the opt-in execution-noise profile the across-seed dispersion of per-run annualized Sharpe rises by roughly two orders of magnitude on three daily datasets, and the seed leg still never refuses.

**Source.** `paper/sections/execution-noise-fragment.tex:29`

**Labels.** `tab:seedleg`

**Artifacts.**
- `paper/evidence/final/seed-leg.jsonl`, sha256 `a3d94d3967beb4f9521db0ec7fcdcc7a1776cee490503aa7bf74637203c96a4a`, digest source: paper/evidence/provenance.json

**Producer command.** cargo run --release -p sharpebench-harness --example seed_leg_eval -- paper/evidence/final/seed-leg.jsonl (paper/sections/A-commands.tex)

**Producing commit.** v0.9.0 evidence snapshot. The commit that cut v0.9.0 is not recorded and stays unknown. paper/evidence/provenance.json records the commit at which the manifest itself was generated, which is rebound whenever a file in its source scope moves and is not the source state these numbers were produced from; the manifest's digest of each artifact below is the identity that does hold.

**Effective configuration.** Nine-agent field on US indices 1d, crypto 1d and FX 1d under the default and CostProfile::Realistic; 2,646 records, being 2,592 per-run rows and 54 per-agent verdict rows.

**Missing provenance.**
- The across-seed standard deviations are estimated from eight seeds, for which the relative standard error of a sample standard deviation is about 27 percent (paper/sections/execution-noise-fragment.tex:63).

**Relevant later changes.**
- Standardized-moment estimators mixed finite-sample normalizations, so every probabilistic and deflated Sharpe here was computed under the superseded convention, and how far the correction moves the frozen values is not established (paper/sections/E-repairs.tex:5).
- The simulator let a zero target cross through flat into a short position and the momentum entrant ignored its declared lookback, so the current engine reconstructs different trajectories (paper/sections/E-repairs.tex:5).

**Present applicability.** Valid as the v0.9.0 record. The per-run PSRs carry the superseded moment convention and the noise model's parameters are hand-set rather than calibrated to a venue.

**Disposition.** `historical-only`. The artifact validly records the frozen run and the manuscript bounds the claim with three stated caveats.

**Owner.** SharpeBench paper/evidence owner

**Follow-up.** none

**Status history.**

| Date | Disposition | Rationale |
|---|---|---|
| 2026-09-23 | historical-only | Initial entry: The artifact validly records the frozen run and the manuscript bounds the claim with three stated caveats. |

### `SB-sec-perturb`

**Claim.** On weekly US indices the committed perturbation report gives a per-run PSR spread of 0.0928 for the risk-managed agent and 0.0534 for buy-and-hold; the fragile-agent separation is produced by a unit test on synthetic data.

**Source.** `paper/sections/05-experiments.tex:237`

**Artifacts.**
- `paper/evidence/final/risk-managed.jsonl`, sha256 `9909246e803bf0014df61b113f7a3ff22a304d89f5252237611b2b9b7becfb99`, digest source: paper/evidence/provenance.json, carries the appended perturbation spread report
- `paper/evidence/final/risk-managed.log`, sha256 `2a5df5ef27ea41b7dc4213de6d055d303784365940261dc6cb53d15234094822`, digest source: computed for this register; the file is outside the provenance manifest scope
- `crates/sharpebench-harness/src/perturb.rs`, sha256 `259ea09e03352312556a43a9d5ad03f8d9b69bba461edb67d874bbf1fa05968a`, digest source: paper/evidence/provenance.json, the perturbation generator and the tests that pin its empirical-range constraint and seed determinism

**Producer command.** cargo run --release -p sharpebench-harness --example risk_managed_eval -- out.jsonl for the committed spread; cargo test -p sharpebench-harness perturb for the fragile-agent separation (paper/sections/A-commands.tex)

**Producing commit.** v0.9.0 evidence snapshot. The commit that cut v0.9.0 is not recorded and stays unknown. paper/evidence/provenance.json records the commit at which the manifest itself was generated, which is rebound whenever a file in its source scope moves and is not the source state these numbers were produced from; the manifest's digest of each artifact below is the identity that does hold. The perturbation unit test runs on the current tree.

**Effective configuration.** Perturbed datasets whose bar-to-bar moves stay inside the empirical range of the original series, asserted per bar; spread is best minus worst per-run PSR.

**Missing provenance.**
- none identified

**Relevant later changes.**
- Standardized-moment estimators mixed finite-sample normalizations, so every probabilistic and deflated Sharpe here was computed under the superseded convention, and how far the correction moves the frozen values is not established (paper/sections/E-repairs.tex:5).

**Present applicability.** The committed spread is a frozen PSR statistic and carries the superseded moment convention. The synthetic separation is re-established by a test on the current tree.

**Disposition.** `historical-only`. The reported spread comes from the frozen artifact; the current-tree test covers the separate synthetic claim, not these two numbers.

**Owner.** SharpeBench paper/evidence owner

**Follow-up.** none

**Status history.**

| Date | Disposition | Rationale |
|---|---|---|
| 2026-09-23 | historical-only | Initial entry: The reported spread comes from the frozen artifact; the current-tree test covers the separate synthetic claim, not these two numbers. |

### `SB-sec-witness`

**Claim.** In a single common-random-number draw the acceptance region is nonempty: the witness becomes rank-eligible at an injected per-period Sharpe of 0.40 weekly (annualized 2.88) and 0.25 daily (annualized 3.97), with pass^k the binding gate on both geometries.

**Source.** `paper/sections/05-experiments.tex:241`, `paper/sections/05-experiments.tex:251`

**Labels.** `fig:witness`

**Artifacts.**
- `paper/evidence/final/pass-witness.jsonl`, sha256 `8922c9125aed8c75e9d7f2734a0756e57dbfa73da4e333e15882aeca040c295f`, digest source: paper/evidence/provenance.json
- `paper/figures/evidence-pass-witness.pdf`, sha256 `07f41b726d4382988f6bcbbb6641a45b20f56bcda95c8e9da6779eea76e46a1a`, digest source: paper/evidence/provenance.json

**Producer command.** cargo run --release -p sharpebench-harness --example pass_witness -- pass-witness.jsonl; figure by python paper/src/make-evidence-figures.py pass-witness (paper/sections/A-commands.tex)

**Producing commit.** The one regenerated artifact. paper/sections/E-repairs.tex:7 records that the committed field is a declared rerun under the corrected producer, executed by the current kernel including the corrected standardized-moment estimators.

**Effective configuration.** Two window geometries, six 77-bar windows at 52 periods per year and six 409-bar windows at 252; a separate five-agent zero-edge calibration field fixes the exogenous DSR bar; common random numbers across the edge grid; edge steps of 0.05.

**Missing provenance.**
- The records hold one draw and no replicate, so the onsets carry sampling uncertainty this draw does not quantify.

**Relevant later changes.**
- paper/sections/E-repairs.tex:7 records the crossing point moving by exactly one grid step on both geometries relative to the superseded draw, jointly attributable to the independent draw and the corrected estimators.

**Present applicability.** Applicable to the current kernel, bounded to one common-random-number draw. Where the boundary lies in expectation is not established.

**Disposition.** `still-applicable`. The documented comparison is paper/sections/E-repairs.tex:7: this is the one field rerun under the repaired producer and the current kernel, and the manuscript states the resulting shift.

**Owner.** SharpeBench paper/evidence owner

**Follow-up.** P13-A: replication over independent noise draws, which the introduction records as planned rather than done (paper/sections/01-introduction.tex:22).

**Status history.**

| Date | Disposition | Rationale |
|---|---|---|
| 2026-09-23 | still-applicable | Initial entry: The documented comparison is paper/sections/E-repairs.tex:7: this is the one field rerun under the repaired producer and the current kernel, and the manuscript states the resulting shift. |

### `SB-sec-falsify`

**Claim.** Under the corrected luck floor the best random agent has a lower raw return than the best reference agent on every one of the nine dataset-timeframe combinations; on weekly crypto the best random agent scores 0.3105 at host N = 1 and 0.0762 at N = 50, and on weekly US indices 0.0800 and 0.0044.

**Source.** `paper/sections/05-experiments.tex:254`, `paper/sections/05-experiments.tex:257`

**Artifacts.**
- `paper/evidence/final/us-indices-1d.jsonl`, sha256 `d8aeabb1b69e9f9345ec04fea82dd6391652b7ce5e7f8e9a4f9487cf469d4324`, digest source: paper/evidence/provenance.json
- `paper/evidence/final/us-indices-1w.jsonl`, sha256 `aa4d8d46ff69a464c64c2080b1ef4c0eaa1403a02e57daac8a1a47de809c105f`, digest source: paper/evidence/provenance.json
- `paper/evidence/final/crypto-majors-1h.jsonl`, sha256 `0b6c74f29540682efec1aef7d860f0638535903bc4416000793e23d8fac2a8a9`, digest source: paper/evidence/provenance.json
- `paper/evidence/final/crypto-majors-4h.jsonl`, sha256 `be6c3abbd6189f8372693b9370148850b99dbc02fa9a1b017f05db894b13cb85`, digest source: paper/evidence/provenance.json
- `paper/evidence/final/crypto-majors-1d.jsonl`, sha256 `5d522ba86c67d11903acb4187950c706e79ebd6a044d84d784bf0d90f74e9258`, digest source: paper/evidence/provenance.json
- `paper/evidence/final/crypto-majors-1w.jsonl`, sha256 `78ac6179bec6f2a7b474c7a05ab35f827fa5db0a5aa9f980083e54961eaa697e`, digest source: paper/evidence/provenance.json
- `paper/evidence/final/fx-majors-1d.jsonl`, sha256 `dc5a219e44a2cc1e4ca5e19f5de4363cb8226aab300dc47d61c780f4e6333337`, digest source: paper/evidence/provenance.json
- `paper/evidence/final/commodities-1d.jsonl`, sha256 `c2e389e99ebe6baa408f415dcb7e2da0c90672933054903e339fc4e05349500d`, digest source: paper/evidence/provenance.json
- `paper/evidence/final/rates-1d.jsonl`, sha256 `34018ad8511d509fc086dc711bb0acb704a192a41e7979a0d9329165e4bedd65`, digest source: paper/evidence/provenance.json

**Producer command.** cargo run --release -p sharpebench-harness --example evidence_sweep -- out.jsonl [dataset] [dsr_bar], assembled by paper/evidence/assemble_sweep.py and reduced by paper/evidence/analyze.py (paper/sections/A-commands.tex)

**Producing commit.** v0.9.0 evidence snapshot. The commit that cut v0.9.0 is not recorded and stays unknown. paper/evidence/provenance.json records the commit at which the manifest itself was generated, which is rebound whenever a file in its source scope moves and is not the source state these numbers were produced from; the manifest's digest of each artifact below is the identity that does hold.

**Effective configuration.** Default cell beta = 0.95, host N = 50, field size eight; eight execution seeds, six disjoint windows, typical cost profile; 512 records per dataset over 64 host configurations, 4,608 in all. The one-symbol floor draws a random gross exposure per period, flat half the time and otherwise a uniform long weight.

**Missing provenance.**
- none identified

**Relevant later changes.**
- Standardized-moment estimators mixed finite-sample normalizations, so every probabilistic and deflated Sharpe here was computed under the superseded convention, and how far the correction moves the frozen values is not established (paper/sections/E-repairs.tex:5).
- The simulator let a zero target cross through flat into a short position and the momentum entrant ignored its declared lookback, so the current engine reconstructs different trajectories (paper/sections/E-repairs.tex:5).

**Present applicability.** Valid as the v0.9.0 record. The raw-return comparison is a path quantity the simulator repair can move, and the deflated Sharpes quoted carry the superseded moment convention.

**Disposition.** `historical-only`. The artifact validly records the frozen run under the corrected floor construction; neither effect of the later repairs on this comparison is established, so the claim stays bound to the snapshot.

**Owner.** SharpeBench paper/evidence owner

**Follow-up.** none

**Status history.**

| Date | Disposition | Rationale |
|---|---|---|
| 2026-09-23 | historical-only | Initial entry: The artifact validly records the frozen run under the corrected floor construction; neither effect of the later repairs on this comparison is established, so the claim stays bound to the snapshot. |

### `SB-sec-luck1000`

**Claim.** One thousand random agents on each of two daily datasets, scored at the observable 1,000-trial footprint, with the shipped floored path and the deliberately unfloored diagnostic shown separately.

**Source.** `paper/sections/hardening-fragment.tex:5`, `paper/sections/hardening-fragment.tex:13`

**Labels.** `fig:luck1000`

**Artifacts.**
- `paper/evidence/final/luck-floor-1000.jsonl`, sha256 `c9c157cccb6dd5e02fa4fdf60a6acfb6b21a1aa13040dea5a4c550e7ef24e0dd`, digest source: paper/evidence/provenance.json
- `paper/figures/evidence-luck-floor-1000.pdf`, sha256 `8fc33351b7d01be6dbed253d3dac87d8f08050cf3c6949e75d415b1cff6c32ae`, digest source: paper/evidence/provenance.json

**Producer command.** cargo run --release -p sharpebench-harness --example luck_floor_1000 -- paper/evidence/final/luck-floor-1000.jsonl; figure by python paper/src/make-evidence-figures.py luck-floor-1000 (paper/sections/A-commands.tex)

**Producing commit.** v0.9.0 evidence snapshot. The commit that cut v0.9.0 is not recorded and stays unknown. paper/evidence/provenance.json records the commit at which the manifest itself was generated, which is rebound whenever a file in its source scope moves and is not the source state these numbers were produced from; the manifest's digest of each artifact below is the identity that does hold.

**Effective configuration.** 1,000 distinct seeds on us-indices-1d and crypto-majors-1d under the same windows, eight execution seeds and costs as the evidence sweep; trial count fixed at 1,000.

**Missing provenance.**
- none identified

**Relevant later changes.**
- Standardized-moment estimators mixed finite-sample normalizations, so every probabilistic and deflated Sharpe here was computed under the superseded convention, and how far the correction moves the frozen values is not established (paper/sections/E-repairs.tex:5).
- The simulator let a zero target cross through flat into a short position and the momentum entrant ignored its declared lookback, so the current engine reconstructs different trajectories (paper/sections/E-repairs.tex:5).

**Present applicability.** Valid as the v0.9.0 record. The field contains no constant track, so the constant-track and dispersion-vote repairs do not reach it; the moment repair still moves every plotted deflated Sharpe by an unquantified amount.

**Disposition.** `historical-only`. The artifact validly records the frozen run, and the repair that does touch it has an unestablished magnitude, so the claim stays bound to the snapshot.

**Owner.** SharpeBench paper/evidence owner

**Follow-up.** none

**Status history.**

| Date | Disposition | Rationale |
|---|---|---|
| 2026-09-23 | historical-only | Initial entry: The artifact validly records the frozen run, and the repair that does touch it has an unestablished magnitude, so the claim stays bound to the snapshot. |

### `SB-sec-sybil`

**Claim.** On the streams as submitted the largest honest pair across every committed field is 0.9904 and nothing merges at the 0.995 collapse threshold; on the seed-averaged streams six of nine panels merge at least one honest pair and the highest honest similarity is 0.9999 on commodities.

**Source.** `paper/sections/sybil-defense-fragment.tex:4`

**Artifacts.**
- `crates/sharpebench-harness/tests/evidence_fields_no_clone_merges.rs`, sha256 `8b92f47a04d8b69a685d4d3ab6d3ee091fd3a3e684b4adc9fa6bbf39d7ad49c5`, digest source: paper/evidence/provenance.json, rebuilds every committed evidence field twice and asserts the merge counts and the stamped dispersion source
- `crates/sharpebench-core/src/rediscovery.rs`, sha256 `a14b70c1d1f317eee9cfec132c35a1a884875e7225497d74d66d1bcdc4db67f3`, digest source: paper/evidence/provenance.json, carries CLONE_COLLAPSE_COSINE = 0.995

**Producer command.** cargo test -p sharpebench-harness --test evidence_fields_no_clone_merges (paper/sections/A-commands.tex)

**Producing commit.** current engineering tree; the exact commit is not recorded in the manuscript.

**Effective configuration.** Absolute cosine similarity over pooled return streams, as submitted and seed-averaged; connected components vote once through their median Sharpe.

**Missing provenance.**
- The similarity figures are printed by the test rather than stored in a committed artifact, so there is no digest for the numbers the section quotes.

**Relevant later changes.**
- A track with no Sharpe ratio no longer votes on another agent's deflation bar. hold voted wherever a record stamps measured or floored: 64 of the 576 default-sweep panels, all 27 panels of the externally specified sweep and four of the nine power-curve panels. How far the repaired measurement would move each bar is not established and is not estimated (paper/sections/E-repairs.tex:15, :17). The dispersion sample is now qualified before clone collapse, which changes which streams the similarity is taken over on panels containing a constant track.

**Present applicability.** The merge behaviour is re-established on every commit by the named test. The quoted similarity values are the test's printed output, which the register cannot pin to a stored artifact.

**Disposition.** `still-applicable`. The documented comparison is the commit-time test that rebuilds every committed field and asserts the outcome; it executes against the current kernel.

**Owner.** SharpeBench paper/evidence owner

**Follow-up.** P15: write the per-field similarity maxima to a committed artifact so the two quoted numbers carry a digest rather than living only in test output.

**Status history.**

| Date | Disposition | Rationale |
|---|---|---|
| 2026-09-23 | still-applicable | Initial entry: The documented comparison is the commit-time test that rebuilds every committed field and asserts the outcome; it executes against the current kernel. |

### `SB-sec-power`

**Claim.** Under serially independent normal returns the default pass^k leg passes a true annualized Sharpe of 1 only 1.4 percent of the time on the daily geometry and 1.1 percent on the weekly, and reaches 95 percent at 2.89 and 3.03; tab:dsr-mde extends this to all nine default panels.

**Source.** `paper/sections/power-fragment.tex:7`, `paper/sections/power-fragment.tex:32`, `paper/sections/power-fragment.tex:39`

**Labels.** `fig:power`, `tab:dsr-mde`

**Artifacts.**
- `paper/evidence/final/power-curve.jsonl`, sha256 `2450017a9ac05475f7547d0fda7651a97476efff02d4e4168ae72cd78304eff5`, digest source: paper/evidence/provenance.json
- `paper/figures/power-curve.pdf`, sha256 `44b0373da860683f3e4e87ec261d14ea4827f1b15e2635c17c9cddf1ee8bb138`, digest source: paper/evidence/provenance.json
- `paper/src/make-power-curve.py`, sha256 `a778b53f07de9975872083db3bdee2905fbe30a9ccef182c9b5b8acf0dcc2598`, digest source: paper/evidence/provenance.json

**Producer command.** python paper/src/make-power-curve.py compute --jobs 32 then python paper/src/make-power-curve.py figure (paper/sections/A-commands.tex)

**Producing commit.** current engineering tree; the exact commit is not recorded in the manuscript. The calculation is new in this revision and reads only the committed default-cell deflation bars; no agent, simulator run, market data or model is involved.

**Effective configuration.** Monte Carlo over 200,000 replications per window geometry; every point's standard error is at most 0.0012. DSR minima are derived in closed form rather than simulated.

**Missing provenance.**
- The bars this calculation reads are the frozen snapshot's, so the table's inputs carry v0.9.0 provenance while its producer is on the current tree.

**Relevant later changes.**
- A track with no Sharpe ratio no longer votes on another agent's deflation bar. hold voted wherever a record stamps measured or floored: 64 of the 576 default-sweep panels, all 27 panels of the externally specified sweep and four of the nine power-curve panels. How far the repaired measurement would move each bar is not established and is not estimated (paper/sections/E-repairs.tex:15, :17). paper/sections/E-repairs.tex:17 counts four of the nine power-curve panels among those whose bar hold voted on: commodities, hourly crypto, daily FX and daily rates.

**Present applicability.** Verified on 2026-09-23 for this register: python -B -m unittest paper/src/test_power_curve.py passes, so the committed file and figure are what the current producer writes. The five panels at the configured 1.1382 bar are unaffected by the dispersion-vote repair; the four measured or floored panels read a bar the current engine would measure differently.

**Disposition.** `needs-rescore`. The producer is deterministic and reads only bars, so a current-engine version of this table is a recomputation rather than a new experiment; it is blocked only on repaired bars, which SB-sec-passk and SB-sec-external own. P00-E maps evidence and authorizes no rerun. The follow-up names the ticket that would own one.

**Owner.** SharpeBench paper/evidence owner

**Follow-up.** P12-A supplies repaired bars; then recompute this table as a separately versioned output beside the frozen one.

**Status history.**

| Date | Disposition | Rationale |
|---|---|---|
| 2026-09-23 | needs-rescore | Initial entry: The producer is deterministic and reads only bars, so a current-engine version of this table is a recomputation rather than a new experiment; it is blocked only on repaired bars, which SB-sec-passk and SB-sec-external own. P00-E maps evidence and authorizes no rerun. The follow-up names the ticket that would own one. |

### `SB-sec-forecast-report`

**Claim.** The committed prospective-forecast report reproduces from two deterministic SharpeArena ledgers under schema sharpebench.forecast-quality.v2, with exact pair support over twelve contracts in six resolution-time blocks, empty unresolved lists, late-revision exclusion, calibration, block-bootstrap comparison and Holm adjustment.

**Source.** `paper/sections/03-benchmark.tex:53`

**Artifacts.**
- `paper/evidence/prospective-forecast-field/report.json`, sha256 `c96d7ec3e946d5cf7f5e9afcfc6004760071b3f0b9bc6ee59160d59e499aad80`, digest source: paper/evidence/provenance.json
- `paper/evidence/prospective-forecast-field/report-check.json`, sha256 `82f28fd922221080b00c11f5d97b41d91efcff230ce94e0cb64389124614122b`, digest source: paper/evidence/provenance.json
- `paper/evidence/prospective-forecast-field/resolution-manifest.json`, sha256 `c7f213512aa80558bd4c0ded3155798516b2a283911929a6e9c67012e2b78bbc`, digest source: paper/evidence/provenance.json
- `paper/src/check-prospective-forecast-report.py`, sha256 `744861445c45c7d5be3bafe903be02d61e95131e4a67231af8346b19befc355b`, digest source: paper/evidence/provenance.json, recomputes the report independently in Python

**Producer command.** cargo run -q -p sharpebench -- forecast-quality examples/forecast-quality/fixtures/agent-alpha.json examples/forecast-quality/fixtures/agent-beta.json --bootstrap-samples 400 --seed 23 --confidence 0.9 --alpha 0.05 --bins 5 --json (paper/sections/A-commands.tex)

**Producing commit.** current engineering tree; the exact commit is not recorded in the manuscript.

**Effective configuration.** Deterministic fixtures; bootstrap seed 23, 400 samples, confidence 0.9, alpha 0.05, five bins.

**Missing provenance.**
- none identified

**Relevant later changes.**
- paper/sections/E-repairs.tex:19 records the move to schema v2 with the same support, because both fields are complete, and that the archived v1 pilot report still verifies.

**Present applicability.** Verified on 2026-09-23 for this register: python -m unittest paper/src/test_check_prospective_forecast_report.py passes, and the report's digests are in the provenance manifest.

**Disposition.** `still-applicable`. The documented comparison is the independent Python recomputation, which runs in CI against the committed report.

**Owner.** SharpeBench paper/evidence owner

**Follow-up.** none

**Status history.**

| Date | Disposition | Rationale |
|---|---|---|
| 2026-09-23 | still-applicable | Initial entry: The documented comparison is the independent Python recomputation, which runs in CI against the committed report. |

### `SB-sec-forecast-size`

**Claim.** Measured over 6,000 null draws with independent normal per-contract differences, five contracts a block and 2,000 replications, the rejection rate at a nominal 5 percent is 0.188 at four blocks, 0.162 at five, 0.138 at six, 0.096 at ten, 0.072 at twenty and 0.060 at forty, and skewed differences push the four-block rate to 0.237.

**Source.** `paper/sections/03-benchmark.tex:59`, `paper/sections/01-introduction.tex:24`, `paper/sections/07-limitations.tex:37`

**Artifacts.**
- none located

**Producer command.** not recorded. The values appear only as the MEASURED_SIZE constant table in crates/sharpebench-core/src/forecast.rs:1321 and in FAMILYWISE_SIZE_RULE at :1298; no producer script, seed, driver or raw draw file was found in the tree.

**Producing commit.** unknown

**Effective configuration.** Stated in prose: 6,000 null draws, five contracts a block, 2,000 replications, independent normal and lognormal per-contract differences. No seed is recorded.

**Missing provenance.**
- No producer command, script or driver for the size study exists in the repository.
- No raw draws, seed or output artifact is committed; the numbers are hard-coded constants.
- The comparison figures for the two rejected alternatives, 0.013 for the studentised statistic and 0.050 and 0.094 for the Student-t, have the same gap.

**Relevant later changes.**
- None recorded; the constant table has no producer to have changed.

**Present applicability.** Not established either way. The numbers are published beside every p-value and are cited as contribution (5) in the introduction, but nothing in the tree lets a reader recompute or verify them.

**Disposition.** `unresolved`. The artifact cannot be located, so no disposition can be chosen without guessing. Recorded as unresolved rather than assumed current, per the ticket's rule.

**Owner.** SharpeBench paper/evidence owner

**Follow-up.** P15: commit the size study's producer and its output, or state in the manuscript that the size table is an uncommitted measurement.

**Status history.**

| Date | Disposition | Rationale |
|---|---|---|
| 2026-09-23 | unresolved | Initial entry: The artifact cannot be located, so no disposition can be chosen without guessing. Recorded as unresolved rather than assumed current, per the ticket's rule. |

### `SB-sec-compute`

**Claim.** Measured compute: the power calculation took 313.2 seconds in one process and 53.6 with 32 workers; regenerating the 4,608-record grid took 2,078 seconds and reproduced all 4,608 committed deflated Sharpes exactly; the externally specified field took 129.3 seconds at a peak resident set of 3,846 MiB.

**Source.** `paper/sections/05-experiments.tex:5`

**Artifacts.**
- none located

**Producer command.** the commands of paper/sections/A-commands.tex, timed on one AMD Ryzen Threadripper PRO 5975WX with 256 GiB of RAM under Windows 11.

**Producing commit.** unknown for the timings; the grid regeneration is stated to have reproduced the frozen records exactly.

**Effective configuration.** Single-threaded scoring processes; the power calculation is the one command that parallelises.

**Missing provenance.**
- No timing artifact, log or benchmark record is committed for any of these measurements.
- The regeneration that reproduced all 4,608 deflated Sharpes left no committed receipt, so the strongest reproducibility statement in the section is unverifiable from the tree.

**Relevant later changes.**
- The repairs of paper/sections/E-repairs.tex change what the current engine computes, so the exact-reproduction claim is a statement about the frozen snapshot only.

**Present applicability.** Not established. The numbers are plausible and internally consistent but no artifact in the repository records them.

**Disposition.** `unresolved`. No artifact could be located for any of the quoted measurements, and the ticket requires unresolved rather than a guess.

**Owner.** SharpeBench paper/evidence owner

**Follow-up.** P15: commit a timing receipt, or state the measurements as uncommitted machine observations.

**Status history.**

| Date | Disposition | Rationale |
|---|---|---|
| 2026-09-23 | unresolved | Initial entry: No artifact could be located for any of the quoted measurements, and the ticket requires unresolved rather than a guess. |

### `SB-claims-i`

**Claim.** Headline claim (i): on eight price-level datasets plus one rates-yield series the benchmark certifies none of the unhedged long-only references as safe to hand capital under any of the three reliability verdicts, and reports the drawdown that justifies the refusal.

**Source.** `paper/sections/05-experiments.tex:266`

**Artifacts.**
- `paper/evidence/final/us-indices-1d.jsonl`, sha256 `d8aeabb1b69e9f9345ec04fea82dd6391652b7ce5e7f8e9a4f9487cf469d4324`, digest source: paper/evidence/provenance.json
- `paper/evidence/final/us-indices-1w.jsonl`, sha256 `aa4d8d46ff69a464c64c2080b1ef4c0eaa1403a02e57daac8a1a47de809c105f`, digest source: paper/evidence/provenance.json
- `paper/evidence/final/crypto-majors-1h.jsonl`, sha256 `0b6c74f29540682efec1aef7d860f0638535903bc4416000793e23d8fac2a8a9`, digest source: paper/evidence/provenance.json
- `paper/evidence/final/crypto-majors-4h.jsonl`, sha256 `be6c3abbd6189f8372693b9370148850b99dbc02fa9a1b017f05db894b13cb85`, digest source: paper/evidence/provenance.json
- `paper/evidence/final/crypto-majors-1d.jsonl`, sha256 `5d522ba86c67d11903acb4187950c706e79ebd6a044d84d784bf0d90f74e9258`, digest source: paper/evidence/provenance.json
- `paper/evidence/final/crypto-majors-1w.jsonl`, sha256 `78ac6179bec6f2a7b474c7a05ab35f827fa5db0a5aa9f980083e54961eaa697e`, digest source: paper/evidence/provenance.json
- `paper/evidence/final/fx-majors-1d.jsonl`, sha256 `dc5a219e44a2cc1e4ca5e19f5de4363cb8226aab300dc47d61c780f4e6333337`, digest source: paper/evidence/provenance.json
- `paper/evidence/final/commodities-1d.jsonl`, sha256 `c2e389e99ebe6baa408f415dcb7e2da0c90672933054903e339fc4e05349500d`, digest source: paper/evidence/provenance.json
- `paper/evidence/final/rates-1d.jsonl`, sha256 `34018ad8511d509fc086dc711bb0acb704a192a41e7979a0d9329165e4bedd65`, digest source: paper/evidence/provenance.json
- `paper/evidence/final/relative-mandate.jsonl`, sha256 `dad27a40a6605bf2bd2273682bda03c142594777896f734725a202caa4b6d577`, digest source: paper/evidence/provenance.json
- `paper/evidence/final/risk-managed.jsonl`, sha256 `9909246e803bf0014df61b113f7a3ff22a304d89f5252237611b2b9b7becfb99`, digest source: paper/evidence/provenance.json

**Producer command.** cargo run --release -p sharpebench-harness --example evidence_sweep -- out.jsonl [dataset] [dsr_bar], assembled by paper/evidence/assemble_sweep.py and reduced by paper/evidence/analyze.py (paper/sections/A-commands.tex); relative and never-catastrophic verdicts from the relative-mandate and risk-managed examples.

**Producing commit.** v0.9.0 evidence snapshot. The commit that cut v0.9.0 is not recorded and stays unknown. paper/evidence/provenance.json records the commit at which the manifest itself was generated, which is rebound whenever a file in its source scope moves and is not the source state these numbers were produced from; the manifest's digest of each artifact below is the identity that does hold.

**Effective configuration.** Default cell beta = 0.95, host N = 50, field size eight; eight execution seeds, six disjoint windows, typical cost profile; 512 records per dataset over 64 host configurations, 4,608 in all.

**Missing provenance.**
- none identified

**Relevant later changes.**
- Standardized-moment estimators mixed finite-sample normalizations, so every probabilistic and deflated Sharpe here was computed under the superseded convention, and how far the correction moves the frozen values is not established (paper/sections/E-repairs.tex:5).
- The simulator let a zero target cross through flat into a short position and the momentum entrant ignored its declared lookback, so the current engine reconstructs different trajectories (paper/sections/E-repairs.tex:5).
- A constant observed track is now refused for PSR and DSR, so the current kernel would refuse all 576 frozen hold records as unavailable rather than scoring them (paper/sections/E-repairs.tex:9).
- A track with no Sharpe ratio no longer votes on another agent's deflation bar. hold voted wherever a record stamps measured or floored: 64 of the 576 default-sweep panels, all 27 panels of the externally specified sweep and four of the nine power-curve panels. How far the repaired measurement would move each bar is not established and is not estimated (paper/sections/E-repairs.tex:15, :17).
- The committed records serialize each agent's deflated and probabilistic Sharpe but not its pooled Sharpe, and the probabilistic Sharpe cannot be inverted without that track's skewness and kurtosis, so the frozen artifacts lack the inputs a repaired dispersion measurement would need (paper/sections/E-repairs.tex:17).

**Present applicability.** Established for the frozen snapshot. As a present-tense claim about the shipped benchmark it inherits SB-sec-passk: three repairs move quantities every verdict reads and their magnitude is not established.

**Disposition.** `needs-new-experiment`. Same reasoning as SB-sec-passk: generation changed and the committed records lack the inputs a repaired measurement needs. This is a headline claim, so it is queued rather than left implicit. P00-E maps evidence and authorizes no rerun. The follow-up names the ticket that would own one.

**Owner.** SharpeBench paper/evidence owner

**Follow-up.** P12-A then P13-A/P14-A.

**Status history.**

| Date | Disposition | Rationale |
|---|---|---|
| 2026-09-23 | needs-new-experiment | Initial entry: Same reasoning as SB-sec-passk: generation changed and the committed records lack the inputs a repaired measurement needs. This is a headline claim, so it is queued rather than left implicit. P00-E maps evidence and authorizes no rerun. The follow-up names the ticket that would own one. |

### `SB-claims-ii`

**Claim.** Headline claim (ii): the corrected luck floor never beats a reference agent, even when expanded to 1,000 agents on two daily datasets.

**Source.** `paper/sections/05-experiments.tex:266`

**Artifacts.**
- `paper/evidence/final/us-indices-1d.jsonl`, sha256 `d8aeabb1b69e9f9345ec04fea82dd6391652b7ce5e7f8e9a4f9487cf469d4324`, digest source: paper/evidence/provenance.json
- `paper/evidence/final/us-indices-1w.jsonl`, sha256 `aa4d8d46ff69a464c64c2080b1ef4c0eaa1403a02e57daac8a1a47de809c105f`, digest source: paper/evidence/provenance.json
- `paper/evidence/final/crypto-majors-1h.jsonl`, sha256 `0b6c74f29540682efec1aef7d860f0638535903bc4416000793e23d8fac2a8a9`, digest source: paper/evidence/provenance.json
- `paper/evidence/final/crypto-majors-4h.jsonl`, sha256 `be6c3abbd6189f8372693b9370148850b99dbc02fa9a1b017f05db894b13cb85`, digest source: paper/evidence/provenance.json
- `paper/evidence/final/crypto-majors-1d.jsonl`, sha256 `5d522ba86c67d11903acb4187950c706e79ebd6a044d84d784bf0d90f74e9258`, digest source: paper/evidence/provenance.json
- `paper/evidence/final/crypto-majors-1w.jsonl`, sha256 `78ac6179bec6f2a7b474c7a05ab35f827fa5db0a5aa9f980083e54961eaa697e`, digest source: paper/evidence/provenance.json
- `paper/evidence/final/fx-majors-1d.jsonl`, sha256 `dc5a219e44a2cc1e4ca5e19f5de4363cb8226aab300dc47d61c780f4e6333337`, digest source: paper/evidence/provenance.json
- `paper/evidence/final/commodities-1d.jsonl`, sha256 `c2e389e99ebe6baa408f415dcb7e2da0c90672933054903e339fc4e05349500d`, digest source: paper/evidence/provenance.json
- `paper/evidence/final/rates-1d.jsonl`, sha256 `34018ad8511d509fc086dc711bb0acb704a192a41e7979a0d9329165e4bedd65`, digest source: paper/evidence/provenance.json
- `paper/evidence/final/luck-floor-1000.jsonl`, sha256 `c9c157cccb6dd5e02fa4fdf60a6acfb6b21a1aa13040dea5a4c550e7ef24e0dd`, digest source: paper/evidence/provenance.json

**Producer command.** cargo run --release -p sharpebench-harness --example evidence_sweep -- out.jsonl [dataset] [dsr_bar], assembled by paper/evidence/assemble_sweep.py and reduced by paper/evidence/analyze.py (paper/sections/A-commands.tex); the thousand-agent extension from the luck_floor_1000 example.

**Producing commit.** v0.9.0 evidence snapshot. The commit that cut v0.9.0 is not recorded and stays unknown. paper/evidence/provenance.json records the commit at which the manifest itself was generated, which is rebound whenever a file in its source scope moves and is not the source state these numbers were produced from; the manifest's digest of each artifact below is the identity that does hold.

**Effective configuration.** Default cell beta = 0.95, host N = 50, field size eight; eight execution seeds, six disjoint windows, typical cost profile; 512 records per dataset over 64 host configurations, 4,608 in all.

**Missing provenance.**
- none identified

**Relevant later changes.**
- The simulator let a zero target cross through flat into a short position and the momentum entrant ignored its declared lookback, so the current engine reconstructs different trajectories (paper/sections/E-repairs.tex:5).
- Standardized-moment estimators mixed finite-sample normalizations, so every probabilistic and deflated Sharpe here was computed under the superseded convention, and how far the correction moves the frozen values is not established (paper/sections/E-repairs.tex:5).

**Present applicability.** Valid as the v0.9.0 record. The comparison is on raw return, which the moment repair does not touch, but which the simulator repair can move through the trajectories themselves.

**Disposition.** `historical-only`. The artifacts validly record the frozen run; whether the simulator repair moves the comparison is not established, so the claim stays bound to the snapshot rather than being asserted of the current engine.

**Owner.** SharpeBench paper/evidence owner

**Follow-up.** none

**Status history.**

| Date | Disposition | Rationale |
|---|---|---|
| 2026-09-23 | historical-only | Initial entry: The artifacts validly record the frozen run; whether the simulator repair moves the comparison is not established, so the claim stays bound to the snapshot rather than being asserted of the current engine. |

### `SB-claims-iii`

**Claim.** Headline claim (iii): four rules whose specifications come from the published literature are refused in all 351 cells, including under a frictionless cost profile.

**Source.** `paper/sections/05-experiments.tex:266`

**Artifacts.**
- `paper/evidence/final/external-rules.jsonl`, sha256 `14fd1306755457d3e16162ec983eca53e7b69f6ed000fc6e9a49875589c2a9af`, digest source: paper/evidence/provenance.json

**Producer command.** cargo run --release -p sharpebench-harness --example external_rules_eval -- paper/evidence/final/external-rules.jsonl [dataset]

**Producing commit.** v0.9.0 evidence snapshot. The commit that cut v0.9.0 is not recorded and stays unknown. paper/evidence/provenance.json records the commit at which the manifest itself was generated, which is rebound whenever a file in its source scope moves and is not the source state these numbers were produced from; the manifest's digest of each artifact below is the identity that does hold.

**Effective configuration.** As SB-tab-external.

**Missing provenance.**
- none identified

**Relevant later changes.**
- Standardized-moment estimators mixed finite-sample normalizations, so every probabilistic and deflated Sharpe here was computed under the superseded convention, and how far the correction moves the frozen values is not established (paper/sections/E-repairs.tex:5).
- The simulator let a zero target cross through flat into a short position and the momentum entrant ignored its declared lookback, so the current engine reconstructs different trajectories (paper/sections/E-repairs.tex:5).
- A constant observed track is now refused for PSR and DSR, so the current kernel would refuse all 576 frozen hold records as unavailable rather than scoring them (paper/sections/E-repairs.tex:9).
- A track with no Sharpe ratio no longer votes on another agent's deflation bar. hold voted wherever a record stamps measured or floored: 64 of the 576 default-sweep panels, all 27 panels of the externally specified sweep and four of the nine power-curve panels. How far the repaired measurement would move each bar is not established and is not estimated (paper/sections/E-repairs.tex:15, :17).
- The committed records serialize each agent's deflated and probabilistic Sharpe but not its pooled Sharpe, and the probabilistic Sharpe cannot be inverted without that track's skewness and kurtosis, so the frozen artifacts lack the inputs a repaired dispersion measurement would need (paper/sections/E-repairs.tex:17).

**Present applicability.** Established for the frozen snapshot only; all 27 panels of this sweep carried a dispersion vote the current engine excludes.

**Disposition.** `needs-new-experiment`. Inherits SB-sec-external: no defensible rescore exists from the committed records. Queued as a headline claim. P00-E maps evidence and authorizes no rerun. The follow-up names the ticket that would own one.

**Owner.** SharpeBench paper/evidence owner

**Follow-up.** P12-A then P13-A/P14-A.

**Status history.**

| Date | Disposition | Rationale |
|---|---|---|
| 2026-09-23 | needs-new-experiment | Initial entry: Inherits SB-sec-external: no defensible rescore exists from the committed records. Queued as a headline claim. P00-E maps evidence and authorizes no rerun. The follow-up names the ticket that would own one. |

### `SB-claims-iv`

**Claim.** Headline claim (iv): all ten self-audit attacks are demoted by their commit-time tests.

**Source.** `paper/sections/05-experiments.tex:266`

**Artifacts.**
- `crates/sharpebench-core/src/selfaudit.rs`, sha256 `579718f85fe269b33174681aad905a3803da9ff5a6a67278cb31afea2af13a16`, digest source: paper/evidence/provenance.json
- `crates/sharpebench-arena/src/audit.rs`, sha256 `12c13aadaa1c5e0028e8d4872838ee6fb332b4fd48bb1bd9906185641620eae4`, digest source: paper/evidence/provenance.json

**Producer command.** sharpebench audit; enforced by the commit-time tests in the two files above.

**Producing commit.** current engineering tree; the exact commit is not recorded in the manuscript.

**Effective configuration.** Fixed synthetic constructions; the audit exits non-zero unless every case is defended and no known gap remains.

**Missing provenance.**
- none identified

**Relevant later changes.**
- The ninth case moved from a recorded known gap to a defended case when ranking gained clone collapse (paper/sections/04-integrity.tex:42).

**Present applicability.** Re-established on every commit against the current kernel.

**Disposition.** `still-applicable`. The documented comparison is the commit-time test, which runs against the current tree rather than against the frozen snapshot.

**Owner.** SharpeBench paper/evidence owner

**Follow-up.** none

**Status history.**

| Date | Disposition | Rationale |
|---|---|---|
| 2026-09-23 | still-applicable | Initial entry: The documented comparison is the commit-time test, which runs against the current tree rather than against the frozen snapshot. |

### `SB-claims-v`

**Claim.** Headline claim (v): the two committed golden fields reproduce byte-identically on Linux, macOS and Windows CI.

**Source.** `paper/sections/05-experiments.tex:266`

**Artifacts.**
- `crates/sharpebench-core/golden/example_submissions.scores.json`, sha256 `00cc54e484c5193e424d46c649d05378a5f5eb3bafc8d5a42a880dac8f7e54f6`, digest source: computed for this register; the file is outside the provenance manifest scope
- `crates/sharpebench-core/golden/synthetic_field.scores.json`, sha256 `678a9783017cb1c783daaf6a14a82bf86bf089c9469a4f41ab1567b0fe613e8c`, digest source: computed for this register; the file is outside the provenance manifest scope
- `crates/sharpebench-core/golden/synthetic_field.input.json`, sha256 `b5d298d72a68fb5922fbcbd252a7922fe1033f133d38203f2c15a4ec01cd8b61`, digest source: computed for this register; the file is outside the provenance manifest scope

**Producer command.** cargo test -p sharpebench-core --test golden_scores; cargo test -p sharpebench-sim --test golden_input; cargo test -p sharpebench-wasm --test native_parity (paper/sections/A-commands.tex)

**Producing commit.** current engineering tree; the exact commit is not recorded in the manuscript.

**Effective configuration.** Regeneration requires SHARPEBENCH_UPDATE_GOLDEN=1 and refuses to run when CI is set.

**Missing provenance.**
- none identified

**Relevant later changes.**
- The golden fields changed in the Pareto flag of hold and in calibration counts, Brier scores and confidence-weighted return; no gate reads those fields (paper/sections/E-repairs.tex:19).

**Present applicability.** Re-established on every CI run across the three targets.

**Disposition.** `still-applicable`. The documented comparison is the golden test on three CI targets, which executes against the current kernel.

**Owner.** SharpeBench paper/evidence owner

**Follow-up.** none

**Status history.**

| Date | Disposition | Rationale |
|---|---|---|
| 2026-09-23 | still-applicable | Initial entry: The documented comparison is the golden test on three CI targets, which executes against the current kernel. |

### `SB-claims-vi`

**Claim.** Headline claim (vi): in a single common-random-number draw a controlled injected edge is first admitted between annualized Sharpe 2.9 and 4, so the acceptance region is nonempty; where its boundary lies is not established beyond that draw.

**Source.** `paper/sections/05-experiments.tex:266`

**Artifacts.**
- `paper/evidence/final/pass-witness.jsonl`, sha256 `8922c9125aed8c75e9d7f2734a0756e57dbfa73da4e333e15882aeca040c295f`, digest source: paper/evidence/provenance.json

**Producer command.** cargo run --release -p sharpebench-harness --example pass_witness -- pass-witness.jsonl

**Producing commit.** the corrected producer under the current kernel (paper/sections/E-repairs.tex:7)

**Effective configuration.** As SB-sec-witness.

**Missing provenance.**
- none identified

**Relevant later changes.**
- The crossing point moved one grid step on both geometries relative to the superseded draw (paper/sections/E-repairs.tex:7).

**Present applicability.** Applicable to the current kernel, bounded to one draw, and the claim already states that bound.

**Disposition.** `still-applicable`. The witness is the one field rerun under the repaired producer and the current kernel, and the claim is stated with its sampling caveat.

**Owner.** SharpeBench paper/evidence owner

**Follow-up.** P13-A: replication over independent noise draws.

**Status history.**

| Date | Disposition | Rationale |
|---|---|---|
| 2026-09-23 | still-applicable | Initial entry: The witness is the one field rerun under the repaired producer and the current kernel, and the claim is stated with its sampling caveat. |
