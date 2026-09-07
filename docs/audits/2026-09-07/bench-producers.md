# Final bounded evidence-producer audit

> Historical audit evidence at [sharpebench `933e0c1`](https://github.com/general-liquidity/sharpebench/tree/933e0c1056a2e4707b28762c294323bf05bdab65) and [sharpearena `1be915f`](https://github.com/general-liquidity/sharpearena/tree/1be915f330acabacd171cc350bec0def58d9e134).
> Findings, quoted claims, test counts and references to "current" describe those
> reviewed baselines. See [IMPLEMENTATION.md](IMPLEMENTATION.md) for later repair
> dispositions and remaining work; this report is not a current defect list.

Eight new findings, ordered by consequence. CONFIRMED means visible in source; historical impact on committed evidence is unverified. No generators, tests, model calls, containers, probes, or network requests were executed. No files changed. The previously reported assemble_sweep/analyze grid defect is excluded.

## Findings

1. CONFIRMED: A model fallback changes the evaluated policy without changing the published model identity.

   File: examples/llm-agent/llm_agent.py:14–17, 235–245; crates/sharpebench-harness/examples/llm_field_eval.rs:194–224, 258–261.

   Claim: “No fallback models are configured: the benchmark pins policy identity to the named model”.

   Implementation:

   ```python
   except anthropic.NotFoundError:
       MODEL = HAIKU_FALLBACK
       return client.messages.create(**request_kwargs(MODEL, prompt))
   ```

   HAIKU_FALLBACK is the unversioned “claude-haiku-4-5” at line 60, while the Rust field requests “claude-haiku-4-5-20251001” at line 51. The producer subsequently records “model.to_string()”, not the effective model returned by the shim.

   There is a second identity mismatch on the first fallback call: the cache key is constructed from MODEL before call_model runs (Python line 264), and the resulting fallback decision is saved under that original key (lines 336–340). CACHE_PATH was also fixed using the original model at line 65.

   Consequence: an alias-served policy can be reported and replayed as the requested versioned model. This is a confirmed conditional path, not evidence that the fallback occurred historically.

   Improvement: reject model substitution, or explicitly record requested and effective identities and bind both to response/cache provenance.

   Reproduction: source trace only; no provider request made.

2. CONFIRMED: The synthetic witness’s “independent” calibration agents reuse identical return streams.

   File: crates/sharpebench-harness/examples/pass_witness.rs:99–106, 137–148.

   Claim: “Estimate the field dispersion exactly once, from independent zero-edge calibrators.”

   Implementation:

   ```rust
   0x00CC_0000 + k as u64
   Rng::new(base_seed ^ ((w as u64) << 32) ^ (k as u64 + 1))
   ```

   The calibration-agent index and execution-seed index occupy overlapping low bits. For the same window, calibration agent 0 with execution value 3 and calibration agent 1 with execution value 2 have exactly the same RNG seed:
   `0xCC0000 XOR 3 = 0xCC0001 XOR 2`.

   Because their injected edge is also identical, their complete return series are identical. The five-by-eight calibration combinations produce only 13 distinct seed values per window, not 40 distinct streams. Those counts follow from the constants and seed expression; they are not observed run-log counts.

   Consequence: the five calibration submissions are not independent as claimed. Their shared draws change the calibration dependence structure and can change measured field dispersion. The operational dispersion floor could mask a numerical effect; that was not checked against frozen results.

   Improvement: derive seeds from an unambiguous tuple of domain, calibration-agent index, window, and execution seed. Test cross-agent/run seed uniqueness separately from intentional common random numbers across witness edge levels.

   Reproduction: exact source-level seed equality; no witness generation run.

3. CONFIRMED: Response-cache identity omits the system prompt and effective request configuration.

   File: examples/llm-agent/llm_agent.py:83–91, 110–123, 264–282.

   Implementation:
   `key = hashlib.sha256((MODEL + "\x00" + prompt).encode()).hexdigest()`.

   The actual request additionally includes `"system": SYSTEM`, `kw["temperature"] = 0`, `kw["max_tokens"] = 300`, or max_tokens 4000 and effort “low”.

   A cache hit immediately reuses stored orders. Neither the system prompt nor those request settings nor a scaffold/parser version participates in cache identity.

   Consequence: changing the allocation instructions, thinking/token settings, or interpretation code can silently reuse decisions from an earlier policy configuration. A rerun can therefore appear to evaluate the current scaffold while executing cached outputs from a different one. Historical cache reuse across such changes is unverified.

   Improvement: hash the canonical effective request plus an explicit scaffold/parser version; preserve that configuration with every cached response.

   Reproduction: source trace only; no cache altered or replayed.

4. CONFIRMED: The paid-model example strips the optional cost controls its instructions tell operators to export.

   File: crates/sharpebench-harness/examples/llm_field_eval.rs:13–16, 205–211; examples/llm-agent/llm_agent.py:61–66; supporting callee crates/sharpebench-sim/src/external.rs:189–208, 227–234.

   Claim: run with “LLM_CACHE_DIR / LLM_STATS_DIR / LLM_STRIDE / LLM_MAX_CALLS” exported.

   Implementation: `ExternalAgent::spawn_with_env( … &["ANTHROPIC_API_KEY"], … )`; the callee calls `command.env_clear().envs(agent_environment(extra_vars));`.

   The advertised LLM_* variables are not included in the producer’s explicit passthrough list. Unless the operator separately opts them in through SHARPEBENCH_AGENT_ENV, the shim uses its defaults: stride 5, call cap 800, and script-relative cache/statistics directories.

   Consequence: following the documented invocation does not honor a lower spending cap, different decision cadence, or isolated evidence-cache location. This can alter cost, policy behavior, and replay provenance.

   Improvement: explicitly pass the supported controls and record their effective values in the evidence. Keep credential passing separately allowlisted.

   Reproduction: source trace only; no paid call made.

5. CONFIRMED: Sanitized local-model names are non-unique evidence join keys.

   File: crates/sharpebench-harness/examples/local_open_weight_field_eval.rs:146–166, 295–298, 348–370.

   Implementation:

   ```rust
   if character.is_ascii_alphanumeric() || character == '-' || character == '_' { … } else { '-' }
   let agent_id = format!("local-{}", safe_name(model));
   identity_dir.join(format!("{}.json", safe_name(model)))
   .find(|(agent_id, _, _, _)| *agent_id == score.agent_id)
   ```

   The accepted tag strings “a:b” and “a-b” both become “local-a-b” and use the same identity-file path. The producer checks neither input-tag uniqueness nor sanitized-ID uniqueness. It later joins both the alternative verdict and model metadata by the resulting ID using first-match lookup.

   The ranking entry point does not reject such IDs: supporting inspection of crates/sharpebench-core/src/composite.rs:1639–1742 shows it scoring the supplied entries.

   Consequence: two distinct local-model entries can share an identity artifact and receive another entry’s model metadata or alternative eligibility result. Whether any historical tag list collided is unverified.

   Improvement: use an injective encoding or a digest of the exact tag for identity keys; reject duplicate IDs before executing any model.

   Reproduction: source-level string collision only; no local model started.

6. CONFIRMED: An unknown dataset selector can be published as a “complete” empty model field.

   File: crates/sharpebench-harness/examples/llm_field_eval.rs:13–16, 166–175, 290–293.

   Claim: “only a completely evaluated field is renamed to the requested output.”

   Implementation: `if o != name { continue; }`, followed unconditionally by `std::fs::rename(&partial, &out).expect("publish completed field atomically");` and `eprintln!("wrote {n_records} complete records to {out}");`.

   The optional selector is never validated against DATASETS. A misspelled selector skips every dataset and reaches the normal publication path with zero records.

   Related source support: evidence_sweep.rs:187–193 explicitly skips failed dataset loads and still reaches its normal final flush/message at 290–291. risk_managed_eval.rs:126–131 and 313–317 likewise permits missing dataset/perturbation support without a failing exit. These warnings are visible on stderr, but the resulting artifact does not encode completeness.

   Consequence: producer success and an ordinary final filename do not establish that the requested evidence support was evaluated. This is an upstream producer-contract defect, not the previously identified downstream grid-key defect.

   Improvement: validate selectors before opening output; require the planned dataset/run support before publishing; otherwise fail or emit an explicitly incomplete manifest.

   Reproduction: control-flow inspection only; no output file created. Actual before/after N: N unverified.

7. CONFIRMED: The evidence figure loader silently discards truncated or non-object-ending records.

   File: paper/src/make-evidence-figures.py:55–63, 229–242.

   Claim: “Every plotted result and data-dependent crossing is reduced from records written by the sweep…” at lines 5–6.

   Implementation: `if line.endswith("}"):` followed by `out.append(json.loads(line))`.

   Any nonempty truncated JSONL record that does not end in “}” is ignored without an error or dropped-record count. The thousand-agent ECDF subsequently uses however many retained agent records remain: `ecdf = [(i + 1) / len(vals) for i in range(len(vals))]`.

   Its axis still says “fraction of the 1,000 random agents” at lines 262 and 283. An intact summary record can also supply annotations while the agent rows plotted underneath are incomplete.

   Consequence: a damaged or incomplete evidence file can produce a normal-looking, renormalized figure with an incorrect support label. Historical corruption or dropped rows were not checked.

   Improvement: parse every nonempty JSONL line strictly; validate expected unique agent identities, row counts, and agreement between summaries and underlying records before plotting.

   Reproduction: source trace only; no malformed file or figure generated. Raw lines → retained records → plotted agents: N unverified at every stage.

