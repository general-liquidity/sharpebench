//! `sharpebench contamination-probe` and `sharpebench presentation-stability`:
//! the shape-level contamination probe and the presentation suite, run against
//! a reference agent, a host process (`--cmd`), an HTTP endpoint (`--http`) or
//! a digest-pinned image in the hardened sandbox (`--image`). `sharpebench
//! identifiability-probe` asks the same external entrants closed questions
//! about masked series over their own transport; the reference agents have no
//! answer channel and are refused there.
//!
//! Both are rank-neutral reporting surface: no board is printed, nothing `run`
//! reads is written, and the JSON document says `rank_input: false`. A spawn,
//! transport, protocol, resource or finalization failure on any arm is
//! reported as a typed unavailability naming the arm, never as a dropped arm,
//! because an external agent that degrades to a hold would otherwise put the
//! harness's holds into the comparison as the entrant's own decisions.

use std::cell::RefCell;
use std::rc::Rc;

use serde::Serialize;
use sharpebench_harness::contamination_probe::{
    ask_external, identifiability_document, probe_document, shape_probe_document, AnswerFailure,
    ProbeContext, PRESENTATION_STABILITY_SCHEMA,
};
use sharpebench_harness::declared_cutoff::{self, DeclaredCutoff};
use sharpebench_harness::FailureKind;
use sharpebench_sim::windows::walk_forward;
use sharpebench_sim::{
    identifiability_probe, presentation_suite, record_run, run_shape_probe, Agent, BuyAndHold,
    Dataset, ExternalAgent, HoldAgent, HttpAgent, IdentifiabilityConfig, IdentifiabilityReport,
    IdentifiabilityUnavailable, IdentityAnswer, IdentityQuestion, LineExchange, Momentum,
    PresentationSuite, ProbeConfig, ShapeProbeRun, ShapeProbeUnavailable, SuiteError, SuiteSpec,
    TwinSpec, Window,
};

use crate::external_capture::{
    sandbox_channel_factory, sandbox_factory, FaultCell, SandboxLauncher,
};
use crate::{emit_json, flag_value, WatchedAgent};

const ENTRANT: &str =
    "(--agent <buy-and-hold|momentum> | --cmd \"<prog>\" | --http <addr> | --image <repository@sha256:...>)";

const TRANSPORTS: [&str; 4] = ["--agent", "--cmd", "--http", "--image"];

/// A fresh agent per arm.
type AgentFactory<'a> = Box<dyn FnMut() -> Box<dyn Agent> + 'a>;

/// A fresh answer channel to the entrant, or the failure that kept it from
/// starting.
type ChannelFactory<'a> = Box<dyn FnMut() -> Result<Box<dyn LineExchange>, FailureKind> + 'a>;

/// The entrant, as a fresh agent per call, and the first failure any of them
/// reported.
struct Entrant<'a> {
    label: String,
    make: AgentFactory<'a>,
    /// A fresh channel per closed question. `None` for the reference agents,
    /// which have no transport to answer on.
    ask: Option<ChannelFactory<'a>>,
    fault: FaultCell,
}

impl Entrant<'_> {
    /// One arm on a fresh agent. The agent is dropped before the fault is
    /// read, because a sandboxed run records its post-exit verdict on drop.
    fn arm(
        &mut self,
        data: &Dataset,
        window: Window,
        seed: u64,
        costs: sharpebench_sim::CostModel,
    ) -> Result<sharpebench_sim::ArmRun, FailureKind> {
        let mut agent = (self.make)();
        let run = record_run(data, agent.as_mut(), window, seed, costs);
        drop(agent);
        match self.fault.borrow().clone() {
            Some(kind) => Err(kind),
            None => Ok(run),
        }
    }

    /// One closed question on a fresh channel, so no answer can lean on an
    /// earlier question: the real window and its twin never meet in one
    /// process. The channel is dropped before the fault is read, because a
    /// sandboxed channel records its post-exit verdict on drop, and that
    /// verdict outranks the answer.
    fn question(&mut self, question: &IdentityQuestion) -> Result<IdentityAnswer, AnswerFailure> {
        let ask = self
            .ask
            .as_mut()
            .expect("the identifiability probe resolves only entrants with a channel");
        let mut channel = ask().map_err(|failure| AnswerFailure::Failed { failure })?;
        let answer = ask_external(channel.as_mut(), question);
        drop(channel);
        match self.fault.borrow().clone() {
            Some(failure) => Err(AnswerFailure::Failed { failure }),
            None => answer,
        }
    }
}

