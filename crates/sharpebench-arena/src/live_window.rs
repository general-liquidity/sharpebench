//! The live window runner: one operator command that takes a forward window
//! from open to a verified board with every entrant on [`crate::live_capture`].
//!
//! [`run_live_window`] does, in order and resumably:
//!
//! 1. checks the [`LiveWindowSpec`] and the [`LiveEntrants`] manifest, and has
//!    the [`LiveHost`] admit every image before anything is written;
//! 2. opens the window with the spec's frozen scorer and config, registers each
//!    entrant's commitment, and advances to the commit deadline, so the field is
//!    fixed before the first bar;
//! 3. pulls bars from a [`LiveFeed`] one at a time, appends each to an
//!    append-only feed journal, and for every run of the execution matrix the
//!    bar belongs to, hands the entrant the point-in-time observation and
//!    records it and the decision in that entrant's live log;
//! 4. at the last bar seals every log under the arena key, writes the revealed
//!    dataset, advances to the data-reveal epoch, scores, publishes and
//!    verifies.
//!
//! The bar count is fixed in the spec, because the execution matrix and so
//! every log header depend on it. Everything the runner keeps lives under
//! `<arena>/live/<window>/`: copies of the spec and manifest, `feed.jsonl` (one
//! bar per line), one log per entrant, and at close `dataset.csv` and
//! `entries.json`. Running the command again on the same directory resumes: a
//! feed line cut short by a crash is dropped (no entrant saw that bar), each
//! log is reopened with a restart entry, a bar that was open is shown to a
//! fresh entrant instance again, and a window already scored or published is
//! only published or verified.
//!
//! Clock checks, under the owner-confirmed conventions of
//! [`crate::live_capture::MAX_CLOCK_SKEW_MS`] and UTC bar days: before a bar is
//! handed to any entrant the host clock must have reached that bar's day, and
//! when a bar arrives the previous bar's decisions must have been recorded
//! before its day began. A failed check stops the run, since every log would
//! already fail rule 5 of live capture and further bars would spend model calls
//! on logs that cannot certify. The decision deadline of the last bar is left
//! to intake.
//!
//! A stalled entrant is handled by the launched transport's own budget: the
//! sandbox and gateway entrants give each decision 30 s net of host serving
//! time, then terminate the instance, and the run holds for every later bar.
//! The hold is the decision applied, so it is what the log records, and the
//! model calls the instance made before it stalled were dispatched and are
//! charged in the gateway journal by its settlement rules.

use std::collections::BTreeMap;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, PoisonError};

use serde::{Deserialize, Serialize};

use sharpebench_attest::{content_digest, make_commitment, SigningKey};
use sharpebench_core::ScoreConfig;
use sharpebench_protocol::{AgentTrajectory, Decision, MarketObservation};
use sharpebench_sim::{run_backtest, Agent, CostModel, Dataset, HoldAgent, Window};

use crate::intake::{execution_matrix, CapturedEntrant, LaunchedEntrant, ReplayWindow};
use crate::live_capture::{
    bar_day_start_ms, LiveClock, LiveEvent, LiveLogHeader, LiveRecorder, LiveWindow,
    SharedRecorder, SystemClock, LIVE_LOG_SCHEMA, MAX_CLOCK_SKEW_MS,
};
use crate::{
    verify_arena, Arena, Refusal, ReturnsProvenance, RevealedEntry, WindowStatus, WINDOWS_DIR,
};

/// Schema of a [`LiveWindowSpec`].
pub const LIVE_WINDOW_SPEC_SCHEMA: &str = "sharpebench-arena-live-window/v1";

/// Schema of a [`LiveEntrants`] manifest.
pub const LIVE_ENTRANTS_SCHEMA: &str = "sharpebench-arena-live-entrants/v1";

/// The directory under an arena that holds the runner's files, one
/// subdirectory per window.
pub const LIVE_DIR: &str = "live";

/// What a live window is, fixed before it opens.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LiveWindowSpec {
    /// Always [`LIVE_WINDOW_SPEC_SCHEMA`].
    pub schema: String,
    pub window_id: String,
    pub commit_deadline: u64,
    pub data_reveal_epoch: u64,
    /// Bars the window runs, warm-up history included. The execution matrix
    /// is [`execution_matrix`] of this count, so it is fixed here.
    pub bars: usize,
    pub scorer_artifact_sha256: String,
    /// The scoring rules; the default config when absent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub score_config: Option<ScoreConfig>,
}

/// One entrant the host runs live.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LiveEntrant {
    pub agent_id: String,
    /// The committed image, `<repository>@sha256:<digest>`.
    pub image: String,
    /// The commitment salt, revealed at close.
    pub salt: String,
}

/// The field of a live window.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LiveEntrants {
    /// Always [`LIVE_ENTRANTS_SCHEMA`].
    pub schema: String,
    pub entrants: Vec<LiveEntrant>,
}

/// One bar as a feed delivers it: its date and every symbol's close.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LiveBar {
    /// `YYYY-MM-DD`, or a `YYYY-DDD` ordinal date.
    pub date: String,
    pub closes: BTreeMap<String, f64>,
}

