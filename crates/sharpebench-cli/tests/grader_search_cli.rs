//! `sharpebench grader-search` through the real binary, on the fixed seed.
//!
//! The pins below are the search's current result, not a claim that the result
//! is good. Autocorrelated zero-edge tracks and the rest are admitted today; the
//! table says so, so that a change to any gate moves a
//! pin and has to be looked at, in either direction.

use std::process::{Command, Output};
use std::sync::OnceLock;

use serde_json::Value;

const SEED: &str = "20261030";
const BUDGET: &str = "120";
const REPLICATES: &str = "12";

fn cli(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_sharpebench"))
        .args(args)
        .output()
        .expect("the CLI runs")
}

fn search(threads: &str) -> (Option<i32>, Value) {
    let output = cli(&[
        "grader-search",
        "--seed",
        SEED,
        "--budget",
        BUDGET,
        "--replicates",
        REPLICATES,
        "--max-findings",
        "1",
        "--threads",
        threads,
        "--json",
    ]);
    let doc = serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!(
            "not JSON ({error}): {}",
            String::from_utf8_lossy(&output.stderr)
        )
    });
    (output.status.code(), doc)
}

fn report() -> &'static (Option<i32>, Value) {
    static REPORT: OnceLock<(Option<i32>, Value)> = OnceLock::new();
    REPORT.get_or_init(|| search("4"))
}

fn doc() -> &'static Value {
    &report().1
}

fn array(value: &Value) -> &Vec<Value> {
    value.as_array().expect("an array")
}

fn row<'a>(name: &str) -> &'a Value {
    array(&doc()["moves"])
        .iter()
        .find(|m| m["name"] == name)
        .unwrap_or_else(|| panic!("{name} is searched"))
}

fn eligible_through_score(finding: &Value) -> bool {
    let dir = tempfile::tempdir().expect("a temp dir");
    let path = dir.path().join("field.json");
    std::fs::write(&path, finding["reproducer"].to_string()).expect("the field writes");
    let output = cli(&[
        "score",
        path.to_str().expect("a UTF-8 path"),
        "--pass-mode",
        finding["pass_mode"].as_str().expect("a mode"),
        "--json",
    ]);
    let board: Value = serde_json::from_slice(&output.stdout).expect("a board");
    let id = finding["candidate_id"].as_str().expect("an id");
    array(&board)
        .iter()
        .find(|r| r["agent_id"] == id)
        .expect("the candidate is on the board")["rank_eligible"]
        .as_bool()
        .expect("a verdict")
}

/// The report says what it is, and a run that reports a finding exits 1.
#[test]
fn the_report_is_rank_neutral_and_its_controls_hold() {
    assert_eq!(doc()["schema_version"], "sharpebench.grader-search.v1");
    assert_eq!(doc()["rank_input"], false);
    assert_eq!(doc()["control"]["skilled_fixture_admitted"], true);
    assert_eq!(doc()["control"]["iid_null_refused"], true);
    assert_eq!(report().0, Some(1), "the seed reports findings today");
    assert!(!array(&doc()["limits"]).is_empty());
}

/// Every move the search is specified to make is made.
#[test]
fn every_required_move_is_searched() {
    for name in [
        "constant_track",
        "near_constant",
        "single_observation",
        "autocorrelated",
        "tail_seller",
        "leverage_timing",
        "window_boundary",
        "clone_threshold",
        "dispersion_grief",
    ] {
        assert!(
            row(name)["evaluated"].as_u64().expect("a count") > 0,
            "{name}"
        );
    }
}

/// An exact constant is the defense the first of these repairs built. The search
/// covers its whole grid, so one admission here is that repair regressing.
#[test]
fn a_constant_track_is_never_admitted() {
    assert_eq!(row("constant_track")["evaluated"], 200);
    assert_eq!(row("constant_track")["admitted_on_screen"], 0);
}

/// A track that never moved is refused however it was perturbed: no point of
/// either grid gets through.
#[test]
fn a_track_that_never_moved_is_never_admitted() {
    for name in ["near_constant", "single_observation"] {
        assert_eq!(row(name)["admitted_on_screen"], 0, "{name}");
    }
    for finding in array(&doc()["findings"]) {
        for variant in ["level_step_jitter", "last_bar_coin"] {
            assert_ne!(finding["variant"], variant, "{finding}");
        }
    }
}

/// The fence holds: a vote outside it never costs an entrant eligibility, and
/// the row says the griefer was not fenced when it sat inside.
#[test]
fn only_a_vote_inside_the_fence_demotes() {
    let demotions = array(&doc()["demotions"]);
    assert!(!demotions.is_empty());
    for d in demotions {
        let fraction = d["fence_fraction"].as_f64().expect("a fraction");
        assert!(fraction < 1.0, "a fenced vote moved the bar: {d}");
        assert_eq!(d["griefer_fenced"], false, "{d}");
    }
}

