//! First LLM field for the SharpeBench paper.
//!
//! Drives external LLM agents (examples/llm-agent/llm_agent.py speaking the
//! stdio ExternalAgent protocol; one submission per model in LLM_MODELS:
//! Claude Fable 5, Claude Opus 5, Claude Haiku 4.5) through the same
//! walk-forward windows,
//! seeds, reference field, luck floor, and scoring kernel as the paper's
//! evidence sweep, on two datasets only (cost scoping): us-indices-1d and
//! crypto-majors-1d. One scoring config uses a host floor of one and a DSR bar
//! of 0.80; ranking still deflates for at least the observed field size under
//! the same measured-dispersion safeguards as every other ranked field.
//!
//! Run from the repo root with ANTHROPIC_API_KEY (and optionally LLM_CACHE_DIR /
//! LLM_STATS_DIR / LLM_STRIDE / LLM_MAX_CALLS) exported. The run fails closed:
//! a provider/transport error or an exhausted call budget aborts the field, and
//! only a completely evaluated field is renamed to the requested output.
//!
//!   cargo run --release -p sharpebench-harness --example llm_field_eval -- <out.jsonl> [dataset]

use std::env;
use std::fs::File;
use std::io::{BufWriter, Write};
use std::time::Duration;

use serde::Serialize;
use sharpebench_core::composite::{rank, ScoreConfig, TrialsSrStdSource};
use sharpebench_core::AgentSubmission;
use sharpebench_core::PassMode;
use sharpebench_harness::luck_floor;
use sharpebench_sim::{
    run_backtest, tag_regime, walk_forward, Agent, BuyAndHold, CostModel, Dataset, ExternalAgent,
    HoldAgent, Momentum, Window,
};

const DATASETS: &[(&str, &str, &str, f64)] = &[
    ("us-indices-1d", "equity-index", "1d", 252.0),
    ("crypto-majors-1d", "crypto", "1d", 365.0),
];

const DSR_BAR: f64 = 0.80;
const N_TRIALS: u32 = 1;
const EXEC_SEEDS: [u64; 8] = [1, 2, 3, 4, 5, 6, 7, 8];
const LUCK_FLOOR_AGENTS: usize = 5;
const NEVER_CATASTROPHIC_RUN_DD: f64 = 0.20;
/// The three-model field, in budget priority order: if spend forces a cut,
/// the frontier model completes first. The model id is handed to the agent
/// script as argv[1]; the script keys its response cache per model.
const LLM_MODELS: &[&str] = &[
    "claude-fable-5",
    "claude-opus-5",
    "claude-haiku-4-5-20251001",
];
const LLM_SCRIPT: &str = "examples/llm-agent/llm_agent.py";
const EXTERNAL_MAX_RETRIES: u32 = 2;
/// Generous per-decision budget: an API round trip (frontier-tier thinking
/// included) plus SDK retries.
const LLM_DECIDE_TIMEOUT: Duration = Duration::from_secs(180);

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
    /// The LLM behind the agent, for LLM rows; None for reference agents and
    /// the luck floor.
    #[serde(skip_serializing_if = "Option::is_none")]
    model: Option<String>,
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
    let mut subs: Vec<AgentSubmission> = agents
        .into_iter()
        .map(|(id, make)| {
            let mut runs = Vec::new();
            for w in windows {
                for seed in EXEC_SEEDS {
                    let mut agent = make();
                    runs.push(run_backtest(
                        data,
                        agent.as_mut(),
                        *w,
                        seed,
                        CostModel::default(),
                    ));
                }
            }
            AgentSubmission {
                agent_id: id.to_string(),
                runs,
                in_sample_trials: 0,
                candidates: Vec::new(),
            }
        })
        .collect();
    subs.extend(luck_floor(
        data,
        windows,
        &EXEC_SEEDS,
        CostModel::default(),
        LUCK_FLOOR_AGENTS,
    ));
    subs
}

