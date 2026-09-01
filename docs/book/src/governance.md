# Governance

SharpeBench is built by [General Liquidity](https://github.com/general-liquidity),
which also builds a trading agent (Gordon) that may compete on the board. A
benchmark hosted by an interested party only works if the host's interest cannot
bias the result. SharpeBench resolves that structurally rather than by asking for
trust.

## What can be checked, and what remains trusted

1. **The scorer is open and deterministic.** Anyone can run `sharpebench-core` on the same
   trajectories and get byte-identical scores. There is no private judge to lean
   on.
2. **Results can carry forward commitments.** Entrants bind artifact bytes before
   an operator-declared deadline (see [Forward attestation](attestation.md)). The
   commitment detects a later pre-image substitution; it does not prove wall
   time, data custody, or prior non-observation.
3. **Boards are tamper-evident.** A published board is an HMAC-signed chain; a
   silently edited or reordered result fails `verify`.
4. **The benchmark self-audits.** `sharpebench audit` proves no agent — including
   the host's — can win by gaming a gate.

The principle is **verify what the artifacts establish and name what they do
not**. Scores and signed history are independently checkable. The operator still
controls epoch advancement, held-out-data custody, intake, and the signing key;
neutral custody and a dispute process are governance work, not properties of the
hash chain.

## Relationship to other efforts

The **Open FinLLM Leaderboard** (FINOS + Columbia) measures the financial
*knowledge* axis — NLP, sentiment, QA, compliance — and has no
trading-performance / Sharpe / deflation track. SharpeBench is complementary: the
skill-vs-luck *trading* track that knowledge leaderboards lack. The intended path
is neutral governance via partnership rather than a rival leaderboard, with
forward commitments as an auditable binding within a shared-governance process.

## Licence

Dual **MIT OR Apache-2.0**, following the permissive open-source convention for
infrastructure meant to become a shared standard.
