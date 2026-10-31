//! The predeclared reading of a finished run.
//!
//! Every interval is the study crate's Wilson interval over the realized count,
//! and the outcome is a function of those intervals and the campaign's decision
//! plan. There is no parameter here to tune, and no path by which a smoke run's
//! counts could be read as a verdict: [`Outcome`] carries the tier it came from.

use std::collections::BTreeMap;

use serde::Serialize;

use sharpebench_study::protocol::ConfidenceLevel;
use sharpebench_study::{wilson_interval, Interval};

use crate::campaign::{BlockRole, Campaign, ControlKind};
use crate::controls::ControlRow;
use crate::fields::{Cell, Row};
use crate::shards::{cell_table, Tier};

/// The two-sided level of every reported interval. With Bonferroni across at
/// most ten rules, each one-sided bound at 0.005 keeps the family at 0.05.
pub const LEVEL: ConfidenceLevel = ConfidenceLevel::NinetyNine;

/// The statistical estimates of one cell.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct CellEstimate {
    pub cell: usize,
    pub block: String,
    pub dgp: String,
    pub history_bars: usize,
    pub effect_annual: f64,
    pub rule: String,
    pub block_prob: f64,
    pub fields: u64,
    pub focal_eligible: u64,
    pub any_false: u64,
    pub unavailable_entrants: u64,
    pub focal_interval: Interval,
    pub any_false_interval: Interval,
    pub legs_dsr: u64,
    pub legs_reliability: u64,
    pub legs_bootstrap: u64,
    pub mean_bar_annual: f64,
}

/// One control's detection rate.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ControlEstimate {
    pub kind: ControlKind,
    pub replicates: u64,
    pub detected: u64,
    pub statistics_admitted: u64,
    pub twin_ok: u64,
    pub detection_interval: Interval,
    pub mean_measure: f64,
}

/// What the study concludes about one rule.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct RuleFinding {
    pub rule: String,
    /// Every primary null cell's whole-field upper bound is at or below the limit.
    pub size_controlled: bool,
    pub worst_null_upper: f64,
    pub worst_null_dgp: String,
    /// Target `(effect, bars)` pairs this rule reached in every primary cell.
    pub targets_reached: Vec<(f64, usize)>,
    pub feasible: bool,
}

/// The stated outcome. Exactly one of these, chosen by the rules in
/// [`decide`], and each of them is a result.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum Outcome {
    /// At least one candidate keeps size everywhere and reaches a target.
    FeasibleRule { rules: Vec<String> },
    /// At least one candidate keeps size, none reaches any target: no feasible
    /// geometry among the candidates at the stated history.
    NoFeasibleGeometry { size_controlled_rules: Vec<String> },
    /// No candidate keeps size across the null grid.
    SizeNotControlled,
}

/// The whole report.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Report {
    pub tier: Tier,
    pub cells: Vec<CellEstimate>,
    pub controls: Vec<ControlEstimate>,
    pub rules: Vec<RuleFinding>,
    /// Absent on a smoke run: reduced counts decide nothing.
    pub outcome: Option<Outcome>,
    /// Every integrity control's lower bound meets the bound. A statement
    /// separate from `outcome`, so a power failure never reads as an integrity
    /// failure or the reverse.
    pub integrity_passed: Option<bool>,
}

#[derive(Default)]
struct Tally {
    fields: u64,
    focal: u64,
    any_false: u64,
    unavailable: u64,
    dsr: u64,
    reliability: u64,
    bootstrap: u64,
    bar_sum: f64,
}

fn interval(events: u64, trials: u64) -> Interval {
    wilson_interval(events, trials, LEVEL).expect("counts come from a tally of those trials")
}

fn estimate(index: usize, cell: &Cell, tally: &Tally) -> CellEstimate {
    CellEstimate {
        cell: index,
        block: cell.block.clone(),
        dgp: cell.dgp.clone(),
        history_bars: cell.history_bars,
        effect_annual: cell.effect_annual,
        rule: cell.rule.clone(),
        block_prob: cell.block_prob,
        fields: tally.fields,
        focal_eligible: tally.focal,
        any_false: tally.any_false,
        unavailable_entrants: tally.unavailable,
        focal_interval: interval(tally.focal, tally.fields),
        any_false_interval: interval(tally.any_false, tally.fields),
        legs_dsr: tally.dsr,
        legs_reliability: tally.reliability,
        legs_bootstrap: tally.bootstrap,
        mean_bar_annual: tally.bar_sum / tally.fields as f64,
    }
}

