//! `--short-borrow-bps` on `run`, `capture` and `verify-trajectory`.
//!
//! Hermetic: the one external entrant is an in-process HTTP fixture on
//! loopback that holds a short, and no model or market data is used. The
//! tests pin that the flag is validated before anything runs, that it changes
//! the fills of a short entrant, and that the rate is part of the cost-model
//! identity: a checkpoint or trajectory bound under one rate is refused under
//! another, while the default command lines keep the default digest.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::process::{Command, Output};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::Duration;

const BIN: &str = env!("CARGO_BIN_EXE_sharpebench");

/// The cost-model digest of `CostModel::default()`, unchanged by the field.
const DEFAULT_COST_DIGEST: &str =
    "2076df8b8a55406134d960434e051e64859bf6c54e4bffb212e353b1e111442a";

/// An entrant that holds half its NAV short in the one fixture symbol.
const HALF_SHORT: &str =
    r#"{"orders":[{"symbol":"FIX","action":"sell","target_weight":-0.5,"confidence":0.5}]}"#;

/// A loopback `/decide` endpoint answering every request with `HALF_SHORT`.
struct Entrant {
    addr: String,
    calls: Arc<AtomicUsize>,
    stopped: Arc<AtomicBool>,
    server: Option<JoinHandle<()>>,
}

impl Entrant {
    fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap().to_string();
        listener.set_nonblocking(true).unwrap();
        let calls = Arc::new(AtomicUsize::new(0));
        let stopped = Arc::new(AtomicBool::new(false));
        let (counter, stop) = (calls.clone(), stopped.clone());
        let server = std::thread::spawn(move || {
            while !stop.load(Ordering::Acquire) {
                let stream = match listener.accept() {
                    Ok((stream, _)) => stream,
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        std::thread::sleep(Duration::from_millis(1));
                        continue;
                    }
                    Err(error) => panic!("fixture accept: {error}"),
                };
                counter.fetch_add(1, Ordering::Relaxed);
                answer(stream);
            }
        });
        Self {
            addr,
            calls,
            stopped,
            server: Some(server),
        }
    }

    fn calls(&self) -> usize {
        self.calls.load(Ordering::Relaxed)
    }
}

impl Drop for Entrant {
    fn drop(&mut self) {
        self.stopped.store(true, Ordering::Release);
        if let Some(server) = self.server.take() {
            let _ = server.join();
        }
    }
}

fn answer(mut stream: TcpStream) {
    stream.set_nonblocking(false).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(3)))
        .unwrap();
    let mut reader = BufReader::new(stream.try_clone().unwrap());
    let mut length = 0;
    loop {
        let mut line = String::new();
        if reader.read_line(&mut line).unwrap_or(0) == 0 {
            return;
        }
        if line == "\r\n" {
            break;
        }
        if let Some(value) = line.to_ascii_lowercase().strip_prefix("content-length:") {
            length = value.trim().parse::<usize>().unwrap();
        }
    }
    let mut request = vec![0; length];
    if reader.read_exact(&mut request).is_err() {
        return;
    }
    let _ = write!(
        stream,
        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        HALF_SHORT.len(),
        HALF_SHORT
    );
}

struct Fixture(PathBuf);

