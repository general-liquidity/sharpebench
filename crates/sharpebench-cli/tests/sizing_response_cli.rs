//! `sharpebench verify-trajectory --diagnostics sizing-response`: how an
//! agent's gross exposure moved with the trailing volatility it faced.
//!
//! Absent, the output is the verification and nothing else. Present, the
//! verification is unchanged and the diagnostic sits beside it: under `--json`
//! as a `sizing_response` member next to a `verification` member equal to the
//! plain output, in the human form as a block after the unchanged text. A bad
//! request is refused with exit code 2 before anything is printed.

use std::path::PathBuf;
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};

struct Fixture(PathBuf);

impl Fixture {
    /// A one-symbol dataset whose volatility alternates between a calm and a
    /// turbulent regime every 40 bars, plus two captured trajectories.
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let dir = loop {
            let path = std::env::temp_dir().join(format!(
                "sharpebench-sizing-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            match std::fs::create_dir(&path) {
                Ok(()) => break path,
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(e) => panic!("create fixture: {e}"),
            }
        };
        let mut csv = String::from("date,symbol,close\n");
        let mut state: u64 = 0x5EED_0916;
        let mut price = 100.0_f64;
        for row in 0..200 {
            state = state
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            let unit = (state >> 11) as f64 / (1u64 << 53) as f64;
            let sigma = if (row / 40) % 2 == 0 { 0.004 } else { 0.03 };
            price *= 1.0 + sigma * (unit - 0.5) * 2.0 * 3f64.sqrt();
            csv.push_str(&format!("2020-{row:03},AAA,{price}\n"));
        }
        std::fs::write(dir.join("data.csv"), csv).unwrap();
        let fixture = Self(dir);
        for agent in ["buy-and-hold", "momentum"] {
            let out = fixture.cli(&[
                "capture",
                agent,
                &format!("{agent}.json"),
                "--data",
                "data.csv",
            ]);
            assert!(out.status.success(), "{}", stderr(&out));
        }
        fixture
    }

    fn cli(&self, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_sharpebench"))
            .current_dir(&self.0)
            .args(args)
            .output()
            .unwrap()
    }

    fn verify(&self, agent: &str, extra: &[&str]) -> Output {
        let traj = format!("{agent}.json");
        let mut args = vec!["verify-trajectory", traj.as_str(), "--data", "data.csv"];
        args.extend_from_slice(extra);
        self.cli(&args)
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.0).expect("remove exclusively owned fixture");
    }
}

fn stderr(out: &Output) -> String {
    String::from_utf8_lossy(&out.stderr).into_owned()
}

fn json(out: &Output) -> serde_json::Value {
    assert_eq!(out.status.code(), Some(0), "{}", stderr(out));
    serde_json::from_slice(&out.stdout).unwrap()
}

/// The data has 200 bars, so the CLI's two windows are 20..110 and 110..200,
/// each captured under 8 execution seeds.
const RUNS: u64 = 16;
const BARS: u64 = 16 * 90;

#[test]
fn absent_flag_prints_the_verification_only() {
    let fx = Fixture::new();
    for extra in [&["--json"][..], &[][..]] {
        let out = fx.verify("momentum", extra);
        assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
        let text = String::from_utf8(out.stdout).unwrap();
        for word in ["sizing", "Spearman", "quintile", "volatility"] {
            assert!(!text.contains(word), "{word} in default output");
        }
    }
    let doc = json(&fx.verify("momentum", &["--json"]));
    assert!(doc.get("verification").is_none());
    assert!(doc.get("agent_id").is_some());
}

#[test]
fn json_report_sits_beside_a_byte_identical_verification() {
    let fx = Fixture::new();
    for agent in ["buy-and-hold", "momentum"] {
        let plain = json(&fx.verify(agent, &["--json"]));
        let with = json(&fx.verify(agent, &["--diagnostics", "sizing-response", "--json"]));
        let mut keys: Vec<&str> = with
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect();
        keys.sort_unstable();
        assert_eq!(keys, ["sizing_response", "verification"]);
        assert_eq!(with["verification"], plain);

        let report = &with["sizing_response"];
        assert_eq!(report["agent_id"], agent);
        assert_eq!(report["used_by_gate"], false);
        assert_eq!(report["runs"], RUNS);
        assert_eq!(report["config"]["vol_lookback"], 20);
        assert_eq!(report["config"]["exposure_resolution"], 0.01);
        assert_eq!(report["census"]["bars_replayed"], BARS);
        assert_eq!(report["census"]["pairs"], BARS);
        assert_eq!(report["rank_correlation"]["pairs"], BARS);
    }
}

