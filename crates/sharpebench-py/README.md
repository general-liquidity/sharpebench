# sharpebench (Python)

**Is my Sharpe real, or an artifact of luck and multiple testing?**

Python distribution of **SharpeBench**'s honest-backtest statistics, a pyo3
binding over the same deterministic Rust kernel the SharpeBench CLI and the
`@general-liquidity/sharpebench` npm package use. Bring your own return series;
everything takes plain numeric sequences (lists, tuples, numpy arrays,
`df["ret"].to_numpy()`) and returns plain floats, lists and dicts.

```python
import numpy as np
from sharpebench import is_my_sharpe_real, bootstrap_dsr_ci

returns = df["strategy_ret"].to_numpy()          # per-period, NOT annualized

# n_trials is the honest one: how many variants did you try before keeping this?
v = is_my_sharpe_real(returns, n_trials=200)
print(v["sharpe"], v["deflated_sharpe"], v["verdict"], v["explanation"])

ci = bootstrap_dsr_ci(returns, n_trials=200)
print(ci["lower"], ci["point"], ci["upper"])
```

## Surface

| Function | Answers |
|---|---|
| `sharpe_ratio(returns)` | observed per-period Sharpe |
| `moments(returns, target=0.0)` | mean / std / skew / kurtosis / downside deviation / Sortino |
| `probabilistic_sharpe_ratio(returns, sr_benchmark=0.0)` | `P(true Sharpe > benchmark)` (PSR) |
| `deflated_sharpe_ratio(returns, n_trials, trials_sr_std=0.5)` | PSR deflated for the size of the search (DSR) |
| `expected_max_sharpe(trials_sr_std, n_trials)` | the Sharpe the best of `n_trials` shows with **zero** skill |
| `min_track_record_length(returns, ...)` | periods needed before the Sharpe is believable |
| `bootstrap_dsr_ci(returns, n_trials, ...)` | `{point, se, lower, upper}` on the DSR itself |
| `bootstrap_pvalue(excess, ...)` | stationary-bootstrap p-value for one series |
| `is_my_sharpe_real(returns, n_trials=1, ...)` | LITE verdict dict: `pass \| borderline \| fail` + explanation |
| `is_my_sharpe_real_full(field, ...)` | FULL verdict over a whole candidate field (LITE + snooping family + PBO + HLZ) |
| `reality_check_pvalue(field, ...)` | White's Reality Check over the field |
| `spa_pvalue` / `spa_consistent_pvalue(field, ...)` | Hansen's SPA (liberal / consistent) |
| `step_down_significant(field, ..., alpha=0.05)` | Romano-Wolf step-down, per candidate, FWER-controlled |
| `probability_of_backtest_overfitting(perf_matrix, s=16)` | CSCV PBO |
| `benjamini_hochberg(p_values, q=0.05)` / `fdr_verdict(...)` | BH-FDR rejections and the operator summary |
| `hlz_gate(t_stat, t_threshold=None)` | the Harvey-Liu-Zhu `\|t\| >= 3.0` factor bar |
| `selection_robustness(candidates, n_trials, ...)` | best vs median DSR: is the headline a lucky pick? |
| `runs_for_power(effect, alpha, power)` | how many runs to detect an effect |
| `pass_k(passed_per_run, mode="all", n=None)` | pass^k reliability: won on **every** run, not on average |

### Matrix orientation

Two conventions, deliberately unchanged from the papers they come from:

- the data-snooping family (`reality_check_pvalue`, `spa_*`, `step_down_significant`,
  `is_my_sharpe_real_full`) takes a **field: N rows (strategies) x T cols (time)**;
- `probability_of_backtest_overfitting` takes the **transpose: T rows (time) x N cols
  (strategies)**.

### Determinism

No I/O, no clock, no ambient randomness. The bootstraps take an explicit `seed`
(defaulted to a fixed constant, so a result is reproducible unless you ask for
otherwise). The same input yields byte-identical output on any platform.

## Relationship to `sharpearena`

`sharpearena` is the **environment**: a leak-free, point-in-time arena where a
trading agent produces a track, scored end-to-end by `score_run`. `sharpebench`
is the **judge for a track you already have**: your own backtest, live P&L, or a
field of candidate strategies. They share one Rust statistics kernel, so the
verdict is identical either way; this package simply does not, and will not,
duplicate arena run-scoring.

## Building from source

```
python -m pip install maturin
python -m maturin develop --manifest-path crates/sharpebench-py/Cargo.toml
python -m pytest crates/sharpebench-py/tests
```

MIT OR Apache-2.0.
