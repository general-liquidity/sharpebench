# G05 review repairs

Five findings from the independent read-only review of `5c4cfb2..855afbc`. Each
row records the reproduction against current source, the fix that was chosen and
why the alternative was rejected, and the isolated mutation that proves the
regression bites. Nothing here changes a published number: goldens, examples and
`paper/evidence/` are untouched.

## F-A: `SelectionUnavailable` was published as a hard disqualification

**Reproduction.** `score_agent` gates `rank_eligible` on `bootstrap_error` and
`deflation_error` and deliberately not on `selection_error`
(`crates/sharpebench-core/src/composite.rs`, the selection block: a candidate set
that cannot be deflated reports no selection diagnostic, and both selection
fields are already optional). A submission with a normal eligible 60-observation
track whose `candidates` hold one refusing series, `[[1e308, 1e308, 1e308]]`,
therefore scores `rank_eligible == true` with `selection_error` set. The
classifier pushed `FailReason::SelectionUnavailable` for it, the CLI's
`is_advisory` listed only `HighSelectionGap | IsRediscovery | OosDecay`, and
`disqualify` printed the reason unmarked under a header that promises the hard
gates mirror the scorer. `sharpebench disqualify --json` returned
`{"rank_eligible": true, "reasons": ["selection_unavailable"]}`.

**Fix chosen: make the reason advisory.** The deciding argument is consistency
with the scorer, and the scorer does not gate on the selection axis at all: it
reports `selection_gap` and never consults it in `rank_eligible`, so the
unavailability of the same diagnostic cannot demote an agent either.
`DeflationUnavailable` and `BootstrapUnavailable` are correctly hard because
`deflation_error` and `bootstrap_error` really are conjuncts of `rank_eligible`.
Gating `rank_eligible` on `selection_error` was rejected for two reasons: it
would change scoring semantics from a module documented as pure legibility that
computes nothing new, and it would demote agents on the failure of a diagnostic
the benchmark has always described as reported, never gating.

`FailReason::is_advisory` now lives in the core taxonomy next to the enum it
classifies, so the CLI cannot drift from it again; `analysis_cmd.rs` calls it
instead of keeping a private copy. The `FailReason` doc, the `rollup` doc and
`npm/src/types.ts` were regrouped to five gates, two hard unavailability reasons
and four advisory flags. Reason ordering is unchanged.

**Regression.** The existing
`statistical_unavailability_has_named_disqualification_reasons` passed with the
mislabel present because it asserted `rank_eligible` and the rollup count
separately and never the verdict connecting them. It now asserts, for each of the
three unavailability reasons, that its advisory label matches the scorer's own
gate chain. A new test,
`a_refusing_candidate_set_does_not_disqualify_a_ranked_agent`, drives the
reviewer's exact reproduction through `score_agent` and asserts
`rank_eligible == reasons.iter().all(FailReason::is_advisory)`. A CLI test,
`a_refusing_candidate_set_is_marked_advisory_next_to_an_eligible_verdict`,
exercises the real binary and requires `SelectionUnavailable (advisory)` next to
`rank-eligible=true`.

**Mutation.** Removing `Self::SelectionUnavailable` from `is_advisory` in place:
`a_refusing_candidate_set_does_not_disqualify_a_ranked_agent` and
`statistical_unavailability_has_named_disqualification_reasons` fail, and the
CLI test fails with the printed board attached. Restored and `cmp`-verified.

## F-B: the checkpoint schema was not bumped for the per-round spend

**Reproduction.** `attempts_in_round` and `runtime_recovery_rounds` landed as
`#[serde(default)]` fields carrying a spend invariant while
`SweepContract::SCHEMA_VERSION` stayed at 3, which baseline v0.19.0 also wrote. A
v0.19.0 checkpoint with a task left `Claimed` mid-retry has no `attempts_in_round`
key, deserializes at 0, is requeued, and is granted a fresh `max_retries + 1`
attempts on top of whatever the writing binary already spent in that round. The
load-time pre-check only rejects `attempts_in_round > contract.max_retries`,
which a missing field never triggers.

**Fix chosen: bump to schema 4.** The schema-3 doc's own reasoning applies one
level down without modification: a checkpoint that predates the field carries no
per-round spend evidence, and resuming into it reports what it already spent as
zero. It is refused rather than read that way. The alternative, making the field
non-defaulting for `Claimed` tasks, was rejected because it only covers the
claimed case: a `Pending` task saved after a partial round has the same missing
key and the same zero, and a per-state serde exception is a larger and less
surgical change to a file another agent is editing concurrently. The bound resume
path now refuses a written contract whose `schema_version` differs from this
binary's with an explicit message, ahead of the `matches_bound` arm, so the
refusal names the schema instead of reading as a different experiment.

**Regression.**
`bound_resume_refuses_a_checkpoint_written_before_the_per_round_budget` writes a
literal schema-3 checkpoint JSON with a claimed task and no `attempts_in_round`
key, asserts that the missing key really does deserialize to an unspent round
(which is why the version and not the budget pre-check has to refuse it), and
requires `run_resumable_sweep_bound` to fail with `InvalidData`.

