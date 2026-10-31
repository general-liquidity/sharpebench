/-
Copyright (c) 2026 Tiberiu Toca. All rights reserved.
Released under Apache 2.0 license as described in the file LICENSE-APACHE.
Authors: Tiberiu Toca
-/
import Mathlib

/-!
# Samuelson's inequality and the near-constant refusal

`sign_is_forced_by_moments` in `crates/sharpebench-stats/src/deflated_sharpe.rs` (on the branch
`fix/near-constant-admission` at the time of writing) refuses a window when
`|mean| * sqrt n > s * (n - 1)`, with `s` the Bessel-corrected standard deviation, and its
documentation calls the threshold a theorem. This module proves that theorem over the reals.

## Main results

- `samuelson_sq`: Samuelson's inequality (1968), `n (x_j - mean)^2 ≤ (n - 1) Σ (x_i - mean)^2`.
- `pos_mul_mean_of_signForced`: when the predicate holds, every observation is strictly on the
  mean's side of zero, so the refusal never refuses a window that has a loss in it.

The module also gives a witness that the predicate can hold and a wrong threshold (a t-statistic
above one) with an explicit window that satisfies it and still contains a loss.

## Scope

Covers: the predicate of `sign_is_forced_by_moments`, as a real-valued statement.

Assumes: exact real arithmetic. The Rust function evaluates the same inequality in `f64`, so a
window within rounding of the threshold can be classified differently from the real-valued
predicate. That rounding is outside this proof. Nothing links this module to the Rust function
mechanically; the boundary tests in `crates/sharpebench-core/tests/near_constant_admission.rs` on
that branch exercise the implementation.
-/

open Finset

namespace SharpeBenchSamuelson

/-- The sample mean of `n` observations. -/
noncomputable def mean {n : ℕ} (x : Fin n → ℝ) : ℝ := (∑ i, x i) / n

/-- The sum of squared deviations from the sample mean. -/
noncomputable def sumSq {n : ℕ} (x : Fin n → ℝ) : ℝ := ∑ i, (x i - mean x) ^ 2

/-- The Bessel-corrected sample standard deviation, as `std_dev` in `stats.rs` computes it. -/
noncomputable def stdDev {n : ℕ} (x : Fin n → ℝ) : ℝ := Real.sqrt (sumSq x / (n - 1))

theorem sum_sub_mean {n : ℕ} (hn : 0 < n) (x : Fin n → ℝ) : ∑ i, (x i - mean x) = 0 := by
  have hn' : (n : ℝ) ≠ 0 := by positivity
  rw [Finset.sum_sub_distrib, Finset.sum_const, Finset.card_univ, Fintype.card_fin, nsmul_eq_mul,
    mean]
  field_simp
  ring

/-- Samuelson's inequality (1968), squared: `n (x_j - mean)^2 ≤ (n - 1) Σ (x_i - mean)^2`. -/
theorem samuelson_sq {n : ℕ} (hn : 0 < n) (x : Fin n → ℝ) (j : Fin n) :
    n * (x j - mean x) ^ 2 ≤ (n - 1) * sumSq x := by
  set d : Fin n → ℝ := fun i ↦ x i - mean x with hd
  have hsum : ∑ i, d i = 0 := sum_sub_mean hn x
  have hrest : ∑ i ∈ univ.erase j, d i = - d j := by
    have := Finset.add_sum_erase univ d (mem_univ j)
    linarith
  have hcs := Finset.sum_mul_sq_le_sq_mul_sq (univ.erase j) (fun _ ↦ (1 : ℝ)) d
  simp only [one_mul, one_pow, sum_const, card_erase_of_mem (mem_univ j), card_univ,
    Fintype.card_fin, nsmul_eq_mul, mul_one, hrest] at hcs
  have hsq : sumSq x = d j ^ 2 + ∑ i ∈ univ.erase j, d i ^ 2 :=
    (Finset.add_sum_erase univ (fun i ↦ d i ^ 2) (mem_univ j)).symm
  have hcast : ((n - 1 : ℕ) : ℝ) = (n : ℝ) - 1 := by
    rw [Nat.cast_sub (by omega)]; simp
  rw [hcast] at hcs
  rw [hsq]
  nlinarith [hcs]

