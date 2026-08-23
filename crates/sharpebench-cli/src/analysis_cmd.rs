//! Standalone analysis subcommands over library modules that previously had no
//! CLI surface: percentile selection, disqualification-reason taxonomy,
//! rediscovery screening, uncertainty decomposition, and the crowding decay
//! prior. Dispatch is wired from `main.rs`; this module owns everything else.
//!
//! Honest-framing invariants carried over from the library docs:
//! - the epistemic leg of the uncertainty decomposition is a lower bound, and
//!   the output text says so;
//! - the crowding half-life is a model prior, reported never gating;
//! - the rediscovery screen flags collinearity (leverage and sign variants
//!   included), not mere correlation;
//! - the selection alpha warns below the recommended floor but never vetoes.

use sharpebench_core::calibration::decompose_uncertainty;
use sharpebench_core::decay::{compare_decay_to_prior, CrowdingParams};
use sharpebench_core::selection::{
    percentile_selection, PercentileSelection, Utility, DEFAULT_SELECTION_ALPHA,
    MIN_RECOMMENDED_SELECTION_ALPHA,
};
use sharpebench_core::{
    classify_disqualification, classify_rediscovery, score_agent, AgentSubmission,
    DisqualThresholds, FailReason, ScoreConfig, DEFAULT_REDISCOVERY_THRESHOLD,
};

/// Run one analysis subcommand. `args` is the full argv with `--json` already
/// stripped, exactly as `main` builds it (`args[0]` = program, `args[1]` =
/// subcommand, operands from `args[2]`). Returns the process exit code:
/// 0 = computed, 1 = I/O or parse failure, 2 = usage error.
pub fn run(subcommand: &str, args: &[String], json: bool) -> i32 {
    match subcommand {
        "select" => run_select(args, json),
        "disqualify" => run_disqualify(args, json),
        "rediscover" => run_rediscover(args, json),
        "uncertainty" => run_uncertainty(args, json),
        "decay-prior" => run_decay_prior(args, json),
        other => {
            eprintln!("unknown analysis command: {other}");
            2
        }
    }
}

// --- shared plumbing ---------------------------------------------------------

/// Value following a `--flag` in argv, if present. Same idiom as `main.rs`.
fn flag_value<'a>(args: &'a [String], flag: &str) -> Option<&'a str> {
    args.iter()
        .position(|a| a == flag)
        .and_then(|i| args.get(i + 1))
        .map(String::as_str)
}

/// Every value following any occurrence of a repeatable `--flag`.
fn flag_values<'a>(args: &'a [String], flag: &str) -> Vec<&'a str> {
    args.iter()
        .enumerate()
        .filter(|(_, a)| *a == flag)
        .filter_map(|(i, _)| args.get(i + 1))
        .map(String::as_str)
        .collect()
}

/// Parse a required numeric flag; `Err` carries the usage message.
fn parse_f64_flag(args: &[String], flag: &str) -> Result<f64, String> {
    let raw = flag_value(args, flag).ok_or_else(|| format!("{flag} <number> is required"))?;
    raw.parse::<f64>()
        .map_err(|_| format!("{flag} must be a number, got `{raw}`"))
}

/// Parse an optional numeric flag with a default.
fn parse_f64_flag_or(args: &[String], flag: &str, default: f64) -> Result<f64, String> {
    match flag_value(args, flag) {
        None => Ok(default),
        Some(raw) => raw
            .parse::<f64>()
            .map_err(|_| format!("{flag} must be a number, got `{raw}`")),
    }
}

/// Read a single column of per-period numbers from CSV text. With `col = None`
/// the first column is used (a header row is skipped if its first cell is
/// non-numeric); with `Some(name)` the column under that header is read.
/// Duplicated from `main.rs` (which owns its own private copy) so this module
/// stays standalone.
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

