//! Local open-weight model field through SharpeArena's canonical Ollama shim.
//!
//! # Cross-repository dependency (this example does not run standalone)
//!
//! The model interaction itself lives in the sibling
//! [SharpeArena](https://github.com/general-liquidity/sharpearena) repository,
//! not here: this example spawns `python -m sharpearena.ollama_shim` and speaks
//! the wire protocol to it. Prompt construction, the Ollama HTTP call,
//! thinking-mode handling, sampling and identity capture are all on that side.
//! Running this example therefore needs, in addition to Ollama itself:
//!
//! ```text
//! pip install sharpearena        # or: pip install -e path/to/sharpearena
//! ```
//!
//! The example preflights that import and exits with an actionable diagnostic
//! when it is absent, rather than letting the missing module surface later as an
//! anonymous spawn failure.
//!
//! This is the registry-compatible bridge between the two products. It runs a
//! predeclared set of installed Ollama models through the same walk-forward,
//! reference-field, luck-floor, and ranking path as `llm_field_eval`, but it
//! never calls a remote model provider. A malformed model decision is an agent
//! protocol failure and becomes a sentinel run; infrastructure failures abort
//! the field and the `.partial` file is never published.
//!
//! Required environment:
//!   SHARPEBENCH_LOCAL_MODELS=tag-a,tag-b
//! Optional:
//!   SHARPEARENA_PYTHON=python
//!   SHARPEBENCH_LOCAL_CADENCE=5
//!   SHARPEBENCH_LOCAL_THINKING=false
//!   SHARPEBENCH_LOCAL_MAX_TOKENS=512
//!   SHARPEBENCH_LOCAL_TIMEOUT_SECONDS=120
//!   SHARPEBENCH_LOCAL_N_TRIALS=1
//!
//! Run one dataset per process (or omit the dataset for all nine):
//!   cargo run --release -p sharpebench-harness --example local_open_weight_field_eval -- \
//!     local-open-weight.jsonl us-indices-1d

use std::env;
use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use sharpebench_core::composite::{rank, ScoreConfig, TrialsSrStdSource};
use sharpebench_core::{AgentSubmission, PassMode};
use sharpebench_harness::luck_floor;
use sharpebench_sim::{
    run_backtest, tag_regime, walk_forward, Agent, BuyAndHold, CostModel, Dataset, ExternalAgent,
    HoldAgent, Momentum, Window,
};

const DATASETS: &[(&str, &str, &str, f64)] = &[
    ("us-indices-1d", "equity-index", "1d", 252.0),
    ("us-indices-1w", "equity-index", "1w", 52.0),
    ("crypto-majors-1h", "crypto", "1h", 8760.0),
    ("crypto-majors-4h", "crypto", "4h", 2190.0),
    ("crypto-majors-1d", "crypto", "1d", 365.0),
    ("crypto-majors-1w", "crypto", "1w", 52.0),
    ("fx-majors-1d", "fx", "1d", 252.0),
    ("commodities-1d", "commodities", "1d", 252.0),
    ("rates-1d", "rates", "1d", 252.0),
];
const EXEC_SEEDS: [u64; 8] = [1, 2, 3, 4, 5, 6, 7, 8];
const LUCK_FLOOR_AGENTS: usize = 5;
const DSR_BAR: f64 = 0.95;
const NEVER_CATASTROPHIC_RUN_DD: f64 = 0.20;
const EXTERNAL_MAX_RETRIES: u32 = 2;

#[derive(Clone, Debug, Deserialize, Serialize)]
struct ModelIdentity {
    model: String,
    digest: String,
    parameter_size: String,
    quantization: String,
    family: String,
    context_length: Option<u64>,
    server: String,
    server_version: String,
    size_bytes: Option<u64>,
    format: String,
    #[serde(default)]
    capabilities: Vec<String>,
    license_sha256: Option<String>,
    modelfile_sha256: Option<String>,
    template_sha256: Option<String>,
    parameters_sha256: Option<String>,
}

#[derive(Serialize)]
struct Record<'a> {
    dataset: &'a str,
    asset_class: &'a str,
    timeframe: &'a str,
    periods_per_year: f64,
    n_bars: usize,
    n_symbols: usize,
    n_windows: usize,
    window_len: usize,
    n_seeds: usize,
    regimes: Vec<String>,
    dsr_bar: f64,
    n_trials: u32,
    sr_std_pinned: Option<f64>,
    agent_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    model: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    model_identity: Option<ModelIdentity>,
    decision_cadence: u32,
    thinking: bool,
    agent_protocol_failures: usize,
    deflated_sharpe: f64,
    psr: f64,
    passed_k: bool,
    process_ok: bool,
    bootstrap_p: f64,
    raw_mean_return: f64,
    rank_eligible: bool,
    eligible_never_catastrophic: bool,
    worst_run_drawdown: f64,
    field_reality_check_p: f64,
    step_down_significant: bool,
    trials_sr_std_used: f64,
    trials_sr_std_source: String,
}

