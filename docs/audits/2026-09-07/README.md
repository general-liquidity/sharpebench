# Sharpe suite audit checkpoint: 7 September 2026

The complete suite checklist and supporting audit evidence are saved here in both
repositories. Repairs are in progress and unfinished: **100 checklist rows, with
47 closed and 53 open**, plus five explicitly deferred items. The eight rows
closed on 2026-09-08 are Batch A's R11, R13, R14, the shard-order and
baseline-scope claims and the historical-impact caveat, plus Batch B's publish
gate and updater claim; that work sits on the `fix/audit-batch-a-b-2026-09-08`
branch in each repository and is not merged. The plan was
restructured on 2026-09-08 from nine descriptive sections into nine ordered
batches; the previous checkpoint recorded 93 rows with 36 closed. The difference
is three grab-bag rows split into single items, one updater row added, and two
post-checkpoint fixes recorded as closed. An open row can contain a partial
repair, a design decision or an unconfirmed probe, so these totals are not a
count of independent confirmed bugs.

Start with [IMPLEMENTATION.md](IMPLEMENTATION.md) for the checklist, the batch
order, the 2026-09-08 review corrections and the closed rows with their repair
commits. The chronological repair diary lives beside it in
[VERIFICATION-LOG.md](VERIFICATION-LOG.md). Every commit cited on a closed row
was verified on 2026-09-08 to be an ancestor of `origin/main` in the named
repository. No additional repair or experiment is claimed by saving these
documents.

## Open rows by batch

Batches run in order. A and B change what the shipped products claim.

