//! `sharpebench` — the command-line entry point.
//!
//! - `sharpebench run` — run the reference agents through the point-in-time
//!   simulator (multiple windows × seeds, costs on) and rank them.
//! - `sharpebench score <submissions.json>` — rank a JSON field of pre-computed
//!   submissions on the luck-robust composite.

#![forbid(unsafe_code)]

use std::process::ExitCode;

use serde::Serialize;
use sharpebench_core::{rank, AgentSubmission, CompositeScore, ScoreConfig};
use sharpebench_harness::accounting::{MonetarySummary, RateCard};
use sharpebench_harness::fault_plan::FaultPlan;
use sharpebench_harness::BackoffSchedule;

use csv_columns::read_returns_column;

mod analysis_cmd;
mod arena_cmd;
mod artifact_preflight;
mod csv_columns;
mod external_capture;
mod forecast_cmd;
mod gateway_cli;
mod import_cmd;
mod lineage_cmd;
mod rescore_cmd;
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
            Some(path) => run_score(path, &args, json),
            None => {
                eprintln!("usage: sharpebench score <submissions.json> [--require-run-keys] [--rank-mode <id>] [--periods-per-year N] [--execution-seeds-per-window N] [--pass-mode <mode>] [--benchmark-agent <id>] [--diagnostics <list>] [--json]");
                ExitCode::from(2)
            }
        },
        Some("commit") => run_commit(&args),
        Some("stress") => run_stress(json),
        Some("audit") => run_audit(json),
        Some("realism") => run_realism(&args, json),
        Some("sign") => run_sign(&args, json),
        Some("verify") => run_verify(&args, json),
        Some("capture") => run_capture(&args, json),
        Some("verify-trajectory") => run_verify_trajectory(&args, json),
        Some("rescore") => ExitCode::from(rescore_cmd::run(&args, json).clamp(0, 255) as u8),
        Some("audit-briefing") => run_audit_briefing(&args, json),
        Some("canary") => run_canary(&args, json),
        Some("sandbox-check") => run_sandbox_check(&args, json),
        Some("score-allocation") => run_score_allocation(&args, json),
        Some("greeks") => run_greeks(&args, json),
        Some("check") => run_check(&args, json),
        Some("regime") => run_regime(&args, json),
        Some("lineage") => ExitCode::from(lineage_cmd::run(&args, json).clamp(0, 255) as u8),
        Some("arena") => ExitCode::from(arena_cmd::run(&args, json).clamp(0, 255) as u8),
        Some("forecast-quality") => {
            ExitCode::from(forecast_cmd::run(&args, json).clamp(0, 255) as u8)
        }
        Some("import") => ExitCode::from(import_cmd::run(&args, json).clamp(0, 255) as u8),
        Some("gateway") => ExitCode::from(gateway_cli::run(&args, json).clamp(0, 255) as u8),
        Some(sub @ ("select" | "disqualify" | "rediscover" | "uncertainty" | "decay-prior")) => {
            ExitCode::from(analysis_cmd::run(sub, &args, json).clamp(0, 255) as u8)
        }
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

fn run_audit_briefing(args: &[String], json: bool) -> ExitCode {
    let Some(path) = args.get(2) else {
        eprintln!("usage: sharpebench audit-briefing <briefing.json> [--json]");
        return ExitCode::from(2);
    };
    let data = match std::fs::read_to_string(path) {
        Ok(d) => d,
        Err(e) => {
            eprintln!("error: cannot read {path}: {e}");
            return ExitCode::FAILURE;
        }
    };
    let briefing: sharpebench_core::Briefing = match serde_json::from_str(&data) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("error: invalid briefing JSON: {e}");
            return ExitCode::FAILURE;
        }
    };
    let audit =
        sharpebench_core::audit_briefing(&briefing, &sharpebench_core::BriefingPolicy::default());
    if json {
        emit_json(&audit);
    } else if audit.balanced {
        println!("BALANCED — no input-side salience bias detected");
    } else {
        println!("BIASED — {} violation(s):", audit.violations.len());
        for v in &audit.violations {
            println!("  - {v:?}");
        }
    }
    if audit.balanced {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

fn run_canary(args: &[String], json: bool) -> ExitCode {
    let Some(seed) = args.get(2) else {
        eprintln!("usage: sharpebench canary <seed> [--json]");
        return ExitCode::from(2);
    };
    let canary = sharpebench_attest::make_canary(seed.as_bytes());
    if json {
        emit_json(&canary);
    } else {
        println!("canary id:    {}", canary.id);
        println!("canary token: {}", canary.token);
        println!("\nEmbed the marker in the scenario artifact; if a model ever emits the token, the held-out set leaked into its training corpus.");
    }
    ExitCode::SUCCESS
}

fn run_sandbox_check(args: &[String], json: bool) -> ExitCode {
    let Some(image) = args.get(2).filter(|value| !value.starts_with('-')) else {
        eprintln!("usage: sharpebench sandbox-check <fixture@sha256:digest> [--json]");
        return ExitCode::from(2);
    };
    match sharpebench_arena::check_sandbox_readiness(image) {
        Ok(report) => {
            if json {
                emit_json(&report);
            } else {
                println!("FIELD-READY — live hostile sandbox checks passed");
                println!("image:  {}", report.image);
                println!("id:     {}", report.image_id);
                println!("docker: {}", report.docker_server_version);
                for check in report.passed_checks {
                    println!("  ok  {check}");
                }
            }
            ExitCode::SUCCESS
        }
        Err(error) => {
            if json {
                emit_json(&serde_json::json!({
                    "field_ready": false,
                    "error": error.to_string(),
                }));
            } else {
                eprintln!("NOT FIELD-READY — {error}");
            }
            ExitCode::FAILURE
        }
    }
}

fn run_score_allocation(args: &[String], json: bool) -> ExitCode {
    let Some(path) = args.get(2) else {
        eprintln!("usage: sharpebench score-allocation <allocation.json> [--json]");
        return ExitCode::from(2);
    };
    let data = match std::fs::read_to_string(path) {
        Ok(d) => d,
        Err(e) => {
            eprintln!("error: cannot read {path}: {e}");
            return ExitCode::FAILURE;
        }
    };
    let traj: sharpebench_core::AllocationTrajectory = match serde_json::from_str(&data) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("error: invalid allocation JSON: {e}");
            return ExitCode::FAILURE;
        }
    };
    let report =
        sharpebench_core::score_allocation(&traj, &sharpebench_core::AllocationPolicy::default());
    if json {
        emit_json(&report);
    } else {
        println!(
            "allocation: valid={} total_turnover={:.4} mean_turnover={:.4}",
            report.valid, report.total_turnover, report.mean_turnover
        );
        for v in &report.weight_violations {
            println!("  - {v:?}");
        }
    }
    if report.valid {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

fn run_greeks(args: &[String], json: bool) -> ExitCode {
    if args.len() < 8 {
        eprintln!(
            "usage: sharpebench greeks <spot> <strike> <t_years> <rate> <vol> <call|put> [--json]"
        );
        return ExitCode::from(2);
    }
    let nums: Result<Vec<f64>, _> = args[2..7].iter().map(|s| s.parse::<f64>()).collect();
    let Ok(n) = nums else {
        eprintln!("error: spot/strike/t/rate/vol must be numbers");
        return ExitCode::from(2);
    };
    let is_call = match args[7].as_str() {
        "call" => true,
        "put" => false,
        other => {
            eprintln!("error: expected call|put, got {other}");
            return ExitCode::from(2);
        }
    };
    let (spot, strike, t, r, vol) = (n[0], n[1], n[2], n[3], n[4]);
    let quote = (|| {
        let price = sharpebench_core::bs_price(spot, strike, t, r, vol, is_call)?;
        let greeks = sharpebench_core::bs_greeks(spot, strike, t, r, vol, is_call)?;
        let risk = sharpebench_core::classify_greeks_risk(
            &greeks,
            &sharpebench_core::GreeksPolicy::default(),
        )?;
        Ok::<_, sharpebench_core::OptionsError>((price, greeks, risk))
    })();
    let (price, greeks, risk) = match quote {
        Ok(quote) => quote,
        Err(error) => {
            eprintln!("error: {error}");
            return ExitCode::from(2);
        }
    };
    if json {
        emit_json(&serde_json::json!({ "price": price, "greeks": greeks, "risk": risk }));
    } else {
        println!("price {price:.4}");
        println!(
            "delta {:.4}  gamma {:.4}  theta {:.4}  vega {:.4}  rho {:.4}",
            greeks.delta, greeks.gamma, greeks.theta, greeks.vega, greeks.rho
        );
        println!(
            "local exposure: net_short_gamma={} short_vega={}",
            risk.net_short_gamma, risk.short_vega
        );
    }
    ExitCode::SUCCESS
}

/// `check` — backtest-honesty verdict over a column of per-period returns.
/// `--trials N` is REQUIRED: a single backtest you kept is the survivor of every
/// variant you discarded, so there is no honest default for the search footprint.
/// `--periods-per-year N` says what a row is; without it the verdict assumes
/// daily bars and its explanation says so.
fn run_check(args: &[String], json: bool) -> ExitCode {
    use sharpebench_edge::{is_my_sharpe_real, HonestyConfig, Verdict};

    let Some(path) = args.get(2).filter(|p| !p.starts_with('-')) else {
        eprintln!("usage: sharpebench check <returns.csv> --trials N [--periods-per-year N] [--col NAME] [--confidence C] [--json]");
        return ExitCode::from(2);
    };
    let Some(trials_str) = flag_value(args, "--trials") else {
        eprintln!("error: --trials N is required (the number of strategies/configs tried before keeping this one). n_trials=1 is usually a lie.");
        return ExitCode::from(2);
    };
    let Ok(n_trials) = trials_str.parse::<u32>() else {
        eprintln!("error: --trials must be a positive integer, got `{trials_str}`");
        return ExitCode::from(2);
    };
    let confidence = match flag_value(args, "--confidence") {
        Some(c) => match c.parse::<f64>() {
            Ok(v) if (0.0..1.0).contains(&v) => v,
            _ => {
                eprintln!("error: --confidence must be in (0, 1), got `{c}`");
                return ExitCode::from(2);
            }
        },
        None => 0.95,
    };
    let periods_per_year = match positive_f64_flag(args, "--periods-per-year") {
        Ok(p) => p,
        Err(e) => {
            eprintln!("error: {e}");
            return ExitCode::from(2);
        }
    };
    let col = flag_value(args, "--col");

    let text = match std::fs::read_to_string(path) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("error: cannot read {path}: {e}");
            return ExitCode::FAILURE;
        }
    };
    let returns = match read_returns_column(&text, col) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("error: {e}");
            return ExitCode::FAILURE;
        }
    };
    if returns.len() < 2 {
        eprintln!("error: need at least 2 returns, got {}", returns.len());
        return ExitCode::FAILURE;
    }

    let cfg = HonestyConfig {
        n_trials,
        periods_per_year,
        confidence,
        ..HonestyConfig::default()
    };
    let v = is_my_sharpe_real(&returns, &cfg);

    if json {
        emit_json(&v);
    } else {
        let tag = match v.verdict {
            Verdict::Pass => "PASS",
            Verdict::Borderline => "BORDERLINE",
            Verdict::Fail => "FAIL",
        };
        println!("Sharpe    : {:.4} ({} obs)", v.sharpe, v.n_obs);
        println!(
            "Deflated  : {:.4}  (n_trials={})",
            v.deflated_sharpe, v.n_trials
        );
        println!("Haircut   : {:.4}", v.haircut);
        let mintrl = if v.min_track_record_len.is_finite() {
            format!("{:.0} periods", v.min_track_record_len)
        } else {
            "unreachable (Sharpe ≤ benchmark)".to_string()
        };
        println!("MinTRL    : {mintrl}");
        println!("Verdict   : {tag}");
        println!("\n{}", v.explanation);
    }

    match v.verdict {
        Verdict::Pass => ExitCode::SUCCESS,
        _ => ExitCode::FAILURE,
    }
}

