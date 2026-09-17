//! Reveal intake: which returns a forward window ranks, and where they come
//! from.
//!
//! Entries arrive after the data-reveal epoch. Returns an entrant supplies at
//! that point can be computed with hindsight over the revealed window, and no
//! statistic separates them from skill: a planted next-bar oracle clears
//! deflation (Gençay, "What survives honest evaluation?", arXiv 2608.27734,
//! p. 7). The arena therefore does not rank supplied returns unless the operator
//! opts into [`IntakeOptions::allow_supplied_returns`], which marks the window
//! noncertifying. An entry reveals a strict trajectory capture instead, and the
//! arena derives its returns:
//!
//! 1. the capture names its entrant artifact ([`CapturedEntrant`]), which must
//!    equal the `artifact_digest` committed before the deadline;
//! 2. its contract names the revealed dataset ([`ReplayWindow::dataset_sha256`],
//!    recorded in the window as `replay_dataset_sha256`) and the window's
//!    execution matrix ([`execution_matrix`]);
//! 3. [`sharpebench_harness::verify_trajectory_strict`] accepts it with the
//!    window's frozen scorer as the runner, and the scored submission is
//!    [`sharpebench_sim::replay_submission`] of it.
//!
//! Replay establishes that the scored returns follow from the recorded
//! decisions on the revealed data, under the committed artifact identity. It
//! does not establish that running that artifact produced those decisions:
//! anyone holding the revealed data can write a capture of hindsight decisions
//! that names the committed artifact and replays exactly. Re-execution
//! ([`IntakeOptions::reexecute`]) checks that part. The committed entrant runs
//! again on the revealed data, one fresh instance per run, and a capture whose
//! score-bearing decisions it does not repeat is refused. The instance sees only
//! the point-in-time observations the harness gives it, so hindsight would have
//! to be inside the artifact, fixed before the deadline. That rests on the
//! operator's custody of the data, which no file here can prove.
//!
//! A board certifies only when every ranked row was re-executed and supplied
//! returns were not accepted ([`board_certifies`]). Intake computes that from
//! the rows ([`Admission::certifying`]); no caller sets it. A replayed or
//! supplied row therefore puts its window on a noncertifying board.
//!
//! Every refusal is recorded, like a failed reveal: against the entry's agent,
//! or against `(unnamed entry <index>)` for an entry that names none.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::rc::Rc;

use serde::{Deserialize, Serialize};

use sharpebench_attest::content_digest;
use sharpebench_attest::registry::Registry;
use sharpebench_core::{AgentSubmission, ScoreConfig};
use sharpebench_harness::{
    trajectory_contract, transport_failure, verify_trajectory_reexecuted, verify_trajectory_strict,
    ReexecutionError,
};
use sharpebench_protocol::{AgentTrajectory, Decision, MarketObservation};
use sharpebench_sim::{
    replay_submission, Agent, BuyAndHold, CostModel, Dataset, HoldAgent, Momentum,
    TransportDiagnostics, Window,
};

use crate::{
    describe_fault_plan, registry_for, validate_sha256, Refusal, RevealedEntry, WindowState,
};

/// How the returns behind one scored arena row were obtained. Rank-neutral:
/// it changes no score and no ordering. It is recorded in the window and
/// published on the row.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ReturnsProvenance {
    /// Supplied by the entrant after the reveal and ranked as given. Only under
    /// [`IntakeOptions::allow_supplied_returns`], which marks the window
    /// noncertifying.
    Supplied,
    /// Derived by strict replay of the entry's capture. The returns follow from
    /// the recorded decisions; nothing re-derived the decisions.
    Replayed,
    /// Replayed, and the committed entrant, run again on the revealed data,
    /// repeated every score-bearing decision the capture records.
    ReExecuted,
}

impl ReturnsProvenance {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Supplied => "supplied",
            Self::Replayed => "replayed",
            Self::ReExecuted => "re-executed",
        }
    }
}

/// Reference agents compiled into the scorer binary, by the name a
/// `sharpebench capture <name>` trajectory carries.
const REFERENCE_ENTRANTS: [&str; 2] = ["buy-and-hold", "momentum"];

