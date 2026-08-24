# Editorial Decision

## Manuscript Information
- **Title**: SharpeBench: The Luck-Robust Benchmark for AI Trading Agents
- **Manuscript ID**: sharpebench/paper (main.tex + sections/, working tree)
- **Submission Date**: not supplied (pre-submission panel)
- **Decision Date**: 2026-08-24
- **Review Round**: Round 1 (five-seat role-separated panel)

## Review Panel Provenance (#540/#740)

No replay-valid `review-panel-provenance/1.0` artifact was supplied to this synthesis. Per protocol, all axes render `unknown` and no independence claim is computed.

| Provenance axis | Status |
|---|---|
| Role-separated | unknown (role separation is asserted by the five report headers; not artifact-verified) |
| Within-panel invocation-context separation | unknown |
| Blind to peer outputs | unknown (R3 discloses "I have not read the other reviewers' reports"; not artifact-verified) |
| Model-family distinct | unknown |
| Provider distinct | unknown |
| Human-reviewer distinct | unknown |

- **Binary independence claim**: Not computed.
- **Correlated-error disclosure**: Model-family status is unknown; seats may share a model family and therefore correlated error processes. Corroboration counts below are counts of role-separated seats, not of independent error processes.

`calibration_status: NOT_CALIBRATED`

---

## Decision

### Major Revision

Re-review is required after revision.

---

## Blocking Issues (immutable source order)

| Transport ref | Blocking issue | Source reviewer(s) | Evidence anchor | Resolving roadmap item |
|---|---|---|---|---|
| B1 | A benchmark titled and framed "for AI trading agents" evaluates zero AI, LLM, or learned agents; every entrant is author-written | R0 W1, R3 W1, DA C1 (R2 corroborates in Significance) | `text: sections/05-experiments.tex "The field on every dataset is three reference agents ... plus a luck floor of five seeded random agents"`; `text: main.tex title` | R-01 |
| B2 | The zero-skill luck floor degenerates into an exact clone of buy-and-hold on the single-symbol rates dataset (partially commodities), so the falsification leg's control is not a control there and the Section 5.6 rates example is buy-and-hold in disguise | R1 W1 | `dataset: paper/evidence/final/rates-1d.jsonl, all five luck-floor agents raw_mean_return = 0.00036124370412967755, bit-identical to buy-and-hold (editor re-verified)` | R-12 |
| B3 | The abstract's "refused solely on deflation after clearing every other gate" contradicts the committed default-verdict record; the claim holds only under the never-catastrophic ablation verdict | DA C2, R1 W3 | `dataset: paper/evidence/final/risk-managed.jsonl, us-indices-1w, passed_k = false alongside psr 0.9998, bootstrap_p 0.0005, worst_run_drawdown 0.113 (editor re-verified)` | R-14, R-46 |

---

## Reviewer Summary

| Reviewer | Role | Recommendation | Confidence |
|---|---|---|---|
| R0 | Journal-Fit (NeurIPS D&B area chair persona) | Major Revision | 4 |
| R1 | Methodology (backtest-overfitting statistics) | Major Revision | 5 |
| R2 | Domain (benchmark design, evaluation integrity, citations) | Minor Revision | 4 |
| R3 | Perspective (practitioner, systematic fund) | Major Revision | 4 |
| DA | Devil's Advocate (fixed adversarial seat) | N/A, findings only | per finding |

Confidence values are transported self-reported scope disclosures; they carried no weight in this synthesis.

---

## Consensus Analysis

Consensus is computed across the four non-DA seats (R0, R1, R2, R3) per sub-claim. The DA is tracked separately and adjudicated below. A severity or action conflict routes to arbitration before any consensus label.

### Sub-Claim Inventory (Step 1b, condensed)

