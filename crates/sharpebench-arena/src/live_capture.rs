//! Live forward capture: a certifying route for an entrant the arena cannot
//! run again.
//!
//! Re-execution certifies only an entrant whose decisions repeat when it runs a
//! second time with no network. A model-backed entrant does not repeat: its
//! decisions depend on sampled model output reached through the host's model
//! gateway. Live capture certifies it on custody instead. During the forward
//! window the host runs the committed image (digest-pinned, under the existing
//! sandbox and gateway) as each bar arrives, and appends to a per-entrant log:
//!
//! - for every bar of every run, the digest of the observation the entrant was
//!   given ([`observation_sha256`]) and the decision it returned, and
//! - every model call the gateway dispatched for it: request and response
//!   digests, provider, model id and revision, reported token usage and timing.
//!
//! Each entry carries the digest of the one before it, starting from the
//! digest of the log header, so the log is one hash chain. When the window
//! closes the host seals the chain's root with the arena's signing key
//! ([`seal_live_log`]). The entrant reveals the sealed log as its entry
//! ([`crate::RevealedEntry::live_log`]), and [`certify_live_log`] admits it only
//! when all of the following hold:
//!
//! 1. the seal verifies and names this log's header, entry count and root;
//! 2. the header names this window, this agent, the committed image digest,
//!    the window's scorer artifact and the window's execution matrix;
//! 3. the chain is unbroken (no entry altered, dropped, inserted or moved);
//! 4. every bar of every run is complete and in order: an observation, any
//!    model calls, then exactly one decision, with no bar missing;
//! 5. the host clock is consistent: no observation is stamped before its bar's
//!    day began, no decision after the next bar's day began, and no run's clock
//!    runs backwards, each within [`MAX_CLOCK_SKEW_MS`];
//! 6. replaying the logged decisions over the revealed data produces, at every
//!    bar, an observation whose digest is the logged one.
//!
//! The arena then derives the ranked returns from the logged decisions by
//! strict replay, as it does for a capture, and the row is
//! [`crate::ReturnsProvenance::LiveCaptured`]. The window records each admitted
//! log's root ([`LiveCaptureRoot`]) and the signed header carries the roots.
//!
//! What this certifies: that the committed image was what ran, that it was told
//! exactly the point-in-time observations the revealed data produces, that its
//! decisions were recorded bar by bar under the host clock before the next bar,
//! and which model calls it made. What it does not certify: that a provider
//! served the model snapshot the route names (the host knows what it asked
//! for, not what answered), that the host clock is true time, or that the host
//! did not hold the data early. The timestamps and the seal are the operator's
//! statement, under the same custody the arena already relies on for its data.

use std::collections::BTreeMap;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};

use serde::{Deserialize, Serialize};

use sharpebench_attest::{
    content_digest, publish_public_chain, verify_public_chain, PublicChain, SigningKey,
};
use sharpebench_core::ScoreConfig;
use sharpebench_harness::gateway::{
    Message, ProviderBody, ProviderCall, ProviderOutcome, ProviderTransport, ToolDefinition,
};
use sharpebench_harness::{verify_trajectory_reexecuted, ReexecutionError};
use sharpebench_protocol::{
    AgentTrajectory, Decision, DecisionStep, MarketObservation, RunTrajectory,
};
use sharpebench_sim::{Agent, HoldAgent};

use crate::intake::ReplayWindow;

/// Schema of a live log header.
pub const LIVE_LOG_SCHEMA: &str = "sharpebench-arena-live-log/v1";

/// Kind of the payload a live log seal signs.
pub const LIVE_SEAL_KIND: &str = "sharpebench-arena-live-seal";

/// Schema of a window that records a live capture root (see
/// [`crate::WindowState::live_capture`]). A binary that predates live capture
/// knows no such version and refuses the record instead of loading it and
/// writing it back without the roots, which its signed header names.
pub const LIVE_CAPTURE_WINDOW_SCHEMA_VERSION: u32 = 8;

/// How far the host clock may disagree with a bar's calendar day, and run
/// backwards within one run, before a live log is refused.
pub const MAX_CLOCK_SKEW_MS: u64 = 120_000;

const DAY_MS: u64 = 86_400_000;

/// One market window of the execution matrix a live log runs.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LiveWindow {
    pub start: usize,
    pub end: usize,
}

/// The first line of a live log, fixed before the first bar.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LiveLogHeader {
    /// Always [`LIVE_LOG_SCHEMA`].
    pub schema: String,
    pub window_id: String,
    pub agent_id: String,
    /// The committed image, `<repository>@sha256:<digest>`.
    pub image: String,
    /// The scorer artifact whose engine produced the observations.
    pub runner_artifact_sha256: String,
    /// The execution matrix: windows in order, then the seeds every window
    /// runs. Runs are window-major, as in a capture. The matrix depends on the
    /// revealed dataset's length, so a live window fixes its bar count before
    /// it opens.
    pub windows: Vec<LiveWindow>,
    pub seeds: Vec<u64>,
}

/// What the gateway's provider did with one model call.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ModelCallOutcome {
    Answered,
    RefusedBeforeWork,
    AmbiguousAfterCommit,
}

/// One model call the gateway dispatched for the entrant. Content is held as
/// digests only: the log binds what was asked and answered without carrying
/// prompts or answers.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModelCallRecord {
    pub provider: String,
    pub model: String,
    pub revision: String,
    /// [`model_request_sha256`] of the dispatched request.
    pub request_sha256: String,
    pub outcome: ModelCallOutcome,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<u16>,
    /// SHA-256 of the provider's response body, when it answered.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub response_sha256: Option<String>,
    /// Provider-reported usage. Absent means unreported, never zero.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input_tokens: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_tokens: Option<u64>,
    pub started_unix_ms: u64,
}

/// What one live log entry records.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub enum LiveEvent {
    /// The entrant was given the observation of this bar.
    Observation {
        observation_id: String,
        observation_sha256: String,
    },
    /// A model call dispatched while the entrant decided this bar.
    ModelCall(ModelCallRecord),
    /// The entrant's decision for this bar.
    Decision { decision: Decision },
    /// The host resumed the log after a restart. A bar that was open when the
    /// host stopped is observed again before it is decided; the entrant
    /// restarts as a fresh instance.
    Restart,
}

/// One appended log entry.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LiveEntry {
    /// 0-based position in the log.
    pub seq: u64,
    /// Digest of the previous entry, or of the header for the first one.
    pub prev_sha256: String,
    /// Host clock when the entry was written. For a model call, when it
    /// finished.
    pub at_unix_ms: u64,
    /// Window-major run index and 0-based step within that run. Zero for a
    /// restart.
    pub run: usize,
    pub step: usize,
    pub event: LiveEvent,
}

