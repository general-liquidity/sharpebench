//! The headline verdict: one word per board row, with the facts that qualify it
//! listed beside it.
//!
//! A reader of a scored row otherwise joins a dozen fields, two configuration
//! bars that are not on the row, and metadata attached after the seal, before
//! they can say what the row shows. [`headline`] does that join once, over the
//! reasons [`classify_disqualification`] already names, and returns a separate
//! object. It is never a [`CompositeScore`] field, so it changes no score bytes,
//! no rank, no ordinal and no hash of the signed chain.
//!
//! The verdict is the first rule that matches, in this order:
//!
//! 1. [`Verdict::Refused`]: no scored row exists (an input or intake refusal).
//! 2. [`Verdict::Unavailable`]: a statistic the gates need could not be
//!    computed: the bootstrap, the deflation family for a numerical or
//!    configuration reason, or the exposure-path statistic.
//! 3. [`Verdict::DegenerateTrack`]: the deflation family refused the track's
//!    shape ([`FailReason::DegenerateTrack`]), so the track has no Sharpe ratio.
//! 4. [`Verdict::Disqualified`]: a conduct gate failed (a block-severity process
//!    violation or a breached mandate), whatever the statistics say.
//! 5. [`Verdict::SkillSupported`]: the row is rank-eligible.
//! 6. [`Verdict::CannotTell`]: pass^k is the only hard reason, and its floors
//!    are out of practical reach on a window it needs, so a NO is expected of
//!    almost any track.
//! 7. [`Verdict::NoEvidenceOfSkill`]: any other ineligible row.
//!
//! There is no "luck" verdict. Failing to reject the no-skill null does not show
//! that a track was lucky, so the last rule says only that the evidence for skill
//! is absent.
//!
//! Qualifiers ([`Qualifiers`]) never change the verdict: the evidence mode, the
//! tie band, the stress mark, the advisory reasons and the labeled second
//! verdicts are reported beside it.

use serde::{Deserialize, Serialize};

use crate::composite::CompositeScore;
use crate::disqualification::{classify_disqualification, DisqualThresholds, FailReason};
use crate::stress_replay::StressReplay;

/// Schema of the headline record.
pub const SCHEMA_VERSION: &str = "sharpebench.headline.v1";

/// The one-word reading of a row. Serialized in `snake_case`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Verdict {
    /// No scored row exists, so nothing was measured.
    Refused,
    /// A statistic a gate needs could not be computed, so the row is neither a
    /// pass nor a fail on it.
    Unavailable,
    /// The track has no Sharpe ratio: the deflation family refused its shape.
    DegenerateTrack,
    /// A conduct gate failed: a block-severity process violation or a breached
    /// mandate.
    Disqualified,
    /// Every hard gate held, so the row is rank-eligible.
    SkillSupported,
    /// pass^k is the only hard reason, and its floors are out of practical
    /// reach on a window it needs.
    CannotTell,
    /// The row is ineligible on the statistics: the no-skill null was not
    /// rejected, or the deflated Sharpe was not certified.
    NoEvidenceOfSkill,
}

impl Verdict {
    /// The serialized name, for tables.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Refused => "refused",
            Self::Unavailable => "unavailable",
            Self::DegenerateTrack => "degenerate_track",
            Self::Disqualified => "disqualified",
            Self::SkillSupported => "skill_supported",
            Self::CannotTell => "cannot_tell",
            Self::NoEvidenceOfSkill => "no_evidence_of_skill",
        }
    }
}

/// What a caller knows about the evidence behind a row. A row scored from a
/// submissions file carries no evidence mode, so it is [`Evidence::Unknown`].
///
/// "Certifying" here has the one meaning of the harness evidence mode
/// (`evidence_mode.certifying`): a forward window, or a historical window whose
/// entrant cell could not reach the network. It is not the rank-mode
/// certification ([`Qualifiers::rank_mode`]); an arena board that does not
/// certify its rows is reported by the caller as a noncertifying reason.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum Evidence {
    /// No evidence mode was recorded for the row.
    #[default]
    Unknown,
    /// The evidence certifies the row under the named mode.
    Certifying { mode: String },
    /// The evidence does not certify the row, for the listed reasons.
    Noncertifying { mode: String, reasons: Vec<String> },
}

