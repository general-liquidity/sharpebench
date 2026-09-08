//! Probability of Backtest Overfitting (PBO) via CSCV.
//!
//! After Bailey, Borwein, López de Prado & Zhu, *The Probability of Backtest
//! Overfitting* (2014). Combinatorially-Symmetric Cross-Validation: split the
//! sample into `s` contiguous blocks, and over every way of choosing `s/2` of
//! them as in-sample (IS, the complement is out-of-sample, OOS), pick the
//! IS-best strategy and measure where it ranks OOS. PBO is the fraction of
//! splits where the IS-winner lands in the bottom half OOS (logit λ ≤ 0).
//!
//! Deterministic: enumerates C(s, s/2) splits with no RNG. Ported from the
//! published procedure, not from any GPL/proprietary library.

use std::fmt;

use sharpebench_stats::sharpe_ratio;

/// Why CSCV could not produce a probability of backtest overfitting.
///
/// A test that cannot be estimated is unavailable, not a zero probability of
/// overfitting. Each variant names the observed quantity that made the estimate
/// impossible so a caller can report the reason instead of a fabricated number.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PboUnavailable {
    /// Fewer than two strategy columns: there is no selection to price.
    TooFewStrategies { strategies: usize },
    /// `s` must be an even count of at least two contiguous blocks.
    InvalidBlockCount { blocks: usize },
    /// Fewer time rows than blocks, so at least one block would be empty.
    TooFewPeriods { periods: usize, blocks: usize },
    /// Row `row` is not the width of row 0, so the matrix is not a field.
    RaggedMatrix {
        row: usize,
        expected: usize,
        actual: usize,
    },
    /// A non-finite cell. Sharpe ratios over it are not defined, and the
    /// split comparisons below silently treat NaN as "worse than everything".
    NonFiniteObservation { row: usize, column: usize },
    /// Every enumerated split was rejected, so no fraction exists.
    NoEvaluableSplit,
}

impl fmt::Display for PboUnavailable {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TooFewStrategies { strategies } => write!(
                f,
                "CSCV needs at least 2 strategy columns, observed {strategies}"
            ),
            Self::InvalidBlockCount { blocks } => {
                write!(f, "block count must be even and at least 2, got {blocks}")
            }
            Self::TooFewPeriods { periods, blocks } => write!(
                f,
                "{blocks} blocks need at least {blocks} periods, observed {periods}"
            ),
            Self::RaggedMatrix {
                row,
                expected,
                actual,
            } => write!(f, "row {row} has {actual} strategies, expected {expected}"),
            Self::NonFiniteObservation { row, column } => {
                write!(f, "observation at row {row}, column {column} is not finite")
            }
            Self::NoEvaluableSplit => write!(f, "no CSCV split could be evaluated"),
        }
    }
}

impl std::error::Error for PboUnavailable {}

impl PboUnavailable {
    /// The value the legacy scalar entry point reports for this reason.
    ///
    /// The shape degeneracies ("nothing to overfit") historically reported
    /// `0.0` and keep doing so, because callers and published surfaces depend
    /// on it. A matrix that could not be validated at all never had a
    /// defensible scalar, so it reports NaN rather than a confident zero.
    fn legacy_scalar(self) -> f64 {
        match self {
            Self::TooFewStrategies { .. }
            | Self::InvalidBlockCount { .. }
            | Self::TooFewPeriods { .. }
            | Self::RaggedMatrix { .. } => 0.0,
            Self::NonFiniteObservation { .. } | Self::NoEvaluableSplit => f64::NAN,
        }
    }
}

