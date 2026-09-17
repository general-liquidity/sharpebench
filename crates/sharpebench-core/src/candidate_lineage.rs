//! Independent verification of SharpeArena generated-candidate lineage.
//!
//! Lineage is diagnostic evidence. It reports where candidates came from and
//! whether parameter variants inside one host-derived family were robust. It
//! never changes eligibility, ranking, or the observed trial denominator.

use std::collections::{BTreeMap, BTreeSet};
use std::error::Error;
use std::fmt;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};

const EVIDENCE_CLASS: &str = "edge_manifest_candidate_pool";
const FAMILY_ROLE: &str = "diagnostic-only-never-a-trial-deduplicator";
const TRIAL_SOURCE: &str = "ledger-counted-before-validation-and-deduplication";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CandidateLineageLedger {
    pub summary: CandidateLineageSummary,
    pub records: Vec<CandidateLineageRecord>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CandidateLineageSummary {
    pub schema_version: u64,
    pub evidence_class: String,
    pub model_digest: String,
    pub split_plan_sha256: String,
    pub observed_trials: usize,
    pub invalid: usize,
    pub duplicates: usize,
    pub selectable: usize,
    pub families: Vec<CandidateFamilyCount>,
    pub family_count: usize,
    pub generator_identity_sha256: String,
    pub plan_bound_idea_sources: usize,
    pub family_grouping_role: String,
    pub n_trials_source: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CandidateFamilyCount {
    pub family_digest: String,
    pub observed_trials: usize,
    pub selectable: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CandidateLineageRecord {
    pub schema_version: u64,
    pub evidence_class: String,
    pub trial_ordinal: usize,
    pub raw_candidate: Value,
    pub raw_candidate_sha256: String,
    pub manifest: Option<Value>,
    pub manifest_sha256: Option<String>,
    pub invalid_reason: Option<String>,
    pub duplicate_of_ordinal: Option<usize>,
    pub model_digest: String,
    pub split_plan_sha256: String,
    pub family_preimage: Value,
    pub family_digest: String,
    pub declared_lineage: Option<DeclaredCandidateLineage>,
    pub parent_candidate_digests: Vec<String>,
    pub generator_identity: Value,
    pub generator_identity_sha256: String,
    pub idea_provenance: Vec<IdeaProvenance>,
    pub lineage_status: String,
    pub lineage_binding_sha256: String,
    pub binding_sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeclaredCandidateLineage {
    pub parent_candidate_ids: Vec<String>,
    pub idea_source_digests: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct IdeaProvenance {
    pub source_type: String,
    pub source_digest: String,
    pub url_or_doi: Option<String>,
    pub commit: Option<String>,
    pub authors: Vec<String>,
    pub license: Option<String>,
    /// Operator-stated first calendar day (`YYYY-MM-DD`) the source content
    /// existed. Absent on undated sources and on every record written before
    /// SharpeArena strategy evidence schema 3.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub available_on: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CandidateLineageScore {
    pub candidate_id: String,
    pub median_deflated_sharpe: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CandidateLineageReport {
    pub schema_version: u64,
    pub evidence_class: String,
    pub verified: bool,
    pub observed_trials: usize,
    pub scored_candidates: usize,
    pub invalid_trials: usize,
    pub duplicate_trials: usize,
    pub family_count: usize,
    pub ancestry_edges: usize,
    pub cited_source_count: usize,
    pub plan_bound_idea_sources: usize,
    pub generator_identity_sha256: String,
    pub trial_denominator: usize,
    pub family_grouping_affects_trial_count: bool,
    pub families: Vec<CandidateFamilyRobustness>,
    pub ancestry: Vec<CandidateAncestry>,
    pub cited_sources: Vec<IdeaProvenance>,
    /// Cited sources dated on or after each split's first bar. Left unset by
    /// [`verify_candidate_lineage`]; the caller attaches it with
    /// [`date_cited_sources`] once it knows the split calendars.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_dating: Option<SourceDatingReport>,
    /// The producer's census of earlier reads of this record's test split,
    /// checked for internal consistency only. Checking it against the journal
    /// needs [`verify_test_split_census`] over every record of that journal.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub declared_test_split_census: Option<TestSplitCensusClaim>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CandidateFamilyRobustness {
    pub family_digest: String,
    pub observed_trials: usize,
    pub selectable_candidates: usize,
    pub scored_candidates: usize,
    pub best_median_deflated_sharpe: Option<f64>,
    pub family_median_deflated_sharpe: Option<f64>,
    pub best_to_median_gap: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CandidateAncestry {
    pub trial_ordinal: usize,
    pub candidate_id: Option<String>,
    pub raw_candidate_sha256: String,
    pub family_digest: String,
    pub parent_candidate_digests: Vec<String>,
    pub idea_source_digests: Vec<String>,
    pub lineage_status: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CandidateLineageError {
    pub path: String,
    pub message: String,
}

impl CandidateLineageError {
    fn at(path: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            path: path.into(),
            message: message.into(),
        }
    }
}

impl fmt::Display for CandidateLineageError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}: {}", self.path, self.message)
    }
}

impl Error for CandidateLineageError {}

/// Verify an Arena ledger and compute within-family robustness diagnostics.
///
/// `scores` contains the validation-split median DSR for every selectable
/// candidate. Exact coverage is required: an omitted selectable candidate or a
/// score for an unrecorded candidate is a refusal, not a partial report.
pub fn verify_candidate_lineage(
    ledger: &CandidateLineageLedger,
    scores: &[CandidateLineageScore],
) -> Result<CandidateLineageReport, CandidateLineageError> {
    verify_summary_header(&ledger.summary)?;

    let mut score_map = BTreeMap::new();
    for (index, score) in scores.iter().enumerate() {
        if score.candidate_id.is_empty() {
            return Err(CandidateLineageError::at(
                format!("scores[{index}].candidate_id"),
                "must not be empty",
            ));
        }
        if !score.median_deflated_sharpe.is_finite() {
            return Err(CandidateLineageError::at(
                format!("scores[{index}].median_deflated_sharpe"),
                "must be finite",
            ));
        }
        if score_map
            .insert(score.candidate_id.clone(), score.median_deflated_sharpe)
            .is_some()
        {
            return Err(CandidateLineageError::at(
                format!("scores[{index}].candidate_id"),
                "must be unique",
            ));
        }
    }

    let mut earlier_by_id = BTreeMap::new();
    let mut earlier_digests = BTreeSet::new();
    let mut selectable_ids = BTreeSet::new();
    let mut recomputed_families: BTreeMap<String, CandidateFamilyAccumulator> = BTreeMap::new();
    let mut ancestry = Vec::with_capacity(ledger.records.len());
    let mut cited_sources = BTreeMap::new();
    let mut invalid = 0;
    let mut duplicates = 0;
    let mut selectable = 0;
    let mut ancestry_edges = 0;

    for (index, record) in ledger.records.iter().enumerate() {
        verify_record_header(record, index, &ledger.summary)?;
        verify_record_digests(record, index)?;
        verify_record_lineage(record, index, &earlier_by_id, &earlier_digests)?;

        let candidate_id = candidate_id(&record.raw_candidate).map(str::to_owned);
        let is_selectable =
            record.invalid_reason.is_none() && record.duplicate_of_ordinal.is_none();
        invalid += usize::from(record.invalid_reason.is_some());
        duplicates += usize::from(record.duplicate_of_ordinal.is_some());
        selectable += usize::from(is_selectable);

        if is_selectable {
            let id = candidate_id.as_ref().ok_or_else(|| {
                CandidateLineageError::at(
                    format!("records[{index}].raw_candidate.id"),
                    "a selectable candidate must have a string id",
                )
            })?;
            if !selectable_ids.insert(id.clone()) {
                return Err(CandidateLineageError::at(
                    format!("records[{index}].raw_candidate.id"),
                    "selectable candidate ids must be unique",
                ));
            }
        }

        let family = recomputed_families
            .entry(record.family_digest.clone())
            .or_default();
        family.observed_trials += 1;
        family.selectable_candidates += usize::from(is_selectable);
        if let Some(id) = candidate_id.as_ref() {
            if is_selectable {
                if let Some(score) = score_map.get(id) {
                    family.scores.push(*score);
                }
            }
            if record.invalid_reason.is_none()
                && earlier_by_id
                    .insert(id.clone(), record.raw_candidate_sha256.clone())
                    .is_some()
            {
                return Err(CandidateLineageError::at(
                    format!("records[{index}].raw_candidate.id"),
                    "valid candidate ids must be unique",
                ));
            }
        }

        for source in &record.idea_provenance {
            if let Some(previous) = cited_sources.insert(source.source_digest.clone(), source) {
                if previous != source {
                    return Err(CandidateLineageError::at(
                        format!("records[{index}].idea_provenance"),
                        "one source digest maps to conflicting metadata",
                    ));
                }
            }
        }
        ancestry_edges += record.parent_candidate_digests.len();
        ancestry.push(CandidateAncestry {
            trial_ordinal: record.trial_ordinal,
            candidate_id,
            raw_candidate_sha256: record.raw_candidate_sha256.clone(),
            family_digest: record.family_digest.clone(),
            parent_candidate_digests: record.parent_candidate_digests.clone(),
            idea_source_digests: record
                .idea_provenance
                .iter()
                .map(|source| source.source_digest.clone())
                .collect(),
            lineage_status: record.lineage_status.clone(),
        });
        earlier_digests.insert(record.raw_candidate_sha256.clone());
    }

    if ledger.summary.observed_trials != ledger.records.len() {
        return Err(CandidateLineageError::at(
            "summary.observed_trials",
            format!(
                "claims {}, but the ledger contains {} rows",
                ledger.summary.observed_trials,
                ledger.records.len()
            ),
        ));
    }
    compare_count("summary.invalid", ledger.summary.invalid, invalid)?;
    compare_count("summary.duplicates", ledger.summary.duplicates, duplicates)?;
    compare_count("summary.selectable", ledger.summary.selectable, selectable)?;
    compare_count(
        "summary.family_count",
        ledger.summary.family_count,
        recomputed_families.len(),
    )?;
    verify_family_summary(&ledger.summary.families, &recomputed_families)?;
    if cited_sources.len() > ledger.summary.plan_bound_idea_sources {
        return Err(CandidateLineageError::at(
            "summary.plan_bound_idea_sources",
            format!(
                "claims {}, but {} distinct cited sources were resolved",
                ledger.summary.plan_bound_idea_sources,
                cited_sources.len()
            ),
        ));
    }

    let score_ids: BTreeSet<_> = score_map.keys().cloned().collect();
    if score_ids != selectable_ids {
        let missing: Vec<_> = selectable_ids.difference(&score_ids).cloned().collect();
        let extra: Vec<_> = score_ids.difference(&selectable_ids).cloned().collect();
        return Err(CandidateLineageError::at(
            "scores",
            format!(
                "must cover selectable candidates exactly; missing={missing:?}, extra={extra:?}"
            ),
        ));
    }

    let families = recomputed_families
        .into_iter()
        .map(|(family_digest, mut family)| {
            family.scores.sort_by(f64::total_cmp);
            let best = family.scores.last().copied();
            let middle = median(&family.scores);
            CandidateFamilyRobustness {
                family_digest,
                observed_trials: family.observed_trials,
                selectable_candidates: family.selectable_candidates,
                scored_candidates: family.scores.len(),
                best_median_deflated_sharpe: best,
                family_median_deflated_sharpe: middle,
                best_to_median_gap: best.zip(middle).map(|(top, center)| top - center),
            }
        })
        .collect();

    Ok(CandidateLineageReport {
        schema_version: 1,
        evidence_class: "verified_candidate_lineage_diagnostic".to_owned(),
        verified: true,
        observed_trials: ledger.records.len(),
        scored_candidates: scores.len(),
        invalid_trials: invalid,
        duplicate_trials: duplicates,
        family_count: ledger.summary.family_count,
        ancestry_edges,
        cited_source_count: cited_sources.len(),
        plan_bound_idea_sources: ledger.summary.plan_bound_idea_sources,
        generator_identity_sha256: ledger.summary.generator_identity_sha256.clone(),
        trial_denominator: ledger.records.len(),
        family_grouping_affects_trial_count: false,
        families,
        ancestry,
        cited_sources: cited_sources.into_values().cloned().collect(),
        source_dating: None,
        declared_test_split_census: None,
    })
}

#[derive(Default)]
struct CandidateFamilyAccumulator {
    observed_trials: usize,
    selectable_candidates: usize,
    scores: Vec<f64>,
}

fn verify_summary_header(summary: &CandidateLineageSummary) -> Result<(), CandidateLineageError> {
    if summary.schema_version < 2 {
        return Err(CandidateLineageError::at(
            "summary.schema_version",
            "lineage verification requires Arena ledger schema version 2 or newer",
        ));
    }
    compare_string(
        "summary.evidence_class",
        &summary.evidence_class,
        EVIDENCE_CLASS,
    )?;
    compare_string(
        "summary.family_grouping_role",
        &summary.family_grouping_role,
        FAMILY_ROLE,
    )?;
    compare_string(
        "summary.n_trials_source",
        &summary.n_trials_source,
        TRIAL_SOURCE,
    )
}

fn verify_record_header(
    record: &CandidateLineageRecord,
    index: usize,
    summary: &CandidateLineageSummary,
) -> Result<(), CandidateLineageError> {
    if record.schema_version < 2 {
        return Err(CandidateLineageError::at(
            format!("records[{index}].schema_version"),
            "lineage verification requires schema version 2 or newer",
        ));
    }
    if record.evidence_class != EVIDENCE_CLASS {
        return Err(CandidateLineageError::at(
            format!("records[{index}].evidence_class"),
            format!("must equal {EVIDENCE_CLASS:?}"),
        ));
    }
    if record.trial_ordinal != index {
        return Err(CandidateLineageError::at(
            format!("records[{index}].trial_ordinal"),
            format!("must be {index}"),
        ));
    }
    if record.model_digest != summary.model_digest {
        return Err(CandidateLineageError::at(
            format!("records[{index}].model_digest"),
            "does not match the summary",
        ));
    }
    if record.split_plan_sha256 != summary.split_plan_sha256 {
        return Err(CandidateLineageError::at(
            format!("records[{index}].split_plan_sha256"),
            "does not match the summary",
        ));
    }
    if record.generator_identity_sha256 != summary.generator_identity_sha256 {
        return Err(CandidateLineageError::at(
            format!("records[{index}].generator_identity_sha256"),
            "does not match the summary",
        ));
    }
    if let Some(duplicate) = record.duplicate_of_ordinal {
        if duplicate >= index {
            return Err(CandidateLineageError::at(
                format!("records[{index}].duplicate_of_ordinal"),
                "must reference an earlier row",
            ));
        }
    }
    Ok(())
}

fn verify_record_digests(
    record: &CandidateLineageRecord,
    index: usize,
) -> Result<(), CandidateLineageError> {
    compare_digest(
        format!("records[{index}].raw_candidate_sha256"),
        &record.raw_candidate_sha256,
        &record.raw_candidate,
    )?;
    let derived_family = derive_arena_strategy_family(record, index)?;
    if record.family_preimage != derived_family {
        return Err(CandidateLineageError::at(
            format!("records[{index}].family_preimage"),
            "does not match the family derived independently from the raw candidate",
        ));
    }
    compare_digest(
        format!("records[{index}].family_digest"),
        &record.family_digest,
        &record.family_preimage,
    )?;
    compare_digest(
        format!("records[{index}].generator_identity_sha256"),
        &record.generator_identity_sha256,
        &record.generator_identity,
    )?;
    let identity_digest = record
        .generator_identity
        .get("digest")
        .and_then(Value::as_str);
    if identity_digest != Some(record.model_digest.as_str()) {
        return Err(CandidateLineageError::at(
            format!("records[{index}].generator_identity.digest"),
            "does not match model_digest",
        ));
    }

    let manifest_digest = record.manifest.as_ref().map(canonical_sha256).transpose()?;
    if manifest_digest != record.manifest_sha256 {
        return Err(CandidateLineageError::at(
            format!("records[{index}].manifest_sha256"),
            "does not match the canonical manifest",
        ));
    }

    let binding = serde_json::json!({
        "raw_candidate_sha256": record.raw_candidate_sha256,
        "manifest_sha256": record.manifest_sha256,
        "model_digest": record.model_digest,
        "split_plan_sha256": record.split_plan_sha256,
        "trial_ordinal": record.trial_ordinal,
    });
    compare_digest(
        format!("records[{index}].binding_sha256"),
        &record.binding_sha256,
        &binding,
    )
}

fn derive_arena_strategy_family(
    record: &CandidateLineageRecord,
    index: usize,
) -> Result<Value, CandidateLineageError> {
    if record.invalid_reason.is_some() {
        return Ok(serde_json::json!({
            "unparsed_raw_candidate_sha256": record.raw_candidate_sha256
        }));
    }
    let path = format!("records[{index}].raw_candidate");
    let long_when = record
        .raw_candidate
        .get("long_when")
        .ok_or_else(|| CandidateLineageError::at(format!("{path}.long_when"), "is missing"))?;
    let short_when = match record.raw_candidate.get("short_when") {
        None | Some(Value::Null) => Value::Null,
        Some(condition) => derive_condition_shape(condition, &format!("{path}.short_when"))?,
    };
    let manifest = record
        .raw_candidate
        .get("edge_manifest")
        .and_then(Value::as_object)
        .ok_or_else(|| {
            CandidateLineageError::at(format!("{path}.edge_manifest"), "must be an object")
        })?;
    let regimes = sorted_string_array(
        manifest.get("regimes"),
        &format!("{path}.edge_manifest.regimes"),
    )?;
    let instruments = sorted_string_array(
        manifest.get("instruments"),
        &format!("{path}.edge_manifest.instruments"),
    )?;
    Ok(serde_json::json!({
        "long_when": derive_condition_shape(long_when, &format!("{path}.long_when"))?,
        "short_when": short_when,
        "regimes": regimes,
        "instruments": instruments,
    }))
}

fn derive_condition_shape(value: &Value, path: &str) -> Result<Value, CandidateLineageError> {
    let object = value
        .as_object()
        .ok_or_else(|| CandidateLineageError::at(path, "must be an object"))?;
    let operator = object
        .get("op")
        .and_then(Value::as_str)
        .ok_or_else(|| CandidateLineageError::at(format!("{path}.op"), "must be a string"))?;
    match operator {
        "gt" | "gte" | "lt" | "lte" => Ok(serde_json::json!({
            "op": operator,
            "left": derive_value_shape(
                object.get("left"),
                &format!("{path}.left"),
            )?,
            "right": derive_value_shape(
                object.get("right"),
                &format!("{path}.right"),
            )?,
        })),
        "and" | "or" => {
            let conditions = object
                .get("conditions")
                .and_then(Value::as_array)
                .ok_or_else(|| {
                    CandidateLineageError::at(format!("{path}.conditions"), "must be an array")
                })?;
            let derived: Result<Vec<_>, _> = conditions
                .iter()
                .enumerate()
                .map(|(condition_index, condition)| {
                    derive_condition_shape(
                        condition,
                        &format!("{path}.conditions[{condition_index}]"),
                    )
                })
                .collect();
            Ok(serde_json::json!({"op": operator, "conditions": derived?}))
        }
        "not" => Ok(serde_json::json!({
            "op": "not",
            "condition": derive_condition_shape(
                object.get("condition").ok_or_else(|| {
                    CandidateLineageError::at(
                        format!("{path}.condition"),
                        "is missing",
                    )
                })?,
                &format!("{path}.condition"),
            )?,
        })),
        _ => Err(CandidateLineageError::at(
            format!("{path}.op"),
            format!("unsupported strategy operator {operator:?}"),
        )),
    }
}

fn derive_value_shape(value: Option<&Value>, path: &str) -> Result<Value, CandidateLineageError> {
    let object = value
        .and_then(Value::as_object)
        .ok_or_else(|| CandidateLineageError::at(path, "must be an object"))?;
    if object.contains_key("constant") {
        return Ok(serde_json::json!({"constant": "parameter"}));
    }
    let indicator = object
        .get("indicator")
        .and_then(Value::as_str)
        .ok_or_else(|| {
            CandidateLineageError::at(format!("{path}.indicator"), "must be a string")
        })?;
    if indicator == "price" {
        Ok(serde_json::json!({"indicator": indicator}))
    } else {
        Ok(serde_json::json!({
            "indicator": indicator,
            "window": "parameter"
        }))
    }
}

fn sorted_string_array(
    value: Option<&Value>,
    path: &str,
) -> Result<Vec<String>, CandidateLineageError> {
    let items = value
        .and_then(Value::as_array)
        .ok_or_else(|| CandidateLineageError::at(path, "must be an array"))?;
    let mut strings: Vec<String> = items
        .iter()
        .enumerate()
        .map(|(item_index, item)| {
            item.as_str().map(str::to_owned).ok_or_else(|| {
                CandidateLineageError::at(format!("{path}[{item_index}]"), "must be a string")
            })
        })
        .collect::<Result<_, _>>()?;
    strings.sort();
    Ok(strings)
}

fn verify_record_lineage(
    record: &CandidateLineageRecord,
    index: usize,
    earlier_by_id: &BTreeMap<String, String>,
    earlier_digests: &BTreeSet<String>,
) -> Result<(), CandidateLineageError> {
    let path = format!("records[{index}]");
    let unique_parents: BTreeSet<_> = record.parent_candidate_digests.iter().collect();
    if unique_parents.len() != record.parent_candidate_digests.len() {
        return Err(CandidateLineageError::at(
            format!("{path}.parent_candidate_digests"),
            "must not contain duplicates",
        ));
    }
    for digest in &record.parent_candidate_digests {
        if !earlier_digests.contains(digest) {
            return Err(CandidateLineageError::at(
                format!("{path}.parent_candidate_digests"),
                format!("{digest:?} does not identify an earlier row"),
            ));
        }
    }

    let source_digests: Vec<_> = record
        .idea_provenance
        .iter()
        .map(|source| source.source_digest.clone())
        .collect();
    let unique_sources: BTreeSet<_> = source_digests.iter().collect();
    if unique_sources.len() != source_digests.len() {
        return Err(CandidateLineageError::at(
            format!("{path}.idea_provenance"),
            "source digests must not contain duplicates",
        ));
    }
    for (source_index, source) in record.idea_provenance.iter().enumerate() {
        verify_source(source, &format!("{path}.idea_provenance[{source_index}]"))?;
    }

    match (&record.declared_lineage, record.lineage_status.as_str()) {
        (Some(declared), "declared") => {
            let expected_parents: Result<Vec<_>, _> = declared
                .parent_candidate_ids
                .iter()
                .map(|id| {
                    earlier_by_id.get(id).cloned().ok_or_else(|| {
                        CandidateLineageError::at(
                            format!("{path}.declared_lineage.parent_candidate_ids"),
                            format!("{id:?} does not name an earlier valid candidate"),
                        )
                    })
                })
                .collect();
            if expected_parents? != record.parent_candidate_digests {
                return Err(CandidateLineageError::at(
                    format!("{path}.parent_candidate_digests"),
                    "does not match the declared parent ids",
                ));
            }
            if declared.idea_source_digests != source_digests {
                return Err(CandidateLineageError::at(
                    format!("{path}.idea_provenance"),
                    "does not match the declared source digests",
                ));
            }
            // The declaration must come from the bytes that were hashed. Everything
            // above checks the declaration against itself and against earlier
            // records; none of it reads `raw_candidate`, so a record could hash one
            // candidate and display a lineage belonging to another. The two sibling
            // arms below already cross-check presence or absence of this field, and
            // this arm was the one that did not.
            let raw = record.raw_candidate.get("lineage").ok_or_else(|| {
                CandidateLineageError::at(
                    format!("{path}.lineage_status"),
                    "claims a declared lineage, but raw_candidate contains no lineage field",
                )
            })?;
            let raw_declared: DeclaredCandidateLineage = serde_json::from_value(raw.clone())
                .map_err(|error| {
                    CandidateLineageError::at(
                        format!("{path}.raw_candidate.lineage"),
                        format!("is not a declared lineage: {error}"),
                    )
                })?;
            if &raw_declared != declared {
                return Err(CandidateLineageError::at(
                    format!("{path}.declared_lineage"),
                    "does not match the lineage in the hashed raw candidate",
                ));
            }
            verify_unique_declaration(declared, &path)?;
        }
        (Some(declared), "invalid") => {
            verify_unique_declaration(declared, &path)?;
            if source_digests
                .iter()
                .any(|digest| !declared.idea_source_digests.contains(digest))
            {
                return Err(CandidateLineageError::at(
                    format!("{path}.idea_provenance"),
                    "contains a source absent from the invalid declaration",
                ));
            }
        }
        (None, "invalid") => {
            if !record.parent_candidate_digests.is_empty() || !record.idea_provenance.is_empty() {
                return Err(CandidateLineageError::at(
                    path.clone(),
                    "an unparsed lineage declaration cannot resolve parents or sources",
                ));
            }
            if record.raw_candidate.get("lineage").is_none() {
                return Err(CandidateLineageError::at(
                    format!("{path}.lineage_status"),
                    "claims invalid lineage, but raw_candidate contains no lineage field",
                ));
            }
        }
        (None, "host-derived-unreferenced") => {
            if !record.parent_candidate_digests.is_empty() || !record.idea_provenance.is_empty() {
                return Err(CandidateLineageError::at(
                    path.clone(),
                    "an undeclared lineage cannot resolve parents or sources",
                ));
            }
            if record.raw_candidate.get("lineage").is_some() {
                return Err(CandidateLineageError::at(
                    format!("{path}.lineage_status"),
                    "claims undeclared lineage, but raw_candidate contains a lineage field",
                ));
            }
        }
        _ => {
            return Err(CandidateLineageError::at(
                format!("{path}.lineage_status"),
                "is inconsistent with declared_lineage",
            ));
        }
    }

    let binding = serde_json::json!({
        "family_digest": record.family_digest,
        "generator_identity_sha256": record.generator_identity_sha256,
        "idea_source_digests": source_digests,
        "parent_candidate_digests": record.parent_candidate_digests,
        "raw_candidate_sha256": record.raw_candidate_sha256,
    });
    compare_digest(
        format!("{path}.lineage_binding_sha256"),
        &record.lineage_binding_sha256,
        &binding,
    )
}

fn verify_unique_declaration(
    declared: &DeclaredCandidateLineage,
    path: &str,
) -> Result<(), CandidateLineageError> {
    let parents: BTreeSet<_> = declared.parent_candidate_ids.iter().collect();
    if parents.len() != declared.parent_candidate_ids.len() {
        return Err(CandidateLineageError::at(
            format!("{path}.declared_lineage.parent_candidate_ids"),
            "must not contain duplicates",
        ));
    }
    let sources: BTreeSet<_> = declared.idea_source_digests.iter().collect();
    if sources.len() != declared.idea_source_digests.len() {
        return Err(CandidateLineageError::at(
            format!("{path}.declared_lineage.idea_source_digests"),
            "must not contain duplicates",
        ));
    }
    Ok(())
}

fn verify_source(source: &IdeaProvenance, path: &str) -> Result<(), CandidateLineageError> {
    const SOURCE_TYPES: [&str; 6] = [
        "dataset",
        "operator_brief",
        "paper",
        "prior_candidate",
        "repository",
        "other",
    ];
    if !SOURCE_TYPES.contains(&source.source_type.as_str()) {
        return Err(CandidateLineageError::at(
            format!("{path}.source_type"),
            "is outside the closed source vocabulary",
        ));
    }
    verify_prefixed_sha256(&source.source_digest, &format!("{path}.source_digest"))?;
    for (field, value) in [
        ("url_or_doi", source.url_or_doi.as_deref()),
        ("commit", source.commit.as_deref()),
        ("license", source.license.as_deref()),
    ] {
        if value.is_some_and(|text| text.trim().is_empty()) {
            return Err(CandidateLineageError::at(
                format!("{path}.{field}"),
                "must not be blank when present",
            ));
        }
    }
    if let Some(commit) = &source.commit {
        if ["head", "latest", "main", "master"].contains(&commit.to_lowercase().as_str()) {
            return Err(CandidateLineageError::at(
                format!("{path}.commit"),
                "must name an immutable revision",
            ));
        }
    }
    let authors: BTreeSet<_> = source.authors.iter().collect();
    if authors.len() != source.authors.len() {
        return Err(CandidateLineageError::at(
            format!("{path}.authors"),
            "must not contain duplicates",
        ));
    }
    if source.authors.iter().any(|author| author.trim().is_empty()) {
        return Err(CandidateLineageError::at(
            format!("{path}.authors"),
            "must not contain blank names",
        ));
    }
    if source
        .available_on
        .as_deref()
        .is_some_and(|day| !is_calendar_date(day))
    {
        return Err(CandidateLineageError::at(
            format!("{path}.available_on"),
            "must be a calendar date written as YYYY-MM-DD",
        ));
    }
    Ok(())
}

fn verify_family_summary(
    declared: &[CandidateFamilyCount],
    actual: &BTreeMap<String, CandidateFamilyAccumulator>,
) -> Result<(), CandidateLineageError> {
    if declared.len() != actual.len() {
        return Err(CandidateLineageError::at(
            "summary.families",
            "does not contain one row per observed family",
        ));
    }
    for (index, (row, (digest, counts))) in declared.iter().zip(actual).enumerate() {
        if &row.family_digest != digest
            || row.observed_trials != counts.observed_trials
            || row.selectable != counts.selectable_candidates
        {
            return Err(CandidateLineageError::at(
                format!("summary.families[{index}]"),
                "does not match the recomputed family counts and sorted order",
            ));
        }
    }
    Ok(())
}

fn compare_count(path: &str, declared: usize, actual: usize) -> Result<(), CandidateLineageError> {
    if declared == actual {
        Ok(())
    } else {
        Err(CandidateLineageError::at(
            path,
            format!("claims {declared}, recomputed {actual}"),
        ))
    }
}

fn compare_string(path: &str, declared: &str, expected: &str) -> Result<(), CandidateLineageError> {
    if declared == expected {
        Ok(())
    } else {
        Err(CandidateLineageError::at(
            path,
            format!("must equal {expected:?}"),
        ))
    }
}

fn compare_digest(
    path: String,
    declared: &str,
    value: &Value,
) -> Result<(), CandidateLineageError> {
    verify_plain_sha256(declared, &path)?;
    let actual = canonical_sha256(value)?;
    if declared == actual {
        Ok(())
    } else {
        Err(CandidateLineageError::at(
            path,
            format!("digest mismatch; recomputed {actual}"),
        ))
    }
}

fn verify_plain_sha256(value: &str, path: &str) -> Result<(), CandidateLineageError> {
    if value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        Ok(())
    } else {
        Err(CandidateLineageError::at(
            path,
            "must contain 64 lowercase hexadecimal digits",
        ))
    }
}

fn verify_prefixed_sha256(value: &str, path: &str) -> Result<(), CandidateLineageError> {
    match value.strip_prefix("sha256:") {
        Some(digest) => verify_plain_sha256(digest, path),
        None => Err(CandidateLineageError::at(path, "must start with sha256:")),
    }
}

fn canonical_sha256(value: &Value) -> Result<String, CandidateLineageError> {
    let mut canonical = String::new();
    write_python_canonical_json(value, &mut canonical)?;
    let digest = Sha256::digest(canonical.as_bytes());
    Ok(crate::lower_hex(&digest))
}

fn write_python_canonical_json(
    value: &Value,
    output: &mut String,
) -> Result<(), CandidateLineageError> {
    match value {
        Value::Null => output.push_str("null"),
        Value::Bool(flag) => output.push_str(if *flag { "true" } else { "false" }),
        Value::Number(number) => output.push_str(&python_number(number)),
        Value::String(text) => output.push_str(
            &serde_json::to_string(text)
                .map_err(|error| CandidateLineageError::at("canonical_json", error.to_string()))?,
        ),
        Value::Array(items) => {
            output.push('[');
            for (index, item) in items.iter().enumerate() {
                if index > 0 {
                    output.push(',');
                }
                write_python_canonical_json(item, output)?;
            }
            output.push(']');
        }
        Value::Object(fields) => {
            output.push('{');
            let mut keys: Vec<_> = fields.keys().collect();
            keys.sort_unstable();
            for (index, key) in keys.into_iter().enumerate() {
                if index > 0 {
                    output.push(',');
                }
                output.push_str(&serde_json::to_string(key).map_err(|error| {
                    CandidateLineageError::at("canonical_json", error.to_string())
                })?);
                output.push(':');
                write_python_canonical_json(&fields[key], output)?;
            }
            output.push('}');
        }
    }
    Ok(())
}

fn python_number(number: &serde_json::Number) -> String {
    let rendered = number.to_string();
    let Some(exponent_at) = rendered.find(['e', 'E']) else {
        return rendered;
    };
    let (mantissa, exponent) = rendered.split_at(exponent_at);
    let exponent = &exponent[1..];
    let (sign, digits) = match exponent.as_bytes().first() {
        Some(b'+') => ("+", &exponent[1..]),
        Some(b'-') => ("-", &exponent[1..]),
        _ => ("+", exponent),
    };
    let padded = if digits.len() < 2 {
        format!("0{digits}")
    } else {
        digits.to_owned()
    };
    format!("{mantissa}e{sign}{padded}")
}

fn candidate_id(candidate: &Value) -> Option<&str> {
    candidate.get("id").and_then(Value::as_str)
}

fn median(sorted: &[f64]) -> Option<f64> {
    if sorted.is_empty() {
        None
    } else if sorted.len() % 2 == 1 {
        Some(sorted[sorted.len() / 2])
    } else {
        let upper = sorted.len() / 2;
        Some((sorted[upper - 1] + sorted[upper]) / 2.0)
    }
}

/// Lowercase hexadecimal SHA-256 of `bytes`. A historical dataset's
/// `content_sha256` is this digest of its CSV text, and a journal record's
/// digest is this digest of its stored line.
pub fn content_sha256(bytes: &[u8]) -> String {
    crate::lower_hex(&Sha256::digest(bytes))
}

/// Whether `value` is exactly `YYYY-MM-DD` naming a Gregorian day in years 1
/// to 9999, the form SharpeArena accepts for `available_on`.
pub fn is_calendar_date(value: &str) -> bool {
    let bytes = value.as_bytes();
    if bytes.len() != 10 || bytes[4] != b'-' || bytes[7] != b'-' {
        return false;
    }
    let number = |digits: &[u8]| {
        digits.iter().try_fold(0_u32, |total, digit| {
            digit
                .is_ascii_digit()
                .then(|| total * 10 + u32::from(digit - b'0'))
        })
    };
    let (Some(year), Some(month), Some(day)) = (
        number(&bytes[..4]),
        number(&bytes[5..7]),
        number(&bytes[8..]),
    ) else {
        return false;
    };
    let leap = year % 4 == 0 && (year % 100 != 0 || year % 400 == 0);
    let month_days = match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if leap => 29,
        2 => 28,
        _ => return false,
    };
    year >= 1 && (1..=month_days).contains(&day)
}

/// The calendar day a bar label starts with: the whole label when it is a day,
/// or its first ten characters when a `T` or a space begins a time of day.
fn calendar_day(label: &str) -> Option<&str> {
    let day = label.get(..10)?;
    let rest = &label.as_bytes()[10..];
    (is_calendar_date(day) && rest.first().is_none_or(|next| matches!(next, b'T' | b' ')))
        .then_some(day)
}

/// Why a split's first calendar day could not be established. Each is
/// reported, never treated as a clean split.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SplitDateUnavailable {
    /// The record carries no usable dataset record for the split.
    SplitNotRecorded,
    /// A synthetic panel has bar labels but no calendar.
    SyntheticSplitHasNoCalendar,
    /// No supplied dataset has the split's content digest.
    DatasetNotSupplied,
    /// The recorded window does not fit inside the supplied dataset.
    WindowOutsideDataset,
    /// The split's first bar label does not begin with a `YYYY-MM-DD` day.
    DateNotIso8601,
}

/// A split's first bar day and the cited sources dated on or after it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum SplitDating {
    Measured {
        first_date: String,
        sources_on_or_after_first_date: usize,
    },
    Unavailable {
        reason: SplitDateUnavailable,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SplitSourceDating {
    pub split: String,
    #[serde(flatten)]
    pub dating: SplitDating,
}

/// Cited idea sources against the calendar of each evaluation split.
///
/// A source dated on or after a split's first bar could not have been known
/// before that split began. Undated sources are counted separately rather than
/// assumed early. The report is diagnostic and never changes a rank.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceDatingReport {
    pub cited_sources: usize,
    pub dated_sources: usize,
    pub undated_sources: usize,
    pub splits: Vec<SplitSourceDating>,
}

struct RecordedSplit<'a> {
    content_sha256: &'a str,
    synthetic: bool,
    window_start: Option<u64>,
    window_end: Option<u64>,
}

