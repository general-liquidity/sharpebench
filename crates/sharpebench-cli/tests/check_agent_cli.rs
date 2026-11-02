//! `init-agent` writes templates that pass `check-agent` end to end, and
//! `check-agent` names the gate a broken entrant fails.
//!
//! The templates are Python, so these tests need a Python 3 interpreter on
//! PATH (`python3`, else `python`). Without one a test prints why it skipped.
//! Nothing here reaches beyond loopback: the HTTP template listens on
//! 127.0.0.1, and the image test builds only from a base image already
//! present locally.

use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};

fn sharpebench(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_sharpebench"))
        .args(args)
        .output()
        .expect("the sharpebench binary runs")
}

/// A Python 3 interpreter on PATH, or `None` with the reason printed.
fn python() -> Option<String> {
    for name in ["python3", "python"] {
        let probe = Command::new(name)
            .args(["-c", "import sys; print(sys.version_info[0])"])
            .output();
        if let Ok(out) = probe {
            if out.status.success() && String::from_utf8_lossy(&out.stdout).trim() == "3" {
                return Some(name.to_string());
            }
        }
    }
    eprintln!(
        "skipped: no Python 3 interpreter (python3 or python) on PATH; the templates are Python"
    );
    None
}

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "sharpebench-check-agent-{}-{name}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn json_of(output: &Output) -> serde_json::Value {
    serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!(
            "stdout is not JSON ({error}): {}\nstderr: {}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        )
    })
}

/// Gate name to status, in report order.
fn statuses(report: &serde_json::Value) -> Vec<(String, String)> {
    report["gates"]
        .as_array()
        .unwrap()
        .iter()
        .map(|g| {
            (
                g["gate"].as_str().unwrap().to_string(),
                g["status"].as_str().unwrap().to_string(),
            )
        })
        .collect()
}

fn detail(report: &serde_json::Value, gate: &str) -> String {
    report["gates"]
        .as_array()
        .unwrap()
        .iter()
        .find(|g| g["gate"] == gate)
        .and_then(|g| g["detail"].as_str())
        .unwrap_or_default()
        .to_string()
}

