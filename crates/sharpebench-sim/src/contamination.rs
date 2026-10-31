//! Shape-level contamination probe: score an agent on a real window and on a
//! synthetic twin of the same window, and report the paired difference.
//!
//! Masking ([`Dataset::masked_view`]) hides names, dates and price level, but
//! not the shape of the path. A model that has memorized a historical path can
//! still recognise its shape and trade on what it remembers happened next. The
//! twin keeps the window's texture (its own bar relatives, resampled in blocks
//! by the stationary bootstrap in `sharpebench-stats`) and its opportunity set
//! (total return, volatility and regime must match within declared tolerances),
//! while breaking the order of the bars. An agent that reads only texture
//! should score about the same on both; one that recalls the real path should
//! score higher on the real window. The edge is real score minus twin score,
//! and the window is the inferential unit.
//!
//! Known limit: block resampling copies stretches of the real path into the
//! twin, so a memorizer that locates itself bar by bar can still score on the
//! copied stretches. That biases the edge toward zero (less power), never
//! toward a false conviction. A shorter mean block (a larger `block_prob`)
//! copies less and preserves less serial texture.
//!
//! Building and validating this probe needs no model call. Running it on a
//! model field is a separate study.

use std::collections::BTreeMap;

use sharpebench_stats::paired_randomization::{paired_swap_test, PairedSwapConfig};
use sharpebench_stats::stationary_bootstrap_path;
use sharpebench_stats::stats::{mean, std_dev};
use sharpebench_stats::StatisticalError;

use crate::data::{Dataset, MaskError};
use crate::engine::Window;
use crate::windows::{tag_regime, Regime};

/// The declared matching rule for one twin.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TwinSpec {
    /// Bars before the scored window that the twin also rebuilds, so the
    /// observation history the agent first sees is synthetic too.
    pub warmup: usize,
    /// Stationary-bootstrap block restart probability (mean block length is
    /// `1 / block_prob`).
    pub block_prob: f64,
    /// Largest accepted absolute gap in scored-segment total return.
    pub return_tolerance: f64,
    /// Largest accepted relative gap in scored-segment per-bar volatility.
    pub volatility_tolerance: f64,
    /// Draws before the window is refused as unmatchable.
    pub max_draws: usize,
}

impl Default for TwinSpec {
    fn default() -> Self {
        TwinSpec {
            warmup: 20,
            block_prob: 0.1,
            return_tolerance: 0.03,
            volatility_tolerance: 0.15,
            max_draws: 20_000,
        }
    }
}

/// The matched statistics of a scored segment.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SegmentStats {
    /// Equal-weight mean over symbols of `last / first - 1` on the scored bars.
    pub total_return: f64,
    /// Equal-weight mean over symbols of the per-bar simple-return standard
    /// deviation on the scored bars.
    pub volatility: f64,
    pub regime: Regime,
}

/// A real window and its accepted twin, both masked, on the same geometry.
#[derive(Clone, Debug)]
pub struct TwinPair {
    /// The real span `[start - warmup, end)`, masked.
    pub real: Dataset,
    /// The twin of that span, masked the same way.
    pub twin: Dataset,
    /// The scored window inside both datasets.
    pub window: Window,
    /// Draws taken before acceptance (1 means the first draw matched).
    pub draws: usize,
    pub real_stats: SegmentStats,
    pub twin_stats: SegmentStats,
}

/// Why a twin could not be built.
#[derive(Clone, Debug, PartialEq)]
pub enum TwinError {
    /// The window, with its warmup, does not fit inside the dataset, or the
    /// scored segment has fewer than two bars.
    WindowOutOfRange,
    /// Bar relatives need strictly positive closes.
    NonPositiveClose {
        symbol: String,
        step: usize,
    },
    /// The real window carries cash dividends, which the twin cannot resample
    /// consistently with its prices.
    DividendsUnsupported,
    /// A flat real segment has no volatility to match.
    ZeroVolatility,
    /// No draw matched within `max_draws`.
    Unmatched {
        draws: usize,
    },
    Mask(MaskError),
    Resampling(StatisticalError),
}

