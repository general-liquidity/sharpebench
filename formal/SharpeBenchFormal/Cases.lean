/-
Copyright (c) 2026 Tiberiu Toca. All rights reserved.
Released under Apache 2.0 license as described in the file LICENSE-APACHE.
Authors: Tiberiu Toca
-/
module

public import SharpeBenchFormal.Forecast
meta import SharpeBenchFormal.Forecast
meta import Lean.Data.Json

/-!
# Shared conformance cases

`formal/cases/forecast.json` holds concrete pair-support cases. This module evaluates every case
against the model's `pairSupport`, `commonSupport` and `admittedDecision` while it elaborates and
throws on the first disagreement, so a wrong case or a changed definition fails `lake build`. The
Rust test `crates/sharpebench-core/tests/formal_cases.rs` reads the same file and checks each case
against `analyze_forecast_quality` and `analyze_forecast_quality_against_plan`. That is the
mechanical link between the model and the implementation: one table both must agree with. It is a
finite set of cases, not a refinement proof.

## Scope

Covers: the cases in `formal/cases/forecast.json`, checked here against
`formal/SharpeBenchFormal/Forecast.lean` and in Rust against
`crates/sharpebench-core/src/forecast.rs` by `crates/sharpebench-core/tests/formal_cases.rs`.

Assumes: contracts are natural-number indices, each mapped by the Rust test to one contract.
-/

open Lean

namespace SharpeBenchFormal.Cases

meta def field {α : Type} [FromJson α] (case : Json) (key : String) : IO α :=
  IO.ofExcept ((case.getObjVal? key).bind fromJson?)

meta def checkSupportCase (case : Json) : IO Unit := do
  let name : String ← field case "name"
  let plan : Option (List Nat) ← field case "plan"
  let left : List Nat ← field case "left"
  let right : List Nat ← field case "right"
  let expected : List Nat ← field case "pair_support"
  let admitted : Bool ← field case "admitted"
  unless pairSupport plan left right == expected do
    throw <| IO.userError s!"{name}: pairSupport is {pairSupport plan left right}, case says {expected}"
  if plan.isNone && commonSupport left right != expected then
    throw <| IO.userError s!"{name}: commonSupport disagrees with the case"
  unless admittedDecision plan left right == admitted do
    throw <| IO.userError s!"{name}: admittedDecision is not {admitted}"

#eval show IO Unit from do
  let cases ← IO.ofExcept (Json.parse (← IO.FS.readFile "cases/forecast.json"))
  let support : Array Json ← field cases "pair_support"
  if support.isEmpty then throw <| IO.userError "cases/forecast.json has no pair_support cases"
  support.forM checkSupportCase

end SharpeBenchFormal.Cases
