//! `sharpebench` — the command-line entry point.
//!
//! - `sharpebench run` — run the reference agents through the point-in-time
//!   simulator (multiple windows × seeds, costs on) and rank them.
//! - `sharpebench score <submissions.json>` — rank a JSON field of pre-computed
//!   submissions on the luck-robust composite.

use std::process::ExitCode;

use sharpebench_core::{rank, AgentSubmission, CompositeScore, ScoreConfig};

#[cfg(feature = "self-update")]
mod update;

fn main() -> ExitCode {
    // `--json` may appear anywhere; strip it so positional parsing is unaffected.
    let raw: Vec<String> = std::env::args().collect();
    let json = raw.iter().any(|a| a == "--json");
    let args: Vec<String> = raw.into_iter().filter(|a| a != "--json").collect();
    let subcommand = args.get(1).map(String::as_str);

    // Throttled, fail-soft "a newer version exists" nudge (opt-in build feature).
    #[cfg(feature = "self-update")]
    update::notify_if_outdated(json, subcommand);

    match subcommand {
        Some("run") => run_demo(&args, json),
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
        Some("capture") => run_capture(&args, json),
        Some("verify-trajectory") => run_verify_trajectory(&args, json),
        Some("self-update" | "update") => run_self_update(),
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

/// Update the running binary in place. Only present (and only pulls a TLS stack)
/// in `--features self-update` builds; the default build prints how to upgrade so
/// the published CLI and the musl static binary stay dependency-free.
fn run_self_update() -> ExitCode {
    #[cfg(feature = "self-update")]
    {
        update::run_self_update()
    }
    #[cfg(not(feature = "self-update"))]
    {
        eprintln!(
            "this build has self-update disabled.\n\
             upgrade with `cargo install sharpebench`, re-download the binary from\n\
             https://github.com/general-liquidity/sharpebench/releases/latest, or\n\
             rebuild with `cargo install sharpebench --features self-update`."
        );
        ExitCode::from(2)
    }
}

/// Print a value as pretty JSON to stdout (machine-readable mode).
fn emit_json<T: serde::Serialize>(value: &T) {
    match serde_json::to_string_pretty(value) {
        Ok(j) => println!("{j}"),
        Err(e) => eprintln!("error: serializing output: {e}"),
    }
}

/// Value following a `--flag` in argv (e.g. `--http 127.0.0.1:8080`), if present.
fn flag_value<'a>(args: &'a [String], flag: &str) -> Option<&'a str> {
    args.iter()
        .position(|a| a == flag)
        .and_then(|i| args.get(i + 1))
        .map(String::as_str)
}

/// Resolve a signing-key argument. To keep secrets out of process listings and
/// shell history, `env:NAME` reads the key from an environment variable and
/// `file:PATH` reads it from a file (trailing newline trimmed); anything else is
/// used as the literal key.
fn resolve_key(spec: &str) -> std::io::Result<Vec<u8>> {
    if let Some(var) = spec.strip_prefix("env:") {
        std::env::var(var)
            .map(String::into_bytes)
            .map_err(|_| std::io::Error::other(format!("env var {var} is not set")))
    } else if let Some(path) = spec.strip_prefix("file:") {
        Ok(std::fs::read_to_string(path)?
            .trim_end()
            .as_bytes()
            .to_vec())
    } else {
        Ok(spec.as_bytes().to_vec())
    }
}

