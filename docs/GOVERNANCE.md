# Governance & Neutrality

A benchmark's authority comes from trust, and a benchmark published by a company
that also *competes on it* has a built-in conflict of interest. SharpeBench
addresses this on two fronts — a technical one we control today, and a
governance path we're explicit about.

## 1. Technical neutrality (today): verify, don't trust

You should not have to trust General Liquidity's word for any number on the
board. Two mechanisms in [`sb-attest`](../crates/sb-attest) make results
independently checkable:

- **Forward-attestation.** An agent publishes a SHA-256 *commitment* binding its
  frozen artifact to a target window **before that window's data exists**. There
  is nothing to overfit, and revealing the pre-image later proves the agent
  wasn't tuned to the window. This is the un-gameable spine: it defeats backtest
  overfitting *structurally*, not just statistically.
- **Signed, tamper-evident results.** Every scored result is HMAC-signed over the
  previous one, forming a chain (the same construction Gordon uses for its audit
  log). Anyone can recompute a published rank from the pre-registered artifact
  and the committed scoring kernel (`sb-core`, deterministic, `Cargo.lock`-pinned)
  and confirm the chain wasn't altered.

So the credible claim is not "trust us" — it's "**reproduce it**."

## 2. Governance path: GL-hosted now → neutral foundation

General Liquidity hosts SharpeBench to start, because shipping and earning
adoption beats a perfect-but-nonexistent neutral standard. But the goal is
neutral, multi-stakeholder governance, and we say so on day one. Gordon (GL's
agent) competes on the board like any other entrant and **does not** operate the
grading.

### The FINOS / Open FinLLM Leaderboard angle

The natural home already exists. The **[Open FinLLM Leaderboard
(OFLL)](https://huggingface.co/spaces/finosfoundation/Open-Financial-LLM-Leaderboard)**
is governed by **FINOS** (the Fintech Open Source Foundation, part of the Linux
Foundation) with Columbia — a credible, neutral, community-backed home for
financial-LLM evaluation.

But OFLL evaluates the **knowledge axis**: financial NLP, sentiment, headline
classification, QA, document analysis, compliance. It has **no trading-performance
axis** — no Sharpe, no risk-adjusted returns, no deflation, no skill-vs-luck. Its
own charter says "Financial LLMs **and Agents**," yet the agent-trading track does
not exist.

That is exactly the gap SharpeBench fills. So the strategic path is not to build a
rival leaderboard and fight OFLL for the "financial AI benchmark" brand — it is to
become **the trading-performance / skill-vs-luck track that OFLL and FINOS lack**,
contributed under (or alongside) their neutral governance:

- OFLL/FINOS bring the neutral host, the brand, and the community.
- SharpeBench brings the methodology no existing financial-AI benchmark has:
  deflated Sharpe, pass^k reliability, process discipline, and forward-attestation.

Knowledge benchmarks (OFLL/FinBen) ask *"does the model know finance?"*
SharpeBench asks *"can the agent trade with skill that survives deflation?"* —
complementary axes, not competitors. (StockBench's own finding underlines why both
are needed: strong static-QA performance does **not** translate into effective
trading.)

## Contributing to governance

If you represent FINOS, a foundation, an exchange, or an academic group
interested in a neutrally-governed trading-agent track, open an issue — neutral
governance is a feature we want, not a threat we're guarding against.
