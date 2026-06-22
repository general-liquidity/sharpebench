//! Confidence calibration — does the agent's *stated* conviction predict its
//! outcomes? An agent that knows when it doesn't know is more trustworthy with
//! capital than one with a marginally higher Sharpe and no self-knowledge. We
//! score this with the Brier score (lower is better; 0 = perfect, 0.25 = the
//! always-0.5 baseline).

/// Brier score between per-decision confidences in [0, 1] and realized binary
/// outcomes (`true` = the call was right). Pairs are matched by index; extra
/// entries on either side are ignored. Returns 0.0 if there are no pairs.
pub fn brier_score(confidences: &[f64], outcomes: &[bool]) -> f64 {
    let n = confidences.len().min(outcomes.len());
    if n == 0 {
        return 0.0;
    }
    let mut sum = 0.0;
    for (&c, &o) in confidences.iter().zip(outcomes.iter()) {
        let outcome = if o { 1.0 } else { 0.0 };
        let conf = c.clamp(0.0, 1.0);
        sum += (conf - outcome) * (conf - outcome);
    }
    sum / n as f64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn perfect_calibration_is_zero() {
        let c = [1.0, 0.0, 1.0, 0.0];
        let o = [true, false, true, false];
        assert!(brier_score(&c, &o) < 1e-12);
    }

    #[test]
    fn always_half_is_quarter() {
        let c = [0.5; 4];
        let o = [true, false, true, false];
        assert!((brier_score(&c, &o) - 0.25).abs() < 1e-12);
    }
}
