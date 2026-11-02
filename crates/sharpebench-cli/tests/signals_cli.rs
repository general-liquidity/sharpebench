//! `sharpebench signals <field.json>` and `sharpebench score --signals`: the
//! rank-neutral fast-signal report, beside the board and never written into
//! it, and the evaluation tier printed first on every human board.

use std::path::PathBuf;
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};

struct Fixture(PathBuf);

impl Fixture {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        loop {
            let path = std::env::temp_dir().join(format!(
                "sharpebench-signals-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            match std::fs::create_dir(&path) {
                Ok(()) => return Self(path),
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(e) => panic!("create fixture: {e}"),
            }
        }
    }

    fn cli(&self, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_sharpebench"))
            .current_dir(&self.0)
            .args(args)
            .output()
            .unwrap()
    }

    fn write(&self, name: &str, contents: &serde_json::Value) {
        std::fs::write(self.0.join(name), contents.to_string()).unwrap();
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.0).expect("remove exclusively owned fixture");
    }
}

fn stdout(out: Output) -> String {
    assert_eq!(out.status.code(), Some(0), "{out:?}");
    String::from_utf8(out.stdout).unwrap()
}

fn json(out: Output) -> serde_json::Value {
    serde_json::from_str(&stdout(out)).unwrap()
}

/// Two agents: `steady` places one gated order per run and reports a cost,
/// `sloppy` places an ungated order on one run of four.
fn field() -> Fixture {
    let fx = Fixture::new();
    let returns: Vec<f64> = (0..40)
        .map(|i| 0.001 + 0.004 * (i as f64 * 0.7).sin())
        .collect();
    let run = |gated: bool, cost: f64| {
        serde_json::json!({
            "returns": returns,
            "trace": {"events": [{"event": "order_placed", "risk_gate_passed": gated}]},
            "cost": cost,
        })
    };
    fx.write(
        "field.json",
        &serde_json::json!([
            {"agent_id": "steady", "runs": [run(true, 1.0), run(true, 3.0), run(true, 2.0), run(true, 2.0)]},
            {"agent_id": "sloppy", "runs": [run(false, 0.0), run(true, 0.0), run(true, 0.0), run(true, 0.0)]},
        ]),
    );
    fx
}

fn measure<'a>(report: &'a serde_json::Value, agent: &str, id: &str) -> &'a serde_json::Value {
    report["agents"]
        .as_array()
        .unwrap()
        .iter()
        .find(|a| a["agent_id"] == agent)
        .and_then(|a| {
            a["measures"]
                .as_array()
                .unwrap()
                .iter()
                .find(|m| m["id"] == id)
        })
        .unwrap_or_else(|| panic!("{agent} {id}"))
}

#[test]
fn the_signals_command_reports_submission_measures_with_intervals() {
    let fx = field();
    let doc = json(fx.cli(&["signals", "field.json", "--json"]));
    let report = &doc["fast_signals"];
    assert_eq!(report["schema"], "sharpebench.fast-signals.v1");
    assert_eq!(report["rank_input"], false);
    assert_eq!(report["skill_evidence"], false);

    let clean = measure(report, "sloppy", "process_clean_runs");
    assert_eq!(clean["value"], 0.75);
    assert_eq!(clean["count"], 3);
    assert_eq!(clean["observations"], 4);
    assert_eq!(clean["interval"]["method"], "clopper_pearson_exact");
    assert_eq!(
        measure(report, "steady", "process_clean_runs")["value"],
        1.0
    );

    let cost = measure(report, "steady", "cost_per_run");
    assert_eq!(cost["value"], 2.0);
    assert_eq!(cost["source"], "entrant_reported");
    assert_eq!(cost["interval"]["method"], "percentile_bootstrap");
    assert!(measure(report, "sloppy", "cost_per_run")["value"].is_null());

    let human = stdout(fx.cli(&["signals", "field.json"]));
    assert!(
        human.contains("Fast signals (sharpebench.fast-signals.v1)"),
        "{human}"
    );
    assert!(human.contains("\nsloppy (4 runs)\n"), "{human}");
    assert!(
        human.contains("No measure in this report is evidence of trading skill."),
        "{human}"
    );
}

