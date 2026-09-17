# Trading literature audit

This audit records how recent AI, reinforcement-learning and language-model trading papers
evaluate their results, and what SharpeBench and SharpeArena took from them. It is a companion
to the [benchmark architecture audit](BENCHMARK_ARCHITECTURE_AUDIT.md). That document reviews
evaluation benchmarks and, where it was available, their code. This one reviews trading-method
papers, most of which propose a model, an agent, an environment or a simulator rather than a
benchmark.

The corpus is 76 papers on AI, reinforcement-learning and language-model agents in quantitative
trading, collected in 2026: 84 PDF files counting versions. Three papers are held in several
versions: FINSABER (v1 to v6), FutureX (v1 to v3) and Prophet Arena (v1 and v2). The corpus was
assembled by hand and is not a systematic sample.

Every paper was read in full, appendices included, on 16 September 2026, by nine AI-assisted
readers working to one written brief. Where a paper is held in several versions, the highest
version was read in full and the earlier versions were compared on their abstracts and evaluation
sections. For each paper a reader recorded the evaluation facts with page numbers, each mechanism
that could transfer to the products with a decision checked against the current source, and one
coded evaluation record.

An earlier coding of 70 of these papers was made from a partial read: the abstract, the
experimental setup, the results tables and captions, and any evaluation, ablation, limitations or
reproducibility section. The full read checked that coding field by field. Section
[What the full read corrected](#what-the-full-read-corrected) lists every field it changed.

## Definitions

Runs, dispersion, significance test and search correction keep the definitions of the earlier
coding. The other fields follow the readers' brief.

- **Type.** One of: method (a trading model, strategy or training method); LLM-agent (a
  language-model agent that makes the trading or research decisions); agent-framework (a system
  that orchestrates agents or research loops); benchmark; environment (an interactive environment
  offered for others to use); simulator (a market simulator studied in its own right); survey (a
  survey or position paper with no experiment of its own); or not a trading paper. Some papers sit
  between categories.
- **Evaluation mode.** One or more of: historical backtest (recorded market data); synthetic
  (generated or simulated data); live paper (live market data, simulated fills); live real money;
  forward (decisions or predictions committed before the outcome exists and scored later); none.
  Several modes are joined with a plus sign.
- **Runs.** The number of independent executions behind the headline result, meaning separate
  random seeds, training runs, or independent simulation replications. A paper that trains once and
  evaluates on one split is one run. Bootstrap resamples of a single run are not runs. "Repeat a
  run" means the paper reports more than one independent execution for the result it headlines.
  - The first value in a Runs cell is the count behind the headline result. Repeats that cover only
    part of the results (an ablation, an appendix, one benchmark or the baselines) follow the words
    "repeats only for" and do not make the paper count as repeating a run.
  - In simulator studies, simulated evaluation episodes count as independent simulation
    replications even when the policy was trained once. The cell says so, and the counts give how
    many papers this applies to.
- **Dispersion.** The paper reports variation across those runs: a standard deviation, a confidence
  interval, or quantiles over the runs themselves. Cross-sectional dispersion over assets, or a
  bootstrap interval computed within a single run, does not count. Labels: yes; partial
  (variation across runs reported without a standard deviation, interval or quantiles, or only for
  part of the results); no; n/a.
- **Significance test.** Any named hypothesis test applied to the paper's own performance
  comparison. Tests that describe the data or the simulated market are recorded after "none". A
  bootstrap interval on the paper's own performance difference, which the paper reads as a
  significance statement, is recorded as the test.
- **Search correction.** A correction for the number of strategies, configurations or
  hyperparameter settings tried before the published one was selected: deflated Sharpe ratio,
  White's Reality Check, Hansen's SPA, a step-down procedure, probability of backtest overfitting,
  or an equivalent best-of-N null. Correcting across reported metrics, or across comparisons within
  one fixed strategy, is recorded separately and does not count here. The ledger writes those as
  "metrics only: name" or "comparisons only: name".
- **Costs.** What the reported evaluation charges: none (the paper states that no cost is charged,
  or no trading is simulated), fees, fees and slippage, fees slippage and impact, or not stated. A
  cost the evaluation charges counts as stated even when its rate is not given. A cost term that
  appears only in a training objective or in a general formula counts as not stated.
- **Leakage control.** The paper's own control against look-ahead or contamination, in a few words,
  or "none stated".

Surveys and papers that are not about trading carry n/a in the evaluation fields.

Decisions in the transfer ledger use the vocabulary of the benchmark audit's literature ledger,
with Adopted for mechanisms built in this round:

- **Adopted:** the paper's mechanism exposed a gap confirmed in the product source, and the change
  is built in this round. The placeholder in the row names its pull request.
- **Already covered:** the products implement the transferable idea with equal or stronger
  evidence.
- **Future:** useful only for a named future surface, or blocked by a known gap.
- **Not transferable:** relies on a model judge, a mutable core, or data the products do not
  acquire, or does not fit trading-agent evaluation.

## Counts

A script computed these counts from the coded ledger in this document, not from the readers'
prose. It reads the first label of each cell under the rules in
[Definitions](#definitions).

| | |
|:--|--:|
| Papers in the corpus | 76 |
| PDF files, counting versions | 84 |
| Published in 2025 or 2026 | 69 |
| Not about trading | 2 |
| Surveys or position papers with no experiment of their own | 6 |
| **Papers running an experiment of their own** | **68** |
| Of those, benchmarks, environments or simulators | 10 |
| Of those, reporting more than one independent run for the headline result | 21 |
| ... of which the repeats are simulated evaluation episodes | 4 |
| Of those, repeating runs only for part of the results (not counted above) | 7 |
| Of those, reporting dispersion across runs | 12 |
| Of those, reporting partial variation across runs | 4 |
| Of those, naming a significance test on their own comparison | 18 |
| **Of those, applying a search correction** | **2** |
| Of those, applying a multiplicity correction only across metrics or comparisons | 2 |
| Of those, stating the costs their evaluation charges | 34 |
| ... of which model slippage or impact | 9 |
| Of those, stating that no cost is charged or no trading is simulated | 11 |
| Of those, not stating costs | 23 |
| Of those, using live or forward evaluation | 6 |
| Of those, stating a leakage control | 54 |
| Of those, with no trading evaluation (mode none) | 1 |

The two search corrections are a deflated Sharpe ratio against about 50 declared configurations
(ML Enhanced Multi-Factor Quantitative Trading, p. 7) and a best-of-K null whose K counts 335
generations of a run that evaluated 743 candidates (MadEvolve, p. 19, p. 31). The full read found no
search correction that the earlier coding had missed. The two multiplicity corrections that are not
search corrections are Benjamini-Hochberg across metrics, described without reported values
(Tail-Safe Hedging, p. 11), and Holm across the models of a replay league (What LLM Trading Agents
Do in Production, p. 12).

Six papers use live or forward evaluation with reported results. A seventh, the Unified
Multi-Modal Framework, describes live deployments at unnamed institutions without data or method
(p. 16); it is coded historical and synthetic, with that claim in the cell. MountainLion is counted
among the papers with an experiment of their own, but it reports a case study and an undated
forecast table and no trading evaluation.

## What the full read corrected

The table lists every field where a reader recorded that the earlier coding differs from the full
read. Pages are PDF pages. Where the ledger below maps a reader's free-text value to a label, the
cell keeps the reader's qualifier; such mappings are not listed here unless the reader recorded a
difference.

The full read changed 34 fields in 26 of the 70 earlier records; the other 44 records agree with it
in every field.

| Paper | Field | Earlier coding | Full read | Page |
|---|---|---|---|---|
| Adaptive and Regime-Aware RL for Portfolio Optimization | Significance test | informal | ANOVA F(1, 65) = 3.231 and Tukey HSD, both p = 0.0769, on regime return differences rather than on the performance comparison | p. 15 |
| Agile-Quant | Year | n/a | 2024 (AAAI 2024; the arXiv v2 file is dated 21 Apr 2025) | p. 1 |
| AI Trading's Alpha Singularity (Agora) | Dispersion | yes | across-seed standard deviations exist only for baselines B2 and B6; the single-seed headline's interval is within one run | pp. 16-18 |
| AI-Driven Alpha Decay | Significance test | Bai-Perron, IV | the IV and DiD designs are specified with expected signs and not estimated; Bai-Perron runs on a simulated series | pp. 26, 28, 31 |
| Alpha-GPT | Year | 2023 | 2025 (the corpus PDF is v2, dated 20 Sep 2025) | p. 1 |
| AlphaNetV4 | Year | 2021 | 2022 (the running header gives May 2022; the masthead prints 2021) | p. 1 |
| AlphaNetV4 | Runs | not stated | one model trained per six-month cycle over five cycles | pp. 23-24 |
| AQuA | Runs | not stated | one frozen configuration scored once on the test window | pp. 5, 14 |
| Auditing AI Investment Recommendations | Dispersion | yes | stability across runs only, with no standard deviation or interval over runs | p. 4 |
| Automate Strategy Finding with LLM | Year | 2024 | 2025 (the corpus PDF is v4, dated 3 Nov 2025) | p. 1 |
| Automate Strategy Finding with LLM | Runs | not stated | one backtest per stated window, not repeated | pp. 6-7 |
| Autonomous AI Agents for Option Hedging | Dispersion | yes | the only intervals are 95% intervals over trading days within one evaluation | pp. 6, 9 |
| Bootstrap Robust Optimization for Portfolios | Dispersion | yes | the intervals are cross-sectional across assets | pp. 15, 19 |
| Bootstrap Robust Optimization for Portfolios | Runs | not stated | one backtest per method, with no repeated executions | pp. 14-15 |
| Event-Aware Sentiment Factors | Significance test | t-tests | p-values with no named test | pp. 6, 8-9 |
| Financial Market as a Self-Organized Ecosystem | Runs | 5 | 300 simulation runs behind the realism table and 5 per ablation setting | pp. 20, 30 |
| Financial Market as a Self-Organized Ecosystem | Type | environment | simulator: an agent-based market whose policy is trained inside the study, not an environment offered to others | pp. 5-6 |
| GIFT | Type | LLM-agent | method: the language model writes state and reward code offline and PPO makes every trading decision | pp. 1, 2, 15 |
| MadEvolve | Runs | 5 evolution runs | five different configurations, each run once | pp. 12-19 |
| MadEvolve | Dispersion | best-of-K null | no dispersion across runs | pp. 13, 31 |
| Market Making Strategies with RL | Runs | 150 to 250 simulations | 5 seeded experiments per setup, each over 150 to 250 sequential simulated sessions | PDF pp. 53, 73, 98, 129 |
| OpenFinGym | Type | benchmark | a gym environment that reports benchmark results | pp. 1-2 |
| QTMRL | Runs | 1 | a single seed (42), and the v2 PDF prints no QTMRL or learned-baseline result | pp. 8, 9 |
| Quantum-Enhanced RL with LSTM | Runs | not stated | the trained policy is executed once to produce the reported results | p. 2 |
| Regime-Conditional GAMLSS ZAGA | Runs | 146 folds, 9,999 bootstrap | one walk-forward run; its folds and bootstrap draws are not independent runs | pp. 4, 15 |
| Regime-Conditional GAMLSS ZAGA | Dispersion | yes | dispersion across folds within one run | p. 13 |
| RL in Financial Decision Making (systematic review) | Significance test | meta-regression | slope tests and pooled-variance two-sample t-tests whose inputs are not documented, checked on a synthetic 167-row dataset, with about 25 to 35 plotted points per panel | pp. 10, 15 |
| RL-Guided NSGA-II with GRC | Dispersion | synthetic only | a mean and standard deviation are said to be computed for one benchmark, and no values are printed | p. 31 |
| RL-Guided NSGA-II with GRC | Significance test | synthetic only | the test is not named and no statistic is printed | p. 31 |
| Tail-Safe Hedging | Dispersion | yes | paired bootstrap intervals are described, and no interval value is reported | pp. 11, 12, 30 |
| Three-Phase Tax-Aware Portfolio Foundation Model | Significance test | block bootstrap CI | an i.i.d. day-level bootstrap of 13 daily returns with the benchmark return held fixed | p. 13 |
| TradingMoE | Runs | 5 | the headline tables use one reference seed; five seeds exist for the Stock benchmark only | p. 17 |
| TradingMoE | Significance test | Ledoit-Wolf HAC Sharpe test | a Ledoit-Wolf Sharpe test plus a separate Newey-West HAC mean test | p. 14 |
| When AI Trading Agents Compete | Runs | 10 to 246 episodes | 10 evaluation episodes, and 20 per side for the aware agent; 140 and 246 are training-episode counts | pp. 4-6 |

Six papers were not in the earlier coding: Can LLM-based Investing Strategies Outperform in the
Long Run (FINSABER), FutureX, LLM-as-a-Prophet (Prophet Arena), What LLM Trading Agents Do in
Production (DXRG), Agentic Quantitative Trading (survey) and R&D-Agent-Quant.

## What changed in the products

Each paragraph below is a change built in this round from a mechanism these papers describe. Each
gap was confirmed in the product source before the change was made. The placeholder after each
paragraph names the pull request. The same round also includes changes that came from benchmark
papers and repositories outside this corpus; they are not described here.

**Sizing response to volatility.** The production record reports a median leverage of 5.0x in
every volatility sextile across a 5.7x volatility spread (What LLM Trading Agents Do in Production,
pp. 7-8, Table 3). No SharpeBench report related gross exposure to trailing realized volatility, so
an agent holding constant exposure through volatility spikes looked the same as a
volatility-targeted one. `sharpebench-sim` now has a rank-neutral diagnostic built on point-in-time
trailing volatility, typed unavailable for constant exposure. (<<SB-PR-C1>>)

**Borrow carry on shorts.** The same record states that its paper engine charges zero funding
(p. 3, p. 15). `financing_cost_frac` charged only gross exposure above 1x, so a fully short book
paid no carry under any profile. `CostModel.short_borrow_bps` is now an opt-in rate, bound into the
cost digest, with a serde default and skipped when zero so existing cost digests and golden bytes
stay identical. (<<SB-PR-C2>>)

**Test-split consultation census.** AQuA's research loops keep memory across runs against a fixed
evaluation split (pp. 7-8), and the paper names adaptive reuse of a holdout as a risk (p. 1,
p. 15). AI Trading's Alpha Singularity (categorical test feedback, p. 30) and Alpha-GPT (a stopping
point read from the held-out curve, p. 6) supply related evidence. SharpeArena's strategy search
deflated each search's winner with only that run's trial count, and `sharpebench lineage` refused
any journal with more than one record, so repeated looks at one test split were counted nowhere.
SharpeArena now records a per-test-split census of prior consultations and cumulative trials in
each record (evidence schema 2 to 3), and `sharpebench lineage` has a census mode over a
multi-record journal that accepts schemas 2 and 3. (<<SB-PR-P1>>, <<SA-PR-P1>>)

**Dated idea sources.** XALPHA builds its research memory from ingested reports with no recorded
publication date (pp. 18-19). `IdeaProvenance` in SharpeBench and the SharpeArena edge manifest
recorded no date for an idea source, so a source published after the test split began passed
`sharpebench lineage` unflagged. Both now accept an optional source date and report how many
sources are dated after each split and how many are undated. (<<SB-PR-P2>>, <<SA-PR-P2>>)

**Exposure-matched random timing.** Adaptive Alpha Weighting with PPO compares its agent with a
random entry and exit baseline matched to the agent's turnover and holding duration (p. 11).
SharpeBench's luck floor is a fully invested random allocator, so no report said whether a mostly
flat entrant beats random timing at its own exposure and holding periods. SharpeBench now reports a
rank-neutral exposure-matched random-timing reference computed by replay. (<<SB-PR-P3>>)

**Lagged replay.** The benchmark audit recorded lagged replay as a candidate from LiveTradeBench.
TradingGroup computes per-decision counterfactual action values from the recorded decision and the
next bar (p. 5), and three papers here trade at the bar after the decision (AlphaQuanter, p. 4;
SBCA, p. 16; FinSMART, p. 6). Recorded decisions were never replayed with a delay, and the stressed
profile's two-bar delay was declared but not applied. SharpeBench now reports a rank-neutral lagged
replay beside the board, valid only where the entrant's trades do not move the price.
(<<SB-PR-P4>>)

**Timing-luck floor.** The production record's methodology canon asks every study to know its
timing-luck floor (rule 17, p. 15). No report showed how a result moves when the window start
shifts by a few bars. SharpeBench now reports a rank-neutral timing-luck floor over start offsets
for the reference agents. (<<SB-PR-P5>>)

**Decision stability.** In the production record's replay league, three frontier models are not
distinguishable on decision quality, while the share of forced cells in which they change their
choice across repeats ranges from 35% to about 90 to 95% (p. 12). Auditing AI Investment
Recommendations measures stability across repeated runs as a separate axis (pp. 3-4). Neither
product measured decision changes across replicate runs that saw identical observations.
SharpeBench now reports rank-neutral decision stability over replicate runs, keyed by observation
digest. (<<SB-PR-P6>>)

**Finish-reason counts.** The canon separates transport success from tool success (rule 14,
p. 14), and Auditing AI Investment Recommendations records a token budget that truncated a
reasoning model's output (p. 4). The SharpeBench gateway carried `finish_reason` per call without
totalling it, and the SharpeArena local field recorded no stop reason. Finish-reason counts now
appear in attempt accounting and in the bridge manifest, rank-neutral. (<<SB-PR-P7>>,
<<SA-PR-P7>>)

**Cell isolation disclosure.** R&D-Agent-Quant stores every round's hypotheses, code and results
and keeps a persistent cache (p. 3, p. 5, p. 19). In SharpeBench, image entrants get a fresh
container per cell, but an `--http` endpoint or a `--cmd` host process can keep state across the
seeds of a window, and the board row did not say so. The row now carries a contract-bound
`cell_isolation` disclosure. It discloses the isolation class; it does not detect carried state.
(<<SB-PR-P8>>)

**Expected shortfall.** Autonomous AI Agents for Option Hedging reports shortfall probability beside
expected shortfall and finds that the two views rank models differently (pp. 5-6, p. 8). Tail-Safe
Hedging evaluates CVaR at a declared level (pp. 4-5, p. 11). SharpeBench had no expected shortfall or
CVaR, and downside deviation cannot separate frequent small losses from rare large ones. An opt-in
expected-shortfall and tail-count diagnostic is now available, rank-neutral. (<<SB-PR-P9>>)

**Pareto flag and inactivity.** In FineFT the conservative policy dominates the routing in most
periods (p. 8), and on one asset the method closed two positions in six months (p. 26).
SharpeBench's `pareto_optimal` marked a never-trading track as optimal, because nothing can dominate
zero drawdown and zero turnover; at the time of the audit the synthetic-field golden recorded the
`hold` row as Pareto-optimal. Tracks the kernel refuses as constant are no longer Pareto candidates.
(<<SB-PR-P10>>)

**Overlapping windows.** AlphaQuanter's robustness check averages three-month windows stepped seven
days (p. 16), and News-Aware Direct RL Trading averages 256 randomly sampled periods of one test
span (p. 4). SharpeBench's `walk_forward` yields overlapping windows whenever the step is shorter
than the test length, and pooled tracks concatenated windows without a disjointness check, so
overlapping bars counted as independent in PSR, DSR and the bootstrap. Strict replay and
sweep-contract construction now refuse overlapping windows with a typed error. `walk_forward`
behaves as before and its overlap is documented, and the SharpeArena README wording is corrected.
(<<SB-PR-P11>>, <<SA-PR-P11>>)

**Paired market-making regret.** The market-making dissertation derives a separate random stream
per agent so that paired comparisons share exogenous randomness (PDF p. 41). SharpeArena's
market-making environment draws arrivals, fills and the mid-price step from one generator, so the
two arms of `mm_regret` shared a mid path only at low arrival rates: a read-only check during the
reading found 16 of 16 seeds shared at the default rate and 0 of 16 at 200 arrivals per step.
`mm_regret` now refuses with a typed error when the two arms' mid paths differ. The default stream
is unchanged, so the frozen F2 results do not move. (<<SA-PR-A1>>)

**Market-making attribution.** The dissertation's reward decomposes into spread earnings, inventory
mark-to-market and hedging cost (Eqs. 4.1 to 4.4, PDF pp. 67-68). SharpeArena's step information
and `mm_regret` exposed one number, so inventory luck and spread capture could not be told apart.
SharpeArena now reports an exact per-step decomposition into spread capture, inventory
mark-to-market, running inventory penalty and liquidation cost that sums to the reward,
rank-neutral. (<<SA-PR-A2>>)

**Volatility-aversion attribution.** Time-Inhomogeneous Volatility Aversion keeps one aversion
coefficient and charges each step's variance around that step's expected reward, so a
deterministic reward path carries no charge (p. 2, p. 4). SharpeArena's
`time_inhomogeneous_vol_aversion` said it follows the paper but applies a time-varying coefficient
to a trailing EMA volatility. The attribution is corrected and the difference stated. The scheme is
training-only and has no rank effect. (<<SA-PR-A3>>)

**Impact-misspecification gap.** Robust Reinforcement Learning in Finance judges robustness by the
portfolio gap between trading with and without market impact (p. 9). SharpeArena had an elliptic
uncertainty set and a robust clearing path, but no report showed how far a policy's return moves
between the point estimate and the worst case on the same seeds. SharpeArena now has a rank-neutral
paired report of that gap. (<<SA-PR-A4>>)

**Same-bar priority in the shared book.** ABIDES-MARL fixes the execution order of simultaneous
agents' actions through one coordinator (p. 4). Financial Market as a Self-Organized Ecosystem
activates one random agent per step (p. 7), FinEvo randomizes wake-up intervals (p. 28, p. 38), and
KineticSim aggregates orders per price level without agent identity (pp. 4-5). SharpeArena's book
processed each bar's orders by agent index, so seat 0 always queued first at a shared price; a
read-only check during the reading found seat 0 ahead on 32 of 32 seeds for identical quoters.
An opt-in same-bar priority rule (seeded rotation or pro rata) is now available, with the default
path and the golden tape unchanged. (<<SA-PR-A7>>)

**Meta-order impact shape.** When AI Trading Agents Compete reports square-root impact during a
meta-order and power-law decay after it (pp. 3-4). SharpeArena's market model called exponents
below one "the square-root-law regime", while its permanent impact compounds every bar and never
decays; a read-only check found impact linear in executed quantity at exponents 1 and 0.5. The
wording is corrected and a rank-neutral meta-order impact-shape probe is added. A transient-impact
kernel stays Future. (<<SA-PR-A8>>)

**Documentation: the alpha-decay prior.** AI-Driven Alpha Decay describes its 13F portfolio
convergence result as coming from a calibrated simulation (pp. 30-31). A comment in
`sharpebench-core/src/decay.rs` called that study the paper's empirical leg; it now describes it as
a calibrated simulation. (<<SB-PR-D1>>)

**Documentation: training runs are trials.** The market-making dissertation chooses one network
among ten checkpoints by evaluated reward before transfer (PDF pp. 58-59), and three identically
configured learners in one market ended with mean rewards of 129, -45 and -5 thousand dollars
(PDF p. 56). SharpeArena's `EVALUATION.md` now counts training runs and checkpoints selected by
evaluated reward among the trials an entrant declares. (<<SA-PR-D2>>)

**Documentation: the risk-aware reward.** A Risk-Aware Reinforcement Learning Reward for Financial
Trading computes its reward terms over the whole horizon, with no per-bar charge (pp. 2-3).
SharpeArena's `risk_aware` scheme said it follows that paper while charging a per-bar conditional
volatility; its attribution is corrected. (<<SA-PR-D4>>)

## What the literature says about its own evaluation

Each quotation below was checked against the extracted text of its PDF. Pages are PDF pages.

- "The growth of financial language models has outpaced the availability of standardized and time
  safe benchmarks that connect textual understanding to tradable decisions." (The New Quant, p. 11)
- "widespread neglect of transaction cost modeling, with most systems assuming frictionless markets"
  (From Deep Learning to LLMs, p. 28)
- "Financial markets offer a single realization of the stochastic process, making it hard to
  achieve statistically significant out-of-sample results" (RL in Financial Decision Making, a
  systematic review, p. 27, attributed there to Harvey et al., 2016)
