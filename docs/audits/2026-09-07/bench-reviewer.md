# Independent SharpeBench reviewer reports

> Historical audit evidence at [sharpebench `933e0c1`](https://github.com/general-liquidity/sharpebench/tree/933e0c1056a2e4707b28762c294323bf05bdab65) and [sharpearena `1be915f`](https://github.com/general-liquidity/sharpearena/tree/1be915f330acabacd171cc350bec0def58d9e134).
> Findings, quoted claims, test counts and references to "current" describe those
> reviewed baselines. See [IMPLEMENTATION.md](IMPLEMENTATION.md) for later repair
> dispositions and remaining work; this report is not a current defect list.

Read-only reports returned by the independent reviewer. Findings, order, confidence labels, category coverage and limitations below are retained as supplied. The reviewer's additional candidates are not promoted into confirmed root findings.

## Recent changes

1. CONFIRMED — Environment configuration changes can still mix experiments within one checkpoint. [main.rs:1248](https://github.com/general-liquidity/sharpebench/blob/933e0c1056a2e4707b28762c294323bf05bdab65/crates/sharpebench-cli/src/main.rs#L1248) hashes `entrant_material`; its command-path construction at line 1561 is `format!("cmd\0{cmd}\0{passthrough}")`. This contains variable names, while [external.rs:198](https://github.com/general-liquidity/sharpebench/blob/933e0c1056a2e4707b28762c294323bf05bdab65/crates/sharpebench-sim/src/external.rs#L198) passes their current values through `std::env::vars()`. Thus, keeping `SHARPEBENCH_AGENT_ENV=AGENT_MODE` and the same executable digest while changing `AGENT_MODE` from `conservative` to `aggressive` leaves both checkpoint digests unchanged. Completed cells from the first configuration can be combined with remaining cells from the second, compromising interpretation of pooled DSR and pass^k as results for one policy. The new documentation explicitly limits this binding to names, but [checkpoint.rs:33](https://github.com/general-liquidity/sharpebench/blob/933e0c1056a2e4707b28762c294323bf05bdab65/crates/sharpebench-harness/src/checkpoint.rs#L33) still claims an identity of “every condition that can change a resumable sweep’s result.” This is a remaining configuration-identity gap.

2. CONFIRMED — The expanded audit ledger claims preservation of retry evidence that the checkpoint path discards. [BENCHMARK_ARCHITECTURE_AUDIT.md:159](https://github.com/general-liquidity/sharpebench/blob/933e0c1056a2e4707b28762c294323bf05bdab65/docs/BENCHMARK_ARCHITECTURE_AUDIT.md#L159) states that “append-only or checkpointed retries preserve prior evidence.” However, [failure.rs:151](https://github.com/general-liquidity/sharpebench/blob/933e0c1056a2e4707b28762c294323bf05bdab65/crates/sharpebench-harness/src/failure.rs#L151) returns `RunOutcome::Completed(run), None` after success, without retaining preceding failed attempts. [checkpoint.rs:538](https://github.com/general-liquidity/sharpebench/blob/933e0c1056a2e4707b28762c294323bf05bdab65/crates/sharpebench-harness/src/checkpoint.rs#L538) stores only that terminal outcome; line 361 hardcodes `attempts: 1` for an eventual agent fault. Two transport failures followed by success therefore leave no retry history, while two transport failures followed by an agent fault report one attempt. The checkpoint cannot support the ledger’s claimed attempt-level auditability or accurate operational retry accounting. The implementation predates this diff; the conflicting ledger claim is newly added.

Claims vs. code: The comprehensive identity claim and newly added retry-preservation claim exceed what the implementation records; findings 1–2 quote both sides.

Sample: For the changed checkpoint workflow, raw CSV rows → blank-row filtering → duplicate-key collapse → common-date intersection → warmup removal each have N unverified before and after. The CLI declares 2 windows × 8 seeds = 16 external cells; observed completed, sentinel, and runtime-failed counts are N unverified. Eight-seed averaging and subsequent clone collapse also have N unverified before and after. No new checkpoint execution log establishes those realized counts.

Merges: Checkpoint matching uses the complete ordered `(window index, seed)` matrix, with unique CLI seeds `0..7`; mismatched contracts or task positions are rejected. The upstream CSV loader keys by `(symbol, date)`, overwrites duplicate keys, and intersects date sets without logging unmatched-row counts. Those loader behaviors are unchanged; duplicate incidence and dropped-row N are unverified.

Variables: The changed variables are artifact/invocation identities; the invocation omission is finding 1. No outcome, regressor, unit conversion, deflation formula, or lag transformation changed.

Silent failures: Retry history disappears on eventual success and attempt counts are understated on eventual agent fault, as detailed in finding 2. No new missing-to-zero or numeric-coercion operation appears in the diff.

Estimation: No estimator, fixed effect, or clustering setting changed. The existing path averages eight aligned executions per market bar before stationary-bootstrap inference. Realized estimation N and resampled block counts for the changed checkpoint workflow are unverified.

Read-only verification passed: 163 source files and 40 artifacts match the provenance manifest; the ledger contains 75 rows. No files were changed.

Without actual checkpoint and per-attempt configuration histories, I could not determine whether any existing resumed result already combines different environment configurations.

## Safety slice

Stopped execution. No source edits, containers, entrant endpoints, or model experiments were used.

### New findings

1. **CONFIRMED — A sandboxed entrant can exhaust host memory through stdout.**\
   `crates/sharpebench-sim/src/external.rs:271`: `let (tx, lines) = mpsc::channel();`\
   The reader continuously queues lines with `tx.send(Ok(wire))` at line 282, while decisions consume one queued line at a time. The 8 MiB limit applies to each line, not the unbounded queue. Many individually permitted lines can exhaust the host process outside the container’s memory limit, killing the sweep before failure accounting or checkpoint persistence.\
   Claim/code disagreement: lines 278–280 describe stopping “the memory burn this cap exists to stop,” but the unlimited number of permitted lines remains unbounded.\
   **Reproduction:** static confirmation only; intentionally did not flood memory. Existing boundary tests exercise one oversized line, not aggregate queued output.

2. **CONFIRMED — The stdio “per-decision wall-clock budget” excludes blocking input writes.**\
   `crates/sharpebench-sim/src/external.rs:303` promises an override of the “per-decision wall-clock budget.” At lines 359–365, execution instead performs:
   ```rust
   writeln!(self.stdin, "{line}")
   self.stdin.flush()
   let deadline = Instant::now() + self.timeout;
   ```
   An entrant that does not read stdin can block a sufficiently large observation before the timer starts. Neither retries nor timeout classification can recover while that write is blocked. This affects the host harness even with a containerized entrant.\
   **Reproduction:** static confirmation only. The existing silent-agent test deliberately consumes input (`read line; sleep 60`) and uses a tiny observation, so it cannot detect this case.

3. **CONFIRMED construction flaw; cross-window exposure depends on key reuse — Sealed datasets reuse their keystream.**\
   `crates/sharpebench-attest/src/sealed.rs:83` derives blocks solely from `key`, `"sb-seal"`, and the counter; line 94 resets that counter to zero for every seal. Line 113 encrypts without a nonce or dataset-specific derivation. The canary is only passed to commitment construction.\
   Claim/code disagreement: lines 107–110 say the published plaintext “cannot be recovered without `key`.” For two datasets sealed under one key, however, `C1 XOR C2 = P1 XOR P2`. Once an earlier dataset’s plaintext is disclosed, the corresponding bytes of a later held-out dataset can be recovered without that key. This undermines the contamination defense the module supplies.\
   **Reproduction:** algebraically confirmed from the complete implementation; no deployment key reuse was observed or tested. Do not claim that a particular published dataset has leaked.

4. **CONFIRMED — Malformed Unicode in hexadecimal input panics the verifier.**\
   `crates/sharpebench-attest/src/lib.rs:141`:
   ```rust
   u8::from_str_radix(&s[i..i + 2], 16)
   ```
   The decoder checks even byte length but slices unvalidated UTF-8 at byte offsets. `€€` has even byte length and fails at a non-character boundary. The same defect appears at `crates/sharpebench-attest/src/sealed.rs:138`, despite the decoder’s line-131 promise of “`None` if malformed.” Public keys, signatures and sealed ciphertext can reach these decoders.\
   **Reproduction succeeded:** the existing native `target/debug/sharpebench verify /dev/stdin --pubkey '€€' --json`, supplied an in-memory JSON document through stdin, exited **101**:
   > `panicked at crates/sharpebench-attest/src/lib.rs:141:39: end byte index 2 is not a char boundary`

   This was a local verification call with no files written. The binary predates the latest checkpoint-only changes; the relevant attestation source is unchanged.

5. **CONFIRMED — A bare relative checkpoint filename fails persistence on Unix after the rename.**\
   `crates/sharpebench-harness/src/checkpoint.rs:248`:
   ```rust
   if let Some(parent) = path.parent() {
       std::fs::File::open(parent)?.sync_all()?;
   }
   ```
   For `Path::new("sweep.json")`, `parent()` is the empty path, not `"."`. Opening it fails. The preceding rename at line 246 has already installed the checkpoint, but `save` returns an error, interrupting a valid sweep using a common filename form. Absolute-path checkpoint tests do not exercise this condition.\
   **Reproduction:** static confirmation; no new checkpoint files created.

6. **CONFIRMED — Chain verification cannot detect deletion of terminal records.**\
   `crates/sharpebench-attest/src/public.rs:12` claims that “a dropped row” breaks the chain. The fully reviewed `verify_chain_public` checks only the supplied links and returns success after that loop; there is no required terminal anchor or signed record count. Every valid prefix therefore remains valid, including removal of the final row. The HMAC verifier has the same limitation.\
   Arena verification also iterates the unsigned `state.json` publication-order list; it does not independently anchor the expected terminal publication. Removing final publications together with their state entries is consequently outside the completeness check.\
   **Reproduction:** static confirmation; no signed artifacts modified. The public-chain drop test removes an interior record, not the final record. Exact verifier-body line numbers were not retained before the stop request; the quoted claim’s line number above was retained.

### Additional candidates, not promoted

- **HTTP deadlines — CONFIRMED construction concern, unexecuted:** `external.rs`, `HttpAgent::decide_once`, connects before setting socket timeouts and uses per-operation read timeouts rather than an absolute decision deadline. Slow progress can extend the advertised budget. Exact line numbers were not retained; no endpoint contacted.
- **Decision-cost overflow — CONFIRMED expression, downstream impact not reproduced:** `protocol/src/lib.rs`, `DecisionCost::billable_units`, adds `u64` token counts before converting to floating point. Semantic validation does not check that sum. Depending on overflow settings, extreme valid integer inputs can panic or wrap. Parent’s scoring/math review may cover this independently.
- **Probe execution — SUSPECTED operational gap:** appended `/bin/sh` commands do not override an image’s existing ENTRYPOINT. The readiness result therefore depends on the supplied fixture executing the appended script faithfully. No container execution was authorized or attempted.
- **OOM classification — SUSPECTED:** the live test deliberately makes the allocator PID 1. Arbitrary entrant wrappers can have different child-OOM behavior; the reviewed mocks cannot establish how those cases appear in Docker state.

### Coverage and limitations

Full production-file review completed:

- `crates/sharpebench-sim/src/external.rs`
- `crates/sharpebench-sim/src/transport.rs`
- `crates/sharpebench-arena/src/lib.rs`
- `crates/sharpebench-arena/src/sandbox.rs`
- `crates/sharpebench-attest/src/lib.rs`
- `crates/sharpebench-attest/src/public.rs`
- `crates/sharpebench-attest/src/registry.rs`
- `crates/sharpebench-attest/src/sealed.rs`
- `crates/sharpebench-attest/src/canary.rs`
- `crates/sharpebench-protocol/src/lib.rs`
- `crates/sharpebench-harness/src/lib.rs`
- `crates/sharpebench-harness/src/checkpoint.rs`
- `crates/sharpebench-harness/src/failure.rs`

The corresponding inline tests were read in full. Arena/CLI integration-test files, protocol `schema_drift.rs`, and simulator `trajectory.rs` were not completed before the stop request. No new test suite was executed; the sole executed flaw reproduction was malformed-key verification.

Claims: concrete contradictions above. Sample: no empirical runs executed; **N unverified** for deployment losses and failures. Merges: no new relational-merge findings in this safety slice. Variables: token-count overflow candidate; no regression/deflation assessment here. Silent failures: incomplete chain detection and incomplete resource/time bounds. Estimation: outside this bounded slice.

The principal unavailable check is whether deployed held-out datasets reuse a sealing key; that determines whether the confirmed cipher defect has exposed actual evaluation data.

## Methodology slice

Completed the read-only slice: 26 production files and their inline tests, approximately 9,747 lines. No source changes, network calls, model runs, or stress tests.

### New findings, ordered by consequence

1. **CONFIRMED — FULL honesty verdict undercounts the search it directly observes.**\
   `crates/sharpebench-edge/src/verdict.rs:211` calls:
   ```rust
   let honesty = is_my_sharpe_real(&field[winner_idx], cfg);
   ```
   It never raises `cfg.n_trials` to the field size. The Python entry point defaults `n_trials = 1` at `crates/sharpebench-py/src/lib.rs:199` and automatically selects the highest-Sharpe candidate at lines 225–234. Thus the advertised search-aware FULL report can select among many candidates while its headline DSR verdict prices only one trial. The additional field tests are sidecar results; they do not alter that verdict.\
   **Improvement:** enforce `max(declared_trials, observed_field_size)` before constructing the headline, as core ranking already does at `composite.rs:1661`. Validate rectangular, finite inputs and return unavailable—not PBO zero—when the test cannot be estimated.

2. **CONFIRMED — Self-describing board verification does not verify its displayed scores.**\
   `crates/sharpebench-leaderboard/src/lib.rs:137` says verification proves integrity of “the published condition and scores.” At lines 179–186, verification checks the chain and only:
   ```rust
   first.payload == spec_payload(&b.spec)
   ```
   It never compares `b.scores` with the signed score payloads or checks their count. Changing those scores leaves this public verifier’s result unchanged. This is distinct from the previously reported terminal-chain deletion issue.\
   **Improvement:** require exactly one spec link plus one link per score and compare every canonical score payload. Existing tests alter the spec, not the exposed score array.

3. **CONFIRMED — Auxiliary durability and behavior attribution use incompatible run axes.**\
   `crates/sharpebench-core/src/roles.rs:88` claims truncation makes streams align “period by period,” but lines 94–109 average every run’s relative index after truncating all runs to the shortest. Harness runs include different market windows, not just repeated observations of the same dates. A 40-period run also causes the final 40 periods of an 80-period run to disappear from this diagnostic—even when that tail carries the entire result.\
   Separately, `composite.rs:1203` feeds per-run **mean returns** into `edge_half_life`; `decay.rs:2` describes a chronological **IC** decay regression. The composite field honestly labels units as runs, but seed replicates are not successive periods of edge aging. Reordering execution seeds can change this reported durability without changing economic history.\
   **Improvement:** preserve window/time coordinates, aggregate seeds within each window, and calculate behavior contributions on the same retained sample as the reported result. Distinguish return-drift decay from actual IC decay.

4. **CONFIRMED — Negative gamma is incorrectly classified as unbounded loss.**\
   `crates/sharpebench-core/src/greeks.rs:212` states: “Short gamma implies negative convexity → unbounded tail loss potential.” Lines 222–225 implement:
   ```rust
   let naked_short_gamma = greeks.gamma < policy.gamma_floor;
   unbounded_tail: naked_short_gamma,
   ```
   A short put has negative gamma but bounded payoff loss for nonnegative underlying prices; a bounded credit spread can also have negative local gamma. Net local Greeks cannot identify whether an exposure is naked or whether terminal loss is unbounded.\
   **Improvement:** report `net_short_gamma` from Greeks; determine boundedness separately from the position’s payoff tails and hedges. Current tests cover a naked short call and a long option, not the counterexamples.

5. **CONFIRMED, REPRODUCED — Zero volatility is incorrectly treated as expiration.**\
   `crates/sharpebench-core/src/greeks.rs:59` promises “discounted intrinsic value,” but lines 62–68 return:
   ```rust
   if t <= 0.0 || vol <= 0.0 {
       // spot - strike, or strike - spot
       return intrinsic.max(0.0);
   }
   ```
   Discounting disappears when `vol == 0` and time remains. The Greeks branch has the same problem.\
   **Local reproduction:** existing CLI, spot/strike 100, maturity 1, rate 0.05:

   | Volatility | Price | Delta |
   |---|---:|---:|
   | 0 | 0 | 0 |
   | 0.000001 | 4.8770575499 | 1 |

   **Improvement:** separate expiration from the deterministic zero-volatility limit; reject negative volatility. Test continuity and put-call parity at zero volatility. The current zero-volatility put test pins the incorrect result.

6. **CONFIRMED — Multi-session credit can resume after a broken dependency chain.**\
   `crates/sharpebench-memory/src/multisession.rs:76` describes withholding credit when “the memory chain … was not retained.” Yet `retained_of` records only `lift > 0` at line 160, and dependency checks at line 178 consult that raw flag, not whether the prerequisite’s own dependencies succeeded. With A failing, B depending on A but showing positive raw lift, and C depending on B, B loses credit but C receives it. Cycles between distinct session IDs are also accepted despite the “earlier sessions” contract.\
   **Improvement:** validate a DAG, propagate dependency-qualified retention topologically, and distinguish raw pooled significance from significance of the credited memory-chain effect. Existing tests cover one-hop failures only.

7. **CONFIRMED — Candidate lineage verification does not reconcile declared lineage with the raw candidate.**\
   `crates/sharpebench-core/src/candidate_lineage.rs:691` validates the separate `declared_lineage` and its resolved parents/sources, but the `"declared"` branch never compares that declaration with `raw_candidate["lineage"]`. Raw candidate hashing at lines 455–459 therefore does not establish that the displayed declaration came from those raw bytes.\
   The existing success fixture demonstrates the gap: `candidate()` at lines 1027–1042 contains no lineage field, while `record()` supplies `declared_lineage` and `"declared"` at lines 1094–1099; the verification test succeeds.\
   **Improvement:** independently parse lineage from the raw candidate and require exact agreement with the derived declaration and status. Also verify duplicate references against the referenced candidate, rather than merely checking that the ordinal is earlier.

8. **CONFIRMED — A newly reversed signal can execute the obsolete pending direction.**\
   `crates/sharpebench-core/src/entrants.rs:379` promises that a parked signal is cancelled when a fresh signal points the other way. Instead, `signal_gate` checks confirmation at line 396 and returns the old direction at line 406 **before** checking the reversal at line 413. A pending long plus a fresh short therefore executes long if the moving averages simultaneously confirm the old long.\
   **Improvement:** evaluate reversal and expiration before releasing a parked signal, with an explicit precedence contract. The reversal test uses moving averages that do not confirm the pending direction, so it misses the simultaneous case.

9. **CONFIRMED — Briefing neutrality limits sections, not asset areas.**\
   `crates/sharpebench-core/src/briefing.rs:86` specifies maximum rows per “single asset-area.” The implementation iterates sections at line 150 and tests each section’s row count and salience independently. It neither groups nor rejects repeated `asset_area` values. Splitting one area into multiple individually acceptable sections can give it all the briefing’s attention while returning `balanced = true`.\
   **Improvement:** aggregate by normalized area identity before checking row limits and salience; reject duplicate identities if sections are intended to be unique. Treat unspecified table ordering as unverified when option-order compliance is required.

10. **CONFIRMED — Evidence-preimage encoding is not unambiguous for arbitrary allowed strings.**\
    `crates/sharpebench-core/src/evidence_coverage.rs:185` claims no value can resemble a field boundary because separators cannot appear in field names. However, `push_record` at lines 235–239 copies unrestricted **values** between literal separators without escaping or length prefixes. Values can themselves contain separators and subsequent field names, making distinct field-value assignments serialize identically.\
    **Improvement:** use length-prefixed records or canonical structured serialization. Existing tests cover field presence, ordering and redaction, not ambiguous values. Repository search found only test callers of this helper, so this is a latent library-contract defect, not evidence that a current published digest was affected.

### Usefulness and concrete methodology improvements

- **Memory:** paired lift and poisoning comparisons are useful descriptive components. Require finite outcomes/costs, valid `alpha`, task identities, and a common oracle task population. `lib.rs:263` currently permits a different-length oracle; `fraction_of_ceiling` then compares potentially different task mixes. The documented “oracle no better than baseline ⇒ zero” behavior also disagrees with the absolute-value guard at line 330.
- **Budget curve:** retaining the unsmoothed curve and recording the budget-search footprint are useful. Match evaluation windows and sample sizes across budgets. A plateau currently triggers `overfit_onset` at `budget_curve.rs:217`; call it non-improvement unless uncertainty supports actual deterioration.
- **Regime comparison:** the documented refusal to claim a GAMLSS fit is appropriate. Rename zero/no-trade mass to near-zero-return mass unless trade/position flags are supplied: a zero return does not identify inactivity. Report sign reversal between regimes even when the pooled gap is exactly zero.
- **Attribution:** `alpha_beta` implements the stated simple regression, but require exact alignment rather than silently taking the shorter prefix. Role loadings are marginal associations, not causal or additive contributions.
- **Allocation, PIT and confabulation:** their basic counting formulas are coherent. Preserve unknown/unmeasured states separately from vacuous passes and validate policy parameters. Allocation turnover is target-weight churn, not realized trading turnover after price drift.
- **Rediscovery:** retain its advisory role; similarities require common dated observations, and single-linkage clusters can connect endpoints that are not themselves near-clones.
- **Disqualification:** useful legibility layer for ordinary finite default-config scores. `rollup` hardcodes default thresholds at `disqualification.rs:146`; accept the actual scoring configuration so explanations cannot contradict a nondefault board.

### Coverage and verification limits

Read all production bodies and inline tests in:

- Memory: `lib`, `confabulation`, `multisession`, `pit`, `poisoning`.
- Edge: `lib`, `hlz`, `mintrl`, `pbo`, `verdict`.
- Leaderboard: `lib`.
- Core: `allocation`, `attribution`, `budget_curve`, `decay`, `greeks`, `econrationality`, `rediscovery`, `rolling`, `regime_compare`, `roles`, `briefing`, `entrants`, `evidence_coverage`, `disqualification`, `candidate_lineage`.

Focused caller inspection covered composite diagnostic integration, harness run ordering, Python/WASM FULL wrappers, CLI Greeks and CLI lineage extraction. Only the two ordinary local pricing calls above were executed; no full test suite was run for this slice. All other findings are code-confirmed, with deployment effect unmeasured. `git status --short` remained empty.
