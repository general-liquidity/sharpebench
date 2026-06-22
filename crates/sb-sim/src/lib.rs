//! sb-sim — point-in-time market simulator + reference agents (Phase 1).
//!
//! The engine feeds an [`Agent`] a [`sb_protocol::MarketObservation`] that only
//! ever contains data at or before the decision date (look-ahead is structurally
//! impossible — [`Dataset`] never hands out a future bar), applies the resulting
//! orders with transaction costs and seeded execution slippage, and emits an
//! [`sb_core::Run`] (per-period returns + decision trace) ready for scoring.
#![forbid(unsafe_code)]

pub mod agent;
pub mod costs;
pub mod data;
pub mod engine;
pub mod external;
pub mod windows;

pub use agent::{Agent, BuyAndHold, HoldAgent, Momentum, RandomAgent, TeamAgent};
pub use costs::CostModel;
pub use data::Dataset;
pub use engine::{run_backtest, Window};
pub use external::{ExternalAgent, HttpAgent};
pub use windows::{tag_regime, walk_forward, Regime};