- "It does not mean that this single trace estimates the distribution of results over repeated
  independent runs." (AgonAlpha, p. 25)
- "Rather than reporting standard error bars or confidence intervals, we report the median
  annualized return" (R&D-Agent-Quant, p. 19, on its baselines' five seeds)
- "The single-agent iterative configuration learns to game the training metric" (AI Trading's
  Alpha Singularity, p. 19, on one of its baselines)
- "treat test isolation as a governance property to be audited over a run, not as a cryptographic
  guarantee" (AQuA, p. 15)
- "LLM-based financial backtests may suffer from temporal leakage when the pretraining corpus
  overlaps with the retrospective evaluation period." (TradingMoE, p. 7, before its rerun with an
  older backbone and its prospective paper trading)
- The expanding-window protocol "accidentally included validation-year returns in the training
  labels", which "inflated test Sharpe by 0.3 points" (ML Enhanced Multi-Factor Quantitative
  Trading, p. 11)
- The correlation between reference sentiment and alpha returns "decreases from 0.41 to 0.03 on TMF
  and from 0.37 to 0.03 on MarketWatch when next-day returns are used" (FinSMART, p. 5)

## Coded evaluation ledger

76 rows, in alphabetical order by short title. Pages are PDF pages; "p." refers to the paper in
the same row.

