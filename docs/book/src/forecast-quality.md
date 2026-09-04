# Prospective forecast quality

`sharpebench forecast-quality` analyzes raw prospective forecasts separately
from the trading leaderboard. Forecast performance cannot satisfy, weaken, or
replace any trading-rank gate.

## Input boundary

The command accepts one `sharpe.forecast-evidence.v1` file per agent:

```bash
sharpebench forecast-quality agent-a.json agent-b.json
sharpebench forecast-quality agent-a.json agent-b.json --json
```

SharpeArena writes the native format, but the boundary is a versioned JSON file,
not a package dependency. SharpeBench rejects unknown fields, unknown versions,
bad digests, duplicate identities, broken revision chains, inconsistent clocks,
missing resolutions, nonfinite values, invalid probability vectors, and outcomes
that do not match the frozen contract.

Each file identifies the model, scaffold, prompt, operator, and configuration by
name or SHA-256. Each revision records whether consensus was visible and, when it
was, the consensus snapshot digest. A late revision remains auditable but never
becomes the scored forecast.

## Scores and calibration

SharpeBench ignores any producer-side calculation and recomputes the declared
score from the raw prediction and outcome:

| Forecast | Recomputed loss or diagnostic |
|---|---|
| point | squared error |
| binary probability | Brier or log loss |
| categorical distribution | multiclass Brier or log loss |
| Normal distribution | closed-form CRPS and probability integral transform |
| direction | zero-one loss outside the frozen neutral band |
| interval | proper interval score at the frozen alpha |

Binary reports include fixed-bin reliability, resolution, uncertainty, and a
Brier skill score against the observed base-rate forecast. Categorical reports
calibrate the selected category's confidence. Normal reports include the PIT
mean, variance, and histogram against the uniform reference. Resolution rates
and blind versus consensus-exposed counts remain visible beside score means.

## Exact support and dependence

Agents are compared only on the exact contract-digest intersection resolved by
the whole field. An unmatched question or horizon is excluded for every pair,
and the excluded count is reported per agent. This prevents a favorable
pair-specific subset from becoming the comparison set.

The resampler treats all assets and questions with the same resolution clock as
one block. It draws whole blocks, preserving contemporaneous dependence rather
than pretending every forecast is independent. The report gives the observed
mean loss difference, a percentile interval, and a two-sided block-bootstrap
p-value. Holm adjustment controls the familywise error rate across all reported
pairs.

Relevant options are:

```text
--bootstrap-samples N   deterministic resample count (default 2000)
--seed N                explicit resampling seed
--confidence C          interval coverage inside (0, 1)
--alpha A               familywise significance level inside (0, 1)
--bins N                calibration and PIT bin count
```

## Interpretation limits

- The ledger clock establishes logical order, not independently verified wall
  time.
- Exact common support removes question mismatch. It does not make agents,
  prompts, or information sets identical.
- Resolution-time blocks preserve a declared dependence unit. They do not prove
  that no longer-range dependence exists.
- Calibration and proper scores describe forecast quality. Trading eligibility
  still requires the Deflated Sharpe, pass^k, significance, process, and mandate
  gates.