fn entrant<'a>(
    args: &'a [String],
    launcher: &'a dyn SandboxLauncher,
) -> Result<Entrant<'a>, (String, i32)> {
    let named: Vec<&str> = TRANSPORTS
        .into_iter()
        .filter(|flag| args.iter().any(|arg| arg == flag))
        .collect();
    if named.len() != 1 {
        return Err((format!("name exactly one entrant: {ENTRANT}"), 2));
    }
    let flag = named[0];
    let value = flag_value(args, flag)
        .filter(|v| !v.starts_with("--") && !v.trim().is_empty())
        .ok_or_else(|| (format!("{flag} needs a value"), 2))?;
    let fault: FaultCell = Rc::new(RefCell::new(None));
    let (label, make, ask): (String, AgentFactory<'a>, Option<ChannelFactory<'a>>) = match flag {
        "--agent" => match value {
            "buy-and-hold" => (
                value.to_string(),
                Box::new(|| Box::new(BuyAndHold) as Box<dyn Agent>),
                None,
            ),
            "momentum" => (
                value.to_string(),
                Box::new(|| Box::new(Momentum::default()) as Box<dyn Agent>),
                None,
            ),
            other => {
                return Err((
                    format!("unknown agent `{other}` (use buy-and-hold or momentum)"),
                    2,
                ))
            }
        },
        "--cmd" => {
            let parts: Vec<String> = value.split_whitespace().map(String::from).collect();
            let (prog, rest) = parts
                .split_first()
                .map(|(p, r)| (p.clone(), r.to_vec()))
                .ok_or_else(|| ("--cmd needs a program to run".to_string(), 2))?;
            eprintln!(
                "warning: --cmd runs `{prog}` directly on this host with NO sandbox, once per probe arm. Only point it at an agent you trust."
            );
            let fault = fault.clone();
            let (ask_prog, ask_rest) = (prog.clone(), rest.clone());
            (
                format!("cmd:{value}"),
                Box::new(move || {
                    let rest_refs: Vec<&str> = rest.iter().map(String::as_str).collect();
                    match ExternalAgent::spawn(&prog, &rest_refs) {
                        Ok(agent) => Box::new(WatchedAgent {
                            inner: agent,
                            fault: fault.clone(),
                        }) as Box<dyn Agent>,
                        Err(_) => {
                            fault.borrow_mut().get_or_insert(FailureKind::SpawnError);
                            Box::new(HoldAgent)
                        }
                    }
                }),
                Some(Box::new(move || {
                    let rest_refs: Vec<&str> = ask_rest.iter().map(String::as_str).collect();
                    ExternalAgent::spawn(&ask_prog, &rest_refs)
                        .map(|agent| Box::new(agent) as Box<dyn LineExchange>)
                        .map_err(|_| FailureKind::SpawnError)
                })),
            )
        }
        "--http" => {
            let addr = value.to_string();
            let ask_addr = addr.clone();
            let fault = fault.clone();
            (
                format!("http:{addr}"),
                Box::new(move || {
                    Box::new(WatchedAgent {
                        inner: HttpAgent::new(addr.clone()),
                        fault: fault.clone(),
                    }) as Box<dyn Agent>
                }),
                Some(Box::new(move || {
                    Ok(Box::new(HttpAgent::new(ask_addr.clone())) as Box<dyn LineExchange>)
                })),
            )
        }
        _ => {
            launcher.admit(value).map_err(|error| {
                (
                    format!("cannot start the sandboxed agent `{value}`: {error}"),
                    1,
                )
            })?;
            (
                format!("sandbox:{value}"),
                Box::new(sandbox_factory(value, launcher, fault.clone())),
                Some(Box::new(sandbox_channel_factory(
                    value,
                    launcher,
                    fault.clone(),
                ))),
            )
        }
    };
    Ok(Entrant {
        label,
        make,
        ask,
        fault,
    })
}

/// What both commands resolve before any arm runs.
struct Setup<'a> {
    entrant: Entrant<'a>,
    data: Dataset,
    dataset: String,
    dataset_sha256: String,
    costs: sharpebench_sim::CostModel,
    cutoff: Option<DeclaredCutoff>,
}

