# Deflated Sharpe & PSR

The **Probabilistic Sharpe Ratio (PSR)** is one minus the one-sided p-value of
the test that an agent's Sharpe is no better than a benchmark (here 0): the
probability of observing a Sharpe below the one observed if the true Sharpe
were exactly the benchmark, given the sample length and the return
distribution's skew and kurtosis. It is **not** the probability that the true
Sharpe exceeds the benchmark. That would be a posterior, it needs a prior, and
López de Prado, Lipton and Zoonekynd, *How to Use the Sharpe Ratio* (2026),
single out reading a p-value that way as a recurring error (their eq. 9
defines PSR as `1 - p`). Fat tails and negative skew, the signatures of
strategies that "work until they don't", lower the PSR for the same headline
Sharpe.

The PSR variance the kernel uses is the 2014 one, and it **assumes serially
independent returns**. Positive autocorrelation makes the true sampling variance
of a Sharpe estimate larger, so on autocorrelated returns the PSR and DSR point
estimates are too favorable. The stationary bootstrap preserves serial
correlation, but only in the bootstrap p-value and the DSR interval, never in
the point estimate the gate reads. The frozen datasets show volatility
clustering, and the simulator's synthetic generator adds an AR(1) momentum
component, so its returns are autocorrelated by construction. The 2026 paper's
generalized variance (its eqs. 2, 3 and 5), which adds a first-order
autocorrelation term, is the form that would relax the assumption. It is
available as an opt-in diagnostic (see
[below](#opt-in-diagnostics-the-gate-does-not-use)); the gate keeps the 2014
variance.

The **Deflated Sharpe Ratio (DSR)** goes further: it is the PSR evaluated against
a benchmark Sharpe that accounts for **how many strategies were tried**. Search
1000 configurations and the best one will look good by chance; the DSR subtracts
exactly that selection effect. The deflation uses three `ScoreConfig` inputs:

- `n_trials`: the multiple-testing footprint (how many agents / configs were in
  the search).
- `trials_sr_std`: the **annualized** standard deviation of Sharpe ratios
  across those trials (a standard deviation, not a variance).
- `periods_per_year`: how many return bars make a year on the dataset being
  scored, which is what converts the annualized dispersion into the units the
  statistic is computed in.

An agent clears the gate only when `DSR >= dsr_bar` (default `0.95`): its edge has
to be likely-real *after* paying for the size of the search that found it.

This is the single most important property of SharpeBench. It is why a lucky
agent with the highest raw return is demoted: deflation prices in the luck.

## Units: the kernel is per-period, the thresholds are annualized

Every Sharpe ratio inside the kernel is computed on **per-period** returns. The
kernel never annualizes, because annualizing a short track multiplies exactly the
noise PSR and DSR exist to expose. That is correct and it does not change.

The thresholds an operator reasons about are quoted **annualized**, because that
is the unit the literature and every published track record use. The two have to
meet somewhere, and that somewhere is `periods_per_year`. For serially
independent returns a Sharpe ratio scales with the square root of the number of
periods, so a dispersion of Sharpes does too:

```text
per-period trials_sr_std = annualized trials_sr_std / sqrt(periods_per_year)
```

Under autocorrelation the scaling factor differs (Lo 2002), so the converted
prior is an approximation on the real datasets. The conversion lives in one
function, `sharpebench_core::per_period_sr_std`, and
every deflation call site reads from it, so the prior can neither be converted
twice nor reach a per-period statistic unconverted. Every `CompositeScore`
reports the per-period value it was actually deflated with, its annualized
equivalent, the configured/measured/measured-floored source, and the resulting
deflation bar in both units.

Execution seeds are Monte-Carlo replicates conditional on one market path, not
additional history. Before PSR, DSR, and the stationary bootstrap are computed,
the scorer averages aligned seed returns within each window and concatenates
the windows. Eight executions of a 409-bar window therefore contribute 409
temporally distinct observations, not 3,272 pseudo-independent ones. Incomplete
or unequal seed blocks are rejected rather than truncated.

When `rank` has a field of at least `min_field_for_measured_sr_std` agents it
*measures* the dispersion of per-period Sharpes across the field instead of using
the prior. That measurement is already per-period and is used as-is;
`trials_sr_std_source` reads `measured`, while
`trials_sr_std_annualized_equivalent` reports the same quantity multiplied by
the square root of periods per year for interpretation. Before measuring,
near-clone streams
(pooled returns whose `|cosine|` reaches `CLONE_COLLAPSE_COSINE`, 0.995, a
stricter constant than the rediscovery screen's 0.97 so that honest collinear
agents keep their vote) are collapsed to one vote per cluster, for the estimate
and for the field count the floor is checked against, so a flood of
near-duplicate submissions cannot shrink the dispersion and lower the bar
(`dedup_clones_for_measured_sr_std`, default on; see the integrity chapter).

### Why this matters: the bar before 0.3.0

Before 0.3.0 the kernel applied `trials_sr_std = 0.5`, an annualized number,
directly at the period frequency. The table shows
the annualized Sharpe an agent had to beat on each shipped timeframe at
`n_trials = 50`, reconstructed from the sweep in `paper/evidence/FINDING-units.md`:

| trials_sr_std | per-period sr_star | 1h (8760/yr) | 4h (2190/yr) | 1d (252/yr) | 1w (52/yr) |
|---|---|---|---|---|---|
| 0.070 (measured) | 0.159 | 14.9 | 7.5 | 2.5 | 1.1 |
| 0.200 | 0.455 | 42.6 | 21.3 | 7.2 | 3.3 |
| 0.350 | 0.797 | 74.6 | 37.3 | 12.6 | 5.7 |
| 0.500 (old default) | 1.138 | 106.5 | 53.3 | 18.1 | 8.2 |

SPX buy-and-hold is roughly 0.5 to 0.9 annualized. Renaissance Medallion is cited
near 2 to 3. Under the old default a daily strategy needed an annualized Sharpe of
18 and an hourly one needed 106; buy-and-hold on the US indices posted
`PSR = 1.0000` and `DSR = 0.0000` on the same series, and zero agents were ever
rank-eligible on any real dataset. The bar was not high, it was unreachable, and
it got more unreachable with the square root of the number of periods per year.

### Where 0.5 comes from: a free prior, not the cited example

The 0.5 default is a free modelling prior. It used to be described as López de
Prado's worked example, and `paper/evidence/FINDING-units.md`, a frozen record,
still carries that wording. The worked example of Bailey and López de Prado
(2014, pp. 9-10 of the working paper) states the cross-trial **variance**,
`V[{SR_n}] = 1/2` annualized, and computes its threshold with
`sqrt(1 / (2 * 250))`, so its dispersion is a standard deviation of
`sqrt(0.5)`, about 0.707. The kernel multiplies `trials_sr_std` as a standard
deviation, so the shipped 0.5 is **less demanding than the cited example by a
factor of sqrt(2)**: at fifty trials it sets an annualized bar of about 1.14,
where the example's dispersion would set about 1.61.

The value is not changed. Published evidence is frozen, and a prior is a stated
choice rather than a bug. The paper's refusal result survives the correction:
the expected-maximum bar rises with the dispersion and the DSR falls as the bar
rises, so a higher prior only refuses more agents, and no agent clears the
current one. The test
`deflated_sharpe::tests::reproduces_the_deflated_sharpe_worked_example` in
`sharpebench-stats` reproduces the example's printed threshold (0.1132 per
period) and deflated Sharpe (0.9004, then 0.9505 at N = 46 and 0.9505 for Normal
returns at N = 88) from the kernel's own formula.

With `trials_sr_std` read as annualized, the same 0.5 at fifty trials says "the
best of fifty lucky strategies looks like an annualized Sharpe of about 1.14",
which is a demanding bar and a satisfiable one. Note that an index-like track at
0.6 annualized still does not clear it, and should not: the prior states that
fifty tries at that dispersion produce a 1.14 by luck alone. Operators scoring a
field of similar strategies should use the measured path or a prior that
describes their field.

The one-series honesty verdict (`sharpebench check`, `is_my_sharpe_real` in
Rust, Python, npm and MCP) uses the same units. Its `trials_sr_std` is
annualized, its `periods_per_year` converts it through the same function, and an
omitted `periods_per_year` is 252 and named in the verdict's explanation. Until
the release after 0.19.0 the verdict applied the annualized prior per period
unconverted, so its bar was the unreachable one in the table above. Its
`sr_benchmark`, the benchmark of the PSR and MinTRL it reports (default 0), is
annualized too and converted through the same function; through 0.21.0 it was
read per period, so a benchmark meant as an annualized 1.0 on daily bars was a
bar of about 15.9 annualized. Zero
is zero in every unit, so default verdicts did not change. The raw
`probabilistic_sharpe_ratio` and `min_track_record_length` primitives take the
benchmark per period.

The budget curve (`sharpebench_core::budget_curve`, Python `budget_curve`)
takes the same annualized `trials_sr_std` and converts it with its own
`periods_per_year`; until the release after 0.19.0 it too deflated with the
prior unconverted. The raw Python primitives `deflated_sharpe_ratio`,
`bootstrap_dsr_ci` and `selection_robustness` take `trials_sr_std` per period,
the unit of the Rust functions they bind, and use an explicit value as given.
Omitted, it is `0.5 / sqrt(periods_per_year)` with `periods_per_year` defaulting
to 252, about 0.0315 per period; until the same release the omitted value was
0.5 per period. A frequency that is not finite and positive is refused on every
one of these surfaces and on the `ScoreConfig` scoring path, because an
infinite one divides the prior to a zero bar.

Getting `periods_per_year` wrong is the single most consequential
misconfiguration in the benchmark. Scoring hourly crypto with the daily default
makes the deflation bar about six times too demanding; scoring weekly bars with
it makes the bar about half as demanding as intended. The CLI takes
`--periods-per-year` on `run` and prints the value it used in every run header.
The shipped datasets: `us-indices-1d`, `fx-majors-1d`, `commodities-1d`,
`rates-1d` 252; `crypto-majors-1d` 365; `crypto-majors-4h` 2190;
`crypto-majors-1h` 8760; `us-indices-1w` and `crypto-majors-1w` 52.

> Bailey & López de Prado, *The Deflated Sharpe Ratio* (2014), is the reference.

## Opt-in diagnostics the gate does not use

Three estimators from the literature audit are implemented as diagnostics a
caller has to ask for. **None of them is read by the gate, by eligibility or by
the rank**, and none is a field of `CompositeScore`: switching the gate to any
of them would move published values, so they sit beside the board instead.
They are library functions in `sharpebench_stats::opt_in_diagnostics` and a
flag on the command line:

```text
sharpebench score field.json --diagnostics autocorrelated-psr,null-se-psr,mppm [--json]
```

Without `--diagnostics` the output is the board and nothing else, byte for byte
as before the flag existed. With it, the human table gains a separate block
after the board, and `--json` prints an object whose `board` member is the
board-only output and whose `sharpe_diagnostics` array carries one record per
row, each marked `"used_by_gate": false`. The diagnostics are computed by
`sharpebench_core::sharpe_diagnostics` on the same pooled track the row's PSR
and DSR read (shared cells, execution seeds averaged), against the same two
benchmarks: zero, the counterpart of `psr`, and the row's
`deflation_bar_per_period`, the counterpart of `deflated_sharpe`. An
input a diagnostic cannot score is reported with its reason and no number.

| Identifier | What it is | Source | What differs from the board |
|---|---|---|---|
| `autocorrelated-psr` | PSR with the pooled track's lag-one autocorrelation `rho` in the Sharpe variance, standard error at the observed Sharpe | López de Prado, Lipton and Zoonekynd (2026), eq. 2 and eq. 3, p. 9; `rho = Cor[x_t, x_{t+1}]`, eq. 34, p. 35 | only the autocorrelation weights |
| `null-se-psr` | PSR with the standard error evaluated at the benchmark, serial independence kept | the same paper, eqs. 4 and 5, p. 10 | only the Sharpe at which the variance is evaluated |
| `mppm` | Manipulation-proof performance measure, risk aversion 3, zero risk-free rate, annualized | Goetzmann, Ingersoll, Spiegel and Welch (2007), working paper eq. 18, printed p. 18 | a different statistic, a certainty equivalent rather than a test |

**Autocorrelation-aware PSR.** The variance bracket is

```text
(1+rho)/(1-rho) - (1+rho+rho^2)/(1-rho^2) g3 SR + (1+rho^2)/(1-rho^2) (g4-1)/4 SR^2
```

which is the kernel's `1 - g3 SR + (g4-1)/4 SR^2` at `rho = 0`. The paper
derives it for a stationary AR(1) series (Appendix A.1). Positive
autocorrelation raises every weight; unless strong positive skewness offsets
the first and third terms it widens the variance, so for a Sharpe above its
benchmark on momentum-like returns, such as the simulator's, the diagnostic is
below the board's PSR. The z
statistic keeps the kernel's `sqrt(T - 1)` where the paper writes `1/T` inside
the variance, so that at `rho = 0` the function returns the kernel's PSR, and
at the deflation bar its DSR, bit for bit (a test pins both). The paper's
worked example (p. 9, two years of monthly returns with skewness -2.448,
kurtosis 10.164 and `rho = 0.2`) is reproduced from the same bracket: standard
error 0.3795 against the printed 0.379, 0.2145 for i.i.d. Normal returns
against 0.214, and PSR 0.9658 and 0.9005 at benchmarks 0 and 0.1 against the
printed 0.966 and 0.900 (p. 11). A strongly negative `rho` with skewed returns
can drive the bracket below zero; that is refused, not floored, because a
floored variance would turn a failed approximation into a PSR of 0 or 1. `rho`
is estimated on the pooled track, so the few pairs that straddle a window
boundary enter it.

**Standard error under the null.** The kernel, following Bailey and López de
Prado (2012), evaluates the variance at the observed Sharpe; the 2026 paper
evaluates it at the benchmark, the least favorable point of the null. The two
coincide exactly when the observed Sharpe equals the benchmark. They do **not**
coincide at a zero benchmark with Normal returns: there the null bracket is 1
and the observed one `1 + SR^2 / 2`, so for a positive Sharpe the null-evaluated
PSR is the higher of the two. Both facts are pinned by tests.

**Manipulation-proof performance measure.**

```text
Theta = 1 / ((1 - rho) dt) * ln( (1/T) sum_t (1 + x_t)^(1 - rho) ),   dt = 1 / periods_per_year
```

with the per-period risk-free rate at zero, the benchmark's cash convention,
and `rho = 3`, the risk aversion the authors use and describe as consistent
with the market portfolio (they report 2 to 4 as the plausible range). `Theta`
is the annualized continuously compounded certainty equivalent: a riskless
stream earning `c` a period scores `ln(1 + c) * periods_per_year` at every risk
aversion. Unlike the Sharpe ratio it cannot be raised by selling tail risk. A
test builds a stream that collects 1.5% in 99 periods and loses 50% in one: its
Sharpe (0.191) and mean beat a symmetric +5.8% / -4.2% stream (Sharpe 0.159),
and its MPPM is lower at risk aversion 2, 3 and 4 (per period -0.00048 against
0.00428 at 3). The measure is the one defense against option-like payoffs that
an imported return series has: the simulator only executes linear exposures,
but nothing checks an imported series. It is reported, not gated. A return at
or below -1 is outside its domain and refused.

Exposure on the other surfaces: the WASM module, the npm package, the MCP tools
and the Python binding are unchanged and do not expose these diagnostics; the
committed WASM was not rebuilt.

## Numerical implementation of the normal functions

PSR, the deflation bar and the DSR interval evaluate the standard normal CDF
and its inverse through `sharpebench_stats::stats`: `erf` is Abramowitz and
Stegun 7.1.26 (absolute error up to about 1.5e-7), `norm_cdf` is
`0.5 * (1 + erf(x / sqrt 2))`, and `norm_ppf` is Acklam's rational
approximation (relative error about 1.2e-9). These are published closed forms,
not correctly rounded values; in particular `erf(0)` evaluates to `1e-9` rather
than `0`, which is why the committed synthetic golden fixture prints
`"psr": 0.5000000005` for a zero-Sharpe stream.

A replacement by `statrs` 0.19.1 (`default-features = false`, so only its
`erf`, `erfc` and `erfc_inv`) was measured against a 60-digit `mpmath`
reference on 1,918,979 whole-domain grid points and on the 178,472 distinct
arguments per function that the kernel evaluates while scoring the two golden
fields and a full evidence sweep. The measurement was then **acted on by
rejecting the migration**, and both halves of that are worth stating.

`statrs` is more accurate almost everywhere. On the kernel's own arguments:

| Function | Shipped max absolute error | `statrs` max absolute error |
|---|---|---|
| `erf` | 1.394e-7 | 4.939e-11 |
| `norm_cdf` | 6.969e-8 | 2.470e-11 |
| `norm_ppf` | 2.895e-9 | 5.593e-16 |

It is closer to the reference on 811,411 of 857,622 `erf` arguments, 604,528 of
857,608 normal-CDF arguments and 203,738 of 203,746 inverse arguments, and it is
never worse on the latter two. Neither implementation is correctly rounded:
`statrs`'s `erf` still carries about 4.9e-11 near `x = 0.5`, so the substitution
buys three to seven orders of magnitude rather than correctness to the last bit.

It is rejected on a property the accuracy work did not measure: **reproducibility
across the three supported targets.** The migration was implemented and put
through CI so the question would be answered by evidence. The two code goldens,
regenerated on one platform, reproduced there and failed on the other two, while
the same three jobs on the hand-rolled bodies pass on every target. Neither
implementation uses a fused multiply-add, so this is not the usual contraction
difference; both call the platform exponential and logarithm, and what differs is
the arguments they pass. The Abramowitz and Stegun form evaluates a single
exponential of negative x squared, and the three platform math libraries agree on
that at every argument this kernel evaluates. The `statrs` rational path does not.
That agreement is an empirical property of three vendors' libraries rather than a
design guarantee.

The deciding argument is what the benchmark promises. A committed field rescored
anywhere should reproduce byte for byte. A disclosed, bounded 1.4e-7 that is
identical on every supported target is compatible with that promise; a 4.9e-11
that varies by target is not, because it makes two honest operators disagree
about the same submission. The hand-rolled error is orders of magnitude below
every bar the kernel tests against, so nothing in the published results turns on
it.

Had the substitution shipped, it would have moved printed values in
`crates/sharpebench-core/golden/example_submissions.scores.json` and
`synthetic_field.scores.json`, and every producer under `paper/evidence/final/`
rerun with its documented command would have written different `psr`,
`deflated_sharpe` and deflation-bar values. The tutorial reports under
`examples/forecast-quality/`, the prospective forecast report, the text board
and the `arena` records are byte-identical either way, because none of their
printed numbers passes through these functions at printed precision.

What stays in place: the hand-rolled bodies unchanged, and
`crates/sharpebench-stats/tests/special_function_bits.rs`, which pins the exact
bits the three functions return so a silent change fails there before it reaches
the golden fixtures. What would reopen the question: an implementation that is
both closer to the reference and bit-identical across the three targets, such as
a vendored correctly-rounded routine restricted to operations IEEE-754 defines
exactly, or a build that forces one deterministic math library on every target
and demonstrates it in the three-platform job. If the paper's numerical evidence
is ever regenerated wholesale, the reproducibility baseline is re-established
from scratch and the question should be reopened then.

The moment estimators (`mean`, `variance`, `std_dev`,
`skewness`, `kurtosis`) stay hand-rolled in either case: the standardized
moments use the population normalisation fixed by the 2026-09-07 audit (R03),
and the proposed special-function substitution does not replace those empirical
moment definitions. This is a scoped implementation choice, not a claim that
no numerical library can compute population-normalized moments.
> The implementation lives in `sharpebench-stats/src/deflated_sharpe.rs` (the
> per-period kernel) and `sharpebench-core/src/composite.rs` (the unit conversion
> and the gates), and is unit-tested for the "deflation penalizes many trials"
> property and for the conversion being applied exactly once.
