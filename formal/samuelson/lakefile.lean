import Lake

open Lake DSL

/-- Samuelson's inequality and the near-constant refusal it justifies. This is a separate package
because it needs Mathlib's real numbers, sums and square roots, while the forecast model does not:
keeping Mathlib out of `formal/lakefile.lean` keeps that build to seconds, and only this package
pays for the Mathlib cache. -/
package SharpeBenchSamuelson where
  version := v!"0.1.0"
  leanOptions := #[
    ⟨`autoImplicit, false⟩,
    ⟨`relaxedAutoImplicit, false⟩,
    ⟨`warningAsError, true⟩
  ]

require mathlib from git "https://github.com/leanprover-community/mathlib4" @ "v4.33.1"

@[default_target]
lean_lib SharpeBenchSamuelson
