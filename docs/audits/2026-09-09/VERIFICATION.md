# Verification record

## Port build and literature corrections, 2026-09-10

Six pull requests merged into SharpeBench and one into SharpeArena. Each had
every check green on its exact pushed head, main had not moved since that head
was tested, and the merged tree was compared against the tested tree and found
identical. A branch that was green on an older main was brought up to date and
tested again before it merged, rather than merged on its earlier result.

| PR | Row | Main after merge |
|---|---|---|
| Bench #58 | G18, fault injection | `c256adc` |
| Bench #60 | G19, literature corrections | `afd0be4` |
| Bench #61 | G11, G18, serving loop and image hygiene | `aba678e` |
| Bench #59 | G18, contract ports | `23efd4c` |
| Bench #62 | G19, LITE verdict units | `5eb826c` |
| Bench #63 | G19, remaining unit defaults | `277f733` |
| Arena #42 | G19, deflation input refusal | `21f9a6d` |

What the gates caught that the authoring work missed.

The mutation gate found nine surviving mutants in the contract ports: the
visibility audit could return an empty report, the seal report's emptiness
could ignore two of its three lists, an array that grew an object inside a
visible field was never tested, and the operation preimage could be replaced by
any constant because its only pin lived in another crate, which the protocol
crate's own mutation run does not execute. Tests now kill all nine, confirmed by
a targeted local run in which all thirteen mutants of those functions were
caught.

The paired-boundary gate rejected the shared annualized-to-per-period
conversion, which documents a finite, positive frequency and had no boundary
test. A test now pins it at one period a year, an infinite frequency (a zero
dispersion, the most favourable bar, which is why callers must refuse it), zero
and a negative value.

The macOS build of the Arena correction failed a pin that had been recorded on
Windows: the old estimator's deflated Sharpe differed by two units in the last
place between the two platforms' math libraries. The recorded constants are now
compared within sixteen units, while the comparison against the pinned
SharpeBench kernel, which runs on the same platform as the code under test,
stays exact.

The fault injector and the contract ports both rewrote the same retry driver.
The textual merge interleaved the two loops and was discarded; the driver was
rebuilt from the backoff version with the fault record added, so each attempt
carries both its injected faults and its scheduled backoff, and the full suite
passed on the combined tree before it was pushed.

One live-container run failed on the cgroup out-of-memory probe on a branch
that does not touch the sandbox, as it did once in the completion round. The
next run, on the same branch after main was merged in, passed. It is recorded
again as an observed flake in a timing-sensitive live probe, not as an
explained one.

Not established by this round: the gateway sandbox launch and the image
allowlist probe have not run against a live Docker daemon, and the allowlist
will likely need the files Docker adds to a container once it does; the fault
plan, backoff schedule and re-execution check are library calls with no CLI
flag yet; and the paper evidence was not regenerated, so the corrected prior
and the proposed serial-correlation term have no new measured result.

## Independent verification, 2026-09-10

A separate verification of the completion round confirmed the repository state and
the green CI, and refuted the claim that every implementation and safety property
was finished. Each finding below was re-established against source before it was
accepted, and the lifecycle finding was reproduced against the current library.

| Finding | Disposition |
|---|---|
| A keyed retry marked an unresolved write answered, so a later blind retry did not block | Repaired, PR #54, main `fbfd9cf` |
| The reservation bounded message content, not billed input, and settlement accepted the overshoot | Repaired, PR #55, main `2062444` |
| Two gateways on one journal each spent the same allowance | Repaired, PR #55 |
| The broker enforced no deadline and its bounds read as enforced | Late answers now refused; read timeout and byte cap stated as adapter obligations, PR #55 |
| The gateway reports configuration and nothing serves the entrant pipe | Accepted. G11 is marked partial, not repaired |
| The hosted field's call ceiling counted cached successes, not dispatches | Repaired, PR #56, main `7fe5a06` |
| An unknown local dataset selector published an empty field as complete | Repaired, PR #56, main `7fe5a06` |
| Zero bootstrap draws returned a zero-width interval rather than unavailability | Repaired, PR #56, main `7fe5a06` |

The verification also withdrew two claims made in this record. The tree-equality
claim was false for PR #50, as recorded above and in G16. And the special-function
measurement's grid counts, maximum errors and zero-verdict-change figures are
narrative: the measurement scripts, raw grids and comparison logs were not
committed, so those numbers are not independently reproducible from this tree. The
rejection of the migration does not depend on them, since it rests on the
three-platform CI result, which is reproducible.

