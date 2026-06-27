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
#![forbid(unsafe_code)]

pub mod deflated_sharpe;
pub mod selection;
pub mod significance;
pub mod stats;

pub use selection::{selection_robustness, SelectionRobustness};
