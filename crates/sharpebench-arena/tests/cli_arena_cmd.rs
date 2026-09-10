//! Compile + drive the CLI's `arena_cmd` module against a real temporary arena.
//! The production CLI dispatches to this module; the path include lets the arena
//! crate exercise the same implementation through a full lifecycle test.

#[path = "../../sharpebench-cli/src/arena_cmd.rs"]
mod arena_cmd;

use std::path::PathBuf;

use sharpebench_attest::{content_digest, make_commitment};
use sharpebench_core::{AgentSubmission, Run};

fn temp_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "sharpebench-arena-cli-{tag}-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn argv(parts: &[&str]) -> Vec<String> {
    // args[0] is the program name and args[1] is "arena", per the run() contract.
    std::iter::once("sharpebench")
        .chain(std::iter::once("arena"))
        .chain(parts.iter().copied())
        .map(String::from)
        .collect()
}

#[test]
fn cli_drives_the_full_lifecycle_and_verify_walks_the_chain() {
    let root = temp_dir("lifecycle");
    let arena_dir = root.join("arena");
    let dir = arena_dir.to_str().unwrap();

    assert_eq!(arena_cmd::run(&argv(&["init", dir]), true), 0);
    let scorer = content_digest(b"cli-scorer-artifact");
    assert_eq!(
        arena_cmd::run(
            &argv(&[
                "open",
                dir,
                "w1",
                "10",
                "20",
                "--scorer-artifact-sha256",
                &scorer,
            ]),
            true,
        ),
        0
    );

    // Commitment file, as `sharpebench commit` would emit it.
    let digest = content_digest(b"cli-artifact");
    let commitment = make_commitment("alpha", "w1", &digest, "salt-a");
    let commit_path = root.join("commitment.json");
    std::fs::write(&commit_path, serde_json::to_string(&commitment).unwrap()).unwrap();
    assert_eq!(
        arena_cmd::run(
            &argv(&["commit", dir, "w1", commit_path.to_str().unwrap()]),
            true
        ),
        0
    );

    assert_eq!(arena_cmd::run(&argv(&["advance", dir, "20"]), true), 0);

    // A late commitment is refused with exit code 1.
    let late = make_commitment("late", "w1", &digest, "salt-l");
    let late_path = root.join("late.json");
    std::fs::write(&late_path, serde_json::to_string(&late).unwrap()).unwrap();
    assert_eq!(
        arena_cmd::run(
            &argv(&["commit", dir, "w1", late_path.to_str().unwrap()]),
            true
        ),
        1
    );

    let dataset_path = root.join("dataset.csv");
    std::fs::write(&dataset_path, b"sym,close\nA,1.0\nA,1.01\n").unwrap();
    let entries = vec![sharpebench_arena::RevealedEntry {
        submission: AgentSubmission {
            agent_id: "alpha".to_string(),
            runs: vec![Run {
                returns: (0..40).map(|i| 0.001 * (i as f64).sin()).collect(),
                ..Run::default()
            }],
            in_sample_trials: 0,
            candidates: Vec::new(),
        },
        artifact_digest: digest.clone(),
        salt: "salt-a".to_string(),
        fault_plan_sha256: None,
    }];
    let entries_path = root.join("entries.json");
    std::fs::write(&entries_path, serde_json::to_string(&entries).unwrap()).unwrap();
    assert_eq!(
        arena_cmd::run(
            &argv(&[
                "score",
                dir,
                "w1",
                dataset_path.to_str().unwrap(),
                entries_path.to_str().unwrap()
            ]),
            true
        ),
        0
    );

    // Key via the file: convention.
    let key_path = root.join("host.key");
    std::fs::write(&key_path, "cli-test-signing-secret\n").unwrap();
    let key_spec = format!("file:{}", key_path.display());
    assert_eq!(
        arena_cmd::run(&argv(&["publish", dir, "w1", &key_spec]), true),
        0
    );

    // Verify with the embedded key, and pinned to the host's public key.
    assert_eq!(arena_cmd::run(&argv(&["verify", dir]), true), 0);
    let vk = sharpebench_arena::SigningKey::derive(b"cli-test-signing-secret")
        .verifying_key()
        .to_hex();
    assert_eq!(
        arena_cmd::run(&argv(&["verify", dir, "--pubkey", &vk]), true),
        0
    );

    // A wrong pinned key fails with exit code 1.
    let wrong = sharpebench_arena::SigningKey::derive(b"impostor")
        .verifying_key()
        .to_hex();
    assert_eq!(
        arena_cmd::run(&argv(&["verify", dir, "--pubkey", &wrong]), true),
        1
    );

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn usage_errors_exit_2() {
    assert_eq!(arena_cmd::run(&argv(&[]), false), 2);
    assert_eq!(arena_cmd::run(&argv(&["nonsense"]), false), 2);
    assert_eq!(arena_cmd::run(&argv(&["open", "somewhere"]), false), 2);
}

fn fault_plan_json(seed: u64) -> String {
    serde_json::json!({
        "schema_version": "sharpebench.fault-plan.v1",
        "seed": seed,
        "declared_relaxations": ["submission_acceptance"],
        "faults": [
            {"id": "limit", "cohort_ppm": 1_000_000,
             "fault": {"mode": "rate_limit", "max_rejected_presentations": 2}},
        ],
    })
    .to_string()
}

fn window_json(dir: &str, window: &str) -> serde_json::Value {
    let path = std::path::Path::new(dir)
        .join("windows")
        .join(window)
        .join("window.json");
    serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap()
}

#[test]
fn open_records_the_digest_of_a_validated_fault_plan() {
    use sharpebench_harness::fault_plan::FaultPlan;

    let root = temp_dir("fault-plan");
    let arena_dir = root.join("arena");
    let dir = arena_dir.to_str().unwrap();
    assert_eq!(arena_cmd::run(&argv(&["init", dir]), true), 0);
    let scorer = content_digest(b"cli-scorer-artifact");
    let open = |window: &str, extra: &[&str]| {
        let mut parts = vec![
            "open",
            dir,
            window,
            "10",
            "20",
            "--scorer-artifact-sha256",
            &scorer,
        ];
        parts.extend_from_slice(extra);
        arena_cmd::run(&argv(&parts), true)
    };

    let plan = fault_plan_json(7);
    let expected = FaultPlan::from_json(plan.as_bytes()).unwrap().digest();
    let plan_path = root.join("plan.json");
    std::fs::write(&plan_path, &plan).unwrap();
    // The same plan reformatted is the same plan, so it records the same digest.
    let pretty_path = root.join("plan-pretty.json");
    let pretty: serde_json::Value = serde_json::from_str(&plan).unwrap();
    std::fs::write(&pretty_path, serde_json::to_string_pretty(&pretty).unwrap()).unwrap();

    assert_eq!(
        open("faulted", &["--fault-plan", plan_path.to_str().unwrap()]),
        0
    );
    assert_eq!(
        open("pretty", &["--fault-plan", pretty_path.to_str().unwrap()]),
        0
    );
    assert_eq!(open("plain", &[]), 0);
    for window in ["faulted", "pretty"] {
        let w = window_json(dir, window);
        assert_eq!(w["fault_plan_sha256"], expected.as_str(), "{window}");
        assert_eq!(w["schema_version"], 3, "{window}");
    }
    let plain = window_json(dir, "plain");
    assert!(plain.get("fault_plan_sha256").is_none(), "{plain}");
    assert_eq!(plain["schema_version"], 2);

    // A plan `run --fault-plan` would refuse is refused here, before the window
    // exists: no path, a missing file, malformed JSON, an unknown field.
    std::fs::write(root.join("bad.json"), "{not json").unwrap();
    std::fs::write(
        root.join("unknown.json"),
        plan.replacen("\"seed\"", "\"surprise\":1,\"seed\"", 1),
    )
    .unwrap();
    let missing = root.join("missing.json");
    let bad = root.join("bad.json");
    let unknown = root.join("unknown.json");
    for (window, extra) in [
        ("no-path", vec!["--fault-plan"]),
        ("missing", vec!["--fault-plan", missing.to_str().unwrap()]),
        ("bad", vec!["--fault-plan", bad.to_str().unwrap()]),
        ("unknown", vec!["--fault-plan", unknown.to_str().unwrap()]),
    ] {
        assert_eq!(open(window, &extra), 1, "{window}");
        assert!(
            !arena_dir.join("windows").join(window).exists(),
            "{window} was opened"
        );
    }
    let _ = std::fs::remove_dir_all(&root);
}
