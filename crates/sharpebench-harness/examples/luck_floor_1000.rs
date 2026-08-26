//! Thousand-agent luck floor (paper revision R-45).
//!
//! The paper's opening image is a thousand monkeys; the shipped evidence floor
//! is five seeded random agents per dataset, which draws the floor's location
//! but not its extreme tail. This example runs the floor at the scale the
//! introduction invokes: N = 1000 distinctly-seeded [`RandomAgent`]s
//! (`sharpebench_harness::luck_floor`, whose N has always been a parameter)
//! across the same walk-forward windows x seeds the evidence sweep uses, on
//! us-indices-1d and crypto-majors-1d. The first five return streams are
//! byte-identical to the shipped five-agent floor; their scores differ because
//! the observable trial count and measured dispersion belong to this larger
//! field.
//!
//! Each agent is scored three ways:
//! - `dsr_configured`: the configured annualized 0.5 prior with the observed
//!   1,000-entry field count used as the trial footprint.
//! - `dsr_field`: the same score with the deflation dispersion *measured from
//!   the 1000-monkey field itself* (the sample standard deviation of pooled
//!   per-period Sharpes, which is what `rank`'s measured path computes). The
//!   measured per-period value is injected through the config by multiplying it
//!   back to annualized units, so `per_period_sr_std` recovers it exactly. The
//!   1,000-entry trial footprint still applies; only the dispersion floor is off.
//! - `dsr_shipped_floor`: the field-measured value after applying the shipped
//!   annualized lower bound. This is the value the current ranking path uses;
//!   the raw measured value is retained as a diagnostic of the empirical tail.
//!
//! Scoring is parallelized across all available cores with std::thread (rayon
//! is not a workspace dependency); results are deterministic regardless of
//! thread count because every agent is scored independently from pinned seeds.
//!
//! Run with:
//!
//!   cargo run --release -p sharpebench-harness --example luck_floor_1000 -- \
//!     paper/evidence/final/luck-floor-1000.jsonl
//!
//! Output: one `record: "agent"` JSON line per (dataset, agent) plus one
//! `record: "summary"` line per dataset with the max/quantile DSRs, the
//! first-five-stream max in the same 1,000-entry context, and the raw-return
//! comparison against the reference agents.

use std::env;
use std::fs::File;
use std::io::{BufWriter, Write};

use serde::Serialize;
use sharpebench_core::composite::{pooled_returns, score_agent, ScoreConfig};
use sharpebench_core::{AgentSubmission, CompositeScore};
use sharpebench_harness::luck_floor;
use sharpebench_sim::{
    run_backtest, walk_forward, Agent, BuyAndHold, CostModel, Dataset, Momentum, Window,
};

const DATASETS: &[(&str, f64)] = &[("us-indices-1d", 252.0), ("crypto-majors-1d", 365.0)];
const EXEC_SEEDS: [u64; 8] = [1, 2, 3, 4, 5, 6, 7, 8];
const N_AGENTS: usize = 1000;
/// The shipped evidence floor size, for the direct old-vs-new comparison.
const SHIPPED_FLOOR: usize = 5;

/// Walk-forward windows sized to the dataset, same shape as evidence_sweep.
fn windows_for(n: usize) -> (Vec<Window>, usize) {
    let warmup = (n / 10).clamp(20, 60);
    let test = ((n - warmup) / 6).max(20);
    (walk_forward(n, warmup, test, test), test)
}

fn mean(xs: &[f64]) -> f64 {
    if xs.is_empty() {
        return 0.0;
    }
    xs.iter().sum::<f64>() / xs.len() as f64
}

/// Sample standard deviation (ddof = 1), matching the kernel's `std_dev`.
fn std_dev(xs: &[f64]) -> f64 {
    if xs.len() < 2 {
        return 0.0;
    }
    let m = mean(xs);
    (xs.iter().map(|x| (x - m).powi(2)).sum::<f64>() / (xs.len() - 1) as f64).sqrt()
}

fn pooled_sharpe(sub: &AgentSubmission) -> f64 {
    let pooled = pooled_returns(sub, EXEC_SEEDS.len());
    let sd = std_dev(&pooled);
    if sd == 0.0 {
        f64::NAN
    } else {
        mean(&pooled) / sd
    }
}

/// Quantile by nearest-rank on a sorted slice.
fn quantile(sorted: &[f64], q: f64) -> f64 {
    if sorted.is_empty() {
        return f64::NAN;
    }
    let idx = ((q * (sorted.len() - 1) as f64).round() as usize).min(sorted.len() - 1);
    sorted[idx]
}

/// Score every submission in parallel across all available cores.
fn score_all(subs: &[AgentSubmission], cfg: &ScoreConfig) -> Vec<CompositeScore> {
    let threads = std::thread::available_parallelism()
        .map(std::num::NonZeroUsize::get)
        .unwrap_or(1);
    let chunk = subs.len().div_ceil(threads).max(1);
    let mut out: Vec<Option<CompositeScore>> = vec![None; subs.len()];
    std::thread::scope(|s| {
        for (subs_chunk, out_chunk) in subs.chunks(chunk).zip(out.chunks_mut(chunk)) {
            s.spawn(move || {
                for (sub, slot) in subs_chunk.iter().zip(out_chunk.iter_mut()) {
                    *slot = Some(score_agent(sub, cfg));
                }
            });
        }
    });
    out.into_iter().map(|o| o.expect("scored")).collect()
}