Two process facts from the repair work bear on how far local results can be
trusted. The mutation gate caught a third untested boundary in the ambiguity repair
itself: resolving one subject's intent would have closed every open chain. It
caught a fourth in the interval repair, where no test fixed the resampled bounds
for a known seed, and then a flaw in the first attempt to close it: with seed 42
the XOR and OR mutants of the stream constant produce the identical stream,
because 42 shares no set bits with the constant's low byte, so the pin could not
tell them apart. The committed pin uses a seed that overlaps the constant. And a
local run reported two false evidence-coverage failures because two worktrees were
compiling into one shared Cargo target directory at once, a setting introduced that
day to save disk. With an isolated target the same tree passed all 662 affected
tests. CI builds fresh and stays authoritative; a local run taken while another
build shares its cache is not.

## Completion round, 2026-09-10

Eight pull requests merged into SharpeBench, each with its relevant checks green
on the exact pushed head and post-main CI green afterwards. The merged tree was
compared against the tested tree for seven of them. The comparison was skipped
for PR #50, whose merge sits 707 additions away from its tested head; see the
correction below.

| PR | Row | Main after merge |
|---|---|---|
| #45 | G02, G03 | `863b5e2` |
| #46 | G11, G12 | `0575972` |
| #44 | G07 | `e293093` |
| #48 | G05 | `072fdb4` |
| #47 | G17 | `35f60f5` |
| #50 | G10 | `3edf6a0` |
| #51 | G13 | `fde2024` |
| #52 | G14, G15 | `1dabf2d` |

Verification notes that bear on how much these results establish.

The live Docker preflight could not be exercised locally: the daemon on this
host answered long enough to measure `Config.Volumes` in its three forms and
then wedged, so the leg was wired into the live-container CI job by exact test
name and ran there against the pinned Alpine fixture. Never run all ignored CLI
tests wholesale, because a subprocess fixture requires an environment mode.

The mutation gate caught two surviving mutants that the authoring work missed,
one per pull request, and both were killed by tests rather than by weakening the
gate. In the selection refusal path no test used an alpha exactly equal to the
recommended floor, so widening the comparison survived; the floor is a minimum,
so an alpha sitting on it must not warn, now asserted on both the refusal and
the accepted path. In the lifecycle check no trace recorded an unobserved
acknowledgment against an order whose acknowledgment had already been observed,
so dropping the stage guard survived; an acknowledged order has an outcome and
cannot become ambiguous. Each was verified by mutating in place, observing the
failure, restoring from the committed tree and comparing byte for byte.

The statrs question was settled by CI rather than by argument. The migration was
implemented, pushed and tested precisely so the reproducibility claim could be
falsified: its regenerated goldens pass on Windows, where they were generated,
and fail on Linux and macOS, while the same three jobs on main with the
hand-rolled bodies pass everywhere. The accuracy measurement that motivated the
migration stands and is preserved; only its conclusion changed.

One live-container failure on the parity pull request was a flake in the cgroup
out-of-memory probe, not a regression: main had passed the same job shortly
before, and a rerun of the failed job alone passed. A rerun is not a diagnosis,
so this is recorded as an observed flake in a timing-sensitive live probe rather
than as an explained one.

Not established by this round: no empirical field was run, because this
environment holds no provider credentials; mdBook is not installed here, so the
book was checked by link script and the CI leg remains the real gate; and the
frozen paper evidence was neither regenerated nor reproduced, since the current
tree already diverges from that snapshot for reasons that predate this work and
sit in the simulator rather than in the statistics.

## Initial review and isolation

- Baselines: Bench `5c4cfb2` (v0.19.0), Arena `4fdf672`.
  Both exact main heads had successful CI. Bench PR #39 at `bedbfb8`
  had 23 successful checks and remained unmerged.
- The two independent reviewers' ten distinct findings are in [AUDIT.md](AUDIT.md).
  Their coverage limits remain part of that report.
- Work is on separate `fix/verified-followups-2026-09-09` branches.
  Existing main checkouts, unrelated branches and historical evidence are preserved.
- The old Linux Bench binary and Windows Arena extension were stale.
  Tests below use fresh Linux builds. The current Python extension was rebuilt
  with maturin in an isolated virtual environment, then separately built as a
  wheel and installed into a fresh consumer environment. The 91 affected Python
  tests passed there, with imports verified to come from site-packages.
