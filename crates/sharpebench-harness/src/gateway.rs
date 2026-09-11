//! Host-observed model gateway: host-mediated model access for entrants that
//! have no network at all.
//!
//! # Why this shape
//!
//! An entrant runs with egress disabled. It reaches a model by writing one
//! newline-delimited JSON request on a pipe the host already owns (the same
//! stdio channel the external-agent protocol uses) and reading one
//! newline-delimited JSON response back. There is no listener, no port, no URL
//! and no TLS on the entrant side, so nothing about this path widens what an
//! entrant can reach: a container with no route to the internet can still be
//! evaluated against a hosted model, and the host sees every call.
//!
//! The request an entrant may write carries *what to ask*, never *where to ask
//! it*. Destination, credential, provider, model, revision, allowed revisions
//! and the frozen rate card are all host configuration, resolved from an alias
//! the host published. The request schema has no field for a URL, a header or a
//! provider-side identifier, and unknown fields are refused, so an entrant
//! cannot smuggle one in or correlate itself with another entrant's call.
//!
//! # What this is not
//!
//! Host-observed usage is what the host saw the provider report on the wire. It
//! is not verified billing, it is not an invoice, and it never reaches a score
//! or a rank. See `docs/audits/2026-09-09/HOST-ACCOUNTING.md`.
//!
//! # Provider transport
//!
//! The actual bytes-on-the-wire step is the [`ProviderTransport`] seam. This
//! crate ships no networked implementation: adding one here would put egress
//! into a library that is otherwise offline, and every test in this module is
//! hermetic by construction because the only transports that exist are fakes.
//! An operator supplies the transport, and the bounds in [`GatewayLimits`] are
//! handed to it rather than left to its discretion. Handing them over is not
//! enforcing them: the timeouts and the response-body bound are obligations the
//! adapter must satisfy, and [`ProviderTransport`] says exactly which parts of
//! them the broker can and cannot check.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use crate::accounting::RateCard;
use crate::gateway_journal::{
    GatewayBudget, GatewayJournal, JournalIdentity, JournalLock, JournalSaveError, ReleaseReason,
    Settlement, UnknownCostReason,
};

#[path = "gateway_serve.rs"]
pub mod serve;

pub const GATEWAY_PROTOCOL: &str = "sharpebench.model-gateway.v1";

/// Field names an entrant must never be able to set. They are refused by name
/// before the schema is even considered, so the refusal says what happened
/// rather than reporting an anonymous parse error.
const FORBIDDEN_REQUEST_FIELDS: &[&str] = &[
    "url",
    "endpoint",
    "base_url",
    "destination",
    "headers",
    "header",
    "authorization",
    "api_key",
    "apikey",
    "credential",
    "token",
    "provider",
    "response_id",
    "request_id",
    "session_id",
    "conversation_id",
    "user",
];

/// Every bound the gateway applies, in one place. Each is checked before the
/// allocation it governs, not after.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GatewayLimits {
    /// Bytes accepted for one request line, before it is parsed.
    pub max_request_line_bytes: usize,
    /// Bytes the host will emit for one response line.
    pub max_response_line_bytes: usize,
    /// Messages in one request.
    pub max_messages: usize,
    /// Bytes of content in one message.
    pub max_message_bytes: usize,
    /// Tool definitions in one request.
    pub max_tools: usize,
    /// Bytes of one tool payload (name plus schema).
    pub max_tool_payload_bytes: usize,
    /// Output tokens the host will ask a provider for, whatever the entrant asks.
    pub max_output_tokens: u32,
    /// Bytes accepted from a provider response body.
    pub max_response_body_bytes: usize,
    /// Characters of model text handed back to the entrant.
    pub max_response_text_bytes: usize,
    /// Extra dispatches after the first for one entrant request.
    pub max_retries_per_request: u32,
    /// Concurrent provider calls across every entrant sharing the permit pool.
    pub max_concurrent_calls: u32,
    /// Read timeout handed to the provider client for one socket read.
    pub provider_read_timeout: Duration,
    /// Wall-clock ceiling for one dispatch, connect and every read included.
    pub provider_call_timeout: Duration,
    /// Gateway requests an entrant may make while answering one observation. A
    /// circuit breaker against a runaway loop, not a budget: it resets with
    /// every observation, and the sweep-scoped budget is the journal's.
    pub max_requests_per_decision: u32,
}

impl Default for GatewayLimits {
    fn default() -> Self {
        Self {
            max_request_line_bytes: 256 * 1024,
            max_response_line_bytes: 256 * 1024,
            max_messages: 64,
            max_message_bytes: 32 * 1024,
            max_tools: 16,
            max_tool_payload_bytes: 32 * 1024,
            max_output_tokens: 8192,
            max_response_body_bytes: 1024 * 1024,
            max_response_text_bytes: 128 * 1024,
            max_retries_per_request: 2,
            max_concurrent_calls: 4,
            provider_read_timeout: Duration::from_secs(60),
            provider_call_timeout: Duration::from_secs(180),
            max_requests_per_decision: 32,
        }
    }
}

/// A credential value. It is never serialized, never printed, and its `Debug`
/// is the redaction marker, so it cannot reach a log through a derived format.
#[derive(Clone, PartialEq, Eq)]
pub struct Secret(String);

impl Secret {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    /// The only way to read the value. Named so a reviewer sees every use.
    pub fn expose(&self) -> &str {
        &self.0
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl std::fmt::Debug for Secret {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("<redacted>")
    }
}

impl std::fmt::Display for Secret {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("<redacted>")
    }
}

/// One host-published model alias and everything the host, and only the host,
/// decides about it.
#[derive(Clone, Debug)]
pub struct ModelRoute {
    alias: String,
    destination: String,
    credential: Secret,
    rate_card: RateCard,
    max_output_tokens: u32,
    input_token_overhead: u64,
}

impl ModelRoute {
    /// Build a route. The rate card carries the provider, model and revision;
    /// they are not separately declarable, so a route cannot price one revision
    /// while calling another.
    ///
    /// `input_token_overhead` is the input the provider bills that never appears
    /// in the entrant's content: the adapter's system framing, the wire encoding
    /// of tool schemas, and whatever the provider counts before a token of the
    /// request is read. The host adds it to the content-byte bound when it
    /// reserves, so an empty request still reserves a nonzero amount. It is per
    /// route because it is a property of the provider's documented framing, and
    /// it is required because a zero would silently restore a reservation that
    /// bounds only part of the billed request.
    pub fn new(
        alias: impl Into<String>,
        destination: impl Into<String>,
        credential: Secret,
        rate_card: RateCard,
        max_output_tokens: u32,
        input_token_overhead: u64,
    ) -> Result<Self, String> {
        let alias = alias.into();
        let destination = destination.into();
        if alias.is_empty()
            || alias.len() > 128
            || !alias
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'.')
        {
            return Err("a model alias is 1..128 bytes of ASCII alphanumerics, '-' and '.'".into());
        }
        if destination.is_empty()
            || destination.len() > 1024
            || destination.contains(char::is_control)
        {
            return Err("a route destination is 1..1024 bytes without control characters".into());
        }
        if credential.is_empty() {
            return Err(format!("route {alias} has no credential"));
        }
        if max_output_tokens == 0 {
            return Err(format!("route {alias} allows no output tokens"));
        }
        if input_token_overhead == 0 {
            return Err(format!(
                "route {alias} declares no input token overhead; state the provider's framing overhead"
            ));
        }
        Ok(Self {
            alias,
            destination,
            credential,
            rate_card,
            max_output_tokens,
            input_token_overhead,
        })
    }

    pub fn alias(&self) -> &str {
        &self.alias
    }

    pub fn rate_card(&self) -> &RateCard {
        &self.rate_card
    }

    pub fn max_output_tokens(&self) -> u32 {
        self.max_output_tokens
    }

    pub fn input_token_overhead(&self) -> u64 {
        self.input_token_overhead
    }

    /// The identity fields, in fixed order, with the destination reduced to a
    /// digest and the credential absent entirely. The overhead is in here
    /// because it decides what the host authorizes per call: changing it must
    /// not resume an existing money journal.
    fn identity_row(&self) -> (String, String, String, String, String, String, u32, u64) {
        (
            self.alias.clone(),
            self.rate_card.provider().to_string(),
            self.rate_card.model().to_string(),
            self.rate_card.revision().to_string(),
            self.rate_card.digest(),
            sharpebench_attest::content_digest(self.destination.as_bytes()),
            self.max_output_tokens,
            self.input_token_overhead,
        )
    }
}

/// The frozen set of aliases an entrant may name. Resolution is exact: there is
/// no prefix pass-through and no fallback to an unversioned alias.
#[derive(Clone, Debug, Default)]
pub struct RouteTable {
    routes: Vec<ModelRoute>,
}

impl RouteTable {
    pub fn new(routes: Vec<ModelRoute>) -> Result<Self, String> {
        for (index, route) in routes.iter().enumerate() {
            if routes[..index]
                .iter()
                .any(|earlier| earlier.alias == route.alias)
            {
                return Err(format!("model alias {} is declared twice", route.alias));
            }
        }
        Ok(Self { routes })
    }

    pub fn resolve(&self, alias: &str) -> Option<&ModelRoute> {
        self.routes.iter().find(|route| route.alias == alias)
    }

    pub fn is_empty(&self) -> bool {
        self.routes.is_empty()
    }

    pub fn aliases(&self) -> Vec<&str> {
        self.routes
            .iter()
            .map(|route| route.alias.as_str())
            .collect()
    }

    /// The one rate card behind every route, when there is exactly one. A table
    /// spanning several cards has no single card to publish a total against.
    pub fn single_rate_card(&self) -> Option<&RateCard> {
        let first = self.routes.first()?.rate_card();
        self.routes
            .iter()
            .all(|route| route.rate_card() == first)
            .then_some(first)
    }

    /// Digest over model, revision, rate-card identity and destination for every
    /// alias. Fold this into the sweep's invocation digest: changing a model, a
    /// revision, a rate card or a destination then cannot resume an existing
    /// checkpoint or an existing money journal.
    pub fn identity_digest(&self) -> String {
        let rows: Vec<_> = self.routes.iter().map(ModelRoute::identity_row).collect();
        sharpebench_attest::content_digest(
            &serde_json::to_vec(&(GATEWAY_PROTOCOL, rows))
                .expect("a validated route table serializes"),
        )
    }

    /// Every credential value in the table, for redaction.
    fn secrets(&self) -> Vec<&str> {
        self.routes
            .iter()
            .map(|route| route.credential.expose())
            .collect()
    }
}

/// Replace credential material and obvious bearer-token shapes with a marker.
///
/// Two passes, because they catch different things: the exact values the host
/// holds (which is the only complete answer for this run) and a generic
/// token-shaped scrub (which still helps when a provider echoes a credential
/// the host did not configure, such as one a proxy added).
pub fn redact(text: &str, secrets: &[&str]) -> String {
    let mut out = text.to_string();
    for secret in secrets {
        if secret.len() >= 4 {
            out = out.replace(secret, "<redacted>");
        }
    }
    let mut scrubbed = String::with_capacity(out.len());
    for token in out.split_inclusive(|c: char| c.is_whitespace() || c == '"' || c == ',') {
        let (word, tail) = match token.char_indices().last() {
            Some((index, last)) if last.is_whitespace() || last == '"' || last == ',' => {
                (&token[..index], &token[index..])
            }
            _ => (token, ""),
        };
        let trimmed = word.trim_start_matches(['\'', '"', '(', '[', ':']);
        let looks_secret = trimmed.len() >= 16
            && (trimmed.starts_with("sk-")
                || trimmed.starts_with("Bearer")
                || trimmed.starts_with("bearer")
                || trimmed.starts_with("sk_")
                || trimmed.starts_with("api-key"));
        if looks_secret {
            scrubbed.push_str("<redacted>");
        } else {
            scrubbed.push_str(word);
        }
        scrubbed.push_str(tail);
    }
    scrubbed
}

/// Why the gateway refused or failed one entrant request. The vocabulary is
/// closed: an entrant learns the category, never a provider body, a
/// destination, a credential or another entrant's identifiers.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GatewayErrorKind {
    RequestTooLarge,
    MalformedRequest,
    ForbiddenField,
    UnknownProtocol,
    UnknownModelAlias,
    TooManyMessages,
    MessageTooLarge,
    TooManyTools,
    ToolPayloadTooLarge,
    OutputTokensOutOfRange,
    BudgetExhausted,
    CallLimitExhausted,
    ConcurrencyLimit,
    ShuttingDown,
    JournalUnwritable,
    JournalOwnershipLost,
    ProviderRateLimited,
    ProviderUnavailable,
    ProviderTimeout,
    ProviderResponseInvalid,
    DecisionRequestLimit,
    ResponseWithheld,
}

