//! The two-tier honesty verdict: "is my Sharpe real?"
//!
//! [`is_my_sharpe_real`] (LITE) answers from a single return series — observed
//! Sharpe, PSR, expected-max-Sharpe under the search, deflated Sharpe, and the
//! minimum track record length — and renders a [`Verdict`].
//!
//! [`is_my_sharpe_real_full`] (FULL) adds the multiple-testing family over the
//! whole field of candidate strategies: White's Reality Check, Hansen's SPA (and
//! its consistent variant), Romano-Wolf step-down, the CSCV Probability of
//! Backtest Overfitting, and the Harvey-Liu-Zhu `|t| >= 3.0` factor gate on the
//! winner.

use serde::{Deserialize, Serialize};

use crate::hlz::{HarveyLiuZhu, HlzGate};
use sharpebench_stats::significance::{
    reality_check_pvalue, spa_consistent_pvalue, spa_pvalue, step_down_significant,
};
use sharpebench_stats::stats::{kurtosis, skewness};
use sharpebench_stats::{
    deflated_sharpe_ratio, expected_max_sharpe, probabilistic_sharpe_ratio, sharpe_ratio,
};

use crate::mintrl::min_track_record_length;
use crate::pbo::pbo_status;

/// The statistics version stamped into every verdict, so an archived result is
/// reproducible against the exact `sharpebench-stats` math that produced it.
/// Taken from the crate version at compile time: `sharpebench-stats` and this
/// crate share the workspace version, so the stamp cannot go stale on a release.
pub const METHODOLOGY_VERSION: &str = concat!("sharpebench-stats/", env!("CARGO_PKG_VERSION"));

/// Default cross-trial Sharpe dispersion used when the caller doesn't supply one.
/// 0.5 is a **modelling prior, not a measurement** — the working value López de
/// Prado uses in worked examples. A LITE verdict sees one return series and has
/// no field to measure dispersion on, so the prior is all it can use; the
/// explanation flags that it was estimated. When a field exists, measure it
/// (`sharpebench_core::rank` does) and pass the value in `trials_sr_std`.
const DEFAULT_TRIALS_SR_STD: f64 = 0.5;

/// Fixed bootstrap settings for the FULL data-snooping family. Held constant so a
/// FULL verdict is deterministic and reproducible across runs.
const SNOOP_SEED: u64 = 0x5BA7_ED60_2026_0008;
const SNOOP_N_BOOT: usize = 2_000;
const SNOOP_BLOCK_PROB: f64 = 0.1;
const SNOOP_ALPHA: f64 = 0.05;

/// The headline call.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Verdict {
    /// Deflated Sharpe clears the Pass threshold — survives the search.
    Pass,
    /// Between the Borderline and Pass thresholds — promising, underpowered.
    Borderline,
    /// Below Borderline — indistinguishable from luck once the search is priced in.
    Fail,
}

/// Knobs for the honesty verdict. `n_trials` is the one the caller must think
/// about: it is the multiple-testing footprint (how many strategies/configs were
/// tried before this one was chosen).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct HonestyConfig {
    /// Number of strategy trials behind this result. REQUIRED to think honestly:
    /// `n_trials = 1` is almost always a lie — a single backtest you kept is the
    /// survivor of every variant you discarded.
    pub n_trials: u32,
    /// Cross-trial Sharpe dispersion. `None` ⇒ estimate at 0.5 and flag it in the
    /// explanation.
    pub trials_sr_std: Option<f64>,
    /// Deflated-Sharpe threshold for a `Pass`. Default 0.95.
    pub confidence: f64,
    /// Deflated-Sharpe threshold for `Borderline`. Default 0.90.
    pub borderline: f64,
    /// PSR / MinTRL benchmark Sharpe to beat. Default 0.0.
    pub sr_benchmark: f64,
}

impl Default for HonestyConfig {
    fn default() -> Self {
        Self {
            n_trials: 1,
            trials_sr_std: None,
            confidence: 0.95,
            borderline: 0.90,
            sr_benchmark: 0.0,
        }
    }
}

