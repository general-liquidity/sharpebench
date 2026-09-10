# Sharpe suite verification and completion

Continuation: [detailed Claude handoff](CLAUDE-HANDOFF.md), including remaining
tasks, recoverable branches and the lost uncommitted Docker prototype.

Status: active. This is a new goal, not a reopening of the completed
[2026-09-07 audit](../2026-09-07/IMPLEMENTATION.md).

## Scope and acceptance

Review the changes following the previous audit checkpoint, reassess the
Hyper-Tau-Bench porting proposals against its implementation and both products,
finish recoverable pending work, and implement verified remaining defects.
A green workflow establishes the checks it actually runs, not universal
correctness, contamination freedom, or an empirical agent result.

Baseline: SharpeBench main `5c4cfb2` (v0.19.0), SharpeArena main `4fdf672`.
Both baseline main CI runs were verified green. Bench PR #39 (`bedbfb8`) was
open and green at intake; its history is now merged through PR #40. Its changes
are numerical compatibility pins and documentation, not a
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
| G04 | Finish PR #39 compatibility pins | Merged | History retained through PR #40; compatibility tests, installed packages and exact-head/post-main CI pass. No statrs migration is implied. |
| G05 | Repair verified analysis and evidence defects | In progress | Reproduce each defect and add a regression that fails without its fix; record individual rows below. |
| G06 | Resume runtime-exhausted cells explicitly | Merged | Bench PR #41, main `4a453d0`, exact-head and post-main CI pass. Runtime-only recovery has three extra rounds per cell, persisted per-round budgets and fresh attempt append. Completed/agent-fault cells and the contract are unchanged. Six isolated mutations are caught. |
| G07 | Entrant artifact contamination preflight | In progress | Byte engine and non-extracting TAR reader implemented on the feature branch. Each has ten tests and thirteen caught mutations. Docker capture, complete image/configuration scope and pre-launch refusal remain required; see [the integration checklist](ARTIFACT-PREFLIGHT.md). No contamination-free claim. |
| G08 | Frozen token rate card | Merged | Bench PR #42, main `630183a`, exact-head and post-main CI pass. HTTP, command and container sweeps accept an integer card bound to checkpoint identity. Failed/recovered usage is retained. Estimates remain entrant-reported, not provider billing. |
| G09 | Publish attempt summaries | Merged | CLI successes and incomplete-sweep errors expose attempts and host time without changing scoring. G08 adds separately labelled pricing; G11 remains responsible for host-observed provider usage. |
| G10 | statrs special-function migration | Open | Independent numerical comparison, dependency/target review, explicit compatibility treatment and evidence impact ledger before replacement. |
| G11 | Host-observed model gateway accounting | Open | Credential isolation, bounded requests and responses, allowlisted destinations, usage provenance, budgets and hermetic adapter tests; no mandatory third-party arena. |
| G12 | Empirical field execution readiness | Open | Test runner preflight and refusal paths. Paid model calls and new empirical results require explicit setup and spending authorization. |
| G13 | Cross-product API and artifact parity | Verified for current repairs | Fresh Arena wheel: 91 affected tests pass. Final rebuilt Bench WASM, 20 npm tests, offline tarball and MCP checks pass, as do exact-head package CI jobs. Arena's published Bench dependency remains separately pinned. |
| G14 | Update product docs and onboarding | Open | Describe implemented behavior and limitations; retain quantitative-trading positioning and Arena sandbox terminology; no em dashes. |
| G15 | Update paper claims affected by these repairs | Open | Keep historical evidence identified; no invented results, no silent regeneration or personal operational context. |
| G16 | Finish delivery | Open | Granular commits, pushed branches, relevant CI green on exact heads, normal merges to main, post-merge verification; preserve unrelated branches/worktrees. |

## Newly identified defects

The independent reports and their limitations are in [AUDIT.md](AUDIT.md).
Red-to-green evidence is in [VERIFICATION.md](VERIFICATION.md). A local pass is
not a completed delivery: package and CI checks still apply to every row.

| Finding | Repair | Status |
|---|---|---|
| F01 | Check computed field statistics and withhold the whole snooping family on error | Merged; exact-head and post-main CI pass |
| F02 | Join resolved identities, contracts and revisions to sealed forecasts in both verifiers | Merged; exact-head and post-main CI pass |
| F03 | Derive effective seed width from validated keys; reject contradictory flags | Merged; exact-head and post-main CI pass |
| F04 | Retain each CSV column's observed date axis | Merged; exact-head and post-main CI pass |
| F05 | Refuse unsupported seed-bootstrap intervals at Rust and Python boundaries | Merged; exact-head and post-main CI pass |
| F06 | Preserve baseline score and pass-rate unavailability without numeric ranking | Merged; exact-head and post-main CI pass |
| F07 | Refuse unobserved or overflowing candidate utilities | Merged; exact-head and post-main CI pass |
| F08 | Support v2 prospective imports with strict digest labels | Merged; exact-head and post-main CI pass |
| F09 | Canonicalize internal numeric settlement identity | Merged; exact-head and post-main CI pass |
| F10 | Add statistical disqualification reasons and rollup labels | Merged; exact-head and post-main CI pass |
| F11 | Preserve statistical error fields and nullable diagnostics in npm | Merged; wrapper, installed tarball and CI pass |
| F12 | Rebuild stale committed WASM and pin methodology through the installed package | Merged; final rebuild, installed package and CI pass |
| F13 | Validate computed quantities in Result-returning deflation before flooring or CDF saturation | Merged; checked arithmetic and valid-input bit compatibility verified |

Arena repairs F02/F05/F06 merged through PR #35 as main `f7614dc`.
The merge tree equals tested head `f5939a9`; post-merge CI passed. Bench PR #40
merged as `4b3cc0d`, with a tree identical to tested head `ecdcea2` and green
post-main CI. Its final mutation run tested 128 mutants: 125 caught, three
unviable, none missed or timed out. Earlier surviving SPA and checked-PSR
mutants led to stronger rational-reference and asymmetric-moment fixtures.
This is not a nominal coverage proof. Arena's documentation mirror is merged
through PR #37 as `1ec75cb`, with equal tested/merged trees and green main CI.

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
