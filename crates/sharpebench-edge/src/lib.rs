//! # sharpebench-edge — the backtest-honesty verdict layer
//!
//! "Is my Sharpe real, or an artifact of luck and multiple testing?" This crate
//! is the headline verdict over [`sharpebench_stats`]: it composes the deflated /
//! probabilistic Sharpe family, the minimum track record length (MinTRL), the
//! data-snooping bootstrap family (Reality Check / SPA / step-down), and the CSCV
//! Probability of Backtest Overfitting (PBO) into a single [`Verdict`].
//!
//! This is the embedded form of what was specced as "EdgeLint" — now a
//! SharpeBench crate, not a separate product.
//!
//! ## Two tiers
//!
//! - [`is_my_sharpe_real`] (LITE) — one return series in, a [`HonestyVerdict`]
//!   out (Sharpe, PSR, deflated Sharpe, haircut, MinTRL, plain-English verdict).
//! - [`is_my_sharpe_real_full`] (FULL) — a whole field of candidate strategies
//!   in, a [`FullVerdict`] out (the winner's LITE verdict plus the multiple-
//!   testing family + PBO).
//!
//! ## Design invariants
//!
//! Inherited from `sharpebench-stats`: pure (no I/O, no clock, no ambient
//! randomness — the bootstrap takes an explicit fixed seed), deterministic (plain
//! `f64`, fixed reduction order, no parallel sums), and no `unsafe`. All math is
//! ported from the published papers (Bailey, Borwein, López de Prado, Zhu;
//! White; Hansen; Romano-Wolf), never from any GPL/proprietary library.
#![forbid(unsafe_code)]

pub mod hlz;
pub mod mintrl;
pub mod pbo;
pub mod verdict;

pub use hlz::{HarveyLiuZhu, HlzGate, HLZ_DEFAULT_T_THRESHOLD};
pub use mintrl::min_track_record_length;
pub use pbo::probability_of_backtest_overfitting;
pub use verdict::{
    is_my_sharpe_real, is_my_sharpe_real_full, FullVerdict, HonestyConfig, HonestyVerdict, Verdict,
    METHODOLOGY_VERSION,
};
