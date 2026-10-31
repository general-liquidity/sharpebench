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
  theorem applied with no plan; the gate is decided by a boolean function,
  which is what the shared cases below record;
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

The package sets `autoImplicit false`, so a misspelt variable in a statement is
an error rather than a fresh universally bound variable, and `warningAsError
true`, so a `sorry` fails the build.

## What links the model to the code

The model states rules declared in `crates/sharpebench-core/src/forecast.rs`.
It is not an extraction of Rust semantics and there is no refinement proof. One
mechanism links it to the implementation: the shared case table
`formal/cases/forecast.json`. `SharpeBenchFormal/Cases.lean` evaluates every
case against `pairSupport`, `commonSupport` and `admittedDecision` while the
library builds and fails the build on a disagreement.
`crates/sharpebench-core/tests/formal_cases.rs` runs every case through
`analyze_forecast_quality` or `analyze_forecast_quality_against_plan` and
asserts that the differenced contract count is the length of the model's pair
support and that the unequal-support refusal fires exactly when the model's gate
refuses. That links pair support (by count, not by contract list, which the
report does not expose) and the inference gate, on the cases in the table only.
The Holm step, the bootstrap counts and the rank projection have no shared case,
so for them the model and the code are still related only by prose and by
separate Rust tests in `crates/sharpebench-core`.

## Gates

The `Lean model` CI job fails when any of these fails:

- `lake build`, then `leanchecker` (a kernel re-check of the library) and the
  axiom audit of `leanprover/lean-action`;
- the statement manifest: `formal/scripts/Statements.lean` prints every declaration's
  elaborated type, every definition's body and every theorem's axioms, and CI
  diffs that output against the committed `formal/statements.txt`. A weakened
  statement, a dropped converse or an extra hypothesis passes build, leanchecker
  and the axiom audit, and fails here. A deliberate change regenerates the file
  (`lake env lean --run scripts/Statements.lean SharpeBenchFormal > statements.txt`)
  and the diff is reviewed with the change. The same script refuses any declared
  `axiom` and any dependency outside `propext`, `Classical.choice` and
  `Quot.sound`, a second axiom check the repository owns;
- `scripts/check-lean-scope.py` (below), and its own tests in
  `scripts/test_check_lean_scope.py`;
- the negative controls: `formal/scripts/negative_controls.py` copies the package to a
  scratch directory and plants a `sorry`, a `sorry` with warnings-as-errors
  switched off, an extra axiom, an undeclared variable, a weakened statement and
  a wrong case row, and fails unless each is refused by the gate meant to refuse
  it.

Every theorem has a non-vacuity entry in `SharpeBenchFormal/NonVacuity.lean`: an
instance, checked by `decide`, where its hypotheses hold and its conclusion is
not trivial, and a plausible wrong definition with a decided instance on which
the theorem's property fails. The scope check fails when a theorem has none.

## Samuelson's inequality

`samuelson/` is a separate Lean package that proves, over the reals, the
theorem the near-constant refusal `sign_is_forced_by_moments` in
`crates/sharpebench-stats/src/deflated_sharpe.rs` relies on (on
`fix/near-constant-admission` when this was written): if
`|mean| * sqrt n > s * (n - 1)`, with `s` the Bessel-corrected standard
deviation, every observation is strictly on the mean's side of zero
(`pos_mul_mean_of_signForced`, from Samuelson's inequality `samuelson_sq`).

It needs Mathlib, and the forecast model does not. Adding Mathlib to this
package would turn a build of seconds into a Mathlib cache download on every
run of the main gate, so the proof lives in its own package with its own CI job
that fetches the Mathlib cache, builds, runs the axiom audit and diffs its own
`statements.txt`. The cost is that job alone. The proof is about real numbers:
the Rust function evaluates the inequality in `f64`, and a window within
rounding of the threshold can be classified differently. That rounding is
outside the proof, and nothing links the package to the Rust function
mechanically. Leanchecker is not run on it: it would re-check Mathlib as well.

## Scope

Every module in the `SharpeBenchFormal` library, the root module and every
module under `SharpeBenchFormal/` at any depth, carries a `## Scope` block in
its doc comment with a `Covers:` line (the production rules it models) and an
`Assumes:` line (the assumptions the proofs rest on).
`scripts/check-lean-scope.py` fails CI when a module lacks the block, when its
block names a repository path that does not exist, when it names a production
symbol (a backticked identifier that is not a Lean name the library uses) that
occurs in none of the non-Lean files the block names, when a module's
`## Main results` list names a theorem the module does not declare, or when a
theorem has no entry in the non-vacuity module. The check proves that every name
exists where the documentation says, not that the model still corresponds to the
code at those names.

`Forecast.lean` covers exact pair support, the intersection of two agents'
resolved contract digests that `compare_agents` differences; the declared
contract plan that `analyze_forecast_quality_against_plan` filters each agent's
rows against before a pair is formed; the condition under which `compare_agents`
emits inference at all, taken as a hypothesis and decided by `admittedDecision`;
one ordered step of `holm_adjust` and the plus-one bootstrap p-value in
`compare_agents` (all in `crates/sharpebench-core/src/forecast.rs`), and the
projection trading rank consumes, standing for the separation from the trading
rank in `crates/sharpebench-core/src/composite.rs`. It does not model the
support-gap arithmetic that decides that condition in Rust (the shared cases
compare the two on a finite table), the per-agent disclosure of resolved
digests a plan does not name, nor the per-agent gap disclosure in
`field_support`. It assumes natural-number fixed-point values, no
floating-point semantics and two agents rather than a field of any size.
