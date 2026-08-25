//! Mandate-relative reliability check for the SharpeBench paper.
//!
//! The default reliability gate (pass^k in `All` mode: per-run PSR >= 0.90 in
//! every window) certifies an all-weather absolute-return mandate, which is why
//! the index itself cannot pass on any range containing a bear window. This
//! example scores the same field the risk-managed check uses (the three reference
//! agents, the risk-managed agent, and the luck floor) on all nine frozen
//! datasets under the default verdict and under
//! `PassMode::RelativeToBenchmark`, where each run's PSR is computed on its
//! excess return over buy-and-hold's run in the same (window, seed) cell. It
//! records, per agent and dataset, the full gate vector under both verdicts and
//! the per-window pass pattern, so the paper can say which windows the relative
//! verdict admits that the absolute one refused, and whether anyone becomes
//! eligible.
//!
//! Deterministic: no clock, no ambient RNG. Same window rule, seeds and cost
//! model as `evidence_sweep` and `risk_managed_eval`. Run with
//!
//!   cargo run --release -p sharpebench-harness --example relative_mandate_eval -- <out.jsonl>

use std::env;
use std::fs::File;
use std::io::{BufWriter, Write};

use serde::Serialize;
use sharpebench_core::composite::{per_run_passes, rank, ScoreConfig};
use sharpebench_core::AgentSubmission;
use sharpebench_harness::{luck_floor, run_agent};
use sharpebench_sim::agent::RiskManaged;
use sharpebench_sim::{
    tag_regime, walk_forward, Agent, BuyAndHold, CostModel, Dataset, HoldAgent, Momentum, Window,
};

/// Frozen datasets and their periods per year: same table as `evidence_sweep`.
const DATASETS: &[(&str, &str, f64)] = &[
    ("us-indices-1d", "1d", 252.0),
    ("us-indices-1w", "1w", 52.0),
    ("crypto-majors-1h", "1h", 8760.0),
    ("crypto-majors-4h", "4h", 2190.0),
    ("crypto-majors-1d", "1d", 365.0),
    ("crypto-majors-1w", "1w", 52.0),
    ("fx-majors-1d", "1d", 252.0),
    ("commodities-1d", "1d", 252.0),
    ("rates-1d", "1d", 252.0),
];

const EXEC_SEEDS: [u64; 8] = [1, 2, 3, 4, 5, 6, 7, 8];
const LUCK_FLOOR_AGENTS: usize = 5;
const BENCHMARK: &str = "buy-and-hold";
const COMMAND: &str = "cargo run --release -p sharpebench-harness --example relative_mandate_eval -- paper/evidence/final/relative-mandate.jsonl";

#[derive(Serialize)]
struct RelativeGateRecord<'a> {
    kind: &'static str,
    command: &'static str,
    dataset: &'a str,
    timeframe: &'a str,
    periods_per_year: f64,
    n_bars: usize,
    n_windows: usize,
    n_seeds: usize,
    regimes: Vec<String>,
    benchmark_agent_id: &'static str,
    agent_id: String,
    /// Statistics computed on the agent's own raw returns; identical under both
    /// verdicts, recorded once.
    deflated_sharpe: f64,
    psr: f64,
    process_ok: bool,
    bootstrap_p: f64,
    raw_mean_return: f64,
    worst_run_drawdown: f64,
    passed_k_default: bool,
    passed_k_relative: bool,
    rank_eligible_default: bool,
    rank_eligible_relative: bool,
    /// Per window: did every seed of that window clear the per-run bar?
    windows_passed_default: Vec<bool>,
    windows_passed_relative: Vec<bool>,
    /// Windows the relative verdict passes that the default refused.
    windows_gained: Vec<usize>,
    /// Windows the default passes that the relative verdict refuses.
    windows_lost: Vec<usize>,
    cells_passed_default: usize,
    cells_passed_relative: usize,
    cells_total: usize,
}

/// Same window rule as `evidence_sweep`: warmup n/10 clamped to 20..60, test
/// windows of (n - warmup)/6 with a 20-bar floor.
fn windows_for(n: usize) -> Vec<Window> {
    let warmup = (n / 10).clamp(20, 60);
    let test = ((n - warmup) / 6).max(20);
    walk_forward(n, warmup, test, test)
}

/// Collapse a window-major per-run vector to one bool per window: true iff
/// every seed of that window passed.
fn per_window(per_run: &[bool], n_seeds: usize) -> Vec<bool> {
    per_run
        .chunks(n_seeds)
        .map(|c| c.iter().all(|&b| b))
        .collect()
}

