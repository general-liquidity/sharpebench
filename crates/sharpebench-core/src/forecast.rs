//! Strict ingestion and independent analysis of prospective forecast evidence.
//!
//! SharpeArena owns the commit-time ledger. SharpeBench accepts only the closed
//! `sharpe.forecast-evidence.v1` file contract, reconstructs every score locally,
//! and compares agents only on the exact contracts resolved for the whole field.
//! This module does not import [`crate::composite`] and its report is not an input
//! to trading-rank eligibility.

use std::collections::{BTreeMap, BTreeSet};
use std::error::Error;
use std::fmt::{Display, Formatter};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::stats::norm_cdf;

pub const FORECAST_EVIDENCE_SCHEMA: &str = "sharpe.forecast-evidence.v1";
pub const FORECAST_QUALITY_SCHEMA: &str = "sharpebench.forecast-quality.v1";
const CONTRACT_SCHEMA: &str = "sharpearena.forecast-contract.v1";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ForecastError(pub String);

impl Display for ForecastError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl Error for ForecastError {}

fn reject(message: impl Into<String>) -> ForecastError {
    ForecastError(message.into())
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ForecastProducer {
    pub name: String,
    pub contract: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ForecastRunIdentity {
    pub agent_id: String,
    pub model_id: String,
    pub model_sha256: String,
    pub scaffold_id: String,
    pub scaffold_sha256: String,
    pub prompt_sha256: String,
    pub operator_id: String,
    pub config_sha256: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ForecastContract {
    pub schema_version: String,
    pub contract_id: String,
    pub question: String,
    pub instrument: String,
    pub target: String,
    pub kind: String,
    pub opens_at: u64,
    pub deadline: u64,
    pub resolves_at: u64,
    pub observation_source: String,
    pub open_definition: String,
    pub close_definition: String,
    pub unit: String,
    pub scoring_rule: String,
    pub neutral_threshold: f64,
    pub boundary_ownership: String,
    pub missing_data_policy: String,
    pub fallback_policy: String,
    pub categories: Vec<String>,
    pub interval_alpha: Option<f64>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct InformationExposure {
    pub observed_at: u64,
    pub market_snapshot_sha256: Option<String>,
    pub consensus_visible: bool,
    pub consensus_snapshot_sha256: Option<String>,
    pub source_ids: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ForecastRevision {
    pub revision_id: String,
    pub claim_id: String,
    pub ordinal: u64,
    pub supersedes: Option<String>,
    pub contract_sha256: String,
    pub prediction: Vec<f64>,
    pub confidence: f64,
    pub rationale: String,
    pub submitted_at: u64,
    pub status: String,
    pub reason: Option<String>,
    pub trigger_event_id: Option<String>,
    pub revision_reason: Option<String>,
    pub exposure: InformationExposure,
    pub idempotency_key: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ForecastResolution {
    pub claim_id: String,
    pub status: String,
    pub outcome: Option<Value>,
    pub available_at: Option<u64>,
    pub recorded_at: u64,
    pub reason: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ForecastEvidence {
    pub schema_version: String,
    pub producer: ForecastProducer,
    pub generated_at: u64,
    pub identity: ForecastRunIdentity,
    pub contracts: Vec<ForecastContract>,
    pub revisions: Vec<ForecastRevision>,
    pub resolutions: Vec<ForecastResolution>,
}

/// Parse the closed evidence envelope and validate all semantic links.
pub fn parse_forecast_evidence(payload: &str) -> Result<ForecastEvidence, ForecastError> {
    let evidence: ForecastEvidence = serde_json::from_str(payload)
        .map_err(|error| reject(format!("invalid forecast evidence JSON: {error}")))?;
    validate_evidence(&evidence)?;
    Ok(evidence)
}

fn nonempty(value: &str, field: &str) -> Result<(), ForecastError> {
    if value.trim().is_empty() {
        Err(reject(format!("{field} must be non-empty")))
    } else {
        Ok(())
    }
}

fn digest(value: &str, field: &str) -> Result<(), ForecastError> {
    if value.len() != 64
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(reject(format!(
            "{field} must be a lowercase SHA-256 digest"
        )));
    }
    Ok(())
}

fn finite(value: f64, field: &str) -> Result<(), ForecastError> {
    if value.is_finite() {
        Ok(())
    } else {
        Err(reject(format!("{field} must be finite")))
    }
}

fn validate_contract(contract: &ForecastContract) -> Result<String, ForecastError> {
    if contract.schema_version != CONTRACT_SCHEMA {
        return Err(reject(format!(
            "contract {} has unsupported schema_version",
            contract.contract_id
        )));
    }
    for (field, value) in [
        ("contract_id", contract.contract_id.as_str()),
        ("question", contract.question.as_str()),
        ("instrument", contract.instrument.as_str()),
        ("target", contract.target.as_str()),
        ("observation_source", contract.observation_source.as_str()),
        ("open_definition", contract.open_definition.as_str()),
        ("close_definition", contract.close_definition.as_str()),
        ("unit", contract.unit.as_str()),
        ("boundary_ownership", contract.boundary_ownership.as_str()),
        ("missing_data_policy", contract.missing_data_policy.as_str()),
        ("fallback_policy", contract.fallback_policy.as_str()),
    ] {
        nonempty(value, field)?;
    }
    if contract.opens_at > contract.deadline || contract.deadline >= contract.resolves_at {
        return Err(reject(format!(
            "contract {} must satisfy opens_at <= deadline < resolves_at",
            contract.contract_id
        )));
    }
    finite(contract.neutral_threshold, "neutral_threshold")?;
    if contract.neutral_threshold < 0.0 {
        return Err(reject("neutral_threshold must be non-negative"));
    }
    let allowed = match contract.kind.as_str() {
        "point" => &["point_errors"][..],
        "probability" => &["binary_brier", "binary_log"][..],
        "categorical" => &["categorical_brier", "categorical_log"][..],
        "normal" => &["normal_crps"][..],
        "direction" => &["direction_accuracy"][..],
        "interval" => &["interval_score"][..],
        other => return Err(reject(format!("unknown forecast kind {other:?}"))),
    };
    if !allowed.contains(&contract.scoring_rule.as_str()) {
        return Err(reject(format!(
            "scoring rule {:?} is invalid for kind {:?}",
            contract.scoring_rule, contract.kind
        )));
    }
    let category_set: BTreeSet<&str> = contract.categories.iter().map(String::as_str).collect();
    if contract.kind == "categorical" {
        if contract.categories.len() < 2 || category_set.len() != contract.categories.len() {
            return Err(reject(
                "categorical contracts need at least two unique categories",
            ));
        }
        for category in &contract.categories {
            nonempty(category, "categories[]")?;
        }
    } else if !contract.categories.is_empty() {
        return Err(reject(
            "categories are valid only for categorical contracts",
        ));
    }
    if contract.kind == "interval" {
        match contract.interval_alpha {
            Some(alpha) if alpha.is_finite() && alpha > 0.0 && alpha < 1.0 => {}
            _ => return Err(reject("interval contracts need interval_alpha in (0, 1)")),
        }
    } else if contract.interval_alpha.is_some() {
        return Err(reject(
            "interval_alpha is valid only for interval contracts",
        ));
    }
    contract_sha256(contract)
}

fn validate_prediction(
    contract: &ForecastContract,
    prediction: &[f64],
) -> Result<(), ForecastError> {
    if prediction.iter().any(|value| !value.is_finite()) {
        return Err(reject("prediction values must be finite"));
    }
    match contract.kind.as_str() {
        "point" => {
            if prediction.len() != 1 {
                return Err(reject("point prediction needs exactly one value"));
            }
        }
        "probability" => {
            if prediction.len() != 1 || !(0.0..=1.0).contains(&prediction[0]) {
                return Err(reject("probability prediction must be one value in [0, 1]"));
            }
            if contract.scoring_rule == "binary_log"
                && (prediction[0] <= 0.0 || prediction[0] >= 1.0)
            {
                return Err(reject("binary_log probability must lie inside (0, 1)"));
            }
        }
        "categorical" => {
            let sum: f64 = prediction.iter().sum();
            if prediction.len() != contract.categories.len()
                || prediction.iter().any(|value| !(0.0..=1.0).contains(value))
                || (sum - 1.0).abs() > 1e-12
            {
                return Err(reject(
                    "categorical prediction must be a simplex matching categories",
                ));
            }
            if contract.scoring_rule == "categorical_log"
                && prediction.iter().any(|value| *value <= 0.0)
            {
                return Err(reject("categorical_log probabilities must be positive"));
            }
        }
        "normal" => {
            if prediction.len() != 2 || prediction[1] <= 0.0 {
                return Err(reject("normal prediction must be [mean, positive sigma]"));
            }
        }
        "direction" => {
            if prediction.len() != 1 || !matches!(prediction[0], -1.0 | 1.0) {
                return Err(reject("direction prediction must be -1 or 1"));
            }
        }
        "interval" => {
            if prediction.len() != 2 || prediction[0] > prediction[1] {
                return Err(reject("interval prediction must be [lo, hi] with lo <= hi"));
            }
        }
        _ => return Err(reject("unknown forecast kind")),
    }
    Ok(())
}

fn validate_evidence(evidence: &ForecastEvidence) -> Result<(), ForecastError> {
    if evidence.schema_version != FORECAST_EVIDENCE_SCHEMA {
        return Err(reject("unsupported forecast evidence schema_version"));
    }
    if evidence.producer.name != "sharpearena" || evidence.producer.contract != "native" {
        return Err(reject("producer must be the native SharpeArena contract"));
    }
    for (field, value) in [
        ("agent_id", evidence.identity.agent_id.as_str()),
        ("model_id", evidence.identity.model_id.as_str()),
        ("scaffold_id", evidence.identity.scaffold_id.as_str()),
        ("operator_id", evidence.identity.operator_id.as_str()),
    ] {
        nonempty(value, field)?;
    }
    for (field, value) in [
        ("model_sha256", evidence.identity.model_sha256.as_str()),
        (
            "scaffold_sha256",
            evidence.identity.scaffold_sha256.as_str(),
        ),
        ("prompt_sha256", evidence.identity.prompt_sha256.as_str()),
        ("config_sha256", evidence.identity.config_sha256.as_str()),
    ] {
        digest(value, field)?;
    }
    if evidence.contracts.is_empty()
        || evidence.revisions.is_empty()
        || evidence.resolutions.is_empty()
    {
        return Err(reject(
            "contracts, revisions, and resolutions must all be non-empty",
        ));
    }

    let mut contracts = BTreeMap::new();
    let mut contract_ids = BTreeSet::new();
    for contract in &evidence.contracts {
        if !contract_ids.insert(contract.contract_id.as_str()) {
            return Err(reject(format!(
                "duplicate contract_id {:?}",
                contract.contract_id
            )));
        }
        let contract_digest = validate_contract(contract)?;
        contracts.insert(contract_digest, contract);
    }

    let mut revision_ids = BTreeSet::new();
    let mut idempotency_keys = BTreeSet::new();
    let mut by_claim: BTreeMap<&str, Vec<&ForecastRevision>> = BTreeMap::new();
    let mut claim_contracts: BTreeMap<&str, &str> = BTreeMap::new();
    for revision in &evidence.revisions {
        nonempty(&revision.revision_id, "revision_id")?;
        nonempty(&revision.claim_id, "claim_id")?;
        nonempty(&revision.idempotency_key, "idempotency_key")?;
        if !revision_ids.insert(revision.revision_id.as_str()) {
            return Err(reject(format!(
                "duplicate revision_id {:?}",
                revision.revision_id
            )));
        }
        if !idempotency_keys.insert(revision.idempotency_key.as_str()) {
            return Err(reject(format!(
                "duplicate idempotency_key {:?}",
                revision.idempotency_key
            )));
        }
        digest(&revision.contract_sha256, "contract_sha256")?;
        let contract = contracts
            .get(&revision.contract_sha256)
            .ok_or_else(|| reject("revision names an unknown contract digest"))?;
        match claim_contracts.insert(&revision.claim_id, &revision.contract_sha256) {
            Some(prior) if prior != revision.contract_sha256 => {
                return Err(reject("a claim changes contract across revisions"));
            }
            _ => {}
        }
        validate_prediction(contract, &revision.prediction)?;
        finite(revision.confidence, "confidence")?;
        if !(0.0..=1.0).contains(&revision.confidence) {
            return Err(reject("confidence must lie in [0, 1]"));
        }
        if revision.exposure.observed_at != revision.submitted_at {
            return Err(reject("exposure.observed_at must equal submitted_at"));
        }
        if let Some(hash) = &revision.exposure.market_snapshot_sha256 {
            digest(hash, "market_snapshot_sha256")?;
        }
        match (
            revision.exposure.consensus_visible,
            &revision.exposure.consensus_snapshot_sha256,
        ) {
            (true, Some(hash)) => digest(hash, "consensus_snapshot_sha256")?,
            (true, None) => return Err(reject("visible consensus requires its snapshot digest")),
            (false, Some(_)) => {
                return Err(reject("hidden consensus cannot carry a snapshot digest"));
            }
            (false, None) => {}
        }
        let sources: BTreeSet<&str> = revision
            .exposure
            .source_ids
            .iter()
            .map(String::as_str)
            .collect();
        if sources.len() != revision.exposure.source_ids.len()
            || sources.iter().any(|source| source.trim().is_empty())
        {
            return Err(reject("source_ids must be unique and non-empty"));
        }
        match revision.status.as_str() {
            "eligible"
                if revision.submitted_at >= contract.opens_at
                    && revision.submitted_at <= contract.deadline
                    && revision.reason.is_none() => {}
            "late"
                if revision.submitted_at > contract.deadline
                    && revision
                        .reason
                        .as_deref()
                        .is_some_and(|reason| !reason.trim().is_empty()) => {}
            "rejected"
                if revision.submitted_at < contract.opens_at
                    && revision
                        .reason
                        .as_deref()
                        .is_some_and(|reason| !reason.trim().is_empty()) => {}
            _ => {
                return Err(reject(
                    "revision status, clock, and reason are inconsistent",
                ))
            }
        }
        if revision
            .trigger_event_id
            .as_deref()
            .is_some_and(|value| value.trim().is_empty())
        {
            return Err(reject("trigger_event_id must be non-empty when present"));
        }
        let chain = by_claim.entry(&revision.claim_id).or_default();
        if revision.ordinal as usize != chain.len() {
            return Err(reject("claim revisions are not in append order"));
        }
        let expected = chain.last().map(|prior| prior.revision_id.as_str());
        if revision.supersedes.as_deref() != expected {
            return Err(reject("revision has the wrong supersedes link"));
        }
        if revision.ordinal == 0 && revision.revision_reason.is_some() {
            return Err(reject("initial forecast cannot have revision_reason"));
        }
        if revision.ordinal > 0
            && revision
                .revision_reason
                .as_deref()
                .is_none_or(|reason| reason.trim().is_empty())
        {
            return Err(reject("a revision requires revision_reason"));
        }
        chain.push(revision);
    }
    let referenced: BTreeSet<&str> = claim_contracts.values().copied().collect();
    if referenced.len() != contracts.len() {
        return Err(reject(
            "every exported contract must be referenced by a claim",
        ));
    }

    let mut resolutions = BTreeMap::new();
    for resolution in &evidence.resolutions {
        nonempty(&resolution.claim_id, "resolution.claim_id")?;
        let revisions = by_claim
            .get(resolution.claim_id.as_str())
            .ok_or_else(|| reject("resolution names an unknown claim"))?;
        if resolutions
            .insert(resolution.claim_id.as_str(), resolution)
            .is_some()
        {
            return Err(reject("duplicate resolution for a claim"));
        }
        let eligible = revisions
            .iter()
            .rev()
            .find(|revision| revision.status == "eligible")
            .copied();
        match resolution.status.as_str() {
            "resolved" => {
                let revision =
                    eligible.ok_or_else(|| reject("resolved claim has no eligible revision"))?;
                let available_at = resolution
                    .available_at
                    .ok_or_else(|| reject("resolved record needs available_at"))?;
                let outcome = resolution
                    .outcome
                    .as_ref()
                    .ok_or_else(|| reject("resolved record needs an outcome"))?;
                if resolution.reason.is_some() {
                    return Err(reject("resolved record cannot carry a reason"));
                }
                let contract = contracts
                    .get(&revision.contract_sha256)
                    .ok_or_else(|| reject("resolved revision lost its contract"))?;
                if available_at <= revision.submitted_at || available_at < contract.resolves_at {
                    return Err(reject(
                        "resolution predates the frozen information boundary",
                    ));
                }
                score_prediction(contract, &revision.prediction, outcome)?;
            }
            "pending" => {
                if eligible.is_none()
                    || resolution.outcome.is_some()
                    || resolution.available_at.is_some()
                    || resolution.reason.is_some()
                {
                    return Err(reject("pending resolution fields are inconsistent"));
                }
            }
            "cancelled" => {
                if resolution.outcome.is_some()
                    || resolution.available_at.is_some()
                    || resolution
                        .reason
                        .as_deref()
                        .is_none_or(|reason| reason.trim().is_empty())
                {
                    return Err(reject("cancelled resolution fields are inconsistent"));
                }
            }
            "rejected" => {
                if eligible.is_some()
                    || resolution.outcome.is_some()
                    || resolution
                        .reason
                        .as_deref()
                        .is_none_or(|reason| reason.trim().is_empty())
                {
                    return Err(reject("rejected resolution fields are inconsistent"));
                }
            }
            _ => return Err(reject("unknown resolution status")),
        }
    }
    if resolutions.len() != by_claim.len() {
        return Err(reject("resolutions must cover every claim exactly once"));
    }
    Ok(())
}

fn canonical_json(value: &Value, output: &mut String) -> Result<(), ForecastError> {
    match value {
        Value::Null => output.push_str("null"),
        Value::Bool(value) => output.push_str(if *value { "true" } else { "false" }),
        Value::Number(number) => output.push_str(&python_number(number)),
        Value::String(value) => output.push_str(
            &serde_json::to_string(value)
                .map_err(|error| reject(format!("cannot encode contract string: {error}")))?,
        ),
        Value::Array(values) => {
            output.push('[');
            for (index, value) in values.iter().enumerate() {
                if index > 0 {
                    output.push(',');
                }
                canonical_json(value, output)?;
            }
            output.push(']');
        }
        Value::Object(values) => {
            output.push('{');
            let mut fields: Vec<_> = values.iter().collect();
            fields.sort_by(|left, right| left.0.cmp(right.0));
            for (index, (key, value)) in fields.into_iter().enumerate() {
                if index > 0 {
                    output.push(',');
                }
                output.push_str(
                    &serde_json::to_string(key)
                        .map_err(|error| reject(format!("cannot encode contract key: {error}")))?,
                );
                output.push(':');
                canonical_json(value, output)?;
            }
            output.push('}');
        }
    }
    Ok(())
}

fn python_number(number: &serde_json::Number) -> String {
    let rendered = number.to_string();
    let Some((mantissa, exponent)) = rendered.split_once('e') else {
        return rendered;
    };
    let (sign, digits) = if let Some(rest) = exponent.strip_prefix('-') {
        ("-", rest)
    } else if let Some(rest) = exponent.strip_prefix('+') {
        ("+", rest)
    } else {
        ("+", exponent)
    };
    format!("{mantissa}e{sign}{digits:0>2}")
}

fn contract_sha256(contract: &ForecastContract) -> Result<String, ForecastError> {
    let value = serde_json::to_value(contract)
        .map_err(|error| reject(format!("cannot serialize forecast contract: {error}")))?;
    let mut preimage = String::new();
    canonical_json(&value, &mut preimage)?;
    Ok(format!("{:x}", Sha256::digest(preimage.as_bytes())))
}

fn number_outcome(outcome: &Value) -> Result<f64, ForecastError> {
    let value = outcome
        .as_f64()
        .ok_or_else(|| reject("forecast outcome must be numeric"))?;
    finite(value, "outcome")?;
    Ok(value)
}

#[derive(Clone, Debug)]
struct ScoredForecast {
    contract_sha256: String,
    instrument: String,
    resolves_at: u64,
    scoring_rule: String,
    loss: f64,
    probability: Option<(f64, bool)>,
    categorical_confidence: Option<(f64, bool)>,
    normal_pit: Option<f64>,
    consensus_visible: bool,
}

type ScoreParts = (f64, Option<(f64, bool)>, Option<(f64, bool)>, Option<f64>);

fn score_prediction(
    contract: &ForecastContract,
    prediction: &[f64],
    outcome: &Value,
) -> Result<ScoreParts, ForecastError> {
    validate_prediction(contract, prediction)?;
    match contract.kind.as_str() {
        "point" => {
            let error = number_outcome(outcome)? - prediction[0];
            Ok((error * error, None, None, None))
        }
        "probability" => {
            let realized = number_outcome(outcome)?;
            if realized != 0.0 && realized != 1.0 {
                return Err(reject("binary outcome must be 0 or 1"));
            }
            let event = realized == 1.0;
            let probability = prediction[0];
            let loss = if contract.scoring_rule == "binary_brier" {
                (probability - realized).powi(2)
            } else if event {
                -probability.ln()
            } else {
                -(1.0 - probability).ln()
            };
            Ok((loss, Some((probability, event)), None, None))
        }
        "categorical" => {
            let realized = outcome
                .as_str()
                .ok_or_else(|| reject("categorical outcome must be a string"))?;
            let index = contract
                .categories
                .iter()
                .position(|category| category == realized)
                .ok_or_else(|| reject("categorical outcome is not in the frozen categories"))?;
            let loss = if contract.scoring_rule == "categorical_brier" {
                prediction
                    .iter()
                    .enumerate()
                    .map(|(at, probability)| {
                        let target = if at == index { 1.0 } else { 0.0 };
                        (probability - target).powi(2)
                    })
                    .sum()
            } else {
                -prediction[index].ln()
            };
            let (chosen, confidence) = prediction
                .iter()
                .copied()
                .enumerate()
                .max_by(|left, right| left.1.total_cmp(&right.1))
                .ok_or_else(|| reject("categorical prediction is empty"))?;
            Ok((loss, None, Some((confidence, chosen == index)), None))
        }
        "normal" => {
            let realized = number_outcome(outcome)?;
            let mean = prediction[0];
            let sigma = prediction[1];
            let z = (realized - mean) / sigma;
            let phi = (-0.5 * z * z).exp() / (2.0 * std::f64::consts::PI).sqrt();
            let cdf = norm_cdf(z);
            let crps =
                sigma * (z * (2.0 * cdf - 1.0) + 2.0 * phi - 1.0 / std::f64::consts::PI.sqrt());
            Ok((crps, None, None, Some(cdf)))
        }
        "direction" => {
            let realized = number_outcome(outcome)?;
            let direction = if realized.abs() <= contract.neutral_threshold {
                0.0
            } else {
                realized.signum()
            };
            let correct = direction != 0.0 && direction == prediction[0];
            Ok((if correct { 0.0 } else { 1.0 }, None, None, None))
        }
        "interval" => {
            let realized = number_outcome(outcome)?;
            let alpha = contract
                .interval_alpha
                .ok_or_else(|| reject("interval_alpha is missing"))?;
            let (lower, upper) = (prediction[0], prediction[1]);
            let penalty = if realized < lower {
                2.0 * (lower - realized) / alpha
            } else if realized > upper {
                2.0 * (realized - upper) / alpha
            } else {
                0.0
            };
            Ok((upper - lower + penalty, None, None, None))
        }
        _ => Err(reject("unknown forecast kind")),
    }
}

fn scored_forecasts(evidence: &ForecastEvidence) -> Result<Vec<ScoredForecast>, ForecastError> {
    let mut contracts = BTreeMap::new();
    for contract in &evidence.contracts {
        contracts.insert(contract_sha256(contract)?, contract);
    }
    let mut effective: BTreeMap<&str, &ForecastRevision> = BTreeMap::new();
    for revision in &evidence.revisions {
        if revision.status == "eligible" {
            effective.insert(&revision.claim_id, revision);
        }
    }
    let resolutions: BTreeMap<&str, &ForecastResolution> = evidence
        .resolutions
        .iter()
        .map(|resolution| (resolution.claim_id.as_str(), resolution))
        .collect();
    let mut used_contracts = BTreeSet::new();
    let mut scored = Vec::new();
    for (claim_id, revision) in effective {
        let Some(resolution) = resolutions.get(claim_id) else {
            continue;
        };
        if resolution.status != "resolved" {
            continue;
        }
        if !used_contracts.insert(revision.contract_sha256.as_str()) {
            return Err(reject(
                "one agent has multiple effective claims for the same contract",
            ));
        }
        let contract = contracts
            .get(&revision.contract_sha256)
            .ok_or_else(|| reject("effective revision names an unknown contract"))?;
        let outcome = resolution
            .outcome
            .as_ref()
            .ok_or_else(|| reject("resolved record has no outcome"))?;
        let (loss, probability, categorical_confidence, normal_pit) =
            score_prediction(contract, &revision.prediction, outcome)?;
        scored.push(ScoredForecast {
            contract_sha256: revision.contract_sha256.clone(),
            instrument: contract.instrument.clone(),
            resolves_at: contract.resolves_at,
            scoring_rule: contract.scoring_rule.clone(),
            loss,
            probability,
            categorical_confidence,
            normal_pit,
            consensus_visible: revision.exposure.consensus_visible,
        });
    }
    Ok(scored)
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ForecastAnalysisConfig {
    pub bootstrap_seed: u64,
    pub bootstrap_samples: usize,
    pub confidence: f64,
    pub familywise_alpha: f64,
    pub calibration_bins: usize,
}

impl Default for ForecastAnalysisConfig {
    fn default() -> Self {
        Self {
            bootstrap_seed: 0xF0EC_A57A_2026,
            bootstrap_samples: 2_000,
            confidence: 0.95,
            familywise_alpha: 0.05,
            calibration_bins: 10,
        }
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct MetricMean {
    pub scoring_rule: String,
    pub n: usize,
    pub mean_loss: f64,
}

#[derive(Clone, Debug, Serialize)]
pub struct CalibrationBin {
    pub lower: f64,
    pub upper: f64,
    pub n: usize,
    pub mean_forecast: Option<f64>,
    pub event_rate: Option<f64>,
}

#[derive(Clone, Debug, Serialize)]
pub struct BinaryCalibration {
    pub n: usize,
    pub brier: f64,
    pub base_rate: f64,
    pub reliability: f64,
    pub resolution: f64,
    pub uncertainty: f64,
    pub brier_skill: Option<f64>,
    pub bins: Vec<CalibrationBin>,
}

#[derive(Clone, Debug, Serialize)]
pub struct ConfidenceCalibration {
    pub n: usize,
    pub mean_confidence: f64,
    pub accuracy: f64,
    pub bins: Vec<CalibrationBin>,
}

#[derive(Clone, Debug, Serialize)]
pub struct DistributionCalibration {
    pub n: usize,
    pub pit_mean: f64,
    pub pit_variance: f64,
    pub uniform_reference_mean: f64,
    pub uniform_reference_variance: f64,
    pub bin_counts: Vec<usize>,
}

#[derive(Clone, Debug, Serialize)]
pub struct AgentForecastSummary {
    pub agent_id: String,
    pub model_id: String,
    pub n_claims: usize,
    pub n_revisions: usize,
    pub n_eligible_claims: usize,
    pub n_resolved: usize,
    pub n_pending: usize,
    pub n_cancelled: usize,
    pub n_rejected: usize,
    pub resolution_rate: f64,
    pub blind_resolved: usize,
    pub consensus_exposed_resolved: usize,
    pub metrics: Vec<MetricMean>,
    pub binary_calibration: Option<BinaryCalibration>,
    pub categorical_confidence_calibration: Option<ConfidenceCalibration>,
    pub normal_distribution_calibration: Option<DistributionCalibration>,
}

#[derive(Clone, Debug, Serialize)]
pub struct CommonSupport {
    pub n_contracts: usize,
    pub contract_sha256: Vec<String>,
    pub excluded_resolved_by_agent: BTreeMap<String, usize>,
}

#[derive(Clone, Debug, Serialize)]
pub struct PairwiseForecastComparison {
    pub agent_a: String,
    pub agent_b: String,
    pub n_contracts: usize,
    pub n_settlement_blocks: usize,
    /// Mean loss(A) minus mean loss(B); negative favors A.
    pub mean_loss_difference: f64,
    pub confidence_lower: f64,
    pub confidence_upper: f64,
    pub raw_p_value: f64,
    pub holm_adjusted_p_value: f64,
    pub familywise_significant: bool,
}

#[derive(Clone, Debug, Serialize)]
pub struct ForecastQualityReport {
    pub schema_version: &'static str,
    pub rank_effect: &'static str,
    pub dependence_unit: &'static str,
    pub config: ForecastAnalysisConfig,
    pub common_support: CommonSupport,
    pub agents: Vec<AgentForecastSummary>,
    pub comparisons: Vec<PairwiseForecastComparison>,
}

/// Recompute forecast quality without changing or consulting trading-rank eligibility.
pub fn analyze_forecast_quality(
    evidence: &[ForecastEvidence],
    config: ForecastAnalysisConfig,
) -> Result<ForecastQualityReport, ForecastError> {
    if evidence.is_empty() {
        return Err(reject(
            "at least one forecast evidence document is required",
        ));
    }
    if config.bootstrap_samples == 0
        || config.calibration_bins == 0
        || !config.confidence.is_finite()
        || !(0.0..1.0).contains(&config.confidence)
        || !config.familywise_alpha.is_finite()
        || !(0.0..1.0).contains(&config.familywise_alpha)
    {
        return Err(reject(
            "forecast analysis configuration is outside its valid range",
        ));
    }
    let mut agent_ids = BTreeSet::new();
    let mut rows = Vec::new();
    for document in evidence {
        validate_evidence(document)?;
        if !agent_ids.insert(document.identity.agent_id.as_str()) {
            return Err(reject("forecast field contains duplicate agent_id values"));
        }
        rows.push(scored_forecasts(document)?);
    }
    let common: BTreeSet<String> = rows
        .iter()
        .map(|agent| {
            agent
                .iter()
                .map(|row| row.contract_sha256.clone())
                .collect::<BTreeSet<_>>()
        })
        .reduce(|left, right| left.intersection(&right).cloned().collect())
        .unwrap_or_default();
    let excluded_resolved_by_agent = evidence
        .iter()
        .zip(&rows)
        .map(|(document, rows)| {
            (
                document.identity.agent_id.clone(),
                rows.len().saturating_sub(common.len()),
            )
        })
        .collect();
    let agents = evidence
        .iter()
        .zip(&rows)
        .map(|(document, rows)| summarize_agent(document, rows, config.calibration_bins))
        .collect();
    let mut comparisons = Vec::new();
    for left in 0..evidence.len() {
        for right in (left + 1)..evidence.len() {
            comparisons.push(compare_agents(
                &evidence[left].identity.agent_id,
                &rows[left],
                &evidence[right].identity.agent_id,
                &rows[right],
                &common,
                config,
            )?);
        }
    }
    holm_adjust(&mut comparisons, config.familywise_alpha);
    Ok(ForecastQualityReport {
        schema_version: FORECAST_QUALITY_SCHEMA,
        rank_effect: "reported_only_never_trading_rank",
        dependence_unit: "whole resolution-clock block across assets and questions",
        config,
        common_support: CommonSupport {
            n_contracts: common.len(),
            contract_sha256: common.into_iter().collect(),
            excluded_resolved_by_agent,
        },
        agents,
        comparisons,
    })
}

fn summarize_agent(
    document: &ForecastEvidence,
    rows: &[ScoredForecast],
    calibration_bins: usize,
) -> AgentForecastSummary {
    let claim_ids: BTreeSet<&str> = document
        .revisions
        .iter()
        .map(|revision| revision.claim_id.as_str())
        .collect();
    let eligible_ids: BTreeSet<&str> = document
        .revisions
        .iter()
        .filter(|revision| revision.status == "eligible")
        .map(|revision| revision.claim_id.as_str())
        .collect();
    let status_count = |status: &str| {
        document
            .resolutions
            .iter()
            .filter(|resolution| resolution.status == status)
            .count()
    };
    let mut metrics: BTreeMap<&str, Vec<f64>> = BTreeMap::new();
    for row in rows {
        metrics
            .entry(row.scoring_rule.as_str())
            .or_default()
            .push(row.loss);
    }
    let metrics = metrics
        .into_iter()
        .map(|(rule, values)| MetricMean {
            scoring_rule: rule.to_string(),
            n: values.len(),
            mean_loss: mean(&values),
        })
        .collect();
    let binary: Vec<_> = rows.iter().filter_map(|row| row.probability).collect();
    let categorical: Vec<_> = rows
        .iter()
        .filter_map(|row| row.categorical_confidence)
        .collect();
    let pits: Vec<_> = rows.iter().filter_map(|row| row.normal_pit).collect();
    AgentForecastSummary {
        agent_id: document.identity.agent_id.clone(),
        model_id: document.identity.model_id.clone(),
        n_claims: claim_ids.len(),
        n_revisions: document.revisions.len(),
        n_eligible_claims: eligible_ids.len(),
        n_resolved: status_count("resolved"),
        n_pending: status_count("pending"),
        n_cancelled: status_count("cancelled"),
        n_rejected: status_count("rejected"),
        resolution_rate: status_count("resolved") as f64 / claim_ids.len() as f64,
        blind_resolved: rows.iter().filter(|row| !row.consensus_visible).count(),
        consensus_exposed_resolved: rows.iter().filter(|row| row.consensus_visible).count(),
        metrics,
        binary_calibration: binary_calibration(&binary, calibration_bins),
        categorical_confidence_calibration: confidence_calibration(&categorical, calibration_bins),
        normal_distribution_calibration: distribution_calibration(&pits, calibration_bins),
    }
}

fn calibration_bins(values: &[(f64, bool)], bins: usize) -> Vec<CalibrationBin> {
    (0..bins)
        .map(|index| {
            let selected: Vec<_> = values
                .iter()
                .filter(|(probability, _)| {
                    ((*probability * bins as f64).floor() as usize).min(bins - 1) == index
                })
                .collect();
            CalibrationBin {
                lower: index as f64 / bins as f64,
                upper: (index + 1) as f64 / bins as f64,
                n: selected.len(),
                mean_forecast: (!selected.is_empty()).then(|| {
                    selected
                        .iter()
                        .map(|(probability, _)| probability)
                        .sum::<f64>()
                        / selected.len() as f64
                }),
                event_rate: (!selected.is_empty()).then(|| {
                    selected.iter().filter(|(_, event)| *event).count() as f64
                        / selected.len() as f64
                }),
            }
        })
        .collect()
}

fn binary_calibration(values: &[(f64, bool)], bins: usize) -> Option<BinaryCalibration> {
    if values.is_empty() {
        return None;
    }
    let bins = calibration_bins(values, bins);
    let n = values.len() as f64;
    let base_rate = values.iter().filter(|(_, event)| *event).count() as f64 / n;
    let brier = values
        .iter()
        .map(|(probability, event)| (probability - if *event { 1.0 } else { 0.0 }).powi(2))
        .sum::<f64>()
        / n;
    let reliability = bins
        .iter()
        .filter_map(|bin| Some((bin.n as f64 / n, bin.mean_forecast?, bin.event_rate?)))
        .map(|(weight, forecast, observed)| weight * (forecast - observed).powi(2))
        .sum();
    let resolution = bins
        .iter()
        .filter_map(|bin| Some((bin.n as f64 / n, bin.event_rate?)))
        .map(|(weight, observed)| weight * (observed - base_rate).powi(2))
        .sum();
    let uncertainty = base_rate * (1.0 - base_rate);
    Some(BinaryCalibration {
        n: values.len(),
        brier,
        base_rate,
        reliability,
        resolution,
        uncertainty,
        brier_skill: (uncertainty > 0.0).then_some(1.0 - brier / uncertainty),
        bins,
    })
}

fn confidence_calibration(values: &[(f64, bool)], bins: usize) -> Option<ConfidenceCalibration> {
    if values.is_empty() {
        return None;
    }
    Some(ConfidenceCalibration {
        n: values.len(),
        mean_confidence: values.iter().map(|(confidence, _)| confidence).sum::<f64>()
            / values.len() as f64,
        accuracy: values.iter().filter(|(_, correct)| *correct).count() as f64
            / values.len() as f64,
        bins: calibration_bins(values, bins),
    })
}

fn distribution_calibration(values: &[f64], bins: usize) -> Option<DistributionCalibration> {
    if values.is_empty() {
        return None;
    }
    let average = mean(values);
    let variance = values
        .iter()
        .map(|value| (value - average).powi(2))
        .sum::<f64>()
        / values.len() as f64;
    let mut bin_counts = vec![0; bins];
    for value in values {
        let index = ((*value * bins as f64).floor() as usize).min(bins - 1);
        bin_counts[index] += 1;
    }
    Some(DistributionCalibration {
        n: values.len(),
        pit_mean: average,
        pit_variance: variance,
        uniform_reference_mean: 0.5,
        uniform_reference_variance: 1.0 / 12.0,
        bin_counts,
    })
}

fn mean(values: &[f64]) -> f64 {
    values.iter().sum::<f64>() / values.len() as f64
}

#[derive(Clone, Copy)]
struct SplitMix64(u64);

impl SplitMix64 {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut value = self.0;
        value = (value ^ (value >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        value = (value ^ (value >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        value ^ (value >> 31)
    }

    fn below(&mut self, upper: usize) -> usize {
        ((self.next() as u128 * upper as u128) >> 64) as usize
    }
}

fn percentile(sorted: &[f64], probability: f64) -> f64 {
    let index = ((sorted.len() - 1) as f64 * probability).round() as usize;
    sorted[index]
}

fn compare_agents(
    agent_a: &str,
    rows_a: &[ScoredForecast],
    agent_b: &str,
    rows_b: &[ScoredForecast],
    common: &BTreeSet<String>,
    config: ForecastAnalysisConfig,
) -> Result<PairwiseForecastComparison, ForecastError> {
    let by_hash_a: BTreeMap<_, _> = rows_a
        .iter()
        .map(|row| (row.contract_sha256.as_str(), row))
        .collect();
    let by_hash_b: BTreeMap<_, _> = rows_b
        .iter()
        .map(|row| (row.contract_sha256.as_str(), row))
        .collect();
    let mut blocks: BTreeMap<u64, Vec<f64>> = BTreeMap::new();
    for contract_hash in common {
        let left = by_hash_a
            .get(contract_hash.as_str())
            .ok_or_else(|| reject("common support is absent from agent A"))?;
        let right = by_hash_b
            .get(contract_hash.as_str())
            .ok_or_else(|| reject("common support is absent from agent B"))?;
        if left.resolves_at != right.resolves_at
            || left.instrument != right.instrument
            || left.scoring_rule != right.scoring_rule
        {
            return Err(reject(
                "equal contract digest resolved to unequal contract semantics",
            ));
        }
        blocks
            .entry(left.resolves_at)
            .or_default()
            .push(left.loss - right.loss);
    }
    let observed_values: Vec<f64> = blocks.values().flatten().copied().collect();
    if observed_values.is_empty() {
        return Ok(PairwiseForecastComparison {
            agent_a: agent_a.to_string(),
            agent_b: agent_b.to_string(),
            n_contracts: 0,
            n_settlement_blocks: 0,
            mean_loss_difference: 0.0,
            confidence_lower: 0.0,
            confidence_upper: 0.0,
            raw_p_value: 1.0,
            holm_adjusted_p_value: 1.0,
            familywise_significant: false,
        });
    }
    let observed = mean(&observed_values);
    let block_values: Vec<&Vec<f64>> = blocks.values().collect();
    let mut rng = SplitMix64(config.bootstrap_seed ^ stable_pair_seed(agent_a, agent_b));
    let mut samples = Vec::with_capacity(config.bootstrap_samples);
    let mut null_extreme = 0usize;
    for _ in 0..config.bootstrap_samples {
        let mut sample = Vec::new();
        for _ in 0..block_values.len() {
            sample.extend_from_slice(block_values[rng.below(block_values.len())]);
        }
        let sample_mean = mean(&sample);
        samples.push(sample_mean);
        let centered =
            sample.iter().map(|value| value - observed).sum::<f64>() / sample.len() as f64;
        if centered.abs() >= observed.abs() {
            null_extreme += 1;
        }
    }
    samples.sort_by(f64::total_cmp);
    let tail = (1.0 - config.confidence) / 2.0;
    Ok(PairwiseForecastComparison {
        agent_a: agent_a.to_string(),
        agent_b: agent_b.to_string(),
        n_contracts: observed_values.len(),
        n_settlement_blocks: block_values.len(),
        mean_loss_difference: observed,
        confidence_lower: percentile(&samples, tail),
        confidence_upper: percentile(&samples, 1.0 - tail),
        raw_p_value: (null_extreme as f64 + 1.0) / (config.bootstrap_samples as f64 + 1.0),
        holm_adjusted_p_value: 1.0,
        familywise_significant: false,
    })
}

fn stable_pair_seed(left: &str, right: &str) -> u64 {
    let digest = Sha256::digest(format!("{left}\0{right}").as_bytes());
    u64::from_be_bytes(digest[..8].try_into().unwrap_or([0; 8]))
}

fn holm_adjust(comparisons: &mut [PairwiseForecastComparison], alpha: f64) {
    let mut order: Vec<usize> = (0..comparisons.len()).collect();
    order.sort_by(|left, right| {
        comparisons[*left]
            .raw_p_value
            .total_cmp(&comparisons[*right].raw_p_value)
    });
    let mut prior = 0.0_f64;
    let family = comparisons.len();
    for (rank, index) in order.into_iter().enumerate() {
        let adjusted = ((family - rank) as f64 * comparisons[index].raw_p_value)
            .max(prior)
            .min(1.0);
        comparisons[index].holm_adjusted_p_value = adjusted;
        comparisons[index].familywise_significant = adjusted <= alpha;
        prior = adjusted;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(agent: &str, probabilities: &[f64], outcomes: &[f64]) -> String {
        assert_eq!(probabilities.len(), outcomes.len());
        let contracts: Vec<_> = probabilities
            .iter()
            .enumerate()
            .map(|(index, _)| ForecastContract {
                schema_version: CONTRACT_SCHEMA.to_string(),
                contract_id: format!("c{index}"),
                question: format!("question {index}"),
                instrument: if index % 2 == 0 { "ES" } else { "NQ" }.to_string(),
                target: "close_up".to_string(),
                kind: "probability".to_string(),
                opens_at: index as u64 * 2,
                deadline: index as u64 * 2 + 1,
                resolves_at: (index as u64 / 2 + 1) * 10,
                observation_source: "fixture:v1".to_string(),
                open_definition: "close at opens_at".to_string(),
                close_definition: "close at resolves_at".to_string(),
                unit: "binary".to_string(),
                scoring_rule: "binary_brier".to_string(),
                neutral_threshold: 0.001,
                boundary_ownership: "threshold is false".to_string(),
                missing_data_policy: "cancel".to_string(),
                fallback_policy: "cancel".to_string(),
                categories: vec![],
                interval_alpha: None,
            })
            .collect();
        let hashes: Vec<_> = contracts
            .iter()
            .map(|contract| contract_sha256(contract).unwrap())
            .collect();
        let evidence = ForecastEvidence {
            schema_version: FORECAST_EVIDENCE_SCHEMA.to_string(),
            producer: ForecastProducer {
                name: "sharpearena".to_string(),
                contract: "native".to_string(),
            },
            generated_at: 100,
            identity: ForecastRunIdentity {
                agent_id: agent.to_string(),
                model_id: format!("model-{agent}"),
                model_sha256: "a".repeat(64),
                scaffold_id: "scaffold".to_string(),
                scaffold_sha256: "b".repeat(64),
                prompt_sha256: "c".repeat(64),
                operator_id: "operator".to_string(),
                config_sha256: "d".repeat(64),
            },
            revisions: probabilities
                .iter()
                .enumerate()
                .map(|(index, probability)| ForecastRevision {
                    revision_id: format!("claim-{index}:r0"),
                    claim_id: format!("claim-{index}"),
                    ordinal: 0,
                    supersedes: None,
                    contract_sha256: hashes[index].clone(),
                    prediction: vec![*probability],
                    confidence: *probability,
                    rationale: "evidence".to_string(),
                    submitted_at: index as u64 * 2,
                    status: "eligible".to_string(),
                    reason: None,
                    trigger_event_id: None,
                    revision_reason: None,
                    exposure: InformationExposure {
                        observed_at: index as u64 * 2,
                        market_snapshot_sha256: Some("e".repeat(64)),
                        consensus_visible: false,
                        consensus_snapshot_sha256: None,
                        source_ids: vec!["market".to_string()],
                    },
                    idempotency_key: format!("request-{index}"),
                })
                .collect(),
            resolutions: outcomes
                .iter()
                .enumerate()
                .map(|(index, outcome)| ForecastResolution {
                    claim_id: format!("claim-{index}"),
                    status: "resolved".to_string(),
                    outcome: Some(Value::from(*outcome)),
                    available_at: Some(contracts[index].resolves_at),
                    recorded_at: 100,
                    reason: None,
                })
                .collect(),
            contracts,
        };
        serde_json::to_string(&evidence).unwrap()
    }

    #[test]
    fn strict_ingestion_rejects_unknown_fields_and_contract_drift() {
        let payload = fixture("a", &[0.8], &[1.0]);
        let mut value: Value = serde_json::from_str(&payload).unwrap();
        value["revisions"][0]["unknown"] = Value::Bool(true);
        assert!(parse_forecast_evidence(&value.to_string()).is_err());

        let mut value: Value = serde_json::from_str(&payload).unwrap();
        value["contracts"][0]["question"] = Value::String("changed".to_string());
        let error = parse_forecast_evidence(&value.to_string()).unwrap_err();
        assert!(error.0.contains("unknown contract digest"));
    }

    #[test]
    fn sharpearena_golden_file_crosses_the_artifact_boundary_exactly() {
        let payload = include_str!("../tests/fixtures/sharpearena-forecast-evidence-v1.json");
        let document = parse_forecast_evidence(payload).unwrap();
        let report =
            analyze_forecast_quality(&[document], ForecastAnalysisConfig::default()).unwrap();
        assert_eq!(report.agents[0].agent_id, "arena-reference");
        assert_eq!(report.agents[0].metrics[0].scoring_rule, "binary_brier");
        assert!((report.agents[0].metrics[0].mean_loss - 0.09).abs() < 1e-12);
    }

    #[test]
    fn scoring_recomputes_brier_and_calibration_from_raw_evidence() {
        let document =
            parse_forecast_evidence(&fixture("a", &[0.8, 0.2, 0.7, 0.1], &[1.0, 0.0, 1.0, 0.0]))
                .unwrap();
        let report =
            analyze_forecast_quality(&[document], ForecastAnalysisConfig::default()).unwrap();
        let summary = &report.agents[0];
        assert!((summary.metrics[0].mean_loss - 0.045).abs() < 1e-12);
        let calibration = summary.binary_calibration.as_ref().unwrap();
        assert!((calibration.brier - 0.045).abs() < 1e-12);
        assert!((calibration.brier_skill.unwrap() - 0.82).abs() < 1e-12);
        assert_eq!(summary.blind_resolved, 4);
        assert_eq!(report.rank_effect, "reported_only_never_trading_rank");
    }

    #[test]
    fn late_revision_is_audited_but_never_becomes_the_scored_forecast() {
        let mut document =
            serde_json::from_str::<ForecastEvidence>(&fixture("a", &[0.8], &[1.0])).unwrap();
        let mut late = document.revisions[0].clone();
        late.revision_id = "claim-0:r1".to_string();
        late.ordinal = 1;
        late.supersedes = Some("claim-0:r0".to_string());
        late.prediction = vec![0.01];
        late.submitted_at = document.contracts[0].deadline + 1;
        late.status = "late".to_string();
        late.reason = Some("submitted after the contract deadline".to_string());
        late.revision_reason = Some("answer became easier".to_string());
        late.exposure.observed_at = late.submitted_at;
        late.idempotency_key = "late-request".to_string();
        document.revisions.push(late);
        validate_evidence(&document).unwrap();

        let report =
            analyze_forecast_quality(&[document], ForecastAnalysisConfig::default()).unwrap();
        assert_eq!(report.agents[0].n_revisions, 2);
        assert!((report.agents[0].metrics[0].mean_loss - 0.04).abs() < 1e-12);
    }

    #[test]
    fn comparison_uses_only_field_common_contracts_and_whole_time_blocks() {
        let a =
            parse_forecast_evidence(&fixture("a", &[0.9, 0.1, 0.8, 0.2], &[1.0, 0.0, 1.0, 0.0]))
                .unwrap();
        let b = parse_forecast_evidence(&fixture(
            "b",
            &[0.6, 0.4, 0.55, 0.45],
            &[1.0, 0.0, 1.0, 0.0],
        ))
        .unwrap();
        let report = analyze_forecast_quality(&[a, b], ForecastAnalysisConfig::default()).unwrap();
        assert_eq!(report.common_support.n_contracts, 4);
        assert_eq!(report.comparisons[0].n_settlement_blocks, 2);
        assert!(report.comparisons[0].mean_loss_difference < 0.0);
    }

    #[test]
    fn unmatched_questions_are_disclosed_and_excluded_from_every_comparison() {
        let a =
            parse_forecast_evidence(&fixture("a", &[0.9, 0.1, 0.8, 0.2], &[1.0, 0.0, 1.0, 0.0]))
                .unwrap();
        let mut b = parse_forecast_evidence(&fixture(
            "b",
            &[0.6, 0.4, 0.55, 0.45],
            &[1.0, 0.0, 1.0, 0.0],
        ))
        .unwrap();
        b.resolutions[3].status = "pending".to_string();
        b.resolutions[3].outcome = None;
        b.resolutions[3].available_at = None;
        let report = analyze_forecast_quality(&[a, b], ForecastAnalysisConfig::default()).unwrap();
        assert_eq!(report.common_support.n_contracts, 3);
        assert_eq!(report.common_support.excluded_resolved_by_agent["a"], 1);
        assert_eq!(report.common_support.excluded_resolved_by_agent["b"], 0);
        assert_eq!(report.comparisons[0].n_contracts, 3);
    }

    #[test]
    fn holm_adjustment_is_monotone_and_controls_the_family() {
        let mut comparisons = vec![0.01, 0.03, 0.04]
            .into_iter()
            .enumerate()
            .map(|(index, p)| PairwiseForecastComparison {
                agent_a: format!("a{index}"),
                agent_b: "b".to_string(),
                n_contracts: 10,
                n_settlement_blocks: 5,
                mean_loss_difference: -0.1,
                confidence_lower: -0.2,
                confidence_upper: -0.01,
                raw_p_value: p,
                holm_adjusted_p_value: 0.0,
                familywise_significant: false,
            })
            .collect::<Vec<_>>();
        holm_adjust(&mut comparisons, 0.05);
        assert_eq!(comparisons[0].holm_adjusted_p_value, 0.03);
        assert_eq!(comparisons[1].holm_adjusted_p_value, 0.06);
        assert_eq!(comparisons[2].holm_adjusted_p_value, 0.06);
        assert!(comparisons[0].familywise_significant);
        assert!(!comparisons[1].familywise_significant);
    }

    #[test]
    fn normal_crps_and_interval_score_match_closed_form_values() {
        let mut normal =
            serde_json::from_str::<ForecastEvidence>(&fixture("n", &[0.5], &[1.0])).unwrap();
        normal.contracts[0].kind = "normal".to_string();
        normal.contracts[0].scoring_rule = "normal_crps".to_string();
        normal.contracts[0].unit = "USD".to_string();
        normal.revisions[0].prediction = vec![1.0, 2.0];
        normal.revisions[0].contract_sha256 = contract_sha256(&normal.contracts[0]).unwrap();
        let score = score_prediction(
            &normal.contracts[0],
            &normal.revisions[0].prediction,
            normal.resolutions[0].outcome.as_ref().unwrap(),
        )
        .unwrap()
        .0;
        let expected =
            2.0 * ((2.0 / std::f64::consts::PI).sqrt() - 1.0 / std::f64::consts::PI.sqrt());
        assert!((score - expected).abs() < 1e-7);
    }

    #[test]
    fn every_declared_scoring_rule_is_recomputed_from_raw_values() {
        let base = serde_json::from_str::<ForecastEvidence>(&fixture("rules", &[0.5], &[1.0]))
            .unwrap()
            .contracts
            .remove(0);

        let mut binary_log = base.clone();
        binary_log.scoring_rule = "binary_log".to_string();
        let score = score_prediction(&binary_log, &[0.8], &Value::from(1.0))
            .unwrap()
            .0;
        assert!((score - -0.8_f64.ln()).abs() < 1e-12);

        let mut categorical = base.clone();
        categorical.kind = "categorical".to_string();
        categorical.scoring_rule = "categorical_brier".to_string();
        categorical.categories = vec!["up".into(), "flat".into(), "down".into()];
        let score = score_prediction(
            &categorical,
            &[0.2, 0.3, 0.5],
            &Value::String("down".to_string()),
        )
        .unwrap()
        .0;
        assert!((score - 0.38).abs() < 1e-12);
        categorical.scoring_rule = "categorical_log".to_string();
        let score = score_prediction(
            &categorical,
            &[0.2, 0.3, 0.5],
            &Value::String("flat".to_string()),
        )
        .unwrap()
        .0;
        assert!((score - -0.3_f64.ln()).abs() < 1e-12);

        let mut direction = base.clone();
        direction.kind = "direction".to_string();
        direction.scoring_rule = "direction_accuracy".to_string();
        direction.neutral_threshold = 0.01;
        assert_eq!(
            score_prediction(&direction, &[1.0], &Value::from(0.009))
                .unwrap()
                .0,
            1.0
        );

        let mut interval = base;
        interval.kind = "interval".to_string();
        interval.scoring_rule = "interval_score".to_string();
        interval.interval_alpha = Some(0.1);
        let score = score_prediction(&interval, &[8.0, 12.0], &Value::from(14.0))
            .unwrap()
            .0;
        assert!((score - 44.0).abs() < 1e-12);
    }

    #[test]
    fn forecast_analysis_cannot_change_the_trading_board() {
        use crate::composite::{rank, AgentSubmission, Run, ScoreConfig};

        let submission = AgentSubmission {
            agent_id: "trader".to_string(),
            runs: vec![Run {
                returns: (0..80)
                    .map(|index| 0.002 + 0.0005 * (index as f64 * 0.7).sin())
                    .collect(),
                ..Run::default()
            }],
            ..AgentSubmission::default()
        };
        let before = rank(std::slice::from_ref(&submission), &ScoreConfig::default());
        let evidence = parse_forecast_evidence(&fixture("forecast-only", &[0.9], &[0.0])).unwrap();
        let report =
            analyze_forecast_quality(&[evidence], ForecastAnalysisConfig::default()).unwrap();
        let after = rank(&[submission], &ScoreConfig::default());
        assert_eq!(before, after);
        assert_eq!(report.rank_effect, "reported_only_never_trading_rank");
    }
}
