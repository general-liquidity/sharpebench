//! `sharpebench` — the command-line entry point.
//!
//! - `sharpebench run` — run the reference agents through the point-in-time
//!   simulator (multiple windows × seeds, costs on) and rank them.
//! - `sharpebench score <submissions.json>` — rank a JSON field of pre-computed
//!   submissions on the luck-robust composite.

use std::process::ExitCode;

use sb_core::{rank, AgentSubmission, CompositeScore, ScoreConfig};

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().collect();
    match args.get(1).map(String::as_str) {
        Some("run") => run_demo(),
        Some("score") => match args.get(2) {
            Some(path) => run_score(path),
            None => {
                eprintln!("usage: sharpebench score <submissions.json>");
                ExitCode::from(2)
            }
        },
        Some("--help") | Some("-h") | None => {
            help();
            ExitCode::SUCCESS
        }
        Some(other) => {
            eprintln!("unknown command: {other}\nrun `sharpebench --help`");
            ExitCode::from(2)
        }
    }
}

fn help() {
    println!("sharpebench — luck-robust benchmark for AI trading agents\n");
    println!("USAGE:");
    println!("  sharpebench run                       run reference agents through the sim and rank them");
    println!(
        "  sharpebench score <submissions.json>  rank a JSON field of pre-computed submissions"
    );
}

fn run_demo() -> ExitCode {
    use sb_sim::{Agent, BuyAndHold, CostModel, Dataset, Momentum, Window};

    let data = Dataset::synthetic(8, 180, 20_260_621);
    let windows = [
        Window {
            start: 20,
            end: 100,
        },
        Window {
            start: 100,
            end: 180,
        },
    ];
    let seeds: Vec<u64> = (0..8).collect();
    let costs = CostModel::default();

    let bh = sb_harness::run_agent("buy-and-hold", &data, &windows, &seeds, costs, || {
        Box::new(BuyAndHold) as Box<dyn Agent>
    });
    let mo = sb_harness::run_agent("momentum", &data, &windows, &seeds, costs, || {
        Box::new(Momentum::default()) as Box<dyn Agent>
    });

    println!(
        "SharpeBench — reference run ({} windows × {} seeds, costs on)\n",
        windows.len(),
        seeds.len()
    );
    print_board(&rank(&[bh, mo], &ScoreConfig::default()));
    ExitCode::SUCCESS
}

fn run_score(path: &str) -> ExitCode {
    let data = match std::fs::read_to_string(path) {
        Ok(d) => d,
        Err(e) => {
            eprintln!("error: cannot read {path}: {e}");
            return ExitCode::FAILURE;
        }
    };
    let subs: Vec<AgentSubmission> = match serde_json::from_str(&data) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("error: invalid submissions JSON: {e}");
            return ExitCode::FAILURE;
        }
    };
    print_board(&rank(&subs, &ScoreConfig::default()));
    ExitCode::SUCCESS
}

fn print_board(board: &[CompositeScore]) {
    println!(
        "{:<4} {:<18} {:>9} {:>8} {:>7} {:>6} {:>9} {:>10}",
        "#", "agent", "DSR", "PSR", "pass^k", "proc", "boot_p", "raw_ret"
    );
    println!("{}", "-".repeat(80));
    for (i, s) in board.iter().enumerate() {
        let pos = if s.rank_eligible {
            format!("{}", i + 1)
        } else {
            "—".to_string()
        };
        println!(
            "{:<4} {:<18} {:>9.4} {:>8.4} {:>7} {:>6} {:>9.4} {:>10.5}",
            pos,
            truncate(&s.agent_id, 18),
            s.deflated_sharpe,
            s.psr,
            yn(s.passed_k),
            yn(s.process_ok),
            s.bootstrap_p,
            s.raw_mean_return,
        );
    }
    println!(
        "\n{} eligible of {} submitted. Rank key = deflated Sharpe; raw return never ranks.",
        board.iter().filter(|s| s.rank_eligible).count(),
        board.len()
    );
}

fn yn(b: bool) -> &'static str {
    if b {
        "yes"
    } else {
        "NO"
    }
}

fn truncate(s: &str, n: usize) -> String {
    if s.len() <= n {
        s.to_string()
    } else {
        format!("{}…", &s[..n - 1])
    }
}