/// `score --signals --json` puts the report beside a board byte-identical to
/// `score --json`.
#[test]
fn score_signals_sits_beside_an_unchanged_board() {
    let fx = field();
    let plain = json(fx.cli(&["score", "field.json", "--json"]));
    let with = json(fx.cli(&["score", "field.json", "--signals", "--json"]));
    let keys: Vec<&str> = with
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    assert_eq!(keys, ["board", "fast_signals"]);
    assert_eq!(with["board"], plain);
    let alone = json(fx.cli(&["signals", "field.json", "--json"]));
    assert_eq!(with["fast_signals"], alone["fast_signals"]);

    let human = stdout(fx.cli(&["score", "field.json", "--signals"]));
    let board = stdout(fx.cli(&["score", "field.json"]));
    assert!(human.starts_with(&board), "{human}");
    assert!(human.contains("Fast signals"), "{human}");
    assert!(!board.contains("Fast signals"));
}

#[test]
fn every_human_board_starts_with_its_evaluation_tier() {
    let fx = field();
    for args in [
        vec!["score", "field.json"],
        vec!["score", "field.json", "--headline"],
        vec!["score", "field.json", "--signals"],
    ] {
        let out = stdout(fx.cli(&args));
        assert!(
            out.lines().next().unwrap().starts_with(
                "evaluation tier: historical-advisory (advisory only: no signed forward window"
            ),
            "{args:?}: {out}"
        );
    }
    let headlines = json(fx.cli(&["score", "field.json", "--headline", "--json"]));
    for h in headlines["headlines"].as_array().unwrap() {
        assert_eq!(h["qualifiers"]["tier"], "historical-advisory");
    }
    let human = stdout(fx.cli(&["headline", "field.json"]));
    assert!(human.contains("steady: "), "{human}");
    for line in human
        .lines()
        .filter(|l| l.starts_with("steady") || l.starts_with("sloppy"))
    {
        assert!(line.contains(", tier historical-advisory"), "{line}");
    }
}