/// Read every column of a CSV as a named numeric series. A first row whose cells
/// are all non-numeric is treated as the header; otherwise columns are named
/// `col0`, `col1`, ... Empty cells are skipped per column.
fn read_all_numeric_columns(text: &str) -> Result<Vec<(String, Vec<f64>)>, String> {
    let mut lines = text.lines().filter(|l| !l.trim().is_empty());
    let Some(first) = lines.next() else {
        return Err("empty file".to_string());
    };
    let first_cells: Vec<&str> = first.split(',').map(str::trim).collect();
    let has_header = first_cells
        .iter()
        .all(|c| !c.is_empty() && c.parse::<f64>().is_err());
    let names: Vec<String> = if has_header {
        first_cells.iter().map(|c| (*c).to_string()).collect()
    } else {
        (0..first_cells.len()).map(|i| format!("col{i}")).collect()
    };
    let mut columns: Vec<Vec<f64>> = vec![Vec::new(); names.len()];
    let body = if has_header { Vec::new() } else { vec![first] };
    for line in body.into_iter().chain(lines) {
        for (i, cell) in line.split(',').map(str::trim).enumerate() {
            if i >= columns.len() || cell.is_empty() {
                continue;
            }
            let v = cell
                .parse::<f64>()
                .map_err(|_| format!("non-numeric value `{cell}` in column `{}`", names[i]))?;
            columns[i].push(v);
        }
    }
    Ok(names.into_iter().zip(columns).collect())
}

fn read_file(path: &str) -> Result<String, String> {
    std::fs::read_to_string(path).map_err(|e| format!("cannot read {path}: {e}"))
}

/// Print a value as pretty JSON to stdout (machine-readable mode).
fn emit_json<T: serde::Serialize>(value: &T) {
    match serde_json::to_string_pretty(value) {
        Ok(j) => println!("{j}"),
        Err(e) => eprintln!("error: serializing output: {e}"),
    }
}

// --- select ------------------------------------------------------------------

const EPISTEMIC_CAVEAT: &str = "the epistemic leg is a lower bound, never an upper one: \
unanimous or correlated signals understate it, so a low reading is weak evidence of knowledge \
and only a high reading is informative";

const DECAY_PRIOR_NOTE: &str = "the expected half-life is a model prior, reported never gating: \
it comes out of a crowding model, not out of a dataset, and nothing ranks on it";

/// Resolve the candidate set for `select`: several files (one candidate each,
/// first column) or exactly one file whose columns are the candidates.
fn load_candidates(paths: &[&String]) -> Result<Vec<(String, Vec<f64>)>, String> {
    if paths.len() == 1 {
        let text = read_file(paths[0])?;
        let cols = read_all_numeric_columns(&text)?;
        if cols.len() > 1 {
            return Ok(cols);
        }
        // Single column: fall through to the per-file reader for the header
        // heuristics shared with `check` / `regime`.
    }
    let mut out = Vec::with_capacity(paths.len());
    for p in paths {
        let text = read_file(p)?;
        let returns = read_returns_column(&text, None).map_err(|e| format!("{p}: {e}"))?;
        out.push(((*p).clone(), returns));
    }
    Ok(out)
}

fn select_to_json(s: &PercentileSelection, names: &[String], utility: &str) -> serde_json::Value {
    serde_json::json!({
        "alpha": s.alpha,
        "alpha_warning": s.alpha_warning,
        "utility": utility,
        "candidates": s.candidates.iter().map(|c| serde_json::json!({
            "index": c.index,
            "name": names[c.index],
            "point_utility": c.point_utility,
            "percentile_utility": c.percentile_utility,
            "optimism_gap": c.optimism_gap,
        })).collect::<Vec<_>>(),
        "selected": s.selected,
        "point_argmax": s.point_argmax,
        "agrees_with_point_argmax": s.agrees_with_point_argmax,
        "point_winner_optimism": s.point_winner_optimism,
    })
}

