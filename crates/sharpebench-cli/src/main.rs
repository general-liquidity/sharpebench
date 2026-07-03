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
        Some("realism") => run_realism(&args, json),
        Some("sign") => run_sign(&args, json),
        Some("verify") => run_verify(&args, json),
        Some("capture") => run_capture(&args, json),
        Some("verify-trajectory") => run_verify_trajectory(&args, json),
        Some("audit-briefing") => run_audit_briefing(&args, json),
        Some("canary") => run_canary(&args, json),
        Some("score-allocation") => run_score_allocation(&args, json),
        Some("greeks") => run_greeks(&args, json),
        Some("check") => run_check(&args, json),
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
    let price = sharpebench_core::bs_price(spot, strike, t, r, vol, is_call);
    let greeks = sharpebench_core::bs_greeks(spot, strike, t, r, vol, is_call);
    let risk =
        sharpebench_core::classify_greeks_risk(&greeks, &sharpebench_core::GreeksPolicy::default());
    if json {
        emit_json(&serde_json::json!({ "price": price, "greeks": greeks, "risk": risk }));
    } else {
        println!("price {price:.4}");
        println!(
            "delta {:.4}  gamma {:.4}  theta {:.4}  vega {:.4}  rho {:.4}",
            greeks.delta, greeks.gamma, greeks.theta, greeks.vega, greeks.rho
        );
        println!(
            "tail-risk: short_gamma={} unbounded_tail={} short_vega={}",
            risk.naked_short_gamma, risk.unbounded_tail, risk.short_vega
        );
    }
    ExitCode::SUCCESS
}

