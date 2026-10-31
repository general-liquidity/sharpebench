//! Does an automated gate agree with a human who triaged the same items?
//!
//! SharpeBench decides things: eligible or not, disqualified or not, promoted to
//! the board or held back. Deterministic gates make those decisions. This module
//! measures how well a gate's decisions match a human's decisions on the same
//! items, and raw hit rate is not that measurement: two decision rules that both
//! approve 95% of entries agree 90% of the time by arithmetic alone, having
//! learned nothing about each other.
//!
//! # What exists and what does not
//!
//! The repository ships the agreement machinery in this module and nothing
//! else. There is no human-triaged gold set in the repository, no committed
//! human verdicts, and no published agreement figure, so no kappa, rho or
//! per-class rate has been computed over real benchmark decisions. The tests
//! below run on hand-built inputs with known answers.
//!
//! A gold set that this module could score would need, per item:
//!
//! - a stable item identifier tied to the evidence the gate read, so the gate
//!   verdict can be recomputed from committed bytes;
//! - the human's pass or fail verdict, recorded before the human saw the gate's
//!   output, and optionally a graded severity for the rank leg;
//! - the gate threshold in force when the gate verdict was taken.
//!
//! It would also need enough items in each human class for the per-class
//! intervals ([`ClassRate::wilson_95`]) to be narrow enough to act on. At 20
//! human fails, a gate that catches all 20 still has a 95% lower bound near
//! 0.84 on its fail rate.
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

use crate::validation::finite_observations;

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
///
/// `None` when either series carries a non-finite value: a NaN sorts as equal to
/// everything under `partial_cmp` and an infinity has no position on the severity
/// scale, so the midranks would be an artifact of input order rather than a
/// measurement. Invalid input is not an agreement score.
pub fn spearman_rho(x: &[f64], y: &[f64]) -> Option<f64> {
    if x.len() != y.len() {
        return None;
    }
    if finite_observations(x).is_err() || finite_observations(y).is_err() {
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
    /// Cohen's kappa on the verdicts. `None` when either side returned the same
    /// single verdict for every item; `kappa_unavailable` then says which.
    ///
    /// A constant rater makes kappa uninformative rather than zero: a gate that
    /// refuses everything has no ordering of items to agree with, and the 0.0
    /// the formula returns for it would read as "measured, at chance". It is
    /// withheld instead, and the trivial-rater baselines below say what a rater
    /// with no skill would have scored.
    pub kappa: Option<f64>,
    /// Why `kappa` is absent. `None` exactly when `kappa` is present.
    pub kappa_unavailable: Option<KappaUnavailable>,
    /// Raw agreement with the human of a rater that always gives the human's
    /// majority verdict (pass on a tie). `observed_agreement` at or below this
    /// figure is no better than ignoring every item.
    pub majority_rater_agreement: f64,
    /// The best raw agreement with the human that a rater reading one supplied
    /// field alone reaches. `None` when no field was supplied (see
    /// [`gate_vs_human_with_fields`]).
    pub best_single_field: Option<FieldBaseline>,
    /// Spearman rho between the gate's continuous severity and the human's, when
    /// the human supplied a graded severity rather than a bare verdict. `None`
    /// when no human severity was supplied or either series is constant.
    pub rho: Option<f64>,
    /// Items the gate passed and the human failed.
    pub gate_lenient: usize,
    /// Items the gate failed and the human passed.
    pub gate_strict: usize,
    /// True positive rate, with "positive" meaning the human passed the item:
    /// of the items the human passed, the share the gate also passed. `None`
    /// when the human passed no item, because the rate has no denominator.
    pub tpr: Option<ClassRate>,
    /// True negative rate: of the items the human failed, the share the gate
    /// also failed. `1 - tnr` is the rate at which the gate lets through what
    /// the human would have stopped. `None` when the human failed no item.
    pub tnr: Option<ClassRate>,
}

/// The share of one human class that the gate placed in the same class, with
/// a 95% Wilson score interval.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ClassRate {
    /// Items in the class on which the gate agreed with the human.
    pub hits: usize,
    /// Items the human placed in the class. Always at least 1.
    pub total: usize,
    /// `hits / total`.
    pub rate: f64,
    /// 95% Wilson score interval for `rate`.
    pub wilson_95: ProportionInterval,
}

/// A two-sided interval for a proportion, inside `[0, 1]`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ProportionInterval {
    pub lower: f64,
    pub upper: f64,
}