/// The payload a seal signs.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LiveSeal {
    /// Always [`LIVE_SEAL_KIND`].
    pub kind: String,
    pub window_id: String,
    pub agent_id: String,
    pub image: String,
    pub header_sha256: String,
    pub entries: u64,
    pub root_sha256: String,
    pub closed_at_unix_ms: u64,
}

/// A closed live log as an entrant reveals it.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SealedLiveLog {
    pub header: LiveLogHeader,
    pub entries: Vec<LiveEntry>,
    /// A one-link chain under the arena key whose payload is a [`LiveSeal`].
    pub seal: PublicChain,
}

/// What a window records of one admitted live log, and its signed header
/// publishes. The seal is kept whole so a reader can check it under the
/// board's key without the log.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LiveCaptureRoot {
    pub image: String,
    pub header_sha256: String,
    pub entries: u64,
    /// Bars decided: one per step of every run.
    pub bars: u64,
    pub model_calls: u64,
    /// Times the host resumed the log. Each one restarted the entrant as a
    /// fresh instance mid-window.
    pub restarts: u64,
    pub root_sha256: String,
    pub closed_at_unix_ms: u64,
    pub seal: PublicChain,
}

/// Why a live log does not certify.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LiveLogRefusal {
    /// The seal is not a valid one-link chain carrying a seal payload.
    UnsignedSeal(String),
    /// The seal signs another log than the one revealed.
    SealMismatch(String),
    /// The header names another window, agent, scorer, schema or matrix.
    HeaderMismatch(String),
    /// The header's image is not the committed artifact.
    WrongImageDigest { logged: String, committed: String },
    /// An entry's position or back-link does not continue the chain.
    BrokenChain { seq: u64 },
    /// An entry is out of order for its run's bar sequence.
    OutOfOrder { seq: u64, detail: String },
    /// A bar of a run has no complete record (a dropped epoch).
    MissingBar { run: usize, step: usize },
    /// The host clock disagrees with the bar calendar or itself.
    ClockSkew { seq: u64, detail: String },
    /// The entrant was told something the revealed data does not produce.
    DataMismatch {
        run: usize,
        step: usize,
        detail: String,
    },
    /// Strict replay of the logged decisions refused them.
    Replay(String),
}

impl std::fmt::Display for LiveLogRefusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnsignedSeal(detail) => write!(f, "live log seal is not valid: {detail}"),
            Self::SealMismatch(detail) => {
                write!(f, "live log seal does not sign this log: {detail}")
            }
            Self::HeaderMismatch(detail) => write!(f, "live log header mismatch: {detail}"),
            Self::WrongImageDigest { logged, committed } => write!(
                f,
                "live log ran image digest {logged}, but the committed artifact digest is {committed}"
            ),
            Self::BrokenChain { seq } => {
                write!(f, "live log chain is broken at entry {seq}")
            }
            Self::OutOfOrder { seq, detail } => {
                write!(f, "live log entry {seq} is out of order: {detail}")
            }
            Self::MissingBar { run, step } => {
                write!(f, "live log has no complete record of run {run} step {step}")
            }
            Self::ClockSkew { seq, detail } => {
                write!(f, "live log entry {seq} fails the clock check: {detail}")
            }
            Self::DataMismatch { run, step, detail } => write!(
                f,
                "live log run {run} step {step} does not match the revealed data: {detail}"
            ),
            Self::Replay(detail) => write!(f, "live log decisions do not replay: {detail}"),
        }
    }
}

/// A live log that certified, with what the window records of it.
pub struct CertifiedLiveLog {
    pub trajectory: AgentTrajectory,
    pub root: LiveCaptureRoot,
}

/// SHA-256 of an observation as the harness hands it to the entrant.
pub fn observation_sha256(observation: &MarketObservation) -> String {
    content_digest(&serde_json::to_vec(observation).expect("observations serialize"))
}

/// SHA-256 of a dispatched model request: provider, model, revision, messages,
/// tools and output bound. The credential and destination are not part of it.
pub fn model_request_sha256(call: &ProviderCall<'_>) -> String {
    let request: (&str, &str, &str, &str, &[Message], &[ToolDefinition], u32) = (
        "sharpebench-arena/live-model-request/v1",
        call.provider,
        call.model,
        call.revision,
        call.messages,
        call.tools,
        call.max_output_tokens,
    );
    content_digest(&serde_json::to_vec(&request).expect("model requests serialize"))
}

/// The digest a header starts the chain from.
pub fn header_sha256(header: &LiveLogHeader) -> String {
    framed_digest("live-log-header", header)
}

/// The digest of one entry, which the next entry carries.
pub fn entry_sha256(entry: &LiveEntry) -> String {
    framed_digest("live-entry", entry)
}

fn framed_digest(label: &str, value: &impl Serialize) -> String {
    let mut bytes = format!("sharpebench-arena/{label}/v1\n").into_bytes();
    bytes.extend(serde_json::to_vec(value).expect("live log records serialize"));
    content_digest(&bytes)
}

/// Check that `entries` continue the chain from `header`, and return its root:
/// the last entry's digest, or the header's when there is none.
pub fn chain_root(header: &LiveLogHeader, entries: &[LiveEntry]) -> Result<String, LiveLogRefusal> {
    let mut prev = header_sha256(header);
    // inv: `prev` is the digest of entries[..i]'s last element (or the header).
    for (index, entry) in entries.iter().enumerate() {
        if entry.seq != index as u64 || entry.prev_sha256 != prev {
            return Err(LiveLogRefusal::BrokenChain { seq: index as u64 });
        }
        prev = entry_sha256(entry);
    }
    Ok(prev)
}

/// Sign a closed log's root under the arena key.
pub fn seal_live_log(
    header: LiveLogHeader,
    entries: Vec<LiveEntry>,
    key: &SigningKey,
    closed_at_unix_ms: u64,
) -> Result<SealedLiveLog, String> {
    let root_sha256 = chain_root(&header, &entries).map_err(|refusal| refusal.to_string())?;
    let seal = LiveSeal {
        kind: LIVE_SEAL_KIND.to_string(),
        window_id: header.window_id.clone(),
        agent_id: header.agent_id.clone(),
        image: header.image.clone(),
        header_sha256: header_sha256(&header),
        entries: entries.len() as u64,
        root_sha256,
        closed_at_unix_ms,
    };
    let payload = serde_json::to_string(&seal).map_err(|e| format!("serialize seal: {e}"))?;
    Ok(SealedLiveLog {
        header,
        entries,
        seal: publish_public_chain(&[payload], key),
    })
}

