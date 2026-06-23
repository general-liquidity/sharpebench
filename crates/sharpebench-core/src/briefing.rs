//! Briefing-neutrality / input-salience-bias audit.
//!
//! Every other SharpeBench integrity check is *output-side* — it inspects an
//! agent's returns and decision trace. But when a whole field is scored against a
//! shared information **briefing**, a leading briefing tilts every agent the same
//! way, and the bias is invisible downstream: the leaderboard looks clean while
//! the whole field was nudged toward the same trade. This module audits the
//! *input* artifact itself.
//!
//! It is a deterministic linter (no NLP, no model) over a structured briefing:
//! it caps how much attention any one asset-area gets, requires each area's facts
//! to be counterbalanced by stated uncertainties, and forbids ranking the
//! trailing-return table by performance (which silently points the reader at the
//! top performer). Borrowed from CapitalBench's briefing-construction rules.

use serde::{Deserialize, Serialize};

/// What a briefing row asserts: a plain fact, an explicit uncertainty/risk, or a
/// counterpoint to the prevailing framing. A neutral briefing balances facts with
/// uncertainties so it doesn't read as a one-sided case.
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RowKind {
    Fact,
    Uncertainty,
    Counterpoint,
}

impl RowKind {
    fn is_counterbalance(self) -> bool {
        matches!(self, RowKind::Uncertainty | RowKind::Counterpoint)
    }
}

/// One line in a briefing section.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BriefingRow {
    pub text: String,
    pub kind: RowKind,
}

/// A briefing section devoted to one asset-area (e.g. "energy", "BTC", "rates").
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BriefingSection {
    pub asset_area: String,
    pub rows: Vec<BriefingRow>,
}

/// How a trailing-return table is ordered. Sorting by performance leads the reader
/// to the best performer; option-order (a fixed, content-independent order) does
/// not.
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TableOrdering {
    /// Rows in a fixed presentation order independent of performance (neutral).
    OptionOrder,
    /// Rows sorted by trailing return (leading — points at the winner).
    Performance,
    /// Ordering not declared.
    Unspecified,
}

/// A trailing-return table shown in the briefing.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ReturnTable {
    pub ordering: TableOrdering,
    pub entries: Vec<ReturnEntry>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ReturnEntry {
    pub label: String,
    pub trailing_return: f64,
}

/// The shared information packet a field of agents is scored against.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Briefing {
    pub sections: Vec<BriefingSection>,
    pub return_table: Option<ReturnTable>,
}

/// Neutrality thresholds the briefing must satisfy.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BriefingPolicy {
    /// Max rows any single asset-area may receive (caps attention concentration).
    pub max_rows_per_area: usize,
    /// Each area with stated facts must also state at least one uncertainty/counterpoint.
    pub require_counterbalance: bool,
    /// The trailing-return table must not be performance-sorted.
    pub require_option_order_sort: bool,
    /// Flag an area whose share of total rows exceeds this fraction (salience tilt).
    pub max_area_salience: f64,
}

impl Default for BriefingPolicy {
    fn default() -> Self {
        BriefingPolicy {
            max_rows_per_area: 5,
            require_counterbalance: true,
            require_option_order_sort: true,
            max_area_salience: 0.5,
        }
    }
}

/// A specific way a briefing fails neutrality.
#[derive(Clone, Debug, Serialize, PartialEq)]
#[serde(tag = "violation", rename_all = "snake_case")]
pub enum BriefingViolation {
    /// An asset-area received more rows than the cap.
    AssetAreaOverweight {
        asset_area: String,
        rows: usize,
        cap: usize,
    },
    /// An asset-area stated facts with no counterbalancing uncertainty/counterpoint.
    MissingCounterbalance { asset_area: String },
    /// The trailing-return table is sorted by performance (a leading frame).
    PerformanceSortedTable,
    /// One asset-area dominates the briefing's attention beyond the salience cap.
    SalienceImbalance { asset_area: String, salience: f64 },
}

/// Per-area share of the briefing's total attention (row count), reported so the
/// tilt is legible even when it doesn't trip a violation.
#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct AreaSalience {
    pub asset_area: String,
    pub row_count: usize,
    /// `row_count / total_rows`, or 0 when the briefing is empty.
    pub salience: f64,
}

/// The result of auditing a briefing.
#[derive(Clone, Debug, Serialize)]
pub struct BriefingAudit {
    pub balanced: bool,
    pub violations: Vec<BriefingViolation>,
    pub salience: Vec<AreaSalience>,
}

