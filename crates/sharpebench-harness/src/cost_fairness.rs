//! Cost and budget fairness disclosures for a board whose rows carry
//! host-observed gateway usage.
//!
//! Three things live here, and none of them moves a score, a rank, an
//! eligibility verdict or a pass^k pool:
//!
//! * [`apply_host_observed_cost`], which [`crate::gateway::serve::attach_host_observed_usage`]
//!   calls, replaces the entrant-reported `cost`, `return_per_cost` and
//!   `dsr_per_cost` columns of a row that has host-observed usage with the
//!   host-observed USD figure, or withholds all three with a label. A row
//!   never mixes the two sources, and a row without host usage is untouched.
//! * [`budget_parity`] states whether every row ran under equal gateway
//!   ceilings, and groups the rows that did not.
//! * [`cost_axis_report`] is an opt-in score against host-observed USD (and
//!   token) frontier. Nothing in the harness calls it or ranks on it.

use serde::Serialize;
use serde_json::{Map, Value};

use crate::accounting::MonetarySummary;
use crate::gateway::serve::HostObservedUsage;
use crate::gateway_journal::GatewayBudget;

/// Schema of the budget-parity disclosure.
pub const BUDGET_PARITY_VERSION: &str = "sharpebench.budget-parity.v1";

/// Schema of the cost-axis report.
pub const COST_AXIS_VERSION: &str = "sharpebench.cost-axis.v1";

/// `cost_source` of a row whose cost columns are the host-observed USD figure.
pub const COST_SOURCE_HOST_OBSERVED: &str = "host_observed_usd";

/// `cost_source` of a row with host usage whose cost columns are withheld.
pub const COST_SOURCE_WITHHELD: &str = "withheld";

const NANOS_PER_USD: f64 = 1_000_000_000.0;

/// The host-observed USD amount of a usage record, in nanodollars, or why
/// there is none to divide by.
fn host_usd_nanos(usage: &HostObservedUsage) -> Result<u128, &'static str> {
    match &usage.monetary_cost {
        MonetarySummary::Estimated {
            usage_source: "host_observed",
            usd_nanos,
            ..
        } => match usd_nanos.parse::<u128>() {
            Ok(0) => Err("host_observed_usd_is_zero"),
            Ok(nanos) => Ok(nanos),
            Err(_) => Err("host_observed_usd_unreadable"),
        },
        MonetarySummary::Estimated { .. } => Err("monetary_cost_not_host_observed"),
        MonetarySummary::Unavailable { reason, .. } => Err(reason),
    }
}

fn finite(value: f64) -> Value {
    serde_json::Number::from_f64(value).map_or(Value::Null, Value::Number)
}

/// Rewrite one row's cost columns from host-observed usage.
///
/// The entrant-reported total moves to `entrant_reported_cost`, which no ratio
/// reads. With a positive host-observed USD amount, `cost` is that amount in
/// USD, `return_per_cost` is `raw_mean_return / cost`, and `dsr_per_cost` is
/// `deflated_sharpe / cost` when the deflation succeeded (a refused deflation
/// leaves it null, which covers every track the scorer itself withholds it
/// for). Otherwise all three are null and `cost_withheld_reason` says why.
/// Applying it twice leaves the entrant figure where the first call put it.
pub fn apply_host_observed_cost(row: &mut Map<String, Value>, usage: &HostObservedUsage) {
    if !row.contains_key("entrant_reported_cost") {
        let entrant = row.remove("cost").unwrap_or(Value::Null);
        row.insert("entrant_reported_cost".to_string(), entrant);
    }
    row.remove("cost_withheld_reason");
    match host_usd_nanos(usage) {
        Ok(nanos) => {
            let cost = nanos as f64 / NANOS_PER_USD;
            let return_per_cost = row
                .get("raw_mean_return")
                .and_then(Value::as_f64)
                .map_or(Value::Null, |raw| finite(raw / cost));
            let deflated = row
                .get("deflation_error")
                .is_none_or(Value::is_null)
                .then(|| row.get("deflated_sharpe").and_then(Value::as_f64))
                .flatten();
            let dsr_per_cost = deflated.map_or(Value::Null, |dsr| finite(dsr / cost));
            row.insert("cost".to_string(), finite(cost));
            row.insert("return_per_cost".to_string(), return_per_cost);
            row.insert("dsr_per_cost".to_string(), dsr_per_cost);
            row.insert(
                "cost_source".to_string(),
                Value::from(COST_SOURCE_HOST_OBSERVED),
            );
        }
        Err(reason) => {
            for column in ["cost", "return_per_cost", "dsr_per_cost"] {
                row.insert(column.to_string(), Value::Null);
            }
            row.insert("cost_source".to_string(), Value::from(COST_SOURCE_WITHHELD));
            row.insert("cost_withheld_reason".to_string(), Value::from(reason));
        }
    }
}

