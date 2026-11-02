# SharpeBench leaderboard export

Schema `sharpebench.leaderboard-export.v1`. Every row was read from a signed board that verified before export.

- Source: arena, ed25519 signatures, key pinned `324d7175f1c14e922259bde9312ea9574b3ef78a96f23d784f515b195a48e51a`
- Completeness: anchored. signed head covers every published window; a rollback to an earlier head together with its windows is undetectable from the documents alone, so pin a head seen out of band with --expect-head
- Head: `2:bbb31f35be1fcdbe8d91a0b282af2cd3faeca5ce9bfd9a06339e7c041effafee`

## Boards

| board | tier | record | final signature sha256 | certifying | supplied returns | rows | refusals |
|---|---|---|---|---|---|---|---|
| w1 | forward-certifying | `windows/w1/board.json` | `268d9e0e62499b59633a321c1b203a8c4bec9672d2e0c4a1bba4c8eb2a0a9f05` | yes | no | 2 | 1 |
| w2 | historical-advisory | `windows/w2/board.json` | `bbb31f35be1fcdbe8d91a0b282af2cd3faeca5ce9bfd9a06339e7c041effafee` | no | yes | 2 | 0 |

## Rows

| board | record | agent | verdict | tier | tie band | DSR | DSR interval | pass^k | eligible | evidence | cost | qualifiers | reasons |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| w1 | `windows/w1/board.json#1` | beta | cannot_tell | forward-certifying | - | 0.9659 | [0.5590, 0.9993] | failed 1/2 | no | certifying (forward, re-executed) | unreported | - | failed_pass_k |
| w1 | `windows/w1/board.json#2` | alpha | no_evidence_of_skill | forward-certifying | - | 0.6486 | [0.0939, 0.9733] | failed 1/2 | no | certifying (forward, re-executed) | unreported | - | failed_pass_k, dsr_below_bar, bootstrap_insignificant |
| w1 | `windows/w1/board.json#0` | gamma | refused | historical-advisory | - | - | - | - | - | unknown | - | - | cause: reveal does not match commitment |
| w2 | `windows/w2/board.json#1` | delta | skill_supported | historical-advisory | 1 | 1.0000 | [1.0000, 1.0000] | passed 3/3 | yes | noncertifying (forward, supplied) | 0.7500 entrant_reported | - | - |
| w2 | `windows/w2/board.json#2` | flat | degenerate_track | historical-advisory | - | unavailable | unavailable | failed 0/3 | no | noncertifying (forward, supplied) | unreported | - | failed_pass_k, deflation_refused, cause: returns must not be constant: a constant series has no Sharpe ratio |
