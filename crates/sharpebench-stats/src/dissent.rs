//! Splitting a disagreement into "the ranking differs" and "the levels differ".
//!
//! Rerun a scorer under a different seed, over a different walk-forward window,
//! or with a different cost profile, and the numbers move. SharpeBench reports
//! that movement as one figure, which conflates two failures that call for
//! opposite responses:
//!
//! - **The ranking moved.** The board is not reproducible. Whoever is on top
//!   depends on the arm, so the leaderboard cannot be published as a ranking at
//!   all until the source of the instability is found.
//! - **The levels moved but the ranking held.** Every entrant shifted together.
//!   The ranking is safe to publish; the absolute figures are not comparable
//!   across arms and must carry their arm.
//!
//! [`dissent`] measures both from the same pair of score vectors, and
//! [`dissent_across`] extends it to a set of arms by averaging over all pairs.
//! Nothing here knows what an arm *is*: seeds, windows, cost profiles and scorer
//! configurations are all the same shape of input.
//!
//! Rank movement is measured with Kendall's tau-b, which corrects for ties in
//! *both* denominators, so tied scores neither inflate nor deflate the reading:
//!
//! - Kendall, M. G. (1938). "A New Measure of Rank Correlation." *Biometrika*
//!   30(1/2), 81-93.
//! - Kendall, M. G. (1945). "The Treatment of Ties in Ranking Problems."
//!   *Biometrika* 33(3), 239-251, which introduces the tau-b tie correction
//!   `(C - D) / sqrt((n0 - n1)(n0 - n2))`.
//!
//! Level movement is measured as the mean absolute difference between the arms,
//! divided by the pooled range of all scores in play. Because every value lies
//! inside that range, the reading is bounded in [0, 1] by construction, with no
//! clamping and no distributional assumption.

/// Default ceiling on rank movement for a board to be publishable as a ranking.
///
/// 0.05 corresponds to tau-b >= 0.90. This is an operating requirement, not a
/// measured constant: pass an explicit [`DissentThresholds`] to move it.
pub const DEFAULT_MAX_RANK_DISSENT: f64 = 0.05;

/// Default ceiling on level movement for absolute scores to be quoted without
/// naming the arm they came from. Same status as [`DEFAULT_MAX_RANK_DISSENT`]:
/// an operating requirement chosen by the operator.
pub const DEFAULT_MAX_LEVEL_DISSENT: f64 = 0.10;

/// Operator-supplied ceilings for [`DissentReport::verdict`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DissentThresholds {
    pub max_rank_dissent: f64,
    pub max_level_dissent: f64,
}

impl Default for DissentThresholds {
    fn default() -> Self {
        Self {
            max_rank_dissent: DEFAULT_MAX_RANK_DISSENT,
            max_level_dissent: DEFAULT_MAX_LEVEL_DISSENT,
        }
    }
}

/// What may be published given how much the arms disagree.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DissentVerdict {
    /// Ranking and levels both reproduce. Gate on either.
    SafeToGate,
    /// The ranking reproduces; the absolute levels do not. Publish the order,
    /// and quote a score only alongside the arm that produced it.
    RankingOnly,
    /// The ranking does not reproduce. Nothing derived from the order may be
    /// gated on, whatever the levels do.
    NotSafeToGate,
    /// Not enough data to decide (fewer than two entrants, or a degenerate arm).
    Undetermined,
}

/// Disagreement between arms, split by kind.
#[derive(Clone, Debug, PartialEq)]
pub struct DissentReport {
    /// Entrants compared.
    pub n: usize,
    /// Arms compared (2 for [`dissent`]).
    pub arms: usize,
    /// Kendall's tau-b between the arms' orderings, in [-1, 1]. `None` when
    /// every value in an arm is tied, which leaves no ordering to compare.
    pub tau_b: Option<f64>,
    /// `(1 - tau_b) / 2`, in [0, 1]. 0 is an identical ordering, 0.5 is an
    /// ordering unrelated to the other arm, 1 is an exact reversal. `None`
    /// exactly when `tau_b` is.
    pub rank_dissent: Option<f64>,
    /// Mean absolute level difference over the pooled range, in [0, 1]. 0 means
    /// the arms produced identical scores. `None` when the pooled range is zero
    /// (every score in every arm identical), which is agreement by degeneracy
    /// rather than a measurement.
    pub level_dissent: Option<f64>,
}

