# pass^k reliability

A single backtest is a coin flip you can re-toss until it lands well. SharpeBench
runs every agent across many seeds and many windows, and asks whether it clears
the per-run bar on **all** of them.

`pass^k` (after Sierra's τ²-bench reliability metric) aggregates `k` runs:

- **mode `All`**: the agent must pass on *every* run. This is the default for
  the eligibility gate (`ScoreConfig::pass_mode`), because a money agent that is
  safe-on-average is not safe.
- **mode `Any`** / **`AtLeast(m)`**: selectable through `pass_mode` for the
  "never catastrophic" verdict described below, and for reporting on
  non-safety axes.

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

## Two verdicts: profitable in every regime, or never catastrophic in any

Every real dataset the benchmark ships contains at least one bear window. In
`All` mode, pass^k asks every window times every seed to clear per-run PSR 0.90
against zero, and a long-only agent in a bear window has a negative realised
Sharpe on that window, so it cannot clear any positive bar. Buy-and-hold, the
baseline every agent must beat, is therefore ineligible on every real dataset.
That is the correct verdict for the question `All` mode asks: "is this agent
profitable, with 90% confidence, in every regime and under every execution
seed?" A regime-dependent edge such as equity beta is not, and the benchmark
declines to certify that owning the index is safe in a bear market.

It is not the only question worth asking. The alternative is "does this agent
have an edge, and does it never blow up in any regime?" That verdict tests the
edge once, on the pooled track (Deflated Sharpe, bootstrap, process), and asks
reliability of the loss side only: no single run may draw down more than a
bound. Both verdicts are expressible from `ScoreConfig`, so the ablation is a
change of config rather than a patched binary:

| | default | `ScoreConfig::reliability_never_catastrophic(x)` |
|---|---|---|
| `pass_mode` | `All` | `Any` |
| `mandate.max_run_drawdown` | `1.0` (unconstrained) | `x` |
| certifies | profitable in every regime with 90% confidence | never draws down more than `x` in any regime, edge tested on the pooled track |
| every other gate | unchanged | unchanged |

The per-run bound is `Mandate::max_run_drawdown`, checked on each run from its
own starting equity and reported on every score as `worst_run_drawdown`. It is
distinct from `Mandate::max_drawdown`, the pooled whole-track cap, which cannot
tell a track that loses 15% in one bear window from one that loses 4% in each of
five windows. Drawdown is multiplicative, so a run's own drawdown is never above
the pooled track's; the per-run bound bites when it is set below the pooled cap,
a loose whole-track budget with a tight per-regime one.

The preset is a **weaker safety claim** than the default, and deliberately so.
It admits a regime-dependent edge, and it admits an agent whose edge lives in one
run as long as the pooled track survives deflation and the bootstrap; the
default refuses both through pass^k. It exists so the paper can show both
verdicts side by side, per asset class and timeframe. It is not the default for
a money agent, and the default has not changed: `pass_mode` deserializes to
`All` and `max_run_drawdown` to `1.0` from any config written before the fields
existed, and the golden fixtures are byte-identical on every existing value.

In JSON, `pass_mode` is `"all"`, `"any"` or `{"at_least": 3}`.
