//! `verify-trajectory --timing-null` and `--lagged-replay`: rank-neutral
//! replay diagnostics reported beside a strictly verified trajectory.
//!
//! Neither flag changes the verification. With both absent the command's
//! output is exactly what it was. With either present the verification is
//! printed as before and the diagnostics follow it: under `--json` as a
//! `replay_diagnostics` member beside the sealed verification fields, in the
//! human output as a block after the verification. The diagnostics replay the
//! trajectory under the same data and cost model the strict path just bound it
//! to, and they need that binding, so they refuse `--allow-unbound-trajectory`
//! and `--reexecute`. They also refuse `--diagnostics`, whose report nests the
//! verification under a different shape; request the two separately. A
//! malformed flag, including a draw count above the cap, is refused before
//! anything is read; a lag longer than the runs allow is refused with the same
//! exit code once the trajectory is read.

use std::process::ExitCode;

use serde::Serialize;
use sharpebench_protocol::AgentTrajectory;
use sharpebench_sim::replay_nulls::{
    lagged_replay, timing_null, LaggedAggregate, LaggedReplayReport, LaggedRun,
    LaggedRunUnavailable, ReplayNullRefusal, RunTimingNull, TimingNullAggregate, TimingNullConfig,
    TimingNullReport, TimingNullUnavailable, TimingPercentile, MAX_TIMING_NULL_DRAWS, VALID_WHEN,
};
use sharpebench_sim::{CostModel, Dataset};

use crate::{emit_json, emit_verification, flag_value};

/// The diagnostics a `verify-trajectory` invocation opted into.
pub(crate) struct Requested {
    timing: Option<TimingNullConfig>,
    lags: Option<Vec<usize>>,
}

fn integer_flag(args: &[String], flag: &str) -> Result<Option<u64>, String> {
    if !args.iter().any(|arg| arg == flag) {
        return Ok(None);
    }
    match flag_value(args, flag).map(str::parse::<u64>) {
        Some(Ok(value)) => Ok(Some(value)),
        _ => Err(format!("{flag} needs a non-negative integer")),
    }
}

/// Parse the opt-in flags. `Ok(None)` when neither diagnostic is requested,
/// which leaves the verification path untouched.
pub(crate) fn requested(args: &[String]) -> Result<Option<Requested>, String> {
    let timing_on = args.iter().any(|arg| arg == "--timing-null");
    let draws = integer_flag(args, "--null-draws")?;
    let seed = integer_flag(args, "--null-seed")?;
    if !timing_on && (draws.is_some() || seed.is_some()) {
        return Err("--null-draws and --null-seed configure --timing-null; add it".to_string());
    }
    let timing = if timing_on {
        let mut config = TimingNullConfig::default();
        if let Some(draws) = draws {
            config.draws = usize::try_from(draws)
                .ok()
                .filter(|&draws| (1..=MAX_TIMING_NULL_DRAWS).contains(&draws))
                .ok_or_else(|| {
                    format!("--null-draws needs an integer from 1 to {MAX_TIMING_NULL_DRAWS}")
                })?;
        }
        if let Some(seed) = seed {
            config.seed = seed;
        }
        Some(config)
    } else {
        None
    };
    let lags = if args.iter().any(|arg| arg == "--lagged-replay") {
        let list = flag_value(args, "--lagged-replay")
            .ok_or("--lagged-replay needs a comma-separated list of bar lags, e.g. 1,2,5")?;
        let lags = list
            .split(',')
            .map(|lag| lag.trim().parse::<usize>())
            .collect::<Result<Vec<_>, _>>()
            .map_err(|_| {
                format!("--lagged-replay `{list}` is not a comma-separated list of bar lags")
            })?;
        Some(lags)
    } else {
        None
    };
    if timing.is_none() && lags.is_none() {
        return Ok(None);
    }
    for flag in ["--allow-unbound-trajectory", "--reexecute", "--diagnostics"] {
        if args.iter().any(|arg| arg == flag) {
            return Err(format!(
                "--timing-null and --lagged-replay replay the strictly bound trajectory; they cannot be combined with {flag}"
            ));
        }
    }
    Ok(Some(Requested { timing, lags }))
}

