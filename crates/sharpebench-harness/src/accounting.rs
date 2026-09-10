//! Frozen, rank-neutral token pricing. A quote is not a provider invoice.
//!
//! Version 1 prices uncached input and total output tokens in integer USD
//! nanodollars per token. Output already includes reasoning tokens. It does not
//! model cache discounts, batch discounts, tool fees, taxes or currency exchange.

use serde::{Deserialize, Serialize};
use sharpebench_protocol::{Decision, MarketObservation};
use sharpebench_sim::{Agent, TransportDiagnostics, TransportHealth};

pub const RATE_CARD_VERSION: &str = "sharpebench.token-rate-card.v1";
pub const MAX_RATE_CARD_BYTES: usize = 64 * 1024;

/// Operator-declared rates for one model revision. No live price lookup occurs.
/// Private validated fields keep the identity stable for the entire sweep.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "RateCardWire")]
pub struct RateCard {
    schema_version: String,
    provider: String,
    model: String,
    revision: String,
    input_usd_nanos_per_token: u64,
    output_usd_nanos_per_token: u64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RateCardWire {
    schema_version: String,
    provider: String,
    model: String,
    revision: String,
    input_usd_nanos_per_token: u64,
    output_usd_nanos_per_token: u64,
}

impl TryFrom<RateCardWire> for RateCard {
    type Error = String;

    fn try_from(wire: RateCardWire) -> Result<Self, Self::Error> {
        if wire.schema_version != RATE_CARD_VERSION {
            return Err(format!(
                "rate card schema_version must be {RATE_CARD_VERSION}"
            ));
        }
        for (name, value) in [
            ("provider", &wire.provider),
            ("model", &wire.model),
            ("revision", &wire.revision),
        ] {
            if value.is_empty()
                || value.len() > 256
                || value.trim() != value
                || value.chars().any(char::is_control)
            {
                return Err(format!("rate card {name} must be 1..256 bytes without surrounding whitespace or control characters"));
            }
        }
        Ok(Self {
            schema_version: wire.schema_version,
            provider: wire.provider,
            model: wire.model,
            revision: wire.revision,
            input_usd_nanos_per_token: wire.input_usd_nanos_per_token,
            output_usd_nanos_per_token: wire.output_usd_nanos_per_token,
        })
    }
}

impl RateCard {
    pub fn from_json(bytes: &[u8]) -> Result<Self, String> {
        if bytes.len() > MAX_RATE_CARD_BYTES {
            return Err("rate card exceeds the 64 KiB input limit".into());
        }
        serde_json::from_slice(bytes).map_err(|error| format!("invalid rate card: {error}"))
    }

    /// Digest of the validated, fixed-order integer record, independent of input
    /// JSON formatting. Its schema string versions this specific encoding.
    pub fn digest(&self) -> String {
        sharpebench_attest::content_digest(
            &serde_json::to_vec(self).expect("a validated integer rate card serializes"),
        )
    }

    pub fn provider(&self) -> &str {
        &self.provider
    }

    pub fn model(&self) -> &str {
        &self.model
    }

    pub fn revision(&self) -> &str {
        &self.revision
    }

    pub fn quote_nanos(&self, input: u64, output: u64) -> Option<u128> {
        (u128::from(input) * u128::from(self.input_usd_nanos_per_token))
            .checked_add(u128::from(output) * u128::from(self.output_usd_nanos_per_token))
    }
}

/// Usage from decisions observed during one attempt, including an attempt that
/// later fails. The legacy decision protocol defaults absent counts to zero;
/// an all-zero report therefore cannot establish an explicitly measured zero.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AttemptUsage {
    pub rate_card: RateCard,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub observed_decisions: u64,
    pub unpriced_decisions: u64,
    /// False on transport failure or arithmetic overflow. Known counts remain
    /// a partial subtotal, never silently promoted to complete monetary cost.
    pub complete: bool,
}

impl AttemptUsage {
    pub fn new(rate_card: RateCard) -> Self {
        Self {
            rate_card,
            input_tokens: 0,
            output_tokens: 0,
            observed_decisions: 0,
            unpriced_decisions: 0,
            complete: true,
        }
    }

