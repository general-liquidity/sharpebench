# Prospective capability rereview, 2026-09-04

## Scope

This rereview separates the frozen v0.9.0 performance evidence from later
engineering capabilities. The prospective forecast report, cross-product
fixture, and Lean model are tested capabilities. They are not a current-model
result and cannot alter the trading leaderboard.

## Checks performed

- Compared the manuscript with `forecast.rs`, the forecast CLI, the frozen
  cross-product report, and the Lean model.
- Ran the focused Rust forecast suite, reproduced the complete JSON report, and
  built the Lean project.
- Verified field-wide exact common support, resolution-clock block construction,
  finite-bootstrap plus-one correction, Holm adjustment, and strict ingestion
  in source and tests.
- Built the paper and checked for overfull boxes, undefined references, and
  undefined citations.
- Scanned the manuscript for private execution context, obsolete model names,
  copied current-version claims, and long embedded digests.

## Findings and dispositions

1. A draft said an executable fixture changed a forecast report and proved a
   trading board byte-identical. The repository instead proves a narrower fact:
   the analyzer has a separate API, and a test ranks the same submission before
   and after forecast analysis and requires equal boards. The paper now states
   exactly that; Lean separately proves rank projection in its abstract model.
2. The introduction described a failed paid-model attempt. That operational
   history is not evidence and was removed. The manuscript states only that no
   current-model result is admitted.
3. Current versions, workflow counts, and test totals had been copied into
   prose. The manuscript now delegates current engineering identity to the
   provenance manifest and CI records.
4. The archived convenience-sample pilot could be mistaken for a comparison.
   Its scores are absent from the paper, and the limitations section records why
   it supports no model claim or trading rank.
5. Proper-score claims now cite the scoring-rule literature. The paper also
   states that directional accuracy is a diagnostic, not a proper probabilistic
   score.

## Residual boundaries

- Exact common support prevents pair-specific subsets but can leave a small or
  unrepresentative intersection.
- Resolution-time blocks preserve the declared contemporaneous unit but do not
  prove longer dependence absent.
- Holm adjustment controls the reported family only under the validity of its
  input p-values and the declared resampling design.
- Proper forecast scores do not establish executable trading edge, costs, risk
  control, or deployability, so they remain outside the rank predicate.
- No current-model performance claim is available until a separately declared
  field is completed and admitted.

