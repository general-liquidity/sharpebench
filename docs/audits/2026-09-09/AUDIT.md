# Independent follow-up audit

Baseline: SharpeBench `5c4cfb2`, SharpeArena `4fdf672`. Line anchors below
refer to those trees, not to later repairs. Two independent reviewers examined
changed claims, sample construction, joins, variables, silent failures and
estimation. Eleven reported findings reduce to ten distinct defects because
both reviewers identified the settlement-number encoding mismatch. No finding
was dropped for lack of an anchor.

This is a focused review, not a certification that every changed file or every
product behavior is correct. Repair progress belongs in IMPLEMENTATION.md.

## Confirmed findings

1. **F01: failed statistical computation can declare every strategy significant.**
   Bench `crates/sharpebench-stats/src/significance.rs:495` computes
   `let t: Vec<f64> = means.iter().map(|m| sqrt_n * m).collect();`
   without checking computed finiteness. Finite inputs
   `[[1e308,1e308],[1e308,1e308]]` overflow; bootstrap NaNs are ignored by
   the maximum fold and every null is rejected.
   `crates/sharpebench-edge/src/verdict.rs:303` only floors step-down on
   its own error, despite a different family member reporting failure.
   The committed WASM returns `step_down:[true,true]` beside
   `snooping_error:"observed field maximum is not finite"`.
   Residual arithmetic, incomplete new error propagation.

2. **F02: resolved forecasts can be rewritten after sealing.**
   Arena `crates/sharpearena-py/python/sharpearena/prospective_field.py:1315`
   calls `_bind_frozen_contracts(document, frozen, agent_id)` and settlement
   agreement checks without joining resolved revisions to the sealed pending
   revisions. This contradicts the independent-consumer verification claim at
   line 1267. Changing one agent's predictions to realized outcomes and updating
   only the resolution manifest makes its Brier loss fall from
   0.2598840959626018 to zero while verification accepts unchanged sealed files.

3. **F03: keyed scoring discards the execution-seed count.**
   Bench `crates/sharpebench-cli/src/main.rs:1854` extracts
   `(field.submissions, field.declarations)` but drops `field.seeds`.
   The default width remains one. Eight seeds with 60 observations in one
   window count as 480 rather than 60 effective observations unless the caller
   repeats the geometry as a CLI flag. A contradictory explicit flag is accepted.
   This conflicts with `composite.rs:510`: replicates are not additional
   independent market-time observations.

4. **F04: missing CSV returns erase the date axis.**
   Bench `crates/sharpebench-cli/src/import_cmd.rs:387` uses
   `periods: if periods.len() == returns.len() { periods.clone() } else { Vec::new() }`
   after dropping empty cells at line 359. The identity checker skips an empty
   period axis at `crates/sharpebench-core/src/run_identity.rs:359`.
   Two agents missing different dates can therefore be paired by position.
   Whether any real imported field was affected is unverified.

5. **F05: official baseline intervals accept no resampling or one seed.**
   Arena `crates/sharpearena-py/python/sharpearena/baselines.py:571`
   attaches an official interval when
   `composite and arena_ci["point"] == row["deflated_sharpe"]`.
   Point equality does not establish inferential support.
   `crates/sharpearena/src/leaderboard_ci.rs:343` returns a point interval
   for zero resamples; one independent seed also produces a point interval.
   The paired diagnostic at line 415 reports `p_value: 0.0` for a nonzero
   difference with zero resamples. The underlying degeneracy predates the
   repair; attaching it as the official interval is new.

6. **F06: baseline ranking bypasses typed score unavailability.**
   Arena `crates/sharpearena-py/python/sharpearena/baselines.py:548`
   publishes `float(composite.get("deflated_sharpe", 0.0))`.
   Error handling at lines 561-569 suppresses only the interval. With
   `confidence=False`, no error reason survives. The Markdown board sorts
   and prints that numeric fallback at lines 594 and 606. A failed score is
   presented as measured no-skill performance despite the new helper elsewhere.

7. **F07: selection can choose a candidate without a usable estimate.**
   Bench `crates/sharpebench-stats/src/selection.rs:229` validates only
   `probability(alpha, ...).and_then(...block_probability...)`.
   An empty series falls back to point utility at line 247 and the report
   carries `input_error: None` at line 295. Committed WASM selects the
   overflowing candidate from `[[0.01,0.02],[1e308,1e308]]`, with null
   utilities and no error; it selects the empty candidate from
   `[[-0.01,-0.02],[]]` as utility zero. Missing observations and failed
   computed utilities must not win selection.