/// `sharpebench regime <a.csv> <b.csv> <regimes.csv>`: compare two strategies'
/// per-period returns inside each market regime. The three files are aligned by
/// row unless `--period-col` requires identical ordered period identities.
/// The regimes file carries one label per period (string column, header
/// optional). Labels are an input: nothing here infers a regime.
fn run_regime(args: &[String], json: bool) -> ExitCode {
    let (Some(path_a), Some(path_b), Some(path_r)) = (args.get(2), args.get(3), args.get(4)) else {
        eprintln!(
            "usage: sharpebench regime <returns_a.csv> <returns_b.csv> <regimes.csv> [--col NAME] [--regime-col NAME] [--period-col NAME] [--json]"
        );
        return ExitCode::from(2);
    };
    for name in ["--col", "--regime-col", "--period-col"] {
        if args.iter().any(|arg| arg == name)
            && flag_value(args, name).is_none_or(|value| value.is_empty() || value.starts_with('-'))
        {
            eprintln!("error: {name} requires a column name");
            return ExitCode::from(2);
        }
    }
    let col = flag_value(args, "--col");

    let read = |path: &str| -> Result<String, ExitCode> {
        std::fs::read_to_string(path).map_err(|e| {
            eprintln!("error: cannot read {path}: {e}");
            ExitCode::FAILURE
        })
    };
    let (text_a, text_b, text_r) = match (read(path_a), read(path_b), read(path_r)) {
        (Ok(a), Ok(b), Ok(r)) => (a, b, r),
        (Err(c), _, _) | (_, Err(c), _) | (_, _, Err(c)) => return c,
    };
    let csv_columns::RegimeInputs { a, b, labels } = match csv_columns::read_regime_inputs(
        &text_a,
        &text_b,
        &text_r,
        col,
        flag_value(args, "--regime-col"),
        flag_value(args, "--period-col"),
    ) {
        Ok(inputs) => inputs,
        Err(error) => {
            eprintln!("error: {error}");
            return ExitCode::FAILURE;
        }
    };

    let regimes: Vec<&str> = labels.iter().map(String::as_str).collect();
    let report = sharpebench_core::compare_by_regime(
        &a,
        &b,
        &regimes,
        sharpebench_core::RegimeCompareOpts::default(),
    );

    if json {
        emit_json(&report);
    } else {
        println!(
            "Pooled mean gap (A-B): {:+.6}  sign={:+}",
            report.pooled_mean_gap, report.pooled_edge_sign
        );
        println!(
            "{:<14} {:>5} {:>9} {:>9} {:>10} {:>10} {:>7} {:>5} counted",
            "regime", "n", "nearzr_a", "nearzr_b", "mean_gap", "cont_gap", "ks", "edge"
        );
        for r in &report.regimes {
            println!(
                "{:<14} {:>5} {:>9.3} {:>9.3} {:>+10.6} {:>+10.6} {:>7.3} {:>+5} {}",
                r.regime,
                r.n_periods,
                r.a.near_zero_return_mass,
                r.b.near_zero_return_mass,
                r.mean_gap,
                r.cont_mean_gap,
                r.ks_statistic,
                r.edge_sign,
                if r.counted { "yes" } else { "no" }
            );
        }
        println!(
            "Edge dispersion across counted regimes: {:.6}",
            report.edge_dispersion
        );
        if report.pooled_hides_reversal {
            let what = if report.pooled_edge_sign == 0 {
                "the pooled gap is a tie and counted regimes disagree in"
            } else {
                "the pooled sign is contradicted in"
            };
            println!("REVERSAL: {what} {}", report.reversal_regimes.join(", "));
        } else {
            println!("No sign reversal among counted regimes.");
        }
    }
    ExitCode::SUCCESS
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

/// Apply the reliability-verdict flags to a config. `--pass-mode` is `all` (the
/// default), `any`, `at-least:N`, or `relative-to-benchmark`, under which each
/// run is tested on its excess return over `--benchmark-agent <id>` (default
/// `buy-and-hold`, the reference agent every `run` field carries) in the same
/// window and seed. Absent flags leave the config untouched, so the default
/// verdict is byte-identical to before the flags existed.
fn apply_pass_mode_flags(args: &[String], cfg: &mut ScoreConfig) -> Result<(), String> {
    use sharpebench_core::PassMode;
    if let Some(raw) = flag_value(args, "--pass-mode") {
        cfg.pass_mode = match raw {
            "all" => PassMode::All,
            "any" => PassMode::Any,
            "relative-to-benchmark" => PassMode::RelativeToBenchmark,
            other => match other
                .strip_prefix("at-least:")
                .and_then(|n| n.parse::<usize>().ok())
            {
                Some(n) => PassMode::AtLeast(n),
                None => {
                    return Err(format!(
                        "--pass-mode must be all | any | at-least:N | relative-to-benchmark, got `{raw}`"
                    ))
                }
            },
        };
    }
    if let Some(id) = flag_value(args, "--benchmark-agent") {
        cfg.benchmark_agent_id = id.to_string();
    }
    Ok(())
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
        "  sharpebench run [--data <csv>] [--http <addr>|--image <ref>|--cmd \"<prog>\"]  run agents and rank"
    );
    println!("                       --data: a frozen CSV (else synthetic) · --http/--image/--cmd: add YOUR agent");
    println!("                       --image <repository@sha256:...>: run the agent in the hardened container sandbox (no daemon = refusal)");
    println!("                       --cmd: UNSANDBOXED host execution, for agents you trust; prints a warning on every run");
    println!("                       --scan-policy <json>: opt-in preflight of the pinned image's configuration and filesystem before it launches (--image only)");
    println!("                       --checkpoint <path>: resumable external-agent sweep (crash-tolerant)");
    println!("                       --retry-runtime-failures: recover exhausted checkpoint cells (3 additional rounds maximum)");
    println!("                       --entrant-sha256 <digest>: exact entrant identity; required with --checkpoint plus --http or --cmd");
    println!("                       --rate-card <json>: frozen token rates; emits a separate self-reported estimate, never a rank input");
    println!("                       --fault-plan <json>: seeded fault injection at the entrant boundary; checkpoint-bound, rank-neutral evidence");
    println!("                       --retry-backoff <ms,ms,...>: wait before each runtime retry (entry i precedes retry i); checkpoint-bound");
    println!("                       --periods-per-year N: bars per year of the dataset (default 252; 1h crypto 8760, 4h 2190, 1d crypto 365, 1w 52)");
    println!("                       --pass-mode all|any|at-least:N|relative-to-benchmark: reliability verdict (default all)");
    println!("                       --benchmark-agent <id>: benchmark for relative-to-benchmark (default buy-and-hold)");
    println!("                       --suite-evidence: with --json, wrap the board as {{board, suite_evidence}} and");
    println!("                         carry the trial census and the control verdicts; the plain table always prints them");
    println!(
        "  sharpebench score <submissions.json>  rank a JSON field of pre-computed submissions"
    );
    println!("                       --pass-mode / --benchmark-agent: as for run");
    println!(
        "                       --require-run-keys: every submission declares one `run_keys` entry"
    );
    println!(
        "                         per run; refuse an unkeyed, partial or duplicated cell grid"
    );
    println!("                         instead of aligning runs across agents by position");
    println!(
        "                       --rank-mode <id>: opt into a versioned rank mode (lifecycle-certified/v1);"
    );
    println!(
        "                         adds a certification verdict per row, never changes the host rank"
    );
    println!(
        "                       --diagnostics <list>: also report opt-in Sharpe diagnostics the gate"
    );
    println!(
        "                         does not use: autocorrelated-psr,null-se-psr,mppm (comma-separated)"
    );
    println!(
        "  sharpebench commit <agent> <window> <digest> <salt> [--fault-plan <plan.json>]  forward-attestation pre-registration"
    );
    println!("  sharpebench stress                    run the adversarial stress suite (masked)");
    println!("  sharpebench audit                     self-audit: prove the scorer resists gaming");
    println!("  sharpebench realism [--data <csv>]    prove a dataset behaves like a market (Cont's stylized facts)");
    println!(
        "  sharpebench sign <subs.json> <key> <out.json> [--ed25519 <secret>]  score + sign a board to a file"
    );
    println!("                       --ed25519: also embed a publicly verifiable Ed25519 chain + its verifying key");
    println!("  sharpebench verify <board.json> <key>  verify a signed board's HMAC chain (needs the secret)");
    println!("  sharpebench verify <board.json> --pubkey <hex>  verify the Ed25519 chain with the public key only");
    println!("  sharpebench verify <board.json> --public  verify the Ed25519 chain with the key embedded in the board");
    println!(
        "  sharpebench capture <agent> <out.json> [--data <csv>]  capture an agent's raw-decision trajectory artifact"
    );
    println!("  sharpebench capture <out.json> --cmd \"<prog>\"|--http <addr>|--image <ref> [--data <csv>]  capture an external entrant's trajectory");
    println!(
        "  sharpebench verify-trajectory <traj.json> [--data <csv>]  strictly replay the complete data/cost/engine/runner/window/seed contract"
    );
    println!("                       --allow-unbound-trajectory: explicit legacy or cross-version regrade; never the default");
    println!("                       --reexecute [--cmd \"<prog>\"|--http <addr>|--image <ref>]: also re-run every captured run with a fresh agent and refuse the first divergent decision");
    println!(
        "  sharpebench rescore <bundle.json>     recompute a declared submission bundle's quality from its frozen, digest-bound files only"
    );
    println!("                       --envelope <envelope.json>: judge the declared compute budget against the field's; a budget difference refuses, environment metadata is disclosed");
    println!("                       --reexecute [--scan-policy <policy.json> [--runtime-allowlist <list.json>]]: also re-run every captured run from the bundle's own pinned image in a network-disabled container");
    println!("  sharpebench audit-briefing <briefing.json>  audit a shared briefing for input-side salience bias");
    println!("  sharpebench canary <seed>             derive a do-not-train contamination tripwire token");
    println!("  sharpebench sandbox-check <image@sha256:digest>  run live hostile field-readiness checks (never skips)");
    println!(
        "  sharpebench score-allocation <alloc.json>  score a weight-vector trajectory (validity + turnover)"
    );
    println!(
        "  sharpebench greeks <spot> <strike> <t> <r> <vol> <call|put>  Black-Scholes price + Greeks + local exposure"
    );
    println!(
        "  sharpebench check <returns.csv> --trials N [--periods-per-year N] [--col NAME] [--confidence C]  is this Sharpe real? (deflated/MinTRL; default 252 periods/year)"
    );
    println!(
        "  sharpebench regime <a.csv> <b.csv> <regimes.csv> [--col NAME]  compare two return series within each regime (labels are an input)"
    );
    println!(
        "  sharpebench lineage <strategy-evidence.json>                   verify Arena candidate ancestry, sources, and within-family robustness"
    );
    println!(
        "  sharpebench arena <init|open|commit|advance|score|publish|verify> ...  drive a forward-attested scoring window (see docs/book/src/arena.md)"
    );
    println!(
        "  sharpebench forecast-quality <evidence.json>...              score prospective forecasts on exact common support (reported only)"
    );
    println!(
        "  sharpebench import <csv|stockbench> ... --out subs.json     convert a rival board's return series into a scoreable field"
    );
    println!(
        "  sharpebench select <candidates.csv...> [--alpha A]          pick a candidate on a bootstrap percentile instead of the observed best"
    );
    println!(
        "  sharpebench disqualify <subs.json>                          classify each agent's hard gates and advisory flags"
    );
    println!(
        "  sharpebench rediscover <submitted.csv> <known.csv...>       flag a submission cosine-near a known strategy"
    );
    println!(
        "  sharpebench uncertainty <returns.csv> [--confidences ...]   split uncertainty into aleatoric, epistemic and distributional legs"
    );
    println!(
        "  sharpebench decay-prior --measured-ic <csv> --adoption X --theta Y --delta-max Z  compare measured edge decay to the crowding prior (reported, never gating)"
    );
    println!("  sharpebench self-update               update the binary in place (--features self-update builds)");
    println!("\n<key>, <secret> and --pubkey accept a literal, or env:NAME / file:PATH to keep secrets out of process listings.");
    println!("HMAC <key> holders can both verify and forge; an Ed25519 verifying key can only verify, so publish it.");
    println!("\nGlobal flags:");
    println!("  --json   emit machine-readable JSON instead of a human table (for agents / CI)");
}

/// JSON key under which a board carries its Ed25519 chain. Sits beside the
/// HMAC `chain` so `sharpebench_leaderboard::load` (which ignores unknown
/// fields) still reads the board exactly as before.
const PUBLIC_CHAIN_FIELD: &str = "public_chain";