impl std::fmt::Display for TwinError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}

impl std::error::Error for TwinError {}

fn slice(data: &Dataset, from: usize, to: usize) -> Dataset {
    Dataset {
        dates: data.dates[from..to].to_vec(),
        closes: data
            .closes
            .iter()
            .map(|(s, v)| (s.clone(), v[from..to].to_vec()))
            .collect(),
        dividends: BTreeMap::new(),
        marks: data.marks.clone(),
    }
}

fn segment_stats(data: &Dataset, window: Window) -> Option<SegmentStats> {
    let mut total = 0.0;
    let mut vol = 0.0;
    let n = data.closes.len() as f64;
    for series in data.closes.values() {
        let seg = &series[window.start..window.end];
        total += seg[seg.len() - 1] / seg[0] - 1.0;
        let rets: Vec<f64> = seg.windows(2).map(|w| w[1] / w[0] - 1.0).collect();
        vol += std_dev(&rets);
    }
    let (total, vol) = (total / n, vol / n);
    (total.is_finite() && vol.is_finite()).then(|| SegmentStats {
        total_return: total,
        volatility: vol,
        regime: tag_regime(data, window),
    })
}

/// One draw's seed: a SplitMix64 finalizer over the window seed and the draw
/// index, so consecutive draws are not overlapping streams of one generator.
fn draw_seed(seed: u64, draw: u64) -> u64 {
    let mut z = seed ^ draw.wrapping_mul(0xD1B5_4A32_D192_ED03);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

/// Build an accepted twin of `window` in `source` by rejection sampling.
///
/// The span `[window.start - warmup, window.end)` is cut out. Its bar relatives
/// `close[i] / close[i - 1]` form the pool, one row per bar, all symbols kept
/// together so the cross-section survives. Each draw takes one stationary
/// bootstrap index path over that pool and rebuilds every symbol from the
/// span's first close. A draw is accepted when the scored segment's total
/// return is within `return_tolerance`, its volatility within
/// `volatility_tolerance` relative, and its regime label equal to the real
/// segment's. Both datasets are then masked, so names, dates and level carry
/// no difference between them.
pub fn build_twin(
    source: &Dataset,
    window: Window,
    spec: &TwinSpec,
    seed: u64,
) -> Result<TwinPair, TwinError> {
    let from = window
        .start
        .checked_sub(spec.warmup)
        .ok_or(TwinError::WindowOutOfRange)?;
    if window.end > source.len() || window.end < window.start + 2 {
        return Err(TwinError::WindowOutOfRange);
    }
    if source
        .dividends
        .values()
        .any(|v| v[from..window.end].iter().any(|d| *d != 0.0))
    {
        return Err(TwinError::DividendsUnsupported);
    }
    let real = slice(source, from, window.end);
    let scored = Window {
        start: spec.warmup,
        end: real.len(),
    };
    for (symbol, series) in &real.closes {
        if let Some(step) = series
            .iter()
            .position(|x| x.partial_cmp(&0.0) != Some(std::cmp::Ordering::Greater))
        {
            return Err(TwinError::NonPositiveClose {
                symbol: symbol.clone(),
                step: from + step,
            });
        }
    }
    let real_stats = segment_stats(&real, scored).ok_or(TwinError::ZeroVolatility)?;
    if real_stats.volatility == 0.0 {
        return Err(TwinError::ZeroVolatility);
    }
    let span = real.len();
    let relatives: BTreeMap<&String, Vec<f64>> = real
        .closes
        .iter()
        .map(|(s, v)| (s, v.windows(2).map(|w| w[1] / w[0]).collect()))
        .collect();
    // inv: every draw before `draw` was rejected; `draw` < max_draws.
    // term: `draw` increases by one each pass and is bounded by `max_draws`.
    for draw in 0..spec.max_draws {
        let path = stationary_bootstrap_path(
            draw_seed(seed, draw as u64),
            span - 1,
            span - 1,
            spec.block_prob,
        )
        .map_err(TwinError::Resampling)?;
        let mut twin = real.clone();
        for (symbol, series) in twin.closes.iter_mut() {
            let rel = &relatives[symbol];
            // inv: series[0..=i] is the rebuilt path up to bar i.
            for (i, &k) in path.iter().enumerate() {
                series[i + 1] = series[i] * rel[k];
            }
        }
        let Some(twin_stats) = segment_stats(&twin, scored) else {
            continue;
        };
        let matched = (twin_stats.total_return - real_stats.total_return).abs()
            <= spec.return_tolerance
            && (twin_stats.volatility / real_stats.volatility - 1.0).abs()
                <= spec.volatility_tolerance
            && twin_stats.regime == real_stats.regime;
        if matched {
            return Ok(TwinPair {
                real: real.masked().map_err(TwinError::Mask)?,
                twin: twin.masked().map_err(TwinError::Mask)?,
                window: scored,
                draws: draw + 1,
                real_stats,
                twin_stats,
            });
        }
    }
    Err(TwinError::Unmatched {
        draws: spec.max_draws,
    })
}

/// Per-bar Sharpe ratio (mean over sample standard deviation, not
/// annualized). `None` when the returns are empty, non-finite, or constant:
/// a ratio with no dispersion carries no sign, and it is not coerced to zero.
pub fn window_sharpe(returns: &[f64]) -> Option<f64> {
    if returns.len() < 2 || returns.iter().any(|r| !r.is_finite()) {
        return None;
    }
    let sd = std_dev(returns);
    (sd > 0.0 && sd.is_finite()).then(|| mean(returns) / sd)
}

/// One window's scores. `None` is a score that could not be computed.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PairedScore {
    pub real: Option<f64>,
    pub twin: Option<f64>,
}

