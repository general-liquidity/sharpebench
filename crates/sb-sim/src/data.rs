//! Point-in-time price data.
//!
//! A [`Dataset`] is a shared date axis plus per-symbol closes aligned to it. The
//! only accessors return data at or before a given step index — there is no way
//! to read a future bar, so look-ahead bias is impossible by construction rather
//! than policed after the fact.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// A point-in-time price dataset.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Dataset {
    pub dates: Vec<String>,
    /// symbol → closes, each `Vec` aligned to `dates`.
    pub closes: BTreeMap<String, Vec<f64>>,
}

impl Dataset {
    pub fn symbols(&self) -> Vec<String> {
        self.closes.keys().cloned().collect()
    }

    pub fn len(&self) -> usize {
        self.dates.len()
    }

    pub fn is_empty(&self) -> bool {
        self.dates.is_empty()
    }

    /// Close for `symbol` at step `t`, or `None` if out of range.
    pub fn close_at(&self, symbol: &str, t: usize) -> Option<f64> {
        self.closes.get(symbol).and_then(|v| v.get(t)).copied()
    }

    /// Trailing closes ending at step `t` (inclusive), at most `lookback` long.
    /// Point-in-time: never includes a bar after `t`.
    pub fn history(&self, symbol: &str, t: usize, lookback: usize) -> Vec<f64> {
        match self.closes.get(symbol) {
            Some(v) if !v.is_empty() => {
                let end = t.min(v.len() - 1);
                let start = end + 1 - lookback.min(end + 1);
                v[start..=end].to_vec()
            }
            _ => Vec::new(),
        }
    }

    /// Build a deterministic synthetic dataset with mild momentum
    /// autocorrelation — enough to make the reference agents behave differently.
    /// Pure function of `seed` (no ambient RNG).
    pub fn synthetic(n_symbols: usize, n_days: usize, seed: u64) -> Dataset {
        let dates: Vec<String> = (0..n_days).map(|d| format!("2025-{:03}", d + 1)).collect();
        let mut closes = BTreeMap::new();
        let mut state = seed ^ 0x1234_5678_9ABC_DEF0;
        let mut next = || {
            state = state.wrapping_add(0x9E37_79B9_7F4A_7C15);
            let mut z = state;
            z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
            z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
            z ^= z >> 31;
            (z >> 11) as f64 / (1u64 << 53) as f64 // [0,1)
        };
        for s in 0..n_symbols {
            let mut price = 100.0;
            let mut momentum = 0.0;
            let drift = 0.0002 + 0.0004 * (s as f64 / n_symbols.max(1) as f64);
            let mut series = Vec::with_capacity(n_days);
            for _ in 0..n_days {
                let shock = (next() - 0.5) * 0.02;
                momentum = 0.9 * momentum + 0.1 * shock; // autocorrelated component
                let ret = drift + momentum + 0.5 * shock;
                price *= 1.0 + ret;
                series.push(price);
            }
            closes.insert(format!("SYM{s:02}"), series);
        }
        Dataset { dates, closes }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn history_is_point_in_time() {
        let d = Dataset::synthetic(2, 50, 7);
        let h = d.history("SYM00", 10, 5);
        assert_eq!(h.len(), 5);
        // The last element of the trailing window equals the close at t=10.
        assert_eq!(*h.last().unwrap(), d.close_at("SYM00", 10).unwrap());
    }

    #[test]
    fn synthetic_is_deterministic() {
        let a = Dataset::synthetic(3, 40, 99);
        let b = Dataset::synthetic(3, 40, 99);
        assert_eq!(a.closes, b.closes);
    }
}
