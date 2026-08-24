# Peer Review Report

## Manuscript Information
- **Title**: SharpeBench: The Luck-Robust Benchmark for AI Trading Agents
- **Manuscript ID**: sharpebench/paper (main.tex, commit-local working tree)
- **Review Date**: 2026-08-24
- **Review Round**: Round 1

---

## Reviewer Information

### Reviewer Role
Peer Reviewer 1 (Methodology)

### Reviewer Identity
Quantitative-finance methodologist: backtest overfitting and the deflated Sharpe ratio (Bailey and Lopez de Prado), multiple-testing corrections (White, Hansen, Romano-Wolf), bootstrap methods, and pass^k reliability metrics for agent evaluation.

### Review Focus
Statistical validity of the deflation math and its units fix; correctness of the four findings' empirical claims; whether every number traces to a listed command and committed data; reproducibility; whether uncertainty is reported where rankings are claimed; sample sizes and statistical red flags.

### Verification performed
This review did not rely on the manuscript's word. I (a) hand-recomputed Eq. (2) and Table 2, (b) re-ran the committed reduction script `paper/evidence/analyze.py` over the committed JSONL under `paper/evidence/final/`, (c) inspected `paper/evidence/final/risk-managed.jsonl` and `risk-managed.log` at full float precision, (d) recomputed the luck-floor deflated-Sharpe maxima by trial count and dispersion path directly from the records, and (e) read `paper/src/make-evidence-figures.py` and `crates/sharpebench-harness/src/perturb.rs` to check what the figures and the perturbation claims are computed from.

---

## Overall Assessment

### Recommendation
- [x] **Major Revision** — Substantial revisions needed, re-review required after revision

### Confidence Score
5 (deflation/DSR mathematics, multiple testing, and bootstrap methodology are core expertise; every load-bearing number was independently recomputed or traced)

Confidence is an uncertainty/scope disclosure only; it never changes consensus counts, severity, decision bearing, or arbitration.

### Calibration Status
`NOT_CALIBRATED`