/// One vote inside the fence is clipped by the influence bound, so where it
/// sits no longer sets the honest bar: before the bound the eight positions
/// measured 0.77 to 1.48 per period, now every one measures the same bar. The
/// honest entrants are still demoted on this six-agent fixture; the bound
/// limits a vote to an honest field's 99th-percentile distance, and keeping all
/// six eligible would need a reach that clips most honest fields.
#[test]
fn the_griefer_position_does_not_set_the_bar() {
    let demotions = array(&doc()["demotions"]);
    let first = demotions[0]["bar_after"].as_f64().expect("a bar");
    for d in demotions {
        let bar = d["bar_after"].as_f64().expect("a bar");
        assert!((bar - first).abs() < 1e-12, "{d}");
        assert!(bar < 0.7, "{d}");
        assert_eq!(d["griefer_clipped"], true, "{d}");
    }
}

/// Puppets the collapse folds do not lift a target: the copy control flips none.
#[test]
fn copies_are_folded_by_the_collapse() {
    for finding in array(&doc()["findings"]) {
        if finding["move"] == "clone_threshold" {
            assert_eq!(finding["replication"]["control_admitted"], 0, "{finding}");
        }
    }
}

/// Puppets just below the collapse cosine lift no target either. Before the
/// independence weighting, 32 of them at a pairwise cosine of 0.97 to 0.99 took
/// a refused zero-edge target's bar from 0.277 to about 0.12 and admitted it;
/// now the flood weighs about one vote and no grid point admits anybody.
#[test]
fn puppets_below_the_collapse_lift_no_target() {
    assert_eq!(row("clone_threshold")["admitted_on_screen"], 0);
    for finding in array(&doc()["findings"]) {
        assert_ne!(finding["move"], "clone_threshold", "{finding}");
    }
}

/// A finding is only a finding if the real scorer agrees from the report alone:
/// the reproducer goes through `sharpebench score`, not through the search.
#[test]
fn every_finding_reproduces_through_score() {
    let findings = array(&doc()["findings"]);
    assert!(!findings.is_empty());
    for finding in findings {
        assert!(eligible_through_score(finding), "{}", finding["id"]);
    }
}

/// The hexadecimal series is the JSON series, so a reader can take either.
#[test]
fn the_exact_bits_match_the_reported_series() {
    for finding in array(&doc()["findings"]) {
        let id = &finding["candidate_id"];
        let candidate = array(&finding["reproducer"])
            .iter()
            .find(|s| s["agent_id"] == *id)
            .expect("the candidate is in its reproducer");
        for (run, bits) in array(&candidate["runs"])
            .iter()
            .zip(array(&finding["return_bits"]))
        {
            for (value, hex) in array(&run["returns"]).iter().zip(array(bits)) {
                let parsed = u64::from_str_radix(hex.as_str().expect("hex"), 16).expect("hex");
                assert_eq!(value.as_f64().expect("a return").to_bits(), parsed);
            }
        }
    }
}

/// Threads shard the work and nothing else.
#[test]
fn the_thread_count_does_not_move_the_report() {
    let (_, single) = search("1");
    assert_eq!(single["digest"], doc()["digest"]);
}

/// The fixed-seed regression. A gate that tightens or loosens moves this table.
#[test]
fn the_fixed_seed_result_is_pinned() {
    let pins: [(&str, u64, u64); 8] = PINS;
    for (name, evaluated, admitted) in pins {
        let m = row(name);
        assert_eq!(m["evaluated"], evaluated, "{name}: {m}");
        assert_eq!(m["admitted_on_screen"], admitted, "{name}: {m}");
    }
    assert_eq!(doc()["structural_findings"], STRUCTURAL);
    assert_eq!(array(&doc()["demotions"]).len(), DEMOTIONS);
    assert_eq!(doc()["digest"], DIGEST);
}

const PINS: [(&str, u64, u64); 8] = [
    ("constant_track", 200, 0),
    ("near_constant", 118, 0),
    ("single_observation", 120, 0),
    // Was 9 before the standard-error certification: the Newey-West correction
    // refuses three of the serially correlated zero-edge points on this budget.
    ("autocorrelated", 120, 6),
    // Was 37 before the observed-loss refusal: a crash-free seller that never
    // lost a bar is refused, and one that loses on ordinary bars is not.
    ("tail_seller", 120, 26),
    ("leverage_timing", 120, 1),
    ("window_boundary", 120, 0),
    // Was 38 before the independence weighting of the dispersion votes: puppets
    // below the collapse cosine no longer cast a full vote each, so none lifts
    // a target the honest field refuses.
    ("clone_threshold", 3384, 0),
];
const STRUCTURAL: u64 = 1;
const DEMOTIONS: usize = 8;
const DIGEST: &str = "035ecf8c9720261c";

#[test]
fn a_bad_invocation_is_refused() {
    for args in [
        &["grader-search", "--budget", "0"][..],
        &["grader-search", "--seed", "x"][..],
        &["grader-search", "--nonsense", "1"][..],
        &["grader-search", "--out"][..],
    ] {
        assert_eq!(cli(args).status.code(), Some(2), "{args:?}");
    }
}
