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

use sharpebench_stats::sharpe_ratio;

/// Probability of backtest overfitting for a performance matrix.
///
/// `perf_matrix` is **T rows (time) × N cols (strategies)** of per-period
/// returns: `perf_matrix[t][n]` is strategy `n`'s return in period `t`. `s` is
/// the (even) number of contiguous time blocks to split into. Returns a
/// probability in `[0, 1]`; near 0 ⇒ the IS-winner generalizes, near 0.5 ⇒ the
/// IS-winner is no better than chance OOS, near 1 ⇒ systematic overfitting.
///
/// Returns 0.0 for degenerate inputs (fewer than 2 strategies, `s < 2`, odd
/// `s`, or too few rows to fill the blocks) — there is nothing to overfit.
pub fn probability_of_backtest_overfitting(perf_matrix: &[Vec<f64>], s: usize) -> f64 {
    let t = perf_matrix.len();
    if t < s || s < 2 || !s.is_multiple_of(2) {
        return 0.0;
    }
    let n_strats = perf_matrix[0].len();
    if n_strats < 2 || perf_matrix.iter().any(|row| row.len() != n_strats) {
        return 0.0;
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

        // IS-best strategy (ties → lowest index, deterministic).
        let n_star = argmax(&is_sharpes);

        // OOS rank of n*, ascending so the OOS-best gets the highest rank:
        // r = 1 + (number of strategies strictly worse OOS). The IS-winner
        // landing OOS-best ⇒ r = N ⇒ ω → 1 ⇒ λ > 0 (generalizes, not overfit);
        // landing OOS-worst ⇒ r = 1 ⇒ ω → 0 ⇒ λ < 0 (overfit).
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
        0.0
    } else {
        overfit as f64 / total as f64
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
}