impl GatewayErrorKind {
    /// A short, fixed explanation. It contains no run-specific material, so it
    /// is safe in a log, in an error and in published evidence.
    pub fn detail(self) -> &'static str {
        match self {
            Self::RequestTooLarge => "the request line exceeded the accepted size",
            Self::MalformedRequest => "the request was not a valid gateway request",
            Self::ForbiddenField => "the request named a host-owned field",
            Self::UnknownProtocol => "the request declared an unsupported protocol",
            Self::UnknownModelAlias => "the model alias is not published by this host",
            Self::TooManyMessages => "the request carried too many messages",
            Self::MessageTooLarge => "a message exceeded the accepted size",
            Self::TooManyTools => "the request carried too many tools",
            Self::ToolPayloadTooLarge => "a tool payload exceeded the accepted size",
            Self::OutputTokensOutOfRange => "the requested output length is outside the bound",
            Self::BudgetExhausted => "the sweep money budget is exhausted",
            Self::CallLimitExhausted => "the sweep call budget is exhausted",
            Self::ConcurrencyLimit => "too many calls are already in flight",
            Self::ShuttingDown => "the gateway is shutting down",
            Self::JournalUnwritable => "the spend journal could not be made durable",
            Self::JournalOwnershipLost => "the spend journal is owned by another writer",
            Self::ProviderRateLimited => "the provider rejected the call",
            Self::ProviderUnavailable => "the provider could not be reached",
            Self::ProviderTimeout => "the provider did not answer within the bound",
            Self::ProviderResponseInvalid => "the provider answer could not be read",
            Self::DecisionRequestLimit => "the decision made too many gateway requests",
            Self::ResponseWithheld => "the answer carried host material and was withheld",
        }
    }
}

/// One validated message. Roles are a closed set; an entrant cannot invent one.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Message {
    pub role: MessageRole,
    pub content: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MessageRole {
    System,
    User,
    Assistant,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ToolDefinition {
    pub name: String,
    pub schema_json: String,
}

/// What an entrant may write. Every field here is content; none of it is
/// routing, addressing or identity.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GatewayRequest {
    pub protocol: String,
    pub model_alias: String,
    pub messages: Vec<Message>,
    pub max_output_tokens: u32,
    #[serde(default)]
    pub tools: Vec<ToolDefinition>,
}

impl GatewayRequest {
    /// Total content bytes. No tokenizer emits more tokens than the text has
    /// bytes, so this bounds the input tokens the *content* becomes. It does
    /// not bound the billed request: the adapter's system framing and wire
    /// encoding are outside it, and an empty request does not imply zero billed
    /// input. The route's `input_token_overhead` covers that part, and the
    /// reservation is the sum.
    fn content_bytes(&self) -> u64 {
        let messages: usize = self
            .messages
            .iter()
            .map(|message| message.content.len())
            .sum();
        let tools: usize = self
            .tools
            .iter()
            .map(|tool| tool.name.len() + tool.schema_json.len())
            .sum();
        (messages + tools) as u64
    }
}

/// What the host writes back. There is no provider identifier here: the ordinal
/// is host-assigned and local to this journal, so two entrants cannot discover a
/// shared handle through the gateway.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GatewayResponse {
    pub protocol: String,
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ordinal: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub finish_reason: Option<String>,
    /// Whether the provider reported usage for this call. False means the cost
    /// is unknown, never that it was zero.
    pub usage_observed: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<GatewayErrorBody>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GatewayErrorBody {
    pub kind: GatewayErrorKind,
    pub detail: String,
}

impl GatewayResponse {
    fn refused(kind: GatewayErrorKind) -> Self {
        Self {
            protocol: GATEWAY_PROTOCOL.to_string(),
            ok: false,
            ordinal: None,
            text: None,
            finish_reason: None,
            usage_observed: false,
            error: Some(GatewayErrorBody {
                kind,
                detail: kind.detail().to_string(),
            }),
        }
    }
}

/// The normalized provider answer the host is willing to read. A transport
/// adapter is responsible for putting a provider's own JSON into this shape;
/// anything else is a [`GatewayErrorKind::ProviderResponseInvalid`].
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProviderBody {
    pub text: String,
    #[serde(default)]
    pub finish_reason: Option<String>,
    /// Absent means the provider reported no usage. It never means zero.
    #[serde(default)]
    pub usage: Option<ProviderUsage>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProviderUsage {
    pub input_tokens: u64,
    pub output_tokens: u64,
}

/// What the host hands a transport.
///
/// The bounds travel with the call so the transport cannot pick its own, but
/// three of them are obligations the adapter must satisfy rather than
/// guarantees the broker makes:
///
/// - `read_timeout` bounds one socket read. The broker never sees a socket.
/// - `call_timeout` bounds the whole dispatch. The broker calls
///   [`ProviderTransport::call`] synchronously and cannot interrupt it; what it
///   does is measure the elapsed time and refuse an answer that arrived after
///   the deadline, charging the call as
///   [`crate::gateway_journal::UnknownCostReason::AdapterDeadlineExceeded`]. An
///   adapter that blocks forever blocks the broker with it.
/// - `max_response_body_bytes` bounds the body. The adapter allocates the
///   buffer, so only the adapter can bound the allocation; the broker checks the
///   length of what it is handed, which is after the fact.
pub struct ProviderCall<'a> {
    pub destination: &'a str,
    pub credential: &'a Secret,
    pub provider: &'a str,
    pub model: &'a str,
    pub revision: &'a str,
    pub messages: &'a [Message],
    pub tools: &'a [ToolDefinition],
    pub max_output_tokens: u32,
    pub read_timeout: Duration,
    pub call_timeout: Duration,
    pub max_response_body_bytes: usize,
}

/// A transport-level fault, in the two categories that decide who pays.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProviderFault {
    Unreachable,
    RateLimited,
    Timeout,
    Io,
}

/// What one dispatch produced.
pub enum ProviderOutcome {
    /// The provider answered with a status and a body.
    Answered { status: u16, body: Vec<u8> },
    /// The request provably did not reach work the provider could bill: the
    /// connection was refused, or the endpoint rejected it outright.
    RefusedBeforeWork(ProviderFault),
    /// The request was committed and the outcome is unknown: a read timeout, a
    /// dropped connection, a cancelled call. The provider may have billed it.
    AmbiguousAfterCommit(ProviderFault),
}

/// The seam between the host's accounting and the bytes on the wire. No
/// networked implementation ships in this crate.
///
/// # Adapter obligations
///
/// An implementation must honour `read_timeout`, `call_timeout` and
/// `max_response_body_bytes` from [`ProviderCall`]. The broker cannot enforce
/// them: it holds no socket, it calls this method synchronously with no way to
/// cancel it, and the response buffer is allocated here. What it does instead is
/// refuse to accept an answer that came back late, and refuse to read a body
/// larger than the bound after the adapter has already built it. An adapter that
/// ignores these bounds costs money and memory before the broker sees anything.
pub trait ProviderTransport {
    fn call(&mut self, call: ProviderCall<'_>) -> ProviderOutcome;
}

/// A shared ceiling on concurrent provider calls. Held by the host and shared
/// across every entrant's gateway, so the bound is on the host's fan-out rather
/// than on any one entrant's politeness.
#[derive(Debug)]
pub struct CallPermits {
    in_flight: AtomicU32,
    max: u32,
}

impl CallPermits {
    pub fn new(max: u32) -> Self {
        Self {
            in_flight: AtomicU32::new(0),
            max: max.max(1),
        }
    }

    pub fn in_flight(&self) -> u32 {
        self.in_flight.load(Ordering::SeqCst)
    }

    pub fn max(&self) -> u32 {
        self.max
    }

    pub fn try_acquire(&self) -> Option<CallPermit<'_>> {
        let mut current = self.in_flight.load(Ordering::SeqCst);
        loop {
            if current >= self.max {
                return None;
            }
            match self.in_flight.compare_exchange(
                current,
                current + 1,
                Ordering::SeqCst,
                Ordering::SeqCst,
            ) {
                Ok(_) => return Some(CallPermit { permits: self }),
                Err(observed) => current = observed,
            }
        }
    }
}

/// Releases its slot on drop, including on an unwind, so a panicking transport
/// cannot leak concurrency.
pub struct CallPermit<'a> {
    permits: &'a CallPermits,
}

impl Drop for CallPermit<'_> {
    fn drop(&mut self) {
        self.permits.in_flight.fetch_sub(1, Ordering::SeqCst);
    }
}

/// Cooperative shutdown. Once set, the gateway starts no further call; the one
/// call that may be in flight is bounded by `provider_call_timeout`.
#[derive(Clone, Debug, Default)]
pub struct GatewayShutdown(Arc<AtomicBool>);

impl GatewayShutdown {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn cancel(&self) {
        self.0.store(true, Ordering::SeqCst);
    }

    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::SeqCst)
    }
}

/// The host-side broker: one request line in, one response line out.
pub struct ModelGateway<'a, T: ProviderTransport> {
    routes: &'a RouteTable,
    permits: &'a CallPermits,
    transport: T,
    journal: GatewayJournal,
    journal_path: Option<PathBuf>,
    /// Exclusive ownership of `journal_path`, held for this gateway's whole
    /// life and released when it drops. `None` for an in-memory journal, which
    /// owns no path.
    journal_lock: Option<JournalLock>,
    limits: GatewayLimits,
    shutdown: GatewayShutdown,
    dispatches: u32,
    /// Latched when a save was refused because another writer owns the journal.
    /// The records this gateway appended are still in memory; what it may not
    /// do is keep spending against a file it no longer owns.
    journal_conflict: bool,
    /// Latched when a settlement could not be made durable. The file on disk
    /// then holds a reservation whose outcome is missing, and this gateway
    /// starts no further call: a settlement above its reservation that never
    /// landed would make a restart under-report real spend.
    ///
    /// The flag itself is in memory and dies with the process. What outlives it
    /// is the record that defines the condition, the reservation with no
    /// outcome, which [`ModelGateway::open`] refuses. Persisting the flag is not
    /// the alternative: it would have to be written to the journal that could
    /// not be written.
    journal_unwritable: bool,
}

impl<'a, T: ProviderTransport> ModelGateway<'a, T> {
    /// Open a gateway over an in-memory journal. Nothing is persisted; suitable
    /// for a single-process run whose spend record is published elsewhere.
    pub fn new(
        routes: &'a RouteTable,
        permits: &'a CallPermits,
        transport: T,
        budget: GatewayBudget,
        limits: GatewayLimits,
    ) -> Self {
        let identity = JournalIdentity::new(routes.identity_digest(), budget);
        Self {
            routes,
            permits,
            transport,
            journal: GatewayJournal::new(identity),
            journal_path: None,
            journal_lock: None,
            limits,
            shutdown: GatewayShutdown::new(),
            dispatches: 0,
            journal_conflict: false,
            journal_unwritable: false,
        }
    }

