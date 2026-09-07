//! Strict readers for the CLI's unquoted, comma-separated analysis tables.
//! Never drop a row or cell: independently compacted columns are not aligned
//! observations, even when the resulting vectors have the same length.

use std::collections::BTreeSet;

struct Table<'a> {
    rows: Vec<Vec<&'a str>>,
}

impl<'a> Table<'a> {
    fn parse(text: &'a str) -> Result<Self, String> {
        if text.contains('"') {
            return Err("quoted CSV fields are unsupported; use an unquoted analysis table".into());
        }
        let rows: Vec<Vec<&str>> = text
            .lines()
            .map(|line| line.split(',').map(str::trim).collect())
            .collect();
        let width = rows.first().ok_or("empty file")?.len();
        for (i, row) in rows.iter().enumerate() {
            if row.iter().all(|cell| cell.is_empty()) {
                return Err(format!("row {}: blank observation", i + 1));
            }
            if row.len() != width {
                return Err(format!(
                    "row {}: expected {width} columns, got {}",
                    i + 1,
                    row.len()
                ));
            }
        }
        Ok(Self { rows })
    }

    fn header(&self) -> Result<(), String> {
        let mut seen = BTreeSet::new();
        for name in &self.rows[0] {
            if name.is_empty() || !seen.insert(*name) {
                return Err(format!("header has an empty or duplicate column `{name}`"));
            }
        }
        Ok(())
    }

    fn column(&self, name: &str) -> Result<usize, String> {
        self.rows[0]
            .iter()
            .position(|h| *h == name)
            .ok_or_else(|| format!("column `{name}` not found in header"))
    }

    fn strings(&self, index: usize, header: bool) -> Result<Vec<String>, String> {
        let start = usize::from(header);
        if self.rows.len() == start {
            return Err("no data rows".into());
        }
        self.rows
            .iter()
            .enumerate()
            .skip(start)
            .map(|(i, row)| {
                if row[index].is_empty() {
                    Err(format!("row {}: empty cell in column {}", i + 1, index + 1))
                } else {
                    Ok(row[index].to_string())
                }
            })
            .collect()
    }
}

fn numbers(values: Vec<String>) -> Result<Vec<f64>, String> {
    values
        .into_iter()
        .enumerate()
        .map(|(i, value)| {
            value
                .parse::<f64>()
                .ok()
                .filter(|v| v.is_finite())
                .ok_or_else(|| {
                    format!(
                        "observation {}: expected a finite number, got `{value}`",
                        i + 1
                    )
                })
        })
        .collect()
}

struct Series<T> {
    values: Vec<T>,
    periods: Option<Vec<String>>,
}

fn series(
    text: &str,
    col: Option<&str>,
    period_col: Option<&str>,
    numeric: bool,
) -> Result<Series<String>, String> {
    let table = Table::parse(text)?;
    let first = table.rows[0][0];
    let header = col.is_some()
        || period_col.is_some()
        || if numeric {
            !first.is_empty() && first.parse::<f64>().is_err()
        } else {
            first.eq_ignore_ascii_case("regime") || first.eq_ignore_ascii_case("label")
        };
    if header {
        table.header()?;
    }
    let index = col.map(|name| table.column(name)).transpose()?.unwrap_or(0);
    let values = table.strings(index, header)?;
    let periods = period_col
        .map(|name| {
            let period_index = table.column(name)?;
            if period_index == index {
                return Err("period and value columns must be distinct".into());
            }
            let ids = table.strings(period_index, true)?;
            let mut seen = BTreeSet::new();
            for id in &ids {
                if !seen.insert(id) {
                    return Err(format!("duplicate period identity `{id}`"));
                }
            }
            Ok(ids)
        })
        .transpose()?;
    Ok(Series { values, periods })
}

pub fn read_returns_column(text: &str, col: Option<&str>) -> Result<Vec<f64>, String> {
    numbers(series(text, col, None, true)?.values)
}

pub fn read_all_numeric_columns(text: &str) -> Result<Vec<(String, Vec<f64>)>, String> {
    let table = Table::parse(text)?;
    let header = table.rows[0]
        .iter()
        .all(|c| !c.is_empty() && c.parse::<f64>().is_err());
    if header {
        table.header()?;
    }
    (0..table.rows[0].len())
        .map(|i| {
            let name = if header {
                table.rows[0][i].to_string()
            } else {
                format!("col{i}")
            };
            Ok((name, numbers(table.strings(i, header)?)?))
        })
        .collect()
}