| sub_claim_id | Parent weakness | Positions (R0 / R1 / R2 / R3) | Disposition | Severity (transported) |
|---|---|---|---|---|
| SC-1 | No AI agent evaluated; claim-population mismatch | raised (W1) / not-mentioned / disputed on severity (Significance row: "caps enthusiasm, not acceptability") / raised (W1) | SPLIT, arbitrated below | Major (R0, R3); DA CRITICAL C1 |
| SC-2 | Title overclaims ("The", "AI") | raised (W4) / not-mentioned / corroborated (Detailed Comments) / corroborated (Detailed Comments) | CONSENSUS-3 (R1 silent) | Minor |
| SC-3 | Abstract "solely on deflation" is verdict-conditional | not-mentioned / raised (W3) / not-mentioned / not-mentioned | single-reviewer + DA CRITICAL C2, adjudicated below | Minor (R1); DA CRITICAL |
| SC-4 | Abstract double-colon sentence | raised / raised / raised / raised (each in Minor Issues) | CONSENSUS-4 | Minor |
| SC-5 | "three findings" vs four-findings count inconsistency | not-mentioned / not-mentioned / raised (Minor Issues) / not-mentioned | single-reviewer (+ DA N1 corroborates) | Minor |
| SC-6 | tab:eligibility caption over-claims coverage; row-selection rule unstated | raised (Minor Issues) / raised (W9) / not-mentioned / raised (Minor Issues) | CONSENSUS-3 (R2 silent) | Minor (R1); DA M8 rates Major |
| SC-7 | Luck-floor degeneracy on single-symbol universes | not-mentioned / raised (W1) / not-mentioned / not-mentioned | single-reviewer, editor-verified against committed records | Major |
| SC-8 | Forward arena described at length but never run; governance absent | raised (W3) / not-mentioned / not-mentioned / corroborated (Practical Impact) | corroborated finding (2 seats) | Major (R0) |
| SC-9 | Contribution framing straddles benchmark release vs self-audit report | raised (W2) / not-mentioned / not-mentioned / not-mentioned | single-reviewer (+ DA corroborates in criterion table) | Major |
| SC-10 | Rival re-scoring promised, delivered as unexecuted adapter | raised (W6) / not-mentioned / not-mentioned / not-mentioned | single-reviewer (+ DA M6 corroborates) | Minor (R0); DA M6 rates Major |
| SC-11 | Measured sigma_trials: small partly-duplicated field, no uncertainty (R1); Sybil gaming vector (R3); bar set by baseline zoo (DA M3) | not-mentioned / raised (W6) / not-mentioned / raised (W6) | corroborated finding (2 seats), related but distinct sub-claims kept as separate roadmap items | Minor (R1 W6); Major (R3 W6, DA M3) |
| SC-12 | sqrt-time scaling assumes IID, unstated; Lo 2002 uncited | not-mentioned / raised (W7) / corroborated (W4) / not-mentioned | corroborated finding (2 seats) | Minor |
| SC-13 | pass^k construct shift: windows measure regime dependence, not tau-bench reliability; seeds near-degenerate | not-mentioned / corroborated (Sampling & Data comment) / raised (W3) / not-mentioned | corroborated finding (2 seats) (+ DA M7) | Major (R2); DA M7 Major |
| SC-14 | Forward-evaluation prior art missing (M6, reusable holdout, Ladder, live arenas) | not-mentioned / not-mentioned / raised (W1) / corroborated (preregistration analogy, S4 and Cross-Disciplinary) | corroborated finding (2 seats) | Major |
| SC-15 | FinRL-Meta and InvestorBench missing from rival set | not-mentioned / not-mentioned / raised (W2) / not-mentioned | single-reviewer | Major |
| SC-16 | Load-bearing 2026-preprint citations and FinBen numbers unverified | corroborated (References comment) / not-mentioned / raised (W5) / not-mentioned | corroborated finding (2 seats) (+ DA N4) | Minor |
| SC-17 | Declared trial counts not incentive-compatible | not-mentioned / not-mentioned / not-mentioned / raised (W2) | single-reviewer (+ DA N6 corroborates) | Major |
| SC-18 | Empty eligible set needs a certification argument and a pass witness | not-mentioned / not-mentioned / not-mentioned / raised (W3) | single-reviewer (+ DA "jointly unsatisfiable" alternative and Unexamined Premise corroborate) | Major |
| SC-19 | Default all-weather reliability mandate is a design choice presented as a safety definition | not-mentioned / not-mentioned / not-mentioned / raised (W4) | single-reviewer | Major |
| SC-20 | Contamination, not multiple testing, is the binding threat for pretrained agents | not-mentioned / not-mentioned / not-mentioned / raised (W5) | single-reviewer | Major |

