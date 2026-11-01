//! One vote cannot set everybody else's deflation bar from just inside the fence.
//!
//! The grader search's dispersion-grief move (seed 20261030) put one vote at
//! 0.5 to 0.995 of the 10.75-scale vote fence beside six honest entrants and took
//! their measured bar from 0.21 to between 0.77 and 1.48 per period, from either
//! side, and their eligibility from 6 to 0. The influence bound now clips each
//! admitted vote to the honest field's 99th-percentile robust distance, so a
//! griefer anywhere past that reach measures the same bar, and the clipped
//! entrant is scored against the bar its own vote measured as cast.
//!
//! Deterministic: a fixed-seed generator, no clock, no market data.

use sharpebench_core::{rank, AgentSubmission, CompositeScore, Run, ScoreConfig};
use sharpebench_stats::stats::std_dev;

const BARS: usize = 60;
const HONEST_SHARPES: [f64; 6] = [0.40, 0.45, 0.50, 0.55, 0.60, 0.65];

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> f64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^= z >> 31;
        ((z >> 11) as f64 + 0.5) / (1u64 << 53) as f64
    }

    fn gauss(&mut self) -> f64 {
        let (u, v) = (self.next(), self.next());
        (-2.0 * u.ln()).sqrt() * (2.0 * std::f64::consts::PI * v).cos()
    }
}

/// Two windows whose pooled per-period Sharpe is `sharpe` to rounding.
fn track(seed: u64, sharpe: f64) -> Vec<Vec<f64>> {
    let mut rng = Rng(seed);
    let raw: Vec<f64> = (0..2 * BARS).map(|_| rng.gauss()).collect();
    let mean = raw.iter().sum::<f64>() / raw.len() as f64;
    let centred: Vec<f64> = raw.iter().map(|x| x - mean).collect();
    let sd = std_dev(&centred);
    let z: Vec<f64> = centred.iter().map(|x| 0.01 * (sharpe + x / sd)).collect();
    vec![z[..BARS].to_vec(), z[BARS..].to_vec()]
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

fn honest_field() -> Vec<AgentSubmission> {
    HONEST_SHARPES
        .iter()
        .enumerate()
        .map(|(k, &s)| agent(&format!("honest-{k}"), track(0xF1E1_D000 + k as u64, s)))
        .collect()
}

fn cfg() -> ScoreConfig {
    ScoreConfig {
        n_boot: 50,
        ..ScoreConfig::default()
    }
}

fn row<'a>(board: &'a [CompositeScore], id: &str) -> &'a CompositeScore {
    board
        .iter()
        .find(|r| r.agent_id == id)
        .expect("on the board")
}

fn with_griefer(sharpe: f64) -> Vec<CompositeScore> {
    let mut field = honest_field();
    field.push(agent("griefer", track(0x6721_4546, sharpe)));
    rank(&field, &cfg())
}

/// The regression. Without the bound the honest bar grows with the griefer's
/// distance (0.77 to 1.48 on the grader search's fixture); with it, every
/// position past the reach measures the same honest bar.
#[test]
fn a_griefer_past_the_reach_measures_one_bar_wherever_it_sits() {
    let near = with_griefer(1.4);
    let far = with_griefer(2.2);
    let low = with_griefer(-0.5);
    let lower = with_griefer(-0.9);
    let bar = |b: &[CompositeScore]| row(b, "honest-0").trials_sr_std;
    assert_eq!(bar(&near).to_bits(), bar(&far).to_bits());
    assert_eq!(bar(&low).to_bits(), bar(&lower).to_bits());
    for b in [&near, &far, &low, &lower] {
        let lev = row(b, "honest-0")
            .trials_sr_std_most_influential_vote
            .as_ref()
            .expect("a measured row discloses");
        assert_eq!(lev.clipped.len(), 1, "{lev:?}");
        assert_eq!(lev.clipped[0].agent_id, "griefer");
        assert!(lev.dispersion_before_clip.unwrap() > bar(b));
    }
}

/// Clipping is not a discount: the griefer is scored against the dispersion
/// its vote measured as cast, which is above the bounded one.
#[test]
fn the_clipped_vote_keeps_the_bar_it_measured() {
    let board = with_griefer(2.2);
    let mut field = honest_field();
    field.push(agent("griefer", track(0x6721_4546, 2.2)));
    let mut votes: Vec<f64> = field
        .iter()
        .map(|s| {
            let pooled: Vec<f64> = s.runs.iter().flat_map(|r| r.returns.clone()).collect();
            sharpebench_stats::deflated_sharpe::observed_sharpe_ratio(&pooled).unwrap()
        })
        .collect();
    votes.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let griefer = row(&board, "griefer");
    assert_eq!(griefer.trials_sr_std.to_bits(), std_dev(&votes).to_bits());
    assert!(griefer.trials_sr_std > row(&board, "honest-0").trials_sr_std);
    assert!(griefer.trials_sr_std_most_influential_vote.is_none());
}

/// A field with nobody past the reach is measured exactly as before: the
/// bound only ever lowers a dispersion it binds on.
#[test]
fn an_honest_field_is_not_clipped() {
    let board = rank(&honest_field(), &cfg());
    let lev = row(&board, "honest-0")
        .trials_sr_std_most_influential_vote
        .as_ref()
        .expect("a measured row discloses");
    assert!(lev.clipped.is_empty());
    assert!(lev.dispersion_before_clip.is_none());
}

/// Declared controls cast no vote and are named as such on the board.
#[test]
fn a_declared_control_is_named_and_does_not_vote() {
    let mut field = honest_field();
    field.push(agent("control", track(0xC0, 2.2)));
    let declared = ScoreConfig {
        dispersion_controls: vec!["control".to_string()],
        ..cfg()
    };
    let board = rank(&field, &declared);
    let clean = rank(&honest_field(), &cfg());
    assert_eq!(
        row(&board, "honest-0").trials_sr_std.to_bits(),
        row(&clean, "honest-0").trials_sr_std.to_bits(),
        "the control moved the entrants' bar"
    );
    let lev = row(&board, "control")
        .trials_sr_std_most_influential_vote
        .as_ref()
        .expect("a control is scored on the measured bar and sees the disclosure");
    assert_eq!(lev.abstained.len(), 1);
    assert_eq!(lev.abstained[0].agent_id, "control");
    assert_eq!(
        serde_json::to_value(lev.abstained[0].reason).unwrap(),
        "control"
    );
}