/// The stress-replay mark beside a row. Never a rank input.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StressMark {
    /// No stress replay was run for the row.
    #[default]
    NotRun,
    /// Every scenario replayed within the declared limit.
    Clean,
    /// Some scenario exceeded the declared limit.
    Marked,
    /// The replay could not produce a loss for every scenario.
    Unavailable,
}

impl StressMark {
    /// The mark a stress-replay record carries, or [`StressMark::NotRun`].
    pub fn of(replay: Option<&StressReplay>) -> Self {
        match replay.map(StressReplay::marked) {
            None => Self::NotRun,
            Some(None) => Self::Unavailable,
            Some(Some(true)) => Self::Marked,
            Some(Some(false)) => Self::Clean,
        }
    }
}

/// Facts the scored row does not carry, supplied by the caller that attached
/// them. The default is a row with no evidence mode and no stress replay.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct HeadlineContext {
    pub evidence: Evidence,
    pub stress: StressMark,
}

/// The row a headline reads: a score, or the refusal that stands in place of
/// one.
#[derive(Clone, Copy, Debug)]
pub enum HeadlineRow<'a> {
    Scored(&'a CompositeScore),
    Refused { agent_id: &'a str, reason: &'a str },
}

/// The facts reported beside the verdict. None of them changes it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Qualifiers {
    pub evidence: Evidence,
    /// Whether the row shares its deflated-Sharpe tie band with another
    /// eligible row (`dsr_tied`).
    pub tied: bool,
    /// The row's 1-based tie band, 0 for an ineligible row (`tie_group`).
    pub tie_group: usize,
    pub stress: StressMark,
    /// The advisory reasons, which never gate.
    pub advisory: Vec<FailReason>,
    /// The declared-mandate verdict, by its existing label, when one was
    /// declared.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub declared_mandate: Option<String>,
    /// The opt-in rank-mode verdict, by its existing label, when a rank mode
    /// was applied.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rank_mode: Option<String>,
}

/// The headline of one row.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Headline {
    pub schema: String,
    pub agent_id: String,
    pub verdict: Verdict,
    /// The hard reasons, in the classifier's order. Empty exactly for
    /// [`Verdict::Refused`] and [`Verdict::SkillSupported`].
    pub hard_reasons: Vec<FailReason>,
    /// The refusal or unavailability text that decided the verdict, when one
    /// did: the input refusal, the bootstrap or deflation error, or the
    /// exposure-path error.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cause: Option<String>,
    pub qualifiers: Qualifiers,
}

/// The `exposure_path_error` the exposure-path gate refuses a track with, as
/// opposed to an exposure-path statistic that could not be computed.
fn is_exposure_path_refusal(error: &str) -> bool {
    error
        .strip_prefix("returns ")
        .is_some_and(|requirement| requirement == sharpebench_stats::EXPOSURE_PATH_UNPROVEN)
}

/// The verdict of a scored row and the error text that decided it, if any.
fn scored_verdict(score: &CompositeScore, hard: &[FailReason]) -> (Verdict, Option<String>) {
    if let Some(error) = &score.bootstrap_error {
        return (Verdict::Unavailable, Some(error.clone()));
    }
    if let Some(error) = &score.deflation_error {
        let verdict = if hard.contains(&FailReason::DegenerateTrack) {
            Verdict::DegenerateTrack
        } else {
            Verdict::Unavailable
        };
        return (verdict, Some(error.clone()));
    }
    if let Some(error) = score
        .exposure_path_error
        .as_ref()
        .filter(|error| !is_exposure_path_refusal(error))
    {
        return (Verdict::Unavailable, Some(error.clone()));
    }
    if !score.process_ok || !score.mandate_ok {
        return (Verdict::Disqualified, None);
    }
    if score.rank_eligible {
        return (Verdict::SkillSupported, None);
    }
    if hard == [FailReason::FailedPassK] && score.pass_k_floors_plausible == Some(false) {
        return (Verdict::CannotTell, None);
    }
    (Verdict::NoEvidenceOfSkill, None)
}