/// The artifact digest an entrant commits to for the reference agent `name`
/// compiled into the runner `runner_sha256`. It binds the name as well as the
/// runner, so one commitment admits one reference agent: an entrant cannot
/// commit to the runner and choose between the agents after the reveal. The
/// pre-image is `sharpebench-arena/reference-entrant/v1`, a newline, `name`, a
/// newline and `runner_sha256`.
pub fn reference_entrant_digest(name: &str, runner_sha256: &str) -> String {
    content_digest(
        format!("sharpebench-arena/reference-entrant/v1\n{name}\n{runner_sha256}").as_bytes(),
    )
}

/// Whether `name` is a reference agent compiled into the scorer.
pub fn is_reference_entrant(name: &str) -> bool {
    REFERENCE_ENTRANTS.contains(&name)
}

/// The entrant artifact a capture says it recorded, read from the capture's
/// `agent_id` as the capture commands write it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CapturedEntrant {
    /// `sandbox:<repository>@sha256:<digest>`, as `sharpebench capture --image`
    /// names its entrant. The artifact is the image digest: the 64 lowercase hex
    /// characters after `@sha256:`.
    Image { reference: String, digest: String },
    /// `buy-and-hold` or `momentum`, as `sharpebench capture <name>` names a
    /// reference agent compiled into the runner the capture's contract records.
    /// The artifact is [`reference_entrant_digest`] of the name and that runner.
    Reference {
        name: String,
        runner_sha256: String,
        artifact_sha256: String,
    },
}

impl CapturedEntrant {
    /// Read the entrant a capture names. A `cmd:` or `http:` capture, or any
    /// other name, identifies no artifact and is refused: a command line or an
    /// address does not identify the bytes behind it. An image reference must
    /// be one the sandbox would launch, so an option-like repository such as
    /// `--privileged@sha256:<digest>` is refused here and never ranked.
    pub fn of(capture: &AgentTrajectory) -> Result<Self, String> {
        let id = capture.agent_id.as_str();
        if let Some(reference) = id.strip_prefix("sandbox:") {
            let digest = reference
                .rsplit_once("@sha256:")
                .filter(|(repository, digest)| {
                    !repository.is_empty() && validate_sha256("image", digest).is_ok()
                })
                .map(|(_, digest)| digest.to_string())
                .ok_or_else(|| {
                    format!(
                        "capture entrant `{id}` is not a digest-pinned image (sandbox:<repository>@sha256:<64 lowercase hex>)"
                    )
                })?;
            crate::resolve_launch(true, reference, &crate::SandboxOptions::default()).map_err(
                |error| format!("capture entrant `{id}` is not a launchable image: {error}"),
            )?;
            return Ok(Self::Image {
                reference: reference.to_string(),
                digest,
            });
        }
        if is_reference_entrant(id) {
            let runner_sha256 = capture
                .contract
                .as_ref()
                .and_then(|contract| contract.runner_artifact_sha256.clone())
                .ok_or_else(|| {
                    format!("reference capture `{id}` records no runner artifact, and the runner is part of the artifact it names")
                })?;
            return Ok(Self::Reference {
                artifact_sha256: reference_entrant_digest(id, &runner_sha256),
                name: id.to_string(),
                runner_sha256,
            });
        }
        Err(format!(
            "capture entrant `{id}` names no artifact: a commitment binds only a digest-pinned image (sandbox:<repository>@sha256:<digest>) or a reference agent compiled into the runner"
        ))
    }

    /// The artifact digest a commitment to this entrant must carry.
    pub fn artifact_sha256(&self) -> &str {
        match self {
            Self::Image { digest, .. } => digest,
            Self::Reference {
                artifact_sha256, ..
            } => artifact_sha256,
        }
    }
}

