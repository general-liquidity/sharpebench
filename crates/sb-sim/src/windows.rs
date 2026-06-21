//! Window generation: walk-forward OOS splits + coarse regime tagging.
//!
//! A single fixed window is the StockBench mistake — one lucky quarter. Real
//! evaluation rolls forward over many disjoint out-of-sample windows and reports
//! stability across regimes, so a bull-market fluke can't masquerade as skill.

use crate::data::Dataset;
use crate::engine::Window;

/// Generate disjoint-start walk-forward test windows of `test` length, stepping
/// by `step`, after a `warmup` burn-in (so features have history). Each window is
/// an out-of-sample slice `[start, start + test)`.
pub fn walk_forward(n_days: usize, warmup: usize, test: usize, step: usize) -> Vec<Window> {
    let mut windows = Vec::new();
    if test == 0 || step == 0 {
        return windows;
    }
    let mut start = warmup;
    while start + test <= n_days {
        windows.push(Window {
            start,
            end: start + test,
        });
        start += step;
    }
    windows
}

/// Coarse market regime over a window.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Regime {
    Bull,
    Bear,
    Chop,
}

/// Tag a window's regime by the equal-weight average total return across symbols
/// (>+3% = bull, < -3% = bear, else chop).
pub fn tag_regime(data: &Dataset, window: Window) -> Regime {
    let end = window.end.min(data.len());
    if window.start + 1 >= end {
        return Regime::Chop;
    }
    let mut total = 0.0;
    let mut count = 0.0;
    for sym in data.symbols() {
        if let (Some(a), Some(b)) = (
            data.close_at(&sym, window.start),
            data.close_at(&sym, end - 1),
        ) {
            if a > 0.0 {
                total += b / a - 1.0;
                count += 1.0;
            }
        }
    }
    if count == 0.0 {
        return Regime::Chop;
    }
    let avg = total / count;
    if avg > 0.03 {
        Regime::Bull
    } else if avg < -0.03 {
        Regime::Bear
    } else {
        Regime::Chop
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn walk_forward_disjoint_and_stepped() {
        let ws = walk_forward(200, 20, 60, 60);
        assert_eq!(ws.len(), 3);
        assert_eq!(ws[0].start, 20);
        assert_eq!(ws[1].start, 80);
        assert!(ws.iter().all(|w| w.end - w.start == 60));
    }

    #[test]
    fn regime_classifies_trends() {
        use std::collections::BTreeMap;
        let mk = |series: Vec<f64>| {
            let mut closes = BTreeMap::new();
            closes.insert("A".to_string(), series);
            Dataset {
                dates: (0..50).map(|i| format!("d{i}")).collect(),
                closes,
            }
        };
        let up: Vec<f64> = (0..50).map(|i| 100.0 * (1.0 + 0.01 * i as f64)).collect();
        let down: Vec<f64> = (0..50).map(|i| 100.0 * (1.0 - 0.005 * i as f64)).collect();
        let flat: Vec<f64> = (0..50).map(|i| 100.0 + 0.001 * (i as f64).sin()).collect();
        let w = Window { start: 0, end: 50 };
        assert_eq!(tag_regime(&mk(up), w), Regime::Bull);
        assert_eq!(tag_regime(&mk(down), w), Regime::Bear);
        assert_eq!(tag_regime(&mk(flat), w), Regime::Chop);
    }
}