/// A source of bars. A live adapter blocks in [`LiveFeed::bar`] until the bar
/// exists; none ships here, because one needs market-data infrastructure and
/// data rights. [`ReplayFeed`] serves a file for tests and dry runs.
pub trait LiveFeed {
    /// Bar `index` (0-based), or `None` when the feed will never deliver it.
    fn bar(&mut self, index: usize) -> Result<Option<LiveBar>, String>;
}

/// Replays the bars of a `date,symbol,close` CSV, as `--data` reads it.
pub struct ReplayFeed {
    bars: Vec<LiveBar>,
}

impl ReplayFeed {
    pub fn from_csv(text: &str) -> Result<Self, String> {
        let data = Dataset::from_csv(text)?;
        let bars = (0..data.len())
            .map(|t| LiveBar {
                date: data.dates[t].clone(),
                closes: data
                    .closes
                    .iter()
                    .map(|(symbol, closes)| (symbol.clone(), closes[t]))
                    .collect(),
            })
            .collect();
        Ok(Self { bars })
    }
}

impl LiveFeed for ReplayFeed {
    fn bar(&mut self, index: usize) -> Result<Option<LiveBar>, String> {
        Ok(self.bars.get(index).cloned())
    }
}

/// The host's clock as the runner reads it.
pub trait HostClock: Send + Sync {
    fn now_unix_ms(&self) -> u64;
    /// Called when the runner starts on the bar dated `date`, on arrival and
    /// again when it resumes that bar. A wall clock ignores it.
    fn bar_arrived(&self, _date: &str) {}
}

impl HostClock for SystemClock {
    fn now_unix_ms(&self) -> u64 {
        LiveClock::now_unix_ms(self)
    }
}

/// A rehearsal clock: it moves to `arrival_offset_ms` past the start of each
/// bar's UTC day when the bar arrives (never backwards), and advances one
/// millisecond per reading. A board run under it rehearses the timing rules;
/// its timestamps are not the host's.
pub struct SimulatedClock {
    now: AtomicU64,
    arrival_offset_ms: u64,
}

impl SimulatedClock {
    pub fn new(arrival_offset_ms: u64) -> Self {
        Self {
            now: AtomicU64::new(0),
            arrival_offset_ms,
        }
    }
}

impl HostClock for SimulatedClock {
    fn now_unix_ms(&self) -> u64 {
        self.now.fetch_add(1, Ordering::SeqCst)
    }

    fn bar_arrived(&self, date: &str) {
        if let Some(day) = bar_day_start_ms(date) {
            self.now
                .fetch_max(day + self.arrival_offset_ms, Ordering::SeqCst);
        }
    }
}

/// A [`HostClock`] as the recorders read it.
struct RecorderClock(Arc<dyn HostClock>);

impl LiveClock for RecorderClock {
    fn now_unix_ms(&self) -> u64 {
        self.0.now_unix_ms()
    }
}

/// Starts entrant instances for a live window.
pub trait LiveHost {
    /// Whether this host can run anything at all.
    fn ready(&mut self) -> Result<(), String> {
        Ok(())
    }
    /// Refuse an image before the window opens.
    fn admit(&mut self, image: &str) -> Result<(), String>;
    /// One fresh instance of `image` for window-major run `run`. A host that
    /// attaches the model gateway wraps its provider transport in
    /// [`crate::live_capture::LoggingTransport`] over `recorder`, so every
    /// dispatched call lands in the log beside the bar it was made for.
    fn launch(
        &mut self,
        image: &str,
        run: usize,
        recorder: &SharedRecorder,
    ) -> Result<Box<dyn LaunchedEntrant>, String>;
}

/// Runs images under the hardened sandbox launch, a fresh container per run.
/// No model gateway is attached, since no provider adapter ships; an operator
/// binary with one implements [`LiveHost`] itself.
impl LiveHost for crate::DockerLauncher {
    fn ready(&mut self) -> Result<(), String> {
        if crate::docker_available() {
            Ok(())
        } else {
            Err("running a live window needs a running Docker daemon".to_string())
        }
    }

    fn admit(&mut self, image: &str) -> Result<(), String> {
        crate::EntrantLauncher::admit(self, image)
    }

    fn launch(
        &mut self,
        image: &str,
        _run: usize,
        _recorder: &SharedRecorder,
    ) -> Result<Box<dyn LaunchedEntrant>, String> {
        crate::EntrantLauncher::launch(self, image)
    }
}

/// Everything one invocation needs.
pub struct LiveWindowRun<'a> {
    pub arena_dir: &'a Path,
    pub spec: &'a LiveWindowSpec,
    pub entrants: &'a LiveEntrants,
    pub feed: &'a mut dyn LiveFeed,
    pub host: &'a mut dyn LiveHost,
    pub clock: Arc<dyn HostClock>,
    pub key: &'a SigningKey,
}

/// What became of one entrant.
#[derive(Clone, Debug, Serialize)]
pub struct LiveEntrantReport {
    pub agent_id: String,
    pub image: String,
    pub log: String,
    /// Transport faults, failed launches and failed finishes seen by this
    /// invocation, `run <n>: <fault>`. A faulted run held from that bar on.
    pub faults: Vec<String>,
    /// From the window's recorded root, when the log certified.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bars: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model_calls: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub restarts: Option<u64>,
}

