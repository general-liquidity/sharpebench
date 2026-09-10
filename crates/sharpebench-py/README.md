# sharpebench (Python)

**Is my Sharpe real, or an artifact of luck and multiple testing?**

Python distribution of **SharpeBench**'s luck-robust quantitative evaluation
statistics, a pyo3 binding over the same deterministic Rust kernel used by the
SharpeBench CLI and the `@general-liquidity/sharpebench` npm package. Bring your own return series;
everything takes plain numeric sequences (lists, tuples, numpy arrays,
`df["ret"].to_numpy()`) and returns plain floats, lists and dicts.

```python
import numpy as np
from sharpebench import is_my_sharpe_real, bootstrap_dsr_ci

returns = df["strategy_ret"].to_numpy()          # per-period, NOT annualized

# n_trials is the honest one: how many variants did you try before keeping this?
# periods_per_year says what a row is (default 252, daily bars).
v = is_my_sharpe_real(returns, n_trials=200, periods_per_year=252)
print(v["sharpe"], v["deflated_sharpe"], v["verdict"], v["explanation"])

ci = bootstrap_dsr_ci(returns, n_trials=200)
print(ci["lower"], ci["point"], ci["upper"])
```

## Surface

| Function | Answers |
|---|---|
| `sharpe_ratio(returns)` | observed per-period Sharpe |
| `moments(returns, target=0.0)` | mean / std / skew / kurtosis / downside deviation / Sortino |
| `probabilistic_sharpe_ratio(returns, sr_benchmark=0.0)` | PSR: one minus the p-value of `H0: SR <= benchmark` (not the probability the true Sharpe exceeds it). `sr_benchmark` is per period |
| `deflated_sharpe_ratio(returns, n_trials, trials_sr_std=None, periods_per_year=None)` | PSR against the best of `n_trials` zero-skill trials (DSR): one minus a p-value, not the probability of skill. `trials_sr_std` is per period; omitted, it is the annualized 0.5 prior over `sqrt(periods_per_year)` (default 252) |
| `expected_max_sharpe(trials_sr_std, n_trials)` | the per-period Sharpe the best of `n_trials` shows with **zero** skill, from a per-period `trials_sr_std` |
| `min_track_record_length(returns, ...)` | periods needed before the Sharpe is believable; `sr_benchmark` is per period |
| `bootstrap_dsr_ci(returns, n_trials, ...)` | `{point, se, lower, upper}` on the DSR itself; `trials_sr_std` / `periods_per_year` as in `deflated_sharpe_ratio` |
| `bootstrap_pvalue(excess, ...)` | stationary-bootstrap p-value for one series |
| `is_my_sharpe_real(returns, n_trials=1, ..., periods_per_year=None)` | LITE verdict dict: `pass \| borderline \| fail` + explanation. `trials_sr_std` and `sr_benchmark` are annualized and `periods_per_year` (default 252, flagged) converts both |
| `is_my_sharpe_real_full(field, ...)` | FULL verdict over a whole candidate field (LITE + snooping family + PBO + HLZ) |
| `reality_check_pvalue(field, ...)` | White's Reality Check over the field |
| `spa_pvalue` / `spa_consistent_pvalue(field, ...)` | Hansen's SPA (liberal / consistent) |
| `step_down_significant(field, ..., alpha=0.05)` | Romano-Wolf step-down, per candidate, FWER-controlled |
| `probability_of_backtest_overfitting(perf_matrix, s=16)` | CSCV PBO |
| `benjamini_hochberg(p_values, q=0.05)` / `fdr_verdict(...)` | BH-FDR rejections and the operator summary |
| `hlz_gate(t_stat, t_threshold=None)` | the Harvey-Liu-Zhu `\|t\| >= 3.0` factor bar |
| `selection_robustness(candidates, n_trials, ...)` | best vs median DSR: is the headline a lucky pick? `trials_sr_std` / `periods_per_year` as in `deflated_sharpe_ratio` |
| `runs_for_power(effect, alpha, power)` | how many runs to detect an effect |
| `pass_k(passed_per_run, mode="all", n=None)` | pass^k reliability: won on **every** run, not on average |
| `budget_curve(...)` | DSR by search budget, marginal DSR, and the non-improvement onset. `trials_sr_std` is annualized (default 0.5) and converted by `periods_per_year` (default 252) |
| `rank_board(submissions, config_json="")` / `score_one(...)` | Full composite scoring over the CLI-compatible JSON contract |
| `rank_returns(field, config_json="")` | Build and rank a board from agent IDs and per-run return arrays |
| `default_score_config()` | Serialize the default scoring configuration |
| `never_catastrophic_config()` | Serialize the preset that asks only whether every run avoids catastrophe |
| `relative_to_benchmark_config(id)` | Serialize the benchmark-relative pass preset |

### Units of the deflation prior

Every Sharpe here is per period. The verdicts (`is_my_sharpe_real*`) and
`budget_curve` take `trials_sr_std` **annualized**, like the leaderboard's
`ScoreConfig`, and divide it by `sqrt(periods_per_year)`. The verdicts take
their PSR and MinTRL `sr_benchmark` annualized too and convert it the same way;
through 0.21.0 they read it per period. The raw `probabilistic_sharpe_ratio`
and `min_track_record_length` take `sr_benchmark` per period. The raw primitives
(`deflated_sharpe_ratio`, `bootstrap_dsr_ci`, `selection_robustness`,
`expected_max_sharpe`) take it **per period** and use an explicit value as
given. Omitted, the first three use the annualized 0.5 prior at
`periods_per_year` (default 252): `0.5 / sqrt(252) = 0.0315` per period on daily
returns. Through 0.19.0 they used 0.5 per period, an annualized dispersion of about
7.9 on daily returns.

```python
deflated_sharpe_ratio(returns, n_trials=200)                         # 0.5 / sqrt(252) per period
deflated_sharpe_ratio(returns, n_trials=200, periods_per_year=8760)  # hourly: 0.5 / sqrt(8760)
deflated_sharpe_ratio(returns, n_trials=200, trials_sr_std=0.02)     # your per-period value, as given
```

### Matrix orientation

Two conventions, deliberately unchanged from the papers they come from:

- the data-snooping family (`reality_check_pvalue`, `spa_*`, `step_down_significant`,
  `is_my_sharpe_real_full`) takes a **field: N rows (strategies) x T cols (time)**;
- `probability_of_backtest_overfitting` takes the **transpose: T rows (time) x N cols
  (strategies)**.

### Determinism

No I/O, no clock, no ambient randomness. The bootstraps take an explicit `seed`
(defaulted to a fixed constant, so a result is reproducible unless you ask for
otherwise). The Python suite matches the public example board to the committed
Rust golden on Ubuntu CI. Rust CI separately checks two committed golden fields
on Linux, macOS, and Windows; neither check covers every possible input or host.

## Relationship to `sharpearena`

`sharpearena` is the **environment**: a point-in-time market API where a
trading agent produces a track, scored end-to-end by its `score_run` helper. It
does not provide process containment. `sharpebench`
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
