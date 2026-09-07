# Independent SharpeArena reviewer reports

> Historical audit evidence at [sharpebench `933e0c1`](https://github.com/general-liquidity/sharpebench/tree/933e0c1056a2e4707b28762c294323bf05bdab65) and [sharpearena `1be915f`](https://github.com/general-liquidity/sharpearena/tree/1be915f330acabacd171cc350bec0def58d9e134).
> Findings, quoted claims, test counts and references to "current" describe those
> reviewed baselines. See [IMPLEMENTATION.md](IMPLEMENTATION.md) for later repair
> dispositions and remaining work; this report is not a current defect list.

Read-only reports returned by the independent reviewer. Findings, order, confidence labels, category coverage and limitations below are retained as supplied.

# Arena recent operational-evidence audit

Read-only audit, 2026-09-07. Repository: [sharpearena baseline](https://github.com/general-liquidity/sharpearena/tree/1be915f330acabacd171cc350bec0def58d9e134). Base v0.24.1; HEAD 1be915f330acabacd171cc350bec0def58d9e134. Four commits; 12 changed files; +234/-15 lines. Commit messages, full diff, changed files in full, and accounting variables outside the diff were reviewed independently under audit-analysis. No source edits or model runs.

## Findings

1. CONFIRMED — Missing and invalid provider accounting becomes measured zero before validation.

   File: crates/sharpearena-py/python/sharpearena/local_agents.py:710. Quote: `prompt_tokens = int(response.get("prompt_eval_count", 0) or 0)`; line 713: `reasoning_tokens = int(response.get("reasoning_count", 0) or 0)`; line 726: `total_duration_ns=int(response.get("total_duration", 0) or 0)`. The OpenAI-compatible path repeats token coercion at lines 964–976. Missing/null fields become zero; negative fractions such as -0.5 truncate to zero. A null reasoning-count key is classified as provider-reported because availability tests key presence. Later integer/nonnegative checks cannot recover the erased distinction.

   Claim: CHANGELOG.md:14 says “preserve source-labelled duration for every local-model request” and “validated nearest-rank p50/p95 latency, token totals, reasoning-token provenance”. Code labels missing duration `backend-reported-total-duration` at local_agents.py:727.

   Executed synthetic reproduction: actual producer/runner with synthetic transport responses and in-memory stand-ins for native environment/scoring completed a three-call cell with missing accounting and reported p50 = p95 = 0 ns and zero tokens. Negative fractional values also passed after integer coercion; null reasoning was reported as available. Consequence: unknown telemetry masquerades as genuine zero work, corrupting latency/capacity comparisons and availability statistics. This affects diagnostics, not the rank key.

2. CONFIRMED — Failed requests and resumed failed attempts disappear from operational totals.

   File: crates/sharpearena-py/python/sharpearena/local_agents.py:1248. Quote: `if not outcome.ok:` records the error and reaches `continue` at line 1259 before accounting at line 1262. File: crates/sharpearena-py/python/sharpearena/bench_bridge.py:141: `records_by_id[cell_id] = record` replaces a preceding failed attempt with its completion; line 395 computes `_operational_profile(model_records)` from retained records.

   Claim: README.md:162–165 says local-field evidence records “every model-call duration and its observation source”; bench_bridge.py:68 calls latency “one model request, nearest-rank percentile”. The implementation's line-53 docstring instead says “completed field cells”.

   Executed synthetic reproduction: five request attempts across a failed attempt and its resumed completion produced two journal rows, then one retained cell. An earlier successful 10,000 ns call inside the failed attempt disappeared; only three 10 ns calls from the final completion survived, with retries reported as zero. The failing request itself carried no duration. Consequence: slow/error-prone models can appear cheaper and faster because expensive failures are removed. Source hashes retain bytes, not their contribution to operational totals.

3. CONFIRMED — Operational validation does not reconcile reasoning observations or realized step count.

   File: crates/sharpearena-py/python/sharpearena/bench_bridge.py:289. Quote: `steps = record.get("steps")`, then `expected_calls = math.ceil(steps / cadence)` at line 294. Steps are not required to equal the realized-return count. At lines 302–306, reasoning validation only requires `reasoning_tokens_source` in `{"provider-reported", "unavailable", "mixed"}`; it does not validate the observation array, its length, availability pattern, or sum.

   Claim: README.md:143–145 describes “validated per-request latency, token, reasoning-token, and retry summaries”; implementation trusts the producer label and an independently alterable step count.

   Executed synthetic reproduction: reasoning total 999 with observations [null] was accepted; steps = 3 with two realized returns was accepted when duration samples matched declared steps. Consequence: internally inconsistent telemetry becomes an apparently validated manifest, including spurious call counts and reasoning provenance.

4. CONFIRMED — Duration-source identity is collapsed and “unspecified” is accepted.

   File: crates/sharpearena-py/python/sharpearena/local_agents.py:1434. Quote: `next(iter(inference_duration_sources[index]))` for one source, otherwise `"mixed"` at line 1437. Sources are collected in a set at line 1299 rather than paired with ordered samples. Line 456: `duration_source: str = "unspecified"`. File: bench_bridge.py:299–301 accepts any nonempty source string.

   Claim: docs/LOCAL_AGENT_ARCHITECTURE.md:31–32 says every duration states whether it came from the backend or a host clock. Mixed cells cannot attribute individual samples, and a custom client's omitted source remains “unspecified” while passing validation; the regression test expects it to survive.

   Source-confirmed consequence: a p95 sample cannot be identified as backend compute time versus host elapsed time in mixed cells, and unlabeled accounting is treated as valid. These are not assured measurements of one comparable quantity.

## Exact coverage and limitations

Changed files read in full: CHANGELOG.md; README.md; crates/sharpearena-py/README.md; crates/sharpearena-py/pyproject.toml; crates/sharpearena-py/python/sharpearena/bench_bridge.py; crates/sharpearena-py/python/sharpearena/local_agents.py; crates/sharpearena-py/tests/test_local_agents.py; docs/LOCAL_AGENT_ARCHITECTURE.md; docs/architecture.md; docs/capabilities.md; docs/evidence.md; paper/evidence/provenance.json.

Commits reviewed: 09d1530 feat(field): preserve rank-neutral operational profiles; ab690fc docs(field): describe rank-neutral operational evidence; db1adbc test(py): scope pytest to the canonical suite; 1be915f chore(provenance): bind operational field evidence.

This is not a paper-to-code or empirical replication review. Synthetic checks used no model server, broker, or network. Historical logs do not establish before/after Ns for the new schema-2 profile. No ranking regression was established here.

Claims: The every-request, validated-accounting, and every-duration-source claims exceed producer/bridge guarantees; no contradiction found in the explicit rank-neutral claim.

Sample: N unverified for empirical fields. Synthetic failed/resumed example: 5 attempts -> 2 journal rows -> 1 retained cell -> 3 duration samples; earlier failed-attempt costs and the failing request are excluded. No collapse/filter log verifies actual production counts.

Merges: cell_id is the collapse key; identical duplicates deduplicate, conflicting completions reject, and failed attempts may be replaced by a completion. This is documented for scoring but loses their costs from operational aggregation. Cartesian coordinates/ordinals are checked; steps are not reconciled to returns. No Stata _merge applies.

Variables: Latency is integer nanoseconds; tokens/retries are counts. No logs/levels or currency-deflation transform occurs in these diagnostics. Missing provider values and source aggregation break interpretation; reasoning observations are not reconciled with totals.

Silent failures: Missing/null accounting becomes zero, negative fractions truncate to zero, unsuccessful outcomes skip accounting, prior failed rows collapse away, and mixed sources lose per-sample identity.

Estimation: Nearest-rank percentiles use retained successful-request samples, not every attempted request; empirical N unverified. No clustering, fixed effects, or regression weights occur in this changed profile path; the profile is not a rank input.

Without the data: I could not quantify how much real field latency/token totals change after including all failed attempts and recovering original provider telemetry.

# Arena full-production interface slice audit

Read-only isolated audit-analysis review, 2026-09-07. Repository [sharpearena baseline](https://github.com/general-liquidity/sharpearena/tree/1be915f330acabacd171cc350bec0def58d9e134); HEAD 1be915f330acabacd171cc350bec0def58d9e134. This extends the recent-diff audit to the explicitly listed production interface slice. It does not repeat the four recent transport-accounting findings. No product edits, model runs, network calls, broker submissions, or training experiments were performed. Ten findings, ordered by consequence.

## Findings

1. CONFIRMED — A committed forecast can be bound to a different contract from the frozen plan and settled against the original instrument.

   File: crates/sharpearena-py/python/sharpearena/prospective_field.py:859. Quote: `expected_contract_ids = [raw["contract_id"] for raw in plan["contracts"]]`; line 885: `revision_predictions[revision["claim_id"]] = prediction[0]`; line 886 checks `list(revision_predictions) != expected_contract_ids`. The validator checks IDs, identity, clocks and predictions, but never compares the pending document's contract bytes or revision contract digests with the frozen plan.

   Claim/code disagreement: the error at lines 887–888 says “revision support differs from the frozen contracts”, but support means IDs only. At lines 1001–1017, `_ledger_from_pending` reconstructs contracts from the pending document itself and submits `contract=contract_by_digest[raw["contract_sha256"]]`. At lines 1095–1098 resolution reads the plan's instrument/target and assigns `outcomes[raw["contract_id"]] = candle["outcome"]`.

   Executed in-memory reproduction: one frozen BTCUSDT contract and one alternate ETHUSDT/different-question contract with the same claim ID but different digest. The actual contract, ledger, evidence, and semantic artifact validators accepted the alternate pending document with consistent identity, clock, predictions and logits; only file reads were supplied in memory. Consequence: valid file hashes and an internally valid ledger do not bind forecast meaning across files; the original BTC outcome can be attached to an ETH-labelled claim. No claim that any published artifact was altered.

2. CONFIRMED — The default RL dataset teaches only action dialects that its parser deliberately rejects.

   File: crates/sharpearena-py/python/sharpearena/dataset.py:42–44. Quote: `<action>{"weights": {"SYM00": 0.5, "SYM01": -0.3}}</action>` or `<action>{"flat": true}</action>`; “Unlisted symbols default to 0.”

   Opposing code/comment: decision_parser.py:6–7 says the former weights/flat dialect “is deliberately rejected”; lines 95–101 reject unknown fields and require `decision.orders` to be an array. Omitted symbols actually retain current weights (lines 189–191), rather than defaulting to zero. verifiers_env.py:327–342 terminates the episode on this parse error.

   Source-confirmed, with existing regression assertions at tests/test_verifiers.py:125–135 explicitly requiring both taught formats to raise DecisionParseError. Consequence: a policy obeying the generated prompt is terminated immediately; this is an incompatible training interface, not evidence of model trading failure. No model experiment was run.

3. CONFIRMED — “Leak-safe” checkpoints serialize the full CSV, including future prices.

   File: crates/sharpearena-py/python/sharpearena/checkpoint.py:27–30. Claim: checkpoints carry “never ... a raw price series” because it would permit peeking at future bars. Opposing code at line 80: `"csv_text": env._csv_text`; lines 126–132 export `"params": dict(self.params)`. The guard at lines 48–56 only rejects dataset/env objects by type name or reset/step methods; a CSV string passes.

   Source-confirmed consequence: deserializing a checkpoint from a CSV environment gives the recipient the entire future tape. This specifically defeats the documented checkpoint artifact boundary; it is not an allegation that ordinary observations expose future bars or that in-process Python is a security sandbox. No native CSV probe was executed.

4. CONFIRMED — A bare endogenous-market reset retains positions, cursor and terminal state from the previous episode.

   File: crates/sharpearena-py/python/sharpearena/market_env.py:349 claims “Reset the shared market.” Code at lines 352–357 rebuilds only under `if seed is not None:`, then calls `self._market.reset_market()`. Native file crates/sharpearena-py/src/lib.rs:1175: `fn reset_market(&self)`; line 1176 merely serializes `self.inner.initial_observations()`. It does not mutate/reset the native market.

   Executed native reproduction: one agent, one symbol, 24 days, seed 7; reset(seed=7), step([0.5]), reset() retained 0.005017964410263338 shares and did not equal the initial observation. A finished market remains done on bare reset. Consequence: standard repeated PettingZoo episodes contaminate one another or cannot advance; resetting with an explicit seed masks the defect. Tests create fresh environments or explicitly reseed and do not cover this sequence.

5. CONFIRMED — The counterfactual ledger invents execution for an atomically refused batch.

   File: crates/sharpearena-py/python/sharpearena/counterfactual.py:368–369. Quote: `elif verdict.get("allowed"):` then `disposition, reason, executed = "executed", "allowed", quantity`. Claim at lines 79–81: executed_quantity is “what reached the broker”. paper_trading.py:1443–1453 records this ledger before lines 1463–1483 inspect refusal and raise “paper batch refused before submission”. Thus all orders can remain unsubmitted while an allowed prefix is marked executed.

   Executed in-memory full-caller reproduction: two 75%-weight buys, account equity 1,000, prices 100, gross cap 1.0. The second order caused a batch refusal. Actual InMemoryPaperBroker submissions = 0 and positions = {}; ledger reported acted_decisions = 1, executed_notional = 750, executed_pnl = 75, foregone_pnl = 75 using settlement prices 110. Consequence: intended-versus-actual execution, selection-gap and P&L evidence are false even without transport failures. tests/test_counterfactual.py:150–170 assert the flawed prefix interpretation rather than the caller's atomic behavior.

6. CONFIRMED — Default checkpoint replay silently changes the scenario-generating process.

   File: crates/sharpearena-py/python/sharpearena/checkpoint.py:64–105. Quote from reconstruction: `distribution_mode=params.get("distribution_mode", "calm")`, `env_kwargs=params.get("env_kwargs") or None`. The closed extraction/reconstruction lists omit vol_clustering, jump_burst_probability, jump_burst_persistence, and jump_burst_size, which gym.py:79–82 stores as separate attributes.

   Claim: checkpoint.py:13–15 promises the restored environment has “same next observations, same next rewards”. Executed native reproduction: seed 4, two symbols, 40 days, controls 0.7/0.4/0.8/0.1; after action [0.3,0.2], default clone/branch rebuilt all four controls as zero. The next close vectors differed and rewards were 0.028954156678085985 versus -0.0009694194514318077. Consequence: tree-search and counterfactual branches evaluate a different market law from their parent. Functional reconstruction at functional.py:90–126 similarly lacks these scenario controls. Existing replay tests exercise default controls.

7. CONFIRMED — LOB reset retains the previous episode's equity reward baseline.

   File: crates/sharpearena-py/python/sharpearena/lob_env.py:103–117. Quote: `self._inventory = {a: 0 for a in self.agents}` and `self._cash = {a: 0.0 for a in self.agents}`; no reset of _prev_equity. At lines 191–195, `prev = getattr(self, "_prev_equity", {}).get(agent, 0.0)` feeds `return float(eq - prev - self._inv_pen * self._inventory[agent] ** 2)`.

   Executed native reproduction: one agent, four steps, seed 2, repeated action [3,3]. First episode rewards [20.951,32.676,-0.324,-0.324]; resetting the same instance with the same seed produced [-33.049,32.676,-0.324,-0.324]. Previous equity 54 was subtracted from the new first reward. Consequence: first-step learning rewards depend on the prior episode despite a reset of book, RNG, cash and inventory; identical seeded episodes are not reward-identical. The defect is interface state, not an audit of native matching math.

8. CONFIRMED — PettingZoo silently runs calm scenarios when a different difficulty is requested.

   File: crates/sharpearena-py/python/sharpearena/pettingzoo_env.py:84 accepts `distribution_mode: str = "calm"`; line 103 stores `self._distribution_mode = str(distribution_mode)`. Opposing construction at lines 132–139 passes n_symbols, n_days, seed, max_weight, allow_short and env_kwargs to SharpeArenaEnv, but omits distribution_mode entirely.

   Executed native reproduction: MultiAgentSharpeArenaEnv with one agent, one symbol, 12 days, distribution_mode="extreme" built its agent environment with _distribution_mode == "calm". Consequence: requested extreme/hard tournament or curriculum arms actually use the default calm generator, invalidating difficulty comparisons and transfer claims. Existing PettingZoo tests use the default mode.

9. CONFIRMED — A protocol failure can terminate an RL episode while retaining its favorable-prefix reward.

   File: crates/sharpearena-py/python/sharpearena/verifiers_env.py:327–333. Quote: `state["protocol_failures"] += 1`, append protocol_error, then `state["_oo_done"] = True`; recorded returns are not invalidated. realized_return_reward at lines 80–83 only reads those returns and computes `float(np.tanh(np.sum(rets)))`; deflated_sharpe_reward at lines 95–96 likewise scores the retained returns without a fault gate.

   Claim: verifiers_env.py:171–173 describes process/format as “gates, not gradient”. Actual rewards.py:387–395 constructs weighted return/Sharpe/mandate functions and merely `rubric.add_metric(process_check_reward)` / `rubric.add_metric(format_reward)`; the rubric never gates on them.

   Source-confirmed consequence: an agent can emit malformed output after favorable returns to choose a shorter scoring horizon with no weighted protocol penalty, instead of being assessed over the planned episode. Real training exploitation is unverified; no training/model experiment or reward-gate runtime probe was performed. Tests exercise malformed first output, where the prefix happens to be empty.

10. CONFIRMED — Dense Gym actions are silently truncated and declared short/weight bounds are not enforced.

    File: crates/sharpearena-py/python/sharpearena/gym.py:170–181. Quote: `weights = np.asarray(action, dtype=np.float64).reshape(-1)`; orders use `for sym, w in zip(self._symbols, weights)`. No exact dimension or Box-bound check precedes submission. At lines 94–97, allow_short/max_weight only define `spaces.Box(low=low, high=max_weight, shape=(n,), ...)`.

    Executed native reproduction: two-symbol, long-only environment with max_weight = 0.1 accepted [], [0.5], [0.1,0.1,nan], and [-0.2,0]. Empty/short actions skipped symbols; the third NaN was discarded; -0.2 produced a short first-symbol position of approximately -0.002006696573209026 shares. Consequence: malformed dense actions can alter only a prefix, extra invalid values vanish, and the configured long-only/weight envelope is not an execution constraint. vector.py:195–212 repeats the zip conversion. This does not claim canonical JSON parsing is permissive; that parser is stricter.

## Exact coverage

Python production modules read in full, under crates/sharpearena-py/python/sharpearena/:

gym.py; vector.py; functional.py; checkpoint.py; wrappers.py; wrappers_vector.py; lookahead_guard.py; rewards.py; counterfactual.py; pettingzoo_env.py; portfolio_env.py; forecast_contract.py; forecast_evidence.py; deferred.py; verifiers_env.py; mandate.py; dataset.py; local_field_cli.py; strategy_cli.py; preprocessing.py; data_blocks.py; effective_config.py; prospective_field.py; decision_parser.py; forecast.py; indicators.py; news.py; obs_extra.py; market_env.py; lob_env.py; market_making.py; execution.py; minari_export.py; spaces.py; execution_noise.py; trace.py; pairs.py; curriculum.py; discrete.py; registration.py; risk.py; cascade.py; paper_trading.py; mcp_server.py; check_env.py; generalization.py; eval_seeds.py; local_agents.py; bench_bridge.py; paper_cli.py; ollama_shim.py; openai_compatible_shim.py.

Rust production/binding modules read in full: crates/sharpearena-py/src/lib.rs (including inline tests); crates/sharpearena-wasm/src/lib.rs (including inline tests); crates/sharpearena/src/lib.rs, vec_env.rs, mandate.rs, exec_noise.rs, richness.rs, curriculum.rs, contract.rs, transport_gate.rs (including their inline tests). scenario_gen.rs lines 1–825 were read, covering all production code through line 647 and the initial tests; test-only lines 826–1456 were not read by this reviewer.

Python relevant tests read in full, under crates/sharpearena-py/tests/:

test_checkpoint.py (217 lines); test_pettingzoo.py (139); test_counterfactual.py (207); test_prospective_field.py (238); test_gym.py (188); test_vector.py (237); test_market.py (248); test_lob_env.py (150); test_verifiers.py (587); test_functional.py (179); test_plan_digests.py (159); test_data_blocks.py (157); test_forecast.py (94); test_forecast_evidence.py (287); test_local_agents.py (read in the recent audit).

Tests not on this list were not fully reviewed. In particular, test_deferred.py, test_risk.py, test_paper_trading.py and all optional-integration suites were not fully read.

Plan/data-path scope: both local_field_cli.py and strategy_cli.py were fully read, including their shared resolved-relative-path containment check for CSV inputs; no specific escape was established there. prospective_field.py's frozen-plan/artifact/settlement lineage was fully read and has finding 1. paper_cli.py and paper_trading.py were fully read, including credential-source checks, paper-only broker origin/redirect rules, lifecycle, and commitment/reveal callers. This is not a complete security audit.

Explicit exclusions for this reviewer: native Arena market.rs and lob_market.rs matching/math; leaderboard_ci.rs; spec_hash.rs and _spec_hash.py; complete __init__.py export surface; baseline/confidence/statistical modules not listed above; ecology.py, manipulation.py, adverse_selection.py, realism.py, regime_eval.py, reward_misspecification.py, failure_taxonomy.py, metrics.py, splitmix_inversion.py; edge_manifest.py; strategy_generation.py and strategy DSL internals; trace_promotion.py; release/build/publication scripts, examples and papers. Parent may cover some independently; this report makes no claims on its behalf. The final optional-analysis follow-up was not started/completed.

## Verification limits

“Executed” means tiny synthetic/in-memory fixtures. Native probes used the already available Windows Python extension, not a newly built Python extension; current Python and Rust source independently establish the stated causes. No full Python test suite was run by this reviewer. The parent reported a passing fresh offline locked Cargo workspace suite; that is parent-supplied status, not a test run performed here. No model, training, market-data download, real broker, or simulation study was invoked. No product file was changed; the last checked product git status was clean.

Sample ledger: no empirical pipeline logs accompanied these interfaces, so N unverified for every real-data filter, merge and collapse. The verified fixture counts are one contract/one revision for forecast substitution; one agent/one symbol and one action before bare market reset; one LOB agent across two four-step episodes; two proposed paper orders -> two preflight verdicts -> zero broker submissions but one acted ledger record; one checkpoint prefix and two continuations; one PettingZoo lane; four malformed/out-of-bounds dense action shapes. These are correctness fixtures, not empirical Ns. CSV date/block construction, short-block discards, symbol dictionaries, trace parsing and Minari episode grouping were read but their production before/after counts have no audit log.

Claims: Frozen-contract meaning, canonical RL instructions, leak-safe checkpoint artifacts, reset/replay identity, actual execution, requested difficulty, and process-reward gates disagree with the cited code. CSV path containment was inspected without an established escape; this is not security clearance.

Sample: N unverified for empirical filters/merges/collapses; only the explicitly listed fixture Ns were observed. Wrong reset and abort behavior changes episode boundaries, while the counterfactual batch records action despite zero submissions.

Merges: Forecast settlement joins by claim ID without binding plan contract digests; counterfactual joins intended orders to positional preflight verdicts rather than actual fills. Symbol maps and trace/episode groupings were traced but production key uniqueness/unmatched counts are N unverified. No Stata _merge applies.

Variables: Observations carry closes, share positions and cash; canonical decisions use target weights. Future CSV leaks via checkpoint params; four scenario controls disappear on replay; long-only/weight declarations are not dense-action guards; previous equity contaminates LOB reward; allowed intent is mislabeled executed P&L. No inflation-deflation transform is part of these interface constructions.

Silent failures: Zip drops malformed dense-action tails, defaults replace requested scenario controls/difficulty, reset retains hidden state, and protocol failure retains a scoreable prefix. Actual occurrence counts are N unverified.

Estimation: Default RL weights are return 1.0, deflated Sharpe 0.5 and optional mandate 0.5, while process/format are zero-weight metrics, not gates. No clustering, fixed effects or cluster-count estimator is implemented in this interface slice; native scoring math is outside this review. Empirical estimation N versus planned horizon is unverified.

Without the data: I could not determine which published evaluations or training runs used these failing interface configurations, or quantify the resulting change in their reported outcomes.
