//! Risk-managed eligibility check for the SharpeBench paper.
//!
//! The methodology paper's first stated limitation is that every entrant so far
//! is unhedged long-only, so nobody has ever been rank-eligible on real data and
//! eligibility might be vacuous. This example runs the [`RiskManaged`] reference
//! agent (trend filter + inverse-vol sizing + drawdown halt, documented
//! defaults, no tuning) beside buy-and-hold and the luck floor on all nine
//! frozen datasets, with the same window rule, seeds and cost model the
//! evidence sweep uses, and prints the full gate vector per agent per dataset
//! under both the default `ScoreConfig` and the never-catastrophic ablation.
//! It also runs the perturbation robustness report on one dataset, closing the
//! self-audit's "no perturbed windows" limit.
//!
//! Deterministic: no clock, no ambient RNG. Run with
//!
//!   cargo run --release -p sharpebench-harness --example risk_managed_eval -- <out.jsonl>
//!
//! The declared support is all nine datasets, the N-sensitivity block on its
//! anchor dataset and the perturbation report on its own. A dataset that failed
//! to load, or a missing perturbation section, used to warn on stderr and still
//! reach the ordinary output filename. The run is now staged through
//! `<out>.partial` and published only when every declared section is present;
//! otherwise it refuses with a non-zero exit.

use std::env;
use std::fs::File;
use std::io::{BufWriter, Write};

use serde::Serialize;
use sharpebench_core::composite::{rank, ScoreConfig};
use sharpebench_harness::perturb::perturbed_field;
use sharpebench_harness::{luck_floor, run_agent, TeamMember};
use sharpebench_sim::agent::RiskManaged;
use sharpebench_sim::{walk_forward, Agent, BuyAndHold, CostModel, Dataset, Window};

#[path = "support/declared_support.rs"]
mod declared_support;
use declared_support::{or_refuse, refuse, require_declared_member, require_evaluated};

/// Frozen datasets and their periods per year — same table as `evidence_sweep`.
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
const NEVER_CATASTROPHIC_RUN_DD: f64 = 0.20;

/// Trial counts for the deflation N-sensitivity report on the Finding-4 anchor
/// dataset: the honest field size (7 agents), 10, 25, the default 50, and 100.
const N_SENSITIVITY: &[u32] = &[7, 10, 25, 50, 100];
const N_SENSITIVITY_DATASET: &str = "us-indices-1w";

/// Dataset for the perturbation report: small enough to sweep quickly, real data.
const PERTURB_DATASET: &str = "us-indices-1w";
const PERTURB_PPY: f64 = 52.0;
const N_PERTURBATIONS: usize = 5;
const PERTURB_SEED: u64 = 7;

#[derive(Serialize)]
struct GateRecord<'a> {
    kind: &'static str,
    dataset: &'a str,
    timeframe: &'a str,
    periods_per_year: f64,
    n_bars: usize,
    n_windows: usize,
    agent_id: String,
    effective_n_trials: u32,
    trials_sr_std_used: f64,
    trials_sr_std_source: String,
    deflated_sharpe: f64,
    psr: f64,
    passed_k: bool,
    process_ok: bool,
    bootstrap_p: f64,
    raw_mean_return: f64,
    worst_run_drawdown: f64,
    rank_eligible: bool,
    eligible_never_catastrophic: bool,
}

#[derive(Serialize)]
struct NSensitivityRecord<'a> {
    kind: &'static str,
    dataset: &'a str,
    /// Host floor requested for this sensitivity cell.
    n_trials: u32,
    /// Actual count after the observable field-size floor.
    effective_n_trials: u32,
    agent_id: String,
    deflated_sharpe: f64,
    rank_eligible: bool,
    eligible_never_catastrophic: bool,
    trials_sr_std_used: f64,
}

#[derive(Serialize)]
struct PerturbRecord<'a> {
    kind: &'static str,
    dataset: &'a str,
    n_perturbations: usize,
    seed: u64,
    agent_id: String,
    original_psr: f64,
    best_perturbed_psr: f64,
    worst_perturbed_psr: f64,
    spread: f64,
}

/// Same window rule as `evidence_sweep`: warmup n/10 clamped to 20..60, test
/// windows of (n - warmup)/6 with a 20-bar floor.
fn windows_for(n: usize) -> Vec<Window> {
    let warmup = (n / 10).clamp(20, 60);
    let test = ((n - warmup) / 6).max(20);
    walk_forward(n, warmup, test, test)
}

