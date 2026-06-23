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
}
