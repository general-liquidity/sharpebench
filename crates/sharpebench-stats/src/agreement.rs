//! Does the automated gate agree with the human who triaged the gold set?
//!
//! SharpeBench decides things: eligible or not, disqualified or not, promoted to
//! the board or held back. Those decisions are made by deterministic gates. The
//! gold set was triaged by a person. Nothing in the kernel could previously say
//! whether the two agree beyond raw hit-rate, and raw hit-rate is not an answer:
//! two decision rules that both approve 95% of entries agree 90% of the time by
//! arithmetic alone, having learned nothing about each other.
//!
//! This module answers it two ways, both pure and deterministic:
//!
//! - **Chance-corrected agreement on the decision.** Cohen's kappa
//!   ([`cohens_kappa`]) subtracts the agreement two raters would reach by
//!   guessing at their own observed marginal rates, so a gate that approves
//!   everything scores near zero however high its hit-rate.
//! - **Rank agreement on the underlying severity.** Spearman's rho
//!   ([`spearman_rho`]) compares the *ordering* of the continuous scores behind
//!   the two decisions, which survives a disagreement that is only about where
//!   the threshold sits.
//!
//! There is no model here, and nothing to prompt. These are the textbook
//! estimators:
//!
//! - Cohen, J. (1960). "A Coefficient of Agreement for Nominal Scales."
//!   *Educational and Psychological Measurement* 20(1), 37-46.
//! - Spearman, C. (1904). "The Proof and Measurement of Association between Two
//!   Things." *American Journal of Psychology* 15(1), 72-101. Ties are handled
//!   by the midrank convention, i.e. Pearson correlation of the tied ranks
//!   (Kendall, M. G., *Rank Correlation Methods*, 1948).

/// Turn continuous severities into decisions at a threshold.
///
/// `values[i] >= threshold` becomes `true`. This is the binarizer that makes a
/// continuous gate output comparable with a yes/no human triage; the threshold
/// is always caller-supplied, never inferred from the data, so the operating
/// point is an input to the measurement rather than something it fits.
pub fn binarize(values: &[f64], threshold: f64) -> Vec<bool> {
    values.iter().map(|&v| v >= threshold).collect()
}