/// The headline of one row under the bars the board was scored with.
///
/// `thresholds` must be [`DisqualThresholds::from_score_config`] of the
/// configuration that produced the score, as for
/// [`classify_disqualification`]. For every score the scorer produced,
/// [`Verdict::SkillSupported`] holds exactly when `rank_eligible` does, and
/// every other verdict but [`Verdict::Refused`] lists at least one hard reason.
pub fn headline(
    row: HeadlineRow<'_>,
    thresholds: &DisqualThresholds,
    context: &HeadlineContext,
) -> Headline {
    let score = match row {
        HeadlineRow::Refused { agent_id, reason } => {
            return Headline {
                schema: SCHEMA_VERSION.to_string(),
                agent_id: agent_id.to_string(),
                verdict: Verdict::Refused,
                hard_reasons: Vec::new(),
                cause: Some(reason.to_string()),
                qualifiers: Qualifiers {
                    evidence: context.evidence.clone(),
                    tied: false,
                    tie_group: 0,
                    stress: context.stress,
                    advisory: Vec::new(),
                    declared_mandate: None,
                    rank_mode: None,
                },
            };
        }
        HeadlineRow::Scored(score) => score,
    };
    let (advisory, hard): (Vec<FailReason>, Vec<FailReason>) =
        classify_disqualification(score, thresholds, None, None)
            .into_iter()
            .partition(|reason| reason.is_advisory());
    let (verdict, cause) = scored_verdict(score, &hard);
    Headline {
        schema: SCHEMA_VERSION.to_string(),
        agent_id: score.agent_id.clone(),
        verdict,
        hard_reasons: hard,
        cause,
        qualifiers: Qualifiers {
            evidence: context.evidence.clone(),
            tied: score.dsr_tied,
            tie_group: score.tie_group,
            stress: context.stress,
            advisory,
            declared_mandate: score.mandate_verdict_label(),
            rank_mode: score.certification.as_ref().map(|c| c.describe()),
        },
    }
}

