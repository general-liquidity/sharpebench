# SharpeBench paper: plan

Status: planning. Nothing in this file is a claim yet. Every empirical sentence in
the paper must trace to a command you can run, and the commands are listed in S6.

## 0. The one-sentence thesis

Ranking trading agents by realized return over a short window ranks the variance
of noise; SharpeBench ranks only the skill that survives multiple-testing deflation,
per-run reliability, process discipline and a bootstrap null, and proves the
ranking forward with pre-registration commitments and a deterministic,
replay-verifiable scorer.

## 1. What exists (sourced from code, not the README)

| Asset | Where | Paper use |
|---|---|---|
| Essay prose, 2,413 words, 17 citations | `paper/src/essay-prose.md` | intro, motivation, related work |
| Two equations (LaTeX source recovered) | `paper/src/original-essay-figures.py` L37-43 | typeset natively, not as images |
| Two data figures + exact generator | same file | regenerate as PDF, reproducible |
| Methodology derivations, ~3,900 words | `docs/book/src/methodology*.md`, `integrity.md`, `attestation.md` | method section, verbatim-accurate |
| Eligibility predicate | `composite.rs:453` | the central equation of the paper |
| 8-attack self-audit, all defended | `selfaudit.rs` | integrity section, table |
| Three-agent demonstration board | `suites/example_submissions.json` | Figure 1 |
| Real frozen datasets: US indices 2016-2026, crypto majors 2023-2026 | `data/` | experiments |
| Cont stylized-facts realism battery, both datasets REALISTIC | `sharpebench realism` | data validity section |
| Adversarial stress paths (flash crash, whipsaw) | `sharpebench stress` | robustness |
| Replay-recompute tamper evidence, 4 tests | `sim/src/trajectory.rs` | integrity section |

## 2. Findings that change the paper (from the product scan, verified)

These are not bugs to hide. They are the honest scope, and stating them is what
turns marketing into a paper. Each one also yields a concrete improvement.

### 2a. Four of six advertised "ranking gates" do not gate. VERIFIED.
`composite.rs:453`: `dsr >= bar && passed_k && process_ok && bootstrap_p < alpha && mandate_ok`.
White RC, Hansen SPA and Romano-Wolf are computed and reported, never read by
eligibility. The mdBook says this correctly; the README headline does not.
- Paper: state the predicate exactly. Describe RC/SPA/step-down as reported
  field-wide significance, which is what they are.
- Product fix: correct README:33-38 and `decay.rs:4`. Consider whether the
  step-down verdict SHOULD gate; that is a design decision, and the paper can
  present it as an ablation (S5, experiment E3).

### 2b. Zero agents have ever been rank-eligible on real data. VERIFIED by running the binary.
On both frozen datasets every agent, including clean buy-and-hold that passes four
of five gates with boot_p = 0.0005, is rejected by DSR = 0.0000. The deflation
demonstration currently holds only on synthetic and hand-built inputs.
- This is the single most important thing a reviewer will find. Ask the question
  ourselves first: is the bar unsatisfiable at real daily effect sizes and track
  lengths, or is buy-and-hold genuinely not an edge after deflation (which is a
  defensible reading of the literature)?
- Paper: this becomes experiment E1, the calibration study. It may well produce
  the paper's most interesting result: "on N windows of real data across M asset
  classes, X of Y agents clear the bar, and here is the sensitivity to dsr_bar,
  n_trials and trials_sr_std."
- Product fix: `trials_sr_std = 0.5` is a hard-coded assumption (verdict.rs:33)
  that materially decides who ranks. It must become a measured quantity or a
  documented, sensitivity-analysed choice. `runs_for_power` exists and is never
  applied to the board; apply it.

### 2c. The HMAC chain is symmetric. VERIFIED.
Anyone who can verify can forge. "Anyone can verify the board independently of
the host" (README:38) is not what HMAC delivers. It is tamper-evident to
key-holders only. No asymmetric signature exists in the repo.
- Paper: say "tamper-evident to holders of the verification key" and list
  asymmetric signatures as the planned upgrade. Do not claim public verifiability.
- Product fix: Ed25519 signing is a small change with a large credibility payoff.
  Worth doing BEFORE the paper so the claim can be made.

### 2d. Determinism is under-tested relative to the claim.
One same-machine diff in CI, one golden test (synthetic prices, not scores), no
cross-platform matrix, no WASM-vs-native comparison. The claim is architectural.
- Paper: say exactly what is enforced. "Byte-identical on any platform" is not yet
  demonstrated.
- Product fix: golden-score fixtures (PLAN.md:73 promised them), a Linux/macOS/
  Windows CI matrix, and one test that scores the same field natively and in WASM
  and asserts equality. All cheap. All make the claim true.