/// Midranks of `xs`: 1-based ranks with tied values sharing their mean rank.
///
/// Deterministic: ties are resolved by value equality, never by input position.
fn midranks(xs: &[f64]) -> Vec<f64> {
    let n = xs.len();
    let mut idx: Vec<usize> = (0..n).collect();
    idx.sort_by(|&a, &b| {
        xs[a]
            .partial_cmp(&xs[b])
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    let mut ranks = vec![0.0; n];
    let mut i = 0;
    while i < n {
        let mut j = i + 1;
        while j < n && xs[idx[j]] == xs[idx[i]] {
            j += 1;
        }
        // Positions i..j (0-based) share the mean of their 1-based ranks.
        let mean_rank = ((i + 1) + j) as f64 / 2.0;
        for &k in &idx[i..j] {
            ranks[k] = mean_rank;
        }
        i = j;
    }
    ranks
}

/// Pearson correlation. `None` when either series has zero variance.
fn pearson(x: &[f64], y: &[f64]) -> Option<f64> {
    let n = x.len();
    if n < 2 || n != y.len() {
        return None;
    }
    let nf = n as f64;
    let mx = x.iter().sum::<f64>() / nf;
    let my = y.iter().sum::<f64>() / nf;
    let mut sxy = 0.0;
    let mut sxx = 0.0;
    let mut syy = 0.0;
    for i in 0..n {
        let dx = x[i] - mx;
        let dy = y[i] - my;
        sxy += dx * dy;
        sxx += dx * dx;
        syy += dy * dy;
    }
    if sxx <= 0.0 || syy <= 0.0 {
        return None;
    }
    Some((sxy / (sxx.sqrt() * syy.sqrt())).clamp(-1.0, 1.0))
}

/// Spearman's rank correlation between two severity series, in [-1, 1].
///
/// Computed as the Pearson correlation of the midranks, which is the tie-correct
/// form. The `1 - 6*sum(d^2)/(n(n^2-1))` shortcut is only valid without ties and
/// is deliberately not used. `None` when fewer than two pairs are supplied, when
/// the lengths differ, or when either series is constant (a constant series has
/// no ordering to correlate).
pub fn spearman_rho(x: &[f64], y: &[f64]) -> Option<f64> {
    if x.len() != y.len() {
        return None;
    }
    pearson(&midranks(x), &midranks(y))
}

/// Cohen's kappa over integer category labels, in [-1, 1].
///
/// `kappa = (p_o - p_e) / (1 - p_e)` where `p_o` is the observed agreement rate
/// and `p_e` is the agreement expected from the two raters' own marginal
/// frequencies. 1.0 is perfect agreement, 0.0 is exactly chance, negative is
/// worse than chance.
///
/// `None` when the inputs are empty or of unequal length, or when `p_e == 1`
/// (both raters used the *same* single category throughout, so chance agreement
/// is certain and the correction is undefined). One constant rater facing a
/// varying one is well defined and scores exactly 0: a rater that always says
/// the same thing carries no information about the other, however often the two
/// happen to coincide.
pub fn cohens_kappa(a: &[usize], b: &[usize]) -> Option<f64> {
    let n = a.len();
    if n == 0 || n != b.len() {
        return None;
    }
    let nf = n as f64;
    let categories: std::collections::BTreeSet<usize> = a.iter().chain(b.iter()).copied().collect();

    let observed = a.iter().zip(b.iter()).filter(|(x, y)| x == y).count() as f64 / nf;

    let mut expected = 0.0;
    for c in &categories {
        let pa = a.iter().filter(|v| *v == c).count() as f64 / nf;
        let pb = b.iter().filter(|v| *v == c).count() as f64 / nf;
        expected += pa * pb;
    }

    if (1.0 - expected).abs() < f64::EPSILON {
        return None;
    }
    Some(((observed - expected) / (1.0 - expected)).clamp(-1.0, 1.0))
}

/// Cohen's kappa over yes/no decisions. `false` is category 0, `true` is 1.
pub fn cohens_kappa_binary(a: &[bool], b: &[bool]) -> Option<f64> {
    let ai: Vec<usize> = a.iter().map(|&v| usize::from(v)).collect();
    let bi: Vec<usize> = b.iter().map(|&v| usize::from(v)).collect();
    cohens_kappa(&ai, &bi)
}

/// How well the automated gate reproduces a human triage.
#[derive(Clone, Debug, PartialEq)]
pub struct GateAgreement {
    /// Items compared.
    pub n: usize,
    /// Fraction of items where the gate and the human reached the same verdict.
    /// Reported because operators ask for it, never interpreted on its own.
    pub observed_agreement: f64,
    /// Agreement the two would reach by guessing at their own observed rates.
    /// The gap between this and `observed_agreement` is the whole signal.
    pub chance_agreement: f64,
    /// Cohen's kappa on the verdicts. `None` only when both sides returned the
    /// same single verdict for every item.
    pub kappa: Option<f64>,
    /// Spearman rho between the gate's continuous severity and the human's, when
    /// the human supplied a graded severity rather than a bare verdict. `None`
    /// when no human severity was supplied or either series is constant.
    pub rho: Option<f64>,
    /// Items the gate passed and the human failed.
    pub gate_lenient: usize,
    /// Items the gate failed and the human passed.
    pub gate_strict: usize,
}

impl GateAgreement {
    /// Whether the gate reproduces the triage well enough to stand in for it.
    ///
    /// `min_kappa` is a caller-supplied operating requirement, not a discovered
    /// constant. An undefined kappa (both sides returned the same single verdict
    /// throughout) is not evidence of agreement and returns `false`.
    pub fn reproduces_triage(&self, min_kappa: f64) -> bool {
        self.kappa.is_some_and(|k| k >= min_kappa)
    }
}

/// Compare an automated gate against a human triage of the same items.
///
/// `gate_severity` is the gate's continuous output and `gate_threshold` the
/// operating point at which it decides. `human_verdict[i]` is `true` when the
/// human passed item `i`. `human_severity`, when supplied, is the human's graded
/// severity for the same items and drives the rank leg.
///
/// Verdict convention: `true` means "passes the gate". `gate_lenient` therefore
/// counts the disagreements that let something through, which is the direction
/// that costs money.
///
/// `None` when the series are empty or their lengths disagree.
pub fn gate_vs_human(
    gate_severity: &[f64],
    gate_threshold: f64,
    human_verdict: &[bool],
    human_severity: Option<&[f64]>,
) -> Option<GateAgreement> {
    let n = gate_severity.len();
    if n == 0 || n != human_verdict.len() {
        return None;
    }
    if let Some(hs) = human_severity {
        if hs.len() != n {
            return None;
        }
    }

    let gate_verdict = binarize(gate_severity, gate_threshold);
    let nf = n as f64;
    let observed_agreement = gate_verdict
        .iter()
        .zip(human_verdict.iter())
        .filter(|(g, h)| g == h)
        .count() as f64
        / nf;

    let pg = gate_verdict.iter().filter(|v| **v).count() as f64 / nf;
    let ph = human_verdict.iter().filter(|v| **v).count() as f64 / nf;
    let chance_agreement = pg * ph + (1.0 - pg) * (1.0 - ph);

    let gate_lenient = gate_verdict
        .iter()
        .zip(human_verdict.iter())
        .filter(|(g, h)| **g && !**h)
        .count();
    let gate_strict = gate_verdict
        .iter()
        .zip(human_verdict.iter())
        .filter(|(g, h)| !**g && **h)
        .count();

    Some(GateAgreement {
        n,
        observed_agreement,
        chance_agreement,
        kappa: cohens_kappa_binary(&gate_verdict, human_verdict),
        rho: human_severity.and_then(|hs| spearman_rho(gate_severity, hs)),
        gate_lenient,
        gate_strict,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn binarizer_uses_the_supplied_threshold_inclusively() {
        assert_eq!(
            binarize(&[0.1, 0.5, 0.9], 0.5),
            vec![false, true, true],
            "a value exactly at the threshold passes"
        );
    }

    #[test]
    fn kappa_is_one_on_perfect_agreement() {
        let a = [true, false, true, true, false, false];
        assert_eq!(cohens_kappa_binary(&a, &a), Some(1.0));
    }

    #[test]
    fn kappa_is_zero_at_exactly_chance() {
        // Marginals 50/50 on both sides with a 2x2 table of equal cells: observed
        // agreement 0.5, chance agreement 0.5, so kappa is exactly 0.
        let a = [true, true, false, false];
        let b = [true, false, true, false];
        let k = cohens_kappa_binary(&a, &b).unwrap();
        assert!(k.abs() < 1e-12, "expected 0, got {k}");
    }

    #[test]
    fn kappa_is_near_zero_when_a_lenient_gate_agrees_by_arithmetic() {
        // A gate that passes everything and a human who passes 18 of 20 agree on
        // 90% of items and have learned nothing. Raw agreement says 0.9.
        let gate = [true; 20];
        let mut human = [true; 20];
        human[0] = false;
        human[1] = false;
        let raw = gate
            .iter()
            .zip(human.iter())
            .filter(|(g, h)| g == h)
            .count() as f64
            / 20.0;
        assert!((raw - 0.9).abs() < 1e-12);
        assert_eq!(
            cohens_kappa_binary(&gate, &human),
            Some(0.0),
            "0.9 raw agreement, exactly zero chance-corrected agreement"
        );

        // Give the gate one disagreeing call so kappa is defined; it stays tiny.
        let mut gate2 = [true; 20];
        gate2[19] = false;
        let k = cohens_kappa_binary(&gate2, &human).unwrap();
        assert!(k.abs() < 0.2, "expected near-chance, got {k}");
    }

    #[test]
    fn kappa_is_negative_when_the_gate_inverts_the_human() {
        let a = [true, true, false, false, true, false];
        let b: Vec<bool> = a.iter().map(|v| !v).collect();
        let k = cohens_kappa_binary(&a, &b).unwrap();
        assert!(k < 0.0, "expected worse than chance, got {k}");
    }

    #[test]
    fn kappa_matches_a_hand_computed_table() {
        // 2x2 table: 20 both-yes, 5 gate-yes/human-no, 10 gate-no/human-yes,
        // 15 both-no. n = 50. p_o = 35/50 = 0.70.
        // gate yes = 25/50 = 0.5, human yes = 30/50 = 0.6.
        // p_e = 0.5*0.6 + 0.5*0.4 = 0.50. kappa = (0.70-0.50)/0.50 = 0.40.
        let mut gate = Vec::new();
        let mut human = Vec::new();
        for _ in 0..20 {
            gate.push(true);
            human.push(true);
        }
        for _ in 0..5 {
            gate.push(true);
            human.push(false);
        }
        for _ in 0..10 {
            gate.push(false);
            human.push(true);
        }
        for _ in 0..15 {
            gate.push(false);
            human.push(false);
        }
        let k = cohens_kappa_binary(&gate, &human).unwrap();
        assert!((k - 0.40).abs() < 1e-12, "expected 0.40, got {k}");
    }

    #[test]
    fn kappa_handles_more_than_two_categories() {
        let a = [0usize, 1, 2, 0, 1, 2];
        assert_eq!(cohens_kappa(&a, &a), Some(1.0));
        assert_eq!(cohens_kappa(&a, &[0, 1, 2]), None, "length mismatch");
        assert_eq!(cohens_kappa(&[], &[]), None);
    }

    #[test]
    fn rho_is_one_on_a_monotone_relabeling() {
        let x = [1.0, 2.0, 3.0, 4.0, 5.0];
        let y = [10.0, 20.0, 33.0, 41.0, 500.0];
        let r = spearman_rho(&x, &y).unwrap();
        assert!((r - 1.0).abs() < 1e-12, "expected 1, got {r}");
    }

    #[test]
    fn rho_is_minus_one_on_a_reversal() {
        let x = [1.0, 2.0, 3.0, 4.0];
        let y = [4.0, 3.0, 2.0, 1.0];
        let r = spearman_rho(&x, &y).unwrap();
        assert!((r + 1.0).abs() < 1e-12, "expected -1, got {r}");
    }

    #[test]
    fn rho_matches_a_hand_computed_untied_case() {
        // Classic worked example: d = (0, 0, 1, -1, 0), sum d^2 = 2, n = 5.
        // rho = 1 - 6*2/(5*24) = 1 - 12/120 = 0.90.
        let x = [1.0, 2.0, 3.0, 4.0, 5.0];
        let y = [1.0, 2.0, 4.0, 3.0, 5.0];
        let r = spearman_rho(&x, &y).unwrap();
        assert!((r - 0.90).abs() < 1e-12, "expected 0.90, got {r}");
    }

    #[test]
    fn rho_handles_ties_without_leaving_its_bounds() {
        let x = [1.0, 1.0, 1.0, 2.0, 2.0, 3.0, 3.0, 3.0, 3.0];
        let y = [5.0, 5.0, 1.0, 2.0, 2.0, 9.0, 9.0, 4.0, 4.0];
        let r = spearman_rho(&x, &y).unwrap();
        assert!((-1.0..=1.0).contains(&r), "rho left its bounds: {r}");

        // A heavily tied series still reads as perfect against itself.
        let tied = [1.0, 1.0, 2.0, 2.0, 3.0, 3.0];
        assert!((spearman_rho(&tied, &tied).unwrap() - 1.0).abs() < 1e-12);
    }

    #[test]
    fn rho_is_symmetric_and_scale_invariant() {
        let x = [3.0, 1.0, 4.0, 1.0, 5.0, 9.0, 2.0, 6.0];
        let y = [2.0, 7.0, 1.0, 8.0, 2.0, 8.0, 1.0, 8.0];
        let a = spearman_rho(&x, &y).unwrap();
        let b = spearman_rho(&y, &x).unwrap();
        assert!((a - b).abs() < 1e-12);

        let scaled: Vec<f64> = x.iter().map(|v| v * 100.0 + 7.0).collect();
        let c = spearman_rho(&scaled, &y).unwrap();
        assert!((a - c).abs() < 1e-12, "rho depends only on order");
    }

    #[test]
    fn rho_is_none_on_a_constant_series_or_a_length_mismatch() {
        assert_eq!(spearman_rho(&[1.0, 1.0, 1.0], &[1.0, 2.0, 3.0]), None);
        assert_eq!(spearman_rho(&[1.0, 2.0], &[1.0]), None);
    }

    #[test]
    fn gate_agreement_reports_both_directions_of_error() {
        // Severities 0.9 and 0.8 pass at 0.5; 0.2 and 0.1 fail.
        let gate = [0.9, 0.8, 0.2, 0.1];
        let human = [true, false, true, false];
        let r = gate_vs_human(&gate, 0.5, &human, None).unwrap();
        assert_eq!(r.n, 4);
        assert_eq!(
            r.gate_lenient, 1,
            "item 1 passed the gate, failed the human"
        );
        assert_eq!(r.gate_strict, 1, "item 2 failed the gate, passed the human");
        assert!((r.observed_agreement - 0.5).abs() < 1e-12);
        assert!((r.chance_agreement - 0.5).abs() < 1e-12);
        assert!(r.kappa.unwrap().abs() < 1e-12);
        assert!(!r.reproduces_triage(0.6));
    }

    #[test]
    fn gate_agreement_carries_the_rank_leg_when_human_severity_is_supplied() {
        let gate = [0.9, 0.8, 0.7, 0.2];
        let human_sev = [4.0, 3.0, 2.0, 1.0];
        let human = [true, true, true, false];
        let r = gate_vs_human(&gate, 0.5, &human, Some(&human_sev)).unwrap();
        assert_eq!(r.kappa, Some(1.0));
        assert!((r.rho.unwrap() - 1.0).abs() < 1e-12);
        assert!(r.reproduces_triage(0.8));
    }

    #[test]
    fn gate_agreement_rejects_mismatched_or_empty_inputs() {
        assert!(gate_vs_human(&[], 0.5, &[], None).is_none());
        assert!(gate_vs_human(&[0.1], 0.5, &[true, false], None).is_none());
        assert!(gate_vs_human(&[0.1, 0.2], 0.5, &[true, false], Some(&[1.0])).is_none());
    }
}
