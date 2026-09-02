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
    Ok(format!("{digest:x}"))
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
        let raw_candidate = candidate(id, threshold);
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
}
