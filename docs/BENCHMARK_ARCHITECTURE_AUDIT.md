# Benchmark architecture audit

This audit compares SharpeBench with all 75 benchmark source trees supplied in
the workspace `benchmarks/` directory. It is an architecture review, not a
leaderboard comparison. Each decision asks whether a mechanism closes a
demonstrated gap in the current trading-agent benchmark.

The evidence references below point to repository names and source files in the
audited workspace. The purpose is to understand the products and extract useful
architecture, not to reproduce each upstream repository's release history.

## Decision rules

- **Adopted:** the audit exposed a current SharpeBench gap and the fix is now in
  the product with non-vacuous regression tests.
- **Already stronger:** SharpeBench already implements the transferable
  invariant with equal or stronger evidence.
- **Future:** useful only for a future hosted service, community task market,
  protocol revision, or explicitly controlled experiment.
- **Rejected:** not reproducible enough, depends on a mutable or model-judged
  core, or does not fit a trading-agent benchmark.

The final count is 4 adopted rows representing three fixes, 45 already
stronger, 18 future, and 8 rejected.

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

## Complete 75-repository ledger

| # | Repository | Transferable mechanism | Decision | Evidence and reason |
|---:|---|---|---|---|
| 1 | ALE-Bench | Bounded public feedback followed by one private final evaluation | Already stronger | SharpeBench precommits held-out evidence and includes search effort in deflation. See `README.md`, public/private evaluation and resource accounting. |
| 2 | AssetOpsBench | Persist traces, then rescore them offline against ground truth | Already stronger | Capture and strict replay already separate generation from deterministic scoring without silently skipping unmatched evidence. See `docs/evaluation.md` and `docs/static-json-evaluation.md`. |
| 3 | BenchLocal | Versioned portable benchmark packs with staged activation | Future | Useful for distribution only after generation parameters and pack provenance are closed and signed. See `BENCH_PROTOCOL_V1.md`. |
| 4 | BikeBench | Retain individual feasibility violations beside performance | Already stronger | SharpeBench's conjunctive process, mandate, significance, and pass^k gates are noncompensatory. See `src/bikebench/benchmarking/benchmarking_utils.py`. |
| 5 | CADTestBench | Demonstrate evaluator adequacy with intended faults | Already stronger | SharpeBench attacks the live scorer, proves the Sybil exposure with its defense disabled, proves the repaired verdict with it enabled, and plants invalid provenance cases by rule family. The supplied CAD snapshot contains no executable mutation harness. |
| 6 | DI-Bench | Pair structural diagnostics with end-to-end execution | Already stronger | SharpeBench separates typed agent faults, retryable runtime failures, and excluded infrastructure failures. See `dibench/evaluate/evaluator.py`. |
| 7 | E3D-Bench | Multi-axis effectiveness, robustness, and efficiency report | Rejected | The supplied artifact is descriptive only and contains no evaluator, code, or data. |
| 8 | EdgeBench | Performance as a curve over a fixed consumed budget | Already stronger | SharpeBench already reports an OOS budget curve, marginal DSR, overfit onset, and selection-deflated peak. |
| 9 | FEA-Bench | Validate gold patches and report dataset attrition | Rejected | Reconstruction depends on mutable external repositories, unavailable data, and a patched external evaluator. |
| 10 | FinPersona-Bench | Placebo-controlled mandate reinjection and salience-decay arms | Future | A causal result requires control of prompt content, cadence, context reset, and token-matched placebo arms. SharpeBench does not own an external entrant's prompt schedule. An observational decay helper would not establish the claimed mechanism. |
| 11 | FormalQualBench | Machine-check critical invariants under an explicit trust boundary | Future | Formal verification of a small scorer kernel may be worthwhile, but theorem validity cannot substitute for empirical calibration. |
| 12 | HarnessBench | Fixed-model scaffold ablation with uniform traces | Future | Requires an attested runner that fixes model, sampling, transport, and prompt while varying only the harness. Current harness identity alone does not create that experiment. |
| 13 | IDE-Bench | Clean-container attempt independence and oracle validation | Already stronger | SharpeBench requires every declared window and seed to pass instead of rewarding one lucky attempt. |
| 14 | KernelBench | Correctness gate before baseline-relative efficiency | Already stronger | Performance cannot compensate for failed SharpeBench eligibility or safety gates. |
| 15 | LAB-Bench | Task-family reporting | Rejected | Exact-answer multiple choice is routed through a nondeterministic model judge with a very small golden sample. |
| 16 | NanoGPT-Bench | Fixed-resource optimization with repeated confirmation | Already stronger | SharpeBench combines selection-deflated budget curves with a digest-pinned, non-root, no-network, no-IPC container boundary. |
| 17 | PAST-Bench | Longitudinal memory ablation | Already stronger | `sharpebench-memory` already requires baseline, retrieval, and oracle arms plus significance, poisoning, point-in-time, multisession, and confabulation checks. |
| 18 | PillagerBench | Opponent-policy matrices | Future | Relevant only when crowding becomes an active strategic multi-agent environment rather than a field diagnostic. |
| 19 | PostTrainBench | Separate anti-reward-hacking review | Already stronger | SharpeBench structurally blocks lookahead and tests live scorer attacks rather than relying on a post-hoc model judge. |
| 20 | QuantCode-Bench | Stage-specific compile, execute, trade, and semantic diagnostics | Rejected | Generated code is unsandboxed, data can fall back to mutable downloads, and semantic grading is intentionally lenient. |
| 21 | QuantumLean-Bench | Pair human-legible rationale with a machine-checked artifact | Future | A rationale-consistency diagnostic needs a specified causal or semantic contract. Free text is intentionally score-neutral today. |
| 22 | SEC-bench-Pro | Counterfactual replay across vulnerable, fixed, and latest environments | Future | Useful for planned policy and cost-profile ablations only if verdicts remain deterministic and fail closed. Its default can count uncertain outcomes as successes. |
| 23 | SEC-bench | Preserve raw exploit evidence before classification | Already stronger | SharpeBench retains decisions and typed failure evidence; it also distinguishes OOM from generic exit 137. |
| 24 | SOP-Bench | Separate infrastructure completion from decision quality | Already stronger | Typed runtime, agent, process, and outcome channels already provide a less parser-dependent split. |
| 25 | SWE-bench | Keep the full task universe and additive error identities | **Adopted** | This exposed the fail-closed execution-matrix gap fixed above. See `swebench/harness/reporting.py` and `grading.py`. |
| 26 | SWEBenchBenchmarkService | Hosted evaluator API with setup, streaming, and aggregation boundaries | Future | Relevant only to a hosted SharpeBench service. Mutable image tags and unsupported evaluation modes are not acceptable in the current kernel. |
| 27 | TraderBench | Evaluator-only scenario data and unseen market windows | Already stronger | SharpeBench binds held-out windows, scorer configuration, and forward evidence into public commitments. |
| 28 | ARC-AGI-3 benchmarking | Explicit terminal reasons and per-step budgets in an interactive loop | Already stronger | SharpeBench already records typed completion, resource, runtime, transport, and agent failures for local replay. |
| 29 | ARC-AGI benchmarking | Record raw attempts, cost, tokens, and duration | Already stronger | Cost and attempt evidence are first class, and reliability is pass^k rather than any-correct-attempt success. Rank-neutral inference timing and token accounting now travel in the SharpeArena bridge manifest; see row 75. |
| 30 | AstaBench | Bind scorer identity independently from entrant runtime | Already stronger | Checkpoints and strict replay bind exact scorer, runner, entrant, data, and costs. |
| 31 | Autoresearch Novelty Bench | Freeze the information frontier at time T | Already stronger | Forward windows and point-in-time observations precommit the available evidence without a model judge. |
| 32 | bench | Common workloads across runtimes | Rejected | One warmup and one timed run with no cross-runtime result-equivalence gate is insufficient methodology. |
| 33 | r-lib bench | Require semantic equality before comparing performance | Already stronger | Golden replay establishes equivalence before cost and performance diagnostics. GC-filtered microbenchmark timing is not the product's unit of analysis. |
| 34 | Urbit benchmark | Attach host and runtime metadata to performance evidence | Future | Optional entrant environment metadata can aid diagnosis, but reporter-controlled hardware must not become a rank input. |
| 35 | Browser-use benchmark | Separate evidence extraction from frozen valuation | Rejected | The published judge plumbing is incomplete and the core extraction is model-dependent. |
| 36 | BigLaw Bench | Score analytical quality separately from source support | Future | Appropriate for a future trading-research evidence track, not the deterministic return-ranking kernel. |
| 37 | CAR-Bench | Paired normal, ambiguous, and deliberately impossible tasks | Future | Useful for an agent-protocol abstention suite once task interaction is in scope; simulated users and policy scoring remain model-based. |
| 38 | Code Review Benchmark | Pair frozen historical evaluation with fresh prospective evidence | Already stronger | SharpeBench combines frozen replay with signed forward records and does not need a model judge. |
| 39 | Conjecture Bench | Preserve supersession and provenance conflicts | Already stronger | SharpeBench evidence records are immutable and signed; the benchmark snapshot itself is a catalog, not an executable evaluator. |
| 40 | Conjecture Bench old | Exact verifier acceptance and honest zero baselines | Already stronger | SharpeBench has one deterministic kernel, cross-platform goldens, replay, and adversarial self-audit. |
| 41 | Create Benchmark Service | Multi-tenant hosted control plane | Future | Quotas, streaming, cleanup, and tenant boundaries become relevant only if SharpeBench becomes a hosted service. |
| 42 | Deep Research Bench | Separate report quality from citation support | Rejected | Unknown items are skipped, weights can be substituted, and the central judge is mutable and model-based. |
| 43 | CARLA driving benchmarks | Factorial route and weather conditions from immutable traces | Already stronger | Dataset, window, seed, cost profile, and replay contracts already define the complete trading condition grid. |
| 44 | FlashInfer Bench | Correctness gates performance and environment metadata travels with results | Already stronger | SharpeBench does not silently skip workload errors or let performance compensate for failed correctness. |
| 45 | GenAI-Bench | Operational load and tail-latency plane | Future | Useful for a hosted serving surface, not trading correctness or rank. |
| 46 | Harness Bench Fast | Versioned task semantics and resumable attempts | Already stronger | Strict contracts prevent cross-version resume and require the complete evidence matrix. |
| 47 | Harness Bench | Paired harness/model matrix | Already stronger | SharpeBench already binds execution identity; a one-task, five-run LLM-graded matrix would be weaker evidence. |
| 48 | Harness Bench real-repository variant | Negative control fails and reference solution passes before task admission | Future | Valuable if community-contributed scenarios are admitted. Current frozen scenarios already have deterministic controls and self-audit. |
| 49 | Interfaze Complete Benchmarks | Persist generation output for independent rescoring | Already stronger | Raw decisions are captured and replayed through the frozen engine under a strict contract. |
| 50 | Live Trade Bench | Longitudinal forward decision provenance | Already stronger | SharpeBench forward records are signed and replayable; terminal-return-only summaries are weaker. |
| 51 | MCP-Bench | Multi-server tool dependency tasks | Rejected | Relevant to Gordon, not SharpeBench ranking, and the overall score depends on a fixed model judge. |
| 52 | MLE-Bench | Resource budgets and difficulty strata | Already stronger | SharpeBench exposes budget curves and condition-level diagnostics without retaining known data leaks to preserve a board. |
| 53 | Proof Bench | Independently executable success witness | Already stronger | The verifier replays every raw decision and recomputes all returns and gates instead of trusting a narrative result. |
| 54 | Ratel Bench | Baseline, retrieval, and oracle arms with cost-normalized lift | Already stronger | This design already ships in `sharpebench-memory`, including oracle headroom and paired significance. |
| 55 | React Grab Bench | Controlled intervention with shortcut removal | Already stronger | SharpeBench's ablations and held-out windows isolate treatments with repeated seeds rather than one trial per case. |
| 56 | StockBench | Explicit exclusion of decision-day data | Already stronger | Point-in-time observations make future data unrepresentable at the agent boundary, with repeated windows and seeds. |
| 57 | tau-bench | End-state correctness and all-trial reliability | Already stronger | Raw decisions need not imitate one path, while pass^k requires reliability across all declared cells. |
| 58 | tau2-bench | Outcome scoring with only invariant process gates | Already stronger | This is already the SharpeBench split: outcomes determine performance, while only risk, process, and mandate invariants gate eligibility. |
| 59 | Terminal-Bench 1 | Version scorer, oracle, task, and environment together | Already stronger | Strict contracts and provenance already bind the complete evaluation identity. The beta lineage should not be mixed with later Harbor boards. |
| 60 | Terminal-Bench 2.1 | Treat reward-hack repairs as benchmark-version changes | Already stronger | SharpeBench goldens, self-audit, and provenance make scorer changes explicit and non-comparable when semantics move. |
| 61 | Terminal-Bench 2 | Human and model task qualification before release | Already stronger | Deterministic fixtures, planted invalid cases, and live attack tests provide executable qualification without an external Harbor dependency. |
| 62 | Terminal-Bench current | Continuous community task-admission pipeline | Future | Relevant only if SharpeBench accepts community scenarios. Any adoption must pin dataset release, harness, and environment instead of `latest`. |
| 63 | Terminal-Bench Science | Validator registry, planted negatives, valid controls, and full-corpus nonempty checks | Future / partially present | The strongest future task-admission reference. Its rule-level planted-negative idea is already applied to provenance validation; the full community workflow is not needed yet. |
| 64 | Turbopuffer benchmark | Cold/warm state and tail-latency workload envelopes | Future | Appropriate for hosted operational diagnostics, not agent correctness or trading skill. |
| 65 | WorkBuddy Bench | Refuse missing tasks, scores, plan items, and shrunken denominators | **Adopted** | This independently exposed the same evidence-geometry gap as SWE-bench. The current fix makes partial execution explicitly noncertifying. |
| 66 | AutomationBench | Freeze a canonical task contract before mutable normalization and bind resumption to it | **Adopted** | Its explicit task contract exposed a narrower checkpoint gap: when an operator supplied `--entrant-sha256`, SharpeBench bound the artifact but not the command, endpoint, image reference, or environment pass-through list. `SweepContract` schema 2 now carries a separate invocation digest, and a same-artifact changed invocation is refused. See `automationbench/task_contract.py` and `automationbench/runner.py`. |
| 67 | JudgmentBench | Cluster-first resampling, evaluator agreement, and near-tie handling | Already stronger | SharpeBench resamples at the declared dependence unit, reports paired intervals and multiplicity-adjusted tests, and uses one deterministic scorer instead of estimating agreement with a model evaluator. See `analysis/analysis_vSubmit.R`. |
| 68 | LocalBench | Normalize task records, retain answer/refusal rates, and report subgroup quality | Already stronger | Closed submission schemas, typed failure categories, complete denominators, and dataset/condition diagnostics already make refusal and subgroup behavior visible without a mutable judge. See `benchmark.py`, `loader.py`, and `metrics/answer_rate.py`. |
| 69 | MA-ProofBench | Require complete outputs, resume missing samples, statically precheck, then execute a proof witness | Already stronger | SharpeBench retains every declared cell, distinguishes retryable infrastructure failure from entrant failure, replays executable evidence, and carries selected scorer invariants in Lean. Pass^k is deliberately stricter than pass@k. See `evaluation/main.py` and `evaluation/checks.py`. |
| 70 | RAD-Bench | Diagnose multi-turn retrieval decay and correlate a benchmark with an external ranking | Future | Turn-conditioned evidence could support a future memory or research-agent track, but the current return and forecast protocols do not define retrieval turns. Adding the metric now would create an unscored surface, and the supplied evaluator is weaker than SharpeBench's closed contracts. See `rad_bench/conversation.py` and `rad_bench/gen_judgment.py`. |
| 71 | ResearchCodeBench | Measure context ablations and contamination by information cutoff | Already stronger | SharpeBench freezes point-in-time observations, precommits forward windows, separates historical from prospective evidence, and records ablations without allowing them to alter the rank key. See `core/generate_solutions.py` and `visualize/contamination_knowledge_cutoff_merged.py`. |
| 72 | SWE-CARE | Keep unevaluated instances in the denominator and disclose context-source ablations | Already stronger | Missing execution cells make a SharpeBench sweep noncertifying, while entrant faults remain as failing sentinels. Context and data identities are bound into evidence rather than inferred from successful rows. See `scripts/eval_report.py` and `scripts/run_eval_pipeline.py`. |
| 73 | HealthBench | Combine positive and negative criteria, hard subsets, and bootstrap support | Already stronger | SharpeBench's process and mandate checks include positive obligations and prohibited behavior, while dependence-aware bootstrap and explicit support counts avoid treating rubric points as independent. Its deterministic kernel removes the mutable-judge dependency. See `judge.py` and `README.md`. |
| 74 | LabBench2 | Stage deterministic validation, preserve failed tasks, and patch retries rather than overwrite them | Already stronger | SharpeBench separates contract, transport, process, outcome, and statistical stages; failed cells remain in the declared geometry; and append-only or checkpointed retries preserve prior evidence. See `evals/evaluators.py`, `evals/run_evals.py`, and `evals/report.py`. |
| 75 | MU-Bench | Publish tail latency and resource accounting beside quality while keeping it out of rank | **Adopted** | SharpeArena raw-field schema 2 records every inference duration and its observation source. Bridge schema 2 validates call counts and totals, then publishes nearest-rank p50/p95 latency, token totals, reasoning-token provenance, and retries with `rank_input: false`. Locale macro-averaging and model-judge controls do not fit this deterministic trading scorer. See `scripts/latency_stats.py`, `scoring/metrics.py`, and `scripts/significance_test.py`. |

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