type AgentFactory = Box<dyn Fn() -> Box<dyn Agent>>;

fn env_parse<T: std::str::FromStr>(name: &str, default: T) -> T
where
    T::Err: std::fmt::Display,
{
    env::var(name)
        .map(|value| {
            value
                .parse::<T>()
                .unwrap_or_else(|error| panic!("invalid {name}={value:?}: {error}"))
        })
        .unwrap_or(default)
}

fn model_tags() -> Vec<String> {
    env::var("SHARPEBENCH_LOCAL_MODELS")
        .expect("SHARPEBENCH_LOCAL_MODELS is required (comma-separated exact Ollama tags)")
        .split(',')
        .map(str::trim)
        .filter(|tag| !tag.is_empty())
        .map(str::to_string)
        .collect::<Vec<_>>()
}

fn safe_name(model: &str) -> String {
    model
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || character == '-' || character == '_' {
                character
            } else {
                '-'
            }
        })
        .collect()
}

fn windows_for(n: usize) -> (Vec<Window>, usize) {
    let warmup = (n / 10).clamp(20, 60);
    let test = ((n - warmup) / 6).max(20);
    (walk_forward(n, warmup, test, test), test)
}

fn reference_field(data: &Dataset, windows: &[Window]) -> Vec<AgentSubmission> {
    let agents: Vec<(&str, AgentFactory)> = vec![
        ("buy-and-hold", Box::new(|| Box::new(BuyAndHold))),
        ("momentum", Box::new(|| Box::new(Momentum::default()))),
        ("hold", Box::new(|| Box::new(HoldAgent))),
    ];
    let mut submissions: Vec<AgentSubmission> = agents
        .into_iter()
        .map(|(agent_id, make)| {
            let mut runs = Vec::new();
            for window in windows {
                for seed in EXEC_SEEDS {
                    let mut agent = make();
                    runs.push(run_backtest(
                        data,
                        agent.as_mut(),
                        *window,
                        seed,
                        CostModel::default(),
                    ));
                }
            }
            AgentSubmission {
                agent_id: agent_id.to_string(),
                runs,
                in_sample_trials: 0,
                candidates: Vec::new(),
            }
        })
        .collect();
    submissions.extend(luck_floor(
        data,
        windows,
        &EXEC_SEEDS,
        CostModel::default(),
        LUCK_FLOOR_AGENTS,
    ));
    submissions
}

/// The shim module, owned by the sibling SharpeArena repository.
const SHIM_MODULE: &str = "sharpearena.ollama_shim";

/// Preflight the cross-repo dependency: can `python` import the shim at all?
///
/// Without this, an absent `sharpearena` package surfaces only once the field is
/// already running, as `ExternalAgent::spawn(...).ok()` returning `None` and
/// then a `FailureKind::SpawnError` panic that names neither the module nor the
/// interpreter. The failure is correct (it fails closed) but unactionable.
fn probe_shim(python: &str) -> Result<(), String> {
    let import = format!("import {SHIM_MODULE}");
    let outcome = std::process::Command::new(python)
        .args(["-c", &import])
        .output();
    let detail = match outcome {
        Err(error) => format!("cannot run the interpreter {python:?}: {error}"),
        Ok(output) if output.status.success() => return Ok(()),
        Ok(output) => String::from_utf8_lossy(&output.stderr).trim().to_string(),
    };
    Err(format!(
        "{SHIM_MODULE} is not importable by {python:?}: {detail}\n\
         This example is a bridge to the sibling SharpeArena repository and cannot run without \
         it. Install the shim (`pip install sharpearena`, or `pip install -e` against a local \
         checkout), or point SHARPEARENA_PYTHON at an interpreter that already has it."
    ))
}

fn read_identity(path: &Path) -> ModelIdentity {
    let text = std::fs::read_to_string(path)
        .unwrap_or_else(|error| panic!("cannot read model identity {}: {error}", path.display()));
    serde_json::from_str(&text)
        .unwrap_or_else(|error| panic!("invalid model identity {}: {error}", path.display()))
}

