//! Compile + drive the CLI's `arena_cmd` module against a real temporary arena.
//! The production CLI dispatches to this module; the path include lets the arena
//! crate exercise the same implementation through a full lifecycle test.

#[path = "../../sharpebench-cli/src/arena_cmd.rs"]
mod arena_cmd;

use std::path::PathBuf;

use sharpebench_arena::{reference_entrant_digest, ReplayWindow, ReturnsProvenance, RevealedEntry};
use sharpebench_attest::{content_digest, make_commitment};
use sharpebench_core::{AgentSubmission, Run, ScoreConfig};
use sharpebench_sim::{Dataset, Momentum};

fn temp_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "sharpebench-arena-cli-{tag}-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// A 60-bar synthetic market as the `date,symbol,close` CSV `--data` reads.
fn write_dataset(path: &std::path::Path) {
    let data = Dataset::synthetic(4, 60, 11);
    let mut out = String::from("date,symbol,close\n");
    for (symbol, closes) in &data.closes {
        for (date, close) in data.dates.iter().zip(closes) {
            out.push_str(&format!("{date},{symbol},{close}\n"));
        }
    }
    std::fs::write(path, out).unwrap();
}

/// Momentum over the window's execution matrix, captured by the window's
/// frozen scorer and naming `entrant` (the reference name `momentum`, or a
/// `sandbox:` image), revealed with the committed `digest`.
fn capture_entry(
    dataset: &std::path::Path,
    entrant: &str,
    digest: &str,
    scorer: &str,
) -> RevealedEntry {
    let bytes = std::fs::read(dataset).unwrap();
    let replay = ReplayWindow::parse(&bytes, &ScoreConfig::default()).unwrap();
    let (_, mut capture) = sharpebench_harness::run_agent_capture(
        entrant,
        &replay.data,
        &replay.windows,
        &replay.seeds,
        replay.costs,
        || Box::new(Momentum::default()),
    );
    capture.contract.as_mut().unwrap().runner_artifact_sha256 = Some(scorer.to_string());
    RevealedEntry {
        agent_id: Some("alpha".to_string()),
        submission: None,
        capture: Some(capture),
        artifact_digest: digest.to_string(),
        salt: "salt-a".to_string(),
        fault_plan_sha256: None,
    }
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
    for window in ["w1", "w2"] {
        assert_eq!(
            arena_cmd::run(
                &argv(&[
                    "open",
                    dir,
                    window,
                    "10",
                    "20",
                    "--scorer-artifact-sha256",
                    &scorer,
                ]),
                true,
            ),
            0
        );
    }

    // w1: alpha commits to the momentum reference agent compiled into the
    // scorer; w2: alpha commits to an image.
    let digest = reference_entrant_digest("momentum", &scorer);
    let image_digest = content_digest(b"cli-artifact");
    for (window, artifact) in [("w1", &digest), ("w2", &image_digest)] {
        // Commitment file, as `sharpebench commit` would emit it.
        let commitment = make_commitment("alpha", window, artifact, "salt-a");
        let commit_path = root.join(format!("{window}-commitment.json"));
        std::fs::write(&commit_path, serde_json::to_string(&commitment).unwrap()).unwrap();
        assert_eq!(
            arena_cmd::run(
                &argv(&["commit", dir, window, commit_path.to_str().unwrap()]),
                true
            ),
            0
        );
    }

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
    write_dataset(&dataset_path);
    let entries = vec![capture_entry(&dataset_path, "momentum", &digest, &scorer)];
    let entries_path = root.join("entries.json");
    std::fs::write(&entries_path, serde_json::to_string(&entries).unwrap()).unwrap();
    // The default intake re-executes: the reference agent runs in process.
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
    let window = window_json(dir, "w1");
    assert_eq!(window["scores"].as_array().unwrap().len(), 1, "{window}");
    assert_eq!(
        window["returns_provenance"]["alpha"], "re-executed",
        "{window}"
    );
    assert_eq!(window["certifying"], true, "{window}");
    assert!(
        window.get("supplied_returns_accepted").is_none(),
        "{window}"
    );

    // --replay-only ranks the image capture without re-executing it, on a
    // board that does not certify it.
    let image_entries = vec![capture_entry(
        &dataset_path,
        &format!("sandbox:cli/alpha@sha256:{image_digest}"),
        &image_digest,
        &scorer,
    )];
    let image_entries_path = root.join("image-entries.json");
    std::fs::write(
        &image_entries_path,
        serde_json::to_string(&image_entries).unwrap(),
    )
    .unwrap();
    assert_eq!(
        arena_cmd::run(
            &argv(&[
                "score",
                dir,
                "w2",
                dataset_path.to_str().unwrap(),
                image_entries_path.to_str().unwrap(),
                "--replay-only",
            ]),
            true
        ),
        0
    );
    let window = window_json(dir, "w2");
    assert_eq!(
        window["returns_provenance"]["alpha"], "replayed",
        "{window}"
    );
    assert_eq!(window["certifying"], false, "{window}");

    // Key via the file: convention.
    let key_path = root.join("host.key");
    std::fs::write(&key_path, "cli-test-signing-secret\n").unwrap();
    let key_spec = format!("file:{}", key_path.display());
    for window in ["w1", "w2"] {
        assert_eq!(
            arena_cmd::run(&argv(&["publish", dir, window, &key_spec]), true),
            0
        );
    }
    let md = |window: &str| {
        std::fs::read_to_string(arena_dir.join("windows").join(window).join("board.md")).unwrap()
    };
    assert!(!md("w1").contains("Noncertifying"), "{}", md("w1"));
    assert!(
        md("w2").contains("**Noncertifying board.**"),
        "{}",
        md("w2")
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

/// `arena reference-artifact` prints the name-bound digest a reference
/// entrant commits to, and refuses a name that is not a reference agent.
#[test]
fn reference_artifact_prints_the_name_bound_digest() {
    let scorer = content_digest(b"cli-scorer-artifact");
    assert_eq!(
        arena_cmd::run(&argv(&["reference-artifact", "momentum", &scorer]), true),
        0
    );
    assert_eq!(
        arena_cmd::run(&argv(&["reference-artifact", "oracle", &scorer]), true),
        1
    );
    assert_eq!(
        arena_cmd::run(&argv(&["reference-artifact", "momentum"]), true),
        2
    );
    assert_ne!(
        reference_entrant_digest("momentum", &scorer),
        reference_entrant_digest("buy-and-hold", &scorer)
    );
}

#[test]
fn usage_errors_exit_2() {
    assert_eq!(arena_cmd::run(&argv(&[]), false), 2);
    assert_eq!(arena_cmd::run(&argv(&["nonsense"]), false), 2);
    assert_eq!(arena_cmd::run(&argv(&["open", "somewhere"]), false), 2);
    // An intake flag in an operand position is a usage error, never a path.
    for flag in ["--replay-only", "--allow-supplied-returns"] {
        assert_eq!(
            arena_cmd::run(&argv(&["score", "dir", "w1", "data.csv", flag]), false),
            2,
            "{flag}"
        );
    }
}

/// `arena score` ranks supplied returns only under `--allow-supplied-returns`,
/// which records the row as `supplied` and the window as noncertifying, through
/// to the signed header and `board.md`.
#[test]
fn supplied_returns_are_ranked_only_under_the_explicit_flag() {
    let root = temp_dir("supplied");
    let arena_dir = root.join("arena");
    let dir = arena_dir.to_str().unwrap();
    assert_eq!(arena_cmd::run(&argv(&["init", dir]), true), 0);
    let scorer = content_digest(b"cli-scorer-artifact");
    let digest = content_digest(b"cli-artifact");
    for window in ["plain", "flagged"] {
        let open = [
            "open",
            dir,
            window,
            "10",
            "20",
            "--scorer-artifact-sha256",
            scorer.as_str(),
        ];
        assert_eq!(arena_cmd::run(&argv(&open), true), 0);
        let path = root.join(format!("{window}-commitment.json"));
        let commitment = make_commitment("alpha", window, &digest, "salt-a");
        std::fs::write(&path, serde_json::to_string(&commitment).unwrap()).unwrap();
        let commit = ["commit", dir, window, path.to_str().unwrap()];
        assert_eq!(arena_cmd::run(&argv(&commit), true), 0);
    }
    assert_eq!(arena_cmd::run(&argv(&["advance", dir, "20"]), true), 0);

    let dataset_path = root.join("dataset.csv");
    write_dataset(&dataset_path);
    let entries = vec![RevealedEntry {
        agent_id: None,
        submission: Some(AgentSubmission {
            agent_id: "alpha".to_string(),
            runs: vec![Run {
                returns: (0..40).map(|i| 0.001 * (i as f64).sin()).collect(),
                ..Run::default()
            }],
            in_sample_trials: 0,
            candidates: Vec::new(),
        }),
        capture: None,
        artifact_digest: digest.clone(),
        salt: "salt-a".to_string(),
        fault_plan_sha256: None,
    }];
    let entries_path = root.join("entries.json");
    std::fs::write(&entries_path, serde_json::to_string(&entries).unwrap()).unwrap();
    let score = |window: &str, extra: &[&str]| {
        let mut parts = vec![
            "score",
            dir,
            window,
            dataset_path.to_str().unwrap(),
            entries_path.to_str().unwrap(),
        ];
        parts.extend_from_slice(extra);
        arena_cmd::run(&argv(&parts), true)
    };

    assert_eq!(score("plain", &[]), 0);
    let plain = window_json(dir, "plain");
    assert!(plain["scores"].as_array().unwrap().is_empty(), "{plain}");
    assert_eq!(
        plain["refusals"][0]["reason"],
        sharpebench_arena::SUPPLIED_RETURNS_REFUSAL
    );
    assert!(plain.get("supplied_returns_accepted").is_none(), "{plain}");

    assert_eq!(score("flagged", &["--allow-supplied-returns"]), 0);
    let flagged = window_json(dir, "flagged");
    assert_eq!(flagged["scores"].as_array().unwrap().len(), 1, "{flagged}");
    assert_eq!(flagged["supplied_returns_accepted"], true, "{flagged}");
    assert_eq!(
        serde_json::from_value::<ReturnsProvenance>(flagged["returns_provenance"]["alpha"].clone())
            .unwrap(),
        ReturnsProvenance::Supplied
    );

    let key_path = root.join("host.key");
    std::fs::write(&key_path, "cli-test-signing-secret\n").unwrap();
    let key_spec = format!("file:{}", key_path.display());
    for window in ["plain", "flagged"] {
        assert_eq!(
            arena_cmd::run(&argv(&["publish", dir, window, &key_spec]), true),
            0
        );
    }
    let board: sharpebench_attest::PublicChain = serde_json::from_slice(
        &std::fs::read(arena_dir.join("windows").join("flagged").join("board.json")).unwrap(),
    )
    .unwrap();
    let header: serde_json::Value = serde_json::from_str(&board.chain[0].payload).unwrap();
    assert_eq!(header["supplied_returns_accepted"], true);
    let row: serde_json::Value = serde_json::from_str(&board.chain[1].payload).unwrap();
    assert_eq!(row["returns_provenance"], "supplied");
    let md = std::fs::read_to_string(arena_dir.join("windows").join("flagged").join("board.md"))
        .unwrap();
    assert!(md.contains("**Noncertifying board.**"), "{md}");
    // `plain` ranked nothing, so its board certifies vacuously.
    let plain_md =
        std::fs::read_to_string(arena_dir.join("windows").join("plain").join("board.md")).unwrap();
    assert!(!plain_md.contains("Noncertifying"), "{plain_md}");
    assert_eq!(window_json(dir, "plain")["certifying"], true);
    assert_eq!(flagged["certifying"], false, "{flagged}");
    assert_eq!(arena_cmd::run(&argv(&["verify", dir]), true), 0);
    let _ = std::fs::remove_dir_all(&root);
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
