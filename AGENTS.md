# AGENTS.md for SharpeBench

Onboarding for coding agents opening this repository. `CLAUDE.md` is an alias
carrying the same rules. SharpeBench is a luck-robust benchmark for trading
agents: a pure Rust scoring kernel, a point-in-time simulator, forward
attestation and a chained board, distributed as CLI, crates, WASM/npm, MCP and
Python over one implementation.

## Current goal: 2026-09-07 audit repair

**The active engineering goal for this repository is the audit repair
checklist at [`docs/audits/2026-09-07/IMPLEMENTATION.md`](docs/audits/2026-09-07/IMPLEMENTATION.md).**
It is shared with SharpeArena and mirrored byte-for-byte there; edit both
copies together or neither. The chronological repair diary is
[`VERIFICATION-LOG.md`](docs/audits/2026-09-07/VERIFICATION-LOG.md) beside it.

Work the batches in the order the checklist gives:

| Batch | Scope |
|---|---|
| A | Paper pass. Historical-impact caveat, R11/R13/R14 text, shard-order claim, submitting.md scope. |
| B | Publication gating. `release.yml` depends on a green `ci.yml` for the tagged commit; narrow the updater claim; package consumers; conformance fixtures. |
| C | Run identity (BI3): keyed run/window/seed identity through CSV import and legacy `Run` arrays. |
| D | Producer rows that touch existing claims: BP6, BP8, AP5, AP3. |
| E | Shared mathematics and contracts: R07/BM10, R06/AI1, R03 and R09 propagation, remaining R02, R05, R12, BR2/AR2. |
| F | Remaining Bench diagnostics: BM1, BM2/BS6, BM3, BM7, BI6 and the split supplementary rows. |
| G | Remaining Arena telemetry (Arena repository). |
| H | Producer rows for the next field run. Not required for the current papers. |
| I | Bounded probes. One attempt each, then promote to a defect row or delete. |

Batches A and B change what the shipped product claims and come first. Five
items are explicitly deferred and listed at the end of the checklist; do not
start them without reopening the decision.

## Goal rules

These come from the goal, not from general practice, and they override
convenience:

1. **No release, tag, force push or history rewrite** under this goal.
2. **No new experiments**: no model downloads, model calls, benchmark fields or
   market-data acquisition. Synthetic regressions, existing fixtures, package
   checks and finite diagnostics are in scope.
3. **Published numerical evidence stays frozen.** Where a repair changes what a
   producer would compute, say so in the paper instead of regenerating the
   number.
4. **A suspected issue is not a confirmed bug.** Every row closes with an
   implementation or test reference, or with an evidence-backed disposition.
5. **Mutation-check every regression.** Revert the fix in an isolated temporary
   copy, confirm the new test fails, restore. Never mutate the production
   worktree.
6. **A source-tree test does not establish installed-package parity.** WASM and
   Python surfaces need the rebuilt artifact exercised.
7. **Verify against the committed tree**, not the working tree: `git show
   HEAD:<path>`. Check exit codes, not the tail of the output.
8. Repair branches merge into `main` by normal merge after all relevant checks
   pass on the exact pushed head; verify the resulting main tree.

## Repository rules

- **Commit author must be `Tiberiu Toca <tibi.toca@gmail.com>`.** Never add a
  `Co-Authored-By` trailer for any agent or model.
- Conventional-commit prefixes. Small commits over explicit paths; never
  `git add -A`. Preserve unrelated edits.
- No repo-wide destructive git: no `git checkout -- .`, `git reset --hard`,
  `git stash` or `git clean`.
- No em dashes in Markdown or paper prose.
- `sharpebench-core` stays pure: no I/O, no system clock, no ambient
  randomness. `#![forbid(unsafe_code)]` at every workspace package root; the
  published PyO3 binding is the disclosed exception.
- Bind provenance on a clean candidate before pushing: `python
  paper/src/make-provenance.py`, then `python paper/src/check-provenance.py`.

## Commands

```bash
cargo fmt --all --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --workspace --exclude xtask   # xtask needs OpenSSL dev files
python -m unittest paper/src/test_provenance.py
python -m unittest paper/src/test_sweep_grid.py
python paper/src/check-provenance.py
```

The full workspace, `cargo deny`, live Docker, mdBook, the three OS matrices
and the packaged consumers run in CI; a local pass is necessary, not
sufficient. Pull requests also run `mutation.yml`: cargo-mutants over the PR
diff in the four pure crates, and the paired-boundary gate
(`python scripts/check-paired-boundaries.py`). See "Standing CI legs" in
`CONTRIBUTING.md`.

## Where things live

| Area | Path |
|---|---|
| Scoring kernel, ranking, forecasts | `crates/sharpebench-core/` |
| Statistics (PSR, DSR, bootstrap, FDR) | `crates/sharpebench-stats/` |
| Simulator, transport, external agents | `crates/sharpebench-sim/` |
| CLI, updater, board rendering | `crates/sharpebench-cli/` |
| Memory ablation benchmark | `crates/sharpebench-memory/` |
| WASM and npm surface | `crates/sharpebench-wasm/`, `npm/` |
| Paper, producers, frozen evidence | `paper/` |
| Forward arena records | `arena/` |
| Product roadmap | `docs/PLAN.md` |
| Audit goal | `docs/audits/2026-09-07/` |
| Release operations | `RELEASING.md`, `scripts/release.py` |
