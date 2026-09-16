use std::collections::BTreeMap;
use std::fs;

use serde_json::Value;
use sharpebench_core::candidate_lineage::{
    check_declared_source_dating, check_test_split_census_claim, content_sha256,
    date_cited_sources, resolve_split_first_date, verify_test_split_census, JournalRecord,
    SourceDatingReport, SplitDating, TestSplitCensusReport,
};
use sharpebench_core::{
    verify_candidate_lineage, CandidateLineageError, CandidateLineageLedger,
    CandidateLineageReport, CandidateLineageScore,
};
use sharpebench_sim::Dataset;

const USAGE: &str =
    "usage: sharpebench lineage <strategy-evidence.json> [--census] [--dataset <prices.csv>]... [--json]";

struct Options<'a> {
    path: &'a str,
    census: bool,
    datasets: Vec<&'a str>,
}

fn parse_options(args: &[String]) -> Option<Options<'_>> {
    let path = args.get(2).filter(|value| !value.starts_with('-'))?;
    let mut options = Options {
        path,
        census: false,
        datasets: Vec::new(),
    };
    let mut rest = args[3..].iter();
    while let Some(arg) = rest.next() {
        match arg.as_str() {
            "--census" => options.census = true,
            "--dataset" => options
                .datasets
                .push(rest.next().filter(|value| !value.starts_with('-'))?),
            _ => return None,
        }
    }
    Some(options)
}

pub(crate) fn run(args: &[String], json: bool) -> i32 {
    let Some(options) = parse_options(args) else {
        eprintln!("{USAGE}");
        return 2;
    };
    let path = options.path;
    let text = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(error) => {
            eprintln!("error: cannot read {path}: {error}");
            return 1;
        }
    };
    let calendars = match load_calendars(&options.datasets) {
        Ok(calendars) => calendars,
        Err(error) => {
            eprintln!("error: {error}");
            return 1;
        }
    };
    if options.census {
        return run_census(&text, &calendars, json);
    }
    let evidence = match parse_evidence(&text) {
        Ok(evidence) => evidence,
        Err(error) => {
            eprintln!("error: {error}");
            return 1;
        }
    };
    match verify_record(&evidence, &calendars, !options.datasets.is_empty()) {
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
        Err(RecordError::Contract(error)) => {
            eprintln!("error: {error}");
            1
        }
        Err(RecordError::Lineage(error)) => {
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
            "strategy evidence contains {count} JSONL records; extract one run so its lineage report is unambiguous, or pass --census to count test-split consultations across the journal"
        )),
    }
}

/// Split a journal into records the way SharpeArena reads it back: one record
/// per nonblank line, identified by the digest of the line with ASCII
/// whitespace stripped. A file holding one pretty-printed record is one record.
fn parse_journal(text: &str) -> Result<Vec<JournalRecord>, String> {
    let ascii_space = |c: char| matches!(c, ' ' | '\t' | '\n' | '\r' | '\x0b' | '\x0c');
    let mut records = Vec::new();
    for (index, raw) in text.split('\n').enumerate() {
        let line = raw.trim_matches(ascii_space);
        if line.is_empty() {
            continue;
        }
        match serde_json::from_str(line) {
            Ok(record) => records.push(JournalRecord {
                line: index + 1,
                record_sha256: content_sha256(line.as_bytes()),
                record,
            }),
            Err(error) => {
                let whole = text.trim_matches(ascii_space);
                return match serde_json::from_str(whole) {
                    Ok(record) => Ok(vec![JournalRecord {
                        line: 1,
                        record_sha256: content_sha256(whole.as_bytes()),
                        record,
                    }]),
                    Err(_) => Err(format!(
                        "line {}: invalid strategy evidence JSON: {error}",
                        index + 1
                    )),
                };
            }
        }
    }
    if records.is_empty() {
        return Err("strategy evidence is empty".to_owned());
    }
    Ok(records)
}

