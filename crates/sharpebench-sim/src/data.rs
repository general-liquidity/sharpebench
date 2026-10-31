//! Point-in-time price data.
//!
//! A [`Dataset`] is a shared date axis plus per-symbol closes aligned to it.
//! Accessors such as [`Dataset::close_at`] take an arbitrary step index, so the
//! type itself does not forbid reading a later bar. The no-look-ahead guarantee
//! is enforced at the agent boundary: the observation builder in `engine.rs`
//! only ever hands an agent history ending at the current step, so a future bar
//! is unrepresentable via the agent interface rather than policed after the
//! fact.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::marks::MarkSheet;

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
    /// When each symbol's close is marked. `None` (the default, and absent from
    /// the serialized form, so every dataset without a declaration keeps its
    /// digest) means the marks were never declared. A declared sheet is part of
    /// the dataset identity, and [`Dataset::check_mark_timing`] refuses to step
    /// symbols marked at different times unless the execution timing is safe.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub marks: Option<MarkSheet>,
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
    /// only ever reads *frozen* data (online fetch tools live outside `sim`),
    /// which pins the replay inputs. Output identity still depends on the
    /// implementation and toolchain; CI checks two committed Rust goldens on
    /// Linux, macOS, and Windows.
    ///
    /// Closes must be finite. Signed/zero raw quotes are preserved, not repaired:
    /// e.g. the frozen WTI series contains a negative observation. Successful
    /// parsing does not establish suitability for a positive-price percentage-
    /// return model; see the dataset's documented domain limitations.
    /// Cash dividends must be finite
    /// and nonnegative. An absent dividend column declares no dividend adjustment;
    /// when the column is present, every row must supply a valid value (including
    /// an explicit zero for no payment). Duplicate `(date, symbol)` keys and
    /// duplicate header names are refused, even when the rows agree.
    pub fn from_csv(text: &str) -> Result<Dataset, String> {
        let mut per_symbol: BTreeMap<String, BTreeMap<String, f64>> = BTreeMap::new();
        let mut per_div: BTreeMap<String, BTreeMap<String, f64>> = BTreeMap::new();

        let mut lines = text.lines();
        let header = lines.next().ok_or("empty CSV")?;
        let cols: Vec<&str> = header.split(',').map(str::trim).collect();
        let unique: std::collections::BTreeSet<_> = cols.iter().copied().collect();
        if unique.len() != cols.len() {
            return Err("CSV header contains duplicate column names".to_string());
        }
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
            if date.is_empty() || symbol.is_empty() {
                return Err(format!(
                    "CSV row {}: date and symbol must be nonempty",
                    n + 2
                ));
            }
            let close: f64 = field(close_i)?
                .parse()
                .map_err(|_| format!("CSV row {}: non-numeric close", n + 2))?;
            if !close.is_finite() {
                return Err(format!("CSV row {}: close must be finite", n + 2));
            }
            let dividend = match div_i {
                Some(di) => {
                    let d: f64 = field(di)?
                        .parse()
                        .map_err(|_| format!("CSV row {}: non-numeric dividend", n + 2))?;
                    if !d.is_finite() || d < 0.0 {
                        return Err(format!(
                            "CSV row {}: dividend must be finite and nonnegative",
                            n + 2
                        ));
                    }
                    Some(d)
                }
                None => None,
            };
            let prices = per_symbol.entry(symbol.clone()).or_default();
            if prices.contains_key(&date) {
                return Err(format!(
                    "CSV row {}: duplicate (date, symbol) key ({date}, {symbol})",
                    n + 2
                ));
            }
            prices.insert(date.clone(), close);
            if let Some(d) = dividend {
                per_div.entry(symbol).or_default().insert(date, d);
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
            marks: None,
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
    /// knob). Pure function of `seed`; only mul/add/div/max (no `ln`/`exp`).
    /// Rust pins two committed goldens on Linux, macOS, and Windows. Wrapper
    /// parity for WASM and Python is checked only on the operating systems used
    /// by their CI jobs, not universally across runtimes and platforms.
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
            marks: None,
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
            marks: None,
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

    /// A contamination-masked copy: symbols renamed to opaque ids, dates
    /// replaced with plain indices, and every series rebased so its first
    /// nonzero close lies in `[64, 128)`. See [`Dataset::masked_view`], which
    /// also returns the map back to the source symbols.
    pub fn masked(&self) -> Result<Dataset, MaskError> {
        self.masked_view().map(|view| view.dataset)
    }

    /// The masked view and its index map back to the source.
    ///
    /// Symbols become `ASSET_{i:03}` in the source's sorted order, dates become
    /// `t{i}`, and each series (closes and per-share cash dividends together) is
    /// multiplied by one power of two, chosen so the first nonzero close lies in
    /// `[64, 128)`. Absolute price level, which identifies an asset and its era,
    /// therefore no longer matches the source, while every simple return and
    /// every backtest outcome is bit-identical: multiplying by a power of two is
    /// exact in binary floating point whenever the result stays normal, so a
    /// price ratio, a share count times a price and a moving average all round
    /// the same way. The scale is checked rather than assumed: a scaled value
    /// that would leave the normal range refuses the whole view.
    ///
    /// A common start level of exactly 100 is not used because it would move
    /// returns by rounding, and the engine's outcomes with them.
    ///
    /// What the mask does not hide: the first close's significand survives (a
    /// source that started at 4472.13 starts at 69.877...), the sorted order of
    /// the source symbols survives in the opaque ids, and the shape of the path
    /// survives entirely. The shape is what [`crate::contamination`] measures.
    /// The dataset carries no volume, so there is no volume to normalize.
    /// Declared mark times are carried under the renamed symbols.
    pub fn masked_view(&self) -> Result<MaskedView, MaskError> {
        let dates: Vec<String> = (0..self.dates.len()).map(|i| format!("t{i}")).collect();
        let mut closes = BTreeMap::new();
        let mut dividends = BTreeMap::new();
        let mut symbols = Vec::with_capacity(self.closes.len());
        for (i, (symbol, series)) in self.closes.iter().enumerate() {
            let opaque = format!("ASSET_{i:03}");
            let exponent = rebase_exponent(symbol, series)?;
            closes.insert(opaque.clone(), scale_series(symbol, series, exponent)?);
            if let Some(stream) = self.dividends.get(symbol) {
                dividends.insert(opaque.clone(), scale_series(symbol, stream, exponent)?);
            }
            symbols.push(MaskedSymbol {
                source: symbol.clone(),
                opaque,
                scale_exponent: exponent,
            });
        }
        let marks = self.marks.as_ref().map(|sheet| {
            let mut renamed = sheet.clone();
            renamed.symbols = symbols
                .iter()
                .filter_map(|m| {
                    sheet
                        .symbols
                        .get(&m.source)
                        .map(|mark| (m.opaque.clone(), mark.clone()))
                })
                .collect();
            renamed
        });
        Ok(MaskedView {
            dataset: Dataset {
                dates,
                closes,
                dividends,
                marks,
            },
            symbols,
        })
    }

    /// Attach a declared mark-time sheet. The sheet must name exactly the
    /// dataset's symbols, so a mark cannot be silently missing for one leg.
    pub fn with_marks(mut self, sheet: MarkSheet) -> Result<Dataset, String> {
        sheet.validate()?;
        let declared: Vec<&String> = sheet.symbols.keys().collect();
        let present: Vec<&String> = self.closes.keys().collect();
        if declared != present {
            return Err(format!(
                "mark-time sheet names {declared:?}, but the dataset holds {present:?}"
            ));
        }
        self.marks = Some(sheet);
        Ok(self)
    }
}

/// The exponent `k` for which `|first nonzero close| * 2^k` lies in `[64, 128)`.
/// A series of zeros keeps `k = 0`: there is no level to hide.
fn rebase_exponent(symbol: &str, series: &[f64]) -> Result<i32, MaskError> {
    let Some(step) = series.iter().position(|x| *x != 0.0) else {
        return Ok(0);
    };
    let first = series[step];
    if !first.is_normal() {
        return Err(MaskError::UnscalableLevel {
            symbol: symbol.to_string(),
            step,
        });
    }
    // The unbiased binary exponent of a normal f64: |first| = m * 2^e, m in [1, 2).
    let e = ((first.to_bits() >> 52) & 0x7ff) as i32 - 1023;
    Ok(6 - e)
}

fn scale_series(symbol: &str, series: &[f64], exponent: i32) -> Result<Vec<f64>, MaskError> {
    if !(-1022..=1023).contains(&exponent) {
        return Err(MaskError::UnscalableLevel {
            symbol: symbol.to_string(),
            step: 0,
        });
    }
    let factor = f64::from_bits(((exponent + 1023) as u64) << 52);
    series
        .iter()
        .enumerate()
        .map(|(step, &x)| {
            let y = x * factor;
            // Exactness needs the product to stay normal (or be an exact zero).
            if x == 0.0 || y.is_normal() {
                Ok(y)
            } else {
                Err(MaskError::UnscalableLevel {
                    symbol: symbol.to_string(),
                    step,
                })
            }
        })
        .collect()
}

/// One source symbol's place in a [`MaskedView`].
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MaskedSymbol {
    pub source: String,
    pub opaque: String,
    /// The source series was multiplied by `2^scale_exponent`.
    pub scale_exponent: i32,
}

/// A masked dataset plus the map from each opaque id back to its source.
#[derive(Clone, Debug)]
pub struct MaskedView {
    pub dataset: Dataset,
    pub symbols: Vec<MaskedSymbol>,
}

/// Why a masked view could not be built without changing a return.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MaskError {
    /// Rebasing this value would leave the normal floating-point range, or the
    /// first nonzero close is itself subnormal, so the scale would not be exact.
    UnscalableLevel { symbol: String, step: usize },
}

