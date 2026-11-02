//! `sharpebench signals <submissions.json>`: the rank-neutral fast-signal
//! report (`sharpebench_core::fast_signals`), with optional evidence read from
//! the JSON documents other commands already write.
//!
//! - `--suite-evidence <path>`: `run --json --suite-evidence` output, or its
//!   `suite_evidence` object. Supplies each agent's trial census.
//! - `--host-cost <path>`: a JSON board whose rows carry host-observed cost
//!   (`cost_source` `host_observed_usd`), as the harness function
//!   `attach_host_observed_usage` writes it.
//! - `--contamination-probe <agent>=<path>`, `--presentation-stability
//!   <agent>=<path>`: the `--json` document of that command for one agent.
//! - `--decision-stability <path>`: the `decision-stability --json` report,
//!   matched by its `agent_id`.
//!
//! Every evidence flag may repeat. Nothing here reads or writes a board row,
//! and the report never reaches a score or a rank.

use std::collections::BTreeMap;

use serde_json::Value;
use sharpebench_core::fast_signals::{HostObservedCost, PresentationCounts};
use sharpebench_core::{AgentCensus, FastSignalsReport, SignalEvidence, SignalMeasure};

use crate::emit_json;

const USAGE: &str = "usage: sharpebench signals <submissions.json> [--suite-evidence <path>] [--host-cost <board.json>] [--contamination-probe <agent>=<path>] [--presentation-stability <agent>=<path>] [--decision-stability <path>] [score flags] [--json]";

/// The repeatable evidence flags.
const EVIDENCE_FLAGS: [&str; 5] = [
    "--suite-evidence",
    "--host-cost",
    "--contamination-probe",
    "--presentation-stability",
    "--decision-stability",
];

/// Every value given for a repeatable flag, in order. A flag with no value is
/// an error.
fn flag_values<'a>(args: &'a [String], flag: &str) -> Result<Vec<&'a str>, String> {
    let mut out = Vec::new();
    for (i, arg) in args.iter().enumerate() {
        if arg == flag {
            match args.get(i + 1).filter(|v| !v.starts_with("--")) {
                Some(value) => out.push(value.as_str()),
                None => return Err(format!("{flag} requires a value")),
            }
        }
    }
    Ok(out)
}

fn read_json(path: &str) -> Result<Value, String> {
    let text =
        std::fs::read_to_string(path).map_err(|error| format!("cannot read {path}: {error}"))?;
    serde_json::from_str(&text).map_err(|error| format!("invalid JSON in {path}: {error}"))
}

/// `<agent>=<path>`, split at the last `=` so an agent id may contain one.
fn agent_and_path(value: &str) -> Result<(&str, &str), String> {
    match value.rsplit_once('=') {
        Some((agent, path)) if !agent.is_empty() && !path.is_empty() => Ok((agent, path)),
        _ => Err(format!("expected <agent>=<path>, got {value}")),
    }
}

/// The per-agent census rows of a suite-evidence document.
fn census_rows(doc: &Value) -> Result<Vec<AgentCensus>, String> {
    let evidence = doc.get("suite_evidence").unwrap_or(doc);
    let rows = evidence
        .pointer("/trials/per_agent")
        .ok_or("the suite evidence has no trials.per_agent")?;
    serde_json::from_value(rows.clone())
        .map_err(|error| format!("the suite evidence census does not parse: {error}"))
}

/// The host-observed spend of every row of a board that has one.
fn host_costs(doc: &Value) -> Result<Vec<(String, HostObservedCost)>, String> {
    let board = doc.get("board").unwrap_or(doc);
    let rows = board
        .as_array()
        .ok_or("the host-cost board is not an array")?;
    Ok(rows
        .iter()
        .filter(|row| row["cost_source"] == "host_observed_usd")
        .filter_map(|row| {
            Some((
                row["agent_id"].as_str()?.to_string(),
                HostObservedCost {
                    usd: row["cost"].as_f64()?,
                    runs: usize::try_from(row["runs_scored"].as_u64()?).ok()?,
                },
            ))
        })
        .collect())
}