fn run_sign(args: &[String], json: bool) -> ExitCode {
    if args.len() < 5 {
        eprintln!(
            "usage: sharpebench sign <submissions.json> <key> <out.json> [--ed25519 <secret>] [--json]"
        );
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

    // Without --ed25519 the output carries the HMAC chain exactly as before, plus
    // the terminal receipt that anchors its length. That receipt is new, so the
    // bytes are no longer identical to the pre-Ed25519 CLI; a reader that ignores
    // unknown fields is unaffected.
    let Some(secret_spec) = flag_value(args, "--ed25519") else {
        return match sharpebench_leaderboard::save(&pb, &args[4]) {
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
        };
    };

    let signing_key = match resolve_key(secret_spec) {
        Ok(s) => sharpebench_attest::SigningKey::derive(&s),
        Err(e) => {
            eprintln!("error: {e}");
            return ExitCode::FAILURE;
        }
    };
    // Sign exactly the payloads the HMAC chain signed, so the two chains are
    // link-for-link comparable and a reader can cross-check one against the other.
    let payloads: Vec<String> = pb.chain.iter().map(|r| r.payload.clone()).collect();
    let public = sharpebench_attest::publish_public_chain(&payloads, &signing_key);
    let verifying_key = public.verifying_key.clone();
    let mut doc = serde_json::to_value(&pb).unwrap_or_default();
    doc[PUBLIC_CHAIN_FIELD] = serde_json::to_value(&public).unwrap_or_default();
    let written = serde_json::to_string_pretty(&doc)
        .map_err(std::io::Error::other)
        .and_then(|s| std::fs::write(&args[4], s));
    match written {
        Ok(()) => {
            if json {
                emit_json(&serde_json::json!({
                    "signed": true,
                    "entries": pb.chain.len(),
                    "path": args[4],
                    "scheme": sharpebench_attest::ED25519_SCHEME,
                    "verifying_key": verifying_key,
                }));
            } else {
                println!(
                    "signed board ({} entries, HMAC + Ed25519) -> {}",
                    pb.chain.len(),
                    args[4]
                );
                println!("verifying key (publish this): {verifying_key}");
            }
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}

/// `verify --pubkey <spec>` / `verify --public`: check the Ed25519 chain with
/// no secret. `--pubkey` pins the key (the embedded one must match, so a board
/// re-signed under a swapped key is rejected); `--public` trusts the embedded
/// key and only proves the document is self-consistent.
fn run_verify_public(path: &str, pubkey_spec: Option<&str>, json: bool) -> ExitCode {
    let doc: serde_json::Value = match std::fs::read_to_string(path)
        .map_err(std::io::Error::other)
        .and_then(|s| serde_json::from_str(&s).map_err(std::io::Error::other))
    {
        Ok(v) => v,
        Err(e) => {
            eprintln!("error: cannot load {path}: {e}");
            return ExitCode::FAILURE;
        }
    };
    let public: sharpebench_attest::PublicChain =
        match serde_json::from_value(doc[PUBLIC_CHAIN_FIELD].clone()) {
            Ok(p) => p,
            Err(_) => {
                eprintln!(
                    "error: {path} carries no `{PUBLIC_CHAIN_FIELD}` (sign it with --ed25519 first)"
                );
                return ExitCode::FAILURE;
            }
        };
    let pinned = match pubkey_spec {
        Some(spec) => match resolve_key(spec) {
            Ok(bytes) => {
                let hex = String::from_utf8_lossy(&bytes).trim().to_string();
                match sharpebench_attest::VerifyingKey::from_hex(&hex) {
                    Some(vk) => Some(vk),
                    None => {
                        eprintln!("error: --pubkey is not a valid 64-hex-char Ed25519 key");
                        return ExitCode::FAILURE;
                    }
                }
            }
            Err(e) => {
                eprintln!("error: {e}");
                return ExitCode::FAILURE;
            }
        },
        None => None,
    };
    let ok = match &pinned {
        Some(vk) => sharpebench_attest::verify_public_chain_with(&public, vk),
        None => sharpebench_attest::verify_public_chain(&public),
    };
    if json {
        emit_json(&serde_json::json!({
            "ok": ok,
            "entries": public.chain.len(),
            "scheme": public.scheme,
            "verifying_key": public.verifying_key,
            "pinned": pinned.is_some(),
        }));
    } else if ok {
        println!(
            "OK — {} entries, Ed25519 chain valid under {} key {}",
            public.chain.len(),
            if pinned.is_some() {
                "pinned"
            } else {
                "embedded"
            },
            public.verifying_key
        );
    } else {
        eprintln!("FAIL — Ed25519 chain invalid (tampered, or the key does not match)");
    }
    if ok {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

fn run_verify(args: &[String], json: bool) -> ExitCode {
    let public_mode =
        args.iter().any(|a| a == "--public") || flag_value(args, "--pubkey").is_some();
    if args.len() < 4 || (!public_mode && args[3].starts_with("--")) {
        eprintln!(
            "usage: sharpebench verify <board.json> <key> | --pubkey <hex> | --public [--json]"
        );
        return ExitCode::from(2);
    }
    if public_mode {
        return run_verify_public(&args[2], flag_value(args, "--pubkey"), json);
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
    // The whole board, not just its chain. `verify_board` recomputes the chain and
    // never reads `pb.scores`, so a board whose displayed scores were rewritten,
    // reordered or truncated printed the same OK as an honest one, which is the
    // opposite of what an operator runs this command to learn.
    let ok = sharpebench_leaderboard::verify_published(&pb, &key);
    if json {
        emit_json(&serde_json::json!({
            "ok": ok,
            "entries": pb.chain.len(),
            "scores": pb.scores.len(),
            "anchored": pb.receipt.is_some(),
        }));
    } else if ok {
        println!(
            "OK - {} entries, {} displayed scores bound to the anchored chain",
            pb.chain.len(),
            pb.scores.len()
        );
    } else if pb.receipt.is_none() {
        eprintln!(
            "FAIL - the document carries no terminal receipt, so its length is unanchored and              records may have been removed from the end"
        );
    } else {
        eprintln!("FAIL - chain, displayed scores or terminal anchor did not verify");
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
                if c.defended {
                    "DEFENDED "
                } else if c.expected_vulnerable {
                    "KNOWN GAP"
                } else {
                    "  GAMED  "
                },
                c.name,
                c.detail
            );
        }
        if report.all_defended {
            let defended = report.cases.len() - report.known_gaps;
            if report.known_gaps > 0 {
                println!(
                    "\n{defended} attacks demoted; {} known gap(s) documented, not defended. The benchmark holds where it claims to.",
                    report.known_gaps
                );
            } else {
                println!("\nAll {defended} attacks demoted. The benchmark holds.");
            }
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

/// `realism` — certify that a dataset exhibits Cont's stylized facts of asset
/// returns (fat tails, volatility clustering, aggregational Gaussianity, and
/// time-reversal/Zumbach asymmetry). Runs on a frozen `--data <csv>` (the intended
/// use: prove the benchmark's scoring data behaves like a market) or the synthetic
/// generator by default — so a generator that drifts into a Gaussian toy fails the
/// proof instead of silently invalidating every score computed on it.
fn run_realism(args: &[String], json: bool) -> ExitCode {
    use sharpebench_sim::Dataset;

    let (data, src) = match flag_value(args, "--data") {
        Some(path) => match Dataset::from_csv_file(path) {
            Ok(d) => (d, path.to_string()),
            Err(e) => {
                eprintln!("error: {e}");
                return ExitCode::FAILURE;
            }
        },
        None => (
            Dataset::synthetic(8, 180, 20_260_621),
            "synthetic".to_string(),
        ),
    };

    // Pool every symbol's simple per-bar returns (BTreeMap iteration is ordered, so
    // the pooled stream is deterministic).
    let mut returns: Vec<f64> = Vec::new();
    for series in data.closes.values() {
        for w in series.windows(2) {
            if w[0] != 0.0 {
                returns.push(w[1] / w[0] - 1.0);
            }
        }
    }
    if returns.len() < 40 {
        eprintln!(
            "error: not enough returns to assess realism ({} < 40)",
            returns.len()
        );
        return ExitCode::FAILURE;
    }

    let v = sharpebench_core::validate_dataset(&returns);
    let r = &v.report;
    if json {
        emit_json(&serde_json::json!({
            "source": src,
            "n_returns": returns.len(),
            "realistic": v.realistic,
            "failures": v.failures.iter().map(|f| format!("{f:?}")).collect::<Vec<_>>(),
            "report": {
                "excess_kurtosis": r.excess_kurtosis,
                "abs_return_autocorr": r.abs_return_autocorr,
                "vol_clustering_acf": r.vol_clustering_acf,
                "gain_loss_skew": r.gain_loss_skew,
                "aggregational_gaussianity": r.aggregational_gaussianity,
                "zumbach_asymmetry": r.zumbach_asymmetry,
            },
        }));
    } else {
        println!(
            "SharpeBench — dataset realism proof ({src}, {} returns)\n",
            returns.len()
        );
        println!(
            "  excess kurtosis (fat tails)      : {:+.3}",
            r.excess_kurtosis
        );
        println!(
            "  |return| autocorr (clustering)   : {:+.3}",
            r.abs_return_autocorr
        );
        println!(
            "  squared-return ACF               : {:+.3}",
            r.vol_clustering_acf
        );
        println!(
            "  skew (gain/loss asymmetry)       : {:+.3}",
            r.gain_loss_skew
        );
        println!(
            "  kurtosis drop under aggregation  : {:+.3}",
            r.aggregational_gaussianity
        );
        println!(
            "  Zumbach time-reversal asymmetry  : {:+.4}",
            r.zumbach_asymmetry
        );
        if v.realistic {
            println!("\nREALISTIC — the dataset exhibits every gated stylized fact.");
        } else {
            println!("\nUNREALISTIC — missing: {:?}", v.failures);
        }
    }

    if v.realistic {
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
        let board = rank(
            &[bh, mo],
            &ScoreConfig {
                execution_seeds_per_window: seeds.len(),
                ..ScoreConfig::default()
            },
        );
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
        eprintln!(
            "usage: sharpebench commit <agent_id> <target_window> <artifact_digest> <salt> [--fault-plan <plan.json>]"
        );
        return ExitCode::from(2);
    }
    // A faulted arena window refuses at reveal any commitment that does not bind
    // its fault plan, so the plan is validated here exactly as `run` validates it.
    let fault_plan_sha256 = match arena_cmd::fault_plan_digest(args) {
        Ok(digest) => digest,
        Err(e) => {
            eprintln!("error: {e}");
            return ExitCode::from(2);
        }
    };
    let c = sharpebench_attest::make_commitment_under_fault_plan(
        &args[2],
        &args[3],
        &args[4],
        &args[5],
        fault_plan_sha256.as_deref(),
    );
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

/// Surface external-agent transport failures instead of hiding them: an unrecovered
/// wire blip (runtime) or an agent protocol fault is printed to stderr so the
/// operator sees that some decisions did not come from the agent honestly, rather
/// than a silently-flattened return series.
#[derive(Debug, Serialize)]
struct ExternalSweepCompleteness {
    expected_cells: usize,
    completed_cells: usize,
    runtime_failed_cells: usize,
    agent_failed_cells: usize,
    complete: bool,
}

fn external_sweep_completeness(
    failures: &sharpebench_harness::FailureLog,
    expected_cells: usize,
    completed_cells: usize,
) -> ExternalSweepCompleteness {
    let runtime_failed_cells = failures.runtime_failures();
    ExternalSweepCompleteness {
        expected_cells,
        completed_cells,
        runtime_failed_cells,
        agent_failed_cells: failures.agent_faults(),
        complete: runtime_failed_cells == 0 && completed_cells == expected_cells,
    }
}

/// `fault_report` is the sweep's `fault_injection` report when `--fault-plan`
/// armed it. An incomplete sweep carries it too, built from the same ledger as
/// a completed row's, so the evidence of the cells that ran is not lost with
/// the board. Without a plan it is `None` and the output is unchanged.
fn report_transport_failures(
    label: &str,
    failures: &sharpebench_harness::FailureLog,
    expected_cells: usize,
    completed_cells: usize,
    accounting: (sharpebench_harness::AttemptSummary, &MonetarySummary),
    fault_report: Option<&serde_json::Value>,
    json: bool,
) -> bool {
    let (attempts, monetary_cost) = accounting;
    let status = external_sweep_completeness(failures, expected_cells, completed_cells);
    if !status.complete {
        if json {
            let mut refusal = serde_json::json!({
                "ok": false,
                "error": "incomplete_external_sweep",
                "agent": label,
                "completeness": status,
                "attempt_accounting": attempt_accounting_with_cost(attempts, monetary_cost),
            });
            if let Some(report) = fault_report {
                refusal["fault_injection"] = report.clone();
            }
            emit_json(&refusal);
        } else {
            eprintln!(
                "error: external sweep for {label} is incomplete: expected {} cells, completed {}, runtime failures {}. No score or board was emitted",
                status.expected_cells,
                status.completed_cells,
                status.runtime_failed_cells,
            );
        }
        if !json {
            print_attempt_accounting(label, attempts, monetary_cost);
            if let Some(report) = fault_report {
                print_fault_injection(label, report);
            }
        }
        return false;
    }
    if failures.is_empty() {
        return true;
    }
    if !json {
        eprintln!(
            "note: {} transport failure(s) surfaced for {label} ({} runtime, {} agent-fault); \
             affected runs were not scored as holds",
            failures.records.len(),
            failures.runtime_failures(),
            failures.agent_faults(),
        );
    }
    true
}

/// Operational observations do not enter the scoring kernel. In particular,
/// elapsed host time is not a measurement of provider billing or token usage.
#[cfg(test)]
fn attempt_accounting(attempts: sharpebench_harness::AttemptSummary) -> serde_json::Value {
    attempt_accounting_with_cost(
        attempts,
        &sharpebench_harness::accounting::summarize_usage([]),
    )
}

fn attempt_accounting_with_cost(
    attempts: sharpebench_harness::AttemptSummary,
    monetary_cost: &MonetarySummary,
) -> serde_json::Value {
    serde_json::json!({
        "schema_version": "sharpebench.attempt-accounting.v1",
        "attempts": attempts,
        "monetary_cost": monetary_cost,
        "legacy_cost_columns_unit": "entrant_selected_usd_or_tokens_not_rate_card_priced",
        "rank_neutral": true,
    })
}

fn print_attempt_accounting(
    label: &str,
    attempts: sharpebench_harness::AttemptSummary,
    monetary_cost: &MonetarySummary,
) {
    eprintln!(
        "attempt accounting for {label}: {} attempts, {} completed, {} failed; observed host duration {} ns ({:?})",
        attempts.attempts, attempts.completed, attempts.failed,
        attempts.duration_ns_total, attempts.duration_source,
    );
    eprintln!(
        "rank-neutral token pricing: {}",
        serde_json::to_string(monetary_cost).expect("integer accounting serializes")
    );
}

/// Everything the externally executed row carries beyond its scores.
struct ExternalRowMetadata<'a> {
    agent: &'a str,
    attempts: sharpebench_harness::AttemptSummary,
    monetary_cost: &'a MonetarySummary,
    /// The opt-in image preflight report, when one authorized this launch.
    artifact_preflight: Option<serde_json::Value>,
    /// The fault-injection report, when `--fault-plan` armed the sweep.
    fault_injection: Option<serde_json::Value>,
}

/// Keep the existing JSON board array and scoring fields. Only the externally
/// executed row gains operational metadata; reference rows have no such ledger.
///
/// The board is and stays an ARRAY of rows. Metadata is attached by finding the
/// entrant's row inside it, never by indexing the board itself with a key: that
/// would panic on exactly the successful runs this path exists to produce.
fn run_board_json(
    board: &[CompositeScore],
    external: Option<ExternalRowMetadata<'_>>,
) -> serde_json::Value {
    // The entrant's own operational metadata is attached by name after the
    // seal; the evaluation rows themselves reach the reader only through it.
    let mut value = serde_json::to_value(sharpebench_core::seal_board(board))
        .expect("composite scores serialize");
    if let Some(external) = external {
        for row in value.as_array_mut().expect("a board is an array") {
            if row["agent_id"].as_str() == Some(external.agent) {
                row["attempt_accounting"] =
                    attempt_accounting_with_cost(external.attempts, external.monetary_cost);
                if let Some(preflight) = &external.artifact_preflight {
                    row["artifact_preflight"] = preflight.clone();
                }
                if let Some(report) = &external.fault_injection {
                    row["fault_injection"] = report.clone();
                }
            }
        }
    }
    value
}

fn digest_json<T: Serialize>(label: &str, value: &T) -> Result<String, String> {
    serde_json::to_vec(value)
        .map(|bytes| sharpebench_attest::content_digest(&bytes))
        .map_err(|error| format!("cannot serialize {label} for the checkpoint contract: {error}"))
}

fn current_executable_sha256() -> Result<String, String> {
    let runner = std::env::current_exe()
        .map_err(|error| format!("cannot locate the running benchmark binary: {error}"))?;
    let bytes = std::fs::read(&runner).map_err(|error| {
        format!(
            "cannot hash the running benchmark binary {}: {error}",
            runner.display()
        )
    })?;
    Ok(sharpebench_attest::content_digest(&bytes))
}

/// The invocation identity of a `--cmd` entrant: the command line plus the
/// effective non-secret environment it will be handed.
///
/// The passed-through variable *names* are not the configuration; the agent
/// receives their current values. Binding names alone let one checkpoint span
/// `AGENT_MODE=conservative` and `AGENT_MODE=aggressive`, so completed cells
/// from one policy could be resumed into the other and pooled as one result.
/// Credential values stay out of the identity by design; see
/// [`sharpebench_sim::agent_env_identity`].
fn cmd_entrant_material(cmd: &str) -> String {
    format!(
        "cmd\0{cmd}\0{}",
        sharpebench_sim::effective_agent_env_identity()
    )
}

struct CheckpointExecution<'a> {
    data: &'a sharpebench_sim::Dataset,
    windows: &'a [sharpebench_sim::Window],
    seeds: &'a [u64],
    costs: sharpebench_sim::CostModel,
    score_config: &'a ScoreConfig,
    max_retries: u32,
    rate_card: Option<&'a RateCard>,
    fault_plan: Option<&'a FaultPlan>,
    backoff: &'a BackoffSchedule,
}

fn checkpoint_contract(
    args: &[String],
    execution: CheckpointExecution<'_>,
    entrant_material: &[u8],
    require_explicit_entrant_digest: bool,
) -> Result<sharpebench_harness::SweepContract, String> {
    let entrant_sha256 = match flag_value(args, "--entrant-sha256") {
        Some(digest)
            if digest.len() == 64
                && digest
                    .bytes()
                    .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)) =>
        {
            digest.to_string()
        }
        Some(digest) => {
            return Err(format!(
                "--entrant-sha256 must be 64 lowercase hexadecimal characters, got `{digest}`"
            ))
        }
        None if require_explicit_entrant_digest => {
            return Err(
                "--checkpoint with --http or --cmd also requires --entrant-sha256 <digest>; an endpoint address or command line does not prove which entrant artifact is running"
                    .to_string(),
            )
        }
        None => sharpebench_attest::content_digest(entrant_material),
    };
    Ok(sharpebench_harness::SweepContract::new(
        sharpebench_harness::SweepIdentity {
            dataset_sha256: digest_json("dataset", execution.data)?,
            cost_model_sha256: sharpebench_harness::cost_model_digest(execution.costs),
            score_config_sha256: digest_json("score configuration", execution.score_config)?,
            runner_artifact_sha256: current_executable_sha256()?,
            entrant_sha256,
            // The artifact digest and its invocation answer different questions.
            // A caller-supplied artifact digest must not make a changed command,
            // endpoint, image reference, or environment pass-through list look
            // like the same resumable experiment. A fault plan and a waiting
            // retry schedule change what the sweep observes, so each is folded
            // in; absent, each binding returns the digest unchanged.
            invocation_sha256: execution.backoff.bind_invocation(
                &sharpebench_harness::fault_plan::bind_invocation(
                    &invocation_with_rates(entrant_material, execution.rate_card)?,
                    execution.fault_plan,
                ),
            ),
        },
        execution.windows,
        execution.seeds,
        execution.max_retries,
    ))
}

fn invocation_with_rates(material: &[u8], card: Option<&RateCard>) -> Result<String, String> {
    match card {
        None => Ok(sharpebench_attest::content_digest(material)),
        Some(card) => digest_json(
            "priced invocation",
            &("sharpebench.priced-invocation.v1", material, card.digest()),
        ),
    }
}

fn load_rate_card(args: &[String]) -> Result<Option<RateCard>, String> {
    use std::io::Read;
    if !args.iter().any(|arg| arg == "--rate-card") {
        return Ok(None);
    }
    if !["--cmd", "--image", "--http"]
        .iter()
        .any(|flag| flag_value(args, flag).is_some())
    {
        return Err("--rate-card requires an external-agent transport".into());
    }
    let path = flag_value(args, "--rate-card")
        .filter(|path| !path.starts_with("--"))
        .ok_or("--rate-card requires a JSON file path")?;
    let file =
        std::fs::File::open(path).map_err(|error| format!("cannot open rate card: {error}"))?;
    let mut bytes = Vec::new();
    file.take(sharpebench_harness::accounting::MAX_RATE_CARD_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| format!("cannot read rate card: {error}"))?;
    RateCard::from_json(&bytes).map(Some)
}

fn has_external_transport(args: &[String]) -> bool {
    ["--cmd", "--image", "--http"]
        .iter()
        .any(|flag| flag_value(args, flag).is_some())
}

/// `--fault-plan <json>`: a frozen, validated fault plan for the external
/// entrant, or `None` when the flag is absent. Read once and capped like the
/// rate card; a malformed, unknown-field, out-of-bounds or unarmable plan is
/// refused before anything launches.
fn load_fault_plan(args: &[String]) -> Result<Option<FaultPlan>, String> {
    use std::io::Read;
    if !args.iter().any(|arg| arg == "--fault-plan") {
        return Ok(None);
    }
    if !has_external_transport(args) {
        return Err("--fault-plan requires an external-agent transport".into());
    }
    let path = flag_value(args, "--fault-plan")
        .filter(|path| !path.starts_with("--"))
        .ok_or("--fault-plan requires a JSON file path")?;
    let file =
        std::fs::File::open(path).map_err(|error| format!("cannot open fault plan: {error}"))?;
    let mut bytes = Vec::new();
    file.take(sharpebench_harness::fault_plan::MAX_FAULT_PLAN_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| format!("cannot read fault plan: {error}"))?;
    FaultPlan::from_json(&bytes).map(Some)
}

/// Upper bound on one `--retry-backoff` wait: ten minutes.
const MAX_RETRY_BACKOFF_MS: u64 = 600_000;

/// `--retry-backoff <ms,ms,...>`: the wait before each runtime retry of a cell,
/// in whole milliseconds. Entry `i` precedes retry `i + 1` and the last entry
/// holds. Absent, retries are immediate and nothing about the sweep changes.
/// A list longer than the per-round retry budget is refused: its tail could
/// never be used, yet it would still change the checkpoint identity.
fn load_retry_backoff(args: &[String], max_retries: u32) -> Result<BackoffSchedule, String> {
    if !args.iter().any(|arg| arg == "--retry-backoff") {
        return Ok(BackoffSchedule::immediate());
    }
    if !has_external_transport(args) {
        return Err("--retry-backoff requires an external-agent transport".into());
    }
    let raw = flag_value(args, "--retry-backoff")
        .filter(|raw| !raw.starts_with("--"))
        .ok_or(
            "--retry-backoff requires a comma-separated list of milliseconds, such as 500,2000",
        )?;
    let mut delays = Vec::new();
    for entry in raw.split(',') {
        let millis = Some(entry)
            .filter(|entry| !entry.is_empty() && entry.bytes().all(|b| b.is_ascii_digit()))
            .and_then(|entry| entry.parse::<u64>().ok())
            .filter(|millis| *millis <= MAX_RETRY_BACKOFF_MS)
            .ok_or_else(|| {
                format!(
                    "--retry-backoff entries must be whole milliseconds in 0..={MAX_RETRY_BACKOFF_MS}, got `{entry}`"
                )
            })?;
        delays.push(std::time::Duration::from_millis(millis));
    }
    if delays.len() > max_retries as usize {
        return Err(format!(
            "--retry-backoff lists {} waits, but a cell retries at most {max_retries} times per round",
            delays.len()
        ));
    }
    Ok(BackoffSchedule::from_delays(&delays))
}

/// Rank-neutral record of what a fault plan did to the sweep: the plan
/// identity, the relaxations it declared to the entrant, per-fault
/// denominators over the swept cells, and every attempt's evidence.
fn fault_injection_report(
    plan: &FaultPlan,
    windows: &[sharpebench_sim::Window],
    seeds: &[u64],
    ledger: &sharpebench_harness::AttemptLedger,
) -> serde_json::Value {
    let cells: Vec<sharpebench_harness::fault_plan::CellId> = windows
        .iter()
        .flat_map(|window| {
            seeds
                .iter()
                .map(move |&seed| sharpebench_harness::fault_plan::CellId::new(*window, seed))
        })
        .collect();
    let evidence: Vec<&sharpebench_harness::fault_plan::InjectedFaults> = ledger
        .attempts
        .iter()
        .filter_map(|record| record.injected_faults.as_ref())
        .collect();
    serde_json::json!({
        "schema_version": "sharpebench.fault-injection-report.v1",
        "plan_sha256": plan.digest(),
        "declared_relaxations": plan.declared_relaxations(),
        "entrant_declaration": plan.entrant_declaration(),
        "denominators": plan.denominators_with_evidence(&cells, ledger),
        "evidence": evidence,
        "rank_neutral": true,
    })
}

/// The attempt ledger a faulted checkpoint sweep persisted, read back for the
/// report. Unfaulted sweeps report nothing and never read it.
fn checkpoint_fault_ledger(
    path: &std::path::Path,
    plan: Option<&FaultPlan>,
) -> Result<sharpebench_harness::AttemptLedger, String> {
    match plan {
        None => Ok(sharpebench_harness::AttemptLedger::default()),
        Some(_) => sharpebench_harness::SweepCheckpoint::load(path)
            .map(|checkpoint| checkpoint.attempt_ledger())
            .map_err(|error| format!("cannot read the fault evidence back: {error}")),
    }
}

fn print_fault_injection(label: &str, report: &serde_json::Value) {
    eprintln!(
        "fault injection for {label}: plan {} (rank-neutral)",
        report["plan_sha256"].as_str().unwrap_or_default()
    );
    for row in report["denominators"].as_array().into_iter().flatten() {
        eprintln!(
            "  {}: assigned {} of {} cells, fired in {}",
            row["fault_id"].as_str().unwrap_or_default(),
            row["assigned"],
            row["cells"],
            row["fired"],
        );
    }
}

/// How many luck-floor monkeys a `run` field carries. Named because the roster
/// is declared before the field is assembled and both must read one number.
const LUCK_FLOOR_AGENTS: usize = 3;

/// The suite's protocol and accounting control: a no-op policy over the same
/// cells the field is scored on.
const HOLD_CONTROL_ID: &str = "pipeline-hold";

/// The suite's refusal control: deliberately invalid orders presented to the
/// closed decision contract.
const REFUSAL_CONTROL_ID: &str = "invalid-order-refusal";

/// The entrant id an external transport flag will be ranked under, resolved
/// from the arguments before anything runs.
///
/// The roster is declared before the run, so the id the board will carry has to
/// be known before the run. The three transport branches take their label from
/// here, so a declaration and its board row cannot name one entrant two ways.
/// The order matches the branches: `--http`, then `--image`, then `--cmd`.
fn external_entrant_label(args: &[String]) -> Option<String> {
    if let Some(addr) = flag_value(args, "--http") {
        return Some(format!("http:{addr}"));
    }
    if let Some(image) = flag_value(args, "--image") {
        return Some(format!("sandbox:{image}"));
    }
    flag_value(args, "--cmd")?
        .split_whitespace()
        .next()
        .map(|prog| format!("cmd:{prog}"))
}

/// A no-op policy that counts the decisions the driver asked it for. It takes no
/// position, so anything it moves is apparatus rather than market.
struct CountedHold {
    inner: sharpebench_sim::HoldAgent,
    decisions: usize,
}

impl sharpebench_sim::Agent for CountedHold {
    fn decide(
        &mut self,
        observation: &sharpebench_protocol::MarketObservation,
    ) -> sharpebench_protocol::Decision {
        self.decisions += 1;
        self.inner.decide(observation)
    }
}

/// Run the no-op control over the declared cells: does the protocol round-trip,
/// and does the accounting close?
///
/// `decisions_expected` comes from the declared window geometry and
/// `decisions_round_tripped` from the calls the driver actually made, so the two
/// sides are counted by independent routes and a driver that skipped a bar shows
/// up as a shortfall rather than agreeing with itself.
///
/// The residual is the compounded NAV drift of a policy that never trades,
/// summed over the cells. A book that takes no position pays no fee, no
/// financing and receives no dividend, so every cell must land back on its
/// opening NAV exactly; anything else is cash moving with no order behind it.
fn protocol_and_accounting_control(
    data: &sharpebench_sim::Dataset,
    windows: &[sharpebench_sim::Window],
    seeds: &[u64],
    costs: sharpebench_sim::CostModel,
) -> sharpebench_core::ControlRun {
    let mut decisions_expected = 0usize;
    let mut decisions_round_tripped = 0usize;
    let mut accounting_residual = 0.0_f64;
    for &window in windows {
        // The driver stops at the dataset's end, so the bars it will ask for are
        // the window's own, clipped the same way it clips them.
        let bars = window.end.min(data.len()).saturating_sub(window.start);
        for &seed in seeds {
            decisions_expected += bars;
            let mut agent = CountedHold {
                inner: sharpebench_sim::HoldAgent,
                decisions: 0,
            };
            let run = sharpebench_sim::run_backtest(data, &mut agent, window, seed, costs);
            decisions_round_tripped += agent.decisions;
            accounting_residual += run
                .returns
                .iter()
                .fold(1.0_f64, |nav, ret| nav * (1.0 + ret))
                - 1.0;
        }
    }
    sharpebench_core::ControlRun {
        control_id: HOLD_CONTROL_ID.to_string(),
        property: sharpebench_core::ControlProperty::ProtocolAndAccounting,
        observation: sharpebench_core::ControlObservation::ProtocolAndAccounting {
            decisions_expected,
            decisions_round_tripped,
            accounting_residual,
        },
    }
}

/// Decisions the closed contract must refuse, built against the observation
/// they answer: an order for a symbol the observation does not carry, two orders
/// for one symbol, and a target weight outside the admissible range.
fn invalid_orders(
    observation: &sharpebench_protocol::MarketObservation,
) -> Vec<sharpebench_protocol::Decision> {
    use sharpebench_protocol::{Action, Decision, Order};
    let order = |symbol: &str, target_weight: f64| Order {
        symbol: symbol.to_string(),
        action: Action::Buy,
        target_weight,
        confidence: 0.5,
        rationale: String::new(),
    };
    let decision = |orders: Vec<Order>| Decision {
        orders,
        reasoning: String::new(),
        cost: None,
    };
    let mut out = vec![decision(vec![order("__no_such_symbol__", 0.1)])];
    if let Some(known) = observation.symbols.first() {
        out.push(decision(vec![
            order(&known.symbol, 0.1),
            order(&known.symbol, 0.2),
        ]));
        out.push(decision(vec![order(&known.symbol, 1.01)]));
    }
    out
}

/// Run the refusal control: present deliberately invalid orders to the same
/// closed contract the live transports validate every external decision against,
/// on an observation drawn from the run's own dataset and window.
fn refusal_control(
    data: &sharpebench_sim::Dataset,
    window: sharpebench_sim::Window,
    seed: u64,
    costs: sharpebench_sim::CostModel,
) -> sharpebench_core::ControlRun {
    let mut env = sharpebench_sim::TradingEnv::new(data.clone(), window, costs, seed);
    let observation = env.reset();
    let submitted = invalid_orders(&observation);
    let refusals_observed = submitted
        .iter()
        .filter(|decision| decision.validate_for(&observation).is_err())
        .count();
    sharpebench_core::ControlRun {
        control_id: REFUSAL_CONTROL_ID.to_string(),
        property: sharpebench_core::ControlProperty::RefusalOfInvalidOrder,
        observation: sharpebench_core::ControlObservation::RefusalOfInvalidOrder {
            invalid_orders_submitted: submitted.len(),
            refusals_observed,
        },
    }
}

/// The census and the controls as a table after the board.
///
/// The census rows come from [`sharpebench_core::attach_census`], which walks
/// the declared roster rather than the board, so a declared entrant the board
/// carries no row for still appears with its counts and its gates instead of
/// vanishing from the record.
fn print_suite_evidence(board: &[CompositeScore], evidence: &sharpebench_core::SuiteEvidence) {
    let census = &evidence.trials;
    println!("\nSuite evidence. Not used by the gate, eligibility or the rank.");
    println!(
        "trial census: {} declared entrants x {} windows x {} seeds = {} declared trials; \
         {} completed, {} failed, {} unreported",
        census.cohort.agents.len(),
        census.cohort.windows.len(),
        census.cohort.seeds.len(),
        census.expected,
        census.completed,
        census.failed,
        census.unreported,
    );
    println!(
        "{:<18} {:>9} {:>10} {:>7} {:>11} {:>7}",
        "agent", "expected", "completed", "failed", "unreported", "scored"
    );
    for row in sharpebench_core::attach_census(board, census) {
        println!(
            "{:<18} {:>9} {:>10} {:>7} {:>11} {:>7}",
            truncate(&row.agent_id, 18),
            row.trials.expected,
            row.trials.completed,
            row.trials.failed,
            row.trials.unreported,
            row.score.is_some(),
        );
        for gate in &row.trials.gates {
            println!("  gate: {gate:?}");
        }
    }
    for control in &evidence.controls.controls {
        println!(
            "control [{}] {}",
            if control.held { "held" } else { "withheld" },
            control.detail
        );
    }
}

fn run_demo(args: &[String], json: bool) -> ExitCode {
    use sharpebench_sim::{
        Agent, BuyAndHold, CostModel, Dataset, ExternalAgent, HttpAgent, Momentum, Window,
    };

    let rate_card = match load_rate_card(args) {
        Ok(card) => card,
        Err(error) => {
            eprintln!("error: {error}");
            return ExitCode::from(2);
        }
    };
    let fault_plan = match load_fault_plan(args) {
        Ok(plan) => plan,
        Err(error) => {
            eprintln!("error: {error}");
            return ExitCode::from(2);
        }
    };
    let backoff = match load_retry_backoff(args, EXTERNAL_MAX_RETRIES) {
        Ok(schedule) => schedule,
        Err(error) => {
            eprintln!("error: {error}");
            return ExitCode::from(2);
        }
    };

    // Opt-in artifact preflight, before any dataset, sweep or launch work. It is
    // inert without `--scan-policy`, and with it the arguments and the policy are
    // validated before Docker is invoked at all.
    let mut preflight_row: Option<serde_json::Value> = None;
    // (configuration ID, policy digest) for a launch a preflight authorized.
    let mut scanned_launch: Option<(String, String)> = None;
    match artifact_preflight::preflight_from_args(args, &artifact_preflight::DockerProcess) {
        Err(failure) => {
            if json {
                emit_json(&serde_json::json!({
                    "ok": false,
                    "error": "artifact_preflight_failed",
                    "artifact_preflight_failure": failure,
                }));
            } else {
                eprintln!("error: {failure}");
            }
            return if failure.stage == "arguments" {
                ExitCode::from(2)
            } else {
                ExitCode::FAILURE
            };
        }
        Ok(None) => {}
        Ok(Some(report)) => {
            let value = serde_json::to_value(&report).expect("the preflight report serializes");
            // A refusal is the whole point of the path: no board, no entrant.
            let Some(image_id) = report.authorized_image_id() else {
                if json {
                    emit_json(&serde_json::json!({
                        "ok": false,
                        "error": "artifact_preflight_refused",
                        "artifact_preflight": value,
                    }));
                } else {
                    eprintln!(
                        "error: the image preflight refused `{}`; no entrant was launched and no \
                         board was emitted",
                        report.image_id
                    );
                }
                return ExitCode::FAILURE;
            };
            scanned_launch = Some((image_id.as_str().to_string(), report.policy_sha256.clone()));
            preflight_row = Some(value);
        }
    }

    let resume_policy = if args.iter().any(|arg| arg == "--retry-runtime-failures") {
        if flag_value(args, "--checkpoint").is_none()
            || !["--cmd", "--image", "--http"]
                .iter()
                .any(|flag| flag_value(args, flag).is_some())
        {
            eprintln!("error: --retry-runtime-failures requires --checkpoint and an external-agent transport");
            return ExitCode::from(2);
        }
        sharpebench_harness::ResumePolicy::RetryRuntimeFailures
    } else {
        sharpebench_harness::ResumePolicy::UnfinishedOnly
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
    // `--periods-per-year N` tells the scorer what a bar is, so the annualized
    // thresholds in `ScoreConfig` convert to the dataset's frequency. The shipped
    // datasets: us-indices-1d, fx-majors-1d, commodities-1d, rates-1d 252;
    // crypto-majors-1d 365; crypto-majors-4h 2190; crypto-majors-1h 8760;
    // us-indices-1w and crypto-majors-1w 52. Scoring hourly bars with the daily
    // default makes the deflation bar about six times too demanding, so the value
    // used is printed in the run header rather than left implicit.
    let periods_per_year = match flag_value(args, "--periods-per-year") {
        Some(raw) => match raw.parse::<f64>() {
            Ok(p) if p.is_finite() && p > 0.0 => p,
            _ => {
                eprintln!("error: --periods-per-year must be a positive number, got `{raw}`");
                return ExitCode::from(2);
            }
        },
        None => ScoreConfig::default().periods_per_year,
    };
    let mut cfg = ScoreConfig::for_periods_per_year(periods_per_year);
    if let Err(e) = apply_pass_mode_flags(args, &mut cfg) {
        eprintln!("error: {e}");
        return ExitCode::from(2);
    }

    let seeds: Vec<u64> = (0..8).collect();
    let costs = CostModel::default();
    cfg.execution_seeds_per_window = seeds.len();

    // The roster is declared here, before anything runs, out of this
    // invocation's own configuration: the entrants it will field, the windows
    // the dataset resolver fixed above, and the seeds. Assembling it afterwards
    // out of whichever rows came back is the defect the census exists to
    // prevent, so the declaration cannot be allowed to depend on the run.
    let declared_entrant = external_entrant_label(args);
    let entrant_ids: Vec<String> = declared_entrant
        .iter()
        .cloned()
        .chain(["buy-and-hold".to_string(), "momentum".to_string()])
        .chain((0..LUCK_FLOOR_AGENTS).map(sharpebench_harness::luck_floor_agent_id))
        .collect();
    let window_ids: Vec<String> = windows
        .iter()
        .map(|window| sharpebench_harness::window_label(*window))
        .collect();
    let roster = match sharpebench_core::TrialRoster::declare(&entrant_ids, &window_ids, &seeds) {
        Ok(roster) => roster,
        Err(error) => {
            eprintln!("error: {error}");
            return ExitCode::FAILURE;
        }
    };
    // The closed-loop driver cannot fail a cell: `run_backtest` returns a run for
    // every bar it is handed. The in-process entrants therefore report every
    // declared cell completed, through the same reporter the external sweep's
    // failure log goes through, rather than through a second code path.
    let clean = sharpebench_harness::FailureLog::default();
    let mut reports: Vec<sharpebench_core::TrialReport> = entrant_ids
        .iter()
        .filter(|id| Some(*id) != declared_entrant.as_ref())
        .flat_map(|id| sharpebench_harness::trial_reports(id, &window_ids, &seeds, &clean))
        .collect();

    let bh = sharpebench_harness::run_agent("buy-and-hold", &data, &windows, &seeds, costs, || {
        Box::new(BuyAndHold) as Box<dyn Agent>
    });
    let mo = sharpebench_harness::run_agent("momentum", &data, &windows, &seeds, costs, || {
        Box::new(Momentum::default()) as Box<dyn Agent>
    });
    // The luck floor: random monkeys that show the zero-skill distribution.
    let mut field = vec![bh, mo];
    let mut external_accounting = None;
    field.extend(sharpebench_harness::luck_floor(
        &data,
        &windows,
        &seeds,
        costs,
        LUCK_FLOOR_AGENTS,
    ));

    // Optionally drive a real external agent (yours) through the *same* sim and
    // rank it into the field. `--http` hits a POST /decide endpoint; `--image` runs a
    // digest-pinned container through `sharpebench_arena`'s hardened boundary; `--cmd`
    // spawns a host subprocess. All three speak newline-delimited JSON over the same
    // protocol (see examples/reference-agent).
    //
    // `--image` is the path for an entrant whose code you do not control: the sandbox
    // refuses to launch when Docker is absent or the reference is not digest-pinned,
    // and never degrades to host execution. `--cmd` is host execution by definition,
    // so it announces itself on stderr rather than resolving to a quiet default.
    //
    // All three go through the transport-honest path: a wire blip is retried and, if it
    // persists, surfaced as an explicit failure instead of a masked degrade-to-hold.
    // `--checkpoint <path>` (external agents only) makes the sweep resumable: a crash
    // mid-run resumes and finishes only the outstanding window × seed tasks.
    //
    // `--fault-plan` wraps every attempt in the seeded fault injector and
    // `--retry-backoff` waits between runtime retries. Both are bound into the
    // checkpoint invocation identity. Without them the faulted and backed-off
    // drivers reduce to the unfaulted, immediate ones, byte for byte.
    const EXTERNAL_MAX_RETRIES: u32 = 2;
    let checkpoint = flag_value(args, "--checkpoint").map(std::path::PathBuf::from);
    let expected_lens: Vec<usize> = windows
        .iter()
        .map(|window| window.end.saturating_sub(window.start))
        .collect();
    let mut fault_row: Option<serde_json::Value> = None;
    if let (Some(plan), false) = (&fault_plan, json) {
        eprintln!(
            "fault plan {} armed; the entrant is told:\n{}",
            plan.digest(),
            plan.entrant_declaration()
        );
    }
    if let Some(addr) = flag_value(args, "--http") {
        let addr = addr.to_string();
        let label = declared_entrant
            .clone()
            .expect("--http is present in this branch, so the roster declared its entrant");
        let http_attempt = |wi: usize, seed: u64| {
            let mut agent = HttpAgent::new(addr.clone());
            sharpebench_harness::fault_plan::modes::run_faulted_backtest_observed(
                &data,
                &mut agent,
                windows[wi],
                seed,
                costs,
                rate_card.as_ref(),
                fault_plan.as_ref(),
            )
        };
        let (res, ledger) = if let Some(ckpt) = &checkpoint {
            let contract = match checkpoint_contract(
                args,
                CheckpointExecution {
                    data: &data,
                    windows: &windows,
                    seeds: &seeds,
                    costs,
                    score_config: &cfg,
                    max_retries: EXTERNAL_MAX_RETRIES,
                    rate_card: rate_card.as_ref(),
                    fault_plan: fault_plan.as_ref(),
                    backoff: &backoff,
                },
                label.as_bytes(),
                true,
            ) {
                Ok(contract) => contract,
                Err(error) => {
                    eprintln!("error: {error}");
                    return ExitCode::FAILURE;
                }
            };
            let res = match sharpebench_harness::run_resumable_sweep_with_backoff(
                ckpt,
                &label,
                &contract,
                &windows,
                resume_policy,
                &backoff,
                &mut sharpebench_harness::ThreadSleeper,
                http_attempt,
            ) {
                Ok(r) => r,
                Err(e) => {
                    eprintln!("error: checkpoint sweep failed: {e}");
                    return ExitCode::FAILURE;
                }
            };
            match checkpoint_fault_ledger(ckpt, fault_plan.as_ref()) {
                Ok(ledger) => (res, ledger),
                Err(error) => {
                    eprintln!("error: {error}");
                    return ExitCode::FAILURE;
                }
            }
        } else {
            sharpebench_harness::run_agent_resilient_faulted(
                &label,
                &expected_lens,
                &seeds,
                EXTERNAL_MAX_RETRIES,
                &backoff,
                &mut sharpebench_harness::ThreadSleeper,
                http_attempt,
            )
        };
        fault_row = fault_plan
            .as_ref()
            .map(|plan| fault_injection_report(plan, &windows, &seeds, &ledger));
        if !report_transport_failures(
            &label,
            &res.failures,
            windows.len() * seeds.len(),
            res.submission.runs.len(),
            (res.attempts, &res.monetary_cost),
            fault_row.as_ref(),
            json,
        ) {
            return ExitCode::FAILURE;
        }
        reports.extend(sharpebench_harness::trial_reports(
            &label,
            &window_ids,
            &seeds,
            &res.failures,
        ));
        external_accounting = Some((label, res.attempts, res.monetary_cost));
        field.insert(0, res.submission);
    } else if let Some(image) = flag_value(args, "--image") {
        let image = image.to_string();
        let mut opts = sharpebench_arena::SandboxOptions::default();
        // A preflight that completed clean hands back Docker's own immutable
        // configuration ID. That, and only that, is what the entrant launches
        // from: a repository digest names a manifest, the configuration ID names
        // the artifact that was actually scanned. The launcher's unpinned option
        // is enabled for this value alone, and the value cannot come from operator
        // input: `ValidatedImageId` is only obtainable from an authorizing report.
        let launch_image = match &scanned_launch {
            Some((image_id, _)) => {
                opts.allow_unpinned_image = true;
                image_id.clone()
            }
            None => image.clone(),
        };
        // Pre-flight: a refusal here (no daemon, unpinned tag, absent image) is the
        // point of this path. There is no fall-through to host execution.
        //
        // The presence check is separate because the launch cannot make it: with
        // `--pull never`, `docker run` against an image that is not there spawns
        // anyway and exits on its own, so an absent artifact would otherwise reach
        // the sweep as an agent that answers nothing rather than as a refusal.
        // The presence probe is redundant after a preflight: inspecting, creating
        // and exporting the image already proved it is present locally, and it
        // would reject a bare configuration ID as unpinned.
        if let Err(error) = sharpebench_arena::resolve_launch(
            sharpebench_arena::docker_available(),
            &launch_image,
            &opts,
        )
        .and_then(|_| {
            if scanned_launch.is_some() {
                Ok(())
            } else {
                sharpebench_arena::require_local_image(&image)
            }
        }) {
            eprintln!("error: cannot start the sandboxed agent `{image}`: {error}");
            return ExitCode::FAILURE;
        }
        let label = declared_entrant
            .clone()
            .expect("--image is present in this branch, so the roster declared its entrant");
        // One attempt = spawn, run, then `finish` for the post-exit resource
        // verdict: a container the kernel OOM-killed for exceeding the published
        // `--memory` budget surfaces as `ResourceLimitExceeded` (an agent fault),
        // not as the retryable transport blip its dead pipe would look like.
        let sandbox_attempt = |wi: usize, seed: u64| match sharpebench_arena::run_external_sandboxed(
            &launch_image,
            &opts,
        ) {
            Ok(mut a) => {
                let mut observed =
                    sharpebench_harness::fault_plan::modes::run_faulted_backtest_observed(
                        &data,
                        &mut a,
                        windows[wi],
                        seed,
                        costs,
                        rate_card.as_ref(),
                        fault_plan.as_ref(),
                    );
                observed.observation.result = match a.finish() {
                    Ok(oom_killed) => sharpebench_harness::apply_oom_verdict(
                        observed.observation.result,
                        oom_killed,
                    ),
                    Err(error) => {
                        // Post-exit inspection and named-container cleanup are
                        // part of the sandbox contract, not optional telemetry.
                        // If either is indeterminate, do not score or retry the
                        // entrant as though its resource verdict were known.
                        eprintln!(
                            "error: sandboxed agent `{image}` could not be finalized: {error}"
                        );
                        Err(sharpebench_harness::FailureKind::TransportError)
                    }
                };
                observed
            }
            Err(_) => sharpebench_harness::AttemptObservation::from(Err(
                sharpebench_harness::FailureKind::SpawnError,
            ))
            .into(),
        };
        // An unscanned run keeps its legacy material byte for byte. A scanned one
        // binds the policy digest, the configuration ID and the scanned scope, so
        // a changed policy is a different experiment rather than a silent resume.
        let entrant_material = match &scanned_launch {
            None => label.clone().into_bytes(),
            Some((image_id, policy_sha256)) => {
                match artifact_preflight::scanned_invocation_material(
                    &label,
                    image_id,
                    policy_sha256,
                ) {
                    Ok(material) => material,
                    Err(error) => {
                        eprintln!("error: {error}");
                        return ExitCode::FAILURE;
                    }
                }
            }
        };
        let (res, ledger) = if let Some(ckpt) = &checkpoint {
            let contract = match checkpoint_contract(
                args,
                CheckpointExecution {
                    data: &data,
                    windows: &windows,
                    seeds: &seeds,
                    costs,
                    score_config: &cfg,
                    max_retries: EXTERNAL_MAX_RETRIES,
                    rate_card: rate_card.as_ref(),
                    fault_plan: fault_plan.as_ref(),
                    backoff: &backoff,
                },
                &entrant_material,
                false,
            ) {
                Ok(contract) => contract,
                Err(error) => {
                    eprintln!("error: {error}");
                    return ExitCode::FAILURE;
                }
            };
            // The sweep layer treats a contract mismatch as a fresh sweep, which
            // would overwrite the file. A scanned run refuses first instead.
            if scanned_launch.is_some() {
                if let Err(error) = artifact_preflight::checkpoint_admits(ckpt, &label, &contract) {
                    eprintln!("error: {error}");
                    return ExitCode::FAILURE;
                }
            }
            let res = match sharpebench_harness::run_resumable_sweep_with_backoff(
                ckpt,
                &label,
                &contract,
                &windows,
                resume_policy,
                &backoff,
                &mut sharpebench_harness::ThreadSleeper,
                sandbox_attempt,
            ) {
                Ok(r) => r,
                Err(e) => {
                    eprintln!("error: checkpoint sweep failed: {e}");
                    return ExitCode::FAILURE;
                }
            };
            match checkpoint_fault_ledger(ckpt, fault_plan.as_ref()) {
                Ok(ledger) => (res, ledger),
                Err(error) => {
                    eprintln!("error: {error}");
                    return ExitCode::FAILURE;
                }
            }
        } else {
            sharpebench_harness::run_agent_resilient_faulted(
                &label,
                &expected_lens,
                &seeds,
                EXTERNAL_MAX_RETRIES,
                &backoff,
                &mut sharpebench_harness::ThreadSleeper,
                sandbox_attempt,
            )
        };
        fault_row = fault_plan
            .as_ref()
            .map(|plan| fault_injection_report(plan, &windows, &seeds, &ledger));
        if !report_transport_failures(
            &label,
            &res.failures,
            windows.len() * seeds.len(),
            res.submission.runs.len(),
            (res.attempts, &res.monetary_cost),
            fault_row.as_ref(),
            json,
        ) {
            return ExitCode::FAILURE;
        }
        reports.extend(sharpebench_harness::trial_reports(
            &label,
            &window_ids,
            &seeds,
            &res.failures,
        ));
        external_accounting = Some((label, res.attempts, res.monetary_cost));
        field.insert(0, res.submission);
    } else if let Some(cmd) = flag_value(args, "--cmd") {
        let parts: Vec<String> = cmd.split_whitespace().map(String::from).collect();
        let Some((prog, rest)) = parts.split_first() else {
            eprintln!("error: --cmd needs a program to run");
            return ExitCode::from(2);
        };
        let prog = prog.clone();
        let rest = rest.to_vec();
        // Unconditional and on stderr, so it is recorded in both output modes and
        // an unsandboxed run can never be mistaken for a sandboxed one.
        eprintln!(
            "warning: --cmd runs `{prog}` directly on this host with NO sandbox: no container, \
             no network or IPC isolation, no capability drop, no read-only root, no memory / \
             CPU / PID limits. The agent receives a CLEARED environment (PATH and platform \
             essentials only, never the harness's API keys); pass named variables through \
             with SHARPEBENCH_AGENT_ENV=NAME1,NAME2. Only point it at an agent you trust. \
             To run an untrusted entrant inside the hardened container boundary, use \
             `--image <repository@sha256:...>`."
        );
        // Pre-flight: fail fast with a clear message if the agent won't spawn at all.
        let rest_refs: Vec<&str> = rest.iter().map(String::as_str).collect();
        if ExternalAgent::spawn(&prog, &rest_refs).is_err() {
            eprintln!("error: cannot spawn agent `{cmd}`");
            return ExitCode::FAILURE;
        }
        let label = declared_entrant
            .clone()
            .expect("--cmd names a program in this branch, so the roster declared its entrant");
        let cmd_attempt = |wi: usize, seed: u64| {
            let rest_refs: Vec<&str> = rest.iter().map(String::as_str).collect();
            match ExternalAgent::spawn(&prog, &rest_refs) {
                Ok(mut a) => sharpebench_harness::fault_plan::modes::run_faulted_backtest_observed(
                    &data,
                    &mut a,
                    windows[wi],
                    seed,
                    costs,
                    rate_card.as_ref(),
                    fault_plan.as_ref(),
                ),
                Err(_) => sharpebench_harness::AttemptObservation::from(Err(
                    sharpebench_harness::FailureKind::SpawnError,
                ))
                .into(),
            }
        };
        let (res, ledger) = if let Some(ckpt) = &checkpoint {
            // The passed-through variable *names* are not the configuration:
            // the agent receives their current values. Binding names alone let
            // one checkpoint span AGENT_MODE=conservative and
            // AGENT_MODE=aggressive, so completed cells from one policy could
            // be resumed into the other. Bind the effective non-secret values.
            // Credentials stay out by design; see `agent_env_identity`.
            let entrant_material = cmd_entrant_material(cmd);
            let contract = match checkpoint_contract(
                args,
                CheckpointExecution {
                    data: &data,
                    windows: &windows,
                    seeds: &seeds,
                    costs,
                    score_config: &cfg,
                    max_retries: EXTERNAL_MAX_RETRIES,
                    rate_card: rate_card.as_ref(),
                    fault_plan: fault_plan.as_ref(),
                    backoff: &backoff,
                },
                entrant_material.as_bytes(),
                true,
            ) {
                Ok(contract) => contract,
                Err(error) => {
                    eprintln!("error: {error}");
                    return ExitCode::FAILURE;
                }
            };
            let res = match sharpebench_harness::run_resumable_sweep_with_backoff(
                ckpt,
                &label,
                &contract,
                &windows,
                resume_policy,
                &backoff,
                &mut sharpebench_harness::ThreadSleeper,
                cmd_attempt,
            ) {
                Ok(r) => r,
                Err(e) => {
                    eprintln!("error: checkpoint sweep failed: {e}");
                    return ExitCode::FAILURE;
                }
            };
            match checkpoint_fault_ledger(ckpt, fault_plan.as_ref()) {
                Ok(ledger) => (res, ledger),
                Err(error) => {
                    eprintln!("error: {error}");
                    return ExitCode::FAILURE;
                }
            }
        } else {
            sharpebench_harness::run_agent_resilient_faulted(
                &label,
                &expected_lens,
                &seeds,
                EXTERNAL_MAX_RETRIES,
                &backoff,
                &mut sharpebench_harness::ThreadSleeper,
                cmd_attempt,
            )
        };
        fault_row = fault_plan
            .as_ref()
            .map(|plan| fault_injection_report(plan, &windows, &seeds, &ledger));
        if !report_transport_failures(
            &label,
            &res.failures,
            windows.len() * seeds.len(),
            res.submission.runs.len(),
            (res.attempts, &res.monetary_cost),
            fault_row.as_ref(),
            json,
        ) {
            return ExitCode::FAILURE;
        }
        reports.extend(sharpebench_harness::trial_reports(
            &label,
            &window_ids,
            &seeds,
            &res.failures,
        ));
        external_accounting = Some((label, res.attempts, res.monetary_cost));
        field.insert(0, res.submission);
    }

    if !json {
        let src = flag_value(args, "--data").unwrap_or("synthetic");
        println!(
            "SharpeBench — run on {src} ({} symbols, {} windows × {} seeds, {periods_per_year} periods/year, costs on; incl. luck floor)\n",
            data.symbols().len(),
            windows.len(),
            seeds.len()
        );
        if cfg.pass_mode == sharpebench_core::PassMode::RelativeToBenchmark {
            println!(
                "reliability verdict: relative to `{}` (each run tested on its excess over the benchmark's run in the same window and seed)\n",
                cfg.benchmark_agent_id
            );
        }
    }
    let board = rank(&field, &cfg);
    // The controls run over the same dataset, windows and seeds the field was
    // scored on, and are bound to the board through `suite_evidence`, whose only
    // construction path refuses a control that is also a ranked entrant. The
    // suite declares no economic comparator: `buy-and-hold` is a ranked
    // reference entrant here, and a row cannot be both.
    let controls = [
        protocol_and_accounting_control(&data, &windows, &seeds, costs),
        refusal_control(&data, windows[0], seeds[0], costs),
    ];
    let evidence = match sharpebench_core::suite_evidence(&roster, &reports, &controls, &board) {
        Ok(evidence) => evidence,
        Err(error) => {
            eprintln!("error: {error}");
            return ExitCode::FAILURE;
        }
    };
    if json {
        let board_json = run_board_json(
            &board,
            external_accounting
                .as_ref()
                .map(|(label, attempts, cost)| ExternalRowMetadata {
                    agent: label.as_str(),
                    attempts: *attempts,
                    monetary_cost: cost,
                    artifact_preflight: preflight_row.clone(),
                    fault_injection: fault_row.clone(),
                }),
        );
        // The board stays the whole document unless the evidence is asked for.
        // Wrapping it unconditionally would rename the top level of a machine
        // output every existing reader parses as an array, and the evidence is
        // reporting surface beside the board rather than part of it.
        if args.iter().any(|arg| arg == "--suite-evidence") {
            emit_json(&serde_json::json!({
                "board": board_json,
                "suite_evidence": evidence,
            }));
        } else {
            emit_json(&board_json);
        }
    } else {
        if let Some((label, attempts, cost)) = external_accounting {
            print_attempt_accounting(&label, attempts, &cost);
            if let Some(report) = &fault_row {
                print_fault_injection(&label, report);
            }
        }
        print_board(&board);
        print_suite_evidence(&board, &evidence);
    }
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

    if external_capture::names_external_entrant(args) {
        return external_capture::run_capture_external(
            args,
            json,
            &external_capture::DockerSandbox,
        );
    }
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
    let (_sub, mut traj) =
        sharpebench_harness::run_agent_capture(agent_id, &data, &windows, &seeds, costs, || make());
    match current_executable_sha256() {
        Ok(digest) => {
            if let Some(contract) = &mut traj.contract {
                contract.runner_artifact_sha256 = Some(digest);
            }
        }
        Err(error) => {
            eprintln!("error: {error}");
            return ExitCode::FAILURE;
        }
    }
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
        eprintln!(
            "usage: sharpebench verify-trajectory <trajectory.json> [--data <csv>] [--allow-unbound-trajectory] [--reexecute [--cmd \"<prog>\"|--http <addr>]] [--json]"
        );
        return ExitCode::from(2);
    }
    let reexecute = args.iter().any(|arg| arg == "--reexecute");
    if !reexecute
        && ["--cmd", "--http"]
            .iter()
            .any(|flag| args.iter().any(|arg| arg == flag))
    {
        eprintln!("error: --cmd and --http name the agent to re-execute; they require --reexecute");
        return ExitCode::from(2);
    }
    if !reexecute && args.iter().any(|arg| arg == "--image") {
        eprintln!("error: --image names the agent to re-execute; it requires --reexecute");
        return ExitCode::from(2);
    }
    if reexecute && args.iter().any(|arg| arg == "--allow-unbound-trajectory") {
        eprintln!(
            "error: --reexecute requires the strict trajectory contract and cannot be combined with --allow-unbound-trajectory"
        );
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
    let costs = CostModel::default();
    let cfg = ScoreConfig::default();
    if reexecute {
        return run_reexecution(
            args,
            &data,
            &traj,
            costs,
            &cfg,
            json,
            &external_capture::DockerSandbox,
        );
    }
    let result = if args.iter().any(|arg| arg == "--allow-unbound-trajectory") {
        sharpebench_harness::verify_trajectory(&data, &traj, costs, &cfg)
    } else {
        let runner = match current_executable_sha256() {
            Ok(digest) => digest,
            Err(error) => {
                eprintln!("error: {error}");
                return ExitCode::FAILURE;
            }
        };
        match sharpebench_harness::verify_trajectory_strict(
            &data,
            &traj,
            costs,
            &cfg,
            Some(&runner),
        ) {
            Ok(result) => result,
            Err(error) => {
                eprintln!(
                    "error: {error}. Use --allow-unbound-trajectory only for an explicit legacy or cross-version regrade."
                );
                return ExitCode::FAILURE;
            }
        }
    };
    emit_verification(&result, json, None);
    ExitCode::SUCCESS
}

/// What a passed re-execution adds to the verification output.
#[derive(Serialize)]
struct ReexecutionSummary {
    agent: String,
    runs_reexecuted: usize,
    decisions_compared: usize,
}

fn emit_verification(
    result: &sharpebench_harness::VerificationResult,
    json: bool,
    reexecution: Option<&ReexecutionSummary>,
) {
    if json {
        let sealed =
            sharpebench_core::seal(result, &sharpebench_harness::VERIFICATION_RESULT_VISIBILITY)
                .expect("verification results serialize");
        match reexecution {
            None => emit_json(&sealed),
            Some(reexecution) => {
                #[derive(Serialize)]
                struct Reexecuted<'a> {
                    #[serde(flatten)]
                    verification: &'a sharpebench_core::EntrantView,
                    reexecution: &'a ReexecutionSummary,
                }
                emit_json(&Reexecuted {
                    verification: &sealed,
                    reexecution,
                });
            }
        }
    } else {
        println!(
            "verified `{}` by replay — {} decisions across {} runs",
            result.agent_id, result.decisions_replayed, result.runs_replayed
        );
        println!("  deflated Sharpe : {:.4}", result.score.deflated_sharpe);
        println!("  raw mean return : {:.5}", result.score.raw_mean_return);
        println!("  rank-eligible   : {}", yn(result.score.rank_eligible));
        match &result.declared_verdict {
            sharpebench_harness::DeclaredVerdictVerification::FieldRequired { benchmark_id } => {
                println!("  declared mandate: requires aligned field benchmark `{benchmark_id}`; not verified from one artifact")
            }
            other => println!("  declared mandate: {other:?}"),
        }
        println!("\n{}", result.verification_explanation);
        if let Some(reexecution) = reexecution {
            println!(
                "\nRe-executed {} runs with a fresh `{}` agent on the same data, window and seed; all {} score-bearing decisions repeated.",
                reexecution.runs_reexecuted, reexecution.agent, reexecution.decisions_compared
            );
        }
    }
}

/// An external agent whose transport health is watched while it re-executes.
/// A degrade-to-hold would otherwise surface as a divergence and be read as
/// non-determinism; the first transport or protocol fault is recorded instead.
struct WatchedAgent<A> {
    inner: A,
    fault: std::rc::Rc<std::cell::RefCell<Option<sharpebench_harness::FailureKind>>>,
}

impl<A: sharpebench_sim::Agent + sharpebench_sim::TransportDiagnostics> sharpebench_sim::Agent
    for WatchedAgent<A>
{
    fn decide(
        &mut self,
        observation: &sharpebench_protocol::MarketObservation,
    ) -> sharpebench_protocol::Decision {
        let decision = self.inner.decide(observation);
        if let Some(kind) = sharpebench_harness::transport_failure(self.inner.health()) {
            self.fault.borrow_mut().get_or_insert(kind);
        }
        decision
    }
}

/// `verify-trajectory --reexecute`: the strict checks, then every captured run
/// re-executed with a fresh agent and compared decision by decision. The agent
/// is `--cmd "<prog>"`, `--http <addr>`, `--image <repository@sha256:...>` (a
/// fresh hardened container per run, launched by `launcher`), or, with none of
/// them, the reference agent the trajectory names (`buy-and-hold` or
/// `momentum`).
fn run_reexecution(
    args: &[String],
    data: &sharpebench_sim::Dataset,
    traj: &sharpebench_protocol::AgentTrajectory,
    costs: sharpebench_sim::CostModel,
    cfg: &ScoreConfig,
    json: bool,
    launcher: &dyn external_capture::SandboxLauncher,
) -> ExitCode {
    use sharpebench_sim::{Agent, BuyAndHold, ExternalAgent, HoldAgent, HttpAgent, Momentum};

    let fault: external_capture::FaultCell = std::rc::Rc::new(std::cell::RefCell::new(None));
    let image = args.iter().any(|arg| arg == "--image");
    if image
        && ["--cmd", "--http"]
            .iter()
            .any(|flag| args.iter().any(|arg| arg == flag))
    {
        eprintln!("error: --image, --cmd and --http each name the agent to re-execute; pass one");
        return ExitCode::from(2);
    }
    let image = match flag_value(args, "--image") {
        Some(reference) if !reference.starts_with("--") => Some(reference.to_string()),
        _ if image => {
            eprintln!("error: --image needs a digest-pinned reference, <repository@sha256:...>");
            return ExitCode::from(2);
        }
        _ => None,
    };
    let (label, mut make): (String, Box<dyn FnMut() -> Box<dyn Agent> + '_>) = if let Some(image) =
        &image
    {
        let label = format!("sandbox:{image}");
        if let Err(error) = launcher.admit(image) {
            if json {
                emit_json(&serde_json::json!({
                    "verified": false,
                    "error": "reexecution_transport_failure",
                    "failure": sharpebench_harness::FailureKind::SpawnError,
                    "agent": label,
                }));
            }
            eprintln!("error: cannot start the sandboxed agent `{image}`: {error}");
            return ExitCode::FAILURE;
        }
        (
            label,
            Box::new(external_capture::sandbox_factory(
                image,
                launcher,
                fault.clone(),
            )),
        )
    } else if let Some(cmd) = flag_value(args, "--cmd") {
        let parts: Vec<String> = cmd.split_whitespace().map(String::from).collect();
        let Some((prog, rest)) = parts.split_first() else {
            eprintln!("error: --cmd needs a program to run");
            return ExitCode::from(2);
        };
        let (prog, rest) = (prog.clone(), rest.to_vec());
        eprintln!(
                "warning: --reexecute --cmd runs `{prog}` directly on this host with NO sandbox, once per captured run. Only point it at an agent you trust."
            );
        let fault = fault.clone();
        (
            format!("cmd:{prog}"),
            Box::new(move || {
                let rest_refs: Vec<&str> = rest.iter().map(String::as_str).collect();
                match ExternalAgent::spawn(&prog, &rest_refs) {
                    Ok(agent) => Box::new(WatchedAgent {
                        inner: agent,
                        fault: fault.clone(),
                    }) as Box<dyn Agent>,
                    Err(_) => {
                        fault
                            .borrow_mut()
                            .get_or_insert(sharpebench_harness::FailureKind::SpawnError);
                        Box::new(HoldAgent)
                    }
                }
            }),
        )
    } else if let Some(addr) = flag_value(args, "--http") {
        let addr = addr.to_string();
        let fault = fault.clone();
        (
            format!("http:{addr}"),
            Box::new(move || {
                Box::new(WatchedAgent {
                    inner: HttpAgent::new(addr.clone()),
                    fault: fault.clone(),
                }) as Box<dyn Agent>
            }),
        )
    } else {
        match traj.agent_id.as_str() {
            "buy-and-hold" => (
                traj.agent_id.clone(),
                Box::new(|| Box::new(BuyAndHold) as Box<dyn Agent>),
            ),
            "momentum" => (
                traj.agent_id.clone(),
                Box::new(|| Box::new(Momentum::default()) as Box<dyn Agent>),
            ),
            other => {
                eprintln!(
                        "error: --reexecute needs the agent to launch: `{other}` is not a reference agent, so pass --cmd \"<prog>\" or --http <addr>"
                    );
                return ExitCode::from(2);
            }
        }
    };
    let runner = match current_executable_sha256() {
        Ok(digest) => digest,
        Err(error) => {
            eprintln!("error: {error}");
            return ExitCode::FAILURE;
        }
    };
    let outcome = sharpebench_harness::verify_trajectory_reexecuted(
        data,
        traj,
        costs,
        cfg,
        Some(&runner),
        &mut make,
    );
    if let Some(kind) = fault.borrow().clone() {
        let message = format!(
            "re-execution of `{label}` hit a {kind:?} failure, so its decisions cannot be compared; no verdict on determinism was reached"
        );
        if json {
            emit_json(&serde_json::json!({
                "verified": false,
                "error": "reexecution_transport_failure",
                "failure": kind,
                "agent": label,
            }));
        }
        eprintln!("error: {message}");
        return ExitCode::FAILURE;
    }
    match outcome {
        Ok(result) => {
            let summary = ReexecutionSummary {
                agent: label,
                runs_reexecuted: result.runs_replayed,
                decisions_compared: result.decisions_replayed,
            };
            emit_verification(&result, json, Some(&summary));
            ExitCode::SUCCESS
        }
        Err(sharpebench_harness::ReexecutionError::Refused(error)) => {
            eprintln!("error: {error}");
            ExitCode::FAILURE
        }
        Err(error @ sharpebench_harness::ReexecutionError::Diverged(_)) => {
            if let sharpebench_harness::ReexecutionError::Diverged(divergence) = &error {
                if json {
                    emit_json(&serde_json::json!({
                        "verified": false,
                        "error": "reexecution_diverged",
                        "agent": label,
                        "divergence": divergence,
                    }));
                }
            }
            eprintln!("error: {error}");
            ExitCode::FAILURE
        }
    }
}

fn run_score(path: &str, args: &[String], json: bool) -> ExitCode {
    let data = match std::fs::read_to_string(path) {
        Ok(d) => d,
        Err(e) => {
            eprintln!("error: cannot read {path}: {e}");
            return ExitCode::FAILURE;
        }
    };
    // Each object is a submission plus an optional `declared_mandate`; the
    // declaration is scored as a labeled second verdict and never moves rank.
    //
    // Cross-agent comparison reads `runs[i]` for every agent. Without
    // `--require-run-keys` that alignment is the caller's assertion, unchecked.
    // With it, every submission must carry one `run_keys` entry per run; the
    // field is refused unless the cells are complete, unique and shared, and the
    // runs are reordered into one canonical cell order before scoring.
    let require_run_keys = args.iter().any(|a| a == "--require-run-keys");
    let (subs, declarations, keyed_seed_count) = if require_run_keys {
        match sharpebench_core::parse_keyed_field(&data) {
            Ok(field) => (
                field.submissions,
                field.declarations,
                Some(field.seeds.len()),
            ),
            Err(e) => {
                eprintln!("error: {e}");
                return ExitCode::FAILURE;
            }
        }
    } else {
        match sharpebench_core::parse_declared_field(&data) {
            Ok((subs, declarations)) => (subs, declarations, None),
            Err(e) => {
                eprintln!("error: {e}");
                return ExitCode::FAILURE;
            }
        }
    };
    let mut cfg = match score_config_from_args(args) {
        Ok(cfg) => cfg,
        Err(error) => {
            eprintln!("error: {error}");
            return ExitCode::from(2);
        }
    };
    if let Some(width) = keyed_seed_count {
        // Canonical keys are window-major and contain the complete seed grid.
        // Preserve that replicate geometry when the scorer pools observations.
        if args.iter().any(|arg| arg == "--execution-seeds-per-window")
            && cfg.execution_seeds_per_window != width
        {
            eprintln!(
                "error: validated run keys require {width} execution seeds per window, but --execution-seeds-per-window is {}",
                cfg.execution_seeds_per_window
            );
            return ExitCode::from(2);
        }
        cfg.execution_seeds_per_window = width;
    }
    // `--rank-mode` is opt-in by versioned identifier. Absent, the board is
    // `rank_declared` byte for byte; an identifier the kernel does not
    // implement is refused rather than silently ranked under the legacy
    // protocol.
    let rank_mode = if args.iter().any(|a| a == "--rank-mode") {
        let Some(id) = flag_value(args, "--rank-mode").filter(|v| !v.starts_with("--")) else {
            eprintln!("error: --rank-mode requires a value");
            return ExitCode::from(2);
        };
        match sharpebench_core::RankMode::parse(id) {
            Ok(mode) => Some(mode),
            Err(error) => {
                eprintln!("error: {error}");
                return ExitCode::from(2);
            }
        }
    } else {
        None
    };
    // `--diagnostics` is opt-in. Absent, nothing below this point differs
    // from the board-only output; present, the diagnostics are computed from
    // the finished board and printed beside it, never written into a row.
    let diagnostics = if args.iter().any(|a| a == "--diagnostics") {
        let Some(list) = flag_value(args, "--diagnostics").filter(|v| !v.starts_with("--")) else {
            eprintln!("error: --diagnostics requires a value");
            return ExitCode::from(2);
        };
        match sharpebench_core::SharpeDiagnostic::parse_list(list) {
            Ok(requested) => Some(requested),
            Err(error) => {
                eprintln!("error: {error}");
                return ExitCode::from(2);
            }
        }
    } else {
        None
    };
    let board = match rank_mode {
        Some(mode) => sharpebench_core::rank_certified(&subs, &declarations, &cfg, mode),
        None => sharpebench_core::rank_declared(&subs, &declarations, &cfg),
    };
    match diagnostics {
        None => emit_board(&board, json),
        Some(requested) => {
            let report = sharpebench_core::sharpe_diagnostics(&subs, &board, &cfg, &requested);
            if json {
                emit_json(&serde_json::json!({
                    "board": sharpebench_core::seal_board(&board),
                    "sharpe_diagnostics": report,
                }));
            } else {
                print_board(&board);
                print_sharpe_diagnostics(&report, &requested);
            }
        }
    }
    ExitCode::SUCCESS
}

/// The opt-in diagnostics as a table after the board. A value that could not
/// be computed prints as `n/a`; the JSON form carries the reason.
fn print_sharpe_diagnostics(
    report: &[sharpebench_core::SharpeDiagnostics],
    requested: &[sharpebench_core::SharpeDiagnostic],
) {
    use sharpebench_core::SharpeDiagnostic;
    let cell = |v: Option<f64>| v.map_or_else(|| "n/a".to_string(), |v| format!("{v:.4}"));
    let mut header = format!("{:<18} {:>6}", "agent", "obs");
    for d in requested {
        header.push_str(match d {
            SharpeDiagnostic::AutocorrelatedPsr => "     rho  ac_PSR  ac_DSR",
            SharpeDiagnostic::NullSePsr => " null_PSR null_DSR",
            SharpeDiagnostic::Mppm => "   MPPM(3)/yr",
        });
    }
    println!("\nOpt-in Sharpe diagnostics. Not used by the gate, eligibility or the rank.");
    println!("{header}");
    println!("{}", "-".repeat(header.chars().count()));
    for row in report {
        let mut line = format!(
            "{:<18} {:>6}",
            truncate(&row.agent_id, 18),
            row.pooled_observations
        );
        for d in requested {
            let text = match d {
                SharpeDiagnostic::AutocorrelatedPsr => {
                    let p = row.autocorrelated_psr.as_ref();
                    format!(
                        " {:>7} {:>7} {:>7}",
                        cell(p.and_then(|p| p.rho)),
                        cell(p.and_then(|p| p.psr)),
                        cell(p.and_then(|p| p.psr_at_deflation_bar))
                    )
                }
                SharpeDiagnostic::NullSePsr => {
                    let p = row.null_se_psr.as_ref();
                    format!(
                        " {:>8} {:>8}",
                        cell(p.and_then(|p| p.psr)),
                        cell(p.and_then(|p| p.psr_at_deflation_bar))
                    )
                }
                SharpeDiagnostic::Mppm => {
                    format!(
                        " {:>12}",
                        cell(row.mppm.as_ref().and_then(|m| m.annualized))
                    )
                }
            };
            line.push_str(&text);
        }
        println!("{line}");
    }
    for d in requested {
        println!(
            "{}",
            match d {
                SharpeDiagnostic::AutocorrelatedPsr =>
                    "ac_*: PSR against 0 and against the row's deflation bar with the pooled track's lag-one autocorrelation (López de Prado, Lipton and Zoonekynd 2026, eqs. 2-3).",
                SharpeDiagnostic::NullSePsr =>
                    "null_*: the same two statistics with the standard error evaluated at the benchmark, serial independence kept (ibid., eqs. 4-5).",
                SharpeDiagnostic::Mppm =>
                    "MPPM(3)/yr: manipulation-proof performance, risk aversion 3, zero risk-free rate, annualized (Goetzmann, Ingersoll, Spiegel and Welch 2007, eq. 18).",
            }
        );
    }
}

/// Shared by `score` and `disqualify`: explanations use the same host verdict,
/// annualization and execution-replicate controls as the displayed board.
fn score_config_from_args(args: &[String]) -> Result<ScoreConfig, String> {
    for name in [
        "--periods-per-year",
        "--execution-seeds-per-window",
        "--pass-mode",
        "--benchmark-agent",
    ] {
        if args.iter().any(|arg| arg == name)
            && flag_value(args, name)
                .is_none_or(|value| value.is_empty() || value.starts_with("--"))
        {
            return Err(format!("{name} requires a value"));
        }
    }
    let periods = positive_f64_flag(args, "--periods-per-year")?
        .unwrap_or(ScoreConfig::default().periods_per_year);
    let mut cfg = ScoreConfig::for_periods_per_year(periods);
    if let Some(width) = positive_usize_flag(args, "--execution-seeds-per-window")? {
        cfg.execution_seeds_per_window = width;
    }
    apply_pass_mode_flags(args, &mut cfg)?;
    Ok(cfg)
}

fn positive_f64_flag(args: &[String], flag: &str) -> Result<Option<f64>, String> {
    let Some(raw) = flag_value(args, flag) else {
        return Ok(None);
    };
    match raw.parse::<f64>() {
        Ok(value) if value.is_finite() && value > 0.0 => Ok(Some(value)),
        _ => Err(format!(
            "{flag} must be a positive finite number, got `{raw}`"
        )),
    }
}

fn positive_usize_flag(args: &[String], flag: &str) -> Result<Option<usize>, String> {
    let Some(raw) = flag_value(args, flag) else {
        return Ok(None);
    };
    match raw.parse::<usize>() {
        Ok(value) if value > 0 => Ok(Some(value)),
        _ => Err(format!("{flag} must be a positive integer, got `{raw}`")),
    }
}

/// Render a board as a human table, or as JSON when `json` is set.
fn emit_board(board: &[CompositeScore], json: bool) {
    if json {
        emit_json(&sharpebench_core::seal_board(board));
    } else {
        print_board(board);
    }
}

fn print_board(board: &[CompositeScore]) {
    // The mandate column appears only when some row declared one, so a board
    // with no declarations prints exactly as before.
    let declared = board.iter().any(|s| s.verdict_applied.is_some());
    let mandate_header = if declared { " mandate" } else { "" };
    // Likewise the certification column: present only under a rank mode.
    let certified = board.iter().any(|s| s.certification.is_some());
    let certified_header = if certified { " certified" } else { "" };
    println!(
        "{:<4} {:<18} {:>9} {:>8} {:>7} {:>6} {:>9} {:>10}{mandate_header}{certified_header}",
        "#", "agent", "DSR", "PSR", "pass^k", "proc", "boot_p", "raw_ret"
    );
    println!("{}", "-".repeat(80));
    for (i, s) in board.iter().enumerate() {
        let pos = if s.rank_eligible {
            format!("{}", i + 1)
        } else {
            "—".to_string()
        };
        let mandate = if declared {
            format!(
                " {}",
                s.mandate_verdict_label()
                    .unwrap_or_else(|| "undeclared".to_string())
            )
        } else {
            String::new()
        };
        let certification = if certified {
            format!(
                " {}",
                s.certification
                    .as_ref()
                    .map_or_else(|| "uncertified".to_string(), |c| c.describe())
            )
        } else {
            String::new()
        };
        println!(
            "{:<4} {:<18} {:>9.4} {:>8.4} {:>7} {:>6} {:>9.4} {:>10.5}{mandate}{certification}",
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
    if declared {
        println!(
            "Declared mandates are reported beside the board verdict, never ranked against it: \
             \"meets\" is eligibility under the verdict the submitter declared, \"all-weather\" \
             is eligibility under this board's verdict, and # counts the latter only."
        );
    }
}

fn yn(b: bool) -> &'static str {
    if b {
        "yes"
    } else {
        "NO"
    }
}

fn truncate(s: &str, n: usize) -> String {
    if n == 0 {
        String::new()
    } else if s.chars().count() <= n {
        s.to_string()
    } else {
        format!("{}…", s.chars().take(n - 1).collect::<String>())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn attempt_metadata_preserves_failed_work_without_changing_the_board() {
        let mut calls = 0;
        let res = sharpebench_harness::run_agent_resilient("external", 1, &[7], 2, 40, |_, _| {
            calls += 1;
            if calls < 3 {
                Err(sharpebench_harness::FailureKind::TransportError)
            } else {
                Ok(sharpebench_harness::failing_sentinel_run(40))
            }
        });
        let reference = AgentSubmission {
            agent_id: "reference".into(),
            ..res.submission.clone()
        };
        let board = rank(&[res.submission, reference], &ScoreConfig::default());
        let plain = run_board_json(&board, None);
        assert_eq!(plain, serde_json::to_value(&board).unwrap());
        let mut observed = run_board_json(
            &board,
            Some(ExternalRowMetadata {
                agent: "external",
                attempts: res.attempts,
                monetary_cost: &res.monetary_cost,
                artifact_preflight: None,
                fault_injection: None,
            }),
        );
        let rows = observed.as_array_mut().unwrap();
        let external = rows
            .iter_mut()
            .find(|row| row["agent_id"] == "external")
            .unwrap();
        let accounting = external
            .as_object_mut()
            .unwrap()
            .remove("attempt_accounting")
            .unwrap();
        assert_eq!(
            accounting["schema_version"],
            "sharpebench.attempt-accounting.v1"
        );
        assert_eq!(accounting["attempts"]["attempts"], 3);
        assert_eq!(accounting["attempts"]["completed"], 1);
        assert_eq!(accounting["attempts"]["failed"], 2);
        assert_eq!(accounting["attempts"]["duration_source"], "host_clock");
        assert_eq!(accounting["monetary_cost"]["status"], "unavailable");
        assert_eq!(
            accounting["monetary_cost"]["reason"],
            "attempt_ledger_has_no_usage_evidence"
        );
        assert_eq!(accounting["rank_neutral"], true);
        assert_eq!(
            observed, plain,
            "metadata must not change scores, order or reference rows"
        );
    }

    /// A successful scanned run emits a real board.
    ///
    /// The regression is deliberate: the board is a JSON ARRAY, and the lost
    /// prototype attached its preflight metadata with `output["artifact_preflight"]`
    /// on that array. `serde_json`'s mutable index panics on a non-object, so the
    /// only run that ever reached that line, a clean scan followed by a completed
    /// sweep, would have aborted the process. Nothing but a passing board catches
    /// it, which is why the old failure-only fake-Docker tests did not.
    #[test]
    fn a_successful_scanned_run_emits_a_board_array_with_preflight_on_the_entrant_row() {
        let res = sharpebench_harness::run_agent_resilient("external", 1, &[7], 2, 40, |_, _| {
            Ok(sharpebench_harness::failing_sentinel_run(40))
        });
        let reference = AgentSubmission {
            agent_id: "reference".into(),
            ..res.submission.clone()
        };
        let board = rank(&[res.submission, reference], &ScoreConfig::default());
        let image_id = format!("sha256:{}", "e".repeat(64));
        let preflight = serde_json::json!({
            "schema_version": artifact_preflight::REPORT_VERSION,
            "scope": artifact_preflight::SCAN_SCOPE,
            "image_id": image_id,
            "cleanup_verified": true,
        });
        let observed = run_board_json(
            &board,
            Some(ExternalRowMetadata {
                agent: "external",
                attempts: res.attempts,
                monetary_cost: &res.monetary_cost,
                artifact_preflight: Some(preflight),
                fault_injection: None,
            }),
        );

        let rows = observed
            .as_array()
            .expect("a board stays an array of rows, never an object");
        assert_eq!(rows.len(), 2);
        assert!(
            observed.get("artifact_preflight").is_none(),
            "the board itself must never carry the metadata key"
        );
        let entrant = rows
            .iter()
            .find(|row| row["agent_id"] == "external")
            .expect("the entrant row is present");
        assert_eq!(entrant["artifact_preflight"]["image_id"], image_id);
        assert_eq!(
            entrant["artifact_preflight"]["schema_version"],
            artifact_preflight::REPORT_VERSION
        );
        assert_eq!(entrant["artifact_preflight"]["cleanup_verified"], true);
        assert_eq!(
            entrant["attempt_accounting"]["schema_version"],
            "sharpebench.attempt-accounting.v1"
        );

        let plain = run_board_json(&board, None);
        let reference_row = rows
            .iter()
            .find(|row| row["agent_id"] == "reference")
            .expect("the reference row is present");
        assert_eq!(
            reference_row,
            &plain.as_array().expect("a board is an array")[rows
                .iter()
                .position(|row| row["agent_id"] == "reference")
                .expect("the reference row is present")],
            "reference rows carry no operational ledger"
        );
        assert!(reference_row.get("artifact_preflight").is_none());
        assert!(reference_row.get("attempt_accounting").is_none());
    }

    #[test]
    fn attempt_metadata_does_not_label_unobserved_time_or_money_as_measured() {
        let mut ledger = sharpebench_harness::AttemptLedger::default();
        ledger.push(sharpebench_harness::AttemptRecord::failed(
            sharpebench_harness::FailureKind::Timeout,
            sharpebench_harness::AttemptDuration::Unavailable,
        ));
        let value = attempt_accounting(ledger.summary());
        assert_eq!(value["attempts"]["failed"], 1);
        assert_eq!(value["attempts"]["duration_source"], "unavailable");
        assert_eq!(value["monetary_cost"]["status"], "unavailable");
        assert!(value["monetary_cost"].get("value").is_none());
    }

    #[test]
    fn truncation_preserves_utf8_and_respects_the_character_budget() {
        for id in [
            "量化交易代理量化交易代理量化交易代理量化交易代理",
            "🦀🦀🦀🦀🦀🦀🦀🦀🦀🦀",
            "e\u{301}name",
        ] {
            for n in 0..20 {
                let result = truncate(id, n);
                assert!(result.chars().count() <= n);
                if id.chars().count() <= n {
                    assert_eq!(result, id);
                } else if n > 0 {
                    assert!(result.ends_with('…'));
                    assert!(id.starts_with(result.trim_end_matches('…')));
                }
            }
        }
        assert_eq!(truncate("ascii", 4), "asc…");
    }

    fn args(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| (*value).to_string()).collect()
    }

    #[test]
    fn score_frequency_and_replicate_flags_are_positive() {
        let values = args(&[
            "sharpebench",
            "score",
            "field.json",
            "--periods-per-year",
            "8760",
            "--execution-seeds-per-window",
            "4",
        ]);
        assert_eq!(
            positive_f64_flag(&values, "--periods-per-year"),
            Ok(Some(8760.0))
        );
        assert_eq!(
            positive_usize_flag(&values, "--execution-seeds-per-window"),
            Ok(Some(4))
        );
    }

    #[test]
    fn score_frequency_and_replicate_flags_reject_invalid_values() {
        let frequency = args(&[
            "sharpebench",
            "score",
            "field.json",
            "--periods-per-year",
            "NaN",
        ]);
        assert!(positive_f64_flag(&frequency, "--periods-per-year").is_err());
        let replicates = args(&[
            "sharpebench",
            "score",
            "field.json",
            "--execution-seeds-per-window",
            "0",
        ]);
        assert!(positive_usize_flag(&replicates, "--execution-seeds-per-window").is_err());
    }

    #[test]
    fn checkpoint_contract_requires_an_artifact_digest_for_remote_entrants() {
        let values = args(&["sharpebench", "run", "--checkpoint", "sweep.json"]);
        let data = sharpebench_sim::Dataset::synthetic(2, 12, 7);
        let windows = [sharpebench_sim::Window { start: 2, end: 12 }];
        let error = checkpoint_contract(
            &values,
            CheckpointExecution {
                data: &data,
                windows: &windows,
                seeds: &[3],
                costs: sharpebench_sim::CostModel::default(),
                score_config: &ScoreConfig::default(),
                max_retries: 2,
                rate_card: None,
                fault_plan: None,
                backoff: &BackoffSchedule::immediate(),
            },
            b"http:127.0.0.1:9000",
            true,
        )
        .expect_err("an endpoint address is not an entrant artifact identity");
        assert!(error.contains("also requires --entrant-sha256"), "{error}");
    }

    #[test]
    fn checkpoint_contract_pins_the_declared_remote_artifact() {
        let digest = "a".repeat(64);
        let values = args(&[
            "sharpebench",
            "run",
            "--checkpoint",
            "sweep.json",
            "--entrant-sha256",
            &digest,
        ]);
        let data = sharpebench_sim::Dataset::synthetic(2, 12, 7);
        let windows = [sharpebench_sim::Window { start: 2, end: 12 }];
        let contract = checkpoint_contract(
            &values,
            CheckpointExecution {
                data: &data,
                windows: &windows,
                seeds: &[3],
                costs: sharpebench_sim::CostModel::default(),
                score_config: &ScoreConfig::default(),
                max_retries: 2,
                rate_card: None,
                fault_plan: None,
                backoff: &BackoffSchedule::immediate(),
            },
            b"http:127.0.0.1:9000",
            true,
        )
        .expect("a declared artifact digest binds the checkpoint");
        assert_eq!(contract.entrant_sha256, digest);
        assert_eq!(
            contract.invocation_sha256,
            sharpebench_attest::content_digest(b"http:127.0.0.1:9000")
        );
    }

    #[test]
    fn checkpoint_contract_separately_binds_artifact_and_invocation() {
        let digest = "a".repeat(64);
        let values = args(&[
            "sharpebench",
            "run",
            "--checkpoint",
            "sweep.json",
            "--entrant-sha256",
            &digest,
        ]);
        let data = sharpebench_sim::Dataset::synthetic(2, 12, 7);
        let windows = [sharpebench_sim::Window { start: 2, end: 12 }];
        let build = |invocation: &[u8]| {
            checkpoint_contract(
                &values,
                CheckpointExecution {
                    data: &data,
                    windows: &windows,
                    seeds: &[3],
                    costs: sharpebench_sim::CostModel::default(),
                    score_config: &ScoreConfig::default(),
                    max_retries: 2,
                    rate_card: None,
                    fault_plan: None,
                    backoff: &BackoffSchedule::immediate(),
                },
                invocation,
                true,
            )
            .expect("a complete checkpoint contract")
        };
        let first = build(b"cmd\0agent --mode conservative\0TOKEN");
        let second = build(b"cmd\0agent --mode aggressive\0TOKEN");
        assert_eq!(first.entrant_sha256, second.entrant_sha256);
        assert_ne!(first.invocation_sha256, second.invocation_sha256);
    }

    /// The `--cmd` invocation identity binds the *effective* value of every
    /// passed-through non-secret variable, not just its name, so a policy
    /// change cannot resume into a checkpoint started under the other policy.
    /// A rotated credential is a separate concern and must not do the same.
    #[test]
    fn cmd_invocation_identity_binds_nonsecret_values_and_excludes_credentials() {
        const MODE: &str = "SHARPEBENCH_TEST_BR1_MODE";
        const TOKEN: &str = "SHARPEBENCH_TEST_BR1_TOKEN";
        std::env::set_var(
            sharpebench_sim::AGENT_ENV_PASSTHROUGH,
            format!("{MODE},{TOKEN}"),
        );
        std::env::set_var(TOKEN, "sk-first");

        std::env::set_var(MODE, "conservative");
        let conservative = cmd_entrant_material("agent.py");
        std::env::set_var(MODE, "aggressive");
        let aggressive = cmd_entrant_material("agent.py");
        assert_ne!(
            conservative, aggressive,
            "a changed policy value must not share a checkpoint identity"
        );

        std::env::set_var(MODE, "conservative");
        std::env::set_var(TOKEN, "sk-second");
        assert_eq!(
            conservative,
            cmd_entrant_material("agent.py"),
            "rotating a credential must not invalidate the checkpoint"
        );
        assert!(
            !conservative.contains("sk-first"),
            "no secret material in the identity: {conservative}"
        );

        std::env::remove_var(MODE);
        std::env::remove_var(TOKEN);
        std::env::remove_var(sharpebench_sim::AGENT_ENV_PASSTHROUGH);
    }

    #[test]
    fn checkpoint_contract_rejects_noncanonical_artifact_digests() {
        for invalid in ["abc".to_string(), "A".repeat(64), "g".repeat(64)] {
            let values = args(&["sharpebench", "run", "--entrant-sha256", &invalid]);
            let data = sharpebench_sim::Dataset::synthetic(2, 12, 7);
            let windows = [sharpebench_sim::Window { start: 2, end: 12 }];
            let error = checkpoint_contract(
                &values,
                CheckpointExecution {
                    data: &data,
                    windows: &windows,
                    seeds: &[3],
                    costs: sharpebench_sim::CostModel::default(),
                    score_config: &ScoreConfig::default(),
                    max_retries: 2,
                    rate_card: None,
                    fault_plan: None,
                    backoff: &BackoffSchedule::immediate(),
                },
                b"entrant",
                true,
            )
            .expect_err("noncanonical digests must be refused");
            assert!(error.contains("64 lowercase hexadecimal"), "{error}");
        }
    }

    #[test]
    fn exhausted_runtime_cells_make_an_external_sweep_noncertifying() {
        let mut failures = sharpebench_harness::FailureLog::default();
        for seed in 0..8 {
            failures.push(sharpebench_harness::FailureRecord {
                window_index: 1,
                seed,
                kind: sharpebench_harness::FailureKind::TransportError,
                attempts: 3,
                runtime: true,
            });
        }
        let status = external_sweep_completeness(&failures, 16, 8);
        assert!(!status.complete);
        assert_eq!(status.expected_cells, 16);
        assert_eq!(status.completed_cells, 8);
        assert_eq!(status.runtime_failed_cells, 8);
    }

    #[test]
    fn agent_fault_sentinels_preserve_the_execution_denominator() {
        let mut failures = sharpebench_harness::FailureLog::default();
        failures.push(sharpebench_harness::FailureRecord {
            window_index: 1,
            seed: 7,
            kind: sharpebench_harness::FailureKind::AgentProtocolViolation,
            attempts: 1,
            runtime: false,
        });
        let status = external_sweep_completeness(&failures, 16, 16);
        assert!(status.complete);
        assert_eq!(status.runtime_failed_cells, 0);
        assert_eq!(status.agent_failed_cells, 1);
    }
}
