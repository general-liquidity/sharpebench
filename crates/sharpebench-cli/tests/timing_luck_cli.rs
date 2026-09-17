//! `sharpebench timing-luck` through the real binary: the floor is measured on
//! the same protocol `run` ranks, and it never touches the board.

use std::io::Write;
use std::process::Command;

use serde_json::Value;

fn cli(args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_sharpebench"))
        .args(args)
        .output()
        .expect("the CLI runs")
}

fn json_of(output: &std::process::Output) -> Value {
    serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!(
            "not JSON ({error}): {}\nstderr: {}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        )
    })
}

fn measured(args: &[&str]) -> Value {
    let output = cli(args);
    assert_eq!(
        output.status.code(),
        Some(0),
        "{args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let doc = json_of(&output);
    assert_eq!(doc["status"], "measured");
    doc
}

/// Cadence 1 is `run`'s own field on its own windows, seeds and costs, so
/// every reference row's all-windows figures and deflation inputs are the
/// board's. The board itself is still the bare array it always was.
#[test]
fn cadence_one_is_the_run_board_on_every_reference_row() {
    let board = cli(&["run", "--json"]);
    assert_eq!(board.status.code(), Some(0));
    let board = json_of(&board);
    let board = board
        .as_array()
        .expect("run --json is still a bare board array");

    let doc = measured(&["timing-luck", "--cadence", "1", "--json"]);
    let rows = doc["report"]["rows"].as_array().unwrap();
    let ranked: Vec<&Value> = rows
        .iter()
        .filter(|row| row["role"] == "ranked_reference")
        .collect();
    assert_eq!(ranked.len(), board.len());
    for entry in board {
        let id = entry["agent_id"].as_str().unwrap();
        let row = ranked
            .iter()
            .find(|row| row["agent_id"] == id)
            .unwrap_or_else(|| panic!("the report has no row for board entrant {id}"));
        let scope = &row["all_windows"];
        assert!(entry["deflation_error"].is_null(), "{entry}");
        assert_eq!(
            scope["deflated_sharpe"]["by_phase"],
            Value::Array(vec![entry["deflated_sharpe"].clone()]),
            "{id}"
        );
        let bar = &scope["deflation"];
        for (field, board_field) in [
            ("trials_sr_std", "trials_sr_std"),
            ("trials_sr_std_source", "trials_sr_std_source"),
            ("effective_n_trials", "effective_n_trials"),
            ("deflation_bar_per_period", "deflation_bar_per_period"),
        ] {
            assert_eq!(bar[field], entry[board_field], "{id} {field}");
        }
        assert_eq!(
            scope["field_dispersion_by_phase"][0]["trials_sr_std"], entry["trials_sr_std"],
            "{id}"
        );
    }
    let hold = rows
        .iter()
        .find(|row| row["role"] == "suite_control")
        .expect("the hold control is reported");
    assert_eq!(hold["agent_id"], "pipeline-hold");
}

#[test]
fn the_report_is_rank_neutral_and_counts_what_is_behind_each_figure() {
    let doc = measured(&["timing-luck", "--cadence", "3", "--json"]);
    let report = &doc["report"];
    assert_eq!(report["schema_version"], "sharpebench.timing-luck.v2");
    assert_eq!(report["rank_input"], false);
    assert_eq!(report["cadence"], 3);
    assert_eq!(report["seeds"], 8);
    assert_eq!(report["dataset_len"], 180);
    let windows = report["windows"].as_array().unwrap();
    assert_eq!(windows[0]["window"], "20-100");
    assert_eq!(windows[0]["bars"], 80);
    assert_eq!(
        windows[0]["decisions_by_phase"],
        serde_json::json!([27, 28, 27])
    );
    assert_eq!(windows[1]["window"], "100-180");

    let rows = report["rows"].as_array().unwrap();
    assert_eq!(
        rows.len(),
        6,
        "five reference entrants and the hold control"
    );
    let mut moved = false;
    for row in rows {
        assert_eq!(row["all_windows"]["scope"], "all-windows");
        assert_eq!(row["all_windows"]["windows"], 2);
        let per_window = row["per_window"].as_array().unwrap();
        assert_eq!(per_window.len(), 2);
        for scope in std::iter::once(&row["all_windows"]).chain(per_window) {
            assert_eq!(scope["phases"], 3);
            assert_eq!(scope["sharpe"]["by_phase"].as_array().unwrap().len(), 3);
            assert_eq!(
                scope["field_dispersion_by_phase"].as_array().unwrap().len(),
                3
            );
            if row["role"] == "ranked_reference" {
                assert_eq!(scope["sharpe"]["phases_measured"], 3);
                moved |= scope["sharpe"]["range"].as_f64().unwrap() > 0.0;
            }
        }
        for scope in per_window {
            assert_eq!(scope["windows"], 1);
        }
    }
    assert!(moved, "moving the phase moved no reference Sharpe: {doc}");
}

/// A cadence longer than a declared window is refused with the input that
/// decided it, before anything runs; a cadence equal to the window measures.
#[test]
fn a_dataset_too_short_for_the_cadence_is_typed_unavailable() {
    let output = cli(&["timing-luck", "--cadence", "81", "--json"]);
    assert_eq!(output.status.code(), Some(1));
    let doc = json_of(&output);
    assert_eq!(doc["status"], "unavailable");
    assert_eq!(doc["rank_input"], false);
    assert_eq!(doc["schema_version"], "sharpebench.timing-luck.v2");
    let unavailable = &doc["unavailable"];
    assert_eq!(unavailable["reason"], "window_shorter_than_cadence");
    assert_eq!(unavailable["window"], "20-100");
    assert_eq!(unavailable["window_len"], 80);
    assert_eq!(unavailable["cadence"], 81);
    assert_eq!(unavailable["min_window_bars"], 81);

    // A 40-row frozen dataset: `run` declares windows 10-25 and 25-40.
    let mut csv = tempfile::NamedTempFile::new().unwrap();
    writeln!(csv, "date,symbol,close").unwrap();
    for day in 0..40 {
        let date = format!("2026-{:02}-{:02}", day / 28 + 1, day % 28 + 1);
        writeln!(csv, "{date},AAA,{}", 100.0 + (day % 7) as f64).unwrap();
        writeln!(csv, "{date},BBB,{}", 50.0 + (day % 5) as f64).unwrap();
    }
    csv.flush().unwrap();
    let path = csv.path().to_str().unwrap();

    let refused = cli(&["timing-luck", "--data", path, "--cadence", "16", "--json"]);
    assert_eq!(refused.status.code(), Some(1));
    let doc = json_of(&refused);
    assert_eq!(doc["unavailable"]["reason"], "window_shorter_than_cadence");
    assert_eq!(doc["unavailable"]["window"], "10-25");
    assert_eq!(doc["unavailable"]["window_len"], 15);

    let doc = measured(&["timing-luck", "--data", path, "--cadence", "15", "--json"]);
    let windows = &doc["report"]["windows"];
    assert_eq!(windows[0]["bars"], 15);
    assert_eq!(windows[1]["window"], "25-40");
    assert_eq!(windows[1]["decisions_by_phase"][0], 1);
    assert_eq!(windows[1]["decisions_by_phase"][14], 2);
}

/// Each refusal names its own cause, so an unknown-command exit cannot pass
/// for one of them.
#[test]
fn usage_errors_and_entrants_are_refused() {
    const CADENCE: &str = "--cadence must be a whole number of bars, at least 1";
    const ENTRANT: &str = "runs no external entrant";
    for (args, says) in [
        (
            vec!["timing-luck"],
            "usage: sharpebench timing-luck --cadence",
        ),
        (
            vec!["timing-luck", "--offsets", "3"],
            "usage: sharpebench timing-luck --cadence",
        ),
        (vec!["timing-luck", "--cadence", "0"], CADENCE),
        (vec!["timing-luck", "--cadence", "-1"], CADENCE),
        (vec!["timing-luck", "--cadence", "two"], CADENCE),
        (
            vec!["timing-luck", "--cadence", "2", "--periods-per-year", "0"],
            "--periods-per-year must be a positive number",
        ),
        (
            vec!["timing-luck", "--cadence", "2", "--short-borrow-bps", "-1"],
            "--short-borrow-bps `-1` is refused",
        ),
        (
            vec!["timing-luck", "--cadence", "2", "--cmd", "python agent.py"],
            ENTRANT,
        ),
        (
            vec!["timing-luck", "--cadence", "2", "--http", "127.0.0.1:9"],
            ENTRANT,
        ),
    ] {
        let output = cli(&args);
        assert_eq!(output.status.code(), Some(2), "{args:?}");
        assert!(output.stdout.is_empty(), "{args:?}");
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(stderr.contains(says), "{args:?}: {stderr}");
    }
}

#[test]
fn the_table_says_it_is_not_a_rank_input() {
    let output = cli(&["timing-luck", "--cadence", "2"]);
    assert_eq!(
        output.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let text = String::from_utf8_lossy(&output.stdout);
    for expected in [
        "Not used by the gate, eligibility or the rank.",
        "cadence 2 so 2 phases",
        "window 20-100 (80 bars): decisions per run by phase 40,41",
        "every phase evaluates every bar",
        "uses the row's phase-0 bar",
        "pipeline-hold",
    ] {
        assert!(text.contains(expected), "missing `{expected}` in:\n{text}");
    }
}