/// Bar labels of each supplied dataset, keyed by its content digest.
///
/// SharpeArena hashes CSV text read with universal newlines, so a file with
/// carriage returns is registered under the digest of its bytes and under the
/// digest of its newline-normalized text. The bars come from the simulator's
/// own CSV reader, so a window index names the same bar the kernel stepped.
fn load_calendars(paths: &[&str]) -> Result<BTreeMap<String, Vec<String>>, String> {
    let mut calendars = BTreeMap::new();
    for path in paths {
        let bytes = fs::read(path).map_err(|error| format!("cannot read {path}: {error}"))?;
        let text =
            String::from_utf8(bytes).map_err(|_| format!("dataset {path} is not UTF-8 text"))?;
        let normalized = text.replace("\r\n", "\n").replace('\r', "\n");
        let dataset = Dataset::from_csv(&normalized)
            .map_err(|error| format!("dataset {path} is not a price panel: {error}"))?;
        calendars.insert(content_sha256(text.as_bytes()), dataset.dates.clone());
        calendars.insert(content_sha256(normalized.as_bytes()), dataset.dates);
    }
    Ok(calendars)
}

enum RecordError {
    Contract(String),
    Lineage(CandidateLineageError),
}

/// Verify one completed record's lineage, its declared census, and its source
/// dating.
///
/// Source dating is attached for schema 3 records, when a cited source is
/// dated, or when `date_sources` is set. Schema 2 records cannot carry source
/// dates, so without a supplied dataset their report is unchanged.
fn verify_record(
    evidence: &Value,
    calendars: &BTreeMap<String, Vec<String>>,
    date_sources: bool,
) -> Result<CandidateLineageReport, RecordError> {
    let (ledger, scores) = extract_contract(evidence).map_err(RecordError::Contract)?;
    let mut report = verify_candidate_lineage(&ledger, &scores).map_err(RecordError::Lineage)?;
    report.declared_test_split_census =
        check_test_split_census_claim(evidence).map_err(RecordError::Lineage)?;
    let schema_3 = evidence.get("schema_version").and_then(Value::as_u64) >= Some(3);
    if schema_3
        || date_sources
        || report
            .cited_sources
            .iter()
            .any(|source| source.available_on.is_some())
    {
        let measured = date_cited_sources(
            &report.cited_sources,
            vec![
                (
                    "selection".to_owned(),
                    resolve_split_first_date(evidence.pointer("/selection/split"), calendars),
                ),
                (
                    "test".to_owned(),
                    resolve_split_first_date(evidence.pointer("/test/split"), calendars),
                ),
            ],
        );
        match evidence.get("source_dating") {
            Some(raw) => {
                let declared: SourceDatingReport =
                    serde_json::from_value(raw.clone()).map_err(|error| {
                        RecordError::Contract(format!("invalid source_dating: {error}"))
                    })?;
                check_declared_source_dating(&declared, &measured).map_err(RecordError::Lineage)?;
            }
            None if schema_3 => {
                return Err(RecordError::Contract(
                    "source_dating is required from strategy evidence schema 3".to_owned(),
                ))
            }
            None => {}
        }
        report.source_dating = Some(measured);
    }
    Ok(report)
}

fn run_census(text: &str, calendars: &BTreeMap<String, Vec<String>>, json: bool) -> i32 {
    let journal = match parse_journal(text) {
        Ok(journal) => journal,
        Err(error) => {
            eprintln!("error: {error}");
            return 1;
        }
    };
    let mut report = match verify_test_split_census(&journal) {
        Ok(report) => report,
        Err(error) => {
            eprintln!("census verification failed: {error}");
            return 1;
        }
    };
    for (entry, item) in report.journal.iter_mut().zip(&journal) {
        let record = &item.record;
        let completed_strategy = record.get("evidence_class").and_then(Value::as_str)
            == Some("retrospective_generated_strategy")
            && record.get("status").and_then(Value::as_str) == Some("completed")
            && record.get("schema_version").and_then(Value::as_u64) >= Some(2);
        if !completed_strategy {
            continue;
        }
        match verify_record(record, calendars, true) {
            Ok(lineage) => entry.lineage = Some(lineage),
            Err(RecordError::Contract(error)) => {
                eprintln!("error: line {}: {error}", item.line);
                return 1;
            }
            Err(RecordError::Lineage(error)) => {
                eprintln!("lineage verification failed: line {}: {error}", item.line);
                return 1;
            }
        }
    }
    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&report).expect("census report is JSON serializable")
        );
    } else {
        print_census(&report);
    }
    0
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
    if let Some(dating) = &report.source_dating {
        println!("\nSOURCE DATING");
        println!(
            "  cited={} dated={} undated={}",
            dating.cited_sources, dating.dated_sources, dating.undated_sources
        );
        for split in &dating.splits {
            println!("  {:<9}  {}", split.split, describe_dating(&split.dating));
        }
    }
    if let Some(census) = &report.declared_test_split_census {
        println!("\nDECLARED TEST SPLIT CENSUS");
        println!(
            "  split={} prior_consultations={} cumulative_trials={} unidentified_prior={}",
            census.test_split_sha256,
            census.prior_test_consultations,
            census.cumulative_observed_n_trials,
            census.unidentified_prior_records
        );
        println!(
            "  checked against this record only; run --census over the whole journal to check the earlier records"
        );
    }
}

