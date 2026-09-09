# Verification record

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
  with maturin in an isolated virtual environment. That is editable-package
  validation, not yet a fresh installed-wheel or cross-platform proof.
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

These regressions use synthetic inputs and existing artifacts. No model was
downloaded or called, no API credits were spent, and no market data was acquired.

## Local commands and results

Bench:

- `cargo test -p sharpebench --locked`: 63 tests passed for the initial
  seed/date repairs. Later full-workspace validation is still required.
- `cargo test -p sharpebench-stats -p sharpebench-edge --locked`:
  102 stats unit tests, 10 statistical-boundary tests, 4 compatibility test
  functions, 31 edge unit tests and both doc tests passed.
- `cargo test -p sharpebench-core --locked --quiet`: 330 unit tests,
  37 integration tests and one doc test passed.
- Affected-package clippy with warnings denied passed.
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

The repairs are granular local commits. Final package rebuilding, installed
consumer tests, pushed-head CI, normal merges and post-merge verification remain
delivery gates. Do not treat a local pass as a released behavior change.

Arena still consumes published Bench `=0.19.0`; the new Bench arithmetic and
diagnostic repairs do not reach that dependency until a subsequent authorized
release and pin update. Arena's own sealed-evidence and confidence repairs are
local to Arena. Historical numerical evidence has not been regenerated.

Hyper-Tau coverage and its proposed ports remain open. In particular, the
artifact scan must not be described as proof of no contamination, rate cards
must not accept nonfinite rates or silently treat missing usage as zero, and
gateway accounting must not imply provider billing was independently verified.
