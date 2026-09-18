/-
Copyright (c) 2026 Tiberiu Toca. All rights reserved.
Released under Apache 2.0 license as described in the file LICENSE-APACHE.
Authors: Tiberiu Toca
-/
module

public import Init.Grind

/-!
# Forecast-quality report invariants

This module is a small Lean model of rules SharpeBench declares for its forecast-quality report:
exact pair support, the declared contract plan that narrows it, the gate that decides whether a
pair receives inference, the separation between forecast reporting and trading rank, one step of
the Holm adjustment, and the finite-bootstrap plus-one correction. The theorems are proved about
this model.

The model is not mechanically linked to the Rust implementation. No Rust, Python or TOML file
references a declaration in this module, and there is no extraction or refinement proof relating
the two. Separate executable Rust tests cover the same rules independently of this model: the unit
tests in `crates/sharpebench-core/src/forecast.rs` and the integration tests in
`crates/sharpebench-core/tests/forecast_settlement_and_support.rs` and
`crates/sharpebench-core/tests/forecast_partial_support.rs` exercise pair support, the Holm
adjustment, the bootstrap p-value and rank isolation on the implementation.

## Main results

- `mem_commonSupport_iff`: a contract is on modelled common support exactly when both agents
  resolved it. The converse half is what makes the support exact rather than merely sound.
- `mem_commonSupport`: the forward half on its own, kept as the projection callers use.
- `mem_pairSupport_iff`: under a declared plan, a contract is differenced exactly when it is in
  scope for both agents. The right-hand side is symmetric in the two agents by construction.
- `mem_pairSupport_comm`: differencing is therefore symmetric, with no hypothesis.
- `pairSupport_eq_of_admitted`: when the gate admits inference, the differenced set is not merely
  contained in each agent's in-scope support, it is that support.
- `commonSupport_eq_of_admitted`: the no-plan specialisation of the previous result, which is the
  rule the v2 report declares. It is a direct application, not a separate proof.
- `rankProjection_attachForecast`: the model's rank projection of an entry paired with a report is
  that entry; this holds by definition for any pair and is not a result about the Rust rank path.
- `holmStep_monotone`: one modelled Holm step never lowers a prior adjusted value within the cap.
- `holmStep_le`: one modelled Holm step never exceeds the cap.
- `correctedBootstrapCounts_pos`: the plus-one numerator and denominator are strictly positive.
- `correctedBootstrapCounts_le`: when the extreme count is at most the sample count, the plus-one
  numerator is at most the denominator.

## Scope

Covers: rules declared in `crates/sharpebench-core/src/forecast.rs`.

Exact pair support, the intersection of two agents' resolved contract digests that
`compare_agents` differences (`commonSupport`, `mem_commonSupport_iff`).

The declared contract universe (`ForecastContractPlan`, `parse_forecast_contract_plan`) that
`analyze_forecast_quality_against_plan` filters every agent's rows against before `compare_agents`
runs, so a planned pair is differenced on the plan restricted to both agents rather than on the two
agents alone (`inScope`, `pairSupport`). This is the rule `PLAN_SUPPORT_RULE` states for report
schema `FORECAST_QUALITY_PLAN_SCHEMA`; taking no plan recovers `SUPPORT_RULE`, the rule
`FORECAST_QUALITY_SCHEMA` states, as the specialisation `commonSupport_eq_of_admitted`.

The condition under which `compare_agents` emits inference at all, modelled as the hypothesis
`inferenceAdmitted`: neither agent resolved a digest in scope that the other did not, which is what
`UnequalResolvedSupport` refuses. Under it the differenced set is each agent's whole in-scope
support (`pairSupport_eq_of_admitted`).

One ordered step of `holm_adjust`, which takes the larger of the prior adjusted value and the
candidate and caps it at 1.0 (`holmStep`); and the plus-one p-value in `compare_agents`, whose
numerator is the extreme count plus one and whose denominator is the bootstrap sample count plus
one (`correctedBootstrapCounts`). It also records the projection trading rank consumes
(`rankProjection`), standing for the separation between the forecast report and the trading rank
in `crates/sharpebench-core/src/composite.rs`, whose submission type carries no forecast field.