/// The execution matrix a forward window scores over a revealed dataset of
/// `len` bars: the two market windows `sharpebench capture --data <dataset>`
/// runs, `[w, m)` and `[m, len)` with `w = clamp(len / 10, 10, 30)` and
/// `m = (w + len) / 2`, and the execution seeds `0..k` for the frozen config's
/// `execution_seeds_per_window` `k`. A capture over any other matrix is
/// refused, so no entrant chooses which part of the revealed data it is scored
/// on.
pub fn execution_matrix(
    len: usize,
    config: &ScoreConfig,
) -> Result<(Vec<Window>, Vec<u64>), String> {
    if len < 40 {
        return Err(format!(
            "the revealed dataset has {len} bars; replay needs at least 40"
        ));
    }
    let warm = (len / 10).clamp(10, 30);
    let mid = (warm + len) / 2;
    let seeds = (0..config.execution_seeds_per_window as u64).collect();
    Ok((
        vec![
            Window {
                start: warm,
                end: mid,
            },
            Window {
                start: mid,
                end: len,
            },
        ],
        seeds,
    ))
}

/// The revealed dataset as the simulator replays it, with the matrix every
/// capture must run.
pub struct ReplayWindow {
    pub data: Dataset,
    pub costs: CostModel,
    /// The dataset identity a capture's contract names
    /// (`TrajectoryContract::dataset_sha256`): the harness digest of the parsed
    /// dataset, not of its file bytes.
    pub dataset_sha256: String,
    pub windows: Vec<Window>,
    pub seeds: Vec<u64>,
}

impl ReplayWindow {
    /// Parse revealed dataset bytes as `--data` reads a CSV, under the default
    /// cost model `sharpebench capture` records.
    pub fn parse(dataset_bytes: &[u8], config: &ScoreConfig) -> Result<Self, String> {
        let text = std::str::from_utf8(dataset_bytes)
            .map_err(|e| format!("the revealed dataset is not UTF-8 CSV, so no capture can be replayed against it: {e}"))?;
        let data = Dataset::from_csv(text)
            .map_err(|e| format!("the revealed dataset cannot be replayed: {e}"))?;
        let costs = CostModel::default();
        let (windows, seeds) = execution_matrix(data.len(), config)?;
        let dataset_sha256 = trajectory_contract(&data, costs, &[], &[]).dataset_sha256;
        Ok(Self {
            data,
            costs,
            dataset_sha256,
            windows,
            seeds,
        })
    }
}

/// Starts committed image entrants for re-execution. The production
/// implementation is [`DockerLauncher`]; tests substitute an in-process one.
pub trait EntrantLauncher {
    /// Whether this launcher can run anything at all. Checked once, before any
    /// entry is recorded, when a capture names an image: an operator
    /// environment that cannot re-execute is an error, not an entrant's refusal.
    fn ready(&mut self) -> Result<(), String> {
        Ok(())
    }
    /// Refuse an image before any instance starts, for example one that is not
    /// available to the scorer. Recorded as the entry's refusal.
    fn admit(&mut self, image: &str) -> Result<(), String>;
    /// One fresh instance for one re-executed run.
    fn launch(&mut self, image: &str) -> Result<Box<dyn LaunchedEntrant>, String>;
}

/// A launched image entrant: a transport while it runs, then a verdict.
pub trait LaunchedEntrant: Agent {
    /// The first transport or protocol fault seen so far. A faulted transport
    /// answers with holds, which are not the entrant's decisions.
    fn fault(&self) -> Option<String>;
    /// End the instance. An error, a resource-budget breach included, makes the
    /// re-execution a refusal.
    fn finish(self: Box<Self>) -> Result<(), String>;
}

/// Re-executes committed images under the hardened launch `sharpebench run
/// --image` uses: `resolve_launch` and `require_local_image` before anything
/// starts, then a fresh `run_external_sandboxed` container per run with default
/// options, so there is no host fallback and no unpinned reference.
pub struct DockerLauncher;

impl EntrantLauncher for DockerLauncher {
    fn ready(&mut self) -> Result<(), String> {
        if crate::docker_available() {
            Ok(())
        } else {
            Err("re-executing an image entrant needs a running Docker daemon; score with --replay-only to publish a noncertifying board without re-execution".to_string())
        }
    }

