# Governance

SharpeBench is built by [General Liquidity](https://github.com/general-liquidity),
which also builds a trading agent (Gordon) that may compete on the board. A
benchmark hosted by an interested party only works if the host's interest cannot
bias the result. SharpeBench resolves that structurally rather than by asking for
trust.

## Why hosting bias is neutralised

1. **The scorer is open and deterministic.** Anyone can run `sb-core` on the same
   trajectories and get byte-identical scores. There is no private judge to lean
   on.
2. **Results are forward-attested.** Entrants pre-commit to strategies before the
   grading data exists (see [Forward attestation](attestation.md)), so the host
   cannot tune the data to a favoured agent after the fact.
3. **Boards are tamper-evident.** A published board is an HMAC-signed chain; a
   silently edited or reordered result fails `verify`.
4. **The benchmark self-audits.** `sharpebench audit` proves no agent — including
   the host's — can win by gaming a gate.

The principle is **verify, don't trust**: the design assumes the host is an
interested party and removes every lever that interest could pull.

## Relationship to other efforts

The **Open FinLLM Leaderboard** (FINOS + Columbia) measures the financial
*knowledge* axis — NLP, sentiment, QA, compliance — and has no
trading-performance / Sharpe / deflation track. SharpeBench is complementary: the
skill-vs-luck *trading* track that knowledge leaderboards lack. The intended path
is neutral governance via partnership rather than a rival leaderboard, with
forward-attestation as the verify-don't-trust mechanism that makes shared
governance credible.

## Licence

Dual **MIT OR Apache-2.0**, following the permissive open-source convention for
infrastructure meant to become a shared standard.
