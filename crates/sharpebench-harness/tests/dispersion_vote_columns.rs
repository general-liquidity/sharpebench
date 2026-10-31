//! The columns `evidence_sweep` and `external_rules_eval` add must be enough to
//! recompute a measured dispersion from a record alone. These tests rebuild
//! `trials_sr_std` from the votes the way the kernel does and compare bits, so a
//! column that drifts from the kernel's own vote fails here and not in a later
//! study.

#[path = "../examples/support/dispersion_votes.rs"]
mod dispersion_votes;

use std::collections::BTreeMap;

use dispersion_votes::{config_digest, dispersion_votes};
use sharpebench_core::composite::{rank, Run, ScoreConfig, TrialsSrStdSource};
use sharpebench_core::stats::std_dev;
use sharpebench_core::AgentSubmission;

const SEEDS: usize = 2;
const WINDOWS: usize = 3;
const BARS: usize = 60;

/// A deterministic series with a mean set by `edge`; `salt` decorrelates.
fn track(edge: f64, salt: u64) -> Vec<f64> {
    let mut state = salt.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1;
    (0..BARS * WINDOWS)
        .map(|_| {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            let u = (state >> 11) as f64 / (1u64 << 53) as f64;
            edge + 0.01 * (u - 0.5)
        })
        .collect()
}

fn submission(id: &str, series: &[f64], noise: f64) -> AgentSubmission {
    let mut runs = Vec::new();
    for w in 0..WINDOWS {
        for s in 0..SEEDS {
            let sign = if s == 0 { 1.0 } else { -1.0 };
            runs.push(Run {
                returns: series[w * BARS..(w + 1) * BARS]
                    .iter()
                    .enumerate()
                    .map(|(t, x)| x + sign * noise * ((t % 7) as f64 - 3.0))
                    .collect(),
                ..Run::default()
            });
        }
    }
    AgentSubmission {
        agent_id: id.to_string(),
        runs,
        in_sample_trials: 0,
        candidates: Vec::new(),
    }
}

/// The kernel's aggregation: one vote per clone cluster, the lower middle of its
/// sorted Sharpes, sample standard deviation over the votes.
fn recomputed_dispersion(votes: &BTreeMap<String, dispersion_votes::DispersionVote>) -> f64 {
    let mut clusters: BTreeMap<usize, Vec<f64>> = BTreeMap::new();
    for vote in votes.values() {
        if let (Some(sharpe), Some(cluster)) = (vote.pooled_sharpe_per_period, vote.clone_cluster) {
            clusters.entry(cluster).or_default().push(sharpe);
        }
    }
    let mut sample: Vec<f64> = clusters
        .into_values()
        .map(|mut members| {
            members.sort_by(f64::total_cmp);
            members[(members.len() - 1) / 2]
        })
        .collect();
    sample.sort_by(f64::total_cmp);
    std_dev(&sample)
}

fn cfg() -> ScoreConfig {
    ScoreConfig {
        execution_seeds_per_window: SEEDS,
        // The floor would hide the measured value this test exists to compare.
        min_measured_trials_sr_std: 0.0,
        min_field_for_measured_sr_std: 5,
        n_boot: 50,
        ..ScoreConfig::default()
    }
}

fn field() -> Vec<AgentSubmission> {
    let mut subs: Vec<AgentSubmission> = (0..6)
        .map(|i| {
            submission(
                &format!("agent-{i}"),
                &track(0.0004 * i as f64, i + 1),
                1e-4,
            )
        })
        .collect();
    // Two near-duplicates of one stream: a clone cluster with one vote.
    let base = track(0.0011, 40);
    subs.push(submission("clone-a", &base, 0.0));
    subs.push(submission("clone-b", &base, 1e-8));
    subs
}

#[test]
fn the_recorded_votes_recompute_the_measured_dispersion_bit_for_bit() {
    let subs = field();
    let config = cfg();
    let board = rank(&subs, &config);
    let scored = &board[0];
    assert_eq!(scored.trials_sr_std_source, TrialsSrStdSource::Measured);
    let votes = dispersion_votes(&subs, SEEDS);
    assert_eq!(votes.len(), subs.len());
    assert_eq!(
        votes["clone-a"].clone_cluster, votes["clone-b"].clone_cluster,
        "the two near-duplicates must share one vote"
    );
    assert_eq!(
        recomputed_dispersion(&votes).to_bits(),
        scored.trials_sr_std.to_bits()
    );
}

#[test]
fn a_track_with_no_sharpe_ratio_records_no_vote_and_a_typed_reason() {
    let mut subs = field();
    subs.push(submission("flat", &vec![0.0; BARS * WINDOWS], 0.0));
    let votes = dispersion_votes(&subs, SEEDS);
    let flat = &votes["flat"];
    assert_eq!(flat.pooled_sharpe_per_period, None);
    assert_eq!(flat.clone_cluster, None);
    assert!(
        flat.pooled_sharpe_unavailable.is_some(),
        "an unavailable vote must say why, not read as zero"
    );
}

#[test]
fn the_config_digest_moves_with_the_config() {
    let a = cfg();
    let b = ScoreConfig {
        dsr_bar: 0.9,
        ..cfg()
    };
    assert_eq!(config_digest(&a), config_digest(&cfg()));
    assert_ne!(config_digest(&a), config_digest(&b));
}
