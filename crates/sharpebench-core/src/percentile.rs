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

/// Where an agent's Deflated Sharpe sits relative to the human-baseline band.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BaselineBand {
    /// Below a marginally-skilled human track: likely no durable edge.
    Below,
    /// Inside the skilled-human range: a credible, human-comparable edge.
    Within,
    /// Above a top-decile human track: superhuman, or a leak worth auditing.
    Above,
}

/// A skilled-human-trader Sharpe band: the collection mechanism the frozen
/// reference population was always meant to have. It turns "DSR = 0.97" into
/// "would sit inside the skilled-human band": a solvability / upper-bound marker
/// the board can plot a DSR against, instead of an abstract 0..1 number.
///
/// The band is expressed as **per-period** Sharpe ratios (never annualized; the
/// rest of the crate scores per-period returns).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct HumanBaseline {
    /// A marginally-skilled track (the floor of "has an edge at all").
    pub floor_sharpe: f64,
    /// A solidly-skilled discretionary trader (the reference point).
    pub median_sharpe: f64,
    /// A top-decile track (the upper bound: is the task even solvable this well?).
    pub ceiling_sharpe: f64,
}

impl HumanBaseline {
    /// Default band for a skilled discretionary trader, derived from the commonly
    /// cited *annualized* Sharpe range (≈0.5 marginal / 1.0 solid / 2.0 top-decile)
    /// de-annualized over 252 trading periods (`SR_period = SR_annual / √252`).
    pub fn skilled_trader() -> Self {
        let per_period = |annual: f64| annual / 252.0_f64.sqrt();
        Self {
            floor_sharpe: per_period(0.5),
            median_sharpe: per_period(1.0),
            ceiling_sharpe: per_period(2.0),
        }
    }

    /// Convert the band into a frozen reference **DSR** population `[floor, median,
    /// ceiling]` that [`percentile_of`] can score an agent against. Each band
    /// Sharpe is mapped to the Deflated Sharpe a *clean normal track* of that
    /// per-period Sharpe and `track_len` periods would earn against `n_trials`
    /// (skew 0, kurtosis 3: a reference marker, not a real return stream).
    pub fn reference_dsr_population(
        &self,
        track_len: usize,
        n_trials: u32,
        trials_sr_std: f64,
    ) -> Vec<f64> {
        [self.floor_sharpe, self.median_sharpe, self.ceiling_sharpe]
            .iter()
            .map(|&sr| dsr_from_sharpe(sr, track_len, n_trials, trials_sr_std))
            .collect()
    }

    /// Classify a Deflated Sharpe against the band: `Below` the floor, `Within`
    /// the skilled-human range, or `Above` the ceiling. Uses the same normal-track
    /// mapping as [`reference_dsr_population`].
    pub fn classify_dsr(
        &self,
        dsr: f64,
        track_len: usize,
        n_trials: u32,
        trials_sr_std: f64,
    ) -> BaselineBand {
        let floor = dsr_from_sharpe(self.floor_sharpe, track_len, n_trials, trials_sr_std);
        let ceiling = dsr_from_sharpe(self.ceiling_sharpe, track_len, n_trials, trials_sr_std);
        if dsr < floor {
            BaselineBand::Below
        } else if dsr > ceiling {
            BaselineBand::Above
        } else {
            BaselineBand::Within
        }
    }
}

/// Deflated Sharpe a clean normal track (skew 0, kurtosis 3) of per-period Sharpe
/// `sr` and length `track_len` would earn against `n_trials`. This is the PSR
/// z-statistic evaluated for a normal return distribution, deflated by the
/// expected maximum Sharpe over the trial footprint: the human-baseline analogue
/// of [`crate::deflated_sharpe::deflated_sharpe_ratio`] on summary statistics.
fn dsr_from_sharpe(sr: f64, track_len: usize, n_trials: u32, trials_sr_std: f64) -> f64 {
    if track_len < 2 {
        return 0.0;
    }
    let sr_star = crate::deflated_sharpe::expected_max_sharpe(trials_sr_std, n_trials);
    // Normal-track PSR denominator: 1 - g3*sr + ((g4-1)/4)*sr^2 with g3=0, g4=3.
    let denom = (1.0 + 0.5 * sr * sr).max(1e-12).sqrt();
    let z = (sr - sr_star) * (track_len as f64 - 1.0).sqrt() / denom;
    crate::stats::norm_cdf(z)
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

    #[test]
    fn skilled_trader_band_is_ordered_and_per_period() {
        let b = HumanBaseline::skilled_trader();
        assert!(b.floor_sharpe < b.median_sharpe && b.median_sharpe < b.ceiling_sharpe);
        // De-annualized: 1.0 annual / sqrt(252) ≈ 0.063 per period.
        assert!(approx(b.median_sharpe, 1.0 / 252.0_f64.sqrt()));
    }

    #[test]
    fn reference_population_is_ordered_and_scores_a_dsr() {
        let b = HumanBaseline::skilled_trader();
        let pop = b.reference_dsr_population(500, 50, 0.5);
        assert_eq!(pop.len(), 3);
        assert!(
            pop[0] <= pop[1] && pop[1] <= pop[2],
            "floor≤median≤ceiling DSR"
        );
        // The population feeds the existing percentile path unchanged.
        let pct = percentile_of(pop[1], &pop);
        assert!((0.0..=100.0).contains(&pct));
    }

    #[test]
    fn classify_dsr_brackets_the_band() {
        let b = HumanBaseline::skilled_trader();
        let (len, nt, disp) = (500, 50, 0.5);
        let pop = b.reference_dsr_population(len, nt, disp);
        // A DSR under the floor, inside the band, and over the ceiling classify right.
        assert_eq!(
            b.classify_dsr(pop[0] - 0.1, len, nt, disp),
            BaselineBand::Below
        );
        assert_eq!(
            b.classify_dsr((pop[0] + pop[2]) / 2.0, len, nt, disp),
            BaselineBand::Within
        );
        assert_eq!(
            b.classify_dsr(pop[2] + 1e-6, len, nt, disp),
            BaselineBand::Above
        );
    }
}
