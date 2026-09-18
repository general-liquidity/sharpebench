# Volatility-response sizing

Two agents can earn similar returns on a calm window while doing very
different things with risk. One keeps its exposure fixed whatever the market
does; the other cuts exposure when volatility rises. The board columns and
gates read returns, so the two look alike until a volatile stretch arrives
and the first one takes the loss. No other SharpeBench report relates an
agent's exposure to the volatility it faced.

The gap is documented in production. DX Research Group's six-month record of
LLM trading agents (*What LLM Trading Agents Actually Do in Production*, 2026,
arXiv:2609.05663, section 5.1 and Table 3) measured each entry's volatility as
the standard deviation of the prior 24 hourly returns, split 6,400 closed
positions into sextiles, and found a median leverage of 5.0x in every sextile
across a 5.7x volatility spread, with a Spearman correlation between
volatility and leverage of -0.001.

`sharpebench_sim::sizing_response` is the matching diagnostic for a recorded
SharpeBench run. It is a reported diagnostic: no gate, eligibility rule or rank
reads it, and every report carries `"used_by_gate": false`.

## What it computes

The diagnostic replays the trajectory's recorded decisions through
`TradingEnv`, the open-loop face of the engine that runs the same per-step
body as `run_backtest`, so the book it reads is the book the run was scored
on. No agent is called. For every bar `t` of every run it forms one pair:

- **Post-fill gross exposure.** The sum over symbols of `|shares * close(t)|`
  for the holdings after the bar's fills, divided by the NAV at `close(t)`
  before those fills. That NAV is the base the engine sizes target weights
  against, so a filled target of weight `w` reads as `|w|` and a short counts
  at its absolute value. Dividing by the NAV after the fills would make a
  fully invested book read above 1x by the bar's own trading costs.
- **Trailing realized volatility.** For each symbol, the sample standard
  deviation of the `vol_lookback` simple returns ending at bar `t`, computed
  from closes `t - vol_lookback` through `t` and never from a later close. The
  bar's figure is the mean across the dataset's symbols.

Over the pairs pooled across every run it reports:

- `rank_correlation.spearman_rho`: Spearman's rank correlation between
  exposure and trailing volatility, computed with the tie-correct Spearman
  in `sharpebench-stats`. Negative means the agent sized down as volatility
  rose; near zero means sizing ignored volatility; positive means it sized up;
- `by_volatility.quintiles`: for each volatility quintile (1 is the calmest),
  the number of pairs, the median volatility and the median gross exposure.
  Quintiles are assigned by the midrank of the volatility, so tied
  volatilities always share a quintile and counts can differ;
- `census`: bars replayed, bars paired, and the bars that could not be
  paired, by reason: before a full volatility window, a non-positive or
  non-finite close in the window, or a pre-fill NAV that is not positive (or
  an exposure that is not finite);
- `config`: the declared `vol_lookback` (default 20 returns),
  `exposure_resolution` (default 0.01 of NAV) and `min_pairs` (default 30).

## Unavailable figures

A figure that cannot mean what it says is absent and replaced by a typed
reason, never by a number. When several apply, a figure reports the first
in this order:

| Reason | When |
|---|---|
| `too_few_pairs` | Fewer pairs than `min_pairs`. Applies to both figures. |
| `constant_exposure` | The pooled exposure spans no more than `exposure_resolution`. The reason carries the smallest and largest exposure. Applies to the correlation only. |
| `constant_volatility` | Every pair faced the same trailing volatility, up to a relative spread of 1e-12 that absorbs floating-point noise. Applies to both figures. |

Buy-and-hold and an always-flat agent report `constant_exposure`, not a
correlation of 0: with no change in sizing there is nothing to rank. Their
quintile table is still reported and shows the flat exposure. The resolution
exists because costs paid out of cash and price drift between rebalances move
a book's exposure by fractions of a percent without any sizing decision. When
the exposure does span more than the resolution, the correlation ranks
exposure rounded to the resolution grid, so those small movements do not
order bars the agent sized alike. The quintile medians use the unrounded
exposure.

A trajectory that does not record a decision for every bar of a run, or whose
window ends after the dataset, is refused as a whole: replaying it would mean
supplying holds the agent never chose, or reading a different dataset.

## What it does NOT do

- It does not test significance. Pairs from different execution seeds of the
  same window share bars, so the pair count is not a count of independent
  observations, and no p-value is reported.
- It is a book-level view. Volatility is averaged across symbols, so an agent
  that keeps its gross exposure constant while rotating into calmer names
  reads as unresponsive.
- It does not judge whether a response is good. A negative correlation says
  the agent sized down in turbulence, not that doing so paid.
- It does not change any score, gate, eligibility decision, rank or golden
  file.

## Running it

The diagnostic needs the trajectory and the dataset it was recorded on, so it
is an opt-in flag on `verify-trajectory`, in the same form as
`score --diagnostics`:

```bash
sharpebench verify-trajectory traj.json --data data.csv --diagnostics sizing-response [--vol-lookback N] [--json]
```

Without `--diagnostics` the output is the verification and nothing else. With
it, the text output is the unchanged verification followed by a separate
block, and `--json` prints `{"verification": ..., "sizing_response": {...}}`,
where `verification` is the output the command prints without the flag. The
report is computed before anything is printed. An unknown identifier, a
missing value, `--vol-lookback` without the diagnostic, a lookback below 2,
or the diagnostic combined with `--reexecute` exits 2 with no output. A
trajectory the diagnostic refuses exits 1 with no output.
