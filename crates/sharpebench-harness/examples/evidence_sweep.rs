//! Evidence sweep for the SharpeBench paper.
//!
//! Scores every frozen dataset with the reference agents plus a luck floor, across
//! a grid of scoring parameters, and writes one JSON record per (dataset, config,
//! agent) cell. The CLI's `run` hard-codes two windows, eight seeds and the default
//! `ScoreConfig`, which cannot answer the question the paper has to answer: on real
//! data, across asset classes and bar sizes, when is the deflated-Sharpe bar
//! satisfiable at all, and by whom? This goes through the same public API the
//! golden tests use, so every number is recomputable by anyone.
//!
//! Deterministic: no clock, no ambient RNG. Seeds and every parameter are pinned
//! below and emitted with each record. Run with
//!
//!   cargo run --release -p sharpebench-harness --example evidence_sweep -- <out.jsonl>
//!
//! The grid: dsr_bar in {0.80, 0.90, 0.95, 0.99}, n_trials in {1, 10, 50, 200},
//! and trials_sr_std measured from the field when the field is large enough (the
//! 0.2.0 default) or pinned to each of {0.20, 0.35, 0.50} with measurement off, so
//! the sensitivity of eligibility to that prior is visible rather than assumed.

use std::env;
use std::fs::File;
use std::io::{BufWriter, Write};

use serde::Serialize;
use sharpebench_core::composite::{rank, ScoreConfig, TrialsSrStdSource};
use sharpebench_core::AgentSubmission;
use sharpebench_core::PassMode;
use sharpebench_harness::luck_floor;
use sharpebench_sim::{
    run_backtest, tag_regime, walk_forward, Agent, BuyAndHold, CostModel, Dataset, HoldAgent,
    Momentum, Window,
};

/// Frozen datasets, their asset class and bar size. Timeframe is what the deflation
/// depends on: the same annualized Sharpe has a different track length at 1h and 1d.
/// The fourth column is periods per year, which is what the annualized deflation
/// prior is converted with. Getting it wrong is the single most consequential
/// misconfiguration in the benchmark, so it is pinned here per dataset rather
/// than inferred.
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

const DSR_BARS: &[f64] = &[0.80, 0.90, 0.95, 0.99];
const N_TRIALS: &[u32] = &[1, 10, 50, 200];
/// `None` = measure from the field (the 0.2.0 default); `Some(x)` = pin the prior.
const SR_STD: &[Option<f64>] = &[None, Some(0.20), Some(0.35), Some(0.50)];

const EXEC_SEEDS: [u64; 8] = [1, 2, 3, 4, 5, 6, 7, 8];
const LUCK_FLOOR_AGENTS: usize = 5;
/// Per-run drawdown bound for the never-catastrophic ablation. 20 percent is the
/// pooled cap the mandate docs use as their example, applied per regime.
const NEVER_CATASTROPHIC_RUN_DD: f64 = 0.20;

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
    effective_n_trials: u32,
    sr_std_pinned: Option<f64>,
    min_field_for_measured_sr_std: usize,
    dedup_clones_for_measured_sr_std: bool,
    min_measured_trials_sr_std_annualized: f64,
    deflation_null_mean_per_period: f64,
    agent_id: String,
    deflated_sharpe: f64,
    psr: f64,
    passed_k: bool,
    process_ok: bool,
    bootstrap_p: f64,
    raw_mean_return: f64,
    rank_eligible: bool,
    /// The same cell scored under the never-catastrophic preset: pass_mode Any
    /// and a 20 percent per-run drawdown bound. The two verdicts side by side are
    /// the ablation the paper turns on.
    eligible_never_catastrophic: bool,
    worst_run_drawdown: f64,
    field_reality_check_p: f64,
    field_spa_p: f64,
    field_spa_consistent_p: f64,
    step_down_significant: bool,
    trials_sr_std_used: f64,
    trials_sr_std_annualized_equivalent: f64,
    trials_sr_std_source: String,
    deflation_bar_per_period: f64,
    deflation_bar_annualized_equivalent: f64,
    pooled_observations: usize,
}

type AgentFactory = Box<dyn Fn() -> Box<dyn Agent>>;