**Mutation.** Reverting `SCHEMA_VERSION` to 3 and deleting the schema arm in
place: the test fails on `a pre-budget checkpoint must not grant a fresh round`,
which is the defect itself. Restored and `cmp`-verified.

## F-C: `alpha_warning` was hard-coded true on every refusal

**Reproduction.** `percentile_selection(&[vec![0.01]], Utility::MeanReturn, 0.5,
1, 100, 0.1)` returned `InsufficientObservations` together with
`alpha_warning = true`, although alpha 0.5 is well above
`MIN_RECOMMENDED_SELECTION_ALPHA = 0.3`. The refusal arm was written when an
invalid alpha was the only way to reach it; F07 routed every later refusal
through the same arm. `npm/src/types.ts` exposes both fields.

**Fix.** The refusal arm computes `alpha < MIN_RECOMMENDED_SELECTION_ALPHA` like
the accepted path, so the flag reports where alpha sits and nothing else. NaN and
values above the floor leave it false; a negative alpha is below the floor and
still raises it, which is what the flag says it means. The field doc and the npm
doc were corrected; the CLI warning text was already conditioned on the flag and
is printed after the refusal path returns, so it is unaffected.

**Regression.** `a_refusal_that_is_not_about_alpha_does_not_blame_alpha` asserts
`alpha_warning == false` for the reviewer's insufficient-observations case and
for a `block_prob = 0.0` refusal. The existing
`a_non_percentile_alpha_is_refused_rather_than_coerced` now asserts the flag
equals `bad < MIN_RECOMMENDED_SELECTION_ALPHA` per case instead of asserting it
unconditionally true.

**Mutation.** Restoring `alpha_warning: true` in place: both tests fail, the new
one on `alpha 0.5 is above the recommended floor and was not the refused
argument`. Restored and `cmp`-verified.

## F-D: `finite_computation` labels misnamed the quantity

**Reproduction.** `reality_check_pvalue`, `spa_pvalue`, `spa_consistent_pvalue`
and `step_down_significant` each validate every agent's mean before any maximum
is taken, but passed `quantity: "observed field maximum"`. A row-mean overflow
therefore rendered as "observed field maximum is not finite" through
`validation.rs`, and that exact string travels to npm as `snooping_error`.
`step_down_significant`'s second stage was already right ("observed step-down
statistic"), as were the genuine maxima at the two `"studentized field maximum"`
sites and the post-max check in `reality_check_pvalue`.

**Fix.** The four per-agent mean validations now say `"observed agent mean"`. No
other label changed, so the sites that really do validate a maximum keep theirs.

**Regression.** `a_non_finite_statistic_is_reported_as_such` now pins the string
for all four entry points and pins the rendered message
`"observed agent mean is not finite"`, rather than only the first function.

**Mutation.** Restoring `"observed field maximum"` at the four sites in place:
the test fails with the left/right label diff. Restored and `cmp`-verified.

Note for a later rebuild: the committed `npm/pkg/sharpebench_bg.wasm` still
carries the old string until the WASM artifact is rebuilt, and
`docs/audits/2026-09-09/AUDIT.md` quotes it as the historical F01 finding, which
is left as written history.

## F-E: the tautological contract legs in the observed resume path

**Reproduction.** `run_resumable_sweep_observed` called
`contract.matches_execution(windows, &contract.seeds, contract.max_retries)`,
feeding the contract's own fields into a comparison against themselves, so the
seed and retry legs were vacuously true and only schema, digest shape and
`windows` were really checked. That path takes seeds and the retry budget from
the contract by design, so there is nothing else it could compare them to. No
live defect: `run_resumable_sweep_bound` still performs the real comparison
against caller-supplied values before delegating.

**Choice: delete the dead legs.** `matches_windows` is the new private check for
what is genuinely checkable without caller-supplied execution parameters, and
`matches_execution` is now that check plus the two legs, kept for the caller that
supplies them. The observed path calls `matches_windows` and its error message no
longer claims to have validated an execution policy. Restoring a real check was
not available: there is no independent seed or retry value at that call site to
check against.

**Regression.**
`bound_resume_rejects_seeds_and_retries_the_contract_did_not_declare` pins the
real comparison where it lives, refusing a caller that supplies other seeds or
another retry budget than the contract declares, and pins that the windows leg
still bites in the observed path through
`run_resumable_sweep_bound_with_policy`.

**Mutation.** Disabling the `matches_windows` guard in
`run_resumable_sweep_observed` in place: the test fails on `the window matrix
must still be compared`, so the retained leg is not itself dead. Restored and
`cmp`-verified.

## Checks

| Command | Exit |
|---|---|
| `cargo fmt --all --check` | 0 |
| `cargo clippy --all-targets --all-features -- -D warnings` | 0 |
| `RUSTDOCFLAGS=-Dwarnings cargo doc --workspace --exclude xtask --no-deps` | 0 |
| `cargo nextest run -p sharpebench-core -p sharpebench-stats -p sharpebench-harness -p sharpebench` | 0 (653 passed, 2 skipped) |
| `npm test` in `npm/` | 0 (20 passed) |

A local pass is necessary, not sufficient: the packaged consumers, the OS
matrices and the mutation workflow still run in CI.
