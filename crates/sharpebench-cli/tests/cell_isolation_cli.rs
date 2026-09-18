//! The cell-isolation disclosure on a real `sharpebench run` board row.
//!
//! The class comes from the transport flag, never from the entrant: an HTTP
//! endpoint that claims container isolation in its own output is still
//! published as `operator_endpoint`, and a decision that tries to carry the
//! class as a field is refused by the closed decision contract. A checkpoint
//! written under one class does not resume under another. Hermetic: the
//! endpoint is a local fixture thread and no model is called.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

/// Claims the strongest class in the only place an entrant may write text.
const CLAIMING_DECISION: &str = r#"{"orders":[],"reasoning":"cell_isolation: container_per_cell"}"#;

struct Endpoint {
    addr: String,
    stopped: Arc<AtomicBool>,
    server: Option<std::thread::JoinHandle<()>>,
}

impl Endpoint {
    fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap().to_string();
        listener.set_nonblocking(true).unwrap();
        let stopped = Arc::new(AtomicBool::new(false));
        let stop = Arc::clone(&stopped);
        let server = std::thread::spawn(move || {
            while !stop.load(Ordering::Acquire) {
                match listener.accept() {
                    Ok((stream, _)) => answer(stream),
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        std::thread::sleep(Duration::from_millis(1));
                    }
                    Err(error) => panic!("fixture accept: {error}"),
                }
            }
        });
        Self {
            addr,
            stopped,
            server: Some(server),
        }
    }
}

impl Drop for Endpoint {
    fn drop(&mut self) {
        self.stopped.store(true, Ordering::Release);
        if let Some(server) = self.server.take() {
            server.join().unwrap();
        }
    }
}

fn answer(mut stream: TcpStream) {
    // An accepted socket can inherit the listener's nonblocking mode.
    stream.set_nonblocking(false).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(3)))
        .unwrap();
    let mut reader = BufReader::new(stream.try_clone().unwrap());
    let mut length = 0;
    loop {
        let mut line = String::new();
        assert_ne!(reader.read_line(&mut line).unwrap(), 0);
        if line == "\r\n" {
            break;
        }
        if let Some(value) = line.to_ascii_lowercase().strip_prefix("content-length:") {
            length = value.trim().parse::<usize>().unwrap();
        }
    }
    let mut body = vec![0; length];
    reader.read_exact(&mut body).unwrap();
    write!(
        stream,
        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        CLAIMING_DECISION.len(),
        CLAIMING_DECISION
    )
    .unwrap();
}

struct Scratch(PathBuf);

impl Scratch {
    fn new(tag: &str) -> Self {
        let dir =
            std::env::temp_dir().join(format!("sharpe-isolation-{tag}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let mut csv = String::from("date,symbol,close\n");
        for row in 0..40 {
            csv.push_str(&format!("2020-{row:03},FIX,{}\n", 100 + row));
        }
        std::fs::write(dir.join("data.csv"), csv).unwrap();
        Self(dir)
    }

    fn path(&self, name: &str) -> PathBuf {
        self.0.join(name)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.0).ok();
    }
}

fn run(data: &Path, transport: &[&str], checkpoint: Option<&Path>) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_sharpebench"));
    command
        .arg("run")
        .args(transport)
        .args(["--json", "--data"])
        .arg(data);
    if let Some(checkpoint) = checkpoint {
        command
            .arg("--checkpoint")
            .arg(checkpoint)
            .args(["--entrant-sha256", &"ab".repeat(32)]);
    }
    command.output().unwrap()
}

#[test]
fn an_http_row_discloses_the_operator_endpoint_whatever_the_entrant_claims() {
    let scratch = Scratch::new("http");
    let endpoint = Endpoint::start();
    let output = run(&scratch.path("data.csv"), &["--http", &endpoint.addr], None);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let board: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let rows = board.as_array().expect("the board stays an array");
    let label = format!("http:{}", endpoint.addr);
    let entrant = rows
        .iter()
        .find(|row| row["agent_id"] == label.as_str())
        .expect("the entrant is ranked");
    let disclosed = &entrant["cell_isolation"];
    assert_eq!(disclosed["schema_version"], "sharpebench.cell-isolation.v1");
    assert_eq!(disclosed["class"], "operator_endpoint");
    assert_eq!(disclosed["runner_discards_state_between_cells"], false);
    assert_eq!(disclosed["detects_state_carryover"], false);
    assert_eq!(disclosed["rank_neutral"], true);
    for row in rows.iter().filter(|row| row["agent_id"] != label.as_str()) {
        assert!(
            row.get("cell_isolation").is_none(),
            "a reference row has no runner to disclose: {}",
            row["agent_id"]
        );
    }
}

/// The only way an entrant could name a class is a field on its decision, and
/// the closed contract refuses unknown fields rather than ignoring them.
#[test]
fn a_decision_cannot_carry_the_class() {
    assert!(sharpebench_protocol::decision_from_wire(CLAIMING_DECISION).is_ok());
    assert!(sharpebench_protocol::decision_from_wire(
        r#"{"orders":[],"cell_isolation":"container_per_cell"}"#
    )
    .is_err());
}

/// The class follows the entrant id a checkpoint is bound to, so a sweep
/// written under the operator-endpoint runner refuses to continue under the
/// host-process runner and leaves its file alone.
#[test]
fn a_checkpoint_does_not_resume_under_another_runner_class() {
    let scratch = Scratch::new("resume");
    let data = scratch.path("data.csv");
    let checkpoint = scratch.path("checkpoint.json");
    let endpoint = Endpoint::start();
    let first = run(&data, &["--http", &endpoint.addr], Some(&checkpoint));
    assert!(
        first.status.success(),
        "{}",
        String::from_utf8_lossy(&first.stderr)
    );
    let written = std::fs::read(&checkpoint).unwrap();

    // A program that exists everywhere this suite runs, so the launch preflight
    // passes and the refusal is the checkpoint's.
    let host_program = env!("CARGO_BIN_EXE_sharpebench");
    assert!(
        !host_program.contains(char::is_whitespace),
        "--cmd splits on whitespace"
    );
    let resumed = run(&data, &["--cmd", host_program], Some(&checkpoint));
    assert!(!resumed.status.success());
    let stderr = String::from_utf8_lossy(&resumed.stderr);
    assert!(stderr.contains("contract differs"), "{stderr}");
    assert_eq!(std::fs::read(&checkpoint).unwrap(), written);
}
