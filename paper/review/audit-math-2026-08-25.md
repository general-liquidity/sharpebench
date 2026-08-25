# SharpeBench paper: mathematical and statistical soundness audit

Date: 2026-08-25. Scope: paper/main.tex + sections/*.tex (including \input fragments), paper/evidence/final/*, crates/sharpebench-core (composite.rs, pass_k.rs, rediscovery.rs, stats via sharpebench-stats), crates/sharpebench-stats (deflated_sharpe.rs, significance.rs, stats.rs, selection.rs). Read-only; every number below was recomputed in Python or read from the committed JSONL.

Severity totals: 3 CRITICAL, 4 MAJOR, 11 MINOR, 14 OK.

Conventions: "pp" = per period, "ann" = annualized, P = periods per year, k(N) = (1-gamma) Z(1-1/N) + gamma Z(1-1/(Ne)); k(50) = 2.2763. Anchors are file:line of the audited working tree.

---

## 1. Displayed equations vs sources vs kernel

### 1.1 eq:psr (03-benchmark.tex:10-12) - OK
Matches Bailey and Lopez de Prado (2012): PSR = Phi[(SR_hat - SR*) sqrt(n-1) / sqrt(1 - g3 SR_hat + (g4-1)/4 SR_hat^2)] with g4 non-excess (normal = 3), so the (g4-1)/4 coefficient is correct. Kernel identical term by term: deflated_sharpe.rs:22-36 uses `(g4 - 1.0)/4.0`, `sqrt(n-1)`, and stats.rs:79-91 documents kurtosis as non-excess (returns 3.0 for degenerate input). The 1e-12 radicand guard is stated in the paper (03-benchmark.tex:17) and present (deflated_sharpe.rs:31-33). Dimensionally consistent: everything per period.

### 1.2 eq:deflation (03-benchmark.tex:14-16) - OK, one stated assumption missing (MINOR, see 1.7/m12)
Matches the BLdP (2014) expected-maximum approximation including the Euler-Mascheroni weighting and the 1/(Ne) quantile. Kernel identical: deflated_sharpe.rs:41-51 (GAMMA = 0.5772156649015329, `norm_ppf(1.0 - 1.0/(n*e))`). The N <= 1 convention (SR*_0 = 0, i.e. DSR degenerates to PSR against 0) is stated at 03-benchmark.tex:17 and implemented (deflated_sharpe.rs:43). Note BLdP write E[max SR_N] = E[SR] + sigma[...]; the kernel and paper drop the E[SR] term, i.e. assume the trial mean is zero. That assumption is materially violated by the shipped field (see Finding M4).

### 1.3 eq:effective-n (03-benchmark.tex:18-20) - OK
N_eff = max(N_host, n_field) + N_declared. Implemented in two pieces: composite.rs:1483-1486 (`rank_cfg.n_trials = cfg.n_trials.max(field.len())`) then composite.rs:1055 (`cfg.n_trials.saturating_add(sub.in_sample_trials)`). Test `ranked_field_size_is_an_observable_trial_floor` (composite.rs:1619) pins it. The paper's "at least eight for every ranked cell" (05-experiments.tex:3) reproduces: every N=1 sweep record has effective_n_trials = 8.

### 1.4 sqrt(periods per year) conversion applied exactly once - OK
Configured prior: composite.rs:581-583 (`trials_sr_std / periods_per_year.sqrt()`), the only conversion site; the measured path never converts (composite.rs:999-1009 and test `measured_sr_std_is_never_reconverted`, composite.rs:2143). Per-run annualized minimum: composite.rs:589-591. Test `annualized_prior_is_converted_per_period_once` (composite.rs:2043) asserts once-and-only-once. Matches 03-benchmark.tex:21 and app:unitsfix (C-simdata.tex:22-24).

### 1.5 eq:dispersion-floor (03-benchmark.tex:22-24) - OK as an equation
sigma_eff = max(measured_pp, sigma_min_ann / sqrt(P)) matches Deflation::measured (composite.rs:999-1009, floor = `min_measured_trials_sr_std / periods_per_year.sqrt()`), defaults 0.5/0.5 (composite.rs:667-690 region), min field 5, clone collapse default on. The floor's threat-model claims are overstated (Finding M3).

### 1.6 Relative verdict eq:relative (relative-mandate-fragment.tex:10-13) - OK
Matches per_run_passes (composite.rs:604-661): same-index cell alignment, missing/misaligned cell fails (never falls back), std(e) > 0 required, PSR(e, bar) >= 0.90. The claim that a zero-dispersion excess yields Sharpe 0 hence PSR = Phi(0) = 0.5 is correct (deflated_sharpe.rs:12-16 defines SR = 0 at zero vol; recorded psr for `hold` is 0.5000000005, the 5e-10 being the A&S erf absolute error, harmless).

### 1.7 Small definitional notes - MINOR (m7, m12)
- stats.rs:64-91 computes "population" skewness/kurtosis (divide by n) but standardizes by the Bessel-corrected std. Mixed convention; bias O(1/n), irrelevant at the n used, but the paper says "sample skewness" (03-benchmark.tex:9). Fix: one sentence, or switch to the standard sample-moment estimators.
- eq:deflation should state the zero-trial-mean assumption of the BLdP E[max] formula, since the shipped field's mean per-period Sharpe is strongly negative (see M4).

### 1.8 Numerical special functions - OK (checked because many reported DSRs are tiny)
A&S 7.1.26 erf has 1.5e-7 absolute error but its tail form is relatively accurate: recomputed relative error of norm_cdf vs exact is +0.16% at z = -5, +0.49% at z = -6.5, +0.87% at z = -7.5. So reported values like 2.02e-8, 4.02e-11, 5.76e-14 are supported to within about 1 percent, and the N-sensitivity monotonicity is real. No action needed.

---

## 2. Hand recomputation of formula-derived numbers

All recomputed with scipy-free Python (NormalDist).

| Paper claim | Location | Recomputed | Verdict |
|---|---|---|---|
| SR*_0 = 1.14 pp at N=50, sigma=0.5 | 05-experiments.tex:31, C-simdata.tex:24 | 1.13815 | OK |
| tab:units row 0.070: 0.159 pp; 14.9 / 7.5 / 2.5 / 1.1 | 05-experiments.tex:43 | 0.1593; 14.914 / 7.457 / 2.529 / 1.149 | OK |
| row 0.200: 0.455; 42.6 / 21.3 / 7.2 / 3.3 | :44 | 0.4553; 42.610 / 21.305 / 7.227 / 3.283 | OK |
| row 0.350: 0.797; 74.6 / 37.3 / 12.6 / 5.7 | :45 | 0.7967; 74.568 / 37.284 / 12.647 / 5.745 | OK |
| row 0.500: 1.138; 106.5 / 53.3 / 18.1 / 8.2 | :46 | 1.1382; 106.525 / 53.263 / 18.068 / 8.207 | OK |
| Annualized Sharpe of 18 (daily) and 106 (hourly) | abstract, 01-introduction.tex:10, 05:31 | 18.068, 106.53 | OK |
| Relative SE of sample std at n=8 is 27 percent, 1/sqrt(2(n-1)) | 03-benchmark.tex:45 | 0.2673 | OK (and code doc "50% at three, 35% at five" = 0.500, 0.354, composite.rs field doc) |
| Onset 0.35 pp weekly -> 2.52 ann; 0.20 pp daily -> 3.17 ann | 05-experiments.tex:111, fig caption :116 | 0.35 sqrt(52) = 2.5239; 0.20 sqrt(252) = 3.1749 | OK |
| Weekly floor 0.5/sqrt(52) = 0.0693 pp | 05-experiments.tex:55 | 0.069338 | OK |
| Sybil: 0.3258 -> 0.0559 under 200 clones | 03-benchmark.tex:45, sybil-defense-fragment.tex:5 | internally consistent: std of 7 honest values (SS = 6 x 0.3258^2) diluted over 206 dof gives 0.0556; matches 0.0559 with clone value near the honest mean | OK |
| Sybil footprint: all 207 count toward trials | sybil fragment:5 | N_eff = max(50, 207) = 207 per eq:effective-n; consistent | OK |
| FinBen intervals overlap: 1.51 +/- 1.08 vs 0.02 +/- 0.87 | abstract, 01:3 | [0.43, 2.59] vs [-0.85, 0.89], overlap | OK |
| fig:deflation "crosses between 32 and 64 trials" | 05-experiments.tex:7 | With the script's sigma_trials = 0.30 pp (make-figures.py:107), SR 0.85, n=150: DSR = 0.989 at N=32, 0.927 at N=64. Crossing of the 0.95 bar confirmed, but ONLY at sigma = 0.30, which the caption (05:22-23) does not state | MINOR m3 |

MINOR m3 fix: add "sigma_trials = 0.30 per period" to the fig:deflation caption; without it the crossing is not reproducible from the stated inputs (at the shipped 0.5 the crossing is below N=32).

---

## 3. Evidence consistency (committed JSONL vs paper tables)

Spot checks were exhaustive where cheap, not three cells: all 9 rows x 5 numeric columns of tab:eligibility, all 36 named rows of tab:relative, the luck-floor summaries, the risk-managed numbers, the N-sensitivity series, the falsification-corner values, and the 1,000-agent floor.

### 3.1 tab:eligibility (05-experiments.tex:59-81) - OK
Default cell (dsr_bar 0.95, n_trials 50, sr_std_pinned null) of each final/*.jsonl reproduces every cell: US1w b&h 0.004596/no/0.320/0.0005; C1w momentum 0.02349/0.563/0.0005; C1d b&h 0.02852/0.476/0.0015; commodities 0/0.957/0.0030; US1d 0/0.336/0.0005; C4h 0/0.479/0.0005; C1h 0/0.490/0.0005; FX 0/0.199/1.0000; rates 0/0.840/0.0035. Zero eligible in all 576 cells (64 x 9), 512 records per dataset, 4,608 total: all confirmed. "Field-measured sigma subject to the floor": weekly panels, crypto-1d and commodities are measured_floored at 0.5/sqrt(P) (0.0693, 0.0693, 0.02617, 0.03150); the rest measured (US1d 0.07040, C4h 0.0627, C1h 0.1253, FX 0.1801, rates 0.0632). The "0.070 measured from the daily US indices field" note in tab:units matches 0.07040.

### 3.2 sec:falsify corner values (05-experiments.tex:130) - OK
Weekly crypto best random: 0.06983 at host N=1 (eff 8) and 1.702e-5 at N=50 (paper 0.0698, 1.70e-5). Weekly US: 3.311e-5 and 5.757e-14 (paper 3.31e-5, 5.76e-14). Luck floor's best raw return below best reference on all nine datasets: confirmed (0 violations).

### 3.3 sec:riskmanaged (05-experiments.tex:95-103) - numbers OK, interpretation hit by C1
risk-managed.jsonl, us-indices-1w: psr 0.99982, bootstrap_p 0.0004998, worst_run_drawdown 0.1128, DSR 2.024e-8, source MeasuredFloored 0.0693: all match the text. N-sensitivity 0.02549 / 0.003481 / 5.966e-6 / 2.024e-8 / 4.024e-11 at N = 7/10/25/50/100: matches, monotone. Smallest worst-window drawdown among trading agents on exactly 7 of 9 datasets (exceptions crypto-1h 0.874 vs b&h 0.490, FX 0.325 vs 0.199): matches "87 vs 49" and "33 vs 20". Perturbation spreads 5.14e-4 and 3.42e-6, both under 1e-3: matches sec:perturb.

### 3.4 tab:relative (relative-mandate-fragment.tex:20-69) - OK
All 36 named rows verified programmatically against relative-mandate.jsonl: regime strings, both pass-pattern strings, cell counts, gained/lost window lists, eligibility flags all match (e.g. C1d risk-managed P..P.. / .....P, 16/8, gained [5], lost [0,3]). Luck-floor summary matches: 45 records, max 12 of 48 cells default, max 3 relative, no window passes on all eight seeds under either verdict, none eligible. Buy-and-hold's 16 lost windows across the nine datasets: recount = 2+2+2+2+2+3+0+2+1 = 16. Window-5 bear on all four crypto panels and window-1 bear on FX and commodities: confirmed in the regime tags.

### 3.5 sec:luck1000 / fig:luck1000 (hardening-fragment.tex) - numbers OK, see M2
luck-floor-1000.jsonl: 2,000 agent rows + 2 summaries. Crypto diagnostic max dsr_field 0.030192 (paper 0.0302); first-five max 0.007510 (paper 0.00751); measured ann dispersion 0.045469 (paper 0.0455); shipped-floor and configured maxima exactly 0; zero eligible; floor's best raw return -0.00134 < b&h 0.00060. All match.

### 3.6 Field-wide test values (03-benchmark.tex:27) - values OK, construction flaged under M1
US-1d RC p 0.0004998 with step-down flagging an agent; commodities RC p 0.52174 with none flagged. Matches "0.0005" and "0.522". But on US-1d the step-down flags buy-and-hold AND hold AND momentum (see M1).

### 3.7 tab:data (03-benchmark.tex:53-74) - MINOR m1
Caption says "Bars are per symbol". JSONL n_bars (bars per symbol): US1d 2513, US1w 522, C1h 24000, C4h 6000, C1d 1000, C1w 307, FX 4157, commodities 4126, rates 4157. The table matches on seven rows but prints US indices 1d as 7540 (= 2513 x 3 + 1, a total row count) and Crypto majors 1d as 5001 (= 1000 x 5 + 1). Fix: 2513 and 1000, or change the caption to totals and adjust all rows consistently.

No other cell failed to reproduce.

---

## 4. Statistical validity

### CRITICAL C1: pooled statistics multiply the effective sample size by the seed count
Every pooled statistic - PSR (composite.rs:1051), DSR (composite.rs:1056), bootstrap p (composite.rs:1083), DSR CI (composite.rs:1207), pooled drawdown (composite.rs:1110) - is computed on the concatenation of all n_windows x n_seeds runs. The paper itself states the eight execution seeds "vary slippage only, a few basis points" (03-benchmark.tex:27), so for every deterministic agent (buy-and-hold, momentum, hold, risk-managed) the eight seed runs are near-duplicates and the pooled track contains each bar roughly eight times. The sqrt(n-1) in eq:psr and the bootstrap therefore run at n about 8x the number of distinct bars.

Evidence from the committed records (implied per-period Sharpe = Z(PSR)/sqrt(n-1)):
- US-1d buy-and-hold: psr = 1 - 1.1e-12, pooled n = 48 x 408 = 19584 implies SR = 0.0502/day = 0.80 annualized (plausible for 2016-2026 US indices); the honest n = 6 x 408 = 2448 would imply 2.26 annualized (implausible). The pooled path is therefore confirmed to include all 8 seeds.
- Recomputed at honest n (one copy per window), the headline gate values change materially:
  - risk-managed, weekly US: PSR 0.9998 -> 0.894 (below the 0.90 per-run bar level), bootstrap p 0.0005 -> approx 0.11 (fails alpha = 0.05). Finding 4's sentence "clears the bootstrap null (p = 0.0005), the probabilistic Sharpe (0.9998) ... refused solely on deflation" (05-experiments.tex:97) does not survive de-duplication: at honest n the agent also fails the bootstrap gate.
  - tab:eligibility DSRs at honest n: US1w b&h 0.0046 -> 0.18; C1w momentum 0.0235 -> 0.24; C1d b&h 0.0285 -> 0.25. Still refused at 0.95, so Finding 2's verdicts stand, but every displayed DSR, PSR and p magnitude is wrong, and any near-bar future submission would be admitted or refused on an inflated n.
  - The pass-witness boundary moves (see C3): the witness's runs ARE independent draws per (window, seed) (pass_witness.rs:91-100, seed = base ^ (w<<32) ^ (k+1)), so the witness legitimately has 48 independent runs while a real agent has effectively 6; the calibration "the shipped defaults certify roughly annualized Sharpe 3" (05-experiments.tex:111) is a statement at 8x the independent data a real deterministic agent brings. Recomputed at honest n via the implied margins, the weekly witness clears the 0.95 DSR bar at no sampled edge (DSR 0.74 at s=0.35, 0.83 at s=0.60) and the daily witness clears near s = 0.30-0.35 (annualized 4.8-5.6), where DSR, not pass^k, binds; the claim "the daily geometry isolates pass^k as the binding gate" inverts.

Fix: pool one run per window for the pooled statistics (or average the seed runs per window before pooling); keep the seeds only in pass^k, where they are used per run. Alternatively deflate the pooled n by the seed replication factor. Then regenerate every evidence file and re-derive Findings 2 and 4 and the witness boundary. The per-run leg (per-run PSR at n = window_len) is unaffected.

### CRITICAL C2: the "corrected" measured-dispersion path re-creates the units trap Finding 1 warns about, and Finding 1's inference and attribution are unsound
(a) Implied annualized deflation bar at N = 50 under the shipped defaults (SR*_0,ann = k(50) x sigma_pp x sqrt(P), from trials_sr_std_used in the default cells): floored panels 1.14; US-1d 2.54; rates 2.28; C4h 6.68; FX 6.51; C1h 26.70. So after the units fix, an hourly agent must beat an annualized Sharpe of about 27 and a 4-hour or FX agent about 6.5 to clear deflation. tab:units (05-experiments.tex:33-49) presents 14.9-106.5 on hourly as the error being documented; the corrected kernel's live bar on the same data is 26.7 and is reported nowhere. The mechanism: the measured dispersion is dominated by the luck-floor agents' cost drag, which is roughly constant per period while volatility shrinks like 1/sqrt(P), so sigma_pp does not scale like 1/sqrt(P) and the annualized bar grows with frequency, exactly the failure mode Finding 1 generalizes ("the error grows with the square root of the number of periods per year", 01-introduction.tex:10).
(b) The catching inference is invalid as stated: "A PSR of one and a DSR of zero on the same series cannot both be verdicts about the data" (05-experiments.tex:29, echoed 02-principles.tex:5). PSR tests SR > 0 and DSR tests SR > SR*_0; both can simultaneously be correct verdicts about the data whenever 0 < SR_true < SR*_0. The corrected tab:eligibility still contains the identical pattern (US-1d: PSR 1.0000, p 0.0005, DSR 0.000), so by the paper's own stated logic the current threshold would also be "a verdict about the threshold". The valid version of the check is "the implied annualized bar (18, 106) exceeds any Sharpe ever recorded", which is what actually diagnosed the bug.
(c) Misattribution: "that units bug alone made the same index score a DSR of 0.984 on weekly bars and 0.000 on daily bars" (05-experiments.tex:31). The project's own evidence note (paper/evidence/FINDING-passk.md, "The DSR is unchanged because every real field has eight agents, so rank takes the measured path... The units fix changed only the configured prior, which real fields do not use") documents that those two numbers came from the measured path, which never contained the units bug. The weekly 0.984 came from an unstable 8-agent measured estimate (later floored), not from the prior's units.

Fix: add sigma_eff annualized and SR*_0 annualized columns to tab:eligibility; state in Finding 2 that the deflation refusal on 1h/4h/FX corresponds to annualized bars of 27 / 6.7 / 6.5 and discuss whether a luck-floor-driven measured dispersion is the intended prior at high frequency; reword the PSR-vs-DSR "cannot both" inference into the implied-annualized-bar check; correct the attribution sentence at 05:31.

### CRITICAL C3: pass-witness circularity - the witness sets most of its own bar
The measured-dispersion path is active in the witness field of six agents (five zero-edge + the witness, pass_witness.rs:37, 127-146). The zero-edge Sharpes are near 0, so once above the floor the measured dispersion is essentially the sample std of {0,0,0,0,0,s_hat} = s_hat/sqrt(6) = 0.408 s_hat. Committed records confirm: trials_sr_std_used rises linearly with the injected edge (daily: 0.0315 floor at s <= 0.05, then 0.0360, 0.0561, 0.0764 ... 0.2389 at s = 0.60, i.e. approx 0.40 x s). Hence SR*_0 = k(50) x 0.408 x s_hat = 0.929 s_hat: the deflation benchmark is 93 percent of the witness's own realized Sharpe at every edge level. Consequences: (i) DSR at the boundary is Phi(0.07 s_hat sqrt(n_pooled - 1)), a statement about the pooled sample size (see C1), not about the shipped defaults' calibration; (ii) the boundary location is an artifact of the zero-edge field size: with 50 zero-edge agents sigma_eff would be about s_hat/sqrt(51) and the bar 0.32 s_hat, moving the onset far down. The reported "acceptance boundary near annualized Sharpe 3" (abstract; 05-experiments.tex:111; 07-limitations conclusion) is therefore a function of an arbitrary field composition plus the seed-inflated n, not a property of the protocol defaults. The paper's parenthetical "so the measured-dispersion path is active, as on every real dataset" presents the circularity as fidelity.
Fix: measure the dispersion leave-one-out (exclude the agent under test from its own sigma), or pin the configured prior for the witness experiment, or report the boundary as a function of field size; recompute after C1. State sigma_eff at each edge in fig:witness.

### MAJOR M1: Reality Check / SPA / Romano-Wolf are computed against the field mean, which the cost-bleeding luck floor drags negative
rank() defines the benchmark as the equal-weight average of all pooled streams including the five luck agents (composite.rs:1531-1537) and feeds field_excess = agent - field_mean into reality_check_pvalue, spa_pvalue, spa_consistent_pvalue and step_down_significant (composite.rs:1588-1624). Excess sums to zero across agents by construction, so the composite null "no agent beats the benchmark" is structurally violated in any heterogeneous field, and with luck agents at -0.00145/day the field mean is about -0.0009/day. Observed consequence in the committed default cells: on US-1d the step-down flags buy-and-hold, hold AND momentum as significant, where hold has identically zero returns and momentum has negative raw mean (-0.00025/day) and PSR 3.4e-6; on FX it flags hold and a negative-mean buy-and-hold. These tests as serialized measure "loses less than the average of a field containing random cost-bleeders", not skill. The paper cites the RC 0.0005 / 0.522 pair as an informative contrast (03-benchmark.tex:27) and stores step_down_significant on every record. White's RC and Hansen's SPA are defined against a fixed benchmark model (zero, risk-free, or a named reference), not the field mean. Not gate-relevant (none gates), but the reported values are not the cited tests.
Fix: use the named benchmark agent (buy-and-hold) or zero as the RC/SPA/RW benchmark, exclude luck-floor agents from the comparison set, and re-serialize; or relabel the columns as "vs field mean" and drop the citations to White/Hansen/Romano-Wolf for these fields. Also note the Romano-Wolf implementation is the non-studentized basic step-down (significance.rs:395-468); RW's recommended studentized version differs, worth one sentence (MINOR m8).

### MAJOR M2: the luck floor is a negative-drift control, so the falsification leg has almost no power
The luck agents rebalance to random weights every period through the full cost model. Result (luck-floor-1000.jsonl): on US-1d all 1,000 Sharpes lie in [-0.129, -0.113] per day (about -1.9 annualized) with std 0.0026; on crypto-1d in [-0.028, -0.014]; 0 of 2,000 agent-dataset cells has a positive raw mean. "On every dataset the luck floor's best agent has a lower raw return than the best reference agent" (05-experiments.tex:128) and "the corrected luck floor never beats a reference agent even when expanded to 1,000 agents" (05:136) are therefore near-deterministic consequences of transaction-cost drag (turnover about 100 percent per period at 2bp fee + 3bp slippage + impact), not evidence that the gates reject luck. The opening thought experiment ("one of them will look like Renaissance", 01-introduction.tex:3) is not actually simulated: the lucky tail is destroyed by costs before any gate sees it. The stated conclusions technically follow from the numbers, but the falsification leg as designed cannot fail.
Fix: add a cost-matched zero-skill null: random agents with turnover matched to buy-and-hold (e.g. random sign held for long stretches, or random weights rebalanced at the reference frequency), and/or run the 1,000-agent floor under the "none" cost profile as a diagnostic. Report that floor's tail against the gates; that is the experiment the abstract promises.

### MAJOR M3: the dispersion floor bounds, but does not close, the dissimilar-stream Sybil channel
sybil-defense-fragment.tex:7 says the precommitted floor is applied so that dissimilar low-dispersion submissions "cannot relax the default bar", and 03-benchmark.tex:25 says measurement "may tighten the bar but may not relax it", both relative to the configured prior. Correct as stated, but the residual is not stated: when the honest measured dispersion is far above the floor (the audit's own honest field measures 0.3258 pp), an adversary flooding genuinely dissimilar low-dispersion streams can pull sigma_eff from 0.3258 down to the floor (0.0315 pp at daily), reducing SR*_0 about 10x (partially offset by the N_eff growth from max(50, field size): k grows only logarithmically). The defense guarantees a floor, not monotonicity against the honest field's measurement.
Fix: state the residual explicitly in sec:incentives/sec:sybil, or make the rule one-way against the pre-attack measurement (e.g. dispersion may only be measured across verified identities, which the paper already lists as unbuilt).

### MAJOR M4: the measured sigma_trials is not the BLdP cross-trial dispersion it is plugged into
BLdP's sigma is the dispersion of Sharpe ratios across candidate strategies from a search, with the E[max] formula taken around a zero trial mean. The measured field statistic here is the spread between long-only references (about +0.04 to +0.12 pp) and cost-bleeding random agents (about -0.12 pp on daily US, -0.17 on hourly): a mean far below zero and a spread driven by cost drag, not by search luck (this is also the mechanism behind C2a). Substituting it for sigma in eq:deflation changes the meaning of "expected best of N trials": with a strongly negative trial mean the BLdP E[max] would be E[SR] + sigma k(N), i.e. much lower. The kernel uses sigma k(N) alone, which is conservative in direction but means the reported DSR is not the DSR of the cited construction.
Fix: state the zero-mean assumption beside eq:deflation; consider measuring dispersion after removing the field mean or over the non-floor agents; at minimum report the field's mean per-period Sharpe next to sigma_eff so a reader can see what population the deflation is calibrated to.

### Remaining statistical items
- Bootstrap p convention - OK with one reporting nit (MINOR m2). significance.rs:47-76: stationary bootstrap (Politis-Romano), null enforced by centering, p = (r+1)/(B+1), B = 2000, block restart prob 0.1, fixed seed. Matches 03-benchmark.tex:27 exactly, including the honest disclosure that the block length is fixed rather than n-scaled. Nit: 1/(B+1) = 0.00049975 is the resolution floor; the many "p = 0.0005" entries (abstract, tab:eligibility, sec:riskmanaged) are censored values and should be printed "p <= 0.0005". Also the null tested is "mean pooled return <= 0", i.e. positive drift, not edge over a benchmark; a bull-market beta passes it trivially, which the paper implicitly acknowledges but could state at 03:27.
- pass^k conjunction not a simultaneous confidence statement - OK. The paper's claim (03-benchmark.tex:37, 05:83, tab:relative caption) is correct: 48 marginal 0.90-level per-run tests neither compose to a 90 percent joint statement nor to 0.90^48 (the seed legs are nearly perfectly dependent for deterministic agents, so the conjunction is effectively over about 6 windows). pass_k aggregation (pass_k.rs:35-45) is a pure boolean fold; RelativeToBenchmark aggregates as All with the series changed upstream, as documented.
- DSR CI - method OK (percentile bootstrap of the DSR with fixed SR*, significance.rs:100-152) but inherits C1's inflated n, and no CI appears anywhere in the paper's tables even though B-checklist.tex:11 advertises it; tab:eligibility compares point DSRs (0.0235 "largest") with no uncertainty (MINOR m9 adjacent).
- Point estimates compared without uncertainty - MINOR m9. The witness onsets are the first passing points of a 0.05-step grid under one common-random-numbers draw: the boundary is an interval, (0.30, 0.35] weekly and (0.15, 0.20] daily, i.e. annualized (2.16, 2.52] and (2.38, 3.17], and has no replication over noise seeds. "Near an annualized Sharpe of 3" should be stated as an interval with the grid step, and ideally replicated over a few witness noise seeds (the CRN monotonicity guard in pass_witness.rs is good but guards a different failure).

---

## 5. Logical soundness of the Findings

- Finding 1 (sec:units): the general lesson and tab:units are correct and reproduce, but the internal inference and one attribution are unsound and the trap recurs in the current results: see C2 (a, b, c).
- Finding 2 (sec:passk): "zero eligible at all 576 cells", both-refusals structure, and all table cells verified. The regime-robustness half is sound and honestly scoped. The deflation half's interpretation is incomplete per C2a (annualized bars of 27 / 6.7 / 6.5 on 1h / 4h / FX are not reported) and its magnitudes per C1. The claim "the nine refusals reduce to roughly five independent panels" is a good honesty device and consistently applied.
- Finding 3 (sec:ablation): verified against the data (FX passes the 20 percent bound with wDD 0.199 and fails bootstrap and deflation; hourly momentum 0.993; crypto b&h 0.476-0.671). The multiplicative-drawdown argument (a run's DD never exceeds the pooled DD) is mathematically correct for concatenated NAV. Two notes: the pooled track whose drawdown backs the pooled cap is a fictitious concatenation across windows and seeds (window 5 of seed 0 followed by window 0 of seed 1), benign only because the default pooled cap is 1.0 (MINOR m6); and the intro's supporting claim "the reference agents drew down between 32 and 99 percent in their worst windows" (01-introduction.tex:12) is contradicted by the evidence it summarizes: FX buy-and-hold 19.9 percent and weekly US momentum 22.9 percent (MINOR m4). Fix: "between 20 and 99 percent, and all but daily FX breach the 20 percent bound".
- Finding 4 (sec:riskmanaged): all numbers reproduce, but the headline sentence "clears the bootstrap, the probabilistic Sharpe ... refused solely on deflation" is an artifact of the 8x pooled n (C1): at honest n the bootstrap p is about 0.11 and PSR about 0.89, so under the never-catastrophic verdict the agent would be refused on deflation AND the bootstrap. The verdict-conditional framing is otherwise careful.
- Witness (sec:witness): C1 + C3. The "predicate is satisfiable" existence claim survives in some form (a large enough injected edge passes), but the located boundary and the binding-gate identification do not.
- sec:luck1000 / sec:falsify: numbers reproduce; evidential force limited per M2. The internal accounting (0.0302 diagnostic vs 0 operational, 0.0455 measured ann dispersion vs 0.5 floor) is consistent and honestly separated.
- Relative verdict (sec:relative): definition, kernel, and all table rows verified; the "hold passes bear windows but can never be eligible because the edge gates run on raw returns" reasoning is correct and correctly caveated.
- Circularity check requested in the brief ("dispersion measured from a field the floor then overrides"): the floor-vs-measured logic itself is clean (max, sources stamped), but two genuine circularities exist and are the subjects of C3 (witness measures its own bar) and, mildly, of the measured path generally: each agent's own Sharpe is one of the 8 votes in the dispersion it is deflated against, and the same agent's DSR differs across committed fields (US-1d buy-and-hold: 4.78e-13 in the 8-agent sweep field, 0.0 in the 7-agent risk-managed field, 0.00211 in the 9-agent relative field). That makes the score a function of the field, which is fine and even intended, but 02-principles.tex:3 states "a score is a pure function of a submission and a configuration"; it is a pure function of a field and a configuration (MINOR m10). No silent default change was found that flips an earlier finding: the paper flags the floor's effect on the weekly scores (05:55) correctly; the unflagged item is C2c.

---

## 6. Consolidated findings list

| ID | Severity | Where | Finding | Fix |
|---|---|---|---|---|
| C1 | CRITICAL | composite.rs:1041-1083, 03-benchmark.tex:27, 05:97, 05:109-116 | Pooled PSR/DSR/bootstrap/CI computed on 8 near-duplicate seed runs; effective n inflated 8x; Finding 4's bootstrap/PSR clears and the witness boundary do not survive honest n | Pool one run per window (seeds only in pass^k) and regenerate evidence |
| C2 | CRITICAL | 05-experiments.tex:29-31, 02-principles.tex:5, tab:eligibility | Measured path re-creates the units trap (annualized bars 26.7 / 6.7 / 6.5 on 1h / 4h / FX, unreported); "PSR 1 and DSR 0 cannot both be verdicts about the data" is an invalid inference that also indicts the corrected results; weekly-0.984/daily-0.000 misattributed to the units bug (contradicted by evidence/FINDING-passk.md) | Report SR*_0 annualized per dataset; reword the inference; fix the attribution |
| C3 | CRITICAL | pass_witness.rs:37,127-146, 05:109-111 | Witness sets 93 percent of its own deflation bar (sigma_eff = 0.408 x own Sharpe); boundary is an artifact of field size 6 and pooled n | Leave-one-out or pinned-prior dispersion for the witness; report boundary vs field size |
| M1 | MAJOR | composite.rs:1531-1624, 03-benchmark.tex:27 | RC/SPA/Romano-Wolf benchmark = field mean incl. luck floor; zero-return hold and negative-mean momentum flagged significant | Benchmark against buy-and-hold or zero; exclude luck floor; relabel columns |
| M2 | MAJOR | 05:126-130, hardening-fragment.tex, luck_floor agents | Luck floor is negative-drift (all 2,000 Sharpes negative, cost drag); falsification leg has no power | Cost/turnover-matched null; cost-free diagnostic floor |
| M3 | MAJOR | sybil-defense-fragment.tex:7, 03:25 | Floor bounds but does not close dissimilar-stream Sybil; measured 0.3258 can be pulled to 0.0315 | State residual or restrict measurement to verified identities |
| M4 | MAJOR | eq:deflation, composite.rs:1388-1416 | Measured sigma is cost-drag spread with strongly negative trial mean, not BLdP search dispersion; zero-mean E[max] assumption unstated | State assumption; report field mean Sharpe beside sigma_eff |
| m1 | MINOR | 03-benchmark.tex:63,67 | tab:data US-1d 7540 and Crypto-1d 5001 are totals, caption says per symbol (true: 2513, 1000) | Correct the two cells |
| m2 | MINOR | tab:eligibility, abstract | p = 0.0005 is the 1/(B+1) censoring floor | Print p <= 0.0005 |
| m3 | MINOR | 05:22-23, make-figures.py:107 | fig:deflation caption omits sigma_trials = 0.30 pp; crossing 32-64 verified only under it | State sigma in caption |
| m4 | MINOR | 01-introduction.tex:12 | "drew down between 32 and 99 percent" contradicted by FX 19.9 and US1w momentum 22.9 | "between 20 and 99" |
| m5 | MINOR | 03-benchmark.tex:35 | "defaults ... a mandate bounding drawdown": default caps are 1.0 (vacuous); the 20 percent bound exists only in the preset | Say the default mandate is unconstrained |
| m6 | MINOR | composite.rs:1110 | Pooled max drawdown is over a fictitious concatenation across windows and seeds | One sentence, or compute per window |
| m7 | MINOR | stats.rs:64-91 | Population moments standardized by Bessel std; paper says sample skewness | Align or footnote |
| m8 | MINOR | significance.rs:395-468 | Romano-Wolf implemented non-studentized | Note the variant or studentize |
| m9 | MINOR | 05:111, fig:witness | Onset is a 0.05-grid point under one noise draw; boundary is an interval, no seed replication | Report intervals (2.16, 2.52] and (2.38, 3.17]; replicate seeds |
| m10 | MINOR | 02-principles.tex:3 | "pure function of a submission and a configuration" vs measured path (same agent: DSR 4.8e-13 / 0.0 / 0.0021 across the three committed fields) | "pure function of a field and a configuration" |
| m11 | MINOR | composite.rs doc near :192 | Doc claims a 50-bar window needs per-period Sharpe about 2.3 for PSR 0.90; correct value 0.183 pp (2.9 ann at 252) | Fix comment |

## 7. Verified-correct (OK) summary
eq:psr, eq:deflation, eq:effective-n, eq:dispersion-floor and eq:relative each match their cited source and the Rust term by term; the sqrt(P) conversion is applied exactly once and is test-pinned; kurtosis is non-excess as required; the N <= 1 convention is stated and implemented; every cell of tab:units, the 18/106 numbers, SR*_0 = 1.14, the 27 percent SE, the 2.52/3.17 annualizations, and the Sybil dilution arithmetic recompute exactly; all checked cells of tab:eligibility, tab:relative, the risk-managed and N-sensitivity series, the falsification corners and the 1,000-agent floor reproduce from the committed JSONL; the erf tail is accurate to about 1 percent at z = -7.5 so the small DSR digits are numerically supported; and the paper's own "not a simultaneous confidence statement" and "roughly five independent panels" caveats are correct.
