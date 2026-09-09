//! Hermetic CLI pricing tests. No provider endpoint or model is used.
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::process::Command;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

fn prepare_fixture_stream(stream: &TcpStream) {
    // Accepted sockets can inherit the listener's nonblocking mode. The
    // listener polls for shutdown, but request parsing must wait for bytes.
    stream.set_nonblocking(false).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(3)))
        .unwrap();
}

#[test]
fn fixture_reader_waits_for_bytes_on_an_initially_nonblocking_socket() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let mut client = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
    let (mut server, _) = listener.accept().unwrap();
    // Force the platform-dependent inherited state even on Linux.
    server.set_nonblocking(true).unwrap();
    let mut byte = [0];
    assert_eq!(
        server.read(&mut byte).unwrap_err().kind(),
        std::io::ErrorKind::WouldBlock
    );
    prepare_fixture_stream(&server);
    let writer = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(100));
        client.write_all(b"x").unwrap();
    });
    let read = server.read_exact(&mut byte);
    writer.join().unwrap();
    read.unwrap();
    assert_eq!(byte, *b"x");
}

#[test]
fn frozen_rates_reach_real_http_sweeps_and_checkpoint_identity_without_changing_rank() {
    let dir = std::env::temp_dir().join(format!("sharpe-cli-rate-{}", std::process::id()));
    std::fs::create_dir(&dir).unwrap();
    let data = dir.join("data.csv");
    let rates = dir.join("rates.json");
    let checkpoint = dir.join("checkpoint.json");
    let mut csv = String::from("date,symbol,close\n");
    for row in 0..40 {
        csv.push_str(&format!("2020-{row:03},FIX,{}\n", 100 + row));
    }
    std::fs::write(&data, csv).unwrap();
    let mut card = serde_json::json!({
        "schema_version": "sharpebench.token-rate-card.v1",
        "provider": "fixture", "model": "test-model", "revision": "test-1",
        "input_usd_nanos_per_token": 125, "output_usd_nanos_per_token": 500,
    });
    std::fs::write(&rates, serde_json::to_vec(&card).unwrap()).unwrap();

    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap().to_string();
    listener.set_nonblocking(true).unwrap();
    let stopped = Arc::new(AtomicBool::new(false));
    let report_usage = Arc::new(AtomicBool::new(true));
    let calls = Arc::new(AtomicUsize::new(0));
    let (stop, usage, counter) = (stopped.clone(), report_usage.clone(), calls.clone());
    let server = std::thread::spawn(move || {
        while !stop.load(Ordering::Acquire) {
            let (mut stream, _) = match listener.accept() {
                Ok(connection) => connection,
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    std::thread::sleep(Duration::from_millis(1));
                    continue;
                }
                Err(error) => panic!("fixture accept: {error}"),
            };
            prepare_fixture_stream(&stream);
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut length = None;
            loop {
                let mut line = String::new();
                assert_ne!(reader.read_line(&mut line).unwrap(), 0);
                if line == "\r\n" {
                    break;
                }
                if let Some(value) = line.to_ascii_lowercase().strip_prefix("content-length:") {
                    length = Some(value.trim().parse::<usize>().unwrap());
                }
            }
            let length = length.unwrap();
            assert!(length < 100_000);
            let mut body = vec![0; length];
            reader.read_exact(&mut body).unwrap();
            counter.fetch_add(1, Ordering::Relaxed);
            let response = if usage.load(Ordering::Acquire) {
                r#"{"orders":[],"cost":{"cost_usd":999,"tokens_in":3,"tokens_out":5,"reasoning_tokens":4}}"#
            } else {
                r#"{"orders":[]}"#
            };
            write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                response.len(),
                response
            )
            .unwrap();
        }
    });
    let invoke = |priced: bool, resume: bool| {
        let mut command = Command::new(env!("CARGO_BIN_EXE_sharpebench"));
        command
            .args(["run", "--http", &addr, "--json", "--data"])
            .arg(&data);
        if priced {
            command.arg("--rate-card").arg(&rates);
        }
        if resume {
            command
                .arg("--checkpoint")
                .arg(&checkpoint)
                .args(["--entrant-sha256", &"ab".repeat(32)]);
        }
        command.output().unwrap()
    };
    let priced = invoke(true, true);
    let pretty_card = serde_json::to_vec_pretty(&card).unwrap();
    std::fs::write(&rates, &pretty_card).unwrap();
    let resumed = invoke(true, true);
    let calls_after_resume = calls.load(Ordering::Relaxed);
    let original_checkpoint = std::fs::read(&checkpoint).unwrap();
    card["input_usd_nanos_per_token"] = serde_json::json!(126);
    std::fs::write(&rates, serde_json::to_vec(&card).unwrap()).unwrap();
    let changed = invoke(true, true);
    let calls_after_refusal = calls.load(Ordering::Relaxed);
    assert_eq!(std::fs::read(&checkpoint).unwrap(), original_checkpoint);
    let plain = invoke(false, false);
    std::fs::write(&rates, pretty_card).unwrap();
    report_usage.store(false, Ordering::Release);
    let unknown = invoke(true, false);
    stopped.store(true, Ordering::Release);
    server.join().unwrap();
    for path in [data, rates, checkpoint] {
        std::fs::remove_file(path).unwrap();
    }
    std::fs::remove_dir(dir).unwrap();

    assert!(
        priced.status.success(),
        "{}",
        String::from_utf8_lossy(&priced.stderr)
    );
    assert!(resumed.status.success());
    assert_eq!(
        calls_after_resume, 240,
        "resume must not charge or execute completed cells"
    );
    assert_eq!(
        calls_after_refusal, 240,
        "different rates must refuse before execution"
    );
    assert!(!changed.status.success());
    assert!(String::from_utf8_lossy(&changed.stderr).contains("contract differs"));
    let mut priced: serde_json::Value = serde_json::from_slice(&priced.stdout).unwrap();
    let resumed: serde_json::Value = serde_json::from_slice(&resumed.stdout).unwrap();
    assert_eq!(
        priced, resumed,
        "checkpoint must retain the estimate and its rates"
    );
    let row = priced
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["agent_id"].as_str().unwrap().starts_with("http:"))
        .unwrap();
    let money = &row["attempt_accounting"]["monetary_cost"];
    assert_eq!(money["status"], "estimated");
    assert_eq!(money["usage_source"], "entrant_reported");
    assert_eq!(money["usd_nanos"], "690000");
    let mut plain: serde_json::Value = serde_json::from_slice(&plain.stdout).unwrap();
    for board in [&mut plain, &mut priced] {
        for row in board.as_array_mut().unwrap() {
            row.as_object_mut().unwrap().remove("attempt_accounting");
        }
    }
    assert_eq!(
        plain, priced,
        "rates must not change any legacy score or board order"
    );
    assert!(unknown.status.success());
    let unknown: serde_json::Value = serde_json::from_slice(&unknown.stdout).unwrap();
    let row = unknown
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["agent_id"].as_str().unwrap().starts_with("http:"))
        .unwrap();
    let money = &row["attempt_accounting"]["monetary_cost"];
    assert_eq!(money["status"], "unavailable");
    assert_eq!(money["reason"], "incomplete_usage_evidence");
    assert!(money.get("usd_nanos").is_none());
}

#[test]
fn bad_or_unused_rate_cards_refuse_before_launch() {
    for args in [
        vec!["run", "--rate-card", "missing.json"],
        vec!["run", "--cmd", "must-not-launch", "--rate-card"],
        vec![
            "run",
            "--cmd",
            "must-not-launch",
            "--rate-card",
            "missing.json",
        ],
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_sharpebench"))
            .args(args)
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(2));
        let error = String::from_utf8_lossy(&output.stderr);
        assert!(error.contains("rate-card") || error.contains("rate card"));
        assert!(!error.contains("cannot spawn"));
    }
}