    /// Open a gateway over a persisted journal at `path`, resuming an existing
    /// one when it is bound to the same routes and budget. A resumed journal is
    /// folded, never repriced, and its records are never rewritten.
    ///
    /// Opening takes exclusive ownership of `path` through a [`JournalLock`]
    /// held for this gateway's lifetime. A second gateway over the same path,
    /// and a path whose earlier holder crashed without releasing it, are both
    /// refused: the error is [`std::io::ErrorKind::AlreadyExists`] carrying a
    /// [`crate::gateway_journal::JournalLockError`] as its source, naming the
    /// lock file.
    ///
    /// A journal holding a reservation with no settlement beside it is refused
    /// too, with [`std::io::ErrorKind::InvalidData`]. That record is what an
    /// earlier gateway leaves when a settlement could not be written, or when a
    /// process died mid-call; either way the file says less than was spent, and
    /// resuming it would re-reserve budget against a figure known to be low.
    pub fn open(
        routes: &'a RouteTable,
        permits: &'a CallPermits,
        transport: T,
        budget: GatewayBudget,
        limits: GatewayLimits,
        path: &Path,
    ) -> std::io::Result<Self> {
        // Ownership before reading: a snapshot taken without the lock could be
        // stale by the time the lock is held.
        let mut journal_lock = JournalLock::acquire(path)?;
        let identity = JournalIdentity::new(routes.identity_digest(), budget);
        let mut journal = match GatewayJournal::load_bound(path, &identity) {
            Ok(journal) => journal,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                GatewayJournal::new(identity)
            }
            Err(error) => return Err(error),
        };
        // A reservation the file holds with no outcome beside it is what a
        // settlement that never landed leaves behind. The in-memory latch that
        // stopped the gateway which produced it does not survive that process,
        // and it cannot be made durable by writing it: the condition is defined
        // by a write to this journal having failed. What is already durable is
        // the record itself, so that is what this reads, and a journal in that
        // state is refused rather than resumed with a clean slate. Resuming
        // charges the reservation, and a provider may price a call above what it
        // reserved, so the resumed gateway would spend against a figure it
        // cannot know is the real one.
        let unsettled = journal.spend().outstanding_calls;
        if unsettled > 0 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!(
                    "gateway journal at {} holds {unsettled} reservation(s) whose settlement never reached the file; \
                     real spend is at least what the record says and may be more, so it is not resumed. \
                     Read it with `sharpebench gateway --journal <path>` and run what is left under a fresh journal.",
                    path.display()
                ),
            ));
        }
        // A journal that does not exist yet names no document, so it is written
        // here rather than at the first call: until there is a document,
        // ownership is keyed on this path's spelling alone, which is what lets
        // a second name for one journal open it.
        journal_lock.bind_document(path, &mut journal)?;
        Ok(Self {
            routes,
            permits,
            transport,
            journal,
            journal_path: Some(path.to_path_buf()),
            journal_lock: Some(journal_lock),
            limits,
            shutdown: GatewayShutdown::new(),
            dispatches: 0,
            journal_conflict: false,
            journal_unwritable: false,
        })
    }

    pub fn shutdown_handle(&self) -> GatewayShutdown {
        self.shutdown.clone()
    }

    pub fn journal(&self) -> &GatewayJournal {
        &self.journal
    }

    pub fn into_journal(self) -> GatewayJournal {
        self.journal
    }

    /// Provider dispatches this process made, retries included. Compare against
    /// the journal's reservation count: they must agree.
    pub fn dispatches(&self) -> u32 {
        self.dispatches
    }

    /// Whether this gateway lost ownership of its journal file. Once true it
    /// starts no further call. Take [`ModelGateway::into_journal`] to recover
    /// the records it appended after the last write it owned.
    pub fn journal_conflict(&self) -> bool {
        self.journal_conflict
    }

    /// Whether a settlement this gateway made could not be written to its
    /// journal file. Once true it starts no further call, because the file no
    /// longer says what the last call cost and a restart would read the
    /// reservation instead. Take [`ModelGateway::into_journal`] to recover the
    /// settled records the file is missing.
    pub fn journal_unwritable(&self) -> bool {
        self.journal_unwritable
    }

    /// The lock file this gateway holds, when it owns a persisted journal.
    pub fn journal_lock_path(&self) -> Option<&Path> {
        self.journal_lock.as_ref().map(JournalLock::path)
    }

    /// Handle one request line and return one response line, newline excluded.
    /// Never panics and never returns more than `max_response_line_bytes`.
    pub fn serve_line(&mut self, line: &str) -> String {
        let response = self.serve(line);
        let mut encoded = serde_json::to_string(&response)
            .unwrap_or_else(|_| encode_refusal(GatewayErrorKind::MalformedRequest));
        if encoded.len() > self.limits.max_response_line_bytes {
            encoded = encode_refusal(GatewayErrorKind::ProviderResponseInvalid);
        }
        encoded
    }

    fn serve(&mut self, line: &str) -> GatewayResponse {
        let request = match self.parse(line) {
            Ok(request) => request,
            Err(kind) => return GatewayResponse::refused(kind),
        };
        // Resolution happens against the host's table; the request never named
        // anything but an alias.
        let Some(route) = self.routes.resolve(&request.model_alias) else {
            return GatewayResponse::refused(GatewayErrorKind::UnknownModelAlias);
        };
        if request.max_output_tokens == 0
            || request.max_output_tokens > self.limits.max_output_tokens
            || request.max_output_tokens > route.max_output_tokens
        {
            return GatewayResponse::refused(GatewayErrorKind::OutputTokensOutOfRange);
        }
        self.dispatch_with_retries(&request)
    }

    fn parse(&self, line: &str) -> Result<GatewayRequest, GatewayErrorKind> {
        // Size first: the bound must hold before anything is allocated from the
        // line, not after a parser has already built a tree from it.
        if line.len() > self.limits.max_request_line_bytes {
            return Err(GatewayErrorKind::RequestTooLarge);
        }
        let raw: serde_json::Value =
            serde_json::from_str(line).map_err(|_| GatewayErrorKind::MalformedRequest)?;
        let object = raw.as_object().ok_or(GatewayErrorKind::MalformedRequest)?;
        for name in FORBIDDEN_REQUEST_FIELDS {
            if object.contains_key(*name) {
                return Err(GatewayErrorKind::ForbiddenField);
            }
        }
        let request: GatewayRequest =
            serde_json::from_value(raw).map_err(|_| GatewayErrorKind::MalformedRequest)?;
        if request.protocol != GATEWAY_PROTOCOL {
            return Err(GatewayErrorKind::UnknownProtocol);
        }
        if request.messages.is_empty() || request.messages.len() > self.limits.max_messages {
            return Err(GatewayErrorKind::TooManyMessages);
        }
        if request
            .messages
            .iter()
            .any(|message| message.content.len() > self.limits.max_message_bytes)
        {
            return Err(GatewayErrorKind::MessageTooLarge);
        }
        if request.tools.len() > self.limits.max_tools {
            return Err(GatewayErrorKind::TooManyTools);
        }
        if request.tools.iter().any(|tool| {
            tool.name.len() + tool.schema_json.len() > self.limits.max_tool_payload_bytes
        }) {
            return Err(GatewayErrorKind::ToolPayloadTooLarge);
        }
        Ok(request)
    }

    fn dispatch_with_retries(&mut self, request: &GatewayRequest) -> GatewayResponse {
        let mut attempt = 0u32;
        loop {
            match self.dispatch_once(request) {
                Ok(response) => return response,
                Err(kind) => {
                    let retryable = matches!(
                        kind,
                        GatewayErrorKind::ProviderRateLimited
                            | GatewayErrorKind::ProviderUnavailable
                            | GatewayErrorKind::ProviderTimeout
                            | GatewayErrorKind::ProviderResponseInvalid
                    );
                    if !retryable || attempt >= self.limits.max_retries_per_request {
                        return GatewayResponse::refused(kind);
                    }
                    attempt += 1;
                }
            }
        }
    }

    /// One reservation, one dispatch, one settlement. Every path through this
    /// function that appends a reservation also appends exactly one settlement.
    fn dispatch_once(
        &mut self,
        request: &GatewayRequest,
    ) -> Result<GatewayResponse, GatewayErrorKind> {
        let route = self
            .routes
            .resolve(&request.model_alias)
            .ok_or(GatewayErrorKind::UnknownModelAlias)?;
        // Refusals that must happen before any money is reserved and before any
        // request starts. A hard budget refusal returns here: nothing is
        // appended, nothing is dispatched.
        if self.shutdown.is_cancelled() {
            return Err(GatewayErrorKind::ShuttingDown);
        }
        if self.journal_conflict {
            return Err(GatewayErrorKind::JournalOwnershipLost);
        }
        // A settlement that never reached disk leaves the file disagreeing with
        // what was actually spent. Starting another call would spend against a
        // record that is already wrong.
        if self.journal_unwritable {
            return Err(GatewayErrorKind::JournalUnwritable);
        }
        // An earlier call whose observed price landed above its reservation can
        // put committed money past the ceiling. That is recorded, not absorbed,
        // and it is not authority to start another call.
        if self.journal.ceiling_breached() {
            return Err(GatewayErrorKind::BudgetExhausted);
        }
        let spend = self.journal.spend();
        if spend.calls_started >= self.journal.identity.budget.max_calls {
            return Err(GatewayErrorKind::CallLimitExhausted);
        }
        // The reservation bounds the whole request the host is willing to
        // authorize: the content, bounded by its bytes, plus the framing the
        // provider bills that never appears in the content.
        let reserved_input_tokens = request
            .content_bytes()
            .saturating_add(route.input_token_overhead());
        let reserve = route
            .rate_card()
            .quote_nanos(reserved_input_tokens, u64::from(request.max_output_tokens))
            .ok_or(GatewayErrorKind::BudgetExhausted)?;
        if reserve > self.journal.available_usd_nanos() {
            return Err(GatewayErrorKind::BudgetExhausted);
        }
        let Some(permit) = self.permits.try_acquire() else {
            return Err(GatewayErrorKind::ConcurrencyLimit);
        };

        let ordinal = self
            .journal
            .reserve(route.alias(), route.rate_card(), reserve);
        // Durable before dispatch: a process that dies during the call leaves a
        // reservation behind, which folds as consumed rather than as free.
        if let Some(path) = &self.journal_path {
            if let Err(error) = self.journal.save(path) {
                // Nothing was dispatched, so the reservation is released. The
                // release stays in memory when the file is owned elsewhere:
                // writing it would replace a record this process never read.
                self.journal.settle(
                    ordinal,
                    Settlement::Released {
                        reason: ReleaseReason::NeverDispatched,
                    },
                );
                match error {
                    JournalSaveError::Conflict { .. } => {
                        self.journal_conflict = true;
                        return Err(GatewayErrorKind::JournalOwnershipLost);
                    }
                    // The reservation reached the file and its durability did
                    // not. The release above stays in memory, so the file holds
                    // a reservation whose outcome is missing: the same shape as
                    // a settlement that could not be written, and it latches for
                    // the same reason. It is this owner's I/O fault, and it is
                    // published as one rather than as another writer.
                    JournalSaveError::Unsynced(_) => {
                        self.journal_unwritable = true;
                        return Err(GatewayErrorKind::JournalUnwritable);
                    }
                    // Nothing landed: the file still holds exactly what it held,
                    // so this is refused per call without latching.
                    JournalSaveError::Io(_) => return Err(GatewayErrorKind::JournalUnwritable),
                }
            }
        }
        // Cancellation observed after the reservation and before the wire: the
        // reservation is released, because nothing was dispatched.
        if self.shutdown.is_cancelled() {
            self.settle_and_persist(
                ordinal,
                Settlement::Released {
                    reason: ReleaseReason::NeverDispatched,
                },
            );
            drop(permit);
            return Err(GatewayErrorKind::ShuttingDown);
        }

        self.dispatches = self.dispatches.saturating_add(1);
        let started = Instant::now();
        let outcome = self.transport.call(ProviderCall {
            destination: &route.destination,
            credential: &route.credential,
            provider: route.rate_card().provider(),
            model: route.rate_card().model(),
            revision: route.rate_card().revision(),
            messages: &request.messages,
            tools: &request.tools,
            max_output_tokens: request.max_output_tokens,
            read_timeout: self.limits.provider_read_timeout,
            call_timeout: self.limits.provider_call_timeout,
            max_response_body_bytes: self.limits.max_response_body_bytes,
        });
        let elapsed = started.elapsed();
        drop(permit);

        // The broker cannot interrupt a synchronous transport, but it does not
        // have to accept what one hands back after its deadline has passed. An
        // adapter that overran breached the bound it was given, so its answer is
        // refused and the call is charged: the request was on the wire, and a
        // free retry is exactly what a slow provider must not get.
        if elapsed > self.limits.provider_call_timeout {
            self.settle_and_persist(
                ordinal,
                Settlement::Unknown {
                    reason: UnknownCostReason::AdapterDeadlineExceeded,
                },
            );
            return Err(GatewayErrorKind::ProviderTimeout);
        }

        match outcome {
            ProviderOutcome::RefusedBeforeWork(fault) => {
                self.settle_and_persist(
                    ordinal,
                    Settlement::Released {
                        reason: ReleaseReason::ProviderRefusedBeforeWork,
                    },
                );
                Err(fault_kind(fault))
            }
            ProviderOutcome::AmbiguousAfterCommit(fault) => {
                self.settle_and_persist(
                    ordinal,
                    Settlement::Unknown {
                        reason: UnknownCostReason::AmbiguousAfterCommit,
                    },
                );
                Err(fault_kind(fault))
            }
            ProviderOutcome::Answered { status, body } => self.settle_answer(
                ordinal,
                route_index(self.routes, &request.model_alias),
                status,
                body,
            ),
        }
    }

    fn settle_answer(
        &mut self,
        ordinal: u32,
        route_index: usize,
        status: u16,
        body: Vec<u8>,
    ) -> Result<GatewayResponse, GatewayErrorKind> {
        // A rejection the provider states outright did no billable work; a
        // server-side failure might have. They settle differently on purpose.
        match status {
            200..=299 => {}
            429 => {
                self.settle_and_persist(
                    ordinal,
                    Settlement::Released {
                        reason: ReleaseReason::ProviderRefusedBeforeWork,
                    },
                );
                return Err(GatewayErrorKind::ProviderRateLimited);
            }
            400..=499 => {
                self.settle_and_persist(
                    ordinal,
                    Settlement::Released {
                        reason: ReleaseReason::ProviderRefusedBeforeWork,
                    },
                );
                return Err(GatewayErrorKind::ProviderUnavailable);
            }
            _ => {
                self.settle_and_persist(
                    ordinal,
                    Settlement::Unknown {
                        reason: UnknownCostReason::AmbiguousAfterCommit,
                    },
                );
                return Err(GatewayErrorKind::ProviderUnavailable);
            }
        }
        if body.len() > self.limits.max_response_body_bytes {
            self.settle_and_persist(
                ordinal,
                Settlement::Unknown {
                    reason: UnknownCostReason::AmbiguousAfterCommit,
                },
            );
            return Err(GatewayErrorKind::ProviderResponseInvalid);
        }
        let Ok(parsed) = serde_json::from_slice::<ProviderBody>(&body) else {
            // The call happened; only the answer is unreadable. Charging it as
            // unknown, not releasing it, is what stops an unparseable answer
            // from becoming a free retry.
            self.settle_and_persist(
                ordinal,
                Settlement::Unknown {
                    reason: UnknownCostReason::AmbiguousAfterCommit,
                },
            );
            return Err(GatewayErrorKind::ProviderResponseInvalid);
        };
        if parsed.text.len() > self.limits.max_response_text_bytes {
            self.settle_and_persist(
                ordinal,
                Settlement::Unknown {
                    reason: UnknownCostReason::AmbiguousAfterCommit,
                },
            );
            return Err(GatewayErrorKind::ProviderResponseInvalid);
        }
        let card = self.routes.routes[route_index].rate_card().clone();
        // An empty completion is still a completed call: it is reconciled and
        // charged like any other, never dropped as unspent work.
        let settlement = match parsed.usage {
            Some(usage) => match card.quote_nanos(usage.input_tokens, usage.output_tokens) {
                Some(usd_nanos) => Settlement::Priced {
                    input_tokens: usage.input_tokens,
                    output_tokens: usage.output_tokens,
                    usd_nanos: usd_nanos.to_string(),
                },
                None => Settlement::Unknown {
                    reason: UnknownCostReason::AmbiguousAfterCommit,
                },
            },
            None => Settlement::Unknown {
                reason: UnknownCostReason::UsageAbsent,
            },
        };
        let usage_observed = matches!(settlement, Settlement::Priced { .. });
        if !self.settle_and_persist(ordinal, settlement) {
            // The call happened and the money is spent; what failed is the
            // record of it. Handing back the completion as a success would let
            // the sweep carry on over a journal that no longer says what it
            // cost, so the answer is refused the same way a reservation that
            // could not be made durable is, and the flag says which.
            return Err(if self.journal_conflict {
                GatewayErrorKind::JournalOwnershipLost
            } else {
                GatewayErrorKind::JournalUnwritable
            });
        }
        Ok(GatewayResponse {
            protocol: GATEWAY_PROTOCOL.to_string(),
            ok: true,
            ordinal: Some(ordinal),
            text: Some(parsed.text),
            finish_reason: parsed.finish_reason,
            usage_observed,
            error: None,
        })
    }

    /// Settle, then persist, and report whether the settlement reached disk.
    ///
    /// A settlement that did not land leaves the file holding a reservation
    /// whose outcome is missing. That is not a safe direction to fail in: a
    /// provider may price a call above what it reserved, and this gateway
    /// records such a settlement at the observed amount, so a restart that
    /// folds the reservation instead under-reports real spend. Both failure
    /// modes therefore latch and stop the gateway: a refused write means
    /// another writer owns the file, an I/O failure means the record cannot be
    /// completed at all. The settled records stay in memory either way,
    /// reachable through [`ModelGateway::into_journal`].
    fn settle_and_persist(&mut self, ordinal: u32, settlement: Settlement) -> bool {
        self.journal.settle(ordinal, settlement);
        let Some(path) = &self.journal_path else {
            return true;
        };
        match self.journal.save(path) {
            Ok(()) => true,
            Err(JournalSaveError::Conflict { .. }) => {
                self.journal_conflict = true;
                false
            }
            Err(JournalSaveError::Unsynced(_) | JournalSaveError::Io(_)) => {
                self.journal_unwritable = true;
                false
            }
        }
    }

    /// Every credential the routes hold, for redacting an operator-facing string.
    pub fn redact_for_evidence(&self, text: &str) -> String {
        redact(text, &self.routes.secrets())
    }
}

