# SharpeBench forward arena, operating record

This directory is a live `sharpebench arena` state directory (see
`docs/book/src/arena.md`). It is committed to the repository so the arena's
operating history is public from its first window onward: every state
transition is a file write here, every published board will be an Ed25519
chain verifiable with only the key below.

## Epoch scheme

Epochs are integer **days since the Unix epoch, UTC**: `epoch = floor(unix_time / 86400)`.
The operator (or CI) advances the clock with `sharpebench arena advance arena <epoch>`.
Epochs are monotonic; the kernel never reads a wall clock.

## Superseded pre-entry records (no window is currently open)

Two windows were opened and withdrawn before any commitment, refusal or score
existed. Both records remain in `windows/` and `state.json` as history; neither
is live, and the next window is not open yet.

| Window id | Opened at epoch | Superseded at epoch | Reason |
|---|---|---|---|
| `window-001` | 20689 (2026-08-24 UTC) | 20689 | schema v1 omitted `execution_seeds_per_window`, the fixed deflation null mean, and scorer artifact provenance; replaced by `window-002` |
| `window-002` | 20689 (2026-08-24 UTC) | 20689 | withdrawn before commitments: its recorded scorer digest identified a local debug executable, not a publicly retrievable immutable artifact |

The next window opens only against a published release of the scorer (a
crates.io version or an immutable image digest) recorded in the window before
commitments open, so an entrant can retrieve byte-for-byte the kernel that will
score the reveal. `state.json` carries both supersession records with the
SHA-256 of each historical window file.

## Host verifying key

The window's board will be signed at publish time with an Ed25519 key held by
the operator. Only the public half appears anywhere in this repository:

- Verifying key (hex): `9495861d442e2ce2f77e5b608a50c3905a51ce0171878a85551875434b336974`
- Fingerprint (SHA-256 of the 32 raw key bytes, first 16 hex): `48756712e3c18a7b`

Verify any published board and the cross-window chain with:

```
sharpebench arena verify arena --pubkey 9495861d442e2ce2f77e5b608a50c3905a51ce0171878a85551875434b336974
```

The signing secret lives outside the repository in the operator's home
directory and is referenced at publish time as
`file:~/.sharpebench/arena-window1.key`. It is not, and must never be,
committed here.

## How to enter

Produce a commitment before the deadline with `sharpebench commit` and deliver
the JSON out of band (a PR against this directory is fine); the operator
registers it with `sharpebench arena commit arena window-001 <commitment.json>`.
At the reveal epoch, reveals that do not match their registered commitment are
refused and the refusal is part of the permanent signed record.