- PR #39's history was merged into the Bench repair branch without cherry-picking.
  Its four special-function test functions pass; no statrs dependency or
  replacement has been added. Its universal claim about other libraries'
  inability to compute empirical moments was narrowed.

## Red-to-green regressions

| Finding | Failure before repair | Local verification after repair |
|---|---|---|
| F01, computed statistical overflow | Both new stats overflow tests failed; the full verdict emitted `[true,true]` instead of withholding rejection. | All stats and edge tests pass. A failure in one family member now withholds the entire snooping family. |
| F02, resolved forecast rewrite | All four Arena cases accepted changed prediction, identity, rationale or exposure despite unchanged sealed files. The Bench importer also accepted three rehashed resolved-field rewrites. | Arena prospective suite: 20 pass. Importer suite: 6 pass including subcases. The committed historical field still verifies without changing its files. |
| F03, replicate width | Keyed score reported 160 pooled observations instead of 80 effective observations. | Automatic width equals explicit width 2; widths 1 and 3 refuse. |
| F04, missing date axis | Retained periods were `[]` rather than `[d1,d3]`. | Each column retains its observed dates; equal-length mismatched dates refuse, aligned dates accept. |
| F05, unsupported confidence | Eight support/resampling tests and nine parameter/baseline tests failed before repair. | Rust and rebuilt Python refuse unsupported requests. A single-seed baseline preserves its point but withholds the interval; duplicate IDs refuse. |
| F06, unavailable baseline rank | Six error-key/confidence-mode cases published a float instead of the reason. | All three typed errors survive with confidence on or off; unavailable rows have no numeric rank and do not enter paired comparisons. |
| F07, invalid candidate selection | The empty candidate won against negative observed returns with no error. | Empty, single-observation, nonfinite and overflowing candidates withhold the whole selection; valid-input and append-stability tests pass. |
| F08, v2 import | Both supported v2 labels were rejected; v1-with-label was accepted. | Both supported labels accept under v2; missing, unknown and v1 labels refuse. |
| F09, numeric settlement identity | Comparing `1` with `1.0` failed as unequal settlement. | Integer/float and signed-zero pairs compare; opposite realized outcomes still refuse. |
| F10, unexplained rejection | A real invalid-CI configuration rejected a strong field without the expected statistical reason. | Deflation, bootstrap and selection errors have stable serialized labels and appear in rollups. |
| F11/F12, installed npm behavior | The new regression first failed on the old WASM's missing statistical disqualification, then on the rebuilt WASM's error discarded by the old wrapper. | Repaired wrapper and rebuilt WASM pass all 20 npm tests and the offline installed-tarball probe. Final rebuilding remains required after further numerical edits. |
| G09, discarded attempt summary | The real-CLI loopback regression failed on an isolated copy of the pre-change tree because incomplete-sweep JSON omitted accounting. | The repaired CLI reports 48 failed attempts for 16 exhausted cells, monetary cost unavailable, and no board. Two unit tests also pin unknown duration and unchanged score/order/reference rows. |
| F13, hidden deflation overflow | Three new regression functions failed against an isolated pre-fix tree: finite returns or parameters produced Ok(NaN), Ok(infinity), or a saturated numeric fallback. | Four regression functions pass, including finite valid-input bit comparisons against the previous scalar PSR and existing short/constant-series behavior. |

These regressions use synthetic inputs and existing artifacts. No model was
downloaded or called, no API credits were spent, and no market data was acquired.

## Local commands and results

Bench:

- `cargo test -p sharpebench --locked`: 63 tests passed for the initial
  seed/date repairs. A subsequent full workspace run (excluding xtask) passed,
  with 14 ignored tests recorded rather than counted as executed.
- `cargo test -p sharpebench-stats -p sharpebench-edge --locked`:
  102 stats unit tests, 10 statistical-boundary tests, 4 compatibility test
  functions, 31 edge unit tests and both doc tests passed.
- `cargo test -p sharpebench-core --locked --quiet`: 330 unit tests,
  37 integration tests and one doc test passed.
- Affected-package clippy with warnings denied passed.
- npm build, 20 tests, offline tarball installation, and the MCP build plus
  nine tests passed against the rebuilt sibling package. All were repeated
  successfully after F13 and the final WASM rebuild with wasm-bindgen 0.2.126,
  matching Cargo.lock.
- `python3 -m unittest paper/src/test_import_prospective_field.py`: 6 tests passed.
- `python3 scripts/check-paired-boundaries.py`: passes with its existing
  24-entry allowlist. A green result is not complete boundary coverage.

