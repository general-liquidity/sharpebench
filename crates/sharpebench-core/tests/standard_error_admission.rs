//! The deflated Sharpe's standard error, certified before a track is admitted.
//!
//! Found by `sharpebench grader-search` on 2026-10-30 (seed 20261030). Two moves
//! with no skill by construction cleared every gate:
//!
//! - `autocorrelated`: an AR(1) track at phi = 0.6, one window of sixty bars,
//!   eligible at DSR 0.9999 and admitted on 7 of 64 held-out seeds against 1
//!   of 64 for an i.i.d. null. The deflated Sharpe treats sixty serially
//!   correlated bars as sixty independent ones.
//! - `window_boundary`: two windows of thirty bars, flat except for three
//!   zero-mean draws at the end of each, eligible at DSR above 0.95. A few
//!   draws make a large positive sample skewness, and the bracket credits it.
//!
//! The refusals are `sharpebench_stats::serial_correlation` (Lo 2002 with the
//! Newey-West long-run variance, Andrews 1991 bandwidth) and
//! `sharpebench_stats::moment_credit` (the Gaussian bracket of Lo 2002 as a
//! floor).

use sharpebench_core::significance::bootstrap_pvalue;
use sharpebench_core::{rank, AgentSubmission, CompositeScore, PassMode, Run, ScoreConfig};
use sharpebench_stats::{
    certify_without_moment_credit, deflated_sharpe_ratio, per_period_from_annualized,
    serially_corrected_deflated_sharpe,
};

/// Literal, so this file fails on assertions against a tree without the rule.
const SERIAL: &str = concat!(
    "returns must clear the deflation bar with the Sharpe standard error corrected for the ",
    "serial correlation measured inside its windows (Lo 2002, Newey-West): a serially correlated ",
    "track has fewer independent observations than bars, and the independent-returns deflated ",
    "Sharpe overstates its evidence"
);
const MOMENT: &str = concat!(
    "returns must clear the deflation bar with a Sharpe standard error no smaller than the ",
    "Gaussian one (Lo 2002): its sample skewness or kurtosis narrows the standard error, and a ",
    "track whose activity sits in a few bars manufactures that credit"
);

fn agent(id: &str, windows: Vec<Vec<f64>>) -> AgentSubmission {
    AgentSubmission {
        agent_id: id.into(),
        runs: windows
            .into_iter()
            .map(|returns| Run {
                returns,
                ..Run::default()
            })
            .collect(),
        in_sample_trials: 0,
        candidates: Vec::new(),
    }
}

fn score(windows: Vec<Vec<f64>>, pass_mode: PassMode) -> CompositeScore {
    let cfg = ScoreConfig {
        pass_mode,
        ..ScoreConfig::default()
    };
    rank(&[agent("candidate", windows)], &cfg).remove(0)
}

fn bits(hex: &[&str]) -> Vec<f64> {
    hex.iter()
        .map(|h| f64::from_bits(u64::from_str_radix(h, 16).expect("hex")))
        .collect()
}

/// `autocorrelated-04be18a07872cafc`: ar1_continuous, phi = 0.6, sigma = 0.001,
/// noise seed 15496652467403779926, as `f64::to_bits`.
fn ar1_admission() -> Vec<Vec<f64>> {
    vec![bits(&[
        "3f4c5a93b8daca40",
        "3f60fcbfebb3f8a8",
        "3f59ecb0257de78c",
        "3f3feb504f01c7c5",
        "bf3c62c77d047190",
        "bf57a0a94e59d2ee",
        "bf60a4879a00c66f",
        "bf4a28e78dbf2a30",
        "bf52467e9ccdc2d0",
        "bf5abc6f62f1731c",
        "3f43ed74e888d1ce",
        "bf44eea953dc4bc6",
        "bf1193f274357d1c",
        "3f48c01456dcfe46",
        "3f4cd65e7375fe94",
        "bf339077eb488a0e",
        "bf4974921c29e4bc",
        "bf22c7a75fed9286",
        "bf2f555ba5b04e98",
        "3f4da9ae4945f21d",
        "3f57b3955edf8264",
        "3f5a6c6dcfe1b3e8",
        "3f541a3cdaa6a5cd",
        "3efa47f5730eaf40",
        "3f2d278b4c7a0aca",
        "3f535834278890c9",
        "3f5542ce9171543c",
        "bf412fa3b370d302",
        "3f43aa2c1a11f598",
        "bf533652fec59eea",
        "bf4d2677df0e3c64",
        "bf42f52858dbfaf8",
        "3f59ce5bc764f0b2",
        "3f61c19c282b1012",
        "3f5bff0e44112b3b",
        "3f520ea301e34df2",
        "bf34bce538c615d4",
        "3f4e0e83fd968491",
        "3f536fb999f8f8c5",
        "bf46de3e6a99f1cb",
        "3f3b8e4bb86a5453",
        "3f421c9b9d525f84",
        "3f43123b26bc14e8",
        "3f5df672e5efa0cb",
        "3f6493d325980e38",
        "3f4ff5d2554c31d0",
        "3f4239abb65e215e",
        "3f3b0c11edbae40c",
        "3f2156368dbd7770",
        "bf4fd293fee8b80b",
        "bf3ea06aea76ff90",
        "bf2e08a73339cabd",
        "3efff6b26f1abd50",
        "bf45a50e42b433a5",
        "bf2f0a56ea547443",
        "3f350589c8bc40dc",
        "3f52b35ef008a77a",
        "3f29999fc510db60",
        "3f487fdfb870b1c1",
        "3f53c6649addce39",
    ])]
}

