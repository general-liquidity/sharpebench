//! Stress replay: a rank-neutral mitigation for a loss the scored windows never
//! contained.
//!
//! **This is a mitigation, not a closure.** A strategy that earns a small carry
//! and loses only in a crash is, on any sample that holds no crash, a
//! positive-drift track. No scorer of that sample can tell it apart from skill,
//! because the evidence that would separate them is not in the sample (the
//! peso problem: Krasker 1980, Rietz 1988). `sharpebench grader-search` reports
//! such tracks as `crash_unseen_in_sample`, and nothing computed from the scored
//! returns can refuse them.
//!
//! What this module adds is evidence from outside the sample. The harness runs
//! each admitted agent through the declared synthetic stress scenarios (the same
//! datasets `sharpebench stress` runs) and this kernel compares the agent's
//! worst loss there with the loss of holding the stressed market unlevered. The
//! result is reported beside the score, in the manner of a supervisory stress
//! test that is reported next to a risk model rather than folded into it (BCBS,
//! *Principles for sound stress testing practices and supervision*, 2009).
//!
//! **Rank-neutral.** Nothing in [`crate::composite`], the board, the seal or any
//! gate reads a [`StressReplay`]. A row whose stress loss exceeds the declared
//! limit is *marked*; its rank, eligibility and score do not move.
//!
//! **The rule.** For each scenario the stress loss is the largest peak-to-trough
//! drawdown of the agent's equity over any execution seed. The benchmark loss is
//! the same quantity for an equal-weight unlevered buy-and-hold book run through
//! the same scenario, window, seeds and costs. A scenario exceeds the limit when
//!
//! ```text
//! agent_loss > STRESS_LOSS_LIMIT * benchmark_loss
//! ```
//!
//! and the row is marked when any scenario exceeds it. The comparison is a ratio
//! of two losses on the same path, so it carries no unit and no crash size: the
//! same rule reads a 30% crash and a 3% crash. The benchmark is the diversified
//! market held without leverage, so an agent that loses more than it carried
//! leverage, concentration or short convexity into the shock. A mark is a
//! statement about these scenarios, not a verdict on skill: a concentrated
//! trend follower can be marked on the whipsaw scenario for losses that are
//! ordinary for its style.
//!
//! **What it cannot catch.** The replay sees only the shapes in the stress set.
//! In particular it misses:
//!
//! - crashes shaped unlike the stress set: a slow bleed, a crash in one asset
//!   while the others hold, a gap that skips the bar a stop would fill on, a
//!   rally against a short book, a volatility spike with no level change, or a
//!   correlation break;
//! - an agent that recognizes the stress scenarios. They are synthetic, seeded
//!   and public, so an agent can detect the path and de-risk only there;
//! - a tail exposure the agent takes only under conditions the scenarios never
//!   produce, such as a state it reaches after a long calm run;
//! - an agent whose crash loss is no larger than the unlevered market's. Such
//!   a loss is ordinary market exposure, and the rule does not mark it.
//!
//! A clean replay therefore says only that the agent survived these scenarios.

use serde::{Deserialize, Serialize};

use crate::composite::max_drawdown;

/// Schema of the per-row stress replay record.
pub const SCHEMA_VERSION: &str = "sharpebench.stress-replay.v1";

/// The declared limit: a scenario is exceeded when the agent's stress loss is
/// larger than this multiple of the unlevered market's loss on the same path.
/// Fixed before the first replay and never tuned to a result.
pub const STRESS_LOSS_LIMIT: f64 = 1.0;

/// The label every record carries, so no reader takes it for a closure.
pub const MITIGATION_LABEL: &str =
    "mitigation: synthetic stress replay, rank-neutral; crashes shaped unlike the stress set are not detected";

/// One scenario's comparison.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ScenarioStress {
    pub scenario: String,
    /// Largest drawdown of the agent's equity over any execution seed, as a
    /// fraction of the running peak, in [0, 1].
    pub agent_loss: f64,
    /// The same quantity for the unlevered equal-weight buy-and-hold book.
    pub benchmark_loss: f64,
    /// `agent_loss > STRESS_LOSS_LIMIT * benchmark_loss`.
    pub exceeds_limit: bool,
}

/// The stress result reported beside a row's score. Never a rank input.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum StressReplay {
    /// Every scenario replayed cleanly.
    Replayed {
        schema: String,
        label: String,
        limit: f64,
        scenarios: Vec<ScenarioStress>,
        /// Whether any scenario exceeds the limit. Marks the row, not the rank.
        marked: bool,
    },
    /// The replay could not produce a loss for every scenario. Never read as a
    /// clean replay and never coerced to a zero loss.
    Unavailable { schema: String, reason: String },
}

impl StressReplay {
    /// A typed refusal with its reason.
    pub fn unavailable(reason: impl Into<String>) -> Self {
        Self::Unavailable {
            schema: SCHEMA_VERSION.to_string(),
            reason: reason.into(),
        }
    }

    /// `Some(true)` when the row is marked, `None` when the replay is unavailable.
    pub fn marked(&self) -> Option<bool> {
        match self {
            Self::Replayed { marked, .. } => Some(*marked),
            Self::Unavailable { .. } => None,
        }
    }
}

/// One scenario's replayed return series: one series per execution seed for
/// the agent and for the benchmark.
#[derive(Clone, Debug)]
pub struct ScenarioRuns {
    pub scenario: String,
    pub agent: Vec<Vec<f64>>,
    pub benchmark: Vec<Vec<f64>>,
}

