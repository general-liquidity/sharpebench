# Final bounded Bench coverage report

> Historical audit evidence at [sharpebench `933e0c1`](https://github.com/general-liquidity/sharpebench/tree/933e0c1056a2e4707b28762c294323bf05bdab65) and [sharpearena `1be915f`](https://github.com/general-liquidity/sharpearena/tree/1be915f330acabacd171cc350bec0def58d9e134).
> Findings, quoted claims, test counts and references to "current" describe those
> reviewed baselines. See [IMPLEMENTATION.md](IMPLEMENTATION.md) for later repair
> dispositions and remaining work; this report is not a current defect list.

Read-only, text-only handoff. All findings below are new relative to the three earlier reports. CONFIRMED means visible in source; none was executed or reproduced in this pass.

## Findings

1. CONFIRMED — Lifecycle violations never reach the composite process gate.

File: crates/sharpebench-core/src/composite.rs:1162–1169.
Claim: “a single block-severity violation in any run is disqualifying.”
Code: “sub.runs.iter().map(|r| process_score(&r.trace)).collect();”

File: crates/sharpebench-core/src/process.rs:854–861.
Existing regression explicitly demonstrates the discrepancy:
“a lifecycle-only trace, however badly ordered, still scores 1.0 under the old API.”
“assert_eq!(process_score(&t).score, 1.0);”
“assert_eq!(process_score_with_ordering(&t).score, 0.0);”

The composite calls the legacy scorer, not process_score_with_ordering. A submitted trace containing an unauthorized submission or a fill without acknowledgment can therefore remain process_ok=true despite the lifecycle checker classifying it as a block. The legacy API’s unchanged behavior is documented; the integration gap is that the headline process gate never invokes the additional control.

Consequence: an otherwise eligible submission is not disqualified for these recorded lifecycle breaches. Improvement: integrate the ordering-aware scorer into the composite, or explicitly label the composite’s lifecycle coverage as absent. Add an end-to-end composite regression, not only standalone lifecycle tests.

2. CONFIRMED — “Disqualification reasons” are computed for a different experiment than the ranked board.

File: crates/sharpebench-cli/src/analysis_cmd.rs:338–340.
Claim: “Pure legibility over the composite score: nothing here changes eligibility semantics.”
Code at :360–366: “let cfg = ScoreConfig::default();” and “let score = score_agent(sub, &cfg);”

File: crates/sharpebench-wasm/src/lib.rs:318–320 makes the same claim; :334–338 independently calls “score_agent(sub, &cfg)” for each submission. npm/src/index.ts:267–275 and npm/mcp/src/server.ts:196–200 expose this path.

The actual field scorer first restricts to shared run positions (composite.rs:1644–1650), floors trial count at observed field size (:1661–1663), estimates field dispersion (:1693–1705), and supplies field benchmarks. score_agent explicitly has no field context (:1096–1100). In relative-to-benchmark mode, its absent benchmark makes every per-run relative test false (:581–592), even when the supplied field contains that benchmark.

Consequence: disqualify/classifyDisqualification can report different eligibility and reasons from score on the same inputs. Improvement: classify the scores returned by rank/rank_declared using the identical configuration. The existing CLI/WASM tests use simple steady/noise examples and do not compare classifier output with field-ranked output.

3. CONFIRMED — CSV import discards the run identities needed for cross-agent alignment.

File: crates/sharpebench-cli/src/import_cmd.rs:311–314.
Claim: “rows group into runs per agent, in first-appearance order for a deterministic field.”
Code at :363–367 groups each agent’s runs independently by run_label; :378 removes those labels:
“(a, runs.into_iter().map(|(_, r)| r).collect())”

Deterministic order is not a shared cell order. If agent A first appears with runs r0 then r1, while B first appears with r1 then r0, their serialized run index 0 refers to different runs. A missing run can similarly shift every later position. Wide import also discards header identities and removes completely empty runs at :304:
“runs.retain(|r| !r.is_empty());”

The downstream relative comparison uses “b.runs.get(i)” and then positionally zips equal-length return arrays (composite.rs:587–602); it cannot recover discarded labels.

Consequence: benchmark-relative passes, shared-cell restriction, and field diagnostics can compare different windows/seeds as though aligned. Improvement: build a common explicit run-key order across agents and reject/report missing cells; preserve period identities as well. The long-format test at import_cmd.rs:462–497 checks grouping/counts, not reversed label order or matching cell identity. N unverified.

4. CONFIRMED — The regime CLI can shift labels and returns while still claiming row alignment.

File: crates/sharpebench-cli/src/main.rs:353–356.
Claim: “The three files are aligned by row.”

The same --col value is used for both return files and the regime file (:364, :377–386). If the regime file lacks that named column, read_label_column silently uses:
“None => (0, false)” (:468).

For a normal command using --col ret on return files and a separate single-column “regime” file, this includes the regime header as the first observation. The resulting length mismatch merely prints a warning and proceeds (:396–411), shifting the actual labels and losing the final period.

Separately, return cells are independently skipped at :521–522 and label cells at :482. Equal remaining lengths do not establish that the same original rows survived. The analysis-command CSV readers similarly skip empty cells independently (analysis_cmd.rs:112–113, :144–145).

Consequence: regime-specific gaps and reversal claims can concern the wrong periods; missingness can silently change support. Improvement: separate return-column and regime-column options, reject unknown headers, and retain/join row identities or apply a common missing-row mask. The WASM wrapper correctly rejects unequal array lengths (:367–373), but that does not repair the CLI reader. N unverified.

5. CONFIRMED — WASM trial counts silently wrap at the 32-bit boundary.

File: crates/sharpebench-wasm/src/lib.rs:109–112.
Code:
“let n_trials = v.get("n_trials").and_then(serde_json::Value::as_u64)
    .ok_or("missing or non-integer field: n_trials")? as u32;”

The parser accepts a u64 and narrows it with an unchecked cast. Thus 4,294,967,297 becomes 1, and 4,294,967,296 becomes 0. Both are exactly representable JavaScript integers. npm’s nTrials input and the MCP n_trials schema provide no corresponding upper-bound validation (npm/src/types.ts:146–148; npm/mcp/src/server.ts:100–105).

Consequence: a large declared search footprint can lose essentially its entire multiple-testing correction in both LITE and FULL wrapper calls. Improvement: use u32::try_from and reject zero/out-of-range values. Existing WASM tests cover omitted and ordinary trial counts, not narrowing boundaries. Static arithmetic demonstration only; no wrapper call executed.

6. CONFIRMED — Team composition discards every member’s compute cost.

File: crates/sharpebench-sim/src/agent.rs:30–34.
Code: “for o in m.decide(obs).orders” retains only orders from each member’s decision.

The returned consensus decision at :54–57 always contains:
“cost: None”.

This is an active harness path: run_team constructs TeamAgent and calls run_backtest (crates/sharpebench-harness/src/lib.rs:756–761). The simulator only accumulates compute cost when decision.cost is Some (crates/sharpebench-sim/src/engine.rs:429–430), and writes that total to Run.cost (:443).

Consequence: a team of paid agents reports zero/unreported compute cost even when all members supplied costs. Cost-normalized columns consequently become unavailable instead of reflecting the team’s actual expenditure; this does not directly change the primary eligibility gate. Improvement: aggregate member billable costs explicitly and document whether sequential latency is summed or separately reported. The team regression at harness/lib.rs:1004–1036 checks role-series lengths but uses cost-free reference agents and never checks cost preservation.

7. CONFIRMED — Momentum’s configured lookback is unused.

File: crates/sharpebench-sim/src/agent.rs:350–357.
Declared API:
“pub struct Momentum { pub lookback: usize }”
Default: “Self { lookback: 10 }”.

The entire decide implementation instead computes:
“h[h.len() - 1] / h[0] - 1.0” (:368–369).

No read of self.lookback exists in that implementation. Momentum always uses all history supplied in the observation, whatever lookback the caller selects.

Consequence: parameter sweeps over lookback produce identical decisions, and the advertised default of 10 does not determine the trading horizon. This reference agent is used by run, stress, capture, and team examples. Improvement: apply the configured trailing slice with explicit insufficient-history behavior; add a history whose long and short lookbacks imply opposite positions. Existing agent tests focus on RiskManaged and RandomAgent, not Momentum parameter sensitivity.

8. CONFIRMED — Python/WASM board wrappers silently discard declared mandates despite CLI-parity claims.

File: crates/sharpebench-py/src/lib.rs:577–582.
Claim: “the output is the same CompositeScore array the CLI prints with --json”.

Code at :589–592 parses “Vec<AgentSubmission>” and calls “core_rank(&subs, &cfg)”. WASM does the same at crates/sharpebench-wasm/src/lib.rs:29–33; npm forwards directly to it.

The CLI instead parses “Vec<sharpebench_core::DeclaredSubmission>” (main.rs:1858), splits declarations (:1865), and calls rank_declared (:1888). The core explicitly says the declaration is part of the submitted object and directs consumers to parse DeclaredSubmission (composite.rs:82–92). AgentSubmission has no declared_mandate member (:62–75), and its serde deserialization does not reject unknown fields.

Consequence: the same valid CLI input silently loses its declared-mandate verdict and within-mandate reporting in Python/WASM/npm. The primary host rank need not change, but the promised complete board does. Improvement: use the same declared-submission parsing/ranking path across wrappers, including single-submission declared scoring where applicable. The Python golden-parity test at test_board.py:39–43 does not exercise a declared-mandate fixture.

9. CONFIRMED — Nonbinary outcomes are silently converted into “correct” outcomes.

File: crates/sharpebench-wasm/src/lib.rs:235–237.
Input contract: “outcomes: bool[] (or 0/1 numbers)”.

Code at :259–262:
“serde_json::Value::Number(n) => Ok(n.as_f64().unwrap_or(0.0) != 0.0)”.

The CLI advertises “0/1 per decision, 1 = the call was right” (analysis_cmd.rs:504), but uses “r.into_iter().map(|v| v != 0.0).collect()” (:529). Its numeric CSV parser does not reject nonfinite floats.

Consequently -1, 2, and 0.3 all become true; a CLI NaN also compares unequal to zero and becomes true. For example, a conventional -1/+1 outcome encoding becomes an all-true vector.

Consequence: uncertainty diagnostics can report zero binary outcome variability for mixed successes/failures or malformed input. Improvement: accept booleans and exact finite 0/1 values only; reject everything else with a row/index error. The existing WASM test at :677–680 exercises valid 0/1 coercion, not invalid numeric values.

10. CONFIRMED — A valid Unicode agent ID can panic the human-readable CLI after scoring.

File: crates/sharpebench-cli/src/main.rs:1953.
Code: “truncate(&s.agent_id, 18)”.

The helper at :1984–1988 compares byte length, then slices:
“format!("{}…", &s[..n - 1])”.

Rust string slicing requires a UTF-8 character boundary. Seven repetitions of a three-byte character have byte length 21, so this path slices at byte 17, inside a character. Agent IDs are unrestricted String values in the submission type.

Consequence: an otherwise valid field can complete scoring and then terminate with a panic instead of producing its human-readable board. JSON mode avoids this rendering helper. Improvement: truncate on character/grapheme boundaries and add multibyte-ID coverage. Static construction only; no panic probe executed.

## Coverage

Complete authored production bodies were read for these 16 files, including inline tests where present:

- crates/sharpebench-core/src/process.rs — 893 lines.
- crates/sharpebench-sim/src/trajectory.rs — 212 lines.
- crates/sharpebench-sim/src/agent.rs — 567 lines.
- crates/sharpebench-cli/src/analysis_cmd.rs — 946 lines.
- crates/sharpebench-cli/src/arena_cmd.rs — 419 lines.
- crates/sharpebench-cli/src/forecast_cmd.rs — 296 lines.
- crates/sharpebench-cli/src/import_cmd.rs — 544 lines.
- crates/sharpebench-cli/src/lineage_cmd.rs — 276 lines.
- crates/sharpebench-cli/src/main.rs — 2,190 lines.
- crates/sharpebench-cli/src/update.rs — 305 lines.
- crates/sharpebench-py/src/lib.rs — 721 lines.
- crates/sharpebench-py/python/sharpebench/__init__.py — 98 lines.
- crates/sharpebench-wasm/src/lib.rs — 744 lines.
- npm/src/index.ts — 302 lines.
- npm/src/types.ts — 385 lines.
- npm/mcp/src/server.ts — 216 lines.

Additional focused test files read completely:

- crates/sharpebench-cli/tests/external_entrant_sandbox.rs.
- crates/sharpebench-arena/tests/cli_arena_cmd.rs.
- crates/sharpebench-py/tests/test_board.py.
- crates/sharpebench-py/tests/test_stats.py.
- npm/test/smoke.test.js.
- npm/mcp/test/smoke.test.js.

Direct integration references additionally inspected: composite.rs submission/declaration parsing, fieldless scoring, process gate, positional excess-return matching, cost columns, and rank preprocessing; engine.rs compute-cost accumulation; harness/lib.rs run_team and its focused regression. These were integration checks, not a repeated primary-math audit.

## Other reviewed boundaries and limitations

The strict trajectory CLI uses verify_trajectory_strict by default. Permissive replay requires the explicit --allow-unbound-trajectory flag. I found no additional trajectory finding to promote.

The optional updater is disabled in the default build. Its checked trust chain is GitHub HTTPS plus a checksum downloaded from the same release; the body does not verify a release signature. Its comments refer to a “signed release binary” (update.rs:10–11, :126). Treat signature verification as an explicit hardening opportunity, not something demonstrated by the present updater. No update request, download, or replacement was attempted.

No new finding was promoted from arena_cmd, forecast_cmd, or lineage_cmd beyond previously reported underlying-library issues. This is not a claim that those broader subsystems are defect-free.

## Per-category coverage

Claims vs. code: composite lifecycle gating, classifier equivalence, wrapper/CLI board parity, row alignment, and outcome encoding exceed what the inspected paths implement; updater signature trust is narrower than “signed release” terminology may suggest.

Sample: independent blank-cell removal, empty imported-run removal, and positional truncation can change support; before/after empirical counts are N unverified. No empirical dataset or execution log was produced in this pass.

Merges: import groups by agent/run label but discards run identity before positional cross-agent matching; common keys, unmatched-cell disposition, and duplicate agent-period pairs are not verified by that adapter.

Variables: traced configured lookback, member compute costs, trial-count narrowing, declared mandates, and outcome coercion through the relevant wrappers/callers; alignment defects are described above.

Silent failures: discarded lifecycle severity, member costs, run labels and declarations; unchecked integer narrowing; independent blank removal; nonbinary-to-true coercion; UTF-8 rendering panic.

Estimation: wrappers inherit the kernel’s estimators; no new regression, fixed effects, clustering, or weights were introduced in these files. The classifier incorrectly omits field-dependent estimation context. Actual estimation N and dependence-unit counts are N unverified.

## Execution and unfinished coverage

No tests, scoring experiments, model calls, containers, network requests, updater actions, or product edits were performed in this final pass. All findings are source-backed, not newly reproduced. Install-smoke/release-install tests and generated WASM/package output were not inspected; the requested authored production slice has no remaining unread files.

Data limitation: without the actual imported CSVs and submitted execution records, I could not determine how many published results used misaligned cells, lost cost/declaration evidence, or affected input encodings.

## Root cross-check of scope

The lifecycle finding above is a confirmed absence of integration, but it is **already explicitly disclosed** in paper/sections/03-benchmark.tex:45: “ordering is available to a caller and is not yet a conjunct” of eligibility. The reviewer report is retained unchanged. Treat this item as a documented coverage/design decision to revisit, not an undisclosed regression or proof that the existing paper claims lifecycle enforcement.
