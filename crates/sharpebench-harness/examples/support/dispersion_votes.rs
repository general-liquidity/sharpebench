//! What a future dispersion needs from a scored field.
//!
//! A frozen record carries the dispersion its field measured and not the votes
//! that produced it, so a later change to how votes are aggregated cannot be
//! evaluated on that record: the field has to be re-simulated. These two columns
//! close that for a rerun. Each entrant's pooled per-period Sharpe and its clone
//! cluster are the kernel's own inputs to `measured_trials_sr_std`, computed by
//! the same public functions, so any later estimator over those votes can be
//! recomputed from the record alone.
//!
//! Unavailable stays typed: an entrant with no Sharpe ratio records `None` and
//! the refusal, never a zero, because a zero is a vote.

#![allow(dead_code)] // each producer uses the subset its own contract needs.

use std::collections::BTreeMap;

use sharpebench_core::composite::{window_tracks, AgentSubmission};
use sharpebench_core::deflated_sharpe::observed_sharpe_ratio_of_windows;
use sharpebench_core::rediscovery::{clone_clusters, CLONE_COLLAPSE_COSINE};

/// One entrant's standing in the dispersion sample.
#[derive(Clone, Debug, PartialEq)]
pub struct DispersionVote {
    /// Pooled per-period Sharpe: seed-averaged, window-concatenated, the value
    /// the kernel votes. `None` when the track has no Sharpe ratio.
    pub pooled_sharpe_per_period: Option<f64>,
    /// Why there is none. `None` when there is one.
    pub pooled_sharpe_unavailable: Option<String>,
    /// Clone cluster among the voting entrants, numbered in the order of
    /// `clone_clusters`. `None` for an entrant that does not vote.
    pub clone_cluster: Option<usize>,
}

/// Every entrant's vote, keyed by id. Assumes a field in which every entrant
/// completed the same cells, which is what these producers build; `rank`'s
/// shared-cell restriction is then the identity.
pub fn dispersion_votes(
    subs: &[AgentSubmission],
    seeds_per_window: usize,
) -> BTreeMap<String, DispersionVote> {
    let tracks: Vec<Vec<Vec<f64>>> = subs
        .iter()
        .map(|s| window_tracks(s, seeds_per_window))
        .collect();
    let sharpes: Vec<_> = tracks
        .iter()
        .map(|t| {
            let windows: Vec<&[f64]> = t.iter().map(Vec::as_slice).collect();
            observed_sharpe_ratio_of_windows(&windows)
        })
        .collect();
    let voting: Vec<usize> = (0..subs.len()).filter(|&i| sharpes[i].is_ok()).collect();
    let streams: Vec<Vec<f64>> = voting.iter().map(|&i| tracks[i].concat()).collect();
    let mut cluster_of: BTreeMap<usize, usize> = BTreeMap::new();
    for (cluster, members) in clone_clusters(&streams, CLONE_COLLAPSE_COSINE, false)
        .iter()
        .enumerate()
    {
        for &member in members {
            cluster_of.insert(voting[member], cluster);
        }
    }
    subs.iter()
        .enumerate()
        .map(|(i, sub)| {
            let (sharpe, refusal) = match &sharpes[i] {
                Ok(value) => (Some(*value), None),
                Err(error) => (None, Some(error.to_string())),
            };
            (
                sub.agent_id.clone(),
                DispersionVote {
                    pooled_sharpe_per_period: sharpe,
                    pooled_sharpe_unavailable: refusal,
                    clone_cluster: cluster_of.get(&i).copied(),
                },
            )
        })
        .collect()
}

/// Lowercase hex SHA-256 of a score config as serialized, so a record names the
/// exact configuration it was scored under and not only the columns the record
/// happens to print.
pub fn config_digest(cfg: &sharpebench_core::composite::ScoreConfig) -> String {
    use sha2::{Digest, Sha256};
    let bytes = serde_json::to_vec(cfg).expect("a score config serializes");
    Sha256::digest(&bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}