fn reference_submission(data: &Dataset, windows: &[Window], id: &str) -> AgentSubmission {
    let mut runs = Vec::new();
    for w in windows {
        for seed in EXEC_SEEDS {
            let mut agent: Box<dyn Agent> = match id {
                "buy-and-hold" => Box::new(BuyAndHold),
                _ => Box::new(Momentum::default()),
            };
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
}

#[derive(Serialize)]
struct AgentRecord<'a> {
    record: &'static str,
    dataset: &'a str,
    agent_id: &'a str,
    sharpe_per_period: f64,
    dsr_configured: f64,
    dsr_field: f64,
    dsr_shipped_floor: f64,
    psr: f64,
    raw_mean_return: f64,
    passed_k: bool,
    rank_eligible_configured: bool,
    rank_eligible_field: bool,
    rank_eligible_shipped_floor: bool,
    max_drawdown: f64,
}

#[derive(Serialize)]
struct DsrSummary {
    max: f64,
    p999: f64,
    p99: f64,
    p90: f64,
    p50: f64,
    mean: f64,
    n_rank_eligible: usize,
    /// Max among the first five return streams, scored in this 1,000-entry
    /// context rather than as a standalone five-entry field.
    max_first_5: f64,
}

#[derive(Serialize)]
struct Summary<'a> {
    record: &'static str,
    dataset: &'a str,
    periods_per_year: f64,
    n_agents: usize,
    n_windows: usize,
    window_len: usize,
    n_seeds: usize,
    host_n_trials: u32,
    operational_n_trials: u32,
    dsr_bar: f64,
    /// Field-measured per-period Sharpe dispersion across the 1000 monkeys.
    measured_sr_std_per_period: f64,
    measured_sr_std_annualized: f64,
    configured: DsrSummary,
    /// Raw measured path with the safety floor deliberately disabled.
    field_measured: DsrSummary,
    /// Field-measured path after the shipped annualized floor is applied.
    shipped_floor: DsrSummary,
    argmax_agent_configured: String,
    argmax_agent_field: String,
    floor_max_raw_return: f64,
    reference_raw_returns: std::collections::BTreeMap<String, f64>,
    /// True when some monkey beat every reference agent on raw return, the
    /// paper's standing claim is that this never happens.
    floor_beats_reference_on_raw: bool,
}

fn summarize(scores: &[CompositeScore], key: impl Fn(&CompositeScore) -> f64) -> DsrSummary {
    let mut xs: Vec<f64> = scores.iter().map(&key).collect();
    xs.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    DsrSummary {
        max: *xs.last().unwrap_or(&f64::NAN),
        p999: quantile(&xs, 0.999),
        p99: quantile(&xs, 0.99),
        p90: quantile(&xs, 0.90),
        p50: quantile(&xs, 0.50),
        mean: mean(&xs),
        n_rank_eligible: 0,
        max_first_5: scores
            .iter()
            .filter(|s| {
                s.agent_id
                    .strip_prefix("luck-floor-")
                    .and_then(|suffix| suffix.parse::<usize>().ok())
                    .is_some_and(|index| index < SHIPPED_FLOOR)
            })
            .map(&key)
            .fold(f64::MIN, f64::max),
    }
}

