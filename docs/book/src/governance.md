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
3. **Forward scores come from the committed artifact's recorded decisions.** A
   forward window ranks returns it replays from a strict capture that names the
   committed artifact, the revealed dataset and the window's execution matrix,
   and each published row says so (`returns_provenance`). Replay does not show
   the decisions were made without hindsight: anyone holding the revealed data
   can record hindsight decisions that replay exactly. Only a row re-executed
   from the committed artifact (`arena score --reexecute`, for a pinned image or
   a reference agent) shows the artifact itself makes those decisions on
   point-in-time observations, and that still rests on the operator's data
   custody. Returns an entrant supplies after the reveal are ranked only on a
   board signed as noncertifying. See [returns intake](arena.md#returns-intake).
4. **Boards are tamper-evident.** A published board is an HMAC-signed chain; a
   silently edited or reordered result fails `verify`.
5. **The benchmark exercises ten catalogued attacks.** `sharpebench audit`
   checks that those fixtures are demoted: nine against the scoring kernel,
   which the WASM, npm and MCP `self_audit` surfaces also run, and a tenth,
   `forward-hindsight-oracle`, a next-bar oracle delivered through the forward
   arena, which needs the simulator and the arena and runs in the CLI only. That
   case first shows the oracle's returns are rank-eligible when ranked directly,
   so it credits intake, not the statistics. The suite is a regression suite
   over named attacks, not a proof that no entrant can find another way to game
   the scorer.

The principle is **verify what the artifacts establish and name what they do
not**. Scores and signed history are independently checkable. The operator still
controls the reported chronology, epoch advancement, held-out-data custody,
intake, and the signing key. The intake the operator chose is on the board: each
forward row names how its returns were obtained, and a window that accepted
supplied returns is signed as noncertifying. A verifying key identifies a host only when it is
authenticated through an independent channel. Prior non-observation, neutral
hosting, neutral custody, and a dispute process are governance work, not
properties of the hash chain.

## Relationship to other efforts

The **Open FinLLM Leaderboard** (FINOS + Columbia) measures the financial
*knowledge* axis (NLP, sentiment, QA, compliance) and has no
trading-performance / Sharpe / deflation track. SharpeBench is complementary: the
skill-vs-luck *trading* track that knowledge leaderboards lack. The intended path
is neutral governance via partnership rather than a rival leaderboard, with
forward commitments as an auditable binding within a shared-governance process.

## Licence

Dual **MIT OR Apache-2.0**, following the permissive open-source convention for
infrastructure meant to become a shared standard.
