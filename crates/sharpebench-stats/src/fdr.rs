//! False-discovery-rate control for a batch of candidate strategies.
//!
//! The data-snooping family in [`crate::significance`] (White's Reality Check,
//! Hansen's SPA, Romano-Wolf step-down) controls the *family-wise* error rate:
//! the probability of even one false positive across the whole field. That is the
//! right guarantee when a single false discovery is catastrophic, but it grows
//! punishing as the batch grows — the critical value scales with the number of
//! agents tried, so real edges in a large field get thrown out with the noise.
//!
//! Benjamini-Hochberg (1995) instead controls the *false-discovery rate*: the
//! expected fraction of the rejected hypotheses that are false positives. It is
//! less conservative than FWER control and keeps its power as the batch grows, so
//! it is the right lens for triaging many candidate strategies at once — you
//! accept that a known fraction `q` of the survivors are luck, in exchange for
//! surfacing far more of the real edges.
//!
//! Pure and deterministic: plain `f64`, a fixed index-order tie-break, no I/O and
//! no randomness.

/// Benjamini-Hochberg (1995) step-up procedure at false-discovery level `q`.
///
/// Given `m` p-values, sort them ascending as `p_(1) <= ... <= p_(m)`, find the
/// largest rank `k` with `p_(k) <= (k / m) * q`, and reject the `k` hypotheses
/// with the smallest p-values. Controls the expected proportion of false
/// discoveries at `q` under independence (and under positive dependence, per
/// Benjamini-Yekutieli 2001).
///
/// Returns a rejection mask in the caller's *original* order (`true` = discovery).
/// An empty batch, or `q <= 0`, rejects nothing. Ties break by original index so
/// the result is fully deterministic.
pub fn benjamini_hochberg(p_values: &[f64], q: f64) -> Vec<bool> {
    let m = p_values.len();
    let mut rejected = vec![false; m];
    if m == 0 || q <= 0.0 {
        return rejected;
    }
    if let Some((order, k)) = bh_cutoff(p_values, q) {
        for &orig in &order[..k] {
            rejected[orig] = true;
        }
    }
    rejected
}

/// A false-discovery-controlled summary over a batch of candidate strategies:
/// how many discoveries survive Benjamini-Hochberg at level `q`.
#[derive(Clone, Debug, PartialEq)]
pub struct FdrVerdict {
    /// The false-discovery level the batch was controlled at.
    pub q: f64,
    /// Number of candidates in the batch.
    pub n_tested: usize,
    /// Number of hypotheses rejected (discoveries surviving FDR control).
    pub n_discoveries: usize,
    /// Per-candidate rejection mask, in the input order (`true` = discovery).
    pub rejected: Vec<bool>,
    /// The largest p-value admitted as a discovery — every p-value at or below
    /// this is rejected. `None` when nothing survives.
    pub threshold: Option<f64>,
}

/// Run Benjamini-Hochberg over a batch and summarize how many discoveries survive
/// false-discovery control at level `q`. See [`benjamini_hochberg`] for the
/// procedure; this wraps it with the batch counts and the admitted-p threshold.
pub fn fdr_verdict(p_values: &[f64], q: f64) -> FdrVerdict {
    let m = p_values.len();
    let (rejected, threshold) = match (m, q) {
        (0, _) => (Vec::new(), None),
        _ if q <= 0.0 => (vec![false; m], None),
        _ => match bh_cutoff(p_values, q) {
            Some((order, k)) => {
                let mut mask = vec![false; m];
                for &orig in &order[..k] {
                    mask[orig] = true;
                }
                (mask, Some(p_values[order[k - 1]]))
            }
            None => (vec![false; m], None),
        },
    };
    let n_discoveries = rejected.iter().filter(|&&r| r).count();
    FdrVerdict {
        q,
        n_tested: m,
        n_discoveries,
        rejected,
        threshold,
    }
}