fn main() {
    let out = env::args()
        .nth(1)
        .unwrap_or_else(|| "llm-field-records.jsonl".to_string());
    let only = env::args().nth(2);
    let partial = format!("{out}.partial");
    let mut w = BufWriter::new(File::create(&partial).expect("create partial output"));
    let mut n_records = 0usize;

    for (name, class, tf, ppy) in DATASETS {
        if let Some(ref o) = only {
            if o != name {
                continue;
            }
        }
        let path = format!("data/{name}.csv");
        let data = Dataset::from_csv_file(&path).expect("load dataset");
        let n = data.len();
        let (windows, window_len) = windows_for(n);
        let regimes: Vec<String> = windows
            .iter()
            .map(|w| format!("{:?}", tag_regime(&data, *w)))
            .collect();
        eprintln!(
            "{name}: {n} bars, {} symbols, {} windows of {window_len}",
            data.symbols().len(),
            windows.len()
        );

        let mut subs = reference_field(&data, &windows);
        let mut model_by_agent: Vec<(String, String)> = Vec::new();

        for model in LLM_MODELS {
            let agent_id = format!("llm-{model}");
            eprintln!("{name}: running {agent_id}");
            let res = sharpebench_harness::run_external_agent(
                &agent_id,
                &data,
                &windows,
                &EXEC_SEEDS,
                CostModel::default(),
                EXTERNAL_MAX_RETRIES,
                || {
                    ExternalAgent::spawn("python", &[LLM_SCRIPT, model])
                        .ok()
                        .map(|a| a.with_decide_timeout(LLM_DECIDE_TIMEOUT))
                },
            );
            if !res.failures.is_empty() {
                panic!(
                    "{name}: incomplete field: {} transport failure(s) for {agent_id} ({} runtime, {} agent-fault); refusing to publish partial evidence",
                    res.failures.records.len(),
                    res.failures.runtime_failures(),
                    res.failures.agent_faults(),
                );
            }
            model_by_agent.push((agent_id, model.to_string()));
            subs.insert(0, res.submission);
        }

        let cfg = ScoreConfig {
            dsr_bar: DSR_BAR,
            n_trials: N_TRIALS,
            ..ScoreConfig::for_periods_per_year(*ppy)
        };
        let mut cfg_nc = cfg.clone();
        cfg_nc.pass_mode = PassMode::Any;
        cfg_nc.mandate.max_run_drawdown = NEVER_CATASTROPHIC_RUN_DD;
        let scored_nc = rank(&subs, &cfg_nc);
        for s in rank(&subs, &cfg) {
            let nc = scored_nc
                .iter()
                .find(|x| x.agent_id == s.agent_id)
                .expect("same field under both gates");
            let rec = Record {
                dataset: name,
                asset_class: class,
                timeframe: tf,
                periods_per_year: *ppy,
                n_bars: n,
                n_symbols: data.symbols().len(),
                n_windows: windows.len(),
                window_len,
                n_seeds: EXEC_SEEDS.len(),
                regimes: regimes.clone(),
                dsr_bar: DSR_BAR,
                n_trials: N_TRIALS,
                sr_std_pinned: None,
                agent_id: s.agent_id.clone(),
                model: model_by_agent
                    .iter()
                    .find(|(a, _)| *a == s.agent_id)
                    .map(|(_, m)| m.clone()),
                deflated_sharpe: s.deflated_sharpe,
                psr: s.psr,
                passed_k: s.passed_k,
                process_ok: s.process_ok,
                bootstrap_p: s.bootstrap_p,
                raw_mean_return: s.raw_mean_return,
                rank_eligible: s.rank_eligible,
                eligible_never_catastrophic: nc.rank_eligible,
                worst_run_drawdown: s.worst_run_drawdown,
                field_reality_check_p: s.field_reality_check_p,
                step_down_significant: s.step_down_significant,
                trials_sr_std_used: s.trials_sr_std,
                trials_sr_std_source: match s.trials_sr_std_source {
                    TrialsSrStdSource::Measured => "measured".into(),
                    TrialsSrStdSource::MeasuredFloored => "measured_floored".into(),
                    TrialsSrStdSource::Configured => "configured".into(),
                },
            };
            serde_json::to_writer(&mut w, &rec).expect("write record");
            w.write_all(b"\n").expect("newline");
            n_records += 1;
        }
        w.flush().expect("flush");
    }
    w.flush().expect("flush completed field");
    drop(w);
    std::fs::rename(&partial, &out).expect("publish completed field atomically");
    eprintln!("wrote {n_records} complete records to {out}");
}
