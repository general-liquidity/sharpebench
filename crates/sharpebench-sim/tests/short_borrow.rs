//! The opt-in short borrow carry (`CostModel::short_borrow_bps`).
//!
//! Leverage financing only reaches gross exposure above 1x NAV, so before the
//! borrow rate existed an unlevered short book carried for free under every
//! cost profile. These tests pin the three halves of the fix: a short book at
//! or below 1x gross pays borrow once the rate is set, the charge falls on the
//! short notional only, and a book with no short exposure replays bit for bit
//! whatever the rate.

use sharpebench_core::Run;
use sharpebench_protocol::{Action, Decision, MarketObservation, Order};
use sharpebench_sim::{
    run_backtest, Agent, BuyAndHold, CostModel, CostProfile, Dataset, Momentum, Window,
};

/// Every order re-targets the same signed weights at each bar.
struct Fixed(Vec<f64>);

impl Agent for Fixed {
    fn decide(&mut self, obs: &MarketObservation) -> Decision {
        Decision {
            orders: obs
                .symbols
                .iter()
                .zip(&self.0)
                .map(|(snapshot, weight)| Order {
                    symbol: snapshot.symbol.clone(),
                    action: if *weight < 0.0 {
                        Action::Sell
                    } else {
                        Action::Buy
                    },
                    target_weight: *weight,
                    confidence: Some(0.5),
                    rationale: String::new(),
                })
                .collect(),
            reasoning: "fixed signed book".to_string(),
            cost: None,
        }
    }
}

/// Unit prices that never move, so every NAV and exposure below is exact and
/// the only thing that can change a return is a cost.
fn flat(symbols: &[&str], bars: usize) -> Dataset {
    let mut csv = String::from("date,symbol,close\n");
    for bar in 0..bars {
        for symbol in symbols {
            csv.push_str(&format!("t{bar:02},{symbol},1\n"));
        }
    }
    Dataset::from_csv(&csv).unwrap()
}

/// No fees, slippage or impact; leverage financing at 100 bps per step so a
/// test also shows it charges nothing at or below 1x gross.
fn frictionless_with(short_borrow_bps: f64) -> CostModel {
    CostModel {
        fee_bps: 0.0,
        slippage_bps: 0.0,
        impact_bps: 0.0,
        financing_bps: 100.0,
        short_borrow_bps,
        ..CostModel::default()
    }
}

fn run(data: &Dataset, weights: &[f64], costs: CostModel) -> Run {
    run_backtest(
        data,
        &mut Fixed(weights.to_vec()),
        Window {
            start: 0,
            end: data.len(),
        },
        3,
        costs,
    )
}

#[test]
fn an_unlevered_short_book_pays_borrow_once_the_rate_is_set() {
    let data = flat(&["AAA"], 6);
    for short in [-0.5, -1.0] {
        let free = run(&data, &[short], frictionless_with(0.0));
        assert!(
            free.returns.iter().all(|r| *r == 0.0),
            "gross {} is not leveraged, so without a borrow rate it carries free: {:?}",
            -short,
            free.returns
        );
        let charged = run(&data, &[short], frictionless_with(100.0));
        assert_eq!(charged.returns.len(), 6);
        // 100 bps per step on a short worth -short of NAV, every bar.
        let expected = -0.01 * -short;
        for r in &charged.returns {
            assert!(
                (r - expected).abs() < 1e-12,
                "short {short}: expected {expected} per step, got {:?}",
                charged.returns
            );
        }
    }
}

#[test]
fn borrow_is_charged_on_the_short_leg_beside_leverage_financing() {
    // Long 1x and short 1x: gross 2x pays one 100 bp financing charge on the
    // leveraged 1x, and borrow falls on the 1x short leg only, not on gross.
    let data = flat(&["AAA", "BBB"], 3);
    let financing_only = run(&data, &[1.0, -1.0], frictionless_with(0.0));
    let both = run(&data, &[1.0, -1.0], frictionless_with(50.0));
    assert!((financing_only.returns[0] + 0.01).abs() < 1e-12);
    assert!(
        (both.returns[0] + 0.015).abs() < 1e-12,
        "financing 0.01 plus borrow 0.005 on the 1x short leg: {}",
        both.returns[0]
    );
}

type AgentFactory = Box<dyn Fn() -> Box<dyn Agent>>;

fn bits(run: &Run) -> String {
    serde_json::to_string(run).unwrap()
}

#[test]
fn a_book_without_shorts_replays_bit_for_bit_whatever_the_rate() {
    let data = Dataset::synthetic(4, 120, 11);
    let window = Window {
        start: 20,
        end: 120,
    };
    for base in [
        CostModel::default(),
        CostProfile::WorstCase.resolve().costs,
        CostProfile::Realistic.resolve().costs,
    ] {
        let borrowed = CostModel {
            short_borrow_bps: 50.0,
            ..base
        };
        // A negative-zero rate is no rate, even for a book that shorts.
        let negative_zero = CostModel {
            short_borrow_bps: -0.0,
            ..base
        };
        let short = |costs| {
            run_backtest(
                &data,
                &mut Fixed(vec![-1.0, 0.5, -0.5, 0.0]),
                window,
                3,
                costs,
            )
        };
        assert_eq!(bits(&short(base)), bits(&short(negative_zero)));
        for seed in [1u64, 7] {
            let agents: [(&str, AgentFactory); 3] = [
                ("buy-and-hold", Box::new(|| Box::new(BuyAndHold))),
                ("momentum", Box::new(|| Box::new(Momentum::default()))),
                ("levered long", Box::new(|| Box::new(Fixed(vec![2.0; 4])))),
            ];
            for (name, make) in agents {
                let a = run_backtest(&data, make().as_mut(), window, seed, base);
                let b = run_backtest(&data, make().as_mut(), window, seed, borrowed);
                let a_bits: Vec<u64> = a.returns.iter().map(|r| r.to_bits()).collect();
                let b_bits: Vec<u64> = b.returns.iter().map(|r| r.to_bits()).collect();
                assert_eq!(a_bits, b_bits, "{name}, seed {seed}");
                assert_eq!(bits(&a), bits(&b), "{name}, seed {seed}");
            }
        }
    }
}

#[test]
#[should_panic(expected = "short_borrow_bps must be finite and >= 0")]
fn a_negative_borrow_rate_is_refused_before_the_run() {
    let data = flat(&["AAA"], 3);
    run(&data, &[-0.5], frictionless_with(-1.0));
}