Remaining single-seat minor items (bootstrap spec, kernel version pin, fragile-agent traceability, realism-failing anchor datasets, field-wide test outputs, PSR provenance, table provenance, refs hygiene, capacity scoping, and copyedits) are carried directly into the roadmap without consensus labels; none has a conflicting position.

### Points of Agreement (Consensus)

**[CONSENSUS-4]**
1. The abstract contains a double-colon sentence ("...safe in a downturn: the market itself cannot pass it") that all four seats independently flagged for splitting (SC-4). All four also independently praised the same strengths: self-corrections reported as findings, the two-sided comparison tables, and number-to-artifact traceability. These strengths are genuine and are preserved in this decision's framing: the revision protects them.

**[CONSENSUS-3]**
1. The title's definite article and "AI" overclaim relative to the evidence (SC-2): R0 raised it (W4), R2 and R3 corroborated in their detailed comments; R1 is silent. Recommended fix is uncontested ("A Luck-Robust Benchmark", scope or drop "AI").
2. tab:eligibility's caption promises coverage the table does not deliver, and the row-selection rule is unstated (SC-6): R0, R1, and R3 agree; R2 is silent (R2 separately asks for a verdict-label gloss on the same caption). DA M8 corroborates and adds that the selection happened at paper-writing time since analyze.py prints all rows.

### Points of Disagreement

**Disagreement 1: Severity of the missing-AI-field problem (SC-1)**
- **R0 and R3 view**: Major and decision-bearing. R0: the promise-evidence mismatch is "the single largest threat to acceptance at this venue" (W1). R3: every headline claim is established only against agents the authors wrote (W1). DA C1 rates it CRITICAL.
- **R2 view**: Real but not decision-bearing: "impact is capped until an alpha-bearing agent competes ... no: caps enthusiasm, not acceptability" (Criterion-Bound Judgements, Significance row).
- **Disagreement type**: Severity disagreement.
- **Editor's Resolution**: Major, decision-bearing. Sustained at R0/R3's severity.
- **Resolution Rationale**: Expertise-first: venue fit and adoption are precisely R0's and R3's remits; R2's remit is literature and positioning, and R2's own scope note assigns the judgement outside its seat. Evidence-first: the manuscript's own inverse table records the empty "LLM agents" cell and Section 5 confirms the field composition. The concern is repairable by either a small LLM field or a retitle-and-reframe; both routes are in the roadmap (R-01).

**Disagreement 2: Severity of the "solely on deflation" claim (SC-3)**
- **R1 view**: Minor, a wording fix: qualify the abstract and Section 5.4 with the verdict condition; "the discrimination story is untouched" (W3).
- **DA view**: CRITICAL (C2): abstract and body make incompatible strength claims about the core thesis, and the discrimination claim additionally rests on one author-built agent at one grid point with no N-sensitivity.
- **Disagreement type**: Severity disagreement (DA vs scoring seat; adjudicated under the DA protocol below, recorded here because the arbitration decides the roadmap items).
- **Editor's Resolution**: The internal-inconsistency core is validated at blocking level (B3); the fix decomposes into a must-fix wording repair (R-14, cost: sentences) plus a should-fix sensitivity experiment (R-46).
- **Resolution Rationale**: I re-verified the committed record myself: `risk-managed.jsonl` us-indices-1w has `passed_k: false`, so as written the abstract states something the paper's own evidence contradicts, in the paper whose differentiator is that every number traces to committed evidence. That elevates it above copyedit regardless of how easy the fix is. R1's point that the discrimination story survives under the correct conditional phrasing is also right, which is why the repair is textual rather than structural; the DA's bundled sensitivity demand (N = 10, honest field size) is a legitimate evidence gap but not itself blocking.

**Disagreement 3: Severity of the eligibility-table presentation (SC-6)**
- **R1 view**: Minor; "the fix is trivial" (W9).
- **DA view**: Major (M8): cherry-picking pattern, 7 rows shown of 4,608 records under an "every dataset" caption.
- **Disagreement type**: Severity disagreement.
- **Editor's Resolution**: Minor severity transported from R1, with the DA's framing noted; must-fix obligation regardless.
- **Resolution Rationale**: Evidence-first: R1 verified that the omitted rows do not contradict the text and one (commodities drawdown 0.957) would strengthen Finding 3, so the selection is presentational, not outcome-steering. The caption is still false as written and this paper cannot afford the pattern it criticizes in rivals; hence must-fix at minor severity.