fn help() {
    println!("sharpebench — luck-robust benchmark for AI trading agents\n");
    println!("USAGE:");
    println!(
        "  sharpebench run [--data <csv>] [--http <addr>|--cmd \"<prog>\"]  run agents and rank"
    );
    println!("                       --data: a frozen CSV (else synthetic) · --http/--cmd: add YOUR agent");
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
    println!(
        "  sharpebench capture <agent> <out.json> [--data <csv>]  capture an agent's raw-decision trajectory artifact"
    );
    println!(
        "  sharpebench verify-trajectory <traj.json> [--data <csv>]  replay a trajectory → recompute its score from raw decisions"
    );
    println!("  sharpebench self-update               update the binary in place (--features self-update builds)");
    println!("\n<key> accepts a literal, or env:NAME / file:PATH to keep secrets out of process listings.");
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
    let key = match resolve_key(&args[3]) {
        Ok(k) => k,
        Err(e) => {
            eprintln!("error: {e}");
            return ExitCode::FAILURE;
        }
    };
    let pb = sharpebench_leaderboard::publish(&rank(&subs, &ScoreConfig::default()), &key);
    match sharpebench_leaderboard::save(&pb, &args[4]) {
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
    let pb = match sharpebench_leaderboard::load(&args[2]) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("error: cannot load {}: {e}", args[2]);
            return ExitCode::FAILURE;
        }
    };
    let key = match resolve_key(&args[3]) {
        Ok(k) => k,
        Err(e) => {
            eprintln!("error: {e}");
            return ExitCode::FAILURE;
        }
    };
    let ok = sharpebench_leaderboard::verify_board(&pb.chain, &key);
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
    let report = sharpebench_core::run_self_audit();
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
    use sharpebench_sim::{Agent, BuyAndHold, CostModel, Dataset, Momentum, Window};

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
        let bh = sharpebench_harness::run_agent(
            "buy-and-hold",
            &masked,
            &windows,
            &seeds,
            costs,
            || Box::new(BuyAndHold) as Box<dyn Agent>,
        );
        let mo =
            sharpebench_harness::run_agent("momentum", &masked, &windows, &seeds, costs, || {
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
    let c = sharpebench_attest::make_commitment(&args[2], &args[3], &args[4], &args[5]);
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

fn run_demo(args: &[String], json: bool) -> ExitCode {
    use sharpebench_sim::{
        Agent, BuyAndHold, CostModel, Dataset, ExternalAgent, HoldAgent, HttpAgent, Momentum,
        Window,
    };

    let (data, windows) = match flag_value(args, "--data") {
        Some(path) => match Dataset::from_csv_file(path) {
            Ok(d) => {
                let n = d.len();
                if n < 40 {
                    eprintln!("error: dataset too short ({n} rows); need at least 40");
                    return ExitCode::FAILURE;
                }
                // A warmup, then split the rest into an in-sample + out-of-sample window.
                let warm = (n / 10).clamp(10, 30);
                let mid = (warm + n) / 2;
                let w = vec![
                    Window {
                        start: warm,
                        end: mid,
                    },
                    Window { start: mid, end: n },
                ];
                (d, w)
            }
            Err(e) => {
                eprintln!("error: {e}");
                return ExitCode::FAILURE;
            }
        },
        None => (
            Dataset::synthetic(8, 180, 20_260_621),
            vec![
                Window {
                    start: 20,
                    end: 100,
                },
                Window {
                    start: 100,
                    end: 180,
                },
            ],
        ),
    };
    let seeds: Vec<u64> = (0..8).collect();
    let costs = CostModel::default();

    let bh = sharpebench_harness::run_agent("buy-and-hold", &data, &windows, &seeds, costs, || {
        Box::new(BuyAndHold) as Box<dyn Agent>
    });
    let mo = sharpebench_harness::run_agent("momentum", &data, &windows, &seeds, costs, || {
        Box::new(Momentum::default()) as Box<dyn Agent>
    });
    // The luck floor: random monkeys that show the zero-skill distribution.
    let mut field = vec![bh, mo];
    field.extend(sharpebench_harness::luck_floor(
        &data, &windows, &seeds, costs, 3,
    ));

    // Optionally drive a real external agent (yours) through the *same* sim and
    // rank it into the field. `--http` hits a POST /decide endpoint; `--cmd` spawns
    // a subprocess speaking newline-delimited JSON over stdio (see examples/reference-agent).
    if let Some(addr) = flag_value(args, "--http") {
        let addr = addr.to_string();
        let label = format!("http:{addr}");
        let sub =
            sharpebench_harness::run_agent(&label, &data, &windows, &seeds, costs, move || {
                Box::new(HttpAgent::new(addr.clone())) as Box<dyn Agent>
            });
        field.insert(0, sub);
    } else if let Some(cmd) = flag_value(args, "--cmd") {
        let parts: Vec<String> = cmd.split_whitespace().map(String::from).collect();
        let Some((prog, rest)) = parts.split_first() else {
            eprintln!("error: --cmd needs a program to run");
            return ExitCode::from(2);
        };
        let prog = prog.clone();
        let rest = rest.to_vec();
        // Pre-flight: fail fast with a clear message if the agent won't spawn.
        let rest_refs: Vec<&str> = rest.iter().map(String::as_str).collect();
        if ExternalAgent::spawn(&prog, &rest_refs).is_err() {
            eprintln!("error: cannot spawn agent `{cmd}`");
            return ExitCode::FAILURE;
        }
        let label = format!("cmd:{prog}");
        let sub =
            sharpebench_harness::run_agent(&label, &data, &windows, &seeds, costs, move || {
                let rest_refs: Vec<&str> = rest.iter().map(String::as_str).collect();
                ExternalAgent::spawn(&prog, &rest_refs)
                    .map(|a| Box::new(a) as Box<dyn Agent>)
                    .unwrap_or_else(|_| Box::new(HoldAgent))
            });
        field.insert(0, sub);
    }

    if !json {
        let src = flag_value(args, "--data").unwrap_or("synthetic");
        println!(
            "SharpeBench — run on {src} ({} symbols, {} windows × {} seeds, costs on; incl. luck floor)\n",
            data.symbols().len(),
            windows.len(),
            seeds.len()
        );
    }
    emit_board(&rank(&field, &ScoreConfig::default()), json);
    ExitCode::SUCCESS
}

/// Resolve the dataset + windows for the trajectory subcommands. Identical logic
/// to `run_demo`'s resolver, so a `capture` and a `verify-trajectory` over the same
/// `--data` (or both synthetic) replay against the byte-identical frozen dataset.
fn resolve_dataset(
    args: &[String],
) -> Result<(sharpebench_sim::Dataset, Vec<sharpebench_sim::Window>), String> {
    use sharpebench_sim::{Dataset, Window};
    match flag_value(args, "--data") {
        Some(path) => {
            let d = Dataset::from_csv_file(path)?;
            let n = d.len();
            if n < 40 {
                return Err(format!("dataset too short ({n} rows); need at least 40"));
            }
            let warm = (n / 10).clamp(10, 30);
            let mid = (warm + n) / 2;
            let w = vec![
                Window {
                    start: warm,
                    end: mid,
                },
                Window { start: mid, end: n },
            ];
            Ok((d, w))
        }
        None => Ok((
            Dataset::synthetic(8, 180, 20_260_621),
            vec![
                Window {
                    start: 20,
                    end: 100,
                },
                Window {
                    start: 100,
                    end: 180,
                },
            ],
        )),
    }
}

/// `capture` — run a reference agent through the sim and persist its raw-decision
/// trajectory artifact (NOT its returns/metrics) to a JSON file.
fn run_capture(args: &[String], json: bool) -> ExitCode {
    use sharpebench_sim::{Agent, BuyAndHold, CostModel, Momentum};

    if args.len() < 4 {
        eprintln!(
            "usage: sharpebench capture <buy-and-hold|momentum> <out.json> [--data <csv>] [--json]"
        );
        return ExitCode::from(2);
    }
    let agent_id = args[2].as_str();
    let out = &args[3];
    let (data, windows) = match resolve_dataset(args) {
        Ok(dw) => dw,
        Err(e) => {
            eprintln!("error: {e}");
            return ExitCode::FAILURE;
        }
    };
    let seeds: Vec<u64> = (0..8).collect();
    let costs = CostModel::default();
    let make: Box<dyn Fn() -> Box<dyn Agent>> = match agent_id {
        "buy-and-hold" => Box::new(|| Box::new(BuyAndHold) as Box<dyn Agent>),
        "momentum" => Box::new(|| Box::new(Momentum::default()) as Box<dyn Agent>),
        other => {
            eprintln!("error: unknown agent `{other}` (use buy-and-hold or momentum)");
            return ExitCode::from(2);
        }
    };
    let (_sub, traj) =
        sharpebench_harness::run_agent_capture(agent_id, &data, &windows, &seeds, costs, || make());
    let payload = match serde_json::to_string_pretty(&traj) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("error: serializing trajectory: {e}");
            return ExitCode::FAILURE;
        }
    };
    if let Err(e) = std::fs::write(out, payload) {
        eprintln!("error: cannot write {out}: {e}");
        return ExitCode::FAILURE;
    }
    if json {
        emit_json(&serde_json::json!({
            "captured": true,
            "agent_id": agent_id,
            "runs": traj.runs.len(),
            "path": out,
        }));
    } else {
        println!(
            "captured trajectory for `{agent_id}` ({} runs) -> {out}",
            traj.runs.len()
        );
    }
    ExitCode::SUCCESS
}

