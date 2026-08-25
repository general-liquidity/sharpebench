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

In JSON, `pass_mode` is `"all"`, `"any"`, `{"at_least": 3}` or
`"relative_to_benchmark"`.

## A third verdict: beats the benchmark in every regime

Both verdicts above are absolute: the per-run test compares each run's Sharpe
to a fixed bar, zero by default. That encodes an all-weather absolute-return
mandate, and it is why the index itself cannot pass on any range containing
2020 or 2022. Real mandates are mostly relative: an allocator asks whether the
manager beat owning the universe, not whether every quarter was positive.

`PassMode::RelativeToBenchmark` is that verdict, opt-in. It aggregates exactly
as `All` (every window, every seed), but each run is tested on its **excess**
return over the benchmark agent's run in the same (window, seed) cell:

```text
e_t   = r_t(agent, window w, seed s) - r_t(benchmark, window w, seed s)
pass  iff  std(e) > 0  and  PSR(e, per_run_min_annual_sharpe / sqrt(periods_per_year)) >= per_run_psr_bar
```

The benchmark is `ScoreConfig::benchmark_agent_id` (default `"buy-and-hold"`),
looked up **by id in the field being ranked**. Its run at position `i` is the
same cell as every other agent's run `i`, produced from the same frozen bars
with the same window, execution seed and cost model, so nothing is fetched from
outside the field and there is no leakage. A field without the named agent, or
a misaligned cell, fails the run; it never falls back to the absolute test.
`score_agent` has no field and therefore fails every run under this mode.

Two rules make the verdict non-trivial:

- **A zero-excess run fails.** The benchmark's own excess series is identically
  zero, and a zero-dispersion series is no evidence of outperformance:
  `sharpe_ratio` defines it as 0 and PSR then returns `norm_cdf(0) = 0.5`, so it
  already fails any bar above one half. The `std(e) > 0` clause makes the
  refusal unconditional, so lowering `per_run_psr_bar` to 0.5 does not admit
  the benchmark, and a clone of the benchmark under another id fails for the
  same reason. The verdict is a claim about excess edge in every window; a run
  indistinguishable from the benchmark has none.
- **Only pass^k changes.** The pooled Deflated Sharpe, the bootstrap, the
  process audit and the drawdown mandate are all still computed on the agent's
  own raw returns. Beating the index in every window with no absolute edge is
  refused on deflation, as before. This is a different reliability question,
  not a weaker set of gates.

| | default | `ScoreConfig::relative_to_benchmark(id)` |
|---|---|---|
| `pass_mode` | `All` | `RelativeToBenchmark` |
| `benchmark_agent_id` | `"buy-and-hold"` (unused) | `id` |
| per-run series | raw returns | excess over `id`'s run in the same cell |
| certifies | profitable in every regime with 90% confidence | beats `id` in every regime with 90% confidence |
| every other gate | unchanged | unchanged |

`sharpebench_core::per_run_passes` exposes the kernel's per-run vector so a
report can say which windows passed, not only whether all did. On the CLI,
`sharpebench run --pass-mode relative-to-benchmark [--benchmark-agent <id>]`
and the same flags on `sharpebench score` select it; in Python,
`relative_to_benchmark_config(id)` serializes the preset. The default is
unchanged: with the flags absent, `pass_mode` deserializes to `All` and the
benchmark id is never read, and the golden fixtures are byte-identical.

## Declaring a mandate at submission

The three verdicts above are selected by the host. A submitter can also declare
one, on the submission itself: `DeclaredMandate` (in `sharpebench-protocol`,
carried on `AgentTrajectory` and accepted as a `declared_mandate` key on a JSON
submission object) names the reliability verdict the agent asks to be judged
under.

| declaration (JSON `kind`) | resolves to | certifies |
|---|---|---|
| `absolute_return` | pass^k `All` on raw returns | profitable in every regime (the default verdict, restated) |
| `relative_to` + `benchmark_id` | pass^k `RelativeToBenchmark` against that id | beats the named agent in every regime |
| `outperform_buy_and_hold` | `relative_to` buy-and-hold | beats buy-and-hold in every regime |
| `drawdown_capped` + `max_per_run_drawdown` | pass^k `Any` plus the declared per-run bound | has an edge and never draws down past the bound in any single run |

The rule, and it is the whole design: **a declaration selects which reliability
question pass^k asks and, for `drawdown_capped`, adds a per-run bound; it never
relaxes a gate.** The deflated-Sharpe bar, the block bootstrap, the process
audit and the host's drawdown mandate are computed on the agent's raw returns
under every declaration. A misdeclared benchmark (an id the field does not
contain, or a misaligned cell) fails the declared verdict closed, exactly as
the host-side relative verdict does, and a declared bound outside `(0, 1]` is a
misdeclaration and fails. Buy-and-hold declaring `outperform_buy_and_hold` is
judged against itself and fails on the zero-excess rule. The former
`long_only_beta` spelling remains a read-only compatibility alias for old
artifacts; it did not describe the excess-return test correctly.

Eligibility is reported per verdict, and the two verdicts never mix:

- `rank_eligible`, the sort key and `rank_ordinal` are the host verdict's,
  byte-identical with or without declarations.
- The declared verdict is an additional labeled column on the same score:
  `declared_mandate` (what was asked), `verdict_applied` (what was tested),
  `declared_passed_k`, and `declared_mandate_eligible` (the host predicate with
  only the pass^k question exchanged, plus the declared bound where one
  exists).
- `declared_mandate_ordinal` orders declared-eligible agents by deflated
  Sharpe within their **mandate class** only (one resolved verdict; the same
  benchmark id, the same bound). Agents under different mandates answer
  different questions and are never ordered against each other, or against the
  host board.

The board row prints both clauses, so a reader sees, for example, `fails
ineligible under declared verdict (relative to buy-and-hold); host-board ineligible` on buy-and-hold's
row instead of a bare refusal. In Rust the entry points are
`rank_declared(subs, &declarations, &cfg)` and
`score_agent_declared(sub, declared, &cfg)`; `rank` is the empty-declaration
special case and the golden fixtures are byte-identical. On the CLI,
`sharpebench score` accepts the `declared_mandate` key on each submission
object; the trajectory verifier carries the artifact's declaration through
`verify_trajectory`. Evidence for the reference agents under matching
declarations is committed at `paper/evidence/final/mandate-declaration.jsonl`:
no reference agent meets its declared mandate. The nearest miss, the
risk-managed agent's `drawdown_capped` 0.20 declaration on daily crypto, is
the only declared reliability pass; it is still refused by both deflation
(DSR 0.2118) and the stationary bootstrap (p 0.1949).
