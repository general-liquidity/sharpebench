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

/// With one offset the report evaluates `run`'s own windows, seeds, costs and
/// field, so every reference row's all-windows deflated Sharpe is the one the
/// board publishes for it, and the board itself is still the bare array it
/// always was.
#[test]
fn one_offset_is_the_run_board_on_every_reference_row() {
    let board = cli(&["run", "--json"]);
    assert_eq!(board.status.code(), Some(0));
    let board = json_of(&board);
    let board = board
        .as_array()
        .expect("run --json is still a bare board array");

    let output = cli(&["timing-luck", "--offsets", "1", "--json"]);
    assert_eq!(
        output.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let doc = json_of(&output);
    assert_eq!(doc["status"], "measured");
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
        let expected = if entry.get("deflation_error").is_some_and(|e| !e.is_null()) {
            Value::Null
        } else {
            entry["deflated_sharpe"].clone()
        };
        assert!(expected.is_number() || expected.is_null(), "{entry}");
        assert_eq!(
            row["all_windows"]["deflated_sharpe"]["by_offset"],
            Value::Array(vec![expected]),
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
    let output = cli(&["timing-luck", "--offsets", "3", "--json"]);
    assert_eq!(
        output.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let doc = json_of(&output);
    let report = &doc["report"];
    assert_eq!(report["schema_version"], "sharpebench.timing-luck.v1");
    assert_eq!(report["rank_input"], false);
    assert_eq!(report["seeds"], 8);
    let geometry = &report["geometry"];
    assert_eq!(geometry["offsets"], 3);
    assert_eq!(geometry["dataset_len"], 180);
    assert_eq!(geometry["declared_windows_disjoint"], true);
    assert_eq!(geometry["instances_of_distinct_windows_disjoint"], true);
    assert_eq!(geometry["instances_of_one_window_overlap"], true);
    let windows = geometry["windows"].as_array().unwrap();
    assert_eq!(windows[0]["declared"], "20-100");
    assert_eq!(windows[0]["first_instance"], "20-98");
    assert_eq!(windows[1]["last_instance"], "102-180");

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
            assert_eq!(scope["offsets"], 3);
            assert_eq!(scope["sharpe"]["by_offset"].as_array().unwrap().len(), 3);
            if row["role"] == "ranked_reference" {
                assert_eq!(scope["sharpe"]["offsets_measured"], 3);
                moved |= scope["sharpe"]["range"].as_f64().unwrap() > 0.0;
            }
        }
        for scope in per_window {
            assert_eq!(scope["windows"], 1);
        }
    }
    assert!(moved, "shifting the start moved no reference Sharpe: {doc}");
}

/// Offsets that leave a shifted window under two bars are refused with the
/// input that decided it, before anything runs; one offset fewer measures.
#[test]
fn a_dataset_too_short_for_the_offsets_is_typed_unavailable() {
    let output = cli(&["timing-luck", "--offsets", "80", "--json"]);
    assert_eq!(output.status.code(), Some(1));
    let doc = json_of(&output);
    assert_eq!(doc["status"], "unavailable");
    assert_eq!(doc["rank_input"], false);
    assert_eq!(doc["schema_version"], "sharpebench.timing-luck.v1");
    let unavailable = &doc["unavailable"];
    assert_eq!(unavailable["reason"], "window_too_short_for_offsets");
    assert_eq!(unavailable["window"], "20-100");
    assert_eq!(unavailable["window_len"], 80);
    assert_eq!(unavailable["offsets"], 80);
    assert_eq!(unavailable["min_instance_bars"], 2);

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

    let refused = cli(&["timing-luck", "--data", path, "--offsets", "15", "--json"]);
    assert_eq!(refused.status.code(), Some(1));
    let doc = json_of(&refused);
    assert_eq!(doc["unavailable"]["reason"], "window_too_short_for_offsets");
    assert_eq!(doc["unavailable"]["window"], "10-25");
    assert_eq!(doc["unavailable"]["window_len"], 15);

    let measured = cli(&["timing-luck", "--data", path, "--offsets", "14", "--json"]);
    assert_eq!(
        measured.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&measured.stderr)
    );
    let doc = json_of(&measured);
    assert_eq!(doc["report"]["geometry"]["windows"][0]["instance_len"], 2);
    assert_eq!(
        doc["report"]["geometry"]["windows"][1]["last_instance"],
        "38-40"
    );
}

#[test]
fn usage_errors_and_entrants_are_refused() {
    for args in [
        vec!["timing-luck"],
        vec!["timing-luck", "--offsets", "0"],
        vec!["timing-luck", "--offsets", "-1"],
        vec!["timing-luck", "--offsets", "two"],
        vec!["timing-luck", "--offsets", "2", "--periods-per-year", "0"],
        vec!["timing-luck", "--offsets", "2", "--cmd", "python agent.py"],
        vec!["timing-luck", "--offsets", "2", "--http", "127.0.0.1:9"],
    ] {
        let output = cli(&args);
        assert_eq!(output.status.code(), Some(2), "{args:?}");
        assert!(output.stdout.is_empty(), "{args:?}");
    }
}

#[test]
fn the_table_says_it_is_not_a_rank_input() {
    let output = cli(&["timing-luck", "--offsets", "2"]);
    assert_eq!(
        output.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let text = String::from_utf8_lossy(&output.stdout);
    assert!(
        text.contains("Not used by the gate, eligibility or the rank."),
        "{text}"
    );
    assert!(
        text.contains("window 20-100: 79-bar shifted windows from 20-99 to 21-100"),
        "{text}"
    );
    assert!(text.contains("not independent draws"), "{text}");
    assert!(text.contains("pipeline-hold"), "{text}");
}
