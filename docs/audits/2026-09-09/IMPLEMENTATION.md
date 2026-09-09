# Sharpe suite verification and completion

Status: active. This is a new goal, not a reopening of the completed
[2026-09-07 audit](../2026-09-07/IMPLEMENTATION.md).

## Scope and acceptance

Review the changes following the previous audit checkpoint, reassess the
Hyper-Tau-Bench porting proposals against its implementation and both products,
finish recoverable pending work, and implement verified remaining defects.
A green workflow establishes the checks it actually runs, not universal
correctness, contamination freedom, or an empirical agent result.

Baseline: SharpeBench main `5c4cfb2` (v0.19.0), SharpeArena main `4fdf672`.
Both main CI runs were verified green. Bench PR #39 (`bedbfb8`) is open and
green. Its changes are numerical compatibility pins and documentation, not a
statrs migration. No new implementation from the later six-agent batch is
present in the registered worktrees inspected so far.

Each row needs source evidence, acceptance tests, and a committed disposition.
Do not mark a proposal implemented because an agent was dispatched. Mirror this
file across both repositories. Findings and verification records belong beside
it; do not overwrite the historical audit diary.

## Work ledger

| ID | Work | Status | Acceptance / limits |
|---|---|---|---|
| G01 | Review Claude's changes against committed code, tests, artifacts and CI | In progress | Independent analysis findings plus implementation/security review; distinguish current source from stale local binaries. |
| G02 | Inventory and study Hyper-Tau-Bench end to end | In progress | Coverage ledger for source, tests, data, runtime, UI and docs; evidence for every port or rejection. |
| G03 | Reconcile original porting recommendations | Open | Trace candidate mechanisms through both products before declaring a gap; no unsupported completeness claims. |
| G04 | Finish PR #39 compatibility pins | Open | Review assertions and measurement claims, test exact head, resolve or merge without losing history. |
| G05 | Repair verified analysis and evidence defects | In progress | Reproduce each defect and add a regression that fails without its fix; record individual rows below. |
| G06 | Resume runtime-exhausted cells explicitly | Open | Opt-in, infrastructure-only retry; preserve cumulative attempt/cost history and contract identity; no result shopping. |
| G07 | Entrant artifact contamination preflight | Open | Bounded known-content detection with explicit scan scope and incomplete-scan refusal; never claim arbitrary contamination is excluded. |
| G08 | Frozen token rate card | Open | Versioned model/rate identity, validated units and missing-usage handling; cost stays rank-neutral. |
| G09 | Publish attempt summaries | Open | Report terminal and nonterminal attempts, failures and spent cost without changing ranking. |
| G10 | statrs special-function migration | Open | Independent numerical comparison, dependency/target review, explicit compatibility treatment and evidence impact ledger before replacement. |
| G11 | Host-observed model gateway accounting | Open | Credential isolation, bounded requests and responses, allowlisted destinations, usage provenance, budgets and hermetic adapter tests; no mandatory third-party arena. |
| G12 | Empirical field execution readiness | Open | Test runner preflight and refusal paths. Paid model calls and new empirical results require explicit setup and spending authorization. |
| G13 | Cross-product API and artifact parity | Open | Test rebuilt installed packages; pin migration only through published dependencies; identify release requirements honestly. |
| G14 | Update product docs and onboarding | Open | Describe implemented behavior and limitations; retain quantitative-trading positioning and Arena sandbox terminology; no em dashes. |
| G15 | Update paper claims affected by these repairs | Open | Keep historical evidence identified; no invented results, no silent regeneration or personal operational context. |
| G16 | Finish delivery | Open | Granular commits, pushed branches, relevant CI green on exact heads, normal merges to main, post-merge verification; preserve unrelated branches/worktrees. |

## Newly identified defects

Auditor findings will be recorded with file/line evidence and reproductions
before they are promoted to implementation rows. An interim report is not a
completed audit.

## Execution policy

- Work in separate repository worktrees and stage explicit paths.
- Preserve existing user edits and the completed audit history.
- Keep the deterministic kernel free of I/O and ambient randomness.
- Use synthetic regressions and existing evidence for verification.
- Record numerical changes before regenerating artifacts; preserve the
  distinction between compatibility and numerical correctness.
- Do not download models, spend API credits, acquire new market data, or claim
  experiments occurred as part of a code test.
- No force pushes, destructive cleanup, or credential disclosure.
- A proposed improvement may close on a justified rejection; an accepted
  implementation task cannot close merely on documentation.