/// Read a SharpeArena dataset record (`selection.split` or `test.split`).
/// A missing or null window bound means the dataset edge.
fn recorded_split(split: &Value) -> Option<RecordedSplit<'_>> {
    let synthetic = match split.get("kind").and_then(Value::as_str)? {
        "historical" => false,
        "synthetic" => true,
        _ => return None,
    };
    let bound = |key: &str| match split.get(key) {
        None | Some(Value::Null) => Some(None),
        Some(value) => value.as_u64().map(Some),
    };
    Some(RecordedSplit {
        content_sha256: split.get("content_sha256").and_then(Value::as_str)?,
        synthetic,
        window_start: bound("window_start")?,
        window_end: bound("window_end")?,
    })
}

/// Resolve the calendar day of a recorded split's first bar.
///
/// `calendars` maps a dataset content digest to that dataset's bar labels in
/// order. Only a historical split whose dataset was supplied can be measured;
/// every other case returns the typed reason it could not be.
pub fn resolve_split_first_date(
    split: Option<&Value>,
    calendars: &BTreeMap<String, Vec<String>>,
) -> Result<String, SplitDateUnavailable> {
    let split = split
        .and_then(recorded_split)
        .ok_or(SplitDateUnavailable::SplitNotRecorded)?;
    if split.synthetic {
        return Err(SplitDateUnavailable::SyntheticSplitHasNoCalendar);
    }
    let labels = calendars
        .get(split.content_sha256)
        .ok_or(SplitDateUnavailable::DatasetNotSupplied)?;
    let bars = u64::try_from(labels.len()).unwrap_or(u64::MAX);
    let start = split.window_start.unwrap_or(0);
    let end = split.window_end.unwrap_or(bars);
    if start >= end || end > bars {
        return Err(SplitDateUnavailable::WindowOutsideDataset);
    }
    usize::try_from(start)
        .ok()
        .and_then(|index| labels.get(index))
        .and_then(|label| calendar_day(label))
        .map(str::to_owned)
        .ok_or(SplitDateUnavailable::DateNotIso8601)
}

