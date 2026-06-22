# Forward attestation

The deepest defense against an overfit leaderboard is temporal: make an agent
**commit to its strategy before the data it will be graded on exists**. SharpeBench
supports this with `sharpebench-attest`.

## Pre-registration commitments

Before the target window opens, an entrant publishes a SHA-256 **commitment** to
its artifact (model hash, config, or strategy digest) plus a salt:

```sh
sharpebench commit my-agent 2026-Q3 <artifact_digest> <salt>
```

The commitment reveals nothing about the strategy, but later — once results are
in — the entrant reveals the artifact and salt, and anyone can `verify_commitment`
that the revealed artifact matches what was committed. An agent cannot retrofit a
strategy to data it pre-committed against.

## Tamper-evident signed boards

A published board is an HMAC-signed **chain**: each entry is signed over the prior
entry's signature (genesis-anchored), so a single altered or reordered row breaks
the chain. The leaderboard host cannot quietly edit a result after the fact.

```sh
sharpebench sign submissions.json <key> board.json   # score + sign
sharpebench verify board.json <key>                  # verify the chain
```

`verify` exits non-zero if the chain is tampered or the key is wrong. Combined
with a `Registry` time-lock (integer epoch, no wall clock — to keep scoring
deterministic), this lets the benchmark be hosted by an interested party (even a
competitor on the board) without requiring anyone to *trust* the host: you verify
instead.