**Disagreement 4: Is the measured-dispersion path a live gaming vector or an estimation-quality issue? (SC-11)**
- **R1 view**: Estimation uncertainty at n = 8 with partial duplication; Minor because no eligibility verdict flips on it (W6).
- **R3 view**: A Sybil attack surface the per-submission audit architecture cannot see; Major (W6). DA M3 goes further: the bar is set by the author's own baseline zoo, a circularity.
- **Disagreement type**: Perspective difference (statistical vs mechanism-design lens), producing a severity spread.
- **Editor's Resolution**: Both stand as separate items: R-17 (uncertainty reporting, minor) and R-37 (Sybil mitigation plus a ninth self-audit attack, major). DA M3's circularity reading is carried in R-37's framing requirement.
- **Resolution Rationale**: These are different defects sharing one mechanism: R1's is about the honesty of the current numbers (bounded, since pass^k independently refuses), R3's is about the open-intake arena the paper aspires to (unbounded there). Expertise-first assigns each to its seat; neither displaces the other.

**Disagreement 5: Overall demandingness (R2's Minor vs the panel's Major)**
- **R2 view**: Everything in the domain remit is repairable by citations and framing text; Minor Revision.
- **R0, R1, R3 view**: Major Revision, each on decision-bearing findings in their own remits (venue fit and framing; a re-analysis-requiring control defect; construct and mechanism gaps).
- **Disagreement type**: Perspective difference, not an existence conflict: R2 does not dispute any other seat's finding; the remits barely overlap.
- **Editor's Resolution**: Major Revision.
- **Resolution Rationale**: The decision matrix for Minor + Major + Major + Major yields Major Revision, and independently, R1's W1 requires re-analysis (redefine the luck floor, rerun two datasets), which is definitionally Major. R2's recommendation is fully honored within its remit: every R2 item is a citation or framing repair and is classed accordingly in the roadmap.

### Devil's Advocate CRITICAL Adjudications

Per protocol, each DA CRITICAL is adjudicated visibly. A VALIDATED or unresolved CRITICAL blocks Accept.

**C1 (claim-population mismatch: benchmark "for AI Trading Agents" evaluates zero AI agents): VALIDATED.**
- DA's argument: every empirical claim about "agents" is evidence only about author-written deterministic rules; title, abstract, and contribution 1 do not scope down; conclusions about the target population are unsupported by the sample.
- Corroboration: R0 W1 (Major) and R3 W1 (Major) raise the same defect independently; R2 acknowledges the cap on significance. This is the panel's most corroborated finding (3 of 4 scoring seats plus the DA).
- Editor's assessment: Valid. The manuscript's own Section 5 field description, inverse-table empty cell, and first limitation confirm the factual basis; the title and abstract's first sentence address "AI trading agents" without scoping. The paper's transparency about the gap does not close it.
- Required author response: R-01. Either populate a minimal LLM/learned-agent field or retitle and reframe (protocol-and-kernel paper, AI field as future work). Blocks Accept until one route is executed.

**C2 (abstract asserts "refused solely on deflation after clearing every other gate"; committed evidence shows the default verdict also fails pass^k; discrimination claim rests on one agent at one grid point): VALIDATED** (core claim; the bundled sensitivity sub-claim is sustained as an evidence gap, not a blocker).
- DA's argument: abstract and body make incompatible strength claims; Section 5.4 lists cleared gates and is silent on default pass^k; no sensitivity at N = 10 or the honest field size.
- Corroboration: R1 W3 raises the identical core defect independently with the same evidence anchor.
- Editor's assessment: Valid, verified first-hand: `paper/evidence/final/risk-managed.jsonl` us-indices-1w reads `passed_k: false` with PSR 0.9998, bootstrap_p 0.0005, worst_run_drawdown 0.113, and `eligible_never_catastrophic: false` (failing deflation alone under that verdict). The abstract's unconditional phrasing is contradicted by the committed record under the shipped default. The DA's further point that one author-built agent at one configuration cannot fully separate "gates discriminate" from "predicate jointly unsatisfiable" is sound and is answered by the pass-witness and sensitivity items (R-34, R-46).
- Required author response: R-14 (must-fix wording repair in abstract and Section 5.4), R-46 (N-sensitivity), R-34 (pass witness).

