//! Legibility: a bare Deflated Sharpe number is illegible to outsiders.
//!
//! Reporting an agent's percentile against a **frozen reference population**
//! (e.g. real fund or human track-record Sharpes) turns the score into "would
//! rank in the Nth percentile of the field" — the single most credibility-
//! multiplying framing. Pairs with the ordinal rank mode (see `composite`), a
//! scale-invariant complement to the cardinal Deflated Sharpe.
//!
//! After ALE-Bench's percentile-against-a-human-population framing.

use sharpebench_stats::StatisticalError;

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
/// rest of the crate scores per-period returns), at the frequency chosen when it
/// was built. Every input the band is compared with must be at that frequency:
/// the track length is a count of those periods, and the trial dispersion is a
/// per-period standard deviation.
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
    /// de-annualized to the scored frequency:
    /// `SR_period = SR_annual / sqrt(periods_per_year)`, with the same
    /// `periods_per_year` as the [`ScoreConfig`](crate::ScoreConfig) the agent was
    /// scored under (252 for daily equities, 8760 for hourly crypto). The
    /// square-root scaling is exact only for serially independent returns.
    ///
    /// # Errors
    ///
    /// `periods_per_year` must be finite and positive; anything else has no
    /// frequency to de-annualize to.
    pub fn skilled_trader(periods_per_year: f64) -> Result<Self, StatisticalError> {
        if !periods_per_year.is_finite() || periods_per_year <= 0.0 {
            return Err(StatisticalError::InvalidParameter {
                name: "periods_per_year",
                requirement: "must be finite and positive",
            });
        }
        let per_period = |annual: f64| annual / periods_per_year.sqrt();
        Ok(Self {
            floor_sharpe: per_period(0.5),
            median_sharpe: per_period(1.0),
            ceiling_sharpe: per_period(2.0),
        })
    }

    /// Convert the band into a frozen reference **DSR** population `[floor, median,
    /// ceiling]` that [`percentile_of`] can score an agent against. Each band
    /// Sharpe is mapped to the Deflated Sharpe a *clean normal track* of that
    /// per-period Sharpe and `track_len` periods would earn against `n_trials`
    /// (skew 0, kurtosis 3: a reference marker, not a real return stream).
    ///
    /// Units: `track_len` counts periods at the band's frequency, and
    /// `trials_sr_std` is the **per-period** standard deviation of trial Sharpes
    /// at that frequency, not the annualized prior of
    /// [`ScoreConfig::trials_sr_std`](crate::ScoreConfig::trials_sr_std). Pass
    /// [`per_period_sr_std`](crate::per_period_sr_std) of the scoring config, or
    /// the per-period `trials_sr_std` a [`CompositeScore`](crate::CompositeScore)
    /// records. An annualized value passed here raises the bar by
    /// `sqrt(periods_per_year)`, the unit error of the paper's first finding.
    ///
    /// # Errors
    ///
    /// `trials_sr_std` must be a dispersion: finite and non-negative. A negative
    /// one used to collapse the deflation bar to zero, which would have mapped
    /// every band Sharpe to its most flattering DSR and made the reference
    /// population the agent is scored against easier than it is.
    pub fn reference_dsr_population(
        &self,
        track_len: usize,
        n_trials: u32,
        trials_sr_std: f64,
    ) -> Result<Vec<f64>, StatisticalError> {
        [self.floor_sharpe, self.median_sharpe, self.ceiling_sharpe]
            .iter()
            .map(|&sr| dsr_from_sharpe(sr, track_len, n_trials, trials_sr_std))
            .collect()
    }

    /// Classify a Deflated Sharpe against the band: `Below` the floor, `Within`
    /// the skilled-human range, or `Above` the ceiling. Uses the same normal-track
    /// mapping, and the same units, as [`Self::reference_dsr_population`].
    ///
    /// # Errors
    ///
    /// Same boundary as [`Self::reference_dsr_population`]: a band that cannot be
    /// placed is not a band the agent sits `Within`.
    pub fn classify_dsr(
        &self,
        dsr: f64,
        track_len: usize,
        n_trials: u32,
        trials_sr_std: f64,
    ) -> Result<BaselineBand, StatisticalError> {
        let floor = dsr_from_sharpe(self.floor_sharpe, track_len, n_trials, trials_sr_std)?;
        let ceiling = dsr_from_sharpe(self.ceiling_sharpe, track_len, n_trials, trials_sr_std)?;
        Ok(if dsr < floor {
            BaselineBand::Below
        } else if dsr > ceiling {
            BaselineBand::Above
        } else {
            BaselineBand::Within
        })
    }
}

