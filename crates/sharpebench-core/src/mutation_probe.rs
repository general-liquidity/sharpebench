//! Confidence calibration — does the agent's *stated* conviction predict its
//! outcomes? An agent that knows when it doesn't know is more trustworthy with
//! capital than one with a marginally higher Sharpe and no self-knowledge. We
//! score this with the Brier score (lower is better; 0 = perfect, 0.25 = the
//! always-0.5 baseline).
//!
//! ## Why one scalar is not enough
//!
//! A Brier score cannot tell two very different agents apart. One of them said
//! "I do not know" and was right to say so, because the outcome series was a coin
//! flip that no amount of further work would have resolved. The other said "I do
//! not know" because it had not looked hard enough, or because its own signals
//! were pulling in opposite directions and it split the difference. Those deserve
//! opposite treatment from an operator: the first agent is behaving correctly and
//! should be trusted with capital, the second is telling you that more evidence
//! would change its mind and should be asked to go get it. So we also split the
//! uncertainty into three legs, adapted from "Trading Confidence: Comprehensive
//! Uncertainty Estimation in Algorithmic Trading" (Lin, Wang, Fu, Fan, PACIS
//! 2025):
//!
//! - [`aleatoric_uncertainty`]: irreducible noise in the outcome series. More
//!   evidence does not reduce it.
//! - [`epistemic_uncertainty`]: the agent does not know. Reducible by more
//!   evidence, and read off disagreement between independent signals plus how
//!   thin the evidence backing them is.
//! - [`distributional_uncertainty`]: the case is unlike the reference set, so
//!   whatever the agent learned on the reference may simply not apply here.
//!
//! ## What is faithful to the source and what is re-derived
//!
//! The three-way taxonomy is the source paper's and is used here as stated. The
//! *estimators* are not. The paper obtains its legs from a neural reinforcement
//! learning stack: MC Dropout passes for epistemic spread, a SHAP-weighted
//! reconstruction error for distributional shift, an LSTM consensus head for the
//! aleatoric term. SharpeBench has no model and no training loop. It is a
//! deterministic scoring kernel that sees a finished [`crate::composite::Run`]
//! and nothing else, so every leg here is re-derived from what a scored run
//! actually carries: stated confidences, realized binary outcomes, and returns.
//!
//! Concretely: the aleatoric leg is the classical Murphy base-rate variance of
//! the outcomes, which is faithful in meaning but not in method. The epistemic
//! leg replaces stochastic forward passes with real disagreement between the
//! agent's own independent confidence streams (its separate seeds and windows),
//! widened when there are few paired observations behind them. The distributional
//! leg replaces reconstruction error with a plain location-and-dispersion
//! distance from a reference return series. The paper's reported accuracy and
//! risk-adjusted-return gains are measured for its RL agents on its own data and
//! say nothing about these estimators: they do not transfer, and no number from
//! that paper is claimed here.
//!
//! ## The load-bearing limitation
//!
//! Agreement is not correctness. Signals that are unanimous and wrong produce
//! near-zero disagreement, so the epistemic leg reads low at exactly the moment
//! the agent is most confidently mistaken. Signals that are correlated, which is
//! the normal case when the same strategy is re-run across seeds on overlapping
//! data, share their errors and therefore also understate it. Both failures push
//! in the same direction: **the epistemic leg is a lower bound, never an upper
//! one.** A low reading is weak evidence of knowledge and a high reading is
//! strong evidence of ignorance, so treat only the high readings as informative.
//! The thinness term exists to keep the leg from reading zero on a single stream,
//! where disagreement is not measurable at all.

use serde::Serialize;

use crate::stats::{mean, std_dev};

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

/// The three legs of uncertainty behind one scored case. Every leg is on [0, 1]
/// so they are readable side by side; none of them is a probability.
///
/// They are reported, never summed into a single verdict. The point of the split
/// is that the legs call for different responses: a high aleatoric leg says stop
/// looking, a high epistemic leg says keep looking, a high distributional leg
/// says this case is outside what the reference can speak to.
#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub struct UncertaintySplit {
    /// Irreducible outcome noise. See [`aleatoric_uncertainty`].
    pub aleatoric: f64,
    /// Reducible ignorance. See [`epistemic_uncertainty`].
    pub epistemic: f64,
    /// Unlikeness to the reference set. See [`distributional_uncertainty`].
    pub distributional: f64,
}