/// `verify-trajectory` — the separate-verifier path: ingest a persisted trajectory
/// artifact, replay its raw decisions through the frozen dataset's point-in-time
/// engine, and recompute the score from those decisions alone (never the agent's
/// self-reported metrics).
fn run_verify_trajectory(args: &[String], json: bool) -> ExitCode {
    use sharpebench_protocol::AgentTrajectory;
    use sharpebench_sim::CostModel;

    if args.len() < 3 {
        eprintln!("usage: sharpebench verify-trajectory <trajectory.json> [--data <csv>] [--json]");
        return ExitCode::from(2);
    }
    let text = match std::fs::read_to_string(&args[2]) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("error: cannot read {}: {e}", args[2]);
            return ExitCode::FAILURE;
        }
    };
    let traj: AgentTrajectory = match serde_json::from_str(&text) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("error: invalid trajectory JSON: {e}");
            return ExitCode::FAILURE;
        }
    };
    let (data, _windows) = match resolve_dataset(args) {
        Ok(dw) => dw,
        Err(e) => {
            eprintln!("error: {e}");
            return ExitCode::FAILURE;
        }
    };
    let result = sharpebench_harness::verify_trajectory(
        &data,
        &traj,
        CostModel::default(),
        &ScoreConfig::default(),
    );
    if json {
        emit_json(&result);
    } else {
        println!(
            "verified `{}` by replay — {} decisions across {} runs",
            result.agent_id, result.decisions_replayed, result.runs_replayed
        );
        println!("  deflated Sharpe : {:.4}", result.score.deflated_sharpe);
        println!("  raw mean return : {:.5}", result.score.raw_mean_return);
        println!("  rank-eligible   : {}", yn(result.score.rank_eligible));
        println!("\n{}", result.verification_explanation);
    }
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
