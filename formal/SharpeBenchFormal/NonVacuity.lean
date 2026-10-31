/-
Copyright (c) 2026 Tiberiu Toca. All rights reserved.
Released under Apache 2.0 license as described in the file LICENSE-APACHE.
Authors: Tiberiu Toca
-/
module

public import SharpeBenchFormal.Forecast

/-!
# Non-vacuity of the forecast-report theorems

A theorem whose hypotheses cannot be met is true of nothing, and a theorem that holds of every
definition says nothing about the one it names. For every theorem in `Forecast.lean` this module
gives two things, each checked by `decide` on a concrete instance:

- a witness: an instance where the hypotheses hold and the conclusion is not trivially true;
- a mutant: a plausible wrong definition, and an instance on which the theorem's property fails
  for it, so the property separates the definition from that bug.

`scripts/check-lean-scope.py` fails CI when a theorem of the library is not named in this module.

## Scope

Covers: no production rule of its own. It exercises the model in
`formal/SharpeBenchFormal/Forecast.lean`, whose own block names the production code.

Assumes: the definitions of that module, evaluated on natural-number contracts.
-/

namespace SharpeBenchFormal.NonVacuity

open SharpeBenchFormal

/-! ## `mem_commonSupport_iff` and `mem_commonSupport` -/

/-- Witness: a contract both agents resolved is on support, and support is not empty. -/
example : 1 ∈ commonSupport [0, 1] [1, 2] ∧ 1 ∈ [0, 1] ∧ 1 ∈ [1, 2] := by decide

/-- Mutant: support taken as a union. A contract only one agent resolved lands on it. -/
def commonSupportUnion (left right : List Nat) : List Nat := left ++ right

example : 1 ∈ commonSupportUnion [0] [1] ∧ ¬ (1 ∈ [0] ∧ 1 ∈ [1]) := by decide

/-! ## `mem_inScope` -/

/-- Witness: under a plan, a planned and resolved digest is in scope. -/
example : 0 ∈ inScope (some [0, 2]) [0, 1] ∧ 0 ∈ [0, 1] := by decide

/-- Mutant: the plan taken as the scope without restricting it to what the agent resolved. -/
def inScopeUnfiltered (plan : Option (List Nat)) (resolved : List Nat) : List Nat :=
  plan.getD resolved

example : 0 ∈ inScopeUnfiltered (some [0]) [] ∧ 0 ∉ ([] : List Nat) := by decide

/-! ## `mem_pairSupport_iff` -/

/-- Witness: under a plan, the differenced set is the planned digests both agents resolved. -/
example : pairSupport (some [0, 1, 2]) [0, 1, 3] [1, 3] = [1] := by decide

/-- Mutant: the plan ignored when differencing. A digest outside the plan is differenced. -/
def pairSupportIgnoringPlan (_plan : Option (List Nat)) (left right : List Nat) : List Nat :=
  left.filter (· ∈ right)

example : 1 ∈ pairSupportIgnoringPlan (some [0]) [0, 1] [0, 1] ∧ 1 ∉ inScope (some [0]) [0, 1] := by
  decide

/-! ## `mem_pairSupport_comm` -/

/-- Witness: the two orders difference the same digests on a non-trivial pair. -/
example : 1 ∈ pairSupport none [0, 1] [1, 2] ∧ 1 ∈ pairSupport none [1, 2] [0, 1] := by decide

/-- Mutant: the first agent's support alone, as if the second agent's gaps did not count. -/
def pairSupportOneSided (plan : Option (List Nat)) (left _right : List Nat) : List Nat :=
  inScope plan left

example : 1 ∈ pairSupportOneSided none [0, 1] [0] ∧ 1 ∉ pairSupportOneSided none [0] [0, 1] := by
  decide

/-! ## `inferenceAdmitted_iff_admittedDecision` -/

/-- Witness: the decision accepts an admitted pair and refuses one with a gap. -/
example : admittedDecision (some [0, 1, 2]) [0, 1] [1, 0] = true ∧
    admittedDecision none [0] [0, 1] = false := by decide

/-- Mutant: a one-sided gate that only asks whether the first agent's support is covered. It
accepts a pair the gate refuses, because the second agent resolved a digest the first did not. -/
def admittedOneSided (plan : Option (List Nat)) (left right : List Nat) : Bool :=
  (inScope plan left).all (· ∈ inScope plan right)

example : admittedOneSided none [0] [0, 1] = true ∧ ¬ inferenceAdmitted none [0] [0, 1] :=
  ⟨by decide, fun h ↦ absurd ((h 1).mpr (by decide)) (by decide)⟩