/// Irreducible noise in the outcome series: the base-rate variance of the
/// outcomes, rescaled so that 1.0 is a fair coin and 0.0 is an outcome series
/// that always went the same way.
///
/// This is the classical Murphy "uncertainty" term of the Brier decomposition,
/// `p(1 - p)` for base rate `p`, multiplied by 4 to put it on the same [0, 1]
/// scale as the other two legs. Multiply by 0.25 to recover the Murphy term and
/// compare it directly against [`brier_score`]: a Brier score that merely matches
/// this floor is a forecaster adding nothing over knowing the base rate.
///
/// It is irreducible in the sense the source taxonomy means: it is a property of
/// the outcome series, so it does not fall as the agent gathers more evidence.
/// Returns 0.0 for an empty series, where there is nothing to be uncertain about.
pub fn aleatoric_uncertainty(outcomes: &[bool]) -> f64 {
    if outcomes.is_empty() {
        return 0.0;
    }
    let base = outcomes.iter().filter(|&&o| o).count() as f64 / outcomes.len() as f64;
    (4.0 * base * (1.0 - base)).clamp(0.0, 1.0)
}

/// Reducible ignorance, read off two observable things: how much the agent's
/// independent confidence streams disagree with each other, and how little
/// evidence stands behind them.
///
/// `signals` are independent per-decision confidence streams for the *same*
/// decisions, matched by index. In SharpeBench these are the agent's separate
/// seeds and windows: the closest honest stand-in for the source paper's MC
/// Dropout ensemble, since both ask what the same policy says when the world is
/// jiggled. Streams of unequal length are compared over their common prefix.
///
/// Disagreement is the mean across decisions of the population variance of the
/// confidences, scaled by 4 so that a maximal split (half the streams at 0.0,
/// half at 1.0) reads 1.0. Thinness is `1 / (1 + n)` over the `n` decisions
/// actually covered, so an estimate resting on almost nothing cannot report
/// confidence it has not earned. The two combine as a noisy OR, meaning either
/// one alone raises the leg and both together raise it further.
///
/// Fewer than two streams gives no measurable disagreement, so the result is the
/// thinness term alone. Read that as "unknown", not as "low": see the module doc
/// on why this leg is a lower bound.
pub fn epistemic_uncertainty(signals: &[&[f64]]) -> f64 {
    let k = signals.len();
    let n = signals.iter().map(|s| s.len()).min().unwrap_or(0);
    let disagreement = if k < 2 || n == 0 {
        0.0
    } else {
        let mut acc = 0.0;
        for i in 0..n {
            let m = signals.iter().map(|s| s[i].clamp(0.0, 1.0)).sum::<f64>() / k as f64;
            let var = signals
                .iter()
                .map(|s| {
                    let d = s[i].clamp(0.0, 1.0) - m;
                    d * d
                })
                .sum::<f64>()
                / k as f64;
            acc += var;
        }
        (4.0 * acc / n as f64).clamp(0.0, 1.0)
    };
    let thinness = 1.0 / (1.0 + n as f64);
    1.0 - (1.0 - disagreement) * (1.0 - thinness)
}

/// How unlike the reference set this case is, in [0, 1).
///
/// The source paper scores this with a SHAP-weighted reconstruction error from a
/// trained encoder. With no model to reconstruct anything, we measure the two
/// ways a return series can be a different animal from its reference: it sits
/// somewhere else (a shift in the mean) or it moves differently (a shift in the
/// dispersion). Both are expressed in reference standard deviations, the larger
/// of the two is taken, and `d / (1 + d)` squashes it so the leg saturates
/// gracefully instead of running away on a single outlier.
///
/// This is the leg that speaks when a case looks fine by every in-sample measure
/// and is nonetheless somewhere the reference cannot vouch for. Returns 0.0 when
/// either series has fewer than two points, since claiming novelty on that
/// evidence would be an invention rather than a measurement.
pub fn distributional_uncertainty(case: &[f64], reference: &[f64]) -> f64 {
    if case.len() < 2 || reference.len() < 2 {
        return 0.0;
    }
    let (m_case, m_ref) = (mean(case), mean(reference));
    let (s_case, s_ref) = (std_dev(case), std_dev(reference));
    if s_ref <= 0.0 {
        // A constant reference admits no scale to measure distance in: the case
        // is either that same constant or it is entirely outside the reference.
        return if m_case == m_ref && s_case <= 0.0 {
            0.0
        } else {
            1.0
        };
    }
    let z_location = (m_case - m_ref).abs() / s_ref;
    let z_dispersion = (s_case - s_ref).abs() / s_ref;
    let d = z_location.max(z_dispersion);
    d / (1.0 + d)
}

