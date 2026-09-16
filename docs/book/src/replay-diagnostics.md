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
(the typical profile, for trajectories the CLI captures), so they refuse
`--allow-unbound-trajectory` and `--reexecute`. A malformed flag exits 2 before
any file is read; a trajectory the diagnostics cannot replay (see below) exits 1.

The library functions are `timing_null` and `lagged_replay` in
`sharpebench_sim::replay_nulls`. They take any `CostModel`.

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

The report also carries, for every run, the exposure profile (bars, invested
bars, holding periods, longest holding period, mean gross exposure when
invested) and the draws' mean invested bars. Without a liquidity cap or
execution noise that mean equals the entrant's invested bars exactly; under a
cap or noise the engine can hold a draw's position for a different number of
bars, and the two numbers show by how much.

Across runs, draw `i` of the reference is the mean of draw `i` over every run
that has timing freedom, and the entrant's mean per-run Sharpe is placed in that
distribution.

The draws are a pure function of the trajectory, the data, the cost model, the
declared draw count and the declared seed. Each run draws from its own stream,
derived from the seed and the run's position in the trajectory. The default
declaration is 200 draws from seed 0.

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

For lag `k` the first `k` bars hold (the book starts in cash, so they are flat)
and the last `k` recorded decisions fall after the window and never execute. A
lag of zero replays the recorded decisions unchanged and reproduces the
undelayed replay exactly. Every row, the undelayed one included, is computed on
the same bars: each run's bars after the first `max(k) + 1`, so each compared
bar is earned by a position every lag has already taken and no row carries its
opening trade. The report states how many leading bars it skipped. Lags are
reported ascending without repeats; an empty list, or a lag that leaves fewer
than two compared bars in some run, is refused.

### Decision delay

The stressed profile declares a two-bar decision delay that the backtest driver
does not apply. The lagged replay is the way to measure decision-delay
sensitivity: `lagged_replay` with the stressed profile's cost model and lag
`decision_delay_bars` replays every decision two bars late under the stressed
frictions. The stressed profile's own behaviour is unchanged, so evidence
produced under it keeps its meaning.

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