/// The unavailability text of a probe document that has no result.
fn unavailable_reason(doc: &Value) -> String {
    let reason = &doc["unavailable"]["reason"];
    reason
        .as_str()
        .map_or_else(|| doc["unavailable"].to_string(), str::to_string)
}

fn contamination_edges(doc: &Value) -> Result<Result<Vec<f64>, String>, String> {
    match doc["status"].as_str() {
        Some("measured") => {
            let edges = doc
                .pointer("/result/report/edges")
                .and_then(Value::as_array)
                .ok_or("the contamination probe has no result.report.edges")?;
            edges
                .iter()
                .map(|e| e.as_f64().ok_or("a contamination edge is not a number"))
                .collect::<Result<Vec<f64>, _>>()
                .map(Ok)
                .map_err(str::to_string)
        }
        Some("unavailable") => Ok(Err(unavailable_reason(doc))),
        _ => Err("the contamination probe has no status".into()),
    }
}

/// `kind`, then every other field of the presentation, e.g. `blinded:full`.
fn variant_label(variant: &Value) -> String {
    let mut label = variant["kind"].as_str().unwrap_or("unknown").to_string();
    if let Some(fields) = variant.as_object() {
        for (key, value) in fields.iter().filter(|(k, _)| k.as_str() != "kind") {
            let text = value
                .as_str()
                .map_or_else(|| value.to_string(), str::to_string);
            label.push_str(&format!(":{key}={text}"));
        }
    }
    label
}

fn presentation_pairs(doc: &Value) -> Result<Result<Vec<PresentationCounts>, String>, String> {
    match doc["status"].as_str() {
        Some("measured") => {
            let pairs = doc
                .pointer("/result/pairs")
                .and_then(Value::as_array)
                .ok_or("the presentation suite has no result.pairs")?;
            pairs
                .iter()
                .map(|pair| {
                    let count = |field: &str| {
                        pair["comparison"][field]
                            .as_u64()
                            .and_then(|n| usize::try_from(n).ok())
                            .ok_or_else(|| format!("a presentation pair has no {field}"))
                    };
                    Ok(PresentationCounts {
                        variant: variant_label(&pair["variant"]),
                        steps: count("steps")?,
                        disagreeing_steps: count("disagreeing_steps")?,
                    })
                })
                .collect::<Result<Vec<_>, String>>()
                .map(Ok)
        }
        Some("unavailable") => Ok(Err(unavailable_reason(doc))),
        _ => Err("the presentation suite has no status".into()),
    }
}

/// `(pairs_compared, differing_pairs)`, or why the rate is unavailable.
type StabilityCounts = Result<(usize, usize), String>;

fn decision_stability(doc: &Value) -> Result<(String, StabilityCounts), String> {
    let agent = doc["agent_id"]
        .as_str()
        .ok_or("the decision-stability report has no agent_id")?
        .to_string();
    let rate = &doc["pairwise_disagreement"];
    let outcome = match rate["status"].as_str() {
        Some("available") => {
            let count = |field: &str| {
                doc[field]
                    .as_u64()
                    .and_then(|n| usize::try_from(n).ok())
                    .ok_or_else(|| format!("the decision-stability report has no {field}"))
            };
            Ok((count("pairs_compared")?, count("differing_pairs")?))
        }
        Some("unavailable") => Err(rate["reason"].as_str().unwrap_or("unavailable").to_string()),
        _ => return Err("the decision-stability report has no pairwise_disagreement".into()),
    };
    Ok((agent, outcome))
}

