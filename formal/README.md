# Formal model

This Lean project models selected invariants of the rules SharpeBench declares
for its prospective forecast-quality report, and proves them about the model:

- exact pair support is the intersection of two agents' resolved contracts;
- the model's rank projection of an entry paired with a forecast report is that
  entry, which holds by definition for any pair and does not show that the Rust
  rank path ignores forecast data;
- one fixed-point Holm step never lowers a prior adjusted value within the cap
  and never exceeds the cap (sorting the raw p-values, the family-size
  multiplier and the withheld comparisons that stay in the family are not
  modelled, and the implementation uses `f64` rather than fixed-point values);
  and
- the finite-bootstrap plus-one numerator and denominator are positive, with
  the numerator bounded by the denominator.

Build it with:

```console
cd formal
lake build
```

The model states rules declared in `crates/sharpebench-core/src/forecast.rs`.
It is not an extraction of Rust semantics, and there is no mechanical link or
refinement proof between the model and the implementation: no Rust, Python or
TOML file references a Lean declaration. Separate executable tests in
`crates/sharpebench-core` cover the same rules independently of this model.

## Scope

Every module under `SharpeBenchFormal/` carries a `## Scope` block in its doc
comment with a `Covers:` line (the production rules it models) and an
`Assumes:` line (the assumptions the proofs rest on).
`scripts/check-lean-scope.py` fails CI when a module lacks the block or its
block references no existing repository path; it runs as one step of the
`Lean model` job. The check proves that a named path exists, not that the model
still corresponds to the code at that path.

`Forecast.lean` covers exact pair support, the intersection of two agents'
resolved contract digests that `compare_agents` differences, one ordered step
of `holm_adjust` and the plus-one bootstrap p-value in `compare_agents` (all in
`crates/sharpebench-core/src/forecast.rs`), and the projection trading rank
consumes, standing for the separation from the trading rank in
`crates/sharpebench-core/src/composite.rs`. It does not model the rule that a
pair receives inference only when the two agents' resolved sets are equal, nor
the per-agent gap disclosure in `field_support`. It assumes natural-number
fixed-point values, no floating-point semantics and two agents rather than a
field of any size.
