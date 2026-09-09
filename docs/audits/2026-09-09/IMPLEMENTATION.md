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
| G02 | Inventory and study Hyper-Tau-Bench end to end | In progress | [Coverage and assessment](HYPER-TAU-REVIEW.md) records complete reads separately from the 8,484-file inventory. |
| G03 | Reconcile original porting recommendations | Open | Trace candidate mechanisms through both products before declaring a gap; no unsupported completeness claims. |
| G04 | Finish PR #39 compatibility pins | Open | Review assertions and measurement claims, test exact head, resolve or merge without losing history. |
| G05 | Repair verified analysis and evidence defects | In progress | Reproduce each defect and add a regression that fails without its fix; record individual rows below. |
| G06 | Resume runtime-exhausted cells explicitly | Implemented locally | Opt-in runtime-only recovery, three extra rounds per cell, durable per-round budgets and fresh attempt append. Completed/agent-fault cells and the contract are unchanged. Tests and six isolated mutations pass; CI/merge pending. |
| G07 | Entrant artifact contamination preflight | Open | Bounded known-content detection with explicit scan scope and incomplete-scan refusal; never claim arbitrary contamination is excluded. |
| G08 | Frozen token rate card | Open | Versioned model/rate identity, validated units and missing-usage handling; cost stays rank-neutral. |
| G09 | Publish attempt summaries | Partially implemented | CLI successes and incomplete-sweep errors expose observed attempts and host time without changing scoring. Monetary usage remains unavailable, pending G08/G11. |
| G10 | statrs special-function migration | Open | Independent numerical comparison, dependency/target review, explicit compatibility treatment and evidence impact ledger before replacement. |
| G11 | Host-observed model gateway accounting | Open | Credential isolation, bounded requests and responses, allowlisted destinations, usage provenance, budgets and hermetic adapter tests; no mandatory third-party arena. |
| G12 | Empirical field execution readiness | Open | Test runner preflight and refusal paths. Paid model calls and new empirical results require explicit setup and spending authorization. |
| G13 | Cross-product API and artifact parity | In progress | Fresh Arena wheel: 91 affected tests pass. Bench wrapper and tarball regression pass after rebuilding stale WASM; final rebuild and CI remain required after further numerical edits. |
| G14 | Update product docs and onboarding | Open | Describe implemented behavior and limitations; retain quantitative-trading positioning and Arena sandbox terminology; no em dashes. |
| G15 | Update paper claims affected by these repairs | Open | Keep historical evidence identified; no invented results, no silent regeneration or personal operational context. |
| G16 | Finish delivery | Open | Granular commits, pushed branches, relevant CI green on exact heads, normal merges to main, post-merge verification; preserve unrelated branches/worktrees. |

## Newly identified defects

The independent reports and their limitations are in [AUDIT.md](AUDIT.md).
Red-to-green evidence is in [VERIFICATION.md](VERIFICATION.md). A local pass is
not a completed delivery: package and CI checks still apply to every row.

| Finding | Repair | Status |
|---|---|---|
| F01 | Check computed field statistics and withhold the whole snooping family on error | Local tests pass |
| F02 | Join resolved identities, contracts and revisions to sealed forecasts in both verifiers | Local tests pass |
| F03 | Derive effective seed width from validated keys; reject contradictory flags | Local tests pass |
| F04 | Retain each CSV column's observed date axis | Local tests pass |
| F05 | Refuse unsupported seed-bootstrap intervals at Rust and Python boundaries | Local tests pass |
| F06 | Preserve baseline score and pass-rate unavailability without numeric ranking | Local tests pass |
| F07 | Refuse unobserved or overflowing candidate utilities | Local tests pass |
| F08 | Support v2 prospective imports with strict digest labels | Local tests pass |
| F09 | Canonicalize internal numeric settlement identity | Local tests pass |
| F10 | Add statistical disqualification reasons and rollup labels | Local tests pass |
| F11 | Preserve statistical error fields and nullable diagnostics in npm | Wrapper and installed-tarball tests pass |
| F12 | Rebuild stale committed WASM and pin methodology through the installed package | Rebuilt after F13; installed-package verification and fresh CI required |
| F13 | Validate computed quantities in Result-returning deflation before flooring or CDF saturation | Four local regression functions pass; valid-input bit compatibility retained |

Arena repairs F02/F05/F06 merged through PR #35 as main `f7614dc`.
The merge tree equals tested head `f5939a9`; post-merge CI passed. Bench PR #40
is still open: ten SPA arithmetic mutants survived its first mutation run.
Three new SPA regression functions, checked against a standalone rational
reference, catch all ten exact mutations in an isolated tree. The control
passes; fresh CI remains required. This is not a nominal coverage proof.

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
