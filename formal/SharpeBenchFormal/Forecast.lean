/-
Copyright (c) 2026 Tiberiu Toca. All rights reserved.
Released under Apache 2.0 license as described in the file LICENSE-APACHE.
Authors: Tiberiu Toca
-/
module

public import Init.Grind
public import Std

/-!
# Forecast-quality report invariants

This module is a small Lean model of rules SharpeBench declares for its forecast-quality report:
exact pair support, the separation between forecast reporting and trading rank, one step of the
Holm adjustment, and the finite-bootstrap plus-one correction. The theorems are proved about this
model.

The model is not mechanically linked to the Rust implementation. No Rust, Python or TOML file
references a declaration in this module, and there is no extraction or refinement proof relating
the two. Separate executable Rust tests cover the same rules independently of this model: the unit
tests in `crates/sharpebench-core/src/forecast.rs` and the integration tests in
`crates/sharpebench-core/tests/forecast_settlement_and_support.rs` and
`crates/sharpebench-core/tests/forecast_partial_support.rs` exercise pair support, the Holm
adjustment, the bootstrap p-value and rank isolation on the implementation.

## Main results

- `mem_commonSupport`: every contract on modelled common support occurs in both agents' lists.
- `attachForecast_rankProjection`: the model's rank projection of an entry paired with a report is
  that entry; this holds by definition for any pair and is not a result about the Rust rank path.
- `holmStep_monotone`: one modelled Holm step never lowers a prior adjusted value within the cap.
- `holmStep_bounded`: one modelled Holm step never exceeds the cap.
- `correctedBootstrapPValue_positive`: the plus-one numerator and denominator are strictly
  positive.
- `correctedBootstrapPValue_bounded`: when the extreme count is at most the sample count, the
  plus-one numerator is at most the denominator.

## Scope

Covers: rules declared in `crates/sharpebench-core/src/forecast.rs`: exact pair support, the
intersection of two agents' resolved contract digests that `compare_agents` differences
(`commonSupport`); one ordered step of `holm_adjust`, which takes the larger of the prior adjusted value and the candidate
and caps it at 1.0 (`holmStep`); and the plus-one p-value in `compare_agents`, whose numerator is
the extreme count plus one and whose denominator is the bootstrap sample count plus one
(`correctedBootstrapCounts`). It also records the projection trading rank consumes
(`rankProjection`), standing for the separation between the forecast report and the trading rank
in `crates/sharpebench-core/src/composite.rs`, whose submission type carries no forecast field.

Assumes: natural-number fixed-point values in place of the Rust `f64` values, with no
floating-point semantics; a generic cap in place of 1.0; a prior adjusted value already within the
cap; an extreme count no greater than the sample count; two agents in place of a field of any
size; and lists over a type with lawful boolean equality in place of the Rust set of digest
strings.

Not modelled: the rule in `compare_agents` that a pair receives inference only when the two agents'
resolved digest sets are equal, so that the intersection is each agent's whole resolved set (the
model's intersection is not required to equal either list); the per-agent gap disclosure in
`field_support`; sorting the raw p-values before the Holm steps, the family-size multiplier that
forms each Holm candidate, the withheld comparisons that stay in the family size without an
adjusted value, the division that turns the plus-one counts into a p-value, and the Rust rank
functions themselves.

Check: the CI scope check (scripts/check-lean-scope.py) proves only that at least one repository
path named in backticks in this block exists. It does not prove that the rules described here still
correspond to the code at that path.
-/

public section

namespace SharpeBenchFormal

variable {Contract : Type} [BEq Contract] [LawfulBEq Contract]

/-- Exact common support for two agents is list intersection, not a union or padded field. -/
def commonSupport (left right : List Contract) : List Contract :=
  left.filter (· ∈ right)

/-- Every contract on exact common support is present for both agents. -/
theorem mem_commonSupport {left right : List Contract} {contract : Contract}
    (h : contract ∈ commonSupport left right) :
    contract ∈ left ∧ contract ∈ right := by
  simpa [commonSupport] using h

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
theorem attachForecast_rankProjection (entry : TradingEntry) (report : ForecastReport) :
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
theorem holmStep_bounded (prior candidate cap : Nat) :
    holmStep prior candidate cap ≤ cap := by
  grind [holmStep]

/-- Numerator and denominator of the finite-bootstrap plus-one correction. -/
def correctedBootstrapCounts (extreme samples : Nat) : Nat × Nat :=
  (extreme + 1, samples + 1)

/-- A finite bootstrap cannot emit a zero numerator or denominator after correction. -/
theorem correctedBootstrapPValue_positive (extreme samples : Nat) :
    0 < (correctedBootstrapCounts extreme samples).1 ∧
      0 < (correctedBootstrapCounts extreme samples).2 := by
  grind [correctedBootstrapCounts]

/-- A valid extreme count produces a corrected numerator no greater than its denominator. -/
theorem correctedBootstrapPValue_bounded {extreme samples : Nat}
    (h : extreme ≤ samples) :
    (correctedBootstrapCounts extreme samples).1 ≤
      (correctedBootstrapCounts extreme samples).2 := by
  grind [correctedBootstrapCounts]

end SharpeBenchFormal