impl std::fmt::Display for MaskError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MaskError::UnscalableLevel { symbol, step } => write!(
                f,
                "cannot rebase {symbol} at step {step} exactly: the scaled value leaves the normal floating-point range"
            ),
        }
    }
}

impl std::error::Error for MaskError {}

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
    fn csv_requires_valid_complete_economic_rows() {
        let valid = "date,symbol,close,dividend\n2025-01-01,AAA,100,1\n2025-01-02,AAA,101,0\n";
        let d = Dataset::from_csv(valid).unwrap();
        assert_eq!(d.dividend_at("AAA", 0), 1.0);
        assert_eq!(d.dividend_at("AAA", 1), 0.0);
        for bad in ["NaN", "inf", "-inf"] {
            let csv = valid.replace("AAA,100,1", &format!("AAA,{bad},1"));
            let error = Dataset::from_csv(&csv).unwrap_err();
            assert!(
                error.contains("row 2") && error.contains("close"),
                "{error}"
            );
        }
        for bad in ["NaN", "inf", "-inf", "-1", "garbage", ""] {
            let csv = valid.replace("AAA,100,1", &format!("AAA,100,{bad}"));
            let error = Dataset::from_csv(&csv).unwrap_err();
            assert!(
                error.contains("row 2") && error.contains("dividend"),
                "{error}"
            );
        }
        let missing = valid.replace("AAA,100,1", "AAA,100");
        assert!(Dataset::from_csv(&missing).unwrap_err().contains("row 2"));
        // Omitted dividend column declares a price-only dataset; a present but
        // invalid cell must not be confused with this intentional omission.
        let price_only = "date,symbol,close\n2025-01-01,AAA,100\n2025-01-02,AAA,101\n";
        assert!(Dataset::from_csv(price_only).unwrap().dividends.is_empty());
    }

    #[test]
    fn raw_csv_preserves_signed_quotes_without_implying_return_model_validity() {
        let csv =
            "date,symbol,close\n2020-04-17,WTI,18.31\n2020-04-20,WTI,-36.98\n2020-04-21,WTI,0\n";
        let dataset = Dataset::from_csv(csv).unwrap();
        assert_eq!(dataset.closes["WTI"], [18.31, -36.98, 0.0]);
    }

    #[test]
    fn csv_duplicate_keys_and_ambiguous_columns_are_refused() {
        let valid = "date,symbol,close,dividend\n2025-01-01,AAA,100,1\n2025-01-02,AAA,101,0\n";
        // Even identical rows are duplicates: row count must not imply distinct
        // observations. A conflicting row must never combine two rows' fields.
        for duplicate in ["2025-01-01,AAA,100,1", "2025-01-01,AAA,102,0"] {
            let error = Dataset::from_csv(&format!("{valid}{duplicate}\n")).unwrap_err();
            assert!(
                error.contains("duplicate") && error.contains("row 4"),
                "{error}"
            );
        }
        let header = valid.replace("close,dividend", "close,close");
        assert!(Dataset::from_csv(&header)
            .unwrap_err()
            .contains("duplicate"));
        for csv in [
            valid.replace("2025-01-01,AAA", ",AAA"),
            valid.replace("2025-01-01,AAA", "2025-01-01,"),
        ] {
            assert!(Dataset::from_csv(&csv).unwrap_err().contains("row 2"));
        }
    }

    #[test]
    fn masking_preserves_dividends_under_the_price_symbol_mapping() {
        let mut d = Dataset::synthetic(3, 4, 1);
        // Only the middle symbol pays: enumerating dividend keys separately
        // would falsely attach this stream to ASSET_000.
        d.dividends.insert("SYM01".into(), vec![0.0, 1.0, 2.0, 0.0]);
        let view = d.masked_view().unwrap();
        let masked = &view.dataset;
        assert_eq!(masked.dividends.len(), 1);
        for (i, original) in d.symbols().iter().enumerate() {
            let renamed = format!("ASSET_{i:03}");
            assert_eq!(view.symbols[i].source, *original);
            assert_eq!(view.symbols[i].opaque, renamed);
            let scale = 2f64.powi(view.symbols[i].scale_exponent);
            for t in 0..d.len() {
                // Closes and dividends share one exact power-of-two scale.
                assert_eq!(
                    d.close_at(original, t).map(|x| x * scale),
                    masked.close_at(&renamed, t)
                );
                assert_eq!(
                    d.dividend_at(original, t) * scale,
                    masked.dividend_at(&renamed, t)
                );
            }
            // Hold one share throughout, reinvesting nothing. Every step's
            // liquidation wealth, relative to the first, must be preserved.
            let mut cash = 0.0;
            let mut masked_cash = 0.0;
            let w0 = d.close_at(original, 0).unwrap();
            let m0 = masked.close_at(&renamed, 0).unwrap();
            for t in 0..d.len() {
                cash += d.dividend_at(original, t);
                masked_cash += masked.dividend_at(&renamed, t);
                assert_eq!(
                    (cash + d.close_at(original, t).unwrap()) / w0,
                    (masked_cash + masked.close_at(&renamed, t).unwrap()) / m0
                );
            }
        }
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
    fn masking_rebases_levels_and_keeps_every_return_bit_for_bit() {
        // Levels far apart, as real assets are: an index near 4472, a coin
        // near 0.31, a stock near 187.
        let csv = "date,symbol,close\n\
                   2020-01-02,COIN,0.3121\n2020-01-02,IDX,4472.13\n2020-01-02,STK,187.5\n\
                   2020-01-03,COIN,0.3307\n2020-01-03,IDX,4490.01\n2020-01-03,STK,185.25\n\
                   2020-01-06,COIN,0.2999\n2020-01-06,IDX,4401.77\n2020-01-06,STK,190.0\n";
        let d = Dataset::from_csv(csv).unwrap();
        let m = d.masked().unwrap();
        assert!(m.symbols().iter().all(|s| s.starts_with("ASSET_")));
        assert!(m.dates.iter().all(|s| s.starts_with('t')));
        for (src, masked) in d.closes.values().zip(m.closes.values()) {
            assert!((64.0..128.0).contains(&masked[0]), "{}", masked[0]);
            assert_ne!(src[0], masked[0], "the level no longer matches the source");
            for t in 1..src.len() {
                assert_eq!(
                    (src[t] / src[t - 1]).to_bits(),
                    (masked[t] / masked[t - 1]).to_bits()
                );
            }
        }
    }

    #[test]
    fn masking_refuses_a_level_it_cannot_scale_exactly() {
        let csv = format!("date,symbol,close\nd0,A,1\nd1,A,{}\n", f64::MAX);
        let d = Dataset::from_csv(&csv).unwrap();
        assert_eq!(
            d.masked().unwrap_err(),
            MaskError::UnscalableLevel {
                symbol: "A".into(),
                step: 1
            }
        );
        // A zero close is kept as an exact zero, and a series of zeros keeps
        // its (zero) level.
        let zeros = Dataset::from_csv("date,symbol,close\nd0,Z,0\nd1,Z,0\n").unwrap();
        assert_eq!(zeros.masked().unwrap().closes["ASSET_000"], vec![0.0, 0.0]);
    }
}