fn main() {
    let out = env::args()
        .nth(1)
        .unwrap_or_else(|| "relative_mandate_eval.jsonl".to_string());
    let mut w = BufWriter::new(File::create(&out).expect("create output"));
    let mut n_records = 0usize;

    for (name, tf, ppy) in DATASETS {
        let path = format!("data/{name}.csv");
        let data = match Dataset::from_csv_file(&path) {
            Ok(d) => d,
            Err(e) => {
                eprintln!("skip {name}: {e}");
                continue;
            }
        };
        let n = data.len();
        let windows = windows_for(n);
        let regimes: Vec<String> = windows
            .iter()
            .map(|w| format!("{:?}", tag_regime(&data, *w)))
            .collect();
        eprintln!("{name}: {n} bars, {} windows", windows.len());

        let mut subs: Vec<AgentSubmission> = vec![
            run_agent(
                BENCHMARK,
                &data,
                &windows,
                &EXEC_SEEDS,
                CostModel::default(),
                || Box::new(BuyAndHold) as Box<dyn Agent>,
            ),
            run_agent(
                "momentum",
                &data,
                &windows,
                &EXEC_SEEDS,
                CostModel::default(),
                || Box::new(Momentum::default()) as Box<dyn Agent>,
            ),
            run_agent(
                "hold",
                &data,
                &windows,
                &EXEC_SEEDS,
                CostModel::default(),
                || Box::new(HoldAgent) as Box<dyn Agent>,
            ),
            run_agent(
                "risk-managed",
                &data,
                &windows,
                &EXEC_SEEDS,
                CostModel::default(),
                || Box::new(RiskManaged::new()) as Box<dyn Agent>,
            ),
        ];
        subs.extend(luck_floor(
            &data,
            &windows,
            &EXEC_SEEDS,
            CostModel::default(),
            LUCK_FLOOR_AGENTS,
        ));
        let bench = subs
            .iter()
            .find(|s| s.agent_id == BENCHMARK)
            .expect("benchmark in field");

        let cfg = ScoreConfig {
            execution_seeds_per_window: EXEC_SEEDS.len(),
            ..ScoreConfig::for_periods_per_year(*ppy)
        };
        let mut cfg_rel = ScoreConfig::relative_to_benchmark(BENCHMARK);
        cfg_rel.periods_per_year = *ppy;
        cfg_rel.execution_seeds_per_window = EXEC_SEEDS.len();
        let scored = rank(&subs, &cfg);
        let scored_rel = rank(&subs, &cfg_rel);

        println!(
            "\n== {name} ({tf}, ppy {ppy}, {} windows: {}) ==",
            windows.len(),
            regimes.join(" ")
        );
        println!(
            "{:<16} {:>8} {:>8} {:>8} {:>9} {:>12} {:>12} {:>9} {:>9}",
            "agent",
            "dsr",
            "psr",
            "boot_p",
            "worst_dd",
            "win_default",
            "win_relative",
            "elig_def",
            "elig_rel"
        );
        for s in &scored {
            let rel = scored_rel
                .iter()
                .find(|x| x.agent_id == s.agent_id)
                .expect("same field under both verdicts");
            let sub = subs
                .iter()
                .find(|x| x.agent_id == s.agent_id)
                .expect("submission");
            let per_run_default = per_run_passes(sub, None, &cfg);
            let per_run_relative = per_run_passes(sub, Some(bench), &cfg_rel);
            let windows_passed_default = per_window(&per_run_default, EXEC_SEEDS.len());
            let windows_passed_relative = per_window(&per_run_relative, EXEC_SEEDS.len());
            let windows_gained: Vec<usize> = windows_passed_relative
                .iter()
                .zip(&windows_passed_default)
                .enumerate()
                .filter(|(_, (r, d))| **r && !**d)
                .map(|(i, _)| i)
                .collect();
            let windows_lost: Vec<usize> = windows_passed_relative
                .iter()
                .zip(&windows_passed_default)
                .enumerate()
                .filter(|(_, (r, d))| !**r && **d)
                .map(|(i, _)| i)
                .collect();
            let pattern =
                |v: &[bool]| -> String { v.iter().map(|&b| if b { 'P' } else { '.' }).collect() };
            println!(
                "{:<16} {:>8.4} {:>8.4} {:>8.4} {:>9.4} {:>12} {:>12} {:>9} {:>9}",
                s.agent_id,
                s.deflated_sharpe,
                s.psr,
                s.bootstrap_p,
                s.worst_run_drawdown,
                pattern(&windows_passed_default),
                pattern(&windows_passed_relative),
                s.rank_eligible,
                rel.rank_eligible,
            );
            let rec = RelativeGateRecord {
                kind: "relative_gate",
                command: COMMAND,
                dataset: name,
                timeframe: tf,
                periods_per_year: *ppy,
                n_bars: n,
                n_windows: windows.len(),
                n_seeds: EXEC_SEEDS.len(),
                regimes: regimes.clone(),
                benchmark_agent_id: BENCHMARK,
                agent_id: s.agent_id.clone(),
                deflated_sharpe: s.deflated_sharpe,
                psr: s.psr,
                process_ok: s.process_ok,
                bootstrap_p: s.bootstrap_p,
                raw_mean_return: s.raw_mean_return,
                worst_run_drawdown: s.worst_run_drawdown,
                passed_k_default: s.passed_k,
                passed_k_relative: rel.passed_k,
                rank_eligible_default: s.rank_eligible,
                rank_eligible_relative: rel.rank_eligible,
                windows_passed_default,
                windows_passed_relative,
                windows_gained,
                windows_lost,
                cells_passed_default: per_run_default.iter().filter(|&&b| b).count(),
                cells_passed_relative: per_run_relative.iter().filter(|&&b| b).count(),
                cells_total: per_run_default.len(),
            };
            serde_json::to_writer(&mut w, &rec).expect("write record");
            w.write_all(b"\n").expect("newline");
            n_records += 1;
        }
    }

    w.flush().expect("flush");
    eprintln!("\nwrote {n_records} records to {out}");
}
