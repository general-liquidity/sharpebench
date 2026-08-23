# pass^k reliability

A single backtest is a coin flip you can re-toss until it lands well. SharpeBench
runs every agent across many seeds and many windows, and asks whether it clears
the per-run bar on **all** of them.

`pass^k` (after Sierra's τ²-bench reliability metric) aggregates `k` runs:

- **mode `All`**: the agent must pass on *every* run. This is the mode used for
  the eligibility gate, because a money agent that is safe-on-average is not safe.
- **mode `Any`** / **`AtLeast(m)`**: available for reporting and for non-safety
  axes.

Each run passes if its individual Probabilistic Sharpe clears `per_run_psr_bar`
(default `0.90`). Because the simulator applies **seeded** execution noise, the
same strategy produces slightly different returns under different execution
seeds, so a one-seed fluke cannot top the board, and a genuine edge shows up
everywhere.

The headline consequence: an agent that earns a spectacular return on one run and
noise on the rest **fails pass^k** and is ineligible, no matter how high its
pooled raw return. See the `lucky_high_return_fails_pass_k` test in
`sharpebench-core/src/composite.rs`.

## Units: what the per-run bar means

The per-run test is

```text
PSR(run returns, per_run_min_annual_sharpe / sqrt(periods_per_year)) >= per_run_psr_bar
```

`per_run_psr_bar` is a probability and stays one. `per_run_min_annual_sharpe` is
the **annualized** Sharpe the run's true Sharpe must exceed with that confidence;
it is converted to per period through `sharpebench_core::per_run_psr_benchmark`,
the same way the deflation prior is. The default is `0.0`, the no-edge null, under
which the test is exactly `PSR(returns, 0) >= 0.90`, the test the benchmark has
always run. An operator who wants "beats an annualized 0.5 on every run" sets
`per_run_min_annual_sharpe = 0.5`, and it means the same thing on hourly, daily
and weekly bars.

## pass^k is necessarily weak on short windows

PSR's z-statistic is `(SR - benchmark) * sqrt(n - 1)`, scaled by a skew and
kurtosis term. A per-run PSR of 0.90 against zero therefore needs
`SR * sqrt(n - 1)` of about 1.28. Converted to the annualized Sharpe a run must
show to reach the bar:

| window length | annualized Sharpe needed for PSR 0.90 (daily bars) |
|---|---|
| 78 bars | 2.32 |
| 250 bars | 1.29 |
| 1300 bars | 0.56 |

On a 1300-bar window the US stock market scrapes the bar (0.9027); on a 250-bar
window it reaches 0.71. pass^k in `All` mode therefore fails whenever any
out-of-sample window is shorter than roughly 1300 daily bars, for the market
itself and for any agent with a market-like edge.

This is a property of the statistic, not a unit error and not a bug. A short
track carries little evidence, and a gate that demands 90% confidence from little
evidence will withhold it. There are two honest responses: score longer windows,
or lower `per_run_psr_bar` knowingly and say so in the report. Lowering the bar silently, or shortening windows to make an
agent look consistent, is the kind of move the self-audit exists to catch.