/// `select <candidates.csv...>`: rank candidates on a percentile of their
/// bootstrapped utility instead of the point-estimate argmax.
fn run_select(args: &[String], json: bool) -> i32 {
    let paths: Vec<&String> = args
        .iter()
        .skip(2)
        .take_while(|a| !a.starts_with("--"))
        .collect();
    if paths.is_empty() {
        eprintln!(
            "usage: sharpebench select <candidates.csv...> [--alpha A] [--utility mean_return|sharpe] [--seed N] [--boot N] [--block-prob P] [--json]\n\
             one file per candidate (first column), or one file with one column per candidate"
        );
        return 2;
    }
    let alpha = match parse_f64_flag_or(args, "--alpha", DEFAULT_SELECTION_ALPHA) {
        Ok(a) => a,
        Err(e) => {
            eprintln!("error: {e}");
            return 2;
        }
    };
    let utility_name = flag_value(args, "--utility").unwrap_or("mean_return");
    let utility = match utility_name {
        "mean_return" => Utility::MeanReturn,
        "sharpe" => Utility::Sharpe,
        other => {
            eprintln!("error: --utility must be mean_return or sharpe, got `{other}`");
            return 2;
        }
    };
    let seed = match flag_value(args, "--seed").map(str::parse::<u64>) {
        None => 0,
        Some(Ok(s)) => s,
        Some(Err(_)) => {
            eprintln!("error: --seed must be a non-negative integer");
            return 2;
        }
    };
    let n_boot = match flag_value(args, "--boot").map(str::parse::<usize>) {
        None => 2000,
        Some(Ok(n)) => n,
        Some(Err(_)) => {
            eprintln!("error: --boot must be a non-negative integer");
            return 2;
        }
    };
    let block_prob = match parse_f64_flag_or(args, "--block-prob", 0.1) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("error: {e}");
            return 2;
        }
    };

    let named = match load_candidates(&paths) {
        Ok(n) => n,
        Err(e) => {
            eprintln!("error: {e}");
            return 1;
        }
    };
    let names: Vec<String> = named.iter().map(|(n, _)| n.clone()).collect();
    let series: Vec<Vec<f64>> = named.into_iter().map(|(_, s)| s).collect();
    let s = percentile_selection(&series, utility, alpha, seed, n_boot, block_prob);

    if s.alpha_warning {
        eprintln!(
            "warning: alpha {:.2} sits below the recommended floor {MIN_RECOMMENDED_SELECTION_ALPHA}; \
             the extreme lower tail is decided by a handful of unlucky resamples nobody has real \
             data for. The result is still computed: this flags a choice, it does not veto one.",
            s.alpha
        );
    }
    if json {
        emit_json(&select_to_json(&s, &names, utility_name));
        return 0;
    }
    println!(
        "Percentile selection (alpha={:.2}, utility={utility_name}, seed={seed}, n_boot={n_boot}, block_prob={block_prob})\n",
        s.alpha
    );
    println!(
        "{:<4} {:<24} {:>14} {:>14} {:>14}",
        "#", "candidate", "point", "percentile", "optimism_gap"
    );
    for c in &s.candidates {
        println!(
            "{:<4} {:<24} {:>14.6} {:>14.6} {:>14.6}",
            c.index, names[c.index], c.point_utility, c.percentile_utility, c.optimism_gap
        );
    }
    let name_of = |i: Option<usize>| i.map_or("none".to_string(), |i| names[i].clone());
    println!("\npoint winner      : {}", name_of(s.point_argmax));
    println!("percentile winner : {}", name_of(s.selected));
    if s.agrees_with_point_argmax {
        println!("AGREE: the point argmax survives resampling.");
    } else {
        println!(
            "DISAGREE: the observed path ranks a candidate that does not hold up across the \
             resampled histories it is consistent with."
        );
    }
    println!(
        "point winner optimism gap: {:.6} (how much of the headline utility fails to survive resampling)",
        s.point_winner_optimism
    );
    0
}

// --- disqualify --------------------------------------------------------------

/// The advisory (non-gating) reasons; everything else mirrors a hard
/// eligibility gate in the scorer.
fn is_advisory(reason: FailReason) -> bool {
    matches!(
        reason,
        FailReason::HighSelectionGap | FailReason::IsRediscovery | FailReason::OosDecay
    )
}

/// `disqualify <subs.json>`: score a field of submissions and name every
/// disqualification/quality signal that fired for each agent. Pure legibility
/// over the composite score: nothing here changes eligibility semantics.
fn run_disqualify(args: &[String], json: bool) -> i32 {
    let Some(path) = args.get(2).filter(|p| !p.starts_with('-')) else {
        eprintln!("usage: sharpebench disqualify <submissions.json> [--json]");
        return 2;
    };
    let data = match read_file(path) {
        Ok(d) => d,
        Err(e) => {
            eprintln!("error: {e}");
            return 1;
        }
    };
    let subs: Vec<AgentSubmission> = match serde_json::from_str(&data) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("error: invalid submissions JSON: {e}");
            return 1;
        }
    };
    let cfg = ScoreConfig::default();
    let thresholds = DisqualThresholds::from_score_config(&cfg);
    let classified: Vec<(String, bool, Vec<FailReason>)> = subs
        .iter()
        .map(|sub| {
            let score = score_agent(sub, &cfg);
            let reasons = classify_disqualification(&score, &thresholds, None, None);
            (score.agent_id.clone(), score.rank_eligible, reasons)
        })
        .collect();

    if json {
        let out: Vec<serde_json::Value> = classified
            .iter()
            .map(|(id, eligible, reasons)| {
                serde_json::json!({
                    "agent_id": id,
                    "rank_eligible": eligible,
                    "reasons": reasons,
                })
            })
            .collect();
        emit_json(&out);
        return 0;
    }
    println!(
        "Disqualification reasons (hard gates mirror the scorer; advisory flags never gate)\n"
    );
    for (id, eligible, reasons) in &classified {
        if reasons.is_empty() {
            println!("{id}: clear (no signal fired), rank-eligible={eligible}");
            continue;
        }
        let listed: Vec<String> = reasons
            .iter()
            .map(|r| {
                if is_advisory(*r) {
                    format!("{r:?} (advisory)")
                } else {
                    format!("{r:?}")
                }
            })
            .collect();
        println!("{id}: {} , rank-eligible={eligible}", listed.join(", "));
    }
    0
}

