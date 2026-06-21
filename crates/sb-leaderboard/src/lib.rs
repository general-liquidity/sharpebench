//! sb-leaderboard — leaderboard storage + rendering (Phase 2, not yet implemented).
//!
//! Persists scored submissions, renders the public board, and maintains a signed
//! result chain (HMAC, the same tamper-evident pattern Gordon uses for its audit
//! log) so every published rank is independently verifiable. Tracked in
//! docs/PLAN.md §8.
#![forbid(unsafe_code)]