/// The seal payload a valid one-link seal chain carries.
fn read_seal(seal: &PublicChain) -> Result<LiveSeal, String> {
    if !verify_public_chain(seal) {
        return Err("the Ed25519 seal does not verify".to_string());
    }
    let [link] = seal.chain.as_slice() else {
        return Err("a seal holds exactly one link".to_string());
    };
    let payload: LiveSeal = serde_json::from_str(&link.payload)
        .map_err(|e| format!("the seal payload is not a live seal: {e}"))?;
    if payload.kind != LIVE_SEAL_KIND {
        return Err(format!("the seal payload has kind `{}`", payload.kind));
    }
    Ok(payload)
}

/// Check a recorded root for `agent_id` in window `window_id`: its seal
/// verifies, under `key` when one is given, and signs exactly the root's
/// fields. Used when a window loads, when it publishes, and when a board
/// verifies.
pub fn check_root(
    window_id: &str,
    agent_id: &str,
    root: &LiveCaptureRoot,
    key: Option<&str>,
) -> Result<(), String> {
    let payload = read_seal(&root.seal)
        .map_err(|error| format!("live capture root for `{agent_id}`: {error}"))?;
    if let Some(key) = key {
        if root.seal.verifying_key != key {
            return Err(format!(
                "live capture root for `{agent_id}` is sealed under key {}, not the arena key {key}",
                root.seal.verifying_key
            ));
        }
    }
    let expected = LiveSeal {
        kind: LIVE_SEAL_KIND.to_string(),
        window_id: window_id.to_string(),
        agent_id: agent_id.to_string(),
        image: root.image.clone(),
        header_sha256: root.header_sha256.clone(),
        entries: root.entries,
        root_sha256: root.root_sha256.clone(),
        closed_at_unix_ms: root.closed_at_unix_ms,
    };
    if payload != expected {
        return Err(format!(
            "live capture root for `{agent_id}` differs from what its seal signs"
        ));
    }
    Ok(())
}

/// Milliseconds since the Unix epoch at the start of a bar's UTC day, for a
/// bar date written `YYYY-MM-DD` or as a `YYYY-DDD` ordinal date. `None` for
/// anything else, or a day before 1970.
pub fn bar_day_start_ms(date: &str) -> Option<u64> {
    let bytes = date.as_bytes();
    if !bytes.iter().all(|b| b.is_ascii_digit() || *b == b'-') {
        return None;
    }
    let number = |range: std::ops::Range<usize>| date.get(range)?.parse::<i64>().ok();
    let (year, days) = match bytes.len() {
        10 if bytes[4] == b'-' && bytes[7] == b'-' => {
            let (year, month, day) = (number(0..4)?, number(5..7)?, number(8..10)?);
            if !(1..=12).contains(&month) || day < 1 || day > days_in_month(year, month) {
                return None;
            }
            (year, days_from_civil(year, month, day))
        }
        8 if bytes[4] == b'-' => {
            let (year, ordinal) = (number(0..4)?, number(5..8)?);
            let length = if is_leap(year) { 366 } else { 365 };
            if !(1..=length).contains(&ordinal) {
                return None;
            }
            (year, days_from_civil(year, 1, 1) + ordinal - 1)
        }
        _ => return None,
    };
    (year >= 1970).then(|| days as u64 * DAY_MS)
}

fn is_leap(year: i64) -> bool {
    (year % 4 == 0 && year % 100 != 0) || year % 400 == 0
}

fn days_in_month(year: i64, month: i64) -> i64 {
    match month {
        2 if is_leap(year) => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    }
}

/// Days from 1970-01-01 to a proleptic Gregorian date (H. Hinnant's
/// `days_from_civil`).
fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let year = if month <= 2 { year - 1 } else { year };
    let era = year.div_euclid(400);
    let year_of_era = year - era * 400;
    let month_index = (month + 9) % 12;
    let day_of_year = (153 * month_index + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146_097 + day_of_era - 719_468
}

/// The state of one run while its entries are read in chain order.
#[derive(Default)]
struct RunState {
    /// The next step to observe.
    next_step: usize,
    /// The open bar's observation digest, when a bar is open.
    open: Option<String>,
    /// A restart happened while the bar was open, so it may be observed again.
    reopenable: bool,
    /// The latest clock reading of this run.
    last_at: u64,
    decisions: Vec<Decision>,
    observations: Vec<String>,
}

