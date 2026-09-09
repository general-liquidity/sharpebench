//! Checked statistical boundaries. Failure to compute is not a p-value.

use std::fmt;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StatisticalError {
    NonFiniteObservation {
        index: usize,
    },
    NonFiniteFieldObservation {
        row: usize,
        index: usize,
    },
    InvalidProbability {
        index: usize,
    },
    InvalidParameter {
        name: &'static str,
        requirement: &'static str,
    },
    InsufficientObservations {
        required: usize,
        actual: usize,
    },
    NonFiniteComputation {
        quantity: &'static str,
    },
}

impl fmt::Display for StatisticalError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NonFiniteObservation { index } => write!(f, "observation {index} must be finite"),
            Self::NonFiniteFieldObservation { row, index } => {
                write!(f, "field row {row} observation {index} must be finite")
            }
            Self::InvalidProbability { index } => {
                write!(f, "p-value {index} must be finite and in [0, 1]")
            }
            Self::InvalidParameter { name, requirement } => write!(f, "{name} {requirement}"),
            Self::InsufficientObservations { required, actual } => write!(
                f,
                "at least {required} observations are required, got {actual}"
            ),
            Self::NonFiniteComputation { quantity } => write!(f, "{quantity} is not finite"),
        }
    }
}

impl std::error::Error for StatisticalError {}

pub(crate) fn finite_computation(
    value: f64,
    quantity: &'static str,
) -> Result<f64, StatisticalError> {
    if value.is_finite() {
        Ok(value)
    } else {
        Err(StatisticalError::NonFiniteComputation { quantity })
    }
}

pub fn finite_observations(values: &[f64]) -> Result<(), StatisticalError> {
    match values.iter().position(|x| !x.is_finite()) {
        Some(index) => Err(StatisticalError::NonFiniteObservation { index }),
        None => Ok(()),
    }
}

pub fn probability(value: f64, name: &'static str) -> Result<(), StatisticalError> {
    if !value.is_finite() || !(0.0..=1.0).contains(&value) {
        return Err(StatisticalError::InvalidParameter {
            name,
            requirement: "must be finite and in [0, 1]",
        });
    }
    Ok(())
}

pub fn bootstrap_inputs(
    values: &[f64],
    n_boot: usize,
    block_prob: f64,
) -> Result<(), StatisticalError> {
    finite_observations(values)?;
    if values.len() < 2 {
        return Err(StatisticalError::InsufficientObservations {
            required: 2,
            actual: values.len(),
        });
    }
    if n_boot == 0 {
        return Err(StatisticalError::InvalidParameter {
            name: "n_boot",
            requirement: "must be positive",
        });
    }
    block_probability(block_prob)
}

pub fn fdr_inputs(values: &[f64], q: f64) -> Result<(), StatisticalError> {
    probability(q, "q")?;
    if let Some(index) = values
        .iter()
        .position(|p| !p.is_finite() || !(0.0..=1.0).contains(p))
    {
        return Err(StatisticalError::InvalidProbability { index });
    }
    Ok(())
}

/// A parameter that must be a real number: no NaN, no infinity.
pub fn finite_parameter(value: f64, name: &'static str) -> Result<(), StatisticalError> {
    if !value.is_finite() {
        return Err(StatisticalError::InvalidParameter {
            name,
            requirement: "must be finite",
        });
    }
    Ok(())
}

/// A dispersion (a standard deviation, a spread): finite and not negative. A
/// negative dispersion is not a small one, it is a malformed input, and folding
/// it into the "no search to deflate for" branch hands the caller the most
/// favorable answer available.
pub fn dispersion(value: f64, name: &'static str) -> Result<(), StatisticalError> {
    if !value.is_finite() || value < 0.0 {
        return Err(StatisticalError::InvalidParameter {
            name,
            requirement: "must be finite and non-negative",
        });
    }
    Ok(())
}

/// The per-step block-restart probability shared by every stationary-bootstrap
/// entry point. `0.0` never restarts a block, which is a degenerate resampler,
/// not a conservative one.
pub fn block_probability(block_prob: f64) -> Result<(), StatisticalError> {
    if !block_prob.is_finite() || block_prob <= 0.0 || block_prob > 1.0 {
        return Err(StatisticalError::InvalidParameter {
            name: "block_prob",
            requirement: "must be finite and in (0, 1]",
        });
    }
    Ok(())
}

/// Shared boundary for the field-wide data-snooping tests. Returns the common
/// length the tests truncate every row to, so the caller does not recompute it.
pub fn field_inputs(
    field: &[Vec<f64>],
    n_boot: usize,
    block_prob: f64,
) -> Result<usize, StatisticalError> {
    if field.is_empty() {
        return Err(StatisticalError::InvalidParameter {
            name: "field",
            requirement: "must not be empty",
        });
    }
    for (row, series) in field.iter().enumerate() {
        if let Some(index) = series.iter().position(|x| !x.is_finite()) {
            return Err(StatisticalError::NonFiniteFieldObservation { row, index });
        }
    }
    let n = field.iter().map(Vec::len).min().unwrap_or(0);
    if n < 2 {
        return Err(StatisticalError::InsufficientObservations {
            required: 2,
            actual: n,
        });
    }
    if n_boot == 0 {
        return Err(StatisticalError::InvalidParameter {
            name: "n_boot",
            requirement: "must be positive",
        });
    }
    block_probability(block_prob)?;
    Ok(n)
}