/// The outcome of [`run_live_window`]: a published board that verified.
#[derive(Clone, Debug, Serialize)]
pub struct LiveWindowReport {
    pub window_id: String,
    pub bars: usize,
    pub entrants: Vec<LiveEntrantReport>,
    pub scored: usize,
    pub refusals: Vec<Refusal>,
    pub returns_provenance: BTreeMap<String, ReturnsProvenance>,
    pub certifying: bool,
    pub board: String,
    pub verifying_key: String,
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

impl LiveWindowSpec {
    fn config(&self) -> ScoreConfig {
        self.score_config.clone().unwrap_or_default()
    }

    /// The execution matrix the bar count fixes, as a log header names it.
    fn matrix(&self) -> Result<(Vec<Window>, Vec<u64>), String> {
        if self.schema != LIVE_WINDOW_SPEC_SCHEMA {
            return Err(format!(
                "live window spec schema `{}`, expected `{LIVE_WINDOW_SPEC_SCHEMA}`",
                self.schema
            ));
        }
        if self.commit_deadline > self.data_reveal_epoch {
            return Err(format!(
                "the commit deadline (epoch {}) is after the data reveal (epoch {})",
                self.commit_deadline, self.data_reveal_epoch
            ));
        }
        execution_matrix(self.bars, &self.config())
    }
}

/// The committed artifact digest of each entrant, checked.
fn entrant_digests(entrants: &LiveEntrants) -> Result<Vec<String>, String> {
    if entrants.schema != LIVE_ENTRANTS_SCHEMA {
        return Err(format!(
            "live entrants schema `{}`, expected `{LIVE_ENTRANTS_SCHEMA}`",
            entrants.schema
        ));
    }
    if entrants.entrants.is_empty() {
        return Err("the live entrants manifest names no entrant".to_string());
    }
    let mut seen = std::collections::BTreeSet::new();
    entrants
        .entrants
        .iter()
        .map(|entrant| {
            if !seen.insert(entrant.agent_id.as_str()) {
                return Err(format!("entrant `{}` is listed twice", entrant.agent_id));
            }
            if entrant.salt.is_empty() {
                return Err(format!("entrant `{}` has an empty salt", entrant.agent_id));
            }
            CapturedEntrant::of(&AgentTrajectory {
                agent_id: format!("sandbox:{}", entrant.image),
                contract: None,
                in_sample_trials: 0,
                declared_mandate: None,
                runs: Vec::new(),
            })
            .map(|captured| captured.artifact_sha256().to_string())
            .map_err(|error| format!("entrant `{}`: {error}", entrant.agent_id))
        })
        .collect()
}

/// The file an entrant's log lives in. Agent ids may hold `/` and `:`, so the
/// name is a digest prefix of the id; the header names the agent.
fn log_name(agent_id: &str) -> String {
    format!("log-{}.jsonl", &content_digest(agent_id.as_bytes())[..16])
}

/// Write `value` to `path` when it does not exist, or require that it holds
/// the same value: a live window's spec and field are fixed when it opens.
fn pin_json<T: Serialize + serde::de::DeserializeOwned>(
    path: &Path,
    value: &T,
    what: &str,
) -> Result<(), String> {
    let expected = serde_json::to_value(value).map_err(|e| e.to_string())?;
    match std::fs::read_to_string(path) {
        Ok(text) => {
            let stored: serde_json::Value = serde_json::from_str(&text)
                .map_err(|e| format!("{} is not valid JSON: {e}", path.display()))?;
            if stored != expected {
                return Err(format!(
                    "the {what} differs from the one this live window opened with ({})",
                    path.display()
                ));
            }
            Ok(())
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            let text = serde_json::to_string_pretty(value).map_err(|e| e.to_string())?;
            std::fs::write(path, text + "\n")
                .map_err(|e| format!("cannot write {}: {e}", path.display()))
        }
        Err(error) => Err(format!("cannot read {}: {error}", path.display())),
    }
}

/// Whether `bar` may follow `previous`, the bars so far starting at `first`:
/// a calendar date in the first bar's format, later than the previous one,
/// and a finite close for exactly the first bar's symbols.
fn check_bar(
    first: Option<&LiveBar>,
    previous: Option<&LiveBar>,
    bar: &LiveBar,
) -> Result<(), String> {
    let date = &bar.date;
    if bar_day_start_ms(date).is_none() {
        return Err(format!("bar date `{date}` is not a calendar date"));
    }
    if let Some(previous) = previous {
        if date.len() != previous.date.len() || *date <= previous.date {
            return Err(format!(
                "bar `{date}` does not follow bar `{}`",
                previous.date
            ));
        }
    }
    for (symbol, close) in &bar.closes {
        let clean =
            !symbol.is_empty() && symbol.trim() == symbol && !symbol.contains([',', '\n', '\r']);
        if !clean {
            return Err(format!("bar `{date}` names symbol {symbol:?}"));
        }
        if !close.is_finite() {
            return Err(format!(
                "bar `{date}` has a close for `{symbol}` that is not finite"
            ));
        }
    }
    let symbols = |b: &LiveBar| b.closes.keys().cloned().collect::<Vec<_>>();
    match first {
        Some(first) if symbols(first) != symbols(bar) => Err(format!(
            "bar `{date}` closes {:?}, the window's symbols are {:?}",
            symbols(bar),
            symbols(first)
        )),
        None if bar.closes.is_empty() => Err(format!("bar `{date}` has no close")),
        _ => Ok(()),
    }
}

/// The append-only bar journal.
struct FeedJournal {
    path: PathBuf,
    bars: Vec<LiveBar>,
}

impl FeedJournal {
    /// Read the journal, dropping a last line cut short by a crash: the bar
    /// it held was never handed to an entrant, because a bar is driven only
    /// after its line is on disk.
    fn open(path: &Path) -> Result<Self, String> {
        let text = match std::fs::read_to_string(path) {
            Ok(text) => text,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => String::new(),
            Err(error) => return Err(format!("cannot read {}: {error}", path.display())),
        };
        let complete = text.rfind('\n').map_or(0, |end| end + 1);
        if complete < text.len() {
            let file = std::fs::OpenOptions::new()
                .write(true)
                .open(path)
                .map_err(|e| format!("cannot open {}: {e}", path.display()))?;
            file.set_len(complete as u64)
                .and_then(|()| file.sync_all())
                .map_err(|e| {
                    format!("cannot drop the cut-short line of {}: {e}", path.display())
                })?;
        }
        let mut journal = Self {
            path: path.to_path_buf(),
            bars: Vec::new(),
        };
        // inv: journal.bars holds the checked bars of the lines read so far.
        for line in text[..complete].lines() {
            let bar: LiveBar = serde_json::from_str(line)
                .map_err(|e| format!("{} holds an unreadable bar: {e}", path.display()))?;
            check_bar(journal.bars.first(), journal.bars.last(), &bar)?;
            journal.bars.push(bar);
        }
        Ok(journal)
    }

