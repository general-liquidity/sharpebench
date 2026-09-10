//! The three harness features the CLI now exposes: `run --fault-plan`,
//! `run --retry-backoff` and `verify-trajectory --reexecute`, with the fault
//! report of an incomplete sweep. Hermetic: every entrant is an in-process
//! HTTP fixture on loopback, and no model or market data is used.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::process::{Command, Output};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::Duration;

const BIN: &str = env!("CARGO_BIN_EXE_sharpebench");

/// What the fixture entrant answers to its `call`-th request (0-based):
/// `Some(body)` is a framed decision, `None` a transport failure.
type Policy = fn(usize) -> Option<String>;

/// A loopback `/decide` endpoint running `policy`, counting its requests.
struct Entrant {
    addr: String,
    calls: Arc<AtomicUsize>,
    stopped: Arc<AtomicBool>,
    server: Option<JoinHandle<()>>,
}

impl Entrant {
    fn start(policy: Policy) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap().to_string();
        listener.set_nonblocking(true).unwrap();
        let calls = Arc::new(AtomicUsize::new(0));
        let stopped = Arc::new(AtomicBool::new(false));
        let (counter, stop) = (calls.clone(), stopped.clone());
        let server = std::thread::spawn(move || {
            while !stop.load(Ordering::Acquire) {
                let (stream, _) = match listener.accept() {
                    Ok(connection) => connection,
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        std::thread::sleep(Duration::from_millis(1));
                        continue;
                    }
                    Err(error) => panic!("fixture accept: {error}"),
                };
                let call = counter.fetch_add(1, Ordering::Relaxed);
                answer(stream, policy(call));
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

fn answer(mut stream: TcpStream, body: Option<String>) {
    let Some(body) = body else {
        // Incomplete HTTP framing is a transport error, not a protocol fault.
        let _ = stream.write_all(b"unframed response");
        return;
    };
    // Accepted sockets can inherit the listener's nonblocking mode.
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
        body.len(),
        body
    );
}

/// The decision `buy-and-hold` makes on the one-symbol fixture dataset.
const BUY_AND_HOLD: &str =
    r#"{"orders":[{"symbol":"FIX","action":"buy","target_weight":1.0,"confidence":0.5}]}"#;

fn deterministic(_: usize) -> Option<String> {
    Some(BUY_AND_HOLD.to_string())
}

/// Carries state across runs: its answer depends on how many requests it has
/// served, not only on the observations of the run.
fn call_counting(call: usize) -> Option<String> {
    let confidence = if call.is_multiple_of(2) {
        "0.5"
    } else {
        "0.25"
    };
    Some(format!(
        r#"{{"orders":[{{"symbol":"FIX","action":"buy","target_weight":1.0,"confidence":{confidence}}}]}}"#
    ))
}

fn unreachable_transport(_: usize) -> Option<String> {
    None
}

struct Fixture(PathBuf);

impl Fixture {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let dir = std::env::temp_dir().join(format!(
            "sharpebench-fault-cli-{}-{}",
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

    fn write(&self, name: &str, text: &str) {
        std::fs::write(self.path(name), text).unwrap();
    }

    fn cli(&self, args: &[&str]) -> Output {
        Command::new(BIN)
            .current_dir(&self.0)
            .args(args)
            .output()
            .unwrap()
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn stdout_json(output: &Output) -> serde_json::Value {
    serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!(
            "stdout is not JSON ({error}); stderr: {}",
            String::from_utf8_lossy(&output.stderr)
        )
    })
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

fn plan(seed: u64) -> String {
    serde_json::json!({
        "schema_version": "sharpebench.fault-plan.v1",
        "seed": seed,
        "declared_relaxations": ["submission_acceptance", "read_your_writes"],
        "faults": [
            {"id": "limit", "cohort_ppm": 1_000_000,
             "fault": {"mode": "rate_limit", "max_rejected_presentations": 2}},
            {"id": "lag", "cohort_ppm": 1_000_000,
             "fault": {"mode": "projection_lag", "max_lag_steps": 2}},
        ],
    })
    .to_string()
}

fn entrant_row<'a>(board: &'a serde_json::Value, prefix: &str) -> &'a serde_json::Value {
    board
        .as_array()
        .expect("a board is an array")
        .iter()
        .find(|row| row["agent_id"].as_str().unwrap().starts_with(prefix))
        .expect("the entrant row is present")
}

/// The board with the entrant's operational metadata removed: what scoring saw.
fn scored_rows(mut board: serde_json::Value) -> serde_json::Value {
    for row in board.as_array_mut().unwrap() {
        let row = row.as_object_mut().unwrap();
        row.remove("attempt_accounting");
        row.remove("fault_injection");
    }
    board
}

#[test]
fn a_fault_plan_is_injected_at_the_entrant_boundary_and_reported_rank_neutral() {
    let fixture = Fixture::new();
    fixture.write("plan.json", &plan(11));
    let entrant = Entrant::start(deterministic);
    let run = |extra: &[&str]| {
        let mut args = vec![
            "run",
            "--http",
            &entrant.addr,
            "--data",
            "data.csv",
            "--json",
        ];
        args.extend_from_slice(extra);
        fixture.cli(&args)
    };
    let plain = run(&[]);
    let plain_calls = entrant.calls();
    let faulted = run(&["--fault-plan", "plan.json"]);
    let faulted_calls = entrant.calls() - plain_calls;
    assert!(plain.status.success(), "{}", stderr(&plain));
    assert!(faulted.status.success(), "{}", stderr(&faulted));
    assert!(
        faulted_calls > plain_calls,
        "a rate limit re-presents observations: {faulted_calls} vs {plain_calls}"
    );

    let plain = stdout_json(&plain);
    let faulted = stdout_json(&faulted);
    assert!(entrant_row(&plain, "http:")
        .get("fault_injection")
        .is_none());
    let report = &entrant_row(&faulted, "http:")["fault_injection"];
    assert_eq!(
        report["schema_version"],
        "sharpebench.fault-injection-report.v1"
    );
    assert_eq!(report["rank_neutral"], true);
    let digest = report["plan_sha256"].as_str().unwrap();
    assert_eq!(digest.len(), 64);
    assert_eq!(
        report["declared_relaxations"],
        serde_json::json!(["read_your_writes", "submission_acceptance"])
    );
    assert!(report["entrant_declaration"]
        .as_str()
        .unwrap()
        .contains("read-your-writes is relaxed"));
    let denominators = report["denominators"].as_array().unwrap();
    assert_eq!(denominators.len(), 2);
    for row in denominators {
        assert_eq!(row["cells"], 16);
        assert_eq!(row["assigned"], 16, "every cell is in a 1e6 ppm cohort");
    }
    let limit = denominators
        .iter()
        .find(|row| row["fault_id"] == "limit")
        .unwrap();
    assert_eq!(
        limit["fired"], 16,
        "an entrant that always orders is limited"
    );
    let evidence = report["evidence"].as_array().unwrap();
    assert_eq!(evidence.len(), 16, "one completed attempt per cell");
    assert!(evidence.iter().all(|e| e["plan_sha256"] == digest));
    // The entrant ignores the perturbed fields and restates its decision, so
    // scoring sees exactly the unfaulted run.
    assert_eq!(scored_rows(plain), scored_rows(faulted));
}

#[test]
fn a_changed_fault_plan_refuses_to_resume_its_checkpoint() {
    let fixture = Fixture::new();
    fixture.write("plan.json", &plan(11));
    let entrant = Entrant::start(deterministic);
    let digest = "ab".repeat(32);
    let run = |extra: &[&str]| {
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
            "checkpoint.json",
        ];
        args.extend_from_slice(extra);
        fixture.cli(&args)
    };
    let first = run(&["--fault-plan", "plan.json"]);
    assert!(first.status.success(), "{}", stderr(&first));
    let written = std::fs::read(fixture.path("checkpoint.json")).unwrap();
    assert!(String::from_utf8_lossy(&written).contains("injected_faults"));
    let calls = entrant.calls();

    // The same plan, reformatted, is the same experiment: it resumes a
    // complete checkpoint without executing anything.
    let pretty: serde_json::Value = serde_json::from_str(&plan(11)).unwrap();
    fixture.write("plan.json", &serde_json::to_string_pretty(&pretty).unwrap());
    let resumed = run(&["--fault-plan", "plan.json"]);
    assert!(resumed.status.success(), "{}", stderr(&resumed));
    assert_eq!(resumed.stdout, first.stdout);
    assert_eq!(entrant.calls(), calls);

    // A changed plan, or no plan at all, is a different experiment.
    fixture.write("changed.json", &plan(12));
    for extra in [&["--fault-plan", "changed.json"][..], &[][..]] {
        let refused = run(extra);
        assert_eq!(refused.status.code(), Some(1), "{extra:?}");
        assert!(refused.stdout.is_empty());
        assert!(
            stderr(&refused).contains("contract differs"),
            "{}",
            stderr(&refused)
        );
        assert_eq!(
            std::fs::read(fixture.path("checkpoint.json")).unwrap(),
            written
        );
        assert_eq!(entrant.calls(), calls, "refused before execution");
    }
}

#[test]
fn a_malformed_or_unusable_fault_plan_refuses_before_launch() {
    let fixture = Fixture::new();
    fixture.write("bad-json.json", "{not json");
    fixture.write(
        "unknown-field.json",
        &plan(1).replacen("\"seed\"", "\"surprise\":1,\"seed\"", 1),
    );
    fixture.write(
        "undeclared.json",
        &plan(1).replace("[\"submission_acceptance\",\"read_your_writes\"]", "[]"),
    );
    fixture.write(
        "unarmable.json",
        &serde_json::json!({
            "schema_version": "sharpebench.fault-plan.v1",
            "seed": 1,
            "declared_relaxations": ["complete_results"],
            "faults": [{"id": "page", "cohort_ppm": 1,
                        "fault": {"mode": "limit_before_sort", "page_size": 5}}],
        })
        .to_string(),
    );
    fixture.write("valid.json", &plan(1));
    let cases: [(&[&str], &str); 7] = [
        (
            &["run", "--fault-plan", "valid.json"],
            "requires an external-agent transport",
        ),
        (
            &["run", "--cmd", "must-not-launch", "--fault-plan"],
            "requires a JSON file path",
        ),
        (
            &[
                "run",
                "--cmd",
                "must-not-launch",
                "--fault-plan",
                "missing.json",
            ],
            "cannot open fault plan",
        ),
        (
            &[
                "run",
                "--cmd",
                "must-not-launch",
                "--fault-plan",
                "bad-json.json",
            ],
            "invalid fault plan",
        ),
        (
            &[
                "run",
                "--cmd",
                "must-not-launch",
                "--fault-plan",
                "unknown-field.json",
            ],
            "unknown field",
        ),
        (
            &[
                "run",
                "--cmd",
                "must-not-launch",
                "--fault-plan",
                "undeclared.json",
            ],
            "declared_relaxations",
        ),
        (
            &[
                "run",
                "--cmd",
                "must-not-launch",
                "--fault-plan",
                "unarmable.json",
            ],
            "row 29",
        ),
    ];
    for (args, message) in cases {
        let output = fixture.cli(args);
        assert_eq!(
            output.status.code(),
            Some(2),
            "{args:?}: {}",
            stderr(&output)
        );
        assert!(output.stdout.is_empty(), "{args:?}");
        let error = stderr(&output);
        assert!(error.contains(message), "{args:?}: {error}");
        assert!(
            !error.contains("cannot spawn"),
            "{args:?}: launched: {error}"
        );
        assert!(!error.contains("NO sandbox"), "{args:?}: launched: {error}");
    }
}

#[test]
fn retry_backoff_waits_are_recorded_and_bound_into_the_checkpoint() {
    let fixture = Fixture::new();
    let entrant = Entrant::start(unreachable_transport);
    let digest = "ab".repeat(32);
    let run = |extra: &[&str]| {
        let mut args = vec!["run", "--http", &entrant.addr, "--json"];
        args.extend_from_slice(extra);
        fixture.cli(&args)
    };

    // Unpersisted: every cell fails three times and waits 1 ms, then 2 ms.
    let immediate = run(&[]);
    let backed_off = run(&["--retry-backoff", "1,2"]);
    for output in [&immediate, &backed_off] {
        assert_eq!(output.status.code(), Some(1));
    }
    let immediate = stdout_json(&immediate);
    let backed_off = stdout_json(&backed_off);
    assert!(immediate["attempt_accounting"]["attempts"]
        .get("backoff_ns_total")
        .is_none());
    let attempts = &backed_off["attempt_accounting"]["attempts"];
    assert_eq!(attempts["attempts"], 48);
    assert_eq!(attempts["backoff_ns_total"], 16 * 3_000_000);

    // Persisted: the waits are on the ledger, and the schedule is identity.
    let checkpointed = |extra: &[&str]| {
        let mut args = vec![
            "--entrant-sha256",
            &digest,
            "--checkpoint",
            "checkpoint.json",
        ];
        args.extend_from_slice(extra);
        run(&args)
    };
    let first = checkpointed(&["--retry-backoff", "1,2"]);
    assert_eq!(first.status.code(), Some(1));
    let written = std::fs::read(fixture.path("checkpoint.json")).unwrap();
    let checkpoint: serde_json::Value = serde_json::from_slice(&written).unwrap();
    let waits: Vec<(u64, u64)> = checkpoint["tasks"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|task| task["attempts"]["attempts"].as_array().unwrap().clone())
        .filter_map(|record| {
            let backoff = record.get("backoff_after")?;
            Some((
                backoff["retry"].as_u64().unwrap(),
                backoff["delay_ns"].as_u64().unwrap(),
            ))
        })
        .collect();
    assert_eq!(waits.len(), 32, "two waits per exhausted cell");
    assert_eq!(waits.iter().map(|(_, ns)| ns).sum::<u64>(), 16 * 3_000_000);
    assert!(waits.contains(&(1, 1_000_000)) && waits.contains(&(2, 2_000_000)));
    let calls = entrant.calls();
    for extra in [&["--retry-backoff", "1,3"][..], &[][..]] {
        let refused = checkpointed(extra);
        assert_eq!(refused.status.code(), Some(1), "{extra:?}");
        assert!(
            stderr(&refused).contains("contract differs"),
            "{}",
            stderr(&refused)
        );
        assert_eq!(
            std::fs::read(fixture.path("checkpoint.json")).unwrap(),
            written
        );
        assert_eq!(entrant.calls(), calls, "refused before execution");
    }
}

#[test]
fn a_malformed_retry_backoff_refuses_before_launch() {
    let fixture = Fixture::new();
    for (value, message) in [
        ("", "whole milliseconds"),
        ("abc", "whole milliseconds"),
        ("-1", "whole milliseconds"),
        ("+5", "whole milliseconds"),
        ("1,,2", "whole milliseconds"),
        ("1.5", "whole milliseconds"),
        ("600001", "whole milliseconds"),
        ("1,2,3", "at most 2 times per round"),
    ] {
        let output = fixture.cli(&["run", "--cmd", "must-not-launch", "--retry-backoff", value]);
        assert_eq!(
            output.status.code(),
            Some(2),
            "{value:?}: {}",
            stderr(&output)
        );
        assert!(output.stdout.is_empty());
        let error = stderr(&output);
        assert!(error.contains(message), "{value:?}: {error}");
        assert!(
            !error.contains("NO sandbox"),
            "{value:?}: launched: {error}"
        );
    }
    for (args, message) in [
        (
            &["run", "--retry-backoff", "5"][..],
            "requires an external-agent transport",
        ),
        (
            &["run", "--cmd", "must-not-launch", "--retry-backoff"][..],
            "comma-separated",
        ),
    ] {
        let output = fixture.cli(args);
        assert_eq!(output.status.code(), Some(2), "{args:?}");
        assert!(stderr(&output).contains(message), "{}", stderr(&output));
    }
}

#[test]
fn reexecution_passes_a_deterministic_entrant_and_refuses_a_divergent_one() {
    let fixture = Fixture::new();
    let captured = fixture.cli(&[
        "capture",
        "buy-and-hold",
        "trajectory.json",
        "--data",
        "data.csv",
    ]);
    assert!(captured.status.success(), "{}", stderr(&captured));
    let verify = |extra: &[&str]| {
        let mut args = vec![
            "verify-trajectory",
            "trajectory.json",
            "--data",
            "data.csv",
            "--json",
        ];
        args.extend_from_slice(extra);
        fixture.cli(&args)
    };

    // Strict replay alone is unchanged by the new flag's existence.
    let replayed = verify(&[]);
    assert!(replayed.status.success(), "{}", stderr(&replayed));
    assert!(stdout_json(&replayed).get("reexecution").is_none());

    let reference = verify(&["--reexecute"]);
    assert!(reference.status.success(), "{}", stderr(&reference));
    let reference = stdout_json(&reference);
    assert_eq!(reference["reexecution"]["agent"], "buy-and-hold");
    assert_eq!(reference["reexecution"]["runs_reexecuted"], 16);
    let mut without = reference.clone();
    without.as_object_mut().unwrap().remove("reexecution");
    assert_eq!(without, stdout_json(&replayed));

    let steady = Entrant::start(deterministic);
    let passed = verify(&["--reexecute", "--http", &steady.addr]);
    assert!(passed.status.success(), "{}", stderr(&passed));
    assert_eq!(
        stdout_json(&passed)["reexecution"]["agent"],
        format!("http:{}", steady.addr)
    );

    let drifting = Entrant::start(call_counting);
    let refused = verify(&["--reexecute", "--http", &drifting.addr]);
    assert_eq!(refused.status.code(), Some(1));
    let refusal = stdout_json(&refused);
    assert_eq!(refusal["verified"], false);
    assert_eq!(refusal["error"], "reexecution_diverged");
    assert_eq!(refusal["divergence"]["run"], 0);
    assert_eq!(refusal["divergence"]["step"], 1);
    assert!(refusal["divergence"]["reexecuted"]
        .as_str()
        .unwrap()
        .contains("0.25"));
    assert!(stderr(&refused).contains("re-execution diverged at run 0 step 1"));

    // A broken transport is a transport failure, never read as
    // non-determinism, even though its degrade-to-hold differs from the record.
    let broken = Entrant::start(unreachable_transport);
    let unreachable = verify(&["--reexecute", "--http", &broken.addr]);
    assert_eq!(unreachable.status.code(), Some(1));
    assert_eq!(
        stdout_json(&unreachable)["error"],
        "reexecution_transport_failure"
    );
}

#[test]
fn reexecution_flags_refuse_an_unlaunchable_or_contradictory_request() {
    let fixture = Fixture::new();
    assert!(fixture
        .cli(&[
            "capture",
            "momentum",
            "trajectory.json",
            "--data",
            "data.csv"
        ])
        .status
        .success());
    let mut renamed: serde_json::Value =
        serde_json::from_slice(&std::fs::read(fixture.path("trajectory.json")).unwrap()).unwrap();
    renamed["agent_id"] = serde_json::json!("someone-elses-agent");
    fixture.write("renamed.json", &renamed.to_string());
    for (args, message) in [
        (
            &[
                "verify-trajectory",
                "trajectory.json",
                "--data",
                "data.csv",
                "--http",
                "127.0.0.1:9",
            ][..],
            "require --reexecute",
        ),
        (
            &[
                "verify-trajectory",
                "trajectory.json",
                "--data",
                "data.csv",
                "--reexecute",
                "--allow-unbound-trajectory",
            ][..],
            "cannot be combined",
        ),
        (
            &[
                "verify-trajectory",
                "renamed.json",
                "--data",
                "data.csv",
                "--reexecute",
            ][..],
            "not a reference agent",
        ),
    ] {
        let output = fixture.cli(args);
        assert_eq!(
            output.status.code(),
            Some(2),
            "{args:?}: {}",
            stderr(&output)
        );
        assert!(output.stdout.is_empty());
        assert!(
            stderr(&output).contains(message),
            "{args:?}: {}",
            stderr(&output)
        );
    }
}

/// Serves the first cells, then breaks for good: a sweep that runs some cells
/// to completion and exhausts the rest.
fn early_cells_then_unreachable(call: usize) -> Option<String> {
    (call < 40).then(|| BUY_AND_HOLD.to_string())
}

/// The recorded `injected_faults` of every attempt in a checkpoint, each as its
/// JSON text, sorted: the evidence the report must carry, whatever its order.
fn checkpoint_evidence(path: &std::path::Path) -> Vec<String> {
    let checkpoint: serde_json::Value =
        serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    let mut evidence: Vec<String> = checkpoint["tasks"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|task| task["attempts"]["attempts"].as_array().unwrap().clone())
        .filter_map(|record| record.get("injected_faults").map(|e| e.to_string()))
        .collect();
    evidence.sort();
    evidence
}

fn sorted_evidence(report: &serde_json::Value) -> Vec<String> {
    let mut evidence: Vec<String> = report["evidence"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e.to_string())
        .collect();
    evidence.sort();
    evidence
}

#[test]
fn an_incomplete_faulted_sweep_keeps_its_fault_report() {
    let fixture = Fixture::new();
    fixture.write("plan.json", &plan(11));
    let digest = "ab".repeat(32);
    let run = |entrant: &Entrant, extra: &[&str]| {
        let mut args = vec!["run", "--http", &entrant.addr, "--data", "data.csv"];
        args.extend_from_slice(extra);
        fixture.cli(&args)
    };

    // The report a completed row carries under the same plan.
    let steady = Entrant::start(deterministic);
    let completed = run(&steady, &["--json", "--fault-plan", "plan.json"]);
    assert!(completed.status.success(), "{}", stderr(&completed));
    let completed = stdout_json(&completed);
    let reference = &entrant_row(&completed, "http:")["fault_injection"];

    // Unpersisted: the refusal carries the report for the cells that ran.
    let breaking = Entrant::start(early_cells_then_unreachable);
    let incomplete = run(&breaking, &["--json", "--fault-plan", "plan.json"]);
    assert_eq!(incomplete.status.code(), Some(1), "{}", stderr(&incomplete));
    let refusal = stdout_json(&incomplete);
    assert_eq!(refusal["error"], "incomplete_external_sweep");
    let completed_cells = refusal["completeness"]["completed_cells"].as_u64().unwrap();
    assert!(
        (1..16).contains(&completed_cells),
        "the fixture must complete some cells and exhaust the rest: {completed_cells}"
    );
    let report = &refusal["fault_injection"];
    for key in [
        "schema_version",
        "plan_sha256",
        "declared_relaxations",
        "entrant_declaration",
        "rank_neutral",
    ] {
        assert_eq!(report[key], reference[key], "{key}");
    }
    assert_eq!(report["rank_neutral"], true);
    let plan_digest = report["plan_sha256"].as_str().unwrap();
    for row in report["denominators"].as_array().unwrap() {
        assert_eq!(row["cells"], 16);
        assert_eq!(row["assigned"], 16);
    }
    let limit = report["denominators"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["fault_id"] == "limit")
        .unwrap();
    let fired = limit["fired"].as_u64().unwrap();
    assert!(
        fired >= completed_cells && fired < 16,
        "the limit fired in the cells that ran, not in all: {fired}"
    );
    let evidence = report["evidence"].as_array().unwrap();
    assert!(evidence.len() as u64 >= completed_cells);
    assert!(evidence.iter().all(|e| e["plan_sha256"] == plan_digest));
    // Rank neutral: the refusal still emits no board.
    assert!(refusal.get("board").is_none() && !refusal.is_array());

    // Human mode prints the same report after the attempt accounting.
    let breaking = Entrant::start(early_cells_then_unreachable);
    let human = run(&breaking, &["--fault-plan", "plan.json"]);
    assert_eq!(human.status.code(), Some(1));
    assert!(human.stdout.is_empty());
    let error = stderr(&human);
    assert!(error.contains("is incomplete"), "{error}");
    assert!(
        error.contains(&format!(
            "fault injection for http:{}: plan {plan_digest} (rank-neutral)",
            breaking.addr
        )),
        "{error}"
    );
    assert!(error.contains("  limit: assigned 16 of 16 cells, fired in "));

    // Without a plan the refusal has no fault field and prints no report.
    let breaking = Entrant::start(early_cells_then_unreachable);
    let unfaulted = run(&breaking, &["--json"]);
    assert_eq!(unfaulted.status.code(), Some(1));
    assert!(stdout_json(&unfaulted).get("fault_injection").is_none());
    let breaking = Entrant::start(early_cells_then_unreachable);
    let unfaulted = run(&breaking, &[]);
    assert!(!stderr(&unfaulted).contains("fault injection"));

    // Persisted: the refusal's evidence is exactly what the checkpoint holds.
    let breaking = Entrant::start(early_cells_then_unreachable);
    let checkpointed = run(
        &breaking,
        &[
            "--json",
            "--fault-plan",
            "plan.json",
            "--entrant-sha256",
            &digest,
            "--checkpoint",
            "checkpoint.json",
        ],
    );
    assert_eq!(checkpointed.status.code(), Some(1));
    let report = &stdout_json(&checkpointed)["fault_injection"];
    assert_eq!(report["plan_sha256"], plan_digest);
    let persisted = checkpoint_evidence(&fixture.path("checkpoint.json"));
    assert!(!persisted.is_empty());
    assert_eq!(sorted_evidence(report), persisted);
}
