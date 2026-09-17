# Timing-luck floor

pass^k varies the execution seed, which moves fills. Nothing on the board moves
the dates on which a strategy acts. A production record of LLM trading fleets
(What LLM Trading Agents Actually Do in Production, arXiv 2609.05663, canon
rule 17) reports identical contracts at a plus or minus fifteen-minute schedule
offset producing a loss of 88 dollars and a gain of 21, and asks every
evaluation to know its timing-luck floor: how far a result moves when only the
phase of the decision schedule moves. If two board rows differ by less than
that floor, the schedule alone could have produced the gap.

`sharpebench timing-luck` measures that floor for the `run` protocol on a
dataset, with no external entrant and no model. The report sits beside a board.
The gate, eligibility and the rank never read it, it carries
`rank_input: false`, and `sharpebench run` prints the same output whether or not
you produce it.

```bash
sharpebench timing-luck --cadence 5                       # synthetic panel
sharpebench timing-luck --cadence 5 --data prices.csv --periods-per-year 252 --json
```

The library entry point is `sharpebench_harness::timing_luck::timing_luck`, and
the JSON document is `sharpebench.timing-luck.v2`.

## What it runs

The command resolves the dataset, the two windows, the eight execution seeds
and the cost model the way `sharpebench run` resolves them: the default costs,
or the `--short-borrow-bps` rate `run` accepts, through the same parser. The
rows are the reference field `run` ranks when you name no entrant, in its order
(`buy-and-hold`, `momentum`, `luck-floor-00` to `luck-floor-02`), then the
`pipeline-hold` suite control. The refusal control produces no return series,
so it has no row.

The board's reference agents decide on every bar. A strategy that decides on
every bar has no schedule phase to move, so the report gives each row a
schedule: `--cadence m` makes every row rebalance every `m` bars, and the report
runs each of the `m` phases of that schedule.

## Schedule

Every run starts in cash, so every phase decides on the window's first bar.
After that, phase `p` decides on window step `j` when `j % m == p`: bars
`start + p`, `start + p + m`, and so on. On any other bar the row submits no
orders and the engine holds its positions. (Under a cost model with execution
noise, an order carried over from a scheduled bar can still fill on the next
bar; the models `timing-luck` accepts have no execution noise.) Every phase
covers every bar of every declared window, so the phases differ only in the
dates on which they rebalance.

`windows[].decisions_by_phase` gives the decisions per run at each phase.
Phase 0 decides `ceil(bars / m)` times. A phase above 0 adds its first-bar
decision, which falls off its cadence: on an 80-bar window at cadence 3 the
counts are 27, 28 and 27.

Cadence 1 has a single phase with a decision on every bar, which is the board's
reference field. With `--cadence 1`, each reference row's all-windows deflated
Sharpe and deflation inputs equal the ones `sharpebench run --json` publishes
for it.
At any cadence above 1 the rows are cadenced versions of the reference agents,
not the board's rows.

A luck-floor row calls its random agent on scheduled bars only, so its `i`-th
decision draws the same weights at every phase. The phase changes only the bar
on which the row applies them.

## Figures

At each phase, the report averages each row's seed replicates bar by bar and
concatenates the windows, as the board does. From that track it reports two
figures for every window together (`all-windows`) and for each declared window
alone:

- **Sharpe**: the per-period Sharpe ratio, as the kernel's
  `observed_sharpe_ratio` computes it. A track the kernel says has no Sharpe
  ratio (a constant one) gives no figure.
- **Deflated Sharpe**: the phase's track tested against one bar for all phases.
  `deflation` holds that bar: the dispersion, its source, the trial count and
  the null mean that the row's score records at phase 0 in the same scope. For a
  reference row that score comes from `rank` over the reference field. For the
  hold control, which is never ranked, it comes from `score_agent` under the
  configured prior.

The bar stays fixed because a board deflates an entrant against one field, and
moving the entrant's schedule does not move that field. The report moves every
reference row's phase together, so a bar measured at each phase would shift with
the random rows and give a row whose own track never changes a spread.
`field_dispersion_by_phase` lists the dispersion each phase's own field
measured, so you can see how far the bar would have moved. A deflated Sharpe
exists whenever the deflation with that bar succeeds. A score that failed only
its bootstrap interval still reports it.

Each figure is listed per phase (`by_phase`, phase 0 first), then summarised
over the phases that produced it: `min`, `max`, `range` (`max - min`) and
`std_dev`. The standard deviation divides by the number of measured phases,
because the phases are every phase of the cadence, not a sample of them. Every
scope states the counts behind its figures: `windows` (all declared windows, or
one), `phases`, and each figure's `phases_measured`. The hold control never
trades, so its track is constant and it has no figures (`phases_measured` 0).

## Reading the report

All phases of a row read the same price bars and differ only in the dates on
which the row acts, so the range is the whole effect of the schedule phase on
that data at that cadence, execution noise included. It is not a confidence
interval.

On a panel where symbol A alternates between 1 and 2 bar by bar and symbol B
stays at 1, with no costs, equal-weight buy-and-hold at cadence 2 rebalances on
A's low bars at phase 0 and on its high bars at phase 1. Over a 40-bar window
the per-period Sharpe is 11 sqrt(105690) / 16260, about 0.2199, at phase 0 and
5 sqrt(34346) / 5284, about 0.1754, at phase 1: a range of 0.0446 from the phase
alone. The harness tests pin those values, which were derived twice in exact
arithmetic. On a cost-free panel where every symbol doubles each bar,
buy-and-hold and momentum never need to trade after entry, and their ranges are
exactly zero.

On the built-in synthetic panel at `--cadence 5`, momentum's per-window Sharpe
moves by about 0.14 and 0.12 per period across the phases, and buy-and-hold's by
about 0.001. Every reference row sits far below its deflation bar there, so no
deflated Sharpe moves by more than about 0.012: read the two ranges together.

## When there is no report

The command exits 1 when the report is unavailable. With `--json` it prints a
document whose `status` is `unavailable` and whose `unavailable.reason` names
the cause; without it, one line on stderr gives the same cause.

| `reason` | Meaning |
|---|---|
| `no_cadence` | cadence 0 (the CLI refuses it as a usage error first) |
| `no_windows` | no window was declared |
| `no_seeds` | no execution seed was declared |
| `invalid_periods_per_year` | the scoring frequency is not finite and positive |
| `window_order` | the declared windows are out of time order or share a bar |
| `window_past_dataset_end` | a declared window ends after the last bar |
| `window_shorter_than_cadence` | a window has fewer bars than `min_window_bars`: the cadence, and at least 2 |

A window shorter than the cadence would leave some phase with no decision after
its first bar. On the synthetic panel the windows have 80 bars, so
`--cadence 80` is the largest cadence that measures. A `--data` file shorter
than 40 rows is refused before anything runs, as `run` refuses it.

## What it does not do

- It runs no entrant. `--cmd`, `--image` and `--http` are usage errors here.
  The floor describes the protocol and the dataset, for display beside a
  board.
- It varies nothing but the schedule phase: the execution seeds, the cost model
  and the roster are the same at every phase.
- It does not replay recorded decisions late, and it does not match an
  entrant's exposure with random timing. Those reports answer other questions.
- Declared windows must not share a bar, but the engine hands each observation
  up to 20 closes ending at its own bar, so the first decisions in a window read
  up to 19 bars from before its start, which can belong to the previous window.
- It changes no board, gate or golden score file.
