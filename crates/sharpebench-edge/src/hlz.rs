//! The Harvey-Liu-Zhu factor gate: `|t| >= 3.0`.
//!
//! Harvey, Liu & Zhu, *... and the Cross-Section of Expected Returns* (Review of
//! Financial Studies, 2016) argue that because hundreds of "factors" have been
//! data-mined from the same returns, the conventional `t >= 2.0` significance bar
//! is far too easy to clear by luck. Adjusting for that multiple-testing history,
//! they conclude a newly-claimed factor needs roughly `|t| >= 3.0` before it
//! should be believed.
//!
//! This gate encodes that bar as a hard threshold that complements the deflated
//! Sharpe scorer: where the deflated Sharpe prices in the *size* of the search,
//! the HLZ gate is a blunt, interpretable floor on the raw t-statistic of the
//! discovered edge. Pure and deterministic.

use serde::{Deserialize, Serialize};

/// The Harvey-Liu-Zhu (2016) recommended minimum t-statistic for a newly
/// discovered factor. The classic `t >= 2.0` bar ignores the multiple-testing
/// history of factor research; HLZ raise it to ~3.0.
pub const HLZ_DEFAULT_T_THRESHOLD: f64 = 3.0;

/// The HLZ factor gate. Rejects a candidate factor unless the absolute value of
/// its t-statistic clears `t_threshold` (default [`HLZ_DEFAULT_T_THRESHOLD`]).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct HarveyLiuZhu {
    /// Minimum `|t|` a factor must reach to pass. Configurable; default 3.0.
    pub t_threshold: f64,
}

impl Default for HarveyLiuZhu {
    fn default() -> Self {
        Self {
            t_threshold: HLZ_DEFAULT_T_THRESHOLD,
        }
    }
}

/// The outcome of applying a [`HarveyLiuZhu`] gate to one factor.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct HlzGate {
    /// The factor's t-statistic as supplied by the caller.
    pub t_stat: f64,
    /// The threshold applied.
    pub t_threshold: f64,
    /// `true` when `|t_stat| >= t_threshold` — the factor clears the HLZ bar.
    pub passed: bool,
    /// One plain-English sentence citing the gate.
    pub explanation: String,
}

impl HarveyLiuZhu {
    /// Build a gate with an explicit threshold.
    pub fn new(t_threshold: f64) -> Self {
        Self { t_threshold }
    }

    /// Evaluate a factor by its t-statistic. Passes when `|t_stat| >= t_threshold`
    /// (the boundary value passes). NaN never passes.
    pub fn evaluate(&self, t_stat: f64) -> HlzGate {
        let passed = t_stat.abs() >= self.t_threshold;
        let explanation = if passed {
            format!(
                "PASS: |t| {:.2} clears the Harvey-Liu-Zhu (2016) {:.2} factor bar.",
                t_stat.abs(),
                self.t_threshold
            )
        } else {
            format!(
                "FAIL: |t| {:.2} is below the Harvey-Liu-Zhu (2016) {:.2} factor bar; likely a multiple-testing artifact.",
                t_stat.abs(),
                self.t_threshold
            )
        };
        HlzGate {
            t_stat,
            t_threshold: self.t_threshold,
            passed,
            explanation,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn passes_above_default_bar() {
        let g = HarveyLiuZhu::default().evaluate(3.5);
        assert!(g.passed);
        assert!(g.explanation.contains("Harvey-Liu-Zhu"));
    }

    #[test]
    fn fails_below_default_bar() {
        let g = HarveyLiuZhu::default().evaluate(2.0);
        assert!(!g.passed);
    }

    #[test]
    fn boundary_exactly_three_passes() {
        assert!(HarveyLiuZhu::default().evaluate(3.0).passed);
    }

    #[test]
    fn negative_t_uses_absolute_value() {
        // A short factor with t = -3.5 is just as significant as +3.5.
        assert!(HarveyLiuZhu::default().evaluate(-3.5).passed);
        assert!(!HarveyLiuZhu::default().evaluate(-2.0).passed);
    }

    #[test]
    fn threshold_is_configurable() {
        let lenient = HarveyLiuZhu::new(2.0);
        assert!(lenient.evaluate(2.5).passed);
        let strict = HarveyLiuZhu::new(4.0);
        assert!(!strict.evaluate(3.5).passed);
    }

    #[test]
    fn nan_never_passes() {
        assert!(!HarveyLiuZhu::default().evaluate(f64::NAN).passed);
    }
}
