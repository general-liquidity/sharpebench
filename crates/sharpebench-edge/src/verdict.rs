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
use crate::pbo::probability_of_backtest_overfitting;

/// The statistics version stamped into every verdict, so an archived result is
/// reproducible against the exact `sharpebench-stats` math that produced it.
pub const METHODOLOGY_VERSION: &str = "sharpebench-stats/0.0.8";

/// Default cross-trial Sharpe dispersion used when the caller doesn't supply one.
/// 0.5 is the López de Prado working assumption; the verdict flags that it was
/// estimated, never reported.
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

    let expected_max = expected_max_sharpe(trials_sr_std, cfg.n_trials);
    let deflated = deflated_sharpe_ratio(returns, cfg.n_trials, trials_sr_std);
    let psr = probabilistic_sharpe_ratio(returns, cfg.sr_benchmark);
    let mintrl = min_track_record_length(returns, cfg.sr_benchmark, cfg.confidence);

    let verdict = if deflated >= cfg.confidence {
        Verdict::Pass
    } else if deflated >= cfg.borderline {
        Verdict::Borderline
    } else {
        Verdict::Fail
    };

    let explanation = explain(
        verdict,
        deflated,
        cfg,
        n_obs,
        mintrl,
        estimated_std,
        trials_sr_std,
    );

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
    let honesty = is_my_sharpe_real(&field[winner_idx], cfg);

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

    // Transpose N×T (strategy rows) → T×N (time rows) for CSCV.
    let pbo = probability_of_backtest_overfitting(&transpose(field), default_pbo_blocks());

    // Harvey-Liu-Zhu factor gate on the winner. The t-statistic of a mean return
    // is sharpe * sqrt(n) (per-period Sharpe = mean / std).
    let winner_t = honesty.sharpe * (honesty.n_obs as f64).sqrt();
    let hlz = HarveyLiuZhu::default().evaluate(winner_t);

    FullVerdict {
        honesty,
        reality_check_p,
        spa_p,
        spa_consistent_p,
        step_down,
        pbo,
        hlz,
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
