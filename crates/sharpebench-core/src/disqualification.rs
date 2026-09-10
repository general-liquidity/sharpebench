//! Disqualification-reason taxonomy — a legibility layer over the composite score.
//!
//! [`crate::composite::score_agent`] already decides *whether* an agent is
//! rank-eligible, but it collapses five independent gates into a single
//! `rank_eligible` bool. When an agent is demoted a human (or a review queue) wants
//! the *reasons*, not just the verdict — did it fail pass^k, breach its mandate, or
//! is its edge indistinguishable from noise? This module reads the signals the
//! scorer already computed and names them.
//!
//! It is **pure legibility**: it changes no scoring or eligibility semantics and
//! computes nothing new — every reason is a comparison against a field the
//! [`CompositeScore`] (plus optional out-of-band decay / rediscovery evidence)
//! already carries.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::composite::{CompositeScore, ScoreConfig};
use crate::oos::OosDecayReport;
use crate::rediscovery::RediscoveryVerdict;

/// A single reason an agent was (or should be) demoted. The first five mirror the
/// original eligibility gates in [`crate::composite::score_agent`]; two more name
/// the unavailability of a statistic the scorer does gate on. The last four are
/// advisory quality flags the scorer reports but does not gate on, surfaced here so
/// they are legible alongside the hard failures. `SelectionUnavailable` is advisory
/// because the whole selection axis is: the scorer reports `selection_gap` and never
/// consults it in `rank_eligible`, so its unavailability cannot demote an agent
/// either.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FailReason {
    /// A run failed the per-run PSR bar (pass^k, mode All): the edge isn't reliable
    /// on every seed×window.
    FailedPassK,
    /// The pooled Deflated Sharpe did not clear `dsr_bar` — the edge does not
    /// survive deflation for the number of strategies tried.
    DsrBelowBar,
    /// A block-severity process violation (risk-gate bypass, ignored halt, …).
    ProcessViolation,
    /// The bootstrap edge test was not significant (`bootstrap_p >= alpha`): the
    /// return stream is statistically indistinguishable from noise.
    BootstrapInsignificant,
    /// The agent breached its trading mandate (e.g. the drawdown cap).
    MandateBreached,
    /// Deflation or its confidence interval could not be computed.
    DeflationUnavailable,
    /// The bootstrap significance test could not be computed.
    BootstrapUnavailable,
    /// Advisory: candidate-selection statistics could not be computed, so the
    /// selection-gap flag below could not be evaluated. The scorer does not gate
    /// on the selection axis, so this never demotes an agent.
    SelectionUnavailable,
    /// Advisory: a large best-minus-median candidate gap — the headline result
    /// looks like a lucky pick from a family of tried strategies, not a robust edge.
    HighSelectionGap,
    /// Advisory: the pooled return stream is a near-duplicate of a known prior
    /// strategy (recycling, not novelty).
    IsRediscovery,
    /// Advisory: the edge decays out of sample — little of the in-sample metric is
    /// retained in later windows.
    OosDecay,
}

impl FailReason {
    /// Whether this reason is advisory: the scorer reports the signal but never
    /// consults it in `rank_eligible`, so an agent carrying only advisory
    /// reasons was still ranked. The complement mirrors a hard eligibility gate
    /// in [`crate::composite::score_agent`], which is what makes
    /// `rank_eligible == reasons.iter().all(FailReason::is_advisory)` hold for
    /// every score the scorer produced.
    pub const fn is_advisory(self) -> bool {
        matches!(
            self,
            Self::SelectionUnavailable
                | Self::HighSelectionGap
                | Self::IsRediscovery
                | Self::OosDecay
        )
    }
}

/// Thresholds for the disqualification classifier. The eligibility-gate bars mirror
/// a [`ScoreConfig`] (so the taxonomy agrees with the scorer that produced the
/// score); the advisory bars are tunable review-queue knobs.
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct DisqualThresholds {
    /// Deflated-Sharpe bar (mirror of `ScoreConfig::dsr_bar`).
    pub dsr_bar: f64,
    /// Bootstrap significance level (mirror of `ScoreConfig::alpha`).
    pub alpha: f64,
    /// Selection gap above which the headline is flagged as a lucky pick.
    pub selection_gap_max: f64,
    /// Out-of-sample retention below which the edge is flagged as decayed.
    pub oos_retention_min: f64,
}

impl Default for DisqualThresholds {
    fn default() -> Self {
        Self {
            dsr_bar: 0.95,
            alpha: 0.05,
            selection_gap_max: 0.20,
            oos_retention_min: 0.50,
        }
    }
}