fn parse_number<T: std::str::FromStr>(args: &[String], flag: &str) -> Result<Option<T>, String> {
    flag_value(args, flag)
        .map(|raw| {
            raw.parse::<T>()
                .map_err(|_| format!("{flag} must be a whole number, got `{raw}`"))
        })
        .transpose()
}

fn setup<'a>(
    args: &'a [String],
    launcher: &'a dyn SandboxLauncher,
) -> Result<Setup<'a>, (String, i32)> {
    if args.iter().any(|arg| arg == "--masked") {
        return Err((
            "--masked does not apply: the probes build their own masked presentations".to_string(),
            2,
        ));
    }
    let costs = crate::cost_model_from_args(args).map_err(|e| (e, 2))?;
    let cutoff = flag_value(args, "--declared-training-cutoff")
        .map(DeclaredCutoff::parse)
        .transpose()
        .map_err(|e| (e, 2))?;
    let entrant = entrant(args, launcher)?;
    let data = crate::load_trading_dataset(flag_value(args, "--data"), args).map_err(|e| (e, 1))?;
    let dataset_sha256 = crate::digest_json("dataset", &data).map_err(|e| (e, 1))?;
    Ok(Setup {
        entrant,
        dataset: flag_value(args, "--data")
            .unwrap_or("synthetic")
            .to_string(),
        dataset_sha256,
        data,
        costs,
        cutoff,
    })
}

fn context<'s>(setup: &'s Setup<'_>, windows: &[Window]) -> ProbeContext<'s> {
    ProbeContext {
        entrant: &setup.entrant.label,
        dataset: &setup.dataset,
        dataset_sha256: &setup.dataset_sha256,
        declared_training_cutoff: setup
            .cutoff
            .as_ref()
            .map(|c| declared_cutoff::disclosure(c, &setup.data, windows)),
    }
}

fn figure(value: Option<f64>) -> String {
    value.map_or_else(|| "n/a".to_string(), |v| format!("{v:.4}"))
}

const SHAPE_USAGE: &str = "usage: sharpebench contamination-probe <entrant> [--data <csv>] [--window-bars N] [--windows N] [--twin-seed N] [--with-costs [--short-borrow-bps <bps>] [--fill-delay-bars N]] [--declared-training-cutoff YYYY-MM-DD] [--json]";

/// Whether both probe arms run without frictions.
///
/// Recall is what the probe measures, not whether recall pays after costs. A
/// recalling agent that switches its book every bar would pay away under the
/// run's impact charge what it remembers, so both arms run frictionless unless
/// the operator opts into `run`'s cost model with `--with-costs`. A cost flag
/// without `--with-costs` would be ignored, so it is refused instead.
fn shape_cost_mode(args: &[String]) -> Result<bool, String> {
    let with_costs = args.iter().any(|arg| arg == "--with-costs");
    if !with_costs {
        if let Some(flag) = ["--short-borrow-bps", "--fill-delay-bars"]
            .into_iter()
            .find(|flag| args.iter().any(|arg| arg == flag))
        {
            return Err(format!(
                "{flag} applies only with --with-costs; the probe runs frictionless by default"
            ));
        }
        return Ok(true);
    }
    if args.iter().any(|arg| arg == "--frictionless") {
        return Err("--with-costs and --frictionless contradict each other".to_string());
    }
    Ok(false)
}

