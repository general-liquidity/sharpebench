//! `sharpebench timing-luck`: the timing-luck floor of the `run` protocol.
//!
//! Resolves the dataset, windows and cost model exactly as `sharpebench run`
//! does (the default costs, or `--short-borrow-bps` through the same parser),
//! with the same eight execution seeds and reference roster, and
//! reports how far the reference rows' Sharpe and deflated Sharpe move when
//! every window's start shifts by 0 to k-1 bars
//! ([`sharpebench_harness::timing_luck`]). No external entrant is accepted, no
//! board is printed, and nothing `run` reads is written, so `run` output is the
//! same whether or not this command has run. The report is reporting surface
//! beside a board, never a rank input.

use serde::Serialize;
use sharpebench_core::ScoreConfig;
use sharpebench_harness::timing_luck::{
    timing_luck, Spread, TimingLuckReport, TimingLuckSpec, TimingLuckUnavailable,
    TIMING_LUCK_SCHEMA_VERSION,
};

const USAGE: &str = "usage: sharpebench timing-luck --offsets <k> [--data <csv>] \
                     [--periods-per-year N] [--short-borrow-bps <bps>] [--json]";

/// The emitted document. An unavailable report still says what it is and that
/// it is not a rank input, so a reader holding the JSON alone cannot mistake it
/// for a measured zero.
#[derive(Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
enum Document<'a> {
    Measured {
        report: &'a TimingLuckReport,
    },
    Unavailable {
        schema_version: &'static str,
        rank_input: bool,
        unavailable: &'a TimingLuckUnavailable,
    },
}

fn figure(value: Option<f64>) -> String {
    value.map_or_else(|| "n/a".to_string(), |v| format!("{v:.4}"))
}

fn span(spread: &Spread) -> String {
    format!("{}..{}", figure(spread.min), figure(spread.max))
}

/// Offsets that produced the figure, over offsets declared.
fn measured(spread: &Spread) -> String {
    format!("{}/{}", spread.offsets_measured, spread.by_offset.len())
}

fn print_report(source: &str, symbols: usize, periods_per_year: f64, report: &TimingLuckReport) {
    let geometry = &report.geometry;
    println!(
        "SharpeBench timing-luck floor on {source} ({symbols} symbols, {} windows x {} seeds, \
         {} start offsets, {periods_per_year} periods/year, costs on)",
        geometry.windows.len(),
        report.seeds,
        geometry.offsets,
    );
    println!(
        "Reference field and hold control only. Not used by the gate, eligibility or the rank."
    );
    for window in &geometry.windows {
        println!(
            "window {}: {}-bar shifted windows from {} to {}; the first and last share {} bars",
            window.declared,
            window.instance_len,
            window.first_instance,
            window.last_instance,
            window.bars_shared_by_first_and_last_instance,
        );
    }
    println!(
        "declared windows disjoint: {}; shifted windows of distinct declared windows disjoint: {}",
        geometry.declared_windows_disjoint, geometry.instances_of_distinct_windows_disjoint,
    );
    if geometry.instances_of_one_window_overlap {
        println!("offsets of one window overlap each other, so they are not independent draws");
    }
    println!("n = offsets that produced the figure / offsets declared; win = windows behind it");
    println!(
        "\n{:<18} {:<12} {:>3} {:>5} {:>10} {:>17} {:>5} {:>10} {:>15}",
        "agent",
        "scope",
        "win",
        "n",
        "sharpe rng",
        "sharpe min..max",
        "n",
        "dsr rng",
        "dsr min..max"
    );
    for row in &report.rows {
        for scope in std::iter::once(&row.all_windows).chain(&row.per_window) {
            println!(
                "{:<18} {:<12} {:>3} {:>5} {:>10} {:>17} {:>5} {:>10} {:>15}",
                crate::truncate(&row.agent_id, 18),
                crate::truncate(&scope.scope, 12),
                scope.windows,
                measured(&scope.sharpe),
                figure(scope.sharpe.range),
                span(&scope.sharpe),
                measured(&scope.deflated_sharpe),
                figure(scope.deflated_sharpe.range),
                span(&scope.deflated_sharpe),
            );
        }
    }
}

/// The operator entry point. `0` is a measured report, `1` an unavailable one
/// or an unreadable dataset, `2` a usage error.
pub fn run(args: &[String], json: bool) -> i32 {
    if ["--cmd", "--image", "--http"]
        .iter()
        .any(|flag| args.iter().any(|arg| arg == flag))
    {
        eprintln!(
            "error: timing-luck measures the reference field only and runs no external entrant\n{USAGE}"
        );
        return 2;
    }
    let Some(raw) = crate::flag_value(args, "--offsets") else {
        eprintln!("{USAGE}");
        return 2;
    };
    let offsets = match raw.parse::<usize>() {
        Ok(offsets) if offsets >= 1 => offsets,
        _ => {
            eprintln!(
                "error: --offsets must be a whole number of at least 1, got `{raw}`\n{USAGE}"
            );
            return 2;
        }
    };
    let periods_per_year = match crate::flag_value(args, "--periods-per-year") {
        Some(raw) => match raw.parse::<f64>() {
            Ok(p) if p.is_finite() && p > 0.0 => p,
            _ => {
                eprintln!("error: --periods-per-year must be a positive number, got `{raw}`");
                return 2;
            }
        },
        None => ScoreConfig::default().periods_per_year,
    };
    let costs = match crate::cost_model_from_args(args) {
        Ok(costs) => costs,
        Err(error) => {
            eprintln!("error: {error}");
            return 2;
        }
    };
    let (data, windows) = match crate::resolve_dataset(args) {
        Ok(resolved) => resolved,
        Err(error) => {
            eprintln!("error: {error}");
            return 1;
        }
    };
    let seeds: Vec<u64> = (0..8).collect();
    let cfg = ScoreConfig::for_periods_per_year(periods_per_year);
    let spec = TimingLuckSpec {
        offsets,
        luck_floor_agents: crate::LUCK_FLOOR_AGENTS,
        hold_control_id: crate::HOLD_CONTROL_ID,
    };
    match timing_luck(&data, &windows, &seeds, costs, &cfg, spec) {
        Ok(report) => {
            if json {
                crate::emit_json(&Document::Measured { report: &report });
            } else {
                print_report(
                    crate::flag_value(args, "--data").unwrap_or("synthetic"),
                    data.symbols().len(),
                    periods_per_year,
                    &report,
                );
            }
            0
        }
        Err(unavailable) => {
            if json {
                crate::emit_json(&Document::Unavailable {
                    schema_version: TIMING_LUCK_SCHEMA_VERSION,
                    rank_input: false,
                    unavailable: &unavailable,
                });
            } else {
                eprintln!("timing-luck floor unavailable: {unavailable}");
            }
            1
        }
    }
}
