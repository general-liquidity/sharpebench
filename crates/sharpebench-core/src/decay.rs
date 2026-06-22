//! Edge decay — does the agent's signal survive forward in time, or is it a
//! one-regime fluke? We estimate the half-life of the absolute information
//! coefficient by regressing `ln|IC|` on time; a fast-decaying edge is penalized
//! by the composite even if its average looks good. (After QuantBench's IC
//! half-life.)

use crate::stats::mean;

/// Estimate the half-life (in periods) of an IC series. Returns `None` if the
/// series is too short or is *not* decaying (flat or improving) — in which case
/// there's nothing to penalize.
pub fn edge_half_life(ic_series: &[f64]) -> Option<f64> {
    // Use (t, ln|ic|) points where |ic| is meaningfully non-zero.
    let pts: Vec<(f64, f64)> = ic_series
        .iter()
        .enumerate()
        .filter_map(|(t, &ic)| {
            let a = ic.abs();
            if a > 1e-9 {
                Some((t as f64, a.ln()))
            } else {
                None
            }
        })
        .collect();
    if pts.len() < 3 {
        return None;
    }
    let xs: Vec<f64> = pts.iter().map(|p| p.0).collect();
    let ys: Vec<f64> = pts.iter().map(|p| p.1).collect();
    let mx = mean(&xs);
    let my = mean(&ys);
    let mut num = 0.0;
    let mut den = 0.0;
    for (&x, &y) in xs.iter().zip(ys.iter()) {
        num += (x - mx) * (y - my);
        den += (x - mx) * (x - mx);
    }
    if den == 0.0 {
        return None;
    }
    let slope = num / den;
    if slope >= 0.0 {
        return None; // not decaying
    }
    Some(std::f64::consts::LN_2 / -slope)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_exponential_decay() {
        // ic_t = 0.2 * exp(-0.1 t) → half-life ≈ ln2 / 0.1 ≈ 6.93
        let ic: Vec<f64> = (0..40).map(|t| 0.2 * (-0.1 * t as f64).exp()).collect();
        let hl = edge_half_life(&ic).expect("should decay");
        assert!((hl - 6.93).abs() < 0.5, "half-life={hl}");
    }

    #[test]
    fn flat_edge_has_no_decay() {
        let ic = vec![0.1; 30];
        assert!(edge_half_life(&ic).is_none());
    }
}
