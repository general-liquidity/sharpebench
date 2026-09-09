//! The real CLI must publish retries even when no score can be emitted.

use std::io::Write;
use std::net::TcpListener;
use std::process::Command;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

#[test]
fn incomplete_sweep_publishes_all_failed_attempts_without_inventing_cost() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    listener.set_nonblocking(true).unwrap();
    let stopped = Arc::new(AtomicBool::new(false));
    let thread_stopped = Arc::clone(&stopped);
    let server = std::thread::spawn(move || {
        while !thread_stopped.load(Ordering::Acquire) {
            match listener.accept() {
                Ok((mut stream, _)) => {
                    // Incomplete HTTP framing is a transport error, not an
                    // agent-protocol fault. Close immediately, never time out.
                    let _ = stream.write_all(b"unframed response");
                }
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    std::thread::sleep(Duration::from_millis(1));
                }
                Err(error) => panic!("local fixture accept failed: {error}"),
            }
        }
    });
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let checkpoint = std::env::temp_dir().join(format!(
        "sharpe-recovery-cli-{}-{}.json",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    let invoke = |recover| {
        let mut command = Command::new(env!("CARGO_BIN_EXE_sharpebench"));
        command
            .args([
                "run",
                "--http",
                &addr.to_string(),
                "--json",
                "--entrant-sha256",
                &"ab".repeat(32),
                "--checkpoint",
            ])
            .arg(&checkpoint);
        if recover {
            command.arg("--retry-runtime-failures");
        }
        command.output().unwrap()
    };
    let output = invoke(false);
    let unchanged = invoke(false);
    let recovered = invoke(true);
    stopped.store(true, Ordering::Release);
    server.join().unwrap();
    std::fs::remove_file(checkpoint).unwrap();
    let unchanged: serde_json::Value = serde_json::from_slice(&unchanged.stdout).unwrap();
    assert_eq!(unchanged["attempt_accounting"]["attempts"]["attempts"], 48);
    assert_eq!(recovered.status.code(), Some(1));
    let recovered: serde_json::Value = serde_json::from_slice(&recovered.stdout).unwrap();
    assert_eq!(recovered["attempt_accounting"]["attempts"]["attempts"], 96);
    assert_eq!(recovered["attempt_accounting"]["attempts"]["failed"], 96);
    assert_eq!(recovered["completeness"]["runtime_failed_cells"], 16);
    assert!(recovered.get("board").is_none());
    assert_eq!(output.status.code(), Some(1));
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["error"], "incomplete_external_sweep");
    assert_eq!(value["completeness"]["runtime_failed_cells"], 16);
    let accounting = &value["attempt_accounting"];
    assert_eq!(
        accounting["schema_version"],
        "sharpebench.attempt-accounting.v1"
    );
    assert_eq!(accounting["attempts"]["attempts"], 48);
    assert_eq!(accounting["attempts"]["failed"], 48);
    assert_eq!(accounting["attempts"]["completed"], 0);
    assert_eq!(accounting["attempts"]["duration_source"], "host_clock");
    assert_eq!(accounting["monetary_cost"]["status"], "unavailable");
    assert!(accounting["monetary_cost"].get("value").is_none());
    assert!(value.get("board").is_none());
}

#[test]
fn recovery_flag_requires_an_external_checkpoint_before_launch() {
    for args in [
        vec!["run", "--retry-runtime-failures"],
        vec![
            "run",
            "--retry-runtime-failures",
            "--cmd",
            "must-not-launch",
        ],
        vec![
            "run",
            "--retry-runtime-failures",
            "--checkpoint",
            "must-not-write.json",
        ],
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_sharpebench"))
            .args(args)
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(2));
        assert!(String::from_utf8_lossy(&output.stderr).contains("requires --checkpoint"));
        assert!(output.stdout.is_empty());
    }
}
