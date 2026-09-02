use std::fs;

use serde_json::Value;
use sharpebench_core::{
    verify_candidate_lineage, CandidateLineageLedger, CandidateLineageReport, CandidateLineageScore,
};

pub(crate) fn run(args: &[String], json: bool) -> i32 {
    let Some(path) = args.get(2).filter(|value| !value.starts_with('-')) else {
        eprintln!("usage: sharpebench lineage <strategy-evidence.json> [--json]");
        return 2;
    };
    let text = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(error) => {
            eprintln!("error: cannot read {path}: {error}");
            return 1;
        }
    };
    let evidence = match parse_evidence(&text) {
        Ok(evidence) => evidence,
        Err(error) => {
            eprintln!("error: {error}");
            return 1;
        }
    };
    let (ledger, scores) = match extract_contract(&evidence) {
        Ok(contract) => contract,
        Err(error) => {
            eprintln!("error: {error}");
            return 1;
        }
    };
    match verify_candidate_lineage(&ledger, &scores) {
        Ok(report) => {
            if json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&report)
                        .expect("lineage report is JSON serializable")
                );
            } else {
                print_report(&report);
            }
            0
        }
        Err(error) => {
            eprintln!("lineage verification failed: {error}");
            1
        }
    }
}

fn parse_evidence(text: &str) -> Result<Value, String> {
    if let Ok(value) = serde_json::from_str(text) {
        return Ok(value);
    }
    let records: Result<Vec<Value>, _> = text
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(serde_json::from_str)
        .collect();
    let mut records =
        records.map_err(|error| format!("invalid strategy evidence JSON: {error}"))?;
    match records.len() {
        0 => Err("strategy evidence is empty".to_owned()),
        1 => Ok(records.remove(0)),
        count => Err(format!(
            "strategy evidence contains {count} JSONL records; extract one run so its lineage report is unambiguous"
        )),
    }
}

fn extract_contract(
    evidence: &Value,
) -> Result<(CandidateLineageLedger, Vec<CandidateLineageScore>), String> {
    if evidence.get("schema_version").and_then(Value::as_u64) < Some(2) {
        return Err("strategy evidence schema_version must be 2 or newer".to_owned());
    }
    if evidence.get("evidence_class").and_then(Value::as_str)
        != Some("retrospective_generated_strategy")
    {
        return Err("evidence_class must be retrospective_generated_strategy".to_owned());
    }
    if evidence.get("status").and_then(Value::as_str) != Some("completed") {
        return Err("strategy evidence status must be completed".to_owned());
    }
    let ledger_value = evidence
        .pointer("/generation/edge_manifest_ledger")
        .ok_or_else(|| {
            "generation.edge_manifest_ledger is missing from the strategy evidence".to_owned()
        })?;
    let ledger: CandidateLineageLedger = serde_json::from_value(ledger_value.clone())
        .map_err(|error| format!("invalid generation.edge_manifest_ledger: {error}"))?;
    let observed = evidence
        .pointer("/generation/observed_n_trials")
        .and_then(Value::as_u64)
        .and_then(|value| usize::try_from(value).ok())
        .ok_or_else(|| "generation.observed_n_trials must be an integer".to_owned())?;
    if observed != ledger.summary.observed_trials {
        return Err(format!(
            "generation.observed_n_trials is {observed}, but the ledger summary claims {}",
            ledger.summary.observed_trials
        ));
    }
    if evidence
        .pointer("/generation/n_trials_source")
        .and_then(Value::as_str)
        != Some(ledger.summary.n_trials_source.as_str())
    {
        return Err("generation.n_trials_source does not match the ledger summary".to_owned());
    }
    if evidence
        .pointer("/selection/metric")
        .and_then(Value::as_str)
        != Some("median per-seed deflated_sharpe")
    {
        return Err("selection.metric must be median per-seed deflated_sharpe".to_owned());
    }

    let scores = extract_scores(evidence)?;
    Ok((ledger, scores))
}

