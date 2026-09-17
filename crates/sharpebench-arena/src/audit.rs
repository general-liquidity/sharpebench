//! The forward-arena leakage case of `sharpebench audit`.
//!
//! The kernel's self-audit (`sharpebench_core::run_self_audit`) fires nine
//! attacks at the scorer and runs wherever the kernel runs, WASM included. This
//! tenth case needs the simulator and the arena intake, which the pure kernel
//! cannot reach, so it lives here and `sharpebench audit` appends it.
//!
//! The attack is Gençay's planted look-ahead oracle (arXiv 2608.27734, p. 7)
//! moved into a forward window. An entrant commits an image before the
//! deadline, then, holding the revealed data, delivers what a next-bar oracle
//! earns on it. The case does not claim that statistics catch this; it first
//! shows the opposite, that the oracle's returns are rank-eligible when ranked
//! directly. It passes only when intake stops the oracle on both routes: the
//! supplied returns are refused, and a capture of the oracle's decisions that
//! names the committed image, which replays exactly and is therefore ranked as
//! `replayed` without re-execution, is refused when that image is re-executed.
//! An in-process momentum agent stands in for the committed image's container.

use std::collections::BTreeMap;

use sharpebench_attest::{content_digest, make_commitment};
use sharpebench_core::selfaudit::AuditCase;
use sharpebench_core::{rank, ScoreConfig};
use sharpebench_harness::run_agent_capture;
use sharpebench_protocol::{Action, AgentTrajectory, Decision, MarketObservation, Order};
use sharpebench_sim::{replay_submission, Agent, Dataset, Momentum};

use crate::intake::{
    admit_entries, Admission, EntrantLauncher, IntakeOptions, LaunchedEntrant, ReplayWindow,
    ReturnsProvenance, SUPPLIED_RETURNS_REFUSAL,
};
use crate::{score_config_digest, RevealedEntry, WindowState, WindowStatus, WINDOW_SCHEMA_VERSION};

/// The case's name in the audit report.
pub const FORWARD_HINDSIGHT_ORACLE: &str = "forward-hindsight-oracle";

const ATTACK: &str = "commit an image before the deadline, then deliver a next-bar oracle's hindsight returns, or a capture of its decisions, over the revealed forward window";
const WINDOW: &str = "audit-forward-window";
const RETURNS_AGENT: &str = "hindsight-returns";
const CAPTURE_AGENT: &str = "hindsight-capture";
const HONEST_AGENT: &str = "committed-image";

/// A next-bar oracle. It holds the whole revealed dataset and puts half its
/// book into the symbol whose next close rises most, and nothing into a bar on
/// which no close rises. Half is the engine's concentration cap, so the oracle
/// raises no process event.
struct HindsightOracle {
    data: Dataset,
}

impl Agent for HindsightOracle {
    fn decide(&mut self, observation: &MarketObservation) -> Decision {
        let now = self
            .data
            .dates
            .iter()
            .position(|date| *date == observation.date);
        let best = now.and_then(|t| {
            self.data
                .closes
                .iter()
                .filter_map(|(symbol, closes)| {
                    let gain = closes.get(t + 1)? / closes[t] - 1.0;
                    (gain > 0.0).then_some((symbol, gain))
                })
                .max_by(|a, b| a.1.total_cmp(&b.1))
                .map(|(symbol, _)| symbol.clone())
        });
        let orders = observation
            .symbols
            .iter()
            .map(|snapshot| {
                let long = best.as_deref() == Some(snapshot.symbol.as_str());
                Order {
                    symbol: snapshot.symbol.clone(),
                    action: if long { Action::Buy } else { Action::Close },
                    target_weight: if long { 0.5 } else { 0.0 },
                    confidence: None,
                    rationale: String::new(),
                }
            })
            .collect();
        Decision {
            orders,
            reasoning: String::new(),
            cost: None,
        }
    }
}