| # | Paper | Year | Source | Type | Mode | Runs | Dispersion | Significance test | Search correction | Costs | Leakage control |
|---:|---|---:|---|---|---|---|---|---|---|---|---|
| 1 | ABIDES-MARL | 2025 | arXiv 2511.02016v1 | environment | synthetic | 30 evaluation episodes (one training run per configuration, p. 18) | yes: standard deviation over the 30 episodes; none across training runs (p. 22) | none on the strategy comparison; regression, Anderson-Darling and ARCH-LM describe the market (p. 19) | none | none: no fees; impact is endogenous to the simulated book (p. 22) | observations lag one step; holdout evaluation episodes (p. 11, p. 18) |
| 2 | Adaptive Alpha Weighting with PPO | 2025 | arXiv 2509.01393v2 | method | historical backtest | 10 (p. 18-19) | yes: mean and standard deviation (p. 20) | Diebold-Mariano; block-bootstrap Sharpe difference (p. 13, p. 22) | none | fees: 0.1% of each position change (p. 7, p. 10) | alphas generated from training data only; next-day return after a close-t decision (p. 7, p. 10) |
| 3 | Adaptive and Regime-Aware RL for Portfolio Optimization | 2025 | arXiv 2509.14385v1 | method | synthetic + historical backtest | not stated; repeats only for the reward ablation: 5 seeds (p. 11) | no | none on the strategy comparison; ANOVA and Tukey HSD on regime returns (p. 15-16) | none | fees: 0.2% L1 turnover penalty in the reward (p. 9) | held-out test horizon asserted, not defined (p. 9) |
| 4 | Agentic Quantitative Trading (survey) | 2026 | arXiv 2608.31041v1 | survey | none | n/a | n/a | n/a | n/a | n/a | n/a |
| 5 | Agile-Quant | 2024 | arXiv 2312.05693v2; AAAI 2024 | not a trading paper | n/a | n/a | n/a | n/a | n/a | n/a | n/a |
| 6 | AgonAlpha | 2026 | arXiv 2608.11250v1 | LLM-agent | historical backtest | 5 deployments, one trace each (p. 18, p. 25) | partial: per-deployment values, no standard deviation, interval or quantiles (p. 18) | none (p. 25) | none; the data-snooping literature is cited (p. 14) | not stated; turnover reported (p. 12-13) | delay-one platform simulation; a reviewer agent checks look-ahead; selection and grading on the same window (p. 7-8, p. 35) |
| 7 | AI Trading's Alpha Singularity (Agora) | 2026 | arXiv 2606.29194v1 | LLM-agent | historical backtest | 1 (p. 16, p. 24); repeats only for baselines: 3 and 2 seeds (p. 16-17) | no: across-seed standard deviation for two baselines only; the headline interval is within one run (p. 17-18) | Newey-West HAC t-test (p. 17) | none; deferred to future work (p. 34) | fees: 9 bps one-way, with a 1x to 5x sensitivity (p. 10, p. 21) | fixed train, test and holdout split; holdout sealed from model inputs; the test segment received categorical feedback (p. 10, p. 23, p. 30) |
| 8 | AI-Driven Alpha Decay | 2026 | arXiv 2605.23905v1 | method | synthetic | not stated (p. 30-35) | no | none on a performance comparison; Bai-Perron on a simulated series; IV and DiD specified, not estimated (p. 26, p. 28, p. 31) | n/a (no strategy selected) | not stated | none stated |
| 9 | Alpha-GPT | 2025 | arXiv 2308.00016v2 | LLM-agent | historical backtest + forward | 1 (p. 5-7) | no | none | none | not stated | real-time competition window, for the competition only (p. 7) |
| 10 | AlphaNetV4 | 2022 | IJSRM 10(5), DOI 10.18535/ijsrm/v10i5.ec01 | method | historical backtest | 1 per walk-forward cycle, five cycles (p. 23-24) | no | none | none; checkpoint chosen on backtested Sharpe (p. 21) | fees: transaction fee and stamp duty (p. 31-32) | expanding-window walk-forward retraining (p. 23-24) |
| 11 | AlphaQuanter | 2025 | arXiv 2510.14264v2 | LLM-agent | historical backtest | 3 seeds, mean reported (p. 5) | no | none | none | fees: 0.1% (p. 4) | chronological split with about 30-day gaps; future-dated tool calls penalised in training (p. 5, p. 16) |
| 12 | AQuA | 2026 | arXiv 2608.12841v1 | agent-framework | historical backtest | 1 (p. 5, p. 14) | no | none | none; probability of backtest overfitting cited (p. 1, p. 4) | fees: one 2 bps two-leg turnover cost of unstated composition (p. 13-14) | sealed sandbox; causal operator language; 2020 embargo year; validation-only feedback (p. 5, p. 11) |
| 13 | Auditing AI Investment Recommendations | 2026 | arXiv 2606.27570v1 | benchmark | synthetic + historical backtest | 3 per scenario for the frontier models; 5 per configuration in the scenario bank (p. 4) | partial: set and sizing stability across runs, no standard deviation or interval (p. 4) | circular block bootstrap interval on a Sharpe difference (p. 6-7) | none | fees: tranche fee; 10 bp turnover cost for the allocator baselines (p. 5-6) | frozen scenario inputs; walk-forward allocator baselines; the basket is not point-in-time (p. 6-7) |
| 14 | Automate Strategy Finding with LLM | 2025 | arXiv 2409.06289v4 | LLM-agent | historical backtest | 1 per stated window (p. 6-7) | no | none | none; parameters set by backtesting with no count (p. 8, p. 17) | not stated: cost modelling mentioned without rates (p. 17) | chronological split; the selection context covers the 2023 test year (p. 5-7, p. 17) |
| 15 | Autonomous AI Agents for Option Hedging | 2026 | arXiv 2603.06587v1 | method | historical backtest + synthetic | not stated | no: 95% intervals over trading days within one evaluation (p. 6, p. 9) | none | none | fees: proportional rate, value not stated (p. 5) | same-day calibration applied along the later realised path (p. 4-5) |
| 16 | Bootstrap Robust Optimization for Portfolios | 2025 | arXiv 2510.12725v1 | method | historical backtest | 1 per method (p. 14-15) | no: the intervals are across assets (p. 15, p. 19) | none; non-overlap of intervals across assets read as significance (p. 19) | none | not stated: a cost term appears only in the general formula (p. 8) | expanding window with data to t-1; 80/20 split for the momentum study (p. 14-15) |
| 17 | Can LLM-based Investing Strategies Outperform in the Long Run (FINSABER) | 2026 | arXiv 2505.07078v6; KDD 2026 | benchmark | historical backtest | not stated (p. 5, p. 12) | no | paired t-test; CAPM alpha t-test (p. 7) | none | fees: per-share commission (p. 4) | point-in-time constituents including delisted names; inputs dated before each window (p. 3, p. 10); pretraining leakage not controlled (p. 9) |
| 18 | Can RL Efficiently Discover Price Manipulation | 2026 | arXiv 2607.06121v1 | method | synthetic | 1,000 test simulations per learned strategy; 100 replications of the model-based pipeline (p. 17) | yes: standard deviation over simulations and replications (p. 18) | t-test (p. 18) | none; 15 cells tested without adjustment (p. 18) | none: no fees; linear temporary impact in the model (p. 5) | separate out-of-sample test simulations (p. 17) |
| 19 | Deep RL for Optimal Trading with Partial Information | 2025 | arXiv 2511.00190v1 | method | synthetic + historical backtest | 500 evaluation episodes per method from one training run; 1 real test day (p. 12-13, p. 18) | yes: standard deviation over the 500 episodes (p. 13) | none on performance; Johansen and VAR describe the data (p. 17) | none | fees: 0.05 per unit traded (p. 3, p. 12) | chronological split; normalisation fit on the training set (p. 18) |
| 20 | DeepSeekMath Meets Order Book | 2026 | arXiv 2605.25527v1 | method | historical backtest | 1 (p. 8) | no (p. 8) | none | none | none: costs and slippage listed as future work (p. 8) | chronological 80/10/10 episode split within one hour of one day (p. 2) |
| 21 | Enhanced PIKAN | 2026 | arXiv 2602.01388v2 | method | historical backtest | not stated (p. 18-22) | no | none | none | fees: 0.25% commission on turnover (p. 7-8) | chronological train and test split (p. 16-17) |
| 22 | Event-Aware Sentiment Factors | 2025 | arXiv 2508.07408v1 | method | historical backtest | 1 | no | none named; p-values reported (p. 6, p. 8-9) | none | not stated | one-day signal lag; no train and test split stated (p. 4) |
| 23 | Financial Market as a Self-Organized Ecosystem | 2026 | arXiv 2604.23975v1 | simulator | synthetic | 300 simulation runs (p. 30); 5 per ablation setting (p. 20) | yes: standard deviation across runs (p. 21-22) | none | none; 216-cell calibration grid selected by distance (p. 14, p. 32) | not stated | separate calibration and test tickers (p. 35) |
| 24 | FineFT | 2025 | arXiv 2512.23773v1; KDD 2026 | method | historical backtest | 1 (p. 21) | no | Wilcoxon signed-rank (p. 24-25) | none | fees and slippage: commission, order-book walking and funding fees (p. 3, p. 23) | chronological train, validation and test split (p. 6, p. 21) |
| 25 | FinEvo | 2026 | arXiv 2602.00948v1 | environment | synthetic | 128 Monte Carlo runs per configuration (p. 4, p. 37) | yes: 95% intervals across runs (p. 9-10) | none | none | fees and slippage: 0.5% per trade including slippage (p. 35) | none stated; pretraining overlap acknowledged, not tested (p. 45) |
| 26 | FinFlowRL | 2025 | arXiv 2510.15883v1 | method | synthetic | 1,000,000 evaluation episodes; training seeds not stated (p. 12-13) | no | none | none | not stated | test generator parameters held out from the training grid (p. 12) |
| 27 | FinSMART | 2026 | arXiv 2607.28127v1 | LLM-agent | historical backtest | not stated; one training run described (p. 5) | no | none | none | not stated (p. 6) | chronological split; day-t articles traded at the t+1 open (p. 5-6) |
| 28 | FinWorld | 2025 | arXiv 2508.02292v2; KDD 2026 | benchmark | historical backtest | 3 seeds, averaged; LLM rows 1 (p. 7-8) | no | none | none | fees: 1e-4 for the RL rows only (p. 18) | single chronological split at 2023-05-01 (p. 7, p. 17) |
| 29 | From Classical Rationality to Contextual Reasoning (quantum logic) | 2025 | no arXiv id or venue stated | survey | none | n/a | n/a | n/a | n/a | n/a | n/a |
| 30 | From Deep Learning to LLMs | 2025 | arXiv 2503.21422v1 | survey | none | n/a | n/a | n/a | n/a | n/a | n/a |
| 31 | From Feedback Loops to Policy Updates (QuantEvolver) | 2026 | arXiv 2605.15412v1 | LLM-agent | historical backtest | 1 (p. 8-10) | no | none | none; fusion settings swept on the reported metric (p. 9-10) | not stated (p. 10) | validation-ranked selection; split dates not stated (p. 7) |
| 32 | FutureX | 2025 | arXiv 2508.11987v3 | benchmark | forward | 1 per model per event (p. 17) | no | regression coefficient tests in a factor analysis (p. 22) | none | none: forecasting, no trading | questions resolve after the prediction date; one-week delay (p. 5, p. 15) |
| 33 | Generating Alpha | 2026 | arXiv 2601.19504v1; ComSIA 2026 | method | historical backtest | 1 (p. 8-9) | no | none | none | not stated (p. 8) | 70/30 time split; news published before 9:30 only (p. 5, p. 8) |
| 34 | GIFT | 2026 | arXiv 2606.08450v1 | method | historical backtest | 1 (p. 6); repeats only for one panel in an appendix: 3 seeds (p. 22-25) | partial: appendix only (p. 22-25) | none | none; interface selected by maximum in-sample Sharpe (p. 20) | fees: 0.1% per unit turnover (p. 5) | interface designed on data before each window, then frozen (p. 4-5) |
| 35 | History Is Not Enough | 2026 | arXiv 2601.10143v1 | method | historical backtest | 1 (p. 9-11) | no | none | none | fees: 0.1% proportional (p. 8, p. 10) | statistics estimated on the training window only; chronological split (p. 4, p. 8) |
| 36 | In-Network Market Prediction | 2026 | arXiv 2608.02424v1 | method | historical backtest | 1 (p. 7) | no | none | none | none: no trading simulated | none stated; no train and test split described (p. 6) |
| 37 | Incorporating Cognitive Biases into RL | 2026 | arXiv 2601.08247v1 | method | synthetic | multiple, count not stated (p. 4) | no | none | none | not stated | none stated; state bins use the whole path's range (p. 3) |
| 38 | Interpretability in Safety-Critical Trading | 2021 | arXiv 2109.15112v1 | method | historical backtest | 1 per setting | no | none | none | not stated | chronological index split; tweet covariates lagged before the open (p. 8-9, p. 16-17) |
| 39 | Janus-Q | 2026 | arXiv 2602.19919v2 | LLM-agent | historical backtest | 1 (p. 7-8) | no | none | none | not stated: a reward cost term with no value (p. 5-6) | chronological 4:4:1:1 split with a lag before the event window (p. 6, p. 12) |
| 40 | KineticSim | 2026 | arXiv 2606.21784v2 | simulator | synthetic | 5 timing trials per configuration, 11 for latency (p. 7) | yes: spread over timing trials (p. 8-9) | none | n/a (no strategy selected) | none: no trading evaluation | none stated; no trading evaluation |
| 41 | Language Model Guided RL in Quantitative Trading | 2025 | arXiv 2508.02366v3 | LLM-agent | historical backtest | 25 per ticker (p. 5-6) | yes: standard deviation across runs per ticker (p. 7) | t-tests (p. 5-6) | none | not stated | news entities and dates anonymised; fundamentals given as changes (p. 3, p. 11) |
| 42 | LLM-as-a-Prophet (Prophet Arena) | 2025 | arXiv 2510.17638v2 | benchmark | forward | 1 per model, event and horizon (p. 4, p. 19) | no: bootstrap intervals over events within one run (p. 26) | none | none | none: fees only normalised out of prices (p. 5) | live unresolved events; forecasts within 3 hours of resolution excluded (p. 4, p. 9) |
| 43 | MadEvolve | 2026 | arXiv 2605.23007v1 | agent-framework | historical backtest | 1 per configuration, five configurations (p. 12-19) | no (p. 13) | best-of-K Gaussian null (p. 31) | best-of-K null: K = 335 generations; 743 candidates evaluated (p. 19, p. 31) | fees slippage and impact: 1.5 bps fee and square-root impact; fills at the limit price with no slippage (p. 35-36) | chronological split; fitness on validation only (p. 11, p. 22) |
| 44 | Market Making Strategies with RL | 2025 | PhD thesis, Universidad Carlos III de Madrid | method | synthetic | 5 seeds per setup, each over 150 to 250 simulated sessions (PDF p. 53, p. 73) | yes: standard deviation over simulations; spread over 5 seeds (PDF p. 54, p. 130) | none | none | none: no fees; hedges pay the market spread (PDF p. 52, p. 68) | none stated; fresh simulated test sessions (PDF p. 73) |
| 45 | ML Enhanced Multi-Factor Quantitative Trading | 2025 | arXiv 2507.07107v2 | method | synthetic + historical backtest | 1 per configuration (p. 6-7) | no | deflated Sharpe ratio (p. 7) | deflated Sharpe: about 50 configurations (p. 7) | fees and slippage: linear 5 to 8 bps including half-spread (p. 6) | tradability mask; chronological train, validation and test split (p. 3, p. 5, p. 7) |
| 46 | MountainLion | 2025 | arXiv 2507.20474v3 | LLM-agent | none | not stated | no | none | none | not stated | none stated |
| 47 | News-Aware Direct RL Trading | 2025 | arXiv 2510.19173v1 | method | historical backtest | not stated (p. 3-4) | no | none | none; trial count of the hyperparameter search not stated (p. 3-4) | not stated (p. 4) | chronological 70/15/15 split; tuning on validation only (p. 3-4) |
| 48 | OOM-RL | 2026 | arXiv 2604.11477v1 | agent-framework | live real money | 1 live deployment (p. 9-10) | no | OLS alpha t-test, mature phase only (p. 7) | none | fees and slippage: about 0.08% per side from live fills (p. 5-6) | none stated beyond live deployment |
| 49 | OpenFinGym | 2026 | arXiv 2606.26350v1 | environment | historical backtest + synthetic | not stated (p. 7); repeats only for the post-training table: 10 seeds (p. 8) | no | none | none | fees and slippage: configurable, values not stated (p. 4) | test labels excluded from containers; host-side verifier; feature-leakage test (p. 6-7) |
| 50 | Pretrained LLM with LoRA as Decision Transformer | 2024 | arXiv 2411.17900v1; ICAIF 2024 workshop | method | historical backtest | 5 seeds (p. 10) | yes: mean and spread over seeds (p. 6) | none | none | not stated (p. 4) | chronological train and test split (p. 5, p. 10) |
| 51 | QTMRL | 2025 | arXiv 2508.20467v2 | method | historical backtest | 1, seed 42 (p. 8); no QTMRL result printed in v2 (p. 9) | no | none | none | fees: 0.05% (p. 8) | chronological train and test years (p. 8) |
| 52 | Quant 4.0 | 2022 | arXiv 2301.04020v1 | survey | none | n/a | n/a | n/a | n/a | n/a | n/a |
| 53 | Quant Convergence | 2026 | arXiv 2606.24575v1 | method | historical backtest | 1, seeds locked (p. 6) | no | Welch t-test (p. 11-12) | none; tuning trial counts not stated (p. 6, p. 16) | not stated (p. 7) | temporal split; time-series tuning; train-only imputation; current constituents (p. 5-7) |
| 54 | Quantum-Enhanced RL with LSTM | 2025 | arXiv 2507.12835v1 | method | historical backtest | 1: the policy is executed once (p. 2) | no | none | none | fees: rate not stated (p. 2) | features use values at time t; normalisation span not stated (p. 2) |
| 55 | R&D-Agent-Quant | 2025 | arXiv 2505.15155v2; NeurIPS 2025 | agent-framework | historical backtest | not stated; repeats only for baselines: 5 seeds, median reported (p. 19) | no: medians of baseline seeds instead of intervals (p. 19) | none | none; 36 to 44 loops per run (p. 7, p. 9) | fees: buy 0.05%, sell 0.15% (p. 24) | schema-only prompts; a later test window against stated model cutoffs (p. 8, p. 24) |
| 56 | Regime-Conditional GAMLSS ZAGA | 2026 | independent preprint, no arXiv id | method | historical backtest | 1 walk-forward run of 146 folds (p. 4, p. 15) | no: fold dispersion within one run (p. 13) | parametric bootstrap; Wald t; likelihood-ratio test (p. 11-12, p. 14-15) | none | none: gross of costs (p. 17) | walk-forward; indicators lagged one period; in-sample-only scaling (p. 5-6) |
| 57 | Risk-Aware RL Reward for Financial Trading | 2025 | arXiv 2506.04358v1 | method | historical backtest | not stated (p. 7-13) | no | none | none; reward weights grid-searched on history (p. 4) | fees: 0.1% per trade (p. 7) | none stated |
| 58 | RL Framework for Quantitative Trading | 2024 | arXiv 2411.07585v1 | method | historical backtest | 1 (p. 5-7) | no | none | none | not stated | none stated |
| 59 | RL in Financial Decision Making (systematic review) | 2025 | arXiv 2512.10913v1 | survey | none | n/a | n/a | slope tests and two-sample t-tests on a premium the text does not define (p. 14-15) | n/a | n/a | n/a |
| 60 | RL-Guided NSGA-II with GRC | 2026 | Mathematics 14(2):296 | method | synthetic + historical backtest | 1 for the headline values; repeats only for one benchmark: count not stated (p. 31) | no: mean and standard deviation said to be computed, not printed (p. 31) | none named; a 5% test is asserted (p. 31) | none | not stated | none stated; universe chosen by index weight at the end of the sample (p. 35) |
| 61 | Robust RL with Elliptic Uncertainty Sets | 2025 | arXiv 2510.19950v3; NeurIPS 2025 | method | historical backtest | 1 (p. 9) | no | none; error bars waived as deterministic (p. 17) | none; a coefficient grid-searched on the training period (p. 40) | fees slippage and impact: 0.1% plus walk-the-book VWAP (p. 36) | chronological split (p. 8) |
| 62 | SBCA | 2026 | arXiv 2605.01384v1 | method | historical backtest | 1, seed 42 (p. 20) | no | none | none | fees: 0.25% commission, with a sweep (p. 8, p. 20) | chronological split; news moved to the next session; training-set standardisation (p. 16-19) |
| 63 | Tail-Safe Hedging | 2025 | arXiv 2510.04555v1 | method | synthetic | multiple, count not stated (p. 11) | no: intervals described, none reported (p. 11-13, p. 30) | paired bootstrap and Benjamini-Hochberg described; no value reported (p. 11) | metrics only: Benjamini-Hochberg (p. 11) | fees slippage and impact: spread, temporary and transient impact (p. 4) | in-distribution and out-of-distribution split of synthetic scenarios (p. 11) |
| 64 | Technical Indicator Networks (TINs) | 2025 | arXiv 2507.20202v2 | method | historical backtest | not stated; one instantiation per stock (p. 10) | no | paired t-test, four tests (p. 16) | none | none (p. 10) | chronological train and test split (p. 10) |
| 65 | The Meta-Learning Gap | 2025 | arXiv 2512.06666v1 | not a trading paper | n/a | n/a | n/a | n/a | n/a | n/a | n/a |
| 66 | The New Quant | 2025 | arXiv 2510.05533v1 | survey | none | n/a | n/a | n/a | n/a | n/a | n/a |
| 67 | Three-Phase Tax-Aware Portfolio Foundation Model | 2026 | arXiv 2606.30997v3 | agent-framework | historical backtest | 1: one 14-day window (p. 12) | no: bootstrap within the one window only (p. 13) | i.i.d. day-level bootstrap interval with the benchmark held fixed (p. 13) | none | none: zero transaction cost (p. 11-12) | walk-forward window after a 2015 to 2024 pretraining corpus (p. 9, p. 11) |
| 68 | Time-Inhomogeneous Volatility Aversion | 2026 | arXiv 2602.12030v1 | method | synthetic | not stated; one representative test path (p. 11) | no | none | none | fees slippage and impact: Almgren-Chriss terms in the execution example (p. 10-11) | none stated |
| 69 | Trade-R1 | 2026 | arXiv 2601.03948v2 | LLM-agent | historical backtest | 5 seeds (p. 7) | yes: standard deviations in an appendix (p. 11) | none | none | fees: 0.15% one-way (p. 6, p. 10) | time split; test window after the model's knowledge cutoff (p. 6, p. 8) |
| 70 | Trading Confidence | 2025 | PACIS 2025 | method | historical backtest | 5 seeds, actions averaged (p. 13) | no | none | none | not stated | chronological segments around recessions (p. 10) |
| 71 | TradingGroup | 2025 | arXiv 2508.17565v1 | agent-framework | historical backtest | 1 (p. 7) | no | none | none | fees: commission charged, rate not stated (p. 5) | fine-tuning windows end before the test window; online modules disabled (p. 5-6) |
| 72 | TradingMoE | 2026 | arXiv 2608.11785v1 | LLM-agent | historical backtest + live paper | 1 reference seed (p. 17); repeats only for the Stock benchmark: 5 seeds (p. 14, p. 17) | partial: standard deviation over 5 Stock seeds (p. 17) | Ledoit-Wolf Sharpe test; Newey-West HAC mean test (p. 14) | none | fees: 5 bps one-way; no slippage or borrow (p. 14) | chronological split with causal preprocessing; older-backbone rerun; prospective paper trading (p. 7-8, p. 13) |
| 73 | Unified Multi-Modal Framework for Financial Systems | not stated | no arXiv id or venue stated | method | historical backtest + synthetic | not stated (p. 13-16) | no | none | none; hyperparameter optimum reported (p. 24) | not stated: a cost term without values (p. 8) | none stated; live deployments claimed without data (p. 13, p. 16, p. 24) |
| 74 | What LLM Trading Agents Do in Production (DXRG) | 2026 | arXiv 2609.05663v1 | LLM-agent | live real money + live paper + historical backtest | multiple: 3,505 live vaults and 500 to 599 agents; replay league of captured scenarios with 3 repeats per forced cell (p. 3, p. 12) | yes: day-clustered intervals; choice changes across repeats (p. 7, p. 12) | regression discontinuity; Mantel-Haenszel; permutation nulls; Holm-adjusted league tests (p. 6, p. 7, p. 11, p. 12) | comparisons only: Holm across league models (p. 12) | fees: swap and builder fees restated at 5.5 bps; the paper engine has zero slippage and funding (p. 3) | live deployment; leave-window-out validation (p. 2, p. 13) |
| 75 | When AI Trading Agents Compete | 2025 | arXiv 2510.27334v1 | method | synthetic | 10 evaluation episodes, 20 per side for the aware agent; one trained agent per variant (p. 4-6) | no | none | none; checkpoint chosen after inspecting behaviour (p. 5-6) | fees: 1 bp on liquidation (p. 4) | none stated |
| 76 | XALPHA | 2026 | arXiv 2607.08332v2 | LLM-agent | historical backtest | 1 (p. 9) | no | none | none | fees: open 0.05%, close 0.15%, 5 CNY minimum (p. 17) | chronological split; test used only for reporting; leakage tests on generated code (p. 6, p. 27, p. 36) |

