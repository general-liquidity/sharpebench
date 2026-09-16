# Timing-luck floor

pass^k varies the execution seed, which moves fills. Nothing on the board
varies *when* a window starts. A production record of LLM trading fleets
(What LLM Trading Agents Actually Do in Production, arXiv 2609.05663, canon
rule 17) reports identical contracts at a plus or minus fifteen-minute schedule
offset producing a loss of 88 dollars and a gain of 21, and asks every
evaluation to know its timing-luck floor: how far a result moves when only the
phase of the schedule moves. Two
board rows whose deflated Sharpe differs by less than that floor are not
separated by anything the schedule did not also produce.

`sharpebench timing-luck` measures that floor for the `run` protocol on a
dataset. It is a property of the protocol and the data: it runs no external
entrant and calls no model. The report is reporting surface beside a board. It
is never read by the gate, eligibility or the rank, it carries
`rank_input: false`, and `sharpebench run` output is the same whether or not it
has been produced.

```bash
sharpebench timing-luck --offsets 5                       # synthetic panel
sharpebench timing-luck --offsets 5 --data prices.csv --periods-per-year 252 --json
```

The library entry point is `sharpebench_harness::timing_luck::timing_luck`.

## What it runs

The dataset, the two windows, the eight execution seeds and the cost model are
resolved exactly as `sharpebench run` resolves them: the default costs, or the
same `--short-borrow-bps` rate `run` accepts, through the same parser. The rows are the
reference field `run` ranks when no entrant is named, in its order
(`buy-and-hold`, `momentum`, `luck-floor-00` to `luck-floor-02`), and the
`pipeline-hold` suite control. The refusal control is not a row: it produces
no return series, so there is no Sharpe to shift.

## Geometry

`--offsets k` declares k start offsets, 0 to k-1 bars. For a declared window
`[start, start + len)`, offset `o` evaluates

```text
[start + o, start + o + len - (k - 1))
```

Every shifted window of a declared window has the same length, and every one
lies inside the declared window it perturbs. With the synthetic panel and
`--offsets 5`, window `20-100` is evaluated as the 76-bar windows `20-96`,
`21-97`, up to `24-100`. Consequences:

- No shifted window reads a bar past its declared window, so none reads past
  the dataset end, including for `run`'s last window, which ends on the last
  bar.
- Declared windows that are disjoint, as `run`'s are, stay disjoint at every
  offset and across offsets. The report records both
  `declared_windows_disjoint` and `instances_of_distinct_windows_disjoint`,
  computed from the windows actually read.
- `--offsets 1` evaluates the declared windows exactly, so its figures are the
  unshifted ones: each reference row's all-windows deflated Sharpe equals the
  one `sharpebench run --json` publishes for it.
- For k above 1 every shifted window is k - 1 bars shorter than its declared
  window. `instance_len` states the length used.

Shifted windows of one declared window overlap each other: adjacent offsets
share all but one bar, and the report gives the bars shared by the first and
last. The offsets are therefore overlapping shifts, not independent samples,
and `instances_of_one_window_overlap` is true whenever k is above 1. Read a
range as the spread the schedule phase alone produced on this data, not as a
confidence interval.

## Figures

At each offset every row is run over the shifted windows, and its seed
replicates are averaged per bar before any figure is taken, as the board does.
Two figures are reported per row, for every window together (`all-windows`)
and for each declared window alone:

- **Sharpe**: the per-period Sharpe ratio of the pooled track, as the kernel's
  `observed_sharpe_ratio` computes it. A track the kernel says has no Sharpe
  ratio (a constant one) gives no figure.
- **Deflated Sharpe**: for a reference row, the value `rank` publishes when the
  reference field is ranked over the same scope. The all-windows figure is the
  board's own deflated Sharpe for the shifted protocol; a per-window figure is
  the one the board would publish if that window were the whole protocol. The
  hold control is never ranked, so its value is `score_agent`'s. A deflated
  Sharpe the kernel refused (`deflation_error`) gives no figure, never a zero.

Each figure is reported per offset (`by_offset`, offset 0 first), then as
`min`, `max` and `range` (`max - min`) over the offsets that produced it. Every
figure carries the counts behind it: `windows` (all declared windows, or one),
`offsets` (declared) and `offsets_measured` (offsets that produced the figure;
the spread is taken over these only). The hold control never trades, so its
track is constant and every figure of its rows is absent with
`offsets_measured` 0.

A deflated Sharpe range of zero is not evidence of no timing luck on its own.
On the built-in synthetic panel with `--offsets 5`, every reference row sits so
far below the deflation bar that its deflated Sharpe is 0 at all five offsets,
while its Sharpe still moves. Read the two ranges together.

## When there is no report

The report is typed unavailable, with exit code 1, when the conditions below
hold. With `--json` the command prints a document whose `status` is
`unavailable` and whose `unavailable.reason` names the cause; without it, one
line on stderr says the same.

| `reason` | Meaning |
|---|---|
| `no_offsets` | k is 0 (the CLI refuses this as a usage error first) |
| `no_windows` | no window was declared |
| `no_seeds` | no execution seed was declared |
| `window_past_dataset_end` | a declared window ends after the last bar |
| `window_too_short_for_offsets` | `len - (k - 1)` is below `min_instance_bars` (2, the fewest returns a Sharpe ratio needs) |

On the synthetic panel the windows have 80 bars, so `--offsets 79` is the
largest k that measures and `--offsets 80` is unavailable. A `--data` file
shorter than 40 rows is refused before anything runs, as `run` refuses it.

## What it does not do

- It does not re-run an entrant. `--cmd`, `--image` and `--http` are usage
  errors here. The floor is a property of the protocol and the dataset, to be
  shown beside a board, not a per-entrant diagnostic.
- It does not vary the execution seeds, the cost model or the reference roster
  across offsets. Only the start moves, so each luck-floor agent draws the same
  random weights at every offset.
- It does not replay recorded decisions late or match an entrant's exposure
  with random timing. Those answer different questions (staleness of decisions,
  and whether a mostly flat entrant beats random timing at its own exposure)
  and are separate reports.
- It does not change the board, the gate or any golden score file.
