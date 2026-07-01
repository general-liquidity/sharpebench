//! # sharpebench-stats — deterministic backtest-honesty statistics
//!
//! The pure statistics core extracted from `sharpebench-core`: moment and normal
//! primitives ([`stats`]), the deflated / probabilistic Sharpe family
//! ([`deflated_sharpe`]), the data-snooping bootstrap family — White's Reality
//! Check, Hansen's SPA, and Romano-Wolf step-down ([`significance`]) — and
//! selection-axis luck control ([`selection`]).
//!
//! Design invariants carried over verbatim from the original modules:
//! - **Pure.** No I/O, no system clock, no ambient randomness. Any randomness
//!   (the significance bootstrap) takes an explicit seed argument.
//! - **Deterministic.** Plain `f64` math, fixed reduction order, no parallel
//!   float sums. The same input yields byte-identical output on any platform.
//! - **No `unsafe`.**
//!
//! ## Example: is this Sharpe real?
//!
//! ```
//! use sharpebench_stats::{deflated_sharpe_ratio, probabilistic_sharpe_ratio, sharpe_ratio};
//!
//! // A per-period (NOT annualized) excess-return series.
//! let returns = [0.012, -0.004, 0.009, 0.011, -0.002, 0.008, 0.010, -0.001];
//!
//! let sr = sharpe_ratio(&returns); // observed, per-period
//! let psr = probabilistic_sharpe_ratio(&returns, 0.0); // P(true Sharpe > 0)
//! // Deflate for the 200 strategies tried, with ~0.5 cross-trial Sharpe dispersion:
//! let dsr = deflated_sharpe_ratio(&returns, 200, 0.5); // P(skill survives the search)
//!
//! assert!(sr > 0.0);
//! assert!((0.0..=1.0).contains(&psr));
//! assert!((0.0..=1.0).contains(&dsr));
//! assert!(dsr <= psr); // deflating for the search never raises the probability
//! ```
#![forbid(unsafe_code)]

pub mod deflated_sharpe;
pub mod fdr;
pub mod selection;
pub mod significance;
pub mod stats;
pub mod stylized_facts;

pub use deflated_sharpe::{
    deflated_sharpe_ratio, expected_max_sharpe, probabilistic_sharpe_ratio, sharpe_ratio,
};
pub use fdr::{benjamini_hochberg, fdr_verdict, FdrVerdict};
pub use selection::{selection_robustness, SelectionRobustness};
pub use stylized_facts::{
    stylized_facts, validate_dataset, validate_dataset_with, RealismFailure, RealismThresholds,
    RealismVerdict, StylizedFactsReport,
};