/// Count `sources` dated on or after each split's first day.
///
/// `split_starts` pairs a split name with its resolved first day or the reason
/// it is unavailable, in report order.
pub fn date_cited_sources(
    sources: &[IdeaProvenance],
    split_starts: Vec<(String, Result<String, SplitDateUnavailable>)>,
) -> SourceDatingReport {
    let dated = sources
        .iter()
        .filter(|source| source.available_on.is_some())
        .count();
    let splits = split_starts
        .into_iter()
        .map(|(split, start)| SplitSourceDating {
            split,
            dating: match start {
                Ok(first_date) => SplitDating::Measured {
                    sources_on_or_after_first_date: sources
                        .iter()
                        .filter(|source| {
                            source
                                .available_on
                                .as_deref()
                                .is_some_and(|day| day >= first_date.as_str())
                        })
                        .count(),
                    first_date,
                },
                Err(reason) => SplitDating::Unavailable { reason },
            },
        })
        .collect();
    SourceDatingReport {
        cited_sources: sources.len(),
        dated_sources: dated,
        undated_sources: sources.len() - dated,
        splits,
    }
}

/// Refuse a producer's declared source dating that contradicts `measured`.
///
/// Counts are always comparable. A split is compared only where `measured`
/// reached a verdict of its own; a split this verifier could not locate or
/// was not given the dataset for is reported unavailable, and the producer's
/// claim about it is neither trusted nor refused.
pub fn check_declared_source_dating(
    declared: &SourceDatingReport,
    measured: &SourceDatingReport,
) -> Result<(), CandidateLineageError> {
    if (
        declared.cited_sources,
        declared.dated_sources,
        declared.undated_sources,
    ) != (
        measured.cited_sources,
        measured.dated_sources,
        measured.undated_sources,
    ) {
        return Err(CandidateLineageError::at(
            "source_dating",
            "declared source counts disagree with the cited sources",
        ));
    }
    if declared.splits.len() != measured.splits.len() {
        return Err(CandidateLineageError::at(
            "source_dating.splits",
            format!("must list {} splits", measured.splits.len()),
        ));
    }
    for (index, (claim, fact)) in declared.splits.iter().zip(&measured.splits).enumerate() {
        let undecided = matches!(
            fact.dating,
            SplitDating::Unavailable {
                reason: SplitDateUnavailable::SplitNotRecorded
                    | SplitDateUnavailable::DatasetNotSupplied
            }
        );
        if claim.split != fact.split || (!undecided && claim != fact) {
            return Err(CandidateLineageError::at(
                format!("source_dating.splits[{index}]"),
                format!("disagrees with the recomputed {} split", fact.split),
            ));
        }
    }
    Ok(())
}

/// Scope stamped by SharpeArena on each record's test-split census.
pub const TEST_SPLIT_CENSUS_SCOPE: &str = "earlier-records-in-this-journal-file-only";
const STRATEGY_EVIDENCE_CLASS: &str = "retrospective_generated_strategy";

