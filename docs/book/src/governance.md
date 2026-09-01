# Governance

SharpeBench is built by [General Liquidity](https://github.com/general-liquidity),
which also builds a trading agent (Gordon) that may compete on the board. A
benchmark hosted by an interested party needs controls that expose some forms of
tampering without pretending to remove the host from the trust boundary.

## What can be checked, and what remains trusted

1. **The scorer is open and deterministic by construction.** Readers can run
   `sharpebench-core` on published trajectories. The cross-platform evidence is
   two committed Rust goldens checked on Linux, macOS, and Windows, as scoped in
   [Integrity and reproducibility](integrity.md).
2. **Results can carry forward commitments.** Entrants bind artifact bytes before
   an operator-declared deadline (see [Forward attestation](attestation.md)). The
   commitment detects a later pre-image substitution; it does not prove wall
   time, data custody, or prior non-observation.
3. **Boards are tamper-evident.** A published board is an HMAC-signed chain; a
   silently edited or reordered result fails `verify`.
4. **The benchmark exercises nine catalogued attacks.** `sharpebench audit`
   checks that those fixtures are demoted. It is a regression suite over named
   attacks, not a proof that no entrant can find another way to game the scorer.

The principle is **verify what the artifacts establish and name what they do
not**. Scores and signed history are independently checkable. The operator still
controls the reported chronology, epoch advancement, held-out-data custody,
intake, and the signing key. A verifying key identifies a host only when it is
authenticated through an independent channel. Prior non-observation, neutral
hosting, neutral custody, and a dispute process are governance work, not
properties of the hash chain.

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