### 2e. Look-ahead is enforced at the observation boundary, not the type.
`Dataset::close_at(sym, t)` is public and takes arbitrary t. The guarantee is real
for the shipped architecture but "impossible by construction" overstates it.
- Paper: "unrepresentable via the agent interface." Precise and still strong.

### 2f. Seven modules are unreachable from every user surface.
`regime_compare`, crowding decay prior, 3-leg uncertainty, percentile selection,
disqualification, rediscovery, comparison_set. Notably `comparison_set`, the
cross-agent fairness control, is not called by `rank`.
- Paper: only describe what a user can invoke. Library-only capabilities go in a
  short "available but not yet surfaced" paragraph or are omitted.
- Product fix: wire `comparison_set` into `rank` (it is a correctness property),
  and surface `regime_compare` in the CLI since it is the largest analysis module.

### 2g. Documentation drift
README "7 attacks" (code: 8), "eleven crates" (twelve), `METHODOLOGY_VERSION`
stamped as 0.0.8 in every verdict (workspace: 0.1.0), MCP server reports 0.0.3.
- Product fix: all four, before the paper cites any of them.

## 3. The multi-asset, multi-timeframe experiment (the operator's point)

Today the evidence is two daily datasets. A benchmark paper about separating
skill from luck is vastly stronger when the claim is tested where luck behaves
differently: across asset classes (different fat-tail and autocorrelation
structure, shown by the realism battery: equity kurtosis 13.96 vs crypto 6.74,
skew sign flips) and across timeframes (deflation depends on track length n, so
the same strategy at 1h vs 1d has a different n and a different DSR for the same
annualized Sharpe).

This is experiment E2 and it is the paper's main empirical contribution. It also
directly answers 2b: if the bar is satisfiable at some timeframe and not others,
that is a finding about the statistic, not a defect.

Grid (to be confirmed against what data is obtainable without a keyed feed):

| Asset class | Source | Timeframes | Notes |
|---|---|---|---|
| US equity indices | FRED (historical evidence only) | 1d; 1w by aggregation | copyrighted; pre-approval required, none recorded |
| Crypto majors | Binance klines (have, no key) | 1h, 4h, 1d, 1w | widest timeframe range available free |
| FX majors | to find (free daily exists) | 1d | different microstructure, near-zero drift |
| Commodities / rates | FRED has some | 1d | low-frequency sanity |
| Single-name equities | BLOCKED, needs keyed feed | | state as limitation |

For each cell: run the same reference agents (buy-and-hold, momentum, N monkeys),
2 windows x 8 seeds minimum, costs on, and report the full gate vector. Two
products: the eligibility map, and the DSR sensitivity surface over
(timeframe, n_trials, dsr_bar). The luck floor should sit below the reference
agents in every cell or the benchmark has a problem to report.

## 4. Structure (NeurIPS Evaluations & Datasets 2026 frame, arXiv preprint)

The track renamed itself to "Evaluations and Datasets" and its explicit frame is
"evaluation as a scientific object." The spine is a measurement argument, not a
leaderboard. Target 9 content pages.

1. Abstract. Failure of existing evaluation -> construct -> protocol -> one
   headline number -> release. Say "diagnostic" and that external validation
   against live performance is outstanding.
2. Introduction. Gap as a failure of evaluation (FinBen CI +-1.08, StockBench
   single window, QuantBench undeflated). Contribution bullets, each tied to a
   section and a runnable command.
3. Design principles (numbered, short). Every later decision traces to one.
   Luck-robust, reliability-gated, process-gated, point-in-time, deterministic,
   forward-attested, judge-free.
4. The benchmark. 4.1 Contract (observation -> decision). 4.2 Simulator and
   cost model (fees, sqrt-impact, financing, partial fills, the three
   profiles). 4.3 Data and its validity (the realism battery as evidence).
   4.4 Metrics, with the equations inline: PSR, DSR, pass^k, the bootstrap,
   and the exact eligibility predicate from composite.rs:453. 4.5 What is
   reported but does not gate (RC, SPA, step-down, Sortino, calibration,
   decay, attribution), stated plainly.
5. Threats to validity and the integrity protocol. Point-in-time at the
   boundary (2e). Self-audit: 8 attacks, table, including adversarial-input
   and the standing limitation that unexercised fragility is invisible.
   Replay-recompute. Forward attestation with the HMAC honesty (2c). Canary.
6. Experiments. E1 calibration of the DSR bar on real data (answers 2b).
   E2 multi-asset multi-timeframe eligibility map and sensitivity surface
   (S3). E3 ablation: each gate removed in turn, what ranks. E4 the stress
   paths. E5 falsification: the whole pipeline under a martingale null must
   rank nobody, and the luck floor must never beat a reference agent.