/-- The predicate `sign_is_forced_by_moments` evaluates, over the reals: for at least two
observations, `|mean| * sqrt n > s * (n - 1)` with `s` the Bessel-corrected standard deviation. -/
def SignForced {n : ℕ} (x : Fin n → ℝ) : Prop :=
  2 ≤ n ∧ |mean x| * Real.sqrt n > stdDev x * (n - 1)

/-- The refusal is sound: when the predicate holds, every observation is strictly on the side of
zero the mean is on, so no observation is a loss (nor a gain, for a negative mean) and none is
zero. -/
theorem pos_mul_mean_of_signForced {n : ℕ} {x : Fin n → ℝ} (h : SignForced x) (j : Fin n) :
    0 < x j * mean x := by
  obtain ⟨hn, hgt⟩ := h
  have hn0 : 0 < n := by omega
  have hnR : (2 : ℝ) ≤ n := by exact_mod_cast hn
  have hS : 0 ≤ sumSq x := Finset.sum_nonneg fun i _ ↦ sq_nonneg _
  -- `s * (n - 1) = sqrt (S * (n - 1))`, so the predicate compares two square roots.
  have hstd : stdDev x * (n - 1) = Real.sqrt (sumSq x * (n - 1)) := by
    have hpos : (0 : ℝ) < n - 1 := by linarith
    rw [show sumSq x * (n - 1) = sumSq x / (n - 1) * (n - 1) ^ 2 by field_simp,
      Real.sqrt_mul (div_nonneg hS hpos.le), Real.sqrt_sq hpos.le, stdDev]
  have hdev : |x j - mean x| * Real.sqrt n ≤ stdDev x * (n - 1) := by
    rw [hstd, ← Real.sqrt_sq (abs_nonneg _), ← Real.sqrt_mul (sq_nonneg _), sq_abs]
    apply Real.sqrt_le_sqrt
    nlinarith [samuelson_sq hn0 x j]
  have hsqrt : 0 < Real.sqrt n := Real.sqrt_pos.mpr (by positivity)
  have hlt : |x j - mean x| < |mean x| := by
    by_contra hcon
    push Not at hcon
    nlinarith [mul_le_mul_of_nonneg_right hcon hsqrt.le]
  have hm : 0 < |mean x| := lt_of_le_of_lt (abs_nonneg _) hlt
  have key : x j * mean x = mean x ^ 2 + (x j - mean x) * mean x := by ring
  have hbound : -(|x j - mean x| * |mean x|) ≤ (x j - mean x) * mean x := by
    rw [← abs_mul]; exact neg_abs_le _
  rw [key, ← sq_abs (mean x)]
  nlinarith [mul_lt_mul_of_pos_right hlt hm]

/-- The hypothesis can be met: two equal observations have no dispersion and a nonzero mean. -/
example : SignForced ![(1 : ℝ), 1] := by
  refine ⟨le_rfl, ?_⟩
  have hm : mean ![(1 : ℝ), 1] = 1 := by simp [mean, Fin.sum_univ_two]
  have hs : stdDev ![(1 : ℝ), 1] = 0 := by simp [stdDev, sumSq, hm, Fin.sum_univ_two]
  rw [hm, hs]
  norm_num

/-- A plausible wrong threshold: a t-statistic above one, `n * mean^2 > s^2`, in place of
Samuelson's `n * mean^2 > s^2 * (n - 1)^2`. -/
def SignForcedByTStat {n : ℕ} (x : Fin n → ℝ) : Prop :=
  2 ≤ n ∧ n * mean x ^ 2 > sumSq x / (n - 1)

/-- The wrong threshold admits a track with a loss: `-1, 3, 3, 3` has mean 2 and `s = 2`, so its
t-statistic is 2, yet its first bar has the opposite sign to the mean. The theorem above is
therefore not true of every threshold; it depends on the one `sign_is_forced_by_moments` uses. -/
example : SignForcedByTStat ![(-1 : ℝ), 3, 3, 3] ∧ ¬ 0 < ![(-1 : ℝ), 3, 3, 3] 0 *
    mean ![(-1 : ℝ), 3, 3, 3] := by
  have hm : mean ![(-1 : ℝ), 3, 3, 3] = 2 := by simp [mean, Fin.sum_univ_four]; norm_num
  refine ⟨⟨by norm_num, ?_⟩, ?_⟩
  · simp [sumSq, hm, Fin.sum_univ_four]; norm_num
  · rw [hm]; norm_num

end SharpeBenchSamuelson