/// Evidence from other commands' documents fills the remaining measures; an
/// agent with no evidence keeps them unavailable.
#[test]
fn evidence_documents_fill_the_fault_cost_probe_and_stability_measures() {
    let fx = field();
    fx.write(
        "suite.json",
        &serde_json::json!({
            "board": [],
            "suite_evidence": {"trials": {"per_agent": [{
                "agent_id": "steady",
                "expected": 4,
                "completed": 3,
                "failed": 1,
                "unreported": 0,
                "gates": [{"gate": "trial_failed", "window": "0-40", "seed": 1,
                           "reason": "agent_fault: decision_budget_exceeded"}],
            }]}},
        }),
    );
    fx.write(
        "host.json",
        &serde_json::json!([
            {"agent_id": "steady", "cost_source": "host_observed_usd", "cost": 2.0, "runs_scored": 4},
            {"agent_id": "sloppy", "cost_source": "withheld", "cost": null, "runs_scored": 4},
        ]),
    );
    fx.write(
        "probe.json",
        &serde_json::json!({"status": "measured", "result": {"report": {"edges": [0.1, 0.2, 0.3]}}}),
    );
    fx.write(
        "probe-b.json",
        &serde_json::json!({"status": "unavailable", "unavailable": {"reason": "score_unavailable", "window": 0}}),
    );
    fx.write(
        "shown.json",
        &serde_json::json!({"status": "measured", "result": {"pairs": [
            {"baseline": {"kind": "unmasked"}, "variant": {"kind": "blinded", "level": "full"},
             "comparison": {"steps": 10, "disagreeing_steps": 2}},
            {"baseline": {"kind": "blinded", "level": "full"}, "variant": {"kind": "reordered"},
             "comparison": {"steps": 10, "disagreeing_steps": 0}},
        ]}}),
    );
    fx.write(
        "stability.json",
        &serde_json::json!({"agent_id": "steady", "pairs_compared": 8, "differing_pairs": 2,
                            "pairwise_disagreement": {"status": "available", "value": 0.25}}),
    );
    fx.write(
        "stability-b.json",
        &serde_json::json!({"agent_id": "sloppy", "pairs_compared": 0, "differing_pairs": 0,
                            "pairwise_disagreement": {"status": "unavailable", "reason": "single_replicate"}}),
    );
    let doc = json(fx.cli(&[
        "signals",
        "field.json",
        "--suite-evidence",
        "suite.json",
        "--host-cost",
        "host.json",
        "--contamination-probe",
        "steady=probe.json",
        "--contamination-probe",
        "sloppy=probe-b.json",
        "--presentation-stability",
        "steady=shown.json",
        "--decision-stability",
        "stability.json",
        "--decision-stability",
        "stability-b.json",
        "--json",
    ]));
    let report = &doc["fast_signals"];
    let faults = measure(report, "steady", "agent_fault_cells");
    assert_eq!(
        (faults["count"].as_u64(), faults["observations"].as_u64()),
        (Some(1), Some(4))
    );
    assert_eq!(faults["detail"]["agent_fault:decision_budget_exceeded"], 1);
    let cost = measure(report, "steady", "cost_per_run");
    assert_eq!(
        (cost["value"].as_f64(), cost["source"].as_str()),
        (Some(0.5), Some("host_observed_usd"))
    );
    assert_eq!(
        measure(report, "sloppy", "cost_per_run")["source"],
        "entrant_reported"
    );
    let edge = measure(report, "steady", "contamination_mean_edge")["value"]
        .as_f64()
        .unwrap();
    assert!((edge - 0.2).abs() < 1e-12, "{edge}");
    assert!(
        measure(report, "sloppy", "contamination_mean_edge")["unavailable"]
            .as_str()
            .unwrap()
            .ends_with("score_unavailable")
    );
    let shown = measure(report, "steady", "presentation_disagreement");
    assert_eq!(shown["value"], 0.1);
    assert_eq!(shown["detail"]["blinded:level=full"], 2);
    assert_eq!(shown["detail"]["reordered"], 0);
    assert_eq!(
        measure(report, "steady", "decision_disagreement")["value"],
        0.25
    );
    assert!(
        measure(report, "sloppy", "decision_disagreement")["unavailable"]
            .as_str()
            .unwrap()
            .ends_with("single_replicate")
    );
    assert!(measure(report, "sloppy", "agent_fault_cells")["value"].is_null());
    assert_eq!(report["unmatched_evidence"], serde_json::json!([]));
}

#[test]
fn bad_usage_and_unreadable_evidence_are_refused() {
    let fx = field();
    assert_eq!(fx.cli(&["signals"]).status.code(), Some(2));
    assert_eq!(fx.cli(&["signals", "missing.json"]).status.code(), Some(1));
    assert_eq!(
        fx.cli(&["signals", "field.json", "--suite-evidence"])
            .status
            .code(),
        Some(2)
    );
    assert_eq!(
        fx.cli(&["signals", "field.json", "--host-cost", "--json"])
            .status
            .code(),
        Some(2)
    );
    assert_eq!(
        fx.cli(&[
            "signals",
            "field.json",
            "--contamination-probe",
            "probe.json"
        ])
        .status
        .code(),
        Some(1)
    );
    assert_eq!(
        fx.cli(&[
            "signals",
            "field.json",
            "--decision-stability",
            "missing.json"
        ])
        .status
        .code(),
        Some(1)
    );
    fx.write("bad.json", &serde_json::json!({"trials": {}}));
    assert_eq!(
        fx.cli(&["signals", "field.json", "--suite-evidence", "bad.json"])
            .status
            .code(),
        Some(1)
    );
    fx.write("probe.json", &serde_json::json!({"result": {}}));
    assert_eq!(
        fx.cli(&[
            "signals",
            "field.json",
            "--contamination-probe",
            "steady=probe.json"
        ])
        .status
        .code(),
        Some(1)
    );
    assert_eq!(
        fx.cli(&[
            "signals",
            "field.json",
            "--presentation-stability",
            "steady=probe.json"
        ])
        .status
        .code(),
        Some(1)
    );
}
