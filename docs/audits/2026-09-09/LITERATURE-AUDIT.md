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

## Additional observation, not acted on

LLZ 2026 eqs. 4 and 5 evaluate the PSR standard error under the null, at
`SR = SR_0`. The kernel, following Bailey and López de Prado (2012), evaluates
it at the observed Sharpe. The two coincide when the benchmark is zero and the
returns are Normal, and differ otherwise. It is recorded here and not changed,
because changing it would move published values.

## Paper build

`paper/main.pdf` was rebuilt with TeX Live 2026 (`pdflatex`, `bibtex`,
`pdflatex`, `pdflatex`, all exit 0): 61 pages, no undefined references or
citations, no overfull boxes. Two entries were added to `paper/refs.bib`:
`lopezdeprado2026howto` and `goetzmann2007manipulation`.