/// The declared decision rule of the probe, fixed before scores are seen.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ProbeConfig {
    pub alpha: f64,
    /// The smallest mean edge that counts as memorization. A significant edge
    /// below it is reported, not convicted.
    pub min_edge: f64,
    pub resamples: usize,
    pub seed: u64,
}

impl Default for ProbeConfig {
    fn default() -> Self {
        ProbeConfig {
            alpha: 0.05,
            min_edge: 0.05,
            resamples: 100_000,
            seed: 0x5348_4150_4550_524f,
        }
    }
}

/// Paired real-minus-twin result over a set of windows.
#[derive(Clone, Debug, PartialEq)]
pub struct ShapeProbeReport {
    /// `real - twin`, one per window, in input order.
    pub edges: Vec<f64>,
    pub mean_edge: f64,
    /// One-sided whole-window sign-flip p-value for a positive mean edge.
    pub pvalue: f64,
    /// `pvalue <= alpha` and `mean_edge >= min_edge`.
    pub memorization: bool,
}

/// Why the probe has no verdict.
#[derive(Clone, Debug, PartialEq)]
pub enum ProbeError {
    /// A window's real or twin score is unavailable. The window is not
    /// dropped, because a shrinking denominator would hide it.
    ScoreUnavailable {
        window: usize,
    },
    Statistics(StatisticalError),
}

