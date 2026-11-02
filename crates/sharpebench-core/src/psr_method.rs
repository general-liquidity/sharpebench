//! Where the scorer evaluates the standard error of the Sharpe estimator when it
//! turns a Sharpe into a PSR, and so into a deflated Sharpe: the opt-in
//! methodology field [`crate::ScoreConfig::psr_standard_error`].
//!
//! The default, [`PsrStandardError::Observed`], is the plug-in standard error
//! at the observed Sharpe of Bailey and López de Prado (2012, 2014), López de
//! Prado, Lipton and Zoonekynd (2026), eq. 3 (p. 9). It is the estimand the
//! joint-gate protocol freezes, and a configuration that leaves it alone
//! serializes, digests and scores exactly as it did before the field existed.
//!
//! [`PsrStandardError::Benchmark`] evaluates the standard error under the null,
//! at the benchmark Sharpe: the same paper's eqs. 4 and 5 (p. 10), the
//! statistic the `null-se-psr` diagnostic reports. Its denominator does not
//! depend on the observed Sharpe, so for fixed moments the PSR is
//! non-decreasing in the observed Sharpe, where the plug-in PSR falls with it
//! once `g3 SR > 2` (`crates/sharpebench-stats/tests/dsr_psr_properties.rs`). At
//! a zero benchmark it is `Phi(SR sqrt(n - 1))` and takes no credit from the
//! sample skewness or kurtosis. One cost: a negatively skewed track loses the
//! penalty the plug-in bracket gives it at a zero benchmark. Tail risk is
//! reported by the `expected-shortfall` diagnostic and bounded by the mandate.
//!
//! When opted in, the evaluation applies to every PSR and deflated Sharpe the
//! board reads: the pooled `psr`, the deflated Sharpe and its bootstrap
//! interval (and so the tie bands), the per-run PSR of pass^k, and the
//! standard-error admission terms that guard the deflated Sharpe. Each row
//! records it as `psr_standard_error`.

use serde::{Deserialize, Serialize};
use sharpebench_stats::StandardErrorAt;

/// Where the PSR standard error is evaluated. See the module documentation.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PsrStandardError {
    /// At the observed Sharpe (eq. 3): the plug-in convention and the default.
    #[default]
    Observed,
    /// At the benchmark Sharpe, the least favorable point of the null (eqs. 4
    /// and 5). Opt-in.
    Benchmark,
}

impl PsrStandardError {
    /// Whether this is the default, which serializes as an absent field.
    pub fn is_default(&self) -> bool {
        *self == Self::default()
    }

    /// The statistics crate's name for the same choice.
    pub fn at(self) -> StandardErrorAt {
        match self {
            Self::Observed => StandardErrorAt::Observed,
            Self::Benchmark => StandardErrorAt::Benchmark,
        }
    }

    /// The command-line identifier: `observed` or `benchmark`.
    pub fn parse(id: &str) -> Result<Self, String> {
        match id {
            "observed" => Ok(Self::Observed),
            "benchmark" => Ok(Self::Benchmark),
            other => Err(format!(
                "unknown PSR standard error `{other}`; expected `observed` (the default) or \
                 `benchmark`"
            )),
        }
    }
}