    fn admit(&mut self, image: &str) -> Result<(), String> {
        crate::resolve_launch(
            crate::docker_available(),
            image,
            &crate::SandboxOptions::default(),
        )
        .and_then(|_| crate::require_local_image(image))
        .map_err(|error| error.to_string())
    }

    fn launch(&mut self, image: &str) -> Result<Box<dyn LaunchedEntrant>, String> {
        crate::run_external_sandboxed(image, &crate::SandboxOptions::default())
            .map(|agent| Box::new(agent) as Box<dyn LaunchedEntrant>)
            .map_err(|error| error.to_string())
    }
}

impl LaunchedEntrant for crate::SandboxedAgent {
    fn fault(&self) -> Option<String> {
        transport_failure(self.health()).map(|kind| format!("{kind:?}"))
    }

    fn finish(self: Box<Self>) -> Result<(), String> {
        match crate::SandboxedAgent::finish(*self) {
            Ok(Some(true)) => Err("the container breached its memory budget".to_string()),
            Ok(_) => Ok(()),
            Err(error) => Err(error.to_string()),
        }
    }
}

type FailureCell = Rc<RefCell<Option<String>>>;

/// One re-executed run of a launched image. The first fault or failed finish
/// is recorded, and from then on the run holds without asking the instance:
/// the outcome is already a refusal.
struct WatchedRun {
    inner: Option<Box<dyn LaunchedEntrant>>,
    failure: FailureCell,
}

impl Agent for WatchedRun {
    fn decide(&mut self, observation: &MarketObservation) -> Decision {
        if self.failure.borrow().is_some() {
            return HoldAgent.decide(observation);
        }
        let inner = self
            .inner
            .as_mut()
            .expect("the instance is present until the run is dropped");
        let decision = inner.decide(observation);
        if let Some(fault) = inner.fault() {
            self.failure.borrow_mut().get_or_insert(fault);
        }
        decision
    }
}

impl Drop for WatchedRun {
    fn drop(&mut self) {
        if let Some(inner) = self.inner.take() {
            if let Err(error) = inner.finish() {
                self.failure.borrow_mut().get_or_insert(error);
            }
        }
    }
}

/// How [`crate::Arena::reveal_and_score_with`] admits entries. The default
/// ranks replayed captures and re-executes nothing, so a window scored with it
/// is noncertifying whenever it ranks a row ([`board_certifies`]).
#[derive(Default)]
pub struct IntakeOptions<'a> {
    /// Also rank returns an entrant supplied after the reveal, as given. The
    /// window is recorded, signed and rendered as noncertifying.
    pub allow_supplied_returns: bool,
    /// Re-execute the entrant every capture names: a reference agent in
    /// process, an image through this launcher. A row that passes is
    /// [`ReturnsProvenance::ReExecuted`].
    pub reexecute: Option<&'a mut dyn EntrantLauncher>,
}

/// What intake admitted from one set of reveals.
#[derive(Clone, Debug)]
pub struct Admission {
    pub field: Vec<AgentSubmission>,
    pub returns_provenance: BTreeMap<String, ReturnsProvenance>,
    pub refusals: Vec<Refusal>,
    /// SHA-256 of the revealed dataset bytes.
    pub dataset_hash: String,
    /// The parsed dataset's identity, when an entry carried a capture.
    pub replay_dataset_sha256: Option<String>,
    /// [`board_certifies`] for this admission, computed from its rows.
    pub certifying: bool,
}

/// Whether a board certifies its rows: supplied returns were not accepted, and
/// every ranked row was re-executed. A replayed row can hold hindsight
/// decisions, and a supplied row holds whatever the entrant sent, so either one
/// makes the board noncertifying. An empty field meets the rule vacuously.
pub fn board_certifies(
    returns_provenance: &BTreeMap<String, ReturnsProvenance>,
    supplied_returns_accepted: bool,
) -> bool {
    !supplied_returns_accepted
        && returns_provenance
            .values()
            .all(|provenance| *provenance == ReturnsProvenance::ReExecuted)
}