/// The LITE verdict: everything derivable from one return series.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct HonestyVerdict {
    pub sharpe: f64,
    pub n_obs: usize,
    pub skew: f64,
    pub kurtosis: f64,
    pub n_trials: u32,
    pub expected_max_sharpe: f64,
    pub deflated_sharpe: f64,
    pub probabilistic_sharpe: f64,
    /// `1 - deflated_sharpe`: the probability the edge is a search artifact.
    pub haircut: f64,
    /// `sharpe * deflated_sharpe`: the Sharpe discounted by survival probability.
    pub haircut_sharpe: f64,
    pub min_track_record_len: f64,
    pub verdict: Verdict,
    pub explanation: String,
    /// The `sharpebench-stats` version that produced these numbers.
    pub methodology_version: String,
    /// Why the deflation family could not be estimated. Omitted for valid
    /// inputs. When present, `expected_max_sharpe` and `deflated_sharpe` are
    /// the no-skill floor (0.0) rather than estimates, `haircut` is 1.0 and the
    /// verdict is `Fail`: a track whose honesty cannot be checked has not
    /// passed the check. The reason is repeated in `explanation`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub statistics_error: Option<String>,
}

/// The FULL verdict: LITE on the winner plus the multiple-testing family.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FullVerdict {
    pub honesty: HonestyVerdict,
    /// White's Reality Check p-value over the field.
    pub reality_check_p: f64,
    /// Hansen's SPA p-value (liberal/lower studentized variant).
    pub spa_p: f64,
    /// Hansen's consistent SPA p-value.
    pub spa_consistent_p: f64,
    /// Romano-Wolf step-down: which field members are significant at α.
    pub step_down: Vec<bool>,
    /// CSCV Probability of Backtest Overfitting over the field.
    pub pbo: f64,
    /// Harvey-Liu-Zhu (2016) factor gate on the winner's t-statistic
    /// (`sharpe * sqrt(n_obs)`): a hard `|t| >= 3.0` floor complementing the
    /// deflated-Sharpe verdict.
    pub hlz: HlzGate,
    /// Why the data-snooping family could not be estimated. Omitted for valid
    /// inputs. The four tests share one bootstrap boundary and fail together,
    /// so when this is present `reality_check_p`, `spa_p` and
    /// `spa_consistent_p` are 1.0 and `step_down` is all-false: the reading
    /// that concedes nothing, not an estimate. A NaN field used to publish the
    /// smallest p the resampler can produce, because no draw exceeds a NaN
    /// observed statistic and the +1 smoothing then does the rest.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub snooping_error: Option<String>,
    /// Why the overfitting probability is unavailable, when it is. `pbo` is NaN
    /// in that case rather than a fabricated probability.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pbo_error: Option<String>,
}

