//! Cross-agent correlation / **crowdedness** — how much an agent is just riding
//! the same factor as everyone else. Two agents with identical Sharpe are not
//! equally valuable: the one whose returns are uncorrelated with the field is
//! diversifying skill; the one tracking the crowd is renting a common beta that
//! will decay (and crash) for the whole field at once. We report each agent's
//! correlation with the rest of the board so crowded edges are visible.
//!
//! Reported, not gating — the third sibling of `decay` and `calibration`. Pure
//! and deterministic: pairwise Pearson with a fixed (field-order) reduction.

use crate::deflated_sharpe::is_constant_track;
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
/// pairs or either series is constant (correlation is undefined there).
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
    // A constant series is recognised by value as well as by its computed sum of
    // squares. An all-zero one sums to exactly zero and was already refused; a
    // constant nonzero one leaves a residual near 1e-37 around its rounded mean,
    // which divides to a noise "correlation" near 1e-17 and counted the stream
    // as a peer of every agent on the board.
    if va == 0.0 || vb == 0.0 || is_constant_track(a) || is_constant_track(b) {
        return None;
    }
    Some((cov / (va.sqrt() * vb.sqrt())).clamp(-1.0, 1.0))
}

/// Score an agent's crowdedness: its mean and max Pearson correlation against
/// each member of `field` (the other agents' aligned return series). Peers that
/// yield an undefined correlation (too short / constant) are skipped.
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

    /// A constant series whose value is not exactly representable leaves a
    /// residual sum of squared deviations around its rounded mean, so the
    /// zero-variance guard alone did not catch it: `pearson` returned a noise
    /// correlation near 2e-17 and the stream counted as a peer.
    ///
    /// Isolated: the same series at a value the mean rounds exactly (2.0 above)
    /// was already refused, so the residual is the whole cause.
    #[test]
    fn a_constant_nonzero_series_is_undefined_not_a_noise_correlation() {
        let a: Vec<f64> = (0..60).map(|i| 0.002 + 0.0005 * (i as f64).sin()).collect();
        let flat = vec![0.001_f64; 60];
        assert_ne!(
            flat.iter()
                .map(|x| x - mean(&flat))
                .map(|d| d * d)
                .sum::<f64>(),
            0.0,
            "the residual this test is about must exist"
        );
        assert!(pearson(&a, &flat).is_none());
        assert!(pearson(&flat, &a).is_none());
        let c = crowdedness(&a, &[&flat]);
        assert_eq!((c.mean_corr, c.max_corr, c.n_peers), (None, None, 0));
    }

    /// The variance guard is not made redundant by the constant-track rule. A
    /// series can hold distinct values whose deviations are small enough that
    /// every squared deviation underflows to zero: `[0.0, 1e-200]` has a mean of
    /// 5e-201 and a sum of squared deviations of exactly 0, and dividing by that
    /// yields a non-finite ratio rather than a correlation.
    ///
    /// Isolated: the series is not constant, so the constant-track rule does not
    /// fire and only the zero-variance disjunct can refuse it.
    #[test]
    fn a_series_whose_squared_deviations_underflow_is_undefined() {
        let underflowing = [0.0_f64, 1e-200];
        let dispersed = [0.002_f64, 0.0035];
        assert_ne!(
            underflowing[0], underflowing[1],
            "the series this test is about must not be constant"
        );
        assert_eq!(
            underflowing
                .iter()
                .map(|x| x - mean(&underflowing))
                .map(|d| d * d)
                .sum::<f64>(),
            0.0,
            "the underflow this test is about must occur"
        );
        assert!(pearson(&underflowing, &dispersed).is_none());
        assert!(pearson(&dispersed, &underflowing).is_none());
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