#[derive(Debug)]
pub struct RegimeInputs {
    pub a: Vec<f64>,
    pub b: Vec<f64>,
    pub labels: Vec<String>,
}

/// With explicit period IDs, require the same unique ordered IDs in all three
/// tables. Without IDs the caller asserts row alignment; only complete, equal
/// row counts are checked. Neither mode truncates or joins an intersection.
pub fn read_regime_inputs(
    a: &str,
    b: &str,
    labels: &str,
    returns_col: Option<&str>,
    labels_col: Option<&str>,
    period_col: Option<&str>,
) -> Result<RegimeInputs, String> {
    let a = series(a, returns_col, period_col, true)?;
    let b = series(b, returns_col, period_col, true)?;
    let labels = series(labels, labels_col, period_col, false)?;
    if a.values.len() != b.values.len() || a.values.len() != labels.values.len() {
        return Err(format!(
            "aligned support required: a={} b={} regimes={}",
            a.values.len(),
            b.values.len(),
            labels.values.len()
        ));
    }
    if a.periods != b.periods || a.periods != labels.periods {
        return Err("period identities or their order differ between the three tables".into());
    }
    Ok(RegimeInputs {
        a: numbers(a.values)?,
        b: numbers(b.values)?,
        labels: labels.values,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn complete_numeric_columns_keep_every_observation() {
        assert_eq!(
            read_all_numeric_columns("a,b\r\n1,2\r\n3,4\r\n").unwrap(),
            vec![("a".into(), vec![1.0, 3.0]), ("b".into(), vec![2.0, 4.0])]
        );
        assert_eq!(
            read_returns_column("date,ret\nt0,1\nt1,2\n", Some("ret")).unwrap(),
            vec![1.0, 2.0]
        );
    }

    #[test]
    fn missing_cells_cannot_be_compacted_into_false_alignment() {
        // Both old columns became length two, but their second observations
        // came from different rows. Equal output lengths would prove nothing.
        assert!(read_all_numeric_columns("a,b\n1,2\n,3\n4,\n").is_err());
        for text in [
            "ret\n1\n\n2\n",
            "ret,x\n1,a\n,b\n",
            "ret,x\n1,a\n2\n",
            "ret\n1\n2,\n",
        ] {
            assert!(read_returns_column(text, Some("ret")).is_err(), "{text:?}");
        }
    }

    #[test]
    fn malformed_numeric_tables_fail_without_partial_results() {
        for text in [
            "",
            "a,b\n",
            "a,a\n1,2\n",
            "a,b\n1,2,3\n",
            "a,b\n1\n",
            "a,b\nNaN,1\n",
            "a,b\n1,inf\n",
            "a,b\n1,1e999\n",
            "\"a\",b\n1,2\n",
        ] {
            assert!(read_all_numeric_columns(text).is_err(), "{text:?}");
        }
        assert!(read_returns_column("ret\n1\n", Some("missing"))
            .unwrap_err()
            .contains("not found"));
    }

    #[test]
    fn regime_columns_are_independent_and_support_is_exact() {
        let a = "period,ret\nt0,1\nt1,2\n";
        let labels = "period,regime\nt0,calm\nt1,stress\n";
        let parsed =
            read_regime_inputs(a, a, labels, Some("ret"), Some("regime"), Some("period")).unwrap();
        assert_eq!(parsed.a, vec![1.0, 2.0]);
        assert_eq!(parsed.labels, vec!["calm", "stress"]);
        assert!(
            read_regime_inputs(a, a, labels, Some("ret"), Some("wrong"), Some("period"))
                .unwrap_err()
                .contains("not found")
        );
    }

    #[test]
    fn regime_rejects_missing_duplicate_and_reordered_periods() {
        let a = "period,ret\nt0,1\nt1,2\n";
        for labels in [
            "period,regime\nt0,calm\n",
            "period,regime\nt0,calm\nt0,stress\n",
            "period,regime\nt1,stress\nt0,calm\n",
            "period,regime\nt0,calm\nt2,stress\n",
        ] {
            assert!(
                read_regime_inputs(a, a, labels, Some("ret"), Some("regime"), Some("period"))
                    .is_err()
            );
        }
        assert!(read_regime_inputs("1\n2\n", "1\n", "calm\nstress\n", None, None, None).is_err());
        let parsed = read_regime_inputs(
            "1\n2\n",
            "3\n4\n",
            "regime\ncalm\nstress\n",
            None,
            None,
            None,
        )
        .unwrap();
        assert_eq!(parsed.labels, vec!["calm", "stress"]);
    }
}