/// Read a header and its entries into one decision and one observation digest
/// per bar of every run, checking order and clocks (rules 4 and 5 of the module
/// docs). Returns the runs and the model call and restart counts.
fn read_bars(
    replay: &ReplayWindow,
    entries: &[LiveEntry],
) -> Result<(Vec<RunState>, u64, u64), LiveLogRefusal> {
    let lengths: Vec<(usize, usize)> = replay
        .windows
        .iter()
        .flat_map(|w| replay.seeds.iter().map(move |_| (w.start, w.end - w.start)))
        .collect();
    let mut runs: Vec<RunState> = lengths.iter().map(|_| RunState::default()).collect();
    let (mut model_calls, mut restarts) = (0, 0);
    let dates = &replay.data.dates;
    // inv: each run's state describes exactly its entries among entries[..i].
    for entry in entries {
        let seq = entry.seq;
        let out_of_order = |detail: String| LiveLogRefusal::OutOfOrder { seq, detail };
        if matches!(entry.event, LiveEvent::Restart) {
            restarts += 1;
            for run in &mut runs {
                run.reopenable = run.open.is_some();
            }
            continue;
        }
        let Some(&(start, length)) = lengths.get(entry.run) else {
            return Err(out_of_order(format!("names run {}", entry.run)));
        };
        let run = &mut runs[entry.run];
        if entry.at_unix_ms + MAX_CLOCK_SKEW_MS < run.last_at {
            return Err(LiveLogRefusal::ClockSkew {
                seq,
                detail: format!(
                    "the run's clock went back from {} to {}",
                    run.last_at, entry.at_unix_ms
                ),
            });
        }
        run.last_at = run.last_at.max(entry.at_unix_ms);
        // A bar open across a restart must be shown to the fresh instance
        // again before it can be decided.
        let at_open_bar = run.open.is_some() && entry.step == run.next_step;
        let open_here = at_open_bar && !run.reopenable;
        match &entry.event {
            LiveEvent::Observation {
                observation_id,
                observation_sha256,
            } => {
                if run.open.is_some() && !(at_open_bar && run.reopenable) {
                    return Err(out_of_order(
                        "observes a bar while another is open".to_string(),
                    ));
                }
                if entry.step < run.next_step || entry.step >= length {
                    return Err(out_of_order(format!(
                        "observes step {}, which is decided or beyond the run",
                        entry.step
                    )));
                }
                if entry.step > run.next_step {
                    return Err(LiveLogRefusal::MissingBar {
                        run: entry.run,
                        step: run.next_step,
                    });
                }
                if run
                    .open
                    .as_ref()
                    .is_some_and(|digest| digest != observation_sha256)
                {
                    return Err(LiveLogRefusal::DataMismatch {
                        run: entry.run,
                        step: entry.step,
                        detail: "the bar was observed again with another observation".to_string(),
                    });
                }
                let date = &dates[start + entry.step];
                if observation_id != date {
                    return Err(LiveLogRefusal::DataMismatch {
                        run: entry.run,
                        step: entry.step,
                        detail: format!("observation `{observation_id}`, revealed bar `{date}`"),
                    });
                }
                let bar = bar_day_start_ms(date).ok_or_else(|| LiveLogRefusal::DataMismatch {
                    run: entry.run,
                    step: entry.step,
                    detail: format!("bar date `{date}` is not a calendar date"),
                })?;
                if entry.at_unix_ms + MAX_CLOCK_SKEW_MS < bar {
                    return Err(LiveLogRefusal::ClockSkew {
                        seq,
                        detail: format!("bar `{date}` was observed before its day began"),
                    });
                }
                run.open = Some(observation_sha256.clone());
                run.reopenable = false;
            }
            LiveEvent::ModelCall(call) => {
                if !open_here {
                    return Err(out_of_order(
                        "a model call outside its bar's decision".to_string(),
                    ));
                }
                if call.started_unix_ms > entry.at_unix_ms {
                    return Err(LiveLogRefusal::ClockSkew {
                        seq,
                        detail: "the model call finished before it started".to_string(),
                    });
                }
                model_calls += 1;
            }
            LiveEvent::Decision { decision } => {
                if !open_here {
                    return Err(out_of_order("a decision for no open bar".to_string()));
                }
                let t = start + entry.step;
                let deadline = match dates.get(t + 1) {
                    Some(next) => bar_day_start_ms(next),
                    None => bar_day_start_ms(&dates[t]).map(|day| day + DAY_MS),
                }
                .ok_or_else(|| LiveLogRefusal::DataMismatch {
                    run: entry.run,
                    step: entry.step,
                    detail: "the next bar's date is not a calendar date".to_string(),
                })?;
                if entry.at_unix_ms >= deadline + MAX_CLOCK_SKEW_MS {
                    return Err(LiveLogRefusal::ClockSkew {
                        seq,
                        detail: format!("step {} was decided after the next bar began", entry.step),
                    });
                }
                run.observations
                    .push(run.open.take().expect("the bar is open"));
                run.decisions.push(decision.clone());
                run.next_step += 1;
                run.reopenable = false;
            }
            LiveEvent::Restart => unreachable!("handled above"),
        }
    }
    for (index, (run, &(_, length))) in runs.iter().zip(&lengths).enumerate() {
        if run.next_step != length {
            return Err(LiveLogRefusal::MissingBar {
                run: index,
                step: run.next_step,
            });
        }
    }
    Ok((runs, model_calls, restarts))
}

/// Plays a run's logged decisions back and records the first bar whose
/// observation is not the logged one.
struct Playback {
    run: usize,
    step: usize,
    decisions: Vec<Decision>,
    observations: Vec<String>,
    mismatch: Arc<Mutex<Option<(usize, usize)>>>,
}