/// The 0.975 quantile of the standard normal, for a two-sided 95% interval.
/// The same constant `sharpebench-study` uses for its 95% Wilson interval.
const Z_95: f64 = 1.959_963_984_540_054;

/// 95% Wilson score interval for `hits` successes in `total` trials.
///
/// Wilson rather than Wald because the classes here are often small and the
/// rates often near 0 or 1, where the Wald interval collapses to zero width:
/// 20 hits out of 20 would read as certainty. Wilson keeps a lower bound below
/// 1 there (about 0.839). Bounds are clamped into `[0, 1]`.
///
/// `None` when `total` is 0 or `hits > total`.
pub fn wilson_interval_95(hits: usize, total: usize) -> Option<ProportionInterval> {
    if total == 0 || hits > total {
        return None;
    }
    let n = total as f64;
    let x = hits as f64;
    let z2 = Z_95 * Z_95;
    let denominator = n + z2;
    let center = (x + z2 / 2.0) / denominator;
    let spread = Z_95 / denominator * (x * (n - x) / n + z2 / 4.0).sqrt();
    Some(ProportionInterval {
        lower: (center - spread).max(0.0),
        upper: (center + spread).min(1.0),
    })
}

fn class_rate(hits: usize, total: usize) -> Option<ClassRate> {
    let wilson_95 = wilson_interval_95(hits, total)?;
    Some(ClassRate {
        hits,
        total,
        rate: hits as f64 / total as f64,
        wilson_95,
    })
}

/// Why kappa was withheld: one rater's marginal is degenerate.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KappaUnavailable {
    /// The gate returned one verdict for every item.
    ConstantGate,
    /// The human returned one verdict for every item.
    ConstantHuman,
    /// Both returned one verdict for every item.
    BothConstant,
}

/// The trivial rater that reads one supplied field and nothing else.
#[derive(Clone, Debug, PartialEq)]
pub struct FieldBaseline {
    /// The field's name, as supplied.
    pub field: String,
    /// True when the field agrees best with the human read the other way round
    /// (field `true` taken as "fails").
    pub inverted: bool,
    /// Raw agreement of that rater with the human.
    pub agreement: f64,
}