fn describe_dating(dating: &SplitDating) -> String {
    match dating {
        SplitDating::Measured {
            first_date,
            sources_on_or_after_first_date,
        } => format!(
            "first_date={first_date} sources_on_or_after_first_date={sources_on_or_after_first_date}"
        ),
        SplitDating::Unavailable { reason } => format!(
            "unavailable: {}",
            serde_json::to_value(reason)
                .ok()
                .and_then(|value| value.as_str().map(str::to_owned))
                .unwrap_or_default()
        ),
    }
}

fn print_census(report: &TestSplitCensusReport) {
    println!("TEST SPLIT CENSUS");
    println!("records:               {}", report.records);
    println!("completed searches:    {}", report.completed_records);
    println!("failed searches:       {}", report.failed_records);
    println!("unidentified records:  {}", report.unidentified_records);
    println!(
        "declared censuses:     {} checked against the records before them",
        report.declared_censuses_verified
    );
    println!("scope:                 {}", report.scope);
    println!("diagnostic only; never changes rank, eligibility, or any record's trial count");
    println!("\nTEST SPLITS");
    for split in &report.splits {
        println!(
            "  {}  consultations={} cumulative_trials={} unconsulted_failures={}",
            split.test_split_sha256,
            split.test_consultations,
            split.cumulative_observed_n_trials,
            split.unconsulted_records
        );
    }
    println!("\nRECORDS");
    for entry in &report.journal {
        let test_dating = entry
            .lineage
            .as_ref()
            .and_then(|lineage| lineage.source_dating.as_ref())
            .map_or_else(
                || "n/a".to_owned(),
                |dating| {
                    dating
                        .splits
                        .iter()
                        .find(|split| split.split == "test")
                        .map_or_else(|| "n/a".to_owned(), |split| describe_dating(&split.dating))
                },
            );
        println!(
            "  line={} schema={} status={} split={} consulted={} trials={} lineage={} test_dating={}",
            entry.line,
            entry
                .schema_version
                .map_or_else(|| "n/a".to_owned(), |version| version.to_string()),
            entry.status.as_deref().unwrap_or("n/a"),
            entry.test_split_sha256.as_deref().unwrap_or("unidentified"),
            entry.test_consulted,
            entry.observed_n_trials,
            if entry.lineage.is_some() {
                "verified"
            } else {
                "not-applicable"
            },
            test_dating
        );
    }
}