/// The six draws of `window_boundary-9881d5c511a548fe` (last_bars_noise,
/// size = 0.01, noise seed 1148914013831189246), three to a window.
fn sparse_draws() -> [Vec<f64>; 2] {
    [
        bits(&["3f75ac85900c1a1f", "3f0436e9149f0a3e", "3f7a156ede34f067"]),
        bits(&["bf385128de3f0667", "3f563a3de7f85e15", "3f8bdefc1540de05"]),
    ]
}

/// Thirty-bar windows, flat except for the draws at `positions`.
fn sparse(positions: [[usize; 3]; 2]) -> Vec<Vec<f64>> {
    sparse_draws()
        .iter()
        .zip(positions)
        .map(|(draws, at)| {
            let mut window = vec![0.0; 30];
            for (x, t) in draws.iter().zip(at) {
                window[t] = *x;
            }
            window
        })
        .collect()
}

/// AR(1) at `phi` with a drift `mu` a bar, over windows of `bars`, driven by
/// SplitMix64 innovations of unit variance (twelve uniforms less six, as the
/// grader search draws them), so the family carries no structure but its own.
fn ar1(phi: f64, mu: f64, windows: usize, bars: usize) -> Vec<Vec<f64>> {
    let mut state: u64 = 0x5EED_A121;
    let mut unit = move || {
        state = state.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = state;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        ((z ^ (z >> 31)) >> 11) as f64 / (1u64 << 53) as f64
    };
    let mut x = 0.0;
    let path: Vec<f64> = (0..windows * bars)
        .map(|_| {
            let e = (0..12).map(|_| unit()).sum::<f64>() - 6.0;
            x = phi * x + (1.0 - phi * phi).sqrt() * e;
            0.01 * (mu + x)
        })
        .collect();
    path.chunks(bars).map(<[f64]>::to_vec).collect()
}

/// The admission the search found is refused by name, and nothing about the
/// track moved: its independent deflated Sharpe still clears the bar. The
/// refusal does not depend on position size.
#[test]
fn the_ar1_admission_is_refused_for_serial_correlation() {
    let found = ar1_admission();
    let sr_std = per_period_from_annualized(0.5, 252.0);
    let at = |w: &[Vec<f64>]| {
        let slices: Vec<&[f64]> = w.iter().map(Vec::as_slice).collect();
        serially_corrected_deflated_sharpe(&slices, 50, 0.0, sr_std).unwrap()
    };
    let scaled: Vec<Vec<f64>> = found
        .iter()
        .map(|w| w.iter().map(|x| x * 0.01).collect())
        .collect();
    let (unscaled, rescaled) = (at(&found), at(&scaled));
    assert!(unscaled.applied_ratio > 1.0, "{unscaled:?}");
    assert_eq!(unscaled.bandwidth, rescaled.bandwidth);
    assert!((unscaled.deflated_sharpe - rescaled.deflated_sharpe).abs() < 1e-9);
    for (label, windows) in [("found", found), ("x0.01", scaled)] {
        let row = score(windows, PassMode::All);
        assert!(row.deflated_sharpe >= 0.95, "{label}: {row:?}");
        assert!(row.passed_k && row.bootstrap_p < 0.05, "{label}: {row:?}");
        assert!(!row.rank_eligible, "{label}: {row:?}");
        assert_eq!(
            row.standard_error_refusal.as_deref(),
            Some(SERIAL),
            "{label}"
        );
    }
}