/// LITE: "is my Sharpe real?" from a single per-period return series.
///
/// ```
/// use sharpebench_edge::{is_my_sharpe_real, HonestyConfig, Verdict};
///
/// // A steady, low-vol edge over a long track.
/// let returns: Vec<f64> = (0..250)
///     .map(|i| 0.001 + 0.0001 * ((i % 5) as f64 - 2.0))
///     .collect();
/// let cfg = HonestyConfig { n_trials: 20, ..Default::default() };
/// let v = is_my_sharpe_real(&returns, &cfg);
/// assert!(v.sharpe > 0.0);
/// assert!((0.0..=1.0).contains(&v.deflated_sharpe));
/// assert!(v.haircut >= 0.0);
/// // The verdict is one of the three tiers.
/// assert!(matches!(v.verdict, Verdict::Pass | Verdict::Borderline | Verdict::Fail));
/// ```
pub fn is_my_sharpe_real(returns: &[f64], cfg: &HonestyConfig) -> HonestyVerdict {
    let sharpe = sharpe_ratio(returns);
    let skew = skewness(returns);
    let kurt = kurtosis(returns);
    let n_obs = returns.len();

    let estimated_std = cfg.trials_sr_std.is_none();
    let trials_sr_std = cfg.trials_sr_std.unwrap_or(DEFAULT_TRIALS_SR_STD);

    // The bar and the ratio share one boundary, so they share one refusal. A
    // negative `trials_sr_std` used to zero the bar and hand back a deflated
    // Sharpe near 1.0: the most favorable verdict available, from the input
    // that had least earned it. There is no substitute number for a deflation
    // that was never computed, so the verdict fails closed and says why.
    let deflation = expected_max_sharpe(trials_sr_std, cfg.n_trials).and_then(|bar| {
        Ok((
            bar,
            deflated_sharpe_ratio(returns, cfg.n_trials, trials_sr_std)?,
        ))
    });
    let statistics_error = deflation.as_ref().err().map(ToString::to_string);
    let (expected_max, deflated) = deflation.unwrap_or((0.0, 0.0));
    let psr = probabilistic_sharpe_ratio(returns, cfg.sr_benchmark);
    let mintrl = min_track_record_length(returns, cfg.sr_benchmark, cfg.confidence);

    let verdict = if statistics_error.is_some() {
        Verdict::Fail
    } else if deflated >= cfg.confidence {
        Verdict::Pass
    } else if deflated >= cfg.borderline {
        Verdict::Borderline
    } else {
        Verdict::Fail
    };

    let explanation = match &statistics_error {
        Some(error) => format!(
            "FAIL: the deflated Sharpe could not be computed ({error}), so this track has not been checked against its search. No verdict is available for these inputs."
        ),
        None => explain(
            verdict,
            deflated,
            cfg,
            n_obs,
            mintrl,
            estimated_std,
            trials_sr_std,
        ),
    };

    HonestyVerdict {
        sharpe,
        n_obs,
        skew,
        kurtosis: kurt,
        n_trials: cfg.n_trials,
        expected_max_sharpe: expected_max,
        deflated_sharpe: deflated,
        probabilistic_sharpe: psr,
        haircut: 1.0 - deflated,
        haircut_sharpe: sharpe * deflated,
        min_track_record_len: mintrl,
        verdict,
        explanation,
        methodology_version: METHODOLOGY_VERSION.to_string(),
        statistics_error,
    }
}

/// FULL: the LITE verdict on `field[winner_idx]` plus the data-snooping family
/// and PBO over the whole field.
///
/// `field` is **N rows (strategies) × T cols (time)** — each row is one
/// candidate's per-period returns, the orientation the `significance` family
/// expects. PBO needs the transpose (T×N), which this function builds internally.
pub fn is_my_sharpe_real_full(
    field: &[Vec<f64>],
    winner_idx: usize,
    cfg: &HonestyConfig,
) -> FullVerdict {
    // The search this function can see is at least the field it was handed. The
    // caller picks the winner out of `field`, so scoring that winner at the
    // caller's declared `n_trials` prices a selection that demonstrably happened
    // over more candidates than that: the Python entry point defaults to 1 and
    // selects the highest Sharpe itself, so the headline priced one trial for a
    // search over the whole field. `rank` in the core applies the same floor for
    // the same reason, and the sidecar field tests below never fed back into this
    // verdict. Declared private search still adds on top.
    let observed = HonestyConfig {
        n_trials: cfg
            .n_trials
            .max(u32::try_from(field.len()).unwrap_or(u32::MAX)),
        ..*cfg
    };
    let honesty = is_my_sharpe_real(&field[winner_idx], &observed);

    let reality_check_p = reality_check_pvalue(field, SNOOP_SEED, SNOOP_N_BOOT, SNOOP_BLOCK_PROB);
    let spa_p = spa_pvalue(field, SNOOP_SEED, SNOOP_N_BOOT, SNOOP_BLOCK_PROB);
    let spa_consistent_p = spa_consistent_pvalue(field, SNOOP_SEED, SNOOP_N_BOOT, SNOOP_BLOCK_PROB);
    let step_down = step_down_significant(
        field,
        SNOOP_SEED,
        SNOOP_N_BOOT,
        SNOOP_BLOCK_PROB,
        SNOOP_ALPHA,
    );

    // Transpose N×T (strategy rows) → T×N (time rows) for CSCV. Through the status
    // API, so an unestimable matrix reports why instead of a number: a single
    // non-finite cell used to yield 1.0, a confident systematic-overfitting
    // verdict, because every Sharpe over it is NaN and NaN loses every comparison.
    let pbo_status = pbo_status(&transpose(field), default_pbo_blocks());
    let pbo_error = pbo_status.as_ref().err().map(|e| e.to_string());
    let pbo = pbo_status.unwrap_or(f64::NAN);

    // Harvey-Liu-Zhu factor gate on the winner. The t-statistic of a mean return
    // is sharpe * sqrt(n) (per-period Sharpe = mean / std).
    let winner_t = honesty.sharpe * (honesty.n_obs as f64).sqrt();
    let hlz = HarveyLiuZhu::default().evaluate(winner_t);

    let snooping_error = reality_check_p
        .as_ref()
        .err()
        .or(spa_p.as_ref().err())
        .or(spa_consistent_p.as_ref().err())
        .or(step_down.as_ref().err())
        .map(ToString::to_string);

    FullVerdict {
        honesty,
        reality_check_p: reality_check_p.unwrap_or(1.0),
        spa_p: spa_p.unwrap_or(1.0),
        spa_consistent_p: spa_consistent_p.unwrap_or(1.0),
        step_down: step_down.unwrap_or_else(|_| vec![false; field.len()]),
        pbo,
        hlz,
        snooping_error,
        pbo_error,
    }
}

