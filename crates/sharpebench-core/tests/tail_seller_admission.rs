//! A tail seller that never lost a bar is refused; one that shows its losses is not.
//!
//! Found by `sharpebench grader-search` on 2026-10-30 (seed 20261030, finding
//! `tail_seller-9e4d48d5382b822c`). A seller of tail insurance collects a premium
//! of 2e-4 on each bar with noise a quarter of it and pays a crash at rate 0.001
//! sized so that every bar's expected return is zero. Its thirty bars drew no
//! crash and no loss, a per-bar Sharpe of 3.8 sits below Samuelson's bound of 5.3,
//! and it was eligible alone and in a thirteen-agent field, PSR and DSR 1.0. Its
//! sample skewness and kurtosis are the premium's, so the PSR's moment terms had
//! nothing to penalize: the tail they price was never in the sample.
//!
//! The rule is `shows_an_observed_loss` in `sharpebench-stats`: a sign test, so it
//! reads no constant and does not move with position size.

use sharpebench_core::{rank, AgentSubmission, CompositeScore, Run, ScoreConfig};
use sharpebench_stats::stats::{mean, std_dev};

/// Literal, so this file fails on assertions against the tree with the defect.
const REFUSED: &str = concat!(
    "returns must show an observed loss: no bar of any window is below zero, so the sample ",
    "holds no observation of the left tail and its moments describe the gains only"
);

/// The admitted track, bit for bit from the report's `return_bits`.
const FOUND: [u64; 30] = [
    0x3f3203b02c48c7f6,
    0x3f23ead934bf9574,
    0x3f2083600d33a7d8,
    0x3f2811333bd8d07e,
    0x3f24ab136b27a7da,
    0x3f2f8535c45a9e39,
    0x3f21f7aa86de6d5a,
    0x3f223262e351ef74,
    0x3f33c756e35cfbf6,
    0x3f23b521022565af,
    0x3f30acf1b94513cd,
    0x3f2e037d1f5bc7e6,
    0x3f23f6fd909ac49f,
    0x3f2c0137bd2db835,
    0x3f2b89034ffb5468,
    0x3f2723f668a5d8f6,
    0x3f2f34a7ee2270e4,
    0x3f3026e12801e679,
    0x3f1b587b752b4c23,
    0x3f2f172c58f87e40,
    0x3f2a89dcd7095ba3,
    0x3f29045049f20335,
    0x3f326a508d31b783,
    0x3f27102cf531532b,
    0x3f2406bd2ac71a65,
    0x3f2866b9807d89ef,
    0x3f31b7b76e842bc3,
    0x3f2b2d4cc9c11b78,
    0x3f2984b116fa3d02,
    0x3f212ae0c9a9e850,
];

fn found() -> Vec<f64> {
    FOUND.iter().map(|b| f64::from_bits(*b)).collect()
}

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

/// A window whose Sharpe is `sharpe` to rounding, on a deterministic carrier
/// with unit standard deviation before scaling.
fn moving(seed: usize, bars: usize, sharpe: f64, scale: f64) -> Vec<f64> {
    let k = seed as f64;
    let raw: Vec<f64> = (0..bars)
        .map(|t| {
            let t = t as f64;
            (t * (0.37 + 0.113 * k) + 0.9 * k).sin() + 0.5 * (t * (1.61 + 0.07 * k) + k).cos()
        })
        .collect();
    let centre = mean(&raw);
    let sd = std_dev(&raw);
    raw.iter()
        .map(|x| scale * (sharpe + (x - centre) / sd))
        .collect()
}

/// Honest entrants on the reported track's layout, one window of thirty bars,
/// so the seller changes no entrant's common support.
fn honest_field(n: usize) -> Vec<AgentSubmission> {
    (0..n)
        .map(|k| {
            let sharpe = 0.40 + 0.05 * (k % 6) as f64;
            agent(&format!("honest-{k}"), vec![moving(k, 30, sharpe, 0.01)])
        })
        .collect()
}

fn row<'a>(board: &'a [CompositeScore], id: &str) -> &'a CompositeScore {
    board
        .iter()
        .find(|r| r.agent_id == id)
        .expect("on the board")
}

fn cfg() -> ScoreConfig {
    ScoreConfig::default()
}

/// The reported track, alone and in the honest fields it was admitted in, is
/// refused by name, is not eligible and leaves every honest entrant as it was.
#[test]
fn the_tail_seller_that_missed_its_crash_is_refused_alone_and_in_a_field() {
    let windows = vec![found()];
    assert!(
        windows[0].iter().all(|x| *x > 0.0),
        "the report's track never lost"
    );
    let alone = rank(&[agent("seller", windows.clone())], &cfg());
    assert_eq!(alone[0].deflation_error.as_deref(), Some(REFUSED));
    assert!(!alone[0].rank_eligible);

    for honest_count in [6, 12] {
        let clean = rank(&honest_field(honest_count), &cfg());
        let mut field = honest_field(honest_count);
        field.push(agent("seller", windows.clone()));
        let board = rank(&field, &cfg());
        let seller = row(&board, "seller");
        assert_eq!(seller.deflation_error.as_deref(), Some(REFUSED));
        assert!(!seller.rank_eligible);
        for r in &clean {
            let after = row(&board, &r.agent_id);
            assert_eq!(
                r.deflation_bar_per_period.to_bits(),
                after.deflation_bar_per_period.to_bits(),
                "{} lost its bar",
                r.agent_id
            );
            assert_eq!(r.rank_eligible, after.rank_eligible, "{}", r.agent_id);
        }
    }
}

/// A sign does not move with size: the same track at any position size is
/// refused, and a track that loses on ordinary bars is kept at every size.
#[test]
fn the_refusal_does_not_move_with_position_size() {
    for scale in [1e-6, 1.0, 1e3] {
        let seller: Vec<f64> = found().iter().map(|x| x * scale).collect();
        let board = rank(&[agent("seller", vec![seller])], &cfg());
        assert_eq!(
            board[0].deflation_error.as_deref(),
            Some(REFUSED),
            "{scale}"
        );
        let losing = vec![moving(1, 60, 1.0, scale), moving(2, 60, 1.0, scale)];
        let board = rank(&[agent("losing", losing)], &cfg());
        assert_eq!(board[0].deflation_error, None, "{scale}");
    }
}

/// When the crash is in the sample the PSR prices it, with no help from this
/// rule. When it is not, a seller whose premium noise loses on ordinary bars is,
/// bar for bar, the honest Gaussian track of its premium: the same series, so the
/// same verdict. That is the limit the rule leaves, and the reason it is not a
/// cap on the Sharpe ratio.
#[test]
fn a_crash_in_the_sample_is_priced_and_one_outside_it_cannot_be() {
    // Per-bar Sharpe 1 with unit noise, so about one bar in six loses.
    let premium = || vec![moving(7, 120, 1.0, 0.002), moving(8, 120, 1.0, 0.002)];
    let crash_free = rank(&[agent("crash-free", premium())], &cfg());
    assert_eq!(crash_free[0].deflation_error, None);
    assert!(crash_free[0].rank_eligible, "{:?}", crash_free[0]);

    // One crash at rate 0.01, sized so the expected bar return is zero.
    let crash_rate = 0.01;
    let mut crashed = premium();
    crashed[1][60] = -0.002 * (1.0 - crash_rate) / crash_rate;
    let board = rank(&[agent("crashed", crashed)], &cfg());
    assert_eq!(board[0].deflation_error, None, "priced, not refused");
    assert!(!board[0].rank_eligible, "{:?}", board[0]);
}