/// Probability of backtest overfitting, with an explicit unavailable status.
///
/// `perf_matrix` is **T rows (time) x N cols (strategies)** of per-period
/// returns: `perf_matrix[t][n]` is strategy `n`'s return in period `t`. `s` is
/// the (even) number of contiguous time blocks to split into.
///
/// # Errors
///
/// Returns [`PboUnavailable`] when the estimate cannot be made: an invalid
/// block count, fewer periods than blocks, fewer than two strategies, a ragged
/// matrix, or a non-finite observation. The caller reports the reason; it must
/// not substitute a probability for it.
pub fn pbo_status(perf_matrix: &[Vec<f64>], s: usize) -> Result<f64, PboUnavailable> {
    if s < 2 || !s.is_multiple_of(2) {
        return Err(PboUnavailable::InvalidBlockCount { blocks: s });
    }
    let t = perf_matrix.len();
    if t < s {
        return Err(PboUnavailable::TooFewPeriods {
            periods: t,
            blocks: s,
        });
    }
    // `t >= s >= 2`, so row 0 exists.
    let n_strats = perf_matrix[0].len();
    for (row, cells) in perf_matrix.iter().enumerate() {
        if cells.len() != n_strats {
            return Err(PboUnavailable::RaggedMatrix {
                row,
                expected: n_strats,
                actual: cells.len(),
            });
        }
        if let Some(column) = cells.iter().position(|x| !x.is_finite()) {
            return Err(PboUnavailable::NonFiniteObservation { row, column });
        }
    }
    if n_strats < 2 {
        return Err(PboUnavailable::TooFewStrategies {
            strategies: n_strats,
        });
    }

    // Contiguous, near-equal block boundaries over the T rows (a short remainder
    // is spread across the leading blocks).
    let block_ranges = block_ranges(t, s);

    let mut overfit = 0usize;
    let mut total = 0usize;
    for is_blocks in combinations(s, s / 2) {
        let is_mask = mask(&is_blocks, s);

        let is_sharpes = column_sharpes(perf_matrix, &block_ranges, &is_mask, true);
        let oos_sharpes = column_sharpes(perf_matrix, &block_ranges, &is_mask, false);

        // IS-best strategy (ties -> lowest index, deterministic).
        let n_star = argmax(&is_sharpes);

        // OOS rank of n*, ascending so the OOS-best gets the highest rank:
        // r = 1 + (number of strategies strictly worse OOS). The IS-winner
        // landing OOS-best => r = N => omega -> 1 => lambda > 0 (generalizes,
        // not overfit); landing OOS-worst => r = 1 => lambda < 0 (overfit).
        let r = 1 + oos_sharpes
            .iter()
            .filter(|&&v| v < oos_sharpes[n_star])
            .count();
        let omega = r as f64 / (n_strats as f64 + 1.0);
        let lambda = (omega / (1.0 - omega)).ln();
        if lambda <= 0.0 {
            overfit += 1;
        }
        total += 1;
    }

    if total == 0 {
        return Err(PboUnavailable::NoEvaluableSplit);
    }
    Ok(overfit as f64 / total as f64)
}

/// Probability of backtest overfitting for a performance matrix, as a scalar.
///
/// Prefer [`pbo_status`]: this entry point cannot distinguish an estimated
/// probability from an estimate that could not be made. It is retained because
/// the published Python and WASM surfaces return a number. Shape degeneracies
/// (fewer than 2 strategies, `s < 2`, odd `s`, too few rows, a ragged matrix)
/// report `0.0` as they always have; a matrix carrying a non-finite observation
/// reports NaN rather than a confident zero.
///
/// Returns a probability in `[0, 1]`; near 0 => the IS-winner generalizes, near
/// 0.5 => the IS-winner is no better than chance OOS, near 1 => systematic
/// overfitting.
pub fn probability_of_backtest_overfitting(perf_matrix: &[Vec<f64>], s: usize) -> f64 {
    match pbo_status(perf_matrix, s) {
        Ok(p) => p,
        Err(reason) => reason.legacy_scalar(),
    }
}

/// Contiguous `[start, end)` ranges for `s` near-equal blocks over `t` rows.
fn block_ranges(t: usize, s: usize) -> Vec<(usize, usize)> {
    let base = t / s;
    let rem = t % s;
    let mut ranges = Vec::with_capacity(s);
    let mut start = 0;
    for b in 0..s {
        let len = base + usize::from(b < rem);
        ranges.push((start, start + len));
        start += len;
    }
    ranges
}

