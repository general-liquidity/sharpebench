//! Legibility: a bare Deflated Sharpe number is illegible to outsiders.
//!
//! Reporting an agent's percentile against a **frozen reference population**
//! (e.g. real fund or human track-record Sharpes) turns the score into "would
//! rank in the Nth percentile of the field" — the single most credibility-
//! multiplying framing. Pairs with the ordinal rank mode (see `composite`), a
//! scale-invariant complement to the cardinal Deflated Sharpe.
//!
//! After ALE-Bench's percentile-against-a-human-population framing.

/// Percentile (0..=100) of `value` within a reference population: the fraction
/// of the population it meets or exceeds, times 100. Empty population → 0.0.
pub fn percentile_of(value: f64, population: &[f64]) -> f64 {
    if population.is_empty() {
        return 0.0;
    }
    let n_le = population.iter().filter(|&&p| value >= p).count();
    100.0 * n_le as f64 / population.len() as f64
}

#[cfg(test)]
mod tests {
    use super::*;

    fn approx(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-9
    }

    #[test]
    fn ranks_within_population() {
        let pop = [0.2, 0.5, 0.8, 1.1, 1.4];
        assert!(approx(percentile_of(0.9, &pop), 60.0)); // beats 0.2/0.5/0.8
        assert!(approx(percentile_of(0.1, &pop), 0.0)); // beats none
        assert!(approx(percentile_of(2.0, &pop), 100.0)); // beats all
        assert!(approx(percentile_of(0.5, &pop), 40.0)); // ties count (>=)
    }

    #[test]
    fn empty_population_is_zero() {
        assert_eq!(percentile_of(1.0, &[]), 0.0);
    }
}
