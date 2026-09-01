# Forward attestation

The strongest intended defense against an overfit leaderboard is temporal: make
an agent commit before it can observe the data it will be graded on.
`sharpebench-attest` supplies the cryptographic binding for that protocol; it
does not itself prove the chronology.

## Pre-registration commitments

Before the target window opens, an entrant publishes a SHA-256 **commitment** to
its artifact (model hash, config, or strategy digest) plus a salt:

```sh
sharpebench commit my-agent 2026-Q3 <artifact_digest> <salt>
```

The commitment reveals nothing about the strategy, but later the entrant reveals
the artifact and salt, and anyone can use `verify_commitment` to check that the
revealed bytes match the earlier commitment. This prevents an undetectable
pre-image substitution after commitment. It does not prove when the commitment
was made, when the data became available, or what the entrant observed.

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

The `Registry` lock uses explicit integer epochs rather than a wall clock, which
keeps every commitment refusal deterministic. The operator advances those
epochs and controls intake, held-out-data custody, reveal timing, and the signing
key. A forward claim therefore assumes an independently auditable mapping from
epochs to wall time, evidence that target data stayed unavailable to entrants
before commitment, and preservation of the commitment and custody records.
Cryptography binds bytes and order; it does not supply those observations.