impl DisqualThresholds {
    /// Take the eligibility-gate bars from a [`ScoreConfig`] so the taxonomy uses
    /// exactly the bars the scorer applied; advisory bars keep their defaults.
    pub fn from_score_config(cfg: &ScoreConfig) -> Self {
        Self {
            dsr_bar: cfg.dsr_bar,
            alpha: cfg.alpha,
            ..Self::default()
        }
    }
}

/// Classify every disqualification/quality signal that fired for one agent.
///
/// Returns the reasons in a stable order (the enum's declaration order). An empty
/// result means no signal fired — the agent cleared every hard gate and tripped no
/// advisory flag given the evidence provided. `oos` / `rediscovery` are optional:
/// when `None`, the corresponding advisory reason is simply never emitted.
pub fn classify_disqualification(
    score: &CompositeScore,
    thresholds: &DisqualThresholds,
    oos: Option<&OosDecayReport>,
    rediscovery: Option<&RediscoveryVerdict>,
) -> Vec<FailReason> {
    let mut reasons = Vec::new();

    // Hard eligibility gates (order matches the enum / the scorer's AND chain).
    if !score.passed_k {
        reasons.push(FailReason::FailedPassK);
    }
    if score.deflated_sharpe < thresholds.dsr_bar {
        reasons.push(FailReason::DsrBelowBar);
    }
    if !score.process_ok {
        reasons.push(FailReason::ProcessViolation);
    }
    if score.bootstrap_p >= thresholds.alpha {
        reasons.push(FailReason::BootstrapInsignificant);
    }
    if !score.mandate_ok {
        reasons.push(FailReason::MandateBreached);
    }
    if score.deflation_error.is_some() {
        reasons.push(FailReason::DeflationUnavailable);
    }
    if score.bootstrap_error.is_some() {
        reasons.push(FailReason::BootstrapUnavailable);
    }

    // Advisory quality flags (reported by the scorer / supplied out of band).
    // `SelectionUnavailable` belongs here, not above: the scorer never gates on
    // the selection axis, so the enum order still places it before the gap flag
    // it would otherwise have produced.
    if score.selection_error.is_some() {
        reasons.push(FailReason::SelectionUnavailable);
    }
    if score
        .selection_gap
        .is_some_and(|g| g > thresholds.selection_gap_max)
    {
        reasons.push(FailReason::HighSelectionGap);
    }
    if rediscovery.is_some_and(|v| v.is_rediscovery) {
        reasons.push(FailReason::IsRediscovery);
    }
    if oos.is_some_and(|r| r.retention < thresholds.oos_retention_min) {
        reasons.push(FailReason::OosDecay);
    }

    reasons
}

