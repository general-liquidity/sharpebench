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

The commitment reveals nothing about the strategy, but later, once results are
in, the entrant reveals the artifact and salt, and anyone can `verify_commitment`
that the revealed artifact matches what was committed. An agent cannot retrofit a
strategy to data it pre-committed against.

## Shared-key HMAC boards

A board can carry an HMAC-signed **chain**: each entry is signed over the prior
entry's signature, so corruption or modification by a party without the key
breaks verification.

```sh
sharpebench sign submissions.json <key> board.json   # score + sign
sharpebench verify board.json <key>                  # verify the chain
```

`verify` exits non-zero if the chain or key does not match. HMAC is a
shared-secret construction: every keyholder can both verify and forge a complete
replacement chain. It detects accidental corruption and edits by non-keyholders;
it does not make a host independently accountable to the public.

## Public Ed25519 boards

Public verification uses Ed25519. The signer keeps the signing key private and
publishes the verifying key through an independent channel. A verifying-key
holder can check the chain but cannot forge a replacement. The forward Arena
publishes Ed25519 boards and links consecutive windows so replacing an earlier
board changes the anchor recorded by the next one.

The `Registry` time-lock uses explicit integer epochs rather than a wall clock,
which keeps every commitment refusal deterministic. Public credibility still
depends on publishing the verifying key independently and preserving the
pre-registration and dataset commitments.
