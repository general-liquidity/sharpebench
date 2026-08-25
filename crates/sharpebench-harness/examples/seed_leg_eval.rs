//! Seed-leg evaluation for the SharpeBench paper.
//!
//! The reliability verdict pass^k bundles two legs: across execution seeds of
//! one window, and across out-of-sample windows. Under the default cost model
//! the eight seeds vary only the base slippage draw, a few basis points, so the
//! seed leg is a narrow stability check and every refusal on the paper's grid
//! is attributable to the window leg. This example re-runs the reference field
//! on three daily datasets under the default profile and under
//! `CostProfile::Realistic` (seed-driven fill delay, partial fills and
//! queue-position slippage), records per-seed per-window PSR, and decomposes
//! each pass^k verdict by leg:
//!
//! * a window whose eight seeds all fail is a **window-leg** refusal (the
//!   regime fails regardless of execution);
//! * a window with some seeds passing and some failing is a **seed-leg**
//!   refusal (the verdict on that window hinges on the execution draw).
//!
//! It also reports the across-seed standard deviation of per-run annualized
//! Sharpe, so the reader can see how much execution noise the seed leg actually
//! resamples under each profile.
//!
//! Deterministic: no clock, no ambient RNG. Run with
//!
//!   cargo run --release -p sharpebench-harness --example seed_leg_eval -- <out.jsonl>

use std::env;
use std::fs::File;
use std::io::{BufWriter, Write};

use serde::Serialize;
use sharpebench_core::composite::{per_run_passes, rank, ScoreConfig};
use sharpebench_core::deflated_sharpe::{probabilistic_sharpe_ratio, sharpe_ratio};
use sharpebench_core::AgentSubmission;
use sharpebench_harness::luck_floor;
use sharpebench_sim::agent::RiskManaged;
use sharpebench_sim::{
    run_backtest, tag_regime, walk_forward, Agent, BuyAndHold, CostModel, CostProfile, Dataset,
    HoldAgent, Momentum, Window,
};

const COMMAND: &str = "cargo run --release -p sharpebench-harness --example seed_leg_eval -- paper/evidence/final/seed-leg.jsonl";

/// Three daily datasets from three market panels, with periods per year.
const DATASETS: &[(&str, &str, f64)] = &[
    ("us-indices-1d", "equity-index", 252.0),
    ("crypto-majors-1d", "crypto", 365.0),
    ("fx-majors-1d", "fx", 252.0),
];

const EXEC_SEEDS: [u64; 8] = [1, 2, 3, 4, 5, 6, 7, 8];
const LUCK_FLOOR_AGENTS: usize = 5;

const PROFILES: &[(&str, CostProfile)] = &[
    ("default", CostProfile::Typical),
    ("realistic", CostProfile::Realistic),
];

type AgentFactory = Box<dyn Fn() -> Box<dyn Agent>>;

#[derive(Serialize)]
struct RunRecord<'a> {
    kind: &'static str,
    command: &'static str,
    dataset: &'a str,
    asset_class: &'a str,
    periods_per_year: f64,
    profile: &'a str,
    agent_id: String,
    window: usize,
    regime: String,
    seed: u64,
    n_bars: usize,
    mean_return: f64,
    std_return: f64,
    sharpe_per_period: f64,
    sharpe_annualized: f64,
    psr: f64,
    passes: bool,
}

#[derive(Serialize)]
struct VerdictRecord<'a> {
    kind: &'static str,
    command: &'static str,
    dataset: &'a str,
    asset_class: &'a str,
    periods_per_year: f64,
    profile: &'a str,
    agent_id: String,
    n_windows: usize,
    n_seeds: usize,
    regimes: Vec<String>,
    /// Per window, how many of the eight seeds pass the per-run PSR bar.
    seeds_passing_per_window: Vec<usize>,
    windows_all_pass: usize,
    windows_mixed: usize,
    windows_all_fail: usize,
    passed_k: bool,
    /// Which leg refuses: "none", "windows", "seeds", or "both".
    refusing_leg: &'static str,
    /// True when the seed leg is the only refusing leg.
    seed_leg_only: bool,
    /// Across-seed sample std of per-run annualized Sharpe, per window.
    seed_sharpe_std_per_window: Vec<f64>,
    seed_sharpe_std_mean: f64,
    seed_sharpe_std_max: f64,
    /// Across-window sample std of the per-window mean annualized Sharpe, the
    /// scale the seed dispersion should be read against.
    window_sharpe_std: f64,
    mean_run_sharpe_annualized: f64,
    deflated_sharpe: f64,
    psr: f64,
    process_ok: bool,
    bootstrap_p: f64,
    worst_run_drawdown: f64,
    rank_eligible: bool,
}

