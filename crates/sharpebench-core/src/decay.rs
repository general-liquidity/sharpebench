//! Edge decay — does the agent's signal survive forward in time, or is it a
//! one-regime fluke? We estimate the half-life of the absolute information
//! coefficient by regressing `ln|IC|` on time. The half-life is reported on the
//! composite so a fast-decaying edge is visible even when its average looks
//! good; it is never read by the eligibility predicate and does not affect
//! rank. (After QuantBench's IC half-life.)
//!
//! [`edge_half_life`] is purely empirical: it reads the decay out of the track.
//! The second half of this module is its complement, an *expected* half-life
//! from a crowding model, so a measured number has something to be surprising
//! against. See [`crowding_half_life`] and [`compare_decay_to_prior`].

use serde::Serialize;

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

/// Parameters of the crowding decay model behind [`crowding_half_life`].
///
/// All rates are **per period of the caller's IC series**. The model has no
/// opinion about whether a period is a day, a week or a month, and it cannot
/// have one: the caller owns that unit, and mixing a monthly `theta` with a
/// daily IC series produces a meaningless comparison. There is deliberately no
/// `Default`. Supplying a stock calibration would smuggle a modelled number into
/// the crate and let it be read as measured, which is exactly what the honesty
/// note on [`crowding_half_life`] is about.
#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub struct CrowdingParams {
    /// `theta`: natural mean-reversion rate of the edge, the decay that happens
    /// with zero adoption because the underlying inefficiency closes on its own.
    pub theta: f64,
    /// The crowding decay rate at full adoption (`phi = 1`). `delta(phi)` scales
    /// up to this.
    pub delta_max: f64,
    /// Exponent on adoption in `delta(phi) = delta_max * phi^curvature`. At
    /// `1.0`, `delta` is linear in adoption and the resulting half-life is
    /// convex-decreasing in `phi`, which is the model's shape claim: the first
    /// arrivals cost you a lot of half-life, later ones progressively less.
    /// Values above 1 delay the onset of crowding; values below 1 front-load it.
    pub curvature: f64,
}

/// The expected half-life implied by a crowding model, alongside the pieces it
/// was built from.
#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub struct CrowdingDecayPrior {
    /// Adoption `phi` used, clamped to [0, 1].
    pub adoption: f64,
    /// `theta`, echoed back.
    pub natural_reversion: f64,
    /// `delta(phi)`: the adoption-driven component.
    pub crowding_decay: f64,
    /// `ln2 / (theta + delta(phi))`, in periods. `None` when the total hazard is
    /// non-positive, which is to say the model says the edge never decays and
    /// there is no half-life to speak of.
    pub expected_half_life: Option<f64>,
}

/// Expected half-life under the crowding model from Meng & Chen (2026),
/// "AI-Driven Alpha Decay":
///
/// ```text
/// h(phi) = ln2 / [ theta + delta(phi) ],   delta(phi) = delta_max * phi^curvature
/// ```
///
/// `theta` is the natural mean reversion an edge suffers with nobody else in it;
/// `delta(phi)` is the extra decay bought by adoption. `delta` rises with `phi`,
/// so `h` falls with it, and falls convexly: the marginal damage of one more
/// adopter shrinks once the trade is already crowded.
///
/// # Honesty note on the paper's numbers
///
/// The paper's headline calibration, roughly 18-month half-lives at high
/// adoption against 5 to 7 years pre-AI, is a **model output**, not a
/// measurement. It comes out of the paper's simulation, not out of a dataset.
/// The genuinely empirical leg of that work is the 13F portfolio-convergence
/// study (99.5M holdings, 2013-2024, +42% convergence); the fund return dynamics
/// and the flash-crash fragility result are simulated. So this crate does not
/// ship those half-lives as constants, does not default `theta` or `delta_max`
/// to values implying them, and never describes the output of this function as
/// measured. It is a prior. Say so wherever you print it.
///
/// Adoption outside [0, 1] is clamped rather than rejected: a caller estimating
/// `phi` from a crowding proxy can land slightly outside the unit interval, and
/// that is not worth an error path.
pub fn crowding_half_life(adoption: f64, params: CrowdingParams) -> CrowdingDecayPrior {
    let phi = adoption.clamp(0.0, 1.0);
    let curvature = if params.curvature > 0.0 {
        params.curvature
    } else {
        1.0
    };
    let delta = params.delta_max.max(0.0) * phi.powf(curvature);
    let hazard = params.theta + delta;
    CrowdingDecayPrior {
        adoption: phi,
        natural_reversion: params.theta,
        crowding_decay: delta,
        expected_half_life: if hazard > 0.0 {
            Some(std::f64::consts::LN_2 / hazard)
        } else {
            None
        },
    }
}

/// Measured decay set against the crowding prior.
#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub struct DecayComparison {
    /// [`edge_half_life`] on the supplied IC series, in periods. `None` when the
    /// series is too short or is not decaying at all.
    pub measured_half_life: Option<f64>,
    /// The modelled expectation the measurement is being read against.
    pub prior: CrowdingDecayPrior,
    /// `measured / expected`. Below 1 means the edge is dying faster than
    /// crowding alone accounts for; above 1 means it is holding up better.
    /// `None` when either side is missing.
    pub ratio: Option<f64>,
    /// True when `ratio` falls below `anomaly_ratio`: the decay is too fast for
    /// crowding to be the whole story, which usually points at overfitting, a
    /// broken data pipeline, or a regime the strategy was never fit for.
    ///
    /// This is **reported, never a gate**. Nothing in [`crate::composite`]
    /// consumes it and nothing should: ranking on agreement with a model would
    /// penalise a real edge for the offence of disagreeing with a simulation,
    /// which inverts the entire point of the board.
    pub anomalous: bool,
}

