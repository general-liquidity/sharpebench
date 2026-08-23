//! Compile + drive the CLI's standalone `arena_cmd` module against a real temp
//! arena. `main.rs` is not wired to it yet (dispatch lands at integration), so
//! this path-include is what keeps the module compiling, clippy-clean, and
//! behaviorally tested in the meantime.

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
    assert_eq!(
        arena_cmd::run(&argv(&["open", dir, "w1", "10", "20"]), true),
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
