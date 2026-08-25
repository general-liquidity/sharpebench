# Structure, Rigor and Coherence Audit, 2026-08-25

Subject: `paper/main.tex` and every `sections/*.tex` fragment as of 2026-08-25, `refs.bib`, and the committed evidence and review directories. Read-only; the only file written is this report. Math is out of scope (parallel audit). Skills applied: research-paper-writing (claim-evidence map, reverse outline), scientific-writing (consistency of repeated numbers, evidence binding), peer-review (location / observation / criterion / why / action), scholar-evaluation (developmental, no accept-reject verdict), anti-slop (tics, hedging, redundancy), citation-management (spot-check and bib hygiene).

Method notes. The committed `main.aux` is 48 bytes and `main.log` records an aborted job dated Aug 24, so cross-references could not be checked from the committed build. A scratch copy was compiled with TeX Live 2026 in the session scratchpad (pdflatex, bibtex, pdflatex x2): 30 pages, zero undefined references, zero undefined citations, zero overfull boxes, all 35 bib keys cited, all 53 labels resolved. Resolved label numbers from that build are used below. House rules verified: zero em-dashes (U+2014) and zero `---` in prose; zero ditto-style table cells.

Severity counts: CRITICAL 2, MAJOR 19, MINOR 18, OK 9.

---

## 1. Structure and integration

**S1 CRITICAL. Main text is 22 pages against a 9-page track limit.**
Anchor: `main.tex:3-7` (declares NeurIPS 2026 Evaluations and Datasets track); resolved page map: Section 7 Limitations starts p.20, Section 9 Reproducibility ends p.22, references p.23-24, appendices p.25-30.
Observation: even allowing that `[preprint]` is the arXiv build, the same source is described as the submission. Nothing in the source is marked as "moves to appendix on submission".
Why it matters: a reviewer reading 22 pages of main text before the appendix will not reach the reproducibility statement.
Fix: mark a submission cut now. Candidates that lose nothing load-bearing: `tab:relative` (36 rows, p.14) to an appendix with a 4-row summary in text; `sec:incentives` (3.3) to appendix, keep the two protocol rules in 3.2; `sec:luck1000` merged into `sec:falsify` as one paragraph plus figure; `sec:perturb` to appendix; the second half of `sec:attest` (CLI lifecycle detail) to Appendix A. Target 9 pages plus references.

**S2 MAJOR. Section 5 has eleven subsections and the experiments that belong together are separated by unrelated ones.**
Anchor: resolved order 5.1 demo, 5.2 units, 5.3 default refusal, 5.4 never-catastrophic, 5.5 risk-managed, 5.6 relative verdict (`\input` at `05-experiments.tex:105`), 5.7 witness, 5.8 perturbation, 5.9 five-agent luck floor, 5.10 thousand-agent floor (`\input` at `05-experiments.tex:132`), 5.11 what the evidence supports.
Observation: the two verdict ablations (5.4 and 5.6) are split by the risk-managed agent (5.5), which itself is reported per verdict and therefore forward-references 5.6. The two falsification runs (5.9 and 5.10) are split from the witness (5.7) by perturbation (5.8), which is a self-audit topic that belongs with Section 4. `sec:ablation:87` already forward-references `sec:relative`.
Fix: reorder to 5.1 demo, 5.2 units, 5.3 default refusal, 5.4 reliability-verdict ablations (never-catastrophic, then relative), 5.5 risk-managed control (now able to cite both verdicts backward), 5.6 witness, 5.7 falsification (five-agent floor then the thousand-agent extension in the same subsection), 5.8 what the evidence supports. Move perturbation to Section 4 beside the adversarial-input attack it narrows.

**S3 MAJOR. The three `\input` fragments read as bolted on.**
Anchors: `sybil-defense-fragment.tex:2` (a `\paragraph` carrying `\label{sec:sybil}`), `hardening-fragment.tex:7` ("We therefore ran", the only first-person plural in the paper), `relative-mandate-fragment.tex:71,73` ("survives the reviewer's objection", "the pattern is exactly the one the objection predicts"), `relative-mandate-fragment.tex:77-83` (a Command paragraph with a verbatim block, duplicated verbatim at `A-commands.tex:83-89`).
Observation: each fragment carries a trace of its origin: a review-response voice, a different grammatical person, and a command block that every other experiment defers to Appendix A.
Fix: delete lines 77-83 of the relative fragment; rewrite line 71 as "The universal refusal is therefore not an artifact of the all-weather mandate: the benchmark-relative verdict, which asks only for outperformance of the universe, refuses every entrant too" and line 73 without "the objection"; change "We therefore ran" to "The falsification leg was therefore extended to". Once done, inline the three fragments into their parent files; the `\input` indirection has no remaining purpose.

**S4 MAJOR. Three `\paragraph` labels resolve to "Section 4", so five cross-references point at the whole section.**
Anchor: resolved targets `sec:selfaudit -> 4`, `sec:sybil -> 4`, `sec:attest -> 4` (paragraphs are unnumbered at the default `secnumdepth`). References: `03-benchmark.tex:45` "(Section 4)" for the Sybil numbers, `04-integrity.tex:16` the `tab:attacks` caption, itself inside Section 4, reads "the clone collapse of Section 4", `05-experiments.tex:29` "(Section 4)" for the self-audit, `06-related.tex:5`, `07-limitations.tex:5,11`, `A-commands.tex:91`, `B-checklist.tex:10`, all resolving to "Section 4" for three different topics.
Fix: promote the three to `\subsection` (4.1 Look-ahead and determinism, 4.2 Self-audit, 4.3 Sybil defense, 4.4 Forward attestation, 4.5 Replay and contamination), or keep paragraphs and write "the Sybil paragraph of Section 4". Same latent issue for `sec:replay`, `sec:contamination`, `sec:data` (these are subsections and resolve correctly to C.2-C.4; no action).