/// `contamination-probe`: `0` measured, `1` unavailable or unreadable, `2` usage.
pub(crate) fn run_shape(args: &[String], json: bool, launcher: &dyn SandboxLauncher) -> i32 {
    let (bars, count, twin_seed) = match (
        parse_number::<usize>(args, "--window-bars"),
        parse_number::<usize>(args, "--windows"),
        parse_number::<u64>(args, "--twin-seed"),
    ) {
        (Ok(b), Ok(c), Ok(s)) => (b.unwrap_or(40), c.unwrap_or(12), s.unwrap_or(1000)),
        (Err(e), _, _) | (_, Err(e), _) | (_, _, Err(e)) => {
            eprintln!("error: {e}\n{SHAPE_USAGE}");
            return 2;
        }
    };
    if bars < 2 || count == 0 {
        eprintln!(
            "error: --window-bars must be at least 2 and --windows at least 1\n{SHAPE_USAGE}"
        );
        return 2;
    }
    let frictionless = match shape_cost_mode(args) {
        Ok(frictionless) => frictionless,
        Err(error) => {
            eprintln!("error: {error}\n{SHAPE_USAGE}");
            return 2;
        }
    };
    let mut setup = match setup(args, launcher) {
        Ok(setup) => setup,
        Err((error, code)) => {
            eprintln!("error: {error}");
            if code == 2 {
                eprintln!("{SHAPE_USAGE}");
            }
            return code;
        }
    };
    let spec = TwinSpec::default();
    let windows: Vec<Window> = walk_forward(setup.data.len(), spec.warmup, bars, bars)
        .into_iter()
        .take(count)
        .collect();
    let costs = if frictionless {
        sharpebench_sim::CostProfile::None.resolve().costs
    } else {
        setup.costs
    };
    let outcome = run_shape_probe(
        &setup.data,
        &windows,
        &spec,
        ProbeConfig::default(),
        twin_seed,
        &mut |_, data, window| {
            setup
                .entrant
                .arm(data, window, 0, costs)
                .map(|run| run.returns)
        },
    );
    let mut document = shape_probe_document(&context(&setup, &windows), &outcome);
    document["context"]["frictionless"] = frictionless.into();
    if json {
        emit_json(&document);
    } else {
        print_shape(&setup.entrant.label, &outcome);
    }
    i32::from(outcome.is_err())
}

fn print_shape(label: &str, outcome: &Result<ShapeProbeRun, ShapeProbeUnavailable<FailureKind>>) {
    println!("shape-level contamination probe for `{label}` (rank-neutral, not a rank input)");
    match outcome {
        Ok(run) => {
            println!(
                "{:>6} {:>6} {:>6} {:>10} {:>10}",
                "start", "end", "draws", "real", "twin"
            );
            for w in &run.windows {
                println!(
                    "{:>6} {:>6} {:>6} {:>10} {:>10}",
                    w.source_start,
                    w.source_end,
                    w.twin_draws,
                    figure(w.real_sharpe),
                    figure(w.twin_sharpe)
                );
            }
            println!(
                "mean edge {:.4}, one-sided p {:.4}, memorization {}",
                run.report.mean_edge, run.report.pvalue, run.report.memorization
            );
        }
        Err(unavailable) => println!(
            "unavailable: {}",
            serde_json::to_string(unavailable).expect("an unavailability serializes")
        ),
    }
}

const IDENTIFY_USAGE: &str = "usage: sharpebench identifiability-probe (--cmd \"<prog>\" | --http <addr> | --image <repository@sha256:...>) [--data <csv>] [--window-bars N] [--windows N] [--twin-seed N] [--top-k N] [--year-tolerance N] [--declared-training-cutoff YYYY-MM-DD] [--json]";

/// The flags `identifiability-probe` refuses rather than ignores: it runs no
/// backtest, so nothing a cost flag sets would reach it, and a reference agent
/// has no channel to answer a question on.
const IDENTIFY_REFUSED: [&str; 5] = [
    "--agent",
    "--with-costs",
    "--frictionless",
    "--short-borrow-bps",
    "--fill-delay-bars",
];

/// The identifiability probe's declared scoring rule, from the command line.
fn identify_config(args: &[String]) -> Result<(IdentifiabilityConfig, usize, usize), String> {
    if let Some(flag) = IDENTIFY_REFUSED
        .into_iter()
        .find(|flag| args.iter().any(|arg| arg == flag))
    {
        return Err(format!(
            "{flag} does not apply: the identifiability probe asks an external entrant closed questions and runs no backtest"
        ));
    }
    let bars = parse_number::<usize>(args, "--window-bars")?.unwrap_or(40);
    let count = parse_number::<usize>(args, "--windows")?.unwrap_or(12);
    let defaults = IdentifiabilityConfig::default();
    let config = IdentifiabilityConfig {
        top_k: parse_number(args, "--top-k")?.unwrap_or(defaults.top_k),
        year_tolerance: parse_number(args, "--year-tolerance")?.unwrap_or(defaults.year_tolerance),
        twin_seed: parse_number(args, "--twin-seed")?.unwrap_or(defaults.twin_seed),
        ..defaults
    };
    if bars < 2 || count == 0 || config.top_k == 0 || config.year_tolerance < 0 {
        return Err(
            "--window-bars must be at least 2, --windows and --top-k at least 1, and --year-tolerance at least 0"
                .to_string(),
        );
    }
    Ok((config, bars, count))
}

