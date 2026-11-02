//! The leaderboard export: a canonical JSON record and a plain Markdown table of
//! rows read from signed boards, for command-line and agent use.
//!
//! This module only shapes rows. It reads no file and checks no signature: the
//! caller (the arena crate) verifies every board first and hands this module
//! the scores the signed links carry, with the facts the board header records.
//! Everything here is a pure function of those inputs, so the same verified
//! boards give the same bytes on every run and platform: rows keep the order
//! of the signed chain, maps are ordered, and numbers are written by
//! `serde_json`'s shortest round-trip formatting or by a fixed-precision format.
//!
//! Each row joins the [`Headline`] of the scored row with the figures a reader
//! of a board checks: the deflated Sharpe and its interval, the pass^k count,
//! eligibility, the cost columns, and the address of the signed link the row
//! came from. A refusal recorded on a board header is exported as a row with
//! the verdict [`Verdict::Refused`] and no figures, so an entrant the board
//! turned away is not silently missing from the export.

use serde::{Deserialize, Serialize};

use crate::composite::CompositeScore;
use crate::disqualification::DisqualThresholds;
use crate::evaluation_tier::EvaluationTier;
use crate::headline::{headline, Evidence, Headline, HeadlineContext, HeadlineRow, StressMark};

/// Schema of the export record.
pub const SCHEMA_VERSION: &str = "sharpebench.leaderboard-export.v1";

/// Where the cost columns come from. A signed board carries the cost the
/// entrant reported on its runs, never a host-observed figure, so the export
/// names that source on every row that has cost columns.
pub const COST_SOURCE_ENTRANT_REPORTED: &str = "entrant_reported";

/// What kind of signed input the rows were read from.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceKind {
    /// An arena directory: every published window board, chained.
    Arena,
    /// One board file written by `sharpebench sign`.
    SignedBoard,
}

/// How the input was verified before any row was read from it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExportSource {
    pub kind: SourceKind,
    /// The signature scheme that was checked: `ed25519` or `hmac-sha256`.
    pub scheme: String,
    /// The Ed25519 verifying key the boards were checked under. `None` under
    /// HMAC, whose key is secret.
    pub verifying_key: Option<String>,
    /// Whether the caller pinned the key (`--pubkey`, or the HMAC key) rather
    /// than trusting the key a document embeds.
    pub key_pinned: bool,
    /// The completeness status the verification reached, in its own words.
    pub completeness: String,
    /// What the verification still cannot establish.
    pub completeness_limit: String,
    /// The head to pin later, `<count>:<sha256>`, when one exists.
    pub head: Option<String>,
}

/// One verified board the rows were read from.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct BoardRecord {
    /// The window id for an arena board, the file name for a signed board.
    pub board_id: String,
    /// The signed board document, relative to the export input.
    pub board_path: String,
    /// The window record beside it, for an arena board.
    pub window_path: Option<String>,
    /// SHA-256 of the board's final signature (hex as stored), which every
    /// later board and the arena head bind.
    pub final_signature_sha256: String,
    /// The dataset hash the board header signs, when it records one.
    pub dataset_hash: Option<String>,
    /// SHA-256 of the score configuration the rows were scored under.
    pub score_config_sha256: String,
    /// The scorer artifact the header names, for an arena board.
    pub scorer_artifact_sha256: Option<String>,
    /// The header's `certifying`. `None` when the header records none.
    pub certifying: Option<bool>,
    /// Whether the board ranked returns the entrant supplied.
    pub supplied_returns_accepted: bool,
    /// Signed score rows on the board.
    pub rows: usize,
    /// Refusals the board header records.
    pub refusals: usize,
}

/// The pass^k figures of a row.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PassK {
    pub passed: bool,
    /// Runs that passed. `None` in a score archived before the count existed.
    pub runs_passed: Option<usize>,
    pub runs_scored: usize,
}

/// The cost columns of a row, with their source.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CostFields {
    /// Always [`COST_SOURCE_ENTRANT_REPORTED`] for a signed board row.
    pub source: String,
    /// Total cost across runs. `None` when no cost was reported.
    pub total: Option<f64>,
    pub return_per_cost: Option<f64>,
    pub dsr_per_cost: Option<f64>,
}