/// The rows that ran under one set of ceilings.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct CeilingGroup {
    pub budget: GatewayBudget,
    /// Sorted, so the disclosure does not follow the board's rank order.
    pub agent_ids: Vec<String>,
}

/// Whether every row of a board ran under equal gateway ceilings. Rank-neutral:
/// it says whether a budget-matched comparison can be claimed, not who won one.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct BudgetParity {
    pub schema_version: &'static str,
    /// True only when every row carries host-observed usage and all of them
    /// declare the same sweep and per-cell ceilings. Equal USD ceilings under
    /// different rate cards are equal spend, not equal tokens.
    pub equal_ceilings: bool,
    /// True only when every row carries host-observed usage and every one of
    /// their budgets has a per-cell ceiling, so no row's later (window, seed)
    /// cells ran on what its earlier cells left over.
    pub per_cell_ceilings: bool,
    /// One group per distinct set of ceilings, in ceiling order.
    pub ceiling_groups: Vec<CeilingGroup>,
    /// Rows with no host-observed budget, which no parity claim can cover.
    pub unbudgeted_agent_ids: Vec<String>,
    pub rank_neutral: bool,
}

fn agent_id(row: &Value) -> String {
    row["agent_id"].as_str().unwrap_or_default().to_string()
}

/// Read the budget parity of a JSON board whose rows may carry
/// `host_observed_usage`.
pub fn budget_parity(board: &Value) -> BudgetParity {
    let mut groups: std::collections::BTreeMap<GatewayBudget, Vec<String>> =
        std::collections::BTreeMap::new();
    let mut unbudgeted = Vec::new();
    for row in board.as_array().map_or(&[][..], Vec::as_slice) {
        let budget = row
            .get("host_observed_usage")
            .and_then(|usage| usage.get("budget"))
            .and_then(|budget| serde_json::from_value::<GatewayBudget>(budget.clone()).ok());
        match budget {
            Some(budget) => groups.entry(budget).or_default().push(agent_id(row)),
            None => unbudgeted.push(agent_id(row)),
        }
    }
    unbudgeted.sort();
    let ceiling_groups: Vec<CeilingGroup> = groups
        .into_iter()
        .map(|(budget, mut agent_ids)| {
            agent_ids.sort();
            CeilingGroup { budget, agent_ids }
        })
        .collect();
    BudgetParity {
        schema_version: BUDGET_PARITY_VERSION,
        equal_ceilings: unbudgeted.is_empty() && ceiling_groups.len() == 1,
        per_cell_ceilings: unbudgeted.is_empty()
            && !ceiling_groups.is_empty()
            && ceiling_groups
                .iter()
                .all(|group| group.budget.per_cell.is_some()),
        ceiling_groups,
        unbudgeted_agent_ids: unbudgeted,
        rank_neutral: true,
    }
}

/// Put the board's [`budget_parity`] on every row under `budget_parity`, the
/// same value on each, as other field-wide figures are. No other key moves.
pub fn attach_budget_parity(board: &mut Value) {
    let parity = serde_json::to_value(budget_parity(board)).expect("budget parity serializes");
    if let Some(rows) = board.as_array_mut() {
        for row in rows {
            row["budget_parity"] = parity.clone();
        }
    }
}

