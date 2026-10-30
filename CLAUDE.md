# CLAUDE.md for SharpeBench

This file is an alias for [`AGENTS.md`](./AGENTS.md). Tools that look for
`AGENTS.md` and tools that look for `CLAUDE.md` find the same onboarding doc.
Read `AGENTS.md` for the full brief; the non-negotiables are repeated here so
nothing critical is lost if only this file is loaded.

## Active goal

Close every row of the gap programme. The row list lives in the maintainer
workspace (`research/product-context/acceptance-map.md` and
`implementation-gaps.md`), not in this repository, and the same goal applies to
the sibling product. A row closes with an implementation and test reference, or
with a recorded, evidence-backed disposition. No release happens until the owner
approves it at the end of the programme.

The audit ledgers under `docs/audits/` are finished records. Rows G01 to G34 of
the 2026-09-09 ledger are closed, and neither that ledger nor the 2026-09-07
audit is an active schedule.

## Non-negotiables

1. Commit author is `Tiberiu Toca <tibi.toca@gmail.com>`. Never add a
   `Co-Authored-By` trailer for any agent or model.
2. No release or tag until the owner approves it at the end of the programme.
   No force push or history rewrite.
3. No model downloads, model calls, benchmark fields or market-data acquisition,
   unless the run is a declared, protocol-frozen experiment the owner has
   approved. Published numerical evidence stays frozen.
4. Stage explicit paths. Never `git add -A`, `git reset --hard`,
   `git checkout -- .`, `git stash` or `git clean`.
5. Mutation-check every regression in an isolated copy, never in the production
   worktree. A source-tree test does not establish installed-package parity.
6. Verify against the committed tree (`git show HEAD:<path>`) and check exit
   codes, not the tail of the output.
7. No em dashes in Markdown or paper prose.