/// Where a row's signed evidence lives.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecordLink {
    pub board_path: String,
    pub window_path: Option<String>,
    /// Position of the signed link the row was read from in the board's
    /// chain. A refusal is read from the header link, position 0.
    pub link: usize,
    /// The signature of that link.
    pub link_signature: String,
}

/// One exported row.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ExportRow {
    pub board_id: String,
    pub headline: Headline,
    /// The row's evaluation tier, the headline's first qualifier:
    /// `forward-certifying` only for a row whose signed board header
    /// certifies it, and `historical-advisory` for every other row.
    pub tier: EvaluationTier,
    /// The row's tie band (`tie_group`) when it is rank-eligible. Rows that
    /// share a band are statistically indistinguishable on the deflated
    /// Sharpe. `None` for a row that cannot be ranked.
    pub tie_band: Option<usize>,
    pub rank_eligible: bool,
    /// The deflated Sharpe. `None` when the deflation family refused the track
    /// or could not compute it, whose stored value is a floor, not a figure.
    pub dsr: Option<f64>,
    /// The bootstrap interval of the deflated Sharpe, `[low, high]`.
    pub dsr_interval: Option<[f64; 2]>,
    /// Why the interval is missing, when the scorer said.
    pub dsr_interval_error: Option<String>,
    pub pass_k: Option<PassK>,
    pub cost: Option<CostFields>,
    /// How the row's returns were obtained, as the arena board records it
    /// (`supplied`, `replayed`, `re-executed` or `live-captured`). `None` for
    /// a refusal and for a board that records no provenance.
    pub returns_provenance: Option<String>,
    pub record: RecordLink,
}

/// The whole export.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct LeaderboardExport {
    pub schema: String,
    pub source: ExportSource,
    pub boards: Vec<BoardRecord>,
    pub rows: Vec<ExportRow>,
}

/// The export row of one signed score link.
///
/// `thresholds` must be [`DisqualThresholds::from_score_config`] of the
/// configuration the board was scored under, as for [`headline`].
pub fn scored_row(
    board: &BoardRecord,
    link: usize,
    link_signature: &str,
    score: &CompositeScore,
    thresholds: &DisqualThresholds,
    evidence: Evidence,
    returns_provenance: Option<&str>,
) -> ExportRow {
    let context = HeadlineContext {
        evidence,
        stress: StressMark::NotRun,
    };
    let headline = headline(HeadlineRow::Scored(score), thresholds, &context);
    let refused_shape = score.deflation_error.is_some() || score.deflation_refusal.is_some();
    let dsr_interval = match (score.dsr_ci_low, score.dsr_ci_high) {
        (Some(low), Some(high)) => Some([low, high]),
        _ => None,
    };
    let total = (score.cost > 0.0).then_some(score.cost);
    ExportRow {
        board_id: board.board_id.clone(),
        tier: headline.qualifiers.tier,
        headline,
        tie_band: score.rank_eligible.then_some(score.tie_group),
        rank_eligible: score.rank_eligible,
        dsr: (!refused_shape).then_some(score.deflated_sharpe),
        dsr_interval,
        dsr_interval_error: score.dsr_ci_error.clone(),
        pass_k: Some(PassK {
            passed: score.passed_k,
            runs_passed: score.runs_passed,
            runs_scored: score.runs_scored,
        }),
        cost: Some(CostFields {
            source: COST_SOURCE_ENTRANT_REPORTED.to_string(),
            total,
            return_per_cost: score.return_per_cost,
            dsr_per_cost: score.dsr_per_cost,
        }),
        returns_provenance: returns_provenance.map(str::to_string),
        record: RecordLink {
            board_path: board.board_path.clone(),
            window_path: board.window_path.clone(),
            link,
            link_signature: link_signature.to_string(),
        },
    }
}

