# Regime-conditional comparison

The board deflates and tests significance on **pooled** returns. Pooling is a
real blind spot: two strategies can share a pooled distribution and still behave
nothing alike once you condition on the market state. One earns its whole edge
in a crisis and bleeds the rest of the time; the other is the mirror image.
Pooled, they look like twins. The edge half-life gestures at the same worry ("is
it a one-regime fluke?") but answers it only through time, not through state.

`sharpebench_core::compare_by_regime` takes per-period returns for two
strategies plus a regime label per period, and compares them *within* each
regime. It compares **distributions**, not just means. This is a reported
diagnostic: it is never read by the eligibility predicate and does not affect
rank.

## What it computes

Per regime, per strategy, a **ZAGA split** (zero-adjusted gamma, after the
"Regime-Conditional Distributional Comparison of Trading Strategies" paper):

- the zero / no-trade mass (periods with `|r| <= zero_tol`), which is how often
  the strategy simply did not play;
- the continuous part: mean, standard deviation, median, and the share of
  positive returns;
- a method-of-moments gamma (shape, rate) matched to the *magnitudes* of the
  continuous part.

Per regime, head to head:

- `mean_gap` (pooled within the regime) and `cont_mean_gap` (no-trade periods
  removed). When the two diverge, the pooled comparison was mostly measuring
  participation rate, not per-trade skill;
- `zero_mass_gap`, a behavioural difference that survives even when the means
  agree;
- a two-sample Kolmogorov-Smirnov statistic between the two continuous parts,
  so a pure shape difference (same mean, fatter left tail) still registers;
- `edge_sign` and whether the regime cleared `min_periods` and therefore
  `counted` toward the verdict.

Across regimes: the pooled mean gap and its sign, the list of counted regimes
whose sign contradicts it, `pooled_hides_reversal` (the headline finding: the
pooled number is averaging over a sign change), and `edge_dispersion`, the
spread between the best and worst counted regime. A large spread with no
reversal still says the edge is concentrated, not general.

Regimes come out in lexicographic label order, so the report is byte-identical
on every recompute.

## What it does NOT do

Verbatim from the module header, so nobody reads more into the output than is
there:

> **Deliberately NOT implemented:** a GAMLSS fit. There are no link functions,
> no penalised splines, no smooth covariate terms for mu / sigma / nu, no
> iterative backfitting, no likelihood maximisation, and no standard errors or
> p-values on fitted parameters. The gamma parameters here are moment matches
> on magnitudes, not maximum-likelihood estimates, and they are descriptive
> summaries rather than a fitted model. Nobody should read this module as
> evidence that a GAMLSS ZAGA fit lives in the crate. If you need the real
> thing, fit it elsewhere and feed the result in.
>
> Regime labels are an **input**. This module does not infer regimes and does
> not contain a classifier: whoever labels the periods owns that judgement, and
> burying a classifier here would let a regime definition quietly become part
> of the scoring kernel.

## Surfaces

- Rust: `sharpebench_core::{compare_by_regime, RegimeCompareOpts}`.
- CLI: `sharpebench regime <a.csv> <b.csv> <regimes.csv> [--json]`, see the
  [CLI reference](cli.md#regime).
- MCP: the `regime_compare` tool takes `returns_a`, `returns_b`, `regimes` and
  the optional `zero_tol` / `min_periods` / `tie_tol` knobs.

Defaults: `zero_tol = 1e-9`, `min_periods = 8`, `tie_tol = 1e-12`.