### Summary Assessment
The paper presents a deterministic, gate-based benchmark for trading agents and four empirical findings from running it on nine frozen datasets, two of the findings being corrections to the benchmark itself. The statistical core is sound: the PSR and expected-maximum-Sharpe formulas match Bailey and Lopez de Prado (2014), the units diagnosis is arithmetically correct (I reproduce the bracket term 2.2776 and every cell of Table 2 to the stated precision), and the annualized-to-per-period conversion by sqrt(periods per year) is the standard scaling. Traceability is exceptional for the field: re-running the committed reduction script over the committed JSONL reproduces every value in Table 3, Finding 4's numbers (DSR 0.3015, PSR 0.9998, bootstrap p 0.0005, drawdowns 0.874/0.325), and the falsification-leg values. However, one substantive defect materially weakens the falsification leg: on the single-symbol rates dataset the fully-invested random-weight luck floor degenerates into an exact clone of buy-and-hold (all five luck agents share buy-and-hold's raw return to the last float bit), so the "zero-skill" control there is not a control, and the Section 5.6 rates example (DSR 0.993 at N=1) is buy-and-hold in disguise. Several claims are also stated more broadly than the evidence path supports (the figure caption's "everywhere", the abstract's "clearing every other gate", the Table 3 caption's "every dataset"), and the bootstrap and final kernel version are under-specified in the manuscript. The core claims survive; the luck-floor construction and the over-broad phrasings need repair, hence Major Revision.

---

## Strengths

### S1: The units diagnosis and its correction are arithmetically verified
The central finding, that a sigma_trials of 0.5 substituted per period at N=50 yields SR*_0 = 1.138 per period, is correct: the expected-maximum bracket at N=50 evaluates to (1-0.5772)*2.0537 + 0.5772*2.4415 = 2.2776, times 0.5 gives 1.139, and the annualized equivalents 106.5 (1h), 53.3 (4h), 18.1 (1d), 8.2 (1w) and the measured-prior row (0.070 -> 0.159 -> 14.9/7.5/2.5/1.1) all reproduce by hand. The fix (state the prior annualized, divide by sqrt(periods per year) once at point of use) is the correct scaling under the standard IID assumption, and Appendix C's test design, including the honest note that the old 0.5 prior cannot be made passable, is exactly right.
**Evidence Anchor**: `table: Table 2 (tab:units), sections/05-experiments.tex lines 31-47, all six columns recomputed`

### S2: Every headline number traces to committed data and a committed command
Re-running `python paper/evidence/analyze.py` over `paper/evidence/final/*.jsonl` reproduces Table 3 exactly (US 1w buy-and-hold 0.984/0.320, crypto 1w momentum 0.978/0.563, crypto 1w buy-and-hold 0.745/0.671, crypto 1d 0.144/0.476, hourly momentum worst drawdown 0.993, 4608 records, zero grid-eligible agents, zero luck-floor raw-return violations), and `final/risk-managed.jsonl` reproduces Finding 4 (DSR 0.3015, PSR 0.9998, p = 0.0005, worst drawdowns 0.874 on hourly crypto and 0.325 on FX, smallest worst-window drawdown on exactly seven of nine datasets). This level of number-to-artifact traceability is far above the norm the paper itself criticizes.
**Evidence Anchor**: `dataset: paper/evidence/final/*.jsonl reduced by paper/evidence/analyze.py; risk-managed.jsonl per-record values`

### S3: The multiplicity design errs in the conservative direction
The grid (64 configurations x 9 datasets) could invite cherry-picking, but the paper's claims are of the form "zero eligible anywhere in 576 cells", which multiplicity can only make harder, not easier; declared in-sample trials raise each agent's own deflation bar; and the field-wide White/Hansen/Romano-Wolf procedures are computed but explicitly demoted from gates after an earlier documentation error. The direction of every multiple-testing choice favors the null.
**Evidence Anchor**: `text: sections/03-benchmark.tex "they do not gate, and earlier documentation that called them gates was wrong"`

### S4: Self-corrections are reported as findings with the failure mechanism quantified
Two of the four findings are errors in the shipped benchmark, reported with the exact mechanism (the sqrt(periods-per-year) growth of the unit error; the same index scoring 0.984 weekly and 0.000 daily) rather than silently patched. The internal consistency check that exposed the bug (PSR = 1.0000 and DSR = 0.0000 on the same series cannot both be verdicts about the data) is a model of statistical sanity checking.
**Evidence Anchor**: `text: sections/05-experiments.tex "A PSR of one and a DSR of zero on the same series cannot both be verdicts about the data"`

---

## Weaknesses

### W1: The luck floor degenerates into buy-and-hold on the single-symbol rates dataset
**Problem**: The luck floor is defined as agents "fully invested in random weights each period" (sections/05-experiments.tex line 3). On `rates-1d`, whose universe is a single series (10y Treasury yield), a fully-invested random weight is always 1.0 on the only symbol, so every "random" agent is buy-and-hold. The committed records confirm it: all five luck-floor agents on rates-1d have raw_mean_return = 0.00036124370412967755, bit-identical to buy-and-hold, and `risk-managed.log` shows identical DSR/PSR/worst-drawdown rows for buy-and-hold and all five luck agents on rates-1d.
**Evidence Anchor**: `dataset: paper/evidence/final/rates-1d.jsonl, default cell, raw_mean_return identical across buy-and-hold and luck-floor-00..04`
**Why it matters**: Three claims silently weaken. (a) The falsification-leg claim "the luck floor's best agent has a lower raw return than the best reference agent, with zero exceptions in nine" (Section 5.6) is satisfied on rates-1d only as an exact tie between clones, which is vacuous, not evidence that "the floor behaves as a floor". (b) The Section 5.6 example "a random agent reaches a DSR of 0.993 on the rates series" at N=1 is not a zero-skill agent getting lucky; it is buy-and-hold itself, so the sentence's rhetorical force (noise looks certain without deflation) is not demonstrated by that example. (c) sigma_trials on rates-1d is "measured across the field", but six of the eight field members are the same return stream, so the measured dispersion (0.014) is estimated from effectively three distinct streams with six duplicates, biasing the dispersion, and therefore the deflation bar, in an unquantified direction. The two-symbol commodities dataset is partially exposed to the same degeneracy.
**Suggestion**: Redefine the luck floor so it has non-trivial randomness on every universe (random gross exposure in [-1, 1] or random long/flat/short per period on single-symbol universes), rerun the sweep on rates-1d and commodities-1d, and either replace the rates example in Section 5.6 with a dataset where the floor is genuinely random (weekly crypto already serves) or state the degeneracy explicitly and exclude single-symbol datasets from the luck-floor claim. Also deduplicate or otherwise guard the measured-dispersion path against fields containing identical return streams.
**Severity**: Major | **Confidence**: 5 — core expertise: zero-skill null construction; verified at full float precision in the committed records

### W2: The figure caption's "under 0.43 everywhere by N=50" is false over the full grid
**Problem**: Figure (b)'s caption claims the best zero-skill agent is "under 0.43 everywhere by N=50". Recomputing from the committed records, the maximum luck-floor DSR at N=50 across all cells is 0.623 (crypto-majors-1w under a pinned annualized prior). The 0.423 maximum holds only on the measured-dispersion path at dsr_bar 0.95, which is what `make-evidence-figures.py` actually plots (it filters `sr_std_pinned is None`). The body text of Section 5.6 is careful ("at the default configuration"); the caption is not.
**Evidence Anchor**: `figure: Fig. (b) caption, sections/05-experiments.tex line 112, "under $0.43$ everywhere by $N = 50$" vs 0.623 in crypto-majors-1w.jsonl at n_trials=50, sr_std pinned`
**Why it matters**: A reader citing the caption would state a false bound; the abstract-level story (deflation crushes the luck floor) survives, but the quantitative claim as written does not match the committed evidence it says it is computed from.
**Suggestion**: Change the caption to "under 0.43 at the default configuration (measured dispersion) by N=50", or extend the figure to the pinned-prior cells and report 0.623.
**Severity**: Minor | **Confidence**: 5 — recomputed directly from the committed JSONL

### W3: "Refused solely on deflation after clearing every other gate" is verdict-conditional and the abstract does not say so
**Problem**: The abstract says the risk-managed agent "is refused solely on deflation after clearing every other gate". The committed record for us-indices-1w shows passed_k = false: under the default every-regime reliability verdict the agent fails pass^k as well. "Solely on deflation" is true only under the never-catastrophic ablation verdict (pass mode any, drawdown bound 0.113 < 0.20). Section 5.4 hints at this by listing "the per-run drawdown bound" among the cleared gates, but neither the abstract nor the introduction states that the claim is conditional on the weaker reliability configuration.
**Evidence Anchor**: `dataset: paper/evidence/final/risk-managed.jsonl, us-indices-1w row, passed_k=false alongside psr=0.9998, bootstrap_p=0.0005, worst_run_drawdown=0.113`
**Why it matters**: Finding 4 is the paper's discrimination claim ("the gates discriminate"). As phrased in the abstract, a reader concludes the default gate refuses this agent on deflation alone, which the committed evidence contradicts; the correct statement is that under the default verdict it fails both pass^k and deflation, and under the never-catastrophic verdict it fails deflation alone.
**Suggestion**: In the abstract and Section 5.4's opening sentence, qualify: "under the never-catastrophic reliability verdict, refused solely on deflation". The discrimination story is untouched.
**Severity**: Minor | **Confidence**: 5 — read directly from the committed record

### W4: Bootstrap specification is absent from the manuscript
**Problem**: The bootstrap gate (p_boot < 0.05) is described only as "a stationary block bootstrap with a fixed seed". The number of resamples, the expected block length, and the p-value convention are not stated anywhere in the paper. The committed value 0.0004997501249375312 = 1/2001 implies B = 2000 with the (r+1)/(B+1) correction, but a reader should not have to reverse-engineer this from a float.
**Evidence Anchor**: `absence: sections/03-benchmark.tex Statistics paragraph — expected the bootstrap resample count, expected block length, and p-value convention; checked §3.1, §5, Appendix A, Appendix C`
**Why it matters**: A gate at alpha = 0.05 driven by an unreported B and block length is not fully specified in the document of record; block length in particular determines how much serial correlation the null preserves, which is the entire point of using a stationary bootstrap on autocorrelated returns.
**Suggestion**: One sentence in Section 3.1: resample count, expected block length (and how it scales with n, e.g. Politis-White), the p-value convention, and the seed.
**Severity**: Minor | **Confidence**: 5 — core expertise: bootstrap methods; convention identified from the committed value

### W5: The final kernel version is not pinned in the manuscript
**Problem**: Appendix A states "Findings 1 and 2 were first observed at v0.2.1. The units fix is v0.3.0. The configurable reliability gate, the per-run drawdown bound, and the evidence sweep ... are in the release after it", and Table 3's caption says "kernel v0.3.0" while the ablation columns it contains are said to come from the unnamed later release. The FINDING evidence notes pin v0.2.1 to commit d0ae4f2, but no tag or commit is given for the kernel that produced `evidence/final/`.
**Evidence Anchor**: `text: sections/A-commands.tex "are in the release after it"`
**Why it matters**: The paper's reproducibility contract is "all commands run from the repository root at the tagged version"; for the evidence that populates every table except Table 2, the tagged version is not named, and the Table 3 caption's "v0.3.0" appears inconsistent with the appendix's "release after it" for the never-catastrophic column.
**Suggestion**: Name the exact version (or commit hash) for the final sweep and the risk-managed evaluation, and reconcile the Table 3 caption with Appendix A.
**Severity**: Minor | **Confidence**: 4 — reproducibility auditing; the inconsistency is textual and easily fixed

### W6: The measured sigma_trials carries no uncertainty and is estimated from eight (partly duplicated) agents
**Problem**: When the field holds at least five agents, sigma_trials is the sample dispersion of per-period Sharpes across the field, here eight agents of which five are luck-floor agents and one never trades. A dispersion estimated from n = 8, with no standard error, feeds directly into the deflation benchmark SR*_0 that decides Finding 2's DSR column (0.984 vs 0.95 is a 0.034 margin). The limitations section discloses that a field can be assembled to move the statistic, but not its sampling variability, and W1 shows the effective n is smaller still where luck agents duplicate references.
**Evidence Anchor**: `text: sections/03-benchmark.tex "When the field holds at least five agents it instead measures the dispersion of per-period Sharpes across the field"`
**Why it matters**: The near-bar values the paper leans on narratively (US 1w at 0.984, crypto 1w momentum at 0.978, both just above beta = 0.95) are conditional on a noisy dispersion estimate; a modest perturbation of sigma_trials moves them across the bar. No ranking rests on them, which limits the damage, but the paper asserts these are "the true verdict of the statistic" without qualification.
**Suggestion**: Report the field size and a resampling-based interval (jackknife over agents) for sigma_trials on each dataset, or at least soften "true verdict" to acknowledge estimation error at n = 8; consider a minimum-distinct-streams requirement for the measured path.
**Severity**: Minor | **Confidence**: 4 — core expertise: DSR mechanics; impact bounded because no eligibility flips on it (pass^k independently refuses)

### W7: The sqrt-time conversion of the annualized prior assumes IID returns and the assumption is unstated
**Problem**: The units fix divides the annualized sigma_trials by sqrt(periods per year). Sharpe ratios scale with sqrt(time) only under IID returns; with serial correlation the correct scaling differs (Lo, 2002, "The Statistics of Sharpe Ratios"), and the paper's own realism battery documents volatility clustering in these very datasets, which is a departure from IID.
**Evidence Anchor**: `text: sections/03-benchmark.tex "the scorer takes it annualized and divides by $\sqrt{\text{periods per year}}$ once at the point of use"`
**Why it matters**: The corrected protocol is right in units and approximately right in magnitude, but the conversion inherits an assumption the data visibly violates; at hourly frequency the discrepancy can be non-trivial. Since the prior is a coarse calibration choice this does not overturn Finding 1, but the paper should say what the conversion assumes, especially in a paper whose first finding is a units-and-assumptions error.
**Suggestion**: One sentence noting the IID assumption behind sqrt-time scaling, citing Lo (2002), and noting the measured-dispersion path avoids it.
**Severity**: Minor | **Confidence**: 5 — core expertise: Sharpe scaling

### W8: The fragile-agent perturbation number is not produced by any listed command over committed data
**Problem**: Section 5.5 claims a deliberately fragile synthetic agent "shows more than twice the reference spread". The committed perturbation report (`final/risk-managed.jsonl`, kind "perturbation", and `risk-managed.log`) contains only risk-managed (spread 0.000514) and buy-and-hold (3.4e-6); the fragile agent exists only inside a unit test in `crates/sharpebench-harness/src/perturb.rs`. The paper's own contract ("Every number is produced by a listed command from committed data", abstract and main.tex preamble) is violated for this number, and Appendix A maps sec:perturb only to `risk_managed_eval`, which does not emit it.
**Evidence Anchor**: `absence: paper/evidence/final/risk-managed.jsonl perturbation records — expected a fragile-agent spread row supporting the "more than twice" claim; checked risk-managed.jsonl, risk-managed.log, Appendix A commands`
**Why it matters**: Small in isolation, but the traceability guarantee is the paper's differentiator; the first untraceable number a skeptical reader finds discredits the guarantee.
**Suggestion**: Either have `risk_managed_eval` include the fragile agent in the emitted perturbation report, or cite the test by name (`cargo test -p sharpebench-harness perturb`) in Appendix A and mark the number as test-produced.
**Severity**: Minor | **Confidence**: 5 — verified by reading the committed records and the test source

### W9: Table 3's caption over-claims its coverage
**Problem**: The caption reads "Default configuration ... on every dataset" but the table shows seven rows spanning six of the nine datasets; FX majors, commodities, and rates are absent, as are the momentum and hold rows for most datasets. The omitted rows (all DSR at or near 0.000, commodities buy-and-hold worst-window drawdown 0.957) are in the committed records and do not contradict the text, but the caption promises a completeness the table does not deliver, and the 95.7 percent commodities drawdown would actually strengthen Finding 3.
**Evidence Anchor**: `table: Table 3 (tab:eligibility), sections/05-experiments.tex lines 59-77, caption "on every dataset" vs six datasets shown`
**Why it matters**: Selective presentation with an over-broad caption is exactly the reporting pattern the paper criticizes in rivals; the fix is trivial.
**Suggestion**: Either show all nine datasets (a compact version fits) or caption honestly: "the datasets with a nonzero DSR plus the daily/hourly zero rows; full records in paper/evidence/".
**Severity**: Minor | **Confidence**: 5 — direct comparison of caption against the table and the committed records

### W10: Headline findings are anchored on datasets that fail, or are barely long enough for, the paper's own realism battery
**Problem**: Finding 2's cleanest gate-separation case and Finding 4's central result both live on weekly US indices (522 bars), which fails the paper's own realism battery for shortness; the Section 5.6 examples use weekly crypto (307 bars, 46-bar windows) and rates (see W1). The paper discloses the realism verdicts, but never discusses whether anchoring its illustrative findings on the short/failed datasets weakens them, and it separately concedes the per-run PSR bar is weak at these lengths.
**Evidence Anchor**: `table: Table 1 (tab:data), US indices 1w row, "fail, short", 522 bars`
**Why it matters**: A 78-bar window PSR and a DSR at 0.984 on a dataset the benchmark itself flags as too short for aggregational tests invite the objection that the most-quoted numbers sit on the weakest data; the argument survives (the daily datasets tell the same eligibility story) but the paper should make that robustness explicit.
**Suggestion**: Add one sentence noting the eligibility conclusions are identical on the seven realism-passing datasets, so nothing rests on the failed two.
**Severity**: Minor | **Confidence**: 4 — general backtest-evaluation standards

### W11: The field-wide multiple-testing outputs are claimed but never shown
**Problem**: Section 3.1 states the Reality Check, SPA, and Romano-Wolf outputs "are computed and reported on every row", yet no value from any of the three appears anywhere in the paper, its tables, or its committed evidence records (the JSONL schema has no field for them).
**Evidence Anchor**: `absence: sections/05-experiments.tex and paper/evidence/final JSONL schema — expected at least one White/SPA/Romano-Wolf value or field; checked Table 3, analyze.py output schema, risk-managed.jsonl keys`
**Why it matters**: For a paper whose related-work section leans on these three procedures by name, showing them once (even as "SPA p across the field: x") would substantiate the claim that they are computed; as written the claim is unverifiable from the paper's own evidence chain.
**Suggestion**: Emit the three field-wide statistics into the sweep records and show them for one dataset, or reword to "computed by the scorer" with a pointer to the crate.
**Severity**: Minor | **Confidence**: 4 — multiple-testing expertise; absence verified in the committed schema

---

## Detailed Comments

### Research Questions & Hypotheses
The construct ("skill that survives deflation, reliability, process, and a bootstrap null") is stated precisely, and the eligibility predicate (Eq. 3) is an operational definition, which is rare and welcome. The four findings are framed as answerable questions and answered with committed data.

### Research Design
Gate-based eligibility rather than ranking sidesteps most ranking-uncertainty objections: since zero agents are eligible, no claimed ranking requires a CI. The design choice to report but not gate on White/SPA/Romano-Wolf is defensible (DSR handles selection within the field), but see W11. The grid design (64 configurations) is used descriptively, not for selection, and the paper's claims are grid-universal, which is the conservative direction.

### Sampling & Data
Nine datasets, hash-pinned, keyless, with a realism battery that can and does fail: good. Sample-size disclosures are honest (522 and 307 bars flagged). The single-symbol universe interacting with the fully-invested luck-floor definition is the one real design defect (W1). The 8-seed x 6-window run set gives pass^k 48 runs, but seeds vary only slippage, so pass^k is in practice a windows test for deterministic reference agents; the paper's bear-window explanation is consistent with that and the conclusion is unaffected.

### Analysis Methods
Eq. (1) matches Bailey and Lopez de Prado (2014) including the skewness/kurtosis denominator; Eq. (2) matches the expected-maximum term with the Euler-Mascheroni mixture; both were verified numerically. The units fix is correct up to the IID caveat (W7). The bootstrap is under-specified in the manuscript (W4). The measured-dispersion path needs an uncertainty statement (W6).

### Results Presentation
Traceability is the paper's outstanding feature and it substantially holds: I reproduced every number in Table 3, Finding 4, and Section 5.6's body text from the committed artifacts. The exceptions are the caption over-claims (W2, W9) and the one untraceable number (W8). Non-results are reported (the risk-managed agent's two whipsaw datasets, the two realism failures), which speaks against selective reporting.

### Reproducibility
Very strong: committed data with SHA-256 sidecars, committed reduction scripts, golden fixtures, cross-OS CI, a deterministic kernel, and a commands appendix. Remaining gaps: the unnamed final kernel version (W5) and the bootstrap parameters (W4). The claim that figures contain no typed-in numbers is true for the evidence figures (verified by reading `make-evidence-figures.py`).

### Methodological Fallacies Checklist
- Survivorship bias: not present; datasets are index/major-pair level and the gap (single names) is disclosed.
- P-hacking: no indicators; the grid is reported in full and the claims are universal over it.
- Overfitting: the paper is about detecting it; its own protocol (frozen data, no tuned parameters in the risk-managed agent) avoids it, though "no tuning" for the risk-managed agent is asserted, not demonstrable from the artifacts.
- Circularity watch: Finding 2's "this is the gate working, not failing" is an interpretation, not a result; the paper mostly frames it honestly as a design question (Section 7), which defuses the objection.

### Arithmetic recompute (Step 4a / #610, standard mode)
No statistic in the manuscript is covered by the four bounded procedures: there are no discrete-scale means or SDs (GRIM/GRIMMER not applicable), no reported test statistics with degrees of freedom (p_from_test_statistic not applicable; the bootstrap p is a resampling p without a test-statistic/df pair), and no df from which N could be inverted (n_from_df not applicable). Basis: checked every numeric claim in the abstract, Sections 3 and 5, Tables 1-3, and Appendix C. Beyond the bounded procedures, I recomputed Eq. (2)/Table 2 by hand (consistent to all printed digits) and reproduced Tables 3 and Finding 4 from the committed records via the committed script (consistent), with the discrepancies reported as W1, W2, W3, W8, W9 above.

---

## Questions for Authors

1. On single-symbol universes (rates-1d; partially commodities-1d), the fully-invested random-weight luck floor is deterministic and identical to buy-and-hold, as the committed records show. Was this understood when the falsification leg was written, and what does the luck floor look like after a redefinition that has genuine randomness on one symbol?
2. What are the bootstrap resample count, expected block length, and p-value convention, and how is the block length chosen as a function of series length?
3. Under the default every-regime verdict, the risk-managed agent on weekly US indices fails pass^k as well as deflation (passed_k = false in the committed record). Do you agree that the abstract's "refused solely on deflation after clearing every other gate" should be conditioned on the never-catastrophic verdict?
4. For the measured-dispersion path, what is the sensitivity of the near-bar DSR values (0.984, 0.978) to sigma_trials estimation error at field size eight, and would a jackknife over agents change any qualitative statement?

---

## Minor Issues

### Language / Grammar
- Abstract, one sentence contains two colons in sequence ("the benchmark declines to certify that owning the index is safe in a downturn: the market itself cannot pass it"); split for readability.

### Figures and Tables
- Table 2 caption: "The value measured from a real field was 0.070" reads as if one value applies to all datasets; 0.070 is the us-indices-1d field (weekly US is 0.034, hourly crypto 0.125 in the committed records). Attribute it to the dataset it came from.
- Table 3: consider adding the bootstrap_p column since Section 5.4 quotes it.

### Layout
- Appendix A, Table 3 caption, and Section 5.3 disagree on which kernel release produced the ablation columns (see W5).

---

## Criterion-Bound Judgements

Calibration status: `NOT_CALIBRATED`

| Dimension | Criterion source | Judgement | Evidence anchor(s) | Rationale | Uncertainty / scope limit | Decision bearing? |
|---|---|---|---|---|---|---|
| Methodological Rigor | methodology_reviewer_agent Steps 1-4; Bailey/Lopez de Prado deflation math | PARTLY_MEETS | equation: Eq. (2), sections/03-benchmark.tex; dataset: rates-1d.jsonl luck duplication | Core statistics correct and verified; luck-floor construction defective on single-symbol universes (W1); sqrt-time IID assumption unstated (W7) | none identified | yes: W1 requires re-analysis before the falsification-leg claim stands as written |
| Statistical Reporting Adequacy | references/statistical_reporting_standards.md §1, §6 | PARTLY_MEETS | absence: bootstrap spec (W4); text: sections/03-benchmark.tex measured-dispersion path (W6) | Point estimates fully traceable; bootstrap parameters and dispersion uncertainty unreported; effect-size/CI norms largely inapplicable because no ranking is claimed | APA-style checklist only partially applicable to a systems-benchmark paper | yes: W4/W6 are repairable in text |
| Evidence Sufficiency | Step 5 (Results Integrity) | MEETS | dataset: paper/evidence/final reduced by analyze.py | Every load-bearing number reproduced from committed artifacts; one untraceable number (W8) and two caption over-claims (W2, W9) | fragile-agent number test-produced, not evidence-produced | yes: repairable |
| Reproducibility | Step 6 | EXCEEDS | text: sections/08-reproducibility.tex; commands verified against evidence | Committed data, scripts, fixtures, cross-OS CI; only the version pin (W5) and bootstrap spec (W4) are gaps | I did not rebuild the Rust kernel; verification was at the committed-records layer | yes: supports acceptance after revision |
| Conclusions within data support | Step 5 | PARTLY_MEETS | text: abstract "clearing every other gate" (W3) | Body text is mostly exact; abstract and two captions state claims more broadly than the verdict path supports | none identified | yes: wording fixes |

Do not total, weight, average, or mechanically map these judgements to the recommendation.

### Recommendation rationale
The unresolved decision-bearing item is W1: the falsification leg's zero-skill control is not a control on one (partially two) of nine datasets, and the Section 5.6 rates example is invalid as stated; fixing it requires a redefinition and a rerun, which is re-analysis, hence Major Revision rather than Minor. Every other finding (W2-W11) is a text, caption, or specification repair. The statistical core, the units finding, and the traceability claim are verified and, once W1 and the phrasing issues are repaired, the paper's central claims are supportable by its own committed evidence.