fn extract_scores(evidence: &Value) -> Result<Vec<CandidateLineageScore>, String> {
    let score_object = evidence
        .pointer("/selection/scores")
        .and_then(Value::as_object)
        .ok_or_else(|| "selection.scores must be an object".to_owned())?;
    let mut scores = Vec::with_capacity(score_object.len());
    for (candidate_id, rows) in score_object {
        let rows = rows
            .as_array()
            .ok_or_else(|| format!("selection.scores.{candidate_id} must be an array"))?;
        if rows.is_empty() {
            return Err(format!(
                "selection.scores.{candidate_id} must contain at least one seed score"
            ));
        }
        let mut values = Vec::with_capacity(rows.len());
        for (index, row) in rows.iter().enumerate() {
            let value = row
                .pointer("/score/deflated_sharpe")
                .and_then(Value::as_f64)
                .ok_or_else(|| {
                    format!(
                        "selection.scores.{candidate_id}[{index}].score.deflated_sharpe must be a number"
                    )
                })?;
            if !value.is_finite() {
                return Err(format!(
                    "selection.scores.{candidate_id}[{index}].score.deflated_sharpe must be finite"
                ));
            }
            values.push(value);
        }
        values.sort_by(f64::total_cmp);
        scores.push(CandidateLineageScore {
            candidate_id: candidate_id.clone(),
            median_deflated_sharpe: median(&values),
        });
    }
    scores.sort_by(|left, right| left.candidate_id.cmp(&right.candidate_id));
    Ok(scores)
}

fn median(sorted: &[f64]) -> f64 {
    if sorted.len() % 2 == 1 {
        sorted[sorted.len() / 2]
    } else {
        let upper = sorted.len() / 2;
        (sorted[upper - 1] + sorted[upper]) / 2.0
    }
}

fn print_report(report: &CandidateLineageReport) {
    println!("VERIFIED candidate lineage");
    println!("observed trials:       {}", report.observed_trials);
    println!("scored candidates:     {}", report.scored_candidates);
    println!("strategy families:     {}", report.family_count);
    println!("ancestry edges:        {}", report.ancestry_edges);
    println!("cited sources:         {}", report.cited_source_count);
    println!("trial denominator:     {}", report.trial_denominator);
    println!("family grouping:       diagnostic only; never changes rank or trial count");
    println!("\nFAMILIES");
    for family in &report.families {
        let best = format_optional(family.best_median_deflated_sharpe);
        let median = format_optional(family.family_median_deflated_sharpe);
        let gap = format_optional(family.best_to_median_gap);
        println!(
            "  {}  observed={} scored={} best={} median={} gap={}",
            family.family_digest,
            family.observed_trials,
            family.scored_candidates,
            best,
            median,
            gap
        );
    }
    if report
        .ancestry
        .iter()
        .any(|candidate| candidate.lineage_status != "host-derived-unreferenced")
    {
        println!("\nANCESTRY");
        for candidate in &report.ancestry {
            if candidate.lineage_status == "host-derived-unreferenced" {
                continue;
            }
            println!(
                "  trial={} candidate={} status={} parents={} sources={}",
                candidate.trial_ordinal,
                candidate.candidate_id.as_deref().unwrap_or("unparsed"),
                candidate.lineage_status,
                candidate.parent_candidate_digests.len(),
                candidate.idea_source_digests.len()
            );
        }
    }
    if !report.cited_sources.is_empty() {
        println!("\nSOURCES");
        for source in &report.cited_sources {
            println!(
                "  {}  {}  {}",
                source.source_type,
                source.source_digest,
                source.url_or_doi.as_deref().unwrap_or("unlocated")
            );
        }
    }
}

fn format_optional(value: Option<f64>) -> String {
    value.map_or_else(|| "n/a".to_owned(), |number| format!("{number:.6}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_an_empty_candidate_score_array() {
        let evidence = serde_json::json!({
            "selection": {"scores": {"candidate": []}}
        });
        let error = extract_scores(&evidence).unwrap_err();
        assert!(error.contains("must contain at least one seed score"));
    }

    #[test]
    fn median_uses_all_seed_scores() {
        assert_eq!(median(&[0.1, 0.3, 0.9]), 0.3);
        assert_eq!(median(&[0.1, 0.3, 0.7, 0.9]), 0.5);
    }

    #[test]
    fn refuses_to_silently_choose_one_run_from_a_multi_record_journal() {
        let error = parse_evidence("{\"run\":1}\n{\"run\":2}\n").unwrap_err();
        assert!(error.contains("2 JSONL records"));
    }

    #[test]
    fn consumes_the_sharpearena_v2_fixture_end_to_end() {
        let evidence: Value = serde_json::from_str(include_str!(
            "../tests/fixtures/sharpearena-lineage-v2.json"
        ))
        .unwrap();
        let (ledger, scores) = extract_contract(&evidence).unwrap();
        let report = verify_candidate_lineage(&ledger, &scores).unwrap();

        assert_eq!(report.observed_trials, 1);
        assert_eq!(report.trial_denominator, 1);
        assert_eq!(report.families[0].family_median_deflated_sharpe, Some(0.5));
        assert_eq!(report.cited_sources[0].source_type, "repository");
    }
}