/// `check` — backtest-honesty verdict over a column of per-period returns.
/// `--trials N` is REQUIRED: a single backtest you kept is the survivor of every
/// variant you discarded, so there is no honest default for the search footprint.
fn run_check(args: &[String], json: bool) -> ExitCode {
    use sharpebench_edge::{is_my_sharpe_real, HonestyConfig, Verdict};

    let Some(path) = args.get(2).filter(|p| !p.starts_with('-')) else {
        eprintln!("usage: sharpebench check <returns.csv> --trials N [--col NAME] [--confidence C] [--json]");
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

/// Read a single column of per-period returns from CSV text. With `col = None`
/// the first numeric column is used (a header row is skipped if its first cell is
/// non-numeric); with `Some(name)` the column under that header is read.
fn read_returns_column(text: &str, col: Option<&str>) -> Result<Vec<f64>, String> {
    let mut lines = text.lines().filter(|l| !l.trim().is_empty());
    let Some(first) = lines.next() else {
        return Err("empty file".to_string());
    };
    let header: Vec<&str> = first.split(',').map(str::trim).collect();

    let (col_idx, skip_first) = match col {
        Some(name) => {
            let idx = header
                .iter()
                .position(|h| *h == name)
                .ok_or_else(|| format!("column `{name}` not found in header"))?;
            (idx, true)
        }
        None => {
            // No column named: pick column 0. Skip the first row only if it is a
            // non-numeric header.
            let skip = header.first().map(|c| c.parse::<f64>().is_err()) == Some(true);
            (0, skip)
        }
    };

    let mut out = Vec::new();
    let body = if skip_first { Vec::new() } else { vec![first] };
    for line in body.into_iter().chain(lines) {
        let cell = line
            .split(',')
            .nth(col_idx)
            .map(str::trim)
            .unwrap_or_default();
        if cell.is_empty() {
            continue;
        }
        let v = cell
            .parse::<f64>()
            .map_err(|_| format!("non-numeric value `{cell}` in returns column"))?;
        out.push(v);
    }
    Ok(out)
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
    println!("                       --checkpoint <path>: resumable external-agent sweep (crash-tolerant)");
    println!(
        "  sharpebench score <submissions.json>  rank a JSON field of pre-computed submissions"
    );
    println!(
        "  sharpebench commit <agent> <window> <digest> <salt>  forward-attestation pre-registration"
    );
    println!("  sharpebench stress                    run the adversarial stress suite (masked)");
    println!("  sharpebench audit                     self-audit: prove the scorer resists gaming");
    println!("  sharpebench realism [--data <csv>]    prove a dataset behaves like a market (Cont's stylized facts)");
    println!("  sharpebench sign <subs.json> <key> <out.json>  score + sign a board to a file");
    println!("  sharpebench verify <board.json> <key>  verify a signed board's chain");
    println!(
        "  sharpebench capture <agent> <out.json> [--data <csv>]  capture an agent's raw-decision trajectory artifact"
    );
    println!(
        "  sharpebench verify-trajectory <traj.json> [--data <csv>]  replay a trajectory → recompute its score from raw decisions"
    );
    println!("  sharpebench audit-briefing <briefing.json>  audit a shared briefing for input-side salience bias");
    println!("  sharpebench canary <seed>             derive a do-not-train contamination tripwire token");
    println!(
        "  sharpebench score-allocation <alloc.json>  score a weight-vector trajectory (validity + turnover)"
    );
    println!(
        "  sharpebench greeks <spot> <strike> <t> <r> <vol> <call|put>  Black-Scholes price + Greeks + tail-risk"
    );
    println!(
        "  sharpebench check <returns.csv> --trials N [--col NAME] [--confidence C]  is this Sharpe real? (deflated/MinTRL)"
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

/// Surface external-agent transport failures instead of hiding them: an unrecovered
/// wire blip (runtime) or an agent protocol fault is printed to stderr so the
/// operator sees that some decisions did not come from the agent honestly, rather
/// than a silently-flattened return series.
fn report_transport_failures(label: &str, failures: &sharpebench_harness::FailureLog, json: bool) {
    if failures.is_empty() {
        return;
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
}

fn run_demo(args: &[String], json: bool) -> ExitCode {
    use sharpebench_sim::{
        Agent, BuyAndHold, CostModel, Dataset, ExternalAgent, HttpAgent, Momentum, Window,
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
    // Both go through the transport-honest path: a wire blip is retried and, if it
    // persists, surfaced as an explicit failure instead of a masked degrade-to-hold.
    // `--checkpoint <path>` (external agents only) makes the sweep resumable: a crash
    // mid-run resumes and finishes only the outstanding window × seed tasks.
    const EXTERNAL_MAX_RETRIES: u32 = 2;
    let checkpoint = flag_value(args, "--checkpoint").map(std::path::PathBuf::from);
    if let Some(addr) = flag_value(args, "--http") {
        let addr = addr.to_string();
        let label = format!("http:{addr}");
        let res = if let Some(ckpt) = &checkpoint {
            match sharpebench_harness::run_resumable_sweep(
                ckpt,
                &label,
                &windows,
                &seeds,
                EXTERNAL_MAX_RETRIES,
                |wi, seed| {
                    let mut agent = HttpAgent::new(addr.clone());
                    sharpebench_harness::run_external_backtest(
                        &data,
                        &mut agent,
                        windows[wi],
                        seed,
                        costs,
                    )
                },
            ) {
                Ok(r) => r,
                Err(e) => {
                    eprintln!("error: checkpoint sweep failed: {e}");
                    return ExitCode::FAILURE;
                }
            }
        } else {
            sharpebench_harness::run_external_agent(
                &label,
                &data,
                &windows,
                &seeds,
                costs,
                EXTERNAL_MAX_RETRIES,
                || Some(HttpAgent::new(addr.clone())),
            )
        };
        report_transport_failures(&label, &res.failures, json);
        field.insert(0, res.submission);
    } else if let Some(cmd) = flag_value(args, "--cmd") {
        let parts: Vec<String> = cmd.split_whitespace().map(String::from).collect();
        let Some((prog, rest)) = parts.split_first() else {
            eprintln!("error: --cmd needs a program to run");
            return ExitCode::from(2);
        };
        let prog = prog.clone();
        let rest = rest.to_vec();
        // Pre-flight: fail fast with a clear message if the agent won't spawn at all.
        let rest_refs: Vec<&str> = rest.iter().map(String::as_str).collect();
        if ExternalAgent::spawn(&prog, &rest_refs).is_err() {
            eprintln!("error: cannot spawn agent `{cmd}`");
            return ExitCode::FAILURE;
        }
        let label = format!("cmd:{prog}");
        let res = if let Some(ckpt) = &checkpoint {
            match sharpebench_harness::run_resumable_sweep(
                ckpt,
                &label,
                &windows,
                &seeds,
                EXTERNAL_MAX_RETRIES,
                |wi, seed| {
                    let rest_refs: Vec<&str> = rest.iter().map(String::as_str).collect();
                    match ExternalAgent::spawn(&prog, &rest_refs) {
                        Ok(mut a) => sharpebench_harness::run_external_backtest(
                            &data,
                            &mut a,
                            windows[wi],
                            seed,
                            costs,
                        ),
                        Err(_) => Err(sharpebench_harness::FailureKind::SpawnError),
                    }
                },
            ) {
                Ok(r) => r,
                Err(e) => {
                    eprintln!("error: checkpoint sweep failed: {e}");
                    return ExitCode::FAILURE;
                }
            }
        } else {
            sharpebench_harness::run_external_agent(
                &label,
                &data,
                &windows,
                &seeds,
                costs,
                EXTERNAL_MAX_RETRIES,
                || {
                    let rest_refs: Vec<&str> = rest.iter().map(String::as_str).collect();
                    ExternalAgent::spawn(&prog, &rest_refs).ok()
                },
            )
        };
        report_transport_failures(&label, &res.failures, json);
        field.insert(0, res.submission);
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
