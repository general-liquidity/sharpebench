#![forbid(unsafe_code)]

//! The executable study-protocol contract.
//!
//! A study protocol is the document a study writes before it runs: what method
//! at what configuration, which claim, which named quantities, which design,
//! how many runs, under whose budget, stopping when. This crate is that
//! document's closed schema, its validator, and the reporting path that makes
//! a precision statement a function of the runs actually executed.
//!
//! # What it is for
//!
//! Three failure modes motivated it, all of them cheap to prevent before an
//! expensive run and impossible to repair afterwards:
//!
//! - A study that never named its estimand reports whichever quantity the
//!   analysis happened to compute. Per-entry and whole-field false positives
//!   are different quantities, and a document that names neither can claim
//!   either after the fact.
//! - A study that mixes raw and benchmark-relative effect units reports a
//!   number that is not a measurement of anything.
//! - A study that plans a large simulation count, runs fewer, and keeps the
//!   planned precision in its write-up has published a precision it did not
//!   buy. [`precision`] makes that shape unrepresentable rather than
//!   discouraged; see its module docs for the three mechanisms.
//!
//! # The three run tiers
//!
//! [`protocol::RunTier`] is a field with consequences, not a label:
//!
//! | Tier | What it may claim |
//! |---|---|
//! | `ci_regression` | Pinned-output behaviour only. Declaring a claim on a rate or power estimand is refused: a fixture suite cannot establish a rare false-positive rate or population power. |
//! | `development_calibration` | Exploratory outputs with recorded threshold choices. Tuning is permitted. |
//! | `frozen_validation` | A fixed method on reserved seeds. Tuning is refused, the document must be marked frozen, and [`amend::check_amendment`] refuses an edit that does not raise the version. |
//!
//! # Using it
//!
//! ```
//! use sharpebench_study::{StudyProtocol, validate};
//!
//! let text = std::fs::read_to_string("examples/placeholder-protocol.json")?;
//! let protocol = StudyProtocol::from_json(&text)?;
//! validate(&protocol)?;
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```
//!
//! # Numeric targets are pending an owner decision
//!
//! `examples/placeholder-protocol.json` is a structurally valid protocol whose
//! numbers are placeholders. Target effects, error limits, power targets and
//! sample counts are decisions D3 and D8 in the product plan and belong to the
//! study owner. The validator checks that a document is coherent and
//! affordable; it does not and cannot supply the targets a study is run to
//! meet. Do not cite the placeholder numbers as this project's targets.

pub mod amend;
pub mod precision;
pub mod protocol;
pub mod refusal;
pub mod validate;

pub use amend::check_amendment;
pub use precision::{
    required_simulation_runs, wilson_interval, ClaimStatus, Interval, PrecisionClaim, StudyReport,
};
pub use protocol::{RunTier, StudyProtocol, STUDY_CONTRACT_VERSION};
pub use refusal::{PrecisionError, ProtocolRefusal, ReportRefusal};
pub use validate::validate;
