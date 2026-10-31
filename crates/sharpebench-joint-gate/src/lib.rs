#![forbid(unsafe_code)]

//! Producer for the joint-gate validation study.
//!
//! The development calibration in `paper/src/joint_gate_power.py` models the
//! statistical legs against fixed bars. This crate runs what that calculation
//! leaves out: the shipped `rank` on whole declared fields, so the host-N floor,
//! measured dispersion, clone collapse, vote fence, seed averaging, the real
//! bootstrap draw and the integrity legs are all inside the measurement.
//!
//! - [`campaign`] is the closed document of rules, generators and blocks.
//! - [`dgp`] draws zero-skill paths from keyed streams.
//! - [`fields`] scores one cell and reads its configuration back.
//! - [`controls`] runs the integrity legs.
//! - [`shards`] holds the work plan, resume and the output directory.
//! - [`report`] states the predeclared reading.
//! - [`gate`] decides whether a run may call itself frozen.
//!
//! Protocol: `docs/studies/joint-gate/PROTOCOL.md`.

pub mod campaign;
pub mod controls;
pub mod dgp;
pub mod fields;
pub mod gate;
pub mod report;
pub mod shards;
