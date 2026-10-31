//! A track that never moved is refused, however it was perturbed.
//!
//! Found by `sharpebench grader-search` on 2026-10-30. Two windows of sixty bars,
//! 0.001 + 1e-8 on even bars and 0.001 - 1e-8 on odd ones, carry no skill by
//! construction and were eligible, ordinal 1 and DSR 1.0 through `sharpebench
//! score` in honest fields. Its Sharpe is about 1e5, the vote fence kept that
//! out of everybody else's bar but left it a bar measured with its own vote, one
//! vote clears such a bar, and a kurtosis of one keeps the PSR denominator small.
//!
//! Refusal is Samuelson's inequality, so these tests straddle a theorem and not
//! a tuned constant: `sign_is_forced_by_moments` in `sharpebench-stats`.

use sharpebench_core::{rank, AgentSubmission, CompositeScore, Run, ScoreConfig};
use sharpebench_stats::stats::{mean, std_dev};

/// Literal, so this file fails on assertions against the tree with the defect.
const REFUSED: &str = concat!(
    "returns must show observed risk: in every window the moments prove that no bar is a loss, ",
    "or would once its single most distant bar is set aside, so the dispersion measures a ",
    "perturbation around a level and not risk"
);

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

/// A window whose Sharpe is `sharpe` to rounding, on a deterministic carrier.
fn moving(seed: usize, bars: usize, sharpe: f64) -> Vec<f64> {
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
        .map(|x| 0.01 * (sharpe + (x - centre) / sd))
        .collect()
}

fn honest_field(n: usize) -> Vec<AgentSubmission> {
    (0..n)
        .map(|k| {
            let sharpe = 0.40 + 0.05 * (k % 6) as f64;
            agent(
                &format!("honest-{k}"),
                vec![moving(k, 60, sharpe), moving(k + 40, 60, sharpe)],
            )
        })
        .collect()
}

fn alternating(eps: f64) -> Vec<Vec<f64>> {
    (0..2)
        .map(|_| {
            (0..60)
                .map(|t| 0.001 + if t % 2 == 0 { eps } else { -eps })
                .collect()
        })
        .collect()
}

fn one_bar(spike: f64) -> Vec<Vec<f64>> {
    (0..2)
        .map(|w| {
            let mut window = vec![0.01_f64; 30];
            window[3 + w] += spike;
            window
        })
        .collect()
}

fn level_steps() -> Vec<Vec<f64>> {
    [0.001, 0.002]
        .iter()
        .map(|level| {
            (0..30)
                .map(|t| level + 1e-12 * if t % 3 == 0 { 1.0 } else { -0.5 })
                .collect()
        })
        .collect()
}

