//! The masked view changes what an agent could recognise (names, dates, price
//! level) and nothing the engine scores. Checked on frozen real datasets whose
//! levels span five orders of magnitude, for every reference agent and every
//! execution profile, bit for bit.

use sharpebench_sim::{
    run_backtest, Agent, BuyAndHold, CostProfile, Dataset, HoldAgent, Momentum, RandomAgent, Window,
};

fn frozen(name: &str) -> Dataset {
    let path = format!("{}/../../data/{name}", env!("CARGO_MANIFEST_DIR"));
    Dataset::from_csv_file(&path).unwrap()
}

type MakeAgent = Box<dyn Fn() -> Box<dyn Agent>>;

fn agents() -> Vec<(&'static str, MakeAgent)> {
    vec![
        ("buy-and-hold", Box::new(|| Box::new(BuyAndHold))),
        ("hold", Box::new(|| Box::new(HoldAgent))),
        ("momentum", Box::new(|| Box::new(Momentum::default()))),
        ("random", Box::new(|| Box::new(RandomAgent::new(7)))),
    ]
}

#[test]
fn every_backtest_outcome_is_invariant_while_levels_no_longer_match() {
    for name in [
        "us-indices-1d.csv",
        "crypto-majors-1d.csv",
        "fx-majors-1d.csv",
    ] {
        let data = frozen(name);
        let view = data.masked_view().unwrap();
        let masked = &view.dataset;
        for m in &view.symbols {
            let src = &data.closes[&m.source];
            let out = &masked.closes[&m.opaque];
            assert!((64.0..128.0).contains(&out[0]), "{name} {}", m.source);
            assert_ne!(src[0], out[0], "{name} {}: level still matches", m.source);
        }
        let windows = [
            Window {
                start: 30,
                end: 400,
            },
            Window {
                start: 400,
                end: data.len(),
            },
        ];
        for profile in [
            CostProfile::None,
            CostProfile::Typical,
            CostProfile::WorstCase,
            CostProfile::Realistic,
        ] {
            let costs = profile.resolve().costs;
            for (agent, make) in agents() {
                for &window in &windows {
                    for seed in [0, 3] {
                        let a = run_backtest(&data, make().as_mut(), window, seed, costs);
                        let b = run_backtest(masked, make().as_mut(), window, seed, costs);
                        let bits = |r: &[f64]| r.iter().map(|x| x.to_bits()).collect::<Vec<_>>();
                        assert_eq!(
                            bits(&a.returns),
                            bits(&b.returns),
                            "{name} {agent} {profile:?} seed {seed}"
                        );
                        assert_eq!(a.confidences, b.confidences);
                        assert_eq!(a.outcomes, b.outcomes);
                        // Process events name symbols; mapped back, they agree.
                        let mut events = serde_json::to_string(&b.trace.events).unwrap();
                        for m in &view.symbols {
                            events = events.replace(
                                &format!("\"{}\"", m.opaque),
                                &format!("\"{}\"", m.source),
                            );
                        }
                        assert_eq!(serde_json::to_string(&a.trace.events).unwrap(), events);
                    }
                }
            }
        }
    }
}