/// Refusal reason for supplied returns under the default intake.
pub const SUPPLIED_RETURNS_REFUSAL: &str = "supplied returns are not ranked: returns revealed after the data can be computed with hindsight and are not bound to the committed artifact; reveal a strict capture, or score with the noncertifying supplied-returns intake (arena score --allow-supplied-returns)";

/// Refusal reason for an entry that names no agent.
pub const UNNAMED_ENTRY_REFUSAL: &str =
    "the entry names no agent: an entry that reveals a capture must set `agent_id`";

/// The name a refusal records for entry `index` when the entry names no agent.
pub fn unnamed_entry(index: usize) -> String {
    format!("(unnamed entry {index})")
}

/// The window's fault plan on every entry. A mismatch is an error for the
/// whole call, before anything is recorded, as a config mismatch is.
pub(crate) fn check_entries(window: &WindowState, entries: &[RevealedEntry]) -> Result<(), String> {
    for (index, entry) in entries.iter().enumerate() {
        // A submission produced under another fault plan, or under none when
        // the window has one (or the reverse), is not the same experiment.
        if entry.fault_plan_sha256 != window.fault_plan_sha256 {
            let name = entry
                .committed_agent_id()
                .map_or_else(|| unnamed_entry(index), str::to_string);
            return Err(format!(
                "entry `{name}` was produced under {}, but window `{}` is scored under {}",
                describe_fault_plan(entry.fault_plan_sha256.as_deref()),
                window.id,
                describe_fault_plan(window.fault_plan_sha256.as_deref())
            ));
        }
    }
    Ok(())
}

type Admissible = (AgentSubmission, ReturnsProvenance);

/// Admit revealed entries into a window's field without touching the disk.
/// Each entry's commitment is revealed through the attest registry at
/// `current_epoch` (which enforces the data-reveal lock), then its returns are
/// taken by the rules in the module docs. An `Err` records nothing; a refused
/// entry is an [`Admission::refusals`] row.
///
/// Every entry is judged on its own first, so an entry that does not open its
/// agent's commitment is refused alone and cannot take an honest reveal down
/// with it. One commitment then admits one entry: admissible copies that agree
/// are ranked once, and admissible entries for one agent that differ are all
/// refused, because the commitment does not say which one its entrant stands
/// behind.
pub fn admit_entries(
    window: &WindowState,
    current_epoch: u64,
    dataset_bytes: &[u8],
    entries: &[RevealedEntry],
    options: IntakeOptions<'_>,
) -> Result<Admission, String> {
    check_entries(window, entries)?;
    let replay = if entries.iter().any(|entry| entry.capture.is_some()) {
        Some(ReplayWindow::parse(dataset_bytes, &window.score_config)?)
    } else {
        None
    };
    let IntakeOptions {
        allow_supplied_returns,
        reexecute: mut launcher,
    } = options;
    // A faulted window refuses every capture, so it re-executes nothing.
    if window.fault_plan_sha256.is_none() {
        if let Some(launcher) = launcher.as_deref_mut() {
            let names_image = entries
                .iter()
                .filter_map(|entry| entry.capture.as_ref())
                .any(|capture| {
                    matches!(
                        CapturedEntrant::of(capture),
                        Ok(CapturedEntrant::Image { .. })
                    )
                });
            if names_image {
                launcher.ready()?;
            }
        }
    }
    let mut registry = registry_for(window, window.data_reveal_epoch, current_epoch)?;
    let mut outcomes: Vec<(String, Result<Admissible, String>)> = Vec::with_capacity(entries.len());
    for (index, entry) in entries.iter().enumerate() {
        let outcome = match entry.committed_agent_id() {
            Some(agent_id) => (
                agent_id.to_string(),
                admit_one(
                    window,
                    &mut registry,
                    replay.as_ref(),
                    entry,
                    agent_id,
                    allow_supplied_returns,
                    launcher
                        .as_mut()
                        .map(|launcher| &mut **launcher as &mut dyn EntrantLauncher),
                ),
            ),
            None => (unnamed_entry(index), Err(UNNAMED_ENTRY_REFUSAL.to_string())),
        };
        outcomes.push(outcome);
    }

    let mut admissible: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for (agent_id, outcome) in &outcomes {
        if let Ok(entry) = outcome {
            let bytes = serde_json::to_string(entry)
                .map_err(|e| format!("internal: serialize an admitted entry: {e}"))?;
            admissible.entry(agent_id.clone()).or_default().push(bytes);
        }
    }
    let mut admission = Admission {
        field: Vec::new(),
        returns_provenance: BTreeMap::new(),
        refusals: Vec::new(),
        dataset_hash: content_digest(dataset_bytes),
        replay_dataset_sha256: replay.as_ref().map(|r| r.dataset_sha256.clone()),
        certifying: false,
    };
    for (agent_id, outcome) in outcomes {
        let reason = match outcome {
            Err(reason) => reason,
            Ok((submission, provenance)) => {
                let copies = &admissible[&agent_id];
                if copies.iter().any(|copy| *copy != copies[0]) {
                    format!(
                        "revealed {} admissible entries that differ; one commitment admits one entry",
                        copies.len()
                    )
                } else if admission.returns_provenance.contains_key(&agent_id) {
                    "an identical copy of this agent's admitted entry; ranked once".to_string()
                } else {
                    admission
                        .returns_provenance
                        .insert(agent_id.clone(), provenance);
                    admission.field.push(submission);
                    continue;
                }
            }
        };
        admission.refusals.push(Refusal { agent_id, reason });
    }
    admission.certifying = board_certifies(&admission.returns_provenance, allow_supplied_returns);
    Ok(admission)
}