/// Nothing but a last-bar coin in each window: the whole Sharpe is two draws.
fn one_trade() -> Vec<Vec<f64>> {
    (0..2)
        .map(|_| {
            let mut window = vec![0.0_f64; 30];
            window[29] = 0.01;
            window
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

/// Every shape the search admitted, alone and in the honest fields that ranked
/// it first, is refused by name, votes on nothing and moves no honest number.
#[test]
fn a_track_that_never_moved_is_refused_alone_and_in_a_field() {
    let shapes = [
        ("alternating 1e-8", alternating(1e-8)),
        ("alternating 1e-12", alternating(1e-12)),
        ("one bar 1e-12", one_bar(1e-12)),
        ("one bar -1e-3", one_bar(-1e-3)),
        ("level steps", level_steps()),
        ("one bar 0.05", one_bar(0.05)),
        ("one bar -0.05", one_bar(-0.05)),
        ("one trade at zero", one_trade()),
    ];
    for (label, windows) in shapes {
        let alone = rank(&[agent("attacker", windows.clone())], &cfg());
        assert_eq!(
            alone[0].deflation_error.as_deref(),
            Some(REFUSED),
            "{label}"
        );
        assert!(!alone[0].rank_eligible, "{label}");

        for honest_count in [6, 7, 12, 24] {
            let clean = rank(&honest_field(honest_count), &cfg());
            let mut attacked = honest_field(honest_count);
            attacked.push(agent("attacker", windows.clone()));
            let board = rank(&attacked, &cfg());
            let attacker = row(&board, "attacker");
            assert_eq!(
                attacker.deflation_error.as_deref(),
                Some(REFUSED),
                "{label}"
            );
            assert!(!attacker.rank_eligible, "{label}");
            assert!(!attacker.pareto_optimal, "{label}");
            for r in &clean {
                let after = row(&board, &r.agent_id);
                assert_eq!(
                    r.deflation_bar_per_period.to_bits(),
                    after.deflation_bar_per_period.to_bits(),
                    "{label}: {} lost its bar",
                    r.agent_id
                );
                assert_eq!(r.rank_eligible, after.rank_eligible, "{label}");
            }
        }
    }
}

/// A flood of near-identical copies is refused copy by copy and casts no vote,
/// so no copy is eligible and the honest bar is the clean field's, bit for bit.
#[test]
fn a_flood_of_near_constant_copies_moves_no_honest_bar() {
    let flat_with_one_moved_bar = |offset: usize| -> Vec<Vec<f64>> {
        (0..2)
            .map(|w| {
                let mut window = vec![0.001_f64; 60];
                window[(20 + offset + 7 * w) % 60] += 1e-12;
                window
            })
            .collect()
    };
    let clean = rank(&honest_field(6), &cfg());
    let mut field = honest_field(6);
    for k in 0..3 {
        field.push(agent(&format!("copy-{k}"), flat_with_one_moved_bar(k)));
    }
    let board = rank(&field, &cfg());
    let honest_bar = row(&board, "honest-0").trials_sr_std;
    assert!(honest_bar < 1.0, "the honest field still measures itself");
    assert_eq!(
        honest_bar.to_bits(),
        row(&clean, "honest-0").trials_sr_std.to_bits()
    );
    for k in 0..3 {
        let copy = row(&board, &format!("copy-{k}"));
        assert_eq!(copy.deflation_error.as_deref(), Some(REFUSED));
        assert!(!copy.rank_eligible, "{copy:?}");
    }
}

/// The rule is the theorem's boundary, stepped across from both sides: just
/// below it the moments do not force a sign, just above they do and the kernel
/// refuses. Well below it, where even the leave-one-out reading cannot cross,
/// the track keeps its Sharpe.
#[test]
fn the_refusal_stands_at_samuelsons_bound() {
    use sharpebench_stats::sign_is_forced_by_moments;
    let n = 60_usize;
    let bound = (n as f64 - 1.0) / (n as f64).sqrt();
    let window = |sharpe: f64| moving(3, n, sharpe);
    assert!(!sign_is_forced_by_moments(&window(bound * 0.99)));
    assert!(sign_is_forced_by_moments(&window(bound * 1.01)));

    let above = rank(&[agent("above", vec![window(bound * 1.01); 2])], &cfg());
    assert_eq!(above[0].deflation_error.as_deref(), Some(REFUSED));
    // A window this strong still loses no bar, so the observed-loss rule
    // (tests/tail_seller_admission.rs) refuses it under its own name. What this
    // test pins is that the moments no longer force its sign.
    let well_below = rank(&[agent("below", vec![window(bound * 0.5); 2])], &cfg());
    assert_ne!(well_below[0].deflation_error.as_deref(), Some(REFUSED));
}

/// The rule reads a ratio. Rescaling a track changes neither its Sharpe nor its
/// answer, so a small position is not refused for being small and a near-constant
/// track cannot buy its way out with size.
#[test]
fn the_answer_does_not_move_with_position_size() {
    for scale in [1e-6, 1.0, 1e3] {
        let scaled = |windows: Vec<Vec<f64>>| -> Vec<Vec<f64>> {
            windows
                .into_iter()
                .map(|w| w.into_iter().map(|x| x * scale).collect())
                .collect()
        };
        let refused = rank(&[agent("a", scaled(alternating(1e-8)))], &cfg());
        assert_eq!(
            refused[0].deflation_error.as_deref(),
            Some(REFUSED),
            "{scale}"
        );
        let kept = rank(
            &[agent(
                "b",
                scaled(vec![moving(1, 60, 1.0), moving(2, 60, 1.0)]),
            )],
            &cfg(),
        );
        assert_eq!(kept[0].deflation_error, None, "{scale}");
    }
}

/// The false positive the rule must not have. A truly strong honest track, far
/// outside the fence, is not refused and is admitted. At a per-bar Sharpe of 1.8
/// its carrier still loses on some bars: a track that never loses is refused by
/// the observed-loss rule (tests/tail_seller_admission.rs), not by this one.
#[test]
fn a_strong_honest_track_far_from_the_field_is_not_refused() {
    let windows = vec![moving(50, 60, 1.8), moving(51, 60, 1.8)];
    assert!(windows.concat().iter().any(|x| *x < 0.0));
    let mut field = honest_field(6);
    field.push(agent("strong", windows));
    let board = rank(&field, &cfg());
    let strong = row(&board, "strong");
    assert_eq!(strong.deflation_error, None);
    assert!(strong.rank_eligible, "{strong:?}");
}

/// One window that resolves its level keeps the track its Sharpe: a flat level
/// beside an ordinary window is not the defect.
#[test]
fn one_ordinary_window_beside_a_flat_level_keeps_the_sharpe() {
    let windows = vec![vec![0.001; 60], moving(4, 60, 1.0)];
    let board = rank(&[agent("mixed", windows)], &cfg());
    assert_eq!(board[0].deflation_error, None);
}

/// The rule must not become a median test. Bailey and Lopez de Prado's worked
/// example sits on one value for most bars, and it has 105 bars away from that
/// value, so no single observation carries it. A sparse agent that trades on two
/// bars of a window keeps its Sharpe the same way.
#[test]
fn a_distribution_with_a_mode_is_not_a_track_that_never_moved() {
    let mut discrete = vec![0.01_f64; 1145];
    discrete.extend(vec![-0.02_f64; 105]);
    let board = rank(&[agent("discrete", vec![discrete])], &cfg());
    assert_eq!(board[0].deflation_error, None);

    let mut sparse = vec![0.0_f64; 30];
    sparse[4] = 0.01;
    sparse[17] = -0.004;
    let board = rank(&[agent("sparse", vec![sparse.clone(), sparse])], &cfg());
    assert_eq!(board[0].deflation_error, None);
}
