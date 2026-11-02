//! Partial blinding levels and calendar shifts of a [`Dataset`].
//!
//! [`Dataset::masked_view`] hides three handles at once: the symbol names, the
//! calendar dates and the price level. A presentation gap measured against it
//! says that the agent reads one of them, not which. The levels here hide one
//! handle at a time (after KTD-Fin's four-level masking protocol,
//! arXiv:2605.28359 section 3.3): its stock-blind level is [`BlindingLevel::TickersOnly`], its
//! date-blind level is [`BlindingLevel::DatesOnly`], and its blinded level is
//! the names-and-dates part of [`BlindingLevel::Full`]. [`BlindingLevel::LevelsOnly`]
//! has no KTD-Fin counterpart: it isolates the price-level handle the full mask
//! adds. Every level keeps every simple return bit-identical, so an agent that
//! reads only returns decides the same way under all of them.
//!
//! [`Dataset::date_shifted`] is the other probe of calendar reading: the same
//! bars under real-looking dates moved by a declared number of days. A date
//! mask shows an agent no calendar at all, which an honest agent may also
//! react to; a shift shows it a calendar that is merely wrong.
//! After Profit Mirage's counterfactual input perturbation (Li, Zeng, Xing, Xu
//! and Xu, arXiv:2510.07920): the same decision problem under a changed input
//! that should not matter, with the decisions compared for consistency.

use std::collections::BTreeMap;

use serde::Serialize;

use crate::data::{rebase_exponent, scale_series, Dataset, MaskError, MaskedSymbol, MaskedView};

/// Which identifying handles a view hides.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum BlindingLevel {
    /// Symbols renamed `ASSET_{i:03}`; dates and price level kept.
    TickersOnly,
    /// Dates replaced by `t{i}`; symbols and price level kept.
    DatesOnly,
    /// Each series rebased by a power of two; symbols and dates kept.
    LevelsOnly,
    /// All three: exactly [`Dataset::masked_view`].
    Full,
}

impl BlindingLevel {
    fn hides(self) -> (bool, bool, bool) {
        match self {
            BlindingLevel::TickersOnly => (true, false, false),
            BlindingLevel::DatesOnly => (false, true, false),
            BlindingLevel::LevelsOnly => (false, false, true),
            BlindingLevel::Full => (true, true, true),
        }
    }
}

/// The view of `data` at `level`. A kept symbol name maps to itself and a kept
/// level has `scale_exponent == 0`, so the returned map is always the map back
/// to the source.
pub(crate) fn blind(data: &Dataset, level: BlindingLevel) -> Result<MaskedView, MaskError> {
    let (names, dates, levels) = level.hides();
    let new_dates: Vec<String> = if dates {
        (0..data.dates.len()).map(|i| format!("t{i}")).collect()
    } else {
        data.dates.clone()
    };
    let mut closes = BTreeMap::new();
    let mut dividends = BTreeMap::new();
    let mut symbols = Vec::with_capacity(data.closes.len());
    for (i, (symbol, series)) in data.closes.iter().enumerate() {
        let opaque = if names {
            format!("ASSET_{i:03}")
        } else {
            symbol.clone()
        };
        let exponent = if levels {
            rebase_exponent(symbol, series)?
        } else {
            0
        };
        closes.insert(opaque.clone(), scale_series(symbol, series, exponent)?);
        if let Some(stream) = data.dividends.get(symbol) {
            dividends.insert(opaque.clone(), scale_series(symbol, stream, exponent)?);
        }
        symbols.push(MaskedSymbol {
            source: symbol.clone(),
            opaque,
            scale_exponent: exponent,
        });
    }
    let marks = data.marks.as_ref().map(|sheet| {
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
            dates: new_dates,
            closes,
            dividends,
            marks,
        },
        symbols,
    })
}

/// Why a date shift could not be applied.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "reason", rename_all = "snake_case")]
pub enum DateShiftError {
    /// A zero shift shows the agent the real calendar: the pair would compare
    /// a presentation with itself.
    ZeroOffset,
    /// This date does not start with a valid `YYYY-MM-DD` calendar day.
    Unparseable { step: usize, date: String },
    /// The shifted day falls outside years 1 to 9999.
    OutOfRange { step: usize },
}

