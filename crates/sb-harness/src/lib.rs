//! sb-harness — run orchestration (Phase 1, not yet implemented).
//!
//! Drives an external agent (via [`sb_protocol`]) through [`sb_sim`] across every
//! seed × window, captures each run's return series and decision trace into the
//! [`sb_core`] submission format, and hands the field to the scoring kernel.
//! Tracked in docs/PLAN.md §9.
#![forbid(unsafe_code)]
