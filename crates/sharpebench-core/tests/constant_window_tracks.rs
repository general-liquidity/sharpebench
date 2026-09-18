//! A track whose every window is constant has no Sharpe ratio.
//!
//! The deflation family recognises a constant track by exact value equality, and
//! it used to ask that question of the pooled concatenation alone. Runs that are
//! each constant at their own level concatenate into a series that varies, so the
//! question was answered on the level difference between windows rather than on
//! any bar of the track. Two thirty-bar windows at 0.001 and 0.002 reach a
//! per-period Sharpe of about 2.97, roughly 47 annualized.
//!
//! That bought three things at once: Pareto membership, a deflated Sharpe, and a
//! vote on the field's measured dispersion, which has a floor and no cap. One
//! such entrant in an eight-agent field raised the annualized deflation bar from
//! the floored 1.1382 to 37.55 and drove every other agent's deflated Sharpe to
//! zero.
//!
//! The question is now asked of the window segments the pooled track is
//! concatenated from.

use sharpebench_core::{
    rank, score_agent, AgentSubmission, CompositeScore, Run, ScoreConfig, TrialsSrStdSource,
};

/// The refusal as `deflation_error` renders it. Pinned here as a literal rather
/// than imported, so this file compiles against the tree that had the defect and
/// fails on its assertions instead of on a missing symbol.
const NO_WINDOW_VARIES: &str = concat!(
    "returns must vary inside at least one window: a track whose every window is ",
    "constant has no within-window dispersion, and the dispersion of its pooled ",
    "track is the level difference between windows"
);

/// One submission, one run per window, each window's returns given directly.
fn agent(id: &str, windows: Vec<Vec<f64>>) -> AgentSubmission {
    AgentSubmission {
        agent_id: id.into(),
        runs: windows
            .into_iter()
            .map(|returns| Run {
                returns,
                ..Run::default()
            })
            .collect(),
        in_sample_trials: 0,
        candidates: Vec::new(),
    }
}

fn constant_windows(id: &str, levels: &[f64], bars: usize) -> AgentSubmission {
    agent(id, levels.iter().map(|&level| vec![level; bars]).collect())
}

/// One of seven honest agents whose two windows both vary. Their phases, periods
/// and scales differ, so clone collapse leaves seven separate dispersion votes
/// and the field takes the measured path. A field of near-identical agents would
/// fall back to the configured prior and make the dispersion test below vacuous,
/// which is why that test asserts the source.
fn honest(i: usize) -> AgentSubmission {
    let k = i as f64;
    let window = |offset: f64| -> Vec<f64> {
        (0..30)
            .map(|t| {
                let x = f64::from(t) * (0.7 + 0.31 * k) + 1.3 * k + offset;
                0.0004 * (1.0 + 0.4 * k) + 0.011 * x.sin() + 0.004 * (2.1 * x + k).cos()
            })
            .collect()
    };
    agent(&format!("honest-{i}"), vec![window(0.0), window(3.0 + k)])
}

fn row<'a>(board: &'a [CompositeScore], id: &str) -> &'a CompositeScore {
    board
        .iter()
        .find(|r| r.agent_id == id)
        .unwrap_or_else(|| panic!("{id} is on the board"))
}

#[test]
fn two_constant_windows_have_no_sharpe_ratio() {
    let cfg = ScoreConfig::default();
    let score = score_agent(
        &constant_windows("split constant", &[0.001, 0.002], 30),
        &cfg,
    );
    assert_eq!(score.deflation_error.as_deref(), Some(NO_WINDOW_VARIES));
    assert_eq!(score.deflated_sharpe, 0.0);
    assert!(!score.rank_eligible);
}

/// The evasion is not limited to two windows or to positive levels.
#[test]
fn any_number_of_constant_windows_is_refused() {
    let cfg = ScoreConfig::default();
    for levels in [
        vec![0.001, 0.002],
        vec![0.001, 0.002, 0.003],
        vec![-0.002, 0.004],
        vec![0.0, 0.001],
        vec![0.001 * 1.0, 0.002, 0.003, 0.004, 0.005, 0.006, 0.007, 0.008],
    ] {
        let score = score_agent(&constant_windows("trap", &levels, 30), &cfg);
        assert_eq!(
            score.deflation_error.as_deref(),
            Some(NO_WINDOW_VARIES),
            "{levels:?}"
        );
    }
}

/// Averaging execution replicates of a constant window leaves it constant, so
/// the seed-block path is refused on the same rule.
#[test]
fn constant_execution_replicates_average_to_a_constant_window() {
    let cfg = ScoreConfig {
        execution_seeds_per_window: 2,
        ..ScoreConfig::default()
    };
    let sub = agent(
        "trap",
        vec![
            vec![0.001; 30],
            vec![0.0012; 30],
            vec![0.002; 30],
            vec![0.0022; 30],
        ],
    );
    let score = score_agent(&sub, &cfg);
    assert_eq!(score.deflation_error.as_deref(), Some(NO_WINDOW_VARIES));
}