**S5 OK.** The NeurIPS D&B arc is present and in order: motivation (1), principles (2), benchmark definition (3), integrity (4), experiments (5), related work (6), limitations and conclusion (7), reproducibility (9 as Section 9; consider making it unnumbered), appendices A-C. A scoping paragraph (`01-introduction.tex:7`) precedes the findings. The construct-validity checklist (App. B) is a genuine asset for the track.

**S6 MINOR. Related work sits after Experiments.** Acceptable at NeurIPS, but Section 5.2 (`05-experiments.tex:51`) says "The rival benchmarks in Section 6 report annualized Sharpe without deflating", relying on a section the reader has not seen. Either move Related Work before Experiments or add the one-clause summary at 5.2.

---

## 2. Duplicated statements that disagree

**D1 CRITICAL. "Largest reference-agent DSR" is contradicted by the paper's own table.**
Anchors: `01-introduction.tex:12` "Weekly crypto momentum has the strongest reference-agent DSR under the current floor"; `05-experiments.tex:89` "Weekly crypto momentum has the largest reference-agent DSR at 0.0235"; `05-experiments.tex:71-72` (`tab:eligibility`) Crypto 1w momentum 0.023, Crypto 1d buy-and-hold 0.029; `review/final-integrity-2026-08-25.md` confirms 0.028517 daily crypto > 0.023492 weekly crypto. Buy-and-hold is a reference agent by the paper's definition (`05-experiments.tex:3`).
Fix: either "the largest DSR of any agent other than buy-and-hold" if that is the intended reading and it matters, or replace the sentence with the true superlative (daily crypto buy-and-hold, 0.029) and keep the momentum row for the raw-return-leaderboard point.

**D2 MAJOR. Drawdown range in the introduction contradicts the table and the body.**
Anchor: `01-introduction.tex:12` "the reference agents drew down between 32 and 99 percent in their worst windows"; `05-experiments.tex:77` FX 1d buy-and-hold worst DD 0.199; `05-experiments.tex:89` "every row but daily FX, which passes the 20 percent bound".
Fix: "between 20 and 99 percent, and above the 20 percent cap on every dataset but daily FX".

**D3 MAJOR. "Four gates" versus a five-term predicate.**
Anchors: `00-abstract.tex:2` "survives four gates"; `01-introduction.tex:5` "four corrections ... all four hold"; `B-checklist.tex:6` "four corrections"; `03-benchmark.tex:33` `eq:eligible` has five conjuncts (DSR, pass^k, process, bootstrap, mandate); `04-integrity.tex:25` lists "mandate" as a demoter.
Fix: either count the drawdown mandate as the fifth gate everywhere, or state once in 3.2 that the mandate is a configuration constraint rather than a skill gate and keep "four" for the skill gates.

**D4 MAJOR. "Two of the four findings are corrections" is not true of the four findings as numbered.**
Anchors: `01-introduction.tex:7` "two of the four began as corrections"; `01-introduction.tex:16` contribution (3) "two of which corrected the benchmark and two of which show the gates discriminate"; `07-limitations.tex:19` "Two of the paper's findings are corrections"; `B-checklist.tex:12`. The numbered findings are 5.2 units (a correction), 5.3 default refusal, 5.4 weaker verdict, 5.5 risk-managed; only one is a correction. The other corrections (field-wide tests wrongly documented as gates, HMAC chain, luck-floor degeneracy) live in `07-limitations.tex:15`, not in the findings list.
Fix: "one of the four findings began as a correction to the benchmark, and three further defects found while writing are listed in Section 7" (and adjust contribution 3 and the conclusion).

**D5 MAJOR. The Sybil numbers appear three times, once in the wrong section.**
Anchors: `03-benchmark.tex:45` (full set: 0.3258, 0.0559, 0.0000, 0.9522, 0.3018, 207), `sybil-defense-fragment.tex:5` (same set), `01-introduction.tex:5` and `07-limitations.tex:11` (prose restatement).
Fix: keep the numbers in the Sybil paragraph only; 3.3 should say "the attack and its defense are quantified in Section 4".

**D6 MAJOR. Relative-verdict command duplicated verbatim.** `relative-mandate-fragment.tex:77-83` and `A-commands.tex:83-89`. Delete the body copy.

**D7 MINOR. The FinBen interval is stated three times, twice within two lines.** `00-abstract.tex:2`, `01-introduction.tex:3` (text gives 0.43 to 2.59; the footnote repeats 1.51 +/- 1.08 and 0.02 +/- 0.87), `06-related.tex:7`. Keep the abstract and Related Work; drop the footnote or the in-text interval.

**D8 MINOR. Repeated sentences and phrases.** "a realism gate that never fails is not a gate" (`03-benchmark.tex:51`, `C-simdata.tex:9`); "Single-name equities are absent for want of a keyless feed" (`03-benchmark.tex:51`, `07-limitations.tex:7`, `B-checklist.tex:8`, `C-simdata.tex:11`); "admirably strict" x3 (abstract, intro bullet 1, 5.2); "wrong in exactly the same way everywhere" x2 (`02-principles.tex:5`, `05-experiments.tex:27`); the 2026 evidence-map sentence is verbatim in `01-introduction.tex:3` and `06-related.tex:7`. Keep one instance of each.

