//! Rediscovery / strategy-recycling detection.
//!
//! A leaderboard is contaminated if an agent resubmits a known prior strategy —
//! a public factor, a leaked baseline, or last season's winner — dressed up as
//! novel. The Deflated Sharpe doesn't catch it (the recycled stream may genuinely
//! be skilled); the harm is to *novelty*, not to luck-robustness. So we screen
//! the submitted pooled return stream against a library of KNOWN prior strategy
//! streams and flag near-duplicates.
//!
//! The similarity metric is **cosine** (not Pearson): two strategies are the same
//! when their return *vectors* point the same direction, including scale and sign.
//! An agent that simply leverages a known stream up 2× has cosine 1.0 with it
//! (same direction) while an inverted clone has cosine -1.0 — both are flagged on
//! `|cos|` because an inverse is just as much a non-novel recycling. Centering is
//! optional (`center: false` by default) — for raw return streams the direction
//! is the strategy; centering would conflate it with Pearson.
//!
//! Pure and deterministic: fixed index-order reduction, no RNG.

use serde::{Deserialize, Serialize};

use crate::stats::mean;

/// Cosine similarity of two series, paired by index (extra tail entries on the
/// longer side are ignored). When `center` is true each series is de-meaned first
/// (which makes this Pearson). `None` — never `NaN` — when there are fewer than 2
/// pairs or either series has zero norm (direction is undefined there).
pub fn cosine_similarity(a: &[f64], b: &[f64], center: bool) -> Option<f64> {
    let n = a.len().min(b.len());
    if n < 2 {
        return None;
    }
    let (ma, mb) = if center {
        (mean(&a[..n]), mean(&b[..n]))
    } else {
        (0.0, 0.0)
    };
    let mut dot = 0.0;
    let mut na = 0.0;
    let mut nb = 0.0;
    for i in 0..n {
        let da = a[i] - ma;
        let db = b[i] - mb;
        dot += da * db;
        na += da * da;
        nb += db * db;
    }
    if na == 0.0 || nb == 0.0 {
        return None;
    }
    Some((dot / (na.sqrt() * nb.sqrt())).clamp(-1.0, 1.0))
}

/// The verdict from screening a submission against a known-strategy library.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct RediscoveryVerdict {
    /// True when `max_similarity` meets or exceeds `threshold`.
    pub is_rediscovery: bool,
    /// The largest `|cosine|` against any known stream, in [0, 1]. 0.0 if the
    /// library is empty or no known stream yielded a defined similarity.
    pub max_similarity: f64,
    /// Index of the nearest known stream (in `known` order), or `None` if none
    /// yielded a defined similarity.
    pub nearest_index: Option<usize>,
    /// The threshold applied (echoed for legibility/audit).
    pub threshold: f64,
}

/// Default rediscovery threshold on `|cosine|`. At 0.97 a stream must be all but
/// collinear with a known one to be flagged — leverage/sign variants included,
/// merely-correlated-but-distinct strategies excluded.
pub const DEFAULT_REDISCOVERY_THRESHOLD: f64 = 0.97;

/// Screen a submitted pooled return stream against a library of known prior
/// strategy streams. Flags rediscovery when the maximum `|cosine|` similarity
/// against any known stream meets or exceeds `threshold`.
///
/// `center` is forwarded to [`cosine_similarity`]; pass `false` (the default
/// semantics) to compare raw direction, `true` to de-mean first.
pub fn classify_rediscovery(
    submitted: &[f64],
    known: &[Vec<f64>],
    threshold: f64,
    center: bool,
) -> RediscoveryVerdict {
    let mut max_similarity = 0.0_f64;
    let mut nearest_index = None;
    for (i, k) in known.iter().enumerate() {
        if let Some(c) = cosine_similarity(submitted, k, center) {
            let abs = c.abs();
            if abs > max_similarity {
                max_similarity = abs;
                nearest_index = Some(i);
            }
        }
    }
    RediscoveryVerdict {
        is_rediscovery: nearest_index.is_some() && max_similarity >= threshold,
        max_similarity,
        nearest_index,
        threshold,
    }
}