## Transfer ledger

76 rows, in the same order and numbering as the coded ledger. Pages refer to the paper in the same
row; product paths are shortened to the crate, directory or file name used in the reading notes.

The ledger records 17 adopted, 41 already covered, 9 future and 9 not transferable.

| # | Paper | Transferable mechanism | Decision | Reason |
|---:|---|---|---|---|
| 1 | ABIDES-MARL | A coordinator fixes the execution order of simultaneous agents' actions (p. 4) | **Adopted** (<<SA-PR-A7>>) | SharpeArena's shared book matched each bar's orders by agent index (`lob_market.rs:317-322`); an opt-in same-bar priority rule is built. The paper's market diagnostics are covered by the calibrated null in `realism.py`. |
| 2 | Adaptive Alpha Weighting with PPO | Random entry and exit reference matched to the agent's turnover and holding duration (p. 11) | **Adopted** (<<SB-PR-P3>>) | The luck floor was a fully invested random allocator (`sharpebench-sim/src/agent.rs:103-112`); an exposure-matched random-timing reference by replay is built, rank-neutral. |
| 3 | Adaptive and Regime-Aware RL for Portfolio Optimization | Regime probabilities in the observation, with a regime-switching Monte Carlo (p. 4-8) | Already covered | SharpeArena labels regimes causally for a rank-neutral breakdown (`regime_eval.py`) and refuses reads of scenario regimes (`lookahead_guard.py`); seeded tiers and cross-regime transfer are in `EVALUATION.md`. |
| 4 | Agentic Quantitative Trading (survey) | Match the evaluation setting to the capability claimed (p. 7) | Already covered | SharpeBench keeps historical, forward and forecast evidence apart (`docs/book/src/methodology.md`), and SharpeArena keeps forward paper trading as a separate evidence class. |
| 5 | Agile-Quant | Inference latency reported beside task quality (p. 5) | Already covered | The SharpeArena bridge reports p50 and p95 latency with `rank_input: false` (`bench_bridge.py`). Not a trading paper. |
| 6 | AgonAlpha | Count every simulated candidate, including eliminated and sign-reflected ones (p. 7) | Already covered | SharpeArena counts candidates before validation and deduplication (`strategy_generation.py`); SharpeBench deflates by the trial footprint (`composite.rs`) and collapses clones on absolute cosine (`rediscovery.rs`). |
| 7 | AI Trading's Alpha Singularity (Agora) | Provenance-sealed reads and a bounded categorical feedback channel from a reused test segment (p. 30) | Already covered | Future data is not representable at the SharpeArena interface and only the selected strategy touches test (`strategy_generation.py`); forward commit and reveal avoids reusing a split. Cited in support of the test-split census. |
| 8 | AI-Driven Alpha Decay | Crowding half-life prior and falling return dispersion under homogenization (p. 14-15) | Already covered | A rank-neutral prior (`sharpebench-core/src/decay.rs`) and a precommitted dispersion floor (`composite.rs`); the code comment on the 13F study is corrected (<<SB-PR-D1>>). |
| 9 | Alpha-GPT | In-sample against out-of-sample IC curve over search iterations (p. 6) | Already covered | The budget curve reports marginal gain, non-improvement onset and a selection-deflated peak (`budget_curve.rs`). Cited in support of the test-split census. |
| 10 | AlphaNetV4 | Expanding-window retraining with early stopping on backtested Sharpe (p. 21-24) | Already covered | Checkpoint selection is declared search inside deflation (`composite.rs`), and the budget curve reports its cost. |
| 11 | AlphaQuanter | Robustness from overlapping rolling windows (p. 16) | **Adopted** (<<SB-PR-P11>>, <<SA-PR-P11>>) | Pooled tracks concatenated windows without a disjointness check (`composite.rs:1686-1721`); strict replay and sweep contracts refuse overlap. The paper's next-close fill corroborates the lagged replay. |
| 12 | AQuA | Cross-run search that reuses a fixed evaluation split (p. 7-8, p. 15) | **Adopted** (<<SB-PR-P1>>, <<SA-PR-P1>>) | Repeated recorded searches against one test split were not counted (`strategy_generation.py`, `lineage_cmd.rs:65-71`); a per-split census of consultations and cumulative trials is built. |
| 13 | Auditing AI Investment Recommendations | Validity, run-to-run stability and reference agreement as separate axes, with a per-run truncation flag (p. 3-5) | **Adopted** (<<SB-PR-P6>>, <<SB-PR-P7>>, <<SA-PR-P7>>) | The admissibility contract is covered by closed schemas and process gates (`sharpebench-protocol/src/lib.rs`); decision stability and finish-reason counts are built as rank-neutral reports. |
| 14 | Automate Strategy Finding with LLM | Model factor selection from multimodal context with a chronological split (p. 5-7) | Future | The split is covered (`strategy_generation.py`). Prompt context is free text, so dating it against the test window stays with the point-in-time citation row of the benchmark audit; dated idea sources cover bound sources only. |
| 15 | Autonomous AI Agents for Option Hedging | Shortfall probability beside expected shortfall (p. 5-6) | **Adopted** (<<SB-PR-P9>>) | SharpeBench had no expected shortfall (`sharpe_diagnostics.rs`); an opt-in expected-shortfall and tail-count diagnostic is built, rank-neutral. |
| 16 | Bootstrap Robust Optimization for Portfolios | Select a configuration on a percentile of its dependent-bootstrap utility (p. 14-19) | Already covered | `percentile_selection` in `sharpebench-stats/src/selection.rs`, with the middle percentile as the default. |
| 17 | Can LLM-based Investing Strategies Outperform in the Long Run (FINSABER) | Point-in-time constituents including delisted names over twenty years of rolling windows (p. 3, p. 10) | Future | SharpeBench universes were selected with hindsight; closing survivorship needs a keyed single-name dataset. Rolling windows are already a pass^k condition (`pass_k.rs`). |
| 18 | Can RL Efficiently Discover Price Manipulation | Optimizer and RL search for profitable round trips under concave impact (p. 7-18) | Future | SharpeArena's manipulation probe samples the round trip (`manipulation.py`); a search is a new experiment, and no scored surface lets an entrant move the price. |
| 19 | Deep RL for Optimal Trading with Partial Information | Latent Markov regime-switching signal the agent must filter (p. 2-4) | Future | The RegimeShift tier has one changepoint (`scenario_gen.rs:352-386`); a recurring latent-regime family is a new calibrated generator. |
| 20 | DeepSeekMath Meets Order Book | Group-normalized episode advantages on order-flow states (p. 6-7) | Not transferable | A training algorithm, not an evaluation mechanism; the book observation already carries queue imbalance and microprice (`lob_market.rs:100-104`). |
| 21 | Enhanced PIKAN | Best variant highlighted per market (p. 18-22) | Already covered | Declared-trial deflation, field-level multiple-testing diagnostics and pass^k across windows; clone collapse handles identical baseline rows (`rediscovery.rs:135`). |
| 22 | Event-Aware Sentiment Factors | Best event labels by Sharpe out of more than 70 (p. 3, p. 6) | Already covered | Deflation by the trial footprint and field-wide Reality Check and step-down (`methodology-significance.md`); text features need data the products do not acquire. |
| 23 | Financial Market as a Self-Organized Ecosystem | Wasserstein distance between simulated and real return, tail and absolute-return clouds (p. 13-14) | Future | Needs a real reference panel the products do not acquire; the SharpeArena manuscript names a distributional divergence as the next gate revision. Its random activation supports the same-bar priority rule. |
| 24 | FineFT | Routing to a conservative policy, with risk metrics reported as wins (p. 5-8) | **Adopted** (<<SB-PR-P10>>) | `pareto_optimal` marked a never-trading track optimal (`composite.rs:2098`); tracks the kernel refuses as constant are no longer Pareto candidates. |
| 25 | FinEvo | Selection, innovation and shocks over a strategy population, replicated over 128 runs (p. 3-4, p. 37) | Already covered | `ecology.py` ports all three without an RNG, and SharpeArena reports a multi-seed replication. Its randomized wake-ups support the same-bar priority rule. |
| 26 | FinFlowRL | Market-making evaluation on generator parameters held out from training (p. 12) | Already covered | Cross-regime transfer and the Avellaneda-Stoikov environment (`market_making.py`); Hawkes order arrivals remain a Future realism axis. |
| 27 | FinSMART | Contemporaneous-return training reward kept apart from next-open evaluation (p. 5-6) | Already covered | Scoring recomputes returns from recorded decisions (`reward_misspecification.py`); next-bar execution is the lagged replay built this round. |
| 28 | FinWorld | All-in-one research platform with seed-averaged benchmark tables (p. 7) | Not transferable | A platform rather than an evaluation protocol; pass^k requires every seed and window instead of an average (`pass_k.rs`). |
| 29 | From Classical Rationality to Contextual Reasoning (quantum logic) | Quantum-probability model of order-dependent investor expectations (p. 9-10) | Not transferable | A belief-formation model with no evaluation protocol; stated probabilities are scored with proper rules (`calibration.rs`, `forecast.rs`). |
| 30 | From Deep Learning to LLMs | Survey finding that agents assume frictionless markets and perfect liquidity (p. 28) | Already covered | Named cost profiles (`sharpebench-sim/src/costs.rs`) and SharpeArena's price-time-priority book (`lob_market.rs`). |
| 31 | From Feedback Loops to Policy Updates (QuantEvolver) | Reinforcement fine-tuning of a factor miner with family and redundancy shaping (p. 5-7) | Already covered | Disjoint train and held-out seed bands, and invalid and family-grouped proposals counted without merging (`strategy_generation.py`). |
| 32 | FutureX | Predict at the start date, score after a one-week delay (p. 5, p. 15) | Already covered | Forward windows with commit and reveal (`docs/book/src/arena.md`); forecast comparisons use exact common support (`forecast.rs`). |
| 33 | Generating Alpha | News admitted only if published before the open (p. 5) | Already covered | The point-in-time observation boundary (`paper/sections/02-principles.tex`). |
| 34 | GIFT | Model-designed state and reward interface selected by in-sample Sharpe, then frozen (p. 3-5, p. 20) | Not transferable | The design loop needs model calls; freezing and counting search are covered by digest binding, forward commitment and deflation. |
| 35 | History Is Not Enough | Discriminative score and stylized-fact distance for synthetic series (p. 11) | Future | Both need real reference data the products do not acquire; a leverage-effect statistic would be informational in `realism.py`. |
| 36 | In-Network Market Prediction | Order book built from add, cancel and update messages (p. 3-4) | Already covered | The integer-tick book supports limit, market, cancel and modify (`lob_market.rs:54-75`). |
| 37 | Incorporating Cognitive Biases into RL | Driftless random walk in which no policy should profit (p. 4-5) | Already covered | The luck floor and the seeded synthetic generator (`docs/book/src/simulator.md`). |
| 38 | Interpretability in Safety-Critical Trading | In-range input perturbation propagated through the full pipeline (p. 5) | Already covered | `sharpebench-harness/src/perturb.rs` and the self-audit's adversarial-input case (`selfaudit.rs:740-777`); a gradient search needs model access. |
| 39 | Janus-Q | Hierarchical gated reward on realized abnormal return (p. 5-6) | Already covered | Conjunctive gates, and executed-return rewards under an environment-owned cursor (`EVALUATION.md`). |
| 40 | KineticSim | Bitwise cross-engine identity from integer order quantities (p. 7-8) | Already covered | Integer ticks and quantities pinned by a golden tape (`lob_market.rs:495-499`). Its identity-blind aggregation supports the same-bar priority rule. |
| 41 | Language Model Guided RL in Quantitative Trading | Entity and date anonymisation and differenced fundamentals against model recall (p. 3, p. 11) | Already covered | `Dataset::masked` renames symbols and dates (`sharpebench-sim/src/data.rs:341-346`); forward windows remain the stronger control. |
| 42 | LLM-as-a-Prophet (Prophet Arena) | Market-implied probability as a baseline forecaster (p. 5, p. 9) | Not transferable | Needs live prices at forecast time, which is market-data acquisition; proper scores with intervals are already in the forecast report (`forecast-quality.md`). |
| 43 | MadEvolve | Best-of-K null and an in-sample against out-of-sample degradation curve (p. 25, p. 31) | Already covered | Deflation by declared and observed trials (`composite.rs:1313`) and the selection-deflated budget curve; SharpeArena counts candidates on the host. |
| 44 | Market Making Strategies with RL | Per-agent random streams for paired comparisons; reward split into spread, inventory and hedging terms (PDF p. 41, p. 67-68) | **Adopted** (<<SA-PR-A1>>, <<SA-PR-A2>>) | `mm_regret` refuses unpaired mid paths and the market-making reward is decomposed per step (`market_making.py`), both rank-neutral; training runs are named as declared trials (<<SA-PR-D2>>). |
| 45 | ML Enhanced Multi-Factor Quantitative Trading | Tradability mask through every rolling operator, with a deflated Sharpe (p. 1-7) | Future | Deflation is covered (`composite.rs:1313`); the bundled datasets carry no price-limit or halt fields, so a mask-aware execution model waits for a keyed equity dataset. |
| 46 | MountainLion | Retrieval-refined reports with a rolling-accuracy forecast fusion (p. 3-13) | Not transferable | No trading evaluation, and live retrieval with no point-in-time boundary. |
| 47 | News-Aware Direct RL Trading | Test score as the mean over randomly sampled sub-periods of one test span (p. 4) | **Adopted** (<<SB-PR-P11>>, <<SA-PR-P11>>) | A second instance of the overlap gap recorded under AlphaQuanter; overlapping windows are refused at the evidence boundary. |
| 48 | OOM-RL | Live loss as an alignment signal, with a principal-loss barrier and a hash-anchored test boundary (p. 3-5) | Already covered | `DrawdownStopper(mode="initial")` (`risk.py`), digest-bound contracts and forward pre-registration. |
| 49 | OpenFinGym | Host-side verifier with a rate-limited scoring API (p. 7) | Already covered | The frozen engine never exposes future bars, forward windows are scored once, and SharpeArena requires private held-out seeds (`EVALUATION.md:56-64`). |
| 50 | Pretrained LLM with LoRA as Decision Transformer | Offline RL from expert trajectories with a later test span (p. 5, p. 10) | Already covered | The Minari export emits leak-safe train and test datasets over disjoint seed bands (`minari_export.py`). |
| 51 | QTMRL | Multi-indicator A2C over a 2^N discrete joint action space (p. 4) | Already covered | A target-weight action with a per-symbol discrete adapter (`discrete.py`) and causal normalisation (`preprocessing.py`); scores are recomputed from decisions. |
| 52 | Quant 4.0 | Market simulator for what-if tests and extremes absent from history (p. 32, p. 37-38) | Already covered | Seeded Hard and Extreme tiers, the endogenous shared book and the price-time-priority book (`scenario_gen.rs`, `market.rs`, `lob_market.rs`). |
| 53 | Quant Convergence | Value-screen features tested by one buy-and-hold portfolio per model (p. 7-12) | Future | Survivorship and point-in-time fundamentals need a keyed single-name dataset; the rest of the protocol is covered by pass^k and step-down. |
| 54 | Quantum-Enhanced RL with LSTM | Auxiliary forecast appended to the observation (p. 2-3) | Already covered | The observation contract and `lookahead_guard.py` bound what the environment reveals; the luck floor replaces a single random run. |
| 55 | R&D-Agent-Quant | Persistent research memory and caching, with a published loop census (p. 3-9) | **Adopted** (<<SB-PR-P8>>) | A stateful `--http` or `--cmd` entrant can keep state across the seeds of a window; a contract-bound `cell_isolation` disclosure is built on the row. It does not detect carried state. |
| 56 | Regime-Conditional GAMLSS ZAGA | Zero-adjusted distributional comparison conditioned on regimes (p. 8-13) | Already covered | `compare_by_regime` (`sharpebench-core/src/regime_compare.rs`), which deliberately fits no GAMLSS model. |
| 57 | Risk-Aware RL Reward for Financial Trading | Composite episode reward of return, downside deviation, differential return and Treynor ratio (p. 3) | Already covered | Bounded training rewards composed with the kernel's deflated Sharpe (`rewards.py:361-406`); the `risk_aware` attribution is corrected (<<SA-PR-D4>>). |
| 58 | RL Framework for Quantitative Trading | Normalisation and reward-design study on one stock (p. 4-7) | Already covered | The generalization-gap requirement (`EVALUATION.md:75-78`), causal normalisation and declared-trial deflation. |
| 59 | RL in Financial Decision Making (systematic review) | Multiple-testing inflation, one-realization limits and survivorship named as open problems (p. 23-27) | Already covered | DSR and CSCV probability of backtest overfitting (`sharpebench-stats`, `sharpebench-edge/src/pbo.rs`) and seeded realizations with held-out bands; survivorship stays Future. |
| 60 | RL-Guided NSGA-II with GRC | In-sample efficient frontier and tangency Sharpe (p. 35-39) | Not transferable | Reinforcement learning tunes an optimiser rather than trading; the `max_sharpe` baseline estimates moments causally (`baselines.py`). |
| 61 | Robust RL with Elliptic Uncertainty Sets | Value gap between nominal and worst-case impact on the same paths (p. 9-10) | **Adopted** (<<SA-PR-A4>>) | `EllipticUncertaintySet` existed (`market.rs:175`) with no gap report; a rank-neutral paired impact-misspecification report is built. |
| 62 | SBCA | Next-session news alignment, training-set standardisation and a commission sweep (p. 16-24) | Already covered | The lagged news channel (`news.py`), causal normalisation and fixed-decision cost profiles (`costs.rs`). |
| 63 | Tail-Safe Hedging | CVaR at a declared level as an evaluation metric (p. 4-5, p. 11) | **Adopted** (<<SB-PR-P9>>) | Built with Option Hedging as the expected-shortfall diagnostic. The per-step safety projection stays Future, because action-time repair would hide what the process gate scores (`process.rs`). |
| 64 | Technical Indicator Networks (TINs) | Learned indicator measured against the classical indicator it starts from (p. 10) | Already covered | Classical rule and buy-and-hold references run through the same gates; an undefined Sortino is returned as undefined (`sharpebench-stats/src/stats.rs:66-71`). |
| 65 | The Meta-Learning Gap | Oracle-utilization ratio (p. 7, p. 12) | Already covered | `sharpebench-memory` reports headroom to an oracle arm. Not a trading paper. |
| 66 | The New Quant | Minimum reporting standard: time-safe splits, full costs, capacity, regimes and baselines (p. 15) | Already covered | Named cost profiles, regime reports, rank-neutral latency and cost, and reference agents; capacity and survivorship remain Future. |
| 67 | Three-Phase Tax-Aware Portfolio Foundation Model | Bracket-aware after-tax reward and tax-lot recommendations (p. 6-9) | Not transferable | Tax treatment is investor- and jurisdiction-specific and needs lot-level holdings (p. 16). |
| 68 | Time-Inhomogeneous Volatility Aversion | Charge each step's reward variance around that step's expected reward (p. 4, p. 8) | **Adopted** (<<SA-PR-A3>>) | `time_inhomogeneous_vol_aversion` (`rewards.py`) now states how it differs from the paper; training-only, no rank effect. |
| 69 | Trade-R1 | Process-consistency score gating a market reward (p. 4-6) | Not transferable | The gate is a model judge; its deterministic residues are the closed contract (`sharpebench-protocol/src/lib.rs:282-296`) and rank-neutral confidence. |
| 70 | Trading Confidence | Aleatoric, epistemic and distributional uncertainty legs (p. 5-12) | Already covered | Ported as a model-free decomposition in `sharpebench-core/src/calibration.rs`. |
| 71 | TradingGroup | Per-decision counterfactual values from the recorded decision and the next bar (p. 5) | **Adopted** (<<SB-PR-P4>>) | Replaying recorded decisions through the frozen engine is built as the rank-neutral lagged replay, valid where the entrant's trades do not move the price. |
| 72 | TradingMoE | Older-backbone rerun and prospective paper trading as leakage controls (p. 7-8) | Already covered | Forward windows with commit and reveal postdate every entrant (`docs/book/src/attestation.md`); pass^k requires every seed; declared trials cover a displayed grid. |
| 73 | Unified Multi-Modal Framework for Financial Systems | Robustness table under observation noise, missing data and distribution shift (p. 28) | Future | Perturbed observations fit only a rank-neutral SharpeArena probe, as for TraderBench in the benchmark audit; SharpeArena perturbs execution, not observations (`exec_noise.rs`). |
| 74 | What LLM Trading Agents Do in Production (DXRG) | A 17-rule methodology canon, choice stability across repeats, sizing against volatility and a zero-funding paper engine (p. 3-15) | **Adopted** (<<SB-PR-P5>>, <<SB-PR-P6>>, <<SB-PR-P7>>, <<SA-PR-P7>>, <<SB-PR-C1>>, <<SB-PR-C2>>) | Sixteen canon rules were already enforced or out of scope; the timing-luck floor, decision stability, finish-reason counts, the sizing-response diagnostic and short-borrow carry are built. |
| 75 | When AI Trading Agents Compete | Meta-order impact shape: square-root growth during execution and decay after it (p. 3-4) | **Adopted** (<<SA-PR-A8>>) | The concave-exponent wording in `market.rs` is corrected and a rank-neutral impact-shape probe is built; a transient-impact kernel stays Future. |
| 76 | XALPHA | Research memory from ingested reports with no availability date (p. 4-5, p. 18-19) | **Adopted** (<<SB-PR-P2>>, <<SA-PR-P2>>) | Idea-source provenance carried no date (`candidate_lineage.rs:82-89`); an optional source date, with counts of sources dated after each split and of undated sources, is built. |