/// The export row of a refusal a board header records: no figures, the
/// verdict [`Verdict::Refused`](crate::headline::Verdict::Refused) and the
/// refusal text as its cause. `header_signature` is the signature of the
/// header link (position 0) that signs the refusal.
pub fn refused_row(
    board: &BoardRecord,
    header_signature: &str,
    agent_id: &str,
    reason: &str,
    thresholds: &DisqualThresholds,
    evidence: Evidence,
) -> ExportRow {
    let context = HeadlineContext {
        evidence,
        stress: StressMark::NotRun,
    };
    let headline = headline(
        HeadlineRow::Refused { agent_id, reason },
        thresholds,
        &context,
    );
    ExportRow {
        board_id: board.board_id.clone(),
        tier: headline.qualifiers.tier,
        headline,
        tie_band: None,
        rank_eligible: false,
        dsr: None,
        dsr_interval: None,
        dsr_interval_error: None,
        pass_k: None,
        cost: None,
        returns_provenance: None,
        record: RecordLink {
            board_path: board.board_path.clone(),
            window_path: board.window_path.clone(),
            link: 0,
            link_signature: header_signature.to_string(),
        },
    }
}

/// The canonical JSON of an export: pretty-printed, fields in declaration
/// order, and a final newline.
pub fn to_canonical_json(export: &LeaderboardExport) -> String {
    let mut text = serde_json::to_string_pretty(export).unwrap_or_default();
    text.push('\n');
    text
}

/// A table cell: pipes and backslashes escaped and control characters
/// (newlines included) replaced by a space, so a signed agent id or refusal
/// text cannot end the cell, the row or the table.
fn cell(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '|' => out.push_str("\\|"),
            c if c.is_control() => out.push(' '),
            c => out.push(c),
        }
    }
    out
}

fn yes_no(value: bool) -> &'static str {
    if value {
        "yes"
    } else {
        "no"
    }
}

fn optional_flag(value: Option<bool>) -> &'static str {
    match value {
        Some(true) => "yes",
        Some(false) => "no",
        None => "unrecorded",
    }
}

fn evidence_cell(row: &ExportRow) -> String {
    let (status, mode) = match &row.headline.qualifiers.evidence {
        Evidence::Unknown => ("unknown", None),
        Evidence::Certifying { mode } => ("certifying", Some(mode)),
        Evidence::Noncertifying { mode, .. } => ("noncertifying", Some(mode)),
    };
    let detail: Vec<String> = mode
        .into_iter()
        .chain(row.returns_provenance.as_ref())
        .map(|text| cell(text))
        .collect();
    if detail.is_empty() {
        status.to_string()
    } else {
        format!("{status} ({})", detail.join(", "))
    }
}

fn snake(value: &impl Serialize) -> String {
    serde_json::to_value(value)
        .ok()
        .and_then(|v| v.as_str().map(str::to_string))
        .unwrap_or_default()
}

fn qualifiers_cell(row: &ExportRow) -> String {
    let q = &row.headline.qualifiers;
    let mut parts = Vec::new();
    if q.tied {
        parts.push("tied".to_string());
    }
    if q.stress != StressMark::NotRun {
        parts.push(format!("stress {}", snake(&q.stress)));
    }
    if !q.advisory.is_empty() {
        let names: Vec<String> = q.advisory.iter().map(snake).collect();
        parts.push(format!("advisory: {}", names.join(", ")));
    }
    if let Some(label) = &q.declared_mandate {
        parts.push(format!("mandate: {}", cell(label)));
    }
    if let Some(label) = &q.rank_mode {
        parts.push(format!("rank mode: {}", cell(label)));
    }
    if parts.is_empty() {
        "-".to_string()
    } else {
        parts.join("; ")
    }
}

fn reasons_cell(row: &ExportRow) -> String {
    let mut parts: Vec<String> = row.headline.hard_reasons.iter().map(snake).collect();
    if let Some(cause) = &row.headline.cause {
        parts.push(format!("cause: {}", cell(cause)));
    }
    if parts.is_empty() {
        "-".to_string()
    } else {
        parts.join(", ")
    }
}

fn link_cell(record: &RecordLink) -> String {
    format!("`{}#{}`", cell(&record.board_path), record.link)
}