/// The worst drawdown across `runs`, capped at a total loss of 1, or `None`
/// when there is no run, a run is empty, or a return is not finite. A return at
/// or below -100% is ruin and counts as a total loss, never as unavailable.
pub fn stress_loss(runs: &[Vec<f64>]) -> Option<f64> {
    if runs.is_empty() {
        return None;
    }
    let mut worst: f64 = 0.0;
    // inv: `worst` is the largest drawdown among the runs already read, and every
    // run read so far was non-empty with finite returns.
    for run in runs {
        if run.is_empty() || run.iter().any(|r| !r.is_finite()) {
            return None;
        }
        worst = worst.max(max_drawdown(run).min(1.0));
    }
    Some(worst)
}

/// Compare every scenario's agent loss with its benchmark loss under
/// [`STRESS_LOSS_LIMIT`]. Any scenario without a usable loss on either side
/// makes the whole record unavailable: a partial replay is not a clean one.
pub fn assess(scenarios: &[ScenarioRuns]) -> StressReplay {
    if scenarios.is_empty() {
        return StressReplay::unavailable("no stress scenario was replayed");
    }
    let mut out = Vec::with_capacity(scenarios.len());
    for s in scenarios {
        let Some(agent_loss) = stress_loss(&s.agent) else {
            return StressReplay::unavailable(format!(
                "scenario `{}`: the agent's replay produced no usable return series",
                s.scenario
            ));
        };
        let Some(benchmark_loss) = stress_loss(&s.benchmark) else {
            return StressReplay::unavailable(format!(
                "scenario `{}`: the benchmark replay produced no usable return series",
                s.scenario
            ));
        };
        out.push(ScenarioStress {
            scenario: s.scenario.clone(),
            agent_loss,
            benchmark_loss,
            exceeds_limit: agent_loss > STRESS_LOSS_LIMIT * benchmark_loss,
        });
    }
    let marked = out.iter().any(|s| s.exceeds_limit);
    StressReplay::Replayed {
        schema: SCHEMA_VERSION.to_string(),
        label: MITIGATION_LABEL.to_string(),
        limit: STRESS_LOSS_LIMIT,
        scenarios: out,
        marked,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn runs(name: &str, agent: Vec<Vec<f64>>, benchmark: Vec<Vec<f64>>) -> ScenarioRuns {
        ScenarioRuns {
            scenario: name.to_string(),
            agent,
            benchmark,
        }
    }

    /// A 30% one-bar crash after a calm run, held unlevered and at 2x.
    fn crash_path(leverage: f64) -> Vec<f64> {
        let mut r = vec![0.001 * leverage; 40];
        r.push(-0.30 * leverage);
        r.extend(vec![0.0; 10]);
        r
    }

    #[test]
    fn levered_crash_loss_marks_the_row_and_unlevered_does_not() {
        let levered = assess(&[runs(
            "flash_crash",
            vec![crash_path(2.0)],
            vec![crash_path(1.0)],
        )]);
        assert_eq!(levered.marked(), Some(true), "{levered:?}");
        let unlevered = assess(&[runs(
            "flash_crash",
            vec![crash_path(1.0)],
            vec![crash_path(1.0)],
        )]);
        assert_eq!(unlevered.marked(), Some(false), "{unlevered:?}");
    }

    #[test]
    fn the_limit_is_scale_free_in_crash_size() {
        // The same 2x book against a 3% crash instead of a 30% one is marked too.
        let small = |lev: f64| {
            let mut r = vec![0.0001 * lev; 40];
            r.push(-0.03 * lev);
            r
        };
        let replay = assess(&[runs("small", vec![small(2.0)], vec![small(1.0)])]);
        assert_eq!(replay.marked(), Some(true), "{replay:?}");
    }

    #[test]
    fn worst_seed_sets_the_loss() {
        let replay = assess(&[runs(
            "flash_crash",
            vec![crash_path(1.0), crash_path(2.0)],
            vec![crash_path(1.0)],
        )]);
        assert_eq!(
            replay.marked(),
            Some(true),
            "one bad seed is enough: {replay:?}"
        );
    }

    #[test]
    fn missing_or_non_finite_replay_is_unavailable_never_zero_loss() {
        for bad in [
            vec![],
            vec![vec![]],
            vec![vec![0.01, f64::NAN]],
            vec![vec![0.01, f64::INFINITY]],
        ] {
            let replay = assess(&[runs("flash_crash", bad.clone(), vec![crash_path(1.0)])]);
            assert_eq!(replay.marked(), None, "{bad:?} -> {replay:?}");
            let replay = assess(&[runs("flash_crash", vec![crash_path(1.0)], bad.clone())]);
            assert_eq!(replay.marked(), None, "benchmark {bad:?} -> {replay:?}");
        }
        assert_eq!(assess(&[]).marked(), None);
    }

    #[test]
    fn ruin_is_a_total_loss_not_an_unavailable_replay() {
        let ruin = vec![vec![0.01, -1.5, 0.2]];
        assert_eq!(stress_loss(&ruin), Some(1.0));
        let replay = assess(&[runs("flash_crash", ruin, vec![crash_path(1.0)])]);
        assert_eq!(replay.marked(), Some(true), "{replay:?}");
    }

    #[test]
    fn record_carries_the_mitigation_label() {
        let replay = assess(&[runs(
            "flash_crash",
            vec![crash_path(1.0)],
            vec![crash_path(1.0)],
        )]);
        let json = serde_json::to_value(&replay).unwrap();
        assert_eq!(json["status"], "replayed");
        assert!(json["label"].as_str().unwrap().starts_with("mitigation"));
    }
}