    fn append(&mut self, bar: LiveBar) -> Result<(), String> {
        check_bar(self.bars.first(), self.bars.last(), &bar)?;
        let line = serde_json::to_string(&bar).map_err(|e| e.to_string())?;
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)
            .map_err(|e| format!("cannot open {}: {e}", self.path.display()))?;
        writeln!(file, "{line}")
            .and_then(|()| file.sync_data())
            .map_err(|e| format!("cannot append to {}: {e}", self.path.display()))?;
        self.bars.push(bar);
        Ok(())
    }

    /// The bars `0..=t` as the simulator reads them: what parsing the
    /// revealed dataset gives, cut at `t`.
    fn prefix(&self, t: usize) -> Dataset {
        let bars = &self.bars[..=t];
        Dataset {
            dates: bars.iter().map(|bar| bar.date.clone()).collect(),
            closes: bars[0]
                .closes
                .keys()
                .map(|symbol| {
                    let series = bars.iter().map(|bar| bar.closes[symbol]).collect();
                    (symbol.clone(), series)
                })
                .collect(),
            dividends: BTreeMap::new(),
            marks: None,
        }
    }

    /// The revealed dataset, `date,symbol,close`.
    fn csv(&self) -> String {
        let mut out = String::from("date,symbol,close\n");
        for bar in &self.bars {
            for (symbol, close) in &bar.closes {
                out.push_str(&format!("{},{symbol},{close}\n", bar.date));
            }
        }
        out
    }
}

/// Replays a run's logged decisions up to the bar being decided, then asks
/// the live entrant for that bar.
struct Stepper<'a> {
    logged: &'a [Decision],
    step: usize,
    live: &'a mut dyn FnMut(&MarketObservation) -> Decision,
}

impl Agent for Stepper<'_> {
    fn decide(&mut self, observation: &MarketObservation) -> Decision {
        let step = self.step;
        self.step += 1;
        match self.logged.get(step) {
            Some(decision) => decision.clone(),
            None => (self.live)(observation),
        }
    }
}

/// One run's instance while the window is live.
#[derive(Default)]
struct RunSlot {
    instance: Option<Box<dyn LaunchedEntrant>>,
    launched: bool,
    faulted: bool,
}

/// One entrant while the window is live.
struct LiveEntrantState {
    image: String,
    recorder: SharedRecorder,
    /// The logged decisions of each run, in step order.
    decisions: Vec<Vec<Decision>>,
    slots: Vec<RunSlot>,
    faults: Vec<String>,
}

/// Bars decided so far in each run, and the latest decision time per bar
/// index, from a log's entries.
fn read_progress(
    entries: &[crate::live_capture::LiveEntry],
    windows: &[(Window, u64)],
    latest: &mut BTreeMap<usize, u64>,
) -> Vec<Vec<Decision>> {
    let mut decisions = vec![Vec::new(); windows.len()];
    for entry in entries {
        if let LiveEvent::Decision { decision } = &entry.event {
            if let Some(runs) = decisions.get_mut(entry.run) {
                runs.push(decision.clone());
                let bar = windows[entry.run].0.start + entry.step;
                let at = latest.entry(bar).or_insert(0);
                *at = (*at).max(entry.at_unix_ms);
            }
        }
    }
    decisions
}

