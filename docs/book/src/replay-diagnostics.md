# Replay diagnostics

Two diagnostics replay a captured trajectory's recorded decisions through the
frozen engine in a different arrangement and report how the result moves. Both
are rank-neutral: the gate and the rank never read them, and turning them on
changes no score. They are reported beside a strictly verified trajectory:

```bash
sharpebench verify-trajectory traj.json --data data.csv \
  --timing-null --null-draws 200 --null-seed 0 \
  --lagged-replay 1,2,5 --json
```

Without either flag the command prints exactly what it printed before. With
either, the verification is printed unchanged and the diagnostics follow it:
under `--json` as a `replay_diagnostics` member beside the verification fields,
in the human output as a block after the verification. The diagnostics replay
under the data and cost model the strict path has just bound the trajectory to
(for trajectories the CLI captures, the typical profile with any
`--short-borrow-bps` rate), so they refuse `--allow-unbound-trajectory` and
`--reexecute`. They also refuse `--diagnostics sizing-response`, whose output
nests the verification in a different shape; request the two separately. A
malformed or contradictory flag exits 2 before any file is read, and so does a
draw count above 100,000. A declared lag too long for the trajectory's runs
exits 2 once the trajectory is read. A trajectory the diagnostics cannot replay
(see below) exits 1.

The library functions are `timing_null` and `lagged_replay` in
`sharpebench_sim::replay_nulls`. They take any `CostModel`; the CLI always
passes the bound one.

## Exposure-matched random timing

The luck floor is a fully invested random allocator, and the significance gate
tests mean return above cash. An entrant that is flat most of the time and long
in a rising window can clear both on drift alone, and neither says whether its
timing beats random timing at its own exposure. `--timing-null` answers that
for each run.

1. The run's recorded decisions are stepped through the engine on the run's own
   window and execution seed. A bar is invested when the book's gross exposure
   after that bar's trades exceeds `1e-9` of NAV. The run's holding periods are
   its maximal stretches of invested bars.
2. Each draw keeps every holding period whole, with the entrant's own decisions
   for its bars, so the number of periods, their lengths and the gross exposure
   inside them are the entrant's. It shuffles the order of the periods and
   spreads the flat bars over the gaps as a uniformly random composition,
   keeping at least one flat bar between consecutive periods so none merge.
   Every bar outside the placed periods closes every position.
3. Each draw is replayed with `replay_run` under the run's window, execution
   seed and cost model, so it pays the same frictions the entrant paid.
4. The run's per-period Sharpe (the figure its plain replay earns) is placed
   among the draws' Sharpe ratios as a mid-rank percentile,
   `(below + ties / 2) / draws`, reported with the draw count.

Each percentile comes with its Monte Carlo standard error: every draw scores 1,
1/2 or 0, and the error is the standard deviation of those scores over the
square root of the draw count. At 200 draws it is about 0.035 near the middle
and 0.015 near 0.05. It reads zero when every draw falls on one side; the
resolution there is one over the draw count. Each run also reports its number of
distinct placements: the distinct orders of its holding periods (periods with
the same orders count once) times the `C(F + 1, c)` ways to spread `F` flat bars
around `c` periods, saturating at `u64::MAX`. A run with one period and one flat
bar has two. When the count is not far above the draw count, draws repeat.

The report also carries, for every run, the exposure profile (bars, invested
bars, holding periods, longest holding period, mean gross exposure when
invested) and the draws' mean invested bars as the engine executed them. That
mean equals the entrant's invested bars except where a moved order meets a
close that is not positive, or a trade value below the engine's `1e-9`
minimum, both of which the engine skips.

The timing reference refuses a cost model with execution noise or a finite
liquidity cap (`ExposureNotPreserved`). There, whether an order fills depends
on when and how often orders are sent. A deferred entry sits on a bar that
reads flat, so the detected period starts on a hold that opens nothing once
moved, and delayed exits lengthen the entrant's periods. A review measurement
under the realistic profile, over 400 runs, found draws holding about 126 bars
per run against the entrant's 147, and a no-skill entrant's mean percentile at
0.40 instead of 0.50 on a driftless panel. The CLI binds the typical profile,
so it never meets this refusal.