/// The stationary bootstrap the gate already runs and the analytic correction
/// point the same way on serially correlated returns: keeping blocks of bars
/// together raises the bootstrap p-value over an independent resample, and the
/// corrected standard error lowers the deflated Sharpe, more for more
/// persistence. On the found track and on a deterministic AR(1) family.
#[test]
fn the_stationary_bootstrap_and_the_analytic_correction_agree_on_direction() {
    let sr_std = per_period_from_annualized(0.5, 252.0);
    let mut tracks: Vec<(String, Vec<Vec<f64>>)> = vec![("found".to_string(), ar1_admission())];
    for phi in [0.5, 0.8, 0.95] {
        tracks.push((format!("phi={phi}"), ar1(phi, 0.12, 2, 120)));
    }
    let mut last_ratio = 1.0;
    for (label, windows) in &tracks {
        let pooled = windows.concat();
        let slices: Vec<&[f64]> = windows.iter().map(Vec::as_slice).collect();
        let blocked = bootstrap_pvalue(&pooled, 7, 4000, 0.1).unwrap();
        let independent_resample = bootstrap_pvalue(&pooled, 7, 4000, 1.0).unwrap();
        assert!(
            blocked > independent_resample,
            "{label}: {blocked} vs {independent_resample}"
        );

        let independent = deflated_sharpe_ratio(&pooled, 50, sr_std).unwrap();
        let corrected = serially_corrected_deflated_sharpe(&slices, 50, 0.0, sr_std).unwrap();
        assert!(corrected.applied_ratio > 1.0, "{label}: {corrected:?}");
        assert!(
            corrected.deflated_sharpe < independent,
            "{label}: {corrected:?} vs {independent}"
        );
        if label != "found" {
            let ratio = corrected.deflated_sharpe / independent;
            assert!(
                ratio < last_ratio,
                "{label}: more persistence, less evidence"
            );
            last_ratio = ratio;
        }
    }
}

/// The search's sparse admission is refused wherever its six draws sit: at the
/// window ends as found, at the starts, scattered through the windows, and
/// across the window split. The refusal reads moments only, so it gives the
/// same answer for every placement; the bootstrap, which does see placement,
/// is never what admits or refuses it.
#[test]
fn the_sparse_admission_is_refused_under_any_placement() {
    let sr_std = per_period_from_annualized(0.5, 252.0);
    let placements = [
        ("at the window ends", [[27, 28, 29], [27, 28, 29]]),
        ("at the window starts", [[0, 1, 2], [0, 1, 2]]),
        ("scattered", [[3, 14, 22], [6, 17, 25]]),
        ("across the split", [[28, 29, 27], [0, 1, 2]]),
    ];
    for (label, positions) in placements {
        let windows = sparse(positions);
        let pooled = windows.concat();
        assert!(
            deflated_sharpe_ratio(&pooled, 50, sr_std).unwrap() >= 0.95,
            "{label}"
        );
        assert!(
            certify_without_moment_credit(&pooled, 50, 0.0, sr_std, 0.95).is_err(),
            "{label}"
        );
        let row = score(windows, PassMode::All);
        assert!(!row.rank_eligible, "{label}: {row:?}");
    }
    let found = score(sparse([[27, 28, 29], [27, 28, 29]]), PassMode::All);
    assert!(found.passed_k && found.bootstrap_p < 0.05, "{found:?}");
    assert_eq!(found.standard_error_refusal.as_deref(), Some(MOMENT));
}

/// A real edge keeps its eligibility through both certifications: a persistent
/// track with a real drift, and a positively skewed one, each strong enough that
/// the corrected standard error still clears the bar.
#[test]
fn a_real_edge_keeps_its_eligibility() {
    let skewed: Vec<Vec<f64>> = (0..2)
        .map(|w| {
            (0..120)
                .map(|t| {
                    let k = (t * 7 + w * 3) % 10;
                    if k == 0 {
                        0.03
                    } else if k < 4 {
                        -0.002
                    } else {
                        0.001 + 0.0005 * ((t + w) as f64).sin()
                    }
                })
                .collect()
        })
        .collect();
    for (label, windows) in [
        ("persistent drift", ar1(0.5, 1.0, 2, 120)),
        ("positive skew", skewed),
    ] {
        let row = score(windows, PassMode::All);
        assert_eq!(row.standard_error_refusal, None, "{label}: {row:?}");
        assert!(row.rank_eligible, "{label}: {row:?}");
    }
}