/// Decide the study from the cell estimates. Separate from the tally so the
/// three outcomes can be tested on constructed estimates.
pub fn decide(campaign: &Campaign, cells: &[CellEstimate]) -> (Vec<RuleFinding>, Option<Outcome>) {
    let role_of = |cell: &CellEstimate| {
        campaign
            .statistical
            .iter()
            .find(|b| b.id == cell.block)
            .map(|b| b.role)
    };
    let d = &campaign.decision;
    let mut findings = Vec::new();
    for rule in &campaign.rules {
        let nulls: Vec<&CellEstimate> = cells
            .iter()
            .filter(|c| c.rule == rule.id && role_of(c) == Some(BlockRole::PrimaryNull))
            .collect();
        if nulls.is_empty() {
            continue;
        }
        let worst = nulls
            .iter()
            .max_by(|a, b| {
                a.any_false_interval
                    .upper
                    .total_cmp(&b.any_false_interval.upper)
            })
            .expect("nulls is not empty");
        let size_controlled = nulls
            .iter()
            .all(|c| c.any_false_interval.upper <= d.whole_field_limit);
        let targets_reached: Vec<(f64, usize)> = d
            .power_targets
            .iter()
            .filter(|t| {
                let powers: Vec<&CellEstimate> = cells
                    .iter()
                    .filter(|c| {
                        c.rule == rule.id
                            && role_of(c) == Some(BlockRole::PrimaryPower)
                            && c.history_bars == t.history_bars
                            && c.effect_annual == t.effect_annual
                    })
                    .collect();
                !powers.is_empty()
                    && powers
                        .iter()
                        .all(|c| c.focal_interval.lower >= d.power_bound)
            })
            .map(|t| (t.effect_annual, t.history_bars))
            .collect();
        findings.push(RuleFinding {
            rule: rule.id.clone(),
            size_controlled,
            worst_null_upper: worst.any_false_interval.upper,
            worst_null_dgp: worst.dgp.clone(),
            feasible: size_controlled && !targets_reached.is_empty(),
            targets_reached,
        });
    }
    let feasible: Vec<String> = findings
        .iter()
        .filter(|f| f.feasible)
        .map(|f| f.rule.clone())
        .collect();
    let controlled: Vec<String> = findings
        .iter()
        .filter(|f| f.size_controlled)
        .map(|f| f.rule.clone())
        .collect();
    let outcome = if !feasible.is_empty() {
        Outcome::FeasibleRule { rules: feasible }
    } else if !controlled.is_empty() {
        Outcome::NoFeasibleGeometry {
            size_controlled_rules: controlled,
        }
    } else {
        Outcome::SizeNotControlled
    };
    (findings, Some(outcome))
}

/// Build the report. On a smoke tier the intervals are shown and the outcome is
/// withheld.
pub fn build(campaign: &Campaign, tier: Tier, rows: &[Row], controls: &[ControlRow]) -> Report {
    let table = cell_table(campaign);
    let mut tallies: BTreeMap<usize, Tally> = BTreeMap::new();
    for row in rows {
        let t = tallies.entry(row.cell).or_default();
        t.fields += 1;
        t.focal += u64::from(row.focal_eligible);
        t.any_false += u64::from(row.any_false);
        t.unavailable += u64::from(row.unavailable);
        t.dsr += u64::from(row.focal_dsr_leg);
        t.reliability += u64::from(row.focal_reliability_leg);
        t.bootstrap += u64::from(row.focal_bootstrap_leg);
        t.bar_sum += row.bar_annual;
    }
    let cells: Vec<CellEstimate> = tallies
        .iter()
        .map(|(&index, tally)| estimate(index, &table[index], tally))
        .collect();

    let mut by_kind: BTreeMap<ControlKind, (u64, u64, u64, u64, f64)> = BTreeMap::new();
    for row in controls {
        let e = by_kind.entry(row.kind).or_default();
        e.0 += 1;
        e.1 += u64::from(row.detected);
        e.2 += u64::from(row.statistics_admitted);
        e.3 += u64::from(row.twin_ok);
        e.4 += row.measure;
    }
    let controls: Vec<ControlEstimate> = by_kind
        .into_iter()
        .map(
            |(kind, (replicates, detected, statistics_admitted, twin_ok, measure))| {
                ControlEstimate {
                    kind,
                    replicates,
                    detected,
                    statistics_admitted,
                    twin_ok,
                    detection_interval: interval(detected, replicates),
                    mean_measure: measure / replicates as f64,
                }
            },
        )
        .collect();

    let (rules, outcome) = decide(campaign, &cells);
    let frozen = tier == Tier::Frozen;
    let integrity_passed = frozen.then(|| {
        controls
            .iter()
            .all(|c| c.detection_interval.lower >= campaign.decision.detection_bound)
    });
    Report {
        tier,
        cells,
        controls,
        rules,
        outcome: if frozen { outcome } else { None },
        integrity_passed,
    }
}