| Batch | Rows | Scope |
| --- | ---: | --- |
| [A. Paper pass](IMPLEMENTATION.md#batch-a-paper-pass-both-products) | 9 | Historical-impact caveat, R11, R13, R14, shard-order and scope claims, rebuilt papers |
| [B. Publication gating](IMPLEMENTATION.md#batch-b-publication-gating) | 4 | Publish graph depends on green CI, narrowed updater claim, package consumers, conformance fixtures |
| [C. Run identity](IMPLEMENTATION.md#batch-c-run-identity-bi3-one-batch) | 4 | BI3 keyed run/window/seed identity, complete-grid validation, BR1 |
| [D. Producer rows touching claims](IMPLEMENTATION.md#batch-d-producer-rows-that-touch-existing-claims) | 4 | BP6, BP8, AP5, AP3 |
| [E. Shared mathematics and contracts](IMPLEMENTATION.md#batch-e-shared-mathematics-and-contracts) | 8 | R07/BM10, R06/AI1, R03 and R09 propagation, remaining R02, R05, R12, BR2/AR2 |
| [F. Remaining Bench diagnostics](IMPLEMENTATION.md#batch-f-remaining-bench-diagnostics) | 16 | BM1, BM2/BS6, BM3, BM7, BI6 and the split supplementary rows |
| [G. Remaining Arena telemetry](IMPLEMENTATION.md#batch-g-remaining-arena-telemetry) | 2 | AR1/AR3, AR4 |
| [H. Producers for the next field run](IMPLEMENTATION.md#batch-h-producer-rows-for-the-next-field-run) | 10 | BP1 to BP7, AP1, AP2, AP4, AP6 |
| [I. Bounded probes](IMPLEMENTATION.md#batch-i-bounded-probes-promote-or-delete) | 4 | HTTP deadline, token overflow, Docker ENTRYPOINT, child OOM |
| Total open | 53 | |

Closed rows are listed per product in
[IMPLEMENTATION.md](IMPLEMENTATION.md#closed-rows).

## Snapshot contents and update rule

The 13-file snapshot contains the full [implementation checklist](IMPLEMENTATION.md),
its [verification log](VERIFICATION-LOG.md), the original audit report below, [coverage and limits](coverage.md),
the [757-file baseline inventory](inventory.md), six independent reports
([Bench reviewer](bench-reviewer.md), [Bench interfaces](bench-interfaces.md),
[Bench producers](bench-producers.md), [Arena reviewer](arena-reviewer.md),
[Arena diagnostics](arena-diagnostics.md), [Arena producers](arena-producers.md)),
and both historical reproduction probes
([Python](reproduce_core_findings.py), [Rust](reproduce_dataset_boundaries.rs)).

The user requested a complete copy in each repository. Both
`docs/audits/2026-09-07/` directories intentionally contain byte-identical files.
For any later correction or checklist update, preserve the row coverage and IDs,
record the new evidence and date, and update both copies together. Keep historical
findings at their original baselines; record repair status in the checklist instead
of rewriting what the audit observed. The unmodified original archive is retained
separately by the coordinator. This published copy changes status framing, source
link portability and probe path setup, while preserving the original findings,
coverage, inventory and verification history.

## Historical source and probe scope

The original audit reviewed [sharpebench `933e0c1`](https://github.com/general-liquidity/sharpebench/tree/933e0c1056a2e4707b28762c294323bf05bdab65) and [sharpearena `1be915f`](https://github.com/general-liquidity/sharpearena/tree/1be915f330acabacd171cc350bec0def58d9e134). All findings and quoted source claims below and in the appendices refer to
those exact commits. "Current", "no files changed" and "repairs not performed"
inside the historical reports describe the audit at that time. Later repairs are
recorded in [IMPLEMENTATION.md](IMPLEMENTATION.md); the reports are not a list of
defects confirmed present on today's main branches. Product-source links are pinned
to the baseline commit and retain their original line anchors.

The probes characterize historical behavior and were not run while saving this
checkpoint. The Python probe requires Linux, two sibling baseline checkouts named
`sharpebench` and `sharpearena`, the baseline Bench CLI at
`sharpebench/target/debug/sharpebench`, and the unpublished Arena package archive at
`sharpearena/target/package/sharpearena-0.24.1.crate`. Its parser expects the baseline
`SPEC_FILES`/`SPEC_EPOCH` form and is not compatible with the repaired hash design.
Supply the directory containing those baseline checkouts explicitly:

```bash
python3 docs/audits/2026-09-07/reproduce_core_findings.py /path/to/baseline-workspace
```

This command assumes the required baseline build/archive already exist. It does
not switch branches or build them. The Rust probe must be linked against a fresh
`sharpebench-sim` build from the audited Bench commit. Its assertions deliberately
expect the old defects and should fail on repaired code. These files are archived
reproduction evidence; the repaired products' own tests are the current regression
suites.

## Original audit report (historical)

Date: 7 September 2026. Status: **audit complete within the stated coverage; repairs not performed**.

This is an audit, not a repair or release. Product source files have not been changed.
All runtime probes use synthetic inputs, existing fixtures, or ordinary test suites.
No model was installed or queried, no benchmark field was run, and no broker order was submitted.

## Scope and interpretation

| Product | Reviewed source | Workspace version | Tracked-file inventory |
| --- | --- | --- | --- |
| SharpeBench | `933e0c1056a2e4707b28762c294323bf05bdab65` | 0.18.4 | 377 files, including 97 Rust and 25 Python files |
| SharpeArena | `1be915f330acabacd171cc350bec0def58d9e134` | 0.24.1 | 380 files, including 21 Rust and 166 Python files |

Both starting worktrees were clean. Inventory coverage is not the same as line-by-line review. The [coverage ledger](coverage.md) separates source inspection, execution and empirical replication, and the [inventory](inventory.md) lists all 757 tracked files. The independent-review appendices state their exact reviewed files and limitations. No audit can establish the absence of every possible defect.

**CONFIRMED** means the contradiction is established by source or a reproduced example. It does not mean that a published run was affected. Executed reproductions are distinguished from source-only findings. **SUSPECTED** findings require additional evidence and must not be presented as established failures.

Priorities: P1 affects an important public correctness, integrity, or resource boundary; P2 affects an auxiliary result or a narrower boundary; P3 is documentation or a secondary usability issue. These are repair priorities, not claims that an exploit occurred.

## Immediate conclusion

The suite is not ready to be described as completely verified. Existing tests and CI pass, but accepted public inputs can remove a process disqualification, misidentify the effective environment, produce unjustified forecast significance, or verify a document without verifying its displayed scores. Several defects are at the connections between individually tested components.

The highest-value next work is to repair those connections, not import more features. The products remain complementary: Arena should produce validated environment and decision evidence, and Bench should independently validate and score it. The separation is useful only if the identities, outcomes, units, and retained samples agree across that boundary.

## Root-reviewed findings

### R01. P1: common-support filtering removes a process violation before eligibility is evaluated

**CONFIRMED, reproduced against a fresh build of the reviewed CLI.**

Sources: [composite.rs:1483](https://github.com/general-liquidity/sharpebench/blob/933e0c1056a2e4707b28762c294323bf05bdab65/crates/sharpebench-core/src/composite.rs#L1483), [composite.rs:1649](https://github.com/general-liquidity/sharpebench/blob/933e0c1056a2e4707b28762c294323bf05bdab65/crates/sharpebench-core/src/composite.rs#L1649), [composite.rs:1168](https://github.com/general-liquidity/sharpebench/blob/933e0c1056a2e4707b28762c294323bf05bdab65/crates/sharpebench-core/src/composite.rs#L1168).

`rank_declared` restricts submissions to shared run positions before `score_agent_with` inspects process traces. An entrant with one clean profitable run and one run containing `denylist_bypass` is correctly ineligible when scored alone. Adding a peer with only the first run changes that same entrant to `process_ok=true` and `rank_eligible=true`; `runs_submitted` remains 2 while `runs_scored` becomes 1.

This contradicts the paper's rule that one block-severity violation in **any** run disqualifies the entry. The demonstrated route is the public raw scoring API/CLI. A complete-grid harness can prevent that particular incomplete field, but the ranking function itself does not preserve the safety invariant.

Repair: validate required geometry at entry, retain original traces for all safety judgments, and restrict only comparison estimators to common support. Add a regression where a peer's incomplete support cannot erase another entrant's violation.

### R02. P1: invalid floating-point inputs can receive significant statistical results

**CONFIRMED, reproduced through the existing Python binding; Rust source independently inspected.**

Sources: [significance.rs:47](https://github.com/general-liquidity/sharpebench/blob/933e0c1056a2e4707b28762c294323bf05bdab65/crates/sharpebench-stats/src/significance.rs#L47), [fdr.rs:95](https://github.com/general-liquidity/sharpebench/blob/933e0c1056a2e4707b28762c294323bf05bdab65/crates/sharpebench-stats/src/fdr.rs#L95).

`bootstrap_pvalue([NaN] * 20, n_boot=1000)` returns approximately `0.000999001`. `benjamini_hochberg([NaN, 0.001], 0.05)` returns `[true, true]`. NaN comparisons skip the intended checks and rejection-prefix logic can include an invalid value.

This is a failure of the public statistical boundary, not proof that a NaN return clears every trading gate. JSON paths may reject non-finite values earlier, while Rust and Python numeric callers can supply them directly.

Repair: checked finite inputs, p-values in [0,1], valid alpha and bootstrap parameters, and a typed invalid/unavailable result. Do not coerce invalid data into a favorable score.

### R03. P2: standardized moments mix incompatible finite-sample normalizations

**CONFIRMED, independently calculated and reproduced.**

Sources: [Bench stats.rs:64](https://github.com/general-liquidity/sharpebench/blob/933e0c1056a2e4707b28762c294323bf05bdab65/crates/sharpebench-stats/src/stats.rs#L64), [stats.rs:79](https://github.com/general-liquidity/sharpebench/blob/933e0c1056a2e4707b28762c294323bf05bdab65/crates/sharpebench-stats/src/stats.rs#L79), [Arena leaderboard_ci.rs:64](https://github.com/general-liquidity/sharpearena/blob/1be915f330acabacd171cc350bec0def58d9e134/crates/sharpearena/src/leaderboard_ci.rs#L64).

The third/fourth moments divide their numerators by n but standardize with a variance using n-1. For `[1,2,3,4]`, the public non-excess kurtosis is `0.9225`; the empirical population-moment coefficient is `1.64`. The implemented skewness and kurtosis equal the empirical coefficients times `((n-1)/n)^(3/2)` and `((n-1)/n)^2`, respectively. They are neither the documented empirical moments nor the usual adjusted coefficients.

The discrepancy vanishes asymptotically but enters PSR/DSR denominators. A synthetic n=20 example crosses a 0.95 threshold solely because of the normalization choice. This does not establish that any frozen table's headline verdict changes.

Repair: choose and document a finite-sample estimator, use it consistently in both implementations, compare against an independent oracle, and quantify changes before regenerating historical evidence. NIST explicitly distinguishes the n-based moment denominator from n-1: [definitions](https://www.itl.nist.gov/div898/handbook/eda/section3/eda35b.htm). The original [DSR paper](https://www.davidhbailey.com/dhbpapers/deflated-sharpe.pdf) also makes distributional and trial-population assumptions; fixing arithmetic does not remove those assumptions.

### R04. P1: Cargo packaging changes Arena's compatibility hash without changing engine semantics

**CONFIRMED using a freshly created, unpublished Cargo package.**

Source: [build.rs:34](https://github.com/general-liquidity/sharpearena/blob/1be915f330acabacd171cc350bec0def58d9e134/crates/sharpearena/build.rs#L34).

`SPEC_FILES` hashes the literal `Cargo.toml` bytes. Cargo normalizes that manifest when producing the registry archive. Recomputing the documented FNV framing over the current source gives `3b02387e8a4b0410`; over the newly packaged 0.24.1 crate it gives `db3e92f460f6e6bc`. Substituting only the archive's `Cargo.toml.orig` restores `3b02387e8a4b0410`.

The same release therefore has a different compatibility identity as a packaged Rust dependency than in its source-built wrapper pins. No wheel installation failure is claimed here: source-built wheels can remain internally consistent. The demonstrated defect is cross-distribution identity.

Repair: fingerprint a stable, semantic dependency/version representation instead of Cargo's editable manifest encoding, and check hash identity after packaging. This should be a package-consumer test, not another source-tree-only golden.

### R05. P1: one settlement block produces a highly significant forecast comparison

**CONFIRMED, reproduced against the current CLI.**

Source: [forecast.rs:1243](https://github.com/general-liquidity/sharpebench/blob/933e0c1056a2e4707b28762c294323bf05bdab65/crates/sharpebench-core/src/forecast.rs#L1243).

Two valid one-contract documents, probabilities 0.9 and 0.6 against outcome 1, yield one independent settlement block, a zero-width interval at -0.15, and `familywise_significant=true` with p=`1/2001`. Increasing the bootstrap replication count makes this unsupported result look more significant; it does not create independent evidence.

Repair: represent insufficient support explicitly. Specify a defensible inferential protocol, validate its independent-block requirements, and withhold significance when those requirements do not hold. A minimum of two blocks alone is not a guarantee of calibrated inference.

### R06. P1: two agents can resolve the same forecast contract to different outcomes

**CONFIRMED, reproduced against the current CLI.**

Source: [forecast.rs:1215](https://github.com/general-liquidity/sharpebench/blob/933e0c1056a2e4707b28762c294323bf05bdab65/crates/sharpebench-core/src/forecast.rs#L1215).

The paired comparison checks contract identity, resolution time, instrument, and rule, but not agreement on the realized outcome. Changing one document's outcome from 1 to 0 under the identical contract digest is accepted and changes the reported comparison. Contract equality is not outcome equality.

Repair: score against one canonical settlement record per contract, or require exact agreement on independently validated settlement objects before comparing agents. Bind and retain the outcome identity rather than discarding it after computing loss.

### R07. P2: Python and Rust disagree about canonical contract numbers

**CONFIRMED, reproduced against the current CLI.**

Source: [forecast.rs:597](https://github.com/general-liquidity/sharpebench/blob/933e0c1056a2e4707b28762c294323bf05bdab65/crates/sharpebench-core/src/forecast.rs#L597).

A valid contract containing `neutral_threshold=1e-5`, hashed using the producer's Python canonical JSON convention, is rejected by Bench as an unknown contract digest. Python emits `1e-05`; Rust's serializer chooses `0.00001`. Padding exponent digits does not fix the fixed-versus-exponential formatting difference.

Repair: adopt one specified cross-language numeric canonicalization, with fixtures at exponent boundaries, signed zero, integer-looking floats, and Unicode boundaries. Keep format versioning explicit.

### R08. P1: aggregate forecast comparisons mix dimensionless and dimensional losses

**CONFIRMED, reproduced against the current CLI.**

Sources: [forecast.rs:1021](https://github.com/general-liquidity/sharpebench/blob/933e0c1056a2e4707b28762c294323bf05bdab65/crates/sharpebench-core/src/forecast.rs#L1021), [forecast.rs:1223](https://github.com/general-liquidity/sharpebench/blob/933e0c1056a2e4707b28762c294323bf05bdab65/crates/sharpebench-core/src/forecast.rs#L1223).

The pairwise comparison averages raw loss differences across all common contracts. A Brier loss is dimensionless; squared price error has currency-squared units. With one binary contract and one point-price contract, an otherwise identical change from dollars to cents changes agent A's mean loss difference from `-0.385` to `149.6`, reversing which agent appears better. Per-agent grouping by scoring rule does not repair the pooled comparison or distinguish currencies within a rule.

Repair: compare homogeneous, declared rule/target/unit strata, or specify a precommitted dimensionless normalization/utility and weights. Preserve raw unit-labelled diagnostics. This surface is forecast-report-only; it does not change trading rank directly.

### R09. P2: a zero target can cross through flat into a short position

**CONFIRMED, reproduced through Arena's current native engine.**

Source: [engine.rs](https://github.com/general-liquidity/sharpebench/blob/933e0c1056a2e4707b28762c294323bf05bdab65/crates/sharpebench-sim/src/engine.rs), target-value to share-quantity conversion in `execute_decision`.

The target delta is calculated at the mid and then divided by a worse sell execution price. With a constant-price synthetic panel, buying weight 0.5 and then requesting target 0 leaves negative shares (`-8.740324423781119e-5` in the probe), instead of flattening. The source acknowledges small execution-price overshoots, but these are consequential for strict long-only, flat-position, and cash/financing invariants. Arena's pinned simulator dependency exhibits the same behavior.

Repair: derive intended target quantity consistently, distinguish execution cost from target inventory, and test flat/long-only invariants with costs enabled. If exact target weights are intentionally not achieved, expose realized fills and bounds rather than describing the order as exact flattening.

### R10. P2: the robust-impact optimizer can return a point outside its uncertainty set

**CONFIRMED, reproduced using the current public Rust API.**

Source: [market.rs:285](https://github.com/general-liquidity/sharpearena/blob/1be915f330acabacd171cc350bec0def58d9e134/crates/sharpearena/src/market.rs#L285).

The optimizer computes an unconstrained ellipse support point and independently floors negative impact coefficients to zero. With nominal `(lambda, eta)=(0.1,0.05)`, radii `(0.2,0.1)`, correlation `-0.9`, and objective coefficients `(0,2)`, its output `(0,0.15)` has normalized ellipse quadratic form approximately `1.8421`, exceeding the boundary value 1.

Repair: solve over the intersection of the ellipse and nonnegative domain, or constrain/document a parameter domain in which post-processing cannot leave the set. This is an opt-in robustness feature, not a demonstrated defect in the historical canonical experiments.

### R11. P2: the paper describes the LOB as a single-price batch mechanism without queue priority

**CONFIRMED by implementation and manuscript.**

Sources: [03-environment.tex:106](https://github.com/general-liquidity/sharpearena/blob/1be915f330acabacd171cc350bec0def58d9e134/paper/sections/03-environment.tex#L106), [lob_market.rs:194](https://github.com/general-liquidity/sharpearena/blob/1be915f330acabacd171cc350bec0def58d9e134/crates/sharpearena/src/lob_market.rs#L194), [lob_market.rs:279](https://github.com/general-liquidity/sharpearena/blob/1be915f330acabacd171cc350bec0def58d9e134/crates/sharpearena/src/lob_market.rs#L279).

The paper says both market models clear at one price and queue-position value does not exist. The LOB executes against multiple price levels using FIFO priority, and modifying order size can preserve or lose queue priority. A separate call-auction query does not make ordinary matching uniform-price clearing.

Repair the mechanism description, not the matching code merely to fit the prose. Distinguish discrete time, sequential submission order, continuous-style price-time matching, and the separate auction query. The endogenous impact model also needs its own execution-price description.

### R12. P3: forward compatibility is overstated for closed schemas

**CONFIRMED as a contract-design contradiction, not a newly broken released field.**

Source: [04-contract.tex:11](https://github.com/general-liquidity/sharpearena/blob/1be915f330acabacd171cc350bec0def58d9e134/paper/sections/04-contract.tex#L11), plus the published `additionalProperties: false` schemas and strict deserializers.

Making a new field optional lets the new reader accept an old message. It does not make an old closed reader accept a new message containing that field. The paper's bidirectional additive-compatibility guarantee does not follow from optional/default alone.

Repair: state message-direction compatibility precisely. Choose version negotiation, parallel namespaces, or a deliberately open extension envelope before adding fields under the same protocol promise.

### R13. P2: the claimed per-run drawdown bound is false after execution-seed averaging

**CONFIRMED, reproduced against the current CLI.**

Sources: [05-experiments.tex:101](https://github.com/general-liquidity/sharpebench/blob/933e0c1056a2e4707b28762c294323bf05bdab65/paper/sections/05-experiments.tex#L101), [composite.rs:246](https://github.com/general-liquidity/sharpebench/blob/933e0c1056a2e4707b28762c294323bf05bdab65/crates/sharpebench-core/src/composite.rs#L246), [composite.rs:786](https://github.com/general-liquidity/sharpebench/blob/933e0c1056a2e4707b28762c294323bf05bdab65/crates/sharpebench-core/src/composite.rs#L786), and the seed-averaging implementation in `pooled_returns`.

The paper says a run's drawdown "never exceeds the pooled track's" and concludes equal per-run and pooled caps are redundant. That argument applies to concatenation, not to the actual within-window averaging of execution seeds. Two aligned runs `[0.1, -0.2]` and `[-0.1, 0.2]`, scored with two execution seeds per window, produce pooled drawdown 0 and worst-run drawdown 0.20. The executable implementation retains both quantities and correctly applies separate caps; the error is the asserted inequality and the resulting interpretation of the per-run gate.

Repair the paper and API comments, retain both constraints, and add the anti-correlated-seed example as a regression. Do not remove the per-run gate because the current prose calls it redundant.

### R14. P3: the single-tier compute fraction has the wrong denominator

**CONFIRMED by arithmetic and manuscript.**

Source: [05-protocol.tex:66](https://github.com/general-liquidity/sharpearena/blob/1be915f330acabacd171cc350bec0def58d9e134/paper/sections/05-protocol.tex#L66).

The reported field is six policies times three tiers times 256 seeds, or 4,608 episodes. A single agent on one tier is 256 episodes, one eighteenth of that count, not "one sixth". One agent on all three tiers is one sixth. Episode counts do not imply exact wall-time scaling across policies, so the correction should state the count ratio rather than promise a particular runtime.

### R15. P2: evidence-shard assembly checks row count, not complete grid identity

**CONFIRMED, reproduced with in-memory file doubles; no evidence file was changed.**

Sources: [assemble_sweep.py:21](https://github.com/general-liquidity/sharpebench/blob/933e0c1056a2e4707b28762c294323bf05bdab65/paper/evidence/assemble_sweep.py#L21), [analyze.py:56](https://github.com/general-liquidity/sharpebench/blob/933e0c1056a2e4707b28762c294323bf05bdab65/paper/evidence/analyze.py#L56), and the canonical-grid assembly claim in the paper's command appendix.

The assembler checks `len(part_lines) != 128`, one DSR bar per shard, and one matching dataset. It does not validate the remaining configuration/entrant keys or their uniqueness. Four shards containing 128 copies of one row each are accepted as 512 records, although there are only four distinct cells. The reducer also derives its `complete` label from `len(recs) == 512`.

Repair: derive the expected Cartesian grid from a declared schema, require every key exactly once, and reject missing, duplicate, unknown, or inconsistent records. This reproduction establishes a validator gap, not that the existing frozen sweep contains duplicate cells. Independently checking all nine committed principal sweeps found 512 rows and 512 unique `(dsr_bar, n_trials, sr_std_pinned, agent_id)` keys in each, with no missing cells in the Cartesian product of the observed axes.

### R16. P2: promoted gold cases check frozen bad output instead of rerunning the component being repaired

**CONFIRMED by the complete promotion implementation and its test.**

Sources: [trace_promotion.py:828](https://github.com/general-liquidity/sharpearena/blob/1be915f330acabacd171cc350bec0def58d9e134/crates/sharpearena-py/python/sharpearena/trace_promotion.py#L828), [test_trace_promotion.py:343](https://github.com/general-liquidity/sharpearena/blob/1be915f330acabacd171cc350bec0def58d9e134/crates/sharpearena-py/tests/test_trace_promotion.py#L343).

`evaluate_gold_case` rebuilds `StrictTrace` from the case's stored `steps` and applies `_run_named_check` directly. It does not reconstruct an environment or invoke the producer that created the faulty observation. Consequently, repairing that producer cannot make an unchanged leaking case pass. The test named `test_a_gold_case_fails_while_the_defect_is_present_and_passes_once_fixed` obtains its pass by deleting `label` from the stored observation and constructing a different scenario under the same case identity.

This is a useful stored-trace detector, but not the advertised executable regression of the producer. Repair by freezing minimal inputs and decisions, rerunning the named component, then checking its newly produced observations. If the intended scope remains detection-only, rename the contract and test so they do not imply a producer fix is verified. Also validate case content against its identity when loading or changing a case.

### R17. P2: the confidence-table renderer can name the wrong winner and confidence level

**CONFIRMED, reproduced by executing the unchanged renderer with a synthetic comparison.**

Source: [confidence.py:137](https://github.com/general-liquidity/sharpearena/blob/1be915f330acabacd171cc350bec0def58d9e134/crates/sharpearena-py/python/sharpearena/confidence.py#L137).

`significance_markdown` ignores `verdict` and writes `A > B beyond seed noise` whenever `significant` is true. A valid-shaped result with `verdict="b_better"`, difference -0.2 and interval [-0.3,-0.1] is rendered as A beating B. The heading is hardcoded to 95% even when the comparison carries `confidence=0.9`; the producer exposes configurable alpha. The adjacent-rank helper usually puts a larger displayed point estimate first, but the public renderer must not reverse the result it receives, and the fixed confidence heading is wrong even on its ordinary configurable path.

Repair: render the actual direction and confidence, reject inconsistent comparison objects, and say “difference not established” instead of interpreting failure to reject as proof of equivalence. `leaderboard_markdown` has the same fixed-95%-heading issue.

### R18. P2: effective-configuration verification is optional and cached by seed rather than performed per environment

**CONFIRMED by source and an in-memory control-flow reproduction of the unchanged function.**

Sources: [baselines.py:502](https://github.com/general-liquidity/sharpearena/blob/1be915f330acabacd171cc350bec0def58d9e134/crates/sharpearena-py/python/sharpearena/baselines.py#L502), [baselines.py:520](https://github.com/general-liquidity/sharpearena/blob/1be915f330acabacd171cc350bec0def58d9e134/crates/sharpearena-py/python/sharpearena/baselines.py#L520).

`run_baselines` says each constructed environment is verified before scoring, but calls the checker only under `readback is not None and s not in readback`. Six policies on one seed construct six environments and verify only the first. With the default `readback=None`, none is verified. Reusing a populated collector across calls also suppresses new validation. A synthetic fixture replacing construction and scoring dependencies counted 6 constructions and 1 validation with an empty collector, and 6 constructions and 0 validations with the default. No model or benchmark field was run.

The current factory normally builds the same settings for a given seed; this is not proof that any historical arm was mislabeled. The issue is that the advertised protection cannot detect a policy-dependent or later-construction mismatch. Repair by checking every constructed consumer unconditionally, then separately deciding how much readback data to retain. Deduplication of reported records must not deduplicate validation.

### R19. P2: contamination masking removes dividend cash flows

**CONFIRMED, reproduced against a fresh build of the current simulator crate.**

Source: [data.rs:305](https://github.com/general-liquidity/sharpebench/blob/933e0c1056a2e4707b28762c294323bf05bdab65/crates/sharpebench-sim/src/data.rs#L305).

`Dataset::masked` renames closes and dates but sets `dividends: BTreeMap::new()`. A synthetic dataset with a 1% per-period dividend returns `1.0047102658336284` for its first original dividend and 0 for the corresponding masked asset. Masking therefore changes total-return economics, not just the identifiers visible to a policy.

The current CLI stress-suite caller uses non-dividend fixtures, so no change to its historical results is established. The defect affects the public masking operation composed with a dividend-bearing dataset. Repair by applying the same stable symbol mapping to every economic stream and asserting cash-flow and replay equivalence under renamed identifiers.

### R20. P2: the CSV dataset boundary accepts invalid numbers and silently merges conflicting records

**CONFIRMED, reproduced against a fresh build of the current simulator crate.**

Source: [data.rs:103](https://github.com/general-liquidity/sharpebench/blob/933e0c1056a2e4707b28762c294323bf05bdab65/crates/sharpebench-sim/src/data.rs#L103).

`Dataset::from_csv` accepts `NaN` as a close; parsing a floating-point number is not finite/positive validation. Duplicate `(date, symbol)` rows overwrite closes, while a malformed dividend is ignored. A duplicate row changing close 100 to 101 and dividend 1 to `garbage` is accepted as close 101 with the earlier dividend 1, an economic record present in neither input row.

Repair: validate the declared numeric domain, reject conflicting or duplicate keys according to an explicit policy, and report malformed dividends instead of substituting a missing value or retaining an earlier row's field. A legitimate zero dividend must remain distinct from an unavailable or invalid dividend. This finding concerns accepted public input, not proof of malformed committed datasets or a bypass of every downstream scoring gate.

### R21. P2: the throughput producer drops the JavaScript command argument on POSIX

**CONFIRMED by source and a finite subprocess argument-forwarding check, not by running the throughput experiment.**

Source: [make-throughput.py:170](https://github.com/general-liquidity/sharpearena/blob/1be915f330acabacd171cc350bec0def58d9e134/paper/src/make-throughput.py#L170).

`subprocess.run(["node", "bench/throughput.js"], shell=True, ...)` invokes the shell with `node` as its command string on POSIX; the next list item is a shell positional argument, not an argument to Node. With closed input, Node exits successfully without running the benchmark, and `json.loads(proc.stdout)` then fails. A controlled `['node', '-p', '1 + 1']` check on Linux returned empty stdout with `shell=True` and `2\n` with `shell=False`, both exit code 0. This confirms the argument-handling mechanism without running a simulation or modifying evidence.

Repair: invoke the Node executable directly with an argument list and `shell=False`; test command construction on both supported platform families. The defect does not show that the historical Windows throughput values are wrong.

## Independent review reports

- [SharpeBench reviewer](bench-reviewer.md): recent configuration/retry changes, transport and attestation boundaries, and auxiliary methodology. The 18 numbered findings retain the reviewer's order and confidence labels within each slice.
- [SharpeArena reviewer](arena-reviewer.md): recent operational accounting and the full listed Python/native interface slice. The 14 numbered findings retain the reviewer's order and confidence labels within each slice.
- [SharpeBench interface review](bench-interfaces.md): ten source-backed findings across CLI, Python, WASM, imports, and the reference agents. The lifecycle integration gap is already disclosed by the paper and is identified as such in the root scope note.
- [SharpeArena diagnostic review](arena-diagnostics.md): eight findings in return/risk metrics, failure labels, regime reporting, reward-misspecification diagnostics, and RNG-state reconstruction.
- [SharpeBench evidence-producer review](bench-producers.md): eight findings concerning model/cache identity, independent calibration seeds, effective spending controls, complete-field publication, and figure support/counts.
- [SharpeArena evidence-producer review](arena-producers.md): six findings concerning equal evaluation support, producer provenance, historical-reference checks, sealed commitment/replay claims, and figure regeneration coverage.

No numbered finding was dropped for lacking an anchor: each has a file, line and quotation. The Bench reviewer's separate, explicitly unpromoted candidates are retained as follow-up leads, not included in the confirmed-finding count. Root findings and reviewer findings may concern different parts of the same underlying defect; these counts must not be presented as a deduplicated count of independent bugs.

## Reproductions

`reproduce_core_findings.py` accompanies this report. It rechecks R01, R04 through R08, and R13 without touching product files, using anonymous in-memory descriptors for CLI inputs. It requires Linux, the reviewed baseline Bench CLI build, and the unpublished baseline Arena 0.24.1 Cargo archive described above. These are adversarial examples for regressions, not statistical experiments or model results.

`reproduce_dataset_boundaries.rs` exercises R19 and R20. During the audit it was linked against the freshly built local `sharpebench-sim` crate, not an arbitrarily selected cached library. Its assertions characterize the faulty behavior at the reviewed snapshot; after repair, those assertions should fail and be replaced by permanent regression tests for the corrected contract.

## Verification completed

| Check | Result and limitation |
| --- | --- |
| Arena Rust workspace tests | Passed, current source |
| Bench product-crate workspace tests | Passed with `--exclude xtask`; the complete workspace command cannot build `xtask` here because native OpenSSL development files are absent |
| Arena Python suite | 1,091 passed, 2 skipped; existing Windows virtual environment/native extension, current Python sources |
| Arena npm tests | 12 passed; tests the current committed package binary |
| Bench npm tests | 11 passed; existing built distribution and committed package binary |
| Clippy | Both product workspaces clean, all targets; Bench excludes the OpenSSL-blocked `xtask` |
| Rust format checks | Both clean |
| Lean | Both projects built successfully, 4 jobs each |
| Python syntax scan | All 25 Bench and 166 Arena tracked Python files parse |
| Bench provenance | 163 sources and 40 artifacts match |
| Arena provenance | 132 sources, 52 artifacts, 0 model manifests match |
| Arena release-tag verifier | v0.24.1 bound to its exact in-scope tagged tree |
| Bench release-tag verifier on current HEAD | Correctly refused: current HEAD is not the tagged commit. This is not a release failure |
| Current remote CI | [Bench](https://github.com/general-liquidity/sharpebench/actions/runs/33927889336) and [Arena](https://github.com/general-liquidity/sharpearena/actions/runs/33927889350) report success at the reviewed HEADs |

No live containment escape, memory exhaustion, new model evaluation, or market-data acquisition was attempted. Existing CI success is not a new local exercise of Docker. Historical evidence was not regenerated, and the papers were not rebuilt during this audit.

## What the formal and statistical checks establish

Lean compilation establishes the theorems in the small checked models. It does not establish that every Python/Rust execution implements those models, nor does it validate data provenance, stationarity, observation independence, a threat model, or a model's trading skill. The papers largely state that distinction correctly.

Neither the products nor the papers can be assumption-free. The practical target is explicit assumptions, checked preconditions, independently tested implementations, and evidence whose claimed scope is no broader than the checks. No Wolfram MCP server is available in this session, so no Wolfram verification is claimed.

There are meaningful positive checks: the current principal Bench sweep artifacts have complete unique grids; both manifests validate under their declared scopes; the native suites, existing package goldens, and Lean models build or pass within the limitations above; the documentation generally separates Arena's evaluation sandbox from Bench's process-containment boundary and separates historical reference-policy evidence from later capabilities. These checks are worth retaining. They do not cancel the newly identified uncovered cases.

The mathematical review used independent finite examples, analytical identities, the statistical-analysis and scientific-critical-thinking skills, and the Lean skill for proof-scope inspection. The engineering review used code-quality-review and independent audit-analysis reviewers. The writing review used anti-slop to distinguish measured statements from sweeping assurances and to keep the report specific. No skill's presence alone is evidence of correctness.

## Recommended implementation sequence

1. **Preserve evidence identity and safety over every transformation.** Keep original process traces before support filtering; bind displayed scores to signed payloads; match plan contracts, pending ledgers, and settlement outcomes; record actual execution facts rather than preflight predictions.
2. **Make environment configuration and state one coherent contract.** Reset all account/reward state, replay every generator knob, validate the actual action space, and route prompts and adapters through executable examples of the canonical parser.
3. **Use checked statistical reports.** Finite-input validation, a deliberate moment convention, homogeneous scoring units, explicit insufficient-support outcomes, and observable search-count floors across every public scoring surface.
4. **Bound the whole transport.** A line cap does not cap an unbounded queue; a read deadline does not bound a blocking write. Use bounded queues, total output budgets, and one absolute end-to-end decision deadline, with ordinary finite regression tests.
5. **Retain all operational attempts and unknowns.** Successful retry must not erase earlier attempts or their compute; absence of a duration/token measurement must not become zero. Validate accounting against the complete attempt ledger.
6. **Test artifacts and compatibility as a consumer.** Compute semantic hashes over stable inputs and require equality after packaging. Define canonical numeric JSON once, with cross-language pathological-number fixtures.
7. **Then revise documentation and papers.** Keep the historical empirical snapshot distinct from repaired engineering guarantees. Update mechanism descriptions and claim limits without inventing new results. Any empirical reanalysis should identify which mathematical changes affect it before replacing evidence.

## Product and feature opportunities

- A shared execution-evidence validator with typed run/window/seed identities would remove duplicated, partly inconsistent checks. Keep package dependency direction one-way through a versioned file protocol.
- A receipt verifier that checks content, count, order, and a trusted final anchor would make board verification match the operator-facing promise.
- An explicit statistical availability/assumptions object would distinguish a valid estimate, a descriptive value, inadequate support, and invalid input.
- A resumable attempt ledger could support honest latency/cost summaries and replay/debugging without changing the trading eligibility predicate.
- A state-transition specification for reset, step, terminal state, fills, and account conservation is a stronger next formal target than proving arithmetic on labels disconnected from implementation.

These are scoped correctness improvements. A hosted league, external arena integration, extra model backends, or another broad feature-porting sweep is not required to repair the findings.

Two smaller, source-confirmed maintenance issues should travel with that work:

- [Bench's manual publisher](https://github.com/general-liquidity/sharpebench/blob/933e0c1056a2e4707b28762c294323bf05bdab65/scripts/publish.sh#L14) still enumerates eleven crates and omits `sharpebench-memory`, although its comment says it mirrors the [twelve-crate release workflow](https://github.com/general-liquidity/sharpebench/blob/933e0c1056a2e4707b28762c294323bf05bdab65/.github/workflows/release.yml#L159). The tag workflow includes the crate, so this is a drift in the fallback/manual route, not a claim that recent automated releases omitted it. Derive both inventories from one checked release graph.
- [Snapshot documentation](https://github.com/general-liquidity/sharpebench/blob/933e0c1056a2e4707b28762c294323bf05bdab65/docs/book/src/simulator.md#L59) and [env.rs:126](https://github.com/general-liquidity/sharpebench/blob/933e0c1056a2e4707b28762c294323bf05bdab65/crates/sharpebench-sim/src/env.rs#L126) describe `clone_state` as O(1), but it clones a [Book](https://github.com/general-liquidity/sharpebench/blob/933e0c1056a2e4707b28762c294323bf05bdab65/crates/sharpebench-sim/src/engine.rs#L53) containing holdings, pending orders and an accumulating `Vec` trace. Copy cost grows with that state. Correct the complexity claim now; consider shared immutable trace prefixes only if measured tree-search costs justify the change. No performance measurement was run.

## Boundaries not resolved by this audit

- No claim that the frozen tables changed: reproduce affected calculations before making that claim. Both manuscripts distinguish historical evidence from later engineering additions.
- Dataset redistribution is already flagged as unresolved in Bench's `data/RIGHTS.md` and paper datasheet. This audit does not grant legal clearance or replace that decision with a code fix.
- Process isolation is not a proof against a container or kernel escape. Arena's evaluation-sandbox term and Bench's explicit container-run path describe different boundaries.
- An independent line-by-line review of every test, generated asset, third-party dependency, and historical release was not performed. The [coverage ledger](coverage.md) must stay visible rather than being replaced by a blanket completeness claim.