/// Walk-forward windows sized to the dataset. Warmup and window scale with length so
/// a 24 000-bar hourly file and a 300-bar weekly file both get several disjoint
/// out-of-sample windows rather than one.
fn windows_for(n: usize) -> (Vec<Window>, usize) {
    let warmup = (n / 10).clamp(20, 60);
    let test = ((n - warmup) / 6).max(20);
    (walk_forward(n, warmup, test, test), test)
}

fn field(data: &Dataset, windows: &[Window]) -> Vec<AgentSubmission> {
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
    // Required positional. An evidence producer with a bare CWD-relative default
    // scatters a copy of a paper artifact wherever it happens to be run from,
    // which is one `git add` away from being committed as the real thing.
    let out = env::args().nth(1).unwrap_or_else(|| {
        eprintln!("usage: evidence_sweep <out.jsonl> [dataset] [dsr_bar]");
        std::process::exit(2);
    });
    let mut w = BufWriter::new(File::create(&out).expect("create output"));
    let mut n_records = 0usize;

    // Optional second argument: run only the named dataset. The sweep is
    // embarrassingly parallel across datasets and the hourly crypto file alone
    // takes longer than the other eight together, so one process per dataset is
    // how the evidence is actually produced. Results are identical to a serial
    // run because every seed is pinned.
    let only = env::args().nth(2);
    // Optional third argument: restrict to one dsr_bar, so the 64-config grid of a
    // single large dataset can be split across processes. The intraday crypto
    // files are the long pole; four processes per file instead of one.
    let only_bar: Option<f64> = env::args().nth(3).map(|b| b.parse().expect("dsr_bar"));
    for (name, class, tf, ppy) in DATASETS {
        if let Some(ref o) = only {
            if o != name {
                continue;
            }
        }
        let path = format!("data/{name}.csv");
        let data = match Dataset::from_csv_file(&path) {
            Ok(d) => d,
            Err(e) => {
                eprintln!("skip {name}: {e}");
                continue;
            }
        };
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
        let subs = field(&data, &windows);

        for &dsr_bar in DSR_BARS {
            if let Some(b) = only_bar {
                if (dsr_bar - b).abs() > 1e-9 {
                    continue;
                }
            }
            for &n_trials in N_TRIALS {
                for &pinned in SR_STD {
                    let mut cfg = ScoreConfig {
                        dsr_bar,
                        n_trials,
                        execution_seeds_per_window: EXEC_SEEDS.len(),
                        ..ScoreConfig::for_periods_per_year(*ppy)
                    };
                    if let Some(x) = pinned {
                        cfg.trials_sr_std = x;
                        // usize::MAX disables measurement: the field can never be that large.
                        cfg.min_field_for_measured_sr_std = usize::MAX;
                    }
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
                            dsr_bar,
                            n_trials,
                            effective_n_trials: s.effective_n_trials,
                            sr_std_pinned: pinned,
                            min_field_for_measured_sr_std: cfg.min_field_for_measured_sr_std,
                            dedup_clones_for_measured_sr_std: cfg.dedup_clones_for_measured_sr_std,
                            min_measured_trials_sr_std_annualized: cfg.min_measured_trials_sr_std,
                            deflation_null_mean_per_period: cfg.deflation_null_mean_per_period,
                            agent_id: s.agent_id.clone(),
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
                            field_spa_p: s.field_spa_p,
                            field_spa_consistent_p: s.field_spa_consistent_p,
                            step_down_significant: s.step_down_significant,
                            trials_sr_std_used: s.trials_sr_std,
                            trials_sr_std_annualized_equivalent: s
                                .trials_sr_std_annualized_equivalent,
                            trials_sr_std_source: match s.trials_sr_std_source {
                                TrialsSrStdSource::Measured => "measured".into(),
                                TrialsSrStdSource::MeasuredFloored => "measured_floored".into(),
                                TrialsSrStdSource::Configured => "configured".into(),
                            },
                            deflation_bar_per_period: s.deflation_bar_per_period,
                            deflation_bar_annualized_equivalent: s
                                .deflation_bar_annualized_equivalent,
                            pooled_observations: s.pooled_observations,
                        };
                        serde_json::to_writer(&mut w, &rec).expect("write record");
                        w.write_all(b"\n").expect("newline");
                        n_records += 1;
                    }
                }
            }
        }
    }
    w.flush().expect("flush");
    eprintln!("wrote {n_records} records to {out}");
}
