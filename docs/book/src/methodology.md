# Methodology

A submission is a set of **runs**: one return series (plus an optional decision
trace and per-decision confidences) for each seed × window. The scorer
(`sharpebench_core::rank`) turns a field of submissions into a ranked board.
Seeds remain separate for pass^k, but pooled statistics first average aligned
seed returns inside each window and then concatenate windows. Replicating an
execution seed therefore cannot multiply the apparent sample size.

An agent is **rank-eligible only if every gate holds**:

```text
eligible = DSR ≥ dsr_bar          (survives multiple-testing deflation)
         ∧ pass^k                  (clears the per-run bar on EVERY run)
         ∧ process_ok              (zero block-severity trace violations)
         ∧ bootstrap_p < alpha     (edge beats the stationary-bootstrap null)
         ∧ mandate_ok              (respected its drawdown mandate)
```

Eligible agents sort by the **rank key** (Deflated Sharpe by default, or Alpha).
Ineligible agents sort last, by raw return, for display only; raw return never
buys rank.

A submission may also **declare a mandate** (see
[pass^k reliability](methodology-pass-k.md)): the declared verdict is scored and
reported beside the board verdict as a labeled column and never moves rank.

Generated-candidate lineage is another reported-only surface. When a
SharpeArena strategy search supplies a v2 ledger, `sharpebench lineage`
independently verifies its raw trial count, candidate ancestry, source
citations, and host-derived family grouping, then reports best-versus-median DSR
inside each family. Those groups never deduplicate trials or enter the composite
score. See [Candidate lineage diagnostics](candidate-lineage.md).

The composite also *reports* (without gating, to keep the default behaviour
stable): alpha/beta attribution vs the field, calibration (Brier), edge half-life (per-window return drift, not information-coefficient decay),
the field-wide Reality Check p-value, the Romano–Wolf step-down verdict, max
drawdown, turnover, Pareto-optimality, confidence-weighted return, cost-efficiency,
rolling worst-case Sharpe, selection robustness, and the **Sortino ratio** with its
downside deviation (excess return per unit of *downside* volatility, MAR = 0). It
rewards an edge that doesn't arrive with downside churn, where the Sharpe penalizes
all volatility symmetrically.

## What the rank does not answer

Two boundaries come straight from Sharpe's own 1994 statement of the ratio, and
both are scope, not defect.

**The ratio ignores correlations, so rank one is a choose-one verdict.** Sharpe
is explicit that the ratio takes no account of correlations, and that when a
choice may affect important correlations with other holdings, that information
should supplement the comparison. SharpeBench scores every agent standalone:
each submission's runs are its own, and the alpha/beta figures regressed against
the field's equal-weight mean are marginal associations reported for
attribution, never a portfolio construction. So the board answers "which single
agent would I rather hold on its own", not "which agent adds the most to what I
already hold". An agent ranked fourth whose returns are uncorrelated with your
book can be the better marginal addition, and nothing here will say so. That
comparison needs the candidate's return stream against your existing one.

**Every figure is ex post.** The Sharpe, the Deflated Sharpe, the PSR and the
pass^k verdict are all computed on returns that already happened. Sharpe warns
that using unadjusted historic ex-post ratios as surrogates for unbiased
predictions of ex-ante ratios is subject to serious question, and the warning
applies here: a rank is a statement about the recorded windows, not a forecast
of the next one. The deflation, the reliability gate and the bootstrap null
exist to stop a lucky in-sample figure being read as skill, which pushes against
the same worry, but none of them converts an ex-post measurement into an ex-ante
one.

The following sections explain each gate, then one reported-only diagnostic that
the pooled gates cannot see.

- [Deflated Sharpe & PSR](methodology-deflated-sharpe.md)
- [pass^k reliability](methodology-pass-k.md)
- [Significance & multiple testing](methodology-significance.md)
- [Process discipline](methodology-process.md)
- [Regime-conditional comparison](methodology-regime.md) (reported, never gating)
- [Candidate lineage diagnostics](candidate-lineage.md) (reported, never gating)