/// Read every evidence flag into the per-agent map the kernel takes.
pub(crate) fn evidence_from_args(
    args: &[String],
) -> Result<BTreeMap<String, SignalEvidence>, String> {
    let mut evidence: BTreeMap<String, SignalEvidence> = BTreeMap::new();
    for path in flag_values(args, "--suite-evidence")? {
        for row in census_rows(&read_json(path)?)? {
            let agent = row.agent_id.clone();
            evidence.entry(agent).or_default().census = Some(row);
        }
    }
    for path in flag_values(args, "--host-cost")? {
        for (agent, cost) in host_costs(&read_json(path)?)? {
            evidence.entry(agent).or_default().host_cost = Some(cost);
        }
    }
    for value in flag_values(args, "--contamination-probe")? {
        let (agent, path) = agent_and_path(value)?;
        let edges = contamination_edges(&read_json(path)?).map_err(|e| format!("{path}: {e}"))?;
        evidence
            .entry(agent.to_string())
            .or_default()
            .contamination_edges = Some(edges);
    }
    for value in flag_values(args, "--presentation-stability")? {
        let (agent, path) = agent_and_path(value)?;
        let pairs = presentation_pairs(&read_json(path)?).map_err(|e| format!("{path}: {e}"))?;
        evidence.entry(agent.to_string()).or_default().presentation = Some(pairs);
    }
    for path in flag_values(args, "--decision-stability")? {
        let (agent, counts) =
            decision_stability(&read_json(path)?).map_err(|e| format!("{path}: {e}"))?;
        evidence.entry(agent).or_default().decision_stability = Some(counts);
    }
    Ok(evidence)
}

/// `sharpebench signals`. Exit 2 for usage, 1 for a field or evidence file
/// that does not read.
pub fn run(args: &[String], json: bool) -> i32 {
    let Some(path) = args.get(2).filter(|p| !p.starts_with('-')) else {
        eprintln!("{USAGE}");
        return 2;
    };
    for flag in EVIDENCE_FLAGS {
        if let Err(error) = flag_values(args, flag) {
            eprintln!(
                "error: {error}
{USAGE}"
            );
            return 2;
        }
    }
    let data = match std::fs::read_to_string(path) {
        Ok(data) => data,
        Err(error) => {
            eprintln!("error: cannot read {path}: {error}");
            return 1;
        }
    };
    let (subs, _, cfg) = match crate::scoring_field_from_args(&data, args) {
        Ok(field) => field,
        Err((error, code)) => {
            eprintln!("error: {error}");
            return i32::from(code);
        }
    };
    let evidence = match evidence_from_args(args) {
        Ok(evidence) => evidence,
        Err(error) => {
            eprintln!("error: {error}");
            return 1;
        }
    };
    let report = sharpebench_core::fast_signals(&subs, &cfg, &evidence);
    if json {
        emit_json(&serde_json::json!({ "fast_signals": report }));
    } else {
        print_report(&report);
    }
    0
}

fn cell(value: Option<f64>) -> String {
    value.map_or_else(|| "n/a".to_string(), |v| format!("{v:.4}"))
}

fn print_measure(m: &SignalMeasure) {
    let direction = match m.direction {
        sharpebench_core::SignalDirection::HigherIsBetter => "higher",
        sharpebench_core::SignalDirection::LowerIsBetter => "lower",
    };
    let interval = m.interval.as_ref().map_or_else(
        || "-".to_string(),
        |i| format!("[{:.4}, {:.4}] {}", i.low, i.high, i.method),
    );
    let mut line = format!(
        "  {:<27} {:>9} {:>7} {:>6}  {interval}",
        m.id,
        cell(m.value),
        m.observations,
        direction
    );
    if let Some(reason) = &m.unavailable {
        line.push_str(&format!("  ({reason})"));
    }
    println!("{line}");
}

/// The human report: one block per agent, then the statement.
pub(crate) fn print_report(report: &FastSignalsReport) {
    println!(
        "\nFast signals ({}). Rank-neutral; never a rank, gate or verdict input.",
        report.schema
    );
    for agent in &report.agents {
        println!("\n{} ({} runs)", agent.agent_id, agent.runs);
        println!(
            "  {:<27} {:>9} {:>7} {:>6}  95% interval",
            "measure", "value", "n", "better"
        );
        for m in &agent.measures {
            print_measure(m);
        }
    }
    if !report.unmatched_evidence.is_empty() {
        println!(
            "\nEvidence for agents not in the field, not reported: {}",
            report.unmatched_evidence.join(", ")
        );
    }
    println!("\n{}", report.statement);
}
