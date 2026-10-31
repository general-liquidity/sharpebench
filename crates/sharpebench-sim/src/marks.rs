//! Declared mark times, and the refusal to step legs marked at different times
//! under an execution timing that would let one leg fill at a stale price.
//!
//! A [`Dataset`] aligns its symbols on the date label alone: bar `t` holds every
//! symbol's close for that date. When two symbols are marked at different times
//! of day (Brent assessed at the London close, WTI at the New York settlement),
//! the engine still hands the agent both closes at bar `t` and, under immediate
//! execution, fills both at those closes. The agent therefore decides after the
//! later mark and trades the earlier leg at a price that was fixed before
//! information it has already seen. A cross-leg agent can harvest that lead-lag,
//! and no trader could have executed it.
//!
//! The engine's timing, read from `engine.rs`: at step `t` the observation ends
//! at close `t` (`build_observation`), the decision is taken on it, and under the
//! default cost model (`fixed_delay: None`) every order fills at close `t`
//! (`apply_order` prices at `price(data, symbol, t)`). Under
//! [`FixedDecisionDelay::Fifo`] with `bars = d`, the decision fills at close
//! `t + d` instead.
//!
//! The rule enforced here: the decision at bar `t` is taken no earlier than the
//! latest mark of bar `t`; each leg must fill at a price marked strictly after
//! that instant, unless every leg is marked at the same declared instant, in
//! which case the close-fill convention is the engine's standard one.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::costs::{CostModel, FixedDecisionDelay};
use crate::data::Dataset;

/// The schema tag every mark sheet must carry.
pub const MARK_SHEET_SCHEMA: &str = "sharpebench.mark-sheet.v1";

/// Minutes in one day, the upper bound on a declared time of day.
const DAY_MINUTES: u32 = 1440;

/// Per-symbol mark times for one dataset.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MarkSheet {
    pub schema_version: String,
    pub symbols: BTreeMap<String, SymbolMark>,
}

/// When one symbol's close is marked, as a range of times of day in minutes
/// after 00:00 UTC on the bar's date label. A range rather than an instant,
/// because a venue on local time moves against UTC with daylight saving.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SymbolMark {
    pub venue: String,
    /// Where the declaration comes from, in words.
    pub basis: String,
    pub utc_minutes_earliest: u32,
    pub utc_minutes_latest: u32,
}

impl MarkSheet {
    /// Parse and validate a sheet from its JSON text.
    pub fn from_json(text: &str) -> Result<MarkSheet, String> {
        let sheet: MarkSheet =
            serde_json::from_str(text).map_err(|e| format!("mark-time sheet: {e}"))?;
        sheet.validate()?;
        Ok(sheet)
    }

    /// Refuse an unknown schema, an empty sheet, a blank venue, or a range that
    /// is reversed or leaves the day.
    pub fn validate(&self) -> Result<(), String> {
        if self.schema_version != MARK_SHEET_SCHEMA {
            return Err(format!(
                "mark-time sheet schema `{}` is not `{MARK_SHEET_SCHEMA}`",
                self.schema_version
            ));
        }
        if self.symbols.is_empty() {
            return Err("mark-time sheet names no symbol".to_string());
        }
        for (symbol, mark) in &self.symbols {
            if mark.venue.trim().is_empty() {
                return Err(format!("mark-time sheet: {symbol} has no venue"));
            }
            if mark.utc_minutes_earliest > mark.utc_minutes_latest
                || mark.utc_minutes_latest >= DAY_MINUTES
            {
                return Err(format!(
                    "mark-time sheet: {symbol} range [{}, {}] is not a time of day in minutes",
                    mark.utc_minutes_earliest, mark.utc_minutes_latest
                ));
            }
        }
        Ok(())
    }
}

/// What [`Dataset::check_mark_timing`] established.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MarkTiming {
    /// No sheet was declared. Nothing is known, and nothing is refused, which
    /// is the legacy behaviour of every dataset without a sheet.
    Undeclared,
    /// Every symbol carries the same declared mark range.
    Synchronous,
    /// The marks differ, and the fixed decision delay of `bars` puts every
    /// fill after the latest mark the decision could have seen.
    SafeUnderDelay { bars: usize },
}

/// Why a dataset cannot be stepped under the given execution timing.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MarkTimingError {
    /// `earlier` is marked before `later`, and with a decision delay of
    /// `delay_bars` the earlier leg would fill at a price marked before the
    /// decision was taken.
    Asynchronous {
        earlier: String,
        later: String,
        delay_bars: usize,
    },
}

impl std::fmt::Display for MarkTimingError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MarkTimingError::Asynchronous {
                earlier,
                later,
                delay_bars,
            } => write!(
                f,
                "{earlier} is marked before {later} on the same date label; with a decision delay of \
                 {delay_bars} bar(s) a decision taken after {later}'s mark would fill {earlier} at a \
                 price marked before it. Declare a fixed decision delay of at least one bar \
                 (--fill-delay-bars 1) or drop one leg"
            ),
        }
    }
}

impl std::error::Error for MarkTimingError {}

