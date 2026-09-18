# Formal model

This Lean project models selected invariants of the rules SharpeBench declares
for its prospective forecast-quality report, and proves them about the model:

- exact pair support is the intersection of two agents' resolved contracts, in
  both directions: a contract is on the support exactly when both agents
  resolved it, so a support that resolved nothing would not satisfy the
  statement;
- under a declared contract plan, a pair is differenced on the plan restricted
  to both agents, and a contract is differenced exactly when it is in scope for
  both, which is symmetric in the two agents with no further hypothesis;
- when the pair passes the gate that refuses unequal resolved support, the
  differenced set is each agent's whole in-scope support rather than some
  subset of it, and the rule a report without a plan declares is the same
  theorem applied with no plan;
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

Every module in the `SharpeBenchFormal` library, the root module and every
module under `SharpeBenchFormal/` at any depth, carries a `## Scope` block in
its doc comment with a `Covers:` line (the production rules it models) and an
`Assumes:` line (the assumptions the proofs rest on).
`scripts/check-lean-scope.py` fails CI when a module lacks the block or its
block names a repository path that does not exist; it runs as one step of the
`Lean model` job. The check proves that every named path exists, not that the
model still corresponds to the code at those paths.

`Forecast.lean` covers exact pair support, the intersection of two agents'
resolved contract digests that `compare_agents` differences; the declared
contract plan that `analyze_forecast_quality_against_plan` filters each agent's
rows against before a pair is formed; the condition under which `compare_agents`
emits inference at all, taken as a hypothesis; one ordered step of `holm_adjust`
and the plus-one bootstrap p-value in `compare_agents` (all in
`crates/sharpebench-core/src/forecast.rs`), and the projection trading rank
consumes, standing for the separation from the trading rank in
`crates/sharpebench-core/src/composite.rs`. It does not model the support-gap
arithmetic that decides that condition in Rust, the per-agent disclosure of
resolved digests a plan does not name, nor the per-agent gap disclosure in
`field_support`. It assumes natural-number fixed-point values, no
floating-point semantics and two agents rather than a field of any size.
