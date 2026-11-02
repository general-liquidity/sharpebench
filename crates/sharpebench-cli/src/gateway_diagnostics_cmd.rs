//! `sharpebench gateway-diagnostics`: how an entrant struggled at the model
//! gateway, read from a sweep's money journal.
//!
//! The report ([`sharpebench_harness::gateway_diagnostics`]) counts calls,
//! retries, requests refused by the per-decision limit, faulted calls by cause,
//! truncated answers and tokens per decision, and breaks the decisions down by
//! how many requests and calls each made and the calls by route alias. It never
//! calls a provider and takes no lock, so a sweep in flight can be read. It is
//! rank-neutral: no score, rank, gate or pass^k pool reads it.

use sharpebench_harness::gateway_diagnostics::{gateway_diagnostics, GatewayDiagnostics};
use sharpebench_harness::gateway_journal::GatewayJournal;

const USAGE: &str = "usage: sharpebench gateway-diagnostics --journal <journal.json> [--json]";

fn histogram(counts: &std::collections::BTreeMap<u32, u64>) -> String {
    counts
        .iter()
        .map(|(k, n)| format!("{k}:{n}"))
        .collect::<Vec<_>>()
        .join(" ")
}

fn print(d: &GatewayDiagnostics) {
    println!("SharpeBench gateway diagnostics (rank-neutral; read by no score or gate)");
    if let Some(sweep) = &d.sweep_sha256 {
        println!("sweep {sweep}");
    }
    println!(
        "attempts {}, decisions {} ({} with requests)",
        d.attempts, d.decisions, d.decisions_with_requests
    );
    println!(
        "requests {}, served {}, refused by the per-decision limit {} (on {} decisions)",
        d.requests, d.served, d.refused_by_limit, d.decisions_refused_by_limit
    );
    println!(
        "calls {}, priced {}, faulted {} (on {} decisions), unlogged {}",
        d.calls, d.priced_calls, d.faulted_calls, d.decisions_with_faults, d.unlogged_calls
    );
    let f = &d.faults;
    println!(
        "faults: usage_absent {}, ambiguous_after_commit {}, adapter_deadline_exceeded {}, \
         interrupted_before_settlement {}, never_dispatched {}, provider_refused_before_work {}, \
         unsettled {}",
        f.usage_absent,
        f.ambiguous_after_commit,
        f.adapter_deadline_exceeded,
        f.interrupted_before_settlement,
        f.never_dispatched,
        f.provider_refused_before_work,
        f.unsettled
    );
    println!(
        "retries {}..{} (decisions retried {}..{}), truncated answers {}",
        d.retries_at_least,
        d.retries_at_most,
        d.decisions_retried_at_least,
        d.decisions_retried_at_most,
        d.truncated_answers
    );
    println!(
        "requests per decision {} (max {})",
        histogram(&d.requests_per_decision),
        d.max_requests_in_a_decision
    );
    println!(
        "calls per decision {} (max {})",
        histogram(&d.calls_per_decision),
        d.max_calls_in_a_decision
    );
    let t = &d.tokens_per_decision;
    let mean = |m: Option<f64>| m.map_or_else(|| "n/a".to_string(), |v| format!("{v:.1}"));
    println!(
        "tokens per decision over {} measured ({} unmeasured): input mean {} max {}, output mean {} max {}",
        t.measured_decisions,
        t.unmeasured_decisions,
        mean(t.input_mean),
        t.input_max,
        mean(t.output_mean),
        t.output_max
    );
    for alias in &d.by_alias {
        let tokens = match (alias.input_tokens, alias.output_tokens) {
            (Some(i), Some(o)) => format!("{i} in / {o} out"),
            _ => "unmeasured".to_string(),
        };
        println!(
            "alias {}: calls {}, priced {}, faulted {}, truncated {}, tokens {}",
            alias.alias,
            alias.calls,
            alias.priced_calls,
            alias.faulted_calls,
            alias.truncated_answers,
            tokens
        );
    }
}

/// Exit 0 with the report, 2 on a usage error, 1 when the journal cannot be
/// read or does not validate.
pub fn run(args: &[String], json: bool) -> i32 {
    let journal = match args.iter().position(|a| a == "--journal") {
        Some(i) => match args.get(i + 1).filter(|v| !v.starts_with("--")) {
            Some(path) => path,
            None => {
                eprintln!("{USAGE}");
                return 2;
            }
        },
        None => {
            eprintln!("{USAGE}");
            return 2;
        }
    };
    let journal = match GatewayJournal::load_for_inspection(std::path::Path::new(journal)) {
        Ok(journal) => journal,
        Err(error) => {
            eprintln!("error: cannot read gateway journal {journal}: {error}");
            return 1;
        }
    };
    let report = gateway_diagnostics(&journal);
    if json {
        crate::emit_json(&report);
    } else {
        print(&report);
    }
    0
}