/// Admit one entry, or return the reason it is refused.
fn admit_one(
    window: &WindowState,
    registry: &mut Registry,
    replay: Option<&ReplayWindow>,
    entry: &RevealedEntry,
    agent_id: &str,
    allow_supplied_returns: bool,
    launcher: Option<&mut dyn EntrantLauncher>,
) -> Result<Admissible, String> {
    if let (Some(named), Some(submission)) = (&entry.agent_id, &entry.submission) {
        if *named != submission.agent_id {
            return Err(format!(
                "the entry names agent `{named}` but its supplied submission names `{}`",
                submission.agent_id
            ));
        }
    }
    // A faulted window's commitments bind its plan digest, so one made for
    // another plan, or for none, does not match and is refused.
    registry.reveal_under_fault_plan(
        agent_id,
        &window.id,
        &entry.artifact_digest,
        &entry.salt,
        window.fault_plan_sha256.as_deref(),
    )?;
    match (&entry.capture, &entry.submission) {
        (Some(_), Some(_)) => Err(
            "the entry carries both supplied returns and a capture; the arena ranks only returns it derives from the capture"
                .to_string(),
        ),
        (Some(capture), None) => admit_capture(
            window,
            replay.expect("a replay window exists whenever an entry carries a capture"),
            entry,
            capture,
            agent_id,
            launcher,
        ),
        (None, Some(submission)) if allow_supplied_returns => {
            Ok((submission.clone(), ReturnsProvenance::Supplied))
        }
        (None, Some(_)) => Err(SUPPLIED_RETURNS_REFUSAL.to_string()),
        (None, None) => Err("the entry carries neither a capture nor returns".to_string()),
    }
}

