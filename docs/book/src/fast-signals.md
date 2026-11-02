# Fast signals

The skill verdict needs a long track, because a Sharpe ratio is estimated
slowly. Several behaviours the benchmark already records can be measured
precisely after a few runs, and they separate agents long before the verdict
can. The fast-signal report (`sharpebench.fast-signals.v1`,
`sharpebench_core::fast_signals`) collects them beside the skill verdict.

No measure in the report is evidence of trading skill. An agent can keep a
clean process, stay inside its mandate, trade little and never fault, and
still have no edge at all. The report is rank-neutral: no score, rank,
ordinal, gate, pass^k pool or headline verdict reads it, and every measure
carries `skill_evidence: false`.

## The measures

Each measure states its scale, its direction (`higher_is_better` or
`lower_is_better`), where its inputs came from, a value, the number of units
behind it and a 95% interval. A measure whose inputs are missing is reported
with `value: null` and the reason, never left out.

| Measure | Family | Scale | Better | Input |
|---|---|---|---|---|
| `process_clean_runs` | process | share of runs with no block-severity process violation | higher | submission trace |
| `warn_violations_per_run` | process | warn-severity violations per run | lower | submission trace |
| `mandate_compliant_runs` | mandate | share of runs whose own maximum drawdown is within the per-run cap | higher | submission returns |
| `orders_per_run` | turnover | orders placed per run, the board's `turnover` | lower | submission trace |
| `cost_per_run` | cost | host-observed USD per run, or entrant-reported cost per run | lower | `--host-cost`, else the submission |
| `agent_fault_cells` | refusal and fault | share of evaluable declared cells that ended in an agent fault | lower | `--suite-evidence` |
| `contamination_mean_edge` | contamination | mean real-minus-twin per-bar Sharpe per window | lower | `--contamination-probe` |
| `presentation_disagreement` | presentation | share of compared steps whose decision changed under another presentation | lower | `--presentation-stability` |
| `decision_disagreement` | decision stability | share of replicate pairs on a shared history that decided differently | lower | `--decision-stability` |

`mandate_compliant_runs` uses the per-run cap of the scoring configuration,
which is 1.0 (unconstrained) by default, so the measure separates agents only
under a declared cap.

`cost_per_run` never mixes sources, following the host cost columns of
[the model gateway](model-gateway.md). When host-observed spend is supplied, it
replaces the entrant figure. It is one total per agent, so the measure has a
value and no interval, and says so. Without it, the entrant-reported `cost` is
used only when every run reports a positive amount.

`agent_fault_cells` reads the trial census. A cell the harness reported as
`agent_fault: <kind>` counts against the agent, and `detail` lists the faults
by kind, for example `agent_fault:decision_budget_exceeded`. Runtime failures,
unreported cells, duplicate reports, off-roster reports and any other typed
failure are the harness's or unestablished, so they leave the denominator and
are listed under `excluded:` in `detail`.

## Intervals

A share of countable units (runs, cells, steps, replicate pairs) carries the
exact Clopper-Pearson binomial interval. A mean over runs or windows carries a
percentile bootstrap over that unit with 2000 resamples under a fixed seed, so
the same input gives the same bits on every platform. One unit gives a value
and no interval.

Both kinds treat their unit as independent, and `interval.sampling_unit` names
the unit. Runs of one window share its bars, and the steps of a presentation
pair share one book, so where the unit is not independent the interval is
narrower than the truth.

## Running it

```bash
sharpebench signals field.json [--suite-evidence run.json] [--host-cost board.json] \
  [--contamination-probe <agent>=probe.json] [--presentation-stability <agent>=suite.json] \
  [--decision-stability stability.json] [--json]
sharpebench score field.json --signals [--json]
```

`score --signals` reports the measures a submissions file supports, after the
unchanged board; the evidence flags belong to `signals`. The evidence documents
are the `--json` outputs of `run --suite-evidence`, `contamination-probe`,
`presentation-stability` and `decision-stability`, and a JSON board whose rows
carry host-observed usage (`attach_host_observed_usage`). See
[the CLI reference](cli.md#signals).
