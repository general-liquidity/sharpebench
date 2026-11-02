//! A declared training cutoff, published as a disclosure and nothing more.
//!
//! An operator may say when an entrant's model stopped seeing data. The host
//! cannot check that statement without model calls.
//! After Gao, Jiang and Yan's Lookahead Propensity (arXiv:2512.23847), a cutoff
//! is checked by a date-only recall query whose answer collapses right after
//! the true cutoff, never by declaration; Lopez-Lira, Tang and Zhu
//! (arXiv:2504.14765) likewise find recall of exact values before a cutoff and
//! none after. This build makes no model calls, so the declaration is never
//! trusted: it does not change [`crate::evidence_mode`], a score, a rank, the
//! gate or certification. Verifying it is future work that needs model calls.
//! After He, Lv, Manela and Wu's chronologically consistent models
//! (arXiv:2502.21206), the only cutoff a host could rely on is one that holds by
//! construction, a model trained on point-in-time text; a declared one is a
//! claim about weights the host never saw.
//!
//! What it does provide is a stratification for a later study: each scored
//! window is classed as ending before the declared day, starting on or after
//! it, straddling it, or unclassifiable (a dataset whose dates are not calendar
//! days). The classes read the cutoff as an information boundary
//! (after Luo, Hong, Li and Xie's FORESIGHT-9, arXiv:2608.29372, which evaluates
//! agents only after a common information boundary): a window that starts on or
//! after it would be prospective for the model if the declaration were true,
//! and one that ends before it is retrospective. Pure: no I/O, no clock.

use sharpebench_sim::{parse_iso_day, Dataset, Window};

/// Schema of the published object.
pub const DECLARED_CUTOFF_VERSION: &str = "sharpebench.declared-training-cutoff.v1";

/// A cutoff day that parsed as a real `YYYY-MM-DD` calendar day.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DeclaredCutoff {
    text: String,
    day: i64,
}

impl DeclaredCutoff {
    /// Accept exactly `YYYY-MM-DD` naming a real calendar day.
    pub fn parse(text: &str) -> Result<Self, String> {
        match parse_iso_day(text) {
            Some((day, "")) => Ok(Self {
                text: text.to_string(),
                day,
            }),
            _ => Err(format!(
                "a declared training cutoff must be a calendar day YYYY-MM-DD, got `{text}`"
            )),
        }
    }

    pub fn as_str(&self) -> &str {
        &self.text
    }
}

/// The disclosure for one entrant over the windows it was scored on.
pub fn disclosure(
    cutoff: &DeclaredCutoff,
    data: &Dataset,
    windows: &[Window],
) -> serde_json::Value {
    let (mut before, mut after, mut straddling, mut unclassified) = (0, 0, 0, 0);
    for w in windows {
        let day = |step: usize| {
            data.dates
                .get(step)
                .and_then(|d| parse_iso_day(d))
                .map(|p| p.0)
        };
        match (day(w.start), w.end.checked_sub(1).and_then(day)) {
            (Some(_), Some(last)) if last < cutoff.day => before += 1,
            (Some(first), Some(_)) if first >= cutoff.day => after += 1,
            (Some(_), Some(_)) => straddling += 1,
            _ => unclassified += 1,
        }
    }
    serde_json::json!({
        "schema_version": DECLARED_CUTOFF_VERSION,
        "declared": cutoff.as_str(),
        "declared_by": "operator",
        "trusted": false,
        "verified": false,
        "verification": "not performed: verifying a training cutoff needs recall queries to the model, which this build does not make",
        "changes_evidence_mode": false,
        "windows": {
            "ending_before_cutoff": before,
            "starting_on_or_after_cutoff": after,
            "straddling_cutoff": straddling,
            "unclassifiable": unclassified,
        },
        "rank_neutral": true,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dated(n: usize) -> Dataset {
        let mut csv = String::from("date,symbol,close\n");
        for d in 0..n {
            // One bar a day from 2024-01-01.
            let (m, day) = if d < 31 { (1, d + 1) } else { (2, d - 30) };
            csv.push_str(&format!("2024-{m:02}-{day:02},A,{}\n", 100 + d));
        }
        Dataset::from_csv(&csv).unwrap()
    }

    #[test]
    fn windows_are_classed_against_the_declared_day() {
        let data = dated(50);
        let cutoff = DeclaredCutoff::parse("2024-01-20").unwrap();
        let windows = [
            Window { start: 0, end: 19 },  // ends 2024-01-19
            Window { start: 0, end: 20 },  // ends on the cutoff day
            Window { start: 19, end: 30 }, // starts on the cutoff day
            Window { start: 25, end: 50 },
        ];
        let value = disclosure(&cutoff, &data, &windows);
        assert_eq!(value["schema_version"], DECLARED_CUTOFF_VERSION);
        assert_eq!(value["declared"], "2024-01-20");
        assert_eq!(value["trusted"], false);
        assert_eq!(value["verified"], false);
        assert_eq!(value["changes_evidence_mode"], false);
        assert_eq!(value["rank_neutral"], true);
        assert_eq!(value["windows"]["ending_before_cutoff"], 1);
        assert_eq!(value["windows"]["straddling_cutoff"], 1);
        assert_eq!(value["windows"]["starting_on_or_after_cutoff"], 2);
        assert_eq!(value["windows"]["unclassifiable"], 0);
    }

    #[test]
    fn undated_data_and_bad_declarations_are_not_guessed() {
        let synthetic = Dataset::synthetic(1, 30, 1);
        let cutoff = DeclaredCutoff::parse("2025-01-01").unwrap();
        let value = disclosure(&cutoff, &synthetic, &[Window { start: 0, end: 10 }]);
        assert_eq!(value["windows"]["unclassifiable"], 1);
        let empty = disclosure(&cutoff, &synthetic, &[Window { start: 0, end: 0 }]);
        assert_eq!(empty["windows"]["unclassifiable"], 1);
        for bad in ["2025-02-30", "2025-01-01T00:00", "January 2025", ""] {
            assert!(DeclaredCutoff::parse(bad).is_err(), "{bad}");
        }
    }
}