/// Clone-collapse threshold on `|cosine|`: the similarity at which two
/// submitted streams are one *vote* on the field-measured `trials_sr_std` (see
/// [`crate::composite::rank`]). Deliberately distinct from
/// [`DEFAULT_REDISCOVERY_THRESHOLD`], because the two serve different purposes:
/// rediscovery flags a *similar strategy* for review, and 0.97 (about 14
/// degrees) is the right net for that; the collapse removes *duplicate votes*
/// and must never silence an honest, merely collinear agent. Honest agents are
/// collinear: on the benchmark's own evidence fields a long-only random-exposure
/// luck-floor agent on a one- to five-symbol universe sits at cosine 0.971 to
/// 0.990 against buy-and-hold, and two such agents at 0.975 against each other,
/// and none of those is a duplicate vote. Sock puppets, by contrast, are copies:
/// the self-audit's 200 puppets sit at 0.99999 or above against each other.
/// 0.995 (about 5.7 degrees) is above the honest maximum with margin and well
/// below any copy that differs by less than about ten percent independent noise.
/// The harness pins the zero-merge property on every committed evidence field.
pub const CLONE_COLLAPSE_COSINE: f64 = 0.995;

/// Partition `streams` into near-clone clusters: two streams are joined when
/// their `|cosine|` (see [`cosine_similarity`], with `center` forwarded) meets or
/// exceeds `threshold`, and clusters are the connected components of that
/// relation (single linkage). A stream with an undefined similarity to every
/// other, a zero-norm one for instance, is its own singleton.
///
/// The partition is a property of the pairwise similarities alone, so it does
/// not depend on submission order: every component is listed by ascending
/// member index and the components are ordered by their smallest member. That
/// is what lets [`crate::composite::rank`] collapse a sock-puppet field to one
/// vote per cluster (at [`CLONE_COLLAPSE_COSINE`]) before measuring the field's
/// Sharpe dispersion, with a result that reshuffling the field cannot move.
pub fn clone_clusters(streams: &[Vec<f64>], threshold: f64, center: bool) -> Vec<Vec<usize>> {
    let n = streams.len();
    let mut parent: Vec<usize> = (0..n).collect();
    fn find(parent: &mut [usize], mut i: usize) -> usize {
        while parent[i] != i {
            parent[i] = parent[parent[i]];
            i = parent[i];
        }
        i
    }
    for i in 0..n {
        for j in (i + 1)..n {
            let joined = cosine_similarity(&streams[i], &streams[j], center)
                .is_some_and(|c| c.abs() >= threshold);
            if joined {
                let (ri, rj) = (find(&mut parent, i), find(&mut parent, j));
                if ri != rj {
                    // Root at the smaller index so the walk below is canonical.
                    parent[ri.max(rj)] = ri.min(rj);
                }
            }
        }
    }
    let mut clusters: Vec<Vec<usize>> = Vec::new();
    let mut slot: Vec<Option<usize>> = vec![None; n];
    for i in 0..n {
        let r = find(&mut parent, i);
        match slot[r] {
            Some(k) => clusters[k].push(i),
            None => {
                slot[r] = Some(clusters.len());
                clusters.push(vec![i]);
            }
        }
    }
    clusters
}

#[cfg(test)]
mod tests {
    use super::*;

    fn approx(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-12
    }

    fn stream(seed: f64, n: usize) -> Vec<f64> {
        (0..n)
            .map(|i| (i as f64 * 0.37 + seed).sin() * 0.01 + 0.001)
            .collect()
    }

    #[test]
    fn identical_is_cosine_one() {
        let a = stream(1.0, 50);
        assert!(approx(cosine_similarity(&a, &a, false).unwrap(), 1.0));
    }

    #[test]
    fn scaled_stream_stays_collinear() {
        // 3× leverage of a known stream → cosine 1.0 (uncentered direction).
        let a = stream(2.0, 50);
        let scaled: Vec<f64> = a.iter().map(|x| x * 3.0).collect();
        assert!(approx(cosine_similarity(&a, &scaled, false).unwrap(), 1.0));
    }

    #[test]
    fn inverse_is_cosine_minus_one() {
        let a = stream(3.0, 50);
        let inv: Vec<f64> = a.iter().map(|x| -x).collect();
        assert!(approx(cosine_similarity(&a, &inv, false).unwrap(), -1.0));
    }

    #[test]
    fn zero_norm_is_undefined() {
        let a = stream(1.0, 40);
        let flat = vec![0.0; 40];
        assert!(cosine_similarity(&a, &flat, false).is_none());
    }

    #[test]
    fn near_duplicate_flags() {
        let known = stream(5.0, 60);
        // A 2× leveraged, slightly-noised resubmission of the known stream.
        let submitted: Vec<f64> = known
            .iter()
            .enumerate()
            .map(|(i, x)| x * 2.0 + 1e-6 * (i as f64).cos())
            .collect();
        let v = classify_rediscovery(&submitted, &[known], DEFAULT_REDISCOVERY_THRESHOLD, false);
        assert!(v.is_rediscovery, "leveraged clone should flag: {v:?}");
        assert_eq!(v.nearest_index, Some(0));
        assert!(v.max_similarity >= DEFAULT_REDISCOVERY_THRESHOLD);
    }

