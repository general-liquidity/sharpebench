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

/// A filesystem- and identifier-safe encoding of an exact Ollama tag, injective
/// on bytes.
///
/// The previous encoding folded every character outside `[A-Za-z0-9-_]` to `-`,
/// so the distinct tags `a:b` and `a-b` both became `a-b`. That name is the
/// agent id, the identity-file stem, and the key both the model metadata and
/// the alternative-gate verdict are joined on by first match, so two colliding
/// entries would overwrite one identity artifact and one of them would be
/// published carrying the other's metadata and eligibility.
///
/// The encoding here is reversible, which is what makes it collision-free:
/// ASCII alphanumerics and `-` stand for themselves, `_` doubles to `__`, and
/// every other byte becomes `_x` followed by two lowercase hex digits. Reading
/// left to right, a `_` is followed either by `_` (one underscore) or by `x`
/// and two hex digits (one byte), so no two byte strings can encode alike.
fn safe_name(model: &str) -> String {
    let mut encoded = String::with_capacity(model.len());
    for byte in model.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' => encoded.push(byte as char),
            b'_' => encoded.push_str("__"),
            other => encoded.push_str(&format!("_x{other:02x}")),
        }
    }
    encoded
}

/// Refuse a model list whose entries cannot be told apart downstream.
///
/// Two checks, because they fail for different reasons: a tag repeated in
/// `SHARPEBENCH_LOCAL_MODELS` would run the same model twice under one id, and
/// two distinct tags encoding alike would silently merge two models' evidence.
/// The second is impossible while `safe_name` stays injective, and is asserted
/// anyway so that a future edit relaxing the encoding fails here instead of in
/// a published field.
fn check_model_ids(models: &[String]) -> Result<(), String> {
    let mut seen: Vec<(&str, String)> = Vec::new();
    for model in models {
        let id = format!("local-{}", safe_name(model));
        if let Some((first, _)) = seen.iter().find(|(tag, _)| *tag == model.as_str()) {
            return Err(format!(
                "SHARPEBENCH_LOCAL_MODELS repeats the tag {first:?}; each model may appear once"
            ));
        }
        if let Some((first, _)) = seen.iter().find(|(_, other)| *other == id) {
            return Err(format!(
                "SHARPEBENCH_LOCAL_MODELS tags {first:?} and {model:?} both encode to the agent id {id:?}"
            ));
        }
        seen.push((model.as_str(), id));
    }
    Ok(())
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

/// A set, non-empty value makes the producer report what it would do and stop
/// before the shim probe, before any model is loaded and before any output.
const DRY_RUN: &str = "SHARPEBENCH_DRY_RUN";

/// Refuse a dataset selector that names no known dataset.
///
/// The field loop skips every dataset the selector does not match, so a
/// misspelled name selected nothing, wrote nothing and still published: the
/// empty partial was renamed into place and its zero records were reported as a
/// complete field. The name is checked against the same table the loop walks,
/// before the output or the identity directory exists.
fn check_dataset_selector(only: Option<&str>) -> Result<(), String> {
    let Some(selected) = only else {
        return Ok(());
    };
    if DATASETS.iter().any(|(known, ..)| *known == selected) {
        return Ok(());
    }
    let known = DATASETS
        .iter()
        .map(|(name, ..)| *name)
        .collect::<Vec<_>>()
        .join(", ");
    Err(format!(
        "unknown dataset {selected:?}; this producer knows: {known}"
    ))
}

/// Refuse to publish a field that scored nothing.
///
/// Independent of the selector check: that one catches a name nobody has, this
/// one catches a run whose planned support legitimately produced no scored
/// record. Either way an empty file under the published name is a field that
/// says every model was evaluated and none placed.
fn check_records_written(n_records: usize) -> Result<(), String> {
    if n_records == 0 {
        return Err("the planned support produced no records; there is no field to publish".into());
    }
    Ok(())
}

/// The effective configuration a ready run would use.
#[derive(Debug, PartialEq)]
struct LocalPlan {
    models: Vec<String>,
    python: String,
    cadence: u32,
    thinking: bool,
    max_tokens: u32,
    timeout_seconds: u64,
    n_trials: u32,
    dry_run: bool,
}

/// Read one optional control, refusing an unparseable or non-positive value
/// rather than panicking or silently falling back to the default.
fn control<T: std::str::FromStr + PartialOrd + Default>(
    lookup: &dyn Fn(&str) -> Option<String>,
    name: &str,
    default: T,
) -> Result<T, String> {
    let Some(raw) = lookup(name) else {
        return Ok(default);
    };
    let value = raw
        .trim()
        .parse::<T>()
        .map_err(|_| format!("invalid {name}={raw:?}"))?;
    if value <= T::default() {
        return Err(format!("{name}={raw:?} must be positive"));
    }
    Ok(value)
}

/// Preflight the effective configuration.
///
/// Pure in `lookup`, and it starts no interpreter: the refusal paths are
/// testable with no environment, no Ollama and no model installed anywhere.
fn plan_local(lookup: &dyn Fn(&str) -> Option<String>) -> Result<LocalPlan, String> {
    let raw = lookup("SHARPEBENCH_LOCAL_MODELS").ok_or(
        "SHARPEBENCH_LOCAL_MODELS is required (comma-separated exact Ollama tags)".to_string(),
    )?;
    let models: Vec<String> = raw
        .split(',')
        .map(str::trim)
        .filter(|tag| !tag.is_empty())
        .map(str::to_string)
        .collect();
    if models.is_empty() {
        return Err("SHARPEBENCH_LOCAL_MODELS contains no tags".to_string());
    }
    // Two entries that cannot be told apart downstream would share one identity
    // artifact and cross their metadata, so this is refused before any model
    // is started rather than discovered in a published field.
    check_model_ids(&models)?;
    let thinking = match lookup("SHARPEBENCH_LOCAL_THINKING") {
        None => false,
        Some(raw) => raw
            .trim()
            .parse::<bool>()
            .map_err(|_| format!("invalid SHARPEBENCH_LOCAL_THINKING={raw:?}"))?,
    };
    Ok(LocalPlan {
        models,
        python: lookup("SHARPEARENA_PYTHON").unwrap_or_else(|| "python".to_string()),
        cadence: control(lookup, "SHARPEBENCH_LOCAL_CADENCE", 5)?,
        thinking,
        max_tokens: control(lookup, "SHARPEBENCH_LOCAL_MAX_TOKENS", 512)?,
        timeout_seconds: control(lookup, "SHARPEBENCH_LOCAL_TIMEOUT_SECONDS", 120)?,
        n_trials: control(lookup, "SHARPEBENCH_LOCAL_N_TRIALS", 1)?,
        dry_run: lookup(DRY_RUN).is_some_and(|value| !value.trim().is_empty()),
    })
}

fn main() {
    // Required positional; see evidence_sweep for why there is no default.
    let out = env::args().nth(1).unwrap_or_else(|| {
        eprintln!("usage: local_open_weight_field_eval <out.jsonl> [dataset]");
        std::process::exit(2);
    });
    let only = env::args().nth(2);
    // Every configuration refusal happens here, before an interpreter is
    // started and before any model is loaded.
    let plan = match plan_local(&|name| env::var(name).ok()) {
        Ok(plan) => plan,
        Err(diagnostic) => {
            eprintln!("refusing to run: {diagnostic}");
            std::process::exit(2);
        }
    };
    if let Err(diagnostic) = check_dataset_selector(only.as_deref()) {
        eprintln!("refusing to run: {diagnostic}");
        std::process::exit(2);
    }
    if plan.dry_run {
        println!(
            "{}",
            serde_json::json!({
                "would_run": {
                    "models": plan.models,
                    "dataset": only,
                    "python": plan.python,
                    "cadence": plan.cadence,
                    "thinking": plan.thinking,
                    "max_tokens": plan.max_tokens,
                    "timeout_seconds": plan.timeout_seconds,
                    "n_trials": plan.n_trials,
                    "output": out,
                },
                "shim_probed": false,
                "models_loaded": 0,
            })
        );
        return;
    }
    let LocalPlan {
        models,
        python,
        cadence,
        thinking,
        max_tokens,
        timeout_seconds,
        n_trials,
        dry_run: _,
    } = plan;
    // Fail before touching any dataset: the shim is the whole model path, and a
    // run that cannot reach it has nothing to produce.
    if let Err(diagnostic) = probe_shim(&python) {
        eprintln!("{diagnostic}");
        std::process::exit(2);
    }

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
    if let Err(diagnostic) = check_records_written(n_records) {
        eprintln!(
            "refusing to publish {out}: {diagnostic}; the empty partial is left at {partial}"
        );
        std::process::exit(2);
    }
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

    /// A dataset name nobody has is refused before anything is created.
    ///
    /// The loop skips a dataset the selector does not match, so a typo used to
    /// select nothing, and the empty partial was renamed into place and its
    /// zero records announced as a complete field. The diagnostic names the
    /// datasets that do exist, because the whole failure is a misspelling.
    #[test]
    fn an_unknown_dataset_selector_is_refused_and_names_the_known_ones() {
        let diagnostic = check_dataset_selector(Some("us-indicies-1d"))
            .expect_err("a misspelled dataset selects nothing and cannot run");
        assert!(
            diagnostic.contains("us-indicies-1d"),
            "the diagnostic must quote the name it refused, got: {diagnostic}"
        );
        for (known, ..) in DATASETS {
            assert!(
                diagnostic.contains(known),
                "the diagnostic must list {known}, got: {diagnostic}"
            );
        }
    }

    /// The refusal cannot be a blanket one: every shipped name still runs, and
    /// so does the all-datasets form that passes no name at all.
    #[test]
    fn every_known_dataset_selector_is_accepted() {
        check_dataset_selector(None).expect("no selector runs every dataset");
        for (known, ..) in DATASETS {
            check_dataset_selector(Some(known)).unwrap_or_else(|error| {
                panic!("{known} is a shipped dataset and must be accepted: {error}")
            });
        }
    }

    /// A field with no records is not published, whatever produced the emptiness.
    ///
    /// The second, independent gate: the selector check catches a name nobody
    /// has, and this catches a planned support that legitimately scored
    /// nothing. Without it the rename published a zero-record file under the
    /// completed name and the run reported success.
    #[test]
    fn an_empty_field_is_not_publishable_but_a_scored_one_is() {
        let diagnostic =
            check_records_written(0).expect_err("a zero-record field cannot be published");
        assert!(
            diagnostic.contains("no field to publish"),
            "the diagnostic must say nothing is published, got: {diagnostic}"
        );
        check_records_written(1).expect("a scored field publishes");
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

    /// The reported collision: two accepted Ollama tags that folded to one
    /// agent id, one identity path, and one first-match metadata join.
    #[test]
    fn colon_and_dash_tags_no_longer_encode_alike() {
        assert_ne!(safe_name("a:b"), safe_name("a-b"));
        assert_eq!(safe_name("a-b"), "a-b");
    }

    /// Injectivity over every byte a tag can hold, checked on the whole ASCII
    /// alphabet plus the shapes Ollama tags actually take.
    #[test]
    fn distinct_tags_never_encode_alike() {
        let mut tags: Vec<String> = (0u8..=127).map(|b| format!("m{}", b as char)).collect();
        tags.extend(
            [
                "llama3.1:8b",
                "llama3.1-8b",
                "llama3_1:8b",
                "llama3__1-8b",
                "qwen2.5:14b-instruct",
                "qwen2-5-14b-instruct",
                "a_xb",
                "a_x5fb",
            ]
            .into_iter()
            .map(str::to_string),
        );
        let mut seen: std::collections::HashMap<String, String> = std::collections::HashMap::new();
        for tag in &tags {
            if let Some(other) = seen.insert(safe_name(tag), tag.clone()) {
                assert_eq!(&other, tag, "{other:?} and {tag:?} encode alike");
            }
        }
    }

    /// The encoded name is still a usable file stem and agent id: nothing in it
    /// can be a path separator, a drive marker or a wildcard.
    #[test]
    fn encoded_names_stay_filesystem_safe() {
        for tag in ["llama3.1:8b", "a/b", r"a\b", "a b", "a*b", "..", "a_b"] {
            let encoded = safe_name(tag);
            assert!(
                encoded
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_'),
                "{tag:?} encoded to {encoded:?}"
            );
        }
    }

    /// A repeated tag is refused before any model starts, rather than running
    /// the same model twice under one id.
    #[test]
    fn a_repeated_tag_is_refused() {
        let models = ["llama3.1:8b".to_string(), "llama3.1:8b".to_string()];
        let diagnostic = check_model_ids(&models).expect_err("a repeated tag must be refused");
        assert!(diagnostic.contains("llama3.1:8b"), "got: {diagnostic}");
    }

    /// The tags the finding named pass the duplicate check now that they encode
    /// differently, so the guard is not a blanket refusal.
    #[test]
    fn distinct_tags_are_accepted() {
        let models = ["a:b".to_string(), "a-b".to_string()];
        check_model_ids(&models).expect("distinct tags must be accepted");
    }

    /// A pure environment: no process environment, no interpreter, no Ollama and
    /// no model installed anywhere.
    fn env_of<'a>(pairs: &'a [(&'a str, &'a str)]) -> impl Fn(&str) -> Option<String> + 'a {
        move |name| {
            pairs
                .iter()
                .find(|(key, _)| *key == name)
                .map(|(_, value)| (*value).to_string())
        }
    }

    /// A ready configuration reports its effective values, including the
    /// defaults an operator did not override.
    #[test]
    fn a_ready_configuration_reports_the_effective_plan() {
        let plan = plan_local(&env_of(&[("SHARPEBENCH_LOCAL_MODELS", "a:1, b:2")]))
            .expect("a declared model list plans");
        assert_eq!(plan.models, vec!["a:1".to_string(), "b:2".to_string()]);
        assert_eq!(plan.python, "python");
        assert_eq!(plan.cadence, 5);
        assert_eq!(plan.max_tokens, 512);
        assert_eq!(plan.timeout_seconds, 120);
        assert_eq!(plan.n_trials, 1);
        assert!(!plan.thinking);
        assert!(!plan.dry_run);

        let overridden = plan_local(&env_of(&[
            ("SHARPEBENCH_LOCAL_MODELS", "a:1"),
            ("SHARPEARENA_PYTHON", "python3.12"),
            ("SHARPEBENCH_LOCAL_CADENCE", "3"),
            ("SHARPEBENCH_LOCAL_THINKING", "true"),
            ("SHARPEBENCH_LOCAL_MAX_TOKENS", "256"),
            ("SHARPEBENCH_LOCAL_TIMEOUT_SECONDS", "60"),
            ("SHARPEBENCH_LOCAL_N_TRIALS", "4"),
        ]))
        .expect("an overridden configuration plans");
        assert_eq!(overridden.python, "python3.12");
        assert_eq!(overridden.cadence, 3);
        assert!(overridden.thinking);
        assert_eq!(overridden.max_tokens, 256);
        assert_eq!(overridden.timeout_seconds, 60);
        assert_eq!(overridden.n_trials, 4);
    }

    /// The model list is required. An absent or effectively empty list refuses
    /// rather than publishing a field with no model in it.
    #[test]
    fn a_missing_model_list_refuses() {
        let absent = plan_local(&env_of(&[])).expect_err("no model list refuses");
        assert!(absent.contains("SHARPEBENCH_LOCAL_MODELS"), "{absent}");
        for value in ["", " , , "] {
            assert!(
                plan_local(&env_of(&[("SHARPEBENCH_LOCAL_MODELS", value)])).is_err(),
                "{value:?} declares no model"
            );
        }
    }

    /// A repeated tag, or two tags that cannot be told apart downstream, refuse
    /// before any model is loaded.
    #[test]
    fn an_unusable_model_list_refuses() {
        let repeated = plan_local(&env_of(&[("SHARPEBENCH_LOCAL_MODELS", "a:1,a:1")]))
            .expect_err("a repeated tag refuses");
        assert!(repeated.contains("repeats"), "{repeated}");
        assert!(check_model_ids(&["a:1".to_string(), "a-1".to_string()]).is_ok());
    }

    /// A control the host cannot use is a refusal that names it, not a panic
    /// and not a silent fallback to the default.
    #[test]
    fn an_unusable_control_refuses_and_names_itself() {
        for (name, value) in [
            ("SHARPEBENCH_LOCAL_CADENCE", "0"),
            ("SHARPEBENCH_LOCAL_CADENCE", "often"),
            ("SHARPEBENCH_LOCAL_MAX_TOKENS", "0"),
            ("SHARPEBENCH_LOCAL_TIMEOUT_SECONDS", "-1"),
            ("SHARPEBENCH_LOCAL_N_TRIALS", "0"),
            ("SHARPEBENCH_LOCAL_THINKING", "yes"),
        ] {
            let error = plan_local(&env_of(&[
                ("SHARPEBENCH_LOCAL_MODELS", "a:1"),
                (name, value),
            ]))
            .expect_err("an unusable control refuses");
            assert!(error.contains(name), "{error} should name {name}");
        }
    }

    /// The dry run is a plan, not a run, and it is not a way around the
    /// refusals: an unready configuration still refuses with the switch set.
    #[test]
    fn the_dry_run_switch_produces_a_plan_without_probing_the_shim() {
        let plan = plan_local(&env_of(&[
            ("SHARPEBENCH_LOCAL_MODELS", "a:1"),
            (DRY_RUN, "1"),
        ]))
        .expect("a dry run still needs a ready configuration");
        assert!(plan.dry_run);
        assert_eq!(plan.models, vec!["a:1".to_string()]);
        assert!(plan_local(&env_of(&[(DRY_RUN, "1")])).is_err());
    }
}