8. CONFIRMED: The “eligible on either path” figure annotation adds path counts instead of counting their union.

   File: paper/src/make-evidence-figures.py:253–264.

   Implementation: `eligible = sum(s["shipped_floor"]["n_rank_eligible"] + s["field_measured"]["n_rank_eligible"] for s in summaries.values())`.

   Label: “{eligible} of {len(agents):,} agent-dataset cells” and “eligible on either path”.

   Adding the two marginal eligibility counts double-counts every agent-dataset cell eligible on both paths. The denominator counts each underlying agent record once.

   Consequence: the annotation can exceed the true number of eligible cells, and potentially its own denominator. If both counts are zero, the error is dormant; no historical nonzero overlap was established.

   Improvement: compute the Boolean union by unique dataset/agent identity from underlying records, then verify it against independently stored summaries.

   Reproduction: source-level aggregation inspection only; no figure generated.

## Coverage

Complete authored bodies read, including inline tests where present:

- crates/sharpebench-harness/examples/evidence_sweep.rs: 292 lines.
- crates/sharpebench-harness/examples/external_rules_eval.rs: 531 lines.
- crates/sharpebench-harness/examples/llm_field_eval.rs: 294 lines.
- crates/sharpebench-harness/examples/local_open_weight_field_eval.rs: 467 lines.
- crates/sharpebench-harness/examples/luck_floor_1000.rs: 386 lines.
- crates/sharpebench-harness/examples/mandate_eval.rs: 249 lines.
- crates/sharpebench-harness/examples/pass_witness.rs: 256 lines.
- crates/sharpebench-harness/examples/relative_mandate_eval.rs: 284 lines.
- crates/sharpebench-harness/examples/risk_managed_eval.rs: 318 lines.
- crates/sharpebench-harness/examples/seed_leg_eval.rs: 341 lines.
- examples/llm-agent/llm_agent.py: 354 lines.
- paper/src/make-evidence-figures.py: 305 lines.
- paper/src/make-figures.py: 136 lines.
- paper/src/original-essay-figures.py: 146 lines.
- paper/evidence/index_sharpe.py: 34 lines.