/// One row on the cost axis.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct CostPoint {
    pub agent_id: String,
    pub deflated_sharpe: f64,
    /// Host-observed USD nanodollars, as a decimal string.
    pub usd_nanos: String,
    /// Host-observed input plus output tokens, when both were measured.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tokens: Option<u64>,
    /// No other point costs no more USD with at least the same deflated
    /// Sharpe and is better on one of the two.
    pub usd_frontier: bool,
    /// The same on the token axis; absent when any point lacks tokens.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub token_frontier: Option<bool>,
}

/// A row left off the cost axis, and why.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct CostExclusion {
    pub agent_id: String,
    pub reason: &'static str,
}

/// Deflated Sharpe against host-observed cost. Opt-in and rank-neutral: no
/// score, rank or gate reads it. The frontiers are computed only when every
/// row is on the axis, because a frontier over a subset would read as the
/// field's.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct CostAxisReport {
    pub schema_version: &'static str,
    /// `available` when every row is a point, else `unavailable` with no
    /// points and the exclusions that caused it.
    pub status: &'static str,
    /// Sorted by agent id.
    pub points: Vec<CostPoint>,
    pub excluded: Vec<CostExclusion>,
    pub rank_neutral: bool,
}

/// What one row contributes, before the frontier is known.
struct Candidate {
    agent_id: String,
    deflated_sharpe: f64,
    usd_nanos: u128,
    tokens: Option<u64>,
}

fn candidate(row: &Value) -> Result<Candidate, &'static str> {
    let usage = row
        .get("host_observed_usage")
        .ok_or("no_host_observed_usage")?;
    let money = &usage["monetary_cost"];
    if money["status"] != "estimated" || money["usage_source"] != "host_observed" {
        return Err("host_observed_usd_unavailable");
    }
    let usd_nanos = money["usd_nanos"]
        .as_str()
        .and_then(|nanos| nanos.parse::<u128>().ok())
        .ok_or("host_observed_usd_unreadable")?;
    if !row.get("deflation_error").is_none_or(Value::is_null) {
        return Err("deflation_refused");
    }
    let deflated_sharpe = row["deflated_sharpe"]
        .as_f64()
        .filter(|value| value.is_finite())
        .ok_or("deflated_sharpe_unavailable")?;
    let tokens = usage["input_tokens"]
        .as_u64()
        .zip(usage["output_tokens"].as_u64())
        .map(|(input, output)| input.saturating_add(output));
    Ok(Candidate {
        agent_id: agent_id(row),
        deflated_sharpe,
        usd_nanos,
        tokens,
    })
}

/// Whether `a` dominates `b`: costs no more, scores no less, and is strictly
/// better on one axis.
fn dominates(a_cost: u128, a_score: f64, b_cost: u128, b_score: f64) -> bool {
    a_cost <= b_cost && a_score >= b_score && (a_cost < b_cost || a_score > b_score)
}

fn on_frontier(costs: &[u128], scores: &[f64], index: usize) -> bool {
    !(0..costs.len()).any(|other| {
        other != index && dominates(costs[other], scores[other], costs[index], scores[index])
    })
}