impl std::fmt::Display for DateShiftError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}

impl std::error::Error for DateShiftError {}

/// Days from 1970-01-01 to the proleptic Gregorian day `y-m-d` (Hinnant's
/// `days_from_civil`). The caller has checked `m` and `d`.
fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// The inverse of [`days_from_civil`].
fn civil_from_days(z: i64) -> (i64, i64, i64) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (
        if m <= 2 {
            yoe + era * 400 + 1
        } else {
            yoe + era * 400
        },
        m,
        d,
    )
}

fn days_in_month(y: i64, m: i64) -> i64 {
    match m {
        2 if (y % 4 == 0 && y % 100 != 0) || y % 400 == 0 => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    }
}

/// The calendar day a date string starts with, as days since 1970-01-01, and
/// the rest of the string. Accepts exactly `YYYY-MM-DD` with a real month and
/// day, optionally followed by a time part that starts with `T` or a space.
pub fn parse_iso_day(date: &str) -> Option<(i64, &str)> {
    let bytes = date.as_bytes();
    if bytes.len() < 10 || bytes[4] != b'-' || bytes[7] != b'-' {
        return None;
    }
    let digits = |range: std::ops::Range<usize>| -> Option<i64> {
        bytes[range.clone()]
            .iter()
            .all(u8::is_ascii_digit)
            .then(|| date[range].parse().ok())
            .flatten()
    };
    let (y, m, d) = (digits(0..4)?, digits(5..7)?, digits(8..10)?);
    let rest = &date[10..];
    if !(rest.is_empty() || rest.starts_with('T') || rest.starts_with(' ')) {
        return None;
    }
    if !(1..=9999).contains(&y) || !(1..=12).contains(&m) || d < 1 || d > days_in_month(y, m) {
        return None;
    }
    Some((days_from_civil(y, m, d), rest))
}

/// The calendar year a date string starts with, when [`parse_iso_day`]
/// accepts it.
pub fn iso_year(date: &str) -> Option<i64> {
    parse_iso_day(date).map(|(days, _)| civil_from_days(days).0)
}

fn format_day(days: i64) -> Option<String> {
    let (y, m, d) = civil_from_days(days);
    (1..=9999)
        .contains(&y)
        .then(|| format!("{y:04}-{m:02}-{d:02}"))
}

impl Dataset {
    /// The view at one blinding level. [`BlindingLevel::Full`] is
    /// [`Dataset::masked_view`] byte for byte.
    pub fn blinded_view(&self, level: BlindingLevel) -> Result<MaskedView, MaskError> {
        blind(self, level)
    }

