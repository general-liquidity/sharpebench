# Final paper-producer audit: read-only, source-only

> Historical audit evidence at [sharpebench `933e0c1`](https://github.com/general-liquidity/sharpebench/tree/933e0c1056a2e4707b28762c294323bf05bdab65) and [sharpearena `1be915f`](https://github.com/general-liquidity/sharpearena/tree/1be915f330acabacd171cc350bec0def58d9e134).
> Findings, quoted claims, test counts and references to "current" describe those
> reviewed baselines. See [IMPLEMENTATION.md](IMPLEMENTATION.md) for later repair
> dispositions and remaining work; this report is not a current defect list.

Six new findings. CONFIRMED means visible in source, not reproduced against historical artifacts. Historical impact is unverified throughout. No experiments, model calls, producer executions, or file changes were made. The previously reported F4 optional-readback/seed-cache issue is excluded.

## Findings

1. CONFIRMED: Predictability comparisons score the oracle on a longer sample.

   File: paper/src/make-predictability.py:62, 129–154, 159–163, 217–231.

   Claim: line 62 defines `WARMUP = 30  # causal prefix length before the first scored prediction`. Lines 8–9 describe “three adversaries evaluated on next-bar return prediction from a causal prefix”.

   Code: baseline and honest predictions initialize with `np.full((T, S), np.nan)` and only populate `range(WARMUP, T)`. Evaluation uses `mask = ~np.isnan(preds[:, 0])`. But line 226 calls `oracle = evaluate(rets.copy(), rets)` without the warmup mask.

   What is wrong: with T return rows, the oracle receives T scored observations; the two causal predictors receive T−30. Their reported DSRs consequently differ in both information and scoring-window support. The oracle also trades the prefix during which the others do not trade.

   Consequence: the measured trading-value comparison is not on a common evaluation sample. The oracle’s perfect directional accuracy remains a theoretical property, but the reported DSR advantage is not isolated from sample-window differences. Actual T and historical numerical impact: N unverified.

2. CONFIRMED: The provenance source snapshot omits a directly executed evidence producer.

   Files: paper/src/make-provenance.py:3–4, 93–94, 108–116; paper/src/provenance_common.py:96–110; paper/src/make-throughput.py:169–178, 229–230.

   Claim: the manifest hashes “every producer/input source”. Its writer further says `source_snapshot_sha256 binds the candidate's actual (possibly not-yet-committed) source bytes`.

   Code: throughput executes `["node", "bench/throughput.js"]` with `cwd=REPO / "npm" / "sharpearena"` and incorporates the returned JSON under `"wasm": run_wasm()`. SOURCE_SCOPE includes Rust/TOML, Python, paper TeX, and selected root configuration, but no npm JavaScript producer or npm package manifests.

   What is wrong: npm/sharpearena/bench/throughput.js is an actual producer input but is outside the source digest. The recorded HEAD does not bind its potentially dirty working-tree bytes, which is precisely the use case claimed by the source snapshot.

   Consequence: the WASM throughput producer can change without changing source_snapshot_sha256. Hashing the resulting evidence binds output bytes, not the omitted implementation that produced them. This finding concerns the source-binding gap; I did not execute the validator or establish that historical throughput used modified JavaScript.

3. CONFIRMED: F6’s “committed vectors” gate compares two freshly generated results.

   File: paper/src/make-f6-adverse-selection.py:20–23, 86–100, 206, 305–311, 355–356.

   Claim: “the script asserts that the comparison's exogenous arm reproduces the committed per-episode vectors exactly”; preceding text claims the pre-existing keys are “byte-identical to the previous run”.

   Code: the gate reads `want = [row[str(h)] for row in committed[leg]]`. However, main constructs `per_episode` by calling the current `_episode_per_unit` implementation for each episode, then calls `endogenous_block(params, per_episode)`. The script writes the new JSON afterward; it does not load the previous committed vectors.

   What is wrong: the parameter named `committed` is a current-run result. The equality test establishes agreement between two current computation paths, not agreement with the previous artifact.

   Consequence: a generator or markout change affecting both paths can pass this gate while changing the historical baseline. The output nevertheless records `"exogenous_arm_matches_committed_vectors": True`. Actual historical drift was not checked.

4. CONFIRMED: The sealed-seed producer does not create a verifiable pre-run commitment record.

   File: paper/src/make-sealed-seeds.py:25–27, 154–164, 208–209, 225–226.

   Claim: “the salt commitment (SHA-256) is recorded before the run, the salt is revealed at the end”.

   Code: `commitment = hashlib.sha256(salt).hexdigest()` is computed before the attacks, but remains a local variable. Both `"salt_commitment_sha256": commitment` and `"salt_revealed_hex": salt.hex()` are first persisted together by the final `out.write_text(...)`, after public and sealed attacks.

   What is wrong: computation order inside one execution is not an externally verifiable commitment before observing outcomes. There is no separate pre-run persisted or published commitment artifact in this producer.

   Consequence: the final JSON demonstrates consistent hash/seed derivation, but cannot establish that this salt was irrevocably selected before the evaluation or rule out choosing among repeated runs. This is a missing audit guarantee, not evidence that selection occurred.

5. CONFIRMED: “Sealed evaluation replay” verification checks only the opening bar of a two-day calm environment.

   File: paper/src/make-sealed-seeds.py:48–50, 62–77, 166–174, 212, 220–222.

   Claim: “the revealed salt is shown to replay every sealed scenario”, and the output note says revealing it “replays the sealed evaluation exactly”.

   Code: replay_ok compares `first_closes(revealed[n])` against `first_closes(sealed_seeds[n])`. first_closes defaults to `n_days: int = 2`, constructs `distribution_mode="calm"`, and returns only each symbol’s opening close. The declared evaluation has `N_DAYS = 120` and `TIER = "hard"`.

   What is wrong: `"reveal_replay_verified"` counts matching opening-bar vectors, not matching complete deployed-tier scenarios. The separate equality assertion checks seed derivation and salt hashing, not later bars.

   Consequence: the verification would not detect divergence after the opening bar or in the hard-tier transformation. Deterministic replay may hold, but the reported verification does not test its claimed scope. Actual historical replay counts and full trajectories: N unverified.

6. CONFIRMED: The purported all-figure renderer leaves several F-series figures untouched.

   Files: paper/src/make-figures.py:2–7, 121–195, 242–245; paper/src/make-f4-realism.py:355; paper/src/make-f5-manipulation.py:681, 714, 766; paper/src/make-f6-adverse-selection.py:276.

   Claim: “Re-render every paper figure from the committed evidence JSON” and “reproduces the figures the make-f* scripts emit”.

   Code: main invokes only `(f1, f2, f3, f4, f5, f6, f7, f8)`. Its F4 renderer writes only f4-realism.pdf; F5 writes only f5-boundaries.pdf and f5-size-response.pdf; F6 writes only f6-markouts.pdf.

   Those producers additionally emit f4-calm-calibration.pdf, f5-concave.pdf, f5-positive-control.pdf, f5-extended-sweeps.pdf, and f6-endogenous.pdf. The central renderer has no corresponding regeneration paths. Witness and predictability figures are also outside its dispatch.

   Consequence: running this command can refresh the base plots while leaving previously generated extension plots stale or absent, without an omission notice for those plots. It cannot deliver the complete committed-evidence figure rebuild its own documentation promises. Whether any committed PDF is currently stale was not checked.

## Category checks

Claims vs. code: findings 2–6 establish source-level overclaims concerning source binding, historical equality, commitment/replay verification, and complete figure regeneration. Finding 1 establishes unequal support despite the declared warmup.

Sample: N unverified for actual historical executions, successful calls, every selection/filter, and final aggregate support because this pass inspected no run logs. Source-planned support includes 16 seeds per predictability tier with T versus T−30 scored rows; 24 paired F6 episodes; and 16 sealed slots checked only at opening. F4 qualifying-cell selection and witness attainable/monotone-crossing selection have data-dependent retained counts; those counts are unverified here. Source constants and loop lengths are not empirical N.

Merges: no Stata/dataframe merge was present in this producer slice. Reviewed dictionary/seed/horizon matching and positional aggregation. Finding 3 identifies the important provenance mismatch: newly computed vectors are supplied where historical committed vectors are claimed. Actual duplicate seed/period records and unmatched empirical records were not verified from logs.

Variables: traced producer-level simple returns, causal AR lags, sign-following policy returns, price-unit manipulation P&L, per-filled-unit markouts, witness strength/crossing summaries, and throughput units. Finding 1 is the new outcome-support defect. No additional hardcoded empirical bar/point value was established; theoretical reference lines and configured thresholds were not treated as fabricated measurements.

Silent failures: unequal NaN masking produces finding 1. F4’s nanmean aggregation can use differing finite support without reporting a per-fact valid count; occurrence is data-dependent and unverified, so it is not promoted to a separate confirmed historical failure. The incomplete renderer can preserve existing unregenerated PDFs. No additional force-coercion/chained-assignment defect was established in this slice.

Estimation: examined producer-level bootstrap/t-interval reductions, pairing, pointwise versus familywise intervals, score calls, and aggregation denominators. Finding 1 changes estimation support. F6’s stated intervals use per-episode observations and paired gaps; they do not repair its historical-reference gate. No fixed-effect estimator was present. Empirical estimation N and effective independent cluster counts remain unverified.

## Exact coverage and limitations

Read end to end all 14 authored paper/src/make-*.py files, totaling 4,230 lines:

make-f1-baselines.py; make-f2-regret.py; make-f3-generalization.py; make-f4-realism.py; make-f5-manipulation.py; make-f6-adverse-selection.py; make-f7-failures.py; make-f8-ecology.py; make-figures.py; make-predictability.py; make-provenance.py; make-sealed-seeds.py; make-throughput.py; make-witness.py.

Also read all 521 lines of paper/src/provenance_common.py, following the provenance producer’s import.

This pass was source-only. It did not execute tests, simulations, models, evidence producers, rendering, or provenance validation; inspect historical run logs or numerical artifacts; compare PDFs; or re-audit the full transitive library/native-engine implementation. F1/F2/F3/F8 and throughput producers were fully read as requested, but known findings from other audit scopes were not repeated. No authored make-*.py file remains unread.

The one thing I could not check without the historical data and execution logs is whether these confirmed defects actually changed the published numerical results, retained samples, or figure contents.

## Root scope note

Finding 2 concerns the explicit source snapshot and its current-tree validation. A recorded Git commit can still locate that commit's JavaScript. The gap is that the snapshot does not detect a subsequent change to this directly executed producer, and it does not cover uncommitted JavaScript bytes. The checker already rejects a newly generated manifest declaring a dirty tree; this finding does not claim that dirty-generation refusal is absent.
