//! Votes that are alike count by how independent they are.
//!
//! The clone collapse folds streams whose `|cosine|` reaches 0.995 into one
//! vote. A flood of puppets just below that, at a pairwise correlation of 0.97
//! to 0.99, used to cast one full vote each: 32 of them around the field's median
//! Sharpe crowded the middle, took the measured dispersion of a seven-agent
//! field from 0.277 to 0.114-0.122 per period on the grader search's fixed seed,
//! and lifted 11 of 32 zero-edge targets that the honest field refuses. The
//! measured dispersion now weighs each vote by `1 / sum_j s_ij`, the share of
//! variance it has in common with the others beyond sampling noise, and never
//! goes below the plain sample standard deviation, so the flood is worth about
//! one vote and an ordinary field is measured exactly as before.
//!
//! Deterministic: a fixed-seed generator, no clock, no market data.

use sharpebench_core::{rank, AgentSubmission, CompositeScore, Run, ScoreConfig};
use sharpebench_stats::deflated_sharpe::observed_sharpe_ratio;
use sharpebench_stats::stats::std_dev;

const BARS: usize = 120;
const HONEST_SHARPES: [f64; 6] = [0.40, 0.45, 0.50, 0.55, 0.60, 0.65];

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> f64 {
        // SplitMix64, mapped to (0, 1).
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

/// Zero-mean, unit-variance noise.
fn standardized(rng: &mut Rng, n: usize) -> Vec<f64> {
    let raw: Vec<f64> = (0..n).map(|_| rng.gauss()).collect();
    let mean = raw.iter().sum::<f64>() / n as f64;
    let centred: Vec<f64> = raw.iter().map(|x| x - mean).collect();
    let sd = std_dev(&centred);
    centred.iter().map(|x| x / sd).collect()
}

/// A track whose per-period Sharpe is `sharpe` to rounding.
fn sharpe_track(rng: &mut Rng, sharpe: f64) -> Vec<f64> {
    standardized(rng, BARS)
        .iter()
        .map(|z| 0.01 * (sharpe + z))
        .collect()
}

fn agent(id: &str, returns: Vec<f64>) -> AgentSubmission {
    AgentSubmission {
        agent_id: id.into(),
        runs: vec![Run {
            returns,
            ..Run::default()
        }],
        in_sample_trials: 0,
        candidates: Vec::new(),
    }
}

fn honest_field() -> Vec<AgentSubmission> {
    HONEST_SHARPES
        .iter()
        .enumerate()
        .map(|(k, &sharpe)| {
            let mut rng = Rng(0xF1E1_D000 + k as u64);
            agent(&format!("honest-{k}"), sharpe_track(&mut rng, sharpe))
        })
        .collect()
}

/// `count` puppets around one base track at the field's median Sharpe, each the
/// base plus its own independent noise, scaled so any two of them correlate at
/// about `pair_correlation`: well below the collapse's 0.995, well above noise.
fn puppets(count: usize, pair_correlation: f64) -> Vec<AgentSubmission> {
    let mut rng = Rng(0x0BAD_5EED);
    let base = sharpe_track(&mut rng, 0.525);
    let scale = (1.0 / pair_correlation - 1.0).sqrt() * 0.01;
    (0..count)
        .map(|k| {
            let noise = standardized(&mut rng, BARS);
            let returns = base
                .iter()
                .zip(&noise)
                .map(|(b, e)| b + scale * e)
                .collect();
            agent(&format!("puppet-{k:02}"), returns)
        })
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
        .unwrap_or_else(|| panic!("{id} is on the board"))
}

fn measured_on(field: &[AgentSubmission]) -> f64 {
    row(&rank(field, &cfg()), "honest-0").trials_sr_std
}

fn plain_dispersion(field: &[AgentSubmission]) -> f64 {
    let mut votes: Vec<f64> = field
        .iter()
        .map(|s| observed_sharpe_ratio(&s.runs[0].returns).expect("every track varies"))
        .collect();
    votes.sort_by(|a, b| a.partial_cmp(b).unwrap());
    std_dev(&votes)
}

/// The regression. Against the tree before the weighting the 32 puppets at 0.97
/// each cast a full vote, and the honest rows measure about 0.44 of the
/// dispersion the field measures without them.
#[test]
fn puppets_below_the_collapse_do_not_crowd_the_bar_down() {
    let honest = honest_field();
    let clean = measured_on(&honest);
    for pair_correlation in [0.90, 0.97, 0.99] {
        let mut field = honest.clone();
        field.extend(puppets(32, pair_correlation));
        let plain = plain_dispersion(&field);
        let measured = measured_on(&field);
        assert!(
            plain < 0.6 * clean,
            "{pair_correlation}: the flood must crowd the plain dispersion, {plain} against {clean}"
        );
        assert!(
            measured > 0.85 * clean,
            "{pair_correlation}: 32 puppets moved the measured dispersion from {clean} to {measured}"
        );
    }
}

/// What the flood is worth: about the one vote a single puppet casts, however
/// many members it has.
#[test]
fn a_flood_of_near_copies_weighs_about_one_vote() {
    let honest = honest_field();
    let mut one = honest.clone();
    one.extend(puppets(1, 0.97));
    let single = measured_on(&one);
    for count in [4, 16, 32] {
        let mut field = honest.clone();
        field.extend(puppets(count, 0.97));
        let measured = measured_on(&field);
        assert!(
            (measured / single - 1.0).abs() < 0.10,
            "{count} puppets measure {measured}, one measures {single}"
        );
    }
}

/// A field whose streams are independent draws is measured with the plain
/// sample standard deviation of its votes, bit for bit: sampling correlation
/// alone re-weights nobody.
#[test]
fn an_independent_field_is_measured_exactly_as_before() {
    let field = honest_field();
    let plain = plain_dispersion(&field);
    for r in &rank(&field, &cfg()) {
        assert_eq!(r.trials_sr_std.to_bits(), plain.to_bits(), "{}", r.agent_id);
    }
}

/// The weighting only ever raises the dispersion it is applied to: a field of
/// agents on one shared factor with unequal loadings is re-weighted, and its
/// measured dispersion is at least the plain one.
#[test]
fn the_measured_dispersion_is_never_below_the_plain_one() {
    let mut rng = Rng(0xFAC7_0000);
    let factor = standardized(&mut rng, BARS);
    let field: Vec<AgentSubmission> = (0..8)
        .map(|k| {
            let loading = 0.2 + 0.1 * k as f64;
            let own = standardized(&mut rng, BARS);
            let drift = 0.0005 * k as f64;
            let returns = factor
                .iter()
                .zip(&own)
                .map(|(f, e)| drift + 0.01 * (loading * f + e))
                .collect();
            agent(&format!("honest-{k}"), returns)
        })
        .collect();
    let plain = plain_dispersion(&field);
    let measured = measured_on(&field);
    assert!(measured >= plain, "{measured} below the plain {plain}");
}

/// Submission order moves nothing: the weights are read in the sorted order of
/// the votes.
#[test]
fn the_weighting_does_not_depend_on_submission_order() {
    let mut field = honest_field();
    field.extend(puppets(16, 0.97));
    let measured = measured_on(&field);
    let mut shuffled = field.clone();
    shuffled.rotate_left(5);
    shuffled.swap(1, 17);
    assert_eq!(measured_on(&shuffled).to_bits(), measured.to_bits());
}

/// The switch that turns the collapse off turns the weighting off with it, so
/// the exposure it exists to reproduce is still reproducible.
#[test]
fn turning_the_clone_defences_off_measures_every_stream_as_a_vote() {
    let mut field = honest_field();
    field.extend(puppets(32, 0.97));
    let off = ScoreConfig {
        dedup_clones_for_measured_sr_std: false,
        ..cfg()
    };
    let board = rank(&field, &off);
    assert_eq!(
        row(&board, "honest-0").trials_sr_std.to_bits(),
        plain_dispersion(&field).to_bits()
    );
}