impl Fixture {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let dir = std::env::temp_dir().join(format!(
            "sharpebench-borrow-cli-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&dir).unwrap();
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

    fn cli(&self, args: &[&str]) -> Output {
        Command::new(BIN)
            .current_dir(&self.0)
            .args(args)
            .output()
            .unwrap()
    }

    fn json(&self, name: &str) -> serde_json::Value {
        serde_json::from_slice(&std::fs::read(self.path(name)).unwrap()).unwrap()
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

#[test]
fn a_malformed_borrow_rate_is_refused_before_anything_runs() {
    let fixture = Fixture::new();
    for command in [
        &["run"][..],
        &["capture", "buy-and-hold", "never.json"][..],
        &["verify-trajectory", "absent.json"][..],
    ] {
        for (value, reason) in [
            ("-1", "must be finite and >= 0"),
            ("NaN", "must be finite and >= 0"),
            ("inf", "must be finite and >= 0"),
            ("abc", "must be a number of basis points"),
            ("--json", "requires a rate in basis points"),
        ] {
            let mut args = command.to_vec();
            args.extend(["--short-borrow-bps", value]);
            let output = fixture.cli(&args);
            assert_eq!(
                output.status.code(),
                Some(2),
                "{args:?}: {}",
                stderr(&output)
            );
            assert!(output.stdout.is_empty(), "{args:?}");
            assert!(
                stderr(&output).contains(reason),
                "{args:?}: {}",
                stderr(&output)
            );
        }
    }
    assert!(!fixture.path("never.json").exists());

    // The external capture path builds the same cost model, so a rate outside
    // its domain is refused there too, before the entrant is contacted. The
    // address below is the discard port, which nothing answers.
    let output = fixture.cli(&[
        "capture",
        "out.json",
        "--http",
        "127.0.0.1:9",
        "--short-borrow-bps",
        "-1",
    ]);
    assert_eq!(output.status.code(), Some(2), "{}", stderr(&output));
    assert!(stderr(&output).contains("must be finite and >= 0"));
    assert!(!fixture.path("out.json").exists());
}

#[test]
fn a_captured_trajectory_verifies_only_under_its_own_borrow_rate() {
    let fixture = Fixture::new();
    let plain = fixture.cli(&["capture", "buy-and-hold", "plain.json", "--json"]);
    assert!(plain.status.success(), "{}", stderr(&plain));
    let borrowed = fixture.cli(&[
        "capture",
        "buy-and-hold",
        "borrowed.json",
        "--short-borrow-bps",
        "25",
        "--json",
    ]);
    assert!(borrowed.status.success(), "{}", stderr(&borrowed));

    let plain_digest = fixture.json("plain.json")["contract"]["cost_model_sha256"].clone();
    let borrowed_digest = fixture.json("borrowed.json")["contract"]["cost_model_sha256"].clone();
    assert_eq!(
        plain_digest, DEFAULT_COST_DIGEST,
        "the default command line"
    );
    assert_ne!(borrowed_digest, DEFAULT_COST_DIGEST);

    let verify = |file: &str, extra: &[&str]| {
        let mut args = vec!["verify-trajectory", file, "--json"];
        args.extend_from_slice(extra);
        fixture.cli(&args)
    };
    assert!(verify("plain.json", &[]).status.success());
    assert!(verify("borrowed.json", &["--short-borrow-bps", "25"])
        .status
        .success());
    for (file, extra) in [
        ("borrowed.json", &[][..]),
        ("borrowed.json", &["--short-borrow-bps", "30"][..]),
        ("plain.json", &["--short-borrow-bps", "25"][..]),
    ] {
        let refused = verify(file, extra);
        assert_eq!(refused.status.code(), Some(1), "{file} {extra:?}");
        assert!(
            stderr(&refused).contains("does not match verifier cost model"),
            "{file} {extra:?}: {}",
            stderr(&refused)
        );
    }
}

/// The per-cell return series a checkpoint stored, in task order.
fn stored_returns(checkpoint: &serde_json::Value) -> Vec<Vec<f64>> {
    checkpoint["tasks"]
        .as_array()
        .unwrap()
        .iter()
        .map(|task| {
            task["run"]["returns"]
                .as_array()
                .unwrap()
                .iter()
                .map(|r| r.as_f64().unwrap())
                .collect()
        })
        .collect()
}

#[test]
fn a_short_entrant_pays_borrow_and_its_checkpoint_refuses_another_rate() {
    let fixture = Fixture::new();
    let entrant = Entrant::start();
    let digest = "ab".repeat(32);
    let run = |checkpoint: &str, extra: &[&str]| {
        let mut args = vec![
            "run",
            "--http",
            &entrant.addr,
            "--data",
            "data.csv",
            "--json",
            "--entrant-sha256",
            &digest,
            "--checkpoint",
            checkpoint,
        ];
        args.extend_from_slice(extra);
        fixture.cli(&args)
    };

    let free = run("free.json", &[]);
    assert!(free.status.success(), "{}", stderr(&free));
    let first = run("borrowed.json", &["--short-borrow-bps", "25"]);
    assert!(first.status.success(), "{}", stderr(&first));
    assert_ne!(first.stdout, free.stdout, "the short entrant's row moves");

    let free_cp = fixture.json("free.json");
    let borrowed_cp = fixture.json("borrowed.json");
    assert_eq!(
        free_cp["contract"]["cost_model_sha256"],
        DEFAULT_COST_DIGEST
    );
    assert_ne!(
        borrowed_cp["contract"]["cost_model_sha256"],
        DEFAULT_COST_DIGEST
    );
    // Half the NAV short at 25 bps per step: every bar of every cell returns
    // less once the rate is set, and nothing else about the cell changes.
    let free_returns = stored_returns(&free_cp);
    let borrowed_returns = stored_returns(&borrowed_cp);
    assert_eq!(free_returns.len(), 16);
    for (free, borrowed) in free_returns.iter().zip(&borrowed_returns) {
        assert_eq!(free.len(), borrowed.len());
        assert!(!free.is_empty());
        for (f, b) in free.iter().zip(borrowed) {
            assert!(b < f, "borrow must lower every return: {f} vs {b}");
        }
    }

    // The same rate resumes the complete checkpoint without executing.
    let written = std::fs::read(fixture.path("borrowed.json")).unwrap();
    let calls = entrant.calls();
    let resumed = run("borrowed.json", &["--short-borrow-bps", "25"]);
    assert!(resumed.status.success(), "{}", stderr(&resumed));
    assert_eq!(resumed.stdout, first.stdout);
    assert_eq!(entrant.calls(), calls);

    // A different rate, or none, is a different experiment.
    for extra in [&["--short-borrow-bps", "30"][..], &[][..]] {
        let refused = run("borrowed.json", extra);
        assert_eq!(refused.status.code(), Some(1), "{extra:?}");
        assert!(refused.stdout.is_empty());
        assert!(
            stderr(&refused).contains("contract differs"),
            "{}",
            stderr(&refused)
        );
        assert_eq!(
            std::fs::read(fixture.path("borrowed.json")).unwrap(),
            written
        );
        assert_eq!(entrant.calls(), calls, "refused before execution");
    }
}
