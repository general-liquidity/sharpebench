//! The opt-in `ScoreConfig::psr_standard_error` methodology field: absent at
//! its default, so the default board and every digest over a default
//! configuration are byte-identical; when set to `benchmark`, every PSR and
//! deflated Sharpe the board reads uses the null standard error, and each row
//! says so.

use sharpebench_core::composite::pooled_returns;
use sharpebench_core::significance::bootstrap_dsr_ci_against_null_at;
use sharpebench_core::{
    per_run_passes, rank, score_agent, AgentSubmission, PsrStandardError, Run, ScoreConfig,
};
use sharpebench_stats::null_standard_error::{
    checked_probabilistic_sharpe_ratio_at, deflated_sharpe_ratio_against_null_at,
};
use sharpebench_stats::StandardErrorAt;

fn sub(id: &str, runs: Vec<Vec<f64>>) -> AgentSubmission {
    AgentSubmission {
        agent_id: id.into(),
        runs: runs
            .into_iter()
            .map(|returns| Run {
                returns,
                ..Run::default()
            })
            .collect(),
        in_sample_trials: 0,
        candidates: vec![],
    }
}

fn wave(drift: f64, phase: f64) -> Vec<f64> {
    (0..240)
        .map(|t| {
            let t = t as f64;
            drift + 0.01 * (1.3 * t + phase).sin() + 0.004 * (0.21 * t).cos()
        })
        .collect()
}

fn field() -> Vec<AgentSubmission> {
    vec![
        sub("a", vec![wave(0.002, 0.0), wave(0.003, 1.0)]),
        sub("b", vec![wave(0.001, 2.0), wave(0.0005, 3.0)]),
        sub("c", vec![wave(-0.001, 4.0), wave(0.004, 5.0)]),
    ]
}

fn benchmark_config() -> ScoreConfig {
    ScoreConfig {
        psr_standard_error: PsrStandardError::Benchmark,
        ..ScoreConfig::default()
    }
}