/// `identifiability-probe`: `0` measured, `1` unavailable or unreadable, `2` usage.
pub(crate) fn run_identify(args: &[String], json: bool, launcher: &dyn SandboxLauncher) -> i32 {
    let (config, bars, count) = match identify_config(args) {
        Ok(parsed) => parsed,
        Err(error) => {
            eprintln!("error: {error}\n{IDENTIFY_USAGE}");
            return 2;
        }
    };
    let mut setup = match setup(args, launcher) {
        Ok(setup) => setup,
        Err((error, code)) => {
            eprintln!("error: {error}");
            if code == 2 {
                eprintln!("{IDENTIFY_USAGE}");
            }
            return code;
        }
    };
    let windows: Vec<Window> = walk_forward(setup.data.len(), config.twin.warmup, bars, bars)
        .into_iter()
        .take(count)
        .collect();
    let entrant = &mut setup.entrant;
    let outcome = identifiability_probe(&setup.data, &windows, config, &mut |question| {
        entrant.question(question)
    });
    let document = identifiability_document(&context(&setup, &windows), &outcome);
    if json {
        emit_json(&document);
    } else {
        print_identify(&setup.entrant.label, &outcome);
    }
    i32::from(outcome.is_err())
}

fn print_identify(
    label: &str,
    outcome: &Result<IdentifiabilityReport, IdentifiabilityUnavailable<AnswerFailure>>,
) {
    println!("identifiability probe for `{label}` (rank-neutral, not a rank input)");
    match outcome {
        Ok(report) => {
            println!(
                "{} windows x {} symbols, top-{} tickers, year within {}",
                report.windows, report.symbols_per_window, report.top_k, report.year_tolerance
            );
            println!(
                "{:<8} {:>9} {:>9} {:>9} {:>9}",
                "arm", "top-1", "top-k", "year", "joint"
            );
            for (arm, top1, top_k, year, joint) in [
                (
                    "real",
                    report.real.ticker_top1,
                    report.real.ticker_top_k,
                    report.real.year,
                    report.real.joint,
                ),
                (
                    "twin",
                    report.twin.ticker_top1,
                    report.twin.ticker_top_k,
                    report.twin.year,
                    report.twin.joint,
                ),
                (
                    "chance",
                    report.chance.ticker_top1,
                    report.chance.ticker_top_k,
                    report.chance.year,
                    report.chance.joint,
                ),
            ] {
                println!("{arm:<8} {top1:>9.4} {top_k:>9.4} {year:>9.4} {joint:>9.4}");
            }
            println!(
                "mean joint edge {:.4}, one-sided p {:.4}, path identified {}",
                report.joint_edge.mean_edge, report.joint_edge.pvalue, report.path_identified
            );
        }
        Err(unavailable) => println!(
            "unavailable: {}",
            serde_json::to_string(unavailable).expect("an unavailability serializes")
        ),
    }
}

const PRESENTATION_USAGE: &str = "usage: sharpebench presentation-stability <entrant> [--data <csv>] [--date-shift-days N] [--short-borrow-bps <bps>] [--declared-training-cutoff YYYY-MM-DD] [--json]";

/// One window's suite.
#[derive(Serialize)]
struct WindowSuite {
    window_start: usize,
    window_end: usize,
    suite: PresentationSuite,
}

/// The window a suite failed on, and why.
#[derive(Serialize)]
struct WindowRefusal {
    window: usize,
    #[serde(flatten)]
    error: SuiteError<FailureKind>,
}