`da_critical_adjudications: [C1=VALIDATED, C2=VALIDATED]`

Both validated CRITICALs block Accept; both are repairable, consistent with Major Revision rather than Reject.

---

## Decision Rationale

The recommendation matrix (one Minor, three Major) yields Major Revision, and three independent grounds each suffice on their own. First, the panel's most corroborated finding (R0 W1, R3 W1, DA C1, with R2 acknowledging the significance cap) is a claim-population mismatch: a benchmark titled for AI trading agents evaluates none, so the delivered evidence exercises the gate structure only on author-written baselines. Second, R1's independently verified W1 shows the zero-skill control degenerates into buy-and-hold on the single-symbol rates dataset, which invalidates the falsification-leg claim as written and the Section 5.6 rates example; the repair requires redefinition and a rerun, which is re-analysis by definition. Third, both DA CRITICALs are validated, including an abstract claim ("refused solely on deflation after clearing every other gate") that this synthesis re-verified against the committed record and found contradicted under the default verdict, in a paper whose differentiator is exact number-to-artifact traceability.

Reject was not seriously in contention: no seat recommended it, the statistical core is independently verified correct (R1 reproduced every load-bearing number), the originality of the gate composition is affirmed by three seats, and every defect found is repairable by reframing, added citations, targeted text, or cheap experiments the deterministic kernel makes tractable. Minor Revision is excluded because R1 W1 requires re-analysis and because two validated DA CRITICALs and the arena/population gaps go to the paper's central promise, not its edges. The panel's unanimous praise for the paper's honesty is real; the revision should convert that honesty into aligned claims.

---

## Required Revisions (Must Fix)

Transport refs follow immutable roadmap source order filtered to `obligation_class == must_fix`. Full details, including the text-vs-experiment split and concrete fixes, are in `revision-roadmap.md`; the table below is the letter-side index.

