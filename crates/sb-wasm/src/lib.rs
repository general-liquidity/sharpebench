//! WASM façade over [`sb_core`] — the bridge that lets Gordon (TypeScript/Bun)
//! consume the **identical** scoring kernel as the public harness, so the
//! internal eval and the published benchmark can never drift.
//!
//! Phase 0 re-exports the kernel; the `wasm-bindgen` surface (a JSON-in/JSON-out
//! `score(...)` export compiled to `wasm32-unknown-unknown`) lands alongside the
//! first Gordon integration. Tracked in docs/PLAN.md §4.
#![forbid(unsafe_code)]

pub use sb_core;