/// The headline of every row of a board, with no context: no evidence mode and
/// no stress replay.
pub fn board_headlines(board: &[CompositeScore], thresholds: &DisqualThresholds) -> Vec<Headline> {
    board
        .iter()
        .map(|score| {
            headline(
                HeadlineRow::Scored(score),
                thresholds,
                &HeadlineContext::default(),
            )
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::composite::{rank, score_agent, AgentSubmission, Run, ScoreConfig};
    use crate::process::{ProcessEvent, Trace};

    fn run(mean_ret: f64, amp: f64, n: usize) -> Run {
        Run {
            returns: (0..n)
                .map(|i| mean_ret + amp * (i as f64 * 0.7).sin())
                .collect(),
            ..Run::default()
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

    /// A skilled agent the scorer ranks: the base every planted row starts
    /// from, so each test changes only the fields its rule reads.
    fn eligible() -> CompositeScore {
        let s = score_agent(
            &agent("skilled", (0..5).map(|_| run(0.002, 0.004, 60)).collect()),
            &ScoreConfig::default(),
        );
        assert!(s.rank_eligible);
        s
    }

    fn verdict_of(score: &CompositeScore) -> Verdict {
        headline(
            HeadlineRow::Scored(score),
            &thresholds(),
            &HeadlineContext::default(),
        )
        .verdict
    }

    const NO_OBSERVED_LOSS_ERROR: &str = "returns must show an observed loss: no bar of any \
        window is below zero, so the sample holds no observation of the left tail and its \
        moments describe the gains only";

    /// One planted row per verdict, each meeting exactly one rule, plus the
    /// rows on which an earlier rule must win over a later one that also
    /// matches.
    #[test]
    fn each_rule_and_its_precedence_decide_the_verdict() {
        let degenerate = |s: &mut CompositeScore| {
            s.deflation_error = Some(NO_OBSERVED_LOSS_ERROR.into());
            s.deflated_sharpe = 0.0;
            s.rank_eligible = false;
        };
        let numerical = |s: &mut CompositeScore| {
            s.deflation_error = Some("dsr_ci_level must be in (0, 1)".into());
            s.rank_eligible = false;
        };
        let bootstrap = |s: &mut CompositeScore| {
            s.bootstrap_error = Some("n_boot must be positive".into());
            s.bootstrap_p = 1.0;
            s.rank_eligible = false;
        };
        let process = |s: &mut CompositeScore| {
            s.process_ok = false;
            s.rank_eligible = false;
        };
        let mandate = |s: &mut CompositeScore| {
            s.mandate_ok = false;
            s.rank_eligible = false;
        };
        let unreachable_pass_k = |s: &mut CompositeScore| {
            s.passed_k = false;
            s.pass_k_floors_plausible = Some(false);
            s.rank_eligible = false;
        };
        let reachable_pass_k = |s: &mut CompositeScore| {
            s.passed_k = false;
            s.pass_k_floors_plausible = Some(true);
            s.rank_eligible = false;
        };
        let below_bar = |s: &mut CompositeScore| {
            s.deflated_sharpe = 0.5;
            s.rank_eligible = false;
        };
        let exposure_refused = |s: &mut CompositeScore| {
            s.exposure_path_error = Some(format!(
                "returns {}",
                sharpebench_stats::EXPOSURE_PATH_UNPROVEN
            ));
            s.rank_eligible = false;
        };
        let exposure_unavailable = |s: &mut CompositeScore| {
            s.exposure_path_error = Some("forgone-risk statistic is not finite".into());
            s.rank_eligible = false;
        };
        let advisory = |s: &mut CompositeScore| s.selection_gap = Some(0.9);
        type Plant = fn(&mut CompositeScore);
        let cases: Vec<(&str, Vec<Plant>, Verdict)> = vec![
            ("clean", vec![], Verdict::SkillSupported),
            ("advisory only", vec![advisory], Verdict::SkillSupported),
            ("bootstrap", vec![bootstrap], Verdict::Unavailable),
            ("numerical deflation", vec![numerical], Verdict::Unavailable),
            (
                "exposure statistic",
                vec![exposure_unavailable],
                Verdict::Unavailable,
            ),
            ("degenerate", vec![degenerate], Verdict::DegenerateTrack),
            ("process", vec![process], Verdict::Disqualified),
            ("mandate", vec![mandate], Verdict::Disqualified),
            (
                "unreachable pass^k",
                vec![unreachable_pass_k],
                Verdict::CannotTell,
            ),
            (
                "reachable pass^k",
                vec![reachable_pass_k],
                Verdict::NoEvidenceOfSkill,
            ),
            ("below bar", vec![below_bar], Verdict::NoEvidenceOfSkill),
            (
                "exposure refused",
                vec![exposure_refused],
                Verdict::NoEvidenceOfSkill,
            ),
            // Precedence: the earlier rule wins.
            (
                "bootstrap over degenerate",
                vec![degenerate, bootstrap],
                Verdict::Unavailable,
            ),
            (
                "numerical over process",
                vec![numerical, process],
                Verdict::Unavailable,
            ),
            (
                "degenerate over process",
                vec![degenerate, process],
                Verdict::DegenerateTrack,
            ),
            (
                "exposure statistic over mandate",
                vec![exposure_unavailable, mandate],
                Verdict::Unavailable,
            ),
            (
                "process over unreachable pass^k",
                vec![process, unreachable_pass_k],
                Verdict::Disqualified,
            ),
            (
                "mandate over below bar",
                vec![mandate, below_bar],
                Verdict::Disqualified,
            ),
            (
                "exposure refusal under process",
                vec![exposure_refused, process],
                Verdict::Disqualified,
            ),
            // cannot_tell needs pass^k to be the only hard reason.
            (
                "unreachable pass^k and below bar",
                vec![unreachable_pass_k, below_bar],
                Verdict::NoEvidenceOfSkill,
            ),
            (
                "unreachable pass^k with advisory",
                vec![unreachable_pass_k, advisory],
                Verdict::CannotTell,
            ),
        ];
        for (label, plants, expected) in cases {
            let mut s = eligible();
            for plant in plants {
                plant(&mut s);
            }
            assert_eq!(verdict_of(&s), expected, "{label}");
        }
    }

    /// An archived score with no recorded plausibility cannot be read as out of
    /// reach: absent is not false.
    #[test]
    fn a_failed_pass_k_with_unrecorded_floors_is_no_evidence_of_skill() {
        let mut s = eligible();
        s.passed_k = false;
        s.pass_k_floors_plausible = None;
        s.rank_eligible = false;
        assert_eq!(verdict_of(&s), Verdict::NoEvidenceOfSkill);
    }

    /// Every exported shape refusal reads `degenerate_track`, and the cause
    /// carries the text; any other deflation error reads `unavailable`.
    #[test]
    fn each_exported_shape_refusal_is_a_degenerate_track() {
        for requirement in crate::disqualification::DEGENERATE_TRACK_REQUIREMENTS {
            let mut s = eligible();
            s.deflation_error = Some(format!("returns {requirement}"));
            s.rank_eligible = false;
            let h = headline(
                HeadlineRow::Scored(&s),
                &thresholds(),
                &HeadlineContext::default(),
            );
            assert_eq!(h.verdict, Verdict::DegenerateTrack, "{requirement}");
            assert_eq!(h.cause, s.deflation_error, "{requirement}");
            assert!(h.hard_reasons.contains(&FailReason::DegenerateTrack));
            assert!(!h.hard_reasons.contains(&FailReason::DeflationUnavailable));
        }
        for near_miss in [
            format!("periods_per_year {}", sharpebench_stats::NO_OBSERVED_LOSS),
            format!("returns {} (extra)", sharpebench_stats::NO_OBSERVED_LOSS),
            sharpebench_stats::NO_OBSERVED_LOSS.to_string(),
        ] {
            let mut s = eligible();
            s.deflation_error = Some(near_miss.clone());
            s.rank_eligible = false;
            assert_eq!(verdict_of(&s), Verdict::Unavailable, "{near_miss}");
        }
    }

    /// Rows the scorer itself produced, one per reachable verdict, so the
    /// planted fields above are the ones the scorer really sets.
    #[test]
    fn scorer_produced_rows_read_their_verdicts() {
        let cfg = ScoreConfig::default();
        let skilled = eligible();
        let gains_only = score_agent(
            &agent(
                "gains_only",
                (0..2)
                    .map(|_| Run {
                        returns: (0..120)
                            .map(|i| 0.002 + 0.001 * (i as f64 * 0.7).sin().abs())
                            .collect(),
                        ..Run::default()
                    })
                    .collect(),
            ),
            &cfg,
        );
        let noise = score_agent(&agent("noise", vec![run(0.0, 0.004, 60)]), &cfg);
        let mut violator = agent("violator", (0..5).map(|_| run(0.002, 0.004, 60)).collect());
        violator.runs[0].trace = Trace {
            events: vec![ProcessEvent::OrderPlaced {
                risk_gate_passed: false,
            }],
        };
        let violator = score_agent(&violator, &cfg);
        let broken = score_agent(
            &agent("broken", vec![run(0.002, 0.004, 60)]),
            &ScoreConfig {
                dsr_ci_level: 1.5,
                ..ScoreConfig::default()
            },
        );
        for (score, expected) in [
            (&skilled, Verdict::SkillSupported),
            (&gains_only, Verdict::DegenerateTrack),
            (&noise, Verdict::NoEvidenceOfSkill),
            (&violator, Verdict::Disqualified),
        ] {
            assert_eq!(verdict_of(score), expected, "{}", score.agent_id);
        }
        let broken_headline = headline(
            HeadlineRow::Scored(&broken),
            &DisqualThresholds::from_score_config(&ScoreConfig {
                dsr_ci_level: 1.5,
                ..ScoreConfig::default()
            }),
            &HeadlineContext::default(),
        );
        assert_eq!(broken_headline.verdict, Verdict::Unavailable);
        assert_eq!(broken_headline.cause, broken.deflation_error);
    }

    /// The invariants over a scored field: `skill_supported` exactly when the
    /// row is rank-eligible, and a hard reason on every other scored verdict.
    #[test]
    fn skill_supported_is_exactly_rank_eligible_over_a_scored_field() {
        let cfg = ScoreConfig::default();
        let mut subs = vec![
            agent("skilled", (0..5).map(|_| run(0.002, 0.004, 60)).collect()),
            agent("noise", vec![run(0.0, 0.004, 60)]),
            agent("loser", vec![run(-0.002, 0.004, 60)]),
            agent(
                "flat",
                vec![Run {
                    returns: vec![0.0; 60],
                    ..Run::default()
                }],
            ),
        ];
        let mut violator = agent("violator", (0..5).map(|_| run(0.002, 0.004, 60)).collect());
        violator.runs[0].trace = Trace {
            events: vec![ProcessEvent::OrderPlaced {
                risk_gate_passed: false,
            }],
        };
        subs.push(violator);
        let board = rank(&subs, &cfg);
        let headlines = board_headlines(&board, &thresholds());
        assert_eq!(headlines.len(), board.len());
        for (score, h) in board.iter().zip(&headlines) {
            assert_eq!(h.agent_id, score.agent_id);
            assert_eq!(
                h.verdict == Verdict::SkillSupported,
                score.rank_eligible,
                "{}",
                score.agent_id
            );
            assert_eq!(
                h.hard_reasons.is_empty(),
                h.verdict == Verdict::SkillSupported,
                "{}: {:?}",
                score.agent_id,
                h.hard_reasons
            );
        }
    }

    /// A refusal has no score, so it has no reasons and no tie band, and it
    /// keeps the caller's evidence and stress context.
    #[test]
    fn a_refused_row_reads_refused_with_its_reason() {
        let context = HeadlineContext {
            evidence: Evidence::Noncertifying {
                mode: "backtest".into(),
                reasons: vec!["supplied returns".into()],
            },
            stress: StressMark::Unavailable,
        };
        let h = headline(
            HeadlineRow::Refused {
                agent_id: "late",
                reason: "submission arrived after the window closed",
            },
            &thresholds(),
            &context,
        );
        assert_eq!(h.verdict, Verdict::Refused);
        assert_eq!(h.agent_id, "late");
        assert!(h.hard_reasons.is_empty());
        assert_eq!(
            h.cause.as_deref(),
            Some("submission arrived after the window closed")
        );
        assert_eq!(h.qualifiers.evidence, context.evidence);
        assert_eq!(h.qualifiers.stress, StressMark::Unavailable);
        assert!(!h.qualifiers.tied && h.qualifiers.tie_group == 0);
    }

    /// Qualifiers are reported beside the verdict and never move it: the same
    /// row reads the same verdict under every evidence and stress context, tied
    /// or not.
    #[test]
    fn qualifiers_never_change_the_verdict() {
        let contexts = [
            HeadlineContext::default(),
            HeadlineContext {
                evidence: Evidence::Certifying {
                    mode: "forward".into(),
                },
                stress: StressMark::Clean,
            },
            HeadlineContext {
                evidence: Evidence::Noncertifying {
                    mode: "backtest".into(),
                    reasons: vec!["network reachable".into()],
                },
                stress: StressMark::Marked,
            },
        ];
        let mut tied = eligible();
        tied.dsr_tied = true;
        tied.tie_group = 2;
        let mut noise = eligible();
        noise.deflated_sharpe = 0.1;
        noise.rank_eligible = false;
        for score in [eligible(), tied.clone(), noise] {
            let base = verdict_of(&score);
            for context in &contexts {
                let h = headline(HeadlineRow::Scored(&score), &thresholds(), context);
                assert_eq!(h.verdict, base);
                assert_eq!(h.qualifiers.evidence, context.evidence);
                assert_eq!(h.qualifiers.stress, context.stress);
                assert_eq!(h.qualifiers.tied, score.dsr_tied);
                assert_eq!(h.qualifiers.tie_group, score.tie_group);
            }
        }
        let h = headline(
            HeadlineRow::Scored(&tied),
            &thresholds(),
            &HeadlineContext::default(),
        );
        assert!(h.qualifiers.tied);
        assert_eq!(h.qualifiers.tie_group, 2);
    }

    /// Advisory reasons are listed as qualifiers, never as hard reasons; the
    /// labeled second verdicts are echoed by their existing labels.
    #[test]
    fn advisory_reasons_and_second_verdicts_are_listed_beside_the_verdict() {
        let mut s = eligible();
        s.selection_error = Some("no comparable candidate".into());
        s.selection_gap = Some(0.9);
        let h = headline(
            HeadlineRow::Scored(&s),
            &thresholds(),
            &HeadlineContext::default(),
        );
        assert_eq!(h.verdict, Verdict::SkillSupported);
        assert!(h.hard_reasons.is_empty());
        assert_eq!(
            h.qualifiers.advisory,
            vec![
                FailReason::SelectionUnavailable,
                FailReason::HighSelectionGap
            ]
        );
        assert_eq!(h.qualifiers.declared_mandate, None);
        assert_eq!(h.qualifiers.rank_mode, None);
        let json = serde_json::to_value(&h).unwrap();
        assert!(json["qualifiers"].get("declared_mandate").is_none());
        assert!(json["qualifiers"].get("rank_mode").is_none());
        assert_eq!(json["verdict"], "skill_supported");
        assert_eq!(json["schema"], SCHEMA_VERSION);
        assert_eq!(json["qualifiers"]["evidence"]["status"], "unknown");
        assert_eq!(json["qualifiers"]["stress"], "not_run");
    }

    #[test]
    fn the_stress_mark_reads_the_replay_record() {
        assert_eq!(StressMark::of(None), StressMark::NotRun);
        assert_eq!(
            StressMark::of(Some(&StressReplay::unavailable("no path"))),
            StressMark::Unavailable
        );
        for (marked, expected) in [(true, StressMark::Marked), (false, StressMark::Clean)] {
            let replay = StressReplay::Replayed {
                schema: crate::stress_replay::SCHEMA_VERSION.into(),
                label: crate::stress_replay::MITIGATION_LABEL.into(),
                limit: 1.0,
                scenarios: Vec::new(),
                marked,
            };
            assert_eq!(StressMark::of(Some(&replay)), expected);
        }
    }

    #[test]
    fn verdict_names_match_their_serialized_form() {
        for verdict in [
            Verdict::Refused,
            Verdict::Unavailable,
            Verdict::DegenerateTrack,
            Verdict::Disqualified,
            Verdict::SkillSupported,
            Verdict::CannotTell,
            Verdict::NoEvidenceOfSkill,
        ] {
            assert_eq!(
                serde_json::to_value(verdict).unwrap(),
                serde_json::json!(verdict.as_str())
            );
        }
    }
}