impl DissentReport {
    /// Apply operator ceilings to the split.
    pub fn verdict(&self, t: DissentThresholds) -> DissentVerdict {
        let (Some(rank), Some(level)) = (self.rank_dissent, self.level_dissent) else {
            return DissentVerdict::Undetermined;
        };
        if rank > t.max_rank_dissent {
            return DissentVerdict::NotSafeToGate;
        }
        if level > t.max_level_dissent {
            return DissentVerdict::RankingOnly;
        }
        DissentVerdict::SafeToGate
    }

    /// Verdict under [`DissentThresholds::default`].
    pub fn default_verdict(&self) -> DissentVerdict {
        self.verdict(DissentThresholds::default())
    }
}

/// Kendall's tau-b, the tie-corrected rank correlation, in [-1, 1].
///
/// `tau_b = (C - D) / sqrt((n0 - n1) * (n0 - n2))` where `C` and `D` count
/// concordant and discordant pairs, `n0 = n(n-1)/2`, and `n1` and `n2` are the
/// tied-pair counts within `x` and within `y` respectively. Pairs tied in both
/// series contribute to neither numerator nor either denominator term.
///
/// `None` when fewer than two values are supplied, when the lengths differ, or
/// when either series is entirely tied (the denominator vanishes).
pub fn kendall_tau_b(x: &[f64], y: &[f64]) -> Option<f64> {
    let n = x.len();
    if n < 2 || n != y.len() {
        return None;
    }
    let mut concordant = 0i64;
    let mut discordant = 0i64;
    let mut tied_x = 0i64;
    let mut tied_y = 0i64;
    for i in 0..n {
        for j in (i + 1)..n {
            let dx = x[i] - x[j];
            let dy = y[i] - y[j];
            let sx = dx.partial_cmp(&0.0);
            let sy = dy.partial_cmp(&0.0);
            let (Some(sx), Some(sy)) = (sx, sy) else {
                return None;
            };
            let x_tied = sx == std::cmp::Ordering::Equal;
            let y_tied = sy == std::cmp::Ordering::Equal;
            if x_tied {
                tied_x += 1;
            }
            if y_tied {
                tied_y += 1;
            }
            if x_tied || y_tied {
                continue;
            }
            if sx == sy {
                concordant += 1;
            } else {
                discordant += 1;
            }
        }
    }
    let n0 = (n * (n - 1) / 2) as i64;
    let dx = (n0 - tied_x) as f64;
    let dy = (n0 - tied_y) as f64;
    if dx <= 0.0 || dy <= 0.0 {
        return None;
    }
    Some((((concordant - discordant) as f64) / (dx * dy).sqrt()).clamp(-1.0, 1.0))
}

/// Split the disagreement between two arms of the same field.
///
/// `a[i]` and `b[i]` must be the score of the *same* entrant under the two arms;
/// the caller is responsible for aligning them. `None` when fewer than two
/// entrants are supplied or the lengths differ.
pub fn dissent(a: &[f64], b: &[f64]) -> Option<DissentReport> {
    let n = a.len();
    if n < 2 || n != b.len() {
        return None;
    }
    let tau_b = kendall_tau_b(a, b);
    Some(DissentReport {
        n,
        arms: 2,
        tau_b,
        rank_dissent: tau_b.map(|t| (1.0 - t) / 2.0),
        level_dissent: level_dissent(&[a, b]),
    })
}

/// Mean absolute pairwise level difference over the pooled range of all arms.
///
/// Bounded in [0, 1]: each `|a_i - b_i|` is at most the pooled range because
/// both values lie inside it. `None` when the pooled range is zero.
fn level_dissent(arms: &[&[f64]]) -> Option<f64> {
    let mut lo = f64::INFINITY;
    let mut hi = f64::NEG_INFINITY;
    for arm in arms {
        for &v in *arm {
            if !v.is_finite() {
                return None;
            }
            lo = lo.min(v);
            hi = hi.max(v);
        }
    }
    let range = hi - lo;
    if range <= 0.0 {
        return None;
    }
    let mut total = 0.0;
    let mut count = 0usize;
    for i in 0..arms.len() {
        for j in (i + 1)..arms.len() {
            for (u, v) in arms[i].iter().zip(arms[j].iter()) {
                total += (u - v).abs();
                count += 1;
            }
        }
    }
    if count == 0 {
        return None;
    }
    Some((total / count as f64 / range).clamp(0.0, 1.0))
}