    #[test]
    fn inverse_clone_flags_on_abs() {
        let known = stream(7.0, 60);
        let inv: Vec<f64> = known.iter().map(|x| -x).collect();
        let v = classify_rediscovery(&inv, &[known], DEFAULT_REDISCOVERY_THRESHOLD, false);
        assert!(v.is_rediscovery, "an inverse clone is non-novel too: {v:?}");
    }

    #[test]
    fn novel_stream_does_not_flag() {
        let known = stream(1.0, 80);
        // An independent, differently-phased stream.
        let novel: Vec<f64> = (0..80)
            .map(|i| (i as f64 * 0.91 + 13.0).cos() * 0.008 - 0.0004)
            .collect();
        let v = classify_rediscovery(&novel, &[known], DEFAULT_REDISCOVERY_THRESHOLD, false);
        assert!(
            !v.is_rediscovery,
            "an independent stream must not flag: {v:?}"
        );
        assert!(v.max_similarity < DEFAULT_REDISCOVERY_THRESHOLD);
    }

    #[test]
    fn empty_library_never_flags() {
        let v = classify_rediscovery(&stream(1.0, 30), &[], DEFAULT_REDISCOVERY_THRESHOLD, false);
        assert!(!v.is_rediscovery);
        assert_eq!(v.nearest_index, None);
        assert_eq!(v.max_similarity, 0.0);
    }

    #[test]
    fn picks_nearest_of_several() {
        let target = stream(4.0, 60);
        let decoy = stream(20.0, 60);
        let submitted: Vec<f64> = target.iter().map(|x| x * 1.5).collect();
        let v = classify_rediscovery(
            &submitted,
            &[decoy, target],
            DEFAULT_REDISCOVERY_THRESHOLD,
            false,
        );
        assert_eq!(
            v.nearest_index,
            Some(1),
            "should match the target, not decoy"
        );
        assert!(v.is_rediscovery);
    }

    #[test]
    fn clusters_collapse_clones_and_keep_distinct_streams_apart() {
        let a = stream(1.0, 60);
        let a2: Vec<f64> = a.iter().map(|x| x * 2.0).collect();
        let a_inv: Vec<f64> = a.iter().map(|x| -x).collect();
        let b: Vec<f64> = (0..60)
            .map(|i| (i as f64 * 0.91 + 13.0).cos() * 0.008 - 0.0004)
            .collect();
        let flat = vec![0.0; 60];
        let streams = vec![b.clone(), a.clone(), flat, a2, a_inv];
        let clusters = clone_clusters(&streams, DEFAULT_REDISCOVERY_THRESHOLD, false);
        assert_eq!(clusters, vec![vec![0], vec![1, 3, 4], vec![2]]);
    }

    #[test]
    fn clusters_are_order_independent() {
        let a = stream(2.0, 50);
        let a2: Vec<f64> = a.iter().map(|x| x * 3.0).collect();
        let b = stream(20.0, 50);
        let forward = vec![a.clone(), a2.clone(), b.clone()];
        let backward = vec![b, a2, a];
        let f = clone_clusters(&forward, DEFAULT_REDISCOVERY_THRESHOLD, false);
        let r = clone_clusters(&backward, DEFAULT_REDISCOVERY_THRESHOLD, false);
        // Same partition once member indices are mapped back to the streams.
        let name = |v: &[Vec<f64>], c: &[usize]| -> Vec<Vec<f64>> {
            let mut m: Vec<Vec<f64>> = c.iter().map(|&i| v[i].clone()).collect();
            m.sort_by(|x, y| x[0].partial_cmp(&y[0]).unwrap());
            m
        };
        let mut pf: Vec<Vec<Vec<f64>>> = f.iter().map(|c| name(&forward, c)).collect();
        let mut pr: Vec<Vec<Vec<f64>>> = r.iter().map(|c| name(&backward, c)).collect();
        pf.sort_by(|x, y| x[0][0].partial_cmp(&y[0][0]).unwrap());
        pr.sort_by(|x, y| x[0][0].partial_cmp(&y[0][0]).unwrap());
        assert_eq!(pf, pr);
        assert_eq!(pf.len(), 2);
    }

    #[test]
    fn empty_and_singleton_fields_cluster_trivially() {
        assert!(clone_clusters(&[], DEFAULT_REDISCOVERY_THRESHOLD, false).is_empty());
        let one = vec![stream(1.0, 30)];
        assert_eq!(
            clone_clusters(&one, DEFAULT_REDISCOVERY_THRESHOLD, false),
            vec![vec![0]]
        );
    }
}