| Transport ref | Revision Item | Sub-Claim(s) | Severity | Evidence Anchor | Confidence | Source | Obligation class | Cost scope | Bounded consequence |
|---|---|---|---|---|---|---|---|---|---|
| R-01 | Close the claim-population mismatch: run a minimal LLM/learned-agent field, or retitle and reframe as protocol-and-kernel | SC-1 | Major (DA: critical) | `text: sections/05-experiments.tex field description; main.tex title` | 5 (R0), 4 (R3, DA) | R0/R3/DA | must_fix | new_data or section: title, abstract, sections/01, 05 | claim_unsupported: title population |
| R-03 | Mark the forward arena as designed-and-tested but never operated in abstract and introduction; add governance sentence (or run one real forward window) | SC-8 | Major | `text: sections/07-limitations.tex "rather than as a running service"` | 4 | R0 | must_fix | sentence: abstract, sections/01, 04 (or new_data: one forward window) | claim_overstated: operational status |
| R-04 | Retitle: drop the definite article; scope or drop "AI" | SC-2 | Minor | `text: main.tex title` | 5 | R0 (+R2, R3) | must_fix | sentence: main.tex title | claim_overstated: uniqueness and population |
| R-08 | Fix tab:eligibility caption: state the row-selection rule, drop "on every dataset" or show all nine datasets; add verdict-label gloss | SC-6 | Minor (DA: major) | `table: sections/05-experiments.tex tab:eligibility caption vs rows` | 5 | R0/R1/R3 (+R2 gloss, DA M8) | must_fix | sentence: tab:eligibility caption or section: table | claim_overstated: coverage |
| R-12 | Redefine the luck floor to be non-degenerate on single-symbol universes; rerun rates-1d and commodities-1d; repair Section 5.6 rates example and the dispersion-measurement path | SC-7 | Major | `dataset: paper/evidence/final/rates-1d.jsonl luck-floor duplication (editor re-verified)` | 5 | R1 | must_fix | re_analysis: kernel luck-floor definition + evidence/final rerun + sections/05 | result_invalid: falsification-leg claim on affected datasets |
| R-13 | Correct figure (b) caption: "under 0.43" holds only at the default (measured-dispersion) configuration; full-grid maximum is 0.623 | SC (R1 W2) | Minor | `figure: fig (b) caption vs crypto-majors-1w.jsonl pinned cells` | 5 | R1 | must_fix | sentence: sections/05-experiments.tex caption | claim_false_as_written: caption bound |
| R-14 | Condition the abstract and Section 5.4 opening: "under the never-catastrophic reliability verdict, refused solely on deflation"; state that the default verdict also fails pass^k | SC-3 | Minor (DA: critical, validated) | `dataset: risk-managed.jsonl us-indices-1w passed_k=false (editor re-verified)` | 5 | R1/DA | must_fix | sentence: sections/00-abstract.tex, sections/05 sec:riskmanaged | claim_contradicted_by_evidence: abstract |
| R-15 | Specify the bootstrap: resample count, expected block length and its scaling, p-value convention, seed | SC (R1 W4) | Minor | `absence: sections/03-benchmark.tex Statistics paragraph` | 5 | R1 | must_fix | sentence: sections/03-benchmark.tex | spec_incomplete: gate of record |
| R-16 | Pin the exact kernel version/commit that produced evidence/final; reconcile Table 3 caption with Appendix A | SC (R1 W5) | Minor | `text: sections/A-commands.tex "are in the release after it"` | 4 | R1 | must_fix | sentence: sections/A-commands.tex, tab:eligibility caption | reproducibility_gap: version of record |
| R-24 | Add forward-and-adaptive-evaluation positioning: M6, reusable holdout (Dwork 2015), The Ladder (Blum-Hardt 2015), live LLM-fund arenas; state what the chained board adds over each | SC-14 | Major | `absence: sections/06-related.tex forward-evaluation lineage` | 4 | R2 (+R3 analogy) | must_fix | section: sections/06-related.tex paragraph | novelty_overstated: forward attestation |
| R-25 | Add FinRL-Meta and InvestorBench to the comparison tables under the marks-only-what-they-claim protocol, or scope them out explicitly | SC-15 | Major | `absence: sections/06-related.tex, tab:related` | 4 | R2 | must_fix | section: sections/06-related.tex + tables | gap_argument_incomplete: rival survey |
| R-26 | Separate the pass^k construct: seeds test execution reliability (tau-bench sense), windows test regime robustness; cite tau-bench for the former only; rename the every-window requirement | SC-13 | Major | `text: sections/03-benchmark.tex "Following pass^k..."` | 4 | R2 (+R1, DA M7) | must_fix | section: sections/03-benchmark.tex + sentences in Findings 2-3 | construct_misattributed: reliability gate |
| R-33 | Add an incentives subsection for declared trial counts: threat model, at minimum state that declared-N is advisory and binding deflation comes from field size; candidate mitigations | SC-17 | Major | `text: sections/03-benchmark.tex "each agent's declared in-sample trials"` | 4 | R3 (+DA N6) | must_fix | section: sections/03 or 04 subsection | mechanism_unanalyzed: central gate input |
| R-34 | Construct a synthetic pass witness (controlled injected edge through the full harness) demonstrating the acceptance region is nonempty, and reposition the near-term artifact as a diagnostic auditor | SC-18 | Major | `text: sections/05-experiments.tex "zero agents are eligible at any of the 576 cells"` | 5 | R3 (+DA alternative 1) | must_fix | new_data: synthetic-edge experiment + section: sections/05, 07 | vacuity_unresolved: certification claim |
| R-35 | Justify the all-weather default against mandate-relative alternatives (one paragraph minimum; mandate declaration at submission preferred) | SC-19 | Major | `text: sections/05-experiments.tex "profitable with 90 percent confidence in every regime"` | 4 | R3 | must_fix | section: sections/03 or 05 paragraph | default_unjustified: reliability gate |
| R-36 | Make backtest-mode verdicts for pretrained models advisory-only as a protocol rule; state trial-count semantics for pretrained policies | SC-20 | Major | `text: sections/C-simdata.tex masking defense` | 3 | R3 | must_fix | section: sections/03/04 protocol rule + sections/07 | threat_model_mismatch: contamination |
| R-37 | Close the Sybil vector on the measured-dispersion path: floor the measured dispersion, count distinct verified commitments, add a ninth self-audit attack; acknowledge the field-composition circularity | SC-11 | Major | `text: sections/07-limitations.tex "a field can in principle be assembled to move it"` | 4 | R3 (+DA M3) | must_fix | re_analysis: self-audit attack + section: sections/04, 07 | gaming_vector_open: deflation bar |
| R-42 | Revise the determinism/self-audit value claim: the audit tests gate-evasion, not gate-calibration; determinism guarantees replicability, not validity | DA M1 | Major | `text: sections/02-principles.tex "wrong in exactly the same way everywhere"` | 5 | DA | must_fix | section: sections/02, 04 sentences | authority_overclaimed: integrity story |
| R-43 | Scope the "market itself cannot pass" claim as sample-period-contingent (frozen ranges contain 2020 and 2022), or add a truncated-range check | DA M2 | Major | `text: sections/05-experiments.tex sec:passk; tab:data Range column` | 4 | DA | must_fix | sentence: sections/00, 05 (or re_analysis: truncated-range rerun) | claim_overstated: structural vs sample property |
| R-44 | Scope "nine datasets" claims by effective independence: resampled timeframes and co-moving symbols give roughly five independent panels | DA M4 | Major | `table: tab:data; text: "zero exceptions in nine"` | 4 | DA | must_fix | sentence: sections/05, 07 | evidence_overcounted: dataset independence |

