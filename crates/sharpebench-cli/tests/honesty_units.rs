//! `sharpebench check` and `sharpebench score` must deflate one track against
//! the same bar. The LITE honesty verdict once applied the annualized 0.5 prior
//! per period while the scorer converted it, so the two surfaces disagreed by a
//! factor of `sqrt(periods_per_year)` on the same returns.

use std::path::PathBuf;
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};

use sharpebench_core::{per_period_sr_std, score_agent, AgentSubmission, ScoreConfig};
use sharpebench_edge::{is_my_sharpe_real, HonestyConfig};

fn track() -> Vec<f64> {
    (0..1008)
        .map(|i| 0.0005 + 0.006 * (0.7 * i as f64).sin())
        .collect()
}

fn submission(returns: &[f64]) -> AgentSubmission {
    serde_json::from_value(serde_json::json!({
        "agent_id": "track",
        "runs": [{ "returns": returns }],
    }))
    .unwrap()
}

/// Bit for bit, across frequencies, dispersions and search sizes: the edge
/// verdict's bar is the core scorer's `deflation_bar_per_period` for the same
/// annualized prior, `periods_per_year` and trial count, and its deflated
/// Sharpe is the scorer's.
#[test]
fn the_edge_bar_is_the_core_bar_bit_for_bit() {
    let r = track();
    let sub = submission(&r);
    for (trials_sr_std, periods_per_year, n_trials) in [
        (0.5, 252.0, 20),
        (0.5, 52.0, 50),
        (0.35, 8760.0, 200),
        (0.2, 365.0, 7),
        (0.5, 2190.0, 1000),
    ] {
        let core_cfg = ScoreConfig {
            n_trials,
            trials_sr_std,
            ..ScoreConfig::for_periods_per_year(periods_per_year)
        };
        let core = score_agent(&sub, &core_cfg);
        assert!(core.deflation_error.is_none());
        assert_eq!(
            core.trials_sr_std.to_bits(),
            per_period_sr_std(&core_cfg).to_bits()
        );

        let edge = is_my_sharpe_real(
            &r,
            &HonestyConfig {
                n_trials,
                trials_sr_std: Some(trials_sr_std),
                periods_per_year: Some(periods_per_year),
                ..HonestyConfig::default()
            },
        );
        let case = format!("{trials_sr_std} at {periods_per_year}/yr over {n_trials}");
        assert!(edge.statistics_error.is_none(), "{case}");
        assert_eq!(
            edge.expected_max_sharpe.to_bits(),
            core.deflation_bar_per_period.to_bits(),
            "bar: {case}"
        );
        assert_eq!(
            edge.deflated_sharpe.to_bits(),
            core.deflated_sharpe.to_bits(),
            "deflated Sharpe: {case}"
        );
    }

    // The defaults agree as well: the edge's unsupplied prior and frequency are
    // the scorer's default prior and frequency.
    let core_cfg = ScoreConfig {
        n_trials: 20,
        ..ScoreConfig::default()
    };
    let core = score_agent(&sub, &core_cfg);
    let edge = is_my_sharpe_real(
        &r,
        &HonestyConfig {
            n_trials: 20,
            ..HonestyConfig::default()
        },
    );
    assert_eq!(
        edge.expected_max_sharpe.to_bits(),
        core.deflation_bar_per_period.to_bits()
    );
}

struct Fixture(PathBuf);

impl Fixture {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        loop {
            let path = std::env::temp_dir().join(format!(
                "sharpebench-honesty-{}-{}",
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

    fn cli(&self, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_sharpebench"))
            .current_dir(&self.0)
            .args(args)
            .output()
            .unwrap()
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.0).expect("remove exclusively owned fixture");
    }
}

/// `check --periods-per-year` reaches the verdict, the default is named in the
/// explanation, and a frequency that is not one is a usage error.
#[test]
fn check_takes_the_frequency_and_refuses_a_bad_one() {
    let fixture = Fixture::new();
    let csv: String = std::iter::once("return".to_string())
        .chain(track().iter().map(|x| format!("{x:e}")))
        .collect::<Vec<_>>()
        .join("\n");
    std::fs::write(fixture.0.join("r.csv"), csv).unwrap();

    let verdict = |extra: &[&str]| -> serde_json::Value {
        let mut args = vec!["check", "r.csv", "--trials", "20", "--json"];
        args.extend_from_slice(extra);
        let out = fixture.cli(&args);
        serde_json::from_slice(&out.stdout).unwrap_or_else(|e| {
            panic!("{e}: {}", String::from_utf8_lossy(&out.stderr));
        })
    };
    let daily = verdict(&[]);
    assert_eq!(daily["verdict"], "Pass");
    assert!(daily["explanation"]
        .as_str()
        .unwrap()
        .contains("periods_per_year was not supplied"));
    let explicit = verdict(&["--periods-per-year", "252"]);
    assert_eq!(
        explicit["expected_max_sharpe"],
        daily["expected_max_sharpe"]
    );
    assert!(!explicit["explanation"]
        .as_str()
        .unwrap()
        .contains("periods_per_year"));
    let weekly = verdict(&["--periods-per-year", "52"]);
    assert!(
        weekly["expected_max_sharpe"].as_f64().unwrap()
            > daily["expected_max_sharpe"].as_f64().unwrap()
    );
    assert_ne!(weekly["verdict"], "Pass");

    for bad in ["0", "-252", "NaN", "inf", "daily"] {
        let out = fixture.cli(&[
            "check",
            "r.csv",
            "--trials",
            "20",
            "--periods-per-year",
            bad,
        ]);
        assert_eq!(out.status.code(), Some(2), "--periods-per-year {bad}");
        assert!(out.stdout.is_empty());
        assert!(String::from_utf8_lossy(&out.stderr).contains("--periods-per-year"));
    }
}