fn init(dir: &Path, kind: &str, python: &str) {
    let out = sharpebench(&[
        "init-agent",
        dir.to_str().unwrap(),
        "--kind",
        kind,
        "--python",
        python,
        "--json",
    ]);
    assert_eq!(
        out.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(json_of(&out)["created"], true);
}

/// A clean template passes every gate that can fail; the host transports are
/// noncertifying on the network gate, and the verdict on the short synthetic
/// window is reported, never failed.
fn assert_template_passes(report: &serde_json::Value, transport: &str) {
    assert_eq!(report["schema_version"], "sharpebench.check-agent.v1");
    assert_eq!(report["ok"], true, "{report:#}");
    let got = statuses(report);
    let expected = [
        ("launch", "pass"),
        ("transport", "pass"),
        ("protocol", "pass"),
        ("budget", "pass"),
        ("determinism", "pass"),
        ("network", "advisory"),
    ];
    for (gate, status) in expected {
        assert!(
            got.contains(&(gate.to_string(), status.to_string())),
            "{gate} should be {status}: {got:?}"
        );
    }
    assert!(got
        .iter()
        .any(|(gate, status)| gate == "verdict" && status != "fail"));
    assert_eq!(report["run_with"][0], transport);
    assert!(detail(report, "protocol").starts_with("120 decisions"));
    assert!(report["headline"]["verdict"].is_string());
}

/// The HTTP template, started on a free port and stopped when dropped.
struct Server(Child);

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

#[test]
fn the_cmd_template_passes_check_agent() {
    let Some(python) = python() else { return };
    let root = scratch("cmd");
    let dir = root.join("my-agent");
    init(&dir, "cmd", &python);
    for file in ["agent.py", "README.md", "sharpebench-agent.json"] {
        assert!(dir.join(file).is_file(), "{file}");
    }
    // Run from elsewhere: the spec resolves agent.py against its directory.
    let out = sharpebench(&["check-agent", dir.to_str().unwrap(), "--json"]);
    let report = json_of(&out);
    assert_eq!(out.status.code(), Some(0), "{report:#}");
    assert_template_passes(&report, "--cmd");
    assert!(String::from_utf8_lossy(&out.stderr).contains("NO sandbox"));

    // The human report says the same and names the next command.
    let out = sharpebench(&["check-agent", dir.to_str().unwrap()]);
    let text = String::from_utf8_lossy(&out.stdout);
    assert_eq!(out.status.code(), Some(0));
    assert!(text.contains("seeds 0 and 1"), "{text}");
    assert!(text.contains("no gate failed. Field it with:"), "{text}");
    assert!(text.contains("sharpebench run --cmd"), "{text}");
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn the_http_template_passes_check_agent() {
    let Some(python) = python() else { return };
    let root = scratch("http");
    let dir = root.join("web-agent");
    init(&dir, "http", &python);
    let mut child = Command::new(&python)
        .arg(dir.join("server.py"))
        .args(["--port", "0"])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("the HTTP template starts");
    let stdout = child.stdout.take().unwrap();
    let server = Server(child);
    let mut line = String::new();
    BufReader::new(stdout).read_line(&mut line).unwrap();
    let addr = line
        .trim()
        .strip_prefix("listening on ")
        .unwrap_or_else(|| panic!("unexpected startup line `{line}`"))
        .to_string();
    let out = sharpebench(&["check-agent", "--http", &addr, "--json"]);
    let report = json_of(&out);
    assert_eq!(out.status.code(), Some(0), "{report:#}");
    assert_template_passes(&report, "--http");
    assert_eq!(report["agent_id"], format!("http:{addr}"));
    drop(server);
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn the_image_template_is_complete_and_refused_until_pinned() {
    let root = scratch("image");
    let dir = root.join("Boxed Agent");
    init(&dir, "image", "python3");
    let dockerfile = std::fs::read_to_string(dir.join("Dockerfile")).unwrap();
    assert!(dockerfile.contains("COPY agent.py /agent/agent.py"));
    assert!(dockerfile.contains(r#"ENTRYPOINT ["python", "/agent/agent.py"]"#));
    assert!(dockerfile.contains("PYTHONDONTWRITEBYTECODE=1"));
    let spec: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(dir.join("sharpebench-agent.json")).unwrap())
            .unwrap();
    assert_eq!(spec["kind"], "image");
    assert_eq!(spec["image"], "boxed-agent");
    // The image template runs the same policy file as the cmd template.
    let cmd_dir = root.join("plain");
    init(&cmd_dir, "cmd", "python3");
    assert_eq!(
        std::fs::read(dir.join("agent.py")).unwrap(),
        std::fs::read(cmd_dir.join("agent.py")).unwrap()
    );

    // An unpinned reference never launches, with or without a daemon.
    let out = sharpebench(&["check-agent", dir.to_str().unwrap(), "--json"]);
    assert_eq!(out.status.code(), Some(1));
    let report = json_of(&out);
    assert_eq!(
        statuses(&report)[0],
        ("launch".to_string(), "fail".to_string())
    );
    assert!(detail(&report, "launch").contains("digest-pinned"));
    assert!(detail(&report, "network").contains("--network none"));

    docker_build(&dir);
    let _ = std::fs::remove_dir_all(&root);
}

/// Build the generated Dockerfile when Docker is available and the base image
/// is already present, so the build reaches no registry.
fn docker_build(dir: &Path) {
    const BASE: &str = "python:3.12-slim";
    let ok = |args: &[&str]| {
        Command::new("docker")
            .args(args)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_ok_and(|status| status.success())
    };
    if !ok(&["version"]) {
        eprintln!("skipped the image build: no Docker daemon is reachable");
        return;
    }
    if !ok(&["image", "inspect", BASE]) {
        eprintln!("skipped the image build: the base image {BASE} is not present locally, and the test does not pull");
        return;
    }
    let tag = format!("sharpebench-init-agent-test:{}", std::process::id());
    let built = Command::new("docker")
        .args(["build", "--pull=false", "--network", "none", "-t", &tag])
        .arg(dir)
        .output()
        .unwrap();
    let _ = ok(&["image", "rm", "-f", &tag]);
    assert!(
        built.status.success(),
        "{}",
        String::from_utf8_lossy(&built.stderr)
    );
}

/// Each planted entrant fails exactly the gate its defect belongs to, and the
/// report says why in words.
#[test]
fn broken_entrants_fail_the_gate_their_defect_belongs_to() {
    let Some(python) = python() else { return };
    let root = scratch("broken");
    let cases = [
        (
            "extra_key.py",
            "import sys, json\nfor line in sys.stdin:\n    print(json.dumps({'orders': [], 'note': 1}), flush=True)\n",
            "protocol",
            "wire contract refuses",
        ),
        (
            "unseen_symbol.py",
            "import sys, json\nfor line in sys.stdin:\n    print(json.dumps({'orders': [{'symbol': 'ZZZ', 'action': 'buy', 'target_weight': 0.5}]}), flush=True)\n",
            "protocol",
            "counts against pass^k",
        ),
        (
            "crash.py",
            "import sys\nsys.stdin.readline()\nsys.exit(3)\n",
            "transport",
            "exited with status 3",
        ),
        (
            "clock.py",
            "import sys, json, random\nfor line in sys.stdin:\n    obs = json.loads(line)\n    s = obs['symbols'][0]['symbol']\n    print(json.dumps({'orders': [{'symbol': s, 'action': 'buy', 'target_weight': round(random.random(), 6)}]}), flush=True)\n",
            "determinism",
            "answered differently when re-executed",
        ),
    ];
    for (file, source, failed, says) in cases {
        let path = root.join(file);
        std::fs::write(&path, source).unwrap();
        let cmd = format!("{python} {}", path.display());
        let out = sharpebench(&["check-agent", "--cmd", &cmd, "--json"]);
        let report = json_of(&out);
        assert_eq!(out.status.code(), Some(1), "{file}: {report:#}");
        assert_eq!(report["ok"], false);
        let failures: Vec<String> = statuses(&report)
            .into_iter()
            .filter(|(_, status)| status == "fail")
            .map(|(gate, _)| gate)
            .collect();
        assert_eq!(failures, [failed], "{file}");
        assert!(detail(&report, failed).contains(says), "{file}: {report:#}");
        if failed == "protocol" {
            // The transport's own diagnostic names the offending field.
            assert!(String::from_utf8_lossy(&out.stderr).contains("agent protocol fault:"));
        }
    }

    // The human report lists the failed gate and points at the book.
    let path = root.join("extra_key.py");
    let out = sharpebench(&[
        "check-agent",
        "--cmd",
        &format!("{python} {}", path.display()),
    ]);
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(text.contains("  FAIL      protocol"), "{text}");
    assert!(
        text.contains("failed: protocol. The book page \"Submit your agent\""),
        "{text}"
    );
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn usage_errors_exit_2_and_help_exits_0() {
    for args in [
        vec!["check-agent"],
        vec!["check-agent", "--cmd"],
        vec!["check-agent", "--cmd", "a", "--image", "b"],
        vec!["init-agent"],
        vec!["init-agent", "x", "--kind", "lambda"],
    ] {
        let out = sharpebench(&args);
        assert_eq!(out.status.code(), Some(2), "{args:?}");
        assert!(
            String::from_utf8_lossy(&out.stderr).contains("usage: sharpebench"),
            "{args:?}"
        );
    }
    let out = sharpebench(&["check-agent", "--help"]);
    assert_eq!(out.status.code(), Some(0));
    assert!(String::from_utf8_lossy(&out.stdout).starts_with("usage: sharpebench check-agent"));
}