/-! ## `pairSupport_eq_of_admitted` and `commonSupport_eq_of_admitted` -/

/-- Witness: an admitted pair under a plan, whose differenced set is a non-empty support. -/
example : inferenceAdmitted (some [0, 1, 2]) [0, 1] [1, 0] ∧
    pairSupport (some [0, 1, 2]) [0, 1] [1, 0] = [0, 1] :=
  ⟨inferenceAdmitted_iff_admittedDecision.mpr (by decide), by decide⟩

/-- Witness for the no-plan rule: equal support, so the gate admits it. -/
example : inferenceAdmitted none [0, 1] [1, 0] ∧ commonSupport [0, 1] [1, 0] = [0, 1] :=
  ⟨inferenceAdmitted_iff_admittedDecision.mpr (by decide), by decide⟩

/-- Mutant: the plan used as the support without restricting it to what both agents resolved. On
an admitted pair it differences a digest neither agent resolved. -/
def pairSupportFromPlan (plan : Option (List Nat)) (left _right : List Nat) : List Nat :=
  plan.getD left

example : inferenceAdmitted (some [0, 1]) [0] [0] ∧
    pairSupportFromPlan (some [0, 1]) [0] [0] ≠ inScope (some [0, 1]) [0] :=
  ⟨inferenceAdmitted_iff_admittedDecision.mpr (by decide), by decide⟩

/-- Mutant for the no-plan rule: the union double-counts a contract both agents resolved. -/
example : inferenceAdmitted none [0] [0] ∧ commonSupportUnion [0] [0] ≠ [0] :=
  ⟨inferenceAdmitted_iff_admittedDecision.mpr (by decide), by decide⟩

/-! ## `rankProjection_attachForecast` -/

/-- Witness: the projection returns an eligible entry unchanged next to a non-empty report. -/
example : rankProjection (attachForecast ⟨"a", true, 7⟩ ⟨3, -2⟩) = ⟨"a", true, 7⟩ := by decide

/-- Mutant: a rank projection that folds the forecast loss into the rank key. -/
def rankProjectionLeaky (value : TradingEntry × ForecastReport) : TradingEntry :=
  { value.1 with rankKey := value.1.rankKey + value.2.meanLossNumerator }

example : rankProjectionLeaky (attachForecast ⟨"a", true, 7⟩ ⟨3, -2⟩) ≠ ⟨"a", true, 7⟩ := by
  decide

/-! ## `holmStep_monotone` and `holmStep_le` -/

/-- Witness: a valid prior, and a step that moves up to the candidate. -/
example : 3 ≤ 10 ∧ holmStep 3 7 10 = 7 := by decide

/-- Witness: a candidate above the cap is capped. -/
example : holmStep 3 12 10 = 10 := by decide

/-- Mutant: the step without the running maximum, which lets an adjusted value fall. -/
def holmStepNoMax (_prior candidate cap : Nat) : Nat := Nat.min cap candidate

example : 5 ≤ 10 ∧ ¬ 5 ≤ holmStepNoMax 5 1 10 := by decide

/-- Mutant: the step without the cap, which lets an adjusted value exceed it. -/
def holmStepNoCap (prior candidate _cap : Nat) : Nat := Nat.max prior candidate

example : ¬ holmStepNoCap 0 20 10 ≤ 10 := by decide

/-! ## `correctedBootstrapCounts_pos` and `correctedBootstrapCounts_le` -/

/-- Witness: no extreme draw still gives a positive numerator; the bound holds with room. -/
example : correctedBootstrapCounts 0 0 = (1, 1) ∧ 2 ≤ 5 ∧
    correctedBootstrapCounts 2 5 = (3, 6) := by
  decide

/-- Mutant: the raw counts, without the plus-one correction, give a zero numerator. -/
def bootstrapCountsUncorrected (extreme samples : Nat) : Nat × Nat := (extreme, samples)

example : ¬ 0 < (bootstrapCountsUncorrected 0 0).1 := by decide

/-- Mutant: plus one on the numerator only, which exceeds the denominator when every draw is
extreme. -/
def bootstrapCountsNumeratorOnly (extreme samples : Nat) : Nat × Nat := (extreme + 1, samples)

example : 3 ≤ 3 ∧
    ¬ (bootstrapCountsNumeratorOnly 3 3).1 ≤ (bootstrapCountsNumeratorOnly 3 3).2 := by
  decide

end SharpeBenchFormal.NonVacuity