/// Shared core: order the p-values ascending (deterministic index tie-break) and
/// return that order together with the BH cutoff rank `k` (1-based count of
/// rejections). `None` when no rank satisfies the step-up line. Callers guarantee
/// `p_values` is non-empty and `q > 0`.
fn bh_cutoff(p_values: &[f64], q: f64) -> Option<(Vec<usize>, usize)> {
    let m = p_values.len();
    let mut order: Vec<usize> = (0..m).collect();
    order.sort_by(|&a, &b| {
        p_values[a]
            .partial_cmp(&p_values[b])
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(a.cmp(&b))
    });
    let mut k_star: Option<usize> = None;
    for rank in 1..=m {
        let p = p_values[order[rank - 1]];
        if p <= (rank as f64 / m as f64) * q {
            k_star = Some(rank);
        }
    }
    k_star.map(|k| (order, k))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_vector_rejects_the_correct_count() {
        // Classic Benjamini-Hochberg worked example (m = 10, q = 0.05). Sorted
        // p-values with the (k/m)*q line:
        //   rank  p        (k/10)*0.05
        //   1     0.0008   0.005   <=  reject
        //   2     0.009    0.010   <=  reject
        //   3     0.165    0.015
        //   4     0.205    0.020
        //   ...
        // The largest k meeting the line is k = 2, so exactly two discoveries.
        let p = [
            0.9, 0.009, 0.205, 0.0008, 0.58, 0.51, 0.35, 0.165, 0.396, 0.75,
        ];
        let rej = benjamini_hochberg(&p, 0.05);
        assert_eq!(rej.iter().filter(|&&r| r).count(), 2);
        // The two smallest p-values (0.0008 at idx 3, 0.009 at idx 1) are rejected.
        assert!(rej[3] && rej[1]);
        assert!(rej
            .iter()
            .enumerate()
            .all(|(i, &r)| r == (i == 1 || i == 3)));
    }

    #[test]
    fn all_null_rejects_none() {
        // Uniformly high p-values: no rank clears the step-up line.
        let p = [0.4, 0.6, 0.55, 0.9, 0.72, 0.83];
        let rej = benjamini_hochberg(&p, 0.05);
        assert!(rej.iter().all(|&r| !r));
        let v = fdr_verdict(&p, 0.05);
        assert_eq!(v.n_discoveries, 0);
        assert_eq!(v.threshold, None);
    }

    #[test]
    fn all_significant_rejects_all() {
        let p = [0.001, 0.002, 0.0005, 0.003];
        let rej = benjamini_hochberg(&p, 0.05);
        assert!(rej.iter().all(|&r| r));
    }

    #[test]
    fn is_less_conservative_than_bonferroni() {
        // A batch where several real edges sit just under q but above q/m. FDR
        // control keeps them; the FWER-style Bonferroni cutoff (q/m) would not.
        let p = [0.001, 0.006, 0.012, 0.018, 0.9, 0.8, 0.7, 0.95, 0.6, 0.85];
        let rej = benjamini_hochberg(&p, 0.05);
        let bonferroni = 0.05 / p.len() as f64; // 0.005
        let bonf_count = p.iter().filter(|&&x| x <= bonferroni).count();
        let bh_count = rej.iter().filter(|&&r| r).count();
        assert!(
            bh_count > bonf_count,
            "BH {bh_count} > Bonferroni {bonf_count}"
        );
    }

    #[test]
    fn verdict_reports_threshold_and_counts() {
        let p = [
            0.9, 0.009, 0.205, 0.0008, 0.58, 0.51, 0.35, 0.165, 0.396, 0.75,
        ];
        let v = fdr_verdict(&p, 0.05);
        assert_eq!(v.n_tested, 10);
        assert_eq!(v.n_discoveries, 2);
        assert_eq!(v.q, 0.05);
        // Threshold is the largest admitted p-value (rank k = 2 → 0.009).
        assert_eq!(v.threshold, Some(0.009));
        assert_eq!(v.rejected.iter().filter(|&&r| r).count(), 2);
    }

    #[test]
    fn empty_and_nonpositive_q_are_safe() {
        assert!(benjamini_hochberg(&[], 0.05).is_empty());
        assert!(benjamini_hochberg(&[0.001, 0.002], 0.0).iter().all(|&r| !r));
        let v = fdr_verdict(&[], 0.05);
        assert_eq!(v.n_tested, 0);
        assert_eq!(v.n_discoveries, 0);
        assert_eq!(v.threshold, None);
    }

    #[test]
    fn deterministic_with_tied_pvalues() {
        // Ties break by original index; repeated calls are byte-identical.
        let p = [0.01, 0.01, 0.01, 0.9, 0.9];
        let a = benjamini_hochberg(&p, 0.05);
        let b = benjamini_hochberg(&p, 0.05);
        assert_eq!(a, b);
    }
}