## Suggested Revisions (Should Fix / Consider)

| Transport ref | Revision Item | Sub-Claim(s) | Severity | Source | Obligation class |
|---|---|---|---|---|---|
| R-02 | Reframe Finding 1 as a general protocol pitfall; commit the abstract's first two sentences to one primary claim | SC-9 | Major | R0 | should_fix |
| R-05 | Restructure the abstract (ordering, density, split the double-colon sentence) | SC-4 | Minor | R0/R1/R2/R3 | should_fix (colon split: must-do copyedit) |
| R-06 | Run the StockBench reproduction path or soften the adapter framing to contributed infrastructure with no demotion claim | SC-10 | Minor (DA: major) | R0/DA | should_fix |
| R-07, R-09, R-10, R-11 | Renaissance citation; duplicate table label; rhetorical-flourish pass; short closing section | copyedit bundle | Minor | R0 | consider |
| R-17 | Report field size and a jackknife interval for measured sigma_trials; soften "true verdict"; minimum-distinct-streams rule | SC-11 | Minor | R1 | should_fix |
| R-18 | State the IID assumption behind sqrt-time scaling; cite Lo 2002 | SC-12 | Minor | R1/R2 | should_fix |
| R-19 | Make the fragile-agent perturbation number traceable (emit it, or cite the producing test in Appendix A) | R1 W8 | Minor | R1 | should_fix |
| R-20 | Note eligibility conclusions are identical on the seven realism-passing datasets | R1 W10 | Minor | R1 | should_fix |
| R-21 | Show or point to the field-wide White/SPA/Romano-Wolf outputs (editor note: `field_reality_check_p` and `step_down_significant` already exist in the committed sweep records) | R1 W11 | Minor | R1 | should_fix |
| R-22, R-23 | Attribute the 0.070 to its dataset in Table 2's caption; consider a bootstrap_p column in Table 3 | R1 minors | Minor | R1 | consider |
| R-27 | Cite Bailey and Lopez de Prado 2012 for Eq. (1); consider Kosowski 2006 and Harvey-Liu-Zhu 2016 | SC (R2 W4) | Minor | R2 | should_fix |
| R-28 | Add exact source locators for the four load-bearing 2026/FinBen citation claims; confirm the +/- semantics | SC-16 | Minor | R2/R0/DA | should_fix |
| R-29 | Per-cell provenance for the comparison-table marks; verify the empty Costs cells for StockBench and QuantBench | R2 W6 | Minor | R2 | should_fix |
| R-30 | refs.bib hygiene (deza2021 entry type, kapoor key/year, uncited entries, FinBen proceedings version) | R2 W7 | Minor | R2 | should_fix |
| R-31 | Fix the "three findings" vs four count in Appendix B and sec:claims | SC-5 | Minor | R2/DA | should_fix |
| R-38 | Scope paragraph: eligibility certifies edge existence under stylized execution, not deployability; report the worst-case cost profile beside typical | R3 W7 | Minor | R3 | should_fix |
| R-39, R-40, R-41 | Repetition trim; tau-bench row caption clause; table placement | R3 minors | Minor | R3 | consider |
| R-45 | Simulate the luck floor at scale (order of a thousand random agents) to match the introduction's motivating claim | DA M5 | Major | DA | should_fix |
| R-46 | Report N-sensitivity for the risk-managed agent (N = 10 and honest field size alongside N = 50) | DA C2 bundle | Major | DA | should_fix |
| R-47, R-48, R-49 | Rates yield-as-price construct note; WTI outlier impact note; scope "any platform" to the tested matrix | DA N2/N3/N5 | Minor | DA | consider |