Arena:

- `cargo test -p sharpearena --locked --quiet`: 161 unit tests and 25
  integration tests passed.
- Workspace clippy with warnings denied passed.
- Rebuilt the native Python extension after the Rust confidence API change.
- Baseline, confidence and prospective-field Python suites: 91 tests passed.
- The existing 3-agent, 24-contract historical field still verifies.
  Bench's importer also verifies its 15-file committed source inventory at
  Arena `4fdf672`.

## Delivery and remaining limits

### Explicit runtime recovery

The bound checkpoint driver now saves each observed attempt before a later
attempt can start. Four integration tests cover recovery eligibility, retained
history, lifetime and per-round retry budgets, interrupted claims, changed
contracts and malformed runtime states. A unit test distinguishes identical
fresh executions from a replayed ledger batch. The real CLI loopback test
reports 48 attempts initially, 48 on default resume, and 96 after explicit
recovery; all cells remain runtime-failed, with no board. Invalid recovery
flag combinations refuse before launch.

All four integration tests and the eight existing ledger tests pass, as do
45 harness unit tests and affected-package clippy. Six isolated mutations
fail: disabled recovery, bypassed lifetime ceiling, deduplicated fresh
executions, reset per-round budget, omitted attempt persistence and erased
prior history. The restored integration target passes. A process killed in
an unobserved attempt can still leave incomplete accounting; no monetary
measurement or immutable-checkpoint claim is made.

### Delivery history

The repairs are granular commits. Arena PR #35 merged as `f7614dc`; its tree
matches tested head `f5939a9` and post-merge CI passed. Its first CI run caught
formatting in the separately excluded PyO3 crate, which was fixed and rebound.

Bench PR #40 is now merged. At its earlier pushed head `5618a46`, the ordinary workflow,
package checks, live Docker probe and three-platform matrix passed. Mutation
testing reported 73 mutants: 60 caught, three unviable, ten missed. The missed
SPA arithmetic mutations require stronger tests; the check is not bypassed or
explained away as a runner flake. Subsequent local changes need a new pushed
head and fresh CI, which are recorded below. CodeRabbit skipped review while the PRs were drafts.

The three new tests in stats/tests/spa_studentization.rs are independently
checked by spa_reference.py. That reference uses rational means, variances and
squared positive-statistic comparisons; only consistent-SPA exclusion uses
floating log/sqrt. All three fixtures have no ties at the observed statistic.
The reference is a numerical cross-check, not evidence of nominal test coverage.
Replaying the ten exact CI-missed arithmetic mutations in an isolated source
copy now produces ten failing test runs. Restoring the source passes all three
tests. This local replay does not replace the next complete CI mutation run.

At head 1f286db the complete mutation run caught the prior SPA gaps, but ten
mutations in the newly added checked-PSR helper survived (115 caught, three
unviable, ten missed). The valid-input compatibility fixtures were centered or
constant, so several moment terms vanished. Adding a two-observation case and
an asymmetric nonzero-mean case catches all ten exact mutations locally; the
unmodified four-test target passes. Only tests changed in this follow-up, not
the numerical implementation or WASM artifact. The subsequent complete CI run passed.

Final package rebuilding and installed consumers passed. Bench PR #40 merged
as `4b3cc0d`, tree-identical to tested head `ecdcea2`. Its complete mutation
run reported 125 caught, three unviable, zero missed and zero timed out.
Post-main CI and npm runs 34403159826 and 34403159785 succeeded. PR #39 was
closed as merged by the retained ancestry, not by discarding its changes.

Bench PR #41 merged as `4a453d0`, tree-identical to tested head `eb8d1a4`.
All 23 checks passed; post-main CI 34404055056 and npm 34404054949 succeeded.
Arena documentation PR #37 merged as `1ec75cb`, tree-identical to `110188a`;
post-main CI 34403517298 succeeded. None of this is a release.

### Frozen token-rate accounting

Seven harness regressions cover the strict card schema, exact integer quotes,
identity changes, overflow, missing and mixed usage, observer transparency,
failed-attempt retention and checkpoint recovery. Two new CLI tests use a
loopback fixture, not a model provider. A 240-decision run produces exactly
690000 USD nanodollars under the synthetic card; resume makes no further
calls and reproduces its accounting. Changing the card refuses before another
request and leaves the checkpoint unchanged. After removing accounting, the
priced and unpriced boards are identical. Missing usage emits no total.

