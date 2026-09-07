# Arena remaining diagnostics and export-surface audit

> Historical audit evidence at [sharpebench `933e0c1`](https://github.com/general-liquidity/sharpebench/tree/933e0c1056a2e4707b28762c294323bf05bdab65) and [sharpearena `1be915f`](https://github.com/general-liquidity/sharpearena/tree/1be915f330acabacd171cc350bec0def58d9e134).
> Findings, quoted claims, test counts and references to "current" describe those
> reviewed baselines. See [IMPLEMENTATION.md](IMPLEMENTATION.md) for later repair
> dispositions and remaining work; this report is not a current defect list.

Read-only audit-analysis follow-up, 2026-09-07. Repository: [sharpearena baseline](https://github.com/general-liquidity/sharpearena/tree/1be915f330acabacd171cc350bec0def58d9e134). No files changed. No model calls, native simulation runs, network requests, or stress tests. Findings below were reproduced with tiny in-memory fixtures unless stated otherwise. They concern public diagnostic/research helpers; no change to the official native rank key was established.

## Findings

1. CONFIRMED — The risk-control radar rewards larger drawdowns under its documented baseline anchors.

   File: crates/sharpearena-py/python/sharpearena/regime_eval.py:133–135. Quote: “Less drawdown is better” followed by `return -float(metrics.get(_RISK_KEY, 0.0))`. But `_anchor_axis`, lines 148–152, calculates `denom = base_raw - zero` and `n = (value - zero) / denom`.

   Claim: lines 168–171 specify the zero anchor as FlatPolicy and the base anchor as EqualWeightLong, with risk-control reading inverted maximum drawdown. Flat has zero drawdown. For a risky base with positive drawdown, the denominator is negative, reversing the intended ordering.

   Executed fixture: zero-anchor drawdown 0, base-anchor drawdown 0.10. Candidate drawdowns 0, 0.05, 0.10, 0.20 produced risk-control scores approximately 0, 26.7949, 50, 80. Consequence: a twice-as-large drawdown receives a substantially better risk-control score and increases the combined radar score.

   Test coverage: tests/test_baselines_eval.py:139–140 assigns the supposed flat anchor a 30% drawdown and the base a 10% drawdown, masking the reversal under the documented real anchors. Its end-to-end test instead supplies panels without drawdown, so every risk input defaults to zero.

2. CONFIRMED — RunMetrics omits the initial return and initial NAV from cumulative return and drawdown.

   File: crates/sharpearena-py/python/sharpearena/metrics.py:75–76 starts the reward-derived NAV path from 1.0 and appends its first post-return value. But lines 92–94 then use `first = self._navs[0] or 1.0`, `self.realized_return = self._navs[-1] / first - 1.0`, and `peak = self._navs[0]`.

   Claim: the module describes these quantities as the run’s “realized return, max drawdown” at lines 5–6. The calculations instead measure from the first post-step NAV.

   Executed fixture: returns [-0.5, 0.1] produced realized_return = +0.10 and max_drawdown = 0.0. The full run ends at NAV 0.55, hence return −0.45 and maximum drawdown 0.50. Another fixture [0.1, −0.1, 0.2, −0.05] produced 0.026 instead of 0.1286; tests/test_metrics_panel.py:51 explicitly pins the truncated value.

   Consequence: first-step losses can disappear entirely from the performance/risk panel, and Calmar inherits the wrong numerator and potentially wrong denominator.

3. CONFIRMED — Failure rollups classify malformed, explicitly failed, and nonfinite episodes as clean.

   File: crates/sharpearena-py/python/sharpearena/failure_taxonomy.py:206–211. Quote: dictionary records are classified using `item.get("returns", [])`, `item.get("events", [])`, and `item.get("mandate")`; status/error fields and required-field presence are not checked. `_nav_bankrupt`, lines 58–59, uses `nav *= 1.0 + float(r)` and `if nav <= 0.0:` without a finite-value check. Line 168 falls through to `FailureMode.CLEAN`.

   Claim: lines 223–225 say callers can “roll up raw rollouts directly”; lines 175–177 describe clean_rate as the share of clean episodes.

   Executed fixtures: `classify_episode_failure([], [{"event": "protocol_error"}])` returned CLEAN. Rolling up `[{"status": "failed", "error": "timeout"}, {"returns": [NaN], "events": []}]` returned total = 2, clean = 2, failures = 0, clean_rate = 1.0.

   Consequence: malformed/missing evidence and protocol failures can inflate the clean-episode rate when supplied to this public helper. Its enumerated financial-failure modes do not constitute a general rollout-validity check. No production evaluation using these malformed inputs was established.

4. CONFIRMED — The momentum negative-control proxy is not the claimed greedy maximizer of its reward.

   File: crates/sharpearena-py/python/sharpearena/reward_misspecification.py:194–197. Claim: MomentumChasePolicy is the “Greedy maximizer of indicator_shaped” and its position sign agrees with short-window momentum “by construction.”

   Opposing code: the policy at lines 211–213 uses only `np.sign(closes - self._prev)`. The reward at lines 116–120 instead compares the current net position with `np.sign(np.sum(rets[i - window : i]))`, using a default three-bar window of portfolio returns. A one-bar market-price change is not the same variable or window.

   Executed fixture: observed closes [100, 80, 64, 70.4] produced actions [+1, −1, −1, +1]. With corresponding simple portfolio-return history [−0.2, +0.2, −0.1, 0], indicator_shaped returned 0.0. Changing only the final action to −1 returned 1.0. Thus the advertised proxy chooses the strictly worse action for its own stated reward.

   Additional source detail: MomentumChasePolicy and RecencyChasePolicy implement the same action rule at lines 205–213 and 228–236, despite representing distinct reward controls.

   Consequence: a score gap from these proxies does not demonstrate the effect of greedily optimizing the named rewards. The module honestly labels proxies as stand-ins rather than trained agents, but the stronger optimization claim is false. No training experiment was performed.

5. CONFIRMED — The diagnostic downside deviation centers losing returns around their own mean, producing zero or numerically explosive Sortino values for repeated losses.

   File: crates/sharpearena-py/python/sharpearena/metrics.py:106–111. Quote: `neg = r[r < 0.0]`, then `self.downside_deviation = float(np.std(neg)) if neg.size else 0.0`, followed by mean return divided by that value.

   This measures dispersion among losing returns, not their deviation below the zero target. For comparison, the package’s explicitly documented downside-risk denominator in rewards.py:90–99 is the RMS of negative returns: `np.sqrt(np.mean(np.square(downside)))`.

   Executed fixtures: two −10% returns produced downside_deviation = 0.0 and sortino = 0.0. Three −10% returns produced floating-point residue of approximately 1.3878e−17 and sortino approximately −7.2058e15.

   Consequence: repeated equal losses are represented as having no downside risk or an enormous numerical ratio depending on array length. The diagnostic panel disagrees with the package’s own downside-risk construction. tests/test_metrics_panel.py:45–46 pins the centered-negative standard deviation and resulting ratio for its hand example.

6. CONFIRMED — RunMetrics keeps a mutable reference to the caller’s action array and can silently lose turnover.

   File: crates/sharpearena-py/python/sharpearena/metrics.py:79. Quote: `w = np.asarray(weights, dtype=float).ravel()`; line 86: `self._prev_weights = w`.

   For an already-compatible NumPy array, these operations retain shared storage rather than an immutable snapshot of the previous action.

   Executed fixture: record weights [0.5, 0.5], mutate the same float array in place to [1, 0], then record again. Reported turnover remained 1.0—the opening allocation only—instead of 2.0 including the second allocation change.

   Consequence: policies or collectors reusing action buffers understate turnover, cost_drag, and an opted-in turnover penalty. Existing tests pass fresh lists, avoiding the aliasing path.

7. CONFIRMED — The efficiency “penalty” improves negative base scores when costs increase.

   File: crates/sharpearena-py/python/sharpearena/metrics.py:10–12. Claim: the penalty “makes a negative score worse” and a cheaper run with the same edge ranks above an expensive one.

   Opposing code at lines 192–194: `cost = max(raw_cost, 0.0) / steps`, `penalty = 1.0 / (1.0 + cost)`, `return base * penalty`. For negative base values, greater cost moves the result upward toward zero.

   Executed fixture using the supported `base_key="mean_return"`: base −1 scored −1.0 with no cost, but −0.5 after recording 10,000 tokens for one step.

   Consequence: descending cost-adjusted ordering rewards expense for negative-valued base metrics. This does not establish a defect for the default nonnegative deflated-Sharpe probability; it contradicts the documented negative-score behavior and affects the public configurable-base path.

8. CONFIRMED — SplitMix candidate enumeration accepts an impossible published unit and returns states inconsistent with it.

   File: crates/sharpearena-py/python/sharpearena/splitmix_inversion.py:63–70. Claim: return “every state consistent with one published 53-bit next_unit.” Lines 72–77 only check the unit’s range, then use `top53 = int(float(unit) * (1 << 53))`. They do not require the supplied value to lie on the generator’s 2⁻⁵³ grid.

   Executed fixture: unit = 2⁻⁶⁰ was accepted and produced 2,048 candidates. Recomputing the published unit from every candidate produced 0.0, not 2⁻⁶⁰.

   Consequence: a caller supplying an arbitrary rounded/off-grid value receives a falsely nonempty “consistent-state” set. This is an input-validation defect in the research primitive, not a demonstrated generator-state recovery attack, and does not refute the 2,048-candidate statement for valid published units.

## Exact coverage

Production files read completely in this pass:

- crates/sharpearena-py/python/sharpearena/regime_eval.py — 203 lines.
- crates/sharpearena-py/python/sharpearena/reward_misspecification.py — 377 lines.
- crates/sharpearena-py/python/sharpearena/failure_taxonomy.py — 253 lines.
- crates/sharpearena-py/python/sharpearena/metrics.py — 200 lines.
- crates/sharpearena-py/python/sharpearena/splitmix_inversion.py — 87 lines.
- crates/sharpearena-py/python/sharpearena/__init__.py — 689 lines.
- crates/sharpearena-py/python/sharpearena/_spec_hash.py — 54 lines.
- crates/sharpearena-py/python/sharpearena/sharpearena_py.pyi — 232 lines.
- crates/sharpearena-py/pyproject.toml — complete.
- crates/sharpearena-py/Cargo.toml — complete.

Also inspected the tracked py.typed packaging marker and relevant call-site searches. Revisited rewards.py:83–103 for the package’s downside-risk definition; that module was already read fully in the interface pass.

Tests read completely in this pass:

- tests/test_exports.py — 71 lines.
- tests/test_failure_taxonomy.py — 213 lines.
- tests/test_metrics_panel.py — 119 lines.
- tests/test_reward_misspecification.py — 165 lines.
- tests/test_spec_hash.py — 69 lines.
- tests/test_splitmix_inversion.py — 29 lines.
- tests/test_stub_drift.py — 242 lines.

Tests read partially: tests/test_baselines_eval.py:125–219 and tests/test_trace.py:175–245.

Export/packaging result: no additional anchored runtime/stub/export discrepancy was established from the source inspection. The import-time hash check occurs before the other package imports; the declared package versions agree at 0.24.1. This is not a freshly built wheel, runtime stub-drift test, Python-version compatibility matrix, or release-artifact verification. The parent’s separate source/packaged-spec-hash work is excluded from this report.

Explicit exclusions: the parent-owned strategy_generation, edge_manifest, trace_promotion, confidence, baselines, ecology, manipulation, adverse_selection and realism implementations were not reopened for a new review. No new review of native scoring, matching, publication scripts, or empirical paper results occurred here.

## Verification and sample limitations

All executions were tiny ordinary arithmetic/input fixtures using the real pure-Python module functions. The package/native imports were replaced with minimal in-memory scaffolding where necessary; native scoring was configured to raise if called and was not invoked. No source/test files were created or modified. The final checked product git status was clean.

No empirical execution logs were supplied for this slice. Production sample sizes are therefore N unverified. Specific code-level sample transformations inspected include:

- regime_eval.py:87–98 pools each seed’s observed rewards into exactly one regime bucket; warmup is assigned to chop, not discarded. Production before/after N unverified.
- reward_misspecification.py:284–290 pools every returned reward, but only episodes with at least two returns enter its per-seed pass-rate list. The denominator therefore excludes shorter episodes; production counts before/after this filter are N unverified.
- metrics.py:106 selects negative returns for the downside calculation; production N before/after selection is unverified.
- failure_taxonomy.py:230–233 counts every supplied episode record without filtering; malformed records can increase the clean count as finding 3 demonstrates.

Claims: Risk-control direction, whole-run return/drawdown, greedy negative-control optimization, negative-score cost penalties, and candidate-state consistency disagree with the quoted implementations. No additional export/stub/hash-handshake discrepancy was established in this pass.

Sample: N unverified for empirical inputs, regime buckets, short-episode pass-rate exclusions, and negative-return selection. Tiny fixture Ns are given in the findings; they are not empirical sample counts.

Merges: No database merge occurs in this slice. Regime results concatenate observations across seeds; proxy scoring concatenates return series and separately filters the pass-rate denominator. No empirical seed uniqueness or observation-level duplication audit was possible from logs.

Variables: The diagnostic NAV baseline drops the first return; downside deviation centers losses; turnover aliases mutable arrays; radar risk direction reverses under documented anchors; the purported greedy proxy uses a different variable/window from its reward. No inflation-deflation transform is implemented in these helpers.

Silent failures: Missing episode fields default to empty evidence and clean classification; NaN escapes bankruptcy detection; action-buffer mutation erases turnover; short episodes leave the pass-rate denominator; impossible SplitMix units truncate onto a different observation.

Estimation: Regime and proxy diagnostics pool returns before native scoring; no clustering, fixed effects, or cluster-count estimator is implemented here. Their production Ns are unverified. Diagnostic risk/cost formulas have the stated errors; no official rank-key change was established.

Without the data: I could not determine which reported diagnostic panels, failure rates, or negative-control conclusions used these paths, nor quantify corrections to published results.