fn row_line(row: &ExportRow) -> String {
    let dash = || "-".to_string();
    let scored = row.pass_k.is_some();
    let tier = row.tier.as_str();
    let tie_band = row.tie_band.map_or_else(dash, |t| t.to_string());
    let dsr = match row.dsr {
        Some(dsr) => format!("{dsr:.4}"),
        None if scored => "unavailable".to_string(),
        None => dash(),
    };
    let interval = match row.dsr_interval {
        Some([low, high]) => format!("[{low:.4}, {high:.4}]"),
        None if scored => "unavailable".to_string(),
        None => dash(),
    };
    let pass_k = row.pass_k.map_or_else(dash, |p| {
        let passed = p
            .runs_passed
            .map_or_else(|| "?".to_string(), |n| n.to_string());
        let word = if p.passed { "passed" } else { "failed" };
        format!("{word} {passed}/{}", p.runs_scored)
    });
    let eligible = if scored {
        yes_no(row.rank_eligible).to_string()
    } else {
        dash()
    };
    let cost = row.cost.as_ref().map_or_else(dash, |c| {
        c.total.map_or_else(
            || "unreported".to_string(),
            |total| format!("{total:.4} {}", c.source),
        )
    });
    format!(
        "| {} | {} | {} | {} | {tier} | {tie_band} | {dsr} | {interval} | {pass_k} | {eligible} | {} | {cost} | {} | {} |",
        cell(&row.board_id),
        link_cell(&row.record),
        cell(&row.headline.agent_id),
        row.headline.verdict.as_str(),
        evidence_cell(row),
        qualifiers_cell(row),
        reasons_cell(row),
    )
}