    /// The same bars with every calendar day moved by `offset_days`. Symbols,
    /// prices, dividends and marks are untouched, and a constant offset keeps
    /// the dates strictly ordered. The shift relabels the calendar; it does not
    /// resample one, so a shifted trading day can fall on a weekend or holiday.
    /// An offset that is a multiple of 7 keeps every weekday (364 days is 52
    /// weeks), which removes that tell for daily bars.
    pub fn date_shifted(&self, offset_days: i64) -> Result<Dataset, DateShiftError> {
        if offset_days == 0 {
            return Err(DateShiftError::ZeroOffset);
        }
        let dates = self
            .dates
            .iter()
            .enumerate()
            .map(|(step, date)| {
                let (day, rest) =
                    parse_iso_day(date).ok_or_else(|| DateShiftError::Unparseable {
                        step,
                        date: date.clone(),
                    })?;
                let shifted = day
                    .checked_add(offset_days)
                    .and_then(format_day)
                    .ok_or(DateShiftError::OutOfRange { step })?;
                Ok(format!("{shifted}{rest}"))
            })
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Dataset {
            dates,
            ..self.clone()
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn two_symbols() -> Dataset {
        Dataset::from_csv(
            "date,symbol,close\n2020-02-28,SPX,3000\n2020-02-28,DJI,25000\n\
             2020-02-29,SPX,3030\n2020-02-29,DJI,25500\n2020-03-01,SPX,2990\n2020-03-01,DJI,25100\n",
        )
        .unwrap()
    }

    #[test]
    fn full_level_is_the_masked_view_and_each_partial_level_hides_one_handle() {
        let data = two_symbols();
        let full = data.blinded_view(BlindingLevel::Full).unwrap();
        let masked = data.masked_view().unwrap();
        assert_eq!(full.symbols, masked.symbols);
        assert_eq!(full.dataset.dates, masked.dataset.dates);
        assert_eq!(full.dataset.closes, masked.dataset.closes);

        let tickers = data.blinded_view(BlindingLevel::TickersOnly).unwrap();
        assert_eq!(tickers.dataset.symbols(), ["ASSET_000", "ASSET_001"]);
        assert_eq!(tickers.dataset.dates, data.dates);
        assert_eq!(
            tickers.dataset.closes["ASSET_000"], data.closes["DJI"],
            "levels kept"
        );

        let dates = data.blinded_view(BlindingLevel::DatesOnly).unwrap();
        assert_eq!(dates.dataset.symbols(), data.symbols());
        assert_eq!(dates.dataset.dates, ["t0", "t1", "t2"]);
        assert_eq!(dates.dataset.closes, data.closes);

        let levels = data.blinded_view(BlindingLevel::LevelsOnly).unwrap();
        assert_eq!(levels.dataset.symbols(), data.symbols());
        assert_eq!(levels.dataset.dates, data.dates);
        assert_eq!(levels.dataset.closes["SPX"][0], 93.75, "3000 / 32");
        for view in [&tickers, &dates, &levels] {
            for m in &view.symbols {
                let (a, b) = (&view.dataset.closes[&m.opaque], &data.closes[&m.source]);
                for t in 1..a.len() {
                    assert_eq!(a[t] / a[t - 1], b[t] / b[t - 1], "returns are exact");
                }
            }
        }
    }

    #[test]
    fn a_date_shift_moves_every_day_across_month_and_leap_boundaries() {
        let data = two_symbols();
        let shifted = data.date_shifted(364).unwrap();
        assert_eq!(shifted.dates, ["2021-02-26", "2021-02-27", "2021-02-28"]);
        assert_eq!(shifted.closes, data.closes);
        let back = shifted.date_shifted(-364).unwrap();
        assert_eq!(back.dates, data.dates);
        assert_eq!(
            data.date_shifted(1).unwrap().dates,
            ["2020-02-29", "2020-03-01", "2020-03-02"]
        );
    }

    #[test]
    fn a_date_shift_refuses_what_it_cannot_shift() {
        let data = two_symbols();
        assert_eq!(
            data.date_shifted(0).unwrap_err(),
            DateShiftError::ZeroOffset
        );
        let synthetic = Dataset::synthetic(1, 3, 1);
        assert!(matches!(
            synthetic.date_shifted(7),
            Err(DateShiftError::Unparseable { step: 0, .. })
        ));
        assert_eq!(
            data.date_shifted(-800_000).unwrap_err(),
            DateShiftError::OutOfRange { step: 0 }
        );
        assert_eq!(
            data.date_shifted(i64::MAX).unwrap_err(),
            DateShiftError::OutOfRange { step: 0 }
        );
    }

    #[test]
    fn iso_days_parse_strictly() {
        assert_eq!(parse_iso_day("1970-01-01"), Some((0, "")));
        assert_eq!(parse_iso_day("2000-03-01T09:30"), Some((11_017, "T09:30")));
        assert_eq!(iso_year("2016-06-20"), Some(2016));
        for bad in [
            "2021-02-29",
            "2021-13-01",
            "2021-00-10",
            "2021-04-31",
            "2021-04-00",
            "2021/04/01",
            "2025-001",
            "0000-01-01",
            "2021-04-01x",
            "+021-04-01",
        ] {
            assert_eq!(parse_iso_day(bad), None, "{bad}");
        }
        assert!(parse_iso_day("2000-02-29").is_some());
        assert!(parse_iso_day("1900-02-29").is_none());
        // Round trip across four centuries, every day.
        for day in -200_000..-100_000i64 {
            let text = format_day(day).unwrap();
            assert_eq!(parse_iso_day(&text), Some((day, "")), "{text}");
        }
    }
}
