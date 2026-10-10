# Approved forecast comparison strata

Owner approved rule/unit-stratified reporting with no pooled primary estimate
on 2026-10-10. Baseline: `1269018`. This closes the R08 implementation choice,
not the separate scientific calibration or study-design requirements.

## Contract

- Newly generated reports use `sharpebench.forecast-quality.v5`.
- Agent loss metrics and pairwise comparisons use exact `(scoring_rule,
  target_unit)` labels. Unit means the contract's unit string, not a guessed
  conversion, common estimand or normalized loss.
- Each pair has a lexically ordered row per resolved stratum in its union.
  An empty stratum keeps its labels and null estimate; neither agent resolving
  anything produces one unavailable row with null labels.
- Settlement agreement and whole-pair support gates apply before partitioning.
  A gap in one stratum withholds inference on the entire pair, preserving the
  anti-selection rule. Plan exclusion still precedes scoring/partitioning.
- Each stratum uses its own resolution-time blocks. Holm adjustment runs once
  over all pair/stratum rows, retaining withheld rows in the family size.
- No pooled primary estimate or mixed-loss verdict is emitted. Calibration
  diagnostics remain descriptive, separate from trading eligibility.

## Verification performed

- All 490 core library tests passed, including 25 forecast-filtered cases.
- Four new controls cover same-rule/different-unit partitioning, mixed-rule
  single-stratum parity and one Holm family, cross-stratum support gaps/null
  estimates, and plan exclusion/empty labels. R08's former pooling
  characterization now asserts separated results under USD/cents rescaling.
- All 14 partial-support integration tests and 8 forecast CLI tests passed.
- The full core package passed 588 tests plus its documentation test. The first
  remote run caught a settlement integration fixture still pinning the older
  comparison JSON shape; its v5 expectation now includes both labels while
  retaining every numerical byte. Library-only checks had missed that fixture.
- Isolated mutations removing unit separation and rule separation each failed
  the respective regression on numeric/support assertions. Restoring the source
  passed all 25 forecast-filtered tests.
- Historical tutorial input digests remain checked. Every historical output
  field matches after removing only the new labels and restoring the historical
  schema tag. Frozen fixture/report bytes were not rewritten.
- Core and CLI Clippy passed with warnings denied; the paired-boundary gate
  passed with its existing allowlist unchanged.
- A separately installed release CLI emitted v5 JSON with rule/unit labels and
  the unchanged tutorial numerical result; its human output printed those labels.
  Exact-head remote CI remains a delivery gate.
  Forecast reports are consumed by the Rust library/CLI; this pass does not
  assert a new Python or WASM forecast-report endpoint.

No study, release, numerical evidence regeneration or Arena registry pin change
was authorized or performed.
