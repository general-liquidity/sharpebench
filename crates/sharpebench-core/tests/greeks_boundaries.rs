use sharpebench_core::{
    bs_greeks, bs_price, classify_greeks_risk, classify_payoff_tail, portfolio_greeks, Greeks,
    GreeksPolicy, Leg, OptionsError,
};

#[test]
fn zero_volatility_discounts_the_strike_and_obeys_put_call_parity() {
    for rate in [-0.05_f64, 0.0, 0.05] {
        for spot in [80.0, 100.0, 120.0] {
            let discounted_strike = 100.0 * (-rate * 1.5).exp();
            let call = bs_price(spot, 100.0, 1.5, rate, 0.0, true).unwrap();
            let put = bs_price(spot, 100.0, 1.5, rate, 0.0, false).unwrap();
            assert!((call - (spot - discounted_strike).max(0.0)).abs() < 1e-12);
            assert!((put - (discounted_strike - spot).max(0.0)).abs() < 1e-12);
            assert!((call - put - (spot - discounted_strike)).abs() < 1e-12);
        }
    }
}

#[test]
fn zero_volatility_greeks_are_continuous_away_from_the_forward_kink() {
    for rate in [-0.05, 0.05] {
        for is_call in [false, true] {
            let zero = bs_greeks(100.0, 100.0, 1.0, rate, 0.0, is_call).unwrap();
            let small = bs_greeks(100.0, 100.0, 1.0, rate, 1e-6, is_call).unwrap();
            for (actual, expected) in [
                (zero.delta, small.delta),
                (zero.gamma, small.gamma),
                (zero.theta, small.theta),
                (zero.vega, small.vega),
                (zero.rho, small.rho),
            ] {
                assert!((actual - expected).abs() < 1e-9, "{actual} != {expected}");
            }
        }
    }
}

#[test]
fn local_greeks_do_not_assert_payoff_boundedness_or_nakedness() {
    let short_put = [Leg {
        strike: 100.0,
        t_years: 1.0,
        is_call: false,
        qty: -1.0,
    }];
    let greeks = portfolio_greeks(&short_put, 100.0, 0.05, 0.2).unwrap();
    assert!(greeks.gamma < 0.0);
    let risk =
        serde_json::to_value(classify_greeks_risk(&greeks, &GreeksPolicy::default()).unwrap())
            .unwrap();
    // Checking only gamma's sign would pass the old false unbounded-loss claim.
    assert!(risk.get("unbounded_tail").is_none());
    assert!(risk.get("naked_short_gamma").is_none());
    assert_eq!(risk["net_short_gamma"], true);
}

#[test]
fn zero_volatility_sensitivities_match_independent_price_differences() {
    let step = 1e-5;
    for (spot, is_call) in [(120.0, true), (80.0, false)] {
        for rate in [-0.05, 0.05] {
            let g = bs_greeks(spot, 100.0, 1.5, rate, 0.0, is_call).unwrap();
            let price = |s, t, r| bs_price(s, 100.0, t, r, 0.0, is_call).unwrap();
            let delta =
                (price(spot + step, 1.5, rate) - price(spot - step, 1.5, rate)) / (2.0 * step);
            let theta =
                -(price(spot, 1.5 + step, rate) - price(spot, 1.5 - step, rate)) / (2.0 * step);
            let rho =
                (price(spot, 1.5, rate + step) - price(spot, 1.5, rate - step)) / (2.0 * step);
            assert!((g.delta - delta).abs() < 1e-7);
            assert!((g.theta - theta).abs() < 1e-7);
            assert!((g.rho - rho).abs() < 1e-7);
            assert_eq!((g.gamma, g.vega), (0.0, 0.0));
        }
    }
}

#[test]
fn a_kink_is_unavailable_but_prices_and_adjacent_greeks_remain_available() {
    for (t, vol) in [(1.0, 0.0), (0.0, 0.2)] {
        for is_call in [true, false] {
            assert_eq!(bs_price(100.0, 100.0, t, 0.0, vol, is_call), Ok(0.0));
            assert_eq!(
                bs_greeks(100.0, 100.0, t, 0.0, vol, is_call),
                Err(OptionsError::UndefinedGreeks)
            );
            for spot in [99.999, 100.001] {
                let g = bs_greeks(spot, 100.0, t, 0.0, vol, is_call).unwrap();
                let expected = if is_call {
                    f64::from(spot > 100.0)
                } else {
                    -f64::from(spot < 100.0)
                };
                assert_eq!(g.delta, expected);
            }
        }
    }
}

#[test]
fn invalid_pricing_inputs_are_refused_even_at_expiry() {
    for field in 0..5 {
        for invalid in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            for t in [0.0, 1.0] {
                let mut args = [100.0, 100.0, t, 0.05, 0.2];
                args[field] = invalid;
                let [s, k, t, r, vol] = args;
                assert!(matches!(
                    bs_price(s, k, t, r, vol, true),
                    Err(OptionsError::InvalidParameter(_))
                ));
                assert!(matches!(
                    bs_greeks(s, k, t, r, vol, true),
                    Err(OptionsError::InvalidParameter(_))
                ));
            }
        }
    }
    for (field, value, name) in [
        (0, 0.0, "spot"),
        (1, 0.0, "strike"),
        (0, -1.0, "spot"),
        (1, -1.0, "strike"),
        (2, -1.0, "t_years"),
        (4, -1.0, "vol"),
    ] {
        let mut args = [100.0, 100.0, 1.0, 0.05, 0.2];
        args[field] = value;
        let [s, k, t, r, vol] = args;
        assert_eq!(
            bs_price(s, k, t, r, vol, true),
            Err(OptionsError::InvalidParameter(name))
        );
        assert_eq!(
            bs_greeks(s, k, t, r, vol, true),
            Err(OptionsError::InvalidParameter(name))
        );
    }
}

