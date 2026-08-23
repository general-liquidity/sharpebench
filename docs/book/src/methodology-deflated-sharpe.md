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
twice nor reach a per-period statistic unconverted. Every `CompositeScore` reports
the per-period value it was actually deflated with as `trials_sr_std`, and the
annualized prior it came from as `trials_sr_std_annualized`.

When `rank` has a field of at least `min_field_for_measured_sr_std` agents it
*measures* the dispersion of per-period Sharpes across the field instead of using
the prior. That measurement is already per-period and is used as-is:
`trials_sr_std_source` reads `measured` and `trials_sr_std_annualized` is `null`,
because no annualized prior was involved.

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
> The implementation lives in `sharpebench-stats/src/deflated_sharpe.rs` (the
> per-period kernel) and `sharpebench-core/src/composite.rs` (the unit conversion
> and the gates), and is unit-tested for the "deflation penalizes many trials"
> property and for the conversion being applied exactly once.
