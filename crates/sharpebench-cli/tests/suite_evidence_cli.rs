//! The suite evidence a real `run` emits: trial counts against the roster the
//! run declared before it ran, and the verdict on each of its controls.
//!
//! Both halves are exercised through the actual CLI binary, not through the
//! library types, because the defect these modules were written against is a
//! producer that never declares anything.

use std::io::{Read, Write};
use std::net::TcpListener;
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

fn cli(args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_sharpebench"))
        .args(args)
        .output()
        .expect("the CLI runs")
}

/// The control that must pass: a clean run declares its roster, counts every
/// declared trial as completed, and both controls hold. Without it a census
/// that reported nothing, and a battery that refused everything, would pass the
/// failure case below.
#[test]
fn a_clean_run_publishes_a_complete_census_and_held_controls() {
    let output = cli(&["run", "--json", "--suite-evidence"]);
    assert_eq!(
        output.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let doc: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("the run emits JSON");

    // Reporting surface beside the board: the record says so itself.
    let evidence = &doc["suite_evidence"];
    assert_eq!(evidence["used_by_gate"], serde_json::Value::Bool(false));

    // Five declared entrants (two reference agents and three luck-floor
    // monkeys) over two windows and eight seeds. The product, not a row count.
    let census = &evidence["trials"];
    assert_eq!(census["cohort"]["agents"].as_array().unwrap().len(), 5);
    assert_eq!(census["cohort"]["windows"].as_array().unwrap().len(), 2);
    assert_eq!(census["cohort"]["seeds"].as_array().unwrap().len(), 8);
    assert_eq!(census["expected"], 80);
    assert_eq!(census["completed"], 80);
    assert_eq!(census["failed"], 0);
    assert_eq!(census["unreported"], 0);
    for row in census["per_agent"].as_array().unwrap() {
        assert_eq!(row["expected"], 16, "{row}");
        assert_eq!(row["completed"], 16, "{row}");
        assert!(row["gates"].as_array().unwrap().is_empty(), "{row}");
    }

    // Identity, intended property and observed outcome travel together.
    let controls = evidence["controls"]["controls"].as_array().unwrap();
    assert_eq!(
        evidence["controls"]["all_held"],
        serde_json::Value::Bool(true)
    );
    let ids: Vec<&str> = controls
        .iter()
        .map(|c| c["control_id"].as_str().unwrap())
        .collect();
    assert_eq!(ids, vec!["pipeline-hold", "invalid-order-refusal"]);
    let hold = &controls[0];
    assert_eq!(hold["property"], "protocol_and_accounting");
    assert_eq!(hold["observation"]["observed"], "protocol_and_accounting");
    assert_eq!(
        hold["observation"]["decisions_round_tripped"],
        hold["observation"]["decisions_expected"]
    );
    assert_eq!(hold["observation"]["accounting_residual"], 0.0);
    assert_eq!(hold["held"], serde_json::Value::Bool(true));
    let refusal = &controls[1];
    assert_eq!(refusal["property"], "refusal_of_invalid_order");
    assert_eq!(refusal["observation"]["invalid_orders_submitted"], 3);
    assert_eq!(refusal["observation"]["refusals_observed"], 3);
    assert_eq!(refusal["held"], serde_json::Value::Bool(true));

    // A control is never a ranked entrant, so no control id appears on the board.
    let board = doc["board"].as_array().unwrap();
    for row in board {
        let agent = row["agent_id"].as_str().unwrap();
        assert!(!ids.contains(&agent), "a control was ranked: {agent}");
    }
    // The suite declares no economic comparator: `buy-and-hold` is a ranked
    // reference entrant here and cannot also be a control.
    assert!(controls
        .iter()
        .all(|c| c["property"] != "economic_comparator"));

    // The evidence is beside the board, not part of it: the board under the
    // envelope is byte-identical to the board the default invocation emits.
    let plain = cli(&["run", "--json"]);
    assert_eq!(plain.status.code(), Some(0));
    let plain: serde_json::Value =
        serde_json::from_slice(&plain.stdout).expect("the board is JSON");
    assert_eq!(
        serde_json::to_string(&doc["board"]).unwrap(),
        serde_json::to_string(&plain).unwrap()
    );
}

/// A decision that parses and is refused by the closed contract, so every cell
/// of the sweep ends in an agent protocol fault.
const INVALID_DECISION: &str = r#"{"orders":[{"symbol":"__no_such_symbol__","action":"buy","target_weight":0.1,"confidence":0.5,"rationale":""}],"reasoning":""}"#;

/// The behaviour the census exists for. Every cell of the external entrant's
/// sweep faults, yet the sweep is complete by row count (an agent fault pools a
/// failing sentinel run), so the board publishes the entrant and the run
/// succeeds. The census must report sixteen failures against an unchanged
/// denominator instead of quietly counting sixteen fewer trials.
#[test]
fn a_failed_trial_is_reported_rather_than_shrinking_the_denominator() {
    let listener = TcpListener::bind("127.0.0.1:0").expect("a local port");
    let addr = listener.local_addr().expect("the bound address");
    listener.set_nonblocking(true).expect("nonblocking accept");
    let stopped = Arc::new(AtomicBool::new(false));
    let thread_stopped = Arc::clone(&stopped);
    let server = std::thread::spawn(move || {
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            INVALID_DECISION.len(),
            INVALID_DECISION
        );
        while !thread_stopped.load(Ordering::Acquire) {
            match listener.accept() {
                Ok((mut stream, _)) => {
                    // Drain the request before answering. Closing the socket on
                    // an unread request resets the connection on some
                    // platforms, and the entrant would then be recorded as a
                    // transport blip rather than the protocol fault this
                    // fixture is here to produce.
                    let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
                    let mut scratch = [0_u8; 8192];
                    let _ = stream.read(&mut scratch);
                    let _ = stream.write_all(response.as_bytes());
                }
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    std::thread::sleep(Duration::from_millis(1));
                }
                Err(error) => panic!("local fixture accept failed: {error}"),
            }
        }
    });

    let output = cli(&[
        "run",
        "--http",
        &addr.to_string(),
        "--json",
        "--suite-evidence",
    ]);
    stopped.store(true, Ordering::Release);
    server.join().expect("the fixture thread joins");

    assert_eq!(
        output.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let doc: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("the run emits JSON");
    let entrant = format!("http:{addr}");

    // The entrant is on the board: by row count the sweep looks complete.
    let board = doc["board"].as_array().expect("a board is an array");
    assert!(
        board
            .iter()
            .any(|row| row["agent_id"].as_str() == Some(entrant.as_str())),
        "the faulted entrant still carries a scored row"
    );

    let census = &doc["suite_evidence"]["trials"];
    // Six declared entrants x two windows x eight seeds. The declaration fixed
    // this before the run and no failure moves it.
    assert_eq!(census["expected"], 96);
    assert_eq!(census["completed"], 80);
    assert_eq!(census["failed"], 16);
    assert_eq!(census["unreported"], 0);

    let row = census["per_agent"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["agent_id"].as_str() == Some(entrant.as_str()))
        .expect("the declared entrant has a census row");
    assert_eq!(row["expected"], 16);
    assert_eq!(row["completed"], 0);
    assert_eq!(row["failed"], 16);
    let gates = row["gates"].as_array().unwrap();
    assert_eq!(gates.len(), 16);
    for gate in gates {
        assert_eq!(gate["gate"], "trial_failed");
        assert_eq!(gate["reason"], "agent_fault: agent_protocol_violation");
    }

    // The controls are about the apparatus, and the apparatus worked: a failing
    // entrant does not withhold a control, and the refusal path that rejected
    // its sixteen cells is the one the control exercises.
    assert_eq!(
        doc["suite_evidence"]["controls"]["all_held"],
        serde_json::Value::Bool(true)
    );
}
