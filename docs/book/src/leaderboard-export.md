# Leaderboard export

`sharpebench leaderboard export` turns signed boards into two files a person or
an agent can read without running the scorer: a canonical JSON record and a
Markdown table. It is a command-line and agent surface. It writes files or
prints to standard output; there is no page, script, server or hosting
configuration, and none is planned.

```bash
sharpebench leaderboard export <arena-dir | board.json> [--out <dir>] [--pubkey <hex>] [--expect-head <count>:<sha256>] [--key <secret>] [--public] [--json]
```

With `--out <dir>` the command writes `leaderboard.json` and `leaderboard.md`
into `<dir>` and prints a one-line summary (a small JSON object under
`--json`). Without `--out` it prints the Markdown table, or the JSON record
under `--json`, to standard output, so an agent can read it from a pipe.

## Verification comes first

Nothing is exported from an input that does not verify. A refused input exits
1, prints why, and writes no file.

- **Arena directory.** The directory is checked exactly as
  [`arena verify`](arena.md#cross-window-chaining) checks it: every board's
  Ed25519 chain, the cross-window anchors, the identity fields, the forward
  evidence ledger and the signed head. `--pubkey` pins the arena key and
  `--expect-head` requires the history to pass through a head seen out of
  band. Any failure refuses the whole export. The boards are then read again
  for their rows, and each is checked a second time on the bytes actually
  read: its chain under the verified key, its window id, its anchor to the
  previous board, and the newest final signature against the verified head. A
  board replaced between the two reads is refused.
- **Board file.** A board written by `sharpebench sign` is checked under its
  HMAC key (`--key <secret>`), or under its Ed25519 chain, pinned
  (`--pubkey <hex>`) or trusting the embedded key (`--public`). Exactly one of
  the three is required. An Ed25519 board must carry its signed receipt, so a
  board with trailing rows removed does not verify.

Rows are read only from signed links. The unsigned `scores` array a board file
also carries and the unsigned `window.json` are never read for a row.

`<secret>` and `<hex>` accept a literal, or `env:NAME` and `file:PATH` to keep
them out of process listings. Usage errors exit 2.

## The record

The JSON record has the schema `sharpebench.leaderboard-export.v1`:

| Field | What it holds |
|---|---|
| `source` | `kind` (`arena` or `signed_board`), `scheme` (`ed25519` or `hmac-sha256`), `verifying_key`, `key_pinned`, `completeness` and `completeness_limit` in the verifier's own words, and `head` (`<count>:<sha256>`, to pin later) |
| `boards` | one record per verified board, in publish order: `board_id`, `board_path`, `window_path`, `final_signature_sha256`, `dataset_hash`, `score_config_sha256`, `scorer_artifact_sha256`, `certifying`, `supplied_returns_accepted`, and the counts of `rows` and `refusals` |
| `rows` | one record per signed score link, in chain order, then one per refusal the board header records |

Each row holds:

| Field | Meaning |
|---|---|
| `headline` | the [headline verdict](reading-a-row.md) with its hard reasons, cause and qualifiers |
| `tier` | the row's tie band (`tie_group`) when it is rank-eligible; rows in one tier are statistically indistinguishable on the deflated Sharpe. `null` for a row that cannot be ranked |
| `rank_eligible` | the scorer's eligibility |
| `dsr`, `dsr_interval`, `dsr_interval_error` | the deflated Sharpe and its bootstrap interval. `dsr` is `null` when the deflation family refused the track or could not compute it, because the stored value is then a floor, not a figure |
| `pass_k` | `passed`, `runs_passed` and `runs_scored` |
| `cost` | `source`, `total`, `return_per_cost`, `dsr_per_cost`. A signed board carries the cost the entrant reported, so `source` is always `entrant_reported`; `total` is `null` when no cost was reported |
| `returns_provenance` | how an arena row's returns were obtained: `supplied`, `replayed`, `re-executed` or `live-captured` |
| `record` | `board_path`, `window_path`, the chain position `link` and its `link_signature` |

A refusal row has the verdict `refused`, the refusal text as its cause, link 0
(the header link that signs the refusal) and no figures.

The evidence qualifier uses the headline's meaning of "certifying" (see
[three meanings](reading-a-row.md#three-meanings-of-certifying)). An arena row
has the mode `forward`; it is `certifying` exactly when the signed header says
the board certifies, and otherwise `noncertifying` with the board's flag and
any accepted supplied returns as reasons. A refusal and a row from a board file
carry no evidence mode, so they read `unknown`. No signed board records a
stress replay, so `stress` reads `not_run`.

A board file records no score configuration. `sharpebench sign` always scores
under the default configuration, so its rows are read under that, and the
stated `completeness_limit` says so.

## The table

`leaderboard.md` is the record's reading, not a second record. It states the
source and the verification limit, lists the boards, and prints one table row
per record row with the columns board, record (`<board_path>#<link>`), agent,
verdict, tier, DSR, DSR interval, pass^k, eligible, evidence, cost, qualifiers
and reasons. A figure that does not exist prints `unavailable` on a scored row
and `-` on a refusal. Pipes and backslashes in signed text are escaped and
control characters become spaces, so an agent id cannot break the table.

## Determinism

The same signed boards give the same bytes on every run and platform. Rows keep
the order of the signed chains, fields keep their declared order, numbers in the
record use the shortest form that reads back exactly, and numbers in the table
use four decimals. The test fixture
`crates/sharpebench-cli/tests/fixtures/leaderboard-arena` is a two-window arena
with its expected `leaderboard.json` and `leaderboard.md`; the CLI test exports
it and compares byte for byte.

## What it does not do

- It does not recompute any score. It reports what the signed links carry; to
  recompute, replay the window's revealed captures.
- It does not publish anything. Hosting the files, and publishing the
  verifying key through a channel a reader trusts, are the operator's choice.
- It does not add host-observed cost. A signed board row has only the
  entrant-reported cost; host-observed usage is attached outside the signed
  row and is not read here.