#[derive(Serialize)]
struct ReplayDiagnostics {
    /// Always true: the gate and the rank never read these figures.
    rank_neutral: bool,
    valid_when: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    timing_null: Option<TimingNullReport>,
    #[serde(skip_serializing_if = "Option::is_none")]
    lagged_replay: Option<LaggedReplayReport>,
}

/// Compute the requested diagnostics and print them beside `result`.
pub(crate) fn report(
    requested: &Requested,
    data: &Dataset,
    traj: &AgentTrajectory,
    costs: CostModel,
    result: &sharpebench_harness::VerificationResult,
    json: bool,
) -> ExitCode {
    let timing_null = match requested.timing {
        Some(config) => match timing_null(data, traj, costs, config) {
            Ok(report) => Some(report),
            Err(error) => {
                eprintln!("error: timing null: {error}");
                return ExitCode::FAILURE;
            }
        },
        None => None,
    };
    let lagged = match &requested.lags {
        Some(lags) => match lagged_replay(data, traj, costs, lags) {
            Ok(report) => Some(report),
            // A declared lag the runs cannot hold is a usage error.
            Err(error @ ReplayNullRefusal::LagTooLong { .. }) => {
                eprintln!("error: lagged replay: {error}");
                return ExitCode::from(2);
            }
            Err(error) => {
                eprintln!("error: lagged replay: {error}");
                return ExitCode::FAILURE;
            }
        },
        None => None,
    };
    let diagnostics = ReplayDiagnostics {
        rank_neutral: true,
        valid_when: VALID_WHEN,
        timing_null,
        lagged_replay: lagged,
    };
    if json {
        #[derive(Serialize)]
        struct WithDiagnostics<'a> {
            #[serde(flatten)]
            verification: &'a sharpebench_core::EntrantView,
            replay_diagnostics: &'a ReplayDiagnostics,
        }
        let sealed =
            sharpebench_core::seal(result, &sharpebench_harness::VERIFICATION_RESULT_VISIBILITY)
                .expect("verification results serialize");
        emit_json(&WithDiagnostics {
            verification: &sealed,
            replay_diagnostics: &diagnostics,
        });
    } else {
        emit_verification(result, false, None);
        print_diagnostics(&diagnostics);
    }
    ExitCode::SUCCESS
}

fn reason(reason: TimingNullUnavailable) -> &'static str {
    match reason {
        TimingNullUnavailable::NeverInvested => "never invested",
        TimingNullUnavailable::AlwaysInvested => "invested on every bar",
        TimingNullUnavailable::NoRunWithTimingFreedom => "no run was both invested and flat",
    }
}

fn lag_reason(why: LaggedRunUnavailable) -> String {
    match why {
        LaggedRunUnavailable::NeverFills { lag } => {
            format!("the lag-{lag} row never holds a position in the window")
        }
        LaggedRunUnavailable::TooFewComparedBars {
            skipped_leading_bars,
        } => format!(
            "fewer than two bars remain after the {skipped_leading_bars} bars through the last opening fill"
        ),
        LaggedRunUnavailable::NoSharpe { lag } => {
            format!("the lag-{lag} row has no Sharpe on the compared bars (constant, as when flat)")
        }
    }
}

fn placements(count: u64) -> String {
    if count == u64::MAX {
        format!(">= {count}")
    } else {
        count.to_string()
    }
}

/// What the percentile's resolution column says. A plug-in standard error where
/// the draws had spread, and the exact one-sided 95% bound where every draw fell
/// on one side, because the plug-in estimate reads zero there for a quantity
/// that is not known exactly.
fn resolution(reference: &TimingPercentile) -> String {
    match (
        reference.monte_carlo_standard_error,
        reference.one_sided_95_bound,
    ) {
        (Some(se), _) => format!("+/- {se:.3}"),
        (None, Some(bound)) if reference.percentile > 0.5 => format!(">= {bound:.4}"),
        (None, Some(bound)) => format!("<= {bound:.4}"),
        (None, None) => "every draw tied".to_string(),
    }
}