/// A SharpeArena record's census of earlier reads of its test split, as the
/// producer wrote it (strategy evidence schema 3).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TestSplitCensusClaim {
    pub scope: String,
    pub test_split_identity: Value,
    pub test_split_sha256: String,
    /// Half-open bar interval the kernel resolved for this test split.
    pub test_window_bars: [u64; 2],
    /// Bar count of the panel the test split reads.
    pub test_dataset_bars: u64,
    pub test_consulted: bool,
    pub prior_test_consultations: usize,
    pub prior_consultation_record_sha256: Vec<String>,
    pub prior_observed_n_trials: u64,
    pub cumulative_observed_n_trials: u64,
    /// Earlier consultations of the same panel whose window intersects
    /// `test_window_bars`, exact matches included.
    pub overlapping_prior_test_consultations: usize,
    pub overlapping_prior_consultation_record_sha256: Vec<String>,
    pub overlapping_prior_observed_n_trials: u64,
    /// Bars of `test_window_bars` that at least one overlapping consultation read.
    pub prior_consulted_test_bars: u64,
    pub unidentified_prior_records: usize,
    /// Digest of the journal's last nonblank line before this record, or
    /// `None` for the first line. Removing, inserting or reordering an earlier
    /// line breaks it; lines cut from the journal's end leave no trace.
    pub previous_record_sha256: Option<String>,
}

/// Newest SharpeArena strategy evidence schema whose census rules this
/// verifier knows.
pub const NEWEST_STRATEGY_SCHEMA: u64 = 3;

/// Identity of the bars one test consultation read, from a recorded split and
/// its seed list, or `None` when either is missing or malformed.
///
/// Historical bars are named by content digest and window; execution seeds do
/// not change them. A synthetic panel is generated from its seeds, so the
/// sorted seeds join the key. Costs and labels are excluded. Windows are
/// compared as recorded, so overlapping or differently written windows over
/// the same bars are different identities. This mirrors SharpeArena's
/// `consulted_split_identity`.
pub fn consulted_split_identity(split: Option<&Value>, seeds: Option<&Value>) -> Option<Value> {
    let split = recorded_split(split?)?;
    let mut seeds = seeds?
        .as_array()?
        .iter()
        .map(Value::as_u64)
        .collect::<Option<Vec<_>>>()?;
    seeds.sort_unstable();
    Some(serde_json::json!({
        "content_sha256": split.content_sha256,
        "window_start": split.window_start,
        "window_end": split.window_end,
        "scenario_seeds": split.synthetic.then_some(seeds),
    }))
}

fn is_split_identity(value: &Value) -> bool {
    let Some(object) = value.as_object() else {
        return false;
    };
    let keys: BTreeSet<&str> = object.keys().map(String::as_str).collect();
    if keys
        != BTreeSet::from([
            "content_sha256",
            "scenario_seeds",
            "window_end",
            "window_start",
        ])
    {
        return false;
    }
    let bound = |key: &str| object[key].is_null() || object[key].is_u64();
    let seeds = match &object["scenario_seeds"] {
        Value::Null => true,
        Value::Array(items) => items
            .iter()
            .map(Value::as_u64)
            .collect::<Option<Vec<_>>>()
            .is_some_and(|seeds| seeds.windows(2).all(|pair| pair[0] <= pair[1])),
        _ => false,
    };
    object["content_sha256"].is_string() && bound("window_start") && bound("window_end") && seeds
}

struct Consultation {
    identity: Option<Value>,
    consulted: bool,
    trials: u64,
}

/// How the census reads one journal record, mirroring SharpeArena.
///
/// A completed strategy record of any schema read its test split and is keyed
/// by `test.split` and `test.seeds`. A failed record has no test block, so only
/// one carrying a well-formed census claim can be keyed. Anything else is
/// unidentified.
fn journal_consultation(record: &Value) -> Consultation {
    let unidentified = Consultation {
        identity: None,
        consulted: false,
        trials: 0,
    };
    if record.get("evidence_class").and_then(Value::as_str) != Some(STRATEGY_EVIDENCE_CLASS) {
        return unidentified;
    }
    let trials = observed_trials(record);
    match record.get("status").and_then(Value::as_str) {
        Some("completed") => match record.get("test") {
            Some(test) if test.is_object() => Consultation {
                identity: consulted_split_identity(test.get("split"), test.get("seeds")),
                consulted: true,
                trials,
            },
            _ => unidentified,
        },
        Some("failed") => {
            let census = record.get("test_split_census");
            let identity = census
                .and_then(|claim| claim.get("test_split_identity"))
                .filter(|identity| is_split_identity(identity));
            let consulted = census
                .and_then(|claim| claim.get("test_consulted"))
                .and_then(Value::as_bool);
            match (identity, consulted) {
                (Some(identity), Some(consulted)) => Consultation {
                    identity: Some(identity.clone()),
                    consulted,
                    trials,
                },
                _ => unidentified,
            }
        }
        _ => unidentified,
    }
}

fn observed_trials(record: &Value) -> u64 {
    record
        .pointer("/generation/observed_n_trials")
        .and_then(Value::as_u64)
        .unwrap_or(0)
}

/// Whether two split identities read bars of the same panel: the same content,
/// and for synthetic panels at least one common seed.
fn shares_bars(prior: &Value, current: &Value) -> bool {
    if prior["content_sha256"] != current["content_sha256"] {
        return false;
    }
    match (
        prior["scenario_seeds"].as_array(),
        current["scenario_seeds"].as_array(),
    ) {
        (Some(prior_seeds), Some(current_seeds)) => {
            prior_seeds.iter().any(|seed| current_seeds.contains(seed))
        }
        (None, None) => true,
        _ => false,
    }
}

/// The bars of `identity`'s window inside `[start, end)`, with omitted bounds
/// resolved against a panel of `bars` bars.
fn shared_interval(identity: &Value, [start, end]: [u64; 2], bars: u64) -> Option<(u64, u64)> {
    let low = start.max(identity["window_start"].as_u64().unwrap_or(0));
    let high = end.min(identity["window_end"].as_u64().unwrap_or(bars));
    (low < high).then_some((low, high))
}

/// Number of bars in the union of half-open intervals.
///
/// In start order, each interval adds only the bars past the furthest bar an
/// earlier one reached, so an interval inside an earlier one adds nothing.
fn covered_bars(mut intervals: Vec<(u64, u64)>) -> u64 {
    intervals.sort_unstable();
    let mut covered = 0;
    let mut reach = 0;
    for (start, end) in intervals {
        covered += end.saturating_sub(start.max(reach));
        reach = reach.max(end);
    }
    covered
}

/// Refuse a declared panel bar count that contradicts a supplied dataset.
///
/// `calendars` maps a content digest to its bar labels, as for
/// [`resolve_split_first_date`]. A synthetic split, or a historical split whose
/// dataset was not supplied, is left unchecked here.
pub fn check_census_dataset_bars(
    claim: &TestSplitCensusClaim,
    calendars: &BTreeMap<String, Vec<String>>,
) -> Result<(), CandidateLineageError> {
    let identity = &claim.test_split_identity;
    let labels = identity["content_sha256"]
        .as_str()
        .filter(|_| identity["scenario_seeds"].is_null())
        .and_then(|content| calendars.get(content));
    match labels {
        Some(labels) if u64::try_from(labels.len()).ok() != Some(claim.test_dataset_bars) => {
            Err(CandidateLineageError::at(
                "test_split_census.test_dataset_bars",
                format!(
                    "claims {}, but the supplied dataset has {} bars",
                    claim.test_dataset_bars,
                    labels.len()
                ),
            ))
        }
        _ => Ok(()),
    }
}

/// Check a record's declared test-split census against the record itself.
///
/// Schema 3 records must carry one. The identity must be well formed and
/// hashed correctly, the declared counts must add up, and a completed record's
/// claim must name the split it actually recorded. Whether the earlier counts
/// are true needs the journal: see [`verify_test_split_census`].
pub fn check_test_split_census_claim(
    record: &Value,
) -> Result<Option<TestSplitCensusClaim>, CandidateLineageError> {
    let Some(raw) = record.get("test_split_census") else {
        if record.get("evidence_class").and_then(Value::as_str) == Some(STRATEGY_EVIDENCE_CLASS)
            && record.get("schema_version").and_then(Value::as_u64) >= Some(3)
        {
            return Err(CandidateLineageError::at(
                "test_split_census",
                "is required from strategy evidence schema 3",
            ));
        }
        return Ok(None);
    };
    let claim: TestSplitCensusClaim = serde_json::from_value(raw.clone())
        .map_err(|error| CandidateLineageError::at("test_split_census", error.to_string()))?;
    compare_string(
        "test_split_census.scope",
        &claim.scope,
        TEST_SPLIT_CENSUS_SCOPE,
    )?;
    if !is_split_identity(&claim.test_split_identity) {
        return Err(CandidateLineageError::at(
            "test_split_census.test_split_identity",
            "is not a split identity",
        ));
    }
    compare_digest(
        "test_split_census.test_split_sha256".to_owned(),
        &claim.test_split_sha256,
        &claim.test_split_identity,
    )?;
    let [start, end] = claim.test_window_bars;
    let bars = claim.test_dataset_bars;
    let identity = &claim.test_split_identity;
    if !(start < end && end <= bars)
        || identity["window_start"].as_u64().unwrap_or(0) != start
        || identity["window_end"].as_u64().unwrap_or(bars) != end
    {
        return Err(CandidateLineageError::at(
            "test_split_census.test_window_bars",
            format!("[{start}, {end}) over {bars} bars does not resolve the identity's window"),
        ));
    }
    compare_count(
        "test_split_census.prior_test_consultations",
        claim.prior_test_consultations,
        claim.prior_consultation_record_sha256.len(),
    )?;
    compare_count(
        "test_split_census.overlapping_prior_test_consultations",
        claim.overlapping_prior_test_consultations,
        claim.overlapping_prior_consultation_record_sha256.len(),
    )?;
    for digest in claim
        .prior_consultation_record_sha256
        .iter()
        .chain(&claim.overlapping_prior_consultation_record_sha256)
    {
        verify_plain_sha256(digest, "test_split_census.prior_consultation_record_sha256")?;
    }
    if let Some(previous) = &claim.previous_record_sha256 {
        verify_plain_sha256(previous, "test_split_census.previous_record_sha256")?;
    }
    if claim.prior_consultation_record_sha256.iter().any(|digest| {
        !claim
            .overlapping_prior_consultation_record_sha256
            .contains(digest)
    }) || claim.overlapping_prior_observed_n_trials < claim.prior_observed_n_trials
    {
        return Err(CandidateLineageError::at(
            "test_split_census.overlapping_prior_consultation_record_sha256",
            "must include every exact prior consultation and its trials",
        ));
    }
    if claim.prior_consulted_test_bars > end - start
        || (claim.prior_consulted_test_bars == 0)
            != (claim.overlapping_prior_test_consultations == 0)
    {
        return Err(CandidateLineageError::at(
            "test_split_census.prior_consulted_test_bars",
            format!(
                "{} is impossible for {} overlapping consultations of a {}-bar window",
                claim.prior_consulted_test_bars,
                claim.overlapping_prior_test_consultations,
                end - start
            ),
        ));
    }
    let own = if claim.test_consulted {
        observed_trials(record)
    } else {
        0
    };
    if claim.prior_observed_n_trials.checked_add(own) != Some(claim.cumulative_observed_n_trials) {
        return Err(CandidateLineageError::at(
            "test_split_census.cumulative_observed_n_trials",
            format!(
                "must equal {} earlier plus {own} observed trials",
                claim.prior_observed_n_trials
            ),
        ));
    }
    if record.get("status").and_then(Value::as_str) == Some("completed") {
        let recorded = record
            .get("test")
            .and_then(|test| consulted_split_identity(test.get("split"), test.get("seeds")));
        if !claim.test_consulted || recorded.as_ref() != Some(&claim.test_split_identity) {
            return Err(CandidateLineageError::at(
                "test_split_census",
                "a completed record must claim a consultation of the test split it recorded",
            ));
        }
        // A synthetic panel's bar count is its recorded length.
        if !identity["scenario_seeds"].is_null()
            && record.pointer("/test/split/n_days").and_then(Value::as_u64) != Some(bars)
        {
            return Err(CandidateLineageError::at(
                "test_split_census.test_dataset_bars",
                "does not equal the synthetic test split's n_days",
            ));
        }
    }
    Ok(Some(claim))
}

/// One record of a strategy-evidence journal, with the digest of its stored
/// bytes and its 1-based line number.
#[derive(Debug, Clone, PartialEq)]
pub struct JournalRecord {
    pub line: usize,
    pub record_sha256: String,
    pub record: Value,
}

/// Reads of one test split across a journal.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TestSplitConsultations {
    pub test_split_sha256: String,
    pub test_split_identity: Value,
    pub test_consultations: usize,
    pub cumulative_observed_n_trials: u64,
    pub consultation_record_sha256: Vec<String>,
    /// Failed records that named this split but stopped before reading it.
    pub unconsulted_records: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CensusJournalEntry {
    pub line: usize,
    pub record_sha256: String,
    pub schema_version: Option<u64>,
    pub status: Option<String>,
    pub test_split_sha256: Option<String>,
    pub test_consulted: bool,
    pub observed_n_trials: u64,
    pub declared_census_verified: bool,
    /// From a verified census claim: earlier consultations whose window
    /// overlaps this record's test window, and the bars of it they read.
    pub overlapping_prior_test_consultations: Option<usize>,
    pub prior_consulted_test_bars: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lineage: Option<CandidateLineageReport>,
}

/// Test-split consultations counted over one strategy-evidence journal.
///
/// Diagnostic only: it never changes eligibility, ranking, or any record's own
/// trial count. It sees only the records in the journal it was given.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TestSplitCensusReport {
    pub schema_version: u64,
    pub evidence_class: String,
    pub scope: String,
    pub records: usize,
    pub completed_records: usize,
    pub failed_records: usize,
    pub unidentified_records: usize,
    pub declared_censuses_verified: usize,
    pub splits: Vec<TestSplitConsultations>,
    pub journal: Vec<CensusJournalEntry>,
}

