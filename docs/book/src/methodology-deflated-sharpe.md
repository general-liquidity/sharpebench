# Deflated Sharpe & PSR

The **Probabilistic Sharpe Ratio (PSR)** is the probability that an agent's true
Sharpe exceeds a benchmark (here 0), given the observed Sharpe, the sample length,
and the return distribution's skew and kurtosis. Fat tails and negative skew,
the signatures of strategies that "work until they don't", lower the PSR for the
same headline Sharpe.

The **Deflated Sharpe Ratio (DSR)** goes further: it is the PSR evaluated against
a benchmark Sharpe that accounts for **how many strategies were tried**. Search
1000 configurations and the best one will look good by chance; the DSR subtracts
exactly that selection effect. The deflation uses three `ScoreConfig` inputs:

- `n_trials`: the multiple-testing footprint (how many agents / configs were in
  the search).
- `trials_sr_std`: the **annualized** dispersion of Sharpe ratios across those
  trials.
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
meet somewhere, and that somewhere is `periods_per_year`. A Sharpe ratio scales
with the square root of the number of periods, so a dispersion of Sharpes does
too:

```text
per-period trials_sr_std = annualized trials_sr_std / sqrt(periods_per_year)
```

The conversion lives in one function, `sharpebench_core::per_period_sr_std`, and
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

Before 0.3.0 the kernel applied `trials_sr_std = 0.5` (López de Prado's worked
example, an annualized number) directly at the period frequency. The table shows
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

With `trials_sr_std` read as annualized, the same 0.5 at fifty trials says "the
best of fifty lucky strategies looks like an annualized Sharpe of about 1.14",
which is a demanding bar and a satisfiable one. Note that an index-like track at
0.6 annualized still does not clear it, and should not: the prior states that
fifty tries at that dispersion produce a 1.14 by luck alone. Operators scoring a
field of similar strategies should use the measured path or a prior that
describes their field.

Getting `periods_per_year` wrong is the single most consequential
misconfiguration in the benchmark. Scoring hourly crypto with the daily default
makes the deflation bar about six times too demanding; scoring weekly bars with
it makes the bar about half as demanding as intended. The CLI takes
`--periods-per-year` on `run` and prints the value it used in every run header.
The shipped datasets: `us-indices-1d`, `fx-majors-1d`, `commodities-1d`,
`rates-1d` 252; `crypto-majors-1d` 365; `crypto-majors-4h` 2190;
`crypto-majors-1h` 8760; `us-indices-1w` and `crypto-majors-1w` 52.

> Bailey & López de Prado, *The Deflated Sharpe Ratio* (2014), is the reference.

## Numerical implementation of the normal functions

PSR, the deflation bar and the DSR interval evaluate the standard normal CDF
and its inverse through `sharpebench_stats::stats`: `erf` is Abramowitz and
Stegun 7.1.26 (absolute error up to about 1.5e-7), `norm_cdf` is
`0.5 * (1 + erf(x / sqrt 2))`, and `norm_ppf` is Acklam's rational
approximation (relative error about 1.2e-9). These are published closed forms,
not correctly rounded values; in particular `erf(0)` evaluates to `1e-9` rather
than `0`, which is why the committed synthetic golden fixture prints
`"psr": 0.5000000005` for a zero-Sharpe stream.

A measured replacement by `statrs` 0.19.1 (2026-09, `default-features =
false`, so only its `erf`, `Normal::cdf` and `Normal::inverse_cdf`) was
compared with the shipped bodies on 1.3 million grid points per function
(plus subnormals, exact zero, saturated tails and infinities) and on the 10,033
distinct arguments the kernel passes while scoring the two golden fields and
the tutorial fixtures:

| Function | Max absolute difference | Grid points differing in any bit | Kernel-passed arguments differing |
|---|---|---|---|
| `erf` | 1.4e-7 (near x = 0.045) | 81.6 percent | 4,432 of 10,033 |
| `norm_cdf` | 7.0e-8 (near x = 0.064) | 90.5 percent | 6,434 of 10,033 |
| `norm_ppf` | 6.8e-8 (at p = 5e-324; 2.0e-9 on the kernel's arguments) | 99.9 percent | 2 of 2 |

The kernel-passed arguments that agree are the saturated ones, where both
implementations return exactly 0 or 1. Under the replacement,
`crates/sharpebench-core/golden/example_submissions.scores.json` moves in
eight printed values (`deflation_bar_per_period`,
`deflation_bar_annualized_equivalent`, `dsr_ci_low`, `dsr_se`),
`synthetic_field.scores.json` moves (`psr` and `deflated_sharpe`), and every
producer under `paper/evidence/final/` rerun with its documented command
writes different `psr`, `deflated_sharpe` and deflation-bar values. The
tutorial reports under `examples/forecast-quality/`, the prospective forecast
report, the text board and the `arena` records are byte-identical, because
none of their printed numbers passes through these functions at printed
precision. The migration therefore cannot ship as a drop-in and is deferred to
the next evidence regeneration, when the golden fixtures and the frozen
records are rescored together. Until then
`crates/sharpebench-stats/tests/special_function_bits.rs` pins the exact bits
the three functions return, so a silent change fails there before it reaches
the golden fixtures. The moment estimators (`mean`, `variance`, `std_dev`,
`skewness`, `kurtosis`) stay hand-rolled in either case: the standardized
moments use the population normalisation fixed by the 2026-09-07 audit (R03),
and no library exposes sample skewness or kurtosis under that convention.
> The implementation lives in `sharpebench-stats/src/deflated_sharpe.rs` (the
> per-period kernel) and `sharpebench-core/src/composite.rs` (the unit conversion
> and the gates), and is unit-tested for the "deflation penalizes many trials"
> property and for the conversion being applied exactly once.
