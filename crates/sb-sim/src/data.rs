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

    /// Adversarial path: a synthetic series with a sudden one-day **flash crash**
    /// of `crash_pct` at `crash_day` that does not fully recover — a tail-stress
    /// scenario that should blow up agents with no risk discipline.
    pub fn flash_crash(
        n_symbols: usize,
        n_days: usize,
        crash_day: usize,
        crash_pct: f64,
        seed: u64,
    ) -> Dataset {
        let mut d = Dataset::synthetic(n_symbols, n_days, seed);
        let factor = (1.0 - crash_pct).max(0.0);
        for series in d.closes.values_mut() {
            for v in series.iter_mut().skip(crash_day) {
                *v *= factor;
            }
        }
        d
    }

    /// **Whipsaw** regime: sharp alternating up/down moves with no drift. Trend and
    /// momentum agents get chopped up by transaction costs.
    pub fn whipsaw(n_symbols: usize, n_days: usize, amplitude: f64, seed: u64) -> Dataset {
        let dates: Vec<String> = (0..n_days).map(|d| format!("2025-{:03}", d + 1)).collect();
        let mut closes = BTreeMap::new();
        let phase = (seed % 2) as usize;
        for s in 0..n_symbols {
            let mut price = 100.0;
            let mut series = Vec::with_capacity(n_days);
            for i in 0..n_days {
                let dir = if (i + s + phase).is_multiple_of(2) {
                    1.0
                } else {
                    -1.0
                };
                price *= 1.0 + dir * amplitude;
                series.push(price);
            }
            closes.insert(format!("SYM{s:02}"), series);
        }
        Dataset { dates, closes }
    }

    /// A named adversarial stress suite — each scenario tests *survival*, not
    /// calm-market return.
    pub fn stress_suite(seed: u64) -> Vec<(&'static str, Dataset)> {
        vec![
            ("flash_crash", Dataset::flash_crash(6, 180, 90, 0.30, seed)),
            ("whipsaw", Dataset::whipsaw(6, 180, 0.04, seed)),
        ]
    }

    /// A contamination-masked copy: symbols renamed to opaque ids and dates
    /// replaced with plain indices, so an agent can't pattern-match a memorized
    /// ticker or calendar window. Prices are preserved. (After KTD-Fin's data-side
    /// masking.)
    pub fn masked(&self) -> Dataset {
        let dates: Vec<String> = (0..self.dates.len()).map(|i| format!("t{i}")).collect();
        let closes: BTreeMap<String, Vec<f64>> = self
            .closes
            .values()
            .enumerate()
            .map(|(i, series)| (format!("ASSET_{i:03}"), series.clone()))
            .collect();
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

    #[test]
    fn flash_crash_has_a_big_drop() {
        let d = Dataset::flash_crash(2, 120, 60, 0.3, 5);
        let s = &d.closes["SYM00"];
        assert!(
            s[60] < s[59] * 0.8,
            "crash should drop ≥20%: {} -> {}",
            s[59],
            s[60]
        );
    }

    #[test]
    fn whipsaw_has_near_zero_drift() {
        let d = Dataset::whipsaw(1, 100, 0.03, 1);
        let s = &d.closes["SYM00"];
        let total = s.last().unwrap() / s[0] - 1.0;
        assert!(total.abs() < 0.1, "whipsaw drift={total}");
    }

    #[test]
    fn stress_suite_has_scenarios() {
        assert_eq!(Dataset::stress_suite(1).len(), 2);
    }

    #[test]
    fn masking_anonymizes_but_preserves_prices() {
        let d = Dataset::synthetic(3, 40, 1);
        let m = d.masked();
        assert_eq!(m.symbols().len(), 3);
        assert!(m.symbols().iter().all(|s| s.starts_with("ASSET_")));
        assert!(m.dates.iter().all(|s| s.starts_with('t')));
        // Prices are preserved (BTreeMap order is stable, so first maps to first).
        assert_eq!(
            d.closes.values().next().unwrap(),
            m.closes.values().next().unwrap()
        );
    }
}
