# Literature audit of the Sharpe statistics

Date: 2026-09-10. Scope: the claims SharpeBench makes about the probabilistic
and deflated Sharpe ratios, their units and their assumptions, read against the
primary sources. The audit changed claims, documentation, one test-only unit
error and one public constructor. No golden, example or `paper/evidence/` value
moved, and the shipped defaults are unchanged.

## Sources read

Each source was read from a local PDF. Page numbers are the pages of that copy.

| Short name | Source | Copy read |
|---|---|---|
| BLdP 2014 | Bailey and López de Prado, *The Deflated Sharpe Ratio: Correcting for Selection Bias, Backtest Overfitting and Non-Normality*, J. Portfolio Management 40(5), 2014 | 22-page working paper; the paper's bibliography cites the journal version, pp. 94-107 |
| LLZ 2026 | López de Prado, Lipton and Zoonekynd, *How to Use the Sharpe Ratio*, ADIA Lab Research Paper Series No. 19, 7 March 2026 | 51 pages |
| Benhamou | Benhamou, *Distribution and Statistics of the Sharpe Ratio*, 2021, hal-03207169 | HAL PDF |
| GISW | Goetzmann, Ingersoll, Spiegel and Welch, *Portfolio Performance Manipulation and Manipulation-Proof Performance Measures*, 2006 working paper (RFS 20(5), 2007) | working paper |
| Sharpe 1994 | Sharpe, *The Sharpe Ratio*, J. Portfolio Management, Fall 1994 | web reprint without page numbers; cited by section |
| Smetters and Zhang | *A Sharper Ratio* | consulted for context; no finding below rests on it |

BLdP 2014 pp. 9-10 and LLZ 2026 pp. 9 and 11 were rendered to images and read
from the rendering, not only from extracted text, because the extracted text of
both drops the mathematics.

## Findings

Only the findings in this brief were worked. The numbering follows the brief;
gaps in it are findings outside this scope.

### F1. The 0.5 dispersion prior was attributed to a source that states a variance

**Source.** BLdP 2014 p. 9: the analyst reports "N = 100, V[{SR_n}] = 1/2,
T = 1250, γ3 = -3 and γ4 = 10". p. 10 computes
`SR_0 = sqrt(1/(2*250)) * ((1-γ) Z^-1[1 - 1/100] + γ Z^-1[1 - e^-1/100]) ≈ 0.1132`,
non-annualized at 250 observations a year, and `DSR ≈ 0.9004 < 0.95`. The same
page gives 0.9505 at N = 46 and 0.9505 for Normal returns at N = 88.

**Verified.** The stated quantity is a variance, 1/2, so its standard deviation
is `sqrt(0.5) ≈ 0.707` annualized. The kernel multiplies `trials_sr_std` as a
standard deviation (`expected_max_sharpe` in
`crates/sharpebench-stats/src/deflated_sharpe.rs`). The shipped 0.5 is
therefore less demanding than the cited example by a factor of `sqrt(2)`: at
N = 50 its annualized bar is about 1.14 where the example's dispersion gives
about 1.61. The refusal result survives: the expected-maximum bar rises with the
dispersion, DSR falls as the bar rises, so a larger prior can only refuse more
agents, and no agent clears the current bar. The pass witness's onsets are set
by pass^k, so a larger prior could only move them later.

**Changed.** The claim, everywhere this brief owns: the `ScoreConfig::trials_sr_std`
rustdoc in `composite.rs`, `paper/sections/05-experiments.tex` (Finding 1),
`paper/sections/01-introduction.tex` (first bullet),
`paper/sections/03-benchmark.tex` (the worked-example sentence), and a new
section of `docs/book/src/methodology-deflated-sharpe.md`. Each now calls 0.5 a
free modelling prior, gives the 0.707 equivalent and the `sqrt(2)` factor, and
states why the refusal survives. The value is unchanged.
`paper/evidence/FINDING-units.md` still carries the earlier attribution and was
not edited, because it is frozen; the paper and the book say so.

**Not changed, outside this brief's ownership.**
`crates/sharpebench-edge/src/verdict.rs` line 35 still describes
`DEFAULT_TRIALS_SR_STD = 0.5` as "the working value López de Prado uses in
worked examples". That crate was not assigned to this audit.

### F2. PSR and DSR assume serially independent returns

**Source.** LLZ 2026 p. 9, eqs. 2 and 3: the generalized sampling variance of
the Sharpe estimator with first-order autocorrelation ρ,
`(1/T) [ (1+ρ)/(1-ρ) - (1+ρ+ρ²)/(1-ρ²) γ3 SR + (1+ρ²)/(1-ρ²) (γ4-1)/4 SR² ]`;
p. 10, eq. 5, the same evaluated under the null. p. 6: Bailey and López de
Prado (2012) "derived closed-form formulas for the i.i.d. non-Normal case".
Benhamou, section 3.6 (PDF pp. 12-14), derives the Sharpe distribution under
AR(1) returns, and p. 3 notes that the square-root rule "is questionable as soon
as there is autocorrelation".