/// Split the disagreement across an arbitrary set of arms.
///
/// `arms[k][i]` is entrant `i`'s score under arm `k`; every arm must cover the
/// same entrants in the same order. The rank leg is the mean tau-b over all arm
/// pairs (pairs whose tau-b is undefined are dropped, and the leg is `None` if
/// none survive); the level leg pools every pair at once.
///
/// Use this for a seed sweep, a window sweep, or a set of scorer configurations.
/// `None` when fewer than two arms are supplied, an arm has fewer than two
/// entrants, or the arms have different lengths.
pub fn dissent_across(arms: &[&[f64]]) -> Option<DissentReport> {
    if arms.len() < 2 {
        return None;
    }
    let n = arms[0].len();
    if n < 2 || arms.iter().any(|a| a.len() != n) {
        return None;
    }
    let mut taus = Vec::new();
    for i in 0..arms.len() {
        for j in (i + 1)..arms.len() {
            if let Some(t) = kendall_tau_b(arms[i], arms[j]) {
                taus.push(t);
            }
        }
    }
    let tau_b = if taus.is_empty() {
        None
    } else {
        Some(taus.iter().sum::<f64>() / taus.len() as f64)
    };
    Some(DissentReport {
        n,
        arms: arms.len(),
        tau_b,
        rank_dissent: tau_b.map(|t| (1.0 - t) / 2.0),
        level_dissent: level_dissent(arms),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tau_b_is_one_on_an_identical_ordering() {
        let x = [1.0, 2.0, 3.0, 4.0, 5.0];
        assert!((kendall_tau_b(&x, &x).unwrap() - 1.0).abs() < 1e-12);
    }

    #[test]
    fn tau_b_is_minus_one_on_an_exact_reversal() {
        let x = [1.0, 2.0, 3.0, 4.0];
        let y = [4.0, 3.0, 2.0, 1.0];
        assert!((kendall_tau_b(&x, &y).unwrap() + 1.0).abs() < 1e-12);
    }

    #[test]
    fn tau_b_matches_a_hand_counted_case() {
        // n = 4, so 6 pairs. y swaps the last two of x: 5 concordant, 1
        // discordant, no ties. tau = (5 - 1) / 6 = 0.666666...
        let x = [1.0, 2.0, 3.0, 4.0];
        let y = [1.0, 2.0, 4.0, 3.0];
        let t = kendall_tau_b(&x, &y).unwrap();
        assert!((t - 4.0 / 6.0).abs() < 1e-12, "expected 2/3, got {t}");
    }

    #[test]
    fn tau_b_tie_correction_uses_both_denominators() {
        // x has one tied pair (the two 1.0s), y has none. n = 3, n0 = 3.
        // Untied pairs: (1.0,2.0) twice, both concordant -> C = 2, D = 0.
        // tau_b = 2 / sqrt((3-1)*(3-0)) = 2 / sqrt(6).
        let x = [1.0, 1.0, 2.0];
        let y = [1.0, 2.0, 3.0];
        let t = kendall_tau_b(&x, &y).unwrap();
        assert!(
            (t - 2.0 / 6.0_f64.sqrt()).abs() < 1e-12,
            "expected 2/sqrt(6), got {t}"
        );
        assert!(t < 1.0, "the tie correction keeps a tied arm below 1");
    }

    #[test]
    fn tau_b_is_symmetric_and_bounded() {
        let x = [3.0, 1.0, 4.0, 1.0, 5.0, 9.0, 2.0, 6.0];
        let y = [2.0, 7.0, 1.0, 8.0, 2.0, 8.0, 1.0, 8.0];
        let a = kendall_tau_b(&x, &y).unwrap();
        let b = kendall_tau_b(&y, &x).unwrap();
        assert!((a - b).abs() < 1e-12, "tau-b is symmetric");
        assert!((-1.0..=1.0).contains(&a), "tau-b left its bounds: {a}");
    }

    #[test]
    fn tau_b_is_none_when_an_arm_is_entirely_tied() {
        assert_eq!(kendall_tau_b(&[1.0, 1.0, 1.0], &[1.0, 2.0, 3.0]), None);
        assert_eq!(kendall_tau_b(&[1.0], &[1.0]), None);
        assert_eq!(kendall_tau_b(&[1.0, 2.0], &[1.0]), None);
    }

    #[test]
    fn identical_arms_agree_on_both_legs() {
        let a = [0.5, 0.2, 0.9, 0.1];
        let r = dissent(&a, &a).unwrap();
        assert_eq!(r.arms, 2);
        assert_eq!(r.n, 4);
        assert!((r.rank_dissent.unwrap()).abs() < 1e-12);
        assert!((r.level_dissent.unwrap()).abs() < 1e-12);
        assert_eq!(r.default_verdict(), DissentVerdict::SafeToGate);
    }

    #[test]
    fn a_uniform_shift_is_level_dissent_not_rank_dissent() {
        // Every entrant moves by the same amount: the order is untouched.
        let a = [0.10, 0.20, 0.30, 0.40];
        let b: Vec<f64> = a.iter().map(|v| v + 0.25).collect();
        let r = dissent(&a, &b).unwrap();
        assert!(
            r.rank_dissent.unwrap().abs() < 1e-12,
            "a shift cannot move the ranking"
        );
        assert!(
            r.level_dissent.unwrap() > 0.4,
            "the levels moved a long way: {:?}",
            r.level_dissent
        );
        assert_eq!(r.default_verdict(), DissentVerdict::RankingOnly);
    }

    #[test]
    fn a_reordering_at_the_same_levels_is_rank_dissent() {
        let a = [0.10, 0.20, 0.30, 0.40];
        let b = [0.40, 0.30, 0.20, 0.10];
        let r = dissent(&a, &b).unwrap();
        assert!(
            (r.rank_dissent.unwrap() - 1.0).abs() < 1e-12,
            "an exact reversal is total rank dissent"
        );
        assert_eq!(r.default_verdict(), DissentVerdict::NotSafeToGate);
    }

    #[test]
    fn rank_dissent_outranks_level_dissent_in_the_verdict() {
        // Both legs blown: the ranking verdict is the one that survives.
        let a = [0.10, 0.20, 0.30, 0.40];
        let b = [4.0, 3.0, 2.0, 1.0];
        assert_eq!(
            dissent(&a, &b).unwrap().default_verdict(),
            DissentVerdict::NotSafeToGate
        );
    }

    #[test]
    fn both_legs_stay_inside_their_bounds_on_adversarial_input() {
        let a = [-5.0, 0.0, 12.0, 3.0, -1.0, 7.0];
        let b = [100.0, -100.0, 0.5, 0.5, 33.0, -2.0];
        let r = dissent(&a, &b).unwrap();
        let rank = r.rank_dissent.unwrap();
        let level = r.level_dissent.unwrap();
        assert!(
            (0.0..=1.0).contains(&rank),
            "rank dissent out of bounds: {rank}"
        );
        assert!(
            (0.0..=1.0).contains(&level),
            "level dissent out of bounds: {level}"
        );
    }

    #[test]
    fn a_degenerate_field_is_undetermined_not_agreeing() {
        let flat = [1.0, 1.0, 1.0];
        let r = dissent(&flat, &flat).unwrap();
        assert_eq!(r.tau_b, None);
        assert_eq!(r.level_dissent, None);
        assert_eq!(r.default_verdict(), DissentVerdict::Undetermined);
    }

    #[test]
    fn dissent_rejects_short_or_mismatched_input() {
        assert!(dissent(&[1.0], &[1.0]).is_none());
        assert!(dissent(&[1.0, 2.0], &[1.0]).is_none());
    }

    #[test]
    fn dissent_across_averages_every_arm_pair() {
        let s1 = [0.1, 0.2, 0.3, 0.4];
        let s2 = [0.11, 0.21, 0.31, 0.41];
        let s3 = [0.12, 0.22, 0.32, 0.42];
        let r = dissent_across(&[&s1, &s2, &s3]).unwrap();
        assert_eq!(r.arms, 3);
        assert_eq!(r.n, 4);
        assert!(
            r.rank_dissent.unwrap().abs() < 1e-12,
            "all three arms agree"
        );
        assert_eq!(r.default_verdict(), DissentVerdict::SafeToGate);
    }

    #[test]
    fn dissent_across_two_arms_reproduces_dissent() {
        let a = [0.4, 0.1, 0.7, 0.2];
        let b = [0.5, 0.3, 0.6, 0.1];
        assert_eq!(dissent(&a, &b).unwrap(), dissent_across(&[&a, &b]).unwrap());
    }

    #[test]
    fn dissent_across_rejects_ragged_or_lonely_input() {
        let a = [0.1, 0.2];
        let short = [0.1];
        assert!(dissent_across(&[&a]).is_none());
        assert!(dissent_across(&[&a, &short]).is_none());
    }

    #[test]
    fn custom_thresholds_move_the_verdict() {
        let a = [0.10, 0.20, 0.30, 0.40];
        let b: Vec<f64> = a.iter().map(|v| v + 0.25).collect();
        let r = dissent(&a, &b).unwrap();
        let permissive = DissentThresholds {
            max_rank_dissent: 0.05,
            max_level_dissent: 0.95,
        };
        assert_eq!(r.verdict(permissive), DissentVerdict::SafeToGate);
    }
}