#[test]
fn buy_and_hold_is_typed_unavailable_and_momentum_is_measured() {
    let fx = Fixture::new();
    let held = json(&fx.verify(
        "buy-and-hold",
        &["--diagnostics", "sizing-response", "--json"],
    ));
    let rank = &held["sizing_response"]["rank_correlation"];
    assert!(rank.get("spearman_rho").is_none(), "{rank}");
    assert_eq!(rank["unavailable"]["reason"], "constant_exposure");
    let min = rank["unavailable"]["min_gross_exposure"].as_f64().unwrap();
    let max = rank["unavailable"]["max_gross_exposure"].as_f64().unwrap();
    assert!((min - 1.0).abs() < 1e-9 && (max - 1.0).abs() < 1e-9);

    let traded = json(&fx.verify("momentum", &["--diagnostics", "sizing-response", "--json"]));
    let report = &traded["sizing_response"];
    let rho = report["rank_correlation"]["spearman_rho"].as_f64().unwrap();
    assert!((-1.0..=1.0).contains(&rho));
    assert!(report["rank_correlation"].get("unavailable").is_none());
    let quintiles = report["by_volatility"]["quintiles"].as_array().unwrap();
    assert_eq!(quintiles.len(), 5);
    let counted: u64 = quintiles.iter().map(|q| q["pairs"].as_u64().unwrap()).sum();
    assert_eq!(counted, BARS);
}

#[test]
fn text_block_follows_the_unchanged_verification() {
    let fx = Fixture::new();
    let plain = fx.verify("buy-and-hold", &[]);
    let with = fx.verify("buy-and-hold", &["--diagnostics", "sizing-response"]);
    assert_eq!(with.status.code(), Some(0), "{}", stderr(&with));
    assert!(with.stdout.starts_with(&plain.stdout));
    let tail = String::from_utf8(with.stdout[plain.stdout.len()..].to_vec()).unwrap();
    assert!(
        tail.starts_with(
            "\nOpt-in sizing-response diagnostic. Not used by the gate, eligibility or the rank.\n"
        ),
        "{tail}"
    );
    assert!(tail.contains("Spearman rho    : n/a (gross exposure stays between"));
    assert!(tail.contains("5 wildest"), "{tail}");
}

#[test]
fn the_lookback_is_declared_and_applied() {
    let fx = Fixture::new();
    let doc = json(&fx.verify(
        "momentum",
        &[
            "--diagnostics",
            "sizing-response",
            "--vol-lookback",
            "30",
            "--json",
        ],
    ));
    let report = &doc["sizing_response"];
    assert_eq!(report["config"]["vol_lookback"], 30);
    // Only the first window starts before bar 30: ten bars short per seed.
    assert_eq!(report["census"]["bars_without_history"], 8 * 10);
    assert_eq!(report["census"]["pairs"], BARS - 80);
}

#[test]
fn bad_requests_are_refused_before_any_output() {
    let fx = Fixture::new();
    for (extra, message) in [
        (&["--diagnostics"][..], "--diagnostics requires a value"),
        (
            &["--diagnostics", "--json"][..],
            "--diagnostics requires a value",
        ),
        (&["--diagnostics", "mppm"][..], "unknown diagnostic `mppm`"),
        (
            &["--diagnostics", "sizing-response,bogus"][..],
            "unknown diagnostic `bogus`",
        ),
        (
            &["--vol-lookback", "10"][..],
            "--vol-lookback requires --diagnostics sizing-response",
        ),
        (
            &["--diagnostics", "sizing-response", "--vol-lookback"][..],
            "--vol-lookback requires a value",
        ),
        (
            &["--diagnostics", "sizing-response", "--vol-lookback", "1"][..],
            "vol_lookback must be at least 2",
        ),
        (
            &["--diagnostics", "sizing-response", "--vol-lookback", "x"][..],
            "--vol-lookback must be a positive integer",
        ),
        (
            &["--diagnostics", "sizing-response", "--reexecute"][..],
            "without --reexecute",
        ),
    ] {
        let out = fx.verify("momentum", extra);
        assert_eq!(out.status.code(), Some(2), "{extra:?}: {}", stderr(&out));
        assert!(out.stdout.is_empty(), "{extra:?}");
        assert!(
            stderr(&out).contains(message),
            "{extra:?}: {}",
            stderr(&out)
        );
    }
}
