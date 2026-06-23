# Methodology

A submission is a set of **runs**: one return series (plus an optional decision
trace and per-decision confidences) for each seed × window. The scorer
(`sharpebench_core::rank`) turns a field of submissions into a ranked board.

An agent is **rank-eligible only if every gate holds**:

```text
eligible = DSR ≥ dsr_bar          (survives multiple-testing deflation)
         ∧ pass^k                  (clears the per-run bar on EVERY run)
         ∧ process_ok              (zero block-severity trace violations)
         ∧ bootstrap_p < alpha     (edge beats the stationary-bootstrap null)
         ∧ mandate_ok              (respected its drawdown mandate)
```

Eligible agents sort by the **rank key** (Deflated Sharpe by default, or Alpha).
Ineligible agents sort last, by raw return, for display only — raw return never
buys rank.

The composite also *reports* (without gating, to keep the default behaviour
stable): alpha/beta attribution vs the field, calibration (Brier), edge half-life,
the field-wide Reality Check p-value, the Romano–Wolf step-down verdict, max
drawdown, turnover, Pareto-optimality, confidence-weighted return, cost-efficiency,
rolling worst-case Sharpe, selection robustness, and the **Sortino ratio** with its
downside deviation (excess return per unit of *downside* volatility, MAR = 0 — it
rewards an edge that doesn't arrive with downside churn, where the Sharpe penalizes
all volatility symmetrically).

The following sections explain each gate.

- [Deflated Sharpe & PSR](methodology-deflated-sharpe.md)
- [pass^k reliability](methodology-pass-k.md)
- [Significance & multiple testing](methodology-significance.md)
- [Process discipline](methodology-process.md)