/// Suite-level rollup: how many agents in a scored field tripped each reason.
///
/// `thresholds` must be the bars the field was actually scored under, which for the
/// gate reasons means [`DisqualThresholds::from_score_config`] of the same
/// [`ScoreConfig`] that produced these scores. A rollup is a legibility layer over a
/// board, and one taken at default bars against a board scored at other bars
/// explains it wrongly: it can count `DsrBelowBar` against agents the board ranked,
/// or count none against agents the board demoted. Takes no out-of-band evidence, so
/// it covers the signals intrinsic to a [`CompositeScore`] (the hard gates plus the
/// advisory selection signals, which are counted but never gated on, so a nonzero
/// `SelectionUnavailable` or `HighSelectionGap` count does not describe agents the
/// board demoted). A [`BTreeMap`] keeps the output ordering deterministic.
pub fn rollup(
    scores: &[CompositeScore],
    thresholds: &DisqualThresholds,
) -> BTreeMap<FailReason, usize> {
    let mut counts: BTreeMap<FailReason, usize> = BTreeMap::new();
    for s in scores {
        for reason in classify_disqualification(s, thresholds, None, None) {
            *counts.entry(reason).or_insert(0) += 1;
        }
    }
    counts
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::composite::{score_agent, AgentSubmission, Run};
    use crate::process::{ProcessEvent, Trace};

    fn run(mean_ret: f64, amp: f64, n: usize) -> Run {
        Run {
            returns: (0..n)
                .map(|i| mean_ret + amp * (i as f64 * 0.7).sin())
                .collect(),
            trace: Trace::default(),
            confidences: Vec::new(),
            outcomes: Vec::new(),
            cost: 0.0,
        }
    }

    fn agent(id: &str, runs: Vec<Run>) -> AgentSubmission {
        AgentSubmission {
            agent_id: id.to_string(),
            runs,
            in_sample_trials: 0,
            candidates: Vec::new(),
        }
    }

    fn thresholds() -> DisqualThresholds {
        DisqualThresholds::from_score_config(&ScoreConfig::default())
    }

    #[test]
    fn skilled_agent_has_no_reasons() {
        let s = score_agent(
            &agent("skilled", (0..5).map(|_| run(0.002, 0.0005, 60)).collect()),
            &ScoreConfig::default(),
        );
        assert!(s.rank_eligible);
        assert!(classify_disqualification(&s, &thresholds(), None, None).is_empty());
    }

    /// The taxonomy must agree with the verdict it explains: an agent the
    /// scorer ranked may carry only advisory reasons, and any hard reason must
    /// come with `rank_eligible == false`. Asserting the two separately (as this
    /// test once did) lets a reason be published as a hard disqualification of an
    /// agent the board ranked.
    #[test]
    fn statistical_unavailability_has_named_disqualification_reasons() {
        let cfg = ScoreConfig {
            dsr_ci_level: 1.5,
            ..ScoreConfig::default()
        };
        let score = score_agent(&agent("strong", vec![run(0.002, 0.0005, 60)]), &cfg);
        assert!(score.deflation_error.is_some());
        assert!(!score.rank_eligible);
        let reasons = classify_disqualification(
            &score,
            &DisqualThresholds::from_score_config(&cfg),
            None,
            None,
        );
        assert!(serde_json::to_string(&reasons)
            .unwrap()
            .contains("deflation_unavailable"));

        for (error, label) in [
            ("deflation", "deflation_unavailable"),
            ("bootstrap", "bootstrap_unavailable"),
            ("selection", "selection_unavailable"),
        ] {
            let mut score = score_agent(
                &agent("strong", vec![run(0.002, 0.0005, 60)]),
                &ScoreConfig::default(),
            );
            assert!(score.rank_eligible);
            match error {
                "deflation" => score.deflation_error = Some("test unavailable".into()),
                "bootstrap" => score.bootstrap_error = Some("test unavailable".into()),
                _ => score.selection_error = Some("test unavailable".into()),
            }
            let reasons = classify_disqualification(&score, &thresholds(), None, None);
            assert_eq!(
                serde_json::to_value(&reasons).unwrap(),
                serde_json::json!([label])
            );
            assert_eq!(
                serde_json::to_value(rollup(&[score], &thresholds())).unwrap(),
                serde_json::json!({label: 1})
            );
            // Deflation and bootstrap are gates in `score_agent`; selection is
            // not, so only the first two may be published as hard reasons.
            assert_eq!(
                reasons[0].is_advisory(),
                error == "selection",
                "{label} is labelled against the scorer's own gate chain"
            );
        }
    }

    /// F-A reproduction: a submission whose declared candidate set refuses to
    /// deflate keeps a normal eligible track. `score_agent` gates on the
    /// bootstrap and deflation errors and deliberately not on `selection_error`,
    /// so the agent is ranked and the reason naming that refusal must not read
    /// as a disqualification.
    #[test]
    fn a_refusing_candidate_set_does_not_disqualify_a_ranked_agent() {
        let mut sub = agent("strong", vec![run(0.002, 0.0005, 60)]);
        sub.candidates = vec![vec![1e308, 1e308, 1e308]];
        let score = score_agent(&sub, &ScoreConfig::default());
        assert!(score.selection_error.is_some());
        assert!(score.rank_eligible, "the scorer ranks this agent");
        let reasons = classify_disqualification(&score, &thresholds(), None, None);
        assert_eq!(
            serde_json::to_value(&reasons).unwrap(),
            serde_json::json!(["selection_unavailable"])
        );
        assert!(
            reasons.iter().all(|r| r.is_advisory()),
            "a ranked agent may carry only advisory reasons: {reasons:?}"
        );
        assert_eq!(
            score.rank_eligible,
            reasons.iter().all(|r| r.is_advisory()),
            "the verdict and its explanation must agree"
        );
    }

    #[test]
    fn lucky_agent_fails_pass_k() {
        let mut runs = vec![run(0.02, 0.002, 60)];
        runs.extend((0..4).map(|_| run(0.0, 0.003, 60)));
        let s = score_agent(&agent("lucky", runs), &ScoreConfig::default());
        let reasons = classify_disqualification(&s, &thresholds(), None, None);
        assert!(reasons.contains(&FailReason::FailedPassK), "{reasons:?}");
    }

    #[test]
    fn process_violation_is_named() {
        let mut runs: Vec<Run> = (0..5).map(|_| run(0.002, 0.0005, 60)).collect();
        runs[0].trace.events.push(ProcessEvent::OrderPlaced {
            risk_gate_passed: false,
        });
        let s = score_agent(&agent("violator", runs), &ScoreConfig::default());
        let reasons = classify_disqualification(&s, &thresholds(), None, None);
        assert!(
            reasons.contains(&FailReason::ProcessViolation),
            "{reasons:?}"
        );
    }

    #[test]
    fn noise_agent_fails_dsr_and_bootstrap() {
        // A zero-drift, noisy agent: no real edge, so it can't clear the DSR bar and
        // its bootstrap edge test is insignificant.
        let s = score_agent(
            &agent("noise", (0..5).map(|_| run(0.0, 0.02, 60)).collect()),
            &ScoreConfig::default(),
        );
        let reasons = classify_disqualification(&s, &thresholds(), None, None);
        assert!(reasons.contains(&FailReason::DsrBelowBar), "{reasons:?}");
        assert!(
            reasons.contains(&FailReason::BootstrapInsignificant),
            "{reasons:?}"
        );
    }

    #[test]
    fn advisory_flags_require_evidence() {
        let mut s = score_agent(
            &agent("s", (0..5).map(|_| run(0.002, 0.0005, 60)).collect()),
            &ScoreConfig::default(),
        );
        // Selection gap is on the score itself; inject a large one.
        s.selection_gap = Some(0.5);
        let oos = OosDecayReport {
            window_metrics: vec![1.0, 0.1],
            in_sample: 1.0,
            out_of_sample: 0.1,
            retention: 0.1,
            monotone_decay: true,
        };
        let redisc = RediscoveryVerdict {
            is_rediscovery: true,
            max_similarity: 0.99,
            nearest_index: Some(0),
            threshold: 0.97,
        };
        let reasons = classify_disqualification(&s, &thresholds(), Some(&oos), Some(&redisc));
        assert!(
            reasons.contains(&FailReason::HighSelectionGap),
            "{reasons:?}"
        );
        assert!(reasons.contains(&FailReason::OosDecay), "{reasons:?}");
        assert!(reasons.contains(&FailReason::IsRediscovery), "{reasons:?}");

        // Without the out-of-band evidence, the advisory reasons vanish (but the
        // score-intrinsic selection gap stays).
        let bare = classify_disqualification(&s, &thresholds(), None, None);
        assert!(bare.contains(&FailReason::HighSelectionGap));
        assert!(!bare.contains(&FailReason::OosDecay));
        assert!(!bare.contains(&FailReason::IsRediscovery));
    }

    #[test]
    fn rollup_counts_across_the_field() {
        let skilled = score_agent(
            &agent("skilled", (0..5).map(|_| run(0.002, 0.0005, 60)).collect()),
            &ScoreConfig::default(),
        );
        let noise1 = score_agent(
            &agent("noise1", (0..5).map(|_| run(0.0, 0.02, 60)).collect()),
            &ScoreConfig::default(),
        );
        let noise2 = score_agent(
            &agent("noise2", (0..5).map(|_| run(0.0, 0.02, 60)).collect()),
            &ScoreConfig::default(),
        );
        let counts = rollup(
            &[skilled, noise1, noise2],
            &DisqualThresholds::from_score_config(&ScoreConfig::default()),
        );
        assert_eq!(counts.get(&FailReason::DsrBelowBar), Some(&2));
        assert_eq!(counts.get(&FailReason::BootstrapInsignificant), Some(&2));
        // The skilled agent contributes no reasons, so no key counts all three.
        assert!(counts.values().all(|&c| c <= 2));
    }

    #[test]
    fn rollup_uses_the_board_s_own_bars_not_the_default_ones() {
        // A board scored with a lowered DSR bar ranks an agent the default bar would
        // demote. The rollup explaining that board has to agree with it.
        let cfg = ScoreConfig {
            dsr_bar: 0.10,
            ..ScoreConfig::default()
        };
        let scored = score_agent(
            &agent("modest", (0..5).map(|_| run(0.0012, 0.02, 60)).collect()),
            &cfg,
        );
        assert!(
            scored.deflated_sharpe >= cfg.dsr_bar
                && scored.deflated_sharpe < DisqualThresholds::default().dsr_bar,
            "fixture must sit between the two bars: dsr={}",
            scored.deflated_sharpe
        );

        let configured = rollup(
            std::slice::from_ref(&scored),
            &DisqualThresholds::from_score_config(&cfg),
        );
        assert_eq!(
            configured.get(&FailReason::DsrBelowBar),
            None,
            "the board cleared this agent, so its explanation must not demote it"
        );

        let defaulted = rollup(&[scored], &DisqualThresholds::default());
        assert_eq!(
            defaulted.get(&FailReason::DsrBelowBar),
            Some(&1),
            "at the default bar the same score does trip the reason"
        );
    }
}