**D9 MINOR. Precision drift between abstract and body.** "99 percent" vs "99.3" (`00-abstract.tex:2`, `05-experiments.tex:89`); "DSR 0.030" vs "0.0302" (`hardening-fragment.tex:7`). Harmless but pick one precision per number.

---

## 3. Claim-support alignment

### Abstract (`00-abstract.tex:2`), claim by claim

| # | Abstract claim | Support | Verdict |
|---|---|---|---|
| A1 | Rival board reports 1.51 +/- 1.08 vs 0.02 +/- 0.87, overlapping | `06-related.tex:7`, `evidence/table-provenance.md` (FinBen Table 4) | OK, provenance recorded; not independently opened in this audit |
| A2 | Ranks only if edge survives four gates | `eq:eligible` has five conjuncts | MAJOR, see D3 |
| A3 | Raw return never ranks | `03-benchmark.tex:35` | OK |
| A4 | Validated on author-written agents across nine datasets, four asset classes, four bar sizes | `05-experiments.tex:3`, `tab:data` | OK |
| A5 | LLM evaluation is the named next experiment, not a result | `07-limitations.tex:3,5` | OK, but 7.2 says an evaluation was attempted and did not complete; say "attempted, incomplete, inadmissible" once up front (MINOR) |
| A6 | Unconverted prior set the bar at annualized 18 (daily) and 106 (hourly) | `tab:units` row 0.500: 18.1, 106.5 | OK |
| A7 | Literature states the prior annualized | `03-benchmark.tex:21`, `05-experiments.tex:31` "worked example from Bailey 2014, an annualized dispersion" | MAJOR, no page locator; see C3 below |
| A8 | With units corrected, no reference agent eligible anywhere | `05-experiments.tex:57` (576 cells) | OK |
| A9 | All nine ranges contain a bear window; only longer panels contain both 2020 and 2022 | `05-experiments.tex:57`, `tab:relative` regime strings | OK, but the bear-labeling rule is never defined (see R4) |
| A10 | Most table-leading references breach the 20 percent cap; hourly momentum loses 99 percent | `tab:eligibility` (8 of 9), `05-experiments.tex:89` | OK |
| A11 | Relative verdict: nothing eligible; every gained window is a bear window gained by a flat agent; every lost window is a bull window where the agent did not beat beta | `relative-mandate-fragment.tex:71-73`, `tab:relative` | OK (supported by the table's Gained and Lost columns) |
| A12 | Risk-managed agent: under never-catastrophic clears every other gate, refused solely on deflation; under default fails reliability too | `05-experiments.tex:97`, `risk-managed.jsonl` per integrity report | OK (B3 from the editorial decision is repaired) |
| A13 | Injected-edge family: acceptance region nonempty, boundary near annualized Sharpe 3 | `05-experiments.tex:111` (2.52 weekly, 3.17 daily) | OK; "near 3" is a fair rounding of two values |
| A14 | Nine defended self-audit attacks incl. Sybil, closed by clone collapse and precommitted lower bounds on field size and dispersion | `04-integrity.tex:9`, Sybil paragraph, `eq:effective-n`, `eq:dispersion-floor` | OK |
| A15 | 1,000-agent run: every random agent ineligible; unfloored diagnostic 0.030; operational max zero | `hardening-fragment.tex:7` | OK, but scope "on two daily datasets" is dropped from the abstract (MINOR) |
| A16 | Forward arena opened its first window under schema and config digest; no result until September 2026 | `04-integrity.tex:35` | OK, stated accurately as open and unresolved |
| A17 | Every number produced by a listed command from committed data | `A-commands.tex` | MINOR overreach: the pre-floor 0.984 / 0.000 pair at `05-experiments.tex:31` and the 32-64 trial crossing at `05-experiments.tex:7` have no listed command for that snapshot (the after-v0.3.0 sweep is in `evidence/after-v0.3.0/` but Appendix A does not name it) |

### Contributions (`01-introduction.tex:16`)

| # | Claim | Support | Verdict |
|---|---|---|---|
| C1 | Protocol with predicate `eq:eligible`, three verdicts from configuration, forward-attestation design "whose published boards chain into one verifiable history" | 3.2, 5.4, 5.6, 4 | MAJOR wording: no board has been published (`04-integrity.tex:35` "zero commitments"; integrity report "zero published windows"). Say "whose boards, once published, chain". |
| C2 | Nine keyless hash-pinned datasets, each scored for realism | `tab:data`, C.2 | OK |
| C3 | Four empirical findings, two corrections, two discrimination | see D4 | MAJOR miscount |
| C4 | Self-audit with nine attacks, replay-recompute check, "a publicly verifiable signed board", forward arena open | 4, C.3 | MAJOR wording: "publicly verifiable signed board" describes a capability (`sharpebench verify --public`), not an artifact. Say "a signing and public-verification scheme". |
| C5 | Direct simulation of the 1,000-agent tail | 5.10 | OK; add "on two daily datasets" |
| C6 | Import adapter; no rival demotion claimed | `06-related.tex:7` | OK, honestly scoped |

### Results in the body not reflected up front

- The field-wide Reality Check / SPA / step-down values (`03-benchmark.tex:27`: daily US indices p = 0.0005, commodities p = 0.522) are reported once in the methods section and never in Experiments or the findings list. Either promote to a `tab:eligibility` column or drop the example numbers from Section 3. (MINOR)
- The frequency dependence of risk management (`05-experiments.tex:101`, whipsaw on hourly crypto and daily FX) is a genuine result and is absent from abstract, findings list and conclusion. (MINOR; a one-clause mention in the Finding 4 bullet would do)
- The process gate never fires on any real entrant; its evidence is the synthetic demo and the audit only. Not stated anywhere as a limitation. (MINOR; add one sentence to 7.4)

---

## 4. Scientific depth and scope

**R1 OK.** Findings are framed as calibrations and negatives with scope: sample-period contingency (`05-experiments.tex:57`), the roughly-five-panels dependence (`03-benchmark.tex:51`, `05-experiments.tex:57`, `07-limitations.tex:7`), verdict-conditionality of Finding 4 (`05-experiments.tex:97`), the witness's "strict, not vacuous" framing (`05-experiments.tex:111`), and the honest "claim it declines" paragraph (`05-experiments.tex:136`).

**R2 OK.** Reviewer-raised alternatives are addressed: joint unsatisfiability (witness, 5.7), N-sensitivity of the risk-managed refusal (`05-experiments.tex:99`), mandate-relative objection (5.6), luck-floor degeneracy (5.9), Sybil vector (Section 4), determinism-is-not-validity (`02-principles.tex:5`).

**R3 MAJOR. Cost model as an alternative explanation is not considered.**
Anchor: `C-simdata.tex:5` ships three cost profiles; `05-experiments.tex:3` runs only "typical"; no sensitivity anywhere. Hourly momentum's 99.3 percent window loss (`05-experiments.tex:89`), the risk-managed agent's 87 percent hourly whipsaw (`:101`), and FX buy-and-hold's bootstrap p = 1.0000 (`tab:eligibility:77`) are all consistent with cost drag on churning agents as much as with regime failure.
Fix: one extra sweep at the "none" profile on hourly crypto and daily FX (the command already takes a dataset argument), reported as a single sentence: does any verdict flip. If not run, add to 7.2 as a named open experiment.

**R4 MAJOR. The window regime labels (bull / bear / chop) that carry Finding 2 are never defined.**
Anchors: `05-experiments.tex:57` "at least one labeled bear window"; `relative-mandate-fragment.tex:23` "Regimes are the six out-of-sample windows in order: U bull, D bear, C chop"; `07-limitations.tex:7`. No rule (sign of buy-and-hold return? threshold? drawdown?) is stated anywhere in the paper or Appendix C.
Fix: one sentence in 3.4 or C.2 giving the labeling rule and where it is computed (`analyze.py` or harness), so "bear window" is a defined term.

**R5 MAJOR. Survivorship is claimed closed and is not.**
Anchor: `06-related.tex:53` "The benchmark's legs are arranged so that each closes a different one" of five failure modes including survivorship. `tab:data` fixes the crypto universe at BTC ETH SOL BNB XRP over 2023-2026, a universe selected by 2026 capitalization; nothing in Sections 3, 4 or 7 discusses survivorship or universe selection.
Fix: drop "each closes a different one" and say which are addressed (look-ahead: 4; overfitting: deflation; costs: C.1; regime shift: pass^k) and that survivorship of the fixed universes is a limitation, added to 7.3.

**R6 OK.** The forward arena is stated as open and unresolved consistently at `00-abstract.tex:2`, `04-integrity.tex:35`, `06-related.tex:5`, `07-limitations.tex:5,13`, `07-limitations.tex:19`, `B-checklist.tex:10`. Dates (opened 2026-08-24, commitments close 2026-09-14, reveal 2026-09-23) appear once, in Section 4.

**R7 OK.** Limitations enumerate what the evidence cannot show: no real agent admitted, deployability not certified, honor-system N, identity governance unbuilt, contamination probe unrun, multi-tenant hosting unbuilt.

**R8 MAJOR. Revision-history language throughout the body.**
Anchors: "this revision" at `01-introduction.tex:5`, `04-integrity.tex:9`, `05-experiments.tex:3,126`; "until this revision" `04-integrity.tex:16`; "earlier documentation that called them gates was wrong" `03-benchmark.tex:27`; "a narrower claim than earlier documentation made" `04-integrity.tex:5`; "Before this paper the cross-platform claim was architectural" `04-integrity.tex:7`; "the previously reported rates DSR of 0.993" `05-experiments.tex:126,130` (a number that appears nowhere else in the paper); "ranking now defends it" `03-benchmark.tex:45`.
Why it matters: a reader of the submitted paper has no "earlier documentation" or "previous revision" to compare against; the sentences read as a changelog. The transparency is valuable and already has a home at `07-limitations.tex:15`.
Fix: state present facts in the body; move every "was / now" contrast into the "What changed during this paper" paragraph, which can grow to a short numbered errata list with the old and new values side by side.

**R9 MAJOR. The 0.984 weekly DSR is attributed to the wrong cause.**
Anchor: `05-experiments.tex:31` "Before the later measured-dispersion floor was introduced, that units bug alone made the same index score a DSR of 0.984 on weekly bars and 0.000 on daily bars". `evidence/FINDING-units.md:58-67` shows these are post-fix (v0.3.0), pre-floor values; under the bug the weekly bar was annualized 8.2 (`tab:units`), which the index cannot reach.
Fix: "After the units fix and before the measured-dispersion floor, the same index scored 0.984 on weekly bars and 0.000 on daily bars, because the per-period bar is lowest where there are fewest periods; the current floored results are reported in 5.3."

---

## 5. Coherence: terminology, notation, versions, references

**T1 MAJOR. The paper renames the default gate in 3.1 and then does not use the name.**
Anchor: `03-benchmark.tex:27` "The paper therefore calls the default every-run requirement a regime-robustness mandate rather than reliability in the tau-bench sense." Counts across sections: "regime-robustness" 1, "reliability gate" 8, "reliability verdict" 8, "default mode" 2, "preset" 4, "all-weather" 9, "every regime" 6. Table columns use "Every regime" and "Never catastr." (`05-experiments.tex:68`), 5.4's title says "reliability gate", 5.6's title says "reliability verdict", 7.4 says "reliability gate".
Fix: define three named verdicts once in 3.2 (for example ALL-WEATHER, NEVER-CATASTROPHIC, RELATIVE), use "verdict" as the noun everywhere, and retire "mode" and "preset" except in `\texttt{}` config names. Keep "pass^k" for the aggregation operator only.

**T2 MINOR. "eligible" / "rank-eligible" / "admitted" / "certified".** Counts: rank-eligible 5, admit* 9, certif* 15. These are used as synonyms. Reserve "eligible" for the predicate, "certify" for what eligibility means to a reader (`05-experiments.tex:136` already does this), and drop "admit".

**T3 MINOR. Notation drift on trials and dispersion.** `eq:deflation` is stated in N and sigma_trials; `eq:effective-n` introduces N_eff; `eq:dispersion-floor` introduces sigma_hat_field, sigma_eff, sigma_min,ann. `eq:deflation` is never restated as evaluated at (N_eff, sigma_eff). `sybil-defense-fragment.tex:3` says "before it measures sigma_trials" although the measured object is sigma_hat_field. Prose alternates "host N", "effective N", "N = 50", "trial footprint", "trial count". Fix: one line after `eq:dispersion-floor`: "Ranking evaluates `eq:deflation` at N = N_eff and sigma_trials = sigma_eff", and use those two symbols thereafter.

**T4 MINOR. The symbol beta is the DSR bar and the word beta is market exposure in the same abstract.** `00-abstract.tex:2` "did not beat its beta"; `eq:eligible` beta = 0.95; `05-experiments.tex:103` "unhedged beta fails". Rename the bar (for example tau or "the DSR bar") or always write "market beta".

**T5 MAJOR. No version of record.** `A-commands.tex:5` names v0.2.1 and v0.3.0 and then says "Current-result artifacts were regenerated from the source snapshot ... Historical package labels are not substituted for that exact content snapshot." `08-reproducibility.tex:3` says "published to crates.io, npm and PyPI at the version stated in Appendix A". No such version is stated. `review/response-to-reviewers.md` records that a v0.5.1 tag was to be cut at submission.
Fix: name the tag and the content hash from `evidence/provenance.json` in Appendix A, and make the reproducibility statement point to both.

**T6 MAJOR. Three figures are never referenced from the body.** Resolved: `fig:witness` (Figure 2) is cited only at `A-commands.tex:67`, not in 5.7; `fig:luck1000` (Figure 3) only at `A-commands.tex:67`, not in 5.10; `fig:luckdeflation` (Figure 4b) only at `A-commands.tex:67` and discussed nowhere. Fix: add `\cref{fig:witness}` at `05-experiments.tex:111`, `\cref{fig:luck1000}` at `hardening-fragment.tex:7`, and either discuss Figure 4b in 5.9 (`05-experiments.tex:130` is the natural place) or delete it.

**T7 MINOR. `eq:psr` is never referenced, and the sqrt(n-1) property is stated in Limitations instead of beside the equation.** `05-experiments.tex:111` "These boundaries reflect the sqrt(n-1) behavior stated in Section 7". Move the sentence to follow `eq:psr` and reference the equation from 5.7 and 7.4.

**T8 MINOR. `tab:eligibility` carries a column that duplicates another by definition.** `05-experiments.tex:68` the pass^k column and the "Every regime" column are the same verdict; both read "no" on every row and the caption says so. Drop "Every regime" and rename the pair "Never catastr." to "Never-catastrophic verdict".

**T9 OK.** All 53 labels resolve; every `\cref` target exists; all 35 bib keys are cited and every cited key exists; `tab:units`, `tab:eligibility`, `tab:relative`, `tab:attacks`, `tab:data`, `tab:related`, `tab:related-inverse`, `fig:demotion`, `fig:deflation`, `fig:drawdowns` are each discussed in text.

**T10 OK.** Appendix A commands match what the text claims for each numbered result, including the honest note that the perturbation separation comes from a unit test rather than the committed report (`A-commands.tex:55-58`, `05-experiments.tex:122`) and that `make-figures.py` reimplements the kernel formulas (`A-commands.tex:23`).

**T11 MINOR. Tense.** Body mixes "the kernel was extended" (`05-experiments.tex:87`), "the harness now generates" (`:122`), "ranking now defends" (`03-benchmark.tex:45`), "is now open" (`06-related.tex:5`). After R8 is applied, use present tense for what the artifact does and past tense only for what was run.

**T12 MINOR. Stale committed build artifacts.** `main.aux` (48 bytes) and `main.log` (aborted job, Aug 24 19:18) do not correspond to `main.pdf` (Aug 25 00:57). Regenerate or remove from the tree so a reviewer who opens the log does not see "Fatal error occurred, no output PDF file produced".

**T13 MINOR. Unverifiable counts.** `08-reproducibility.tex:3` "twelve crates" and "twelve jobs"; `B-checklist.tex:7` "fourteen statistics that do not gate" against the ten or so listed at `03-benchmark.tex:35`. Reconcile against the workspace manifest and the score record.

---

## 6. Writing quality

**W1 MAJOR. The abstract is 520 words in one paragraph carrying seventeen numbers.** `00-abstract.tex:2`. NeurIPS abstracts run 150-250 words. The sentence "This paper's primary claim is an evaluation protocol" states a category, not a claim. The abstract also enumerates a "third verdict", the Sybil defense, the thousand-agent run and the arena, which are not among the "Four findings" it announces.
Fix: 200 words: problem (one sentence with the FinBen interval), what SharpeBench is (one sentence, gates named), scope of validation (one sentence), three results (units trap; universal refusal under all three verdicts with the risk-managed control and the witness locating the bar near annualized Sharpe 3; nine defended attacks and a 1,000-agent floor), arena status (one clause), reproducibility (one clause).

**W2 MAJOR. Over-long, multi-message paragraphs.** By word count from the source: `06-related.tex:7` 473 words (positioning, both tables, six rivals, the evidence map, the import adapter); `03-benchmark.tex:9-25` 379 words (PSR, DSR, N_eff, unit conversion, IID caveat, dispersion floor, provenance flag); `03-benchmark.tex:27` 334 words (seeds, windows, bootstrap, three field-wide tests, process gate: five topics under one "Reliability" heading); `01-introduction.tex:9-14` 444 words of bullets; `relative-mandate-fragment.tex:16` 270 words; `04-integrity.tex:35` 258 words.
Fix: one message per paragraph, first sentence states it. Split `03-benchmark.tex:27` into Reliability (seeds vs windows), Bootstrap null, Field-wide tests (reported, not gating), Process gate.

**W3 MINOR. Undefined acronyms at first use.** DSR first appears at `00-abstract.tex:2` and PSR at `02-principles.tex:5`, neither expanded; both are expanded only in Related Work (`06-related.tex:3`). SPA appears in `A-commands.tex:48` and `B-checklist.tex:11` without the expansion given at `03-benchmark.tex:27`. "CI" means continuous integration at `05-experiments.tex:136` and `A-commands.tex:3,115` and confidence interval at `01-introduction.tex:3` and `06-related.tex:7`. Fix: expand DSR and PSR at their first body use (3.1) and write "continuous-integration" in full.

**W4 MINOR. Tics and hedges (anti-slop pass).** Counts across sections: "honest / honestly" 11, "byte-identical" 9, "deliberately" 8, "legible" 3, "admirably" 3. Colloquial referents without definition: "look like Renaissance" (`01-introduction.tex:3`), "hedge-fund-legend size" (`05-experiments.tex:111`), "in disguise" (`05-experiments.tex:126`). Recurring construction "X is not Y; it is Z" (at least nine instances, for example `05-experiments.tex:31,89,111`, `07-limitations.tex:9`, `relative-mandate-fragment.tex:75`). None is filler, but the density reads as a single mannerism. Fix: halve "honest", replace "Renaissance" with "a legendary fund" or drop, and rewrite every second "not X; Z" pair as a plain statement.

**W5 MINOR. Sentences that state the same number twice.** `01-introduction.tex:3` (interval in text plus the same interval as mean and half-width in the footnote); `05-experiments.tex:111` states 0.35 and 0.20 twice each between text and `fig:witness` caption (caption repetition is acceptable; the text should not repeat the daily 0.10 / 0.20 pair a third time in the same paragraph).

**W6 OK.** Hedging is proportionate: every negative result carries its scope, and no "may suggest" or "could potentially" constructions were found. No em-dashes; no ditto cells; no "delve", "leverage", "navigate", or meta-commentary.

**W7 MINOR. Grammatical person.** `hardening-fragment.tex:7` "We therefore ran" is the sole first-person plural; the rest of the paper uses "this paper" and passive constructions. Align.

---

## 7. Citations

### Spot-check (8 uses read against what the source states)

| Cite | Use in paper | Assessment |
|---|---|---|
| `barras2010false` | `01-introduction.tex:3`, `06-related.tex:3`: roughly three quarters of funds show no genuine alpha; most apparent winners are false positives | OK. Barras, Scaillet, Wermers report about 75 percent zero-alpha, and that most positive-alpha funds are lucky. |
| `bailey2014deflated` | `05-experiments.tex:31` "The default sigma_trials = 0.5 is the worked example from Bailey (2014), an annualized dispersion"; `03-benchmark.tex:21` "annualized in the literature it comes from" | MAJOR. The DSR paper's formalism is in non-annualized Sharpe (its PSR uses sqrt(T-1) on per-period statistics). If the 0.5 worked example is annualized in the source, the paper must cite the page or equation where that unit is stated, because Finding 1's framing ("the literature supplies it annualized") rests on it. If it is not, Finding 1 should be reframed as "the kernel's own configured prior was in the wrong unit". |
| `lo2002sharpe` | `03-benchmark.tex:21`, `06-related.tex:3`: sqrt-of-time scaling holds only under IID | OK. |
| `taubench2024` | `03-benchmark.tex:27`, `06-related.tex:53`: pass^k as success on all k attempts | OK. |
| `politis1994stationary` | `03-benchmark.tex:27` stationary block bootstrap with geometric block length | OK. |
| `betterbench2024`, `kapoor2024agents` | `04-integrity.tex:9` "Surveys of benchmark integrity find that benchmarks with a model judge or a single tunable target get gamed" | MAJOR. BetterBench is a 46-criterion assessment of 24 benchmarks; Kapoor et al. argue for cost-controlled evaluation and adequate holdouts and describe overfitting to benchmarks. Neither states the model-judge finding as phrased. Rephrase to what they show (statistical significance rarely reported; holdouts inadequate; benchmark overfitting) or cite a source on judge gaming. The use at `06-related.tex:53` is accurate. |
| `constructvalidity2025` | `B-checklist.tex:3` "review 445 benchmarks and recommend an operational checklist" | OK. |
| `cont2001empirical` | `03-benchmark.tex:51`, `C-simdata.tex:9` stylized-facts battery | OK. |

Not verifiable in this audit (post-cutoff arXiv identifiers, all in the 2026 range): `llmtradingaudit2026` (2605.19337, "nineteen primary empirical studies, none top-tier"), `finmultiagentsurvey2026` (2603.27539, "five recurring failure modes"), `agentreliability2026` (2602.16666, "Accepted at ICML 2026"). The bib header asserts they were checked on 2026-08-23/24; the accountable author should re-open all three before submission, since the paper quotes specific counts from each.

### Missing citations for standard claims (MINOR unless noted)

- Square-root own-order impact (`C-simdata.tex:5`): Almgren, Thum, Hauptmann, Li (2005) or Gatheral (2010).
- Trend filter plus volatility targeting plus drawdown halt as "textbook risk management" (`05-experiments.tex:95`): Moskowitz, Ooi, Pedersen (2012); Harvey et al. (2018) on volatility targeting.
- "Sybil" as a term of art (`04-integrity.tex:9` and throughout): Douceur (2002).
- The (r+1)/(B+1) p-value convention (`03-benchmark.tex:27`): Davison and Hinkley (1997).
- Ed25519 (`04-integrity.tex:35`): Bernstein et al. (2012).
- Cosine similarity for clone detection needs no citation, but the 0.995 threshold's provenance (why that value) is asserted only by the honest-pair maximum 0.990; state that it is chosen with margin above the observed maximum, which the text nearly does.

### Bib hygiene

- `makridakis2024m6`: key says 2024, `year = {2023}`; arXiv 2310.13357 is October 2023. Rename key or fix year.
- Preprints with published versions: `taubench2024` (ICLR 2025), `kapoor2024agents` (published 2025; verify venue), `stockbench2025`, `quantbench2025` (check for 2026 proceedings). Citation-management pitfall 7.
- Mixed arXiv entry styles: `@misc` with `eprint` (deza, makridakis, deepfund, investorbench) versus `@article` with `journal = {arXiv preprint ...}` (stockbench, quantbench, taubench, miller, agentreliability, llmtradingaudit, kapoor). Pick one.
- DOI missing where one exists: `bailey2014pseudo` (10.1090/noti1105), `harvey2015backtesting` (10.3905/jpm.2015.42.1.013), `white2000reality` (10.1111/1468-0262.00152), `arnott2019protocol` (10.3905/jfds.2019.1.064). The author-hosted PDF URLs should be replaced by DOIs.
- `blum2015ladder` lacks pages and PMLR volume (37:1006-1014).
- `finrlmeta2022`, `finben2024`, `constructvalidity2025`, `betterbench2024`: proceedings entries with only arXiv URLs; add the OpenReview or proceedings URL.
- The header comment "Do not cite anything not here" is a good rule; keep it.

---

## 8. Consolidated findings list with severities

CRITICAL (2)
1. S1 Main text 22 pages against the track's 9-page limit; no submission cut marked. `main.tex:3-7`.
2. D1 "Largest / strongest reference-agent DSR" (weekly crypto momentum 0.0235) contradicted by `tab:eligibility` (daily crypto buy-and-hold 0.029). `01-introduction.tex:12`, `05-experiments.tex:89`.

MAJOR (19)
3. S2 Section 5 ordering separates paired experiments; perturbation belongs in Section 4.
4. S3 Fragments carry review-response voice, first person, and a duplicated command block. `relative-mandate-fragment.tex:71,73,77-83`, `hardening-fragment.tex:7`.
5. S4 Paragraph labels `sec:sybil`, `sec:selfaudit`, `sec:attest` all render as "Section 4"; the `tab:attacks` caption self-references. `04-integrity.tex:9,16,35`.
6. D2 Intro drawdown range "32 to 99 percent" contradicts FX 0.199. `01-introduction.tex:12`.
7. D3 "Four gates" versus five conjuncts in `eq:eligible`. `00-abstract.tex:2`, `01-introduction.tex:5`, `B-checklist.tex:6`.
8. D4 "Two of four findings are corrections" miscounts the numbered findings. `01-introduction.tex:7,16`, `07-limitations.tex:19`.
9. D5 Sybil numbers triplicated, including in 3.3 where they do not belong. `03-benchmark.tex:45`.
10. D6 Relative-verdict command duplicated in body and Appendix A.
11. A7 / spot-check "literature states the prior annualized" lacks a page locator in `bailey2014deflated`. `03-benchmark.tex:21`, `05-experiments.tex:31`.
12. C1 / C4 Contributions describe "published boards" and "a publicly verifiable signed board" that do not yet exist. `01-introduction.tex:16`.
13. R3 Cost profile never varied; cost drag is an unconsidered alternative explanation for the hourly and FX refusals.
14. R4 Bull / bear / chop window labels undefined. `05-experiments.tex:57`, `relative-mandate-fragment.tex:23`.
15. R5 Survivorship claimed closed by a benchmark leg; it is not addressed. `06-related.tex:53`.
16. R8 Revision-history language throughout the body ("this revision", "earlier documentation", "previously reported 0.993").
17. R9 0.984 weekly DSR attributed to the units bug; it is the post-fix pre-floor value. `05-experiments.tex:31`.
18. T1 The "regime-robustness mandate" name introduced at `03-benchmark.tex:27` is used once; "reliability gate / verdict / mode / preset" used elsewhere.
19. T5 No version of record; reproducibility statement points to a version Appendix A does not state. `A-commands.tex:5`, `08-reproducibility.tex:3`.
20. T6 `fig:witness`, `fig:luck1000`, `fig:luckdeflation` never referenced from the body; Figure 4b undiscussed.
21. W1 Abstract 520 words, seventeen numbers, announces four findings then lists eight results.
22. W2 Six paragraphs above 250 words carrying three to five messages each.
23. Spot-check `betterbench2024` / `kapoor2024agents` cited for a model-judge gaming finding neither makes. `04-integrity.tex:9`.

MINOR (18)
24. S6 Section 5.2 relies on Related Work (Section 6) not yet read.
25. A5 LLM evaluation described as "next experiment" up front and as "attempted, did not complete" in 7.2.
26. A15 / C5 Thousand-agent scope ("two daily datasets") dropped in abstract and contribution 5.
27. A17 "Every number produced by a listed command" does not cover the pre-floor 0.984 / 0.000 pair or the 32-64 trial crossing.
28. D7 FinBen interval stated three times; intro text and footnote repeat the same numbers.
29. D8 Repeated sentences (realism gate; single-name equities x4; admirably strict x3; wrong in the same way x2; evidence map verbatim x2).
30. D9 Precision drift (99 vs 99.3; 0.030 vs 0.0302).
31. Field-wide test p-values reported only in Section 3, absent from Experiments.
32. Frequency dependence of risk management (5.5) absent from findings list and conclusion.
33. Process gate never fires on a real entrant; not listed as a limitation.
34. T2 eligible / admitted / certified used as synonyms.
35. T3 Notation drift (N vs N_eff, sigma_trials vs sigma_eff vs sigma_hat_field); `eq:deflation` never restated at the evaluated arguments.
36. T4 beta as DSR bar and beta as market exposure in the same abstract.
37. T7 `eq:psr` never referenced; sqrt(n-1) property stated in Limitations rather than beside the equation.
38. T8 `tab:eligibility` "Every regime" column duplicates the pass^k column.
39. T11 / W7 Tense and person drift ("now", "was extended", "We therefore ran").
40. T12 Stale `main.aux` / `main.log` (aborted job) committed beside a newer `main.pdf`.
41. T13 Unverified counts (twelve crates, twelve jobs, fourteen non-gating statistics).
42. W3 / W4 / W5 Acronyms (DSR, PSR, SPA, CI ambiguity), tics ("honest" x11, "deliberately" x8), colloquial referents, doubled numbers; bib hygiene (M6 key/year, preprints with published versions, mixed arXiv styles, missing DOIs, missing standard citations).

OK (9)
43. S5 D&B arc present and in order with an up-front scoping paragraph.
44. R1 Findings framed as calibrations and negatives with stated scope.
45. R2 Reviewer-raised alternatives (vacuity, N-sensitivity, relative mandate, floor degeneracy, Sybil, determinism) each addressed with evidence.
46. R6 Forward arena stated as open and unresolved everywhere it is mentioned.
47. R7 Limitations enumerate what cannot be shown.
48. T9 All labels, cross-references and citations resolve; seven of ten figures/tables discussed in text.
49. T10 Appendix A commands match the text's claims, including the two honest "produced by a test, not this command" notes.
50. W6 No em-dashes, no ditto cells, no slop vocabulary, hedging proportionate.
51. Abstract claims A3, A4, A6, A8, A10-A14, A16 and contributions C2, C5, C6 are supported at the phrasing used.

---

## 9. Twelve-line summary

1. Counts: CRITICAL 2, MAJOR 19, MINOR 18, OK 9.
2. CRITICAL: main text runs 22 pages against a 9-page track limit with no submission cut marked (`main.tex:3-7`).
3. CRITICAL: "largest reference-agent DSR" (weekly crypto momentum 0.0235) is contradicted by the paper's own table (daily crypto buy-and-hold 0.029) at `01-introduction.tex:12` and `05-experiments.tex:89`.
4. The intro's "32 to 99 percent" drawdown range contradicts the FX row (0.199) and the body's own statement that FX passes the cap.
5. "Four gates" in abstract, intro and checklist versus five conjuncts in `eq:eligible`; "two of four findings are corrections" miscounts the numbered findings.
6. Three `\paragraph` labels (Sybil, self-audit, attestation) all render as "Section 4", and the `tab:attacks` caption cites its own section.
7. `fig:witness`, `fig:luck1000` and `fig:luckdeflation` are never referenced from the body; Figure 4b is undiscussed.
8. Revision-diary language ("this revision", "earlier documentation", "previously reported 0.993") and a leaked "reviewer's objection" survive in the body; the 0.984 weekly DSR is misattributed to the units bug rather than the pre-floor fix.
9. Terminology: 3.1 renames the default gate "regime-robustness mandate" and then the paper uses reliability gate / verdict / mode / preset; no version of record is named although the reproducibility statement points to one.
10. Scientific gaps: cost profile never varied (cost drag is an unexamined explanation for the hourly and FX refusals); bull/bear/chop labels undefined; survivorship claimed closed but unaddressed.
11. Citations: `bailey2014deflated` needs a page locator for "the literature states sigma_trials annualized"; `betterbench2024` / `kapoor2024agents` are cited for a model-judge finding they do not make; three 2026 arXiv sources are unverifiable here; bib needs DOIs and published-version updates.
12. House rules pass (zero em-dashes, zero ditto cells); arena status is stated accurately as open and unresolved; limitations and the witness-based vacuity defense are sound; the abstract at 520 words and six 250-plus-word paragraphs are the main writing-quality work.
