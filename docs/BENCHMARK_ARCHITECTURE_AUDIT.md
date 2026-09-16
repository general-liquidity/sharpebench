# Benchmark architecture audit

This audit compares SharpeBench and SharpeArena with other evaluation benchmarks, in two
ledgers kept separate because their evidence differs. It is an architecture review, not a
leaderboard comparison. Each decision asks whether a mechanism closes a demonstrated gap in the
current trading-agent benchmark.

The source-tree ledger records benchmarks whose code was read: the 75 source trees supplied in
the workspace `benchmarks/` directory, six reference repositories reviewed separately in a
bounded pass, and 18 repositories from the workspace `bench-repos/` directory reviewed on
16 September 2026. Its evidence references point to repository names and source files in the
audited workspace. The purpose is to understand the products and extract useful architecture,
not to reproduce each upstream repository's release history.

The literature and landscape ledger records benchmarks known from their papers or public pages:
the 67-paper benchmark corpus behind the manuscripts' structural audit, a scan of finance,
trading and forecasting benchmarks and live trading arenas as of 16 September 2026, and 17
benchmark papers from the workspace `bench-papers/` directory read in full on that day. No code
was read for those rows, so each carries an evidence level and weighs less than a source-tree
decision.

## Decision rules

Source-tree ledger:

- **Adopted:** the audit exposed a current SharpeBench gap and the fix is now in
  the product with non-vacuous regression tests.
- **Already stronger:** SharpeBench already implements the transferable
  invariant with equal or stronger evidence.
- **Future:** useful only for a future hosted service, community task market,
  protocol revision, or explicitly controlled experiment.
- **Rejected:** not reproducible enough, depends on a mutable or model-judged
  core, or does not fit a trading-agent benchmark.

Literature and landscape ledger:

- **Candidate:** a mechanism the products lack that could be adopted now, with
  the gap it would close named. Not built.
- **Already covered:** the products implement the transferable idea with equal
  or stronger evidence.
- **Future:** useful only for a named future surface, or blocked by a known gap.
- **Not transferable:** relies on a model judge, a mutable core, or fixed gold
  answers with no stochastic outcome, or does not fit trading-agent evaluation.

Evidence levels in the literature ledger: `paper` means the construction,
protocol and metrics sections were read; `abstract` means the abstract only;
`page` means a public page was read directly; `summary` means a model summary of
the page was read, and the row should be checked against its source before it is
quoted.

The source-tree ledger records 12 adopted rows, 53 already stronger,
22 future and 11 rejected. The literature ledger records 60 already covered,
4 candidates, 20 future and 23 not transferable.

## What changed because of the audit

SWE-bench and WorkBuddy exposed the same missing invariant: infrastructure
failure must not shrink the experiment denominator. SharpeBench now closes that
gap at every relevant boundary.

- `SweepContract` binds dataset, costs, score configuration, runner, entrant,
  invocation, ordered windows, seeds, and retry policy before a checkpoint can
  resume. Entrant artifact identity and launch identity are separate digests,
  so supplying `--entrant-sha256` cannot make a changed command, endpoint,
  image reference, or environment pass-through list look like the same run.
- `TrajectoryContract` schema 2 binds dataset, costs, engine, runner, ordered
  windows, and seeds.
- Strict replay requires exactly one run for every declared cell in
  window-major and seed-major order. Missing, duplicated, extra, or reordered
  cells are refused.
- Every run must cover its full window. Step indices and observation dates must
  match the frozen dataset exactly. A short nonempty trace can no longer be
  padded with synthetic holds.
- Strict replay derives the execution-replicate grouping from the contract. An
  eight-seed capture cannot be silently rescored as one-seed market time.
- Exhausted runtime cells make a CLI sweep noncertifying. Text and JSON modes
  report expected, completed, runtime-failed, and agent-failed cell counts, emit
  no board, and exit unsuccessfully.
- Agent faults remain in the denominator through failing sentinel runs, with
  each sentinel using its own window length.
- Terminal checkpoint records are validated before assembly so inconsistent
  state cannot silently disappear.
- SharpeArena's bridge manifest now preserves per-request p50 and p95 inference
  latency, token counts, reasoning-token availability, retries, and observation
  sources as a rank-neutral operational profile. The bridge validates the
  accounting before SharpeBench receives a submission.

The audit also found and fixed a calibration error independent of any one
reference repository. Confidence and outcome vectors were flattened
separately, so unequal per-run lengths could pair a confidence from one run with
an outcome from another. Pairing now stops at each run boundary.

### 16 September 2026 review

On 16 September 2026, 18 repositories and 17 benchmark papers were reviewed
alongside a reading of trading papers. Each gap below was confirmed against
the cited source before its fix was built. The first six fixes come from the
two ledgers. The rest come from the trading-paper reading and are listed here
because they change SharpeBench.

- **Volatility-response sizing** (continuous-record-llm-trading-agents). No
  report related gross exposure to trailing realized volatility, so an agent
  holding constant exposure through volatility spikes looked the same as a
  volatility-targeted one. A rank-neutral sizing diagnostic now measures that
  response from point-in-time trailing volatility and is typed unavailable for
  constant exposure (PR <<SB-PR-C1>>).
- **Short borrow carry** (continuous-record-llm-trading-agents). Financing was
  charged only on gross exposure above 1x, so a 100% short paid no carry under
  any cost profile. An opt-in `short_borrow_bps` rate now charges it, is bound
  into the cost digest, and is omitted when zero, so existing cost digests and
  golden bytes do not move (PR <<SB-PR-C2>>).
- **Forecast common support** (ForecastBench). The forecast comparison
  intersected resolved contracts across every document, so one partial document
  shrank the common support of every pair. Common support is now taken over
  complete agents only, and an incomplete pair returns a typed inference error
  inside the Holm family (PR <<SB-PR-C3>>).
- **Stated confidence only** (ICBCBench). Three producers filled in a
  confidence of 0.5 that nobody stated, so a hold agent that places no orders
  reported a Brier score of 0.25 over 240 observations in the committed golden.
  Order confidence is now optional and the simulator pairs only stated
  confidences with outcomes (PR <<SB-PR-C4>>); SharpeArena's `score_returns`
  passes no pairs.
- **Duplicate period refusal** (QuantitativeFinance-Bench). The CSV importer
  stored a repeated period row as a second observation and the keyed scorer
  checked only the period count, so one period's return entered the mean,
  variance, PSR sample length and bootstrap twice.
  `RunIdentityError::DuplicatePeriod` now refuses a period repeated within a
  run, and the importer names the row (PR <<SB-PR-C5>>).
- **Bound arena intake** (Gençay, What survives honest evaluation?). The
  forward arena verified each commitment and then ranked returns the entrant
  supplied after the data-reveal epoch, so a track computed with hindsight
  would pass every gate, as the paper's planted look-ahead oracle passes
  deflation with DSR 1.00. Arena intake now binds scored returns to the
  committed artifact by trajectory contract and replay, each row discloses its
  evidence provenance, and the self-audit carries a leakage attack
  (PR <<SB-PR-F1>>).
- **Exposure-matched random timing** (Adaptive Alpha Weighting with PPO). The
  luck floor is a fully invested random allocator, so no report said whether a
  mostly flat entrant beats random timing at its own exposure and holding
  periods. A rank-neutral exposure-matched random-timing reference, computed by
  replay, now makes that comparison (PR <<SB-PR-P3>>).
- **Lagged replay** (LiveTradeBench; TradingGroup). Recorded decisions were
  never replayed with a delay. A rank-neutral lagged replay is now reported
  beside the board, valid only where the entrant's trades do not move the
  price (PR <<SB-PR-P4>>).
- **Timing-luck floor** (What LLM Trading Agents Actually Do in Production).
  No report showed how a result moves when the window start shifts by a few
  bars. A rank-neutral timing-luck floor over start offsets now shows it for
  the reference agents (PR <<SB-PR-P5>>).
- **Decision stability** (What LLM Trading Agents Actually Do in Production;
  Auditing AI Investment Recommendations). No report measured how often
  decisions change across replicate runs that saw identical observations. A
  rank-neutral decision-stability report over replicate runs, keyed by
  observation digest, now measures it (PR <<SB-PR-P6>>).
- **Finish-reason counts** (What LLM Trading Agents Actually Do in Production;
  Auditing AI Investment Recommendations). The model gateway carried a
  `finish_reason` for each call, but no report totalled it. Attempt accounting
  now counts finish reasons, and the counts are rank-neutral (PR <<SB-PR-P7>>).
- **Cell isolation disclosure** (R&D-Agent-Quant). Image entrants get a fresh
  container for each cell, but an `--http` endpoint or `--cmd` host process can
  keep state across the seeds of a window, and the board row did not say so. A
  contract-bound `cell_isolation` disclosure now travels on the row; it does
  not detect carried state (PR <<SB-PR-P8>>).
- **Expected shortfall** (Autonomous AI Agents for Option Hedging; Tail-Safe
  Hedging). SharpeBench reported no expected shortfall or CVaR, and downside
  deviation cannot separate frequent small losses from rare large ones. An
  opt-in, rank-neutral expected-shortfall and tail-count diagnostic now
  separates them (PR <<SB-PR-P9>>).
- **Constant tracks off the Pareto front** (FineFT). `pareto_optimal` marked a
  never-trading track optimal, because nothing can dominate zero drawdown and
  zero turnover, and the committed golden showed `hold` as Pareto optimal.
  Tracks the kernel refuses as constant are no longer Pareto candidates
  (PR <<SB-PR-P10>>).

## Statistical support made visible

Three diagnostics previously exposed estimates without their support:

- `calibration_observations` reports the exact confidence/outcome pairs behind
  the Brier score.
- `field_crowdedness_peers` reports how many defined peer correlations support
  crowdedness.
- `rolling_windows` reports how many rolling windows support the stability
  summary.

Wolfram Language checks confirmed how quickly uncertainty changes with support.
For a Bernoulli rate at 0.5, the standard error is 0.5 at `n=1`, 0.224 at
`n=5`, 0.158 at `n=10`, 0.100 at `n=25`, and 0.050 at `n=100`. Wolfram|Alpha's
95 percent Wilson interval for 50 successes in 100 trials is approximately
0.404 to 0.596. SharpeBench therefore reports denominators rather than imposing
one universal minimum that would have different meanings across diagnostics.

