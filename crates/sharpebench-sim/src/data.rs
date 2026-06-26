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
    /// symbol → per-share cash dividend paid at each step, aligned to `dates`.
    /// Empty (the default) means no corporate actions. Stock splits need no entry
    /// here: on a split-adjusted close series they are price-neutral by
    /// construction, so only the cash dividend stream changes total return.
    #[serde(default)]
    pub dividends: BTreeMap<String, Vec<f64>>,
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

    /// Per-share cash dividend paid by `symbol` at step `t` (0.0 if none).
    pub fn dividend_at(&self, symbol: &str, t: usize) -> f64 {
        self.dividends
            .get(symbol)
            .and_then(|v| v.get(t))
            .copied()
            .unwrap_or(0.0)
    }

    /// Attach a constant dividend yield: every symbol pays `per_period_yield` of
    /// its close as a cash dividend each step (e.g. an annual 4% yield on daily
    /// bars ≈ `0.04 / 252`). Models the cash-flow half of corporate actions.
    pub fn with_dividend_yield(mut self, per_period_yield: f64) -> Self {
        self.dividends = self
            .closes
            .iter()
            .map(|(sym, series)| {
                let stream = series.iter().map(|&px| px * per_period_yield).collect();
                (sym.clone(), stream)
            })
            .collect();
        self
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

    /// Load a frozen dataset from long-format CSV (`date,symbol,close[,dividend]`,
    /// header required). The series are aligned on the **intersection** of every
    /// symbol's dates, so `close_at(sym, t)` lines up across symbols; ISO
    /// `YYYY-MM-DD` dates sort chronologically. Pure — no network. The benchmark
    /// only ever reads *frozen* data (offline fetchers live in the `xtask` crate),
    /// which is what keeps a score reproducible forever.
    pub fn from_csv(text: &str) -> Result<Dataset, String> {
        let mut per_symbol: BTreeMap<String, BTreeMap<String, f64>> = BTreeMap::new();
        let mut per_div: BTreeMap<String, BTreeMap<String, f64>> = BTreeMap::new();

        let mut lines = text.lines();
        let header = lines.next().ok_or("empty CSV")?;
        let cols: Vec<&str> = header.split(',').map(str::trim).collect();
        let col = |name: &str| cols.iter().position(|c| *c == name);
        let date_i = col("date").ok_or("CSV header missing 'date'")?;
        let sym_i = col("symbol").ok_or("CSV header missing 'symbol'")?;
        let close_i = col("close").ok_or("CSV header missing 'close'")?;
        let div_i = col("dividend");

        for (n, line) in lines.enumerate() {
            if line.trim().is_empty() {
                continue;
            }
            let f: Vec<&str> = line.split(',').map(str::trim).collect();
            let field = |i: usize| {
                f.get(i)
                    .copied()
                    .ok_or_else(|| format!("CSV row {}: too few columns", n + 2))
            };
            let date = field(date_i)?.to_string();
            let symbol = field(sym_i)?.to_string();
            let close: f64 = field(close_i)?
                .parse()
                .map_err(|_| format!("CSV row {}: non-numeric close", n + 2))?;
            per_symbol
                .entry(symbol.clone())
                .or_default()
                .insert(date.clone(), close);
            if let Some(di) = div_i {
                if let Some(Ok(d)) = f.get(di).map(|s| s.trim().parse::<f64>()) {
                    per_div.entry(symbol).or_default().insert(date, d);
                }
            }
        }
        if per_symbol.is_empty() {
            return Err("CSV has no data rows".to_string());
        }

        // Shared axis = the intersection of every symbol's dates (guarantees the
        // per-symbol series are step-for-step aligned).
        let mut axis: Option<std::collections::BTreeSet<String>> = None;
        for m in per_symbol.values() {
            let set: std::collections::BTreeSet<String> = m.keys().cloned().collect();
            axis = Some(match axis {
                Some(a) => a.intersection(&set).cloned().collect(),
                None => set,
            });
        }
        let dates: Vec<String> = axis.unwrap_or_default().into_iter().collect();
        if dates.len() < 2 {
            return Err("CSV has fewer than 2 dates common to all symbols".to_string());
        }

        let mut closes = BTreeMap::new();
        let mut dividends = BTreeMap::new();
        for (sym, m) in &per_symbol {
            closes.insert(sym.clone(), dates.iter().map(|d| m[d]).collect());
            if let Some(dm) = per_div.get(sym) {
                let stream: Vec<f64> = dates
                    .iter()
                    .map(|d| dm.get(d).copied().unwrap_or(0.0))
                    .collect();
                if stream.iter().any(|&x| x != 0.0) {
                    dividends.insert(sym.clone(), stream);
                }
            }
        }
        Ok(Dataset {
            dates,
            closes,
            dividends,
        })
    }

    /// Load a frozen dataset from a CSV file path. See [`Dataset::from_csv`].
    pub fn from_csv_file(path: &str) -> Result<Dataset, String> {
        let text = std::fs::read_to_string(path).map_err(|e| format!("cannot read {path}: {e}"))?;
        Self::from_csv(&text)
    }

    /// Build a deterministic synthetic dataset with mild momentum
    /// autocorrelation — enough to make the reference agents behave differently.
    /// Pure function of `seed` (no ambient RNG). Thin wrapper over
    /// [`Dataset::synthetic_parameterized`] at the calm-market parameters
    /// (unit vol, no jumps) — byte-identical to the standalone generator it
    /// replaced (pinned by `synthetic_is_byte_identical_golden`).
    pub fn synthetic(n_symbols: usize, n_days: usize, seed: u64) -> Dataset {
        Dataset::synthetic_parameterized(n_symbols, n_days, seed, 1.0, 0.0, 0.0)
    }

    /// The continuous-vol / jump-diffusion generalization of [`Dataset::synthetic`]:
    /// the same drift + AR(1)-momentum path, with each bar's Gaussian-ish shock
    /// scaled by `vol_mult` and seeded **bounded-uniform jumps** of magnitude
    /// `jump_size` injected with per-bar probability `jump_prob` (a fat-tail stress
    /// knob). Pure function of `seed`; only mul/add/div/max (no `ln`/`exp`), so the
    /// path is byte-identical across Rust/WASM/Python.
    ///
    /// Determinism note: the jump draws are taken **only** when `jump_prob > 0`, so
    /// the no-jump call consumes the RNG identically to the original `synthetic`
    /// (one draw per bar) — `vol_mult = 1.0, jump_prob = 0.0` reproduces it exactly.
    /// Prices are kept strictly positive by flooring the per-bar growth factor.
    pub fn synthetic_parameterized(
        n_symbols: usize,
        n_days: usize,
        seed: u64,
        vol_mult: f64,
        jump_prob: f64,
        jump_size: f64,
    ) -> Dataset {
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
                let shock = (next() - 0.5) * 0.02 * vol_mult;
                momentum = 0.9 * momentum + 0.1 * shock; // autocorrelated component
                let mut ret = drift + momentum + 0.5 * shock;
                // Jumps are opt-in: the `&&` short-circuit only consumes RNG when
                // armed, so the no-jump path leaves the calm stream byte-identical.
                if jump_prob > 0.0 && next() < jump_prob {
                    // Bounded-uniform jump in (-jump_size, jump_size).
                    ret += (next() - 0.5) * 2.0 * jump_size;
                }
                price *= (1.0 + ret).max(1e-9); // floor keeps prices positive
                series.push(price);
            }
            closes.insert(format!("SYM{s:02}"), series);
        }
        Dataset {
            dates,
            closes,
            dividends: BTreeMap::new(),
        }
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
        Dataset {
            dates,
            closes,
            dividends: BTreeMap::new(),
        }
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
        Dataset {
            dates,
            closes,
            dividends: BTreeMap::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_csv_aligns_on_common_dates() {
        // BBB is missing 2025-01-03, so the shared axis is the first two dates.
        let csv = "date,symbol,close\n\
                   2025-01-01,AAA,10\n2025-01-01,BBB,20\n\
                   2025-01-02,AAA,11\n2025-01-02,BBB,19\n\
                   2025-01-03,AAA,12\n";
        let ds = Dataset::from_csv(csv).unwrap();
        assert_eq!(ds.dates, vec!["2025-01-01", "2025-01-02"]);
        assert_eq!(ds.closes["AAA"], vec![10.0, 11.0]);
        assert_eq!(ds.closes["BBB"], vec![20.0, 19.0]);
        assert_eq!(ds.close_at("AAA", 1), Some(11.0));
    }

    #[test]
    fn from_csv_rejects_malformed_input() {
        assert!(Dataset::from_csv("date,symbol\n2025-01-01,AAA").is_err()); // no close column
        assert!(Dataset::from_csv("date,symbol,close\n2025-01-01,AAA,10").is_err()); // < 2 dates
        assert!(
            Dataset::from_csv("date,symbol,close\n2025-01-01,AAA,oops\n2025-01-02,AAA,11").is_err()
        ); // non-numeric close
    }

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

    /// Order-independent fold over every close (bit-exact) — a stand-in for a hash
    /// that survives BTreeMap iteration order.
    fn closes_fingerprint(d: &Dataset) -> u64 {
        let mut h = 0xcbf2_9ce4_8422_2325u64; // FNV offset basis
        for series in d.closes.values() {
            for px in series {
                h ^= px.to_bits();
                h = h.wrapping_mul(0x0000_0100_0000_01b3); // FNV prime
            }
        }
        h
    }

    /// Golden pin: the refactor that routed `synthetic` through
    /// `synthetic_parameterized(.., 1.0, 0.0, 0.0)` must reproduce the original
    /// generator bit-for-bit. The constant is the fingerprint captured *before*
    /// the refactor (seed 99, 3×40); a regression flips it.
    #[test]
    fn synthetic_is_byte_identical_golden() {
        let d = Dataset::synthetic(3, 40, 99);
        assert_eq!(
            closes_fingerprint(&d),
            298_678_261_974_633_681,
            "synthetic price path drifted from the pre-refactor golden"
        );
        // The parameterized generator at calm parameters is the same path.
        let p = Dataset::synthetic_parameterized(3, 40, 99, 1.0, 0.0, 0.0);
        assert_eq!(d.closes, p.closes);
    }

    #[test]
    fn vol_mult_widens_the_path_and_jumps_perturb_it() {
        let base = Dataset::synthetic_parameterized(2, 200, 7, 1.0, 0.0, 0.0);
        let calm = Dataset::synthetic(2, 200, 7);
        assert_eq!(base.closes, calm.closes, "calm params == synthetic");

        // Higher vol multiplier must change the realized path.
        let hot = Dataset::synthetic_parameterized(2, 200, 7, 3.0, 0.0, 0.0);
        assert_ne!(base.closes, hot.closes, "vol_mult must move the path");

        // Jumps must perturb the path and keep prices strictly positive.
        let jumpy = Dataset::synthetic_parameterized(2, 200, 7, 1.0, 0.5, 0.1);
        assert_ne!(base.closes, jumpy.closes, "jumps must perturb the path");
        assert!(
            jumpy.closes.values().flatten().all(|&px| px > 0.0),
            "prices must stay positive through jumps"
        );
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
    fn dividend_yield_builder_pays_a_fraction_of_price() {
        let d = Dataset::synthetic(2, 30, 3).with_dividend_yield(0.01);
        let px = d.close_at("SYM00", 5).unwrap();
        assert!((d.dividend_at("SYM00", 5) - px * 0.01).abs() < 1e-12);
        // A symbol/step with no dividend stream returns 0.
        let plain = Dataset::synthetic(2, 30, 3);
        assert_eq!(plain.dividend_at("SYM00", 5), 0.0);
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