/// Build the opt-in cost axis of a JSON board whose rows carry
/// `host_observed_usage`.
pub fn cost_axis_report(board: &Value) -> CostAxisReport {
    let mut candidates = Vec::new();
    let mut excluded = Vec::new();
    for row in board.as_array().map_or(&[][..], Vec::as_slice) {
        match candidate(row) {
            Ok(point) => candidates.push(point),
            Err(reason) => excluded.push(CostExclusion {
                agent_id: agent_id(row),
                reason,
            }),
        }
    }
    excluded.sort_by(|a, b| a.agent_id.cmp(&b.agent_id));
    if !excluded.is_empty() || candidates.is_empty() {
        return CostAxisReport {
            schema_version: COST_AXIS_VERSION,
            status: "unavailable",
            points: Vec::new(),
            excluded,
            rank_neutral: true,
        };
    }
    candidates.sort_by(|a, b| a.agent_id.cmp(&b.agent_id));
    let scores: Vec<f64> = candidates.iter().map(|c| c.deflated_sharpe).collect();
    let usd: Vec<u128> = candidates.iter().map(|c| c.usd_nanos).collect();
    let tokens: Option<Vec<u128>> = candidates
        .iter()
        .map(|c| c.tokens.map(u128::from))
        .collect();
    let points = candidates
        .iter()
        .enumerate()
        .map(|(index, c)| CostPoint {
            agent_id: c.agent_id.clone(),
            deflated_sharpe: c.deflated_sharpe,
            usd_nanos: c.usd_nanos.to_string(),
            tokens: c.tokens,
            usd_frontier: on_frontier(&usd, &scores, index),
            token_frontier: tokens
                .as_deref()
                .map(|tokens| on_frontier(tokens, &scores, index)),
        })
        .collect();
    CostAxisReport {
        schema_version: COST_AXIS_VERSION,
        status: "available",
        points,
        excluded,
        rank_neutral: true,
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::accounting::RateCard;
    use crate::gateway_journal::{CellBudget, FinishReasonCounts};

    fn card() -> RateCard {
        RateCard::from_json(
            br#"{"schema_version":"sharpebench.token-rate-card.v1","provider":"fake","model":"fake-1","revision":"2026-01-01","input_usd_nanos_per_token":1,"output_usd_nanos_per_token":1}"#,
        )
        .expect("a valid test rate card")
    }

    fn budget(max_usd_nanos: u128, per_cell: Option<CellBudget>) -> GatewayBudget {
        GatewayBudget {
            max_usd_nanos,
            max_calls: 10,
            per_cell,
        }
    }

    fn usage(monetary_cost: MonetarySummary) -> HostObservedUsage {
        HostObservedUsage {
            schema_version: crate::gateway::serve::HOST_USAGE_VERSION,
            route_table_sha256: "r".repeat(64),
            sweep_sha256: "s".repeat(64),
            monetary_cost,
            calls_started: 2,
            priced_calls: 2,
            unknown_calls: 0,
            released_calls: 0,
            overspent_usd_nanos: "0".to_string(),
            overspent_calls: 0,
            finish_reasons: FinishReasonCounts::default(),
            input_tokens: Some(30),
            output_tokens: Some(12),
            cache_read_input_tokens: None,
            cache_write_input_tokens: None,
            ceiling_breached: false,
            budget: budget(1_000, None),
            journal_ownership_lost: false,
            journal_unwritable: false,
            rank_neutral: true,
        }
    }

    fn priced(usd_nanos: &str) -> MonetarySummary {
        MonetarySummary::Estimated {
            rate_card: card(),
            rate_card_sha256: card().digest(),
            usage_source: "host_observed",
            usd_nanos: usd_nanos.to_string(),
        }
    }

    fn unavailable(reason: &'static str) -> MonetarySummary {
        MonetarySummary::Unavailable {
            reason,
            known_subtotal_usd_nanos: None,
            rate_card: None,
            rate_card_sha256: None,
        }
    }

    /// A scored row as `rank` publishes it: an entrant-reported cost of 4 and
    /// the ratios the scorer derived from it.
    fn row(agent_id: &str) -> Map<String, Value> {
        let value = json!({
            "agent_id": agent_id,
            "deflated_sharpe": 0.6,
            "deflation_error": null,
            "raw_mean_return": 0.02,
            "cost": 4.0,
            "return_per_cost": 0.005,
            "dsr_per_cost": 0.15,
            "rank_ordinal": 1,
        });
        value.as_object().expect("an object").clone()
    }

    #[test]
    fn host_observed_usd_replaces_every_entrant_reported_cost_column() {
        let mut columns = row("a");
        apply_host_observed_cost(&mut columns, &usage(priced("2000000000")));
        assert_eq!(columns["entrant_reported_cost"], json!(4.0));
        assert_eq!(columns["cost"], json!(2.0));
        assert_eq!(columns["return_per_cost"], json!(0.01));
        assert_eq!(columns["dsr_per_cost"], json!(0.3));
        assert_eq!(columns["cost_source"], COST_SOURCE_HOST_OBSERVED);
        assert!(!columns.contains_key("cost_withheld_reason"));
        assert_eq!(columns["rank_ordinal"], json!(1), "no rank field moves");
    }

    /// The scorer withholds `dsr_per_cost` for a track whose deflation it
    /// refused; the host-observed figure does not bring it back.
    #[test]
    fn a_refused_deflation_keeps_dsr_per_cost_null_under_host_cost() {
        let mut columns = row("a");
        columns.insert("deflation_error".to_string(), json!("constant track"));
        apply_host_observed_cost(&mut columns, &usage(priced("2000000000")));
        assert_eq!(columns["dsr_per_cost"], Value::Null);
        assert_eq!(columns["return_per_cost"], json!(0.01));
        assert_eq!(columns["cost_source"], COST_SOURCE_HOST_OBSERVED);
    }

    /// No host figure to divide by means no ratio at all: the entrant's own
    /// figure is never the fallback.
    #[test]
    fn an_unmeasured_host_cost_withholds_the_columns_with_its_reason() {
        for (monetary, reason) in [
            (
                unavailable("host_observed_usage_incomplete"),
                "host_observed_usage_incomplete",
            ),
            (priced("0"), "host_observed_usd_is_zero"),
            (priced("not-a-number"), "host_observed_usd_unreadable"),
            (
                MonetarySummary::Estimated {
                    rate_card: card(),
                    rate_card_sha256: card().digest(),
                    usage_source: "entrant_reported",
                    usd_nanos: "5".to_string(),
                },
                "monetary_cost_not_host_observed",
            ),
        ] {
            let mut columns = row("a");
            apply_host_observed_cost(&mut columns, &usage(monetary));
            for column in ["cost", "return_per_cost", "dsr_per_cost"] {
                assert_eq!(columns[column], Value::Null, "{reason}: {column}");
            }
            assert_eq!(columns["entrant_reported_cost"], json!(4.0));
            assert_eq!(columns["cost_source"], COST_SOURCE_WITHHELD);
            assert_eq!(columns["cost_withheld_reason"], reason);
        }
    }

    /// Re-attaching keeps the entrant figure where the first attach put it
    /// and replaces a stale withheld reason.
    #[test]
    fn attaching_host_cost_twice_never_files_the_host_figure_as_the_entrants() {
        let mut columns = row("a");
        apply_host_observed_cost(
            &mut columns,
            &usage(unavailable("gateway_journal_has_no_calls")),
        );
        apply_host_observed_cost(&mut columns, &usage(priced("1000000000")));
        assert_eq!(columns["entrant_reported_cost"], json!(4.0));
        assert_eq!(columns["cost"], json!(1.0));
        assert!(!columns.contains_key("cost_withheld_reason"));
    }

    fn board_row(agent_id: &str, budget: Option<GatewayBudget>) -> Value {
        let mut value = Value::Object(row(agent_id));
        if let Some(budget) = budget {
            value["host_observed_usage"] = json!({ "budget": budget });
        }
        value
    }

    #[test]
    fn rows_under_one_set_of_ceilings_are_at_parity() {
        let board = json!([
            board_row("b", Some(budget(1_000, None))),
            board_row("a", Some(budget(1_000, None))),
        ]);
        let parity = budget_parity(&board);
        assert!(parity.equal_ceilings && parity.rank_neutral);
        assert!(!parity.per_cell_ceilings, "a sweep ceiling alone");
        assert_eq!(parity.schema_version, BUDGET_PARITY_VERSION);
        assert_eq!(
            parity.ceiling_groups,
            [CeilingGroup {
                budget: budget(1_000, None),
                agent_ids: vec!["a".to_string(), "b".to_string()],
            }]
        );
        assert!(parity.unbudgeted_agent_ids.is_empty());
    }

    /// A different per-cell ceiling is a different budget, and the
    /// disclosure names which rows ran under which.
    #[test]
    fn differing_ceilings_are_grouped_and_break_parity() {
        let cell = Some(CellBudget {
            max_usd_nanos: 500,
            max_calls: 5,
        });
        let board = json!([
            board_row("a", Some(budget(1_000, None))),
            board_row("b", Some(budget(1_000, cell))),
            board_row("c", Some(budget(1_000, None))),
        ]);
        let parity = budget_parity(&board);
        assert!(!parity.equal_ceilings);
        assert!(
            !parity.per_cell_ceilings,
            "a and c ran on one shared ceiling"
        );
        assert_eq!(
            parity.ceiling_groups,
            [
                CeilingGroup {
                    budget: budget(1_000, None),
                    agent_ids: vec!["a".to_string(), "c".to_string()],
                },
                CeilingGroup {
                    budget: budget(1_000, cell),
                    agent_ids: vec!["b".to_string()],
                },
            ]
        );
    }

    /// The disclosure says per-cell ceilings applied only when every row's
    /// budget carries one, whether or not the rows' ceilings are equal.
    #[test]
    fn per_cell_ceilings_are_disclosed_only_when_every_row_ran_under_one() {
        let cell = |max_calls| {
            Some(CellBudget {
                max_usd_nanos: 500,
                max_calls,
            })
        };
        let all = json!([
            board_row("a", Some(budget(1_000, cell(5)))),
            board_row("b", Some(budget(1_000, cell(4)))),
        ]);
        let parity = budget_parity(&all);
        assert!(parity.per_cell_ceilings && !parity.equal_ceilings);
        let unbudgeted = json!([
            board_row("a", Some(budget(1_000, cell(5)))),
            board_row("z", None),
        ]);
        assert!(!budget_parity(&unbudgeted).per_cell_ceilings);
    }

    /// A row with no host budget cannot be covered by a parity claim, and an
    /// empty board claims nothing.
    #[test]
    fn a_row_without_a_host_budget_breaks_parity() {
        let board = json!([
            board_row("a", Some(budget(1_000, None))),
            board_row("z", None),
        ]);
        let parity = budget_parity(&board);
        assert!(!parity.equal_ceilings);
        assert_eq!(parity.unbudgeted_agent_ids, ["z"]);
        assert_eq!(parity.ceiling_groups.len(), 1);
        let empty = budget_parity(&json!([]));
        assert!(!empty.equal_ceilings && !empty.per_cell_ceilings);
    }

    #[test]
    fn budget_parity_is_attached_to_every_row_and_moves_nothing_else() {
        let plain = json!([
            board_row("a", Some(budget(1_000, None))),
            board_row("b", Some(budget(2_000, None))),
        ]);
        let mut board = plain.clone();
        attach_budget_parity(&mut board);
        let expected = serde_json::to_value(budget_parity(&plain)).expect("serializes");
        for row in board.as_array_mut().expect("an array") {
            assert_eq!(row["budget_parity"], expected);
            assert_eq!(row["budget_parity"]["equal_ceilings"], false);
            row.as_object_mut().expect("a row").remove("budget_parity");
        }
        assert_eq!(board, plain);
    }

    fn axis_row(agent_id: &str, dsr: f64, usd_nanos: &str, tokens: Option<(u64, u64)>) -> Value {
        let mut value = Value::Object(row(agent_id));
        value["deflated_sharpe"] = json!(dsr);
        value["host_observed_usage"] = json!({
            "monetary_cost": {
                "status": "estimated",
                "usage_source": "host_observed",
                "usd_nanos": usd_nanos,
            },
        });
        if let Some((input, output)) = tokens {
            value["host_observed_usage"]["input_tokens"] = json!(input);
            value["host_observed_usage"]["output_tokens"] = json!(output);
        }
        value
    }

    fn frontier(report: &CostAxisReport) -> Vec<(&str, bool, Option<bool>)> {
        report
            .points
            .iter()
            .map(|p| (p.agent_id.as_str(), p.usd_frontier, p.token_frontier))
            .collect()
    }

    /// `b` costs more than `a` for a lower score, so `a` dominates it. `c`
    /// costs the most and scores the most. `d` ties `a` exactly, and two equal
    /// points do not dominate each other. On tokens the order differs.
    #[test]
    fn the_cost_axis_marks_the_usd_and_token_frontiers() {
        let board = json!([
            axis_row("c", 0.9, "300", Some((10, 10))),
            axis_row("b", 0.4, "200", Some((1, 1))),
            axis_row("a", 0.5, "100", Some((50, 50))),
            axis_row("d", 0.5, "100", Some((50, 50))),
        ]);
        let report = cost_axis_report(&board);
        assert_eq!(report.status, "available");
        assert_eq!(report.schema_version, COST_AXIS_VERSION);
        assert!(report.rank_neutral && report.excluded.is_empty());
        assert_eq!(
            frontier(&report),
            [
                ("a", true, Some(false)),
                ("b", false, Some(true)),
                ("c", true, Some(true)),
                ("d", true, Some(false)),
            ]
        );
        assert_eq!(report.points[0].usd_nanos, "100");
        assert_eq!(report.points[0].tokens, Some(100));
    }

    /// A point cheaper at an equal score dominates, and so does one at an
    /// equal cost with a higher score.
    #[test]
    fn a_point_better_on_one_axis_and_equal_on_the_other_dominates() {
        let board = json!([
            axis_row("cheap", 0.5, "100", None),
            axis_row("pricey", 0.5, "101", None),
            axis_row("better", 0.6, "100", None),
        ]);
        let report = cost_axis_report(&board);
        assert_eq!(
            frontier(&report),
            [
                ("better", true, None),
                ("cheap", false, None),
                ("pricey", false, None)
            ]
        );
        // Alone, the cheaper of two equal scores is what removes the other.
        let pair = json!([
            axis_row("cheap", 0.5, "100", None),
            axis_row("pricey", 0.5, "101", None),
        ]);
        assert_eq!(
            frontier(&cost_axis_report(&pair)),
            [("cheap", true, None), ("pricey", false, None)]
        );
    }

    /// A frontier over a subset would read as the field's, so any row off the
    /// axis withholds the whole report and says which and why.
    #[test]
    fn a_row_off_the_cost_axis_withholds_the_frontier() {
        let mut refused = axis_row("r", 0.5, "100", None);
        refused["deflation_error"] = json!("constant track");
        let mut unpriced = axis_row("u", 0.5, "100", None);
        unpriced["host_observed_usage"]["monetary_cost"] =
            json!({ "status": "unavailable", "reason": "host_observed_usage_incomplete" });
        let mut entrant = axis_row("e", 0.5, "100", None);
        entrant["host_observed_usage"]["monetary_cost"]["usage_source"] = json!("entrant");
        let garbled = axis_row("g", 0.5, "x", None);
        let mut missing = axis_row("n", 0.5, "100", None);
        missing["deflated_sharpe"] = Value::Null;
        let board = json!([
            axis_row("a", 0.5, "100", None),
            refused,
            unpriced,
            entrant,
            garbled,
            missing,
            Value::Object(row("plain")),
        ]);
        let report = cost_axis_report(&board);
        assert_eq!(report.status, "unavailable");
        assert!(report.points.is_empty());
        let reasons: Vec<(&str, &str)> = report
            .excluded
            .iter()
            .map(|e| (e.agent_id.as_str(), e.reason))
            .collect();
        assert_eq!(
            reasons,
            [
                ("e", "host_observed_usd_unavailable"),
                ("g", "host_observed_usd_unreadable"),
                ("n", "deflated_sharpe_unavailable"),
                ("plain", "no_host_observed_usage"),
                ("r", "deflation_refused"),
                ("u", "host_observed_usd_unavailable"),
            ]
        );
        assert_eq!(cost_axis_report(&json!([])).status, "unavailable");
    }

    /// A row that reports only one token count has no token total, and then
    /// no point is placed on the token axis.
    #[test]
    fn a_missing_token_count_withholds_the_token_frontier_only() {
        let mut half = axis_row("h", 0.5, "100", None);
        half["host_observed_usage"]["input_tokens"] = json!(5);
        let board = json!([axis_row("a", 0.4, "200", Some((1, 1))), half]);
        let report = cost_axis_report(&board);
        assert_eq!(report.status, "available");
        assert_eq!(report.points[1].tokens, None);
        assert_eq!(frontier(&report), [("a", false, None), ("h", true, None)]);
    }
}