/// All three legs for one case in a single call.
///
/// `outcomes` drives the aleatoric leg, `signals` the epistemic leg, and
/// `case_returns` against `reference_returns` the distributional leg. Any of them
/// may be empty; the corresponding leg then reports what it can honestly report
/// on no evidence (see each function's own doc).
pub fn decompose_uncertainty(
    outcomes: &[bool],
    signals: &[&[f64]],
    case_returns: &[f64],
    reference_returns: &[f64],
) -> UncertaintySplit {
    UncertaintySplit {
        aleatoric: aleatoric_uncertainty(outcomes),
        epistemic: epistemic_uncertainty(signals),
        distributional: distributional_uncertainty(case_returns, reference_returns),
    }
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

    #[test]
    fn aleatoric_is_maximal_for_a_coin_flip() {
        let o = [true, false, true, false];
        assert!((aleatoric_uncertainty(&o) - 1.0).abs() < 1e-12);
        // Rescaled back, this is exactly the Murphy uncertainty term of 0.25,
        // which is also the Brier score of the always-0.5 forecaster above.
        assert!((0.25 * aleatoric_uncertainty(&o) - 0.25).abs() < 1e-12);
    }

    #[test]
    fn aleatoric_is_zero_when_the_outcome_never_varies() {
        assert_eq!(aleatoric_uncertainty(&[true; 8]), 0.0);
        assert_eq!(aleatoric_uncertainty(&[false; 8]), 0.0);
        assert_eq!(aleatoric_uncertainty(&[]), 0.0);
    }

    #[test]
    fn epistemic_rises_with_disagreement_between_signals() {
        let a = vec![0.9; 50];
        let b = vec![0.9; 50];
        let against = vec![0.1; 50];
        let agreeing = epistemic_uncertainty(&[&a, &b]);
        let split = epistemic_uncertainty(&[&a, &against]);
        assert!(
            split > agreeing,
            "disagreement {split} must exceed agreement {agreeing}"
        );
        // A maximal split reads 1.0 exactly.
        let low = vec![0.0; 50];
        let high = vec![1.0; 50];
        assert!((epistemic_uncertainty(&[&low, &high]) - 1.0).abs() < 1e-12);
    }

    #[test]
    fn epistemic_rises_when_the_evidence_is_thin() {
        let long_a = vec![0.7; 100];
        let long_b = vec![0.7; 100];
        let short_a = vec![0.7; 2];
        let short_b = vec![0.7; 2];
        let thick = epistemic_uncertainty(&[&long_a, &long_b]);
        let thin = epistemic_uncertainty(&[&short_a, &short_b]);
        assert!(thin > thick, "thin {thin} must exceed thick {thick}");
        // No evidence at all is maximal ignorance, not zero ignorance.
        assert!((epistemic_uncertainty(&[]) - 1.0).abs() < 1e-12);
    }

    /// The limitation the module doc calls load-bearing: signals that agree get a
    /// low reading whether or not they are right, so the leg is a lower bound.
    #[test]
    fn unanimous_but_wrong_signals_understate_epistemic() {
        let a = vec![1.0; 100];
        let b = vec![1.0; 100];
        let c = vec![1.0; 100];
        let confidently_wrong = epistemic_uncertainty(&[&a, &b, &c]);
        // Every one of these calls was wrong, and the leg still reads near zero.
        let outcomes = vec![false; 100];
        assert!(brier_score(&a, &outcomes) > 0.9, "the calls were all wrong");
        assert!(
            confidently_wrong < 0.02,
            "agreement alone drives the leg to {confidently_wrong}"
        );
    }

    /// Correlated streams share their errors, so they read as agreement. Perfectly
    /// duplicated signals are the limit case: the leg cannot see the duplication.
    #[test]
    fn correlated_signals_understate_epistemic() {
        let s: Vec<f64> = (0..100)
            .map(|i| 0.5 + 0.4 * (i as f64 * 0.3).sin())
            .collect();
        let duplicated = epistemic_uncertainty(&[&s, &s, &s]);
        let single = epistemic_uncertainty(&[&s]);
        assert!(
            (duplicated - single).abs() < 1e-12,
            "three copies read exactly like one: {duplicated} vs {single}"
        );
    }

    #[test]
    fn distributional_flags_a_case_that_sits_elsewhere() {
        let reference: Vec<f64> = (0..100)
            .map(|i| 0.004 + 0.0005 * (i as f64).sin())
            .collect();
        let same: Vec<f64> = (0..100)
            .map(|i| 0.004 + 0.0005 * (i as f64).cos())
            .collect();
        let shifted: Vec<f64> = reference.iter().map(|r| r - 0.02).collect();
        let familiar = distributional_uncertainty(&same, &reference);
        let novel = distributional_uncertainty(&shifted, &reference);
        assert!(
            familiar < 0.5,
            "a like-for-like case is familiar: {familiar}"
        );
        assert!(novel > 0.9, "a displaced case is novel: {novel}");
    }

    #[test]
    fn distributional_flags_a_case_that_moves_differently() {
        let reference: Vec<f64> = (0..100)
            .map(|i| 0.004 + 0.0005 * (i as f64).sin())
            .collect();
        // Same centre, ten times the dispersion.
        let wild: Vec<f64> = (0..100).map(|i| 0.004 + 0.005 * (i as f64).sin()).collect();
        assert!(
            distributional_uncertainty(&wild, &reference) > 0.8,
            "a dispersion shift is a distribution shift too"
        );
    }

    #[test]
    fn distributional_claims_nothing_without_a_reference() {
        let case = [0.01, 0.02, 0.03];
        assert_eq!(distributional_uncertainty(&case, &[]), 0.0);
        assert_eq!(distributional_uncertainty(&case, &[0.01]), 0.0);
        assert_eq!(distributional_uncertainty(&[], &case), 0.0);
    }

    #[test]
    fn distributional_handles_a_constant_reference() {
        let flat = [0.01; 10];
        assert_eq!(distributional_uncertainty(&[0.01; 10], &flat), 0.0);
        assert_eq!(distributional_uncertainty(&[0.05; 10], &flat), 1.0);
    }

    /// The whole reason for the split: a Brier score of 0.25 can mean either
    /// "nothing was knowable here" or "the agent had no idea", and the legs
    /// separate the two even though the scalar cannot.
    #[test]
    fn decomposition_separates_noise_from_ignorance() {
        let coin: Vec<bool> = (0..100).map(|i| i % 2 == 0).collect();
        let hedged = vec![0.5; 100];
        let reference: Vec<f64> = (0..100)
            .map(|i| 0.004 + 0.0005 * (i as f64).sin())
            .collect();

        // Case A: the outcome was a coin flip and the agent's streams agree that
        // it was. High aleatoric, low epistemic: correct humility.
        let humble = decompose_uncertainty(&coin, &[&hedged, &hedged], &reference, &reference);
        // Case B: same Brier score, but the streams contradict each other, so the
        // 0.5 is an average of two opposite convictions rather than a judgement.
        let sure_lo = vec![0.0; 100];
        let sure_hi = vec![1.0; 100];
        let confused = decompose_uncertainty(&coin, &[&sure_lo, &sure_hi], &reference, &reference);

        assert!((brier_score(&hedged, &coin) - 0.25).abs() < 1e-12);
        assert!(
            (humble.aleatoric - confused.aleatoric).abs() < 1e-12,
            "same outcome series, same irreducible noise"
        );
        assert!(
            confused.epistemic > humble.epistemic + 0.9,
            "only the epistemic leg tells the two apart: {} vs {}",
            confused.epistemic,
            humble.epistemic
        );
        assert!(
            humble.distributional < 1e-12,
            "a case compared against itself is not novel"
        );
    }
}