/// `presentation-stability`: `0` measured, `1` unavailable or unreadable, `2` usage.
pub(crate) fn run_presentation(args: &[String], json: bool, launcher: &dyn SandboxLauncher) -> i32 {
    let shift = match parse_number::<i64>(args, "--date-shift-days") {
        Ok(shift) => shift,
        Err(error) => {
            eprintln!("error: {error}\n{PRESENTATION_USAGE}");
            return 2;
        }
    };
    let mut setup = match setup(args, launcher) {
        Ok(setup) => setup,
        Err((error, code)) => {
            eprintln!("error: {error}");
            if code == 2 {
                eprintln!("{PRESENTATION_USAGE}");
            }
            return code;
        }
    };
    let windows = match crate::resolve_dataset(args) {
        Ok((_, windows)) => windows,
        Err(error) => {
            eprintln!("error: {error}");
            return 1;
        }
    };
    let spec = SuiteSpec {
        date_shift_days: shift,
    };
    let costs = setup.costs;
    let mut suites = Vec::with_capacity(windows.len());
    let mut refusal = None;
    for (i, window) in windows.iter().enumerate() {
        let entrant = &mut setup.entrant;
        match presentation_suite(&setup.data, *window, 0, costs, spec, &mut |request| {
            entrant.arm(request.data, request.window, request.seed, request.costs)
        }) {
            Ok(suite) => suites.push(WindowSuite {
                window_start: window.start,
                window_end: window.end,
                suite,
            }),
            Err(error) => {
                refusal = Some(WindowRefusal { window: i, error });
                break;
            }
        }
    }
    let outcome = match refusal {
        None => Ok(suites),
        Some(refusal) => Err(refusal),
    };
    let document = probe_document(
        PRESENTATION_STABILITY_SCHEMA,
        &context(&setup, &windows),
        outcome.as_ref(),
    );
    if json {
        emit_json(&document);
    } else {
        print_presentation(&setup.entrant.label, &outcome);
    }
    i32::from(outcome.is_err())
}

fn variant_name(variant: &sharpebench_sim::Presentation) -> String {
    serde_json::to_value(variant)
        .ok()
        .and_then(|v| {
            let kind = v["kind"].as_str()?.to_string();
            Some(match (v.get("level"), v.get("offset_days")) {
                (Some(level), _) => level.as_str().unwrap_or_default().to_string(),
                (_, Some(days)) => format!("date shift {days}d"),
                _ => kind,
            })
        })
        .unwrap_or_default()
}