8. **F08: the prospective importer rejects the supported v2 envelope.**
   Bench `paper/src/import-prospective-field.py:128` requires
   `document.get("schema_version") != "sharpe.forecast-evidence.v1"`
   to be false, while Arena's current producer writes v2.
   A valid v1 document imports; its valid v2 counterpart fails with
   `wrong envelope: phi-4`. The envelope migration missed this consumer.

9. **F09: equivalent numeric settlements look disputed.**
   Bench `crates/sharpebench-core/src/forecast.rs:819` calls
   `legacy_canonical_json(outcome, &mut preimage)?`, although the comment
   at line 810 claims the contract's canonical encoding. Scoring converts the
   same outcome through `as_f64` at line 825. Thus `1` and `1.0`, or
   `0.0` and `-0.0`, score identically but hash differently; comparison at
   line 1538 refuses them as `unequal realized outcomes`. Both reviewers
   independently found this canonical-migration interaction.

10. **F10: statistical rejection has no disqualification reason.**
    Bench `crates/sharpebench-core/src/disqualification.rs:106` enumerates
    `Hard eligibility gates` without inspecting `deflation_error`,
    `bootstrap_error` or `selection_error`. A strong valid series with
    `dsr_ci_level:1.5` becomes ineligible through the new statistical gate
    but the committed WASM classifier emits `rank_eligible:false,reasons:[]`.
    Rollups cannot explain or count this rejection.

## Integration findings after the independent reports

11. **F11: npm discarded typed statistical unavailability.**
    In the baseline npm/src/index.ts, toHonestyVerdict omitted statistics_error,
    and the full verdict mapper omitted snooping_error and pbo_error. Type
    declarations advertised numeric values where the kernel serializes null.
    A rebuilt WASM module alone therefore did not repair the public consumer.
    The wrapper now preserves error fields, nullable diagnostics and the HLZ
    result. The new installed-tarball regression fails with the old mapper.

12. **F12: the committed WASM did not match the package's numerical version.**
    Bench package metadata at v0.19.0 accompanied a committed WASM module that
    reported sharpebench-stats/0.18.4. It still selected an empty candidate and
    emitted unexplained statistical disqualifications. Building current Rust
    changed those observable behaviors. The tarball check now asserts the
    methodology version against installed package metadata and exercises
    repaired refusal paths. A version string alone is not binary provenance.

## Reviewer coverage and limits

The following coverage statements are reproduced verbatim.

Claims: Found the settlement-verifier overclaim, discarded replicate semantics, missed v2 importer propagation, and legacy outcome hashing; inspected certification’s documented additive predicate and frozen tutorial/witness claims without identifying another claim/code discrepancy there.

Sample: Found replicate sample inflation and undocumented loss of CSV period identities. Frozen witness records: 156 unique cells → 26 witness rows, with 13 per geometry; pooled N is 462 weekly and 2,454 daily. Supported tutorial: 14/13 revisions → 12 effective resolved contracts per agent → 12 common contracts → 6 blocks. Withheld tutorial: 10/9 revisions → 8 effective resolved contracts per agent → 8 common contracts → 2 blocks. Other repaired producers’ new execution Ns are unverified.

Merges: Found the missing resolved-to-sealed identity/revision join, discarded period keys, and false outcome mismatches from numeric formatting; the inspected forecast comparison reports its common-support exclusions, and the frozen witness cell keys are unique.

Limits: This was a focused audit of changed claims/sample/merge paths, not a claim that every changed file received a complete read. Both worktrees stayed clean. The Linux Bench executable was stale, so its numerical diagnostics were excluded from current-tree evidence. Without the original imported CSVs, I could not establish whether the demonstrated period-loss defect affected any real submitted field.

Variables: found outcome representation inconsistency; no additional confirmed unit or period-alignment defect in the reviewed revised role construction.

Silent failures: found unavailable-score ranking, invalid-candidate selection, and missing disqualification reasons.

Estimation: found unsupported confidence publication and significance surviving a numerical failure.

Verification limits: no files changed, builds, fresh experiments, or full-suite runs. Synthetic execution used the committed WASM artifact, which stamps stats `0.18.4`; corresponding HEAD source paths were checked. The existing Arena Python extension refuses import because its spec hash differs from the wrapper, so I did not bypass it. Without runnable matching artifacts and the underlying evaluation data, I could not determine which published baseline results were affected or check empirical CI coverage.
