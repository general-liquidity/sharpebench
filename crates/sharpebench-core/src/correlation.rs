//! Cross-agent correlation / **crowdedness** — how much an agent is just riding
//! the same factor as everyone else. Two agents with identical Sharpe are not
//! equally valuable: the one whose returns are uncorrelated with the field is
//! diversifying skill; the one tracking the crowd is renting a common beta that
//! will decay (and crash) for the whole field at once. We report each agent's
//! correlation with the rest of the board so crowded edges are visible.
//!
//! Reported, not gating — the third sibling of `decay` and `calibration`. Pure
//! and deterministic: pairwise Pearson with a fixed (field-order) reduction.

use crate::stats::mean;

/// An agent's crowdedness against the rest of the field.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Crowdedness {
    /// Mean pairwise Pearson correlation with the field, in [-1, 1]. `None` when
    /// no peer yields a defined correlation (empty field / all degenerate).
    pub mean_corr: Option<f64>,
    /// Correlation with the agent's *most-correlated* peer, in [-1, 1] — the
    /// "who am I a clone of" signal. `None` under the same condition.
    pub max_corr: Option<f64>,
    /// Number of peers that yielded a defined correlation.
    pub n_peers: usize,
}

/// Pearson correlation of two series, paired by index (extra tail entries on the
/// longer side are ignored). `None` — never `NaN` — when there are fewer than 2
/// pairs or either series has zero variance (correlation is undefined there).
pub fn pearson(a: &[f64], b: &[f64]) -> Option<f64> {
    let n = a.len().min(b.len());
    if n < 2 {
        return None;
    }
    let a = &a[..n];
    let b = &b[..n];
    let ma = mean(a);
    let mb = mean(b);
    // Fixed index-order reduction so the result is bit-reproducible.
    let mut cov = 0.0;
    let mut va = 0.0;
    let mut vb = 0.0;
    for i in 0..n {
        let da = a[i] - ma;
        let db = b[i] - mb;
        cov += da * db;
        va += da * da;
        vb += db * db;
    }
    if va == 0.0 || vb == 0.0 {
        return None;
    }
    Some((cov / (va.sqrt() * vb.sqrt())).clamp(-1.0, 1.0))
}

/// Score an agent's crowdedness: its mean and max Pearson correlation against
/// each member of `field` (the other agents' aligned return series). Peers that
/// yield an undefined correlation (too short / zero-variance) are skipped.
pub fn crowdedness(agent: &[f64], field: &[&[f64]]) -> Crowdedness {
    let mut corrs: Vec<f64> = Vec::with_capacity(field.len());
    for peer in field {
        if let Some(r) = pearson(agent, peer) {
            corrs.push(r);
        }
    }
    let n_peers = corrs.len();
    if n_peers == 0 {
        return Crowdedness {
            mean_corr: None,
            max_corr: None,
            n_peers: 0,
        };
    }
    let mean_corr = corrs.iter().sum::<f64>() / n_peers as f64;
    let max_corr = corrs.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    Crowdedness {
        mean_corr: Some(mean_corr),
        max_corr: Some(max_corr),
        n_peers,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn approx(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-12
    }

    #[test]
    fn identical_series_is_perfectly_correlated() {
        let a = [1.0, 2.0, 3.0, 4.0, 5.0];
        assert!(approx(pearson(&a, &a).unwrap(), 1.0));
    }

    #[test]
    fn affine_transform_is_perfectly_correlated() {
        // b = 2a + 1 — a positive affine map preserves correlation exactly.
        let a = [1.0, 2.0, 3.0, 4.0, 5.0];
        let b = [3.0, 5.0, 7.0, 9.0, 11.0];
        assert!(approx(pearson(&a, &b).unwrap(), 1.0));
    }

    #[test]
    fn reversed_series_is_perfectly_anticorrelated() {
        let a = [1.0, 2.0, 3.0, 4.0, 5.0];
        let b = [5.0, 4.0, 3.0, 2.0, 1.0];
        assert!(approx(pearson(&a, &b).unwrap(), -1.0));
    }

    #[test]
    fn orthogonal_series_is_uncorrelated() {
        // Two zero-mean orthogonal vectors → correlation exactly 0.
        let a = [1.0, -1.0, 1.0, -1.0];
        let b = [1.0, 1.0, -1.0, -1.0];
        assert!(approx(pearson(&a, &b).unwrap(), 0.0));
    }

    #[test]
    fn zero_variance_is_undefined() {
        let a = [1.0, 2.0, 3.0, 4.0];
        let flat = [2.0, 2.0, 2.0, 2.0];
        assert!(pearson(&a, &flat).is_none());
    }

    #[test]
    fn too_short_is_undefined() {
        assert!(pearson(&[1.0], &[1.0]).is_none());
    }

    #[test]
    fn crowdedness_summarizes_field() {
        // Field: one clone (+1) and one mirror (-1) → mean 0, max 1, 2 peers.
        let agent = [1.0, 2.0, 3.0, 4.0, 5.0];
        let clone = [1.0, 2.0, 3.0, 4.0, 5.0];
        let mirror = [5.0, 4.0, 3.0, 2.0, 1.0];
        let c = crowdedness(&agent, &[&clone, &mirror]);
        assert_eq!(c.n_peers, 2);
        assert!(approx(c.mean_corr.unwrap(), 0.0));
        assert!(approx(c.max_corr.unwrap(), 1.0));
    }

    #[test]
    fn crowdedness_skips_degenerate_peers_and_empty_field() {
        let agent = [1.0, 2.0, 3.0, 4.0];
        // Empty field → undefined.
        let empty = crowdedness(&agent, &[]);
        assert_eq!(empty.n_peers, 0);
        assert!(empty.mean_corr.is_none() && empty.max_corr.is_none());
        // A single zero-variance peer is skipped → still undefined.
        let flat = [7.0, 7.0, 7.0, 7.0];
        let degenerate = crowdedness(&agent, &[&flat]);
        assert_eq!(degenerate.n_peers, 0);
        assert!(degenerate.mean_corr.is_none());
    }
}