/// Default CSCV block count.
fn default_pbo_blocks() -> usize {
    10
}

/// N×T → T×N. Rows must be equal length; a ragged or empty field yields an empty
/// matrix (PBO then returns 0 for the degenerate input).
fn transpose(field: &[Vec<f64>]) -> Vec<Vec<f64>> {
    let n = field.len();
    if n == 0 {
        return Vec::new();
    }
    let t = field[0].len();
    if field.iter().any(|row| row.len() != t) {
        return Vec::new();
    }
    let mut out = vec![Vec::with_capacity(n); t];
    for row in field {
        for (col, &v) in row.iter().enumerate() {
            out[col].push(v);
        }
    }
    out
}

/// One honest plain-English sentence for the verdict.
fn explain(
    verdict: Verdict,
    deflated: f64,
    cfg: &HonestyConfig,
    n_obs: usize,
    mintrl: f64,
    estimated_std: bool,
    trials_sr_std: f64,
) -> String {
    let head = match verdict {
        Verdict::Pass => format!(
            "PASS: deflated Sharpe {deflated:.3} clears {:.2} after pricing in {} trial(s) — the edge survives the search.",
            cfg.confidence, cfg.n_trials
        ),
        Verdict::Borderline => format!(
            "BORDERLINE: deflated Sharpe {deflated:.3} is between {:.2} and {:.2} over {} trial(s) — promising but underpowered.",
            cfg.borderline, cfg.confidence, cfg.n_trials
        ),
        Verdict::Fail => format!(
            "FAIL: deflated Sharpe {deflated:.3} is below {:.2} over {} trial(s) — indistinguishable from luck once the search is priced in.",
            cfg.borderline, cfg.n_trials
        ),
    };

    let mut notes = String::new();
    if estimated_std {
        notes.push_str(&format!(
            " trials_sr_std was not supplied and was estimated at {trials_sr_std:.2}."
        ));
    }
    if mintrl.is_finite() && (n_obs as f64) < mintrl {
        notes.push_str(&format!(
            " Track is too short: {n_obs} obs < MinTRL {mintrl:.0} required at {:.2} confidence.",
            cfg.confidence
        ));
    } else if !mintrl.is_finite() {
        notes
            .push_str(" Observed Sharpe does not beat the benchmark, so no track length suffices.");
    }

    format!("{head}{notes}")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// PSR/DSR sanity: both in [0,1], and DSR ≤ PSR vs the same benchmark.
    #[test]
    fn psr_dsr_bounds_and_ordering() {
        let r: Vec<f64> = (0..150)
            .map(|i| 0.002 + 0.01 * (i as f64 * 0.3).sin())
            .collect();
        let cfg = HonestyConfig {
            n_trials: 50,
            ..Default::default()
        };
        let v = is_my_sharpe_real(&r, &cfg);
        assert!((0.0..=1.0).contains(&v.probabilistic_sharpe));
        assert!((0.0..=1.0).contains(&v.deflated_sharpe));
        assert!(v.deflated_sharpe <= v.probabilistic_sharpe + 1e-12);
        assert_eq!(v.haircut, 1.0 - v.deflated_sharpe);
        assert_eq!(v.methodology_version, METHODOLOGY_VERSION);
    }

    /// A long, clean, single-trial edge passes.
    #[test]
    fn clearly_good_series_passes() {
        let r: Vec<f64> = (0..400)
            .map(|i| 0.001 + 0.00005 * ((i % 4) as f64 - 1.5))
            .collect();
        let cfg = HonestyConfig {
            n_trials: 1,
            ..Default::default()
        };
        let v = is_my_sharpe_real(&r, &cfg);
        assert_eq!(v.verdict, Verdict::Pass);
    }

    /// A short, noisy series mined over many trials fails.
    #[test]
    fn clearly_overfit_series_fails() {
        let r: Vec<f64> = (0..30).map(|i| 0.001 * ((i % 7) as f64 - 3.0)).collect();
        let cfg = HonestyConfig {
            n_trials: 1000,
            ..Default::default()
        };
        let v = is_my_sharpe_real(&r, &cfg);
        assert_eq!(v.verdict, Verdict::Fail);
    }

    /// Estimated dispersion is flagged in the explanation.
    #[test]
    fn estimated_std_flagged() {
        let r: Vec<f64> = (0..100).map(|i| 0.001 + 0.002 * (i as f64).cos()).collect();
        let cfg = HonestyConfig {
            n_trials: 10,
            trials_sr_std: None,
            ..Default::default()
        };
        let v = is_my_sharpe_real(&r, &cfg);
        assert!(v.explanation.contains("estimated"));
    }

    /// FULL runs the snooping family + PBO without panicking, and the winner's
    /// lite verdict matches the standalone LITE call.
    #[test]
    fn full_matches_lite_on_winner() {
        // Field of N=5 strategies × T=80 periods; strategy 2 is the strongest.
        let field: Vec<Vec<f64>> = (0..5)
            .map(|j| {
                (0..80)
                    .map(|i| {
                        let edge = if j == 2 { 0.004 } else { 0.0005 };
                        edge + 0.003 * (((i + j) % 6) as f64 - 2.5)
                    })
                    .collect()
            })
            .collect();
        let cfg = HonestyConfig {
            n_trials: 5,
            ..Default::default()
        };
        let full = is_my_sharpe_real_full(&field, 2, &cfg);
        let lite = is_my_sharpe_real(&field[2], &cfg);
        assert_eq!(full.honesty, lite);
        assert!((0.0..=1.0).contains(&full.reality_check_p));
        assert!((0.0..=1.0).contains(&full.spa_p));
        assert!((0.0..=1.0).contains(&full.spa_consistent_p));
        assert!((0.0..=1.0).contains(&full.pbo));
        assert_eq!(full.step_down.len(), field.len());
        // The HLZ gate reports the winner's t-statistic against the 3.0 bar.
        assert_eq!(full.hlz.t_threshold, 3.0);
        assert_eq!(
            full.hlz.t_stat,
            full.honesty.sharpe * (full.honesty.n_obs as f64).sqrt()
        );
    }

    /// R02: a dispersion that is not a dispersion fails closed.
    ///
    /// `expected_max_sharpe(-1.0, n)` used to return 0.0, so the deflation bar
    /// collapsed to the benchmark and this clean 400-period track came back as
    /// a `Pass` with a deflated Sharpe of essentially 1.0: the single most
    /// favorable verdict the product can issue, from an input no estimator
    /// accepts.
    #[test]
    fn an_invalid_dispersion_fails_closed_instead_of_passing() {
        let r: Vec<f64> = (0..400)
            .map(|i| 0.001 + 0.00005 * ((i % 4) as f64 - 1.5))
            .collect();
        for bad in [-1.0, f64::NAN, f64::INFINITY] {
            let cfg = HonestyConfig {
                n_trials: 500,
                trials_sr_std: Some(bad),
                ..Default::default()
            };
            let v = is_my_sharpe_real(&r, &cfg);
            assert_eq!(v.verdict, Verdict::Fail, "trials_sr_std {bad}");
            assert!(v.statistics_error.is_some());
            assert_eq!(v.deflated_sharpe, 0.0);
            assert_eq!(v.expected_max_sharpe, 0.0);
            assert_eq!(v.haircut, 1.0);
            assert_eq!(v.haircut_sharpe, 0.0);
            assert!(v.explanation.contains("could not be computed"));
        }
    }

    /// R02: a non-finite track cannot be verdicted at all.
    #[test]
    fn a_non_finite_track_fails_closed() {
        let mut r = vec![0.001; 40];
        r[7] = f64::NAN;
        let v = is_my_sharpe_real(&r, &HonestyConfig::default());
        assert_eq!(v.verdict, Verdict::Fail);
        assert_eq!(
            v.statistics_error.as_deref(),
            Some("observation 7 must be finite")
        );
    }

    /// R02: the FULL family reports its own unavailability instead of the
    /// smallest attainable p-value.
    ///
    /// With a NaN field the observed statistic is NaN, no bootstrap draw
    /// exceeds it, and `(0 + 1) / (2000 + 1)` used to publish p = 0.0005 for
    /// all three snooping tests while every step-down hypothesis was rejected.
    #[test]
    fn a_non_finite_field_reports_no_snooping_p_values() {
        let field = vec![vec![f64::NAN; 40], vec![0.001; 40]];
        let full = is_my_sharpe_real_full(&field, 1, &HonestyConfig::default());
        assert!(full.snooping_error.is_some());
        assert_eq!(full.reality_check_p, 1.0);
        assert_eq!(full.spa_p, 1.0);
        assert_eq!(full.spa_consistent_p, 1.0);
        assert_eq!(full.step_down, vec![false; field.len()]);
    }

    /// R02 guard: a valid verdict is byte-for-byte what it always was.
    ///
    /// The numbers are recomputed from the same public primitives the verdict
    /// is defined in terms of, so a change to the boundary that moved any of
    /// them would fail here.
    #[test]
    fn valid_verdict_inputs_return_the_same_numbers() {
        let r: Vec<f64> = (0..400)
            .map(|i| 0.001 + 0.00005 * ((i % 4) as f64 - 1.5))
            .collect();
        let cfg = HonestyConfig {
            n_trials: 500,
            trials_sr_std: Some(0.5),
            ..Default::default()
        };
        let v = is_my_sharpe_real(&r, &cfg);
        assert!(v.statistics_error.is_none());
        assert_eq!(v.sharpe, sharpe_ratio(&r));
        assert_eq!(
            v.expected_max_sharpe,
            expected_max_sharpe(0.5, 500).unwrap()
        );
        assert_eq!(
            v.deflated_sharpe,
            deflated_sharpe_ratio(&r, 500, 0.5).unwrap()
        );
        assert_eq!(v.haircut, 1.0 - v.deflated_sharpe);
        assert_eq!(v.haircut_sharpe, v.sharpe * v.deflated_sharpe);
        assert_eq!(v.verdict, Verdict::Pass);
        assert!(v.explanation.starts_with("PASS: "));

        // And the estimated-dispersion path (`trials_sr_std: None`) still uses
        // the 0.5 prior and still flags it.
        let estimated = is_my_sharpe_real(
            &r,
            &HonestyConfig {
                n_trials: 500,
                ..Default::default()
            },
        );
        assert_eq!(estimated.deflated_sharpe, v.deflated_sharpe);
        assert!(estimated.explanation.contains("estimated"));
    }

    /// R02 guard: the FULL family on a valid field still publishes the same
    /// four snooping results.
    #[test]
    fn valid_full_inputs_return_the_same_numbers() {
        let field: Vec<Vec<f64>> = (0..5)
            .map(|j| {
                (0..80)
                    .map(|i| {
                        let edge = if j == 2 { 0.004 } else { 0.0005 };
                        edge + 0.003 * (((i + j) % 6) as f64 - 2.5)
                    })
                    .collect()
            })
            .collect();
        let cfg = HonestyConfig {
            n_trials: 5,
            ..Default::default()
        };
        let full = is_my_sharpe_real_full(&field, 2, &cfg);
        assert!(full.snooping_error.is_none());
        assert_eq!(
            full.reality_check_p,
            reality_check_pvalue(&field, SNOOP_SEED, SNOOP_N_BOOT, SNOOP_BLOCK_PROB).unwrap()
        );
        assert_eq!(
            full.spa_p,
            spa_pvalue(&field, SNOOP_SEED, SNOOP_N_BOOT, SNOOP_BLOCK_PROB).unwrap()
        );
        assert_eq!(
            full.spa_consistent_p,
            spa_consistent_pvalue(&field, SNOOP_SEED, SNOOP_N_BOOT, SNOOP_BLOCK_PROB).unwrap()
        );
        assert_eq!(
            full.step_down,
            step_down_significant(
                &field,
                SNOOP_SEED,
                SNOOP_N_BOOT,
                SNOOP_BLOCK_PROB,
                SNOOP_ALPHA
            )
            .unwrap()
        );
    }

    #[test]
    fn transpose_roundtrip_shape() {
        let field = vec![vec![1.0, 2.0, 3.0], vec![4.0, 5.0, 6.0]];
        let txn = transpose(&field);
        assert_eq!(txn.len(), 3);
        assert_eq!(txn[0], vec![1.0, 4.0]);
        // Ragged → empty.
        assert!(transpose(&[vec![1.0, 2.0], vec![3.0]]).is_empty());
    }
}