## Limits

- The reading was AI-assisted. Each paper had one reader and no second full read, so a fact a
  reader missed is missing here. Where a reader disagreed with the earlier coding, the reader gave
  a page number, and the disagreement is listed above.
- Page numbers are PDF page numbers, which differ from printed page numbers in several papers
  (for example the market-making dissertation and Quant 4.0). The quotations were checked against
  the extracted PDF text; the other page references were not re-checked.
- The corpus is a convenience sample assembled by hand. It over-represents 2025 and 2026, so the
  counts describe this corpus, not the field.
- Each record describes the highest version of a paper held in the corpus on 16 September 2026.
  Authors revise papers, and a revision can change the fields coded here. The files named FutureX
  (v1) and (v3) are swapped relative to arXiv; the arXiv v3 text was read in full.
- The fields rest on the stated definitions, and some papers sit between labels. Mapping a reader's
  free text to a label, deciding which result is the headline, and counting simulated evaluation
  episodes as replications all affect the counts; the ledger cells keep the qualifiers so that the
  counts can be recomputed under other rules.
- Three product gaps rest on read-only checks made during the reading (seat order in the shared
  book, the meta-order impact path and the market-making mid path), not on any paper's claim.
- The transfer decisions were checked against the product source as it stood on 16 September 2026.