impl Agent for Playback {
    fn decide(&mut self, observation: &MarketObservation) -> Decision {
        let step = self.step;
        self.step += 1;
        if self.observations.get(step) != Some(&observation_sha256(observation)) {
            lock(&self.mismatch).get_or_insert((self.run, step));
        }
        match self.decisions.get(step) {
            Some(decision) => decision.clone(),
            None => HoldAgent.decide(observation),
        }
    }
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Admit a revealed live log for `agent_id` in window `window_id`, whose
/// commitment opened with `committed_artifact`. See the module docs for the
/// rules. Returns the trajectory the arena replays and the root it records.
pub fn certify_live_log(
    window_id: &str,
    scorer_artifact_sha256: &str,
    config: &ScoreConfig,
    replay: &ReplayWindow,
    agent_id: &str,
    committed_artifact: &str,
    log: &SealedLiveLog,
) -> Result<CertifiedLiveLog, LiveLogRefusal> {
    let seal = read_seal(&log.seal).map_err(LiveLogRefusal::UnsignedSeal)?;
    let header = &log.header;
    let header_sha256 = header_sha256(header);
    let root_sha256 = chain_root(header, &log.entries)?;
    let sealed = (
        &seal.window_id,
        &seal.agent_id,
        &seal.image,
        &seal.header_sha256,
        seal.entries,
        &seal.root_sha256,
    );
    let logged = (
        &header.window_id,
        &header.agent_id,
        &header.image,
        &header_sha256,
        log.entries.len() as u64,
        &root_sha256,
    );
    if sealed != logged {
        return Err(LiveLogRefusal::SealMismatch(format!(
            "the seal signs {sealed:?}, the log is {logged:?}"
        )));
    }
    let mismatch = |detail: String| Err(LiveLogRefusal::HeaderMismatch(detail));
    if header.schema != LIVE_LOG_SCHEMA {
        return mismatch(format!("schema `{}`", header.schema));
    }
    if header.window_id != window_id || header.agent_id != agent_id {
        return mismatch(format!(
            "the log is for agent `{}` in window `{}`",
            header.agent_id, header.window_id
        ));
    }
    if header.runner_artifact_sha256 != scorer_artifact_sha256 {
        return mismatch(format!(
            "the host ran scorer {}, the window freezes {scorer_artifact_sha256}",
            header.runner_artifact_sha256
        ));
    }
    let planned: Vec<LiveWindow> = replay
        .windows
        .iter()
        .map(|w| LiveWindow {
            start: w.start,
            end: w.end,
        })
        .collect();
    if header.windows != planned || header.seeds != replay.seeds {
        return mismatch(format!(
            "the log runs windows {:?} seeds {:?}, the revealed data scores {planned:?} seeds {:?}",
            header.windows, header.seeds, replay.seeds
        ));
    }
    let entrant = crate::intake::CapturedEntrant::of(&AgentTrajectory {
        agent_id: format!("sandbox:{}", header.image),
        contract: None,
        in_sample_trials: 0,
        declared_mandate: None,
        runs: Vec::new(),
    })
    .map_err(LiveLogRefusal::HeaderMismatch)?;
    if entrant.artifact_sha256() != committed_artifact {
        return Err(LiveLogRefusal::WrongImageDigest {
            logged: entrant.artifact_sha256().to_string(),
            committed: committed_artifact.to_string(),
        });
    }

    let (runs, model_calls, restarts) = read_bars(replay, &log.entries)?;
    let windows = &replay.windows;
    let mut contract = sharpebench_harness::trajectory_contract(
        &replay.data,
        replay.costs,
        windows,
        &replay.seeds,
    );
    contract.runner_artifact_sha256 = Some(scorer_artifact_sha256.to_string());
    let coordinates = windows
        .iter()
        .flat_map(|w| replay.seeds.iter().map(move |seed| (w.start, w.end, *seed)));
    let trajectory = AgentTrajectory {
        agent_id: format!("sandbox:{}", header.image),
        contract: Some(contract),
        in_sample_trials: 0,
        declared_mandate: None,
        runs: coordinates
            .zip(&runs)
            .map(|((window_start, window_end, seed), run)| RunTrajectory {
                window_start,
                window_end,
                seed,
                steps: run
                    .decisions
                    .iter()
                    .enumerate()
                    .map(|(step, decision)| DecisionStep {
                        step,
                        observation_id: replay.data.dates[window_start + step].clone(),
                        decision: decision.clone(),
                    })
                    .collect(),
            })
            .collect(),
    };
    let first_mismatch = Arc::new(Mutex::new(None));
    let mut next_run = 0;
    let verified = verify_trajectory_reexecuted(
        &replay.data,
        &trajectory,
        replay.costs,
        config,
        Some(scorer_artifact_sha256),
        || {
            let run = next_run;
            next_run += 1;
            Box::new(Playback {
                run,
                step: 0,
                decisions: runs[run].decisions.clone(),
                observations: runs[run].observations.clone(),
                mismatch: Arc::clone(&first_mismatch),
            })
        },
    );
    if let Some((run, step)) = *lock(&first_mismatch) {
        return Err(LiveLogRefusal::DataMismatch {
            run,
            step,
            detail: "the logged observation is not the one the revealed data produces".to_string(),
        });
    }
    verified.map_err(|error| match error {
        ReexecutionError::Refused(reason) => LiveLogRefusal::Replay(reason),
        diverged @ ReexecutionError::Diverged(_) => LiveLogRefusal::Replay(diverged.to_string()),
    })?;
    let bars = runs.iter().map(|run| run.decisions.len() as u64).sum();
    Ok(CertifiedLiveLog {
        trajectory,
        root: LiveCaptureRoot {
            image: header.image.clone(),
            header_sha256,
            entries: log.entries.len() as u64,
            bars,
            model_calls,
            restarts,
            root_sha256,
            closed_at_unix_ms: seal.closed_at_unix_ms,
            seal: log.seal.clone(),
        },
    })
}

/// Check a window's recorded roots against the rest of its record: a window
/// with roots has no fault plan, replayed its entries, and records exactly one
/// `live-captured` row per root, each sealed as [`check_root`] requires.
pub(crate) fn check_window_roots(window: &crate::WindowState) -> Result<(), String> {
    if window.live_capture.is_empty() {
        return Ok(());
    }
    let id = &window.id;
    if window.fault_plan_sha256.is_some() || window.execution.is_none() {
        return Err(format!(
            "window `{id}` records a live capture root but is faulted or replayed nothing"
        ));
    }
    check_rows_match_roots(&window.returns_provenance, &window.live_capture)
        .map_err(|error| format!("window `{id}`: {error}"))?;
    for (agent_id, root) in &window.live_capture {
        check_root(id, agent_id, root, None).map_err(|error| format!("window `{id}`: {error}"))?;
    }
    Ok(())
}

fn check_rows_match_roots(
    provenance: &BTreeMap<String, crate::ReturnsProvenance>,
    roots: &BTreeMap<String, LiveCaptureRoot>,
) -> Result<(), String> {
    let live: Vec<&String> = provenance
        .iter()
        .filter(|(_, p)| **p == crate::ReturnsProvenance::LiveCaptured)
        .map(|(agent, _)| agent)
        .collect();
    let rooted: Vec<&String> = roots.keys().collect();
    if live != rooted {
        return Err(format!(
            "live-captured rows {live:?} do not match live capture roots {rooted:?}"
        ));
    }
    Ok(())
}

/// The live capture evidence a signed board fails on, if any: every
/// `live-captured` row has a root in the header, every root a row, and every
/// root's seal verifies under the board's key.
pub(crate) fn board_error(header: &crate::WindowHeader, board: &PublicChain) -> Option<String> {
    let mut provenance = BTreeMap::new();
    for link in board.chain.iter().skip(1) {
        let row: crate::BoardRow = serde_json::from_str(&link.payload).ok()?;
        if let Some(source) = row.returns_provenance {
            provenance.insert(row.score.agent_id, source);
        }
    }
    check_rows_match_roots(&provenance, &header.live_capture)
        .and_then(|()| {
            header.live_capture.iter().try_for_each(|(agent_id, root)| {
                check_root(
                    &header.window_id,
                    agent_id,
                    root,
                    Some(&board.verifying_key),
                )
            })
        })
        .err()
}

/// A source of host time. The host uses [`SystemClock`]; tests substitute a
/// fake to place bars and skew the clock.
pub trait LiveClock: Send {
    fn now_unix_ms(&self) -> u64;
}

/// The host's wall clock.
pub struct SystemClock;

impl LiveClock for SystemClock {
    fn now_unix_ms(&self) -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |elapsed| elapsed.as_millis() as u64)
    }
}

/// The host side of a live log: appends entries to an append-only JSON-lines
/// file (header first, one entry per line), flushing each one to disk before
/// the next, and keeps them for sealing.
///
/// A write that fails is latched: nothing further is appended and the log can
/// no longer be sealed, so a gap can never be sealed over.
pub struct LiveRecorder {
    path: PathBuf,
    header: LiveLogHeader,
    entries: Vec<LiveEntry>,
    last_sha256: String,
    clock: Box<dyn LiveClock>,
    /// The bar the entrant is deciding, for model calls made meanwhile.
    bar: (usize, usize),
    failure: Option<String>,
}

/// A recorder shared by the agent wrapper and the gateway transport.
pub type SharedRecorder = Arc<Mutex<LiveRecorder>>;