fn main() {
    // Required positional; see evidence_sweep for why there is no default.
    let out = env::args().nth(1).unwrap_or_else(|| {
        eprintln!("usage: risk_managed_eval <out.jsonl>");
        std::process::exit(2);
    });
    // The two named constants are support this producer promises to report on.
    // Checking them against the dataset table before an output file exists keeps
    // a renamed dataset from becoming a silently absent section.
    let declared: Vec<&str> = DATASETS.iter().map(|(name, ..)| *name).collect();
    or_refuse(require_declared_member(
        "n-sensitivity dataset",
        &declared,
        N_SENSITIVITY_DATASET,
    ));
    or_refuse(require_declared_member(
        "perturbation dataset",
        &declared,
        PERTURB_DATASET,
    ));
    let planned: Vec<String> = declared.iter().map(|d| (*d).to_string()).collect();
    let partial = format!("{out}.partial");
    let mut w = BufWriter::new(File::create(&partial).expect("create partial output"));
    let mut evaluated: Vec<String> = Vec::new();
    let mut n_sensitivity_rows = 0usize;

    for (name, tf, ppy) in DATASETS {
        let path = format!("data/{name}.csv");
        let data = match Dataset::from_csv_file(&path) {
            Ok(d) => d,
            // Dropping a declared dataset leaves the remaining rows under a
            // filename that claims the whole nine-dataset evaluation.
            Err(e) => refuse(format!("declared dataset {name} did not load: {e}")),
        };
        let n = data.len();
        let windows = windows_for(n);
        eprintln!("{name}: {n} bars, {} windows", windows.len());

        let mut subs = vec![
            run_agent(
                "risk-managed",
                &data,
                &windows,
                &EXEC_SEEDS,
                CostModel::default(),
                || Box::new(RiskManaged::new()) as Box<dyn Agent>,
            ),
            run_agent(
                "buy-and-hold",
                &data,
                &windows,
                &EXEC_SEEDS,
                CostModel::default(),
                || Box::new(BuyAndHold) as Box<dyn Agent>,
            ),
        ];
        subs.extend(luck_floor(
            &data,
            &windows,
            &EXEC_SEEDS,
            CostModel::default(),
            LUCK_FLOOR_AGENTS,
        ));

        let mut cfg = ScoreConfig::for_periods_per_year(*ppy);
        cfg.execution_seeds_per_window = EXEC_SEEDS.len();
        let mut cfg_nc = ScoreConfig::reliability_never_catastrophic(NEVER_CATASTROPHIC_RUN_DD);
        cfg_nc.periods_per_year = *ppy;
        cfg_nc.execution_seeds_per_window = EXEC_SEEDS.len();
        let scored = rank(&subs, &cfg);
        let scored_nc = rank(&subs, &cfg_nc);

        println!("\n== {name} ({tf}, ppy {ppy}) ==");
        println!(
            "{:<16} {:>8} {:>8} {:>7} {:>8} {:>8} {:>9} {:>9} {:>7}",
            "agent", "dsr", "psr", "pass^k", "process", "boot_p", "worst_dd", "eligible", "nc"
        );
        for s in &scored {
            let nc = scored_nc
                .iter()
                .find(|x| x.agent_id == s.agent_id)
                .expect("same field under both gates");
            println!(
                "{:<16} {:>8.4} {:>8.4} {:>7} {:>8} {:>8.4} {:>9.4} {:>9} {:>7}",
                s.agent_id,
                s.deflated_sharpe,
                s.psr,
                s.passed_k,
                s.process_ok,
                s.bootstrap_p,
                s.worst_run_drawdown,
                s.rank_eligible,
                nc.rank_eligible,
            );
            let rec = GateRecord {
                kind: "gate",
                dataset: name,
                timeframe: tf,
                periods_per_year: *ppy,
                n_bars: n,
                n_windows: windows.len(),
                agent_id: s.agent_id.clone(),
                effective_n_trials: s.effective_n_trials,
                trials_sr_std_used: s.trials_sr_std,
                trials_sr_std_source: format!("{:?}", s.trials_sr_std_source),
                deflated_sharpe: s.deflated_sharpe,
                psr: s.psr,
                passed_k: s.passed_k,
                process_ok: s.process_ok,
                bootstrap_p: s.bootstrap_p,
                raw_mean_return: s.raw_mean_return,
                worst_run_drawdown: s.worst_run_drawdown,
                rank_eligible: s.rank_eligible,
                eligible_never_catastrophic: nc.rank_eligible,
            };
            serde_json::to_writer(&mut w, &rec).expect("write record");
            w.write_all(b"\n").expect("newline");
        }

        // Deflation N-sensitivity on the Finding-4 anchor: the same field scored
        // with the declared trial count varied, under both reliability verdicts.
        if *name == N_SENSITIVITY_DATASET {
            println!("\n== {name}: deflation N-sensitivity ==");
            for &n_trials in N_SENSITIVITY {
                let mut cfg_n = cfg.clone();
                cfg_n.n_trials = n_trials;
                let mut cfg_n_nc = cfg_nc.clone();
                cfg_n_nc.n_trials = n_trials;
                let scored_n = rank(&subs, &cfg_n);
                let scored_n_nc = rank(&subs, &cfg_n_nc);
                for s in &scored_n {
                    let nc = scored_n_nc
                        .iter()
                        .find(|x| x.agent_id == s.agent_id)
                        .expect("same field under both gates");
                    if s.agent_id == "risk-managed" {
                        println!(
                            "N={n_trials:<4} dsr {:>7.4} eligible {:>5} never-catastrophic {:>5}",
                            s.deflated_sharpe, s.rank_eligible, nc.rank_eligible
                        );
                    }
                    let rec = NSensitivityRecord {
                        kind: "n_sensitivity",
                        dataset: name,
                        n_trials,
                        effective_n_trials: s.effective_n_trials,
                        agent_id: s.agent_id.clone(),
                        deflated_sharpe: s.deflated_sharpe,
                        rank_eligible: s.rank_eligible,
                        eligible_never_catastrophic: nc.rank_eligible,
                        trials_sr_std_used: s.trials_sr_std,
                    };
                    serde_json::to_writer(&mut w, &rec).expect("write record");
                    w.write_all(b"\n").expect("newline");
                    n_sensitivity_rows += 1;
                }
            }
        }
        if !scored.is_empty() {
            evaluated.push((*name).to_string());
        }
    }
    or_refuse(require_evaluated("dataset", &planned, &evaluated));
    if n_sensitivity_rows == 0 {
        refuse(format!(
            "no N-sensitivity records for declared anchor {N_SENSITIVITY_DATASET}"
        ));
    }

    // Perturbation robustness report on one frozen dataset.
    let path = format!("data/{PERTURB_DATASET}.csv");
    let mut perturbation_rows = 0usize;
    match Dataset::from_csv_file(&path) {
        Ok(data) => {
            let windows = windows_for(data.len());
            let agents = [
                TeamMember::new("risk-managed", || {
                    Box::new(RiskManaged::new()) as Box<dyn Agent>
                }),
                TeamMember::new("buy-and-hold", || Box::new(BuyAndHold) as Box<dyn Agent>),
            ];
            let report = perturbed_field(
                &data,
                &windows,
                &EXEC_SEEDS,
                CostModel::default(),
                &agents,
                N_PERTURBATIONS,
                PERTURB_SEED,
                &ScoreConfig {
                    execution_seeds_per_window: EXEC_SEEDS.len(),
                    ..ScoreConfig::for_periods_per_year(PERTURB_PPY)
                },
            );
            println!(
                "\n== perturbation report: {PERTURB_DATASET} ({N_PERTURBATIONS} variants, seed {PERTURB_SEED}) =="
            );
            println!(
                "{:<16} {:>10} {:>10} {:>10} {:>10}",
                "agent", "orig_psr", "best", "worst", "spread"
            );
            for s in &report.spreads {
                println!(
                    "{:<16} {:>10.4} {:>10.4} {:>10.4} {:>10.4}",
                    s.agent_id,
                    s.original_psr,
                    s.best_perturbed_psr,
                    s.worst_perturbed_psr,
                    s.spread
                );
                let rec = PerturbRecord {
                    kind: "perturbation",
                    dataset: PERTURB_DATASET,
                    n_perturbations: report.n_perturbations,
                    seed: report.seed,
                    agent_id: s.agent_id.clone(),
                    original_psr: s.original_psr,
                    best_perturbed_psr: s.best_perturbed_psr,
                    worst_perturbed_psr: s.worst_perturbed_psr,
                    spread: s.spread,
                };
                serde_json::to_writer(&mut w, &rec).expect("write record");
                w.write_all(b"\n").expect("newline");
                perturbation_rows += 1;
            }
        }
        // The self-audit's "no perturbed windows" limit is closed by this
        // section existing, so a skipped report must not be published as one.
        Err(e) => refuse(format!(
            "declared perturbation dataset {PERTURB_DATASET} did not load: {e}"
        )),
    }
    if perturbation_rows == 0 {
        refuse(format!(
            "the perturbation report on {PERTURB_DATASET} produced no rows"
        ));
    }

    w.flush().expect("flush");
    drop(w);
    std::fs::rename(&partial, &out).expect("publish complete evaluation atomically");
    eprintln!("\nwrote {out}");
}
