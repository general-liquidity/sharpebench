# Importing a rival benchmark's field

`sharpebench import` converts per-period return series published by another
benchmark into a SharpeBench submissions file, so that its field can be
re-scored under SharpeBench's gates and the two rankings compared:

```
sharpebench import csv <dir-or-file> --out subs.json [--trials N]
sharpebench score subs.json
```

Read the caveats below before quoting any result of such a re-score. Honesty
about what an import can and cannot claim is the point of the command.

## Input formats

### Directory of per-agent CSVs

One `<agent_id>.csv` per agent. Each file holds that agent's per-period simple
returns, one run per column, periods down the rows. A header row is optional
(any row whose first cells are not numbers is skipped). Columns may have
unequal lengths; trailing empty cells end a column's series. Files are read in
sorted filename order so the produced field is deterministic.

```
run_a,run_b
0.0010,0.0014
0.0013,0.0009
...
```

`suites/import-example/` holds a runnable synthetic demonstration of this
shape (it is not market data).

### Single CSV with an agent column

Long format: a header row containing an `agent` (or `agent_id`) column, a
returns column (`return`, `returns` or `ret`; otherwise the first remaining
column), and optionally a `run` column. One row per period. Rows group into
runs per agent; without a `run` column each agent gets a single pooled run.
Agents and runs keep first-appearance order.

```
agent,run,return
gpt-x,r0,0.0012
gpt-x,r0,-0.0004
claude-y,r0,0.0008
...
```

A single CSV without an `agent` column is also accepted: it becomes one agent
named after the file, one run per column.

### `import stockbench`

Deliberately not a parser; see the StockBench section below.

## What the re-score can and cannot claim

An imported submission carries returns and nothing else, so two of
SharpeBench's five eligibility conjuncts are only partially observable:

- **Process gate: trivially passes.** There is no audit trace, so the process
  checks have nothing to inspect and every agent scores clean. A pass here
  says nothing about how the foreign agent actually behaved.
- **Calibration: absent.** There are no per-decision confidences or outcomes,
  so the calibration signal is empty.

The comparison that remains is real but narrower: deflation (deflated Sharpe
and PSR against the declared search footprint), per-run reliability
(`pass^k` across the imported runs) and the bootstrap significance test. The
configured mandate remains a scoreable return-path constraint, but an import
cannot establish the foreign process that produced a trace-free record. The
import command prints this notice loudly on every run, and embeds the same
text in each submission under an `_import_note` field. The scorer's
deserialization ignores unknown fields, so the caveat travels inside the file
without breaking `sharpebench score`.

## The trials question

Deflation needs the search footprint: how many strategies, configurations and
reruns were tried before the published one was kept. For an imported field
that number is unknowable. The publishing benchmark rarely reports it, and the
agents' own in-sample search is invisible from outside.

`--trials N` sets `in_sample_trials` on every imported submission. The default
is 0, which the scorer treats as an undeclared footprint. That choice
**understates deflation**: the true footprint of any published result is at
least 1 and usually far larger, and a larger footprint can only lower the
deflated Sharpe. The consequence cuts one way, and it is the honest way to
read every imported re-score:

> Any demotion the re-score shows is a **lower bound** on how much the rival
> ranking overstates skill. With the true (larger) trial count, the deflation
> would bite harder, never softer.

If the source documents its protocol (for example "3 runs per model"), pass a
defensible `--trials` and say in your write-up what you assumed.

A second calibration knob: `sharpebench score` uses the default
`ScoreConfig`, whose annualization assumes daily bars (252 periods per year).
Imported daily equity series match it; for other bar sizes, score through the
library with `ScoreConfig::for_periods_per_year` as `sharpebench run
--periods-per-year` does.

## StockBench: the experiment that needs the authors' artifacts

The related-work experiment this adapter was built for is re-scoring the field
of StockBench, "Can LLM Agents Trade Stocks Profitably In Real-world
Markets?" (<https://arxiv.org/abs/2510.02209>). It cannot be done from public
artifacts, and this section records exactly why (surveyed 2026-08-23).

What StockBench publishes:

- Repository <https://github.com/ChenYXxxx/stockbench> (Apache-2.0): the
  harness code and the **environment inputs**: per-symbol daily price
  parquets, financial statements, corporate actions and news caches for the
  top 20 DJIA stocks, post-2024.
- Leaderboard <https://stockbench.github.io/>: **summary statistics only**
  per model: final return (%), maximum drawdown (%) and Sortino ratio,
  averaged over 3 runs.

What it does not publish: any per-agent per-period series. No daily portfolio
values, no per-step returns, no decision trajectories for the evaluated
models appear in the repository or on the site. Their harness does produce
exactly what the import needs when run: it writes `daily_nav.parquet` (daily
net asset value), `trades.parquet` and `metrics.json` per run under
`storage/reports/backtest/<run_id>/` (see `stockbench/backtest/reports.py`).
Those artifacts stay on the machine that ran the backtest; the published
tables are the aggregates computed from them.

Summary statistics cannot be turned into a return series without inventing
the very data the gates interrogate. A series synthesized to match a final
return, drawdown and Sortino would have its deflated Sharpe, `pass^k` and
bootstrap p-value determined by our synthesis choices, not by the agents. So
`sharpebench import stockbench` refuses and explains rather than parse
something that does not exist.

The experiment therefore needs one of:

- **The authors' raw artifacts**: the `daily_nav.parquet` files for the
  evaluated models (3 runs per model would slot directly into `pass^k`), or
- **A reproduction**: run their Apache-2.0 harness against the committed
  environment data with your own model keys, then export each run's NAV
  percentage changes to one CSV per model and import with
  `sharpebench import csv <dir> --out subs.json --trials 3`.

Either route yields per-model, per-run daily return series, and the re-score
becomes the one-command experiment this chapter describes, with the caveats
above attached: same field, gated ranking, and any demotion a lower bound.