**Verified.** `checked_psr` and `probabilistic_sharpe_ratio` use
`1 - γ3 SR + (γ4-1)/4 SR²`, the ρ = 0 case. The stationary bootstrap reaches
`bootstrap_pvalue` and `bootstrap_dsr_ci_against_null` only, never the DSR
point estimate the gate reads. `Dataset::synthetic_parameterized`
(`crates/sharpebench-sim/src/data.rs`) builds each return as
`drift + momentum + 0.5 * shock` with `momentum = 0.9 * momentum + 0.1 * shock`,
so the synthetic returns are positively autocorrelated by construction. The
paper said only that the bootstrap preserves serial correlation.

**Changed.** The estimator is unchanged. The independence assumption, the
bootstrap's limited reach, the synthetic generator's AR(1) component and the
LLZ 2026 generalized variance as the form that would relax the assumption are
now stated in `paper/sections/03-benchmark.tex` (after eq. `eq:psr` and at the
bootstrap sentence), `paper/sections/07-limitations.tex` (new paragraph),
`docs/book/src/methodology-deflated-sharpe.md`, and the module and function
rustdoc of `deflated_sharpe.rs`.

### F3. The tail-seller attack is "demoted by process" only because the audit injects the event

**Source.** GISW, introduction and section 1: a fund that sells
out-of-the-money options can raise its measured Sharpe ratio, so the Sharpe ratio is manipulable by
option-like payoffs.

**Verified.** `ProcessEvent::TailSellingExposure` is constructed in exactly one
production place, `crates/sharpebench-core/src/selfaudit.rs`, and otherwise
only in `process.rs` unit tests. No harness, simulator, importer, WASM, npm,
Python or CLI path emits it. The wire `Order` in
`crates/sharpebench-protocol/src/lib.rs` is a symbol, an action and a target
weight in [-1, 1], so the simulator executes linear exposures only and a
harness-run agent cannot build an option book. An imported return series and a
direct caller of `rank` have no such protection.

**Changed.** `paper/sections/04-integrity.tex`: the tail-seller row of
`tab:attacks` now reads "process, audit-injected" with a table note, and the
self-audit paragraph states that the row proves the kernel's response to the
event, names the linear-only simulator as the real protection, and states that
imported series and direct `rank` callers have none. The case comment in
`selfaudit.rs` says the same. `docs/book/src/methodology-process.md` gained a
paragraph under the event table.

### F5. Square-root-of-time scaling stated without its independence condition

**Source.** Sharpe 1994, "Time Dependence": the T-period Sharpe is
`sqrt(T)` times the one-period Sharpe under the assumption that one-period
differential returns "have zero serial correlation". LLZ 2026 p. 10: "When
returns are i.i.d., the scaling factor is the square root of the number of
observations per year." The paper's `03-benchmark.tex` already gave the caveat
with Lo (2002).

**Changed.** `docs/book/src/methodology-deflated-sharpe.md` (Units section) and
the `per_period_sr_std` rustdoc in `composite.rs` now state that the scaling
holds for serially independent returns and is an approximation otherwise.

### F8. The human-baseline band hard-coded 252 periods a year

**Verified.** `HumanBaseline::skilled_trader()` in
`crates/sharpebench-core/src/percentile.rs` divided by `sqrt(252)` whatever
the scored frequency. `reference_dsr_population` did not state the unit of its
dispersion, and its tests passed `0.5`, the annualized prior, as a per-period
standard deviation, the unit error of the paper's Finding 1. No in-tree caller
uses either function; the arena windows carry an empty
`reference_dsr_population`, and no golden or evidence value depends on the band.

**Changed.** `skilled_trader(periods_per_year: f64) -> Result<HumanBaseline,
StatisticalError>` refuses a non-finite or non-positive frequency with
`InvalidParameter { name: "periods_per_year" }`. The struct, `classify_dsr` and
`reference_dsr_population` docs state that the band, `track_len` and
`trials_sr_std` are all per period at the band's frequency, and point to
`per_period_sr_std` or `CompositeScore::trials_sr_std` for the dispersion. The
tests now pass `0.5 / sqrt(252)`. New tests:
`skilled_trader_follows_periods_per_year` (52, 252, 365, 2190 and 8760 periods
re-annualize to 0.5 / 1.0 / 2.0) and `skilled_trader_periods_per_year_boundary`
(0, -0, negative, NaN and both infinities refused). The boundary test covers the
new documented domain, so `scripts/check-paired-boundaries.py` stays green
without an allowlist change. This is a breaking change to a published
constructor and needs a changelog entry at the next release.

### F9. PSR described as the probability that the true Sharpe exceeds the benchmark