impl GateAgreement {
    /// Whether the gate reproduces the triage well enough to stand in for it.
    ///
    /// `min_kappa` is a caller-supplied operating requirement, not a discovered
    /// constant. An unavailable kappa (either side returned the same single
    /// verdict throughout) is not evidence of agreement and returns `false`.
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
/// `None` when the series are empty, their lengths disagree, or any supplied
/// severity or the threshold is non-finite. A gate severity that is not a real
/// number has no side of the operating point, so neither the verdict split nor
/// the rank leg is defined.
pub fn gate_vs_human(
    gate_severity: &[f64],
    gate_threshold: f64,
    human_verdict: &[bool],
    human_severity: Option<&[f64]>,
) -> Option<GateAgreement> {
    gate_vs_human_with_fields(
        gate_severity,
        gate_threshold,
        human_verdict,
        human_severity,
        &[],
    )
}

/// [`gate_vs_human`], plus the single-field baseline over `fields`: each is a
/// named yes/no feature of the same items, and the baseline is the field (in
/// either polarity) whose verdict agrees most with the human. Ties go to the
/// earlier field and to the uninverted reading, so the result does not depend
/// on anything but the input order. A gate that does no better than one field
/// it could have read directly has not shown it learned the triage.
///
/// `None` under the same conditions as [`gate_vs_human`], and when any field's
/// length differs from the item count.
pub fn gate_vs_human_with_fields(
    gate_severity: &[f64],
    gate_threshold: f64,
    human_verdict: &[bool],
    human_severity: Option<&[f64]>,
    fields: &[(&str, &[bool])],
) -> Option<GateAgreement> {
    let n = gate_severity.len();
    if n == 0 || n != human_verdict.len() {
        return None;
    }
    if fields.iter().any(|(_, values)| values.len() != n) {
        return None;
    }
    if !gate_threshold.is_finite() || finite_observations(gate_severity).is_err() {
        return None;
    }
    if let Some(hs) = human_severity {
        if hs.len() != n || finite_observations(hs).is_err() {
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

    let human_passes = human_verdict.iter().filter(|v| **v).count();
    let human_fails = n - human_passes;

    let constant = |v: &[bool]| v.iter().all(|x| *x) || v.iter().all(|x| !*x);
    let kappa_unavailable = match (constant(&gate_verdict), constant(human_verdict)) {
        (true, true) => Some(KappaUnavailable::BothConstant),
        (true, false) => Some(KappaUnavailable::ConstantGate),
        (false, true) => Some(KappaUnavailable::ConstantHuman),
        (false, false) => None,
    };
    let kappa = match kappa_unavailable {
        Some(_) => None,
        None => cohens_kappa_binary(&gate_verdict, human_verdict),
    };
    let majority_rater_agreement = ph.max(1.0 - ph);
    let agreement_of = |field: &[bool]| {
        field
            .iter()
            .zip(human_verdict)
            .filter(|(f, h)| f == h)
            .count() as f64
            / nf
    };
    // inv: `best` is the highest agreement over the fields and polarities
    // visited so far, the earliest on a tie. Ends after 2 * fields.len() steps.
    let mut best: Option<FieldBaseline> = None;
    for (name, values) in fields {
        let plain = agreement_of(values);
        for (inverted, agreement) in [(false, plain), (true, 1.0 - plain)] {
            if best.as_ref().is_none_or(|b| agreement > b.agreement) {
                best = Some(FieldBaseline {
                    field: (*name).to_string(),
                    inverted,
                    agreement,
                });
            }
        }
    }

    Some(GateAgreement {
        n,
        observed_agreement,
        chance_agreement,
        kappa,
        kappa_unavailable,
        majority_rater_agreement,
        best_single_field: best,
        rho: human_severity.and_then(|hs| spearman_rho(gate_severity, hs)),
        gate_lenient,
        gate_strict,
        tpr: class_rate(human_passes - gate_strict, human_passes),
        tnr: class_rate(human_fails - gate_lenient, human_fails),
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

    /// A gate that refuses everything used to report kappa 0.0, which reads as
    /// a measured chance-level agreement. Its kappa is now withheld, with the
    /// side named, and the trivial baselines are reported beside it.
    #[test]
    fn a_constant_rater_withholds_kappa_and_reports_the_baselines() {
        let refuse_all = [0.0; 10];
        let human = [
            true, true, true, true, true, true, true, false, false, false,
        ];
        let r = gate_vs_human(&refuse_all, 0.5, &human, None).unwrap();
        assert_eq!(r.kappa, None);
        assert_eq!(r.kappa_unavailable, Some(KappaUnavailable::ConstantGate));
        assert!((r.majority_rater_agreement - 0.7).abs() < 1e-12);
        assert!(!r.reproduces_triage(-1.0));

        let varied = [0.9, 0.1, 0.9, 0.1, 0.9, 0.1, 0.9, 0.1, 0.9, 0.1];
        let r = gate_vs_human(&varied, 0.5, &[false; 10], None).unwrap();
        assert_eq!(r.kappa, None);
        assert_eq!(r.kappa_unavailable, Some(KappaUnavailable::ConstantHuman));
        assert_eq!(r.majority_rater_agreement, 1.0);

        let r = gate_vs_human(&refuse_all, 0.5, &[false; 10], None).unwrap();
        assert_eq!(r.kappa_unavailable, Some(KappaUnavailable::BothConstant));
    }

    #[test]
    fn a_varying_pair_keeps_kappa_and_names_no_cause() {
        let gate = [0.9, 0.8, 0.2, 0.1];
        let r = gate_vs_human(&gate, 0.5, &[true, true, false, false], None).unwrap();
        assert_eq!(r.kappa, Some(1.0));
        assert_eq!(r.kappa_unavailable, None);
        assert_eq!(r.best_single_field, None, "no field supplied");
    }

    /// The single-field rater is reported in its better polarity, so a gate
    /// whose agreement equals one field's is visibly no better than that field.
    #[test]
    fn the_single_field_baseline_takes_the_best_field_and_polarity() {
        let gate = [0.9, 0.9, 0.1, 0.1, 0.9, 0.1];
        let human = [true, true, false, false, true, true];
        let noise: &[bool] = &[true, false, true, false, true, false];
        let flipped: &[bool] = &[false, false, true, true, false, true];
        let r = gate_vs_human_with_fields(
            &gate,
            0.5,
            &human,
            None,
            &[("noise", noise), ("flipped", flipped)],
        )
        .unwrap();
        let best = r.best_single_field.unwrap();
        assert_eq!(best.field, "flipped");
        assert!(best.inverted);
        assert!((best.agreement - 5.0 / 6.0).abs() < 1e-12);
        assert!((r.observed_agreement - best.agreement).abs() < 1e-12);

        assert!(
            gate_vs_human_with_fields(&gate, 0.5, &human, None, &[("short", &[true])]).is_none(),
            "a field of the wrong length is refused"
        );
    }

    fn close(actual: f64, expected: f64, what: &str) {
        assert!(
            (actual - expected).abs() < 1e-4,
            "{what}: expected {expected}, got {actual}"
        );
    }

    /// The 2x2 table of `kappa_matches_a_hand_computed_table`, read per class.
    /// The human passed 30 items and the gate passed 20 of them; the human
    /// failed 20 and the gate failed 15 of them. 15 of 20 at 95% is the
    /// published Wilson case (0.5313, 0.8881) that `sharpebench-study` also pins.
    #[test]
    fn per_class_rates_match_a_hand_computed_table() {
        let mut gate = Vec::new();
        let mut human = Vec::new();
        for (g, h, count) in [
            (0.9, true, 20),
            (0.9, false, 5),
            (0.1, true, 10),
            (0.1, false, 15),
        ] {
            for _ in 0..count {
                gate.push(g);
                human.push(h);
            }
        }
        let r = gate_vs_human(&gate, 0.5, &human, None).unwrap();

        let tpr = r.tpr.unwrap();
        assert_eq!((tpr.hits, tpr.total), (20, 30));
        assert!((tpr.rate - 20.0 / 30.0).abs() < 1e-12);
        close(tpr.wilson_95.lower, 0.4878, "tpr lower");
        close(tpr.wilson_95.upper, 0.8077, "tpr upper");

        let tnr = r.tnr.unwrap();
        assert_eq!((tnr.hits, tnr.total), (15, 20));
        assert!((tnr.rate - 0.75).abs() < 1e-12);
        close(tnr.wilson_95.lower, 0.5313, "tnr lower");
        close(tnr.wilson_95.upper, 0.8881, "tnr upper");
    }

    /// A gate that passes everything agrees with a human who passes 18 of 20 on
    /// 90% of items. The per-class split shows where that 90% comes from: it
    /// passes every human pass and stops none of the two human fails.
    #[test]
    fn per_class_rates_expose_a_gate_that_stops_nothing() {
        let mut human = [true; 20];
        human[0] = false;
        human[1] = false;
        let r = gate_vs_human(&[1.0; 20], 0.5, &human, None).unwrap();
        assert!((r.observed_agreement - 0.9).abs() < 1e-12);
        let tpr = r.tpr.unwrap();
        assert_eq!((tpr.hits, tpr.total, tpr.rate), (18, 18, 1.0));
        close(
            tpr.wilson_95.lower,
            0.8241,
            "a perfect rate on 18 items is not certain",
        );
        let tnr = r.tnr.unwrap();
        assert_eq!((tnr.hits, tnr.total, tnr.rate), (0, 2, 0.0));
        close(
            tnr.wilson_95.upper,
            0.6576,
            "two items leave the fail rate wide open",
        );
    }

    /// A rate over an empty human class has no denominator. It is withheld,
    /// never reported as 0 or 1.
    #[test]
    fn a_rate_over_an_empty_human_class_is_withheld() {
        let r = gate_vs_human(&[0.9, 0.1, 0.9], 0.5, &[true; 3], None).unwrap();
        assert_eq!(r.tnr, None, "the human failed nothing");
        assert_eq!(r.tpr.unwrap().total, 3);
        let r = gate_vs_human(&[0.9, 0.1, 0.9], 0.5, &[false; 3], None).unwrap();
        assert_eq!(r.tpr, None, "the human passed nothing");
        assert_eq!(r.tnr.unwrap().hits, 1);
    }

    #[test]
    fn wilson_interval_keeps_width_at_the_boundaries_and_refuses_impossible_counts() {
        let none = wilson_interval_95(0, 20).unwrap();
        assert!(none.lower.abs() < 1e-12, "lower bound at zero hits is 0");
        close(
            none.upper,
            0.1611,
            "zero hits still leave a positive upper bound",
        );
        let all = wilson_interval_95(20, 20).unwrap();
        assert_eq!(all.upper, 1.0);
        close(all.lower, 0.8389, "twenty of twenty is not certainty");
        assert_eq!(wilson_interval_95(0, 0), None);
        assert_eq!(wilson_interval_95(3, 2), None);
    }

    #[test]
    fn gate_agreement_rejects_mismatched_or_empty_inputs() {
        assert!(gate_vs_human(&[], 0.5, &[], None).is_none());
        assert!(gate_vs_human(&[0.1], 0.5, &[true, false], None).is_none());
        assert!(gate_vs_human(&[0.1, 0.2], 0.5, &[true, false], Some(&[1.0])).is_none());
    }
}