/// Deflated Sharpe a clean normal track (skew 0, kurtosis 3) of per-period Sharpe
/// `sr` and length `track_len` would earn against `n_trials`. This is the PSR
/// z-statistic evaluated for a normal return distribution, deflated by the
/// expected maximum Sharpe over the trial footprint: the human-baseline analogue
/// of [`crate::deflated_sharpe::deflated_sharpe_ratio`] on summary statistics.
fn dsr_from_sharpe(
    sr: f64,
    track_len: usize,
    n_trials: u32,
    trials_sr_std: f64,
) -> Result<f64, StatisticalError> {
    let sr_star = crate::deflated_sharpe::expected_max_sharpe(trials_sr_std, n_trials)?;
    if track_len < 2 {
        return Ok(0.0);
    }
    // Normal-track PSR denominator: 1 - g3*sr + ((g4-1)/4)*sr^2 with g3=0, g4=3.
    let denom = (1.0 + 0.5 * sr * sr).max(1e-12).sqrt();
    let z = (sr - sr_star) * (track_len as f64 - 1.0).sqrt() / denom;
    Ok(crate::stats::norm_cdf(z))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn approx(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-9
    }

    /// The daily band, and the shipped annualized prior of 0.5 in the per-period
    /// unit the band is compared in.
    fn daily() -> (HumanBaseline, f64) {
        (
            HumanBaseline::skilled_trader(252.0).unwrap(),
            0.5 / 252.0_f64.sqrt(),
        )
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
        let (b, _) = daily();
        assert!(b.floor_sharpe < b.median_sharpe && b.median_sharpe < b.ceiling_sharpe);
        // De-annualized: 1.0 annual / sqrt(252) ≈ 0.063 per period.
        assert!(approx(b.median_sharpe, 1.0 / 252.0_f64.sqrt()));
    }

    /// F8: the band follows the scored frequency instead of assuming 252 bars a
    /// year. An hourly band is sqrt(8760 / 252) times smaller per period than
    /// the daily one, a weekly band larger, and each re-annualizes to the same
    /// 0.5 / 1.0 / 2.0 at its own frequency.
    #[test]
    fn skilled_trader_follows_periods_per_year() {
        for ppy in [52.0_f64, 252.0, 365.0, 2190.0, 8760.0] {
            let b = HumanBaseline::skilled_trader(ppy).unwrap();
            let annual = |sr: f64| sr * ppy.sqrt();
            assert!(approx(annual(b.floor_sharpe), 0.5), "{ppy}");
            assert!(approx(annual(b.median_sharpe), 1.0), "{ppy}");
            assert!(approx(annual(b.ceiling_sharpe), 2.0), "{ppy}");
        }
        let daily = HumanBaseline::skilled_trader(252.0).unwrap();
        let hourly = HumanBaseline::skilled_trader(8760.0).unwrap();
        assert!(approx(
            daily.median_sharpe / hourly.median_sharpe,
            (8760.0_f64 / 252.0).sqrt()
        ));
    }

    /// F8 boundary: a frequency that is not a positive real number is refused
    /// rather than turned into an infinite or NaN band.
    #[test]
    fn skilled_trader_periods_per_year_boundary() {
        let expected = Err(StatisticalError::InvalidParameter {
            name: "periods_per_year",
            requirement: "must be finite and positive",
        });
        for bad in [
            0.0,
            -0.0,
            -252.0,
            f64::NAN,
            f64::INFINITY,
            f64::NEG_INFINITY,
        ] {
            assert_eq!(HumanBaseline::skilled_trader(bad), expected, "{bad}");
        }
        let smallest = HumanBaseline::skilled_trader(f64::MIN_POSITIVE).unwrap();
        assert!(smallest.ceiling_sharpe.is_finite());
        assert!(approx(
            HumanBaseline::skilled_trader(1.0).unwrap().median_sharpe,
            1.0
        ));
    }

    #[test]
    fn reference_population_is_ordered_and_scores_a_dsr() {
        let (b, sigma) = daily();
        let pop = b.reference_dsr_population(500, 50, sigma).unwrap();
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
        let (b, disp) = daily();
        let (len, nt) = (500, 50);
        let pop = b.reference_dsr_population(len, nt, disp).unwrap();
        // A DSR under the floor, inside the band, and over the ceiling classify right.
        assert_eq!(
            b.classify_dsr(pop[0] - 0.1, len, nt, disp).unwrap(),
            BaselineBand::Below
        );
        assert_eq!(
            b.classify_dsr((pop[0] + pop[2]) / 2.0, len, nt, disp)
                .unwrap(),
            BaselineBand::Within
        );
        assert_eq!(
            b.classify_dsr(pop[2] + 1e-6, len, nt, disp).unwrap(),
            BaselineBand::Above
        );
    }

    /// R02: a dispersion that is not a dispersion cannot place the human band.
    ///
    /// A negative `trials_sr_std` used to reach `expected_max_sharpe`'s
    /// "nothing to deflate for" branch, zeroing the bar and lifting every
    /// reference DSR toward 1.0, so an agent would be compared against a band
    /// that had been quietly made easier.
    #[test]
    fn an_invalid_dispersion_cannot_place_the_band() {
        let (b, _) = daily();
        let expected = StatisticalError::InvalidParameter {
            name: "trials_sr_std",
            requirement: "must be finite and non-negative",
        };
        for bad in [-1.0, f64::NAN, f64::INFINITY] {
            assert_eq!(b.reference_dsr_population(500, 50, bad), Err(expected));
            assert_eq!(b.classify_dsr(0.9, 500, 50, bad), Err(expected));
            // A short track is refused too: the boundary precedes the
            // degenerate-length shortcut, so it cannot be stepped around.
            assert_eq!(b.reference_dsr_population(1, 50, bad), Err(expected));
        }
    }

    /// R02 guard: the valid-input population and its classification are the
    /// numbers the board publishes, so they are pinned exactly.
    #[test]
    fn valid_baseline_inputs_return_the_same_numbers() {
        let (b, sigma) = daily();
        let pop = b.reference_dsr_population(500, 50, sigma).unwrap();
        let sr_star = crate::deflated_sharpe::expected_max_sharpe(sigma, 50).unwrap();
        let expect = |sr: f64| {
            let denom = (1.0 + 0.5 * sr * sr).max(1e-12).sqrt();
            crate::stats::norm_cdf((sr - sr_star) * (500.0_f64 - 1.0).sqrt() / denom)
        };
        assert_eq!(pop[0], expect(b.floor_sharpe));
        assert_eq!(pop[1], expect(b.median_sharpe));
        assert_eq!(pop[2], expect(b.ceiling_sharpe));
        // A track too short to score still reports the historical 0.0 band.
        assert_eq!(
            b.reference_dsr_population(1, 50, sigma).unwrap(),
            vec![0.0; 3]
        );
    }
}
