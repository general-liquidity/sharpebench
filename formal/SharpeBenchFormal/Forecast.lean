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

This module formalizes exact common support, the separation between forecast reporting and
trading rank, the monotone Holm adjustment step, and the finite-bootstrap plus-one correction
used by SharpeBench.

The executable Rust implementation is linked to this model by conformance tests. This module
is not a proof-producing extraction of that Rust program.

## Main results

- `mem_commonSupport`: every common-support contract occurs for both agents.
- `attachForecast_rankProjection`: attaching a forecast report cannot change trading rank.
- `holmStep_monotone`: sorted fixed-point Holm adjusted values cannot decrease.
- `holmStep_bounded`: fixed-point Holm adjusted values remain capped.
- `correctedBootstrapPValue_positive`: the plus-one counts are strictly positive.
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

/-- The only projection consumed by trading rank. -/
def rankProjection (value : TradingEntry × ForecastReport) : TradingEntry :=
  value.1

/-- Forecast reporting is rank-isolated by construction in the formal model. -/
theorem attachForecast_rankProjection (entry : TradingEntry) (report : ForecastReport) :
    rankProjection (attachForecast entry report) = entry := by
  rfl

/-- One ordered Holm step on fixed-point values, matching `max(prior, candidate).min(cap)`. -/
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