/// Test whether real scores exceed twin scores, with the window as the unit.
///
/// Under the null of no recall, a window's real and twin arms are
/// exchangeable, so each edge's sign is a fair coin. The test flips whole
/// windows, which is exact enumeration for up to 16 windows at the default
/// resample count.
pub fn shape_probe(
    scores: &[PairedScore],
    config: ProbeConfig,
) -> Result<ShapeProbeReport, ProbeError> {
    let mut edges = Vec::with_capacity(scores.len());
    for (window, s) in scores.iter().enumerate() {
        match (s.real, s.twin) {
            (Some(r), Some(t)) => edges.push(r - t),
            _ => return Err(ProbeError::ScoreUnavailable { window }),
        }
    }
    let orientations: Vec<[f64; 2]> = edges.iter().map(|&e| [e, -e]).collect();
    let test = paired_swap_test(
        &orientations,
        PairedSwapConfig {
            resamples: config.resamples,
            seed: config.seed,
        },
    )
    .map_err(ProbeError::Statistics)?;
    Ok(ShapeProbeReport {
        mean_edge: test.observed_mean,
        pvalue: test.pvalue,
        memorization: test.pvalue <= config.alpha && test.observed_mean >= config.min_edge,
        edges,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn twin_matches_declared_moments_and_keeps_the_geometry() {
        let source = Dataset::synthetic(3, 200, 11);
        let window = Window {
            start: 60,
            end: 100,
        };
        let spec = TwinSpec::default();
        let pair = build_twin(&source, window, &spec, 7).unwrap();
        assert_eq!(pair.real.len(), 60);
        assert_eq!(pair.twin.len(), 60);
        assert_eq!((pair.window.start, pair.window.end), (20, 60));
        assert!(
            (pair.twin_stats.total_return - pair.real_stats.total_return).abs()
                <= spec.return_tolerance
        );
        assert!(
            (pair.twin_stats.volatility / pair.real_stats.volatility - 1.0).abs()
                <= spec.volatility_tolerance
        );
        assert_eq!(pair.twin_stats.regime, pair.real_stats.regime);
        assert_ne!(pair.real.closes, pair.twin.closes, "the twin reorders bars");
        // Both start from the same masked level, so level does not separate them.
        for (r, t) in pair.real.closes.values().zip(pair.twin.closes.values()) {
            assert_eq!(r[0], t[0]);
            assert!((64.0..128.0).contains(&r[0]));
        }
        assert_eq!(pair.real.dates, pair.twin.dates);
        assert_eq!(pair.real.symbols(), pair.twin.symbols());
    }

    #[test]
    fn twins_refuse_what_they_cannot_match() {
        let source = Dataset::synthetic(2, 100, 3);
        let spec = TwinSpec::default();
        assert_eq!(
            build_twin(&source, Window { start: 10, end: 50 }, &spec, 1).unwrap_err(),
            TwinError::WindowOutOfRange
        );
        let negative =
            Dataset::from_csv("date,symbol,close\nd0,A,10\nd1,A,-1\nd2,A,10\nd3,A,11\nd4,A,12\n")
                .unwrap();
        let tight = TwinSpec { warmup: 1, ..spec };
        assert!(matches!(
            build_twin(&negative, Window { start: 1, end: 5 }, &tight, 1),
            Err(TwinError::NonPositiveClose { .. })
        ));
        let impossible = TwinSpec {
            return_tolerance: 0.0,
            volatility_tolerance: 0.0,
            max_draws: 50,
            ..spec
        };
        // A window with distinct relatives almost never reproduces its exact
        // total return and volatility, so the draw budget runs out.
        assert_eq!(
            build_twin(&source, Window { start: 20, end: 60 }, &impossible, 1).unwrap_err(),
            TwinError::Unmatched { draws: 50 }
        );
    }

    #[test]
    fn unavailable_scores_refuse_the_verdict_rather_than_shrink_the_sample() {
        let scores = [
            PairedScore {
                real: Some(0.2),
                twin: Some(0.1),
            },
            PairedScore {
                real: Some(0.3),
                twin: None,
            },
        ];
        assert_eq!(
            shape_probe(&scores, ProbeConfig::default()),
            Err(ProbeError::ScoreUnavailable { window: 1 })
        );
        assert_eq!(window_sharpe(&[0.0, 0.0, 0.0]), None);
    }

    #[test]
    fn a_significant_but_tiny_edge_is_not_convicted() {
        let scores: Vec<PairedScore> = (0..12)
            .map(|_| PairedScore {
                real: Some(0.1 + 1e-9),
                twin: Some(0.1),
            })
            .collect();
        let report = shape_probe(&scores, ProbeConfig::default()).unwrap();
        assert!(report.pvalue <= 0.05, "{report:?}");
        assert!(!report.memorization, "{report:?}");
    }
}
