//! Offline validation of the presentation-stability report with planted
//! agents. No model is called.

use sharpebench_protocol::{Action, Decision, MarketObservation, Order};
use sharpebench_sim::{
    presentation_report, Agent, BuyAndHold, CostProfile, Dataset, Momentum, Window,
};

fn frozen(name: &str) -> Dataset {
    let path = format!("{}/../../data/{name}", env!("CARGO_MANIFEST_DIR"));
    Dataset::from_csv_file(&path).unwrap()
}

fn decide_by(obs: &MarketObservation, pick: impl Fn(usize, &str, f64) -> f64) -> Decision {
    Decision {
        orders: obs
            .symbols
            .iter()
            .enumerate()
            .map(|(i, s)| {
                let w = pick(i, &s.symbol, *s.close_history.last().unwrap());
                Order {
                    symbol: s.symbol.clone(),
                    action: if w > 0.0 { Action::Buy } else { Action::Close },
                    target_weight: w,
                    confidence: None,
                    rationale: String::new(),
                }
            })
            .collect(),
        reasoning: String::new(),
        cost: None,
    }
}

/// Position bias: always buys whatever is listed first.
struct FirstListed;
impl Agent for FirstListed {
    fn decide(&mut self, obs: &MarketObservation) -> Decision {
        decide_by(obs, |i, _, _| if i == 0 { 1.0 } else { 0.0 })
    }
}

/// Recalls a ticker: buys SPX when it can see the name, else holds evenly.
struct TickerRecall;
impl Agent for TickerRecall {
    fn decide(&mut self, obs: &MarketObservation) -> Decision {
        let named = obs.symbols.iter().any(|s| s.symbol == "SPX");
        let n = obs.symbols.len() as f64;
        decide_by(obs, |_, sym, _| match (named, sym == "SPX") {
            (true, true) => 1.0,
            (true, false) => 0.0,
            (false, _) => 1.0 / n,
        })
    }
}

/// Reads the price level: buys anything quoted above 1000.
struct LevelReader;
impl Agent for LevelReader {
    fn decide(&mut self, obs: &MarketObservation) -> Decision {
        decide_by(obs, |_, _, px| if px > 1000.0 { 0.3 } else { 0.0 })
    }
}

fn report(make: &mut dyn FnMut() -> Box<dyn Agent>) -> sharpebench_sim::PresentationReport {
    let data = frozen("us-indices-1d.csv");
    let costs = CostProfile::None.resolve().costs;
    presentation_report(
        &data,
        Window {
            start: 30,
            end: 230,
        },
        0,
        costs,
        make,
    )
    .unwrap()
}

#[test]
fn observation_only_agents_decide_identically_in_every_presentation() {
    for (name, mut make) in [
        (
            "momentum",
            Box::new(|| Box::new(Momentum::default()) as Box<dyn Agent>)
                as Box<dyn FnMut() -> Box<dyn Agent>>,
        ),
        (
            "buy-and-hold",
            Box::new(|| Box::new(BuyAndHold) as Box<dyn Agent>),
        ),
    ] {
        let r = report(&mut make);
        for c in [&r.unmasked_vs_masked, &r.canonical_vs_reordered] {
            assert_eq!(c.steps, 200, "{name}");
            assert_eq!(c.disagreeing_steps, 0, "{name} {c:?}");
        }
        // Masking is exact. Reordering sums the book in another order, so the
        // score may move in the last bits and no further.
        let m = &r.unmasked_vs_masked;
        assert_eq!(m.baseline_sharpe, m.variant_sharpe, "{name}");
        let o = &r.canonical_vs_reordered;
        let (a, b) = (o.baseline_sharpe.unwrap(), o.variant_sharpe.unwrap());
        assert!((a - b).abs() <= 1e-12 * a.abs().max(1.0), "{name} {a} {b}");
    }
}

#[test]
fn planted_biases_show_up_in_the_pair_that_exposes_them() {
    let position = report(&mut || Box::new(FirstListed));
    assert_eq!(position.unmasked_vs_masked.disagreeing_steps, 0);
    assert_eq!(position.canonical_vs_reordered.disagreeing_steps, 200);
    assert_eq!(position.canonical_vs_reordered.first_disagreement, Some(0));

    let ticker = report(&mut || Box::new(TickerRecall));
    assert_eq!(ticker.unmasked_vs_masked.disagreeing_steps, 200);
    assert_eq!(ticker.canonical_vs_reordered.disagreeing_steps, 0);
    assert_ne!(
        ticker.unmasked_vs_masked.baseline_sharpe,
        ticker.unmasked_vs_masked.variant_sharpe
    );

    let level = report(&mut || Box::new(LevelReader));
    assert_eq!(level.unmasked_vs_masked.disagreeing_steps, 200);
    assert_eq!(level.canonical_vs_reordered.disagreeing_steps, 0);
    assert!(level.unmasked_vs_masked.mean_max_weight_gap > 0.29);
}
