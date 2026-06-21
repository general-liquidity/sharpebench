//! sb-attest — forward-attestation (Phase 2, not yet implemented).
//!
//! The un-gameable spine: an agent **pre-registers** a hash of its binary/config
//! before the evaluation window's data exists; submissions are time-locked to a
//! future window; and anyone can re-run a pinned submission against the revealed
//! data and reproduce the **signed** score. This is what makes a GL-hosted
//! benchmark independently trustworthy. Tracked in docs/PLAN.md §7.
#![forbid(unsafe_code)]