// --- rediscover --------------------------------------------------------------

/// `rediscover <submitted.csv> <known.csv...>`: screen a submitted pooled return
/// stream against a library of known prior strategy streams. Flags collinearity
/// on |cosine| (leverage and inverted variants included); merely-correlated but
/// distinct strategies are not flagged.
fn run_rediscover(args: &[String], json: bool) -> i32 {
    let paths: Vec<&String> = args
        .iter()
        .skip(2)
        .take_while(|a| !a.starts_with("--"))
        .collect();
    if paths.len() < 2 {
        eprintln!(
            "usage: sharpebench rediscover <submitted.csv> <known.csv...> [--threshold T] [--center] [--json]"
        );
        return 2;
    }
    let threshold = match parse_f64_flag_or(args, "--threshold", DEFAULT_REDISCOVERY_THRESHOLD) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("error: {e}");
            return 2;
        }
    };
    let center = args.iter().any(|a| a == "--center");

    let submitted = match read_file(paths[0]).and_then(|t| read_returns_column(&t, None)) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("error: {}: {e}", paths[0]);
            return 1;
        }
    };
    let mut known = Vec::with_capacity(paths.len() - 1);
    for p in &paths[1..] {
        match read_file(p).and_then(|t| read_returns_column(&t, None)) {
            Ok(s) => known.push(s),
            Err(e) => {
                eprintln!("error: {p}: {e}");
                return 1;
            }
        }
    }
    let v = classify_rediscovery(&submitted, &known, threshold, center);
    let nearest_name = v.nearest_index.map(|i| paths[1 + i].clone());

    if json {
        emit_json(&serde_json::json!({
            "is_rediscovery": v.is_rediscovery,
            "max_similarity": v.max_similarity,
            "nearest_index": v.nearest_index,
            "nearest_known": nearest_name,
            "threshold": v.threshold,
            "centered": center,
        }));
        return 0;
    }
    println!(
        "Rediscovery screen (threshold {:.3} on |cosine|, {})\n",
        v.threshold,
        if center {
            "centered/Pearson"
        } else {
            "uncentered direction"
        }
    );
    println!("max |cosine|  : {:.6}", v.max_similarity);
    match &nearest_name {
        Some(name) => println!("nearest known : {name}"),
        None => println!("nearest known : none (empty library or no defined similarity)"),
    }
    if v.is_rediscovery {
        println!("verdict       : REDISCOVERY (the submitted stream is all but collinear with a known one)");
    } else {
        println!("verdict       : novel at this threshold");
    }
    println!(
        "\nnote: leveraged and inverted variants of a known stream flag too (|cosine|); \
         correlated-but-distinct strategies do not. Novelty screening only: this says nothing \
         about whether the stream is skilled."
    );
    0
}

// --- uncertainty -------------------------------------------------------------