/// Stands in for the container runtime: every launch of the committed image
/// is a fresh momentum agent, which sees only point-in-time observations.
struct CommittedImage {
    reference: String,
}

struct InProcessImage(Momentum);

impl Agent for InProcessImage {
    fn decide(&mut self, observation: &MarketObservation) -> Decision {
        self.0.decide(observation)
    }
}

impl LaunchedEntrant for InProcessImage {
    fn fault(&self) -> Option<String> {
        None
    }

    fn finish(self: Box<Self>) -> Result<(), String> {
        Ok(())
    }
}

impl EntrantLauncher for CommittedImage {
    fn admit(&mut self, image: &str) -> Result<(), String> {
        if image == self.reference {
            Ok(())
        } else {
            Err(format!("image `{image}` is not the committed image"))
        }
    }

    fn launch(&mut self, _image: &str) -> Result<Box<dyn LaunchedEntrant>, String> {
        Ok(Box::new(InProcessImage(Momentum::default())))
    }
}

/// The dataset as the `date,symbol,close` CSV a forward window reveals.
/// `f64` display is the shortest representation that parses back to the same
/// bits, so the parsed dataset equals `data`.
fn dataset_csv(data: &Dataset) -> String {
    let mut out = String::from("date,symbol,close\n");
    for (symbol, closes) in &data.closes {
        for (date, close) in data.dates.iter().zip(closes) {
            out.push_str(&format!("{date},{symbol},{close}\n"));
        }
    }
    out
}

fn entry(agent_id: &str, capture: Option<AgentTrajectory>, artifact: &str) -> RevealedEntry {
    RevealedEntry {
        agent_id: Some(agent_id.to_string()),
        submission: None,
        capture,
        artifact_digest: artifact.to_string(),
        salt: format!("salt-{agent_id}"),
        fault_plan_sha256: None,
    }
}

fn refusal(admission: &Admission, agent_id: &str) -> Option<String> {
    admission
        .refusals
        .iter()
        .find(|refusal| refusal.agent_id == agent_id)
        .map(|refusal| refusal.reason.clone())
}

fn describe(admission: &Admission, agent_id: &str) -> String {
    match admission.returns_provenance.get(agent_id) {
        Some(provenance) => format!("ranked as {}", provenance.as_str()),
        None => match refusal(admission, agent_id) {
            Some(reason) => format!("refused ({reason})"),
            None => "absent".to_string(),
        },
    }
}

/// Run the forward hindsight-oracle case. Defended when the oracle's returns
/// are rank-eligible if ranked directly, its supplied returns are refused under
/// both intakes, its capture is refused on re-execution, and the committed
/// image's own capture is ranked as re-executed.
pub fn forward_hindsight_oracle_case() -> AuditCase {
    run_case().unwrap_or_else(|error| AuditCase {
        name: FORWARD_HINDSIGHT_ORACLE.to_string(),
        attack: ATTACK.to_string(),
        defended: false,
        expected_vulnerable: false,
        detail: format!("the case could not run: {error}"),
    })
}