## Source-tree ledger

98 rows, in alphabetical order. Six of them, Deep-SWE-bench, Frontier-SWE,
FrontierCode audit, Hyper-Tau-Bench, OpenHands benchmarks and SWE-EVO, come from a separate
bounded review of the reference repositories; the four adopted among them are recorded in
`docs/audits/2026-09-09/PORT-RECONCILIATION.md` and in rows G28 and G31 of
`docs/audits/2026-09-09/IMPLEMENTATION.md`. Seventeen more cover the 18 repositories from
`bench-repos/` reviewed on 16 September 2026, with ForecastBench and forecastbench-datasets
sharing one row; the fixes behind the four adopted among them are listed under the
16 September 2026 review above.

| # | Repository | Transferable mechanism | Decision | Evidence and reason |
|---:|---|---|---|---|
| 1 | ALE-Bench | Bounded public feedback followed by one private final evaluation | Already stronger | SharpeBench precommits held-out evidence and includes search effort in deflation. See `README.md`, public/private evaluation and resource accounting. |
| 2 | AlphaForgeBench | Pass@k over generated strategy code, with metrics averaged over passing samples | Rejected | Pass^k with failing sentinels refuses its any-success reliability and survivor-only averages (`AlphaForgeBench/metrics.py:36`, `:356`); `backtest_runner.py:887-931` keeps a sample valid when symbol-years error. Its instability claim is covered by the re-execution divergence refusal. |
| 3 | ARC-AGI benchmarking | Record raw attempts, cost, tokens, and duration | Already stronger | Cost and attempt evidence are first class, and reliability is pass^k rather than any-correct-attempt success. Rank-neutral inference timing and token accounting now travel in the SharpeArena bridge manifest; see row 59. |
| 4 | ARC-AGI-3 benchmarking | Explicit terminal reasons and per-step budgets in an interactive loop | Already stronger | SharpeBench already records typed completion, resource, runtime, transport, and agent failures for local replay. |
| 5 | AssetOpsBench | Persist traces, then rescore them offline against ground truth | Already stronger | Capture and strict replay already separate generation from deterministic scoring without silently skipping unmatched evidence. See `docs/evaluation.md` and `docs/static-json-evaluation.md`. |
| 6 | AstaBench | Bind scorer identity independently from entrant runtime | Already stronger | Checkpoints and strict replay bind exact scorer, runner, entrant, data, and costs. |
| 7 | AutomationBench | Freeze a canonical task contract before mutable normalization and bind resumption to it | **Adopted** | Its explicit task contract exposed a narrower checkpoint gap: when an operator supplied `--entrant-sha256`, SharpeBench bound the artifact but not the command, endpoint, image reference, or environment pass-through list. `SweepContract` schema 2 now carries a separate invocation digest, and a same-artifact changed invocation is refused. See `automationbench/task_contract.py` and `automationbench/runner.py`. |
| 8 | Autoresearch Novelty Bench | Freeze the information frontier at time T | Already stronger | Forward windows and point-in-time observations precommit the available evidence without a model judge. |
| 9 | Backtrader-Bench | Code-verified multiple-choice items kept only when a no-tools model misses them | Already stronger | `mine_backtrader_questions.py:673-706` screens on one draw of one model, so a lucky guess discards a hard item and the pool fits that model. SharpeBench measures what an agent without skill achieves with never-ranked controls (`suite_controls.rs:1-31`) and a random-agent floor (`sharpebench-harness/src/lib.rs:551`). |
| 10 | bench | Common workloads across runtimes | Rejected | One warmup and one timed run with no cross-runtime result-equivalence gate is insufficient methodology. |
| 11 | BenchLocal | Versioned portable benchmark packs with staged activation | Future | Useful for distribution only after generation parameters and pack provenance are closed and signed. See `BENCH_PROTOCOL_V1.md`. |
| 12 | BigLaw Bench | Score analytical quality separately from source support | Future | Appropriate for a future trading-research evidence track, not the deterministic return-ranking kernel. |
| 13 | BikeBench | Retain individual feasibility violations beside performance | Already stronger | SharpeBench's conjunctive process, mandate, significance, and pass^k gates are noncompensatory. See `src/bikebench/benchmarking/benchmarking_utils.py`. |
| 14 | Browser-use benchmark | Separate evidence extraction from frozen valuation | Rejected | The published judge plumbing is incomplete and the core extraction is model-dependent. |
| 15 | CADTestBench | Demonstrate evaluator adequacy with intended faults | Already stronger | SharpeBench attacks the live scorer, proves the Sybil exposure with its defense disabled, proves the repaired verdict with it enabled, and plants invalid provenance cases by rule family. The supplied CAD snapshot contains no executable mutation harness. |
| 16 | CAR-Bench | Paired normal, ambiguous, and deliberately impossible tasks | Future | Useful for an agent-protocol abstention suite once task interaction is in scope; simulated users and policy scoring remain model-based. |
| 17 | CARLA driving benchmarks | Factorial route and weather conditions from immutable traces | Already stronger | Dataset, window, seed, cost profile, and replay contracts already define the complete trading condition grid. |
| 18 | Code Review Benchmark | Pair frozen historical evaluation with fresh prospective evidence | Already stronger | SharpeBench combines frozen replay with signed forward records and does not need a model judge. |
| 19 | CoffeeBench | Pre-generated exogenous shocks, a fixed apply order, and a floor and scripted reference in the focal seat | Already stronger | Delivery rolls share one RNG stream whose use depends on agent actions (`environment.py:780`). SharpeArena keys noise by seed and step (`exec_noise.rs:32-37`), folds flow in sorted agent order (`market.rs:20-23`) and attributes the agent's own impact (`manipulation.py:31-37`). Focal-seat evaluation against a fixed population stays Future with PillagerBench. |
| 20 | Conjecture Bench | Preserve supersession and provenance conflicts | Already stronger | SharpeBench evidence records are immutable and signed; the benchmark snapshot itself is a catalog, not an executable evaluator. |
| 21 | Conjecture Bench old | Exact verifier acceptance and honest zero baselines | Already stronger | SharpeBench has one deterministic kernel, cross-platform goldens, replay, and adversarial self-audit. |
| 22 | continuous-record-llm-trading-agents | Volatility-response sizing diagnostic from the decision trace, and borrow carry on unlevered shorts | **Adopted** | Its flat 5.0x median leverage across a 5.7x volatility spread, and its disclosure of zero funding as a paper-engine flaw (`README.md:18`, `paper.pdf` sections 5.1 and 11), exposed two gaps. No SharpeBench report related gross exposure to trailing realized volatility; a rank-neutral volatility-response sizing diagnostic now measures that relation from point-in-time trailing volatility and is typed unavailable for constant exposure (PR <<SB-PR-C1>>). `financing_cost_frac` charged only gross exposure above 1x, so a 100% short paid no carry under any profile; an opt-in `short_borrow_bps` rate now charges it, is bound into the cost digest, and is omitted when zero so existing cost digests and golden bytes do not move (PR <<SB-PR-C2>>). The 17-rule methodology canon is already met by current scoring; the capture gap and bracket counterfactual are Future. |
| 23 | Create Benchmark Service | Multi-tenant hosted control plane | Future | Quotas, streaming, cleanup, and tenant boundaries become relevant only if SharpeBench becomes a hosted service. |
| 24 | Create Benchmark Service (Vals.ai) | Fail-closed task and result projection, immutable artifact admission, declared dataset semver | Already stronger | `base.py:192-227` and `submission_artifacts.py:38-52` match `entrant_visibility.rs:1-17` and the digest-bound `checkpoint.rs:36-71`; its dataset version is a label whose content is not verified (`base.py:47`). Tenants, trial mode and quotas are a hosted control plane and stay out of scope. |
| 25 | CryptoAnalystBench | Claims checked against the context the agent received, plus a yes/no error taxonomy | Future | Every label is a judge call (`citation_hallucination_check.py:345-373`). SharpeBench's deterministic reason per gate (`disqualification.rs:32-65`) is already stronger than the taxonomy. A deterministic grounding check needs a protocol-v2 structured-claim field (literature row 102). |
| 26 | CryptoBench | Retire a saturated judge-scored question set for agent tasks | Already stronger | Scores bunched at 70 to 92 (`README.md:7`) and judge failures score 0 (`Scoring.py:305-306`). SharpeBench ranks a deflated Sharpe on committed forward windows and tests universal refusal with a witness. |
| 27 | Deep Research Bench | Separate report quality from citation support | Rejected | Unknown items are skipped, weights can be substituted, and the central judge is mutable and model-based. |
| 28 | Deep-SWE-bench | Treatment-activation evidence with a placebo arm, plus comparison and regrade receipts | **Adopted** | Its observational-memory arms and `harness/result_provenance.py` exposed that "memory enabled" does not show information reached a decision. `sharpebench-memory` now requires activation evidence at the decision boundary and compares a byte-matched placebo only under identical model, task and budget identities; comparison and regrade receipts declare the treatment axis and link a superseding grade to its source (implementation ledger G28, G31). Its missing-number-to-zero helper, `harness/analyze.py:72`, is rejected. Bounded review. |
| 29 | DI-Bench | Pair structural diagnostics with end-to-end execution | Already stronger | SharpeBench separates typed agent faults, retryable runtime failures, and excluded infrastructure failures. See `dibench/evaluate/evaluator.py`. |
| 30 | E3D-Bench | Multi-axis effectiveness, robustness, and efficiency report | Rejected | The supplied artifact is descriptive only and contains no evaluator, code, or data. |
| 31 | EdgeBench | Performance as a curve over a fixed consumed budget | Already stronger | SharpeBench already reports an OOS budget curve, marginal DSR, overfit onset, and selection-deflated peak. |
| 32 | FEA-Bench | Validate gold patches and report dataset attrition | Rejected | Reconstruction depends on mutable external repositories, unavailable data, and a patched external evaluator. |
| 33 | FinPersona-Bench | Placebo-controlled mandate reinjection and salience-decay arms | Future | A causal result requires control of prompt content, cadence, context reset, and token-matched placebo arms. SharpeBench does not own an external entrant's prompt schedule. An observational decay helper would not establish the claimed mechanism. |
| 34 | FlashInfer Bench | Correctness gates performance and environment metadata travels with results | Already stronger | SharpeBench does not silently skip workload errors or let performance compensate for failed correctness. |
| 35 | ForecastBench (with forecastbench-datasets) | A forecaster with missing forecasts is flagged and kept out of everyone else's comparison set | **Adopted** | Its leaderboard excludes a model more than 5% imputed (`leaderboard/main.py:577-603`, `resolve/_impute.py:36-43`), which exposed that SharpeBench's forecast comparison intersected resolved contracts across every document, so one partial document shrank every pair's common support and a pinned test charged the exclusion to the complete agent. Common support is now taken over complete agents only, and an incomplete pair returns a typed inference error inside the Holm family (PR <<SB-PR-C3>>). Two-way fixed-effects difficulty adjustment, information strata and naive reference forecasters are Future; bootstrap intervals and contamination control are already stronger. |
| 36 | ForecastBench-Sim | Worlds as plugins sharing one resolver; conditional forecasts by forking a checkpoint | Already stronger | SharpeArena shares SharpeBench's protocol, simulator and kernel under `SPEC_HASH` and runs paired counterfactual arms on one seed. The fbsim resolver fails open (`resolver.py:120`) and its difficulty estimate is circular (`difficulty.py:280-343`); both are rejected. |
| 37 | FormalQualBench | Machine-check critical invariants under an explicit trust boundary | Future | Formal verification of a small scorer kernel may be worthwhile, but theorem validity cannot substitute for empirical calibration. |
| 38 | Frontier-SWE (v1 and v2) | Trusted rescore of a frozen submission bundle, a trial census, and typed pipeline controls | **Adopted** | V2 verifies declared deliverables in a separate container from protected evaluator paths and labels its oracle a pipeline-validation bypass rather than a ceiling (`tasks/optimizer-design/task.toml:3`, `:30`, `:44`). SharpeBench added a trial census and typed suite controls whose verdicts never enter the competitive score (G28, Bench PR #105). V2's warn-only frozen-file check and non-failing preflight summary were not ported. Bounded review of 12 files. |
| 39 | FrontierCode audit | Blind second-coder agreement on outcome labels | Future | Its 98 coded closures with blind-pack and second-coder records are the model for rater agreement. SharpeBench ships agreement statistics but has no second rater at a size worth reporting; this belongs to a practitioner grading study. Its own evidence chain was found internally inconsistent on the Diamond deprecation, and no implementation was copied. |
| 40 | GenAI-Bench | Operational load and tail-latency plane | Future | Useful for a hosted serving surface, not trading correctness or rank. |
| 41 | Harness Bench | Paired harness/model matrix | Already stronger | SharpeBench already binds execution identity; a one-task, five-run LLM-graded matrix would be weaker evidence. |
| 42 | Harness Bench Fast | Versioned task semantics and resumable attempts | Already stronger | Strict contracts prevent cross-version resume and require the complete evidence matrix. |
| 43 | Harness Bench real-repository variant | Negative control fails and reference solution passes before task admission | Future | Valuable if community-contributed scenarios are admitted. Current frozen scenarios already have deterministic controls and self-audit. |
| 44 | HarnessBench | Fixed-model scaffold ablation with uniform traces | Future | Requires an attested runner that fixes model, sampling, transport, and prompt while varying only the harness. Current harness identity alone does not create that experiment. |
| 45 | HealthBench | Combine positive and negative criteria, hard subsets, and bootstrap support | Already stronger | SharpeBench's process and mandate checks include positive obligations and prohibited behavior, while dependence-aware bootstrap and explicit support counts avoid treating rubric points as independent. Its deterministic kernel removes the mutable-judge dependency. See `judge.py` and `README.md`. |
| 46 | Hyper-Tau-Bench | Host-side model gateway, frozen fault manifests, and a re-execution contract | **Adopted** | 45 port decisions: take 6, adapt 13, reject 23, defer 3. Every accepted row is built or closed with a reason, and eleven rejections cite a stricter existing Sharpe mechanism. See `docs/audits/2026-09-09/PORT-RECONCILIATION.md`. |
| 47 | ICBCBench | Explicit missing-confidence rule beside a binned calibration error | **Adopted** | Its evaluator never treats a missing confidence as stated (`objective_eval/prompts.json:7`, `metrics.py:65-73`), which exposed that SharpeBench filled in 0.5 in three producers: `Order.confidence` defaulted to it, a step with no orders pushed it, and SharpeArena's kernel scorer filled it for every bar, so `calibration_brier` and `calibration_observations` counted confidences nobody stated. Order confidence is now optional and the simulator pairs only stated confidences with outcomes (PR <<SB-PR-C4>>), SharpeArena's `score_returns` passes no pairs, and the change is rank-neutral. Binned calibration is already stronger in `forecast.rs:1036-1063`. |
| 48 | IDE-Bench | Clean-container attempt independence and oracle validation | Already stronger | SharpeBench requires every declared window and seed to pass instead of rewarding one lucky attempt. |
| 49 | Interfaze Complete Benchmarks | Persist generation output for independent rescoring | Already stronger | Raw decisions are captured and replayed through the frozen engine under a strict contract. |
| 50 | JudgmentBench | Cluster-first resampling, evaluator agreement, and near-tie handling | Already stronger | SharpeBench resamples at the declared dependence unit, reports paired intervals and multiplicity-adjusted tests, and uses one deterministic scorer instead of estimating agreement with a model evaluator. See `analysis/analysis_vSubmit.R`. |
| 51 | KernelBench | Correctness gate before baseline-relative efficiency | Already stronger | Performance cannot compensate for failed SharpeBench eligibility or safety gates. |
| 52 | LAB-Bench | Task-family reporting | Rejected | Exact-answer multiple choice is routed through a nondeterministic model judge with a very small golden sample. |
| 53 | LabBench2 | Stage deterministic validation, preserve failed tasks, and patch retries rather than overwrite them | Already stronger | SharpeBench separates contract, transport, process, outcome, and statistical stages; failed cells remain in the declared geometry; and retries are append-only per cell, so a completion is recorded after the attempts it superseded rather than over them. That last clause was aspirational when written: the retry path returned only the completed run and the checkpoint stored only the terminal outcome, so a cell that failed twice before completing reported the cost of the completion alone. It became true with the attempt ledger. See `evals/evaluators.py`, `evals/run_evals.py`, and `evals/report.py`. |
| 54 | Live Trade Bench | Longitudinal forward decision provenance | Already stronger | SharpeBench forward records are signed and replayable; terminal-return-only summaries are weaker. |
| 55 | LocalBench | Normalize task records, retain answer/refusal rates, and report subgroup quality | Already stronger | Closed submission schemas, typed failure categories, complete denominators, and dataset/condition diagnostics already make refusal and subgroup behavior visible without a mutable judge. See `benchmark.py`, `loader.py`, and `metrics/answer_rate.py`. |
| 56 | MA-ProofBench | Require complete outputs, resume missing samples, statically precheck, then execute a proof witness | Already stronger | SharpeBench retains every declared cell, distinguishes retryable infrastructure failure from entrant failure, replays executable evidence, and carries selected scorer invariants in Lean. Pass^k is deliberately stricter than pass@k. See `evaluation/main.py` and `evaluation/checks.py`. |
| 57 | MCP-Bench | Multi-server tool dependency tasks | Rejected | Relevant to Gordon, not SharpeBench ranking, and the overall score depends on a fixed model judge. |
| 58 | MLE-Bench | Resource budgets and difficulty strata | Already stronger | SharpeBench exposes budget curves and condition-level diagnostics without retaining known data leaks to preserve a board. |
| 59 | MU-Bench | Publish tail latency and resource accounting beside quality while keeping it out of rank | **Adopted** | SharpeArena raw-field schema 2 records every inference duration and its observation source. Bridge schema 2 validates call counts and totals, then publishes nearest-rank p50/p95 latency, token totals, reasoning-token provenance, and retries with `rank_input: false`. Locale macro-averaging and model-judge controls do not fit this deterministic trading scorer. See `scripts/latency_stats.py`, `scoring/metrics.py`, and `scripts/significance_test.py`. |
| 60 | NanoGPT-Bench | Fixed-resource optimization with repeated confirmation | Already stronger | SharpeBench combines selection-deflated budget curves with a digest-pinned, non-root, no-network, no-IPC container boundary. |
| 61 | OpenHands benchmarks | Separate inner infrastructure retries from outer attempts, with per-attempt cost | Already stronger | SharpeBench's attempt ledger appends every retry per cell and separates retryable runtime failure from entrant failure. Its contribution example skips recoverable instances and its Hybrid-Gym reporters derive totals from submitted rows, so any reuse would have to keep the declared denominator. `benchmarks/utils/` read in full; no implementation copied. |
| 62 | PAST-Bench | Longitudinal memory ablation | Already stronger | `sharpebench-memory` already requires baseline, retrieval, and oracle arms plus significance, poisoning, point-in-time, multisession, and confabulation checks. |
| 63 | PillagerBench | Opponent-policy matrices | Future | Relevant only when crowding becomes an active strategic multi-agent environment rather than a field diagnostic. |
| 64 | PortBench | Profile-specific crisis-window drawdown gate and the CEPS stage-propagation score | Already stronger | Per-window pass^k plus a per-run drawdown bound covers the gate without a silent fallback window (`stress_scenarios.py:215`). CEPS scores stages against ex-post answers (`stages.py:992`) through a view inside the agent that the closed contract excludes. Asset-class profile caps are Future until a multi-class dataset exists; paired stage interventions are Future with an attested runner. |
| 65 | PostTrainBench | Separate anti-reward-hacking review | Already stronger | SharpeBench structurally blocks lookahead and tests live scorer attacks rather than relying on a post-hoc model judge. |
| 66 | Proof Bench | Independently executable success witness | Already stronger | The verifier replays every raw decision and recomputes all returns and gates instead of trusting a narrative result. |
| 67 | QuantCode-Bench | Stage-specific compile, execute, trade, and semantic diagnostics | Rejected | Generated code is unsandboxed, data can fall back to mutable downloads, and semantic grading is intentionally lenient. |
| 68 | QuantitativeFinance-Bench | Planted dirty-data classes applied to the harness input path | **Adopted** | Its planted duplicate dates (`tasks/historical-var-data-prep/instruction.md:24`) exposed that SharpeBench's CSV import stored a repeated period row as a second observation and the keyed scorer checked only the period count (`import_cmd.rs:512-515`, `run_identity.rs:292-300`), so one period's return entered the mean, variance, PSR sample length and bootstrap twice; `csv_columns.rs:124-128` already refused a duplicate period identity on the diagnostic path. A period repeated within a keyed run is now refused with `RunIdentityError::DuplicatePeriod`, and the importer names the row (PR <<SB-PR-C5>>). Numeric tolerance checks and oracle review are already stronger through exact replay and positive controls; best-of convention paths with 50/50 partial credit (`verifier.py:538-548,621-651`) are rejected. |
| 69 | QuantumLean-Bench | Pair human-legible rationale with a machine-checked artifact | Future | A rationale-consistency diagnostic needs a specified causal or semantic contract. Free text is intentionally score-neutral today. |
| 70 | Qwen DianJin | Adversarial interventions at declared entry surfaces, judged from agent-attributed side effects, attack success reported apart from capability | Future | Effect-based verdicts, presentation-only tampering from a frozen plan, a separate attack success rate and paired placebo or poisoning arms are already stronger (`fault_plan.rs:7-20,59-63`, `fault_modes.rs:3-7`, `poisoning.rs:39-42`, `activation.rs:18-22`). DianJin-RED sends most verdicts to an LLM and drops judge errors from the denominator. Instruction injection into the `news` channel judged by mandate breach stays Future beside literature row 107 because no entrant reads text. |
| 71 | r-lib bench | Require semantic equality before comparing performance | Already stronger | Golden replay establishes equivalence before cost and performance diagnostics. GC-filtered microbenchmark timing is not the product's unit of analysis. |
| 72 | RAD-Bench | Diagnose multi-turn retrieval decay and correlate a benchmark with an external ranking | Future | Turn-conditioned evidence could support a future memory or research-agent track, but the current return and forecast protocols do not define retrieval turns. Adding the metric now would create an unscored surface, and the supplied evaluator is weaker than SharpeBench's closed contracts. See `rad_bench/conversation.py` and `rad_bench/gen_judgment.py`. |
| 73 | Ratel Bench | Baseline, retrieval, and oracle arms with cost-normalized lift | Already stronger | This design already ships in `sharpebench-memory`, including oracle headroom and paired significance. |
| 74 | React Grab Bench | Controlled intervention with shortcut removal | Already stronger | SharpeBench's ablations and held-out windows isolate treatments with repeated seeds rather than one trial per case. |
| 75 | ResearchCodeBench | Measure context ablations and contamination by information cutoff | Already stronger | SharpeBench freezes point-in-time observations, precommits forward windows, separates historical from prospective evidence, and records ablations without allowing them to alter the rank key. See `core/generate_solutions.py` and `visualize/contamination_knowledge_cutoff_merged.py`. |
| 76 | SEC-bench | Preserve raw exploit evidence before classification | Already stronger | SharpeBench retains decisions and typed failure evidence; it also distinguishes OOM from generic exit 137. |
| 77 | SEC-bench-Pro | Counterfactual replay across vulnerable, fixed, and latest environments | Future | Useful for planned policy and cost-profile ablations only if verdicts remain deterministic and fail closed. Its default can count uncertain outcomes as successes. |
| 78 | SOP-Bench | Separate infrastructure completion from decision quality | Already stronger | Typed runtime, agent, process, and outcome channels already provide a less parser-dependent split. |
| 79 | StockBench | Explicit exclusion of decision-day data | Already stronger | Point-in-time observations make future data unrepresentable at the agent boundary, with repeated windows and seeds. |
| 80 | SWE-bench | Keep the full task universe and additive error identities | **Adopted** | This exposed the fail-closed execution-matrix gap fixed above. See `swebench/harness/reporting.py` and `grading.py`. |
| 81 | SWE-CARE | Keep unevaluated instances in the denominator and disclose context-source ablations | Already stronger | Missing execution cells make a SharpeBench sweep noncertifying, while entrant faults remain as failing sentinels. Context and data identities are bound into evidence rather than inferred from successful rows. See `scripts/eval_report.py` and `scripts/run_eval_pipeline.py`. |
| 82 | SWE-EVO | Versioned multi-stage tasks with preservation obligations | **Adopted** | Release-pair construction (`make_hf_dataset.py:43`) exposed that multi-stage scenarios need declared carryover. Scenario-transition manifests now declare fresh-episode or continuous-portfolio carryover, refuse an undeclared mode, and build each stage from facts dated no later than its effective date (G31). The reviewed code shows version-pair instances, not executed state carryover, and is credited only for the former. |
| 83 | SWEBenchBenchmarkService | Hosted evaluator API with setup, streaming, and aggregation boundaries | Future | Relevant only to a hosted SharpeBench service. Mutable image tags and unsupported evaluation modes are not acceptable in the current kernel. |
| 84 | tau-bench | End-state correctness and all-trial reliability | Already stronger | Raw decisions need not imitate one path, while pass^k requires reliability across all declared cells. |
| 85 | tau2-bench | Outcome scoring with only invariant process gates | Already stronger | This is already the SharpeBench split: outcomes determine performance, while only risk, process, and mandate invariants gate eligibility. |
| 86 | Tax Agent Bench | Deterministic citation lookup under an as-of date, with a held-out scored set | Future | Citation lookup reports resolution depth (`authority.py:240-247`) but is a tool, not a scoring check. Checking agent claims needs protocol-v2 structured claims; the as-of boundary and held-out split are already stronger. |
| 87 | taxeval | Single-sample multiple-choice accuracy parsed from free text | Rejected | `main.py:56` takes the first digit it finds and runs once at default temperature. SharpeBench refuses malformed decisions as typed agent faults and requires a pass on every seed and window. |
| 88 | Terminal-Bench 1 | Version scorer, oracle, task, and environment together | Already stronger | Strict contracts and provenance already bind the complete evaluation identity. The beta lineage should not be mixed with later Harbor boards. |
| 89 | Terminal-Bench 2 | Human and model task qualification before release | Already stronger | Deterministic fixtures, planted invalid cases, and live attack tests provide executable qualification without an external Harbor dependency. |
| 90 | Terminal-Bench 2.1 | Treat reward-hack repairs as benchmark-version changes | Already stronger | SharpeBench goldens, self-audit, and provenance make scorer changes explicit and non-comparable when semantics move. |
| 91 | Terminal-Bench benchmark service (Vals.ai) | Separate verifier sandbox, hostile-archive staging, verifier faults typed apart from agent failure, reward range guard | Already stronger | See `isolated_verifier.py:57-62,172-240`. SharpeBench crosses only closed JSON, scans archives without extracting (`artifact_tar.rs:1-9`), and makes runtime failures noncertifying, whereas this service still scores verifier faults 0.0 (`benchmark_service.py:308-324`). |
| 92 | Terminal-Bench current | Continuous community task-admission pipeline | Future | Relevant only if SharpeBench accepts community scenarios. Any adoption must pin dataset release, harness, and environment instead of `latest`. |
| 93 | Terminal-Bench Science | Validator registry, planted negatives, valid controls, and full-corpus nonempty checks | Future / partially present | The strongest future task-admission reference. Its rule-level planted-negative idea is already applied to provenance validation; the full community workflow is not needed yet. |
| 94 | TraderBench | Evaluator-only scenario data and unseen market windows | Already stronger | SharpeBench binds held-out windows, scorer configuration, and forward evidence into public commitments. |
| 95 | Trata Hedge-Bench | Rubric of themes and moves with a sparse all-themes reward and claim grounding in a closed corpus | Rejected | Scoring is a Gemini judge that fails open (`grade.py:136-138`, `test.sh:34-37`) and per-task grader copies have drifted into 93 and 9 variants. Sparse conjunctive reporting is already SharpeBench's gate design, and citation grounding is Future as in literature row 102. |
| 96 | Turbopuffer benchmark | Cold/warm state and tail-latency workload envelopes | Future | Appropriate for hosted operational diagnostics, not agent correctness or trading skill. |
| 97 | Urbit benchmark | Attach host and runtime metadata to performance evidence | Future | Optional entrant environment metadata can aid diagnosis, but reporter-controlled hardware must not become a rank input. |
| 98 | WorkBuddy Bench | Refuse missing tasks, scores, plan items, and shrunken denominators | **Adopted** | This independently exposed the same evidence-geometry gap as SWE-bench. The current fix makes partial execution explicitly noncertifying. |

## Literature and landscape ledger

107 rows, in alphabetical order. Seventeen of them record benchmark papers read in full on
16 September 2026: 13 rows are new, and AlphaEval, AlphaQT-Bench, Backtrader-Bench and Gençay
were raised to `paper` evidence from an earlier summary or abstract. Where a paper describes a
benchmark that also has a source-tree row, the reason names that row; the two rows can
disagree, because the paper and the code do not always describe the same mechanism.

| # | Benchmark | Domain | Source | Evidence | Transferable mechanism | Decision | Reason |
|---:|---|---|---|---|---|---|---|
| 1 | Agent Market Arena (When Agents Trade) | trading | arXiv 2510.11695v2 | paper | Live daily paper trading of four agent frameworks crossed with five backbones | Already covered | Forward windows with commit and reveal give the same post-cutoff evidence, while AMA ranks one two-month run per cell by return with no costs stated for its results table, no intervals and 252-day annualization on crypto; its six identical HedgeFundAgent rows across five backbones and a vote show why a backbone claim needs gateway or activation evidence. |
| 2 | AI-Trader | trading | arXiv 2512.10971 | summary | Live, time-gated multi-market paper trading across LLMs | Already covered | Forward windows with commit and reveal give the same contamination resistance, and SharpeBench ranks nothing on a single run, where AI-Trader ranks one run on cumulative return. |
| 3 | Alpha Arena (Nof1) | trading | nof1.ai | page | Real-money live instances per model, ranked by account value | Not transferable | Ranking one live instance on account value is the evaluation SharpeBench exists to refuse; real capital adds no statistical evidence. |
| 4 | AlphaBench | trading | ICLR 2026 | summary | Factor-mining evaluation with bear and bull sub-periods | Already covered | Factor search is where a search correction matters most and none was found; deflation by configurations tried is already a gate. |
| 5 | AlphaEval | trading | arXiv 2508.13174v2, KDD 2026 | paper | Backtest-free factor quality: stability, noise robustness, eigenvalue-entropy diversity | Not transferable | Scores factor properties rather than the outcomes of an agent's decisions, one of its five dimensions is a GPT-4o judge, and its composite is z-scored within each round under unstated weights; in its own table the random reference scores the highest diversity and beats three miners on robustness. |
| 6 | AlphaForgeBench | trading | arXiv 2602.18481, KDD 2026 | summary | Stochastic LLM generation followed by deterministic strategy execution | Already covered | See source-tree row: the same split is SharpeBench's; it reports five-generation variance to justify ranks but applies no deflation, no second window and no gate. |
| 7 | AlphaQT-Bench | trading | ACL 2026 Findings, pp. 2871-2886 | paper | Dynamic truncation test: outputs on five truncated prefixes must equal the full-series outputs exactly | Already covered | SharpeArena already asserts prefix equality for its observation wrappers (`test_indicators.py:125`, `test_obs_extra.py:93` and `:151`, `test_conformance.py:145`) and SharpeBench makes future bars unrepresentable; the same test on entrant-written strategy code fits only a future code-generation track, and the paper's functional-accuracy rule (correlation above 0.999 or NRMSE below 0.001) is weaker than byte-exact goldens. |
| 8 | ARC-AGI-2 | other | arXiv 2505.11831 | paper | Count repeated score disclosure on a hidden set as a leakage channel | Already covered | Forward windows scored once after commit/reveal and a deflated Sharpe that counts every configuration tried already bound the feedback leakage ARC-AGI-2 attributes to ARC-AGI-1's reused private set, and its human-solvability calibration belongs to the separate no-human-baseline gap. |
| 9 | ARC-AGI-3 | agents | arXiv 2603.24621 | paper | Admit an environment only if random play wins below a bounded probability | Already covered | See source-tree row ARC-AGI-3 benchmarking: SharpeBench's thousand-random-agent luck floor already measures the deflated Sharpe tail of seeded random agents on the scored windows, while ARC-AGI-3's human-normalized efficiency score depends on the known missing human baseline. |
| 10 | AssetOpsBench | agents | arXiv 2506.03828 | paper | Cross identical models with two orchestration paradigms to isolate scaffold effects | Future | See source-tree row: the Agent-As-Tool versus Plan-Execute crossing targets the known gap that model and scaffold effects are not separated, but it needs a populated frontier-model field and its headline scores come from a Llama judge. |
| 11 | AstaBench | science | arXiv 2510.21652 | paper | Price logged token usage with a frozen cost map and report score-cost Pareto frontiers | Future | See source-tree row: SharpeArena already carries rank-neutral token accounting, but pricing it on a cost frontier needs the populated frontier-model field and touches the known gap that inference cost never enters rank. |
| 12 | Auditing AI Investment Recommendations | trading | trading-papers corpus | paper | Frozen, replayable audit of model advice as executable actions | Already covered | Strict replay recomputes performance from recorded decisions against frozen inputs. |
| 13 | Automated LLM Speedrunning Benchmark | science | arXiv 2506.22419 | paper | Crossed model, scaffold, hint and seed factorial at an equal search budget | Future | Its 4 models by 5 scaffolds by 6 hint regimes by 3 seeds design, with a flat best-of-M control at a fixed 20-node budget, is a template for the known gap of separating model and scaffold effects once a frontier-model field exists. |
| 14 | AutomationBench | agents | arXiv 2604.18934 | paper | Run optimization against the grader to surface reward hacks before release | **Candidate** | See source-tree row: SharpeBench's self-audit is a regression suite over nine named attacks and says it is not a proof that no entrant can game the scorer (`docs/book/src/governance.md:22`); both scoring fail-opens closed in 2026-09 were found by review, not by the suite, so a search without model calls for submissions that pass the gates without skill would test attacks nobody has named. |
| 15 | BacktestBench | trading | arXiv 2605.17937v2, KDD 2026 | paper | Two independent implementations of the metric engine must agree before use | Already covered | SharpeBench computes every statistic in one deterministic kernel checked against cross-platform goldens, and the paper's finding that the best model computes Sharpe correctly in 31.7% of cases shows why entrants never report their own metrics; its selection tasks reward the in-sample best of about 200,000 strategies at zero cost. |
| 16 | Backtrader-Bench | trading | arXiv 2608.11232v1, IJCAI 2026 FinLLM Workshop | paper | Questions generated at runtime under a seed so answers never appear online, each re-derived by an independent backtest re-run | Already covered | See source-tree row for its question-mining filter. SharpeArena's sealed seeds, committed before submissions close, are stronger: Backtrader-Bench regenerates from a public seed over public AAPL 2020 to 2024 prices and releases its 160-question pool, so its items are enumerable. Its re-run checker matches strict replay, and its no-tools accuracy drops tool-attempt questions from the denominator. |
| 17 | BikeBench | other | arXiv 2508.00830 | paper | Report every score under a standardized evaluator-call budget bracket | Already covered | See source-tree row: selection-deflated budget curves and the attempt ledger already charge search effort against the evidence, whereas BikeBench's 0 to 1B evaluation brackets are only a leaderboard filter. |
| 18 | c-CRAB | software | arXiv 2603.23448 | paper | Convert human review intent into fail-then-pass tests executed after a fixed downstream agent | Not transferable | The oracle is a fixed human-derived test and scoring routes through a second coding agent, so a review is judged by another model's revision, not by an outcome SharpeBench could replay deterministically. |
| 19 | CADTestBench | other | arXiv 2605.07807 | paper | Accept a test suite only if invariance-augmented references pass and every mutant dies | Already covered | See source-tree row: SharpeBench already plants invalid provenance cases by rule family and proves its Sybil defense by switching it off, while the paper's test suites and mutants are written by an LLM and refined for at most four rounds. |
| 20 | CarBench | science | arXiv 2512.07847 | paper | Paired stratified bootstrap over shared test units to resolve adjacent rank order | Already covered | SharpeBench already resamples at the declared dependence unit with a stationary block bootstrap and reports paired intervals, and pass^k covers the seed-to-seed variability that CarBench's single-training-run intervals explicitly omit. |
| 21 | CLQT | trading | arXiv 2606.29771 | summary | Noise band from repeated runs; dominance required across sub-periods | Already covered | pass^k across disjoint windows makes consistency a condition of eligibility rather than a diagnostic. |
| 22 | CodeFuse-CR-Bench | software | arXiv 2509.14856 | paper | Oracle versus BM25 top-k retrieved context arms under one scorer | Already covered | Ships from the codefuse-ai/SWE-CARE repository that the source-tree SWE-CARE row names, sharpebench-memory's baseline, retrieval and oracle arms with placebo controls already cover context-source arms, and the review score itself rests on a reward model plus an o3 judge. |
| 23 | CoffeeBench | agents | arXiv 2606.16613v1 | paper | Environment-owned net-income ledger valuing goods at weighted-average cost so self-dealt prices cannot raise the score | **Candidate** | See source-tree row for the shock and apply-order design. SharpeArena's LOB market-making reward marks inventory to a book mid set by the agent's own resting quotes (`lob_env.py:186-201`, `lob_market.rs:362-369`), and the matching engine allows self-trades; the focal-seat-among-background-firms design is the PillagerBench future row, and CoffeeBench's board ranks on three runs with no interval. |
| 24 | Commit0 | software | arXiv 2412.01769 | paper | Grade by re-running tests on a clean clone of the final commit | Already covered | Replay already recomputes every return and gate from recorded decisions instead of trusting entrant-side results, and Commit0's unlimited interactive access to the grading tests would leak the evaluation in a trading benchmark. |
| 25 | ConjectureBench | science | arXiv 2510.11986 | paper | Score with and without the intermediate answer supplied to expose inflated end-to-end accuracy | Not transferable | Possibly the project behind source-tree row Conjecture Bench; it grades fixed gold conjectures with an LLM judge and pass@k, which has no stochastic market outcome and no counterpart in SharpeBench's deterministic kernel. |
| 26 | ContextBench | software | arXiv 2602.05892 | paper | Trace inspected versus used context against human-annotated gold context sets | Not transferable | Scoring depends on fixed human gold contexts per issue, and a trading decision has no unique gold set of observations to score recall against. |
| 27 | CR-Bench | software | arXiv 2603.11078 | paper | Signal-to-noise of findings exposes the recall-precision frontier hidden by resolution rate | Not transferable | Every finding is classified against one gold defect by an LLM judge, so the score depends on a model judge and fixed answers with no stochastic outcome. |
| 28 | CryptoAnalystBench | trading | arXiv 2602.11304v1 | paper | Claim-level grounding of long-form analysis against the tool trace (exact, derived, fabricated) plus a seven-category failure taxonomy | Not transferable | See source-tree row: claim extraction, verification and the rubric all run through a DeepSeek V3.1 judge whose weighted kappa against experts is 0.35 to 0.42, with one response per query; the failure taxonomy is already matched by typed failure channels and SharpeArena's F7 taxonomy, and a deterministic structured-claim check belongs with the future trading-research evidence track. |
| 29 | CryptoBench | trading | arXiv 2512.00417v5 | paper | Expert-written templated questions refreshed monthly, split into retrieval and prediction quadrants | Already covered | See source-tree row: sealed procedural seeds and forward windows with commit and reveal supply fresh evidence, and forecast quality is already scored apart from trading rank; CryptoBench reports one October to November window across all five versions and one run per model, uses an unnamed LLM judge with a 5% tolerance, and runs one model through a web interface. |
| 30 | DeepResearch Bench | agents | arXiv 2506.11763 | paper | Score reports relative to a reference report under judge-generated weighted criteria | Not transferable | Appears to be the same benchmark as source-tree row Deep Research Bench (name spaced differently), and both RACE and FACT route scoring through a mutable Gemini judge against a Gemini-generated reference report. |
| 31 | DriveNetBench | other | arXiv 2505.01893 | paper | Gate scoring on logged calibration error of the measurement instrument | Already covered | SharpeBench's cross-platform golden fixtures and digest-bound contracts check the scoring instrument before any board, which is stronger than a homography error threshold shown only on four illustrative human-driven trials. |
| 32 | E3D-Bench | other | arXiv 2506.01933 | paper | Report in-distribution and extreme out-of-distribution splits separately, beside latency | Not transferable | See source-tree row: scoring is against fixed ground-truth depth, pose and point clouds with no stochastic outcome, and SharpeBench's disjoint market windows already fill the distribution-shift role. |
| 33 | EconEvals | economics agents | arXiv 2503.18825v4 | paper | Litmus tests reporting a tendency beside a competency score on the single-objective variant and a reliability score across runs, with the multi-agent game converted to a solo game for the competency run | **Candidate** | The solo conversion transfers now as a seat-removal externality in SharpeArena's shared-book manipulation probe, which reads only the manipulator's outcome (`manipulation.py:476`); the full litmus triple needs an objective-pair vocabulary and a model field. The reading also found that `econrationality.rs:1` credited EconEvals for dominance and money-pump checks that no version of the paper contains. |
| 34 | EdgeBench | agents | arXiv 2607.05155 | paper | Compare one continuous run with best-of-n independent restarts at equal budget | Already covered | See source-tree row: SharpeBench already deflates for configurations tried and reports a selection-deflated peak on its budget curve, so a lucky best attempt cannot pass as improvement. |
| 35 | Excel Modeling Benchmark (Vals.ai) | finance knowledge | vals.ai/benchmarks/emb | page | Recalculate every submission in the real engine before grading | Already covered | SharpeBench recomputes every submitted decision through the frozen simulator rather than accepting reported results; the scratch-mode rubric is LLM-judged and not transferable. |
| 36 | FEA-Bench | software | arXiv 2503.06680 | paper | Report edit-application success separately and ablate output format against resolved rate | Already covered | See source-tree row: closed JSON contracts already record malformed or undeliverable submissions as typed failed cells, separate from outcome, and FEA-Bench's GPT-4o intent filter adds a model-judged step to task construction. |
| 37 | Finance Agent v2 (Vals.ai) | finance knowledge | vals.ai/benchmarks/fabv2 | page | Public, licensable validation and never-shared test tiers with published correlation | Future | SharpeBench has no validation tier; a licensable window pool with published validation-to-test correlation needs permission-compatible data first. Its three runs per model are reported, not gated. |
| 38 | FinDeepForecast | forecasting | arXiv 2601.05039 | summary | Live deep-research forecasting on financial questions | Not transferable | Point accuracy on forecast questions without inferential statistics; SharpeBench's forecast report already scores proper rules outside the trading rank. |
| 39 | FinEvo | trading | trading-papers corpus | paper | Many Monte Carlo runs of an ecological market game with confidence intervals | Already covered | SharpeArena's ecology probe was replicated across seeds and reports that single-run narratives do not survive. |
| 40 | FinMCP-Bench | finance tool use | arXiv 2603.24943v1 | paper | Tool precision, recall, F1 and exact grouping match against reference MCP call sets drawn from production logs | Not transferable | Scores tool names against one reference path under teacher-forced gold history, with no outcome and no argument check, from one run per model; SharpeArena's canonical parser already makes call validity deterministic on its MCP surface and SharpeBench's gateway journals host-observed calls, as in the MCP-Bench row. |
| 41 | FinPersona-Bench | trading | arXiv 2606.31522 | paper | Synthetic market with hidden fundamental value gives objective per-decision rationality ground truth | Future | See source-tree row: SharpeArena's shared-book market already has an exogenous fundamental component, so value alignment could become a reported-only Arena probe, but it would measure fidelity to a synthetic generator rather than skill on market windows. |
| 42 | FinRL-Meta | trading | arXiv 2112.06753 | paper | Train, validate, backtest, then paper-trade a later window to confirm consistency | Already covered | Forward windows with commit/reveal already give signed out-of-sample confirmation, while FinRL-Meta reports one run per agent with no seeds, significance test or deflation. |
| 43 | FINSABER | trading | arXiv 2505.07078, KDD 2026 | summary | Point-in-time universes including delisted stocks over two decades | Future | SharpeBench's fixed universes were selected with hindsight and do not close survivorship; this belongs with a keyed single-name equity dataset. Its paired t-tests carry no multiplicity correction. |
| 44 | FinTradeBench | finance knowledge | arXiv 2603.19225v5 | paper | LLM judge calibrated to an expert on a seed set, with RAG, no-RAG and precomputed-context arms | Not transferable | Scores fixed gold answers through a Claude judge whose rank agreement with its human expert is rho 0.07; its retrieval arms are already covered by sharpebench-memory. |
| 45 | FinWorld | trading | trading-papers corpus | paper | All-in-one research platform averaged over three seeds | Not transferable | A platform rather than an evaluation protocol; seeds are averaged, not gated. |
| 46 | FlashInfer-Bench | software | arXiv 2601.00227 | paper | Immutable evaluation record binding task definition, solution, workload and environment snapshot | Already covered | See source-tree row FlashInfer Bench: SweepContract and TrajectoryContract already bind dataset, costs, configuration, runner, entrant and invocation digests, and a changed invocation is refused on resume. |
| 47 | ForecastBench | forecasting | ICLR 2025 | summary | Superforecaster and public human baselines on the same questions | Future | See source-tree row ForecastBench: SharpeBench has no practitioner baseline; this is the model for one. |
| 48 | Foresight Arena | forecasting | arXiv 2605.00420 | summary | Commit-reveal forecasts with a power-based minimum number of rounds | Already covered | Forward windows already commit before reveal, and the gates carry a published power analysis; its minimum is advice rather than an enforced rule. |
| 49 | Formal Conjectures | science | arXiv 2605.13171 | paper | Unsolved targets give zero-contamination evaluation without keeping a test set secret | Already covered | SharpeBench's forward windows with commit and reveal are the trading version, since the outcome does not exist when the entrant commits, while historical windows remain the known gap that cannot be secret. |
| 50 | FormalProofBench | science | arXiv 2603.26996, ICLR 2026 | paper | Refuse submissions using verifier-bypassing constructs such as axioms before kernel acceptance | Already covered | SharpeBench accepts only closed-schema decisions and recomputes every return and gate itself, so an entrant has no way to certify its own result the way an axiom can in Lean. |
| 51 | FrontierMath | other | arXiv 2411.04872 | paper | Admit only guessproof problems with under 1% chance of a lucky correct answer | Already covered | Deflated Sharpe against the expected maximum of zero-skill trials and stationary block-bootstrap significance already limit how far luck can buy rank, without relying on a reviewer's judgment of guessability. |
| 52 | FutureX | forecasting | arXiv 2508.11987, ICLR 2026 | summary | Live questions resolving after model cutoff | Already covered | Forward windows resolve after commitment; FutureX reports accuracy without inferential statistics. |
| 53 | GenAI-Bench | other | arXiv 2406.13743 | paper | Validate an automatic metric against human ratings with tie-calibrated pairwise accuracy | Not transferable | A different artifact from the source-tree GenAI-Bench row, which audits a serving load and latency tool; this paper scores text-to-visual models with a VQA-model metric checked against Likert human ratings, a model judge SharpeBench's deterministic kernel does not need. |
| 54 | GeneBench-Pro | science | bioRxiv 2026.06.29.735386 | paper | Tiered release: public problems, third-party-held subset, and internal holdout | Future | Maps to the known gaps of no validation data tier and non-secret historical windows, since SharpeBench's only secret tier is forward commit/reveal; GeneBench-Pro also drops infrastructure-failed attempts from the denominator, which is weaker than strict replay. |
| 55 | Gençay, What survives honest evaluation? | trading | arXiv 2608.27734v1 | paper | A planted look-ahead oracle that clears every statistical test, beside a host-recorded trial ledger | Already covered | Nearest prior art. Deflation by declared and observed trials was already a gate, and SharpeArena, which counts every emitted candidate before validation, keeps a stricter trial ledger than the paper. The full read exposed a gap: the oracle (evaluation Sharpe 51.5, DSR 1.00) shows statistics cannot catch leakage, and SharpeBench's forward arena ranked returns supplied after the data reveal without binding them to the committed artifact. Arena intake now binds scored returns to that artifact by trajectory contract and replay, discloses provenance on each row, and the self-audit carries a leakage attack (PR <<SB-PR-F1>>). Gençay certifies individual strategies rather than ranking agents, treats repeated runs as coverage rather than a condition, and checks no mandate; its finding that every LLM-discovered strategy fails certification corroborates SharpeBench's refusals. |
| 56 | Harness-Bench | agents | arXiv 2605.27922 | paper | Full model-by-harness factorial under fixed tasks, budgets, timeouts and evaluator | Future | See source-tree row Harness Bench: separating model from scaffold effects is a known gap that needs a populated model field and an attested runner, and this matrix runs one trajectory per cell with an LLM process rubric inside the score. |
| 57 | HarnessOpt-Bench | agents | arXiv 2608.06301 | paper | Development reveals traces, validation only aggregates, test stays inaccessible during search | Future | It addresses the known missing validation tier, but no disclosure policy can hide public historical windows, so the only partition that can truly be withheld is SharpeBench's committed forward windows. |
| 58 | HealthBench | medicine | arXiv 2505.08775 | paper | Expert baselines written with and without access to model reference responses | Future | See source-tree row: the unassisted and model-assisted physician arms map directly to the known gap of no human or practitioner baseline, while the rubric score itself stays model-judged. |
| 59 | Hedge-Bench | finance research | arXiv 2606.03918v1 | paper | Rubrics derived from paired hedge-fund analyst transcripts, scored against a document pool frozen at each transcript's date | Not transferable | See source-tree row Trata Hedge-Bench: grading is concept matching by an LLM judge whose model family is also scored, the rubric comes from one LLM pass that may drift from the transcript, and invalid trials leave the metric; the date-frozen document pool is SharpeBench's existing point-in-time boundary, and the finding that analysts agree on questions but not answers belongs to the future practitioner study. |
| 60 | ICBCBench | finance research | arXiv 2606.17458v1 | paper | RMS calibration error on objective answers with a stated rule for absent confidence, plus a sequestered private hold-out | Already covered | See source-tree row: binned calibration, exact common support and a withheld tier were already covered by the forecast report, SharpeArena's sealed seeds and forward windows. The full read exposed the gap in absent-confidence handling: ICBCBench states its imputation, while SharpeBench's engine silently imputed 0.5 and counted it as calibration support, so a no-order hold agent reported a Brier of 0.25 over 240 observations in the committed golden. Only stated confidences are now paired with outcomes (PR <<SB-PR-C4>>). |
| 61 | IDE-Bench | software | arXiv 2601.20886 | paper | Null and oracle baselines on the same harness bracket every reported score | Already covered | See source-tree row: SharpeBench suite controls already run cash or no-op and buy-and-hold policies through the same pipeline and sharpebench-memory carries an oracle arm, while a perfect-foresight trading ceiling would not bound a risk-adjusted rank. |
| 62 | InvestorBench | trading | arXiv 2412.18174 | paper | Report the median-Sharpe trajectory of five repeated runs against buy-and-hold | Already covered | Pass^k requires every seed and window to pass rather than a median run, buy-and-hold is already a reference agent, and InvestorBench's 2020 to 2023 test windows overlap the training periods of several backbones, which forward windows avoid. |
| 63 | JudgmentBench | evaluation methodology | arXiv 2605.25240 | paper | Construct outputs at known quality levels and test which protocol recovers the ordering | Already covered | See source-tree row: the paper compares ways of collecting human and LLM judgments, which a deterministic scorer does not use, and SharpeBench's gate power analysis already states detection probability against a known true edge, computed analytically rather than with constructed entrants. |
| 64 | KernelBench | software | arXiv 2502.10517 | paper | fast_p: share of outputs both correct and faster than baseline by threshold p | Already covered | See source-tree row: SharpeBench eligibility gates likewise come before performance and cannot be bought by it, and the paper's best-of-k variant fast_p@k is weaker than pass^k. |
| 65 | LAB-Bench | science | arXiv 2407.10362 | paper | Incentivized expert human baseline on a matched subset of tasks | Future | See source-tree row: this targets the known gap of no human or practitioner baseline, but a trading version needs attested no-AI practitioner runs over the same windows, and LAB-Bench itself could not enforce its no-AI rule. |
| 66 | LifeSciBench | science | OpenAI preprint, no arXiv id | paper | Derive the task taxonomy from a survey of practitioners' most frequent workflows | Future | It depends on the known gaps of no expert workflow taxonomy and no practitioner baseline, and task scores come from an automated rubric grader spot-checked against experts. |
| 67 | LiveTradeBench | trading | arXiv 2511.03628 | paper | Replay recorded allocations lagged k steps to test dependence on timely information | **Candidate** | See source-tree row Live Trade Bench: SharpeBench has no counterfactual lagged replay, so its attribution cannot tell a policy using current observations from a static tilt that earns the same when stale. The paper plans a delay sensitivity and the stressed profile's declared two-bar delay is not applied by the backtest driver, so a rank-neutral k-bar lag of recorded decisions through the frozen engine is the concrete route, valid where the entrant's trades do not move the price. |
| 68 | LocalBench | other | arXiv 2511.10459 | paper | Closed-book and retrieval-augmented arms on identical items expose retrieval that hurts | Already covered | See source-tree row: sharpebench-memory already runs baseline, retrieval and oracle arms with paired significance, and SharpeBench applies multiplicity-adjusted tests where LocalBench uses Bonferroni-corrected paired t-tests over an LLM-judged gold set. |
| 69 | Long-Horizon-Terminal-Bench | software | arXiv 2607.08964 | paper | Low-weight public checks; hidden, dynamically generated stress cases carry most reward | Already covered | SharpeBench already requires every disjoint window and seed to pass and binds held-out windows into public commitments, SharpeArena's disjoint seed bands supply the unseen variants, and the paper's dense partial credit would let strengths offset failures where SharpeBench's gates are all-or-nothing. |
| 70 | MA-ProofBench | science | arXiv 2606.13782 | paper | State non-degeneracy premises explicitly so a statement cannot be satisfied vacuously | Already covered | See source-tree row: SharpeBench refuses constant return tracks by exact value equality and refuses control batteries that would pass vacuously, which is how an explicit non-degeneracy premise looks in trading. |
| 71 | MCP-Bench | agents | arXiv 2508.20453 | paper | Rule-based tool-name validity, schema compliance and dependency-order checks from traces | Already covered | See source-tree row: SharpeArena's closed-schema JSON contract with typed transport faults already enforces call validity deterministically, while MCP-Bench's headline task score still rests on an LLM judge. |
| 72 | MLE-bench | software | arXiv 2410.07095 (ICLR 2025) | paper | Grade against a real human leaderboard with field-size-scaled medal thresholds | Future | See source-tree row: this maps to the known missing practitioner baseline; its pass@k scaling and GPT-4o log review for rule-breaking are weaker than pass^k and the typed process audit. |
| 73 | MortgageTax (Vals.ai) | finance knowledge | vals.ai/benchmarks/mortgage_tax | page | Field extraction from mortgage tax certificate images | Not transferable | Fixed labels with no stochastic outcome, and the page does not state its scoring rule. |
| 74 | Multi-SWE-bench | software | arXiv 2504.02605 | paper | Track skipped and missing test states; discard any abnormal status transition | Already covered | Strict replay already refuses missing, duplicated, reordered or short runs instead of letting an absent cell count as a pass. |
| 75 | NoveltyBench | other | arXiv 2504.05228 (COLM 2025) | paper | Count functionally distinct outputs among k samples, weighted by quality with patience decay | Not transferable | The core is model-judged: a fine-tuned DeBERTa classifier with 79% human agreement assigns the equivalence classes and a reward model scores quality, while SharpeBench collapses Sybil clones deterministically. |
| 76 | OpenFinGym | trading | trading-papers corpus | paper | Host-side verifier withholding ground truth from the agent container | Already covered | The frozen engine never exposes future bars and verification replays outside the entrant's control. |
| 77 | PAST-Bench | agents | arXiv 2608.04003 | paper | Count a retention gain only above stale, distractor and wrong-mechanism control bounds | Already covered | See source-tree row: sharpebench-memory already runs baseline, retrieval and oracle arms with placebo, poisoning, point-in-time and confabulation checks, without the LLM judge PAST-Bench uses for open-ended tasks. |
| 78 | PillagerBench | agents | arXiv 2509.06235 | paper | Score against a fixed scripted-opponent roster, sabotage measured versus do-nothing baseline | Future | See source-tree row: a scripted-opponent roster with sabotage normalized against a do-nothing team would fit ranking on SharpeArena's shared-book market, where manipulation and ecology probes sit outside the five rank gates. |
| 79 | PortBench | trading | arXiv 2605.27887v5 | paper | Stress-window drawdown gate plus stage-level process score against a lookback oracle | Already covered | See source-tree row: SharpeBench's drawdown mandate already gates every run and outcome stays the rank key, whereas PortBench ranks by an oracle-agreement composite that correlates -0.17 with Sharpe, whose ranks invert (rho -0.62) when the oracle window changes, and whose primary oracle changed between versions. PortBench reports one run per cell at temperature zero. |
| 80 | PostTrainBench | agents | arXiv 2603.08640 | paper | Runs flagged for cheating score as the untouched base model, not dropped | Already covered | See source-tree row: SharpeBench keeps agent faults in the denominator as failing sentinels and blocks lookahead structurally, whereas PostTrainBench's contamination and substitution checks rest on an LLM judge. |
| 81 | Prediction Arena | trading | arXiv 2604.07355 | summary | Real-money prediction-market trading with complete trade and reasoning audit trails | Already covered | Trajectory capture and signed forward records keep the same audit trail; ranking one instance on account value is not transferable. |
| 82 | Prophet Arena | forecasting | arXiv 2510.17638 | summary | Bootstrapped 95 percent intervals on Brier and calibration | Already covered | Bootstrap intervals and proper scoring are already reported; Prophet Arena reports them without gating. |
| 83 | QuanBench+ | software | arXiv 2604.08570, ICLR 2026 workshop | paper | Calibrate a stochastic acceptance threshold from repeated reference executions | Already covered | SharpeBench already sets significance by stationary block bootstrap and deflates for configurations tried, and its all-cells pass^k is stricter than the paper's Pass@5, which counts any single success. |
| 84 | QuantBench | trading | arXiv 2504.18600 | paper | Report alpha decay, cross-strategy correlation, and robustness beside task metrics | Already covered | SharpeBench already reports decay, rolling stability and field crowdedness as rank-neutral diagnostics with explicit support counts, while QuantBench defines its robustness metrics only by example. |
| 85 | QuantCode-Bench | trading | arXiv 2604.15151 | paper | Nested compile, backtest, trade, and judge stages attribute each failure to one stage | Already covered | See source-tree row: SharpeBench already names the failed conjunctive gate in its report, and the paper's final stage is an LLM judge of whether the code matches the description, so only the stage naming transfers. |
| 86 | QuantEval | trading | arXiv 2601.08689v2 | paper | Execute generated strategy code under one released deterministic backtest configuration and score the MAE of its return, drawdown and Sharpe against an expert reference implementation | Not transferable | Scores distance to a fixed gold strategy from one greedy generation per task, with the MAE taken only over executable outputs and QA graded partly by an LLM; SharpeBench has no gold strategy and keeps failed cells in the denominator, and the released cost and fill configuration is what SweepContract already binds. |
| 87 | QuantitativeFinance-Bench | trading | anonymous NeurIPS 2026 submission, qfbench.com | paper | Task admission gate: the reference solution must pass and a single-call baseline must fail before a task merges | Already covered | See source-tree row: SharpeBench's synthetic witness must become eligible above a sampled boundary while its luck floor and suite controls stay below the bar, and canary GUIDs already ship; the paper's headline pass@k counts a task solved if any of k rounds passes, which reorders its own Table 1 and which SharpeBench refuses, its numeric verifier grades fixed gold values, and its failure taxonomy is LLM-judged. |
| 88 | QuantumBench | science | arXiv 2511.00092 | paper | Compare models only within human-rated difficulty and expertise strata | Not transferable | Eight-option multiple choice with fixed gold answers has no stochastic outcome, and SharpeBench already reports condition-level diagnostics by dataset and window. |
| 89 | RAT-Bench | other | arXiv 2602.12806 | paper | Score consequence-weighted residual risk under an adaptive attacker, not equal-weight recall | Not transferable | The attacker and text generator are LLMs over fixed synthetic profiles, and SharpeBench already attacks its own scorer with nine live attacks instead of a model-based adversary. |
| 90 | ResearchCodeBench | science | arXiv 2506.02314 | paper | Contamination-safe subset: tasks first committed after every evaluated model's cutoff | Already covered | See source-tree row: forward windows postdate every entrant by construction and historical evidence is kept apart from prospective evidence, which is stronger than dating repositories against self-reported knowledge cutoffs. |
| 91 | SEC-bench | software | arXiv 2506.11791 | paper | Deterministic sanitizer oracle: PoC fires before the patch and not after | Already covered | See source-tree row: the deterministic Rust kernel, planted invalid cases and live scorer attacks already give a judge-free oracle, and the point-in-time boundary does structurally what SEC-bench does by manually stripping leaked patches from bug reports. |
| 92 | SEC-bench Pro | software | arXiv 2605.26548 | paper | Admit a task only if the reference reproduces unpatched and every patched rerun blocks | Future | See source-tree row SEC-bench-Pro: the two-sided admission check fits only a future pipeline for admitting community scenarios, and the headline grade is an LLM judge that counts unsure verdicts as successes. |
| 93 | SOP-Bench | agents | arXiv 2506.08119 | paper | Separate execution completion rate from success conditional on completion | Already covered | See source-tree row: typed runtime, agent, process and outcome channels already split completion from decision quality, and SOP-Bench's precomputed mock tool outputs are a weaker reproducibility device than recorded-decision replay. |
| 94 | StockBench | trading | arXiv 2510.02209 | paper | Re-rank agents across separate downturn and upturn windows to expose regime-dependent rankings | Already covered | See source-tree row: pass^k already requires eligibility on every disjoint market window and regime comparison reports sign reversals, while StockBench averages three seeds and ranks by a z-score composite without a significance test. |
| 95 | SWE-bench | software | arXiv 2310.06770 (ICLR 2024) | paper | Continually add tasks created after model training cutoffs to limit contamination | Already covered | See source-tree row: forward windows with commit/reveal and the canary GUID tripwire already supply post-cutoff evidence, and the paper describes refresh as something its pipeline can do, not a committed schedule. |
| 96 | SWE-bench Multimodal | software | arXiv 2410.03859 | paper | Held-out domain exposes systems overfit to the original benchmark's scaffolding | Already covered | Disjoint market windows inside pass^k and SharpeArena's disjoint seed bands already test transfer beyond tuned conditions, and the deterministic kernel with cross-platform goldens removes the flaky verdicts this paper filters out by rerunning validation ten times. |
| 97 | SWE-Bench Pro | software | arXiv 2509.16941 | paper | Private held-out split mirroring the public set, reserved for later overfitting checks | Future | Public market history cannot be held out, so a private tier would need licensed non-public data to close the known gaps of no validation tier and non-secret historical windows, while forward windows with commit/reveal already give the prospective holdout. |
| 98 | SWE-bench-Live | software | arXiv 2505.23419 | paper | Monthly refresh from post-cutoff issues, keeping only instances stable across repeated validation | Already covered | Forward windows with commit/reveal and the canary GUID already supply post-cutoff evidence and pass^k already demands stability across repeats, while the static-versus-live overfit gap it reports needs the missing frontier-model field. |
| 99 | SWE-smith | software | arXiv 2504.21798 | paper | Environment-first synthesis; keep only perturbations that break passing tests | Not transferable | A training-data generator whose task gold is a fixed fail-to-pass test verdict with no stochastic outcome, and SharpeArena's disjoint Procgen-style seed bands and curricula already supply scalable training variation. |
| 100 | SWT-Bench | software | arXiv 2406.12952 | paper | A test counts only if it fails before and passes after the reference fix | Already covered | SharpeBench's live scorer attacks already apply the same before-and-after check to its own checks: each exposure is shown with the defense disabled and the repaired verdict with it enabled, and entrants never write tests. |
| 101 | tau-bench | agents | arXiv 2406.12045 | paper | pass^k: probability that all k i.i.d. trials succeed | Already covered | See source-tree row: SharpeBench pass^k is stricter because it requires success on every execution seed and every disjoint market window, where tau-bench estimates it over conversations with an LLM-simulated user. |
| 102 | Tax Agent Bench (Vals.ai) | finance knowledge | vals.ai/benchmarks/tax_agent_bench | page | Point-in-time sources and a score weighted by citation validity | Future | See source-tree row Tax Agent Bench: SharpeBench blocks look-ahead in observations but leaves an agent's stated sources score-neutral; checking that cited data existed at decision time fits a future trading-research evidence track, as with BigLaw Bench. |
| 103 | TaxEval v2 (Vals.ai) | finance knowledge | vals.ai/benchmarks/tax_eval_v2 | page | LLM-judged answer correctness and stepwise reasoning | Not transferable | Grades fixed gold answers with a model judge that was replaced mid-life. |
| 104 | Terminal-Bench 2.0 | agents | arXiv 2601.11868 | paper | Neutral single-tool reference scaffold separates model effects from agent-scaffold effects | Future | See source-tree row Terminal-Bench 2: this needs the populated frontier-model field and addresses the known gap that model and scaffold effects are not separated, matching the HarnessBench future row that requires an attested runner. |
| 105 | TimeSeek | forecasting | arXiv 2604.04220 | summary | Brier skill against the market at fixed points in each market's life | Not transferable | Descriptive point estimates without intervals. |
| 106 | TradeRank | trading | traderank.ai | page | Paper-trading seasons with leverage and position-count mandates | Already covered | Mandates here constrain behaviour but are not gated statistically; SharpeBench's drawdown mandate and process audit are eligibility conditions. |
| 107 | TraderBench | trading | arXiv 2603.00285 | paper | Progressive adversarial price transforms separate inert robustness from adaptive resilience | Future | See source-tree row: perturbed observation streams fit only a rank-neutral SharpeArena robustness probe because injected false signals are not a point-in-time market, and inert strategies are already visible through turnover and the buy-and-hold control, while TraderBench itself reports one run without intervals. |

## Candidates

Four mechanisms from the literature ledger were not built when their rows were written, and each
would close a named gap. One stays open; the other three are being built in the 16 September
2026 round.

**Search against the grader** (AutomationBench, arXiv 2604.18934). The self-audit
is a regression suite over named attacks and states that it is not a proof
that no entrant can find another way to game the scorer
(`docs/book/src/governance.md`). The two scoring fail-opens repaired earlier in
September 2026, a constant track scoring as a perfect edge and a constant track
setting the field's deflation bar, were found by review rather than by that
suite, and the unbound arena intake fixed in the 16 September 2026 round was
found by reading a paper. A search over synthetic submissions for ones that
clear the gates without skill needs no model calls and targets exactly that
class of defect. This candidate stays open.

**Lagged replay** (LiveTradeBench, arXiv 2511.03628). Replaying recorded
decisions delayed by k bars through the frozen engine would separate a policy
that uses current observations from a static tilt that earns the same when
stale. The manuscript plans a decision-delay sensitivity, and the stressed cost
profile declares a two-bar delay that the backtest driver does not apply; lagged
replay is a concrete route to both. It is valid only where the entrant's trades
do not move the price, and it would be reported, never ranked. It is now being
built as P4 (PR <<SB-PR-P4>>).

**Marks the scored agent cannot set** (CoffeeBench, arXiv 2606.16613). CoffeeBench
values goods at weighted-average cost so that self-dealt prices cannot raise the
score. SharpeArena's LOB market-making reward marks inventory to a book mid that
the agent's own resting quotes set, and the matching engine has no self-trade
prevention, so an agent can raise its own mark without a counterparty.
SharpeArena is building a mark the agent cannot move in the 16 September 2026
round.

**Seat-removal externality** (EconEvals, arXiv 2503.18825). EconEvals measures
competency on a solo variant with the other party neutralized on the same
instance. SharpeArena's shared-book manipulation probe reports only the
manipulator's result, so it cannot say what the manipulator's presence cost the
followers it trades against. A per-follower P&L difference against a
flat-manipulator run on the same seed would measure that cost, reported and
never ranked; SharpeArena is building it in the 16 September 2026 round.

## Ideas deliberately not built

The following are legitimate designs, but implementing them in the current
product would create an unused surface or weaken the benchmark's scope.

- A hosted multi-tenant evaluator, quota service, or streaming control plane.
- Community task admission and mutable benchmark marketplaces.
- Prompt reinjection and placebo experiments without an attested runner that
  controls the prompt schedule.
- A generic model-judge layer for research quality, rationale quality, or
  policy compliance.
- Best-of-k or any-success ranking.
- Operational serving latency as a trading-skill rank input.
- A protocol-v1 wire-handshake retrofit that would break existing entrants.

The future protocol-v2 design should consider a runtime wire fingerprint, but
only as a versioned negotiation. Closed JSON schemas and exact runner hashes are
complementary controls, not substitutes for a live cross-process handshake.
