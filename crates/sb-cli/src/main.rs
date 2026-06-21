//! `sharpebench` — the command-line entry point.
//!
//! Phase 0 ships the `score` subcommand: read a JSON array of agent submissions,
//! rank them on the luck-robust composite, and print the leaderboard. The harness
//! that *produces* those submissions (sim + agent protocol) lands in Phase 1.
//!
//! ```text
//! sharpebench score <submissions.json>
//! ```

use std::process::ExitCode;

use sb_core::{rank, AgentSubmission, ScoreConfig};

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().collect();
    match args.get(1).map(String::as_str) {
        Some("score") => match args.get(2) {
            Some(path) => run_score(path),
            None => {
                eprintln!("usage: sharpebench score <submissions.json>");
                ExitCode::from(2)
            }
        },
        Some("--help") | Some("-h") | None => {
            println!("sharpebench — luck-robust benchmark for AI trading agents\n");
            println!("USAGE:\n  sharpebench score <submissions.json>\n");
            println!("A submission file is a JSON array of {{ agent_id, runs: [{{ returns, trace?, ... }}] }}.");
            ExitCode::SUCCESS
        }
        Some(other) => {
            eprintln!("unknown command: {other}\nrun `sharpebench --help`");
            ExitCode::from(2)
        }
    }
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

    let cfg = ScoreConfig::default();
    let board = rank(&subs, &cfg);

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
    ExitCode::SUCCESS
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