7. Related work as a comparison TABLE against FinBen, StockBench, QuantBench,
   Open FinLLM Leaderboard, tau-bench, AstaBench, HAL along: interaction,
   horizon, contamination defense, cost reporting, deflation, reliability,
   process gate, determinism, forward commitment. Then the finance lineage:
   Bailey & Lopez de Prado, Harvey & Liu, White, Hansen, Romano-Wolf, BSW,
   Fama-French, Arnott-Harvey-Markowitz protocol.
8. Limitations (graded, required). 2a-2g above, no single-name equities, no
   sandbox for third-party agents, no hosted arena, no external validation,
   thin observation (20 trailing closes, empty news/fundamentals), trials_sr_std
   assumption, the epistemic leg is a lower bound.
9. Reproducibility statement, ethics, maintenance plan.
Appendices: construct-validity checklist (Measuring What Matters, answered),
BetterBench minimum-QA, full per-cell tables, NeurIPS checklist, Croissant
pointer in anc/.

## 5. Experiments, precisely

| ID | Question | Command / code | Output |
|---|---|---|---|
| E0 | Does the demonstration hold? | `sharpebench score suites/example_submissions.json` | Fig 1, the three-agent board |
| E1 | Is the DSR bar satisfiable on real daily data? | `run --data` x both datasets, sweep dsr_bar in {0.80,0.90,0.95,0.99}, n_trials in {1,10,50,200}, trials_sr_std in {0.2,0.35,0.5} | eligibility vs parameters; power via `runs_for_power` |
| E2 | Across assets and timeframes? | new datasets per S3, same sweep | eligibility map + DSR surface |
| E3 | What does each gate do? | rank with one gate disabled at a time | per-gate ablation table |
| E4 | Adversarial paths | `sharpebench stress` | flash crash, whipsaw |
| E5 | Falsification | shuffled / martingale null returns through the full pipeline | must be 0 eligible; luck floor never beats reference |
| E6 | Integrity | `sharpebench audit` | 8/8 table |
| E7 | Data validity | `sharpebench realism --data` per dataset | stylized-facts table |

All runs: costs on (Typical profile), seeds and windows stated, block-bootstrap
CIs not iid. Report annualization factor. Report N trials beside every Sharpe.

## 6. Improvements to make BEFORE writing (ordered by credibility per hour)

1. Fix doc drift (2g). One hour. Otherwise the paper cites wrong numbers.
2. Wire `comparison_set` into `rank` (2f). A correctness fix the paper can then
   claim.
3. Golden-score fixtures + WASM-vs-native equality test + OS matrix (2d). Makes
   the determinism claim true rather than architectural.
4. Ed25519 signing alongside HMAC (2c). Makes "publicly verifiable" true.
5. Make `trials_sr_std` measured-or-documented with a sensitivity flag (2b).
6. Surface `regime_compare` in the CLI (2f); E2 will want it.
7. Obtain the S3 datasets. Crypto at 1h/4h/1w from Binance needs no key.

Items 1-3 are prerequisites. 4-7 are the difference between a good paper and a
strong one.

## 7. Toolchain

No LaTeX or SVG converter is installed locally. Decisions:
- Equations: typeset natively from the recovered LaTeX source. No conversion.
- Figures: regenerate from the matplotlib generator as PDF (`fig.savefig("x.pdf")`).
  matplotlib 3.10.0 is installed, the same version that made the originals.
  No SVG conversion needed anywhere.
- Class: `neurips_2026.sty` with `[preprint]` for arXiv, natbib + BibTeX.
- Build: needs TeX Live 2025. Install locally (MiKTeX or TeX Live) or build on
  Overleaf. arXiv defaults to TeX Live 2025; ship the `.bbl`.
- Packages: booktabs, siunitx, subcaption, microtype, hyperref(hidelinks),
  cleveref last, algorithm2e for the eligibility/audit procedures.
- Layout: `paper/main.tex`, `paper/sections/*.tex`, `paper/figures/*.pdf`,
  `paper/refs.bib`, `paper/anc/` for Croissant + a data sample.

## 8. Open decisions for the operator

1. Venue: NeurIPS E&D 2026 (double-blind, 9 pages, Croissant + RAI mandatory,
   ~25% acceptance) vs a finance venue vs arXiv-only. Affects class and length.
   Recommendation: author in neurips_2026 now, decide later; it costs nothing.
2. Scope `sharpebench-memory` out. It is a second benchmark sharing only -stats,
   with no agent runner, and it dilutes the thesis. Recommend a one-line mention.
3. Should Romano-Wolf step-down GATE? Today it only reports. E3 can answer it
   empirically. Decide after E3, not before.
4. Authorship and affiliation for a double-blind submission.
5. Whether items 4-7 in S6 happen before the first draft or in parallel.