fn run_case() -> Result<AuditCase, String> {
    let config = ScoreConfig {
        execution_seeds_per_window: 2,
        ..ScoreConfig::default()
    };
    // Three times the default shock amplitude. At the default amplitude the
    // square-root impact of rotating half the book every bar costs more than
    // hindsight earns, and the case would show only that costs stop a weak
    // oracle.
    let csv = dataset_csv(&Dataset::synthetic_parameterized(
        4, 160, 20_260_916, 3.0, 0.0, 0.0,
    ));
    let replay = ReplayWindow::parse(csv.as_bytes(), &config)?;
    let scorer = content_digest(b"sharpebench-audit/frozen-scorer");
    let image = content_digest(b"sharpebench-audit/committed-image");
    let reference = format!("audit/entrant@sha256:{image}");

    let capture = |make: &dyn Fn() -> Box<dyn Agent>| {
        let (_, mut trajectory) = run_agent_capture(
            &format!("sandbox:{reference}"),
            &replay.data,
            &replay.windows,
            &replay.seeds,
            replay.costs,
            make,
        );
        if let Some(contract) = &mut trajectory.contract {
            contract.runner_artifact_sha256 = Some(scorer.clone());
        }
        trajectory
    };
    let oracle = capture(&|| {
        Box::new(HindsightOracle {
            data: replay.data.clone(),
        })
    });
    let honest = capture(&|| Box::new(Momentum::default()));

    // What the oracle earns on the revealed window is what a hindsight entrant
    // would deliver as its returns.
    let mut supplied = replay_submission(&replay.data, &oracle, replay.costs);
    supplied.agent_id = RETURNS_AGENT.to_string();
    let direct = rank(std::slice::from_ref(&supplied), &config);
    let exposed = &direct[0];

    let entries = vec![
        RevealedEntry {
            agent_id: None,
            submission: Some(supplied),
            ..entry(RETURNS_AGENT, None, &image)
        },
        entry(CAPTURE_AGENT, Some(oracle), &image),
        entry(HONEST_AGENT, Some(honest), &image),
    ];
    let commitments = entries
        .iter()
        .map(|e| {
            make_commitment(
                e.committed_agent_id().unwrap_or_default(),
                WINDOW,
                &e.artifact_digest,
                &e.salt,
            )
        })
        .collect();
    let window = WindowState {
        schema_version: WINDOW_SCHEMA_VERSION,
        id: WINDOW.to_string(),
        commit_deadline: 1,
        data_reveal_epoch: 2,
        status: WindowStatus::Committed,
        score_config_sha256: score_config_digest(&config)?,
        score_config: config,
        scorer_artifact_sha256: scorer.clone(),
        sealed_eval_salt_sha256: None,
        fault_plan_sha256: None,
        commitments,
        refusals: Vec::new(),
        dataset_hash: None,
        replay_dataset_sha256: None,
        supplied_returns_accepted: false,
        scores: Vec::new(),
        returns_provenance: BTreeMap::new(),
    };

    let replayed = admit_entries(
        &window,
        2,
        csv.as_bytes(),
        &entries,
        IntakeOptions::default(),
    )?;
    let mut launcher = CommittedImage { reference };
    let reexecuted = admit_entries(
        &window,
        2,
        csv.as_bytes(),
        &entries,
        IntakeOptions {
            allow_supplied_returns: false,
            reexecute: Some(&mut launcher),
        },
    )?;

    let supplied_refused = [&replayed, &reexecuted]
        .iter()
        .all(|a| refusal(a, RETURNS_AGENT).as_deref() == Some(SUPPLIED_RETURNS_REFUSAL));
    let capture_refused = refusal(&reexecuted, CAPTURE_AGENT)
        .is_some_and(|reason| reason.starts_with("re-execution diverged"));
    let honest_reexecuted =
        reexecuted.returns_provenance.get(HONEST_AGENT) == Some(&ReturnsProvenance::ReExecuted);
    let defended =
        exposed.rank_eligible && supplied_refused && capture_refused && honest_reexecuted;
    Ok(AuditCase {
        name: FORWARD_HINDSIGHT_ORACLE.to_string(),
        attack: ATTACK.to_string(),
        defended,
        expected_vulnerable: false,
        detail: format!(
            "oracle returns ranked directly: DSR {:.3}, eligible {}; at intake its supplied returns are {}; its capture naming the committed image is {} by replay alone and {} under re-execution; the committed image's own capture is {}",
            exposed.deflated_sharpe,
            exposed.rank_eligible,
            if supplied_refused { "refused" } else { "ranked" },
            describe(&replayed, CAPTURE_AGENT),
            describe(&reexecuted, CAPTURE_AGENT),
            describe(&reexecuted, HONEST_AGENT),
        ),
    })
}