**Source.** LLZ 2026 p. 11, eq. 9: `PSR = P[SR^ < SR^* | H0] = Z[z*[SR_0]] = 1 - p`.
p. 3: practitioners compute the p-value "but use it as if it represented its
Bayesian posterior"; p. 6: "Researchers often misinterpret p-values as the
probability of the null hypothesis given the evidence". BLdP 2014 p. 10 itself
uses the posterior reading ("only a 90% chance that the true SR associated with
this strategy is greater than zero"), which is where the kernel's wording came
from.

**Changed.** The frequentist statement replaces the posterior reading in the
`probabilistic_sharpe_ratio` and `deflated_sharpe_ratio` rustdoc, the opening of
`docs/book/src/methodology-deflated-sharpe.md`, the PSR definition in
`paper/sections/03-benchmark.tex`, and the per-run benchmark description in
`docs/book/src/methodology-pass-k.md`, which had the same reading.

### F11. No test pinned the worked example

**Changed.** `deflated_sharpe::tests::reproduces_the_deflated_sharpe_worked_example`
reproduces BLdP 2014 pp. 9-10 through the public `expected_max_sharpe` and
`deflated_sharpe_ratio`. The per-period dispersion is `sqrt(0.5) / sqrt(250)`
and the per-period Sharpe `2.5 / sqrt(250)`. The return series are constructed:
a two-point distribution has kurtosis exactly skewness squared plus one, so 1145
high and 105 low values give the example's moments to within the integer
constraint, and a symmetric 208 / 834 / 208 three-point series gives the Normal
moments. The test asserts each value within 5e-5 of the printed figure, and that
reading 0.5 as the standard deviation lowers the threshold by exactly `sqrt(2)`.

| Quantity | Paper | Kernel |
|---|---|---|
| SR_0 per period, N = 100 | 0.1132 | 0.1131720019 |
| SR_0 with 0.5 read as the standard deviation | (not in paper) | 0.0800246900 |
| Constructed skewness, kurtosis | -3, 10 | -2.999411, 9.996465 |
| DSR, N = 100 | 0.9004 | 0.9004052244 |
| DSR, N = 46 | 0.9505 | 0.9505080772 |
| DSR, Normal returns, N = 88 (kurtosis 3.004808) | 0.9505 | 0.9504883270 |

Mutation check. Each mutant was applied in place to the committed file, the
test was run with `cargo nextest run -p sharpebench-stats worked_example`, and
the file was restored from `git show HEAD:<path>` and confirmed identical with
`cmp`.

| Mutant | Result |
|---|---|
| Swap the Euler-Mascheroni weights in `expected_max_sharpe` | killed: SR_0 0.110728 |
| `(γ4 - 1)/4` to `(γ4 - 3)/4` in `checked_psr` | killed: DSR 0.901325 |
| `sqrt(n - 1)` to `sqrt(n)` in `checked_psr` | killed: DSR 0.900495 |
| `- γ3 SR` to `+ γ3 SR` in `checked_psr` | killed: DSR 0.981328 |
| `1 - 1/(N e)` to `1 - 1/N` in the second quantile | killed: SR_0 0.104037 |
| F8: restore the hard-coded `sqrt(252)` in `skilled_trader` | killed: both new percentile tests fail |

### F12. The bootstrap input was called excess returns

**Source.** Sharpe 1994, "The Ex Ante Sharpe Ratio": the ratio is defined on
the differential return against a benchmark.

**Verified.** `score_agent_with` in `composite.rs` passes the seed-averaged
pooled returns straight to `bootstrap_pvalue`; nothing is subtracted.

**Changed.** `docs/book/src/methodology-significance.md` now calls them the raw
pooled per-period returns, measured against a zero-rate cash benchmark, not
excess returns.

### F14. The LITE honesty verdict applied the annualized prior per period

**Found.** `is_my_sharpe_real` in `sharpebench-edge` computes a per-period
Sharpe ratio and deflated it with the 0.5 prior passed unconverted into the
per-period kernels. `HonestyConfig` had no frequency and did not state the unit
of `trials_sr_std`, and no surface converted it. At 20 trials the bar was about
an annualized Sharpe of 15 on daily bars, so almost every default verdict
failed. It is the unit error that `FINDING-units.md` records and that the
ranking path had already retired.

**Changed.** Bench PR #62. `HonestyConfig` gains `periods_per_year`, 252 when
omitted and flagged in the explanation; a non-finite or non-positive value is a
Fail with a statistics error. The prior is documented as annualized and
converted through `sharpebench_stats::per_period_from_annualized`, which the
core ranking path now shares, so the two bars agree bit for bit. The field is
exposed on the command line, WASM, npm, MCP and Python. On a four-year daily
track with an annualized Sharpe of 1.9 at 20 trials, the deflated Sharpe moves
from about 1e-153 (Fail) to 0.971 (Pass). This is a breaking change to the
verdict and is stated in the changelog. No published value contains a LITE
verdict.

### F15. The annualized prior reached per-period kernels outside the LITE verdict

**Scope.** PR #62 fixed the LITE honesty verdict, which applied the annualized
0.5 trial-dispersion prior per period. Its author reported related defects and
left them; this section records each one checked against the source on the
branch `fix/unit-defaults`, which builds on #62.

**Source.** BLdP 2014 p. 10 converts the annualized dispersion to the
frequency of the returns before it enters the expected maximum:
`SR_0 = sqrt(1/(2*250)) * (...)`, a per-period quantity at 250 observations a
year. The kernel's `expected_max_sharpe` and `deflated_sharpe_ratio` take that
per-period dispersion (their rustdoc says so), and
`sharpebench_stats::per_period_from_annualized` (added by #62) is the one
conversion, `annualized / sqrt(periods_per_year)`. LLZ 2026 p. 11, eq. 9, gives
the PSR as `1 - p`; F9 applies.

**Verified and changed.**

1. *Python raw primitives.* Confirmed: `deflated_sharpe_ratio`,
   `bootstrap_dsr_ci` and `selection_robustness` in
   `crates/sharpebench-py/src/lib.rs` defaulted `trials_sr_std` to
   `DEFAULT_TRIALS_SR_STD = 0.5` and passed it to the per-period kernels, an
   annualized dispersion of `0.5 * sqrt(252) = 7.9` on daily returns. The
   parameter stays per period, the unit of the Rust functions these bind, so
   an explicit value keeps its meaning. Omitted, it is now
   `per_period_from_annualized(0.5, periods_per_year)`, with a new keyword
   `periods_per_year` defaulting to 252 as in `ScoreConfig` and the verdict.
   The keyword converts only the default: beside an explicit `trials_sr_std` it
   is a `ValueError`, and so is a non-finite or non-positive frequency. On a
   four-year daily track at an annualized Sharpe of 1.90 the default DSR at 20
   trials moves from 0.0 to 0.971, and at 200 trials from 0.0 to 0.849; at one
   trial nothing moves. The WASM, npm and MCP surfaces expose none of these
   primitives (checked in `crates/sharpebench-wasm/src/lib.rs`,
   `npm/src/index.ts` and `npm/mcp/src/server.ts`), so the committed WASM was
   not rebuilt.
2. *A sixth instance, not in the report: `budget_curve`.*
   `BudgetCurveOpts::trials_sr_std` is documented as matching
   `ScoreConfig::trials_sr_std`, the annualized prior, with the same 0.5
   default, and `budget_curve` in `crates/sharpebench-core/src/budget_curve.rs`
   passed it to `deflated_sharpe_ratio` unconverted, although the options
   already carried `periods_per_year`. It is now converted, and a non-finite or
   non-positive `periods_per_year` is an `Err` (a negative one used to be
   clamped to zero for the annualized display). The Python `budget_curve`
   inherits the fix. On the Python test's five-point daily curve the
   selection-deflated peak moves from 0.005 to 0.875. Nothing in
   `paper/`, `examples/` or the goldens calls it.
3. *Core accepts an invalid frequency.* Confirmed: `per_period_sr_std` in
   `crates/sharpebench-core/src/composite.rs` did not validate
   `ScoreConfig::periods_per_year`. `+inf` gave a zero dispersion and scored
   against no bar; zero gave an infinite dispersion and a negative or NaN one a
   NaN, which `expected_max_sharpe` refused under the name `trials_sr_std`. On
   the measured path, which converts only the floor, a negative, NaN or
   infinite frequency was not refused at all: `f64::max` drops a NaN floor and
   an infinite frequency floors at zero, so the measured dispersion was used
   with no reason given. A malformed `trials_sr_std` is refused on the deflation
   boundary of `score_agent_with` (R02: `deflation_error`, zero deflated
   Sharpe and composite, no interval, ineligible). The frequency is now checked
   on the same boundary, with the same outcome and its own reason, and the
   selection diagnostic is withheld with it. `score_agent`,
   `score_agent_declared` and `rank` all pass through that boundary. The core
   goldens and the WASM parity goldens are unchanged.
4. *Inlined formula.* `Deflation::measured` wrote the floor's division by hand,
   and so did `per_run_psr_benchmark`. Both now call
   `per_period_from_annualized`. The body is the same expression,
   `a / b.sqrt()`, so the result is identical bit for bit; the committed
   goldens (configured path), the WASM native-parity goldens and
   `measured_dispersion_cannot_fall_below_the_precommitted_floor`, which
   compares the floor's bits with `per_period_sr_std`, pass unchanged. No
   golden exercises the measured floor, so that test and the identity of the
   expression carry the claim for it.
5. *npm NaN prior.* Confirmed: `honestyConfigJson` in `npm/src/index.ts` passed
   `trialsSrStd` through, `JSON.stringify` turned NaN and infinity into `null`,
   and `parse_honesty_config` in the WASM reads a null `trials_sr_std` as
   omitted, so the verdict used the 0.5 prior. It now throws a `RangeError`, as
   #62 does for `periodsPerYear`.
6. *Docs.* The `sharpebench-stats` crate example passed 0.5 per period and
   labelled the results "P(true Sharpe > 0)" and "P(skill survives the
   search)". It now derives `per_period_from_annualized(0.5, 252.0)`, about
   0.0315, and words PSR and DSR as one minus a p-value. The same posterior
   reading was corrected in the Python PSR, DSR and Reality Check docstrings,
   the Python README's PSR row, and the `haircut` docs of
   `sharpebench-edge::HonestyVerdict` and the npm `HonestyVerdict` type. The
   paired-boundary gate had flagged `per_period_from_annualized` since #62; a
   boundary test now pins its unvalidated edges.

**Not changed.** The LITE verdict's `Pass` explanation string still ends "the
edge survives the search", and `paper/src/essay-prose.md` still describes the
PSR as the probability that the true Sharpe exceeds a benchmark. The first is
verdict output a caller may match on; the second is paper prose outside the
package surfaces. The first was changed later, with a changelog entry, as F17. `expected_max_sharpe` keeps a required, per-period
`trials_sr_std` with no default.

**Mutation check.** Each mutant was applied in place to the committed file, the
named tests were run (Python mutants through a fresh `maturin build` and
install, npm mutants through `npm run build`), and the file was restored from
`git show HEAD:<path>` and confirmed identical with `cmp` and `git diff --quiet`.

| Mutant | Result |
|---|---|
| Core: remove the frequency refusal | killed: `+inf` scored |
| Core: refuse only non-finite frequencies | killed: zero refused as `trials_sr_std` |
| Core: refuse only non-positive frequencies | killed: `+inf` scored |
| Core: leave the DSR interval ungated | killed: interval reported |
| Core: leave the selection diagnostic ungated | killed: no `selection_error` |
| Core: measured floor multiplied by the root | killed: three measured-path tests |
| Core: per-run benchmark left annualized | killed: `default_min_annual_sharpe_is_identical_to_the_old_per_run_test` |
| Budget curve: prior unconverted | killed at 52 periods a year |
| Budget curve: no frequency refusal | killed |
| Budget curve: refuse only non-positive | killed at `+inf` |
| Python: default 0.5 per period again | killed: two tests |
| Python: accept a frequency beside an explicit dispersion | killed |
| Python: no frequency refusal | killed: all four values |
| Python: `bootstrap_dsr_ci` ignores the converted default | killed |
| Python: `selection_robustness` ignores the converted default | killed |
| npm: no `trialsSrStd` check | killed: no `RangeError` |
| npm: type check only, NaN passes | killed: no `RangeError` |

### F17. The LITE verdict's explanation read the deflated Sharpe as a probability of skill

**Source.** LLZ 2026 p. 11, eq. 9, as in F9: the deflated Sharpe is the PSR
benchmarked at the expected maximum Sharpe of `n_trials` zero-skill trials, so
it equals `1 - p` for the one-sided null that the observed Sharpe is that
maximum. It is not the probability that skill exists.

**Found.** The explanation strings of `sharpebench_edge::is_my_sharpe_real`
(`explain` in `crates/sharpebench-edge/src/verdict.rs`) read the number as a
posterior or as a verdict on luck, and the `Verdict` variant docs repeated them.
The strings joined their halves with an em dash, written here as `\u{2014}`:

| Tier | Before |
|---|---|
| Pass | `PASS: deflated Sharpe {d} clears {confidence} after pricing in {n} trial(s) \u{2014} the edge survives the search.` |
| Borderline | `BORDERLINE: deflated Sharpe {d} is between {borderline} and {confidence} over {n} trial(s) \u{2014} promising but underpowered.` |
| Fail | `FAIL: deflated Sharpe {d} is below {borderline} over {n} trial(s) \u{2014} indistinguishable from luck once the search is priced in.` |

**Changed.** Commit `045a673`. Each sentence now states the test and quotes the
p-value, which is the verdict's `haircut`:

| Tier | After |
|---|---|
| Pass | `PASS: deflated Sharpe {d} clears {confidence}: the Sharpe is significant against the expected maximum Sharpe of {n} zero-skill trial(s), one-sided p = {1-d} <= {1-confidence}.` |
| Borderline | `BORDERLINE: deflated Sharpe {d} is between {borderline} and {confidence}: against the expected maximum Sharpe of {n} zero-skill trial(s), one-sided p = {1-d} is at most {1-borderline} but above {1-confidence}.` |
| Fail | `FAIL: deflated Sharpe {d} is below {borderline}: the Sharpe is not significant against the expected maximum Sharpe of {n} zero-skill trial(s), one-sided p = {1-d} > {1-borderline}.` |

The deflated Sharpe prints to three decimals and the thresholds to two, as
before. The prefixes, the appended notes (estimated dispersion, assumed
frequency, short track), the statistics-error sentence, the tier boundaries and
every numeric field are unchanged. The `Verdict` variant docs now state each
tier as a significance level. On the four-year daily track of F14 at twenty
trials the Pass sentence reads `... one-sided p = 0.029 <= 0.05.`

Callers may have matched on the old text, so the CHANGELOG states the change
under Unreleased, Breaking. No test, snapshot, README, book page, npm, MCP or
Python document quoted the old sentences. A search for each distinctive phrase
finds only this audit, the CHANGELOG and a figure caption in
`paper/src/essay-prose.md` that uses "indistinguishable from luck" about a
figure, not about the verdict; the caption is paper prose and was left alone.

**Surfaces.** The strings are compiled into the WASM module, so
`npm/pkg/sharpebench_bg.wasm` was rebuilt with `wasm-pack` 0.15.0 (commit
`56419d1`); the rebuilt module contains the new sentence, not the old, and no
worktree path. The npm smoke test now pins the Pass sentence the shipped module
emits. The Python wheel was rebuilt with `maturin` and installed into a fresh
virtual environment: its `is_my_sharpe_real` returns the new sentence and the
binding tests pass. The command line prints the same `explanation` field.

**Tests.** `explanations_state_a_significance_level_not_a_probability_of_skill`
pins all three sentences exactly for fixed inputs, refuses `survives`, `luck`,
`probability` and `promising` in any of them, and checks that a real verdict
quotes its own `haircut` as the p-value.

**Mutation check.** Applied in place to the committed file, restored from
`git show HEAD:<path>`, confirmed identical with `cmp`.

| Mutant | Result |
|---|---|
| Edge: restore the old Pass sentence | killed: the new test failed |
| Edge: Pass quotes `1 - borderline` instead of `1 - confidence` | killed: the new test failed |
| npm: run the tests against the previously committed WASM module | killed: `isMySharpeReal converts the annualized prior by periodsPerYear` failed; the rebuilt module restored and `cmp`-verified |

**Not changed.** `hlz.rs` still ends its Fail sentence "likely a
multiple-testing artifact", and `paper/src/essay-prose.md` still describes the
PSR as a probability; both are outside this finding.

**Commands**, run in the worktree with the build directory inside it, for this
finding and F16 in [CONTRACT-PORTS.md](CONTRACT-PORTS.md#f16-evidence-inventory-gap):

| Command | Result |
|---|---|
| `cargo fmt --all --check` | exit 0 |
| `cargo clippy --all-targets --all-features -- -D warnings` | exit 0 |
| `RUSTDOCFLAGS=-Dwarnings cargo doc --workspace --exclude xtask --no-deps` | exit 0 |
| `cargo nextest run --workspace --exclude xtask` | exit 0, 1229 passed, 15 skipped |
| `wasm-pack build crates/sharpebench-wasm --target nodejs --out-dir ../../npm/pkg --out-name sharpebench` | exit 0 |
| `npm ci && npm run build && npm test` in `npm/` | exit 0, 23 passed |
| `maturin build --release` (temporary `[workspace]` table, restored and `cmp`-verified), install into a fresh venv, `pytest crates/sharpebench-py/tests` | exit 0, 94 passed |

No golden, example or `paper/evidence/` file changed.

## SharpeArena findings

The same read applied to SharpeArena found three defects of its own, repaired in
Arena PR #42.

**F6.** The leaderboard confidence code annualized at a hard-coded 252 periods
a year. `deflated_sharpe`, `bootstrap_dsr_ci` and `paired_dsr_diff` now take an
explicit `periods_per_year`; the Python bindings default it to 252, so existing
callers are unchanged.

**F7.** `expected_max_sharpe` returned a zero bar for a negative or
negative-infinite dispersion, the most favourable bar available, reachable
through the public `deflated_sharpe`. It now refuses negative, NaN and infinite
dispersion and a zero trial count with a typed error; a zero dispersion or a
single trial still means no deflation. On valid inputs the values are
bit-identical to the old estimator and to the pinned SharpeBench kernel.
Refusing a zero trial count is stricter than SharpeBench 0.19.0, which reads it
as one trial.

**F13.** The `deflated_sharpe_ci` docstring called `score_run` an older
estimator; since the 0.19.0 pin they share the same moments.

## Additional observation, not acted on

LLZ 2026 eqs. 4 and 5 evaluate the PSR standard error under the null, at
`SR = SR_0`. The kernel, following Bailey and López de Prado (2012), evaluates
it at the observed Sharpe. The two coincide when the benchmark is zero and the
returns are Normal, and differ otherwise. It is recorded here and not changed,
because changing it would move published values.

## Deferred items: implementation note

Three changes were deferred because moving the gate to them would move
published values: a serial-correlation term in the PSR variance (F2), the
standard error evaluated under the null (the observation above), and a
manipulation-proof measure (F3; `IMPLEMENTATION.md` G19). All three are now
built as **opt-in diagnostics**. The gate, eligibility, the rank predicate and
every default output are unchanged; nothing in the scoring path calls them.

**Where.** `crates/sharpebench-stats/src/opt_in_diagnostics.rs` (the
estimators), `crates/sharpebench-core/src/sharpe_diagnostics.rs` (the same
pooled track and the same two benchmarks as a board row, returned as a
separate record marked `used_by_gate: false`), and `sharpebench score
--diagnostics autocorrelated-psr,null-se-psr,mppm` in
`crates/sharpebench-cli/src/main.rs`. Documented in
[the deflated-Sharpe chapter](../../book/src/methodology-deflated-sharpe.md#opt-in-diagnostics-the-gate-does-not-use)
and the CLI reference. WASM, npm, MCP and Python were left unchanged: exposing
the diagnostics there would mean rebuilding the committed WASM module, which
the parity files pin, for a diagnostic nobody has asked for on those surfaces.

**Sources, read from the local PDFs** (LLZ 2026 pp. 9, 10, 11 and 35 to 41 and
GISW printed pp. 2, 17 and 18 were rendered to images, because the extracted
text drops the mathematics):

| Diagnostic | Formula taken from | Function |
|---|---|---|
| Autocorrelation-aware variance | LLZ 2026 eq. 2, p. 9; derivation Appendix A.1, eqs. 34 to 58, pp. 35 to 39, for an AR(1) series (eq. 44, p. 37) with `rho = Cor[x_t, x_{t+1}]` (eq. 34, p. 35) | `sharpe_variance_factor`, `first_order_autocorrelation` |
| PSR, standard error at the observed Sharpe | LLZ 2026 eq. 3, p. 9; PSR as `Z[z*] = 1 - p`, eq. 9, p. 11 | `probabilistic_sharpe_ratio_autocorrelated(.., StandardErrorAt::Observed)` |
| PSR, standard error at the benchmark | LLZ 2026 eqs. 4 and 5, p. 10 | `probabilistic_sharpe_ratio_autocorrelated(.., StandardErrorAt::Benchmark)`, `sharpe_standard_error_autocorrelated` |
| MPPM | GISW working paper eq. 18, printed p. 18 (PDF p. 20), also eq. 1, printed p. 2; risk aversion 3, printed p. 18; concavity and the geometric average as the `rho = 1` case, printed p. 17 | `manipulation_proof_performance`, `DEFAULT_MPPM_RISK_AVERSION` |

Two conventions were chosen and are stated in the rustdoc. The PSR's z
statistic keeps the kernel's `sqrt(T - 1)` (Bailey and López de Prado 2012)
where LLZ 2026 writes `1/T` inside the variance, so that at `rho = 0` the
diagnostic is the kernel's PSR bit for bit; the two differ by
`sqrt(T / (T - 1))`. A negative variance bracket, reachable only with a
strongly negative `rho` and skewed returns or with moments that violate the
Pearson inequality, is refused rather than floored.

**Correction to the observation above.** It says the two evaluations
"coincide when the benchmark is zero and the returns are Normal". They do not.
With Normal returns the bracket at the observed Sharpe is `1 + SR^2 / 2` and at
a zero benchmark it is `1`, so they coincide only at `SR = 0`; in general they
coincide exactly when the observed Sharpe equals the benchmark, where both
evaluations put the same Sharpe into the same variance.
`a_zero_benchmark_does_not_make_the_evaluations_coincide` and
`null_and_observed_standard_errors_coincide_at_the_benchmark` pin both
statements. The conclusion of the observation, that switching would move
published values, stands.

**Numerical checks.**

| Check | Source value | Implementation |
|---|---|---|
| LLZ worked example, `sigma[SR*]` with `(0.036%, 0.079%, -2.448, 10.164, 0.2, 24)` | 0.379 (p. 9) | 0.3794899975 |
| Same, i.i.d. Normal | 0.214, "approximately 43% smaller" (p. 9) | 0.2144595450, 43.5% smaller |
| PSR at `SR_0 = 0` (standard error 0.25) | 0.966 (p. 11) | 0.9658320054 |
| PSR at `SR_0 = 0.1` (standard error 0.2769641348) | 0.900 (p. 11) | 0.9004759174 |
| `rho = 0`, observed Sharpe, against the kernel's PSR on seven series and five benchmarks, and against its DSR | identical | bit for bit |
| Lag-one autocorrelation, bracket and eight PSRs on a 300-point autocorrelated series, against an independent Python implementation | rho 0.3671387205, bracket 2.1614557840 | all within 1e-12 |
| Normal AR(1) bracket at `SR = 0`, `rho = 0.5` | `(1 + rho)/(1 - rho) = 3` (eq. 60, p. 40) | 3 exactly |
| MPPM of a riskless stream earning `c` | `ln(1 + c) / dt` at every risk aversion | within 1e-12 for four streams, six risk aversions |
| MPPM of log returns `m +/- s` | `m + ln(cosh((1 - rho) s)) / (1 - rho)` | within 1e-14 |
| MPPM on the 300-point series, risk aversion 3, 1 and 2, against Python | 0.1055143057, 0.0004765888, 0.0053717808 | within 1e-12 |
| Short-volatility stream: 1.5% in 99 periods, -50% in one, against +5.8% / -4.2% | Sharpe 0.191 against 0.159, higher mean | MPPM lower at risk aversion 2, 3 and 4 (per period -0.00048 against 0.00428 at 3) |

The Python reference was written from the papers with the `math` module
(moments with the kernel's normalization, eq. 2 and eq. 18 summed directly);
the PSR comparison uses the kernel's frozen Abramowitz-Stegun CDF on both sides,
and the worked-example PSRs use the exact Normal CDF the paper uses.

**Byte identity of default outputs.** The `sharpebench` binary built from
`origin/main` (`ce2691b`, extracted with `git archive`) and the branch binary
were run on the same inputs, comparing exit code, stdout and stderr byte for
byte: `score` on `suites/example_submissions.json`,
`crates/sharpebench-core/golden/synthetic_field.input.json` and a three-agent
autocorrelated field, each plain, `--json`, `--rank-mode
lifecycle-certified/v1 --json`, `--periods-per-year 52 --json`, `--pass-mode
any` and `--execution-seeds-per-window 1 --json`, plus `run`, `run --json`,
`audit --json`, `stress --json` and a missing input file. All 23 are identical.
The only differences are `--help` and the `score` usage line, which name the new
flag. No golden, example, snapshot, WASM parity file or `paper/evidence/` file
is in the diff, and the full workspace suite passes against them unchanged.

**Tests.** `crates/sharpebench-stats/tests/opt_in_diagnostics.rs` (17, five
of them paired-boundary tests, one per new function with a documented domain,
so `scripts/check-paired-boundaries.py` stays green without an allowlist
change: 37 candidates, 13 covered), four in
`sharpebench_core::sharpe_diagnostics::tests`, and five in
`crates/sharpebench-cli/tests/sharpe_diagnostics_cli.rs`.

**Mutation check.** Each mutant was applied in place to the committed file,
the named suite was run, and the file was restored from `git show HEAD:<path>`
and confirmed identical with `cmp` and `git diff --quiet`.

| Mutant | Result |
|---|---|
| Eq. 2 first weight `(1+rho)/(1-rho)` to 1 | killed: worked example, Python cross-check, eq. 2 weights |
| Eq. 2 second weight drops its `rho` term | killed: same three |
| Eq. 2 third weight `(1+rho^2)` to 1 | killed: same three |
| Skewness term sign flipped | killed: worked example, Python cross-check, two boundary tests |
| Autocorrelation estimate scaled by `T/(T-1)` | killed: Python cross-check, boundary test |
| `rho` domain admits -1 and 1 | killed: two boundary tests |
| Negative bracket floored instead of refused | killed: two boundary tests |
| z statistic scaled by `sqrt(T)` | killed: Python cross-check, bit-identity reduction |
| Benchmark evaluation uses the observed Sharpe | killed: four tests |
| Observed evaluation uses the benchmark | killed: four tests, including the bit-identity reduction |
| Standard error drops `1/(T-1)` | killed: two tests |
| MPPM drops `1/(1 - rho)` | killed: four tests |
| MPPM uses periods per year as `dt` | killed: three tests |
| MPPM exponent `rho - 1` | killed: four tests, including the short-volatility test |
| MPPM log-sum-exp shift dropped | killed: four tests |
| MPPM `rho = 1` branch not annualized | killed: two tests |
| MPPM admits a gross return of zero | killed: boundary test |
| MPPM admits a risk aversion of zero | killed: boundary test |
| MPPM returns -0.0 on a flat track | killed: riskless-stream test |
| Core skips the shared-cell restriction | killed: `diagnostics_follow_the_shared_cell_restriction` |
| Core tests the deflation-bar counterpart against zero | killed |
| Core evaluates `autocorrelated-psr` under the null | killed |
| Core evaluates `null-se-psr` at the observed Sharpe | killed |
| CLI prints diagnostics without the flag | killed: three CLI tests |
| CLI ignores an unknown identifier | killed |

**Commands**, run in the worktree:

| Command | Result |
|---|---|
| `cargo fmt --all --check` | exit 0 |
| `cargo clippy --all-targets --all-features -- -D warnings` | exit 0 |
| `RUSTDOCFLAGS=-Dwarnings cargo doc --workspace --exclude xtask --no-deps` | exit 0 |
| `cargo nextest run --workspace --exclude xtask` | exit 0, 1277 passed, 15 skipped |
| `python scripts/check-paired-boundaries.py` | exit 0 |

**What would make them gating.** Each would move published values and belongs
with the next evidence regeneration, not a code change: positive
autocorrelation widens the variance unless strong positive skewness offsets
it, so it lowers the PSR and DSR of a Sharpe above its benchmark on
momentum-like tracks, the simulator's synthetic returns among them; the null
evaluation moves the PSR of every track whose Sharpe differs from the
benchmark; and the MPPM is a different statistic whose use as a gate would need
its own threshold.

## Paper build

`paper/main.pdf` was rebuilt with TeX Live 2026 (`pdflatex`, `bibtex`,
`pdflatex`, `pdflatex`, all exit 0): 61 pages, no undefined references or
citations, no overfull boxes. Two entries were added to `paper/refs.bib`:
`lopezdeprado2026howto` and `goetzmann2007manipulation`.
