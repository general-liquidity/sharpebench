//! Declared support contracts shared by the paper's evidence producers.
//!
//! A producer's ordinary output filename is read as a claim that the requested
//! evidence support was evaluated. Neither an exit code of zero nor that
//! filename establishes it on its own: an optional selector that matches
//! nothing, or a dataset that fails to load, otherwise skips every cell and
//! still reaches the normal publication path. These checks run before the
//! output is opened, and again before it is published, so a selector that
//! names nothing is a typed refusal rather than a silent empty field.
//!
//! Structural support, not correctness: this establishes that the declared
//! cells were evaluated, never that their scores are right.

#![allow(dead_code)] // each producer uses the subset its own contract needs.

use std::fmt;

/// A producer cannot establish the support its output would claim.
#[derive(Debug, PartialEq)]
pub enum SupportError {
    /// A selector value outside the producer's declared set.
    UnknownSelector {
        kind: &'static str,
        requested: String,
        declared: Vec<String>,
    },
    /// Planned support the run never evaluated.
    MissingSupport {
        kind: &'static str,
        missing: Vec<String>,
    },
    /// Fewer cells than the declared grid has.
    IncompleteGrid { expected: usize, produced: usize },
}

impl fmt::Display for SupportError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SupportError::UnknownSelector {
                kind,
                requested,
                declared,
            } => write!(
                f,
                "unknown {kind} selector {requested:?}; declared {kind}s: {}",
                declared.join(", ")
            ),
            SupportError::MissingSupport { kind, missing } => {
                write!(f, "no records for declared {kind}: {}", missing.join(", "))
            }
            SupportError::IncompleteGrid { expected, produced } => write!(
                f,
                "declared grid has {expected} cells, the run produced {produced}"
            ),
        }
    }
}

/// The single exit for an unsupported run: a non-zero status, and no artifact
/// under the requested name. Producers that stage through `<out>.partial`
/// leave the partial file behind, so the incompleteness stays visible on disk
/// under a name no consumer reads as published evidence.
pub fn refuse(what: impl fmt::Display) -> ! {
    eprintln!("refusing to publish: {what}");
    std::process::exit(1)
}

/// Unwrap a support check, refusing rather than continuing to the output.
pub fn or_refuse<T>(result: Result<T, SupportError>) -> T {
    match result {
        Ok(value) => value,
        Err(err) => refuse(err),
    }
}

/// Resolve an optional name selector against the declared set.
///
/// `None` plans every declared value. A value outside the set is refused
/// instead of silently planning nothing.
pub fn resolve_selector(
    kind: &'static str,
    declared: &[&str],
    requested: Option<&str>,
) -> Result<Vec<String>, SupportError> {
    let Some(requested) = requested else {
        return Ok(declared.iter().map(|d| (*d).to_string()).collect());
    };
    if declared.contains(&requested) {
        return Ok(vec![requested.to_string()]);
    }
    Err(SupportError::UnknownSelector {
        kind,
        requested: requested.to_string(),
        declared: declared.iter().map(|d| (*d).to_string()).collect(),
    })
}

/// Resolve an optional numeric selector against the declared axis.
///
/// Compared with the same tolerance the sweep filters cells with, so a
/// selector that would match no cell is refused rather than planning nothing.
pub fn resolve_numeric_selector(
    kind: &'static str,
    declared: &[f64],
    requested: Option<f64>,
) -> Result<Vec<f64>, SupportError> {
    let Some(requested) = requested else {
        return Ok(declared.to_vec());
    };
    if let Some(&value) = declared.iter().find(|d| (**d - requested).abs() <= 1e-9) {
        return Ok(vec![value]);
    }
    Err(SupportError::UnknownSelector {
        kind,
        requested: format!("{requested}"),
        declared: declared.iter().map(|d| format!("{d}")).collect(),
    })
}

/// A constant that names one member of a declared set, checked before the run
/// rather than discovered as an absent section of the output.
pub fn require_declared_member(
    kind: &'static str,
    declared: &[&str],
    value: &str,
) -> Result<(), SupportError> {
    if declared.contains(&value) {
        return Ok(());
    }
    Err(SupportError::UnknownSelector {
        kind,
        requested: value.to_string(),
        declared: declared.iter().map(|d| (*d).to_string()).collect(),
    })
}

/// Every planned value must have contributed records.
pub fn require_evaluated(
    kind: &'static str,
    planned: &[String],
    evaluated: &[String],
) -> Result<(), SupportError> {
    let missing: Vec<String> = planned
        .iter()
        .filter(|p| !evaluated.contains(p))
        .cloned()
        .collect();
    if missing.is_empty() {
        return Ok(());
    }
    Err(SupportError::MissingSupport { kind, missing })
}

/// The produced record count must be the declared grid, not merely non-zero.
pub fn require_grid(expected: usize, produced: usize) -> Result<(), SupportError> {
    if expected == produced {
        return Ok(());
    }
    Err(SupportError::IncompleteGrid { expected, produced })
}