impl LiveRecorder {
    /// Start a new log at `path`, which must not exist.
    pub fn create(
        path: &Path,
        header: LiveLogHeader,
        clock: Box<dyn LiveClock>,
    ) -> Result<Self, String> {
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path)
            .map_err(|e| format!("cannot create live log {}: {e}", path.display()))?;
        let line = serde_json::to_string(&header).map_err(|e| e.to_string())?;
        writeln!(file, "{line}")
            .and_then(|()| file.sync_all())
            .map_err(|e| format!("cannot write live log {}: {e}", path.display()))?;
        Ok(Self {
            path: path.to_path_buf(),
            last_sha256: header_sha256(&header),
            header,
            entries: Vec::new(),
            clock,
            bar: (0, 0),
            failure: None,
        })
    }

    /// Read a log written by a recorder: the header line, then one entry per
    /// line, the chain intact. A file whose last line is incomplete (a write
    /// cut short) is refused rather than repaired.
    pub fn read(path: &Path) -> Result<(LiveLogHeader, Vec<LiveEntry>), String> {
        let text = std::fs::read_to_string(path)
            .map_err(|e| format!("cannot read live log {}: {e}", path.display()))?;
        let Some(body) = text.strip_suffix('\n') else {
            return Err(format!(
                "live log {} ends in an incomplete line",
                path.display()
            ));
        };
        let mut lines = body.split('\n');
        let header: LiveLogHeader = serde_json::from_str(lines.next().unwrap_or_default())
            .map_err(|e| format!("live log {} has no header: {e}", path.display()))?;
        let entries = lines
            .map(serde_json::from_str)
            .collect::<Result<Vec<LiveEntry>, _>>()
            .map_err(|e| format!("live log {} holds an unreadable entry: {e}", path.display()))?;
        chain_root(&header, &entries).map_err(|refusal| refusal.to_string())?;
        Ok((header, entries))
    }

    /// Reopen a log after the host stopped, and record the restart.
    pub fn resume(path: &Path, clock: Box<dyn LiveClock>) -> Result<Self, String> {
        let (header, entries) = Self::read(path)?;
        let mut recorder = Self {
            path: path.to_path_buf(),
            last_sha256: chain_root(&header, &entries).map_err(|r| r.to_string())?,
            header,
            entries,
            clock,
            bar: (0, 0),
            failure: None,
        };
        recorder.append(0, 0, LiveEvent::Restart, None);
        recorder.failure.clone().map_or(Ok(recorder), Err)
    }

    pub fn header(&self) -> &LiveLogHeader {
        &self.header
    }

    pub fn entries(&self) -> &[LiveEntry] {
        &self.entries
    }

    /// The first write that failed, after which nothing was appended.
    pub fn failure(&self) -> Option<&str> {
        self.failure.as_deref()
    }

    fn append(&mut self, run: usize, step: usize, event: LiveEvent, at: Option<u64>) {
        if self.failure.is_some() {
            return;
        }
        let entry = LiveEntry {
            seq: self.entries.len() as u64,
            prev_sha256: self.last_sha256.clone(),
            at_unix_ms: at.unwrap_or_else(|| self.clock.now_unix_ms()),
            run,
            step,
            event,
        };
        let written = serde_json::to_string(&entry)
            .map_err(|e| e.to_string())
            .and_then(|line| {
                let mut file = std::fs::OpenOptions::new()
                    .append(true)
                    .open(&self.path)
                    .map_err(|e| e.to_string())?;
                writeln!(file, "{line}")
                    .and_then(|()| file.sync_data())
                    .map_err(|e| e.to_string())
            });
        match written {
            Ok(()) => {
                self.last_sha256 = entry_sha256(&entry);
                self.entries.push(entry);
            }
            Err(error) => {
                self.failure = Some(format!(
                    "live log {} lost entry {}: {error}",
                    self.path.display(),
                    entry.seq
                ));
            }
        }
    }

    /// Record the observation of `step` in `run`; model calls are attributed
    /// to this bar until it is decided.
    pub fn record_observation(&mut self, run: usize, step: usize, observation: &MarketObservation) {
        self.bar = (run, step);
        let event = LiveEvent::Observation {
            observation_id: observation.date.clone(),
            observation_sha256: observation_sha256(observation),
        };
        self.append(run, step, event, None);
    }

    /// Record a model call that finished now, for the bar being decided.
    pub fn record_model_call(&mut self, call: ModelCallRecord) {
        let (run, step) = self.bar;
        self.append(run, step, LiveEvent::ModelCall(call), None);
    }

    pub fn record_decision(&mut self, run: usize, step: usize, decision: &Decision) {
        let event = LiveEvent::Decision {
            decision: decision.clone(),
        };
        self.append(run, step, event, None);
    }

    pub fn now_unix_ms(&self) -> u64 {
        self.clock.now_unix_ms()
    }

    /// Seal the log at window close. Refused when a write was lost.
    pub fn seal(&self, key: &SigningKey) -> Result<SealedLiveLog, String> {
        if let Some(failure) = &self.failure {
            return Err(failure.clone());
        }
        seal_live_log(
            self.header.clone(),
            self.entries.clone(),
            key,
            self.clock.now_unix_ms(),
        )
    }
}

/// Wraps the entrant of one run: records each observation it is given and
/// each decision it returns.
pub struct LiveLoggedAgent<A> {
    inner: A,
    recorder: SharedRecorder,
    run: usize,
    step: usize,
}

impl<A: Agent> LiveLoggedAgent<A> {
    /// The entrant of window-major run `run`, starting at its first step.
    pub fn new(inner: A, recorder: SharedRecorder, run: usize) -> Self {
        Self::resuming(inner, recorder, run, 0)
    }

    /// A fresh entrant instance that takes over `run` at `step`, after a
    /// restart.
    pub fn resuming(inner: A, recorder: SharedRecorder, run: usize, step: usize) -> Self {
        Self {
            inner,
            recorder,
            run,
            step,
        }
    }
}

impl<A: Agent> Agent for LiveLoggedAgent<A> {
    fn decide(&mut self, observation: &MarketObservation) -> Decision {
        let step = self.step;
        self.step += 1;
        lock(&self.recorder).record_observation(self.run, step, observation);
        let decision = self.inner.decide(observation);
        lock(&self.recorder).record_decision(self.run, step, &decision);
        decision
    }
}

/// Wraps the gateway's provider transport: every dispatch is recorded as a
/// model call of the bar being decided, and its outcome passed through
/// unchanged.
pub struct LoggingTransport<T> {
    inner: T,
    recorder: SharedRecorder,
}

impl<T: ProviderTransport> LoggingTransport<T> {
    pub fn new(inner: T, recorder: SharedRecorder) -> Self {
        Self { inner, recorder }
    }
}