Total: 15 requested files, 4,393 lines. Supporting callee checks: external.rs:184–242 and composite.rs:1611–1745; these were targeted checks, not another full-file review.

## Tests and limitations

The three inline tests in local_open_weight_field_eval.rs:420–467 were read. They cover shim preflight diagnostics/module naming; one requires the sibling installation and is explicitly ignored. No inline tests were found in the other requested bodies. Existing witness monotonicity and seed-leg decomposition assertions were also inspected. No tests or assertions were executed, and no frozen artifact, cache, figure PDF, or execution log was used to establish historical impact.

## Per-category coverage

Claims: confirmed disagreements concerning pinned model identity, independent calibration, exported controls, completed fields, and plotted support/eligibility labels.

Sample: ingestion, selectors, window construction, finite-Sharpe filtering, figure filters, and aggregations were traced in source. Actual N before and after filters/collapses: N unverified; no execution logs were examined. Seed-identity collisions are established algebraically, not presented as logged sample counts.

Merges: local-model metadata/alternative-verdict joins use a non-unique sanitized agent ID; figure summary dictionaries and agent subsets have no completeness/identity reconciliation. Observed unmatched counts and duplicate dataset-agent/run pairs: N unverified. No Stata _merge exists.

Variables: traced requested/effective model and controls, cache keys, calibration seeds, annualized versus per-period Sharpe, gate fields, drawdowns, and ECDF/eligibility counts. Primary scoring formulas were left to the root audit.

Silent failures: confirmed selector skips, successful publication after dataset skips, and malformed-line omission. Credential passing is explicitly allowlisted; no new direct credential-disclosure finding was established.

Estimation: these surfaces invoke benchmark scorers or descriptive reductions rather than fixed-effect regressions; regression clustering, cluster counts, and weights are not applicable. Seed/window dependence matters, with a confirmed calibration seed collision. Estimation N versus realized support remains N unverified.

The one thing I could not check without the frozen artifacts, caches, and run logs is whether these paths changed any published model identity, numerical result, eligibility verdict, or figure.

## Root cross-check

The root independently inspected the witness seed construction and enumerated the 5-by-8 seed-expression inputs. They yield exactly 13 distinct RNG seeds per window, with `0xCC0000 XOR 3 == 0xCC0001 XOR 2`. This checks the collision arithmetic only; no return series, witness field or historical result was regenerated.
