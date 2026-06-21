//! Performance attribution — separate skill (alpha) from market beta.
//!
//! "Return rank = luck" taken one step further: how much of an agent's return is
//! its own decisions versus simply riding the field/market? We regress an agent's
//! returns on a market proxy (CAPM-style) and report the intercept (alpha — the
//! skill component) and slope (beta — market exposure). Computed field-relative in
//! [`crate::rank`], with the market proxy = the equal-weight average of all
//! submitted agents' returns. (After KTD-Fin's Barra-style attribution.)

use crate::stats::{mean, variance};

/// CAPM decomposition of `agent` returns against an aligned `market` series.
/// Returns `(alpha_per_period, beta)`: alpha is the agent's mean return net of its
/// beta-weighted market exposure — the part that isn't just market drift.
pub fn alpha_beta(agent: &[f64], market: &[f64]) -> (f64, f64) {
    let n = agent.len().min(market.len());
    if n < 2 {
        return (mean(agent), 0.0);
    }
    let a = &agent[..n];
    let m = &market[..n];
    let ma = mean(a);
    let mm = mean(m);
    let var_m = variance(m);
    if var_m == 0.0 {
        return (ma, 0.0);
    }
    let cov = a
        .iter()
        .zip(m.iter())
        .map(|(x, y)| (x - ma) * (y - mm))
        .sum::<f64>()
        / (n as f64 - 1.0);
    let beta = cov / var_m;
    let alpha = ma - beta * mm;
    (alpha, beta)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pure_market_follower_has_zero_alpha() {
        let market: Vec<f64> = (0..50).map(|i| 0.001 * (i as f64 * 0.3).sin()).collect();
        let agent: Vec<f64> = market.iter().map(|m| 1.5 * m).collect();
        let (alpha, beta) = alpha_beta(&agent, &market);
        assert!((beta - 1.5).abs() < 1e-9, "beta={beta}");
        assert!(alpha.abs() < 1e-9, "alpha={alpha}");
    }

    #[test]
    fn constant_excess_is_pure_alpha() {
        let market: Vec<f64> = (0..50).map(|i| 0.001 * (i as f64 * 0.3).sin()).collect();
        let agent: Vec<f64> = market.iter().map(|m| m + 0.002).collect();
        let (alpha, beta) = alpha_beta(&agent, &market);
        assert!((beta - 1.0).abs() < 1e-9, "beta={beta}");
        assert!((alpha - 0.002).abs() < 1e-9, "alpha={alpha}");
    }
}