impl<T: ProviderTransport> ProviderTransport for LoggingTransport<T> {
    fn call(&mut self, call: ProviderCall<'_>) -> ProviderOutcome {
        let started_unix_ms = lock(&self.recorder).now_unix_ms();
        let (provider, model, revision) = (
            call.provider.to_string(),
            call.model.to_string(),
            call.revision.to_string(),
        );
        let request_sha256 = model_request_sha256(&call);
        let outcome = self.inner.call(call);
        let mut record = ModelCallRecord {
            provider,
            model,
            revision,
            request_sha256,
            outcome: ModelCallOutcome::RefusedBeforeWork,
            status: None,
            response_sha256: None,
            input_tokens: None,
            output_tokens: None,
            started_unix_ms,
        };
        match &outcome {
            ProviderOutcome::Answered { status, body } => {
                record.outcome = ModelCallOutcome::Answered;
                record.status = Some(*status);
                record.response_sha256 = Some(content_digest(body));
                let usage = serde_json::from_slice::<ProviderBody>(body)
                    .ok()
                    .and_then(|body| body.usage);
                record.input_tokens = usage.map(|u| u.input_tokens);
                record.output_tokens = usage.map(|u| u.output_tokens);
            }
            ProviderOutcome::RefusedBeforeWork(_) => {}
            ProviderOutcome::AmbiguousAfterCommit(_) => {
                record.outcome = ModelCallOutcome::AmbiguousAfterCommit;
            }
        }
        lock(&self.recorder).record_model_call(record);
        outcome
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sharpebench_harness::gateway::{MessageRole, ProviderFault, ProviderUsage, Secret};

    #[test]
    fn bar_dates_map_to_the_start_of_their_utc_day() {
        let day = |n: u64| Some(n * DAY_MS);
        let cases = [
            ("1970-01-01", day(0)),
            ("1970-001", day(0)),
            ("1970-03-01", day(59)),
            ("2000-02-29", day(11_016)),
            ("2000-03-01", day(11_017)),
            ("2100-03-01", day(47_541)),
            ("2024-02-29", day(19_782)),
            ("2024-366", day(20_088)),
            ("2024-12-31", day(20_088)),
            ("2025-01-01", day(20_089)),
            ("2025-001", day(20_089)),
            ("2025-365", day(20_453)),
            ("2025-04-30", day(20_208)),
            ("2025-366", None),
            ("2025-000", None),
            ("2025-02-29", None),
            ("2100-02-29", None),
            ("2025-04-31", None),
            ("2025-01-32", None),
            ("2025-01-00", None),
            ("2025-13-01", None),
            ("2025-00-10", None),
            ("2025--1-01", None),
            ("1969-12-31", None),
            ("2025/01/01", None),
            ("20250101", None),
            ("2025-1-01", None),
            ("+025-01-01", None),
            ("", None),
        ];
        for (date, expected) in cases {
            assert_eq!(bar_day_start_ms(date), expected, "{date}");
        }
    }

    /// A provider that answers each call with the next scripted outcome.
    struct Scripted(Vec<ProviderOutcome>);

    impl ProviderTransport for Scripted {
        fn call(&mut self, _call: ProviderCall<'_>) -> ProviderOutcome {
            self.0.remove(0)
        }
    }

    fn header() -> LiveLogHeader {
        LiveLogHeader {
            schema: LIVE_LOG_SCHEMA.to_string(),
            window_id: "w".to_string(),
            agent_id: "a".to_string(),
            image: format!("test/x@sha256:{}", "0".repeat(64)),
            runner_artifact_sha256: "1".repeat(64),
            windows: vec![LiveWindow { start: 0, end: 1 }],
            seeds: vec![0],
        }
    }

    struct Fixed(u64);

    impl LiveClock for Fixed {
        fn now_unix_ms(&self) -> u64 {
            self.0
        }
    }

    fn recorder(tag: &str) -> (PathBuf, SharedRecorder) {
        let path = std::env::temp_dir().join(format!(
            "sharpebench-live-unit-{tag}-{}.jsonl",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&path);
        let recorder = LiveRecorder::create(&path, header(), Box::new(Fixed(7))).unwrap();
        (path, Arc::new(Mutex::new(recorder)))
    }

    fn hold() -> Decision {
        Decision {
            orders: Vec::new(),
            reasoning: String::new(),
            cost: None,
        }
    }

    fn user(content: &str) -> [Message; 1] {
        [Message {
            role: MessageRole::User,
            content: content.to_string(),
        }]
    }

    #[test]
    fn the_logging_transport_records_every_outcome_and_passes_it_through() {
        let (_, shared) = recorder("transport");
        let body = serde_json::to_vec(&ProviderBody {
            text: "ok".to_string(),
            finish_reason: None,
            usage: Some(ProviderUsage {
                input_tokens: 11,
                output_tokens: 3,
                cache_read_input_tokens: None,
                cache_write_input_tokens: None,
            }),
        })
        .unwrap();
        let mut transport = LoggingTransport::new(
            Scripted(vec![
                ProviderOutcome::Answered {
                    status: 200,
                    body: body.clone(),
                },
                ProviderOutcome::Answered {
                    status: 500,
                    body: b"not json".to_vec(),
                },
                ProviderOutcome::RefusedBeforeWork(ProviderFault::RateLimited),
                ProviderOutcome::AmbiguousAfterCommit(ProviderFault::Timeout),
            ]),
            Arc::clone(&shared),
        );
        lock(&shared).record_observation(
            0,
            0,
            &MarketObservation {
                date: "2025-001".to_string(),
                cash: 1.0,
                symbols: Vec::new(),
                portfolio: Vec::new(),
            },
        );
        let credential = Secret::new("sk-unit-test-not-a-secret");
        let messages = user("bar");
        let call = || ProviderCall {
            destination: "https://provider.invalid",
            credential: &credential,
            provider: "fake",
            model: "fake-1",
            revision: "r1",
            messages: &messages,
            tools: &[],
            max_output_tokens: 4,
            read_timeout: std::time::Duration::from_secs(1),
            call_timeout: std::time::Duration::from_secs(1),
            max_response_body_bytes: 1024,
        };
        let request_sha256 = model_request_sha256(&call());
        let outcomes: Vec<bool> = (0..4)
            .map(|_| matches!(transport.call(call()), ProviderOutcome::Answered { .. }))
            .collect();
        assert_eq!(outcomes, [true, true, false, false]);
        let guard = lock(&shared);
        let records: Vec<&ModelCallRecord> = guard
            .entries()
            .iter()
            .filter_map(|entry| match &entry.event {
                LiveEvent::ModelCall(record) => Some(record),
                _ => None,
            })
            .collect();
        let summary: Vec<_> = records
            .iter()
            .map(|r| {
                (
                    r.outcome,
                    r.status,
                    r.response_sha256.clone(),
                    r.input_tokens,
                    r.output_tokens,
                )
            })
            .collect();
        assert_eq!(
            summary,
            [
                (
                    ModelCallOutcome::Answered,
                    Some(200),
                    Some(content_digest(&body)),
                    Some(11),
                    Some(3)
                ),
                (
                    ModelCallOutcome::Answered,
                    Some(500),
                    Some(content_digest(b"not json")),
                    None,
                    None
                ),
                (ModelCallOutcome::RefusedBeforeWork, None, None, None, None),
                (
                    ModelCallOutcome::AmbiguousAfterCommit,
                    None,
                    None,
                    None,
                    None
                ),
            ]
        );
        for record in records {
            assert_eq!(
                (
                    record.provider.as_str(),
                    record.model.as_str(),
                    record.revision.as_str()
                ),
                ("fake", "fake-1", "r1")
            );
            assert_eq!(record.request_sha256, request_sha256);
            assert_eq!(record.started_unix_ms, 7);
        }
    }

    #[test]
    fn a_request_digest_binds_every_part_of_the_dispatched_request() {
        let credential = Secret::new("sk-unit-test-not-a-secret");
        let messages = user("bar");
        let other_messages = user("other");
        let tools = [ToolDefinition {
            name: "t".to_string(),
            schema_json: "{}".to_string(),
        }];
        let base = || ProviderCall {
            destination: "https://provider.invalid",
            credential: &credential,
            provider: "fake",
            model: "fake-1",
            revision: "r1",
            messages: &messages,
            tools: &[],
            max_output_tokens: 4,
            read_timeout: std::time::Duration::from_secs(1),
            call_timeout: std::time::Duration::from_secs(1),
            max_response_body_bytes: 1024,
        };
        let digest = model_request_sha256(&base());
        let variants = [
            ProviderCall {
                provider: "other",
                ..base()
            },
            ProviderCall {
                model: "fake-2",
                ..base()
            },
            ProviderCall {
                revision: "r2",
                ..base()
            },
            ProviderCall {
                messages: &other_messages,
                ..base()
            },
            ProviderCall {
                tools: &tools,
                ..base()
            },
            ProviderCall {
                max_output_tokens: 5,
                ..base()
            },
        ];
        for variant in variants {
            assert_ne!(model_request_sha256(&variant), digest);
        }
        // Where the call goes and with which credential is not content.
        let other_credential = Secret::new("sk-another-credential");
        assert_eq!(
            model_request_sha256(&ProviderCall {
                destination: "https://elsewhere.invalid",
                credential: &other_credential,
                ..base()
            }),
            digest
        );
    }

    #[test]
    fn a_lost_write_is_latched_and_the_log_cannot_be_sealed() {
        let (path, shared) = recorder("latch");
        lock(&shared).record_decision(0, 0, &hold());
        std::fs::remove_file(&path).unwrap();
        lock(&shared).record_decision(0, 1, &hold());
        std::fs::write(&path, b"").unwrap();
        lock(&shared).record_decision(0, 2, &hold());
        let guard = lock(&shared);
        assert_eq!(guard.entries().len(), 1);
        assert!(guard.failure().unwrap().contains("lost entry 1"));
        let key = SigningKey::derive(b"unit");
        assert_eq!(guard.seal(&key).unwrap_err(), guard.failure().unwrap());
        assert_eq!(guard.header(), &header());
        assert_eq!(guard.now_unix_ms(), 7);
    }

    #[test]
    fn a_resumed_log_continues_the_chain_from_its_last_entry() {
        let (path, shared) = recorder("resume");
        lock(&shared).record_decision(0, 3, &hold());
        drop(shared);
        let resumed = LiveRecorder::resume(&path, Box::new(Fixed(9))).unwrap();
        let entries = resumed.entries();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[1].event, LiveEvent::Restart);
        assert_eq!(
            (
                entries[1].seq,
                entries[1].at_unix_ms,
                entries[1].run,
                entries[1].step
            ),
            (1, 9, 0, 0)
        );
        assert_eq!(entries[1].prev_sha256, entry_sha256(&entries[0]));
        assert_eq!(
            LiveRecorder::read(&path).unwrap().1,
            entries.to_vec(),
            "the file holds what the recorder holds"
        );
        let sealed = resumed.seal(&SigningKey::derive(b"unit")).unwrap();
        let payload = read_seal(&sealed.seal).unwrap();
        assert_eq!(payload.entries, 2);
        assert_eq!(payload.closed_at_unix_ms, 9);
        assert_eq!(
            payload.root_sha256,
            chain_root(&sealed.header, &sealed.entries).unwrap()
        );
    }

    #[test]
    fn a_recorded_root_is_checked_against_exactly_what_its_seal_signs() {
        let key = SigningKey::derive(b"unit");
        let sealed = seal_live_log(header(), Vec::new(), &key, 5).unwrap();
        let root = LiveCaptureRoot {
            image: header().image,
            header_sha256: header_sha256(&header()),
            entries: 0,
            bars: 0,
            model_calls: 0,
            restarts: 0,
            root_sha256: header_sha256(&header()),
            closed_at_unix_ms: 5,
            seal: sealed.seal.clone(),
        };
        let own = key.verifying_key().to_hex();
        assert_eq!(check_root("w", "a", &root, Some(&own)), Ok(()));
        assert!(check_root("w", "b", &root, None).is_err());
        assert!(check_root("v", "a", &root, None).is_err());
        let other = SigningKey::derive(b"other").verifying_key().to_hex();
        assert!(check_root("w", "a", &root, Some(&other))
            .unwrap_err()
            .contains("not the arena key"));
        let edits: [fn(&mut LiveCaptureRoot); 5] = [
            |r| r.image.push('x'),
            |r| r.header_sha256 = "2".repeat(64),
            |r| r.entries = 1,
            |r| r.root_sha256 = "3".repeat(64),
            |r| r.closed_at_unix_ms = 6,
        ];
        for edit in edits {
            let mut altered = root.clone();
            edit(&mut altered);
            assert!(check_root("w", "a", &altered, None).is_err());
        }
        // A seal of two links, or of a payload of another kind, is no seal.
        let two = publish_public_chain(
            &[sealed.seal.chain[0].payload.clone(), "{}".to_string()],
            &key,
        );
        assert!(read_seal(&two).unwrap_err().contains("exactly one link"));
        let mut kind: LiveSeal = serde_json::from_str(&sealed.seal.chain[0].payload).unwrap();
        kind.kind = "other".to_string();
        let wrong_kind = publish_public_chain(&[serde_json::to_string(&kind).unwrap()], &key);
        assert!(read_seal(&wrong_kind).unwrap_err().contains("has kind"));
        let not_seal = publish_public_chain(&["{}".to_string()], &key);
        assert!(read_seal(&not_seal)
            .unwrap_err()
            .contains("not a live seal"));
    }
}