#[test]
fn the_default_is_absent_from_a_serialized_config_and_benchmark_round_trips() {
    let default = serde_json::to_string(&ScoreConfig::default()).unwrap();
    assert!(!default.contains("psr_standard_error"), "{default}");

    let opted = serde_json::to_string(&benchmark_config()).unwrap();
    assert!(
        opted.contains(r#""psr_standard_error":"benchmark""#),
        "{opted}"
    );
    let back: ScoreConfig = serde_json::from_str(&opted).unwrap();
    assert_eq!(back.psr_standard_error, PsrStandardError::Benchmark);
    assert_eq!(serde_json::to_string(&back).unwrap(), opted);

    // A configuration written before the field existed reads as the default,
    // and an explicit default writes back without the field.
    let legacy: ScoreConfig = serde_json::from_str(&default).unwrap();
    assert_eq!(legacy.psr_standard_error, PsrStandardError::Observed);
    let explicit = default.replacen('{', r#"{"psr_standard_error":"observed","#, 1);
    let explicit: ScoreConfig = serde_json::from_str(&explicit).unwrap();
    assert_eq!(serde_json::to_string(&explicit).unwrap(), default);

    let unknown = default.replacen('{', r#"{"psr_standard_error":"null","#, 1);
    assert!(serde_json::from_str::<ScoreConfig>(&unknown).is_err());
}

#[test]
fn the_command_line_identifiers_parse_and_anything_else_is_refused() {
    assert_eq!(
        PsrStandardError::parse("observed"),
        Ok(PsrStandardError::Observed)
    );
    assert_eq!(
        PsrStandardError::parse("benchmark"),
        Ok(PsrStandardError::Benchmark)
    );
    for bad in ["", "Benchmark", "null", "plug-in"] {
        assert!(PsrStandardError::parse(bad).is_err(), "{bad}");
    }
    assert_eq!(PsrStandardError::Observed.at(), StandardErrorAt::Observed);
    assert_eq!(PsrStandardError::Benchmark.at(), StandardErrorAt::Benchmark);
    assert!(PsrStandardError::default().is_default());
    assert!(!PsrStandardError::Benchmark.is_default());
}

#[test]
fn a_default_board_does_not_carry_the_field_and_an_opted_board_records_it_on_every_row() {
    let subs = field();
    let default = serde_json::to_string(&rank(&subs, &ScoreConfig::default())).unwrap();
    assert!(!default.contains("psr_standard_error"));
    let explicit = ScoreConfig {
        psr_standard_error: PsrStandardError::Observed,
        ..ScoreConfig::default()
    };
    assert_eq!(
        serde_json::to_string(&rank(&subs, &explicit)).unwrap(),
        default
    );

    let opted = rank(&subs, &benchmark_config());
    assert!(opted
        .iter()
        .all(|row| row.psr_standard_error == PsrStandardError::Benchmark));
    let json = serde_json::to_string(&opted).unwrap();
    assert_eq!(
        json.matches(r#""psr_standard_error":"benchmark""#).count(),
        subs.len()
    );
}

/// Every PSR and deflated Sharpe on an opted row is the null-SE statistic on
/// the row's own pooled track and bar, bit for bit, and the interval brackets
/// the same statistic.
#[test]
fn an_opted_row_reads_the_null_standard_error_everywhere_it_reads_a_psr() {
    let subs = field();
    let cfg = benchmark_config();
    let board = rank(&subs, &cfg);
    let default_board = rank(&subs, &ScoreConfig::default());
    let at = StandardErrorAt::Benchmark;
    let mut moved = false;
    for row in &board {
        let pooled = pooled_returns(
            subs.iter().find(|s| s.agent_id == row.agent_id).unwrap(),
            cfg.execution_seeds_per_window,
        );
        assert_eq!(
            row.psr.to_bits(),
            checked_probabilistic_sharpe_ratio_at(&pooled, 0.0, at)
                .unwrap()
                .to_bits(),
            "{}",
            row.agent_id
        );
        let dsr = deflated_sharpe_ratio_against_null_at(
            &pooled,
            row.effective_n_trials,
            row.deflation_null_mean_per_period,
            row.trials_sr_std,
            at,
        )
        .unwrap();
        assert_eq!(
            row.deflated_sharpe.to_bits(),
            dsr.to_bits(),
            "{}",
            row.agent_id
        );
        let ci = bootstrap_dsr_ci_against_null_at(
            &pooled,
            row.effective_n_trials,
            row.deflation_null_mean_per_period,
            row.trials_sr_std,
            cfg.bootstrap_seed,
            cfg.n_boot,
            cfg.block_prob,
            cfg.dsr_ci_level,
            at,
        )
        .unwrap();
        assert_eq!(
            (row.dsr_ci_low, row.dsr_ci_high, row.dsr_se),
            (Some(ci.lower), Some(ci.upper), Some(ci.se))
        );
        let plug_in = default_board
            .iter()
            .find(|r| r.agent_id == row.agent_id)
            .unwrap();
        moved |= plug_in.psr.to_bits() != row.psr.to_bits()
            || plug_in.deflated_sharpe.to_bits() != row.deflated_sharpe.to_bits();
    }
    assert!(moved, "the field never separated the two evaluations");
}

/// A run with one deep loss among steady gains: the plug-in bracket charges
/// its negative skew and fails the per-run bar, the null bracket at a zero
/// benchmark is one and passes it. pass^k reads whichever the config names.
#[test]
fn pass_k_reads_the_configured_standard_error() {
    let mut run = vec![0.01; 19];
    run.push(-0.05);
    let cfg_default = ScoreConfig::default();
    let cfg_null = benchmark_config();
    let bar = cfg_default.per_run_psr_bar;
    let observed = checked_probabilistic_sharpe_ratio_at(&run, 0.0, StandardErrorAt::Observed);
    let null = checked_probabilistic_sharpe_ratio_at(&run, 0.0, StandardErrorAt::Benchmark);
    assert!(observed.unwrap() < bar && null.unwrap() >= bar);

    let agent = sub("tail-seller", vec![run.clone(), run]);
    assert_eq!(
        per_run_passes(&agent, None, &cfg_default),
        vec![false, false]
    );
    assert_eq!(per_run_passes(&agent, None, &cfg_null), vec![true, true]);
    assert!(!score_agent(&agent, &cfg_default).passed_k);
    assert!(score_agent(&agent, &cfg_null).passed_k);
    assert_eq!(
        score_agent(&agent, &cfg_null).psr_standard_error,
        PsrStandardError::Benchmark
    );
}