fn admit_capture(
    window: &WindowState,
    replay: &ReplayWindow,
    entry: &RevealedEntry,
    capture: &AgentTrajectory,
    agent_id: &str,
    launcher: Option<&mut dyn EntrantLauncher>,
) -> Result<(AgentSubmission, ReturnsProvenance), String> {
    if window.fault_plan_sha256.is_some() {
        return Err("no capture path applies a fault plan, so a capture cannot be replayed as this window's faulted experiment; a faulted window ranks only supplied returns, under the noncertifying intake".to_string());
    }
    let entrant = CapturedEntrant::of(capture)?;
    if entrant.artifact_sha256() != entry.artifact_digest {
        return Err(format!(
            "the capture records entrant artifact {}, but the committed artifact digest is {}",
            entrant.artifact_sha256(),
            entry.artifact_digest
        ));
    }
    let contract = capture
        .contract
        .as_ref()
        .ok_or("the capture has no execution contract")?;
    if contract.dataset_sha256 != replay.dataset_sha256 {
        return Err(format!(
            "the capture was made over dataset {}, but the window revealed dataset {}",
            contract.dataset_sha256, replay.dataset_sha256
        ));
    }
    let captured: Vec<(usize, usize)> = contract.windows.iter().map(|w| (w.start, w.end)).collect();
    let scored: Vec<(usize, usize)> = replay.windows.iter().map(|w| (w.start, w.end)).collect();
    if captured != scored {
        return Err(format!(
            "the capture runs market windows {captured:?}, but the forward window scores {scored:?}"
        ));
    }
    if contract.seeds != replay.seeds {
        return Err(format!(
            "the capture runs execution seeds {:?}, but the forward window scores {:?}",
            contract.seeds, replay.seeds
        ));
    }
    let runner = Some(window.scorer_artifact_sha256.as_str());
    let provenance = match launcher {
        None => {
            verify_trajectory_strict(
                &replay.data,
                capture,
                replay.costs,
                &window.score_config,
                runner,
            )?;
            ReturnsProvenance::Replayed
        }
        Some(launcher) => {
            reexecute(
                replay,
                capture,
                &window.score_config,
                runner,
                &entrant,
                launcher,
            )?;
            ReturnsProvenance::ReExecuted
        }
    };
    let mut submission = replay_submission(&replay.data, capture, replay.costs);
    submission.agent_id = agent_id.to_string();
    Ok((submission, provenance))
}

fn reexecute(
    replay: &ReplayWindow,
    capture: &AgentTrajectory,
    config: &ScoreConfig,
    runner: Option<&str>,
    entrant: &CapturedEntrant,
    launcher: &mut dyn EntrantLauncher,
) -> Result<(), String> {
    let outcome = match entrant {
        CapturedEntrant::Reference { name, .. } => {
            let momentum = name == "momentum";
            verify_trajectory_reexecuted(
                &replay.data,
                capture,
                replay.costs,
                config,
                runner,
                || {
                    if momentum {
                        Box::new(Momentum::default()) as Box<dyn Agent>
                    } else {
                        Box::new(BuyAndHold)
                    }
                },
            )
        }
        CapturedEntrant::Image { reference, .. } => {
            launcher.admit(reference).map_err(|error| {
                format!("the committed image `{reference}` cannot be re-executed: {error}")
            })?;
            let failure: FailureCell = Rc::new(RefCell::new(None));
            let outcome = verify_trajectory_reexecuted(
                &replay.data,
                capture,
                replay.costs,
                config,
                runner,
                || {
                    if failure.borrow().is_some() {
                        return Box::new(HoldAgent) as Box<dyn Agent>;
                    }
                    match launcher.launch(reference) {
                        Ok(inner) => Box::new(WatchedRun {
                            inner: Some(inner),
                            failure: Rc::clone(&failure),
                        }),
                        Err(error) => {
                            failure
                                .borrow_mut()
                                .get_or_insert(format!("an instance did not start: {error}"));
                            Box::new(HoldAgent)
                        }
                    }
                },
            );
            // A fault makes the instance hold, which would read as a
            // divergence; the fault is the reason, so it is reported first.
            if let Some(error) = failure.borrow_mut().take() {
                return Err(format!(
                    "re-execution of the committed image `{reference}` failed: {error}"
                ));
            }
            outcome
        }
    };
    outcome.map(|_| ()).map_err(|error| match error {
        ReexecutionError::Refused(reason) => reason,
        ReexecutionError::Diverged(divergence) => format!(
            "re-execution diverged from the capture at run {} step {} (observation `{}`): the capture records a decision the committed entrant does not make",
            divergence.run, divergence.step, divergence.observation_id
        ),
    })
}