/// Count how often each test split was read across a journal, and check every
/// record's declared census against the records before it.
///
/// A record whose declared census disagrees with the earlier records is a
/// refusal. Lineage is not verified here; the caller attaches it per record.
pub fn verify_test_split_census(
    journal: &[JournalRecord],
) -> Result<TestSplitCensusReport, CandidateLineageError> {
    let mut splits: BTreeMap<String, TestSplitConsultations> = BTreeMap::new();
    let mut reads: Vec<(Value, &str, u64)> = Vec::new();
    let mut entries = Vec::with_capacity(journal.len());
    let mut unidentified = 0;
    let mut completed = 0;
    let mut failed = 0;
    let mut verified_claims = 0;
    let mut previous: Option<&str> = None;
    for item in journal {
        let located = |error: CandidateLineageError| {
            CandidateLineageError::at(format!("line {}.{}", item.line, error.path), error.message)
        };
        let strategy = item.record.get("evidence_class").and_then(Value::as_str)
            == Some(STRATEGY_EVIDENCE_CLASS);
        if strategy
            && item.record.get("schema_version").and_then(Value::as_u64)
                > Some(NEWEST_STRATEGY_SCHEMA)
        {
            return Err(located(CandidateLineageError::at(
                "schema_version",
                format!(
                    "is newer than strategy evidence schema {NEWEST_STRATEGY_SCHEMA}, whose \
                     census rules this verifier knows"
                ),
            )));
        }
        let claim = check_test_split_census_claim(&item.record).map_err(located)?;
        if let Some(claim) = &claim {
            if claim.previous_record_sha256.as_deref() != previous {
                return Err(located(CandidateLineageError::at(
                    "test_split_census.previous_record_sha256",
                    "does not name the line before this record; a line was removed, inserted \
                     or reordered",
                )));
            }
            let (earlier, earlier_trials) =
                splits
                    .get(&claim.test_split_sha256)
                    .map_or((&[][..], 0), |group| {
                        (
                            group.consultation_record_sha256.as_slice(),
                            group.cumulative_observed_n_trials,
                        )
                    });
            if earlier != claim.prior_consultation_record_sha256.as_slice()
                || earlier_trials != claim.prior_observed_n_trials
                || unidentified != claim.unidentified_prior_records
            {
                return Err(located(CandidateLineageError::at(
                    "test_split_census",
                    format!(
                        "declares {} earlier consultations with {} trials and {} unidentified \
                         records, but the journal before it holds {} with {} trials and {}",
                        claim.prior_test_consultations,
                        claim.prior_observed_n_trials,
                        claim.unidentified_prior_records,
                        earlier.len(),
                        earlier_trials,
                        unidentified
                    ),
                )));
            }
            let mut overlapping = Vec::new();
            let mut overlapping_trials = 0_u64;
            let mut intervals = Vec::new();
            for (identity, digest, trials) in &reads {
                if !shares_bars(identity, &claim.test_split_identity) {
                    continue;
                }
                if let Some(interval) =
                    shared_interval(identity, claim.test_window_bars, claim.test_dataset_bars)
                {
                    overlapping.push(*digest);
                    overlapping_trials = overlapping_trials.saturating_add(*trials);
                    intervals.push(interval);
                }
            }
            let bars = covered_bars(intervals);
            if overlapping != claim.overlapping_prior_consultation_record_sha256
                || overlapping_trials != claim.overlapping_prior_observed_n_trials
                || bars != claim.prior_consulted_test_bars
            {
                return Err(located(CandidateLineageError::at(
                    "test_split_census",
                    format!(
                        "declares {} overlapping consultations with {} trials covering {} bars, \
                         but the journal before it holds {} with {} trials covering {}",
                        claim.overlapping_prior_test_consultations,
                        claim.overlapping_prior_observed_n_trials,
                        claim.prior_consulted_test_bars,
                        overlapping.len(),
                        overlapping_trials,
                        bars
                    ),
                )));
            }
            verified_claims += 1;
        }

        let consultation = journal_consultation(&item.record);
        let status = item
            .record
            .get("status")
            .and_then(Value::as_str)
            .map(str::to_owned);
        if strategy {
            completed += usize::from(status.as_deref() == Some("completed"));
            failed += usize::from(status.as_deref() == Some("failed"));
        }
        let split_sha256 = match &consultation.identity {
            None => {
                unidentified += 1;
                None
            }
            Some(identity) => {
                let digest = canonical_sha256(identity)?;
                let group =
                    splits
                        .entry(digest.clone())
                        .or_insert_with(|| TestSplitConsultations {
                            test_split_sha256: digest.clone(),
                            test_split_identity: identity.clone(),
                            test_consultations: 0,
                            cumulative_observed_n_trials: 0,
                            consultation_record_sha256: Vec::new(),
                            unconsulted_records: 0,
                        });
                if consultation.consulted {
                    reads.push((
                        identity.clone(),
                        item.record_sha256.as_str(),
                        consultation.trials,
                    ));
                    group.test_consultations += 1;
                    group.cumulative_observed_n_trials = group
                        .cumulative_observed_n_trials
                        .saturating_add(consultation.trials);
                    group
                        .consultation_record_sha256
                        .push(item.record_sha256.clone());
                } else {
                    group.unconsulted_records += 1;
                }
                Some(digest)
            }
        };
        entries.push(CensusJournalEntry {
            line: item.line,
            record_sha256: item.record_sha256.clone(),
            schema_version: item.record.get("schema_version").and_then(Value::as_u64),
            status,
            test_split_sha256: split_sha256,
            test_consulted: consultation.identity.is_some() && consultation.consulted,
            observed_n_trials: consultation.trials,
            declared_census_verified: claim.is_some(),
            overlapping_prior_test_consultations: claim
                .as_ref()
                .map(|claim| claim.overlapping_prior_test_consultations),
            prior_consulted_test_bars: claim.as_ref().map(|claim| claim.prior_consulted_test_bars),
            lineage: None,
        });
        previous = Some(&item.record_sha256);
    }
    Ok(TestSplitCensusReport {
        schema_version: 1,
        evidence_class: "test_split_consultation_census".to_owned(),
        scope: "one journal file; searches written to other journal files, or never \
                recorded, are not counted"
            .to_owned(),
        records: journal.len(),
        completed_records: completed,
        failed_records: failed,
        unidentified_records: unidentified,
        declared_censuses_verified: verified_claims,
        splits: splits.into_values().collect(),
        journal: entries,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn candidate(id: &str, threshold: f64) -> Value {
        serde_json::json!({
            "id": id,
            "threshold": threshold,
            "label": "café",
            "long_when": {
                "op": "gt",
                "left": {"indicator": "momentum", "window": 3},
                "right": {"constant": threshold}
            },
            "short_when": null,
            "edge_manifest": {
                "regimes": ["trending"],
                "instruments": ["synthetic_panel"]
            }
        })
    }

    fn digest(value: &Value) -> String {
        canonical_sha256(value).unwrap()
    }

    fn record(
        ordinal: usize,
        id: &str,
        threshold: f64,
        family_preimage: Value,
        declared_lineage: Option<DeclaredCandidateLineage>,
        parents: Vec<String>,
        sources: Vec<IdeaProvenance>,
    ) -> CandidateLineageRecord {
        // The raw candidate carries the lineage the record declares, because that
        // is what a real generator emits and what the hash is taken over. The
        // fixture previously hashed a candidate with no lineage field at all while
        // the record displayed one, and the verification passed: it demonstrated
        // the very gap it was supposed to guard.
        let mut raw_candidate = candidate(id, threshold);
        if let Some(declared) = &declared_lineage {
            raw_candidate["lineage"] = serde_json::to_value(declared).expect("lineage is JSON");
        }
        let raw_candidate_sha256 = digest(&raw_candidate);
        let family_digest = digest(&family_preimage);
        let generator_identity = serde_json::json!({
            "digest": "sha256:model",
            "model": "fixture"
        });
        let generator_identity_sha256 = digest(&generator_identity);
        let lineage_binding = serde_json::json!({
            "family_digest": family_digest,
            "generator_identity_sha256": generator_identity_sha256,
            "idea_source_digests": sources.iter().map(|source| source.source_digest.clone()).collect::<Vec<_>>(),
            "parent_candidate_digests": parents,
            "raw_candidate_sha256": raw_candidate_sha256,
        });
        let binding = serde_json::json!({
            "raw_candidate_sha256": raw_candidate_sha256,
            "manifest_sha256": null,
            "model_digest": "sha256:model",
            "split_plan_sha256": "split",
            "trial_ordinal": ordinal,
        });
        CandidateLineageRecord {
            schema_version: 2,
            evidence_class: EVIDENCE_CLASS.to_owned(),
            trial_ordinal: ordinal,
            raw_candidate,
            raw_candidate_sha256,
            manifest: None,
            manifest_sha256: None,
            invalid_reason: None,
            duplicate_of_ordinal: None,
            model_digest: "sha256:model".to_owned(),
            split_plan_sha256: "split".to_owned(),
            family_preimage,
            family_digest,
            declared_lineage,
            parent_candidate_digests: parents,
            generator_identity,
            generator_identity_sha256,
            idea_provenance: sources,
            lineage_status: "declared".to_owned(),
            lineage_binding_sha256: digest(&lineage_binding),
            binding_sha256: digest(&binding),
        }
    }

    fn ledger() -> CandidateLineageLedger {
        let source = IdeaProvenance {
            source_type: "paper".to_owned(),
            source_digest: format!("sha256:{}", "a".repeat(64)),
            url_or_doi: Some("doi:10.0000/example".to_owned()),
            commit: None,
            authors: vec!["A. Researcher".to_owned()],
            license: Some("CC-BY-4.0".to_owned()),
            available_on: None,
        };
        let family = serde_json::json!({
            "long_when": {
                "op": "gt",
                "left": {"indicator": "momentum", "window": "parameter"},
                "right": {"constant": "parameter"}
            },
            "short_when": null,
            "regimes": ["trending"],
            "instruments": ["synthetic_panel"]
        });
        let first = record(
            0,
            "base",
            1e-7,
            family.clone(),
            Some(DeclaredCandidateLineage {
                parent_candidate_ids: vec![],
                idea_source_digests: vec![source.source_digest.clone()],
            }),
            vec![],
            vec![source.clone()],
        );
        let second = record(
            1,
            "retuned",
            0.2,
            family,
            Some(DeclaredCandidateLineage {
                parent_candidate_ids: vec!["base".to_owned()],
                idea_source_digests: vec![source.source_digest.clone()],
            }),
            vec![first.raw_candidate_sha256.clone()],
            vec![source],
        );
        let family_digest = first.family_digest.clone();
        let generator_identity_sha256 = first.generator_identity_sha256.clone();
        CandidateLineageLedger {
            summary: CandidateLineageSummary {
                schema_version: 2,
                evidence_class: EVIDENCE_CLASS.to_owned(),
                model_digest: "sha256:model".to_owned(),
                split_plan_sha256: "split".to_owned(),
                observed_trials: 2,
                invalid: 0,
                duplicates: 0,
                selectable: 2,
                families: vec![CandidateFamilyCount {
                    family_digest,
                    observed_trials: 2,
                    selectable: 2,
                }],
                family_count: 1,
                generator_identity_sha256,
                plan_bound_idea_sources: 1,
                family_grouping_role: FAMILY_ROLE.to_owned(),
                n_trials_source: TRIAL_SOURCE.to_owned(),
            },
            records: vec![first, second],
        }
    }

    #[test]
    fn verifies_ancestry_and_reports_family_best_versus_median() {
        let report = verify_candidate_lineage(
            &ledger(),
            &[
                CandidateLineageScore {
                    candidate_id: "base".to_owned(),
                    median_deflated_sharpe: 0.2,
                },
                CandidateLineageScore {
                    candidate_id: "retuned".to_owned(),
                    median_deflated_sharpe: 0.8,
                },
            ],
        )
        .unwrap();

        assert!(report.verified);
        assert_eq!(report.observed_trials, 2);
        assert_eq!(report.trial_denominator, 2);
        assert!(!report.family_grouping_affects_trial_count);
        assert_eq!(report.ancestry_edges, 1);
        assert_eq!(report.cited_source_count, 1);
        assert_eq!(report.families[0].best_median_deflated_sharpe, Some(0.8));
        assert_eq!(report.families[0].family_median_deflated_sharpe, Some(0.5));
        assert!((report.families[0].best_to_median_gap.unwrap() - 0.3).abs() < f64::EPSILON);
    }

    #[test]
    fn canonical_json_matches_python_for_unicode_and_small_exponents() {
        let value = candidate("base", 1e-7);
        assert_eq!(
            canonical_sha256(&value).unwrap(),
            "1053610ec9fb321fa75f14724bec545af1d2f566e36d86148fb1e26de38edb69"
        );
    }

    #[test]
    fn a_declaration_absent_from_the_hashed_candidate_is_refused() {
        // Everything else about a declared record checks the declaration against
        // itself and against earlier records. Without this, a generator could hash
        // one candidate and display the lineage of another, and the raw-candidate
        // digest would still verify.
        let source = IdeaProvenance {
            source_type: "paper".to_owned(),
            source_digest: format!("sha256:{}", "a".repeat(64)),
            url_or_doi: None,
            commit: None,
            authors: vec!["A. Researcher".to_owned()],
            license: None,
            available_on: None,
        };
        let mut rec = record(
            0,
            "c0",
            0.5,
            serde_json::json!({"family": "f"}),
            Some(DeclaredCandidateLineage {
                parent_candidate_ids: vec![],
                idea_source_digests: vec![source.source_digest.clone()],
            }),
            vec![],
            vec![source],
        );
        rec.raw_candidate
            .as_object_mut()
            .expect("candidate is an object")
            .remove("lineage");
        let error = verify_record_lineage(&rec, 0, &Default::default(), &Default::default())
            .expect_err("a declaration the hashed bytes do not carry must be refused");
        assert!(
            format!("{error}").contains("lineage"),
            "the refusal must name the lineage: {error}"
        );
    }

    #[test]
    fn refuses_a_self_consistent_but_fabricated_family() {
        let mut ledger = ledger();
        ledger.records[1].family_preimage = serde_json::json!({"signal": "different"});
        ledger.records[1].family_digest = digest(&ledger.records[1].family_preimage);
        let forged_binding = {
            let record = &ledger.records[1];
            digest(&serde_json::json!({
                "family_digest": record.family_digest,
                "generator_identity_sha256": record.generator_identity_sha256,
                "idea_source_digests": record.idea_provenance.iter().map(|source| source.source_digest.clone()).collect::<Vec<_>>(),
                "parent_candidate_digests": record.parent_candidate_digests,
                "raw_candidate_sha256": record.raw_candidate_sha256,
            }))
        };
        ledger.records[1].lineage_binding_sha256 = forged_binding;
        ledger.summary.family_count = 2;
        ledger.summary.families = ledger
            .records
            .iter()
            .map(|record| CandidateFamilyCount {
                family_digest: record.family_digest.clone(),
                observed_trials: 1,
                selectable: 1,
            })
            .collect();
        ledger
            .summary
            .families
            .sort_by(|left, right| left.family_digest.cmp(&right.family_digest));
        let error = verify_candidate_lineage(
            &ledger,
            &[
                CandidateLineageScore {
                    candidate_id: "base".to_owned(),
                    median_deflated_sharpe: 0.2,
                },
                CandidateLineageScore {
                    candidate_id: "retuned".to_owned(),
                    median_deflated_sharpe: 0.8,
                },
            ],
        )
        .unwrap_err();
        assert_eq!(error.path, "records[1].family_preimage");
    }

    #[test]
    fn refuses_partial_score_coverage() {
        let error = verify_candidate_lineage(
            &ledger(),
            &[CandidateLineageScore {
                candidate_id: "base".to_owned(),
                median_deflated_sharpe: 0.2,
            }],
        )
        .unwrap_err();
        assert_eq!(error.path, "scores");
        assert!(error.message.contains("retuned"));
    }

    use serde_json::json;

    fn dated(digest: char, available_on: Option<&str>) -> IdeaProvenance {
        IdeaProvenance {
            source_type: "paper".to_owned(),
            source_digest: format!("sha256:{}", digest.to_string().repeat(64)),
            url_or_doi: None,
            commit: None,
            authors: vec![],
            license: None,
            available_on: available_on.map(str::to_owned),
        }
    }

    #[test]
    fn content_digest_is_plain_sha256() {
        assert_eq!(
            content_sha256(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn calendar_dates_follow_the_gregorian_calendar_exactly() {
        for valid in [
            "0001-01-01",
            "2024-02-29",
            "2000-02-29",
            "2025-02-28",
            "2025-01-31",
            "2025-04-30",
            "2025-11-30",
            "2025-12-31",
            "9999-12-31",
        ] {
            assert!(is_calendar_date(valid), "{valid}");
        }
        for invalid in [
            "0000-01-01",
            "2023-02-29",
            "1900-02-29",
            "2025-01-32",
            "2025-04-31",
            "2025-06-31",
            "2025-09-31",
            "2025-11-31",
            "2025-02-30",
            "2025-13-01",
            "2025-00-10",
            "2025-01-00",
            "2025-1-01",
            "2025-01-011",
            "2025/01-01",
            "2025-01/01",
            "20a5-01-01",
            "2025-0b-01",
            "2025-01-3c",
            "",
        ] {
            assert!(!is_calendar_date(invalid), "{invalid}");
        }
    }

    #[test]
    fn a_bar_label_names_a_day_only_when_it_starts_with_one() {
        assert_eq!(calendar_day("2025-03-04"), Some("2025-03-04"));
        assert_eq!(calendar_day("2025-03-04T09:30:00Z"), Some("2025-03-04"));
        assert_eq!(calendar_day("2025-03-04 09:30"), Some("2025-03-04"));
        for label in [
            "2025-03-04Z",
            "2025-03-0",
            "2025-02-30",
            "2025-001",
            "t0",
            "ééééé-01",
        ] {
            assert_eq!(calendar_day(label), None, "{label}");
        }
    }

    #[test]
    fn a_malformed_source_date_is_refused() {
        assert!(verify_source(&dated('a', Some("2025-01-11")), "s").is_ok());
        let error = verify_source(&dated('a', Some("2025-02-30")), "s").unwrap_err();
        assert_eq!(error.path, "s.available_on");
        let undated: IdeaProvenance = serde_json::from_value(json!({
            "source_type": "paper",
            "source_digest": format!("sha256:{}", "a".repeat(64)),
            "url_or_doi": null,
            "commit": null,
            "authors": [],
            "license": null
        }))
        .unwrap();
        assert_eq!(undated.available_on, None);
        assert!(!serde_json::to_string(&undated)
            .unwrap()
            .contains("available_on"));
    }

    fn historical(start: Value, end: Value) -> Value {
        json!({
            "kind": "historical",
            "content_sha256": "c".repeat(64),
            "window_start": start,
            "window_end": end,
            "dataset_id": "test",
            "fee_bps": 3.0
        })
    }

    fn calendars(labels: &[&str]) -> BTreeMap<String, Vec<String>> {
        BTreeMap::from([(
            "c".repeat(64),
            labels.iter().map(|label| (*label).to_owned()).collect(),
        )])
    }

    #[test]
    fn a_split_first_date_is_measured_only_from_a_supplied_historical_dataset() {
        use SplitDateUnavailable::*;
        let days = calendars(&["2025-01-01", "2025-01-02", "2025-01-03", "2025-01-04"]);
        let resolve = |split: &Value, known: &BTreeMap<String, Vec<String>>| {
            resolve_split_first_date(Some(split), known)
        };
        assert_eq!(
            resolve(&historical(json!(2), json!(4)), &days),
            Ok("2025-01-03".to_owned())
        );
        assert_eq!(
            resolve(&historical(Value::Null, Value::Null), &days),
            Ok("2025-01-01".to_owned())
        );
        assert_eq!(
            resolve(&historical(json!(3), Value::Null), &days),
            Ok("2025-01-04".to_owned())
        );
        let mut missing = historical(json!(1), json!(2));
        missing.as_object_mut().unwrap().remove("window_start");
        assert_eq!(resolve(&missing, &days), Ok("2025-01-01".to_owned()));
        assert_eq!(resolve_split_first_date(None, &days), Err(SplitNotRecorded));
        for unrecorded in [
            json!({"kind": "live", "content_sha256": "c".repeat(64)}),
            json!({"content_sha256": "c".repeat(64)}),
            json!({"kind": "historical"}),
            historical(json!("2"), json!(4)),
            historical(json!(0), json!(-1)),
        ] {
            assert_eq!(
                resolve(&unrecorded, &days),
                Err(SplitNotRecorded),
                "{unrecorded}"
            );
        }
        let mut synthetic = historical(json!(0), json!(2));
        synthetic["kind"] = json!("synthetic");
        assert_eq!(resolve(&synthetic, &days), Err(SyntheticSplitHasNoCalendar));
        assert_eq!(
            resolve(&historical(json!(0), json!(2)), &BTreeMap::new()),
            Err(DatasetNotSupplied)
        );
        for outside in [
            historical(json!(2), json!(2)),
            historical(json!(3), json!(2)),
            historical(json!(0), json!(5)),
            historical(json!(4), Value::Null),
        ] {
            assert_eq!(
                resolve(&outside, &days),
                Err(WindowOutsideDataset),
                "{outside}"
            );
        }
        let labels = calendars(&["t0", "2025-01-02"]);
        assert_eq!(
            resolve(&historical(json!(0), json!(2)), &labels),
            Err(DateNotIso8601)
        );
        assert_eq!(
            resolve(&historical(json!(1), json!(2)), &labels),
            Ok("2025-01-02".to_owned())
        );
    }

    #[test]
    fn sources_on_or_after_a_split_start_are_counted_and_undated_ones_separately() {
        let sources = [
            dated('a', Some("2025-01-10")),
            dated('b', Some("2025-01-11")),
            dated('c', Some("2025-01-12")),
            dated('d', None),
        ];
        let report = date_cited_sources(
            &sources,
            vec![
                ("selection".to_owned(), Ok("2025-01-01".to_owned())),
                ("test".to_owned(), Ok("2025-01-11".to_owned())),
                (
                    "other".to_owned(),
                    Err(SplitDateUnavailable::SyntheticSplitHasNoCalendar),
                ),
            ],
        );
        assert_eq!(
            (
                report.cited_sources,
                report.dated_sources,
                report.undated_sources
            ),
            (4, 3, 1)
        );
        let names: Vec<_> = report
            .splits
            .iter()
            .map(|split| split.split.as_str())
            .collect();
        assert_eq!(names, ["selection", "test", "other"]);
        assert_eq!(
            report.splits[0].dating,
            SplitDating::Measured {
                first_date: "2025-01-01".to_owned(),
                sources_on_or_after_first_date: 3
            }
        );
        assert_eq!(
            report.splits[1].dating,
            SplitDating::Measured {
                first_date: "2025-01-11".to_owned(),
                sources_on_or_after_first_date: 2
            }
        );
        assert_eq!(
            serde_json::to_value(&report.splits[2]).unwrap(),
            json!({"split": "other", "status": "unavailable", "reason": "synthetic_split_has_no_calendar"})
        );
        let empty = date_cited_sources(&[], vec![]);
        assert_eq!(
            (
                empty.cited_sources,
                empty.dated_sources,
                empty.undated_sources
            ),
            (0, 0, 0)
        );
    }

    #[test]
    fn a_declared_source_dating_is_checked_only_where_it_can_be_recomputed() {
        use SplitDateUnavailable::*;
        let sources = [dated('a', Some("2025-01-11")), dated('b', None)];
        let report = |test: Result<String, SplitDateUnavailable>| {
            date_cited_sources(
                &sources,
                vec![
                    ("selection".to_owned(), Err(SyntheticSplitHasNoCalendar)),
                    ("test".to_owned(), test),
                ],
            )
        };
        let measured = report(Ok("2025-01-11".to_owned()));
        assert!(check_declared_source_dating(&measured, &measured).is_ok());
        let later = report(Ok("2025-01-12".to_owned()));
        let error = check_declared_source_dating(&later, &measured).unwrap_err();
        assert_eq!(error.path, "source_dating.splits[1]");
        for undecided in [DatasetNotSupplied, SplitNotRecorded] {
            assert!(check_declared_source_dating(&later, &report(Err(undecided))).is_ok());
        }
        for decided in [
            SyntheticSplitHasNoCalendar,
            WindowOutsideDataset,
            DateNotIso8601,
        ] {
            assert!(check_declared_source_dating(&later, &report(Err(decided))).is_err());
        }
        for field in 0..3 {
            let mut wrong = measured.clone();
            match field {
                0 => wrong.cited_sources += 1,
                1 => wrong.dated_sources += 1,
                _ => wrong.undated_sources += 1,
            }
            assert_eq!(
                check_declared_source_dating(&wrong, &measured)
                    .unwrap_err()
                    .path,
                "source_dating"
            );
        }
        let mut short = measured.clone();
        short.splits.pop();
        assert_eq!(
            check_declared_source_dating(&short, &measured)
                .unwrap_err()
                .path,
            "source_dating.splits"
        );
        let mut renamed = measured.clone();
        renamed.splits[1].split = "confirmation".to_owned();
        assert!(check_declared_source_dating(&renamed, &report(Err(DatasetNotSupplied))).is_err());
    }

    #[test]
    fn a_split_identity_keys_bars_and_synthetic_seeds_only() {
        let seeds = json!([9, 3]);
        let identity =
            consulted_split_identity(Some(&historical(json!(0), json!(4))), Some(&seeds)).unwrap();
        assert_eq!(
            identity,
            json!({"content_sha256": "c".repeat(64), "window_start": 0, "window_end": 4, "scenario_seeds": null})
        );
        assert!(is_split_identity(&identity));
        let mut synthetic = historical(Value::Null, Value::Null);
        synthetic["kind"] = json!("synthetic");
        let generated = consulted_split_identity(Some(&synthetic), Some(&seeds)).unwrap();
        assert_eq!(generated["scenario_seeds"], json!([3, 9]));
        assert!(is_split_identity(&generated));
        let split = historical(json!(0), json!(4));
        assert_eq!(consulted_split_identity(None, Some(&seeds)), None);
        assert_eq!(consulted_split_identity(Some(&split), None), None);
        assert_eq!(
            consulted_split_identity(Some(&split), Some(&json!("3"))),
            None
        );
        assert_eq!(
            consulted_split_identity(Some(&split), Some(&json!([3, -1]))),
            None
        );

        assert!(!is_split_identity(&json!("identity")));
        let mut extra = identity.clone();
        extra["fee_bps"] = json!(1.0);
        assert!(!is_split_identity(&extra));
        for (key, value) in [
            ("content_sha256", json!(7)),
            ("window_start", json!("0")),
            ("window_end", json!(-4)),
            ("scenario_seeds", json!([9, 3])),
            ("scenario_seeds", json!("3")),
            ("scenario_seeds", json!([3, "9"])),
        ] {
            let mut broken = identity.clone();
            broken[key] = value;
            assert!(!is_split_identity(&broken), "{broken}");
        }
        let mut repeated = identity.clone();
        repeated["scenario_seeds"] = json!([3, 3, 9]);
        assert!(is_split_identity(&repeated));
        let mut missing = identity;
        missing.as_object_mut().unwrap().remove("window_end");
        assert!(!is_split_identity(&missing));
    }

    /// Bar count of the historical panel the census tests read.
    const PANEL_BARS: u64 = 24;

    fn search_at(split: Value, trials: u64) -> Value {
        json!({
            "schema_version": 2,
            "evidence_class": STRATEGY_EVIDENCE_CLASS,
            "status": "completed",
            "generation": {"observed_n_trials": trials},
            "test": {"split": split, "seeds": [3]}
        })
    }

    fn search(window_start: u64, trials: u64) -> Value {
        search_at(historical(json!(window_start), json!(20)), trials)
    }

    fn identity_at(split: &Value) -> Value {
        consulted_split_identity(Some(split), Some(&json!([3]))).unwrap()
    }

    fn identity_of(window_start: u64) -> Value {
        identity_at(&historical(json!(window_start), json!(20)))
    }

    fn digests(records: &[&JournalRecord]) -> Vec<String> {
        records
            .iter()
            .map(|item| item.record_sha256.clone())
            .collect()
    }

    fn trials_of(records: &[&JournalRecord]) -> u64 {
        records
            .iter()
            .map(|item| observed_trials(&item.record))
            .sum()
    }

    /// Stamp a schema 3 census. `exact` and `overlap` name the earlier
    /// consultations the claim declares, and `prior_bars` the bars of the
    /// window they read, all worked out by the caller.
    fn with_census(
        mut record: Value,
        identity: Value,
        consulted: bool,
        exact: &[&JournalRecord],
        overlap: &[&JournalRecord],
        prior_bars: u64,
        unidentified: usize,
    ) -> Value {
        let own = if consulted {
            observed_trials(&record)
        } else {
            0
        };
        let start = identity["window_start"].as_u64().unwrap_or(0);
        let end = identity["window_end"].as_u64().unwrap_or(PANEL_BARS);
        record["schema_version"] = json!(3);
        record["test_split_census"] = json!({
            "scope": TEST_SPLIT_CENSUS_SCOPE,
            "test_split_sha256": canonical_sha256(&identity).unwrap(),
            "test_split_identity": identity,
            "test_window_bars": [start, end],
            "test_dataset_bars": PANEL_BARS,
            "test_consulted": consulted,
            "prior_test_consultations": exact.len(),
            "prior_consultation_record_sha256": digests(exact),
            "prior_observed_n_trials": trials_of(exact),
            "cumulative_observed_n_trials": trials_of(exact) + own,
            "overlapping_prior_test_consultations": overlap.len(),
            "overlapping_prior_consultation_record_sha256": digests(overlap),
            "overlapping_prior_observed_n_trials": trials_of(overlap),
            "prior_consulted_test_bars": prior_bars,
            "unidentified_prior_records": unidentified,
            "previous_record_sha256": null,
        });
        record
    }

    /// A journal record that follows `previous`: its census, if any, chains to it.
    fn linked(previous: &JournalRecord, line: usize, mut record: Value) -> JournalRecord {
        if let Some(claim) = record.get_mut("test_split_census") {
            claim["previous_record_sha256"] = json!(previous.record_sha256);
        }
        entry(line, record)
    }

    #[test]
    fn panels_share_bars_through_content_and_a_common_seed() {
        let history = |content: &str| json!({"content_sha256": content, "scenario_seeds": null});
        let panel = |seeds: Value| json!({"content_sha256": "s", "scenario_seeds": seeds});
        assert!(shares_bars(&history("h"), &history("h")));
        assert!(!shares_bars(&history("h"), &history("g")));
        assert!(shares_bars(&panel(json!([1, 2])), &panel(json!([2, 3]))));
        assert!(!shares_bars(&panel(json!([1, 2])), &panel(json!([3]))));
        assert!(!shares_bars(&history("s"), &panel(json!([1]))));
        assert!(!shares_bars(&panel(json!([1])), &history("s")));
        assert!(!shares_bars(
            &panel(json!([1])),
            &json!({"content_sha256": "t", "scenario_seeds": [1]})
        ));
    }

    #[test]
    fn a_shared_interval_resolves_omitted_bounds_against_the_panel() {
        let window = |start: Value, end: Value| json!({"window_start": start, "window_end": end});
        assert_eq!(
            shared_interval(&window(json!(10), json!(20)), [12, 22], 24),
            Some((12, 20))
        );
        assert_eq!(
            shared_interval(&window(Value::Null, Value::Null), [5, 10], 24),
            Some((5, 10))
        );
        assert_eq!(
            shared_interval(&window(json!(10), Value::Null), [0, 30], 24),
            Some((10, 24))
        );
        assert_eq!(
            shared_interval(&window(json!(10), Value::Null), [0, 20], 24),
            Some((10, 20))
        );
        assert_eq!(
            shared_interval(&window(json!(10), json!(20)), [20, 24], 24),
            None
        );
        assert_eq!(
            shared_interval(&window(json!(10), json!(20)), [0, 10], 24),
            None
        );
        assert_eq!(
            shared_interval(&window(json!(10), json!(20)), [19, 24], 24),
            Some((19, 20))
        );
        assert_eq!(
            shared_interval(&window(json!(10), json!(20)), [0, 11], 24),
            Some((10, 11))
        );
    }

    #[test]
    fn covered_bars_counts_the_union_of_windows() {
        assert_eq!(covered_bars(vec![]), 0);
        assert_eq!(covered_bars(vec![(3, 5)]), 2);
        assert_eq!(covered_bars(vec![(10, 20), (12, 22), (8, 10)]), 14);
        assert_eq!(covered_bars(vec![(0, 10), (2, 4), (4, 6)]), 10);
        assert_eq!(covered_bars(vec![(5, 7), (0, 2)]), 4);
        assert_eq!(covered_bars(vec![(0, 4), (4, 8)]), 8);
        assert_eq!(covered_bars(vec![(2, 6), (0, 3)]), 6);
        // An empty interval covers nothing and hides nothing after it.
        assert_eq!(covered_bars(vec![(2, 2)]), 0);
        assert_eq!(covered_bars(vec![(2, 2), (1, 3)]), 2);
    }

    #[test]
    fn a_declared_panel_bar_count_must_match_a_supplied_dataset() {
        let honest = with_census(search(10, 6), identity_of(10), true, &[], &[], 0, 0);
        let claim = check_test_split_census_claim(&honest).unwrap().unwrap();
        let labels =
            |bars: usize| BTreeMap::from([("c".repeat(64), vec!["2025-01-01".to_owned(); bars])]);
        assert!(check_census_dataset_bars(&claim, &labels(24)).is_ok());
        assert!(check_census_dataset_bars(&claim, &BTreeMap::new()).is_ok());
        let error = check_census_dataset_bars(&claim, &labels(23)).unwrap_err();
        assert_eq!(error.path, "test_split_census.test_dataset_bars");
        assert!(check_census_dataset_bars(&claim, &labels(25)).is_err());
        let mut synthetic = claim.clone();
        synthetic.test_split_identity["scenario_seeds"] = json!([3]);
        assert!(check_census_dataset_bars(&synthetic, &labels(23)).is_ok());
    }

    fn entry(line: usize, record: Value) -> JournalRecord {
        JournalRecord {
            line,
            record_sha256: content_sha256(serde_json::to_string(&record).unwrap().as_bytes()),
            record,
        }
    }

    fn failed(trials: Option<u64>) -> Value {
        json!({
            "schema_version": 3,
            "evidence_class": STRATEGY_EVIDENCE_CLASS,
            "status": "failed",
            "generation": {"observed_n_trials": trials}
        })
    }

    #[test]
    fn a_census_counts_every_read_of_a_test_split_across_one_journal() {
        // Windows over one 24-bar panel "c...": A = [10, 20) for lines 1, 2, 5
        // and 6, B = [12, 20) for line 7, D = [15, 24) for line 10. Line 9 reads
        // [10, 24) of a different panel "d...". Bars read before each claim:
        // lines 2, 5, 6 have A before them, so all 10 of A's bars; line 7 has
        // A three times, covering 12..19, 8 bars; line 9 shares no panel, 0;
        // line 10 has A and B, covering 15..19, 5 bars.
        let first = entry(1, search(10, 4));
        let unconsulted = linked(
            &first,
            2,
            with_census(
                failed(None),
                identity_of(10),
                false,
                &[&first],
                &[&first],
                10,
                0,
            ),
        );
        let foreign = entry(
            3,
            json!({"evidence_class": "forecast_evidence", "status": "completed"}),
        );
        let legacy_failure = entry(
            4,
            json!({"schema_version": 2, "evidence_class": STRATEGY_EVIDENCE_CLASS, "status": "failed"}),
        );
        let second = linked(
            &legacy_failure,
            5,
            with_census(
                search(10, 6),
                identity_of(10),
                true,
                &[&first],
                &[&first],
                10,
                2,
            ),
        );
        let consulted_failure = linked(
            &second,
            6,
            with_census(
                failed(Some(5)),
                identity_of(10),
                true,
                &[&first, &second],
                &[&first, &second],
                10,
                2,
            ),
        );
        let elsewhere = linked(
            &consulted_failure,
            7,
            with_census(
                search(12, 9),
                identity_of(12),
                true,
                &[],
                &[&first, &second, &consulted_failure],
                8,
                2,
            ),
        );
        let running = entry(
            8,
            json!({"schema_version": 2, "evidence_class": STRATEGY_EVIDENCE_CLASS, "status": "running"}),
        );
        let other_panel = json!({
            "kind": "historical",
            "content_sha256": "d".repeat(64),
            "window_start": 10,
            "window_end": null
        });
        let other_content = linked(
            &running,
            9,
            with_census(
                search_at(other_panel.clone(), 2),
                identity_at(&other_panel),
                true,
                &[],
                &[],
                0,
                3,
            ),
        );
        let late_panel = historical(json!(15), Value::Null);
        let shifted = linked(
            &other_content,
            10,
            with_census(
                search_at(late_panel.clone(), 3),
                identity_at(&late_panel),
                true,
                &[],
                &[&first, &second, &consulted_failure, &elsewhere],
                5,
                3,
            ),
        );
        let journal = [
            first.clone(),
            unconsulted,
            foreign,
            legacy_failure,
            second.clone(),
            consulted_failure.clone(),
            elsewhere,
            running,
            other_content,
            shifted,
        ];
        let report = verify_test_split_census(&journal).unwrap();
        assert_eq!(report.records, 10);
        // Neither completed nor failed: counted in neither.
        assert_eq!(report.completed_records, 5);
        assert_eq!(report.failed_records, 3);
        assert_eq!(report.unidentified_records, 3);
        assert_eq!(report.declared_censuses_verified, 6);
        let shared = canonical_sha256(&identity_of(10)).unwrap();
        let group = report
            .splits
            .iter()
            .find(|split| split.test_split_sha256 == shared)
            .unwrap();
        assert_eq!(group.test_split_identity, identity_of(10));
        assert_eq!(group.test_consultations, 3);
        assert_eq!(group.cumulative_observed_n_trials, 15);
        assert_eq!(group.unconsulted_records, 1);
        assert_eq!(
            group.consultation_record_sha256,
            [&first, &second, &consulted_failure].map(|item| item.record_sha256.clone())
        );
        assert_eq!(report.splits.len(), 4);
        let moved = canonical_sha256(&identity_of(12)).unwrap();
        let other = report
            .splits
            .iter()
            .find(|split| split.test_split_sha256 == moved)
            .unwrap();
        assert_eq!(
            (other.test_consultations, other.cumulative_observed_n_trials),
            (1, 9)
        );
        let rows: Vec<_> = report
            .journal
            .iter()
            .map(|row| {
                (
                    row.line,
                    row.test_split_sha256.is_some(),
                    row.test_consulted,
                    row.observed_n_trials,
                    row.declared_census_verified,
                    row.overlapping_prior_test_consultations,
                    row.prior_consulted_test_bars,
                )
            })
            .collect();
        assert_eq!(
            rows,
            [
                (1, true, true, 4, false, None, None),
                (2, true, false, 0, true, Some(1), Some(10)),
                (3, false, false, 0, false, None, None),
                (4, false, false, 0, false, None, None),
                (5, true, true, 6, true, Some(1), Some(10)),
                (6, true, true, 5, true, Some(2), Some(10)),
                (7, true, true, 9, true, Some(3), Some(8)),
                (8, false, false, 0, false, None, None),
                (9, true, true, 2, true, Some(0), Some(0)),
                (10, true, true, 3, true, Some(4), Some(5)),
            ]
        );
        assert_eq!(report.journal[0].schema_version, Some(2));
        assert_eq!(report.journal[0].status.as_deref(), Some("completed"));
        assert_eq!(report.journal[2].status.as_deref(), Some("completed"));
        assert_eq!(report.journal[0].record_sha256, first.record_sha256);
    }

    #[test]
    fn a_census_refuses_a_declared_history_the_journal_does_not_hold() {
        let first = entry(1, search(10, 4));
        let honest = with_census(
            search(10, 6),
            identity_of(10),
            true,
            &[&first],
            &[&first],
            10,
            0,
        );
        let path_after_first = |claim: Value| {
            verify_test_split_census(&[first.clone(), linked(&first, 2, claim)])
                .map(|_| ())
                .map_err(|error| error.path)
        };
        assert_eq!(path_after_first(honest.clone()), Ok(()));

        // Written against a journal that held the first search, then moved.
        let error = verify_test_split_census(&[entry(9, honest.clone())]).unwrap_err();
        assert_eq!(error.path, "line 9.test_split_census");
        assert!(error.message.contains("declares 1 earlier"), "{error}");

        let mut inflated = honest.clone();
        inflated["test_split_census"]["prior_observed_n_trials"] = json!(5);
        inflated["test_split_census"]["cumulative_observed_n_trials"] = json!(11);
        inflated["test_split_census"]["overlapping_prior_observed_n_trials"] = json!(5);
        assert_eq!(
            path_after_first(inflated),
            Err("line 2.test_split_census".to_owned())
        );

        // Only the earlier trial sum is wrong, and the overlap counts agree, so
        // nothing but the exact-history check can refuse it.
        let mut understated = honest.clone();
        understated["test_split_census"]["prior_observed_n_trials"] = json!(0);
        understated["test_split_census"]["cumulative_observed_n_trials"] = json!(6);
        assert_eq!(
            path_after_first(understated),
            Err("line 2.test_split_census".to_owned())
        );

        let mut hidden = honest.clone();
        hidden["test_split_census"]["unidentified_prior_records"] = json!(1);
        assert_eq!(
            path_after_first(hidden),
            Err("line 2.test_split_census".to_owned())
        );

        let foreign = entry(1, json!({"evidence_class": "other"}));
        let blind = with_census(search(10, 6), identity_of(10), true, &[], &[], 0, 0);
        let error =
            verify_test_split_census(&[foreign.clone(), linked(&foreign, 2, blind)]).unwrap_err();
        assert_eq!(error.path, "line 2.test_split_census");
        assert!(error.message.contains("0 unidentified"), "{error}");

        let broken = with_census(search(10, 6), json!({}), true, &[], &[], 0, 0);
        let error = verify_test_split_census(&[entry(4, broken)]).unwrap_err();
        assert_eq!(error.path, "line 4.test_split_census.test_split_identity");

        // An earlier window [0, 12) overlaps [10, 20) in bars 10 and 11, so the
        // honest claim declares two overlapping reads covering all 10 bars.
        let early_split = historical(json!(0), json!(12));
        let early = entry(2, search_at(early_split, 3));
        let journal = |claim: Value| {
            verify_test_split_census(&[first.clone(), early.clone(), linked(&early, 3, claim)])
                .map(|_| ())
                .map_err(|error| (error.path, error.message))
        };
        let overlapped = with_census(
            search(10, 6),
            identity_of(10),
            true,
            &[&first],
            &[&first, &early],
            10,
            0,
        );
        assert_eq!(journal(overlapped.clone()), Ok(()));
        let overlap_refusal = |claim: Value| {
            let (path, message) = journal(claim).unwrap_err();
            assert_eq!(path, "line 3.test_split_census");
            assert!(message.contains("overlapping"), "{message}");
        };
        overlap_refusal(honest.clone());
        let mut short = overlapped.clone();
        short["test_split_census"]["prior_consulted_test_bars"] = json!(9);
        overlap_refusal(short);
        let mut heavy = overlapped.clone();
        heavy["test_split_census"]["overlapping_prior_observed_n_trials"] = json!(8);
        overlap_refusal(heavy);
        let mut reordered = overlapped.clone();
        reordered["test_split_census"]["overlapping_prior_consultation_record_sha256"] =
            json!(digests(&[&early, &first]));
        overlap_refusal(reordered);
        // Only the earlier window's two shared bars were read before [10, 20)
        // when the first search is absent.
        let partial = with_census(search(10, 6), identity_of(10), true, &[], &[&early], 2, 0);
        assert!(verify_test_split_census(&[early.clone(), linked(&early, 3, partial)]).is_ok());
    }

    #[test]
    fn the_census_chain_exposes_a_removed_inserted_or_reordered_line() {
        let chain_path = "test_split_census.previous_record_sha256";
        let first = entry(1, search(10, 4));
        let other = entry(2, search(12, 5));
        let second = linked(
            &other,
            3,
            with_census(
                search(10, 6),
                identity_of(10),
                true,
                &[&first],
                &[&first, &other],
                10,
                0,
            ),
        );
        let refusal =
            |journal: &[JournalRecord]| verify_test_split_census(journal).unwrap_err().path;
        assert!(verify_test_split_census(&[first.clone(), other.clone(), second.clone()]).is_ok());
        // Removing the line on another split leaves every count intact, so
        // only the chain can tell.
        assert_eq!(
            refusal(&[first.clone(), second.clone()]),
            format!("line 3.{chain_path}")
        );
        assert_eq!(
            refusal(&[other.clone(), first.clone(), second.clone()]),
            format!("line 3.{chain_path}")
        );
        let inserted = entry(4, json!({"evidence_class": "other"}));
        assert_eq!(
            refusal(&[first.clone(), other.clone(), inserted, second.clone()]),
            format!("line 3.{chain_path}")
        );
        // The first line claims no predecessor.
        let orphan = linked(
            &first,
            1,
            with_census(search(10, 6), identity_of(10), true, &[], &[], 0, 0),
        );
        assert_eq!(refusal(&[orphan]), format!("line 1.{chain_path}"));
        let mut malformed = with_census(search(10, 6), identity_of(10), true, &[], &[], 0, 0);
        malformed["test_split_census"]["previous_record_sha256"] = json!("line 0");
        assert_eq!(
            refusal(&[entry(1, malformed)]),
            format!("line 1.{chain_path}")
        );
        // Lines cut from the end leave no record to notice.
        assert!(verify_test_split_census(&[first, other]).is_ok());
    }

    #[test]
    fn a_census_refuses_a_strategy_schema_it_does_not_know() {
        let mut future = search(10, 4);
        future["schema_version"] = json!(NEWEST_STRATEGY_SCHEMA + 1);
        let error = verify_test_split_census(&[entry(5, future)]).unwrap_err();
        assert_eq!(error.path, "line 5.schema_version");
        assert!(verify_test_split_census(&[entry(1, search(10, 4))]).is_ok());
        // Schema 3 strategy records must carry a census; other evidence never does.
        let mut newest = search(10, 4);
        newest["schema_version"] = json!(NEWEST_STRATEGY_SCHEMA);
        let error = verify_test_split_census(&[entry(2, newest)]).unwrap_err();
        assert_eq!(error.path, "line 2.test_split_census");
        let foreign = json!({"evidence_class": "other", "schema_version": 9});
        let report = verify_test_split_census(&[entry(1, foreign)]).unwrap();
        assert_eq!(report.unidentified_records, 1);
    }

    #[test]
    fn a_failed_record_without_a_usable_claim_is_unidentified() {
        let mut claimless = failed(Some(3));
        claimless["schema_version"] = json!(2);
        let mut odd_status = search(10, 4);
        odd_status["status"] = json!("running");
        let mut no_test = search(10, 4);
        no_test["test"] = json!("hidden");
        let mut malformed_claim = failed(Some(3));
        malformed_claim["test_split_census"] =
            json!({"test_split_identity": {}, "test_consulted": true});
        let mut unflagged_claim = failed(Some(3));
        unflagged_claim["test_split_census"] =
            json!({"test_split_identity": identity_of(10), "test_consulted": "yes"});
        for record in [
            claimless,
            odd_status,
            no_test,
            malformed_claim,
            unflagged_claim,
        ] {
            let consultation = journal_consultation(&record);
            assert!(consultation.identity.is_none(), "{record}");
            assert!(!consultation.consulted);
            assert_eq!(consultation.trials, 0);
        }
        let mut claimed = failed(Some(3));
        claimed["test_split_census"] =
            json!({"test_split_identity": identity_of(10), "test_consulted": true});
        let consultation = journal_consultation(&claimed);
        assert_eq!(consultation.identity, Some(identity_of(10)));
        assert!(consultation.consulted);
        assert_eq!(consultation.trials, 3);
        assert_eq!(
            observed_trials(&json!({"generation": {"observed_n_trials": null}})),
            0
        );
    }

    #[test]
    fn a_declared_census_must_be_internally_consistent() {
        let first = entry(1, search(10, 4));
        let honest = with_census(
            search(10, 6),
            identity_of(10),
            true,
            &[&first],
            &[&first],
            10,
            0,
        );
        let claim = check_test_split_census_claim(&honest).unwrap().unwrap();
        assert_eq!(claim.cumulative_observed_n_trials, 10);
        assert_eq!(claim.test_window_bars, [10, 20]);
        assert_eq!(claim.test_dataset_bars, 24);
        assert_eq!(check_test_split_census_claim(&search(10, 4)).unwrap(), None);

        let mut missing = honest.clone();
        missing.as_object_mut().unwrap().remove("test_split_census");
        assert_eq!(
            check_test_split_census_claim(&missing).unwrap_err().path,
            "test_split_census"
        );

        let path_of = |mutate: &dyn Fn(&mut Value)| {
            let mut record = honest.clone();
            mutate(&mut record);
            check_test_split_census_claim(&record).unwrap_err().path
        };
        assert_eq!(
            path_of(&|r| r["test_split_census"] = json!("claim")),
            "test_split_census"
        );
        assert_eq!(
            path_of(&|r| r["test_split_census"]["scope"] = json!("everywhere")),
            "test_split_census.scope"
        );
        assert_eq!(
            path_of(&|r| r["test_split_census"]["test_split_sha256"] = json!("0".repeat(64))),
            "test_split_census.test_split_sha256"
        );
        assert_eq!(
            path_of(&|r| r["test_split_census"]["prior_test_consultations"] = json!(2)),
            "test_split_census.prior_test_consultations"
        );
        assert_eq!(
            path_of(
                &|r| r["test_split_census"]["prior_consultation_record_sha256"] = json!(["ABC"])
            ),
            "test_split_census.prior_consultation_record_sha256"
        );
        assert_eq!(
            path_of(&|r| r["test_split_census"]["cumulative_observed_n_trials"] = json!(9)),
            "test_split_census.cumulative_observed_n_trials"
        );
        assert_eq!(
            path_of(&|r| {
                r["test_split_census"]["prior_observed_n_trials"] = json!(u64::MAX);
                r["test_split_census"]["overlapping_prior_observed_n_trials"] = json!(u64::MAX);
                r["test_split_census"]["cumulative_observed_n_trials"] = json!(u64::MAX);
            }),
            "test_split_census.cumulative_observed_n_trials"
        );
        // A completed search read its test split, and the one it recorded.
        assert_eq!(
            path_of(&|r| {
                r["test_split_census"]["test_consulted"] = json!(false);
                r["test_split_census"]["cumulative_observed_n_trials"] = json!(4);
            }),
            "test_split_census"
        );
        assert_eq!(
            path_of(&|r| r["test"]["seeds"] = json!("3")),
            "test_split_census"
        );
        let moved = with_census(
            search(10, 6),
            identity_of(12),
            true,
            &[&first],
            &[&first],
            8,
            0,
        );
        assert_eq!(
            check_test_split_census_claim(&moved).unwrap_err().path,
            "test_split_census"
        );

        // A failed search that never reached the test split adds no trials.
        let unconsulted = with_census(
            failed(Some(7)),
            identity_of(10),
            false,
            &[&first],
            &[&first],
            10,
            0,
        );
        let claim = check_test_split_census_claim(&unconsulted)
            .unwrap()
            .unwrap();
        assert_eq!(claim.cumulative_observed_n_trials, 4);

        // The resolved window must be the identity's window on the panel.
        let bars_path = "test_split_census.test_window_bars";
        assert_eq!(
            path_of(&|r| r["test_split_census"]["test_window_bars"] = json!([11, 20])),
            bars_path
        );
        assert_eq!(
            path_of(&|r| r["test_split_census"]["test_window_bars"] = json!([10, 19])),
            bars_path
        );
        let resolved = |start: Value, end: Value, bars: Value, window: Value| {
            let split = historical(start, end);
            let mut record =
                with_census(failed(Some(1)), identity_at(&split), false, &[], &[], 0, 0);
            record["test_split_census"]["test_dataset_bars"] = bars;
            record["test_split_census"]["test_window_bars"] = window;
            check_test_split_census_claim(&record)
                .map(|_| ())
                .map_err(|error| error.path)
        };
        assert_eq!(
            resolved(Value::Null, Value::Null, json!(24), json!([0, 24])),
            Ok(())
        );
        assert_eq!(
            resolved(json!(3), Value::Null, json!(30), json!([3, 30])),
            Ok(())
        );
        assert_eq!(
            resolved(json!(3), Value::Null, json!(30), json!([3, 24])).unwrap_err(),
            bars_path
        );
        assert_eq!(
            resolved(json!(20), json!(20), json!(24), json!([20, 20])).unwrap_err(),
            bars_path
        );
        assert_eq!(
            resolved(json!(21), json!(20), json!(24), json!([21, 20])).unwrap_err(),
            bars_path
        );
        assert_eq!(
            resolved(json!(10), json!(30), json!(24), json!([10, 30])).unwrap_err(),
            bars_path
        );
        assert_eq!(
            resolved(json!(10), json!(24), json!(24), json!([10, 24])),
            Ok(())
        );

        let overlap_path = "test_split_census.overlapping_prior_consultation_record_sha256";
        assert_eq!(
            path_of(&|r| r["test_split_census"]["overlapping_prior_test_consultations"] = json!(2)),
            "test_split_census.overlapping_prior_test_consultations"
        );
        assert_eq!(
            path_of(
                &|r| r["test_split_census"]["overlapping_prior_consultation_record_sha256"] =
                    json!(["ABC"])
            ),
            "test_split_census.prior_consultation_record_sha256"
        );
        assert_eq!(
            path_of(&|r| {
                r["test_split_census"]["overlapping_prior_consultation_record_sha256"] =
                    json!(["0".repeat(64)]);
            }),
            overlap_path
        );
        assert_eq!(
            path_of(&|r| r["test_split_census"]["overlapping_prior_observed_n_trials"] = json!(3)),
            overlap_path
        );
        let mut heavier = honest.clone();
        heavier["test_split_census"]["overlapping_prior_observed_n_trials"] = json!(5);
        assert!(check_test_split_census_claim(&heavier).is_ok());

        let prior_bars = "test_split_census.prior_consulted_test_bars";
        assert_eq!(
            path_of(&|r| r["test_split_census"]["prior_consulted_test_bars"] = json!(11)),
            prior_bars
        );
        assert_eq!(
            path_of(&|r| r["test_split_census"]["prior_consulted_test_bars"] = json!(0)),
            prior_bars
        );
        let fresh = with_census(search(10, 6), identity_of(10), true, &[], &[], 0, 0);
        assert!(check_test_split_census_claim(&fresh).is_ok());
        let mut phantom = fresh;
        phantom["test_split_census"]["prior_consulted_test_bars"] = json!(5);
        assert_eq!(
            check_test_split_census_claim(&phantom).unwrap_err().path,
            prior_bars
        );

        // A synthetic panel's bar count is its recorded length.
        let mut panel = historical(json!(0), json!(20));
        panel["kind"] = json!("synthetic");
        panel["n_days"] = json!(24);
        let synthetic = |n_days: Value| {
            let mut split = panel.clone();
            split["n_days"] = n_days;
            let mut record = search_at(split, 2);
            record = with_census(record, identity_at(&panel), true, &[], &[], 0, 0);
            check_test_split_census_claim(&record)
                .map(|_| ())
                .map_err(|error| error.path)
        };
        assert_eq!(synthetic(json!(24)), Ok(()));
        assert_eq!(
            synthetic(json!(30)).unwrap_err(),
            "test_split_census.test_dataset_bars"
        );
        assert_eq!(
            synthetic(Value::Null).unwrap_err(),
            "test_split_census.test_dataset_bars"
        );
    }
}