/// Which blocks are in-sample, as a length-`s` boolean mask.
fn mask(is_blocks: &[usize], s: usize) -> Vec<bool> {
    let mut m = vec![false; s];
    for &b in is_blocks {
        m[b] = true;
    }
    m
}

/// Per-strategy (per-column) Sharpe over the rows of the selected blocks. When
/// `want_is` the IS blocks are used, otherwise the OOS complement.
fn column_sharpes(
    perf_matrix: &[Vec<f64>],
    block_ranges: &[(usize, usize)],
    is_mask: &[bool],
    want_is: bool,
) -> Vec<f64> {
    let n_strats = perf_matrix[0].len();
    let mut out = Vec::with_capacity(n_strats);
    let mut col: Vec<f64> = Vec::new();
    for strat in 0..n_strats {
        col.clear();
        for (b, &(lo, hi)) in block_ranges.iter().enumerate() {
            if is_mask[b] == want_is {
                for row in &perf_matrix[lo..hi] {
                    col.push(row[strat]);
                }
            }
        }
        out.push(sharpe_ratio(&col));
    }
    out
}

/// Index of the maximum value (lowest index on ties).
fn argmax(xs: &[f64]) -> usize {
    let mut best = 0;
    for (i, &v) in xs.iter().enumerate().skip(1) {
        if v > xs[best] {
            best = i;
        }
    }
    best
}

