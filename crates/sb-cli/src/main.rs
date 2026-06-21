//! `sharpebench` — the command-line entry point.
//!
//! - `sharpebench run` — run the reference agents through the point-in-time
//!   simulator (multiple windows × seeds, costs on) and rank them.
//! - `sharpebench score <submissions.json>` — rank a JSON field of pre-computed
//!   submissions on the luck-robust composite.

use std::process::ExitCode;

use sb_core::{rank, AgentSubmission, CompositeScore, ScoreConfig};

fn main() -> ExitCode {
    // `--json` may appear anywhere; strip it so positional parsing is unaffected.
    let raw: Vec<String> = std::env::args().collect();
    let json = raw.iter().any(|a| a == "--json");
    let args: Vec<String> = raw.into_iter().filter(|a| a != "--json").collect();
    match args.get(1).map(String::as_str) {
        Some("run") => run_demo(json),
        Some("score") => match args.get(2) {
            Some(path) => run_score(path, json),
            None => {
                eprintln!("usage: sharpebench score <submissions.json> [--json]");
                ExitCode::from(2)
            }
        },
        Some("commit") => run_commit(&args),
        Some("stress") => run_stress(json),
        Some("audit") => run_audit(json),
        Some("sign") => run_sign(&args, json),
        Some("verify") => run_verify(&args, json),
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

/// Print a value as pretty JSON to stdout (machine-readable mode).
fn emit_json<T: serde::Serialize>(value: &T) {
    match serde_json::to_string_pretty(value) {
        Ok(j) => println!("{j}"),
        Err(e) => eprintln!("error: serializing output: {e}"),
    }
}

fn help() {
    println!("sharpebench — luck-robust benchmark for AI trading agents\n");
    println!("USAGE:");
    println!("  sharpebench run                       run reference agents through the sim and rank them");
    println!(
        "  sharpebench score <submissions.json>  rank a JSON field of pre-computed submissions"
    );
    println!(
        "  sharpebench commit <agent> <window> <digest> <salt>  forward-attestation pre-registration"
    );
    println!("  sharpebench stress                    run the adversarial stress suite (masked)");
    println!("  sharpebench audit                     self-audit: prove the scorer resists gaming");
    println!("  sharpebench sign <subs.json> <key> <out.json>  score + sign a board to a file");
    println!("  sharpebench verify <board.json> <key>  verify a signed board's chain");
    println!("\nGlobal flags:");
    println!("  --json   emit machine-readable JSON instead of a human table (for agents / CI)");
}

fn run_sign(args: &[String], json: bool) -> ExitCode {
    if args.len() < 5 {
        eprintln!("usage: sharpebench sign <submissions.json> <key> <out.json> [--json]");
        return ExitCode::from(2);
    }
    let data = match std::fs::read_to_string(&args[2]) {
        Ok(d) => d,
        Err(e) => {
            eprintln!("error: cannot read {}: {e}", args[2]);
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
    let pb = sb_leaderboard::publish(&rank(&subs, &ScoreConfig::default()), args[3].as_bytes());
    match sb_leaderboard::save(&pb, &args[4]) {
        Ok(()) => {
            if json {
                emit_json(&serde_json::json!({
                    "signed": true,
                    "entries": pb.chain.len(),
                    "path": args[4],
                }));
            } else {
                println!("signed board ({} entries) -> {}", pb.chain.len(), args[4]);
            }
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}

fn run_verify(args: &[String], json: bool) -> ExitCode {
    if args.len() < 4 {
        eprintln!("usage: sharpebench verify <board.json> <key> [--json]");
        return ExitCode::from(2);
    }
    let pb = match sb_leaderboard::load(&args[2]) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("error: cannot load {}: {e}", args[2]);
            return ExitCode::FAILURE;
        }
    };
    let ok = sb_leaderboard::verify_board(&pb.chain, args[3].as_bytes());
    if json {
        emit_json(&serde_json::json!({ "ok": ok, "entries": pb.chain.len() }));
    } else if ok {
        println!("OK — {} entries, signature chain valid", pb.chain.len());
    } else {
        eprintln!("FAIL — signature chain invalid (tampered or wrong key)");
    }
    if ok {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

fn run_audit(json: bool) -> ExitCode {
    let report = sb_core::run_self_audit();
    if json {
        emit_json(&report);
    } else {
        println!("SharpeBench — benchmark self-audit (does the scorer resist gaming?)\n");
        for c in &report.cases {
            println!(
                "[{}] {:<26} {}",
                if c.defended { "DEFENDED" } else { "  GAMED " },
                c.name,
                c.detail
            );
        }
        if report.all_defended {
            println!(
                "\nAll {} attacks demoted. The benchmark holds.",
                report.cases.len()
            );
        } else {
            eprintln!("\nFAIL — an attack was not demoted; a gate has regressed.");
        }
    }
    if report.all_defended {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

fn run_stress(json: bool) -> ExitCode {
    use sb_sim::{Agent, BuyAndHold, CostModel, Dataset, Momentum, Window};

    let seeds: Vec<u64> = (0..6).collect();
    let costs = CostModel::default();
    if !json {
        println!("SharpeBench — adversarial stress suite (contamination-masked, costs on)\n");
    }
    let mut scenarios: Vec<serde_json::Value> = Vec::new();
    for (name, data) in Dataset::stress_suite(20_260_621) {
        let masked = data.masked();
        let windows = [Window {
            start: 20,
            end: masked.len(),
        }];
        let bh = sb_harness::run_agent("buy-and-hold", &masked, &windows, &seeds, costs, || {
            Box::new(BuyAndHold) as Box<dyn Agent>
        });
        let mo = sb_harness::run_agent("momentum", &masked, &windows, &seeds, costs, || {
            Box::new(Momentum::default()) as Box<dyn Agent>
        });
        let board = rank(&[bh, mo], &ScoreConfig::default());
        if json {
            scenarios.push(serde_json::json!({ "scenario": name, "board": board }));
        } else {
            println!("# scenario: {name}");
            print_board(&board);
            println!();
        }
    }
    if json {
        emit_json(&scenarios);
    }
    ExitCode::SUCCESS
}

fn run_commit(args: &[String]) -> ExitCode {
    if args.len() < 6 {
        eprintln!("usage: sharpebench commit <agent_id> <target_window> <artifact_digest> <salt>");
        return ExitCode::from(2);
    }
    let c = sb_attest::make_commitment(&args[2], &args[3], &args[4], &args[5]);
    match serde_json::to_string_pretty(&c) {
        Ok(j) => {
            println!("{j}");
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}

fn run_demo(json: bool) -> ExitCode {
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
    // The luck floor: random monkeys that show the zero-skill distribution.
    let mut field = vec![bh, mo];
    field.extend(sb_harness::luck_floor(&data, &windows, &seeds, costs, 3));

    if !json {
        println!(
            "SharpeBench — reference run ({} windows × {} seeds, costs on; incl. luck floor)\n",
            windows.len(),
            seeds.len()
        );
    }
    emit_board(&rank(&field, &ScoreConfig::default()), json);
    ExitCode::SUCCESS
}

fn run_score(path: &str, json: bool) -> ExitCode {
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
    emit_board(&rank(&subs, &ScoreConfig::default()), json);
    ExitCode::SUCCESS
}

/// Render a board as a human table, or as JSON when `json` is set.
fn emit_board(board: &[CompositeScore], json: bool) {
    if json {
        emit_json(&board);
    } else {
        print_board(board);
    }
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
