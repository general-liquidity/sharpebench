# Why a new benchmark

Four contemporary trading/finance benchmarks were the starting point. Each is
valuable; none deflates for luck or tests reliability across runs.

| Benchmark | What it measures | What it lacks |
|---|---|---|
| **FinBen** | First LLM stock-trading eval (FinMem/FinTrade agent); CR/SR/DV/AV/MD. | Sharpe CIs ±1.08 → rankings statistically tied; no deflation, leakage control, or significance. |
| **QuantBench** | Full AI-quant pipeline (factor→model→portfolio→exec); IC/ICIR/SR/MDD/turnover + robustness/decay. | No Deflated Sharpe, no agent loop. |
| **StockBench** | LLM agent, multi-month, Dow-20, daily buy/sell/hold; CR/MDD/Sortino, contamination-free window. | Single window, single run, no deflation/significance. |
| **FinBench (QA)** | Static financial knowledge QA. | No trading or risk metrics at all. |

The shared gap is the **"return-rank = luck" trap**: ranking on a point estimate
of return (or Sharpe) over too little data, with no correction for how many
candidates were tried.

## What SharpeBench adds

- **Deflated Sharpe / PSR** — deflate the observed Sharpe by the number of
  leaderboard agents, the track length, and the return distribution's skew and
  kurtosis. A high Sharpe found after many tries is discounted to what it is
  worth.
- **pass^k reliability (mode "all")** — skill must show up on *every* seed ×
  window, not on average. One lucky seed cannot carry a submission.
- **Multiple-testing significance** — a stationary bootstrap p-value per agent,
  White's Reality Check across the field, and Romano–Wolf step-down for
  family-wise-error-controlled per-agent verdicts.
- **Process discipline** — block-severity gates over the audit trace: an order
  that skipped the risk gate, or a manipulative/absurd-size order, is
  disqualifying regardless of return.
- **Point-in-time rigor** — the simulator never hands an agent a future bar, so
  look-ahead is unrepresentable by construction.
- **Costs in** — fees, seeded slippage, and own-order market impact, so size and
  turnover are paid for.
- **A luck floor** — a random-agent baseline that shows the luck distribution the
  leaders must clear.

These are not knobs a contestant can tune around: they are deterministic
properties of the scorer, re-provable on demand (see
[Benchmark integrity](integrity.md)).
