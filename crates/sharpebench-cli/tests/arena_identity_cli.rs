//! The installed CLI end to end on a faulted arena window: an entrant's
//! commitment binds the window's fault plan (`arena commitment --fault-plan`),
//! a commitment without it is refused at reveal, and `arena verify` fails with
//! a non-zero exit when the window file no longer records the plan its signed
//! header names.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};

use sharpebench_attest::{content_digest, make_commitment_under_fault_plan};
use sharpebench_harness::fault_plan::FaultPlan;

struct Fixture(PathBuf);

impl Fixture {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        loop {
            let path = std::env::temp_dir().join(format!(
                "sharpebench-arena-identity-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            match std::fs::create_dir(&path) {
                Ok(()) => return Self(path),
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(e) => panic!("create fixture: {e}"),
            }
        }
    }

    fn path(&self, name: &str) -> PathBuf {
        self.0.join(name)
    }

    fn cli(&self, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_sharpebench"))
            .current_dir(&self.0)
            .args(args)
            .output()
            .unwrap()
    }

    /// Run `args`, require exit `code`, and save stdout to `save` when given.
    fn expect(&self, code: i32, args: &[&str], save: Option<&str>) -> Output {
        let output = self.cli(args);
        assert_eq!(
            output.status.code(),
            Some(code),
            "{args:?}\nstdout: {}\nstderr: {}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        if let Some(name) = save {
            std::fs::write(self.path(name), &output.stdout).unwrap();
        }
        output
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.0).expect("remove exclusively owned fixture");
    }
}

const PLAN: &str = r#"{"schema_version":"sharpebench.fault-plan.v1","seed":7,"declared_relaxations":["submission_acceptance"],"faults":[{"id":"limit","cohort_ppm":1000000,"fault":{"mode":"rate_limit","max_rejected_presentations":2}}]}"#;

fn entry(agent_id: &str, artifact: &str, salt: &str, plan: &str) -> serde_json::Value {
    serde_json::json!({
        "submission": {
            "agent_id": agent_id,
            "runs": [{"returns": (0..40).map(|i| 0.001 * (f64::from(i) + 1.0).sin()).collect::<Vec<_>>()}],
            "in_sample_trials": 0,
            "candidates": [],
        },
        "artifact_digest": artifact,
        "salt": salt,
        "fault_plan_sha256": plan,
    })
}

fn window_file(fx: &Fixture) -> PathBuf {
    Path::new(&fx.0)
        .join("arena")
        .join("windows")
        .join("w1")
        .join("window.json")
}

#[test]
fn arena_commitment_without_a_plan_is_sharpebench_commit() {
    let fx = Fixture::new();
    let artifact = content_digest(b"artifact");
    let plain = fx.expect(0, &["commit", "alpha", "w1", &artifact, "salt-a"], None);
    let arena = fx.expect(
        0,
        &["arena", "commitment", "alpha", "w1", &artifact, "salt-a"],
        None,
    );
    assert_eq!(arena.stdout, plain.stdout);

    std::fs::write(fx.path("plan.json"), PLAN).unwrap();
    let digest = FaultPlan::from_json(PLAN.as_bytes()).unwrap().digest();
    let faulted = fx.expect(
        0,
        &[
            "arena",
            "commitment",
            "alpha",
            "w1",
            &artifact,
            "salt-a",
            "--fault-plan",
            "plan.json",
        ],
        None,
    );
    let printed: sharpebench_attest::Commitment = serde_json::from_slice(&faulted.stdout).unwrap();
    assert_eq!(
        printed,
        make_commitment_under_fault_plan("alpha", "w1", &artifact, "salt-a", Some(&digest))
    );

    // A plan `run --fault-plan` would refuse is refused; missing operands are usage.
    std::fs::write(fx.path("bad.json"), "{not json").unwrap();
    fx.expect(
        1,
        &[
            "arena",
            "commitment",
            "alpha",
            "w1",
            &artifact,
            "salt-a",
            "--fault-plan",
            "bad.json",
        ],
        None,
    );
    fx.expect(2, &["arena", "commitment", "alpha", "w1", &artifact], None);
}

#[test]
fn a_faulted_window_binds_its_plan_from_commitment_to_verify() {
    let fx = Fixture::new();
    std::fs::write(fx.path("plan.json"), PLAN).unwrap();
    let digest = FaultPlan::from_json(PLAN.as_bytes()).unwrap().digest();
    let artifact = content_digest(b"artifact");
    let scorer = content_digest(b"scorer");

    fx.expect(0, &["arena", "init", "arena"], None);
    fx.expect(
        0,
        &[
            "arena",
            "open",
            "arena",
            "w1",
            "10",
            "20",
            "--scorer-artifact-sha256",
            &scorer,
            "--fault-plan",
            "plan.json",
        ],
        None,
    );
    // alpha commits under the window's plan; beta with the plan-less `commit`.
    fx.expect(
        0,
        &[
            "arena",
            "commitment",
            "alpha",
            "w1",
            &artifact,
            "salt-a",
            "--fault-plan",
            "plan.json",
        ],
        Some("alpha.json"),
    );
    fx.expect(
        0,
        &["commit", "beta", "w1", &artifact, "salt-b"],
        Some("beta.json"),
    );
    fx.expect(0, &["arena", "commit", "arena", "w1", "alpha.json"], None);
    fx.expect(0, &["arena", "commit", "arena", "w1", "beta.json"], None);
    fx.expect(0, &["arena", "advance", "arena", "20"], None);

    std::fs::write(fx.path("data.csv"), "sym,close\nA,1.0\nA,1.01\n").unwrap();
    let entries = serde_json::json!([
        entry("alpha", &artifact, "salt-a", &digest),
        entry("beta", &artifact, "salt-b", &digest),
    ]);
    std::fs::write(fx.path("entries.json"), entries.to_string()).unwrap();
    let scored = fx.expect(
        0,
        &[
            "--json",
            "arena",
            "score",
            "arena",
            "w1",
            "data.csv",
            "entries.json",
        ],
        None,
    );
    let scored: serde_json::Value = serde_json::from_slice(&scored.stdout).unwrap();
    assert_eq!(scored["scored"], 1, "{scored}");
    assert_eq!(scored["refused"][0]["agent_id"], "beta", "{scored}");
    assert_eq!(
        scored["refused"][0]["reason"], "reveal does not match commitment",
        "{scored}"
    );

    fx.expect(0, &["arena", "publish", "arena", "w1", "host-secret"], None);
    fx.expect(0, &["arena", "verify", "arena"], None);

    // Drop the plan from the unsigned window file: the signed header still
    // names it, so verify fails, reporting the typed mismatch.
    let path = window_file(&fx);
    let mut window: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    window.as_object_mut().unwrap().remove("fault_plan_sha256");
    std::fs::write(&path, serde_json::to_string_pretty(&window).unwrap()).unwrap();

    let text = fx.expect(1, &["arena", "verify", "arena"], None);
    let stderr = String::from_utf8_lossy(&text.stderr);
    assert!(
        stderr.contains(&format!(
            "fault_plan_sha256 is {digest} in the header but absent in window.json"
        )),
        "{stderr}"
    );
    let json = fx.expect(1, &["--json", "arena", "verify", "arena"], None);
    let report: serde_json::Value = serde_json::from_slice(&json.stdout).unwrap();
    assert_eq!(report["ok"], false);
    assert_eq!(
        report["windows"][0]["identity_mismatches"],
        serde_json::json!([{"field": "fault_plan_sha256", "header": digest, "window": null}])
    );
}
