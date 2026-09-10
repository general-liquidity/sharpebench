//! # sharpebench-stats — deterministic backtest-honesty statistics
//!
//! The pure statistics core extracted from `sharpebench-core`: moment and normal
//! primitives ([`stats`]), the deflated / probabilistic Sharpe family
//! ([`deflated_sharpe`]), the data-snooping bootstrap family — White's Reality
//! Check, Hansen's SPA, and Romano-Wolf step-down ([`significance`]) — and
//! selection-axis luck control ([`selection`]). Three estimators the gate does
//! not use, an autocorrelation-aware PSR, the PSR with its standard error
//! evaluated under the null, and the manipulation-proof performance measure,
//! are opt-in diagnostics in [`opt_in_diagnostics`].
//!
//! Design invariants carried over verbatim from the original modules:
//! - **Pure.** No I/O, no system clock, no ambient randomness. Any randomness
//!   (the significance bootstrap) takes an explicit seed argument.
//! - **Deterministic.** Plain `f64` math, fixed reduction order, no parallel
//!   float sums. The enclosing benchmark pins two committed golden fields on
//!   Linux, macOS, and Windows; that check does not cover every input or host.
//! - **No `unsafe`.**
//!
//! ## Example: is this Sharpe real?
//!
//! ```
//! use sharpebench_stats::{
//!     deflated_sharpe_ratio, per_period_from_annualized, probabilistic_sharpe_ratio,
//!     sharpe_ratio,
//! };
//!
//! // A per-period (NOT annualized) excess-return series of daily bars.
//! let returns = [0.012, -0.004, 0.009, 0.011, -0.002, 0.008, 0.010, -0.001];
//!
//! let sr = sharpe_ratio(&returns); // observed, per-period
//! // One minus the one-sided p-value of H0: SR <= 0. Not the probability that
//! // the true Sharpe is positive: that is a posterior, and needs a prior.
//! let psr = probabilistic_sharpe_ratio(&returns, 0.0);
//! // The cross-trial Sharpe dispersion is per period, like every Sharpe here. An
//! // annualized dispersion of 0.5 on daily bars is 0.5 / sqrt(252), about 0.0315
//! // per period; 0.5 passed per period would be about 7.9 annualized.
//! let trials_sr_std = per_period_from_annualized(0.5, 252.0);
//! assert_eq!(trials_sr_std, 0.5 / 252f64.sqrt());
//! // Deflate for the 200 strategies tried. `Err` when an input is not one the
//! // estimator accepts: a non-finite return or a `trials_sr_std` that is not a
//! // dispersion has no deflated Sharpe. The result is one minus the p-value of
//! // the test whose null is that this Sharpe is the best of 200 zero-skill
//! // trials, not the probability that the strategy is skilled.
//! let dsr = deflated_sharpe_ratio(&returns, 200, trials_sr_std).unwrap();
//!
//! assert!(sr > 0.0);
//! assert!((0.0..=1.0).contains(&psr));
//! assert!((0.0..=1.0).contains(&dsr));
//! assert!(dsr <= psr); // deflating for the search never raises the statistic
//! ```
#![forbid(unsafe_code)]

pub mod agreement;
pub mod deflated_sharpe;
pub mod dissent;
pub mod fdr;
pub mod opt_in_diagnostics;
pub mod paired_randomization;
pub mod selection;
pub mod significance;
pub mod stats;
pub mod stylized_facts;
pub mod validation;

pub use validation::StatisticalError;

pub use agreement::{
    binarize, cohens_kappa, cohens_kappa_binary, gate_vs_human, spearman_rho, GateAgreement,
};
pub use deflated_sharpe::{
    deflated_sharpe_ratio, expected_max_sharpe, per_period_from_annualized,
    probabilistic_sharpe_ratio, sharpe_ratio,
};
pub use dissent::{
    dissent, dissent_across, kendall_tau_b, DissentReport, DissentThresholds, DissentVerdict,
    DEFAULT_MAX_LEVEL_DISSENT, DEFAULT_MAX_RANK_DISSENT,
};
pub use fdr::{benjamini_hochberg, fdr_verdict, FdrVerdict};
pub use opt_in_diagnostics::{
    first_order_autocorrelation, manipulation_proof_performance,
    probabilistic_sharpe_ratio_autocorrelated, sharpe_standard_error_autocorrelated,
    sharpe_variance_factor, StandardErrorAt, DEFAULT_MPPM_RISK_AVERSION,
};
pub use selection::{
    percentile_selection, selection_robustness, CandidateUtility, PercentileSelection,
    SelectionRobustness, Utility, DEFAULT_SELECTION_ALPHA, MIN_RECOMMENDED_SELECTION_ALPHA,
};
pub use stylized_facts::{
    stylized_facts, validate_dataset, validate_dataset_with, RealismFailure, RealismThresholds,
    RealismVerdict, StylizedFactsReport,
};