/// The Markdown form of an export: the verification summary, one table of the
/// boards and one table of the rows, in the export's order. Every figure in it
/// is in the JSON; the JSON is the record and this is its reading.
pub fn render_markdown(export: &LeaderboardExport) -> String {
    let source = &export.source;
    let mut out = String::new();
    out.push_str("# SharpeBench leaderboard export\n\n");
    out.push_str(&format!(
        "Schema `{}`. Every row was read from a signed board that verified before export.\n\n",
        cell(&export.schema)
    ));
    out.push_str(&format!(
        "- Source: {}, {} signatures, key {}{}\n",
        snake(&source.kind),
        cell(&source.scheme),
        if source.key_pinned {
            "pinned"
        } else {
            "embedded"
        },
        source
            .verifying_key
            .as_ref()
            .map_or_else(String::new, |k| format!(" `{}`", cell(k))),
    ));
    out.push_str(&format!(
        "- Completeness: {}. {}\n",
        cell(&source.completeness),
        cell(&source.completeness_limit)
    ));
    if let Some(head) = &source.head {
        out.push_str(&format!("- Head: `{}`\n", cell(head)));
    }
    out.push_str("\n## Boards\n\n");
    out.push_str("| board | tier | record | final signature sha256 | certifying | supplied returns | rows | refusals |\n");
    out.push_str("|---|---|---|---|---|---|---|---|\n");
    for b in &export.boards {
        out.push_str(&format!(
            "| {} | {} | `{}` | `{}` | {} | {} | {} | {} |\n",
            cell(&b.board_id),
            EvaluationTier::of_signed_forward_board(b.certifying).as_str(),
            cell(&b.board_path),
            cell(&b.final_signature_sha256),
            optional_flag(b.certifying),
            yes_no(b.supplied_returns_accepted),
            b.rows,
            b.refusals,
        ));
    }
    out.push_str("\n## Rows\n\n");
    out.push_str("| board | record | agent | verdict | tier | tie band | DSR | DSR interval | pass^k | eligible | evidence | cost | qualifiers | reasons |\n");
    out.push_str("|---|---|---|---|---|---|---|---|---|---|---|---|---|---|\n");
    for row in &export.rows {
        out.push_str(&row_line(row));
        out.push('\n');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::composite::{rank, AgentSubmission, Run, ScoreConfig};
    use crate::disqualification::FailReason;
    use crate::headline::Verdict;

    fn run(mean_ret: f64, amp: f64, n: usize, cost: f64) -> Run {
        Run {
            returns: (0..n)
                .map(|i| mean_ret + amp * (i as f64 * 0.7).sin())
                .collect(),
            cost,
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

    /// A ranked field: a skilled entrant with reported cost, and a flat-mean
    /// one with none.
    fn field() -> Vec<CompositeScore> {
        let board = rank(
            &[
                agent(
                    "skilled",
                    (0..5).map(|_| run(0.002, 0.004, 60, 0.5)).collect(),
                ),
                agent("noisy", (0..5).map(|_| run(0.0, 0.004, 60, 0.0)).collect()),
            ],
            &ScoreConfig::default(),
        );
        assert!(board[0].rank_eligible && !board[1].rank_eligible);
        board
    }

    fn board() -> BoardRecord {
        BoardRecord {
            board_id: "w1".to_string(),
            board_path: "windows/w1/board.json".to_string(),
            window_path: Some("windows/w1/window.json".to_string()),
            final_signature_sha256: "ab".repeat(32),
            dataset_hash: Some("cd".repeat(32)),
            score_config_sha256: "ef".repeat(32),
            scorer_artifact_sha256: Some("01".repeat(32)),
            certifying: Some(true),
            supplied_returns_accepted: false,
            rows: 2,
            refusals: 1,
        }
    }

    fn thresholds() -> DisqualThresholds {
        DisqualThresholds::from_score_config(&ScoreConfig::default())
    }

    fn certifying() -> Evidence {
        Evidence::Certifying {
            mode: "forward".to_string(),
        }
    }

    #[test]
    fn a_scored_row_carries_the_signed_figures_and_its_link() {
        let scores = field();
        let s = &scores[0];
        let row = scored_row(
            &board(),
            1,
            "sig-1",
            s,
            &thresholds(),
            certifying(),
            Some("re-executed"),
        );
        assert_eq!(row.returns_provenance.as_deref(), Some("re-executed"));
        assert_eq!(row.board_id, "w1");
        assert_eq!(row.headline.agent_id, "skilled");
        assert_eq!(row.headline.verdict, Verdict::SkillSupported);
        assert_eq!(row.headline.qualifiers.evidence, certifying());
        assert_eq!(row.headline.qualifiers.stress, StressMark::NotRun);
        assert_eq!(row.tier, EvaluationTier::ForwardCertifying);
        assert_eq!(row.tier, row.headline.qualifiers.tier);
        assert_eq!(row.tie_band, Some(s.tie_group));
        assert!(s.tie_group > 0);
        assert!(row.rank_eligible);
        assert_eq!(row.dsr, Some(s.deflated_sharpe));
        assert_eq!(
            row.dsr_interval,
            Some([s.dsr_ci_low.unwrap(), s.dsr_ci_high.unwrap()])
        );
        assert_eq!(row.dsr_interval_error, None);
        assert_eq!(
            row.pass_k,
            Some(PassK {
                passed: s.passed_k,
                runs_passed: s.runs_passed,
                runs_scored: s.runs_scored,
            })
        );
        let cost = row.cost.as_ref().unwrap();
        assert_eq!(cost.source, COST_SOURCE_ENTRANT_REPORTED);
        assert_eq!(cost.total, Some(s.cost));
        assert!(s.cost > 0.0);
        assert_eq!(cost.return_per_cost, s.return_per_cost);
        assert_eq!(cost.dsr_per_cost, s.dsr_per_cost);
        assert_eq!(
            row.record,
            RecordLink {
                board_path: "windows/w1/board.json".to_string(),
                window_path: Some("windows/w1/window.json".to_string()),
                link: 1,
                link_signature: "sig-1".to_string(),
            }
        );
    }

    #[test]
    fn an_ineligible_row_has_no_tie_band_and_unreported_cost_is_absent() {
        let scores = field();
        let s = &scores[1];
        let row = scored_row(
            &board(),
            2,
            "sig-2",
            s,
            &thresholds(),
            Evidence::Unknown,
            None,
        );
        assert_eq!(row.returns_provenance, None);
        assert!(!row.rank_eligible);
        assert_eq!(row.tie_band, None);
        assert_eq!(row.tier, EvaluationTier::HistoricalAdvisory);
        assert_ne!(row.headline.verdict, Verdict::SkillSupported);
        assert!(!row.headline.hard_reasons.is_empty());
        let cost = row.cost.unwrap();
        assert_eq!(s.cost, 0.0);
        assert_eq!(cost.total, None);
        assert_eq!(cost.return_per_cost, None);
    }

    #[test]
    fn a_refused_or_uncomputed_deflation_exports_no_dsr() {
        let base = field()[0].clone();
        let mut refused = base.clone();
        refused.deflation_refusal = Some("returns must vary".into());
        let mut failed = base.clone();
        failed.deflation_error = Some("periods_per_year must be positive".into());
        let mut no_interval = base.clone();
        no_interval.dsr_ci_low = None;
        no_interval.dsr_ci_error = Some("interval refused".into());
        let mut half = base.clone();
        half.dsr_ci_high = None;
        let row = |s: &CompositeScore| {
            scored_row(&board(), 1, "s", s, &thresholds(), Evidence::Unknown, None)
        };
        assert_eq!(row(&refused).dsr, None);
        assert_eq!(row(&failed).dsr, None);
        assert_eq!(row(&base).dsr, Some(base.deflated_sharpe));
        assert_eq!(row(&no_interval).dsr_interval, None);
        assert_eq!(
            row(&no_interval).dsr_interval_error.as_deref(),
            Some("interval refused")
        );
        assert_eq!(row(&half).dsr_interval, None);
    }

    #[test]
    fn a_refusal_row_has_no_figures_and_names_its_cause() {
        let row = refused_row(
            &board(),
            "sig-0",
            "late",
            "commitment after the deadline",
            &thresholds(),
            Evidence::Unknown,
        );
        assert_eq!(row.headline.verdict, Verdict::Refused);
        assert_eq!(row.headline.agent_id, "late");
        assert_eq!(
            row.headline.cause.as_deref(),
            Some("commitment after the deadline")
        );
        assert_eq!(row.tie_band, None);
        assert_eq!(row.tier, EvaluationTier::HistoricalAdvisory);
        assert!(!row.rank_eligible);
        assert_eq!((row.dsr, row.dsr_interval), (None, None));
        assert_eq!((row.pass_k, row.cost), (None, None));
        assert_eq!(row.returns_provenance, None);
        assert_eq!(row.record.link, 0);
        assert_eq!(row.record.link_signature, "sig-0");
        assert_eq!(row.record.board_path, "windows/w1/board.json");
        assert_eq!(
            row.record.window_path.as_deref(),
            Some("windows/w1/window.json")
        );
    }

    fn export() -> LeaderboardExport {
        let scores = field();
        let b = board();
        let mut rows: Vec<ExportRow> = scores
            .iter()
            .enumerate()
            .map(|(i, s)| {
                scored_row(
                    &b,
                    i + 1,
                    &format!("sig-{}", i + 1),
                    s,
                    &thresholds(),
                    certifying(),
                    Some("re-executed"),
                )
            })
            .collect();
        rows.push(refused_row(
            &b,
            "sig-0",
            "a|b\nc",
            "bad \\ reveal",
            &thresholds(),
            Evidence::Noncertifying {
                mode: "unrecorded".to_string(),
                reasons: vec!["x".to_string()],
            },
        ));
        LeaderboardExport {
            schema: SCHEMA_VERSION.to_string(),
            source: ExportSource {
                kind: SourceKind::Arena,
                scheme: "ed25519".to_string(),
                verifying_key: Some("ff".repeat(32)),
                key_pinned: true,
                completeness: "anchored".to_string(),
                completeness_limit: "rollback is undetectable".to_string(),
                head: Some(format!("1:{}", "ab".repeat(32))),
            },
            boards: vec![b],
            rows,
        }
    }

    #[test]
    fn the_canonical_json_round_trips_and_ends_with_one_newline() {
        let e = export();
        let text = to_canonical_json(&e);
        assert!(text.ends_with("}\n") && !text.ends_with("\n\n"));
        assert!(text.starts_with("{\n  \"schema\": \"sharpebench.leaderboard-export.v1\""));
        let back: LeaderboardExport = serde_json::from_str(&text).unwrap();
        assert_eq!(back, e);
        assert_eq!(to_canonical_json(&back), text);
    }

    #[test]
    fn the_markdown_names_the_source_and_every_row() {
        let e = export();
        let md = render_markdown(&e);
        let s = &field()[0];
        let lines: Vec<&str> = md.lines().collect();
        assert_eq!(lines[0], "# SharpeBench leaderboard export");
        assert!(md.contains(&format!(
            "- Source: arena, ed25519 signatures, key pinned `{}`\n",
            "ff".repeat(32)
        )));
        assert!(md.contains("- Completeness: anchored. rollback is undetectable\n"));
        assert!(md.contains(&format!("- Head: `1:{}`\n", "ab".repeat(32))));
        assert!(md.contains(&format!(
            "| w1 | forward-certifying | `windows/w1/board.json` | `{}` | yes | no | 2 | 1 |\n",
            "ab".repeat(32)
        )));
        let skilled = format!(
            "| w1 | `windows/w1/board.json#1` | skilled | skill_supported | forward-certifying | {} | {:.4} | [{:.4}, {:.4}] | passed {}/{} | yes | certifying (forward, re-executed) | {:.4} entrant_reported | - | - |",
            s.tie_group,
            s.deflated_sharpe,
            s.dsr_ci_low.unwrap(),
            s.dsr_ci_high.unwrap(),
            s.runs_passed.unwrap(),
            s.runs_scored,
            s.cost,
        );
        assert!(md.contains(&skilled), "{md}");
        let noisy = lines
            .iter()
            .find(|l| l.contains("| noisy |"))
            .expect("the noisy row");
        assert!(
            noisy.contains(" | forward-certifying | - | "),
            "no tie band: {noisy}"
        );
        assert!(noisy.contains("| unreported |"), "{noisy}");
        assert!(noisy.contains(" | no | "), "{noisy}");
        assert!(noisy.contains("failed ") || noisy.contains("passed "));
        let refused = lines.last().unwrap();
        assert_eq!(
            *refused,
            "| w1 | `windows/w1/board.json#0` | a\\|b c | refused | historical-advisory | - | - | - | - | - | noncertifying (unrecorded) | - | - | cause: bad \\\\ reveal |"
        );
        // Every row line has the header's column count once escapes are
        // discounted.
        let columns = |l: &str| l.replace("\\|", "").matches('|').count();
        let header = lines
            .iter()
            .find(|l| l.starts_with("| board | record | agent"))
            .unwrap();
        for l in lines
            .iter()
            .filter(|l| l.contains("windows/w1/board.json#"))
        {
            assert_eq!(columns(l), columns(header), "{l}");
        }
    }

    #[test]
    fn markdown_cells_spell_absence_and_each_qualifier() {
        let mut e = export();
        e.source.key_pinned = false;
        e.source.verifying_key = None;
        e.source.head = None;
        e.boards[0].certifying = None;
        e.boards[0].supplied_returns_accepted = true;
        let row = &mut e.rows[0];
        row.dsr = None;
        row.dsr_interval = None;
        row.pass_k = Some(PassK {
            passed: false,
            runs_passed: None,
            runs_scored: 4,
        });
        row.cost.as_mut().unwrap().total = None;
        row.headline.hard_reasons = vec![FailReason::DsrBelowBar, FailReason::FailedPassK];
        row.headline.cause = Some("why".into());
        let q = &mut row.headline.qualifiers;
        q.tied = true;
        q.stress = StressMark::Marked;
        q.advisory = vec![FailReason::HighSelectionGap, FailReason::OosDecay];
        q.declared_mandate = Some("m".into());
        q.rank_mode = Some("r".into());
        q.evidence = Evidence::Unknown;
        row.returns_provenance = None;
        let md = render_markdown(&e);
        assert!(
            md.contains("- Source: arena, ed25519 signatures, key embedded\n"),
            "{md}"
        );
        assert!(!md.contains("- Head:"));
        assert!(md.contains("| unrecorded | yes | 2 | 1 |"), "{md}");
        let line = md.lines().find(|l| l.contains("| skilled |")).unwrap();
        assert!(line.contains(
            "| unavailable | unavailable | failed ?/4 | yes | unknown | unreported | tied; stress marked; advisory: high_selection_gap, oos_decay; mandate: m; rank mode: r | dsr_below_bar, failed_pass_k, cause: why |"
        ), "{line}");
        assert!(md.contains("| w1 | historical-advisory | `windows/w1/board.json` |"));
        e.boards[0].certifying = Some(false);
        let md = render_markdown(&e);
        assert!(md.contains("| no | yes | 2 | 1 |"));
        assert!(md.contains("| w1 | historical-advisory | `windows/w1/board.json` |"));
    }

    #[test]
    fn a_cell_escapes_pipes_backslashes_and_control_characters() {
        assert_eq!(cell("plain"), "plain");
        assert_eq!(cell("a|b"), "a\\|b");
        assert_eq!(cell("a\\b"), "a\\\\b");
        assert_eq!(cell("a\nb\rc\td"), "a b c d");
    }
}