fn print_diagnostics(diagnostics: &ReplayDiagnostics) {
    println!(
        "
Replay diagnostics (rank-neutral: the gate and the rank never read them)"
    );
    println!("  {}", diagnostics.valid_when);
    if let Some(report) = &diagnostics.timing_null {
        println!(
            "
  Exposure-matched random timing: {} draws, seed {}; runs over one window share placements",
            report.draws, report.seed
        );
        println!(
            "  {:>4}  {:>11}  {:>9}  {:>8}  {:>11}  {:>10}  {:>12}  {:>20}  {:>7}",
            "run",
            "window",
            "invested",
            "periods",
            "entrant SR",
            "percentile",
            "MC s.e.",
            "distinct placements",
            "max pct"
        );
        for run in &report.runs {
            match run {
                RunTimingNull::Available {
                    run,
                    window_start,
                    window_end,
                    exposure,
                    entrant_sharpe,
                    reference,
                    distinct_placements,
                    max_attainable_percentile,
                    ..
                } => println!(
                    "  {run:>4}  {:>11}  {:>9}  {:>8}  {entrant_sharpe:>11.4}  {:>10.3}  {:>12}  {:>20}  {max_attainable_percentile:>7.3}",
                    format!("[{window_start}, {window_end})"),
                    format!("{}/{}", exposure.invested_bars, exposure.bars),
                    exposure.holding_periods,
                    reference.percentile,
                    resolution(reference),
                    placements(*distinct_placements)
                ),
                RunTimingNull::Unavailable {
                    run, reason: why, ..
                } => {
                    println!("  {run:>4}  unavailable: {}", reason(*why))
                }
            }
        }
        match &report.aggregate {
            TimingNullAggregate::Available {
                runs,
                entrant_mean_sharpe,
                reference,
            } => println!(
                "  across {runs} runs: mean Sharpe {entrant_mean_sharpe:.4} sits at percentile {:.3} ({}) of {} draws (reference mean {:.4})",
                reference.percentile,
                resolution(reference),
                reference.draws,
                reference.reference_mean_sharpe
            ),
            TimingNullAggregate::Unavailable { reason: why } => {
                println!("  across runs: unavailable, {}", reason(*why))
            }
        }
    }
    if let Some(report) = &diagnostics.lagged_replay {
        println!(
            "
  Lagged replay: decisions executed k bars late; each run is compared on the bars after every row's opening fill"
        );
        println!("  {}", report.end_effect);
        for run in &report.runs {
            match run {
                LaggedRun::Available {
                    run,
                    skipped_leading_bars,
                    compared_bars,
                    ..
                } => println!(
                    "  run {run:>4}: {compared_bars} bars compared after skipping {skipped_leading_bars}"
                ),
                LaggedRun::Unavailable { run, why } => {
                    println!("  run {run:>4}: unavailable, {}", lag_reason(*why))
                }
            }
        }
        match &report.aggregate {
            LaggedAggregate::Available {
                runs,
                compared_bars,
                undelayed,
                lagged,
            } => {
                println!("  across {runs} runs and {compared_bars} compared bars:");
                println!(
                    "  {:>9}  {:>11}  {:>11}  {:>16}",
                    "lag", "mean SR", "mean return", "Sharpe change"
                );
                println!(
                    "  {:>9}  {:>11.4}  {:>11.6}  {:>16}",
                    "undelayed", undelayed.mean_sharpe, undelayed.mean_return, "-"
                );
                for row in lagged {
                    println!(
                        "  {:>9}  {:>11.4}  {:>11.6}  {:>+16.4}",
                        row.lag,
                        row.mean_sharpe,
                        row.mean_return,
                        row.mean_sharpe - undelayed.mean_sharpe
                    );
                }
            }
            LaggedAggregate::Unavailable { .. } => {
                println!("  across runs: unavailable, no run has comparable rows")
            }
        }
    }
}
