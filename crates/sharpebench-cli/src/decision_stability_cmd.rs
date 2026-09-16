//! `sharpebench decision-stability`: how often an entrant decided differently
//! when it had been shown the same observations, over the replicate runs of its
//! captured trajectories. Rank-neutral; see the book's "Decision stability"
//! chapter.

use std::collections::BTreeSet;
use std::fs;
use std::path::PathBuf;

use sharpebench_core::{DecisionStabilityReport, StabilityCounts, StabilityRate};
use sharpebench_harness::decision_stability::decision_stability_from_trajectories;
use sharpebench_protocol::AgentTrajectory;

const USAGE: &str = "usage: sharpebench decision-stability <traj.json> [<traj.json> ...] [--data <csv>] [--short-borrow-bps <bps>] [--json]";

pub(crate) fn run(args: &[String], json: bool) -> i32 {
    let paths = match trajectory_paths(args) {
        Ok(paths) => paths,
        Err(error) => {
            eprintln!("error: {error}\n{USAGE}");
            return 2;
        }
    };
    // The replay must use the cost model the trajectories were captured
    // under; the strict checks refuse any other one by its digest.
    let costs = match crate::cost_model_from_args(args) {
        Ok(costs) => costs,
        Err(error) => {
            eprintln!("error: {error}");
            return 2;
        }
    };
    let mut trajectories = Vec::with_capacity(paths.len());
    for path in &paths {
        let text = match fs::read_to_string(path) {
            Ok(text) => text,
            Err(error) => {
                eprintln!("error: cannot read {path}: {error}");
                return 1;
            }
        };
        match serde_json::from_str::<AgentTrajectory>(&text) {
            Ok(trajectory) => trajectories.push(trajectory),
            Err(error) => {
                eprintln!("error: invalid trajectory JSON in {path}: {error}");
                return 1;
            }
        }
    }
    let (data, _windows) = match crate::resolve_dataset(args) {
        Ok(resolved) => resolved,
        Err(error) => {
            eprintln!("error: {error}");
            return 1;
        }
    };
    let runner = match crate::current_executable_sha256() {
        Ok(digest) => digest,
        Err(error) => {
            eprintln!("error: {error}");
            return 1;
        }
    };
    match decision_stability_from_trajectories(&data, &trajectories, costs, Some(&runner)) {
        Ok(report) => {
            if json {
                crate::emit_json(&report);
            } else {
                print_report(&report);
            }
            0
        }
        Err(error) => {
            eprintln!("error: {error}");
            1
        }
    }
}

/// The positional trajectory paths, each named once.
fn trajectory_paths(args: &[String]) -> Result<Vec<String>, String> {
    let mut paths = Vec::new();
    let mut seen = BTreeSet::new();
    let mut rest = args.iter().skip(2);
    while let Some(arg) = rest.next() {
        if arg == "--data" {
            rest.next()
                .ok_or_else(|| "--data requires a CSV path".to_string())?;
        } else if arg == "--short-borrow-bps" {
            // The value is validated by `cost_model_from_args`.
            rest.next();
        } else if arg.starts_with("--") {
            return Err(format!("unknown option `{arg}`"));
        } else {
            // The same file twice would be one capture counted as two
            // replicates that agree with each other by construction.
            let identity = fs::canonicalize(arg).unwrap_or_else(|_| PathBuf::from(arg));
            if !seen.insert(identity) {
                return Err(format!("trajectory `{arg}` is named more than once"));
            }
            paths.push(arg.clone());
        }
    }
    if paths.is_empty() {
        return Err("name at least one trajectory".to_string());
    }
    Ok(paths)
}

fn rate(rate: &StabilityRate) -> String {
    match rate {
        StabilityRate::Available { value } => format!("{value:.4}"),
        StabilityRate::Unavailable { reason } => format!(
            "unavailable ({})",
            serde_json::to_value(reason)
                .ok()
                .and_then(|value| value.as_str().map(str::to_string))
                .unwrap_or_default()
        ),
    }
}

fn print_counts(indent: &str, counts: &StabilityCounts) {
    println!(
        "{indent}differing fraction : {} ({} of {} groups)",
        rate(&counts.differing_fraction),
        counts.groups_with_differing_decisions,
        counts.groups_compared
    );
    println!(
        "{indent}steps              : {} compared, {} excluded (observation diverged), {} without a replicate, {} total",
        counts.steps_compared,
        counts.steps_excluded_diverged_observation,
        counts.steps_unreplicated,
        counts.steps_total
    );
}

fn print_report(report: &DecisionStabilityReport) {
    println!(
        "decision stability for `{}` over {} replicate runs (rank-neutral, not a rank input)",
        report.agent_id, report.replicate_runs
    );
    print_counts("  ", &report.totals);
    for window in &report.windows {
        println!(
            "  window [{}, {}): {} replicates",
            window.window_start, window.window_end, window.replicates
        );
        print_counts("    ", &window.counts);
        for group in &window.differing_groups {
            println!(
                "    step {}: {} distinct decisions among {} replicates (observation {})",
                group.step, group.distinct_decisions, group.replicates, group.observation_sha256
            );
        }
    }
    println!("\ngrouping: {}", report.grouping);
    println!("differ  : {}", report.decision_difference);
}
