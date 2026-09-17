//! The installed CLI end to end on bound forward intake: a trajectory written by
//! `sharpebench capture --data` over the revealed dataset, committed under the
//! digest `arena reference-artifact` prints, is ranked by `arena score` as
//! `re-executed` on a certifying board, and as `replayed` on a noncertifying
//! board under `--replay-only`. So the arena's execution matrix is the one the
//! capture command runs. `sharpebench audit` carries the forward
//! hindsight-oracle case as its tenth attack.

use std::path::PathBuf;
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};

use sharpebench_attest::content_digest;
use sharpebench_core::ScoreConfig;
use sharpebench_sim::Dataset;

struct Fixture(PathBuf);

impl Fixture {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        loop {
            let path = std::env::temp_dir().join(format!(
                "sharpebench-forward-intake-{}-{}",
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

    /// Run `args` in the fixture, require exit `code`, return stdout.
    fn expect(&self, code: i32, args: &[&str]) -> Vec<u8> {
        let output: Output = Command::new(env!("CARGO_BIN_EXE_sharpebench"))
            .current_dir(&self.0)
            .args(args)
            .output()
            .unwrap();
        assert_eq!(
            output.status.code(),
            Some(code),
            "{args:?}\nstdout: {}\nstderr: {}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        output.stdout
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.0).expect("remove exclusively owned fixture");
    }
}

#[test]
fn a_cli_capture_is_ranked_replayed_and_reexecuted_by_arena_score() {
    let fx = Fixture::new();
    let data = Dataset::synthetic(4, 60, 5);
    let mut csv = String::from("date,symbol,close\n");
    for (symbol, closes) in &data.closes {
        for (date, close) in data.dates.iter().zip(closes) {
            csv.push_str(&format!("{date},{symbol},{close}\n"));
        }
    }
    std::fs::write(fx.path("data.csv"), csv).unwrap();
    fx.expect(
        0,
        &["capture", "momentum", "capture.json", "--data", "data.csv"],
    );

    // The capture runs 8 execution seeds, so the window freezes 8, and its
    // runner is this binary, so the window's scorer artifact is this binary.
    let config = ScoreConfig {
        execution_seeds_per_window: 8,
        ..ScoreConfig::default()
    };
    std::fs::write(
        fx.path("config.json"),
        serde_json::to_string(&config).unwrap(),
    )
    .unwrap();
    let scorer = content_digest(&std::fs::read(env!("CARGO_BIN_EXE_sharpebench")).unwrap());
    // The commitment names the reference agent as well as the runner.
    let artifact =
        String::from_utf8(fx.expect(0, &["arena", "reference-artifact", "momentum", &scorer]))
            .unwrap()
            .trim()
            .to_string();
    assert_eq!(artifact.len(), 64, "{artifact}");
    assert_ne!(artifact, scorer);
    let capture: serde_json::Value =
        serde_json::from_slice(&std::fs::read(fx.path("capture.json")).unwrap()).unwrap();
    let entries = serde_json::json!([{
        "agent_id": "alpha",
        "capture": capture,
        "artifact_digest": artifact,
        "salt": "salt-a",
    }]);
    std::fs::write(fx.path("entries.json"), entries.to_string()).unwrap();

    fx.expect(0, &["arena", "init", "arena"]);
    for window in ["replay", "reexecute"] {
        fx.expect(
            0,
            &[
                "arena",
                "open",
                "arena",
                window,
                "10",
                "20",
                "--scorer-artifact-sha256",
                &scorer,
                "--config",
                "config.json",
            ],
        );
        let commitment = fx.expect(0, &["commit", "alpha", window, &artifact, "salt-a"]);
        let name = format!("{window}-commitment.json");
        std::fs::write(fx.path(&name), commitment).unwrap();
        fx.expect(0, &["arena", "commit", "arena", window, &name]);
    }
    fx.expect(0, &["arena", "advance", "arena", "20"]);

    for (window, extra, expected, certifying) in [
        ("replay", Some("--replay-only"), "replayed", false),
        ("reexecute", None, "re-executed", true),
    ] {
        let mut args = vec![
            "--json",
            "arena",
            "score",
            "arena",
            window,
            "data.csv",
            "entries.json",
        ];
        args.extend(extra);
        let scored: serde_json::Value = serde_json::from_slice(&fx.expect(0, &args)).unwrap();
        assert_eq!(scored["scored"], 1, "{scored}");
        assert_eq!(scored["refused"], serde_json::json!([]), "{scored}");
        assert_eq!(scored["returns_provenance"]["alpha"], expected, "{scored}");
        assert_eq!(scored["certifying"], certifying, "{scored}");
        assert!(
            scored.get("supplied_returns_accepted").is_none(),
            "{scored}"
        );
    }
}

#[test]
fn audit_runs_the_forward_hindsight_oracle_as_its_tenth_attack() {
    let fx = Fixture::new();
    let report: serde_json::Value =
        serde_json::from_slice(&fx.expect(0, &["--json", "audit"])).unwrap();
    let cases = report["cases"].as_array().unwrap();
    assert_eq!(cases.len(), 10, "{report}");
    assert_eq!(cases[9]["name"], "forward-hindsight-oracle");
    assert_eq!(cases[9]["defended"], true, "{report}");
    assert_eq!(report["all_defended"], true);
    assert_eq!(report["known_gaps"], 0);
}
