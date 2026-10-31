/-
Copyright (c) 2026 Tiberiu Toca. All rights reserved.
Released under Apache 2.0 license as described in the file LICENSE-APACHE.
Authors: Tiberiu Toca
-/
module

public import SharpeBenchFormal.Forecast
public import SharpeBenchFormal.NonVacuity
public import SharpeBenchFormal.Cases

/-!
# SharpeBench formal model

Selected mathematical invariants used by the independent forecast-quality report.

## Scope

Covers: no rule of its own. This is the library root; it re-exports
`formal/SharpeBenchFormal/Forecast.lean`, whose own `## Scope` block names the rules that module
models and the assumptions its proofs rest on, together with the non-vacuity witnesses in
`formal/SharpeBenchFormal/NonVacuity.lean` and the shared conformance cases checked by
`formal/SharpeBenchFormal/Cases.lean`.

Assumes: nothing beyond the assumptions the re-exported modules declare.

Check: the CI scope check (scripts/check-lean-scope.py) proves that every repository path named in
backticks in this block exists and that every production symbol named in backticks occurs in a file
the block names. It does not prove that the rules described here still correspond to that code.
-/