/// Standing aside in one regime is not the same mistake. The traded window is
/// what the Sharpe ratio is measured on, and the agent keeps it.
#[test]
fn a_flat_window_beside_a_traded_one_still_has_a_sharpe_ratio() {
    let cfg = ScoreConfig::default();
    let mut sub = honest(3);
    sub.runs[0].returns = vec![0.0; 30];
    let score = score_agent(&sub, &cfg);
    assert_eq!(score.deflation_error, None);
    assert!(score.deflated_sharpe.is_finite());
}

/// The messages a track already carried do not move: the pooled question is
/// asked first, and only a track it lets through can meet the window rule.
#[test]
fn a_wholly_constant_track_keeps_the_error_it_had() {
    const CONSTANT_TRACK: &str =
        "returns must not be constant: a constant series has no Sharpe ratio";
    let cfg = ScoreConfig::default();
    for levels in [vec![0.0, 0.0], vec![0.001, 0.001]] {
        let score = score_agent(&constant_windows("flat", &levels, 30), &cfg);
        assert_eq!(
            score.deflation_error.as_deref(),
            Some(CONSTANT_TRACK),
            "{levels:?}"
        );
    }
    let empty = score_agent(&agent("empty", vec![Vec::new()]), &cfg);
    assert_eq!(
        empty.deflation_error.as_deref(),
        Some("at least 2 observations are required, got 0")
    );
}

/// The measured field dispersion: one such entrant moved the bar for everybody.
///
/// On the tree that had the defect this field measures an annualized bar of
/// 1.3692 and seven honest deflated Sharpes between 0.4076 and 0.7038. Adding
/// the entrant took the bar to 36.7192 and every honest deflated Sharpe to
/// exactly zero, while the entrant itself scored 1.0000 and sat on the Pareto
/// front.
#[test]
fn a_constant_window_track_does_not_vote_on_the_field_dispersion() {
    let cfg = ScoreConfig::default();
    let honest_field: Vec<AgentSubmission> = (0..7).map(honest).collect();

    let clean = rank(&honest_field, &cfg);
    assert_eq!(
        row(&clean, "honest-0").trials_sr_std_source,
        TrialsSrStdSource::Measured,
        "the honest field must reach the measured path, or this test proves nothing"
    );

    let mut with_trap = honest_field.clone();
    with_trap.push(constant_windows("split constant", &[0.001, 0.002], 30));
    let board = rank(&with_trap, &cfg);
    let trap = row(&board, "split constant");

    assert_eq!(trap.deflation_error.as_deref(), Some(NO_WINDOW_VARIES));
    assert_eq!(trap.deflated_sharpe, 0.0, "a refused track is not deflated");
    assert!(!trap.pareto_optimal, "a refused track is not a member");
    assert!(!trap.rank_eligible);

    for i in 0..7 {
        let id = format!("honest-{i}");
        let before = row(&clean, &id);
        let after = row(&board, &id);
        assert_eq!(
            before.trials_sr_std, after.trials_sr_std,
            "{id}: the entrant must cast no dispersion vote"
        );
        assert_eq!(
            before.deflation_bar_annualized_equivalent, after.deflation_bar_annualized_equivalent,
            "{id}: the entrant must not move anybody's bar"
        );
        assert_eq!(
            before.deflated_sharpe, after.deflated_sharpe,
            "{id}: the entrant must not move anybody's deflated Sharpe"
        );
        assert!(after.deflated_sharpe > 0.0, "{id}: not zeroed");
    }
}

/// The honest field's own numbers, so the test above is not vacuous: the bar it
/// holds is a real bar, and a vote from the trap would have moved it.
#[test]
fn the_dispersion_the_trap_would_have_cast_is_large() {
    // Per-period Sharpe of two thirty-bar windows at 0.001 and 0.002, by hand:
    // mean 0.0015, sample sd sqrt(60 * 0.0005^2 / 59).
    let pooled: Vec<f64> = [0.001; 30].into_iter().chain([0.002; 30]).collect();
    let mean = pooled.iter().sum::<f64>() / 60.0;
    let var = pooled.iter().map(|x| (x - mean) * (x - mean)).sum::<f64>() / 59.0;
    let sharpe = mean / var.sqrt();
    assert!(
        (sharpe - 2.974_895).abs() < 1e-5,
        "per-period Sharpe {sharpe}"
    );
    assert!(
        (sharpe * 252.0_f64.sqrt() - 47.22).abs() < 0.01,
        "annualized {}",
        sharpe * 252.0_f64.sqrt()
    );
}