    fn observe(&mut self, decision: &Decision) {
        self.observed_decisions = match self.observed_decisions.checked_add(1) {
            Some(count) => count,
            None => {
                self.complete = false;
                return;
            }
        };
        let cost = match decision.cost {
            Some(cost)
                if (cost.tokens_in != 0 || cost.tokens_out != 0)
                    && cost.reasoning_tokens <= cost.tokens_out =>
            {
                cost
            }
            _ => {
                self.unpriced_decisions = self.unpriced_decisions.saturating_add(1);
                self.complete = false;
                return;
            }
        };
        match (
            self.input_tokens.checked_add(cost.tokens_in),
            self.output_tokens.checked_add(cost.tokens_out),
        ) {
            (Some(input), Some(output)) => {
                self.input_tokens = input;
                self.output_tokens = output;
            }
            _ => self.complete = false,
        }
    }
}

/// Observe the existing protocol without changing decisions, returns, legacy
/// cost columns or rank. The model identity is declared by the operator, not
/// attested by this adapter; a later host gateway needs separate provenance.
pub struct UsageObservedAgent<'a, A> {
    inner: &'a mut A,
    usage: AttemptUsage,
}

impl<'a, A: Agent + TransportDiagnostics> UsageObservedAgent<'a, A> {
    pub fn new(inner: &'a mut A, rate_card: &RateCard) -> Self {
        Self {
            inner,
            usage: AttemptUsage::new(rate_card.clone()),
        }
    }

    pub fn into_usage(mut self) -> AttemptUsage {
        if self.inner.health().degraded() {
            self.usage.complete = false;
        }
        self.usage
    }
}

impl<A: Agent + TransportDiagnostics> Agent for UsageObservedAgent<'_, A> {
    fn decide(&mut self, observation: &MarketObservation) -> Decision {
        let decision = self.inner.decide(observation);
        self.usage.observe(&decision);
        decision
    }
}

impl<A: Agent + TransportDiagnostics> TransportDiagnostics for UsageObservedAgent<'_, A> {
    fn health(&self) -> &TransportHealth {
        self.inner.health()
    }
}

/// JSON-safe exact integer amounts are decimal strings, not rounded f64 values.
/// `unavailable` never includes a total; a known subtotal is labelled separately.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum MonetarySummary {
    Estimated {
        rate_card: RateCard,
        rate_card_sha256: String,
        usage_source: &'static str,
        usd_nanos: String,
    },
    Unavailable {
        reason: &'static str,
        #[serde(skip_serializing_if = "Option::is_none")]
        known_subtotal_usd_nanos: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        rate_card: Option<RateCard>,
        #[serde(skip_serializing_if = "Option::is_none")]
        rate_card_sha256: Option<String>,
    },
}

pub fn summarize_usage<'a>(
    records: impl IntoIterator<Item = Option<&'a AttemptUsage>>,
) -> MonetarySummary {
    let mut card: Option<&RateCard> = None;
    let mut total = 0_u128;
    let mut complete = true;
    for record in records {
        let Some(usage) = record else {
            complete = false;
            continue;
        };
        if card.is_some_and(|expected| expected != &usage.rate_card) {
            return unavailable("mixed_rate_card_identities", None, None);
        }
        card = Some(&usage.rate_card);
        complete &= usage.complete && usage.unpriced_decisions == 0 && usage.observed_decisions > 0;
        let Some(next) = usage
            .rate_card
            .quote_nanos(usage.input_tokens, usage.output_tokens)
            .and_then(|amount| total.checked_add(amount))
        else {
            return unavailable("monetary_arithmetic_overflow", None, card);
        };
        total = next;
    }
    match card {
        None => unavailable("attempt_ledger_has_no_usage_evidence", None, None),
        Some(card) if complete => MonetarySummary::Estimated {
            rate_card: card.clone(),
            rate_card_sha256: card.digest(),
            usage_source: "entrant_reported",
            usd_nanos: total.to_string(),
        },
        Some(card) => unavailable(
            "incomplete_usage_evidence",
            Some(total.to_string()),
            Some(card),
        ),
    }
}

fn unavailable(
    reason: &'static str,
    subtotal: Option<String>,
    card: Option<&RateCard>,
) -> MonetarySummary {
    MonetarySummary::Unavailable {
        reason,
        known_subtotal_usd_nanos: subtotal,
        rate_card: card.cloned(),
        rate_card_sha256: card.map(RateCard::digest),
    }
}