/// Measure decay empirically and report it against the crowding prior.
///
/// `anomaly_ratio` is the measured-over-expected threshold below which the decay
/// is flagged as unexplained by crowding; 0.5 (dying twice as fast as the model
/// expects) is a reasonable starting point, but it is a judgement call and the
/// caller makes it.
///
/// Pure and deterministic. The flag is a diagnostic, not a verdict: see
/// [`DecayComparison::anomalous`].
pub fn compare_decay_to_prior(
    ic_series: &[f64],
    adoption: f64,
    params: CrowdingParams,
    anomaly_ratio: f64,
) -> DecayComparison {
    let measured_half_life = edge_half_life(ic_series);
    let prior = crowding_half_life(adoption, params);
    let ratio = match (measured_half_life, prior.expected_half_life) {
        (Some(m), Some(e)) if e > 0.0 => Some(m / e),
        _ => None,
    };
    DecayComparison {
        measured_half_life,
        prior,
        ratio,
        anomalous: ratio.is_some_and(|r| r < anomaly_ratio),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Per-period rates for a series whose half-life sits near 7 periods, chosen
    /// only so the arithmetic in these tests is legible. Not a calibration.
    const TEST_PARAMS: CrowdingParams = CrowdingParams {
        theta: 0.05,
        delta_max: 0.05,
        curvature: 1.0,
    };

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

    #[test]
    fn crowding_shortens_the_expected_half_life() {
        let lonely = crowding_half_life(0.0, TEST_PARAMS);
        let crowded = crowding_half_life(1.0, TEST_PARAMS);
        assert!(
            lonely.crowding_decay.abs() < 1e-12,
            "nobody has arrived yet"
        );
        assert!((crowded.crowding_decay - 0.05).abs() < 1e-12);
        // ln2/0.05 = 13.863 periods alone, ln2/0.10 = 6.931 once crowded.
        assert!((lonely.expected_half_life.unwrap() - 13.8629).abs() < 1e-3);
        assert!((crowded.expected_half_life.unwrap() - 6.9315).abs() < 1e-3);
    }

    #[test]
    fn expected_half_life_is_convex_decreasing_in_adoption() {
        let h0 = crowding_half_life(0.0, TEST_PARAMS)
            .expected_half_life
            .unwrap();
        let h_mid = crowding_half_life(0.5, TEST_PARAMS)
            .expected_half_life
            .unwrap();
        let h1 = crowding_half_life(1.0, TEST_PARAMS)
            .expected_half_life
            .unwrap();
        assert!(h0 > h_mid && h_mid > h1, "decreasing: {h0} {h_mid} {h1}");
        assert!(
            h_mid < 0.5 * (h0 + h1),
            "convex: midpoint {h_mid} should sit under the chord {}",
            0.5 * (h0 + h1)
        );
    }

    #[test]
    fn adoption_is_clamped_rather_than_rejected() {
        assert_eq!(
            crowding_half_life(-3.0, TEST_PARAMS),
            crowding_half_life(0.0, TEST_PARAMS)
        );
        assert_eq!(
            crowding_half_life(4.0, TEST_PARAMS),
            crowding_half_life(1.0, TEST_PARAMS)
        );
    }

    #[test]
    fn a_model_with_no_decay_has_no_half_life() {
        let params = CrowdingParams {
            theta: 0.0,
            delta_max: 0.0,
            curvature: 1.0,
        };
        assert!(crowding_half_life(1.0, params).expected_half_life.is_none());
    }

    #[test]
    fn measured_matching_the_prior_is_not_anomalous() {
        // ic_t = 0.2 * exp(-0.1 t) → measured half-life ≈ 6.93, which is exactly
        // what the model expects at full adoption.
        let ic: Vec<f64> = (0..40).map(|t| 0.2 * (-0.1 * t as f64).exp()).collect();
        let c = compare_decay_to_prior(&ic, 1.0, TEST_PARAMS, 0.5);
        let ratio = c.ratio.expect("both sides present");
        assert!((ratio - 1.0).abs() < 0.1, "ratio={ratio}");
        assert!(!c.anomalous);
    }

    #[test]
    fn decay_faster_than_crowding_explains_is_flagged() {
        // Same track, but the operator says nobody else is in this trade. The
        // model then expects ~13.86 periods and the tape delivers ~6.93.
        let ic: Vec<f64> = (0..40).map(|t| 0.2 * (-0.1 * t as f64).exp()).collect();
        let c = compare_decay_to_prior(&ic, 0.0, TEST_PARAMS, 0.6);
        let ratio = c.ratio.expect("both sides present");
        assert!((ratio - 0.5).abs() < 0.05, "ratio={ratio}");
        assert!(c.anomalous, "twice the modelled decay rate should flag");
        // The flag is a threshold call, so a looser threshold clears it.
        assert!(!compare_decay_to_prior(&ic, 0.0, TEST_PARAMS, 0.4).anomalous);
    }

    #[test]
    fn a_non_decaying_edge_yields_no_ratio_and_no_flag() {
        let ic = vec![0.1; 30];
        let c = compare_decay_to_prior(&ic, 0.5, TEST_PARAMS, 0.5);
        assert!(c.measured_half_life.is_none());
        assert!(c.ratio.is_none());
        assert!(!c.anomalous, "absence of measurement is not an anomaly");
        assert!(
            c.prior.expected_half_life.is_some(),
            "the prior still stands"
        );
    }
}