Assumes: natural-number fixed-point values in place of the Rust `f64` values, with no
floating-point semantics; a generic cap in place of 1.0; a prior adjusted value already within the
cap; an extreme count no greater than the sample count; two agents in place of a field of any
size; and lists over a type with lawful boolean equality in place of the Rust set of digest
strings, with the plan a plain list rather than the duplicate-free vector
`parse_forecast_contract_plan` validates.

Not modelled: that the Rust `SupportGap` arithmetic decides `inferenceAdmitted`. The gate is a
hypothesis here, not a decision procedure, so nothing in this module shows that the two agree.
Also not modelled: `outside_plan_by_agent`, the per-agent disclosure of resolved digests the plan
does not name; the per-agent gap disclosure in `field_support`, whose field support is a union and
not this intersection; sorting the raw p-values before the Holm steps, the family-size multiplier
that forms each Holm candidate, the withheld comparisons that stay in the family size without an
adjusted value, the division that turns the plus-one counts into a p-value, and the Rust rank
functions themselves.

Check: the CI scope check (scripts/check-lean-scope.py) proves only that every repository path
named in backticks in this block exists. It does not prove that the rules described here still
correspond to the code at that path.
-/

public section

namespace SharpeBenchFormal

variable {Contract : Type} [BEq Contract] [LawfulBEq Contract]

/-- Exact common support for two agents is list intersection, not a union or padded field. -/
def commonSupport (left right : List Contract) : List Contract :=
  left.filter (· ∈ right)

/-- A contract is on exact common support exactly when both agents resolved it.

The forward direction alone would be satisfied by a support that resolves nothing, so it is the
converse that earns the word exact. -/
theorem mem_commonSupport_iff {left right : List Contract} {contract : Contract} :
    contract ∈ commonSupport left right ↔ contract ∈ left ∧ contract ∈ right := by
  simp [commonSupport]

/-- Every contract on exact common support is present for both agents. -/
theorem mem_commonSupport {left right : List Contract} {contract : Contract}
    (h : contract ∈ commonSupport left right) :
    contract ∈ left ∧ contract ∈ right :=
  mem_commonSupport_iff.mp h

/-- The digests in scope for one agent: the declared plan restricted to what that agent resolved,
or the agent's whole resolved list when no plan is declared.

Models the row filter `analyze_forecast_quality_against_plan` applies before any pair is formed. -/
def inScope (plan : Option (List Contract)) (resolved : List Contract) : List Contract :=
  match plan with
  | none => resolved
  | some plan => plan.filter (· ∈ resolved)

/-- The support a pair is differenced on: what is in scope for the first agent and resolved by the
second. Under a plan this is the plan restricted to both agents, not the two agents alone. -/
def pairSupport (plan : Option (List Contract)) (left right : List Contract) : List Contract :=
  (inScope plan left).filter (· ∈ right)

/-- The condition `compare_agents` requires before it emits an interval, a p-value or a
significance flag: neither agent resolved a digest in scope that the other did not. -/
def inferenceAdmitted (plan : Option (List Contract)) (left right : List Contract) : Prop :=
  ∀ contract, contract ∈ inScope plan left ↔ contract ∈ inScope plan right

/-- A digest in scope for an agent was resolved by that agent. -/
theorem mem_inScope {plan : Option (List Contract)} {resolved : List Contract} {contract : Contract}
    (h : contract ∈ inScope plan resolved) : contract ∈ resolved := by
  cases plan with
  | none => simpa [inScope] using h
  | some plan => exact (by simpa [inScope] using h : contract ∈ plan ∧ contract ∈ resolved).2

/-- A contract is differenced exactly when it is in scope for both agents.

The right-hand side is symmetric in the two agents, which is what makes the two support gaps
`compare_agents` computes measure the same set from either side. -/
theorem mem_pairSupport_iff {plan : Option (List Contract)} {left right : List Contract}
    {contract : Contract} :
    contract ∈ pairSupport plan left right ↔
      contract ∈ inScope plan left ∧ contract ∈ inScope plan right := by
  cases plan with
  | none => simp [pairSupport, inScope]
  | some plan =>
    simp only [pairSupport, inScope, List.mem_filter, decide_eq_true_eq]
    grind

/-- Differencing is symmetric in the two agents, with no hypothesis.