/// `uncertainty <returns.csv> [--reference <csv>] [--outcomes <csv>]
/// [--confidences <csv>]...`: decompose the uncertainty behind one scored case
/// into its aleatoric, epistemic and distributional legs. The legs are reported
/// side by side, never summed, and the epistemic leg is a lower bound.
fn run_uncertainty(args: &[String], json: bool) -> i32 {
    let Some(case_path) = args.get(2).filter(|p| !p.starts_with('-')) else {
        eprintln!(
            "usage: sharpebench uncertainty <returns.csv> [--reference <csv>] [--outcomes <csv>] [--confidences <csv>]... [--json]\n\
             --reference: reference return series (distributional leg)\n\
             --outcomes: 0/1 per decision, 1 = the call was right (aleatoric leg)\n\
             --confidences: one independent confidence stream per flag, repeatable (epistemic leg)"
        );
        return 2;
    };
    let case_returns = match read_file(case_path).and_then(|t| read_returns_column(&t, None)) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("error: {case_path}: {e}");
            return 1;
        }
    };
    let reference = match flag_value(args, "--reference") {
        None => Vec::new(),
        Some(p) => match read_file(p).and_then(|t| read_returns_column(&t, None)) {
            Ok(r) => r,
            Err(e) => {
                eprintln!("error: {p}: {e}");
                return 1;
            }
        },
    };
    let outcomes: Vec<bool> = match flag_value(args, "--outcomes") {
        None => Vec::new(),
        Some(p) => match read_file(p).and_then(|t| read_returns_column(&t, None)) {
            Ok(r) => r.into_iter().map(|v| v != 0.0).collect(),
            Err(e) => {
                eprintln!("error: {p}: {e}");
                return 1;
            }
        },
    };
    let mut signal_series: Vec<Vec<f64>> = Vec::new();
    for p in flag_values(args, "--confidences") {
        match read_file(p).and_then(|t| read_returns_column(&t, None)) {
            Ok(s) => signal_series.push(s),
            Err(e) => {
                eprintln!("error: {p}: {e}");
                return 1;
            }
        }
    }
    let signals: Vec<&[f64]> = signal_series.iter().map(Vec::as_slice).collect();
    let split = decompose_uncertainty(&outcomes, &signals, &case_returns, &reference);

    if json {
        emit_json(&serde_json::json!({
            "aleatoric": split.aleatoric,
            "epistemic": split.epistemic,
            "distributional": split.distributional,
            "outcomes_supplied": !outcomes.is_empty(),
            "signal_streams": signals.len(),
            "reference_supplied": reference.len() >= 2,
            "epistemic_caveat": EPISTEMIC_CAVEAT,
        }));
        return 0;
    }
    println!("Uncertainty decomposition (three legs, reported side by side, never summed)\n");
    let aleatoric_note = if outcomes.is_empty() {
        " (no --outcomes supplied: nothing measured, not evidence of low noise)"
    } else {
        " (irreducible outcome noise; more evidence will not reduce it)"
    };
    println!("aleatoric      : {:.4}{aleatoric_note}", split.aleatoric);
    let epistemic_note = match signals.len() {
        0 => " (no --confidences supplied: thinness alone, read as unknown)",
        1 => " (one stream: disagreement is not measurable, thinness alone)",
        _ => " (signal disagreement plus evidence thinness; reducible by more evidence)",
    };
    println!("epistemic      : {:.4}{epistemic_note}", split.epistemic);
    let distributional_note = if reference.len() < 2 {
        " (no usable --reference supplied: novelty was not measured, not absent)"
    } else {
        " (unlikeness to the reference series: location or dispersion shift)"
    };
    println!(
        "distributional : {:.4}{distributional_note}",
        split.distributional
    );
    println!(
        "\nhigh aleatoric says stop looking; high epistemic says keep looking; high \
         distributional says the reference cannot vouch for this case."
    );
    println!("caveat: {EPISTEMIC_CAVEAT}.");
    0
}

// --- decay-prior -------------------------------------------------------------