Seven isolated mutations are caught: substituting the input rate for output,
accepting mixed cards, treating absent attempt usage as free, treating failed
usage as complete, accepting invalid reasoning counts, dropping failed usage,
and removing the card from the CLI checkpoint identity. Restored controls pass.
The 45 harness unit tests, eight existing ledger tests, four recovery tests,
48 CLI unit tests and both existing CLI recovery tests also pass. Affected
clippy passes with warnings denied. CI and merging of this new card work
remain pending.

The first PR #42 macOS run failed in the loopback fixture, not a score
assertion: the accepted socket returned `WouldBlock` during header parsing.
The fixture now explicitly resets accepted sockets to blocking mode while
retaining its read timeout. A third CLI test forces the initial nonblocking
state; removing the reset in an isolated copy reproduces `WouldBlock`, and
restoring it passes. All three pricing CLI tests and targeted clippy pass
locally. The corrected head `d957fe5` passed all PR checks, including macOS
and Windows. PR #42 merged as `630183a` with an identical tree; post-main
CI/npm runs 34407524104 and 34407524144 succeeded.

### Raw artifact scan engine: partial G07 implementation

The feature branch adds a streaming byte engine with a validated policy and
explicit raw-file scope. Nine integration tests and one deadline unit test
pass. Thirteen isolated mutations are caught: dropping sequence matching,
dropping whole-file digest matches, bypassing the file-byte limit, accepting an
empty scope, swallowing read errors, accepting truncated files, ignoring prior
incompleteness, omitting names from inventory identity, overflowing the match
list, accepting duplicate entries, resetting match state at chunk boundaries,
accepting an empty policy, and omitting the final deadline check.
Restored controls pass; targeted clippy passes with warnings denied.
The full harness package suite passes 98 tests with two explicitly ignored
tests (the slow CI leg and the installed sibling shim). Rustdoc with warnings
denied and workspace formatting also pass.

This does not yet establish pre-launch protection. No artifact enumerator,
Docker export capture or CLI refusal path is wired to this engine at this
checkpoint. Its caller must impose blocking-I/O deadlines and report
enumeration failures. Negative raw-byte matching cannot exclude compressed,
encoded, transformed or previously memorized content.

### Non-extracting TAR reader: G07 integration in progress

Nine synthetic archive tests and one deadline unit test pass. They cover
repeated paths, concatenated archives, complete archive hashing, link/header
content, GNU long names, PAX metadata, unsupported sparse/size forms, malformed
records, bounded metadata allocation, padding/count limits, truncation, read
errors, dangling extensions and duplicate pending extensions. No archive is
extracted and no Docker container is started by these tests.

Thirteen isolated mutations are caught: stopping at zero blocks, removing
header matching, removing body matching, accepting PAX size overrides,
ignoring blank PAX records, removing the metadata cap, excluding padding from
the byte bound, swallowing enumeration errors, replacing the archive digest,
hiding extension entries, accepting dangling extensions, accepting duplicate
extensions and ignoring the reader deadline. Restored controls pass and the
restored source is byte-identical to the feature worktree after formatting.
The final harness suite passes 108 tests with two explicit ignores. Targeted
clippy, rustdoc with warnings denied and formatting pass.

The existing byte-engine head `c4d1c29` passed all PR #44 checks. That result
does not cover this subsequent reader addition; its new dependency and package
checks must run on the updated head. The dependency is `tar` 0.4.46 with
default features disabled, adding `filetime` transitively. Docker capture,
image configuration/volume handling and real launch refusal remain open.

Rates use the legacy entrant-reported token fields. Individual omitted counts
default to zero in that protocol; neither count completeness nor the declared
model is independently verified. Partial usage, failed requests and absent
records remain explicit limitations. Quotes are not provider invoices and do
not replace the legacy cost-normalized score columns.

Arena still consumes published Bench `=0.19.0`; the new Bench arithmetic and
diagnostic repairs do not reach that dependency until a subsequent authorized
release and pin update. Arena's own sealed-evidence and confidence repairs are
local to Arena. Historical numerical evidence has not been regenerated.

Hyper-Tau coverage and its proposed ports remain open; the detailed
[assessment](HYPER-TAU-REVIEW.md) records source reads and corrections.
In particular, the
artifact scan must not be described as proof of no contamination, rate cards
must not accept nonfinite rates or silently treat missing usage as zero, and
gateway accounting must not imply provider billing was independently verified.