List equality is not: `pairSupport` keeps the order of its first argument, so only membership is
symmetric. That is the level the support-gap counts are taken at. -/
theorem mem_pairSupport_comm {plan : Option (List Contract)} {left right : List Contract}
    {contract : Contract} :
    contract ∈ pairSupport plan left right ↔ contract ∈ pairSupport plan right left := by
  rw [mem_pairSupport_iff, mem_pairSupport_iff]
  grind

/-- When the gate admits inference, the differenced set is each agent's whole in-scope support.

This is what makes the reported contract count, the bootstrap blocks and the Holm family size mean
what the report says they mean: a subset would leave every p-value computed over less than the
support it is attributed to. -/
theorem pairSupport_eq_of_admitted {plan : Option (List Contract)} {left right : List Contract}
    (h : inferenceAdmitted plan left right) :
    pairSupport plan left right = inScope plan left := by
  refine List.filter_eq_self.mpr ?_
  intro contract hcontract
  simpa using mem_inScope ((h contract).mp hcontract)

/-- The rule a report without a declared plan states, as the specialisation of the plan rule.

One theorem covers both report schemas: this is an application of `pairSupport_eq_of_admitted`,
not a second proof. -/
theorem commonSupport_eq_of_admitted {left right : List Contract}
    (h : inferenceAdmitted none left right) : commonSupport left right = left :=
  pairSupport_eq_of_admitted h

/-- A trading leaderboard entry, reduced to the fields relevant to rank projection. -/
structure TradingEntry where
  entrant : String
  eligible : Bool
  rankKey : Int
  deriving DecidableEq, Repr

/-- Forecast diagnostics are deliberately not part of a trading entry. -/
structure ForecastReport where
  resolvedClaims : Nat
  meanLossNumerator : Int
  deriving DecidableEq, Repr

/-- Pair a report with an entry without mutating the entry. -/
def attachForecast (entry : TradingEntry) (report : ForecastReport) :
    TradingEntry × ForecastReport :=
  (entry, report)

/-- In this model, trading rank consumes only the first component of the pair. -/
def rankProjection (value : TradingEntry × ForecastReport) : TradingEntry :=
  value.1

/-- The projection rank consumes returns the entry that was paired with a report.

This holds by `rfl`: it unfolds to `(entry, report).1 = entry`, which is true for a pair of any
two types. The theorem records which projection rank consumes in this model. It does not establish
that the Rust rank path ignores forecast data.
-/
theorem rankProjection_attachForecast (entry : TradingEntry) (report : ForecastReport) :
    rankProjection (attachForecast entry report) = entry := by
  rfl

/-- One ordered Holm step on natural-number fixed-point values, `min cap (max prior candidate)`.

It has the shape of the `f64` step in `holm_adjust`; floating-point semantics are not modelled.
-/
def holmStep (prior candidate cap : Nat) : Nat :=
  Nat.min cap (Nat.max prior candidate)

/-- Provided the previous adjusted value is valid, the next ordered value cannot decrease. -/
theorem holmStep_monotone {prior candidate cap : Nat} (hprior : prior ≤ cap) :
    prior ≤ holmStep prior candidate cap := by
  grind [holmStep]

/-- Every fixed-point Holm step stays under its probability cap. -/
theorem holmStep_le (prior candidate cap : Nat) :
    holmStep prior candidate cap ≤ cap := by
  grind [holmStep]

/-- Numerator and denominator of the finite-bootstrap plus-one correction. -/
def correctedBootstrapCounts (extreme samples : Nat) : Nat × Nat :=
  (extreme + 1, samples + 1)

/-- A finite bootstrap cannot emit a zero numerator or denominator after correction. -/
theorem correctedBootstrapCounts_pos (extreme samples : Nat) :
    0 < (correctedBootstrapCounts extreme samples).1 ∧
      0 < (correctedBootstrapCounts extreme samples).2 := by
  grind [correctedBootstrapCounts]

/-- A valid extreme count produces a corrected numerator no greater than its denominator. -/
theorem correctedBootstrapCounts_le {extreme samples : Nat}
    (h : extreme ≤ samples) :
    (correctedBootstrapCounts extreme samples).1 ≤
      (correctedBootstrapCounts extreme samples).2 := by
  grind [correctedBootstrapCounts]

end SharpeBenchFormal
