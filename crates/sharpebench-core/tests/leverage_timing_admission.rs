//! Leverage timing: exposure sized from the track's own profit and loss.
//!
//! The grader search of 2026-10-30 found leverage-timing tracks admitted with no
//! edge: martingale doubling, pyramiding and stopping once ahead. Its strongest
//! form is optional stopping, which trades a fair coin until the running
//! t-statistic is high and then goes flat, so the bars that could take the gain
//! back are never drawn. These tests pin that such a track is refused for the
//! risk its exposure history implies, and that honest dynamic sizing, which
//! sizes from the market and not from its own profit, keeps its eligibility.

use sharpebench_core::{rank, AgentSubmission, CompositeScore, PassMode, Run, ScoreConfig};
use sharpebench_stats::exposure_path::{forgone_risk_z, EXPOSURE_PATH_UNPROVEN};

/// SplitMix64: additions, shifts and multiplications only, so every platform
/// draws the same bits.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    fn coin(&mut self) -> f64 {
        if self.next() >> 63 == 0 {
            1.0
        } else {
            -1.0
        }
    }
}

fn agent(id: &str, windows: &[Vec<f64>]) -> AgentSubmission {
    AgentSubmission {
        agent_id: id.to_string(),
        runs: windows
            .iter()
            .map(|returns| Run {
                returns: returns.clone(),
                ..Run::default()
            })
            .collect(),
        ..AgentSubmission::default()
    }
}

fn score_alone(windows: &[Vec<f64>]) -> CompositeScore {
    let cfg = ScoreConfig {
        pass_mode: PassMode::All,
        ..ScoreConfig::default()
    };
    rank(&[agent("candidate", windows)], &cfg)
        .into_iter()
        .next()
        .expect("one row")
}

/// Every gate but the exposure-path one: what the kernel answered before it.
fn eligible_without_exposure_path(s: &CompositeScore) -> bool {
    let cfg = ScoreConfig::default();
    s.deflated_sharpe >= cfg.dsr_bar
        && s.passed_k
        && s.process_ok
        && s.bootstrap_error.is_none()
        && s.deflation_error.is_none()
        && s.bootstrap_p < cfg.alpha
        && s.mandate_ok
}

/// A fair coin of size `sigma` a bar, traded until the running t-statistic
/// `S_k / (sigma sqrt(k))` reaches `stop_at`, then flat to the window's end.
/// No bar has an edge: the stake is chosen from the past only.
fn stop_when_significant(seed: u64, windows: usize, bars: usize, stop_at: f64) -> Vec<Vec<f64>> {
    let sigma = 0.01;
    let mut rng = Rng(seed);
    (0..windows)
        .map(|_| {
            let mut sum = 0.0_f64;
            let mut stopped = false;
            (1..=bars)
                .map(|k| {
                    if stopped {
                        return 0.0;
                    }
                    let r = sigma * rng.coin();
                    sum += r;
                    stopped = sum > 0.0 && sum >= stop_at * sigma * (k as f64).sqrt();
                    r
                })
                .collect()
        })
        .collect()
}

/// Volatility targeting with a fixed rule: the stake is a target over the
/// exponentially weighted volatility of the strategy's own past returns, on a
/// market whose volatility switches between regimes and whose every bar carries
/// a positive edge of 0.3 of its volatility.
fn volatility_targeted(seed: u64, windows: usize, bars: usize) -> Vec<Vec<f64>> {
    let mut rng = Rng(seed);
    (0..windows)
        .map(|w| {
            let mut ewma = 1e-4_f64;
            (0..bars)
                .map(|t| {
                    let vol = if (t / 20 + w) % 3 == 0 { 0.03 } else { 0.01 };
                    let market = vol * (0.3 + rng.coin());
                    let stake = 0.01 / ewma.sqrt();
                    ewma = 0.94 * ewma + 0.06 * market * market;
                    stake * market
                })
                .collect()
        })
        .collect()
}

/// A fixed Kelly fraction of the same market: exposure moves with the market's
/// volatility and never with the strategy's own profit.
fn fixed_fraction(seed: u64, windows: usize, bars: usize) -> Vec<Vec<f64>> {
    let mut rng = Rng(seed);
    (0..windows)
        .map(|w| {
            (0..bars)
                .map(|t| {
                    let vol = if (t / 20 + w) % 3 == 0 { 0.03 } else { 0.01 };
                    0.5 * vol * (0.3 + rng.coin())
                })
                .collect()
        })
        .collect()
}

#[test]
fn going_flat_once_significant_is_refused_for_the_risk_it_stopped_carrying() {
    let track = stop_when_significant(STOPPER_SEED, 2, 60, 2.0);
    let score = score_alone(&track);
    assert!(
        eligible_without_exposure_path(&score),
        "the fixture must be one every other gate admits: {score:?}"
    );
    assert!(!score.rank_eligible, "{score:?}");
    let reason = score
        .exposure_path_error
        .as_deref()
        .expect("a typed refusal");
    assert!(reason.contains(EXPOSURE_PATH_UNPROVEN), "{reason}");

    // Scale-free: the same decisions at a thousandth of the size are refused too.
    let small: Vec<Vec<f64>> = track
        .iter()
        .map(|w| w.iter().map(|x| x * 1e-3).collect())
        .collect();
    assert!(score_alone(&small).exposure_path_error.is_some());
}

#[test]
fn honest_dynamic_sizing_keeps_its_eligibility() {
    for (label, track) in [
        (
            "volatility targeting",
            volatility_targeted(HONEST_SEED, 2, 120),
        ),
        ("fixed Kelly fraction", fixed_fraction(HONEST_SEED, 2, 120)),
    ] {
        let score = score_alone(&track);
        let windows: Vec<&[f64]> = track.iter().map(Vec::as_slice).collect();
        assert!(
            score.rank_eligible && score.exposure_path_error.is_none(),
            "{label}: z {:?} {score:?}",
            forgone_risk_z(&windows)
        );
    }
}

/// A seed whose stopped coin every other gate admits; 94 of seeds 0..4000 do,
/// against 16 of 4000 for a coin of the same size traded to the end.
const STOPPER_SEED: u64 = 63;
const HONEST_SEED: u64 = 0;
