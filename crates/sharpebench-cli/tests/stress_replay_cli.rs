//! `sharpebench run --stress-replay`: a rank-neutral mitigation for the
//! `crash_unseen_in_sample` tail seller. Every admitted row is replayed through
//! the synthetic stress suite and a row that loses more than the unlevered
//! market there is marked beside its score. The rank does not move.

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::process::{Command, Output};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

/// A dataset with a strong, steady drift, so the reference agents are admitted.
/// Written with a fixed LCG so the file is identical on every platform.
fn drift_csv() -> String {
    let mut state: u64 = 7;
    let mut unit = || {
        state = state
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        (state >> 11) as f64 / (1u64 << 53) as f64
    };
    let mut out = String::from("date,symbol,close\n");
    for s in 0..4 {
        let mut price = 100.0;
        for d in 0..300 {
            // Uniform noise with a 1% standard deviation around a 0.4% drift.
            price *= 1.0 + 0.004 + (unit() - 0.5) * 0.0346;
            out.push_str(&format!("2024-{d:04},S{s},{price:.6}\n"));
        }
    }
    out
}

struct Fixture(PathBuf);

impl Fixture {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "sharpebench-stress-replay-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&path).unwrap();
        std::fs::write(path.join("drift.csv"), drift_csv()).unwrap();
        Self(path)
    }

    fn run(&self, extra: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_sharpebench"))
            .current_dir(&self.0)
            .args(["run", "--data", "drift.csv", "--json"])
            .args(extra)
            .output()
            .unwrap()
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn board(output: &Output) -> Vec<serde_json::Value> {
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice::<serde_json::Value>(&output.stdout)
        .unwrap()
        .as_array()
        .unwrap()
        .clone()
}

fn row<'a>(board: &'a [serde_json::Value], id: &str) -> &'a serde_json::Value {
    board.iter().find(|r| r["agent_id"] == id).unwrap()
}

#[test]
fn replay_marks_rows_beside_an_unchanged_board() {
    let fixture = Fixture::new();
    let plain = board(&fixture.run(&[]));
    let replayed = board(&fixture.run(&["--stress-replay"]));

    // Rank-neutral: removing the record gives back the plain board exactly.
    let stripped: Vec<serde_json::Value> = replayed
        .iter()
        .cloned()
        .map(|mut r| {
            r.as_object_mut().unwrap().remove("stress_replay");
            r
        })
        .collect();
    assert_eq!(stripped, plain);

    let admitted: Vec<&serde_json::Value> = replayed
        .iter()
        .filter(|r| r["rank_eligible"] == true)
        .collect();
    assert!(!admitted.is_empty(), "the drift fixture admits a row");
    for r in &replayed {
        let record = r.get("stress_replay");
        if r["rank_eligible"] == true {
            let record = record.expect("every admitted row is replayed");
            assert_eq!(record["status"], "replayed", "{record}");
            assert!(record["label"].as_str().unwrap().starts_with("mitigation"));
        } else {
            assert!(
                record.is_none(),
                "a row that is not admitted is not replayed: {r}"
            );
        }
    }
    // The unlevered market itself is never above its own loss.
    assert_eq!(
        row(&replayed, "buy-and-hold")["stress_replay"]["marked"],
        false
    );
}

/// Serve a levered carry book over HTTP: every symbol at `leverage` times NAV
/// in total. Returns the address and a stop flag.
fn levered_endpoint(leverage: f64) -> (String, Arc<AtomicBool>, std::thread::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap().to_string();
    listener.set_nonblocking(true).unwrap();
    let stop = Arc::new(AtomicBool::new(false));
    let thread_stop = Arc::clone(&stop);
    let server = std::thread::spawn(move || {
        while !thread_stop.load(Ordering::Acquire) {
            match listener.accept() {
                Ok((stream, _)) => answer(stream, leverage),
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    std::thread::sleep(Duration::from_millis(1));
                }
                Err(e) => panic!("fixture accept failed: {e}"),
            }
        }
    });
    (addr, stop, server)
}

fn answer(mut stream: TcpStream, leverage: f64) {
    stream.set_nonblocking(false).unwrap();
    let mut raw = Vec::new();
    let mut chunk = [0u8; 8192];
    // inv: `raw` holds every byte read so far; the loop ends once the headers and
    // the declared body are in, or the peer closes.
    let body = loop {
        let n = stream.read(&mut chunk).unwrap();
        if n == 0 {
            return;
        }
        raw.extend_from_slice(&chunk[..n]);
        let text = String::from_utf8_lossy(&raw).to_string();
        if let Some((head, body)) = text.split_once("\r\n\r\n") {
            let len: usize = head
                .lines()
                .find_map(|l| l.strip_prefix("Content-Length: "))
                .and_then(|v| v.trim().parse().ok())
                .unwrap_or(0);
            if body.len() >= len {
                break body.to_string();
            }
        }
    };
    let obs: serde_json::Value = serde_json::from_str(&body).unwrap();
    let symbols = obs["symbols"].as_array().unwrap();
    let w = leverage / symbols.len().max(1) as f64;
    let orders: Vec<serde_json::Value> = symbols
        .iter()
        .map(|s| serde_json::json!({"symbol": s["symbol"], "action": "buy", "target_weight": w}))
        .collect();
    let decision = serde_json::json!({"orders": orders, "reasoning": "levered carry"}).to_string();
    let _ = stream.write_all(
        format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{decision}",
            decision.len()
        )
        .as_bytes(),
    );
}

#[test]
fn an_admitted_levered_carry_entrant_is_marked_and_keeps_its_rank() {
    let fixture = Fixture::new();
    let (addr, stop, server) = levered_endpoint(2.0);
    let plain = board(&fixture.run(&["--http", &addr]));
    let replayed = board(&fixture.run(&["--http", &addr, "--stress-replay"]));
    stop.store(true, Ordering::Release);
    server.join().unwrap();

    let entrant = replayed
        .iter()
        .find(|r| r.get("attempt_accounting").is_some())
        .expect("the external entrant's row");
    let id = entrant["agent_id"].as_str().unwrap();
    assert_eq!(entrant["rank_eligible"], true, "{entrant}");
    let record = &entrant["stress_replay"];
    assert_eq!(record["status"], "replayed", "{record}");
    assert_eq!(record["marked"], true, "{record}");
    let crash = record["scenarios"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["scenario"] == "flash_crash")
        .unwrap();
    assert_eq!(crash["exceeds_limit"], true, "{crash}");
    // Marked, not demoted.
    assert_eq!(entrant["rank_ordinal"], row(&plain, id)["rank_ordinal"]);
    assert_eq!(entrant["rank_eligible"], row(&plain, id)["rank_eligible"]);
}