Across runs, draw `i` of the reference is the mean of draw `i` over every run
that has timing freedom, and the entrant's mean per-run Sharpe is placed in that
distribution. Runs over the same window read the same placement stream, so draw
`i` places the holding periods of every execution-seed copy of a window at the
same bars. A capture records one run per window and seed, and a price-only
entrant's copies carry the same decisions. A separate placement per copy would
shrink the spread of the reference mean by about the number of copies and put a
no-skill entrant in the tails far too often. A test holds a no-skill entrant
with four copies per window to the nominal tail rate.

The draws are a pure function of the trajectory, the data, the cost model, the
declared draw count and the declared seed. The placement stream is derived from
the seed and the window. The default declaration is 200 draws from seed 0, and
the most a report accepts is 100,000.

A run with no timing to test is typed unavailable rather than given a
degenerate percentile: `never_invested` when no bar was invested, and
`always_invested` when every bar was, since then the only placement is the
entrant's own. The aggregate is unavailable when no run has timing freedom, with
the runs' shared reason, or `no_run_with_timing_freedom` when they differ.

## Lagged replay

`--lagged-replay k,...` replays every run with each decision executed `k` bars
after it was recorded, for each declared `k`, and reports the mean per-run
Sharpe and the pooled mean per-period return beside the undelayed figures. A
policy that uses current observations loses its edge when its decisions arrive
late; a static tilt earns the same.

For lag `k` the first `k` bars hold (the book starts in cash, so they are flat).
A lag of zero replays the recorded decisions unchanged and reproduces the
undelayed replay exactly. Lags are reported ascending without repeats. An empty
list is refused, and so is any lag above the shortest run's length less 3,
since such a row has no two bars to compare. The check uses checked
arithmetic, so a lag near `usize::MAX` is refused rather than wrapped.

Within a run every row, the undelayed one included, is computed on the same
bars: those after the latest bar at which any row first holds a position. No
row carries its opening trade, and every compared bar is earned by a position
each row has already taken. An entrant whose first trade comes late is compared
from after its latest row's first fill, not from a fixed offset. Each run
reports how many leading bars it skipped and how many it compared. The
aggregate averages per-run Sharpe over the runs with rows and pools the mean
return over their compared bars.

The window's end is not symmetric, and every report says so as `end_effect`: a
lag-`k` row never executes the run's last `k` recorded decisions, whose fills
would fall after the window, while the undelayed row executes them on the
window's last `k` bars. Under execution noise a moved decision also meets
other noise draws, so a lag row then mixes the delay with a different fill
realization.

A run whose rows cannot all be compared is typed unavailable and left out of
the aggregate: `never_fills` when a row never holds a position in the window,
`too_few_compared_bars` when skipping past every opening fill leaves fewer than
two bars, and `no_sharpe` when the kernel's `observed_sharpe_ratio` refuses a
row's compared bars, as it does for a book that is flat on all of them. A flat
row is not averaged in as a Sharpe of zero. When no run has rows, the
aggregate is unavailable (`no_comparable_run`).

### Decision delay

The stressed profile declares a two-bar decision delay that the backtest driver
does not apply. The lagged replay is the way to measure decision-delay
sensitivity: `lagged_replay` with the stressed profile's cost model and lag
`decision_delay_bars` replays every decision two bars late under the stressed
frictions. This route is library-only: the CLI replays a trajectory under the
cost model it is bound to, which for CLI captures is the typical profile, so
`--lagged-replay 2` there measures the delay under typical costs. The stressed
profile's own behaviour is unchanged, so evidence produced under it keeps its
meaning.

## Validity

Both diagnostics compare the entrant's decisions with the same decisions at
other times. That comparison is fair only while the entrant's own orders do not
move the prices its positions are marked at, and every report carries that
condition as `valid_when`. SharpeBench marks every position at the frozen
dataset close and charges own-order impact on the fill price of the trade that
causes it, never on a later close, so the condition holds under every shipped
cost profile; a test checks, under all four, that a held position's value moves
with the frozen closes to within `1e-12`. There is therefore no price-moving
market model in SharpeBench for the diagnostics to refuse. A market model in
which an entrant's orders move later prices, such as a shared order book, would
invalidate both.

Two further limits follow from the design. Both diagnostics replay decisions,
not the policy: an agent that would have decided differently at another time is
not re-run. And the random-timing reference keeps the entrant's cross-sectional
choices inside each holding period, so it tests when the entrant was invested,
not what it held.
