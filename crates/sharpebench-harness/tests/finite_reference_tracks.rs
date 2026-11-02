//! Every reference agent keeps a finite track on every frozen dataset.
//!
//! The drift audit of 2026-11-02 found that the commodities buy-and-hold track
//! and all five luck floors had a non-finite mean return on the current engine:
//! 4378ad4 sized target inventory at the observed mark, d882803 readmitted the
//! WTI settlement of -36.98 on 2020-04-20, and a long book marked through it was
//! then sized against a negative NAV until its return overflowed. No test ran a
//! reference agent over a frozen dataset, so the change shipped unnoticed.
//!
//! The engine's rule for that event is ruin (see `RUIN_NAV` in
//! `sharpebench-sim`'s engine): the bar books the realized loss, which can be
//! below -100 percent, the book is closed at the mark, and every later bar
//! books exactly zero. This test asserts that rule, and a finite scored mean,
//! drawdown and PSR, for every reference agent the producers rank, on every
//! `data/*.csv`, under the producers' own window rule and cost model.

use std::path::{Path, PathBuf};

use sharpebench_core::composite::{score_agent, ScoreConfig};
use sharpebench_core::{AgentSubmission, Run};
use sharpebench_harness::{luck_floor, run_agent};
use sharpebench_sim::agent::RiskManaged;
use sharpebench_sim::{
    run_backtest, walk_forward, BuyAndHold, CostModel, Dataset, HoldAgent, Momentum, Window,
};

/// The producers pin eight execution seeds. One keeps a debug run short; the
/// commodities case below runs all eight on the window that matters.
const SEEDS: [u64; 1] = [1];
const LUCK_FLOOR_AGENTS: usize = 5;
/// The NAV at or below which the engine treats a book as ruined.
const RUIN_NAV: f64 = 1e-12;

fn data_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data")
}

/// The window rule `evidence_sweep` and the other producers use.
fn windows_for(n: usize) -> Vec<Window> {
    let warmup = (n / 10).clamp(20, 60);
    let test = ((n - warmup) / 6).max(20);
    walk_forward(n, warmup, test, test)
}

fn frozen_datasets() -> Vec<(String, Dataset)> {
    let mut paths: Vec<PathBuf> = std::fs::read_dir(data_dir())
        .expect("data directory")
        .map(|entry| entry.expect("data entry").path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "csv"))
        .collect();
    paths.sort();
    assert!(
        paths.len() >= 9,
        "expected the nine frozen datasets: {paths:?}"
    );
    paths
        .into_iter()
        .map(|path| {
            let name = path.file_stem().unwrap().to_string_lossy().into_owned();
            let data = Dataset::from_csv_file(path.to_str().unwrap())
                .unwrap_or_else(|e| panic!("{name} does not load: {e}"));
            (name, data)
        })
        .collect()
}

fn reference_field(data: &Dataset, windows: &[Window]) -> Vec<AgentSubmission> {
    let costs = CostModel::default();
    let mut field = vec![
        run_agent("buy-and-hold", data, windows, &SEEDS, costs, || {
            Box::new(BuyAndHold)
        }),
        run_agent("momentum", data, windows, &SEEDS, costs, || {
            Box::new(Momentum::default())
        }),
        run_agent("hold", data, windows, &SEEDS, costs, || Box::new(HoldAgent)),
        run_agent("risk-managed", data, windows, &SEEDS, costs, || {
            Box::new(RiskManaged::new())
        }),
    ];
    field.extend(luck_floor(data, windows, &SEEDS, costs, LUCK_FLOOR_AGENTS));
    field
}

/// Every return is finite, and once the compounded equity is at or below the
/// ruin level every later return is exactly zero. Returns the bar of ruin.
fn check_track(run: &Run, label: &str) -> Option<usize> {
    let mut wealth = 1.0_f64;
    let mut ruined_at = None;
    // inv: `wealth` is the product of (1 + r) over the returns read so far,
    // and `ruined_at` is the first bar after which it was not above RUIN_NAV.
    for (bar, &r) in run.returns.iter().enumerate() {
        assert!(r.is_finite(), "{label}: return {r} at bar {bar}");
        if let Some(at) = ruined_at {
            assert_eq!(r, 0.0, "{label}: ruined at bar {at}, traded at {bar}");
        }
        wealth *= 1.0 + r;
        if ruined_at.is_none() && wealth <= RUIN_NAV {
            ruined_at = Some(bar);
        }
    }
    ruined_at
}

#[test]
fn every_reference_agent_has_a_finite_track_on_every_frozen_dataset() {
    for (name, data) in frozen_datasets() {
        let windows = windows_for(data.len());
        for sub in reference_field(&data, &windows) {
            for (index, run) in sub.runs.iter().enumerate() {
                check_track(run, &format!("{name}/{}/run {index}", sub.agent_id));
            }
            // The bootstrap does not enter the mean, drawdown or PSR.
            let cfg = ScoreConfig {
                n_boot: 20,
                ..ScoreConfig::default()
            };
            let score = score_agent(&sub, &cfg);
            let label = format!("{name}/{}", sub.agent_id);
            assert!(score.raw_mean_return.is_finite(), "{label}: mean return");
            assert!(score.worst_run_drawdown.is_finite(), "{label}: drawdown");
            assert!(score.psr.is_finite(), "{label}: psr");
        }
    }
}

/// The case the audit found: buy-and-hold on the frozen commodities file is
/// ruined by the WTI close of 2020-04-20 and only there, books the mark loss
/// on that bar and nothing afterwards.
#[test]
fn commodities_buy_and_hold_is_ruined_once_by_the_negative_wti_settlement() {
    let text = std::fs::read_to_string(data_dir().join("commodities-1d.csv")).unwrap();
    let data = Dataset::from_csv(&text).unwrap();
    let negative = data
        .dates
        .iter()
        .position(|d| d == "2020-04-20")
        .expect("the WTI settlement date is on the axis");
    assert!(data.close_at("WTI", negative).unwrap() < 0.0);
    let mut ruined = 0;
    for window in windows_for(data.len()) {
        let covers = (window.start..window.end).contains(&negative);
        for seed in 1..=8 {
            let run = run_backtest(&data, &mut BuyAndHold, window, seed, CostModel::default());
            let label = format!("window {}..{} seed {seed}", window.start, window.end);
            match check_track(&run, &label) {
                Some(bar) => {
                    assert!(covers, "{label}: ruined outside the WTI window");
                    assert_eq!(window.start + bar, negative, "{label}");
                    let loss = run.returns[bar];
                    assert!((-1.6..-1.5).contains(&loss), "{label}: {loss}");
                    ruined += 1;
                }
                None => assert!(!covers, "{label}: survived the WTI settlement"),
            }
        }
    }
    assert_eq!(ruined, 8, "one covering window, eight seeds");
}
