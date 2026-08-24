# Revision Roadmap

Manuscript: SharpeBench: The Luck-Robust Benchmark for AI Trading Agents
Companion to: `editorial-decision.md` (Decision: Major Revision)

Ordering is immutable source order: R0 (journal fit), R1 (methodology), R2 (domain), R3 (perspective), DA (devil's advocate), in each report's own order. It is NOT a work ranking and implies no priority. Severity is transported from the source seat; where seats disagree the spread is shown. Every item traces to a specific Phase 1 report; no item is editor-invented.

Fix-type legend:
- **TEXT**: fixable by reframing, caveats, citations, or wording accuracy alone.
- **NEW EXPERIMENT**: requires producing new evidence; the concrete command or experiment is stated.
- **TEXT or NEW EXPERIMENT**: reviewer offered both routes; either discharges the item.

---

## R0-sourced (Journal-Fit)

### R-01 | Close the claim-population mismatch (no AI agent evaluated)
- **Source seat(s)**: R0 W1 (raised) + R3 W1 (raised) + DA C1 (VALIDATED) + R2 Significance row (corroborated, lower decision bearing)
- **Severity**: Major (R0, R3); CRITICAL (DA C1) | must_fix | Blocking issue B1
- **Location**: main.tex title; sections/00-abstract.tex first sentence; sections/01-introduction.tex Contributions; sections/05-experiments.tex field description
- **What to change**: The paper promises a benchmark for AI trading agents and evaluates none; every entrant is author-written.
- **Fix**: **NEW EXPERIMENT (preferred)**: run two or three open LLM agents (rival boards' own baseline agents suffice) through the existing stdin/HTTP harness contract on one or two datasets and report where each is refused and why. Experiment: wire each agent to the harness agent contract, then run the Appendix A sweep command over the chosen datasets and reduce with `python paper/evidence/analyze.py`. **TEXT (alternative)**: retitle and reframe as an evaluation-protocol-and-kernel paper validated on reference agents, with the AI field as the stated outstanding experiment; scope every "agents" claim accordingly.

### R-02 | Commit to one primary claim; reframe Finding 1 as a general protocol pitfall
- **Source seat(s)**: R0 W2 (raised); DA criterion table corroborates ("two findings are bug fixes counted as contributions")
- **Severity**: Major | should_fix
- **Location**: sections/00-abstract.tex first two sentences; sections/01-introduction.tex; sections/05-experiments.tex sec:units; version-history detail to appendix
- **What to change**: The paper straddles "benchmark release" and "audit of our own benchmark"; two of four headline findings are internal bug fixes presented at the same rank as market-facing findings.
- **Fix**: **TEXT**. State in the abstract's opening whether the primary claim is the protocol or the empirical refusal result. Recast Finding 1 as the general trap (annualized-vs-per-period dispersion produces benchmarks that look admirably strict, the mirror image of the rivals' error), currently one sentence in Sec. 5.2; move version-history detail to the appendix.

### R-03 | Mark the forward arena as designed but never operated; add governance plan
- **Source seat(s)**: R0 W3 (raised) + R3 Practical Impact / Significance row (corroborated)
- **Severity**: Major | must_fix | Blocking-adjacent
- **Location**: sections/00-abstract.tex; sections/01-introduction.tex; sections/04-integrity.tex; sections/07-limitations.tex
- **What to change**: A substantial fraction of the integrity section describes infrastructure with no real forward window; the abstract features the arena without distinguishing design contribution from evidence contribution; no governance plan (neutral hosting, key custody, disputes) for a single-author self-hosted board.
- **Fix**: **TEXT (minimum)**: state in abstract and introduction that the arena is designed-and-tested but not yet operated; add a governance-plan sentence. **NEW EXPERIMENT (stronger)**: run one short operator-driven real forward window (weekly bars, small field) before submission and report it.

### R-04 | Retitle: drop "The", scope or drop "AI"
- **Source seat(s)**: R0 W4 (raised) + R2 Detailed Comments (corroborated) + R3 Detailed Comments (corroborated); CONSENSUS-3 (R1 silent)
- **Severity**: Minor | must_fix
- **Location**: main.tex title
- **What to change**: The definite article claims uniqueness and "AI" claims a field the paper does not have.
- **Fix**: **TEXT**. "SharpeBench: A Luck-Robust Benchmark for Trading Agents" or "...A Luck-Robust Evaluation Protocol for Trading Agents" (align with the R-01 route chosen).

### R-05 | Restructure the abstract; split the double-colon sentence
- **Source seat(s)**: R0 W5 (raised) + R1, R2, R3 Minor Issues (all flag the double-colon sentence); colon split is CONSENSUS-4
- **Severity**: Minor | should_fix (colon split itself: do it)
- **Location**: sections/00-abstract.tex
- **What to change**: One dense paragraph buries the strongest result mid-paragraph; the sentence "...safe in a downturn: the market itself cannot pass it" carries two colons.
- **Fix**: **TEXT**. Reorder: problem, construct, headline finding, discrimination result, integrity inventory in one clause, open attainability question stated plainly. Split the double-colon sentence. Define "luck floor" in one clause at first use.

### R-06 | Deliver or soften the rival re-scoring experiment
- **Source seat(s)**: R0 W6 (raised) + DA M6 (corroborated)
- **Severity**: Minor (R0); Major (DA M6) | should_fix
- **Location**: sections/06-related.tex import-adapter passage
- **What to change**: The "one command" import experiment is built up and then reported as not run; the "their leaders would not survive these gates" implication remains an insinuation.
- **Fix**: **NEW EXPERIMENT (preferred)**: run the StockBench reproduction path (their public harness on their window, export per-period returns through the adapter, score with the SharpeBench kernel; the adapter command the paper already documents). **TEXT (alternative)**: state the adapter is contributed infrastructure and that no rival demotion is claimed.

### R-07 | Cite or cut the Renaissance/Medallion comparison
- **Source seat(s)**: R0 Minor Issues (Citation Format)
- **Severity**: Minor | consider
- **Location**: sections/05-experiments.tex Sec. 5.2
- **Fix**: **TEXT**. Add a citation for "commonly cited near 2 to 3" or delete the sentence.

### R-08 | Fix tab:eligibility caption: row-selection rule, coverage claim, verdict-label gloss
- **Source seat(s)**: R0 Minor Issues (raised) + R1 W9 (raised) + R3 Minor Issues (raised) + R2 Minor Issues (verdict-label gloss) + DA M8; CONSENSUS-3 on the core (R2 partially)
- **Severity**: Minor (R1: "fix is trivial"); Major band (DA M8) | must_fix
- **Location**: sections/05-experiments.tex tab:eligibility caption and rows
- **What to change**: Caption says "on every dataset" over seven rows spanning six of nine datasets; row-selection rule unstated; "under either reliability verdict" not linked to the column labels "Every regime"/"Never catastrophic". R1 verified the omitted rows do not contradict the text and the commodities 0.957 drawdown would strengthen Finding 3.
- **Fix**: **TEXT**. Either show all nine datasets compactly, or caption honestly ("datasets with a nonzero DSR plus the daily/hourly zero rows; full records in paper/evidence/"); state the row-selection rule; add one clause mapping column labels to the two verdicts.

### R-09 | Resolve the duplicate data-table label
- **Source seat(s)**: R0 Minor Issues (Figures and Tables)
- **Severity**: Minor | consider
- **Location**: Sec. 3 tab:data and Appendix C tab:data
- **Fix**: **TEXT**. Confirm the labels do not collide; cross-reference instead of repeating (also relieves the table-heavy page budget R0 notes).

### R-10 | Light pass on rhetorical flourishes
- **Source seat(s)**: R0 Minor Issues (Language) + R3 Minor Issues (repetition, see R-39)
- **Severity**: Minor | consider
- **Location**: sections/05-experiments.tex Sec. 5.2 and similar ("The bar was not high. It was unreachable", "Give a thousand random agents...")
- **Fix**: **TEXT**. Keep a few; trim the rest toward venue median register.

### R-11 | Add a short closing section
- **Source seat(s)**: R0 Detailed Comments (Structural Coherence)
- **Severity**: Minor | consider
- **Location**: after sections/07-limitations.tex
- **Fix**: **TEXT**. A brief conclusion so Sec. 5.7 does not carry the closing weight alone.

---

## R1-sourced (Methodology)

### R-12 | Redefine the luck floor for single-symbol universes and rerun affected datasets
- **Source seat(s)**: R1 W1 (raised); single-seat but editor-verified against committed records (all five luck agents on rates-1d share buy-and-hold's raw_mean_return 0.00036124370412967755 bit-identically)
- **Severity**: Major | must_fix | Blocking issue B2
- **Location**: luck-floor definition (kernel + sections/05-experiments.tex line 3); Section 5.6 rates example (DSR 0.993 at N=1); measured-dispersion path in sections/03-benchmark.tex; paper/evidence/final/rates-1d.jsonl and commodities-1d.jsonl
- **What to change**: Fully-invested random weights on a one-symbol universe are deterministically buy-and-hold, so the zero-skill control is not a control there; the falsification-leg "zero exceptions in nine" is satisfied on rates-1d only as a tie between clones; measured sigma_trials there is estimated from effectively three distinct streams with six duplicates.
- **Fix**: **NEW EXPERIMENT**. Redefine the luck floor to have genuine randomness on every universe (random gross exposure in [-1, 1], or random long/flat/short per period on single-symbol universes); rerun the sweep on rates-1d and commodities-1d (the Appendix A sweep command for those datasets, reduced by `python paper/evidence/analyze.py`); replace the Section 5.6 rates example with a genuinely random-floor dataset (weekly crypto serves) or state the degeneracy and exclude single-symbol datasets from the luck-floor claim; add a deduplication/distinct-streams guard to the measured-dispersion path.

### R-13 | Correct the figure (b) caption bound
- **Source seat(s)**: R1 W2 (raised)
- **Severity**: Minor | must_fix
- **Location**: sections/05-experiments.tex line 112, figure (b) caption
- **What to change**: "under 0.43 everywhere by N=50" is false over the full grid; the full-grid maximum at N=50 is 0.623 (crypto-majors-1w, pinned annualized prior); 0.423 holds only on the measured-dispersion path that the figure actually plots.
- **Fix**: **TEXT**. Caption: "under 0.43 at the default configuration (measured dispersion) by N=50", or extend the figure to pinned-prior cells and report 0.623.

### R-14 | Condition the "refused solely on deflation" claim on the verdict
- **Source seat(s)**: R1 W3 (raised) + DA C2 (VALIDATED); editor re-verified `passed_k: false` in the committed us-indices-1w record
- **Severity**: Minor (R1); CRITICAL (DA C2, validated) | must_fix | Blocking issue B3
- **Location**: sections/00-abstract.tex; sections/05-experiments.tex sec:riskmanaged opening sentence
- **What to change**: Under the default every-regime verdict the risk-managed agent fails pass^k as well as deflation; "solely on deflation after clearing every other gate" is true only under the never-catastrophic verdict.
- **Fix**: **TEXT**. Abstract and Sec. 5.4: "under the never-catastrophic reliability verdict, refused solely on deflation"; state explicitly that under the default verdict it fails both pass^k and deflation. The discrimination story survives the corrected phrasing. (Sensitivity companion: R-46.)

### R-15 | Specify the bootstrap in the document of record
- **Source seat(s)**: R1 W4 (raised)
- **Severity**: Minor | must_fix
- **Location**: sections/03-benchmark.tex Statistics paragraph (Sec. 3.1)
- **Fix**: **TEXT**. One sentence: resample count (committed value implies B = 2000 with the (r+1)/(B+1) convention), expected block length and its scaling with n (e.g. Politis-White), p-value convention, and the seed.

### R-16 | Pin the final kernel version; reconcile Table 3 caption with Appendix A
- **Source seat(s)**: R1 W5 (raised)
- **Severity**: Minor | must_fix
- **Location**: sections/A-commands.tex ("are in the release after it"); tab:eligibility caption ("kernel v0.3.0")
- **Fix**: **TEXT**. Name the exact tag or commit hash that produced `paper/evidence/final/` and the risk-managed evaluation; make the caption and appendix agree.

### R-17 | Report uncertainty for the measured sigma_trials
- **Source seat(s)**: R1 W6 (raised) + DA M3 framing (bar set by the baseline zoo); see also R-37 for the R3 mechanism side
- **Severity**: Minor (R1; bounded because no eligibility flips on it) | should_fix
- **Location**: sections/03-benchmark.tex measured-dispersion paragraph; near-bar narrative values in Sec. 5 (0.984, 0.978 vs beta = 0.95)
- **Fix**: **TEXT (minimum)**: report field size and soften "the true verdict of the statistic" to acknowledge estimation error at n = 8. **NEW EXPERIMENT (better)**: jackknife over agents per dataset (extend `paper/evidence/analyze.py` to drop-one-agent recomputation of sigma_trials and the resulting DSR interval); add a minimum-distinct-streams requirement for the measured path (composes with R-12's dedup guard).

### R-18 | State the IID assumption behind sqrt-time scaling; cite Lo 2002
- **Source seat(s)**: R1 W7 (raised) + R2 W4 (corroborated: Lo 2002 is the direct ancestor of Finding 1)
- **Severity**: Minor | should_fix
- **Location**: sections/03-benchmark.tex (the sqrt(periods per year) conversion); Finding 1 in sections/05-experiments.tex; sections/06-related.tex
- **Fix**: **TEXT**. One sentence noting Sharpe ratios scale with sqrt(time) only under IID returns, that the paper's own realism battery documents volatility clustering, and that the measured-dispersion path avoids the assumption; cite Lo (2002), "The Statistics of Sharpe Ratios", FAJ 58(4).

### R-19 | Make the fragile-agent perturbation number traceable
- **Source seat(s)**: R1 W8 (raised)
- **Severity**: Minor | should_fix
- **Location**: sections/05-experiments.tex Sec. 5.5 ("more than twice the reference spread"); sections/A-commands.tex mapping for sec:perturb
- **What to change**: The number exists only inside a unit test; the committed perturbation report contains no fragile-agent row, violating the paper's every-number-from-a-listed-command contract.
- **Fix**: **TEXT or small evidence change**. Either have `risk_managed_eval` emit the fragile agent into the perturbation report (then it appears in `risk-managed.jsonl`), or cite the producing test in Appendix A (`cargo test -p sharpebench-harness perturb`) and mark the number as test-produced.

### R-20 | State robustness of eligibility conclusions to the realism-failing datasets
- **Source seat(s)**: R1 W10 (raised)
- **Severity**: Minor | should_fix
- **Location**: sections/05-experiments.tex (Findings 2 and 4, anchored on weekly US indices, 522 bars, realism-battery fail-short)
- **Fix**: **TEXT**. One sentence noting the eligibility conclusions are identical on the seven realism-passing datasets, so nothing rests on the failed two.

### R-21 | Show or correctly describe the field-wide multiple-testing outputs
- **Source seat(s)**: R1 W11 (raised)
- **Severity**: Minor | should_fix
- **Location**: sections/03-benchmark.tex Sec. 3.1 ("computed and reported on every row"); sections/05-experiments.tex tables
- **Editor evidence note**: the committed sweep records DO carry `field_reality_check_p` and `step_down_significant` fields (verified in `paper/evidence/final/rates-1d.jsonl` schema), which narrows R1's absence claim (anchored on analyze.py output and risk-managed.jsonl keys); the paper still never shows a value.
- **Fix**: **TEXT**. Show the field-wide statistics for at least one dataset from the existing record fields (e.g. "Reality Check p across the field: x"), or reword to "computed by the scorer and emitted in the sweep records" with a pointer to the field names.

### R-22 | Attribute the 0.070 dispersion to its dataset
- **Source seat(s)**: R1 Minor Issues (Figures and Tables)
- **Severity**: Minor | consider
- **Location**: Table 2 caption ("The value measured from a real field was 0.070")
- **Fix**: **TEXT**. Attribute 0.070 to us-indices-1d (weekly US is 0.034, hourly crypto 0.125 per the committed records).

### R-23 | Consider a bootstrap_p column in Table 3
- **Source seat(s)**: R1 Minor Issues (Figures and Tables)
- **Severity**: Minor | consider
- **Location**: tab:eligibility
- **Fix**: **TEXT**. Add the column Sec. 5.4 quotes from.

---

## R2-sourced (Domain)

### R-24 | Add the forward-and-adaptive-evaluation prior art
- **Source seat(s)**: R2 W1 (raised) + R3 (corroborated: preregistration analogy "should cite the analogy explicitly", S4 and Cross-Disciplinary Connections)
- **Severity**: Major | must_fix
- **Location**: sections/06-related.tex (new passage); sections/04-integrity.tex forward-attestation framing; refs.bib
- **What to change**: "Forward-attested" is claimed as a contribution with no positioning against M6, the reusable holdout, The Ladder, or live LLM-fund arenas; the novelty reads larger than it is and a D&B pool will notice.
- **Fix**: **TEXT**. Add a short forward-and-adaptive-evaluation passage citing Makridakis et al. (M6, IJF; arXiv 2310.13357), Dwork et al. 2015 (Science 349(6248)), Blum and Hardt 2015 (ICML), and the live-arena preprints (R2 lead: DeepFund, arXiv 2503.18313 [UNVERIFIED]; Numerai design literature); state exactly what the Ed25519 chained board adds over each (verifiability without trusting the host; pre-commitment of scoring rules, not just entries). Optionally add the clinical-trial preregistration analogy (R3).

### R-25 | Add FinRL-Meta and InvestorBench to the rival set
- **Source seat(s)**: R2 W2 (raised)
- **Severity**: Major | must_fix
- **Location**: sections/06-related.tex; tab:related and tab:related-inverse; sections/01-introduction.tex ("no existing agent board applies as gates")
- **Fix**: **TEXT**. Add FinRL-Meta (Liu et al., NeurIPS 2022 D&B) and InvestorBench (arXiv 2412.18174) to both tables under the marks-only-what-they-claim protocol, or state explicitly why each is out of scope (e.g. FinRL-Meta benchmarks RL environments rather than ranking submitted agents); soften or defend the introduction's universal claim accordingly.

### R-26 | Separate the pass^k construct: seeds vs windows
- **Source seat(s)**: R2 W3 (raised) + R1 Sampling & Data comment (corroborated: "seeds vary only slippage, so pass^k is in practice a windows test") + DA M7 (seed-level reliability near-degenerate, k of roughly 6 effective runs, no cross-seed variance reported)
- **Severity**: Major (R2, DA M7) | must_fix
- **Location**: sections/03-benchmark.tex Sec. 3.1 pass^k paragraph; Findings 2 and 3 in sections/05-experiments.tex; sections/00-abstract.tex "reliability across every execution seed and out-of-sample window"
- **What to change**: tau-bench's pass^k measures reliability over stochastic attempts at the same task; here windows measure regime dependence of the strategy, and the eight seeds (slippage-only, ~3 bp) are near-copies, so the borrowed construct is stretched and the "every execution seed" claim rests on roughly 6 effective runs.
- **Fix**: **TEXT (core)**: one paragraph in 3.1 separating seeds (execution reliability, the tau-bench sense, cite tau-bench here only) from windows (regime robustness; name the every-window requirement a regime-robustness mandate), plus one sentence each in Findings 2 and 3. **NEW EXPERIMENT (optional, discharges DA M7 fully)**: report per-seed dispersion statistics from the existing run records (extend analyze.py to emit cross-seed variance per window) and scope the abstract's claim to what they show.

### R-27 | Cite original sources for the PSR and the skill-vs-luck lineage
- **Source seat(s)**: R2 W4 (raised)
- **Severity**: Minor | should_fix
- **Location**: Eq. (1) attribution in sections/03-benchmark.tex; skill-vs-luck paragraph in sections/06-related.tex; refs.bib
- **Fix**: **TEXT**. Cite Bailey and Lopez de Prado (2012), Journal of Risk 15(2), for Eq. (1) (keep 2014 for Eq. (2)); consider Kosowski et al. (2006), JF 61(6), and Harvey, Liu and Zhu (2016), RFS 29(1). (Lo 2002 handled in R-18.)

### R-28 | Harden the load-bearing 2026-preprint and FinBen citation claims
- **Source seat(s)**: R2 W5 (raised) + R0 References comment (corroborated: phantom-citation risk flagged) + DA N4 (FinBen +/- may be a standard deviation, not a confidence interval)
- **Severity**: Minor | should_fix
- **Location**: sections/01-introduction.tex and sections/00-abstract.tex (FinBen 1.51 +/- 1.08 vs 0.02 +/- 0.87); sections/06-related.tex (llmtradingaudit2026, agentreliability2026, finmultiagentsurvey2026)
- **Fix**: **TEXT (verification)**. For each of the four claims, add the exact source locator (table or section number) in a footnote or the evidence directory; confirm the FinBen numbers name the task and model variant as the source table does and whether the +/- is a cross-asset SD or a CI (the abstract says "intervals that overlap"; if it is an SD, restate the overlap argument); verify the three 2026 arXiv ids and attributions are bibliographically real.

### R-29 | Add per-cell provenance for the comparison-table marks; verify the Costs column
- **Source seat(s)**: R2 W6 (raised)
- **Severity**: Minor | should_fix
- **Location**: tab:related and tab:related-inverse; new appendix or evidence file
- **Fix**: **TEXT**. Ship a per-cell provenance record (source section/page per mark and per deliberate blank), mirroring the paper's own-number discipline; specifically check whether StockBench's and QuantBench's simulations charge transaction costs before leaving their Costs cells empty.

### R-30 | Bibliography hygiene
- **Source seat(s)**: R2 W7 (raised)
- **Severity**: Minor | should_fix
- **Location**: refs.bib
- **Fix**: **TEXT**. Convert deza2021interpretability from @inproceedings-with-arXiv-booktitle to @misc/@article; align kapoor2025agents key/year (key 2025, entry 2024); cite or delete sortino1991downside (Sec. 3.2 names Sortino) and sharpebench2026 (Sec. 8 or footnote 1); upgrade finben2024 to its NeurIPS 2024 D&B proceedings version.

### R-31 | Fix the findings-count inconsistency
- **Source seat(s)**: R2 Minor Issues (raised) + DA N1 (corroborated)
- **Severity**: Minor | should_fix
- **Location**: sections/B-checklist.tex ("two of the three findings"); sections/05-experiments.tex sec:claims ("the three findings")
- **Fix**: **TEXT**. Make both consistent with the four-findings count used in the abstract, introduction, and contributions.

### R-32 | Gloss the verdict labels in the eligibility-table caption
- **Source seat(s)**: R2 Minor Issues (Figures and Tables)
- **Severity**: Minor | consider (absorbed into R-08's caption rewrite if done together)
- **Location**: tab:eligibility caption
- **Fix**: **TEXT**. One clause linking "Every regime" / "Never catastrophic" to the two reliability verdicts of Sec. 5.3.

---

## R3-sourced (Perspective)

### R-33 | Add an incentives analysis for declared trial counts
- **Source seat(s)**: R3 W2 (raised) + DA N6 (corroborated: deflation "for that search" only when the search is confessed)
- **Severity**: Major | must_fix
- **Location**: sections/03-benchmark.tex (declared-N definition); new incentives subsection in Sec. 3 or 4; sections/07-limitations.tex
- **What to change**: Declared-N is an honor system; every submitter's dominant strategy is understatement, which is undetectable, so the heaviest overfitters face the lowest effective bar, inverting the selection the benchmark exists to perform. The eight-attack self-audit does not contain this attack.
- **Fix**: **TEXT**. Add an incentives subsection with a stated threat model: at minimum, state that declared-N is advisory and binding deflation comes from field size. Candidate mitigations to discuss (R3): preregistration of the search itself (a trial ledger under the existing hash-chain machinery); a host-set floor on N per entrant; audit-lottery random deep audits with disqualification. Cite the strategic-classification/Goodhart and forecasting-truthfulness literatures (R3 marks specific citations [UNVERIFIED], search leads).

### R-34 | Construct a pass witness; reposition the near-term artifact
- **Source seat(s)**: R3 W3 (raised) + DA Ignored Alternative 1 and Unexamined Premise (corroborated: universal refusal is equally predicted by a jointly unsatisfiable conjunction; a swept family of injected-edge agents would separate the hypotheses and "was not run")
- **Severity**: Major | must_fix
- **Location**: sections/05-experiments.tex (new subsection); sections/00-abstract.tex and sections/07-limitations.tex framing
- **What to change**: Across 576 cells zero agents are eligible, and attainability is conjectural, so the artifact today is a refusal engine with an unproven acceptance region; the "this is the gate working" defense assumes the certification premise the DA challenges.
- **Fix**: **NEW EXPERIMENT**: build a synthetic agent (family) with controlled injected edge (returns generated at known per-period Sharpe), run it through the full harness at the shipped defaults, sweep the edge size, and report the eligibility boundary in Sharpe-drawdown space, demonstrating the acceptance region is nonempty and locating it. The deterministic kernel makes this cheap; use the same sweep + `analyze.py` reduction path as the existing evidence. **TEXT (companion)**: reposition the near-term product as an audit and refusal-diagnosis tool (the per-gate reason codes) with the leaderboard as aspiration; one paragraph, per R3 "the paper is one section away from this framing".

### R-35 | Justify or generalize the all-weather default reliability mandate
- **Source seat(s)**: R3 W4 (raised)
- **Severity**: Major | must_fix (minimum form)
- **Location**: sections/05-experiments.tex Sec. 5.2 default-gate definition; sections/03-benchmark.tex
- **What to change**: The default certifies all-regime profitability at 90 percent confidence, a mandate essentially no real allocator holds; the paper ships the extreme default and builds Finding 2 on it, presenting a mandate choice as a safety definition.
- **Fix**: **TEXT (minimum)**: one paragraph justifying the shipped default against mandate-relative alternatives (UCITS/Basel constrain risk relative to a declared mandate). **Design change (preferred by R3, authors' choice)**: mandate declaration at submission (long-only benchmark-relative, absolute-return, market-neutral) with the reliability gate certifying against the declared mandate, making buy-and-hold's verdict "meets its mandate, mandate is not all-weather".

### R-36 | Make backtest-mode verdicts advisory for pretrained models; state trial semantics
- **Source seat(s)**: R3 W5 (raised)
- **Severity**: Major | must_fix
- **Location**: sections/03/04 (protocol rule); sections/C-simdata.tex contamination discussion; sections/07-limitations.tex
- **What to change**: For LLM agents the dominant validity threat is training-set contamination, not multiple testing; masking defeats ticker lookup but not shape-level memorization; the deflation construct's trial-count semantics are undefined for policies whose search happened in pretraining.
- **Fix**: **TEXT (core)**: state as a protocol rule (not a limitation paragraph) that backtest-mode results for pretrained models are advisory and only forward-arena results are certifiable for them; state a position on declared-N semantics for pretrained policies (R3 offers one: declared-N is meaningless for an LLM agent and the forward window is the only deflation-free evidence). **NEW EXPERIMENT (optional)**: a shape-level contamination probe scoring the same agent on real vs moment-matched surrogate series, flagging memorization by the gap; composes with the existing perturbation diagnostic.

### R-37 | Close the Sybil vector on the measured-dispersion path
- **Source seat(s)**: R3 W6 (raised) + DA M3 (corroborated: the bar is currently set by the author's own baseline zoo, a circularity the limitations note does not acknowledge)
- **Severity**: Major | must_fix
- **Location**: sections/03-benchmark.tex measured-dispersion path; sections/04-integrity.tex self-audit; sections/07-limitations.tex
- **What to change**: An entrant can submit near-identical sock puppets to shrink measured sigma_trials and lower its own deflation bar; the attack targets gate configuration, so the per-submission audit cannot see it, and no committed test exercises it.
- **Fix**: **NEW EXPERIMENT + TEXT**. Any one of R3's mitigations plus a paragraph: floor the measured dispersion at a configured annualized minimum; measure dispersion only over entrants with distinct verified commitments and cap per-identity entries per window; and add a ninth self-audit attack that assembles a sock-puppet field and asserts the bar does not drop (implement in the self-audit battery and run it; R3: "engineering the authors can do in a day"). In the same passage, acknowledge DA M3's point that the current measured 0.070 is a property of the author's baseline zoo, not of a strategy population (connects to R-17's uncertainty reporting and R-12's dedup guard).

### R-38 | Scope the verdict: edge existence, not deployability
- **Source seat(s)**: R3 W7 (raised)
- **Severity**: Minor | should_fix
- **Location**: sections/05-experiments.tex or sections/07-limitations.tex; headline table
- **Fix**: **TEXT (core)**: one paragraph stating eligibility means a statistically survivable edge under stylized execution on narrow universes, not a deployability, capacity, or breadth certificate. **Cheap run (optional)**: report the worst-case cost profile beside the typical one for the headline table (the harness already supports it).

### R-39 | Trim the repeated strictness assertion
- **Source seat(s)**: R3 Minor Issues (Language)
- **Severity**: Minor | consider
- **Location**: Sec. 5.2 "This is strictness, not breakage"; Sec. 2 "That is not a failure of the benchmark"
- **Fix**: **TEXT**. Assert once.

### R-40 | Explain the tau-bench row in the comparison table
- **Source seat(s)**: R3 Minor Issues (Figures and Tables)
- **Severity**: Minor | consider
- **Location**: tab:related caption
- **Fix**: **TEXT**. One clause on why a customer-service benchmark appears in a trading comparison (construct source for pass^k).

### R-41 | Pair the two comparison tables
- **Source seat(s)**: R3 Minor Issues (Layout)
- **Severity**: Minor | consider
- **Location**: tab:related / tab:related-inverse placement
- **Fix**: **TEXT**. Place tab:related-inverse beside tab:related; the text reads them as a pair.

---

## DA-sourced (Devil's Advocate; items not already absorbed above)

DA C1 is absorbed into R-01; DA C2's core into R-14; DA M3 into R-17/R-37; DA M6 into R-06; DA M7 into R-26; DA M8 into R-08; DA N1 into R-31; DA N4 into R-28; DA N6 into R-33.

### R-42 | Revise the determinism/self-audit value claim
- **Source seat(s)**: DA M1 (raised)
- **Severity**: Major | must_fix
- **Location**: sections/02-principles.tex ("a reproducible scorer is wrong in exactly the same way everywhere"); sections/04-integrity.tex; sections/05-experiments.tex sec:units
- **What to change**: The paper's authority argument (determinism plus a judge-free self-audit makes the benchmark trustworthy) is contradicted by its own first finding: the eight-attack audit stayed green while the bar was off by up to sqrt(8760), and determinism propagated the error byte-identically; the audit tests gate-evasion, not gate-calibration, and nothing in the integrity protocol would catch the next calibration error.
- **Fix**: **TEXT**. Revise the value claim explicitly: determinism buys replicability and auditability, not validity; state the audit's scope (evasion, not calibration) and, if available, name what would catch the next calibration error (e.g. the cross-statistic consistency check that actually caught this one, per R1 S4).

### R-43 | Scope "the market itself cannot pass" as sample-period-contingent
- **Source seat(s)**: DA M2 (raised)
- **Severity**: Major | must_fix
- **Location**: sections/00-abstract.tex; sections/05-experiments.tex sec:passk; sections/C-simdata.tex tab:data Range column
- **What to change**: Every bear window driving Finding 2 comes from 2020 and 2022 sitting inside every frozen range; on a sample ending 2021 weekly US indices would plausibly clear both bars. The refusal is presented as a property of the benchmark; it is equally a property of the chosen decade. The pass^k finding is near-tautological (long-only loses in bear windows).
- **Fix**: **TEXT (minimum)**: scope the claim ("on histories containing at least one bear window, which all nine frozen ranges do") and acknowledge the window-contingency. **NEW EXPERIMENT (stronger)**: rerun the sweep on a truncated range ending 2021 for one dataset and report that buy-and-hold's verdict flips, which converts the DA's objection into a demonstrated property of the gate (command: regenerate the truncated dataset via the existing fetch scripts, run the Appendix A sweep, reduce with analyze.py).

### R-44 | Scope claims by effective dataset independence
- **Source seat(s)**: DA M4 (raised)
- **Severity**: Major | must_fix
- **Location**: sections/05-experiments.tex ("zero exceptions in nine", "on every real dataset"); sections/03-benchmark.tex tab:data; sections/07-limitations.tex
- **What to change**: Crypto 1h/4h/1d/1w are one price history resampled four ways; US indices 1d/1w likewise; rates is one series; commodities two co-moving contracts; effective independent panels are about five, overlapping in the same 2020-2022 events.
- **Fix**: **TEXT**. State the effective-independence structure once (a sentence or a tab:data caption note) and phrase counting claims accordingly ("nine dataset-timeframe combinations over roughly five independent market panels").

### R-45 | Run the luck floor at the scale the introduction invokes
- **Source seat(s)**: DA M5 (raised)
- **Severity**: Major | should_fix
- **Location**: sections/01-introduction.tex ("Give a thousand random agents..."); sections/05-experiments.tex falsification leg (five seeded random agents)
- **What to change**: Five fully-invested random-weight agents on correlated baskets almost cannot beat concentrated buy-and-hold on raw return, so the falsification leg is a weak test of the motivating claim; the interesting tail event (one random agent in many looking like Renaissance) is never simulated.
- **Fix**: **NEW EXPERIMENT**. Simulate a large random field (order of 1,000 seeded agents; the deterministic kernel makes it cheap) on one or two datasets; report the maximum raw and deflated Sharpe of the field and that deflation at the honest N crushes it. This turns the intro's thought experiment into evidence and strengthens Finding 4's context. (If R-12's luck-floor redefinition lands first, run at scale under the new definition.)

### R-46 | Report N-sensitivity for the risk-managed discrimination claim
- **Source seat(s)**: DA C2 bundle (raised; the sensitivity sub-claim adjudicated as an evidence gap, not a blocker)
- **Severity**: Major (DA band) | should_fix
- **Location**: sections/05-experiments.tex sec:riskmanaged
- **What to change**: The discrimination result is reported at N = 50 trials the agent never incurred, against a sigma_trials from a majority-random field; no sensitivity at N = 10 or the honest field size is shown.
- **Fix**: **NEW EXPERIMENT**. Rerun the risk-managed evaluation at N in {honest field size, 10, 50} (the existing `risk_managed_eval` command with the trial-count parameter varied) and report whether the deflation refusal is robust across them; one sentence plus a small table.

### R-47 | Address the rates yield-as-price construct
- **Source seat(s)**: DA N2 (raised)
- **Severity**: Minor | consider
- **Location**: sections/C-simdata.tex ("The rates file's close column is a yield in percent, not a price")
- **Fix**: **TEXT**. One or two sentences arguing (or scoping) what Sharpe on yield changes means as a trading construct, and noting cross-sectional momentum is undefined on a one-symbol universe (interacts with R-12).

### R-48 | Note the WTI outlier's effect on commodities statistics
- **Source seat(s)**: DA N3 (raised)
- **Severity**: Minor | consider
- **Location**: sections/C-simdata.tex (negative close -36.98 "dominates its moments")
- **Fix**: **TEXT**. One sentence in the body (not only documentation) stating that every commodities statistic inherits the retained outlier and what the opt-out changes.

### R-49 | Scope "byte-identical on any platform" to the tested matrix
- **Source seat(s)**: DA N5 (raised)
- **Severity**: Minor | consider
- **Location**: sections/04-integrity.tex
- **Fix**: **TEXT**. "byte-identical on the three CI targets (Linux, macOS, Windows) on every commit".

---

## Tally

- Items: 49 (R-01 through R-49)
- Severity (as recorded above): 2 carry a validated DA CRITICAL (R-01, R-14); Major 18 (R-01, R-02, R-03, R-12, R-24, R-25, R-26, R-33, R-34, R-35, R-36, R-37, R-42, R-43, R-44, R-45, R-46, plus R-14's DA band); Minor 31
- Obligation: must_fix 20; should_fix 17; consider 12
- Fix type: TEXT-only 36; NEW EXPERIMENT required 5 (R-12, R-34, R-37 ninth attack, R-45, R-46); TEXT-or-EXPERIMENT (either route discharges) 8 (R-01, R-03, R-06, R-17, R-26, R-36, R-38, R-43)