fn print_presentation(label: &str, outcome: &Result<Vec<WindowSuite>, WindowRefusal>) {
    println!("presentation stability for `{label}` (rank-neutral, not a rank input)");
    match outcome {
        Ok(windows) => {
            for w in windows {
                println!("window [{}, {})", w.window_start, w.window_end);
                for pair in &w.suite.pairs {
                    let c = &pair.comparison;
                    println!(
                        "  {:<16} disagreeing {:>4} of {:>4}  mean max weight gap {:.4}  sharpe {} -> {}",
                        variant_name(&pair.variant),
                        c.disagreeing_steps,
                        c.steps,
                        c.mean_max_weight_gap,
                        figure(c.baseline_sharpe),
                        figure(c.variant_sharpe)
                    );
                }
            }
        }
        Err(refusal) => println!(
            "unavailable: {}",
            serde_json::to_string(refusal).expect("a refusal serializes")
        ),
    }
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;

    use sharpebench_sim::TransportHealth;

    use super::*;
    use crate::external_capture::SandboxEntrant;

    const IMAGE: &str =
        "example/entrant@sha256:0000000000000000000000000000000000000000000000000000000000000000";

    /// A daemon-free stand-in for the Docker boundary: every container runs
    /// momentum, and the launch numbered `oom` finishes over its memory budget.
    /// Asked a closed question, a container names the candidates in reverse
    /// and the first candidate year, or declines when `decline` is set.
    struct FakeSandbox {
        oom: Option<usize>,
        decline: bool,
        launched: Cell<usize>,
        finished: Rc<Cell<usize>>,
    }

    struct FakeContainer {
        policy: Momentum,
        health: TransportHealth,
        oom: bool,
        decline: bool,
        finished: Rc<Cell<usize>>,
    }

    impl LineExchange for FakeContainer {
        fn exchange(&mut self, request: &str) -> Result<String, sharpebench_sim::DecideError> {
            let question: IdentityQuestion = serde_json::from_str(request).unwrap();
            let mut tickers = question.candidate_tickers.clone();
            tickers.reverse();
            let answer = if self.decline {
                serde_json::json!({ "declined": true })
            } else {
                serde_json::json!({ "tickers": tickers, "year": question.candidate_years[0] })
            };
            let mut answer = answer.as_object().unwrap().clone();
            answer.insert(
                "schema_version".into(),
                sharpebench_sim::IDENTITY_ANSWER_SCHEMA.into(),
            );
            answer.insert("question_id".into(), question.question_id.into());
            Ok(serde_json::Value::Object(answer).to_string())
        }
    }

    impl Agent for FakeContainer {
        fn decide(
            &mut self,
            observation: &sharpebench_protocol::MarketObservation,
        ) -> sharpebench_protocol::Decision {
            self.policy.decide(observation)
        }
    }

    impl sharpebench_sim::TransportDiagnostics for FakeContainer {
        fn health(&self) -> &TransportHealth {
            &self.health
        }
    }

    impl SandboxEntrant for FakeContainer {
        fn finish(self: Box<Self>) -> Result<Option<bool>, String> {
            self.finished.set(self.finished.get() + 1);
            Ok(Some(self.oom))
        }
    }

    impl SandboxLauncher for FakeSandbox {
        fn admit(&self, image: &str) -> Result<(), String> {
            assert_eq!(image, IMAGE);
            Ok(())
        }

        fn launch(&self, image: &str) -> Result<Box<dyn SandboxEntrant>, String> {
            assert_eq!(image, IMAGE);
            let id = self.launched.get();
            self.launched.set(id + 1);
            Ok(Box::new(FakeContainer {
                policy: Momentum::default(),
                health: TransportHealth::default(),
                oom: self.oom == Some(id),
                decline: self.decline,
                finished: self.finished.clone(),
            }))
        }
    }

    fn sandbox(oom: Option<usize>) -> FakeSandbox {
        FakeSandbox {
            oom,
            decline: false,
            launched: Cell::new(0),
            finished: Rc::new(Cell::new(0)),
        }
    }

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|arg| arg.to_string()).collect()
    }

    /// One fresh container per arm, every one finished, and a clean report.
    #[test]
    fn an_image_entrant_gets_a_fresh_container_per_arm() {
        let fake = sandbox(None);
        let argv = args(&["sharpebench", "presentation-stability", "--image", IMAGE]);
        assert_eq!(run_presentation(&argv, true, &fake), 0);
        // Two synthetic windows, six arms each: unmasked, four levels, reordered.
        assert_eq!(fake.launched.get(), 12);
        assert_eq!(fake.finished.get(), 12);
    }

    /// An arm whose container breached its memory budget refuses the report,
    /// and no further container starts.
    #[test]
    fn an_out_of_memory_arm_refuses_the_report_and_stops_launching() {
        let fake = sandbox(Some(2));
        let argv = args(&["sharpebench", "presentation-stability", "--image", IMAGE]);
        assert_eq!(run_presentation(&argv, true, &fake), 1);
        assert_eq!(fake.launched.get(), 3);
        assert_eq!(fake.finished.get(), 3);

        let fake = sandbox(Some(1));
        let argv = args(&[
            "sharpebench",
            "contamination-probe",
            "--image",
            IMAGE,
            "--windows",
            "3",
        ]);
        assert_eq!(run_shape(&argv, true, &fake), 1);
        assert_eq!(
            fake.launched.get(),
            2,
            "the real arm, then the failed twin arm"
        );
    }

    fn frozen() -> String {
        format!(
            "{}/../../data/us-indices-1d.csv",
            env!("CARGO_MANIFEST_DIR")
        )
    }

    fn identify(fake: &FakeSandbox, windows: &str) -> i32 {
        let data = frozen();
        let argv = args(&[
            "sharpebench",
            "identifiability-probe",
            "--image",
            IMAGE,
            "--data",
            &data,
            "--windows",
            windows,
        ]);
        run_identify(&argv, true, fake)
    }

    /// Every closed question gets its own container, and every container is
    /// finished: two windows of three symbols, a real and a twin arm each.
    #[test]
    fn an_image_entrant_answers_each_question_in_a_fresh_container() {
        let fake = sandbox(None);
        assert_eq!(identify(&fake, "2"), 0);
        assert_eq!(fake.launched.get(), 12);
        assert_eq!(fake.finished.get(), 12);
    }

    /// A question whose container breached its memory budget refuses the
    /// report although the answer arrived, and no further container starts.
    /// A declining container refuses it at the first question.
    #[test]
    fn a_failed_or_declined_question_refuses_and_stops_launching() {
        let fake = sandbox(Some(1));
        assert_eq!(identify(&fake, "2"), 1);
        assert_eq!(fake.launched.get(), 2);
        assert_eq!(fake.finished.get(), 2);

        let fake = FakeSandbox {
            decline: true,
            ..sandbox(None)
        };
        assert_eq!(identify(&fake, "2"), 1);
        assert_eq!(fake.launched.get(), 1);
    }

    /// The question's channel outlives no question, and its post-exit verdict
    /// outranks the answer it gave.
    #[test]
    fn a_question_reads_the_sandbox_verdict_after_the_channel_is_dropped() {
        let fake = sandbox(Some(0));
        let argv = args(&["sharpebench", "x", "--image", IMAGE]);
        let mut first = entrant(&argv, &fake).ok().unwrap();
        let question = IdentityQuestion {
            schema_version: sharpebench_sim::IDENTITY_QUESTION_SCHEMA.to_string(),
            question_id: 4,
            symbol: "ASSET_000".to_string(),
            dates: vec!["t0".to_string(), "t1".to_string()],
            closes: vec![1.0, 2.0],
            scored_from: 1,
            candidate_tickers: vec!["A".to_string(), "B".to_string()],
            candidate_years: vec![2016, 2017],
            top_k: 2,
        };
        assert_eq!(
            first.question(&question),
            Err(AnswerFailure::Failed {
                failure: FailureKind::ResourceLimitExceeded
            })
        );
        let fake = sandbox(None);
        let mut second = entrant(&argv, &fake).ok().unwrap();
        assert_eq!(
            second.question(&question),
            Ok(IdentityAnswer {
                tickers: vec!["B".to_string(), "A".to_string()],
                year: Some(2016),
            })
        );
        assert_eq!(fake.finished.get(), 1);
    }

    #[test]
    fn the_identifiability_rule_is_read_from_the_command_line() {
        let (config, bars, count) = identify_config(&args(&[
            "sharpebench",
            "identifiability-probe",
            "--top-k",
            "1",
            "--year-tolerance",
            "2",
            "--twin-seed",
            "7",
            "--window-bars",
            "30",
            "--windows",
            "5",
        ]))
        .unwrap();
        assert_eq!(
            (config.top_k, config.year_tolerance, config.twin_seed),
            (1, 2, 7)
        );
        assert_eq!((bars, count), (30, 5));
        let (config, bars, count) = identify_config(&args(&["sharpebench", "x"])).unwrap();
        assert_eq!(config, IdentifiabilityConfig::default());
        assert_eq!((bars, count), (40, 12));
        for refused in [
            vec!["--agent", "momentum"],
            vec!["--with-costs"],
            vec!["--frictionless"],
            vec!["--short-borrow-bps", "25"],
            vec!["--fill-delay-bars", "1"],
            vec!["--top-k", "0"],
            vec!["--year-tolerance", "-1"],
            vec!["--windows", "0"],
            vec!["--window-bars", "1"],
            vec!["--top-k", "two"],
        ] {
            let mut list = vec!["sharpebench", "identifiability-probe"];
            list.extend(refused.iter().copied());
            assert!(identify_config(&args(&list)).is_err(), "{refused:?}");
        }
        let fake = sandbox(None);
        let argv = args(&[
            "sharpebench",
            "identifiability-probe",
            "--agent",
            "momentum",
        ]);
        assert_eq!(run_identify(&argv, true, &fake), 2);
        assert_eq!(fake.launched.get(), 0);
    }

    #[test]
    fn the_entrant_is_resolved_from_exactly_one_flag() {
        let fake = sandbox(None);
        for (list, code) in [
            (vec!["sharpebench", "presentation-stability"], 2),
            (
                vec![
                    "sharpebench",
                    "presentation-stability",
                    "--agent",
                    "momentum",
                    "--image",
                    IMAGE,
                ],
                2,
            ),
            (vec!["sharpebench", "presentation-stability", "--image"], 2),
            (
                vec!["sharpebench", "presentation-stability", "--cmd", " "],
                2,
            ),
        ] {
            let argv = args(&list);
            match entrant(&argv, &fake) {
                Ok(_) => panic!("{list:?} resolved"),
                Err((_, got)) => assert_eq!(got, code, "{list:?}"),
            };
        }
        assert_eq!(fake.launched.get(), 0);
        let argv = args(&["sharpebench", "x", "--agent", "buy-and-hold"]);
        assert_eq!(entrant(&argv, &fake).ok().unwrap().label, "buy-and-hold");
        let argv = args(&["sharpebench", "x", "--image", IMAGE]);
        assert_eq!(
            entrant(&argv, &fake).ok().unwrap().label,
            format!("sandbox:{IMAGE}")
        );
    }
}