fn main() {
    // Required positional; see evidence_sweep for why there is no default.
    let out = env::args().nth(1).unwrap_or_else(|| {
        eprintln!("usage: local_open_weight_field_eval <out.jsonl> [dataset]");
        std::process::exit(2);
    });
    let only = env::args().nth(2);
    let models = model_tags();
    assert!(
        !models.is_empty(),
        "SHARPEBENCH_LOCAL_MODELS contains no tags"
    );
    let python = env::var("SHARPEARENA_PYTHON").unwrap_or_else(|_| "python".to_string());
    // Fail before touching any dataset: the shim is the whole model path, and a
    // run that cannot reach it has nothing to produce.
    if let Err(diagnostic) = probe_shim(&python) {
        eprintln!("{diagnostic}");
        std::process::exit(2);
    }
    let cadence: u32 = env_parse("SHARPEBENCH_LOCAL_CADENCE", 5);
    let thinking: bool = env_parse("SHARPEBENCH_LOCAL_THINKING", false);
    let max_tokens: u32 = env_parse("SHARPEBENCH_LOCAL_MAX_TOKENS", 512);
    let timeout_seconds: u64 = env_parse("SHARPEBENCH_LOCAL_TIMEOUT_SECONDS", 120);
    let n_trials: u32 = env_parse("SHARPEBENCH_LOCAL_N_TRIALS", 1);
    assert!(cadence > 0 && max_tokens > 0 && n_trials > 0);

    let partial = format!("{out}.partial");
    let identity_dir = PathBuf::from(format!("{out}.identities"));
    std::fs::create_dir_all(&identity_dir).expect("create model identity directory");
    let mut writer = BufWriter::new(File::create(&partial).expect("create partial output"));
    let mut n_records = 0usize;

    for (name, asset_class, timeframe, periods_per_year) in DATASETS {
        if only.as_ref().is_some_and(|selected| selected != name) {
            continue;
        }
        let data = Dataset::from_csv_file(&format!("data/{name}.csv"))
            .unwrap_or_else(|error| panic!("load data/{name}.csv: {error}"));
        let (windows, window_len) = windows_for(data.len());
        let regimes = windows
            .iter()
            .map(|window| format!("{:?}", tag_regime(&data, *window)))
            .collect::<Vec<_>>();
        let mut submissions = reference_field(&data, &windows);
        let mut model_meta = Vec::new();

        for model in &models {
            let agent_id = format!("local-{}", safe_name(model));
            let identity_path = identity_dir.join(format!("{}.json", safe_name(model)));
            let identity_arg = identity_path.to_string_lossy().into_owned();
            let cadence_arg = cadence.to_string();
            let max_tokens_arg = max_tokens.to_string();
            let shim_timeout_arg = timeout_seconds.to_string();
            let mut owned_args = vec![
                "-m".to_string(),
                "sharpearena.ollama_shim".to_string(),
                "--model".to_string(),
                model.clone(),
                "--decision-cadence".to_string(),
                cadence_arg,
                "--max-tokens".to_string(),
                max_tokens_arg,
                "--timeout-seconds".to_string(),
                shim_timeout_arg,
                "--identity-out".to_string(),
                identity_arg,
            ];
            if thinking {
                owned_args.push("--thinking".to_string());
            }
            let arg_refs = owned_args.iter().map(String::as_str).collect::<Vec<_>>();
            eprintln!("{name}: running {agent_id} ({model})");
            let result = sharpebench_harness::run_external_agent(
                &agent_id,
                &data,
                &windows,
                &EXEC_SEEDS,
                CostModel::default(),
                EXTERNAL_MAX_RETRIES,
                || {
                    // Hermetic spawn; the shim resolves as an installed module
                    // or via PYTHONPATH, and talks to a possibly non-default
                    // Ollama endpoint — those two names pass through, no more.
                    ExternalAgent::spawn_with_env(
                        &python,
                        &arg_refs,
                        &["PYTHONPATH", "OLLAMA_HOST"],
                    )
                    .ok()
                    .map(|agent| agent.with_decide_timeout(Duration::from_secs(timeout_seconds)))
                },
            );
            if result.failures.runtime_failures() > 0 {
                panic!(
                    "{name}: incomplete local field: {} runtime failure(s) for {agent_id}; refusing to publish partial evidence",
                    result.failures.runtime_failures()
                );
            }
            let protocol_failures = result.failures.agent_faults();
            let identity = read_identity(&identity_path);
            model_meta.push((agent_id, model.clone(), identity, protocol_failures));
            submissions.insert(0, result.submission);
        }

        let config = ScoreConfig {
            dsr_bar: DSR_BAR,
            n_trials,
            execution_seeds_per_window: EXEC_SEEDS.len(),
            ..ScoreConfig::for_periods_per_year(*periods_per_year)
        };
        let mut never_catastrophic = config.clone();
        never_catastrophic.pass_mode = PassMode::Any;
        never_catastrophic.mandate.max_run_drawdown = NEVER_CATASTROPHIC_RUN_DD;
        let ablation = rank(&submissions, &never_catastrophic);
        for score in rank(&submissions, &config) {
            let alternate = ablation
                .iter()
                .find(|candidate| candidate.agent_id == score.agent_id)
                .expect("same field under both gates");
            let metadata = model_meta
                .iter()
                .find(|(agent_id, _, _, _)| *agent_id == score.agent_id);
            let record = Record {
                dataset: name,
                asset_class,
                timeframe,
                periods_per_year: *periods_per_year,
                n_bars: data.len(),
                n_symbols: data.symbols().len(),
                n_windows: windows.len(),
                window_len,
                n_seeds: EXEC_SEEDS.len(),
                regimes: regimes.clone(),
                dsr_bar: DSR_BAR,
                n_trials,
                sr_std_pinned: None,
                agent_id: score.agent_id.clone(),
                model: metadata.map(|(_, model, _, _)| model.clone()),
                model_identity: metadata.map(|(_, _, identity, _)| identity.clone()),
                decision_cadence: cadence,
                thinking,
                agent_protocol_failures: metadata.map_or(0, |(_, _, _, count)| *count),
                deflated_sharpe: score.deflated_sharpe,
                psr: score.psr,
                passed_k: score.passed_k,
                process_ok: score.process_ok,
                bootstrap_p: score.bootstrap_p,
                raw_mean_return: score.raw_mean_return,
                rank_eligible: score.rank_eligible,
                eligible_never_catastrophic: alternate.rank_eligible,
                worst_run_drawdown: score.worst_run_drawdown,
                field_reality_check_p: score.field_reality_check_p,
                step_down_significant: score.step_down_significant,
                trials_sr_std_used: score.trials_sr_std,
                trials_sr_std_source: match score.trials_sr_std_source {
                    TrialsSrStdSource::Measured => "measured".to_string(),
                    TrialsSrStdSource::MeasuredFloored => "measured_floored".to_string(),
                    TrialsSrStdSource::Configured => "configured".to_string(),
                },
            };
            serde_json::to_writer(&mut writer, &record).expect("write record");
            writer.write_all(b"\n").expect("write newline");
            n_records += 1;
        }
        writer.flush().expect("flush completed dataset");
    }
    drop(writer);
    std::fs::rename(&partial, &out).expect("publish completed field atomically");
    eprintln!("wrote {n_records} complete records to {out}");
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The cross-repo dependency must announce itself by name. Before this
    /// preflight existed, an absent `sharpearena` package reached the operator
    /// as `FailureKind::SpawnError` from inside the field loop, naming neither
    /// the module nor the interpreter.
    ///
    /// Driven with an interpreter that cannot exist, so the assertion holds on a
    /// machine that does have the shim installed.
    #[test]
    fn missing_shim_names_the_module_the_interpreter_and_the_fix() {
        let diagnostic = probe_shim("sharpebench-no-such-interpreter")
            .expect_err("a nonexistent interpreter cannot import the shim");
        assert!(
            diagnostic.contains(SHIM_MODULE),
            "the diagnostic must name the missing module, got: {diagnostic}"
        );
        assert!(
            diagnostic.contains("sharpebench-no-such-interpreter"),
            "the diagnostic must name the interpreter it tried, got: {diagnostic}"
        );
        assert!(
            diagnostic.contains("pip install sharpearena")
                && diagnostic.contains("SHARPEARENA_PYTHON"),
            "the diagnostic must state both remedies, got: {diagnostic}"
        );
    }

    /// The shim module path is the contract with the sibling repository. If it
    /// is renamed there, this pins where the corresponding edit belongs.
    #[test]
    fn shim_module_path_is_pinned() {
        assert_eq!(SHIM_MODULE, "sharpearena.ollama_shim");
    }

    /// A working interpreter that genuinely has the module reports Ok, so the
    /// preflight cannot be a blanket refusal. Runs only where the sibling
    /// package is installed; `#[ignore]` keeps the skip visible in the summary
    /// rather than counting as a pass.
    #[test]
    #[ignore = "needs the sibling SharpeArena package installed"]
    fn present_shim_passes_the_preflight() {
        let python = env::var("SHARPEARENA_PYTHON").unwrap_or_else(|_| "python".to_string());
        probe_shim(&python).expect("the shim is installed for this interpreter");
    }
}
