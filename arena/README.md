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

## Window 001 (open)

| Field | Value |
|---|---|
| Window id | `window-001` |
| Opened at epoch | 20689 (2026-08-24 UTC) |
| Commit deadline | epoch 20710 (2026-09-14 UTC), commitments at or after this epoch are refused |
| Data reveal / resolution | epoch 20719 (2026-09-23 UTC), scoring runs on data revealed here |
| Scoring rules | recorded in `windows/window-001/window.json` at open time, before any entry or any forward data exists |
| Scoring config SHA-256 | `5050b3aa20298bd188a9418e5e76c0ff4de2027732920d8617d5e176448b8bcf` (over the compact serialization of the typed `score_config`, in schema field order) |

The recorded config is the shipped default for daily bars: `n_trials = 50`,
annualized `trials_sr_std` prior 0.5 with the measured path enabled at a
minimum field of 5 and bounded below by annualized dispersion 0.5,
`dsr_bar = 0.95`, per-run PSR bar 0.90 in pass-all mode, and
`periods_per_year = 252`. The v1 loader requires the complete score-config key
set and rejects missing or extra fields before deserialization, so later serde
defaults cannot silently change this opened window.

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