/// Audit a briefing for input-side neutrality. Deterministic: section order is
/// preserved, so the same briefing always yields the same report.
pub fn audit_briefing(briefing: &Briefing, policy: &BriefingPolicy) -> BriefingAudit {
    let mut violations = Vec::new();
    let total_rows: usize = briefing.sections.iter().map(|s| s.rows.len()).sum();

    let mut salience = Vec::with_capacity(briefing.sections.len());
    for section in &briefing.sections {
        let n = section.rows.len();
        let share = if total_rows == 0 {
            0.0
        } else {
            n as f64 / total_rows as f64
        };
        salience.push(AreaSalience {
            asset_area: section.asset_area.clone(),
            row_count: n,
            salience: share,
        });

        if n > policy.max_rows_per_area {
            violations.push(BriefingViolation::AssetAreaOverweight {
                asset_area: section.asset_area.clone(),
                rows: n,
                cap: policy.max_rows_per_area,
            });
        }

        if policy.require_counterbalance {
            let has_fact = section.rows.iter().any(|r| r.kind == RowKind::Fact);
            let has_balance = section.rows.iter().any(|r| r.kind.is_counterbalance());
            if has_fact && !has_balance {
                violations.push(BriefingViolation::MissingCounterbalance {
                    asset_area: section.asset_area.clone(),
                });
            }
        }

        // A single area carrying more than its salience cap of total attention is a
        // tilt even when it is under the per-area row cap (e.g. a small briefing).
        if briefing.sections.len() > 1 && share > policy.max_area_salience {
            violations.push(BriefingViolation::SalienceImbalance {
                asset_area: section.asset_area.clone(),
                salience: share,
            });
        }
    }

    if policy.require_option_order_sort {
        if let Some(table) = &briefing.return_table {
            if table.ordering == TableOrdering::Performance {
                violations.push(BriefingViolation::PerformanceSortedTable);
            }
        }
    }

    BriefingAudit {
        balanced: violations.is_empty(),
        violations,
        salience,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(text: &str, kind: RowKind) -> BriefingRow {
        BriefingRow {
            text: text.to_string(),
            kind,
        }
    }

    fn balanced_section(area: &str) -> BriefingSection {
        BriefingSection {
            asset_area: area.to_string(),
            rows: vec![
                row("earnings beat", RowKind::Fact),
                row("but guidance is soft", RowKind::Uncertainty),
            ],
        }
    }

    #[test]
    fn a_balanced_briefing_passes() {
        let b = Briefing {
            sections: vec![balanced_section("energy"), balanced_section("rates")],
            return_table: Some(ReturnTable {
                ordering: TableOrdering::OptionOrder,
                entries: vec![
                    ReturnEntry {
                        label: "A".into(),
                        trailing_return: 0.1,
                    },
                    ReturnEntry {
                        label: "B".into(),
                        trailing_return: 0.2,
                    },
                ],
            }),
        };
        let audit = audit_briefing(&b, &BriefingPolicy::default());
        assert!(audit.balanced, "violations: {:?}", audit.violations);
        assert_eq!(audit.salience.len(), 2);
    }

    #[test]
    fn asset_area_overweight_flags() {
        let mut heavy = balanced_section("energy");
        // 6 rows > default cap of 5 (keep it counterbalanced so only the cap trips).
        heavy.rows = vec![
            row("f1", RowKind::Fact),
            row("f2", RowKind::Fact),
            row("f3", RowKind::Fact),
            row("f4", RowKind::Fact),
            row("f5", RowKind::Fact),
            row("u1", RowKind::Uncertainty),
        ];
        let b = Briefing {
            sections: vec![heavy, balanced_section("rates")],
            return_table: None,
        };
        let audit = audit_briefing(&b, &BriefingPolicy::default());
        assert!(!audit.balanced);
        assert!(audit.violations.iter().any(|v| matches!(
            v,
            BriefingViolation::AssetAreaOverweight { asset_area, rows: 6, cap: 5 }
                if asset_area == "energy"
        )));
    }

    #[test]
    fn performance_sorted_table_flags() {
        let b = Briefing {
            sections: vec![balanced_section("energy"), balanced_section("rates")],
            return_table: Some(ReturnTable {
                ordering: TableOrdering::Performance,
                entries: vec![ReturnEntry {
                    label: "A".into(),
                    trailing_return: 0.3,
                }],
            }),
        };
        let audit = audit_briefing(&b, &BriefingPolicy::default());
        assert!(!audit.balanced);
        assert!(audit
            .violations
            .contains(&BriefingViolation::PerformanceSortedTable));
    }

    #[test]
    fn missing_counterbalance_flags() {
        let one_sided = BriefingSection {
            asset_area: "energy".into(),
            rows: vec![row("bullish fact", RowKind::Fact)],
        };
        let b = Briefing {
            sections: vec![one_sided, balanced_section("rates")],
            return_table: None,
        };
        let audit = audit_briefing(&b, &BriefingPolicy::default());
        assert!(!audit.balanced);
        assert!(audit.violations.iter().any(|v| matches!(
            v,
            BriefingViolation::MissingCounterbalance { asset_area } if asset_area == "energy"
        )));
    }

    #[test]
    fn one_area_dominating_attention_flags_salience() {
        // Energy gets 4 of 6 rows (0.67 > 0.5 cap) while staying under the row cap.
        let energy = BriefingSection {
            asset_area: "energy".into(),
            rows: vec![
                row("f1", RowKind::Fact),
                row("f2", RowKind::Fact),
                row("u1", RowKind::Uncertainty),
                row("c1", RowKind::Counterpoint),
            ],
        };
        let b = Briefing {
            sections: vec![energy, balanced_section("rates")],
            return_table: None,
        };
        let audit = audit_briefing(&b, &BriefingPolicy::default());
        assert!(!audit.balanced);
        assert!(audit.violations.iter().any(|v| matches!(
            v,
            BriefingViolation::SalienceImbalance { asset_area, .. } if asset_area == "energy"
        )));
    }

    #[test]
    fn empty_briefing_is_trivially_balanced() {
        let audit = audit_briefing(&Briefing::default(), &BriefingPolicy::default());
        assert!(audit.balanced);
        assert!(audit.salience.is_empty());
    }
}
