# Sharpe suite verification and completion

Continuation: [detailed Claude handoff](CLAUDE-HANDOFF.md), including remaining
tasks, recoverable branches and the lost uncommitted Docker prototype.

Status: all rows closed on 2026-09-10 except G11, which is partial. An independent verification the same day found defects in the completion work; see [its disposition](VERIFICATION.md#independent-verification-2026-09-10). This was a new goal, not a reopening of the completed
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
| G01 | Review the recent changes against committed code, tests, artifacts and CI | Closed | An independent read-only review of `5c4cfb2..855afbc` produced five findings, each reproduced against source and each repaired under G05. Its coverage and its limits are recorded in [the repair record](REVIEW-REPAIRS.md). The reviewer could execute nothing, so its red-before claims were re-established by the repairs rather than taken on trust. |
| G02 | Inventory and study Hyper-Tau-Bench end to end | Closed | [Coverage and assessment](HYPER-TAU-REVIEW.md) declares a disposition for all 8,484 files: 1,676 complete reads, 4,914 generated or duplicate, 1,730 binary and 164 unread, each named by subtree and none load-bearing for a port. Bench PR #45, main `863b5e2`. The foreign test suite was not run because some of its tests call providers. |
| G03 | Reconcile original porting recommendations | Closed | [Port reconciliation](PORT-RECONCILIATION.md), 45 rows carrying source path, the Sharpe consumer search and its result, benefit, threat and assumption changes, tests and a decision: 6 take, 13 adapt, 23 reject, 3 defer. Eleven rejections cite the Sharpe line that already holds the mechanism more strictly. Bench PR #45. |
| G04 | Finish PR #39 compatibility pins | Merged | History retained through PR #40; compatibility tests, installed packages and exact-head and post-main CI pass. No statrs migration is implied. |
| G05 | Repair verified analysis and evidence defects | Merged | Bench PR #48, main `072fdb4`. Five findings repaired, each with a regression that fails without its fix and an isolated mutation check. A selection unavailability is advisory rather than a hard gate, matching the scorer; the checkpoint schema is bumped to 4 so a pre-budget checkpoint cannot be granted a fresh round; the alpha warning is computed from alpha; four finiteness guards name their true quantity; and a tautological execution check is deleted. Evidence in [REVIEW-REPAIRS.md](REVIEW-REPAIRS.md). |
| G06 | Resume runtime-exhausted cells explicitly | Merged | Bench PR #41, main `4a453d0`, exact-head and post-main CI pass. Runtime-only recovery has three extra rounds per cell, persisted per-round budgets and fresh attempt append. Completed and agent-fault cells and the contract are unchanged. Six isolated mutations are caught. |
| G07 | Entrant artifact contamination preflight | Merged | Bench PR #44, main `e293093`. The byte engine, the non-extracting TAR reader and the Docker capture with its CLI refusal layer are complete; [the checklist](ARTIFACT-PREFLIGHT.md) records the measured Docker volume semantics and the export-scope limitation. The live Docker leg ran in CI against the pinned Alpine fixture and passed. Five isolated mutations caught, including the lost prototype's board-array indexing bug. No contamination-free claim is made. |
| G08 | Frozen token rate card | Merged | Bench PR #42, main `630183a`, exact-head and post-main CI pass. HTTP, command and container sweeps accept an integer card bound to checkpoint identity. Failed and recovered usage is retained. Estimates remain entrant-reported, not provider billing. |
| G09 | Publish attempt summaries | Merged | CLI successes and incomplete-sweep errors expose attempts and host time without changing scoring. G08 adds separately labelled pricing; G11 owns host-observed provider usage. |
| G10 | statrs special-function migration | Closed on a measured rejection | [The measurement](NUMERICS-MEASUREMENT.md) compares both implementations against mpmath at 60 digits over 1.9 million grid points and the arguments the kernel evaluates: statrs is three to seven orders of magnitude more accurate. It is rejected anyway. The migration was implemented and pushed as PR #49 so CI could answer the question, and its regenerated goldens reproduce only on the platform that generated them while the hand-rolled bodies pass on all three targets. Byte-identical reproduction is a published guarantee, and the hand-rolled error sits orders below every bar the kernel tests against. Bench PR #50, main `3edf6a0`; PR #49 closed unmerged. |
| G11 | Host-observed model gateway accounting | Partial | Bench PR #46, main `0575972`, repaired in Bench PR #55, main `2062444`. What ships is a broker library and an inspection command: `sharpebench gateway` reports routes, bounds, budget and spend and returns. Nothing serves the entrant pipe. The broker's request handling has only test callers, the only transport implementation is the test fake, and no sweep attaches the broker to an entrant or consumes its journal as scored output. So a network-disabled entrant is NOT yet evaluable through this path, and the earlier wording of this row said otherwise. The broker itself is sound after PR #55: the reservation now covers a required per-route input overhead so the money ceiling binds before dispatch, observed usage above a reservation is recorded and latches the sweep, a versioned journal refuses a save from a stale snapshot so two gateways cannot spend one allowance, and an answer returned after the deadline is settled at its reservation. The read timeout and response byte cap remain obligations of the operator-supplied adapter, stated on the transport type. Remaining: the entrant-serving loop and its end-to-end test. |
| G12 | Empirical field execution readiness | Merged | Bench PR #46, repaired in Bench PR #56, main `7fe5a06`. Both field runners gained a preflight that is pure in an environment lookup, with tests for effective configuration, missing credentials, missing budget, unsupported model, dry run and refusal. Independent verification found two defects that preflight left undetected, both inherited and both now repaired: the hosted field's call ceiling counted cached successes rather than dispatches, so a failed call could be retried under the same allowance, and an unknown local dataset selector skipped every dataset and published an empty field as complete. Retry layers inside a provider client are still outside what the ceiling can count, and the docstring says so. No empirical run was attempted, and none is possible here because this environment holds no provider credentials. |
| G13 | Cross-product API and artifact parity | Merged | Bench PR #51, main `fde2024`. The committed WASM still carried a pre-repair label string and was rebuilt with the pinned release recipe; 20 npm tests, 9 MCP tests and the offline packed-tarball check pass on the rebuilt artifact. The earlier Arena wheel and package checks stand for the repairs they covered. Arena's published Bench dependency remains separately pinned at `=0.19.0`. |
| G14 | Update product docs and onboarding | Merged | Bench PR #52, main `1dabf2d`. Two new book chapters for the image preflight and the model gateway, including what a negative scan result is not, plus command line, evidence contract, WASM and npm updates for schema 4, bounded recovery, rate cards, the corrected disqualification taxonomy and the ambiguous-write check. mdBook is not installed on this host, so link targets were checked by script and the CI mdBook leg remains the real gate. |
| G15 | Update paper claims affected by these repairs | Merged | Bench PR #52. The rank predicate was stated as an equivalence over five conjuncts while the scorer also requires both statistics to be estimable; it now carries an availability condition, propagated to the abstract, the introduction and the checklist. Field-wide tests are described as withheld together behind an unscored label, and claims about the canary, the sealed dataset, the Lean scope, tamper detection and reproduction cost are bounded. No frozen number was regenerated, and the PDF rebuilds with no undefined references, citations, warnings or overfull boxes. |
| G16 | Finish delivery | Closed with a correction | Twelve pull requests merged into Bench on 2026-09-10, #44 through #56 except #49, which was closed unmerged. Each had its relevant checks green on the exact pushed head and post-main CI green afterwards. The earlier claim that every merged tree was verified byte-identical to its tested head was false for PR #50: main moved between its CI run and its merge, the comparison was skipped because the change was documentation, and the merge commit `3edf6a0` sits four files and 707 additions away from tested head `cf4bc43`, including the lifecycle change merged immediately before it. The combined tree was then tested by post-main CI, which passed, but the narrower claim was not earned and is withdrawn. Merged branches were deleted in both repositories and the audit records are mirrored into SharpeArena. |
| G17 | Ambiguous-write replay safety | Merged | Bench PR #47, main `35f60f5`, repaired in Bench PR #54, main `fbfd9cf`. A write whose acknowledgment was never observed, retried without a stable client key, is a block-severity violation, and ambiguity is a fact the trace states rather than a clock reading. Independent verification found the first version unsound: it marked an ambiguous write answered on any retry, including one sharing the key, so a keyed retry followed by a blind one produced zero blocks and a process score of 0.9. A shared key makes a retry safe because the venue can collapse the writes into one intent, but it observes nothing about whether that intent arrived. The unit is now the intent: a chain stays open across every keyed retry until an acknowledgment, fill or reconciliation is observed for a member, and resolution is per chain, never global. Evidence in [AMBIGUITY-CHAIN.md](AMBIGUITY-CHAIN.md). |

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