/// All `k`-subsets of `0..n`, in lexicographic order. Deterministic.
fn combinations(n: usize, k: usize) -> Vec<Vec<usize>> {
    let mut out = Vec::new();
    if k > n {
        return out;
    }
    let mut idx: Vec<usize> = (0..k).collect();
    loop {
        out.push(idx.clone());
        // Advance to the next combination in lex order.
        let mut i = k;
        while i > 0 {
            i -= 1;
            if idx[i] != i + n - k {
                idx[i] += 1;
                for j in (i + 1)..k {
                    idx[j] = idx[j - 1] + 1;
                }
                break;
            }
            if i == 0 {
                return out;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn combinations_count_matches_binomial() {
        // C(10, 5) = 252.
        assert_eq!(combinations(10, 5).len(), 252);
        // C(6, 3) = 20.
        assert_eq!(combinations(6, 3).len(), 20);
    }

    /// One strategy is genuinely best everywhere (a higher-mean column) → it wins
    /// IS and OOS on every split → PBO is low.
    #[test]
    fn dominant_strategy_low_pbo() {
        let t = 60;
        let n = 5;
        let perf: Vec<Vec<f64>> = (0..t)
            .map(|i| {
                (0..n)
                    .map(|j| {
                        let edge = if j == 0 { 0.01 } else { 0.0 };
                        edge + 0.002 * (((i + j) % 5) as f64 - 2.0)
                    })
                    .collect()
            })
            .collect();
        let pbo = probability_of_backtest_overfitting(&perf, 10);
        assert!(pbo < 0.2, "dominant-strategy PBO {pbo} should be low");
    }

    /// Pure deterministic noise where no column has a persistent edge → the
    /// IS-winner is essentially random OOS → PBO near 0.5.
    #[test]
    fn noise_pbo_near_half() {
        // Deterministic SplitMix64 stream → genuinely iid cells (no serial
        // structure, so no spurious persistence or anti-persistence).
        let mut state: u64 = 0x9E37_79B9_7F4A_7C15;
        let mut next = || {
            state = state.wrapping_add(0x9E37_79B9_7F4A_7C15);
            let mut z = state;
            z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
            z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
            ((z ^ (z >> 31)) >> 11) as f64 / (1u64 << 53) as f64 - 0.5
        };
        let t = 120;
        let n = 8;
        let perf: Vec<Vec<f64>> = (0..t).map(|_| (0..n).map(|_| next()).collect()).collect();
        let pbo = probability_of_backtest_overfitting(&perf, 10);
        assert!(
            (0.3..=0.7).contains(&pbo),
            "noise PBO {pbo} should be near 0.5"
        );
    }

    #[test]
    fn degenerate_inputs_return_zero() {
        assert_eq!(probability_of_backtest_overfitting(&[], 10), 0.0);
        let one_col: Vec<Vec<f64>> = (0..20).map(|_| vec![0.01]).collect();
        assert_eq!(probability_of_backtest_overfitting(&one_col, 10), 0.0);
        // Odd s.
        let m: Vec<Vec<f64>> = (0..20).map(|_| vec![0.01, 0.02]).collect();
        assert_eq!(probability_of_backtest_overfitting(&m, 5), 0.0);
    }

    /// Every shape the estimate cannot be made on names *which* quantity made it
    /// impossible, instead of being flattened into one number.
    #[test]
    fn unavailable_reasons_are_distinguished_not_collapsed() {
        let ok: Vec<Vec<f64>> = (0..20)
            .map(|i| vec![0.01 + 0.001 * (i as f64).sin(), 0.002 * (i as f64).cos()])
            .collect();
        assert!(pbo_status(&ok, 4).is_ok());

        assert_eq!(
            pbo_status(&ok, 5),
            Err(PboUnavailable::InvalidBlockCount { blocks: 5 })
        );
        assert_eq!(
            pbo_status(&ok, 0),
            Err(PboUnavailable::InvalidBlockCount { blocks: 0 })
        );
        assert_eq!(
            pbo_status(&ok, 40),
            Err(PboUnavailable::TooFewPeriods {
                periods: 20,
                blocks: 40
            })
        );
        let one_col: Vec<Vec<f64>> = (0..20).map(|_| vec![0.01]).collect();
        assert_eq!(
            pbo_status(&one_col, 4),
            Err(PboUnavailable::TooFewStrategies { strategies: 1 })
        );

        let mut ragged = ok.clone();
        ragged[7].push(0.5);
        assert_eq!(
            pbo_status(&ragged, 4),
            Err(PboUnavailable::RaggedMatrix {
                row: 7,
                expected: 2,
                actual: 3
            })
        );
    }

    /// The reported regression: a matrix carrying a non-finite cell used to be
    /// scored anyway. Every Sharpe over the affected column is NaN, and NaN
    /// loses every `>` comparison in `argmax`, so the winner and its OOS rank
    /// were decided by column order. The result was a confident-looking
    /// probability computed from an input the test is not defined on.
    #[test]
    fn a_non_finite_observation_is_unavailable_not_a_scored_probability() {
        let with_nan = |row: usize, column: usize, bad: f64| {
            let mut m: Vec<Vec<f64>> = (0..20)
                .map(|i| {
                    (0..3)
                        .map(|j| 0.001 * ((i + j) as f64).sin() + 0.002 * (j as f64))
                        .collect::<Vec<f64>>()
                })
                .collect();
            m[row][column] = bad;
            m
        };

        for (row, column, bad) in [
            (0, 0, f64::NAN),
            (11, 2, f64::INFINITY),
            (19, 1, f64::NEG_INFINITY),
        ] {
            let m = with_nan(row, column, bad);
            assert_eq!(
                pbo_status(&m, 4),
                Err(PboUnavailable::NonFiniteObservation { row, column })
            );
            // The scalar surface cannot report a reason, so it reports NaN. It
            // must not report 0.0, which reads as "no overfitting detected".
            let scalar = probability_of_backtest_overfitting(&m, 4);
            assert!(
                scalar.is_nan(),
                "expected NaN for a non-finite cell, got {scalar}"
            );
        }
    }

    /// The repair must not move any number a valid matrix already produced.
    #[test]
    fn valid_matrices_are_unchanged_by_the_status_api() {
        let t = 60;
        let n = 5;
        let perf: Vec<Vec<f64>> = (0..t)
            .map(|i| {
                (0..n)
                    .map(|j| {
                        let edge = if j == 0 { 0.01 } else { 0.0 };
                        edge + 0.002 * (((i + j) % 5) as f64 - 2.0)
                    })
                    .collect()
            })
            .collect();
        for s in [2, 4, 10] {
            let status = pbo_status(&perf, s).expect("a rectangular finite matrix is estimable");
            assert_eq!(status, probability_of_backtest_overfitting(&perf, s));
            assert!((0.0..=1.0).contains(&status));
        }
    }
}