fn route_index(routes: &RouteTable, alias: &str) -> usize {
    routes
        .routes
        .iter()
        .position(|route| route.alias == alias)
        .expect("the alias resolved moments ago")
}

fn fault_kind(fault: ProviderFault) -> GatewayErrorKind {
    match fault {
        ProviderFault::RateLimited => GatewayErrorKind::ProviderRateLimited,
        ProviderFault::Timeout => GatewayErrorKind::ProviderTimeout,
        ProviderFault::Unreachable | ProviderFault::Io => GatewayErrorKind::ProviderUnavailable,
    }
}

fn encode_refusal(kind: GatewayErrorKind) -> String {
    serde_json::to_string(&GatewayResponse::refused(kind))
        .expect("a fixed refusal vocabulary serializes")
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::AtomicUsize;

    use super::*;
    use crate::gateway_journal::{JournalLockError, JournalRecord};
    use crate::scratch::ScratchDir;

    const KEY: &str = "sk-live-test-do-not-log-0123456789";
    /// Framing tokens a test route declares the provider bills on every call,
    /// standing in for the overhead an operator would read off provider docs.
    /// A reservation in these tests is content bytes plus this plus the
    /// requested output, so `request("hello", 16)` reserves 5 + 8 + 16 = 29.
    const TEST_OVERHEAD: u64 = 8;

    fn card(input: u64, output: u64, revision: &str) -> RateCard {
        let json = format!(
            r#"{{"schema_version":"sharpebench.token-rate-card.v1","provider":"fake","model":"fake-1","revision":"{revision}","input_usd_nanos_per_token":{input},"output_usd_nanos_per_token":{output}}}"#
        );
        RateCard::from_json(json.as_bytes()).expect("a valid test rate card")
    }

    fn table(revision: &str) -> RouteTable {
        RouteTable::new(vec![ModelRoute::new(
            "fake.v1",
            "https://provider.invalid/v1/messages",
            Secret::new(KEY),
            card(1, 1, revision),
            4096,
            TEST_OVERHEAD,
        )
        .expect("a valid route")])
        .expect("a valid table")
    }

    fn budget(max_usd_nanos: u128, max_calls: u32) -> GatewayBudget {
        GatewayBudget {
            max_usd_nanos,
            max_calls,
        }
    }

    fn request(content: &str, max_output_tokens: u32) -> String {
        serde_json::to_string(&GatewayRequest {
            protocol: GATEWAY_PROTOCOL.to_string(),
            model_alias: "fake.v1".into(),
            messages: vec![Message {
                role: MessageRole::User,
                content: content.into(),
            }],
            max_output_tokens,
            tools: Vec::new(),
        })
        .expect("a request serializes")
    }

    fn body(text: &str, usage: Option<(u64, u64)>) -> Vec<u8> {
        serde_json::to_vec(&ProviderBody {
            text: text.into(),
            finish_reason: Some("stop".into()),
            usage: usage.map(|(input_tokens, output_tokens)| ProviderUsage {
                input_tokens,
                output_tokens,
            }),
        })
        .expect("a provider body serializes")
    }

    /// What one dispatch saw, so a test can assert on what the host handed the
    /// wire rather than only on what came back.
    #[derive(Clone, Debug, PartialEq, Eq)]
    struct Seen {
        destination: String,
        credential: String,
        provider: String,
        model: String,
        revision: String,
        max_output_tokens: u32,
        read_timeout: Duration,
        call_timeout: Duration,
        max_response_body_bytes: usize,
    }

    /// A scripted, hermetic provider. It never opens a socket: the only thing it
    /// can do is hand back an outcome from its script and record what it saw.
    struct FakeProvider {
        script: Vec<ProviderOutcome>,
        seen: Vec<Seen>,
        /// Read at dispatch time, so a test can assert the reservation was
        /// already durable when the call started.
        watch_journal: Option<PathBuf>,
        journal_at_dispatch: Vec<Option<String>>,
    }

    impl FakeProvider {
        fn new(script: Vec<ProviderOutcome>) -> Self {
            Self {
                script,
                seen: Vec::new(),
                watch_journal: None,
                journal_at_dispatch: Vec::new(),
            }
        }

        fn answering(count: usize) -> Self {
            Self::new(
                (0..count)
                    .map(|_| ProviderOutcome::Answered {
                        status: 200,
                        body: body("ok", Some((10, 5))),
                    })
                    .collect(),
            )
        }
    }

    impl ProviderTransport for FakeProvider {
        fn call(&mut self, call: ProviderCall<'_>) -> ProviderOutcome {
            self.seen.push(Seen {
                destination: call.destination.to_string(),
                credential: call.credential.expose().to_string(),
                provider: call.provider.to_string(),
                model: call.model.to_string(),
                revision: call.revision.to_string(),
                max_output_tokens: call.max_output_tokens,
                read_timeout: call.read_timeout,
                call_timeout: call.call_timeout,
                max_response_body_bytes: call.max_response_body_bytes,
            });
            if let Some(path) = &self.watch_journal {
                self.journal_at_dispatch
                    .push(std::fs::read_to_string(path).ok());
            }
            if self.script.is_empty() {
                ProviderOutcome::RefusedBeforeWork(ProviderFault::Unreachable)
            } else {
                self.script.remove(0)
            }
        }
    }

    /// A provider that makes the journal path unwritable while its call is in
    /// flight, so the failure lands at settlement and nowhere else: the
    /// reservation was already durable before the call started.
    ///
    /// The seam is the transport, not the persistence gate: nothing in the
    /// gateway is weakened to make this reachable. Putting a directory where
    /// the journal was is the portable way to make both the version read and
    /// the rename fail, on Windows and on Unix alike.
    struct SabotageJournal {
        path: PathBuf,
        /// What was at the journal path when it was taken away, so a test can
        /// put it back exactly and separate the latch from a fresh failure.
        removed: std::sync::Arc<std::sync::Mutex<Option<String>>>,
    }

    impl ProviderTransport for SabotageJournal {
        fn call(&mut self, _call: ProviderCall<'_>) -> ProviderOutcome {
            let mut removed = self.removed.lock().expect("the mutex holds");
            if removed.is_none() {
                *removed = Some(
                    std::fs::read_to_string(&self.path)
                        .expect("the reservation was on disk before the call started"),
                );
                std::fs::remove_file(&self.path).expect("the journal file goes");
                std::fs::create_dir(&self.path).expect("a directory where the journal was");
            }
            drop(removed);
            ProviderOutcome::Answered {
                status: 200,
                body: body("ok", Some((10, 5))),
            }
        }
    }

    fn temp_dir(tag: &str) -> ScratchDir {
        ScratchDir::new(&format!("gateway-{tag}"))
    }

    fn parse(line: &str) -> GatewayResponse {
        serde_json::from_str(line).expect("the gateway emits its own response schema")
    }

    fn settlements(journal: &GatewayJournal) -> Vec<Settlement> {
        journal
            .records()
            .iter()
            .filter_map(|record| match record {
                JournalRecord::Settled { settlement, .. } => Some(settlement.clone()),
                JournalRecord::Reserved { .. } => None,
            })
            .collect()
    }

    fn reservations(journal: &GatewayJournal) -> usize {
        journal
            .records()
            .iter()
            .filter(|record| matches!(record, JournalRecord::Reserved { .. }))
            .count()
    }

    /// The request schema has no field for a destination, a credential or an
    /// identifier a second entrant could also name, and one smuggled in is
    /// refused by name before anything is dispatched.
    #[test]
    fn an_entrant_cannot_supply_a_destination_a_credential_or_a_shared_identifier() {
        let routes = table("2026-01-01");
        let permits = CallPermits::new(4);
        for field in [
            "url",
            "endpoint",
            "base_url",
            "destination",
            "headers",
            "authorization",
            "api_key",
            "credential",
            "provider",
            "response_id",
            "session_id",
            "conversation_id",
        ] {
            let mut gateway = ModelGateway::new(
                &routes,
                &permits,
                FakeProvider::answering(1),
                budget(u128::MAX, 8),
                GatewayLimits::default(),
            );
            let mut value: serde_json::Value =
                serde_json::from_str(&request("hello", 16)).expect("json");
            value[field] = serde_json::Value::String("https://attacker.invalid".into());
            let response = parse(&gateway.serve_line(&value.to_string()));
            assert!(!response.ok, "{field} must be refused");
            assert_eq!(
                response.error.expect("an error body").kind,
                GatewayErrorKind::ForbiddenField,
                "{field} must be refused by name"
            );
            assert_eq!(gateway.dispatches(), 0, "{field} must dispatch nothing");
            assert_eq!(reservations(gateway.journal()), 0);
        }
    }

    /// Routing is exact against the host's published table. There is no prefix
    /// pass-through and no fallback to an unversioned alias.
    #[test]
    fn an_unpublished_alias_is_refused_and_nothing_is_dispatched() {
        let routes = table("2026-01-01");
        let permits = CallPermits::new(4);
        let mut gateway = ModelGateway::new(
            &routes,
            &permits,
            FakeProvider::answering(1),
            budget(u128::MAX, 8),
            GatewayLimits::default(),
        );
        for alias in ["fake", "fake.v2", "fake.v1.1", "FAKE.V1"] {
            let mut value: serde_json::Value =
                serde_json::from_str(&request("hello", 16)).expect("json");
            value["model_alias"] = serde_json::Value::String(alias.into());
            let response = parse(&gateway.serve_line(&value.to_string()));
            assert_eq!(
                response.error.expect("an error body").kind,
                GatewayErrorKind::UnknownModelAlias,
                "{alias} must not resolve"
            );
        }
        assert_eq!(gateway.dispatches(), 0);
    }

    /// The host owns the destination, the credential, the model revision and
    /// every bound the transport works under.
    #[test]
    fn the_host_supplies_the_destination_credential_revision_and_bounds() {
        let routes = table("2026-03-04");
        let permits = CallPermits::new(4);
        let limits = GatewayLimits::default();
        let mut gateway = ModelGateway::new(
            &routes,
            &permits,
            FakeProvider::answering(1),
            budget(u128::MAX, 8),
            limits,
        );
        let response = parse(&gateway.serve_line(&request("hello", 16)));
        assert!(response.ok, "{response:?}");
        let seen = std::mem::take(&mut gateway.transport.seen);
        assert_eq!(seen.len(), 1);
        assert_eq!(seen[0].destination, "https://provider.invalid/v1/messages");
        assert_eq!(seen[0].credential, KEY);
        assert_eq!(seen[0].provider, "fake");
        assert_eq!(seen[0].model, "fake-1");
        assert_eq!(seen[0].revision, "2026-03-04");
        assert_eq!(seen[0].read_timeout, limits.provider_read_timeout);
        assert_eq!(seen[0].call_timeout, limits.provider_call_timeout);
        assert_eq!(
            seen[0].max_response_body_bytes,
            limits.max_response_body_bytes
        );
    }

    /// Every wire bound is checked before the allocation it governs: the request
    /// line, the message count, one message, the tool count and one tool payload.
    #[test]
    fn the_request_envelope_is_bounded_before_it_is_parsed() {
        let routes = table("2026-01-01");
        let permits = CallPermits::new(4);
        let limits = GatewayLimits {
            max_request_line_bytes: 512,
            max_messages: 2,
            max_message_bytes: 64,
            max_tools: 1,
            max_tool_payload_bytes: 32,
            ..GatewayLimits::default()
        };
        let mut gateway = ModelGateway::new(
            &routes,
            &permits,
            FakeProvider::answering(8),
            budget(u128::MAX, 32),
            limits,
        );
        let oversized = parse(&gateway.serve_line(&request(&"a".repeat(600), 16)));
        assert_eq!(
            oversized.error.expect("an error").kind,
            GatewayErrorKind::RequestTooLarge
        );

        let message = Message {
            role: MessageRole::User,
            content: "a".into(),
        };
        let three = serde_json::to_string(&GatewayRequest {
            protocol: GATEWAY_PROTOCOL.into(),
            model_alias: "fake.v1".into(),
            messages: vec![message.clone(), message.clone(), message.clone()],
            max_output_tokens: 16,
            tools: Vec::new(),
        })
        .expect("json");
        assert_eq!(
            parse(&gateway.serve_line(&three))
                .error
                .expect("error")
                .kind,
            GatewayErrorKind::TooManyMessages
        );
        assert_eq!(
            parse(&gateway.serve_line(&request(&"a".repeat(65), 16)))
                .error
                .expect("error")
                .kind,
            GatewayErrorKind::MessageTooLarge
        );

        let tool = ToolDefinition {
            name: "t".into(),
            schema_json: "{}".into(),
        };
        let two_tools = serde_json::to_string(&GatewayRequest {
            protocol: GATEWAY_PROTOCOL.into(),
            model_alias: "fake.v1".into(),
            messages: vec![message.clone()],
            max_output_tokens: 16,
            tools: vec![tool.clone(), tool],
        })
        .expect("json");
        assert_eq!(
            parse(&gateway.serve_line(&two_tools))
                .error
                .expect("error")
                .kind,
            GatewayErrorKind::TooManyTools
        );
        let fat_tool = serde_json::to_string(&GatewayRequest {
            protocol: GATEWAY_PROTOCOL.into(),
            model_alias: "fake.v1".into(),
            messages: vec![message],
            max_output_tokens: 16,
            tools: vec![ToolDefinition {
                name: "t".into(),
                schema_json: "x".repeat(64),
            }],
        })
        .expect("json");
        assert_eq!(
            parse(&gateway.serve_line(&fat_tool))
                .error
                .expect("error")
                .kind,
            GatewayErrorKind::ToolPayloadTooLarge
        );
        assert_eq!(
            gateway.dispatches(),
            0,
            "no bounded refusal reaches the wire"
        );
    }

    /// The requested output length is bounded by the host limit and by the
    /// route, whichever is smaller, and zero is not a length.
    #[test]
    fn the_requested_output_length_is_bounded_by_host_and_route() {
        let routes = RouteTable::new(vec![ModelRoute::new(
            "fake.v1",
            "https://provider.invalid",
            Secret::new(KEY),
            card(1, 1, "2026-01-01"),
            64,
            TEST_OVERHEAD,
        )
        .expect("route")])
        .expect("table");
        let permits = CallPermits::new(4);
        let limits = GatewayLimits {
            max_output_tokens: 128,
            ..GatewayLimits::default()
        };
        let mut gateway = ModelGateway::new(
            &routes,
            &permits,
            FakeProvider::answering(4),
            budget(u128::MAX, 8),
            limits,
        );
        for tokens in [0, 65, 129, 1_000_000] {
            let response = parse(&gateway.serve_line(&request("hello", tokens)));
            assert_eq!(
                response.error.expect("error").kind,
                GatewayErrorKind::OutputTokensOutOfRange,
                "{tokens} output tokens must be refused"
            );
        }
        assert_eq!(gateway.dispatches(), 0);
        assert!(parse(&gateway.serve_line(&request("hello", 64))).ok);
    }

    /// A provider answer larger than the bound, or model text larger than the
    /// bound, is refused rather than allocated into the response line.
    #[test]
    fn oversized_and_unreadable_provider_answers_are_refused() {
        let routes = table("2026-01-01");
        let permits = CallPermits::new(4);
        let limits = GatewayLimits {
            max_response_body_bytes: 256,
            max_response_text_bytes: 32,
            max_retries_per_request: 0,
            ..GatewayLimits::default()
        };
        let script = vec![
            ProviderOutcome::Answered {
                status: 200,
                body: vec![b'x'; 512],
            },
            ProviderOutcome::Answered {
                status: 200,
                body: body(&"t".repeat(64), Some((1, 1))),
            },
            ProviderOutcome::Answered {
                status: 200,
                body: b"{\"text\": \"trunc".to_vec(),
            },
            ProviderOutcome::Answered {
                status: 200,
                body: b"{\"text\":\"ok\",\"id\":\"resp_shared\"}".to_vec(),
            },
        ];
        let mut gateway = ModelGateway::new(
            &routes,
            &permits,
            FakeProvider::new(script),
            budget(u128::MAX, 32),
            limits,
        );
        for _ in 0..4 {
            let response = parse(&gateway.serve_line(&request("hello", 16)));
            assert_eq!(
                response.error.expect("error").kind,
                GatewayErrorKind::ProviderResponseInvalid
            );
        }
        // Four dispatches, four reservations, four settlements: an unreadable
        // answer is still a reconciled attempt.
        assert_eq!(gateway.dispatches(), 4);
        assert_eq!(reservations(gateway.journal()), 4);
        assert_eq!(settlements(gateway.journal()).len(), 4);
        assert!(settlements(gateway.journal())
            .iter()
            .all(|settlement| matches!(settlement, Settlement::Unknown { .. })));
    }

    /// Concurrency is a host ceiling on the whole fan-out, held outside any one
    /// entrant's gateway, and it is released even when a transport panics.
    #[test]
    fn concurrent_calls_are_bounded_by_the_shared_permit_pool() {
        let permits = CallPermits::new(2);
        let first = permits.try_acquire().expect("first permit");
        let second = permits.try_acquire().expect("second permit");
        assert_eq!(permits.in_flight(), 2);
        assert!(permits.try_acquire().is_none(), "the pool is a hard bound");
        drop(first);
        assert!(permits.try_acquire().is_some());
        drop(second);

        let saturated = CallPermits::new(1);
        let _held = saturated.try_acquire().expect("held elsewhere");
        let routes = table("2026-01-01");
        let mut gateway = ModelGateway::new(
            &routes,
            &saturated,
            FakeProvider::answering(1),
            budget(u128::MAX, 8),
            GatewayLimits::default(),
        );
        let response = parse(&gateway.serve_line(&request("hello", 16)));
        assert_eq!(
            response.error.expect("error").kind,
            GatewayErrorKind::ConcurrencyLimit
        );
        assert_eq!(gateway.dispatches(), 0);
        // A reservation taken before the permit check would leak money; the
        // permit is acquired first, so nothing was reserved.
        assert_eq!(reservations(gateway.journal()), 0);
    }

    /// The reservation is on disk before the call it pays for is dispatched. If
    /// the process dies during the call, the money is already accounted for.
    #[test]
    fn a_reservation_is_durable_before_the_call_is_dispatched() {
        let dir = temp_dir("durable");
        let path = dir.join("journal.json");
        let routes = table("2026-01-01");
        let permits = CallPermits::new(4);
        let mut transport = FakeProvider::answering(1);
        transport.watch_journal = Some(path.clone());
        let mut gateway = ModelGateway::open(
            &routes,
            &permits,
            transport,
            budget(u128::MAX, 8),
            GatewayLimits::default(),
            &path,
        )
        .expect("open");
        assert!(parse(&gateway.serve_line(&request("hello", 16))).ok);
        let at_dispatch = gateway.transport.journal_at_dispatch.remove(0);
        let at_dispatch = at_dispatch.expect("the journal existed when the call started");
        let value: serde_json::Value = serde_json::from_str(&at_dispatch).expect("json");
        let records = value["records"].as_array().expect("records");
        assert_eq!(
            records.len(),
            1,
            "exactly the reservation, not a settlement"
        );
        assert_eq!(records[0]["record"], "reserved");
        assert_eq!(records[0]["revision"], "2026-01-01");
    }

    /// A hard budget refusal starts no request. Both ceilings, money and calls,
    /// are checked before a reservation is taken and before anything dispatches.
    #[test]
    fn a_hard_budget_refusal_starts_no_request() {
        let routes = table("2026-01-01");
        let permits = CallPermits::new(4);
        // One nanodollar cannot cover a request whose reservation is at least
        // the content bytes plus the requested output tokens.
        let mut broke = ModelGateway::new(
            &routes,
            &permits,
            FakeProvider::answering(4),
            budget(1, 8),
            GatewayLimits::default(),
        );
        let response = parse(&broke.serve_line(&request("hello", 16)));
        assert_eq!(
            response.error.expect("error").kind,
            GatewayErrorKind::BudgetExhausted
        );
        assert_eq!(
            broke.dispatches(),
            0,
            "no request may start after a refusal"
        );
        assert_eq!(reservations(broke.journal()), 0);

        let mut counted = ModelGateway::new(
            &routes,
            &permits,
            FakeProvider::answering(4),
            budget(u128::MAX, 2),
            GatewayLimits::default(),
        );
        assert!(parse(&counted.serve_line(&request("hello", 16))).ok);
        assert!(parse(&counted.serve_line(&request("hello", 16))).ok);
        let refused = parse(&counted.serve_line(&request("hello", 16)));
        assert_eq!(
            refused.error.expect("error").kind,
            GatewayErrorKind::CallLimitExhausted
        );
        assert_eq!(counted.dispatches(), 2, "the third call never started");
        assert_eq!(reservations(counted.journal()), 2);
    }

    /// Money spent earlier in the sweep is still spent: the second call is
    /// refused because the first one consumed the budget, not because of a
    /// per-call bound.
    #[test]
    fn spend_accumulates_across_calls_until_the_budget_refuses() {
        let routes = table("2026-01-01");
        let permits = CallPermits::new(4);
        // "hello" is five content bytes and sixteen output tokens at one
        // nanodollar each: a reservation of twenty one.
        let mut gateway = ModelGateway::new(
            &routes,
            &permits,
            FakeProvider::answering(4),
            budget(30, 8),
            GatewayLimits::default(),
        );
        assert!(parse(&gateway.serve_line(&request("hello", 16))).ok);
        let refused = parse(&gateway.serve_line(&request("hello", 16)));
        assert_eq!(
            refused.error.expect("error").kind,
            GatewayErrorKind::BudgetExhausted
        );
        assert_eq!(gateway.dispatches(), 1);
        // Ten input plus five output tokens at one nanodollar each.
        assert_eq!(gateway.journal().spend().priced_usd_nanos, 15);
    }

    /// Every dispatch, retries included, is reserved and settled exactly once.
    #[test]
    fn every_retry_is_reserved_and_reconciled_exactly_once() {
        let routes = table("2026-01-01");
        let permits = CallPermits::new(4);
        let script = vec![
            ProviderOutcome::Answered {
                status: 429,
                body: Vec::new(),
            },
            ProviderOutcome::AmbiguousAfterCommit(ProviderFault::Timeout),
            ProviderOutcome::Answered {
                status: 200,
                body: body("ok", Some((10, 5))),
            },
        ];
        let mut gateway = ModelGateway::new(
            &routes,
            &permits,
            FakeProvider::new(script),
            budget(u128::MAX, 32),
            GatewayLimits {
                max_retries_per_request: 2,
                ..GatewayLimits::default()
            },
        );
        let response = parse(&gateway.serve_line(&request("hello", 16)));
        assert!(response.ok, "the third attempt recovered: {response:?}");
        assert_eq!(gateway.dispatches(), 3);
        assert_eq!(
            reservations(gateway.journal()),
            3,
            "one reservation per dispatch, retries included"
        );
        let settled = settlements(gateway.journal());
        assert_eq!(settled.len(), 3, "every attempt is reconciled");
        assert!(matches!(settled[0], Settlement::Released { .. }));
        assert!(matches!(settled[1], Settlement::Unknown { .. }));
        assert!(matches!(settled[2], Settlement::Priced { .. }));
        assert_eq!(gateway.journal().spend().calls_started, 3);
    }

    /// An empty completion is a completed call. It is reconciled and charged
    /// like any other, never dropped as unspent work.
    #[test]
    fn an_empty_completion_is_reconciled_like_any_other_call() {
        let routes = table("2026-01-01");
        let permits = CallPermits::new(4);
        let mut gateway = ModelGateway::new(
            &routes,
            &permits,
            FakeProvider::new(vec![ProviderOutcome::Answered {
                status: 200,
                body: body("", Some((10, 0))),
            }]),
            budget(u128::MAX, 8),
            GatewayLimits::default(),
        );
        let response = parse(&gateway.serve_line(&request("hello", 16)));
        assert!(response.ok);
        assert_eq!(response.text.as_deref(), Some(""));
        assert!(response.usage_observed);
        assert_eq!(gateway.journal().spend().priced_usd_nanos, 10);
        assert_eq!(settlements(gateway.journal()).len(), 1);
    }

    /// A provider that reports no usage leaves an unknown cost, not a zero, and
    /// the response says the usage was not observed.
    #[test]
    fn absent_usage_is_an_unknown_cost_not_a_zero() {
        let routes = table("2026-01-01");
        let permits = CallPermits::new(4);
        let mut gateway = ModelGateway::new(
            &routes,
            &permits,
            FakeProvider::new(vec![ProviderOutcome::Answered {
                status: 200,
                body: body("ok", None),
            }]),
            budget(u128::MAX, 8),
            GatewayLimits::default(),
        );
        let response = parse(&gateway.serve_line(&request("hello", 16)));
        assert!(response.ok);
        assert!(
            !response.usage_observed,
            "the entrant is told the usage was not observed"
        );
        let spend = gateway.journal().spend();
        assert_eq!(spend.priced_usd_nanos, 0);
        assert_eq!(spend.unknown_usd_nanos, 29, "charged at its reservation");
        assert!(spend.is_partial());
        assert!(matches!(
            settlements(gateway.journal())[0],
            Settlement::Unknown {
                reason: UnknownCostReason::UsageAbsent
            }
        ));
    }

    /// An ambiguous post-commit failure is neither refunded nor free: the money
    /// stays committed and the retry pays for itself.
    #[test]
    fn an_ambiguous_post_commit_failure_is_charged_and_the_retry_pays_again() {
        let routes = table("2026-01-01");
        let permits = CallPermits::new(4);
        let mut gateway = ModelGateway::new(
            &routes,
            &permits,
            FakeProvider::new(vec![
                ProviderOutcome::AmbiguousAfterCommit(ProviderFault::Timeout),
                ProviderOutcome::Answered {
                    status: 200,
                    body: body("ok", Some((10, 5))),
                },
            ]),
            budget(u128::MAX, 8),
            GatewayLimits::default(),
        );
        assert!(parse(&gateway.serve_line(&request("hello", 16))).ok);
        let spend = gateway.journal().spend();
        assert_eq!(spend.unknown_usd_nanos, 29, "the timed out call is charged");
        assert_eq!(spend.priced_usd_nanos, 15, "the retry is charged too");
        assert_eq!(spend.calls_started, 2);
        assert!(spend.is_partial(), "an unknown amount keeps it partial");
    }

    /// A provider rejection that provably did no billable work releases its
    /// reservation, and still consumes one of the sweep's calls.
    #[test]
    fn a_rejection_before_work_releases_money_but_consumes_a_call() {
        let routes = table("2026-01-01");
        let permits = CallPermits::new(4);
        let mut gateway = ModelGateway::new(
            &routes,
            &permits,
            FakeProvider::new(vec![
                ProviderOutcome::Answered {
                    status: 429,
                    body: Vec::new(),
                },
                ProviderOutcome::RefusedBeforeWork(ProviderFault::Unreachable),
            ]),
            budget(u128::MAX, 8),
            GatewayLimits {
                max_retries_per_request: 1,
                ..GatewayLimits::default()
            },
        );
        let response = parse(&gateway.serve_line(&request("hello", 16)));
        assert_eq!(
            response.error.expect("error").kind,
            GatewayErrorKind::ProviderUnavailable
        );
        let spend = gateway.journal().spend();
        assert_eq!(spend.committed_usd_nanos(), 0);
        assert_eq!(spend.calls_started, 2);
        assert_eq!(spend.released_calls, 2);

        // A rate-limit rejection and any other client-side rejection release
        // the same way, so the money assertions above hold with the 429 arm
        // deleted and the entrant told only that the provider was unreachable.
        // One 429, no retry, so the kind the entrant reads is the 429's own.
        let mut limited = ModelGateway::new(
            &routes,
            &permits,
            FakeProvider::new(vec![ProviderOutcome::Answered {
                status: 429,
                body: Vec::new(),
            }]),
            budget(u128::MAX, 8),
            GatewayLimits {
                max_retries_per_request: 0,
                ..GatewayLimits::default()
            },
        );
        let refused = parse(&limited.serve_line(&request("hello", 16)));
        assert_eq!(
            refused.error.expect("error").kind,
            GatewayErrorKind::ProviderRateLimited
        );
        let spend = limited.journal().spend();
        assert_eq!(spend.released_calls, 1);
        assert_eq!(spend.committed_usd_nanos(), 0);
    }

    /// A server-side failure might have done billable work, so it settles as
    /// unknown rather than being released like an outright rejection.
    #[test]
    fn a_server_failure_settles_unknown_while_a_rejection_releases() {
        let routes = table("2026-01-01");
        let permits = CallPermits::new(4);
        let mut gateway = ModelGateway::new(
            &routes,
            &permits,
            FakeProvider::new(vec![ProviderOutcome::Answered {
                status: 503,
                body: Vec::new(),
            }]),
            budget(u128::MAX, 8),
            GatewayLimits {
                max_retries_per_request: 0,
                ..GatewayLimits::default()
            },
        );
        assert!(!parse(&gateway.serve_line(&request("hello", 16))).ok);
        assert!(matches!(
            settlements(gateway.journal())[0],
            Settlement::Unknown {
                reason: UnknownCostReason::AmbiguousAfterCommit
            }
        ));
        assert_eq!(gateway.journal().spend().unknown_usd_nanos, 29);
    }

    /// Resuming continues the same journal: earlier spend still counts against
    /// the budget, and no amount is recomputed.
    #[test]
    fn a_resumed_gateway_keeps_earlier_spend_and_reprices_nothing() {
        let dir = temp_dir("resume");
        let path = dir.join("journal.json");
        let routes = table("2026-01-01");
        let permits = CallPermits::new(4);
        let limits = GatewayLimits::default();
        {
            let mut gateway = ModelGateway::open(
                &routes,
                &permits,
                FakeProvider::answering(1),
                budget(u128::MAX, 8),
                limits,
                &path,
            )
            .expect("open");
            assert!(parse(&gateway.serve_line(&request("hello", 16))).ok);
        }
        let mut resumed = ModelGateway::open(
            &routes,
            &permits,
            FakeProvider::answering(1),
            budget(u128::MAX, 8),
            limits,
            &path,
        )
        .expect("resume");
        assert_eq!(
            resumed.journal().spend().priced_usd_nanos,
            15,
            "the recorded amount survives the resume"
        );
        assert_eq!(resumed.journal().spend().calls_started, 1);
        assert_eq!(resumed.dispatches(), 0, "a resume redoes no call");
        assert!(parse(&resumed.serve_line(&request("hello", 16))).ok);
        assert_eq!(
            resumed.journal().spend().priced_usd_nanos,
            30,
            "the second call adds to the first, it does not replace it"
        );
        assert_eq!(reservations(resumed.journal()), 2);
    }

    /// Model, revision and rate-card identity are frozen into the gateway's
    /// identity, so any of them changing invalidates both the checkpoint
    /// invocation digest and the money journal.
    #[test]
    fn model_revision_and_rate_card_identity_are_frozen() {
        let base = table("2026-01-01").identity_digest();
        assert_ne!(
            base,
            table("2026-06-01").identity_digest(),
            "a revision change is an identity change"
        );
        let repriced = RouteTable::new(vec![ModelRoute::new(
            "fake.v1",
            "https://provider.invalid/v1/messages",
            Secret::new(KEY),
            card(2, 2, "2026-01-01"),
            4096,
            TEST_OVERHEAD,
        )
        .expect("route")])
        .expect("table");
        assert_ne!(base, repriced.identity_digest(), "a rate change is one too");
        let rerouted = RouteTable::new(vec![ModelRoute::new(
            "fake.v1",
            "https://elsewhere.invalid/v1/messages",
            Secret::new(KEY),
            card(1, 1, "2026-01-01"),
            4096,
            TEST_OVERHEAD,
        )
        .expect("route")])
        .expect("table");
        assert_ne!(base, rerouted.identity_digest(), "so is a destination");
        // Rotating the credential is not an experiment change and must not
        // invalidate a resumable sweep.
        let rotated = RouteTable::new(vec![ModelRoute::new(
            "fake.v1",
            "https://provider.invalid/v1/messages",
            Secret::new("sk-live-rotated-9876543210abcdef"),
            card(1, 1, "2026-01-01"),
            4096,
            TEST_OVERHEAD,
        )
        .expect("route")])
        .expect("table");
        assert_eq!(base, rotated.identity_digest());
    }

    /// A journal written under one model cannot be resumed under another: the
    /// spend record belongs to the experiment that produced it.
    #[test]
    fn a_journal_cannot_be_resumed_under_a_different_model_revision() {
        let dir = temp_dir("rebind");
        let path = dir.join("journal.json");
        let permits = CallPermits::new(4);
        let first = table("2026-01-01");
        {
            let mut gateway = ModelGateway::open(
                &first,
                &permits,
                FakeProvider::answering(1),
                budget(u128::MAX, 8),
                GatewayLimits::default(),
                &path,
            )
            .expect("open");
            assert!(parse(&gateway.serve_line(&request("hello", 16))).ok);
        }
        let second = table("2026-06-01");
        let error = ModelGateway::open(
            &second,
            &permits,
            FakeProvider::answering(1),
            budget(u128::MAX, 8),
            GatewayLimits::default(),
            &path,
        )
        .err()
        .expect("a rebound journal is refused");
        assert!(
            error.to_string().contains("different route table"),
            "{error}"
        );
    }

    /// Credentials never reach an error, a log or published evidence, whether
    /// they came from the host's own table or from a provider echoing one back.
    #[test]
    fn credentials_and_request_bodies_are_redacted() {
        let routes = table("2026-01-01");
        let permits = CallPermits::new(4);
        let gateway = ModelGateway::new(
            &routes,
            &permits,
            FakeProvider::answering(0),
            budget(u128::MAX, 8),
            GatewayLimits::default(),
        );
        let leak = format!(
            "provider said: 401 for Authorization: Bearer {KEY} (retry with api-key sk_other_credential_value)"
        );
        let clean = gateway.redact_for_evidence(&leak);
        assert!(!clean.contains(KEY), "{clean}");
        assert!(!clean.contains("sk_other_credential_value"), "{clean}");
        assert!(clean.contains("<redacted>"));
        // `KEY` is bearer-shaped, so the generic scrub removes it whether or not
        // the host's own table is consulted: the assertions above hold with the
        // exact-value pass deleted. A credential that does not look like a token
        // is what isolates that pass, and the control below is the same text
        // under a gateway that does not hold it, which the generic scrub leaves
        // alone.
        const OPAQUE: &str = "opaque-host-credential-0123456789";
        let opaque_routes = RouteTable::new(vec![ModelRoute::new(
            "fake.v1",
            "https://provider.invalid/v1/messages",
            Secret::new(OPAQUE),
            card(1, 1, "2026-01-01"),
            4096,
            TEST_OVERHEAD,
        )
        .expect("a valid route")])
        .expect("a valid table");
        let opaque_gateway = ModelGateway::new(
            &opaque_routes,
            &permits,
            FakeProvider::answering(0),
            budget(u128::MAX, 8),
            GatewayLimits::default(),
        );
        let opaque_leak = format!("provider said: 401 for {OPAQUE}");
        let cleaned = opaque_gateway.redact_for_evidence(&opaque_leak);
        assert!(!cleaned.contains(OPAQUE), "{cleaned}");
        assert!(cleaned.contains("<redacted>"), "{cleaned}");
        assert_eq!(
            gateway.redact_for_evidence(&opaque_leak),
            opaque_leak,
            "nothing but the host's own table can remove an opaque credential"
        );

        // The Secret type cannot leak through a derived format either.
        assert_eq!(format!("{:?}", Secret::new(KEY)), "<redacted>");
        assert_eq!(format!("{}", Secret::new(KEY)), "<redacted>");
        assert!(!format!("{:?}", routes.resolve("fake.v1").expect("route")).contains(KEY));
    }

    /// Errors handed to an entrant carry a fixed vocabulary: no provider body,
    /// no destination, no credential and no identifier from another entrant.
    #[test]
    fn an_error_to_the_entrant_carries_no_provider_material() {
        let routes = table("2026-01-01");
        let permits = CallPermits::new(4);
        let mut gateway = ModelGateway::new(
            &routes,
            &permits,
            FakeProvider::new(vec![ProviderOutcome::Answered {
                status: 500,
                body: format!("{{\"error\":\"bad key {KEY}\",\"trace\":\"provider.invalid\"}}")
                    .into_bytes(),
            }]),
            budget(u128::MAX, 8),
            GatewayLimits {
                max_retries_per_request: 0,
                ..GatewayLimits::default()
            },
        );
        let line = gateway.serve_line(&request("hello", 16));
        assert!(!line.contains(KEY), "{line}");
        assert!(!line.contains("provider.invalid"), "{line}");
        assert!(!line.contains("bad key"), "{line}");
        let response = parse(&line);
        let error = response.error.expect("an error body");
        assert_eq!(error.kind, GatewayErrorKind::ProviderUnavailable);
        // The literal, not `GatewayErrorKind::detail()`: comparing the field
        // against the function that filled it is the same value on both sides,
        // so every explanation could go empty and this would still hold. The
        // entrant reads this text and nothing else about the failure.
        assert_eq!(error.detail, "the provider could not be reached");
    }

    /// The answer an entrant sees carries no provider identifier, so two
    /// entrants cannot discover a shared handle through the gateway.
    #[test]
    fn the_response_carries_no_shared_provider_identifier() {
        let routes = table("2026-01-01");
        let permits = CallPermits::new(4);
        let mut gateway = ModelGateway::new(
            &routes,
            &permits,
            FakeProvider::new(vec![ProviderOutcome::Answered {
                status: 200,
                body: body("ok", Some((10, 5))),
            }]),
            budget(u128::MAX, 8),
            GatewayLimits::default(),
        );
        let line = gateway.serve_line(&request("hello", 16));
        let value: serde_json::Value = serde_json::from_str(&line).expect("json");
        let keys: Vec<&str> = value
            .as_object()
            .expect("object")
            .keys()
            .map(String::as_str)
            .collect();
        for key in &keys {
            assert!(
                [
                    "protocol",
                    "ok",
                    "ordinal",
                    "text",
                    "finish_reason",
                    "usage_observed",
                ]
                .contains(key),
                "unexpected response key {key}"
            );
        }
        assert_eq!(
            value["ordinal"], 0,
            "the ordinal is host-assigned and local"
        );
    }

    /// A response line never exceeds the bound, whatever a provider returns.
    #[test]
    fn a_response_line_is_bounded() {
        let routes = table("2026-01-01");
        let permits = CallPermits::new(4);
        let limits = GatewayLimits {
            max_response_line_bytes: 256,
            max_response_text_bytes: 1024,
            max_retries_per_request: 0,
            ..GatewayLimits::default()
        };
        let mut gateway = ModelGateway::new(
            &routes,
            &permits,
            FakeProvider::new(vec![ProviderOutcome::Answered {
                status: 200,
                body: body(&"t".repeat(1000), Some((1, 1))),
            }]),
            budget(u128::MAX, 8),
            limits,
        );
        let line = gateway.serve_line(&request("hello", 16));
        assert!(
            line.len() <= limits.max_response_line_bytes,
            "{}",
            line.len()
        );
        assert!(!parse(&line).ok);
    }

    /// Cancellation stops new calls and releases the reservation of a call that
    /// was never dispatched. Nothing is left outstanding after a shutdown.
    #[test]
    fn cancellation_starts_no_new_call_and_leaves_nothing_outstanding() {
        let routes = table("2026-01-01");
        let permits = CallPermits::new(4);
        let mut gateway = ModelGateway::new(
            &routes,
            &permits,
            FakeProvider::answering(4),
            budget(u128::MAX, 8),
            GatewayLimits::default(),
        );
        assert!(parse(&gateway.serve_line(&request("hello", 16))).ok);
        let shutdown = gateway.shutdown_handle();
        shutdown.cancel();
        let response = parse(&gateway.serve_line(&request("hello", 16)));
        assert_eq!(
            response.error.expect("error").kind,
            GatewayErrorKind::ShuttingDown
        );
        assert_eq!(gateway.dispatches(), 1, "no call starts after cancellation");
        let spend = gateway.journal().spend();
        assert_eq!(
            spend.outstanding_usd_nanos, 0,
            "a shutdown leaves no unsettled reservation"
        );
        assert_eq!(permits.in_flight(), 0, "no permit is leaked");
    }

    /// Host-observed spend is published beside the scored pool, never inside it.
    /// Driving calls through the gateway leaves the submission byte-identical.
    #[test]
    fn host_observed_spend_is_rank_neutral() {
        let routes = table("2026-01-01");
        let permits = CallPermits::new(4);
        let mut gateway = ModelGateway::new(
            &routes,
            &permits,
            FakeProvider::answering(2),
            budget(u128::MAX, 8),
            GatewayLimits::default(),
        );
        assert!(parse(&gateway.serve_line(&request("hello", 16))).ok);
        assert!(parse(&gateway.serve_line(&request("hello", 16))).ok);

        let submission = sharpebench_core::AgentSubmission {
            agent_id: "entrant".into(),
            runs: vec![crate::failing_sentinel_run(4)],
            in_sample_trials: 0,
            candidates: Vec::new(),
        };
        let before = serde_json::to_string(&submission).expect("json");
        let resilient = crate::ResilientSubmission {
            submission,
            failures: crate::FailureLog::default(),
            attempts: crate::AttemptSummary::default(),
            monetary_cost: gateway
                .journal()
                .monetary_summary(routes.single_rate_card()),
        };
        assert_eq!(
            serde_json::to_string(&resilient.submission).expect("json"),
            before,
            "spend sits beside the scored pool, not in it"
        );
        match resilient.monetary_cost {
            crate::accounting::MonetarySummary::Estimated {
                usage_source,
                usd_nanos,
                ..
            } => {
                assert_eq!(usage_source, "host_observed");
                assert_eq!(usd_nanos, "30");
            }
            other => panic!("expected a complete host-observed total, got {other:?}"),
        }
    }

    /// A route table is validated when it is built: duplicate aliases, empty
    /// credentials and unusable aliases are refused before any run starts.
    #[test]
    fn a_route_table_is_validated_when_it_is_built() {
        assert!(ModelRoute::new(
            "fake v1",
            "https://provider.invalid",
            Secret::new(KEY),
            card(1, 1, "2026-01-01"),
            16,
            TEST_OVERHEAD,
        )
        .is_err());
        assert!(ModelRoute::new(
            "fake.v1",
            "https://provider.invalid",
            Secret::new(""),
            card(1, 1, "2026-01-01"),
            16,
            TEST_OVERHEAD,
        )
        .is_err());
        assert!(ModelRoute::new(
            "fake.v1",
            "",
            Secret::new(KEY),
            card(1, 1, "2026-01-01"),
            16,
            TEST_OVERHEAD,
        )
        .is_err());
        let duplicate = RouteTable::new(vec![
            ModelRoute::new(
                "fake.v1",
                "https://a.invalid",
                Secret::new(KEY),
                card(1, 1, "2026-01-01"),
                16,
                TEST_OVERHEAD,
            )
            .expect("route"),
            ModelRoute::new(
                "fake.v1",
                "https://b.invalid",
                Secret::new(KEY),
                card(1, 1, "2026-01-01"),
                16,
                TEST_OVERHEAD,
            )
            .expect("route"),
        ]);
        assert!(duplicate.is_err());
    }

    /// A transport that ignores the call deadline it was handed. It opens no
    /// socket: it sleeps, then answers from memory.
    struct SlowProvider {
        delay: Duration,
    }

    impl ProviderTransport for SlowProvider {
        fn call(&mut self, _call: ProviderCall<'_>) -> ProviderOutcome {
            std::thread::sleep(self.delay);
            ProviderOutcome::Answered {
                status: 200,
                body: body("ok", Some((10, 5))),
            }
        }
    }

    /// The reservation bounds the whole request the host authorizes, not the
    /// part of it the entrant wrote. Empty content is not zero billed input:
    /// the provider still frames the call, so a budget that cannot cover the
    /// framing refuses before anything reaches the wire.
    #[test]
    fn a_reservation_covers_framing_the_entrant_never_wrote() {
        let routes = table("2026-01-01");
        let permits = CallPermits::new(4);
        let mut gateway = ModelGateway::new(
            &routes,
            &permits,
            FakeProvider::new(vec![ProviderOutcome::Answered {
                status: 200,
                body: body("ok", Some((10, 1))),
            }]),
            budget(1, 4),
            GatewayLimits::default(),
        );
        let empty = serde_json::to_string(&GatewayRequest {
            protocol: GATEWAY_PROTOCOL.into(),
            model_alias: "fake.v1".into(),
            messages: vec![Message {
                role: MessageRole::User,
                content: String::new(),
            }],
            max_output_tokens: 1,
            tools: Vec::new(),
        })
        .expect("json");

        let response = parse(&gateway.serve_line(&empty));
        assert!(
            !response.ok,
            "a one-unit budget cannot authorize a framed call"
        );
        assert_eq!(
            response.error.expect("error").kind,
            GatewayErrorKind::BudgetExhausted
        );
        assert_eq!(gateway.dispatches(), 0, "no request starts");
        assert_eq!(reservations(gateway.journal()), 0, "nothing is appended");
        assert_eq!(gateway.journal().spend().committed_usd_nanos(), 0);
    }

    /// A reservation bounds what this host authorizes, not what a provider may
    /// bill. When the observed price lands above it, the gap is recorded rather
    /// than absorbed, and the sweep stops instead of spending past the ceiling.
    #[test]
    fn usage_above_the_reservation_is_recorded_and_stops_the_sweep() {
        // A priced route and a route the host prices at nothing, sharing one
        // budget. The free route is what separates a breached ceiling from a
        // merely exhausted one: it reserves zero, so only the ceiling stops it.
        let routes = RouteTable::new(vec![
            ModelRoute::new(
                "fake.v1",
                "https://provider.invalid/v1/messages",
                Secret::new(KEY),
                card(1, 1, "2026-01-01"),
                4096,
                TEST_OVERHEAD,
            )
            .expect("route"),
            ModelRoute::new(
                "free.v1",
                "https://provider.invalid/v1/free",
                Secret::new(KEY),
                card(0, 0, "2026-01-01"),
                4096,
                TEST_OVERHEAD,
            )
            .expect("route"),
        ])
        .expect("table");
        let permits = CallPermits::new(4);
        let mut gateway = ModelGateway::new(
            &routes,
            &permits,
            FakeProvider::new(vec![
                ProviderOutcome::Answered {
                    status: 200,
                    body: body("ok", Some((100, 0))),
                },
                ProviderOutcome::Answered {
                    status: 200,
                    body: body("ok", Some((1, 1))),
                },
            ]),
            budget(40, 4),
            GatewayLimits::default(),
        );
        assert!(parse(&gateway.serve_line(&request("hello", 16))).ok);

        let spend = gateway.journal().spend();
        assert_eq!(
            spend.priced_usd_nanos, 100,
            "the observed price is recorded, never clipped to the reservation"
        );
        assert_eq!(
            spend.overspent_usd_nanos, 71,
            "the gap above the 29 reserved is what the host did not authorize"
        );
        assert_eq!(spend.overspent_calls, 1);
        assert!(gateway.journal().ceiling_breached());

        let paid = parse(&gateway.serve_line(&request("hello", 16)));
        assert_eq!(
            paid.error.expect("error").kind,
            GatewayErrorKind::BudgetExhausted
        );
        let free = serde_json::to_string(&GatewayRequest {
            protocol: GATEWAY_PROTOCOL.into(),
            model_alias: "free.v1".into(),
            messages: vec![Message {
                role: MessageRole::User,
                content: "hello".into(),
            }],
            max_output_tokens: 16,
            tools: Vec::new(),
        })
        .expect("json");
        assert_eq!(
            parse(&gateway.serve_line(&free)).error.expect("error").kind,
            GatewayErrorKind::BudgetExhausted,
            "a breached ceiling stops even a call that would reserve nothing"
        );
        assert_eq!(
            gateway.dispatches(),
            1,
            "a breached ceiling starts no further call"
        );
    }

    /// Two gateways over one journal path are not a shared budget. The second
    /// never opens: the first holds the path for its lifetime, and the refusal
    /// is typed and names the lock file rather than being a bare failure.
    #[test]
    fn a_second_gateway_cannot_open_the_journal_the_first_owns() {
        let dir = temp_dir("lockopen");
        let path = dir.join("journal.json");
        let routes = table("2026-01-01");
        let permits = CallPermits::new(4);
        let budget = budget(u128::MAX, 4);
        let first = ModelGateway::open(
            &routes,
            &permits,
            FakeProvider::answering(1),
            budget,
            GatewayLimits::default(),
            &path,
        )
        .expect("open");
        let lock = first
            .journal_lock_path()
            .expect("a persisted gateway owns a lock")
            .to_path_buf();

        let refused = ModelGateway::open(
            &routes,
            &permits,
            FakeProvider::answering(1),
            budget,
            GatewayLimits::default(),
            &path,
        )
        .err()
        .expect("a second gateway over one journal is refused");
        assert_eq!(refused.kind(), std::io::ErrorKind::AlreadyExists);
        assert!(
            matches!(
                refused
                    .get_ref()
                    .and_then(|source| source.downcast_ref::<JournalLockError>()),
                Some(JournalLockError::Held { .. })
            ),
            "the refusal is typed, not a bare failure: {refused}"
        );
        assert!(
            refused.to_string().contains(&lock.display().to_string()),
            "the refusal names the lock file: {refused}"
        );

        // Released on drop, so a sequential second gateway opens normally.
        drop(first);
        assert!(!lock.exists(), "the lock is released with its gateway");
        ModelGateway::open(
            &routes,
            &permits,
            FakeProvider::answering(1),
            budget,
            GatewayLimits::default(),
            &path,
        )
        .expect("the released path opens again");
    }

    /// Many gateways racing for one journal path: the file system admits
    /// exactly one, every other attempt is refused by type, and the record the
    /// admitted writer made is on disk whole. The barrier is what makes the
    /// race real; the assertion holds whichever thread wins it.
    #[test]
    fn only_one_of_many_racing_gateways_owns_the_journal() {
        const WRITERS: usize = 8;
        let dir = temp_dir("race");
        let path = dir.join("journal.json");
        let routes = table("2026-01-01");
        let permits = CallPermits::new(WRITERS as u32);
        let budget = budget(u128::MAX, 8);
        let start = std::sync::Barrier::new(WRITERS);
        let admitted = AtomicUsize::new(0);
        let refused = std::sync::Mutex::new(Vec::new());

        std::thread::scope(|scope| {
            for _ in 0..WRITERS {
                scope.spawn(|| {
                    start.wait();
                    match ModelGateway::open(
                        &routes,
                        &permits,
                        FakeProvider::answering(1),
                        budget,
                        GatewayLimits::default(),
                        &path,
                    ) {
                        Ok(mut gateway) => {
                            admitted.fetch_add(1, Ordering::SeqCst);
                            assert!(
                                parse(&gateway.serve_line(&request("hello", 16))).ok,
                                "the admitted writer spends"
                            );
                        }
                        Err(error) => refused.lock().expect("the mutex holds").push(error),
                    }
                });
            }
        });

        assert_eq!(
            admitted.load(Ordering::SeqCst),
            1,
            "one writer is admitted to one budget, not several"
        );
        let refused = refused.into_inner().expect("the mutex holds");
        assert_eq!(refused.len(), WRITERS - 1);
        for error in &refused {
            assert_eq!(error.kind(), std::io::ErrorKind::AlreadyExists);
            assert!(
                matches!(
                    error
                        .get_ref()
                        .and_then(|source| source.downcast_ref::<JournalLockError>()),
                    Some(JournalLockError::Held { .. })
                ),
                "every refusal is typed: {error}"
            );
        }

        let identity = JournalIdentity::new(routes.identity_digest(), budget);
        let on_disk = GatewayJournal::load_bound(&path, &identity).expect("the journal survives");
        assert_eq!(
            on_disk.spend().calls_started,
            1,
            "the admitted call is recorded and nothing replaced it"
        );
        assert_eq!(settlements(&on_disk).len(), 1, "no record is lost");
        assert_eq!(on_disk.spend().priced_usd_nanos, 15);
    }

    /// A settlement that cannot be written is not a lost log line. The file
    /// keeps the reservation, and a reservation is not what the call cost: this
    /// gateway records observed usage above a reservation, so a restart that
    /// folds the reservation instead under-reports real spend. The gateway
    /// therefore fails closed, and says so.
    #[test]
    fn a_settlement_that_cannot_be_persisted_stops_the_gateway() {
        let dir = temp_dir("unwritable");
        let path = dir.join("journal.json");
        let routes = table("2026-01-01");
        let permits = CallPermits::new(4);
        let removed = std::sync::Arc::new(std::sync::Mutex::new(None));
        let mut gateway = ModelGateway::open(
            &routes,
            &permits,
            SabotageJournal {
                path: path.clone(),
                removed: std::sync::Arc::clone(&removed),
            },
            budget(u128::MAX, 8),
            GatewayLimits::default(),
            &path,
        )
        .expect("open");

        let response = parse(&gateway.serve_line(&request("hello", 16)));
        assert!(
            !response.ok,
            "an answer whose cost was not recorded is not handed back as a success"
        );
        assert_eq!(
            response.error.expect("error").kind,
            GatewayErrorKind::JournalUnwritable
        );
        assert!(gateway.journal_unwritable(), "the failure latches");
        assert!(
            !gateway.journal_conflict(),
            "an I/O failure is not another writer"
        );
        assert_eq!(
            gateway.journal().spend().priced_usd_nanos,
            15,
            "the settlement the file is missing is still in memory"
        );

        // Put back exactly what was taken away. The path is writable again, so
        // what refuses the next call is the latch and nothing else.
        std::fs::remove_dir(&path).expect("the directory goes");
        std::fs::write(
            &path,
            removed
                .lock()
                .expect("the mutex holds")
                .as_ref()
                .expect("the journal was read before it was taken away"),
        )
        .expect("the journal is back, byte for byte");

        let next = parse(&gateway.serve_line(&request("hello", 16)));
        assert_eq!(
            next.error.expect("error").kind,
            GatewayErrorKind::JournalUnwritable,
            "a gateway whose record is incomplete starts no further call, even once the file is writable again"
        );
        assert_eq!(
            gateway.dispatches(),
            1,
            "the refusal happens before anything is dispatched"
        );
        drop(gateway);
    }

    /// The latch that stops a gateway whose settlement never landed is a flag in
    /// memory, and the process it belongs to is what a restart replaces. What
    /// cannot be restarted away is the record: the file holds the reservation
    /// and no outcome beside it. A reopen that read that and carried on would
    /// re-reserve budget against a total the file is known to understate, since
    /// the settlement that went missing may have priced above its reservation.
    /// So the reopen is refused.
    ///
    /// The refusal is not asserted on the latch, which is gone: the on-disk
    /// shape it leaves is checked here independently, one reservation and no
    /// settlement, before the reopen is attempted.
    ///
    /// Three causes could refuse a reopen of this path. The lock is ruled out by
    /// the first gateway being dropped and its lock file gone. A binding
    /// mismatch is ruled out by the same routes and budget opening the same file
    /// once the missing settlement is appended, at the end. A malformed document
    /// is ruled out by the journal loading and folding here.
    #[test]
    fn a_journal_whose_settlement_never_landed_is_not_reopened_with_a_clean_slate() {
        let dir = temp_dir("unwritablereopen");
        let path = dir.join("journal.json");
        let routes = table("2026-01-01");
        let permits = CallPermits::new(4);
        let budget = budget(u128::MAX, 8);
        let removed = std::sync::Arc::new(std::sync::Mutex::new(None));
        let mut gateway = ModelGateway::open(
            &routes,
            &permits,
            SabotageJournal {
                path: path.clone(),
                removed: std::sync::Arc::clone(&removed),
            },
            budget,
            GatewayLimits::default(),
            &path,
        )
        .expect("open");
        let response = parse(&gateway.serve_line(&request("hello", 16)));
        assert_eq!(
            response.error.expect("error").kind,
            GatewayErrorKind::JournalUnwritable
        );
        assert!(
            gateway.journal_unwritable(),
            "the condition arose in memory"
        );

        // Put back exactly what the call took away, which is the file as it
        // stood when the settlement failed to reach it.
        std::fs::remove_dir(&path).expect("the directory goes");
        std::fs::write(
            &path,
            removed
                .lock()
                .expect("the mutex holds")
                .as_ref()
                .expect("the journal was read before it was taken away"),
        )
        .expect("the journal is back, byte for byte");
        drop(gateway);
        assert!(
            !JournalLock::lock_path(&path).expect("a lock path").exists(),
            "the lock is released, so nothing but the record can refuse the reopen"
        );

        let identity = JournalIdentity::new(routes.identity_digest(), budget);
        let mut stranded = GatewayJournal::load_bound(&path, &identity).expect("the record loads");
        assert_eq!(
            stranded.records().len(),
            1,
            "the file holds the reservation and nothing else"
        );
        assert!(
            matches!(stranded.records()[0], JournalRecord::Reserved { .. }),
            "and what it holds is a reservation with no outcome"
        );

        let refused = ModelGateway::open(
            &routes,
            &permits,
            FakeProvider::answering(1),
            budget,
            GatewayLimits::default(),
            &path,
        )
        .err()
        .expect("a journal missing a settlement is not resumed");
        assert_eq!(refused.kind(), std::io::ErrorKind::InvalidData);
        assert!(
            refused.to_string().contains(&path.display().to_string()),
            "the refusal names the journal: {refused}"
        );
        assert!(
            !JournalLock::lock_path(&path).expect("a lock path").exists(),
            "a gateway that is refused owns nothing"
        );

        // And the refusal is the missing outcome, not the path: once the
        // reservation has one, the same routes and budget open the same file.
        stranded.settle(
            0,
            Settlement::Unknown {
                reason: UnknownCostReason::UsageAbsent,
            },
        );
        stranded.save(&path).expect("the record is completed");
        ModelGateway::open(
            &routes,
            &permits,
            FakeProvider::answering(1),
            budget,
            GatewayLimits::default(),
            &path,
        )
        .expect("a complete record resumes");
    }

    /// A7. An I/O fault by the sole owner is published as an I/O fault. A
    /// reservation whose rename landed and whose durability did not leaves the
    /// file holding a call the gateway will not settle, which is the
    /// `journal_unwritable` condition; what it is not is another writer, and
    /// the sweep publishes `journal_ownership_lost` beside the pool.
    ///
    /// Three causes could leave `journal_conflict` false here: the save not
    /// failing at all, the save failing before the rename, and the repair. The
    /// first is ruled out by the refusal itself, the second by the file being a
    /// version ahead of where the last whole save left it, and the third is
    /// what the second request pins: a gateway a version behind its own file is
    /// refused as another writer on its next save.
    #[test]
    fn a_reservation_whose_durability_is_unconfirmed_is_not_published_as_lost_ownership() {
        let dir = temp_dir("unsynced");
        let path = dir.join("journal.json");
        let routes = table("2026-01-01");
        let permits = CallPermits::new(4);
        let mut gateway = ModelGateway::open(
            &routes,
            &permits,
            FakeProvider::answering(4),
            budget(u128::MAX, 8),
            GatewayLimits::default(),
            &path,
        )
        .expect("open");
        let whole = GatewayJournal::version_on_disk(&path).expect("the journal is there");

        crate::gateway_journal::fault_injection::fail_next_parent_sync();
        let refused = parse(&gateway.serve_line(&request("hello", 16)));
        assert_eq!(
            refused.error.expect("error").kind,
            GatewayErrorKind::JournalUnwritable
        );
        assert_eq!(
            gateway.dispatches(),
            0,
            "the reservation could not be made durable, so nothing was dispatched"
        );
        assert!(gateway.journal_unwritable(), "the fault latches");
        assert!(
            !gateway.journal_conflict(),
            "the sole owner's I/O fault is not lost ownership"
        );
        assert_eq!(
            GatewayJournal::version_on_disk(&path).expect("readable"),
            whole.map(|version| version + 1),
            "the rename landed, so the file holds the reservation"
        );

        let next = parse(&gateway.serve_line(&request("hello", 16)));
        assert_eq!(
            next.error.expect("error").kind,
            GatewayErrorKind::JournalUnwritable,
            "and the next refusal names the same cause rather than another writer"
        );
        assert!(!gateway.journal_conflict());
    }

    /// The compare-and-swap is the second line of defence, for a journal that
    /// moved under a single writer: a file restored from a backup, or edited by
    /// hand, while one gateway holds the lock. That gateway is refused and its
    /// records stay in memory. The unlocked second gateway here stands in for
    /// whatever moved the file; the lock is what stops a real second gateway.
    #[test]
    fn a_second_gateway_cannot_spend_the_journal_the_first_owns() {
        let dir = temp_dir("ownership");
        let path = dir.join("journal.json");
        let routes = table("2026-01-01");
        let permits = CallPermits::new(4);
        let budget = budget(u128::MAX, 1);
        let mut first = ModelGateway::open(
            &routes,
            &permits,
            FakeProvider::answering(1),
            budget,
            GatewayLimits::default(),
            &path,
        )
        .expect("open");
        let mut second = ModelGateway {
            routes: &routes,
            permits: &permits,
            transport: FakeProvider::answering(1),
            journal: GatewayJournal::new(JournalIdentity::new(routes.identity_digest(), budget)),
            journal_path: Some(path.clone()),
            journal_lock: None,
            limits: GatewayLimits::default(),
            shutdown: GatewayShutdown::new(),
            dispatches: 0,
            journal_conflict: false,
            journal_unwritable: false,
        };

        assert!(parse(&first.serve_line(&request("hello", 16))).ok);
        let refused = parse(&second.serve_line(&request("hello", 16)));
        assert!(!refused.ok, "the second gateway does not own the journal");
        assert_eq!(
            refused.error.expect("error").kind,
            GatewayErrorKind::JournalOwnershipLost
        );
        assert_eq!(second.dispatches(), 0, "the refused gateway starts no call");
        assert!(second.journal_conflict(), "the refusal latches");
        assert_eq!(
            settlements(second.journal()).len(),
            1,
            "the refused reservation is released in memory, not lost"
        );

        let identity = JournalIdentity::new(routes.identity_digest(), budget);
        let on_disk = GatewayJournal::load_bound(&path, &identity).expect("the journal survives");
        assert_eq!(
            on_disk.spend().calls_started,
            1,
            "the record on disk is the one that was written, not a stale replacement"
        );
        assert_eq!(on_disk.spend().priced_usd_nanos, 15);
    }

    /// The broker cannot interrupt a synchronous transport, but it does not
    /// accept an answer that came back after the deadline it handed out. The
    /// call is charged, because the request was on the wire.
    #[test]
    fn an_answer_returned_after_the_deadline_is_refused_and_charged() {
        let routes = table("2026-01-01");
        let permits = CallPermits::new(4);
        let mut gateway = ModelGateway::new(
            &routes,
            &permits,
            SlowProvider {
                delay: Duration::from_millis(30),
            },
            budget(u128::MAX, 4),
            GatewayLimits {
                provider_call_timeout: Duration::from_millis(1),
                max_retries_per_request: 0,
                ..GatewayLimits::default()
            },
        );

        let response = parse(&gateway.serve_line(&request("hello", 16)));
        assert!(!response.ok, "an over-deadline answer is not a success");
        assert_eq!(
            response.error.expect("error").kind,
            GatewayErrorKind::ProviderTimeout
        );
        assert_eq!(gateway.dispatches(), 1);
        assert!(matches!(
            settlements(gateway.journal())[0],
            Settlement::Unknown {
                reason: UnknownCostReason::AdapterDeadlineExceeded
            }
        ));
        assert_eq!(
            gateway.journal().spend().unknown_usd_nanos,
            29,
            "the breached call is charged at its reservation"
        );
    }
}
