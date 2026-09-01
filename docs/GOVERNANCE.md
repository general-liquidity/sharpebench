# Governance and neutrality

A benchmark published by a potential entrant has a structural conflict of
interest. SharpeBench separates the technical evidence that can be verified now
from governance arrangements that do not yet exist.

## Current status

The repository publishes a deterministic benchmark implementation, frozen
teaching fields, and a forward-window protocol. It does not currently operate a
hosted intake service or an admitted external-model leaderboard. No Gordon or
other LLM trading-agent result appears in the paper evidence.

## What can be checked independently

Three mechanisms reduce the amount a reader must take on trust:

- **Forward-attestation.** An agent publishes a SHA-256 *commitment* binding its
  frozen artifact to a target window **before that window's data exists**. There
  is nothing to overfit, and revealing the pre-image later proves that the
  revealed artifact matches the commitment.
- **Recomputable scoring.** A reader can replay published decisions under the
  committed dataset, cost model, scorer, windows, and seeds.
- **Public result signatures.** Ed25519 boards can be checked with a separately
  obtained verifying key. HMAC chains are also supported, but every HMAC
  keyholder can forge; they are not a public-verification substitute.

These controls establish consistency with published inputs. They do not prove
that participant identity, dataset publication, scheduling, or key custody was
neutral.

## Governance path

If a public competition is launched, host and entrant roles should be separated,
the scorer and field specification should be committed before intake, the
verifying key should be published through an independent channel, and governance
should include parties that do not compete on the board.

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

That is the gap SharpeBench is designed to fill. A possible path is not to build a
rival leaderboard and fight OFLL for the "financial AI benchmark" brand, but to
become **the trading-performance / skill-vs-luck track that OFLL and FINOS lack**,
contributed under (or alongside) their neutral governance:

- OFLL/FINOS bring the neutral host, the brand, and the community.
- SharpeBench brings the methodology no existing financial-AI benchmark has:
  deflated Sharpe, pass^k reliability, process discipline, and forward-attestation.

Knowledge benchmarks (OFLL/FinBen) ask *"does the model know finance?"*
SharpeBench asks *"can the agent trade with skill that survives deflation?"*,
complementary axes, not competitors. (StockBench's own finding underlines why both
are needed: strong static-QA performance does **not** translate into effective
trading.)

## Contributing to governance

If you represent FINOS, a foundation, an exchange, or an academic group
interested in a neutrally-governed trading-agent track, open an issue. Neutral
governance is a feature we want, not a threat we're guarding against.