#[cfg(test)]
mod bm1_search_footprint {
    use super::*;

    fn field(n: usize) -> Vec<Vec<f64>> {
        (0..n)
            .map(|i| {
                (0..64)
                    .map(|t| ((i * 64 + t) as f64 * 0.37).sin() * 0.01 + 0.001)
                    .collect()
            })
            .collect()
    }

    #[test]
    fn the_headline_prices_the_field_it_selected_over() {
        // The caller picks the winner out of `field`, so a declared n_trials of 1
        // prices a selection that demonstrably ranged over every candidate.
        let f = field(40);
        let cfg = HonestyConfig {
            n_trials: 1,
            ..Default::default()
        };
        let full = is_my_sharpe_real_full(&f, 0, &cfg);
        let declared_only = is_my_sharpe_real(&f[0], &cfg);
        assert!(
            full.honesty.expected_max_sharpe > declared_only.expected_max_sharpe,
            "the observed field must raise the deflation bar above the declared one"
        );
    }

    #[test]
    fn a_declared_search_larger_than_the_field_is_not_lowered() {
        // The floor only ever raises: declared private search still dominates.
        let f = field(4);
        let cfg = HonestyConfig {
            n_trials: 500,
            ..Default::default()
        };
        let full = is_my_sharpe_real_full(&f, 0, &cfg);
        let declared_only = is_my_sharpe_real(&f[0], &cfg);
        assert_eq!(
            full.honesty.expected_max_sharpe.to_bits(),
            declared_only.expected_max_sharpe.to_bits()
        );
    }

    #[test]
    fn an_unestimable_matrix_reports_no_overfitting_probability() {
        let mut f = field(8);
        f[3][7] = f64::NAN;
        let full = is_my_sharpe_real_full(&f, 0, &HonestyConfig::default());
        assert!(
            full.pbo.is_nan(),
            "a non-finite cell must not produce a confident probability, got {}",
            full.pbo
        );
        assert!(full.pbo_error.is_some(), "the reason must be reported");
    }
}