fn main() {
    // Required positional; see evidence_sweep for why there is no default. This
    // one defaulted straight into paper/evidence/final/, so a stray run
    // overwrote a committed artifact in place.
    let out = env::args().nth(1).unwrap_or_else(|| {
        eprintln!("usage: luck_floor_1000 <out.jsonl>");
        std::process::exit(2);
    });
    let mut w = BufWriter::new(File::create(&out).expect("create output"));

    for (name, ppy) in DATASETS {
        let path = format!("data/{name}.csv");
        let data = Dataset::from_csv_file(&path).expect("frozen dataset");
        let (windows, window_len) = windows_for(data.len());
        eprintln!(
            "{name}: {} bars, {} windows of {window_len}, {N_AGENTS} random agents...",
            data.len(),
            windows.len()
        );

        let monkeys = luck_floor(&data, &windows, &EXEC_SEEDS, CostModel::default(), N_AGENTS);

        let cfg = ScoreConfig {
            execution_seeds_per_window: EXEC_SEEDS.len(),
            ..ScoreConfig::for_periods_per_year(*ppy)
        };
        let monkey_sharpes: Vec<f64> = monkeys
            .iter()
            .map(pooled_sharpe)
            .filter(|sr| sr.is_finite())
            .collect();
        let measured_pp = std_dev(&monkey_sharpes);
        let measured_annualized = measured_pp * ppy.sqrt();

        // `score_agent` has no field context, so spell out the three inputs the
        // ranking path would resolve before scoring. This keeps the expensive
        // 1,000-agent bootstrap parallel while preserving the exact DSR inputs:
        // an observable trial footprint of 1,000, the raw field dispersion, and
        // that dispersion floored by the shipped annualized lower bound. Clone
        // collapse is immaterial here because the 1,000 independently seeded
        // random streams are not near-duplicate submissions.
        let mut cfg_configured = cfg.clone();
        cfg_configured.n_trials = N_AGENTS as u32;
        let mut cfg_field = cfg_configured.clone();
        cfg_field.trials_sr_std = measured_annualized;
        let mut cfg_shipped_floor = cfg_configured.clone();
        cfg_shipped_floor.trials_sr_std = measured_annualized.max(cfg.min_measured_trials_sr_std);

        eprintln!("{name}: scoring (configured prior)...");
        let scored_cfg = score_all(&monkeys, &cfg_configured);
        eprintln!("{name}: scoring (field-measured dispersion)...");
        let scored_field = score_all(&monkeys, &cfg_field);
        eprintln!("{name}: scoring (field-measured with shipped floor)...");
        let scored_shipped_floor = score_all(&monkeys, &cfg_shipped_floor);

        let references: Vec<AgentSubmission> = ["buy-and-hold", "momentum"]
            .iter()
            .map(|id| reference_submission(&data, &windows, id))
            .collect();
        let ref_scores = score_all(&references, &cfg);

        for (((m, sc), sf), shipped) in monkeys
            .iter()
            .zip(&scored_cfg)
            .zip(&scored_field)
            .zip(&scored_shipped_floor)
        {
            let rec = AgentRecord {
                record: "agent",
                dataset: name,
                agent_id: &m.agent_id,
                sharpe_per_period: pooled_sharpe(m),
                dsr_configured: sc.deflated_sharpe,
                dsr_field: sf.deflated_sharpe,
                dsr_shipped_floor: shipped.deflated_sharpe,
                psr: sc.psr,
                raw_mean_return: sc.raw_mean_return,
                passed_k: sc.passed_k,
                rank_eligible_configured: sc.rank_eligible,
                rank_eligible_field: sf.rank_eligible,
                rank_eligible_shipped_floor: shipped.rank_eligible,
                max_drawdown: sc.max_drawdown,
            };
            serde_json::to_writer(&mut w, &rec).expect("write record");
            w.write_all(b"\n").expect("newline");
        }

        let mut configured = summarize(&scored_cfg, |s| s.deflated_sharpe);
        configured.n_rank_eligible = scored_cfg.iter().filter(|s| s.rank_eligible).count();
        let mut field_measured = summarize(&scored_field, |s| s.deflated_sharpe);
        field_measured.n_rank_eligible = scored_field.iter().filter(|s| s.rank_eligible).count();
        let mut shipped_floor = summarize(&scored_shipped_floor, |s| s.deflated_sharpe);
        shipped_floor.n_rank_eligible = scored_shipped_floor
            .iter()
            .filter(|s| s.rank_eligible)
            .count();

        let argmax = |scores: &[CompositeScore]| -> String {
            scores
                .iter()
                .max_by(|a, b| {
                    a.deflated_sharpe
                        .partial_cmp(&b.deflated_sharpe)
                        .unwrap_or(std::cmp::Ordering::Equal)
                })
                .map(|s| s.agent_id.clone())
                .unwrap_or_default()
        };

        let floor_max_raw = scored_cfg
            .iter()
            .map(|s| s.raw_mean_return)
            .fold(f64::MIN, f64::max);
        let reference_raw_returns: std::collections::BTreeMap<String, f64> = ref_scores
            .iter()
            .map(|s| (s.agent_id.clone(), s.raw_mean_return))
            .collect();
        let best_ref_raw = ref_scores
            .iter()
            .map(|s| s.raw_mean_return)
            .fold(f64::MIN, f64::max);

        let summary = Summary {
            record: "summary",
            dataset: name,
            periods_per_year: *ppy,
            n_agents: N_AGENTS,
            n_windows: windows.len(),
            window_len,
            n_seeds: EXEC_SEEDS.len(),
            host_n_trials: cfg.n_trials,
            operational_n_trials: cfg_shipped_floor.n_trials,
            dsr_bar: cfg.dsr_bar,
            measured_sr_std_per_period: measured_pp,
            measured_sr_std_annualized: measured_pp * ppy.sqrt(),
            configured,
            field_measured,
            shipped_floor,
            argmax_agent_configured: argmax(&scored_cfg),
            argmax_agent_field: argmax(&scored_field),
            floor_max_raw_return: floor_max_raw,
            reference_raw_returns,
            floor_beats_reference_on_raw: floor_max_raw > best_ref_raw,
        };
        serde_json::to_writer(&mut w, &summary).expect("write summary");
        w.write_all(b"\n").expect("newline");
        w.flush().expect("flush");
        eprintln!(
            "{name}: max DSR configured={:.4}, raw-measured={:.4}, shipped-floor={:.4}, first-5={:.4}",
            summary.configured.max,
            summary.field_measured.max,
            summary.shipped_floor.max,
            summary.configured.max_first_5
        );
    }
    eprintln!("wrote {out}");
}