impl Dataset {
    /// Check the declared marks against the execution timing in `costs`.
    ///
    /// With a sheet whose ranges differ, a fill at bar `t + d` for leg `L` is
    /// marked no earlier than `d` calendar days (date labels strictly increase)
    /// plus `L`'s earliest time of day after the start of bar `t`'s date, and
    /// the decision is taken no earlier than the latest mark at bar `t`. Every
    /// fill is after the decision when `d >= 1` and
    /// `min earliest + 1440 * d > max latest`. With `d = 0` the earlier leg
    /// fills at a price fixed before the decision, so it is refused.
    pub fn check_mark_timing(&self, costs: &CostModel) -> Result<MarkTiming, MarkTimingError> {
        let Some(sheet) = &self.marks else {
            return Ok(MarkTiming::Undeclared);
        };
        let ranges: Vec<(&String, u32, u32)> = sheet
            .symbols
            .iter()
            .map(|(s, m)| (s, m.utc_minutes_earliest, m.utc_minutes_latest))
            .collect();
        let first = (ranges[0].1, ranges[0].2);
        if ranges.iter().all(|(_, e, l)| (*e, *l) == first) {
            return Ok(MarkTiming::Synchronous);
        }
        let delay = match costs.fixed_delay {
            None => 0,
            Some(FixedDecisionDelay::Fifo { bars }) => bars,
        };
        // inv: `earlier` holds the range with the smallest earliest time seen,
        // `later` the one with the largest latest time seen.
        let earlier = ranges.iter().min_by_key(|(_, e, _)| *e).unwrap();
        let later = ranges.iter().max_by_key(|(_, _, l)| *l).unwrap();
        let after_decision = (delay as u64)
            .checked_mul(DAY_MINUTES as u64)
            .map(|shift| earlier.1 as u64 + shift > later.2 as u64)
            .unwrap_or(true);
        if delay >= 1 && after_decision {
            Ok(MarkTiming::SafeUnderDelay { bars: delay })
        } else {
            Err(MarkTimingError::Asynchronous {
                earlier: earlier.0.clone(),
                later: later.0.clone(),
                delay_bars: delay,
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mark(venue: &str, earliest: u32, latest: u32) -> SymbolMark {
        SymbolMark {
            venue: venue.to_string(),
            basis: "test".to_string(),
            utc_minutes_earliest: earliest,
            utc_minutes_latest: latest,
        }
    }

    fn two_legs(a: SymbolMark, b: SymbolMark) -> Dataset {
        let data = Dataset::from_csv(
            "date,symbol,close\n2025-01-01,A,10\n2025-01-01,B,20\n2025-01-02,A,11\n2025-01-02,B,21\n",
        )
        .unwrap();
        data.with_marks(MarkSheet {
            schema_version: MARK_SHEET_SCHEMA.to_string(),
            symbols: BTreeMap::from([("A".to_string(), a), ("B".to_string(), b)]),
        })
        .unwrap()
    }

    fn delayed(bars: usize) -> CostModel {
        CostModel {
            fixed_delay: Some(FixedDecisionDelay::Fifo { bars }),
            ..CostModel::default()
        }
    }

    #[test]
    fn asynchronous_marks_refuse_immediate_fills_and_accept_a_one_bar_delay() {
        let data = two_legs(mark("London", 930, 990), mark("New York", 1110, 1170));
        assert_eq!(
            data.check_mark_timing(&CostModel::default()),
            Err(MarkTimingError::Asynchronous {
                earlier: "A".into(),
                later: "B".into(),
                delay_bars: 0
            })
        );
        assert_eq!(
            data.check_mark_timing(&delayed(1)),
            Ok(MarkTiming::SafeUnderDelay { bars: 1 })
        );
    }

    #[test]
    fn identical_declared_marks_are_synchronous_and_absent_marks_are_undeclared() {
        let same = two_legs(mark("X", 1260, 1260), mark("Y", 1260, 1260));
        assert_eq!(
            same.check_mark_timing(&CostModel::default()),
            Ok(MarkTiming::Synchronous)
        );
        let plain = Dataset::synthetic(2, 5, 1);
        assert_eq!(
            plain.check_mark_timing(&CostModel::default()),
            Ok(MarkTiming::Undeclared)
        );
    }

    #[test]
    fn sheet_must_name_exactly_the_datasets_symbols_and_stay_inside_the_day() {
        let data = Dataset::synthetic(2, 5, 1);
        let one = MarkSheet {
            schema_version: MARK_SHEET_SCHEMA.into(),
            symbols: BTreeMap::from([("SYM00".to_string(), mark("X", 0, 10))]),
        };
        assert!(data.clone().with_marks(one).is_err());
        let reversed = MarkSheet {
            schema_version: MARK_SHEET_SCHEMA.into(),
            symbols: BTreeMap::from([
                ("SYM00".to_string(), mark("X", 20, 10)),
                ("SYM01".to_string(), mark("X", 0, 10)),
            ]),
        };
        assert!(data.clone().with_marks(reversed).is_err());
        let past_midnight = MarkSheet {
            schema_version: MARK_SHEET_SCHEMA.into(),
            symbols: BTreeMap::from([
                ("SYM00".to_string(), mark("X", 0, 1440)),
                ("SYM01".to_string(), mark("X", 0, 10)),
            ]),
        };
        assert!(data.with_marks(past_midnight).is_err());
        assert!(MarkSheet::from_json(
            r#"{"schema_version":"sharpebench.mark-sheet.v1","symbols":{},"extra":1}"#
        )
        .is_err());
    }

    #[test]
    fn a_declared_sheet_moves_the_dataset_identity_and_an_absent_one_does_not() {
        let plain = Dataset::synthetic(2, 5, 1);
        let before = serde_json::to_string(&plain).unwrap();
        assert!(!before.contains("marks"), "{before}");
        let marked = plain
            .with_marks(MarkSheet {
                schema_version: MARK_SHEET_SCHEMA.into(),
                symbols: BTreeMap::from([
                    ("SYM00".to_string(), mark("X", 0, 10)),
                    ("SYM01".to_string(), mark("Y", 20, 30)),
                ]),
            })
            .unwrap();
        assert!(serde_json::to_string(&marked).unwrap().contains("marks"));
    }
}