fn format_optional(value: Option<f64>) -> String {
    value.map_or_else(|| "n/a".to_owned(), |number| format!("{number:.6}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    const JOURNAL: &str = include_str!("../tests/fixtures/sharpearena-census-journal.jsonl");
    const PRICES: &str = include_str!("../tests/fixtures/sharpearena-census-prices.csv");

    fn journal_line(number: usize) -> Value {
        serde_json::from_str(JOURNAL.lines().nth(number - 1).unwrap()).unwrap()
    }

    fn prices_calendar() -> BTreeMap<String, Vec<String>> {
        let dataset = Dataset::from_csv(PRICES).unwrap();
        BTreeMap::from([(content_sha256(PRICES.as_bytes()), dataset.dates)])
    }

    fn test_split(report: &CandidateLineageReport) -> &SplitDating {
        &report
            .source_dating
            .as_ref()
            .expect("dating is attached")
            .splits
            .iter()
            .find(|split| split.split == "test")
            .expect("test split is reported")
            .dating
    }

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
        assert!(error.contains("--census"));
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

        // Schema 2 carries no source dates and no census, so its report gains
        // nothing unless a dataset is supplied.
        let report = verify_record(&evidence, &BTreeMap::new(), false)
            .ok()
            .unwrap();
        assert_eq!(report.source_dating, None);
        assert_eq!(report.declared_test_split_census, None);
        let report = verify_record(&evidence, &BTreeMap::new(), true)
            .ok()
            .unwrap();
        let dating = report.source_dating.unwrap();
        assert_eq!((dating.cited_sources, dating.undated_sources), (1, 1));
        assert_eq!(
            dating.splits[1].dating,
            SplitDating::Unavailable {
                reason: sharpebench_core::candidate_lineage::SplitDateUnavailable::SplitNotRecorded
            }
        );
    }

    #[test]
    fn options_accept_census_and_repeated_datasets_and_refuse_unknown_flags() {
        let args = |rest: &[&str]| {
            ["sharpebench", "lineage"]
                .iter()
                .chain(rest)
                .map(|value| value.to_string())
                .collect::<Vec<_>>()
        };
        let parsed = args(&[
            "j.jsonl",
            "--dataset",
            "a.csv",
            "--census",
            "--dataset",
            "b.csv",
        ]);
        let options = parse_options(&parsed).unwrap();
        assert_eq!(options.path, "j.jsonl");
        assert!(options.census);
        assert_eq!(options.datasets, ["a.csv", "b.csv"]);
        let plain = args(&["j.jsonl"]);
        let options = parse_options(&plain).unwrap();
        assert!(!options.census && options.datasets.is_empty());
        for refused in [
            args(&[]),
            args(&["--census"]),
            args(&["j.jsonl", "--dataset"]),
            args(&["j.jsonl", "--dataset", "--census"]),
            args(&["j.jsonl", "--datasets", "a.csv"]),
            args(&["j.jsonl", "extra"]),
        ] {
            assert!(parse_options(&refused).is_none(), "{refused:?}");
        }
    }

    #[test]
    fn census_counts_repeated_test_consultations_across_one_journal() {
        let journal = parse_journal(JOURNAL).unwrap();
        assert_eq!(journal.len(), 5);
        assert_eq!(
            journal[0].record_sha256,
            content_sha256(JOURNAL.lines().next().unwrap().as_bytes())
        );
        let report = verify_test_split_census(&journal).unwrap();
        assert_eq!(report.records, 5);
        assert_eq!(report.completed_records, 4);
        assert_eq!(report.failed_records, 1);
        assert_eq!(report.unidentified_records, 0);
        // Line 1 is schema 2 and declares no census; lines 2 to 5 do.
        assert_eq!(report.declared_censuses_verified, 4);
        let shared = &report.journal[0].test_split_sha256;
        let group = report
            .splits
            .iter()
            .find(|split| Some(&split.test_split_sha256) == shared.as_ref())
            .unwrap();
        assert_eq!(group.test_consultations, 3);
        assert_eq!(group.cumulative_observed_n_trials, 12);
        assert_eq!(group.unconsulted_records, 1);
        assert_eq!(
            group.consultation_record_sha256,
            [0, 2, 4].map(|index| journal[index].record_sha256.clone())
        );
        let other = report
            .splits
            .iter()
            .find(|split| Some(&split.test_split_sha256) != shared.as_ref())
            .unwrap();
        assert_eq!(
            (other.test_consultations, other.cumulative_observed_n_trials),
            (1, 4)
        );
        assert!(report.scope.contains("other journal files"));
    }

    #[test]
    fn census_refuses_a_record_whose_declared_prior_consultations_are_false() {
        // Dropping the first search leaves line 3 claiming a consultation the
        // journal no longer holds: exactly what a split journal would show.
        let trimmed: String = JOURNAL
            .lines()
            .skip(1)
            .map(|line| format!("{line}\n"))
            .collect();
        let error = verify_test_split_census(&parse_journal(&trimmed).unwrap()).unwrap_err();
        assert!(error.path.starts_with("line 1."), "{error}");
        assert!(error.message.contains("declares 1 earlier"), "{error}");
    }

    #[test]
    fn census_mode_verifies_each_completed_record_and_dates_sources() {
        let calendars = prices_calendar();
        let lines: Vec<Value> = (1..=5).map(journal_line).collect();
        let late = verify_record(&lines[2], &calendars, true).ok().unwrap();
        assert_eq!(
            test_split(&late),
            &SplitDating::Measured {
                first_date: "2025-01-11".to_owned(),
                sources_on_or_after_first_date: 1
            }
        );
        let dating = late.source_dating.as_ref().unwrap();
        assert_eq!(
            (
                dating.cited_sources,
                dating.dated_sources,
                dating.undated_sources
            ),
            (3, 2, 1)
        );
        let census = late.declared_test_split_census.unwrap();
        assert_eq!(census.prior_test_consultations, 1);
        assert_eq!(census.cumulative_observed_n_trials, 8);

        let without_dataset = verify_record(&lines[2], &BTreeMap::new(), false)
            .ok()
            .unwrap();
        assert_eq!(
            test_split(&without_dataset),
            &SplitDating::Unavailable {
                reason:
                    sharpebench_core::candidate_lineage::SplitDateUnavailable::DatasetNotSupplied
            }
        );
        assert_eq!(run_census(JOURNAL, &calendars, true), 0);
        assert_eq!(run_census(JOURNAL, &BTreeMap::new(), false), 0);
    }

    #[test]
    fn a_schema_3_record_must_carry_its_census_and_source_dating() {
        let mut record = journal_line(3);
        record.as_object_mut().unwrap().remove("source_dating");
        assert!(matches!(
            verify_record(&record, &BTreeMap::new(), false),
            Err(RecordError::Contract(message)) if message.contains("source_dating is required")
        ));
        let mut record = journal_line(3);
        record.as_object_mut().unwrap().remove("test_split_census");
        assert!(matches!(
            verify_record(&record, &BTreeMap::new(), false),
            Err(RecordError::Lineage(error)) if error.path == "test_split_census"
        ));
    }

    #[test]
    fn a_declared_first_date_that_contradicts_the_dataset_is_refused() {
        let mut record = journal_line(3);
        record["source_dating"]["splits"][1]["first_date"] = Value::from("2025-01-12");
        record["source_dating"]["splits"][1]["sources_on_or_after_first_date"] = Value::from(0);
        // Without the dataset the claim is neither trusted nor refused.
        assert!(verify_record(&record, &BTreeMap::new(), false).is_ok());
        assert!(matches!(
            verify_record(&record, &prices_calendar(), true),
            Err(RecordError::Lineage(error)) if error.path == "source_dating.splits[1]"
        ));
        let mut record = journal_line(3);
        record["source_dating"]["undated_sources"] = Value::from(0);
        assert!(matches!(
            verify_record(&record, &BTreeMap::new(), false),
            Err(RecordError::Lineage(error)) if error.path == "source_dating"
        ));
        let mut record = journal_line(3);
        record["source_dating"] = Value::from("dated");
        assert!(matches!(
            verify_record(&record, &BTreeMap::new(), false),
            Err(RecordError::Contract(message)) if message.contains("invalid source_dating")
        ));
    }

    #[test]
    fn a_carriage_return_dataset_resolves_under_the_normalized_digest() {
        let scratch = std::env::temp_dir().join(format!(
            "sharpebench-lineage-crlf-{}.csv",
            std::process::id()
        ));
        fs::write(&scratch, PRICES.replace('\n', "\r\n")).unwrap();
        let calendars = load_calendars(&[scratch.to_str().unwrap()]);
        fs::remove_file(&scratch).unwrap();
        let calendars = calendars.unwrap();
        assert_eq!(calendars.len(), 2);
        assert_eq!(
            calendars.get(&content_sha256(PRICES.as_bytes())),
            prices_calendar().values().next()
        );
        assert!(load_calendars(&["no-such-dataset.csv"])
            .unwrap_err()
            .contains("cannot read"));
    }

    #[test]
    fn journal_parsing_matches_the_arena_reader() {
        let journal = parse_journal("\n{\"a\":1}\r\n\n {\"b\":2}\x0b\n").unwrap();
        assert_eq!(
            journal.iter().map(|item| item.line).collect::<Vec<_>>(),
            [2, 4]
        );
        assert_eq!(journal[0].record_sha256, content_sha256(b"{\"a\":1}"));
        assert_eq!(journal[1].record_sha256, content_sha256(b"{\"b\":2}"));
        let pretty = parse_journal("{\n  \"a\": 1\n}\n").unwrap();
        assert_eq!(pretty.len(), 1);
        assert_eq!(pretty[0].record_sha256, content_sha256(b"{\n  \"a\": 1\n}"));
        assert!(parse_journal("{\"a\":1}\nnot-json\n")
            .unwrap_err()
            .starts_with("line 2:"));
        assert_eq!(
            parse_journal(" \n\n").unwrap_err(),
            "strategy evidence is empty"
        );
    }
}
