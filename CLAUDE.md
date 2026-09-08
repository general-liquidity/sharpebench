# CLAUDE.md for SharpeBench

This file is an alias for [`AGENTS.md`](./AGENTS.md). Tools that look for
`AGENTS.md` and tools that look for `CLAUDE.md` find the same onboarding doc.
Read `AGENTS.md` for the full brief; the non-negotiables are repeated here so
nothing critical is lost if only this file is loaded.

## Active goal

The 2026-09-07 audit repair checklist at
[`docs/audits/2026-09-07/IMPLEMENTATION.md`](docs/audits/2026-09-07/IMPLEMENTATION.md),
shared with the sibling product and mirrored byte-for-byte there. Work batches
A to I in order; A (paper pass) and B (publication gating) come first.

## Non-negotiables

1. Commit author is `Tiberiu Toca <tibi.toca@gmail.com>`. Never add a
   `Co-Authored-By` trailer for any agent or model.
2. No release, tag, force push or history rewrite under this goal.
3. No model calls, benchmark fields or market-data acquisition. Published
   numerical evidence stays frozen.
4. Stage explicit paths. Never `git add -A`, `git reset --hard`,
   `git checkout -- .`, `git stash` or `git clean`.
5. Mutation-check every regression in an isolated copy, never in the production
   worktree. A source-tree test does not establish installed-package parity.
6. Verify against the committed tree (`git show HEAD:<path>`) and check exit
   codes, not the tail of the output.
7. No em dashes in Markdown or paper prose.
