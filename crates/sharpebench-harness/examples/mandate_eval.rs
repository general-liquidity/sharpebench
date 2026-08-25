//! Declared-mandate evidence for the SharpeBench paper.
//!
//! The paper's stated design direction is a mandate declared at submission and
//! certified against, under which buy-and-hold's verdict reads "meets its
//! eligibility under the declared verdict beside host-board eligibility instead of a blanket
//! refusal. This example runs the reference field (buy-and-hold, momentum,
//! hold, the risk-managed agent, and the luck floor) on all nine frozen
//! datasets with each named agent declaring the mandate that matches its
//! nature: the excess-return comparison declares `OutperformBuyAndHold`, hold
//! and momentum declare `AbsoluteReturn` (the default verdict,
//! restated), and the risk-managed agent declares `DrawdownCapped { 0.20 }`
//! (never lose more than 20% in any single run). The board is ranked once with
//! `rank_declared`: the host verdict is untouched and decides rank, and each
//! declared verdict is recorded beside it, per agent and dataset.
//!
//! Deterministic: no clock, no ambient RNG. Same window rule, seeds and cost
//! model as `evidence_sweep`, `risk_managed_eval` and `relative_mandate_eval`.
//! Run with
//!
//!   cargo run --release -p sharpebench-harness --example mandate_eval -- <out.jsonl>

use std::env;
use std::fs::File;
use std::io::{BufWriter, Write};

use serde::Serialize;
use sharpebench_core::composite::{
    rank_declared, DeclaredMandate, MandateDeclarations, MandateVerdict, ScoreConfig,
};
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
const RISK_MANAGED_DECLARED_DD: f64 = 0.20;
const COMMAND: &str = "cargo run --release -p sharpebench-harness --example mandate_eval -- paper/evidence/final/mandate-declaration.jsonl";

#[derive(Serialize)]
struct MandateRecord<'a> {
    kind: &'static str,
    command: &'static str,
    dataset: &'a str,
    timeframe: &'a str,
    periods_per_year: f64,
    n_bars: usize,
    n_windows: usize,
    n_seeds: usize,
    regimes: Vec<String>,
    agent_id: String,
    /// Host-verdict statistics: identical to an undeclared board.
    deflated_sharpe: f64,
    psr: f64,
    process_ok: bool,
    bootstrap_p: f64,
    raw_mean_return: f64,
    worst_run_drawdown: f64,
    passed_k: bool,
    rank_eligible: bool,
    /// The declared column, absent for undeclared agents (the luck floor).
    #[serde(skip_serializing_if = "Option::is_none")]
    declared_mandate: Option<DeclaredMandate>,
    #[serde(skip_serializing_if = "Option::is_none")]
    verdict_applied: Option<MandateVerdict>,
    #[serde(skip_serializing_if = "Option::is_none")]
    declared_passed_k: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    declared_mandate_eligible: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    declared_mandate_ordinal: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    mandate_verdict_label: Option<String>,
}

/// Same window rule as `evidence_sweep`: warmup n/10 clamped to 20..60, test
/// windows of (n - warmup)/6 with a 20-bar floor.
fn windows_for(n: usize) -> Vec<Window> {
    let warmup = (n / 10).clamp(20, 60);
    let test = ((n - warmup) / 6).max(20);
    walk_forward(n, warmup, test, test)
}

fn main() {
    let out = env::args()
        .nth(1)
        .unwrap_or_else(|| "mandate_eval.jsonl".to_string());
    let mut w = BufWriter::new(File::create(&out).expect("create output"));
    let mut n_records = 0usize;
    let mut n_meets = 0usize;

    let declarations: MandateDeclarations = [
        ("buy-and-hold", DeclaredMandate::OutperformBuyAndHold),
        ("hold", DeclaredMandate::AbsoluteReturn),
        ("momentum", DeclaredMandate::AbsoluteReturn),
        (
            "risk-managed",
            DeclaredMandate::DrawdownCapped {
                max_per_run_drawdown: RISK_MANAGED_DECLARED_DD,
            },
        ),
    ]
    .into_iter()
    .map(|(id, m)| (id.to_string(), m))
    .collect();

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
                "buy-and-hold",
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

        let cfg = ScoreConfig {
            execution_seeds_per_window: EXEC_SEEDS.len(),
            ..ScoreConfig::for_periods_per_year(*ppy)
        };
        let scored = rank_declared(&subs, &declarations, &cfg);

        println!(
            "\n== {name} ({tf}, ppy {ppy}, {} windows: {}) ==",
            windows.len(),
            regimes.join(" ")
        );
        println!(
            "{:<16} {:>8} {:>8} {:>9} {:>7} {:>6} {:>9}  mandate",
            "agent", "dsr", "boot_p", "worst_dd", "pass^k", "elig", "decl_pass"
        );
        for s in &scored {
            let label = s.mandate_verdict_label();
            println!(
                "{:<16} {:>8.4} {:>8.4} {:>9.4} {:>7} {:>6} {:>9}  {}",
                s.agent_id,
                s.deflated_sharpe,
                s.bootstrap_p,
                s.worst_run_drawdown,
                s.passed_k,
                s.rank_eligible,
                s.declared_passed_k
                    .map_or("-".to_string(), |b| b.to_string()),
                label.as_deref().unwrap_or("undeclared"),
            );
            if s.declared_mandate_eligible == Some(true) {
                n_meets += 1;
            }
            let rec = MandateRecord {
                kind: "mandate_declaration",
                command: COMMAND,
                dataset: name,
                timeframe: tf,
                periods_per_year: *ppy,
                n_bars: n,
                n_windows: windows.len(),
                n_seeds: EXEC_SEEDS.len(),
                regimes: regimes.clone(),
                agent_id: s.agent_id.clone(),
                deflated_sharpe: s.deflated_sharpe,
                psr: s.psr,
                process_ok: s.process_ok,
                bootstrap_p: s.bootstrap_p,
                raw_mean_return: s.raw_mean_return,
                worst_run_drawdown: s.worst_run_drawdown,
                passed_k: s.passed_k,
                rank_eligible: s.rank_eligible,
                declared_mandate: s.declared_mandate.clone(),
                verdict_applied: s.verdict_applied.clone(),
                declared_passed_k: s.declared_passed_k,
                declared_mandate_eligible: s.declared_mandate_eligible,
                declared_mandate_ordinal: s.declared_mandate_ordinal,
                mandate_verdict_label: label,
            };
            serde_json::to_writer(&mut w, &rec).expect("write record");
            w.write_all(b"\n").expect("newline");
            n_records += 1;
        }
    }

    w.flush().expect("flush");
    eprintln!("\nwrote {n_records} records to {out}; {n_meets} row(s) meet their declared mandate");
}
