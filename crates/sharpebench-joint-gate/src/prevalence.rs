//! The Rogan-Gladen reading of an observed eligible share.
//!
//! A board reports how many of its tracks the joint gate admitted. That share
//! mixes skilled tracks the gate caught with zero-skill tracks it let through.
//! This study measures both rates: a primary power cell's focal eligibility is
//! the gate's sensitivity at that cell's effect and history, and a primary null
//! cell's focal eligibility is its false-eligibility rate (one minus
//! specificity). [`corrected_prevalence`] puts an observed count beside those
//! two cells and reports the share of truly skilled tracks it implies, with the
//! clamping rule and an interval that carries all three measurements, through
//! [`sharpebench_stats::prevalence`].
//!
//! The reading is a diagnostic beside the report, never part of [`crate::report`]'s
//! predeclared outcome, and it moves no rank. Its premise is the study's own:
//! the corrected share is the share of tracks whose skill matches the power
//! cell's effect, so a field whose skilled tracks are weaker than that effect
//! is undercounted. Read it as the share of skill at least that strong.

use serde::Serialize;

use sharpebench_stats::prevalence::{rogan_gladen, rogan_gladen_bounds, Clamp};
use sharpebench_study::{wilson_interval, Interval};

use crate::campaign::{BlockRole, Campaign};
use crate::report::{CellEstimate, Report, LEVEL};
use crate::shards::Tier;

/// Each of the three input intervals is two-sided at [`LEVEL`] (99 percent), so
/// the box of all three covers its true values with probability at least
/// `1 - 3 * 0.01` by the union bound, and so does the corrected interval.
pub const JOINT_COVERAGE_FLOOR: f64 = 0.97;

/// One measured rate with its interval.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Rate {
    pub events: u64,
    pub trials: u64,
    pub rate: f64,
    pub interval: Interval,
}

/// The corrected share of skilled tracks behind an observed eligible count.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct PrevalenceReading {
    /// The tier the two cells came from. On a smoke run the rates are not
    /// estimates of anything, and neither is this reading.
    pub tier: Tier,
    pub rule: String,
    pub history_bars: usize,
    /// The annual effect the power cell planted: the skill this reading counts.
    pub effect_annual: f64,
    pub power_cell: usize,
    pub null_cell: usize,
    /// The observed eligible share on the board being read.
    pub apparent: Rate,
    /// The power cell's focal eligibility.
    pub sensitivity: Rate,
    /// The null cell's focal eligibility, one minus specificity.
    pub false_eligibility: Rate,
    /// Youden's index, `sensitivity + specificity - 1`; positive, or the
    /// reading is refused.
    pub youden: f64,
    /// `(apparent - false_eligibility) / (sensitivity - false_eligibility)`.
    pub raw: f64,
    /// `raw` clamped to `[0, 1]`.
    pub estimate: f64,
    /// `none`, `zero` (fewer admitted than false eligibility alone predicts) or
    /// `one` (more admitted than the power predicts).
    pub clamped_to: &'static str,
    pub interval: Interval,
    /// False when the measured intervals reach a gate with sensitivity +
    /// specificity <= 1, where the interval is the whole of `[0, 1]`.
    pub interval_informative: bool,
    pub joint_coverage_floor: f64,
    pub rank_neutral: bool,
}

fn rate(events: u64, trials: u64, what: &str) -> Result<Rate, String> {
    let interval = wilson_interval(events, trials, LEVEL).map_err(|e| format!("{what}: {e}"))?;
    Ok(Rate {
        events,
        trials,
        rate: events as f64 / trials as f64,
        interval,
    })
}

fn cell(report: &Report, index: usize) -> Result<&CellEstimate, String> {
    report
        .cells
        .iter()
        .find(|c| c.cell == index)
        .ok_or_else(|| format!("cell {index} is not in the report"))
}

/// The Rogan-Gladen corrected share of skilled tracks for `eligible` admitted
/// out of `tracks`, against the sensitivity of report cell `power_cell` and the
/// false-eligibility rate of report cell `null_cell`.
///
/// Refused when either cell is missing, is not a primary power or primary null
/// cell of `campaign` respectively, when the two cells score different rules or
/// histories, when the count is empty or impossible, or when the measured
/// sensitivity does not exceed the measured false-eligibility rate
/// (sensitivity + specificity <= 1).
pub fn corrected_prevalence(
    campaign: &Campaign,
    report: &Report,
    power_cell: usize,
    null_cell: usize,
    eligible: u64,
    tracks: u64,
) -> Result<PrevalenceReading, String> {
    let role = |c: &CellEstimate| {
        campaign
            .statistical
            .iter()
            .find(|b| b.id == c.block)
            .map(|b| b.role)
    };
    let power = cell(report, power_cell)?;
    let null = cell(report, null_cell)?;
    if role(power) != Some(BlockRole::PrimaryPower) {
        return Err(format!("cell {power_cell} is not a primary power cell"));
    }
    if role(null) != Some(BlockRole::PrimaryNull) {
        return Err(format!("cell {null_cell} is not a primary null cell"));
    }
    if power.rule != null.rule || power.history_bars != null.history_bars {
        return Err(format!(
            "cells {power_cell} and {null_cell} score different rules or histories: \
             sensitivity and specificity must describe the same gate"
        ));
    }
    let apparent = rate(eligible, tracks, "observed eligible count")?;
    let sensitivity = rate(power.focal_eligible, power.fields, "power cell")?;
    let false_eligibility = rate(null.focal_eligible, null.fields, "null cell")?;
    let point = rogan_gladen(apparent.rate, sensitivity.rate, false_eligibility.rate)
        .map_err(|e| e.to_string())?;
    let bounds = rogan_gladen_bounds(
        (apparent.interval.lower, apparent.interval.upper),
        (sensitivity.interval.lower, sensitivity.interval.upper),
        (
            false_eligibility.interval.lower,
            false_eligibility.interval.upper,
        ),
    )
    .map_err(|e| e.to_string())?;
    Ok(PrevalenceReading {
        tier: report.tier,
        rule: power.rule.clone(),
        history_bars: power.history_bars,
        effect_annual: power.effect_annual,
        power_cell,
        null_cell,
        youden: sensitivity.rate - false_eligibility.rate,
        raw: point.raw,
        estimate: point.estimate,
        clamped_to: match point.clamp {
            Clamp::None => "none",
            Clamp::Zero => "zero",
            Clamp::One => "one",
        },
        interval: Interval {
            lower: bounds.lower,
            upper: bounds.upper,
        },
        interval_informative: bounds.informative,
        joint_coverage_floor: JOINT_COVERAGE_FLOOR,
        rank_neutral: true,
        apparent,
        sensitivity,
        false_eligibility,
    })
}