---

## Revision Roadmap

### Source-traceability checklist

The complete, source-ordered, non-ranked roadmap with concrete fixes and the TEXT vs NEW EXPERIMENT split is in `revision-roadmap.md` (49 items, R-01 through R-49). Order is immutable source order (R0, R1, R2, R3, DA; report order within seat); it is not a work ranking.

- [ ] R-01 must_fix, R-03 must_fix, R-04 must_fix, R-08 must_fix (R0-sourced blockers and caption/title repairs)
- [ ] R-12 must_fix, R-13 must_fix, R-14 must_fix, R-15 must_fix, R-16 must_fix (R1-sourced evidence and specification repairs)
- [ ] R-24 must_fix, R-25 must_fix, R-26 must_fix (R2-sourced literature and construct repairs)
- [ ] R-33 must_fix, R-34 must_fix, R-35 must_fix, R-36 must_fix, R-37 must_fix (R3-sourced mechanism and certification repairs)
- [ ] R-42 must_fix, R-43 must_fix, R-44 must_fix (DA-sourced scoping repairs)
- [ ] All remaining should_fix / consider items per `revision-roadmap.md`

---

## Journal-Supplied Deadline (Optional Transport)

- **Exact deadline from source letter**: NOT PROVIDED

---

## Response Letter Instructions

Please respond item by item using the ids in `revision-roadmap.md` (R-01 through R-49): for each must_fix, the change made and where; for each should_fix/consider, adopted or the reason not; mark all changes in the revised manuscript. The revised manuscript will undergo re-review.

---

## Closing

We encourage you to carefully consider the reviewers' comments and submit a substantially revised manuscript. The panel was unanimous that the artifact quality, traceability discipline, and honesty about self-corrections are well above the norm for this genre; the revisions requested align the paper's claims with that evidence rather than asking for new virtues. Please note that the revised manuscript will undergo another round of review.

---

## Part 3: Reviewer Report Summary (Appendix)

### Journal-Fit Reviewer (R0)
- Recommendation: Major Revision | Confidence: 4
- Key Point: Near-exemplary D&B artifact undercut by a promise-evidence mismatch: no AI agent evaluated, no forward window run, no rival re-scored; repairable by populating the field or reframing.

### Reviewer 1 (Methodology)
- Recommendation: Major Revision | Confidence: 5
- Key Point: Statistical core and traceability independently verified correct, but the zero-skill luck floor degenerates into buy-and-hold on single-symbol universes (re-analysis required) and several claims are phrased more broadly than the evidence path supports.

### Reviewer 2 (Domain)
- Recommendation: Minor Revision | Confidence: 4
- Key Point: Citation core verified accurate and positioning unusually fair; literature gaps (forward evaluation, FinRL-Meta/InvestorBench, PSR provenance) and the pass^k construct shift are repairable with citations and framing text.

### Reviewer 3 (Perspective)
- Recommendation: Major Revision | Confidence: 4
- Key Point: Engineering integrity is excellent, but the artifact today is a refusal engine with an unproven acceptance region, an honor-system trial declaration, and an open Sybil vector; needs a pass witness, an incentives analysis, and honest near-term repositioning.

### Devil's Advocate
- Recommendation: N/A, findings only
- Key Challenge: Universal refusal is compatible with "gates discriminate" and with "predicate jointly unsatisfiable at the shipped defaults on this decade's windows", and the paper's evidence, built entirely from author-written agents, cannot yet separate the two. Both CRITICALs VALIDATED.