#[test]
fn numerical_overflow_and_discount_underflow_are_not_successful_quotes() {
    for rate in [-1000.0, 1000.0] {
        assert_eq!(
            bs_price(100.0, 100.0, 1.0, rate, 0.0, true),
            Err(OptionsError::NumericalRange)
        );
        assert_eq!(
            bs_greeks(100.0, 100.0, 1.0, rate, 0.0, true),
            Err(OptionsError::NumericalRange)
        );
    }
}

fn leg(strike: f64, is_call: bool, qty: f64) -> Leg {
    Leg {
        strike,
        t_years: 1.0,
        is_call,
        qty,
    }
}

#[test]
fn short_put_credit_spread_and_covered_call_have_bounded_terminal_loss() {
    for (legs, hedge) in [
        (vec![leg(100.0, false, -1.0)], 0.0),
        (vec![leg(100.0, true, -1.0), leg(150.0, true, 1.0)], 0.0),
        (vec![leg(100.0, true, -1.0)], 1.0),
    ] {
        let greeks = portfolio_greeks(&legs, 100.0, 0.05, 0.2).unwrap();
        assert!(
            greeks.gamma < 0.0,
            "counterexample must actually have negative gamma"
        );
        assert!(
            classify_greeks_risk(&greeks, &GreeksPolicy::default())
                .unwrap()
                .net_short_gamma
        );
        let tails = classify_payoff_tail(&legs, hedge).unwrap();
        assert!(!tails.unbounded_loss);
        assert_eq!(tails.high_spot_slope, 0.0);
    }
}

#[test]
fn positive_gamma_does_not_hide_an_unbounded_terminal_loss() {
    let legs = [leg(100.0, true, 1.0), leg(200.0, true, -2.0)];
    let greeks = portfolio_greeks(&legs, 100.0, 0.05, 0.2).unwrap();
    assert!(greeks.gamma > 0.0);
    assert!(
        !classify_greeks_risk(&greeks, &GreeksPolicy::default())
            .unwrap()
            .net_short_gamma
    );
    let tails = classify_payoff_tail(&legs, 0.0).unwrap();
    assert!(tails.unbounded_loss);
    assert_eq!(tails.high_spot_slope, -1.0);
}

#[test]
fn payoff_tail_uses_the_hedge_and_refuses_mixed_expiries() {
    let mut legs = [leg(100.0, true, -1.0), leg(150.0, true, 1.0)];
    assert!(!classify_payoff_tail(&legs, 0.0).unwrap().unbounded_loss);
    legs[1].t_years = 0.5;
    assert_eq!(
        classify_payoff_tail(&legs, 0.0),
        Err(OptionsError::MismatchedExpiries)
    );
    let short = [leg(100.0, true, -1.0)];
    assert!(classify_payoff_tail(&short, 0.999).unwrap().unbounded_loss);
    assert!(!classify_payoff_tail(&short, 1.0).unwrap().unbounded_loss);
    assert!(classify_payoff_tail(&[], -1.0).unwrap().unbounded_loss);
    assert!(!classify_payoff_tail(&[], 0.0).unwrap().unbounded_loss);
}

#[test]
fn invalid_portfolios_and_policies_do_not_default_to_safe() {
    assert_eq!(
        classify_payoff_tail(&[], f64::NAN),
        Err(OptionsError::InvalidParameter("underlying_qty"))
    );
    let legs = [leg(100.0, true, f64::INFINITY)];
    assert_eq!(
        classify_payoff_tail(&legs, 0.0),
        Err(OptionsError::InvalidParameter("qty"))
    );
    assert_eq!(
        portfolio_greeks(&legs, 100.0, 0.0, 0.2),
        Err(OptionsError::InvalidParameter("qty"))
    );
    let policy = GreeksPolicy {
        gamma_floor: 0.01,
        vega_floor: 0.0,
    };
    assert_eq!(
        classify_greeks_risk(&Greeks::default(), &policy),
        Err(OptionsError::InvalidParameter("gamma_floor"))
    );
    let malformed = Greeks {
        gamma: f64::NAN,
        ..Greeks::default()
    };
    assert_eq!(
        classify_greeks_risk(&malformed, &GreeksPolicy::default()),
        Err(OptionsError::NumericalRange)
    );
    assert!(
        !classify_greeks_risk(&Greeks::default(), &GreeksPolicy::default())
            .unwrap()
            .net_short_gamma
    );
}

#[test]
fn payoff_tail_sign_survives_large_leg_cancellation_in_each_order() {
    for quantities in [
        [1e16, -1.0, -1e16],
        [1e16, -1e16, -1.0],
        [-1.0, 1e16, -1e16],
        [-1.0, -1e16, 1e16],
        [-1e16, 1e16, -1.0],
        [-1e16, -1.0, 1e16],
    ] {
        let legs = quantities.map(|qty| leg(100.0, true, qty));
        let tail = classify_payoff_tail(&legs, 0.0).unwrap();
        assert_eq!(tail.high_spot_slope, -1.0);
        assert!(tail.unbounded_loss);
    }
    let tail =
        classify_payoff_tail(&[leg(100.0, true, -1.0), leg(100.0, true, -1e16)], 1e16).unwrap();
    assert_eq!(tail.high_spot_slope, -1.0);
    assert!(tail.unbounded_loss);
}