/// Same window rule as the evidence sweep.
fn windows_for(n: usize) -> (Vec<Window>, usize) {
    let warmup = (n / 10).clamp(20, 60);
    let test = ((n - warmup) / 6).max(20);
    (walk_forward(n, warmup, test, test), test)
}

fn field(data: &Dataset, windows: &[Window], costs: CostModel) -> Vec<AgentSubmission> {
    let agents: Vec<(&str, AgentFactory)> = vec![
        ("buy-and-hold", Box::new(|| Box::new(BuyAndHold))),
        ("momentum", Box::new(|| Box::new(Momentum::default()))),
        ("hold", Box::new(|| Box::new(HoldAgent))),
        ("risk-managed", Box::new(|| Box::new(RiskManaged::new()))),
    ];
    let mut subs: Vec<AgentSubmission> = agents
        .into_iter()
        .map(|(id, make)| {
            let mut runs = Vec::new();
            for w in windows {
                for seed in EXEC_SEEDS {
                    let mut agent = make();
                    runs.push(run_backtest(data, agent.as_mut(), *w, seed, costs));
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
        costs,
        LUCK_FLOOR_AGENTS,
    ));
    subs
}

fn mean(xs: &[f64]) -> f64 {
    if xs.is_empty() {
        return 0.0;
    }
    xs.iter().sum::<f64>() / xs.len() as f64
}

fn sample_std(xs: &[f64]) -> f64 {
    if xs.len() < 2 {
        return 0.0;
    }
    let m = mean(xs);
    (xs.iter().map(|x| (x - m).powi(2)).sum::<f64>() / (xs.len() - 1) as f64).sqrt()
}

fn main() {
    let out = env::args()
        .nth(1)
        .unwrap_or_else(|| "seed-leg.jsonl".to_string());
    let mut w = BufWriter::new(File::create(&out).expect("create output"));
    let mut n_records = 0usize;

    for (name, class, ppy) in DATASETS {
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
            .map(|win| format!("{:?}", tag_regime(&data, *win)))
            .collect();
        eprintln!(
            "{name}: {n} bars, {} symbols, {} windows of {window_len}",
            data.symbols().len(),
            windows.len()
        );
        let cfg = ScoreConfig {
            execution_seeds_per_window: EXEC_SEEDS.len(),
            ..ScoreConfig::for_periods_per_year(*ppy)
        };
        let n_windows = windows.len();
        let n_seeds = EXEC_SEEDS.len();

        for (profile, cp) in PROFILES {
            let costs = cp.resolve().costs;
            let subs = field(&data, &windows, costs);
            let scored = rank(&subs, &cfg);
            println!("== {name} / {profile} ==");
            println!(
                "{:<14} {:>7} {:>6} {:>6} {:>6} {:>8} {:>9} {:>9} {:>9}",
                "agent", "pass^k", "allP", "mixed", "allF", "leg", "seedStd", "winStd", "meanSR"
            );
            for sub in &subs {
                let per_run = per_run_passes(sub, None, &cfg);
                let sc = scored
                    .iter()
                    .find(|s| s.agent_id == sub.agent_id)
                    .expect("scored");
                let mut seeds_passing = Vec::with_capacity(n_windows);
                let mut seed_std = Vec::with_capacity(n_windows);
                let mut window_means = Vec::with_capacity(n_windows);
                let mut all_sr = Vec::new();
                for (wi, win) in windows.iter().enumerate() {
                    let mut n_pass = 0usize;
                    let mut srs = Vec::with_capacity(n_seeds);
                    for (si, seed) in EXEC_SEEDS.iter().enumerate() {
                        let idx = wi * n_seeds + si;
                        let run = &sub.runs[idx];
                        let sr = sharpe_ratio(&run.returns);
                        let sr_ann = sr * ppy.sqrt();
                        let psr = probabilistic_sharpe_ratio(&run.returns, 0.0);
                        let passes = per_run[idx];
                        n_pass += usize::from(passes);
                        srs.push(sr_ann);
                        all_sr.push(sr_ann);
                        let rec = RunRecord {
                            kind: "seed_leg_run",
                            command: COMMAND,
                            dataset: name,
                            asset_class: class,
                            periods_per_year: *ppy,
                            profile,
                            agent_id: sub.agent_id.clone(),
                            window: wi,
                            regime: regimes[wi].clone(),
                            seed: *seed,
                            n_bars: win.end - win.start,
                            mean_return: mean(&run.returns),
                            std_return: sample_std(&run.returns),
                            sharpe_per_period: sr,
                            sharpe_annualized: sr_ann,
                            psr,
                            passes,
                        };
                        serde_json::to_writer(&mut w, &rec).expect("write record");
                        w.write_all(b"\n").expect("newline");
                        n_records += 1;
                    }
                    seeds_passing.push(n_pass);
                    seed_std.push(sample_std(&srs));
                    window_means.push(mean(&srs));
                }
                let windows_all_pass = seeds_passing.iter().filter(|&&k| k == n_seeds).count();
                let windows_all_fail = seeds_passing.iter().filter(|&&k| k == 0).count();
                let windows_mixed = n_windows - windows_all_pass - windows_all_fail;
                let refusing_leg = match (windows_all_fail > 0, windows_mixed > 0) {
                    (false, false) => "none",
                    (true, false) => "windows",
                    (false, true) => "seeds",
                    (true, true) => "both",
                };
                let seed_leg_only = refusing_leg == "seeds";
                let passed_k = per_run.iter().all(|&p| p);
                assert_eq!(
                    passed_k, sc.passed_k,
                    "per-run vector must agree with the kernel verdict"
                );
                assert_eq!(
                    passed_k,
                    refusing_leg == "none",
                    "leg decomposition must be exhaustive"
                );
                let rec = VerdictRecord {
                    kind: "seed_leg_verdict",
                    command: COMMAND,
                    dataset: name,
                    asset_class: class,
                    periods_per_year: *ppy,
                    profile,
                    agent_id: sub.agent_id.clone(),
                    n_windows,
                    n_seeds,
                    regimes: regimes.clone(),
                    seeds_passing_per_window: seeds_passing.clone(),
                    windows_all_pass,
                    windows_mixed,
                    windows_all_fail,
                    passed_k,
                    refusing_leg,
                    seed_leg_only,
                    seed_sharpe_std_per_window: seed_std.clone(),
                    seed_sharpe_std_mean: mean(&seed_std),
                    seed_sharpe_std_max: seed_std.iter().cloned().fold(0.0, f64::max),
                    window_sharpe_std: sample_std(&window_means),
                    mean_run_sharpe_annualized: mean(&all_sr),
                    deflated_sharpe: sc.deflated_sharpe,
                    psr: sc.psr,
                    process_ok: sc.process_ok,
                    bootstrap_p: sc.bootstrap_p,
                    worst_run_drawdown: sc.worst_run_drawdown,
                    rank_eligible: sc.rank_eligible,
                };
                println!(
                    "{:<14} {:>7} {:>6} {:>6} {:>6} {:>8} {:>9.4} {:>9.4} {:>9.4}",
                    rec.agent_id,
                    rec.passed_k,
                    rec.windows_all_pass,
                    rec.windows_mixed,
                    rec.windows_all_fail,
                    rec.refusing_leg,
                    rec.seed_sharpe_std_mean,
                    rec.window_sharpe_std,
                    rec.mean_run_sharpe_annualized
                );
                serde_json::to_writer(&mut w, &rec).expect("write record");
                w.write_all(b"\n").expect("newline");
                n_records += 1;
            }
        }
    }
    w.flush().expect("flush");
    eprintln!("wrote {n_records} records to {out}");
}
