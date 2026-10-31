//! Is the frozen commodities file affected by mark-time misalignment under the
//! engine's actual timing? Yes. These tests show why, on the frozen file and
//! its declared marks.
//!
//! Engine timing (`engine.rs`): at step `t` the agent sees closes up to `t`
//! and, with no fixed decision delay, its orders fill at close `t`. Brent is
//! declared as marked at the London close and WTI at the New York settlement,
//! hours later on the same date label. So the agent decides after seeing WTI_t
//! and buys Brent at Brent_t, a price fixed before WTI_t existed.

use sharpebench_protocol::{Action, Decision, MarketObservation, Order};
use sharpebench_sim::{
    run_backtest, window_sharpe, Agent, CostModel, CostProfile, Dataset, FixedDecisionDelay,
    MarkSheet, MarkTiming, MarkTimingError, Window,
};

fn commodities() -> Dataset {
    let dir = format!("{}/../../data", env!("CARGO_MANIFEST_DIR"));
    let data = Dataset::from_csv_file(&format!("{dir}/commodities-1d.csv")).unwrap();
    let sheet = std::fs::read_to_string(format!("{dir}/commodities-1d.marks.json")).unwrap();
    data.with_marks(MarkSheet::from_json(&sheet).unwrap())
        .unwrap()
}

fn delayed(costs: CostModel, bars: usize) -> CostModel {
    CostModel {
        fixed_delay: Some(FixedDecisionDelay::Fifo { bars }),
        ..costs
    }
}

#[test]
fn commodities_are_refused_under_immediate_fills_and_accepted_one_bar_late() {
    let data = commodities();
    assert_eq!(
        data.check_mark_timing(&CostModel::default()),
        Err(MarkTimingError::Asynchronous {
            earlier: "BRENT".into(),
            later: "WTI".into(),
            delay_bars: 0,
        })
    );
    assert_eq!(
        data.check_mark_timing(&delayed(CostModel::default(), 1)),
        Ok(MarkTiming::SafeUnderDelay { bars: 1 })
    );
    // The masked view carries the declaration under the opaque ids.
    let masked = data.masked().unwrap();
    assert!(masked.check_mark_timing(&CostModel::default()).is_err());
}

/// Trades Brent on the gap between WTI's and Brent's latest one-bar moves:
/// when WTI moved more, Brent is expected to catch up on the next date label.
struct LeadLag;

fn last_return(h: &[f64]) -> Option<f64> {
    let (a, b) = (h[h.len().checked_sub(2)?], h[h.len() - 1]);
    (a > 0.0 && b > 0.0).then(|| b / a - 1.0)
}

impl Agent for LeadLag {
    fn decide(&mut self, obs: &MarketObservation) -> Decision {
        let get = |name: &str| {
            obs.symbols
                .iter()
                .find(|s| s.symbol == name)
                .and_then(|s| last_return(&s.close_history))
        };
        let w = match (get("WTI"), get("BRENT")) {
            (Some(wti), Some(brent)) if wti > brent => 1.0,
            (Some(_), Some(_)) => -1.0,
            _ => 0.0,
        };
        Decision {
            orders: vec![Order {
                symbol: "BRENT".to_string(),
                action: if w > 0.0 { Action::Buy } else { Action::Sell },
                target_weight: w,
                confidence: None,
                rationale: String::new(),
            }],
            reasoning: String::new(),
            cost: None,
        }
    }
}

#[test]
fn a_cross_leg_agent_harvests_the_stale_leg_only_under_immediate_fills() {
    let data = commodities();
    // From 2020-05-01, after the negative WTI close, so every return is a
    // return on a positive price.
    let start = data
        .dates
        .iter()
        .position(|d| d.as_str() >= "2020-05-01")
        .unwrap();
    let window = Window {
        start,
        end: data.len(),
    };
    let costs = CostProfile::None.resolve().costs;
    let now = run_backtest(&data, &mut LeadLag, window, 0, costs);
    let late = run_backtest(&data, &mut LeadLag, window, 0, delayed(costs, 1));
    let (s_now, s_late) = (
        window_sharpe(&now.returns).unwrap(),
        window_sharpe(&late.returns).unwrap(),
    );
    eprintln!("lead-lag per-bar Sharpe: immediate {s_now:.4}, one bar late {s_late:.4}");
    // Measured on the frozen file (1,544 bars): per-bar Sharpe 0.049
    // immediate (about 0.78 annualized, roughly two standard errors from
    // zero) against 0.001 one bar late. The edge exists only while Brent fills
    // at a mark fixed before the WTI close the decision read.
    assert!(s_now > 0.04, "immediate {s_now}");
    assert!(s_late < s_now / 3.0, "late {s_late} vs immediate {s_now}");
}