/// `decay-prior --measured-ic <csv> --adoption X --theta Y --delta-max Z`:
/// measure the edge's half-life from its IC series and set it against the
/// crowding model's expected half-life. The prior is a model output, reported
/// never gating; all rates are per period of the supplied IC series.
fn run_decay_prior(args: &[String], json: bool) -> i32 {
    let usage = "usage: sharpebench decay-prior --measured-ic <ic.csv> --adoption X --theta Y --delta-max Z [--curvature C] [--anomaly-ratio R] [--json]\n\
                 theta / delta-max are per period of the IC series; there is no default calibration on purpose";
    let Some(ic_path) = flag_value(args, "--measured-ic") else {
        eprintln!("{usage}");
        return 2;
    };
    let (adoption, theta, delta_max) = match (
        parse_f64_flag(args, "--adoption"),
        parse_f64_flag(args, "--theta"),
        parse_f64_flag(args, "--delta-max"),
    ) {
        (Ok(a), Ok(t), Ok(d)) => (a, t, d),
        (Err(e), _, _) | (_, Err(e), _) | (_, _, Err(e)) => {
            eprintln!("error: {e}\n{usage}");
            return 2;
        }
    };
    let (curvature, anomaly_ratio) = match (
        parse_f64_flag_or(args, "--curvature", 1.0),
        parse_f64_flag_or(args, "--anomaly-ratio", 0.5),
    ) {
        (Ok(c), Ok(r)) => (c, r),
        (Err(e), _) | (_, Err(e)) => {
            eprintln!("error: {e}");
            return 2;
        }
    };
    let ic = match read_file(ic_path).and_then(|t| read_returns_column(&t, None)) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("error: {ic_path}: {e}");
            return 1;
        }
    };
    let params = CrowdingParams {
        theta,
        delta_max,
        curvature,
    };
    let c = compare_decay_to_prior(&ic, adoption, params, anomaly_ratio);

    if json {
        emit_json(&serde_json::json!({
            "measured_half_life": c.measured_half_life,
            "prior": c.prior,
            "ratio": c.ratio,
            "anomalous": c.anomalous,
            "anomaly_ratio": anomaly_ratio,
            "note": DECAY_PRIOR_NOTE,
        }));
        return 0;
    }
    println!("Edge decay vs crowding prior (all rates per period of the IC series)\n");
    match c.measured_half_life {
        Some(h) => println!("measured half-life : {h:.4} periods"),
        None => println!(
            "measured half-life : none (series too short, or not decaying; nothing to penalize)"
        ),
    }
    match c.prior.expected_half_life {
        Some(h) => println!("expected half-life : {h:.4} periods (model prior, reported never gating)"),
        None => println!(
            "expected half-life : none (non-positive total hazard: the model says the edge never decays)"
        ),
    }
    println!(
        "prior pieces       : theta={:.4} (natural reversion), delta(phi)={:.4} at adoption {:.2}",
        c.prior.natural_reversion, c.prior.crowding_decay, c.prior.adoption
    );
    match c.ratio {
        Some(r) => println!("measured/expected  : {r:.4}"),
        None => println!("measured/expected  : undefined (one side missing)"),
    }
    if c.anomalous {
        println!(
            "ANOMALOUS (< {anomaly_ratio:.2}): the decay is too fast for crowding to be the whole \
             story; look for overfitting, a broken data pipeline, or a regime the strategy was \
             never fit for. A diagnostic, not a verdict."
        );
    } else {
        println!("not anomalous at ratio threshold {anomaly_ratio:.2}");
    }
    println!("\nnote: {DECAY_PRIOR_NOTE}.");
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Write `contents` to a unique file under a per-process temp dir.
    fn temp_file(name: &str, contents: &str) -> String {
        let dir = std::env::temp_dir().join(format!(
            "sharpebench-analysis-cmd-tests-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).expect("create temp dir");
        let path = dir.join(name);
        std::fs::write(&path, contents).expect("write temp file");
        path.to_string_lossy().into_owned()
    }

    /// Build the argv shape `run` documents: program, subcommand, operands.
    fn argv(subcommand: &str, rest: &[&str]) -> Vec<String> {
        let mut v = vec!["sharpebench".to_string(), subcommand.to_string()];
        v.extend(rest.iter().map(|s| (*s).to_string()));
        v
    }

    fn csv_of(values: &[f64]) -> String {
        values
            .iter()
            .map(f64::to_string)
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn unknown_subcommand_is_usage_error() {
        assert_eq!(run("nonsense", &argv("nonsense", &[]), false), 2);
    }

    #[test]
    fn select_requires_candidates() {
        assert_eq!(run("select", &argv("select", &[]), false), 2);
    }

    #[test]
    fn select_rejects_bad_flags() {
        let f = temp_file("sel-bad-flags.csv", &csv_of(&[0.01, 0.02, 0.01]));
        let bad_alpha = argv("select", &[&f, "--alpha", "abc"]);
        assert_eq!(run("select", &bad_alpha, false), 2);
        let bad_utility = argv("select", &[&f, "--utility", "sortino"]);
        assert_eq!(run("select", &bad_utility, false), 2);
        let bad_seed = argv("select", &[&f, "--seed", "-1"]);
        assert_eq!(run("select", &bad_seed, false), 2);
        let bad_boot = argv("select", &[&f, "--boot", "many"]);
        assert_eq!(run("select", &bad_boot, false), 2);
    }

    #[test]
    fn select_happy_path_over_two_files() {
        let steady: Vec<f64> = (0..60)
            .map(|i| 0.003 + 0.0001 * (i as f64 * 0.9).sin())
            .collect();
        let mut spiky = vec![0.0005; 60];
        spiky[30] = 0.35;
        let a = temp_file("sel-steady.csv", &csv_of(&steady));
        let b = temp_file("sel-spiky.csv", &csv_of(&spiky));
        let args = argv("select", &[&a, &b, "--alpha", "0.4", "--boot", "500"]);
        assert_eq!(run("select", &args, false), 0);
        assert_eq!(run("select", &args, true), 0);
    }

    #[test]
    fn select_happy_path_over_one_multi_column_file() {
        let mut rows = String::from("steady,spiky\n");
        for i in 0..40 {
            let spike = if i == 20 { 0.3 } else { 0.0005 };
            rows.push_str(&format!("{},{spike}\n", 0.003 + 0.0001 * f64::from(i)));
        }
        let f = temp_file("sel-columns.csv", &rows);
        let args = argv("select", &[&f, "--boot", "300"]);
        assert_eq!(run("select", &args, false), 0);
    }

    #[test]
    fn select_missing_file_is_failure() {
        let args = argv("select", &["no-such-file-anywhere.csv"]);
        assert_eq!(run("select", &args, false), 1);
    }

    #[test]
    fn disqualify_requires_a_path_and_valid_json() {
        assert_eq!(run("disqualify", &argv("disqualify", &[]), false), 2);
        let bad = temp_file("disq-bad.json", "not json");
        assert_eq!(run("disqualify", &argv("disqualify", &[&bad]), false), 1);
    }

    #[test]
    fn disqualify_happy_path() {
        let steady: Vec<f64> = (0..60)
            .map(|i| 0.002 + 0.0005 * (i as f64 * 0.7).sin())
            .collect();
        let noisy: Vec<f64> = (0..60).map(|i| 0.02 * (i as f64 * 0.7).sin()).collect();
        let subs = serde_json::json!([
            { "agent_id": "steady", "runs": (0..5).map(|_| serde_json::json!({"returns": steady})).collect::<Vec<_>>() },
            { "agent_id": "noise", "runs": (0..5).map(|_| serde_json::json!({"returns": noisy})).collect::<Vec<_>>() },
        ]);
        let f = temp_file("disq-subs.json", &subs.to_string());
        assert_eq!(run("disqualify", &argv("disqualify", &[&f]), false), 0);
        assert_eq!(run("disqualify", &argv("disqualify", &[&f]), true), 0);
    }

    #[test]
    fn rediscover_requires_a_submission_and_a_library() {
        assert_eq!(run("rediscover", &argv("rediscover", &[]), false), 2);
        let one = temp_file("red-one.csv", &csv_of(&[0.01, 0.02]));
        assert_eq!(run("rediscover", &argv("rediscover", &[&one]), false), 2);
    }

    #[test]
    fn rediscover_happy_path_flags_a_leveraged_clone() {
        let known: Vec<f64> = (0..60)
            .map(|i| (i as f64 * 0.37).sin() * 0.01 + 0.001)
            .collect();
        let submitted: Vec<f64> = known.iter().map(|x| x * 2.0).collect();
        let k = temp_file("red-known.csv", &csv_of(&known));
        let s = temp_file("red-submitted.csv", &csv_of(&submitted));
        let args = argv("rediscover", &[&s, &k]);
        assert_eq!(run("rediscover", &args, false), 0);
        assert_eq!(run("rediscover", &args, true), 0);
        let bad_threshold = argv("rediscover", &[&s, &k, "--threshold", "abc"]);
        assert_eq!(run("rediscover", &bad_threshold, false), 2);
    }

    #[test]
    fn uncertainty_requires_case_returns() {
        assert_eq!(run("uncertainty", &argv("uncertainty", &[]), false), 2);
        let args = argv("uncertainty", &["no-such-file.csv"]);
        assert_eq!(run("uncertainty", &args, false), 1);
    }

    #[test]
    fn uncertainty_happy_path_with_all_legs() {
        let case: Vec<f64> = (0..50).map(|i| 0.004 + 0.0005 * (i as f64).sin()).collect();
        let outcomes: Vec<f64> = (0..50).map(|i| f64::from(u8::from(i % 2 == 0))).collect();
        let case_f = temp_file("unc-case.csv", &csv_of(&case));
        let ref_f = temp_file("unc-ref.csv", &csv_of(&case));
        let out_f = temp_file("unc-outcomes.csv", &csv_of(&outcomes));
        let conf_a = temp_file("unc-conf-a.csv", &csv_of(&vec![0.9; 50]));
        let conf_b = temp_file("unc-conf-b.csv", &csv_of(&vec![0.1; 50]));
        let args = argv(
            "uncertainty",
            &[
                &case_f,
                "--reference",
                &ref_f,
                "--outcomes",
                &out_f,
                "--confidences",
                &conf_a,
                "--confidences",
                &conf_b,
            ],
        );
        assert_eq!(run("uncertainty", &args, false), 0);
        assert_eq!(run("uncertainty", &args, true), 0);
    }

    #[test]
    fn uncertainty_runs_without_optional_legs() {
        let case: Vec<f64> = (0..30).map(|i| 0.001 * (i as f64).cos()).collect();
        let case_f = temp_file("unc-case-only.csv", &csv_of(&case));
        assert_eq!(
            run("uncertainty", &argv("uncertainty", &[&case_f]), false),
            0
        );
    }

    #[test]
    fn decay_prior_requires_every_model_rate() {
        assert_eq!(run("decay-prior", &argv("decay-prior", &[]), false), 2);
        let ic = temp_file("decay-ic.csv", &csv_of(&[0.2, 0.18, 0.16]));
        let missing_theta = argv(
            "decay-prior",
            &[
                "--measured-ic",
                &ic,
                "--adoption",
                "0.5",
                "--delta-max",
                "0.05",
            ],
        );
        assert_eq!(run("decay-prior", &missing_theta, false), 2);
        let bad_number = argv(
            "decay-prior",
            &[
                "--measured-ic",
                &ic,
                "--adoption",
                "xyz",
                "--theta",
                "0.05",
                "--delta-max",
                "0.05",
            ],
        );
        assert_eq!(run("decay-prior", &bad_number, false), 2);
    }

    #[test]
    fn decay_prior_happy_path() {
        let ic: Vec<f64> = (0..40).map(|t| 0.2 * (-0.1 * f64::from(t)).exp()).collect();
        let ic_f = temp_file("decay-ic-exp.csv", &csv_of(&ic));
        let args = argv(
            "decay-prior",
            &[
                "--measured-ic",
                &ic_f,
                "--adoption",
                "0.0",
                "--theta",
                "0.05",
                "--delta-max",
                "0.05",
                "--anomaly-ratio",
                "0.6",
            ],
        );
        assert_eq!(run("decay-prior", &args, false), 0);
        assert_eq!(run("decay-prior", &args, true), 0);
    }

    #[test]
    fn flag_helpers_parse_argv() {
        let args = argv(
            "select",
            &[
                "a.csv",
                "--alpha",
                "0.4",
                "--confidences",
                "x",
                "--confidences",
                "y",
            ],
        );
        assert_eq!(flag_value(&args, "--alpha"), Some("0.4"));
        assert_eq!(flag_value(&args, "--missing"), None);
        assert_eq!(flag_values(&args, "--confidences"), vec!["x", "y"]);
        assert_eq!(parse_f64_flag_or(&args, "--alpha", 0.5).unwrap(), 0.4);
        assert_eq!(parse_f64_flag_or(&args, "--missing", 0.5).unwrap(), 0.5);
        assert!(parse_f64_flag(&args, "--missing").is_err());
    }

    #[test]
    fn multi_column_reader_names_and_splits() {
        let cols = read_all_numeric_columns("a,b\n1,2\n3,4\n").unwrap();
        assert_eq!(cols.len(), 2);
        assert_eq!(cols[0], ("a".to_string(), vec![1.0, 3.0]));
        assert_eq!(cols[1], ("b".to_string(), vec![2.0, 4.0]));
        let headerless = read_all_numeric_columns("1,2\n3,4\n").unwrap();
        assert_eq!(headerless[0].0, "col0");
        assert!(read_all_numeric_columns("a,b\n1,zzz\n").is_err());
        assert!(read_all_numeric_columns("").is_err());
    }
}
