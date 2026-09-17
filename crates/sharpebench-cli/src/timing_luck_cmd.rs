//! `sharpebench timing-luck`: the timing-luck floor of the `run` protocol.
//!
//! Resolves the dataset, windows and cost model exactly as `sharpebench run`
//! does (the default costs, or `--short-borrow-bps` through the same parser),
//! with the same eight execution seeds and reference roster. It then reports
//! how far the reference rows' Sharpe and deflated Sharpe move when they
//! rebalance every `--cadence` bars and only the phase of that schedule moves
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

const USAGE: &str = "usage: sharpebench timing-luck --cadence <m> [--data <csv>] \
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

/// Phases that produced the figure, over phases evaluated.
fn measured(spread: &Spread) -> String {
    format!("{}/{}", spread.phases_measured, spread.by_phase.len())
}

fn print_report(source: &str, symbols: usize, periods_per_year: f64, report: &TimingLuckReport) {
    println!(
        "SharpeBench timing-luck floor on {source} ({symbols} symbols, {} windows x {} seeds, \
         cadence {} so {} phases, {periods_per_year} periods/year, costs on)",
        report.windows.len(),
        report.seeds,
        report.cadence,
        report.cadence,
    );
    println!(
        "Reference field and hold control only. Not used by the gate, eligibility or the rank."
    );
    println!(
        "Every phase decides on a window's first bar, then on bars start+p, start+p+{}, ...; \
         every phase evaluates every bar.",
        report.cadence
    );
    for window in &report.windows {
        let counts: Vec<String> = window
            .decisions_by_phase
            .iter()
            .map(ToString::to_string)
            .collect();
        println!(
            "window {} ({} bars): decisions per run by phase {}",
            window.window,
            window.bars,
            counts.join(",")
        );
    }
    println!(
        "The deflated Sharpe of every phase uses the row's phase-0 bar; dsr bar and field sd \
         (min..max over phases) show what each phase's own field measured."
    );
    println!("n = phases that produced the figure / phases; win = windows behind it");
    println!(
        "\n{:<18} {:<12} {:>3} {:>5} {:>10} {:>17} {:>5} {:>10} {:>15} {:>9} {:>17}",
        "agent",
        "scope",
        "win",
        "n",
        "sharpe rng",
        "sharpe min..max",
        "n",
        "dsr rng",
        "dsr min..max",
        "dsr bar",
        "field sd"
    );
    for row in &report.rows {
        for scope in std::iter::once(&row.all_windows).chain(&row.per_window) {
            let field_sd: Vec<f64> = scope
                .field_dispersion_by_phase
                .iter()
                .map(|f| f.trials_sr_std)
                .collect();
            let field_span = format!(
                "{}..{}",
                figure(field_sd.iter().copied().reduce(f64::min)),
                figure(field_sd.iter().copied().reduce(f64::max))
            );
            println!(
                "{:<18} {:<12} {:>3} {:>5} {:>10} {:>17} {:>5} {:>10} {:>15} {:>9} {:>17}",
                crate::truncate(&row.agent_id, 18),
                crate::truncate(&scope.scope, 12),
                scope.windows,
                measured(&scope.sharpe),
                figure(scope.sharpe.range),
                span(&scope.sharpe),
                measured(&scope.deflated_sharpe),
                figure(scope.deflated_sharpe.range),
                span(&scope.deflated_sharpe),
                figure(scope.deflation.deflation_bar_per_period),
                field_span,
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
    let Some(raw) = crate::flag_value(args, "--cadence") else {
        eprintln!("{USAGE}");
        return 2;
    };
    let cadence = match raw.parse::<usize>() {
        Ok(cadence) if cadence >= 1 => cadence,
        _ => {
            eprintln!(
                "error: --cadence must be a whole number of bars, at least 1, got `{raw}`\n{USAGE}"
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
        cadence,
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