/// Open a live window, run it to its last bar, then seal, score, publish and
/// verify it. Safe to call again on the same directory after any failure: it
/// resumes where the record stops.
pub fn run_live_window(run: LiveWindowRun<'_>) -> Result<LiveWindowReport, String> {
    let LiveWindowRun {
        arena_dir,
        spec,
        entrants,
        feed,
        host,
        clock,
        key,
    } = run;
    let (windows, seeds) = spec.matrix()?;
    let digests = entrant_digests(entrants)?;
    host.ready()?;
    for entrant in &entrants.entrants {
        host.admit(&entrant.image)
            .map_err(|error| format!("entrant `{}`: {error}", entrant.agent_id))?;
    }

    let live_dir = arena_dir.join(LIVE_DIR).join(&spec.window_id);
    let mut arena = if arena_dir.join(crate::STATE_FILE).exists() {
        Arena::load(arena_dir)?
    } else {
        Arena::init(arena_dir)?
    };
    let id = spec.window_id.as_str();
    match arena.window(id) {
        None => arena.open_window_with_fault_plan(
            id,
            spec.commit_deadline,
            spec.data_reveal_epoch,
            spec.config(),
            None,
            spec.scorer_artifact_sha256.clone(),
            None,
        )?,
        Some(window) => {
            let same = window.commit_deadline == spec.commit_deadline
                && window.data_reveal_epoch == spec.data_reveal_epoch
                && window.scorer_artifact_sha256 == spec.scorer_artifact_sha256
                && window.fault_plan_sha256.is_none()
                && serde_json::to_value(&window.score_config).ok()
                    == serde_json::to_value(spec.config()).ok();
            if !same {
                return Err(format!(
                    "window `{id}` exists with another deadline, reveal epoch, scorer, config or fault plan than the live window spec"
                ));
            }
        }
    }
    std::fs::create_dir_all(&live_dir)
        .map_err(|e| format!("cannot create {}: {e}", live_dir.display()))?;
    pin_json(&live_dir.join("spec.json"), spec, "live window spec")?;
    pin_json(
        &live_dir.join("entrants.json"),
        entrants,
        "live entrants manifest",
    )?;

    let status = arena.window(id).map(|w| w.status);
    if matches!(status, Some(WindowStatus::Open)) {
        for (entrant, digest) in entrants.entrants.iter().zip(&digests) {
            let expected = make_commitment(&entrant.agent_id, id, digest, &entrant.salt);
            let registered = arena
                .window(id)
                .and_then(|w| {
                    w.commitments
                        .iter()
                        .find(|c| c.agent_id == entrant.agent_id)
                })
                .map(|c| c.commit_hash.clone());
            let Some(hash) = registered else {
                arena.register_entry(id, expected)?;
                continue;
            };
            if hash != expected.commit_hash {
                let agent = &entrant.agent_id;
                return Err(format!(
                    "window `{id}` holds another commitment for `{agent}` than its image and salt make"
                ));
            }
        }
        let epoch = arena.current_epoch().max(spec.commit_deadline);
        arena.advance(epoch)?;
    }
    let status = arena.window(id).map(|w| w.status);
    let ids: std::collections::BTreeSet<&str> = entrants
        .entrants
        .iter()
        .map(|e| e.agent_id.as_str())
        .collect();
    let committed: std::collections::BTreeSet<&str> = arena
        .window(id)
        .map(|w| w.commitments.iter().map(|c| c.agent_id.as_str()).collect())
        .unwrap_or_default();
    if committed != ids {
        return Err(format!(
            "window `{id}` commits {committed:?}, the live entrants manifest lists {ids:?}"
        ));
    }

    let mut faults = vec![Vec::new(); entrants.entrants.len()];
    if status == Some(WindowStatus::Committed) {
        let header_windows: Vec<LiveWindow> = windows
            .iter()
            .map(|w| LiveWindow {
                start: w.start,
                end: w.end,
            })
            .collect();
        let runs: Vec<(Window, u64)> = windows
            .iter()
            .flat_map(|w| seeds.iter().map(move |seed| (*w, *seed)))
            .collect();
        let mut journal = FeedJournal::open(&live_dir.join("feed.jsonl"))?;
        if journal.bars.len() > spec.bars {
            return Err(format!(
                "the feed journal holds {} bars, the window runs {}",
                journal.bars.len(),
                spec.bars
            ));
        }
        if let Some(last) = journal.bars.last() {
            clock.bar_arrived(&last.date);
        }
        let mut latest: BTreeMap<usize, u64> = BTreeMap::new();
        let mut states = Vec::new();
        for entrant in &entrants.entrants {
            let header = LiveLogHeader {
                schema: LIVE_LOG_SCHEMA.to_string(),
                window_id: id.to_string(),
                agent_id: entrant.agent_id.clone(),
                image: entrant.image.clone(),
                runner_artifact_sha256: spec.scorer_artifact_sha256.clone(),
                windows: header_windows.clone(),
                seeds: seeds.clone(),
            };
            let log = live_dir.join(log_name(&entrant.agent_id));
            let recorder_clock = Box::new(RecorderClock(Arc::clone(&clock)));
            let (logged_header, logged) = if log.exists() {
                LiveRecorder::read(&log)?
            } else {
                (header.clone(), Vec::new())
            };
            if logged_header != header {
                return Err(format!(
                    "live log {} has another header than this window and entrant",
                    log.display()
                ));
            }
            let decisions = read_progress(&logged, &runs, &mut latest);
            // A run has decided every stored bar of its window, or every one
            // but the last, which was open when the host stopped.
            let stored = journal.bars.len();
            for (r, (window, _)) in runs.iter().enumerate() {
                let upto = |n: usize| n.clamp(window.start, window.end) - window.start;
                let decided = decisions[r].len();
                if decided != upto(stored) && decided != upto(stored.saturating_sub(1)) {
                    return Err(format!(
                        "live log {} decided {decided} bars of run {r}, which the feed journal's {stored} bars do not explain",
                        log.display()
                    ));
                }
            }
            // Checked before the restart is recorded, so a refused resume
            // leaves the log as it was.
            let recorder = if log.exists() {
                LiveRecorder::resume(&log, recorder_clock)?
            } else {
                LiveRecorder::create(&log, header, recorder_clock)?
            };
            states.push(LiveEntrantState {
                image: entrant.image.clone(),
                recorder: Arc::new(Mutex::new(recorder)),
                decisions,
                slots: runs.iter().map(|_| RunSlot::default()).collect(),
                faults: Vec::new(),
            });
        }

        let costs = CostModel::default();
        // inv: every bar before t is in the journal and decided in every run
        // whose window holds it, in every entrant's log.
        for t in 0..spec.bars {
            if t == journal.bars.len() {
                let bar = feed.bar(t)?.ok_or_else(|| {
                    format!(
                        "the feed ended at bar {t}, before the window's {} bars",
                        spec.bars
                    )
                })?;
                journal.append(bar)?;
            }
            let date = journal.bars[t].date.clone();
            clock.bar_arrived(&date);
            let pending = |state: &LiveEntrantState, r: usize| {
                let (window, _) = runs[r];
                (window.start..window.end).contains(&t)
                    && state.decisions[r].len() == t - window.start
            };
            let any_pending = states
                .iter()
                .any(|state| (0..runs.len()).any(|r| pending(state, r)));
            if !any_pending {
                continue;
            }
            let day = bar_day_start_ms(&date).expect("journal bars carry calendar dates");
            let now = clock.now_unix_ms();
            if now + MAX_CLOCK_SKEW_MS < day {
                return Err(format!(
                    "bar `{date}` arrived at host time {now}, before its UTC day began at {day}: the host clock or the feed is wrong"
                ));
            }
            if let Some(&decided_at) = t.checked_sub(1).and_then(|p| latest.get(&p)) {
                if decided_at >= day + MAX_CLOCK_SKEW_MS {
                    return Err(format!(
                        "bar {} was decided at host time {decided_at}, after bar `{date}` began at {day}: no live log of this window can certify",
                        t - 1
                    ));
                }
            }
            let prefix = journal.prefix(t);
            for state in states.iter_mut() {
                for (r, &(window, seed)) in runs.iter().enumerate() {
                    if !pending(state, r) {
                        continue;
                    }
                    let step = t - window.start;
                    let slot = &mut state.slots[r];
                    if !slot.launched {
                        slot.launched = true;
                        match host.launch(&state.image, r, &state.recorder) {
                            Ok(instance) => slot.instance = Some(instance),
                            Err(error) => {
                                slot.faulted = true;
                                state
                                    .faults
                                    .push(format!("run {r}: launch failed: {error}"));
                            }
                        }
                    }
                    let recorder = Arc::clone(&state.recorder);
                    let instance = &mut slot.instance;
                    let mut decided = None;
                    let mut live = |observation: &MarketObservation| {
                        lock(&recorder).record_observation(r, step, observation);
                        let decision = match instance.as_mut() {
                            Some(instance) => instance.decide(observation),
                            None => HoldAgent.decide(observation),
                        };
                        lock(&recorder).record_decision(r, step, &decision);
                        decided = Some(decision.clone());
                        decision
                    };
                    let mut stepper = Stepper {
                        logged: &state.decisions[r],
                        step: 0,
                        live: &mut live,
                    };
                    run_backtest(&prefix, &mut stepper, window, seed, costs);
                    let decision = decided.expect("the bar's step reaches the live entrant");
                    let guard = lock(&state.recorder);
                    if let Some(failure) = guard.failure() {
                        return Err(failure.to_string());
                    }
                    let at = guard.entries().last().map_or(0, |entry| entry.at_unix_ms);
                    drop(guard);
                    let latest_at = latest.entry(t).or_insert(0);
                    *latest_at = (*latest_at).max(at);
                    state.decisions[r].push(decision);
                    if let Some(fault) = slot.instance.as_ref().and_then(|i| i.fault()) {
                        if !slot.faulted {
                            slot.faulted = true;
                            state.faults.push(format!("run {r}: {fault}"));
                        }
                    }
                    if t + 1 == window.end {
                        if let Some(Err(error)) = slot.instance.take().map(|i| i.finish()) {
                            state.faults.push(format!("run {r}: {error}"));
                        }
                    }
                }
            }
        }

        // Close: seal, reveal, score.
        let csv = journal.csv();
        let replay = ReplayWindow::parse(csv.as_bytes(), &spec.config())?;
        let full = journal.prefix(spec.bars - 1);
        if (&replay.data.dates, &replay.data.closes) != (&full.dates, &full.closes) {
            return Err(
                "the revealed dataset does not parse back to the bars the entrants were shown"
                    .to_string(),
            );
        }
        let dataset = live_dir.join("dataset.csv");
        std::fs::write(&dataset, &csv)
            .map_err(|e| format!("cannot write {}: {e}", dataset.display()))?;
        let mut revealed = Vec::new();
        for ((entrant, digest), state) in entrants.entrants.iter().zip(&digests).zip(&states) {
            let sealed = lock(&state.recorder).seal(key)?;
            revealed.push(RevealedEntry {
                agent_id: Some(entrant.agent_id.clone()),
                submission: None,
                capture: None,
                artifact_digest: digest.clone(),
                salt: entrant.salt.clone(),
                fault_plan_sha256: None,
                live_log: Some(sealed),
            });
        }
        let entries_path = live_dir.join("entries.json");
        let text = serde_json::to_string(&revealed).map_err(|e| e.to_string())?;
        std::fs::write(&entries_path, text + "\n")
            .map_err(|e| format!("cannot write {}: {e}", entries_path.display()))?;
        for (slot_faults, state) in faults.iter_mut().zip(states) {
            *slot_faults = state.faults;
        }
        let mut arena = Arena::load(arena_dir)?;
        let epoch = arena.current_epoch().max(spec.data_reveal_epoch);
        arena.advance(epoch)?;
        arena.reveal_and_score(id, &dataset, &revealed)?;
    }

    let mut arena = Arena::load(arena_dir)?;
    if arena.window(id).map(|w| w.status) == Some(WindowStatus::Scoring) {
        arena.publish(id, key)?;
    }
    let window = arena
        .window(id)
        .ok_or_else(|| format!("no such window `{id}`"))?;
    if window.status != WindowStatus::Published {
        return Err(format!(
            "window `{id}` is {:?}, not published; a live window runs while it is committed",
            window.status
        ));
    }
    let verifying_key = key.verifying_key();
    let verification = verify_arena(arena_dir, Some(&verifying_key))?;
    if !verification.ok {
        return Err(format!(
            "the published arena does not verify: {:?}",
            verification.windows
        ));
    }
    let entrant_reports = entrants
        .entrants
        .iter()
        .zip(faults)
        .map(|(entrant, faults)| {
            let root = window.live_capture.get(&entrant.agent_id);
            LiveEntrantReport {
                agent_id: entrant.agent_id.clone(),
                image: entrant.image.clone(),
                log: live_dir
                    .join(log_name(&entrant.agent_id))
                    .display()
                    .to_string(),
                faults,
                bars: root.map(|r| r.bars),
                model_calls: root.map(|r| r.model_calls),
                restarts: root.map(|r| r.restarts),
            }
        })
        .collect();
    Ok(LiveWindowReport {
        window_id: id.to_string(),
        bars: spec.bars,
        entrants: entrant_reports,
        scored: window.scores.len(),
        refusals: window.refusals.clone(),
        returns_provenance: window.returns_provenance.clone(),
        certifying: window.certifying == Some(true),
        board: arena_dir
            .join(WINDOWS_DIR)
            .join(id)
            .join(crate::BOARD_FILE)
            .display()
            .to_string(),
        verifying_key: verifying_key.to_hex(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const DAY_MS: u64 = 86_400_000;

    fn bar(date: &str, closes: &[(&str, f64)]) -> LiveBar {
        LiveBar {
            date: date.to_string(),
            closes: closes.iter().map(|(s, c)| (s.to_string(), *c)).collect(),
        }
    }

    #[test]
    fn a_bar_follows_only_a_complete_earlier_bar_of_the_same_symbols() {
        let first = bar("2025-01-02", &[("A", 1.0), ("B", 2.0)]);
        let ok = bar("2025-01-03", &[("A", 1.5), ("B", 2.5)]);
        assert_eq!(check_bar(None, None, &first), Ok(()));
        assert_eq!(check_bar(Some(&first), Some(&first), &ok), Ok(()));
        let refused = [
            ("not a date", bar("2025-13-01", &[("A", 1.0), ("B", 2.0)])),
            ("same day", bar("2025-01-02", &[("A", 1.0), ("B", 2.0)])),
            ("earlier", bar("2025-01-01", &[("A", 1.0), ("B", 2.0)])),
            ("other format", bar("2025-004", &[("A", 1.0), ("B", 2.0)])),
            ("missing symbol", bar("2025-01-03", &[("A", 1.0)])),
            (
                "extra symbol",
                bar("2025-01-03", &[("A", 1.0), ("B", 2.0), ("C", 1.0)]),
            ),
            ("nan", bar("2025-01-03", &[("A", f64::NAN), ("B", 2.0)])),
            (
                "inf",
                bar("2025-01-03", &[("A", f64::INFINITY), ("B", 2.0)]),
            ),
        ];
        for (case, candidate) in refused {
            assert!(
                check_bar(Some(&first), Some(&first), &candidate).is_err(),
                "{case}"
            );
        }
        assert!(check_bar(None, None, &bar("2025-01-02", &[])).is_err());
        // A first bar has no symbol set to differ from, so each symbol rule
        // stands alone here.
        for symbol in ["A,", "A\n", "A\r", " A", "A ", ""] {
            let candidate = bar("2025-01-02", &[(symbol, 1.0)]);
            assert!(check_bar(None, None, &candidate).is_err(), "{symbol:?}");
        }
    }

    #[test]
    fn the_wall_clock_reads_the_host_time() {
        let before = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64;
        let read = HostClock::now_unix_ms(&SystemClock);
        assert!(
            read >= before && read < before + 60_000,
            "{read} vs {before}"
        );
    }

    #[test]
    fn the_sandbox_host_is_ready_only_with_docker_and_admits_only_pinned_images() {
        use crate::DockerLauncher;
        assert_eq!(
            LiveHost::ready(&mut DockerLauncher).is_ok(),
            crate::docker_available()
        );
        assert!(LiveHost::admit(&mut DockerLauncher, "test/x:latest").is_err());
    }

    #[test]
    fn a_spec_may_commit_and_reveal_at_the_same_epoch() {
        let spec = LiveWindowSpec {
            schema: LIVE_WINDOW_SPEC_SCHEMA.to_string(),
            window_id: "w".to_string(),
            commit_deadline: 5,
            data_reveal_epoch: 5,
            bars: 40,
            scorer_artifact_sha256: "0".repeat(64),
            score_config: None,
        };
        assert!(spec.matrix().is_ok());
    }

    #[test]
    fn a_path_that_cannot_be_read_is_an_error_not_a_fresh_file() {
        let dir =
            std::env::temp_dir().join(format!("sharpebench-live-dir-unit-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let error = pin_json(&dir, &1u8, "spec").unwrap_err();
        assert!(error.contains("cannot read"), "{error}");
        let error = FeedJournal::open(&dir).err().unwrap();
        assert!(error.contains("cannot read"), "{error}");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn progress_is_read_per_run_and_timed_per_bar_index() {
        use crate::live_capture::LiveEntry;
        let runs = [
            (Window { start: 10, end: 20 }, 0),
            (Window { start: 20, end: 30 }, 0),
        ];
        let hold = Decision {
            orders: Vec::new(),
            reasoning: String::new(),
            cost: None,
        };
        let entry = |run, step, at, event| LiveEntry {
            seq: 0,
            prev_sha256: String::new(),
            at_unix_ms: at,
            run,
            step,
            event,
        };
        let decided = |run, step, at| {
            entry(
                run,
                step,
                at,
                LiveEvent::Decision {
                    decision: hold.clone(),
                },
            )
        };
        let entries = [
            decided(0, 0, 5),
            decided(0, 1, 9),
            entry(0, 2, 11, LiveEvent::Restart),
            decided(1, 1, 7),
            decided(1, 2, 3),
        ];
        let mut latest = BTreeMap::from([(21, 8)]);
        let decisions = read_progress(&entries, &runs, &mut latest);
        assert_eq!(decisions.iter().map(Vec::len).collect::<Vec<_>>(), [2, 2]);
        assert_eq!(latest, BTreeMap::from([(10, 5), (11, 9), (21, 8), (22, 3)]));
    }

    #[test]
    fn a_simulated_clock_moves_to_each_bar_and_never_back() {
        let clock = SimulatedClock::new(1_000);
        clock.bar_arrived("1970-01-02");
        assert_eq!(clock.now_unix_ms(), DAY_MS + 1_000);
        assert_eq!(clock.now_unix_ms(), DAY_MS + 1_001);
        clock.bar_arrived("1970-01-01");
        assert_eq!(clock.now_unix_ms(), DAY_MS + 1_002);
        clock.bar_arrived("not a date");
        assert_eq!(clock.now_unix_ms(), DAY_MS + 1_003);
    }

    #[test]
    fn a_feed_journal_drops_a_cut_short_line_and_keeps_the_rest() {
        let path = std::env::temp_dir().join(format!(
            "sharpebench-live-feed-unit-{}.jsonl",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&path);
        let mut journal = FeedJournal::open(&path).unwrap();
        journal.append(bar("2025-01-02", &[("A", 1.0)])).unwrap();
        journal
            .append(bar("2025-01-03", &[("A", 0.1 + 0.2)]))
            .unwrap();
        assert!(journal.append(bar("2025-01-03", &[("A", 1.0)])).is_err());
        let mut file = std::fs::OpenOptions::new()
            .append(true)
            .open(&path)
            .unwrap();
        write!(file, "{{\"date\":\"2025-01-0").unwrap();
        drop(file);
        let reopened = FeedJournal::open(&path).unwrap();
        assert_eq!(reopened.bars, journal.bars);
        assert!(std::fs::read_to_string(&path).unwrap().ends_with("}\n"));
        assert_eq!(
            reopened.csv(),
            "date,symbol,close\n2025-01-02,A,1\n2025-01-03,A,0.30000000000000004\n"
        );
        let parsed = Dataset::from_csv(&reopened.csv()).unwrap();
        let prefix = reopened.prefix(1);
        assert_eq!((parsed.dates, parsed.closes), (prefix.dates, prefix.closes));
        assert_eq!(reopened.prefix(0).dates, ["2025-01-02"]);
        std::fs::write(&path, "{\"date\":\"2025-01-02\",\"closes\":{}}\n").unwrap();
        assert!(FeedJournal::open(&path).is_err());
        std::fs::remove_file(&path).unwrap();
    }
}
