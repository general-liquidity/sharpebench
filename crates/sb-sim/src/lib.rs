//! sb-sim — point-in-time backtest engine (Phase 1, not yet implemented).
//!
//! Will provide: a point-in-time data store that exposes only rows with
//! `t <= decision_time` (look-ahead **unrepresentable** by construction rather
//! than policed after the fact), cost / slippage / own-order-market-impact
//! models, and multi-window OOS + walk-forward + regime tagging. The engine is
//! deterministic given `(data, seed, decisions)`. Tracked in docs/PLAN.md §6.
#![forbid(unsafe_code)]
