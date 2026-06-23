//! Options pricing + Greeks-exposure risk scoring.
//!
//! A purely return-based score (even a deflated, pass^k-gated Sharpe) runs on a
//! linear P&L series and is blind to *how* the P&L was earned. An agent that sells
//! tail risk — short gamma / short vega — looks like a steady winner right up until
//! the move that wipes it out. This module gives SharpeBench eyes for that regime:
//! a deterministic Black-Scholes pricer, position Greeks, and a classifier that
//! flags the tail-selling exposures a benchmark for *trustworthy* trading agents
//! should charge against.
//!
//! Pure f64, no dependencies: the normal CDF is an Abramowitz-Stegun erf
//! approximation (abs error < 1.5e-7), so prices reproduce byte-for-byte.

use serde::{Deserialize, Serialize};

/// Standard-normal PDF.
fn norm_pdf(x: f64) -> f64 {
    use std::f64::consts::PI;
    (-0.5 * x * x).exp() / (2.0 * PI).sqrt()
}

/// erf via Abramowitz & Stegun 7.1.26 (max abs error 1.5e-7).
fn erf(x: f64) -> f64 {
    let sign = if x < 0.0 { -1.0 } else { 1.0 };
    let x = x.abs();
    let t = 1.0 / (1.0 + 0.327_591_1 * x);
    let y = 1.0
        - (((((1.061_405_429 * t - 1.453_152_027) * t) + 1.421_413_741) * t - 0.284_496_736) * t
            + 0.254_829_592)
            * t
            * (-x * x).exp();
    sign * y
}

/// Standard-normal CDF.
fn norm_cdf(x: f64) -> f64 {
    use std::f64::consts::SQRT_2;
    0.5 * (1.0 + erf(x / SQRT_2))
}

/// The first-order risk sensitivities of an option. Conventions: `theta` is per
/// year, `vega` is per 1.00 (100 vol-points) of volatility, `rho` is per 1.00 of
/// the rate.
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct Greeks {
    pub delta: f64,
    pub gamma: f64,
    pub theta: f64,
    pub vega: f64,
    pub rho: f64,
}

fn d1_d2(spot: f64, strike: f64, t: f64, r: f64, vol: f64) -> (f64, f64) {
    let sqrt_t = t.sqrt();
    let d1 = ((spot / strike).ln() + (r + 0.5 * vol * vol) * t) / (vol * sqrt_t);
    (d1, d1 - vol * sqrt_t)
}

/// Black-Scholes price of a European option. Degenerate inputs (`t <= 0` or
/// `vol <= 0`) collapse to discounted intrinsic value.
pub fn bs_price(spot: f64, strike: f64, t: f64, r: f64, vol: f64, is_call: bool) -> f64 {
    if t <= 0.0 || vol <= 0.0 {
        let intrinsic = if is_call {
            spot - strike
        } else {
            strike - spot
        };
        return intrinsic.max(0.0);
    }
    let (d1, d2) = d1_d2(spot, strike, t, r, vol);
    let disc = (-r * t).exp();
    if is_call {
        spot * norm_cdf(d1) - strike * disc * norm_cdf(d2)
    } else {
        strike * disc * norm_cdf(-d2) - spot * norm_cdf(-d1)
    }
}

/// Black-Scholes Greeks for a European option. Degenerate inputs return zeroed
/// sensitivities except a step-function delta.
pub fn bs_greeks(spot: f64, strike: f64, t: f64, r: f64, vol: f64, is_call: bool) -> Greeks {
    if t <= 0.0 || vol <= 0.0 {
        let delta = if is_call {
            f64::from(spot > strike)
        } else {
            -f64::from(spot < strike)
        };
        return Greeks {
            delta,
            ..Greeks::default()
        };
    }
    let (d1, d2) = d1_d2(spot, strike, t, r, vol);
    let sqrt_t = t.sqrt();
    let disc = (-r * t).exp();
    let pdf_d1 = norm_pdf(d1);

    let delta = if is_call {
        norm_cdf(d1)
    } else {
        norm_cdf(d1) - 1.0
    };
    let gamma = pdf_d1 / (spot * vol * sqrt_t);
    let vega = spot * pdf_d1 * sqrt_t;
    let theta = if is_call {
        -(spot * pdf_d1 * vol) / (2.0 * sqrt_t) - r * strike * disc * norm_cdf(d2)
    } else {
        -(spot * pdf_d1 * vol) / (2.0 * sqrt_t) + r * strike * disc * norm_cdf(-d2)
    };
    let rho = if is_call {
        strike * t * disc * norm_cdf(d2)
    } else {
        -strike * t * disc * norm_cdf(-d2)
    };
    Greeks {
        delta,
        gamma,
        theta,
        vega,
        rho,
    }
}

/// One leg of an options position. `qty` is signed: negative is short (sold).
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct Leg {
    pub strike: f64,
    pub t_years: f64,
    pub is_call: bool,
    pub qty: f64,
}

/// Net Greeks of a multi-leg position (Σ per-leg Greeks × qty), all legs priced off
/// the same spot / rate / vol.
pub fn portfolio_greeks(legs: &[Leg], spot: f64, r: f64, vol: f64) -> Greeks {
    let mut g = Greeks::default();
    for leg in legs {
        let lg = bs_greeks(spot, leg.strike, leg.t_years, r, vol, leg.is_call);
        g.delta += leg.qty * lg.delta;
        g.gamma += leg.qty * lg.gamma;
        g.theta += leg.qty * lg.theta;
        g.vega += leg.qty * lg.vega;
        g.rho += leg.qty * lg.rho;
    }
    g
}

/// Net payoff of the position at expiry for a terminal `spot` (intrinsic value ×
/// qty, summed). Excludes premium — see [`payoff_breakevens`] for premium-aware
/// break-even spots.
pub fn portfolio_payoff_at_expiry(legs: &[Leg], spot: f64) -> f64 {
    legs.iter()
        .map(|leg| {
            let intrinsic = if leg.is_call {
                (spot - leg.strike).max(0.0)
            } else {
                (leg.strike - spot).max(0.0)
            };
            leg.qty * intrinsic
        })
        .sum()
}

/// Approximate break-even spots: the terminal prices in `spots` where net payoff
/// minus `net_premium` crosses zero (returned as the midpoint of each sign-change
/// interval). `net_premium` is what the position cost to open (credit = negative).
/// Deterministic over the caller-supplied grid — no root solver, no ambient state.
pub fn payoff_breakevens(legs: &[Leg], net_premium: f64, spots: &[f64]) -> Vec<f64> {
    let mut out = Vec::new();
    let f = |s: f64| portfolio_payoff_at_expiry(legs, s) - net_premium;
    for pair in spots.windows(2) {
        let (a, b) = (pair[0], pair[1]);
        let (fa, fb) = (f(a), f(b));
        if fa == 0.0 {
            out.push(a);
        } else if fa * fb < 0.0 {
            out.push(0.5 * (a + b));
        }
    }
    if let Some(&last) = spots.last() {
        if f(last) == 0.0 {
            out.push(last);
        }
    }
    out
}

/// Thresholds for flagging tail-selling exposure. Defaults flag any net-negative
/// gamma or vega.
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct GreeksPolicy {
    /// Net gamma at or below this is "short gamma" (default 0.0).
    pub gamma_floor: f64,
    /// Net vega at or below this is "short vega" (default 0.0).
    pub vega_floor: f64,
}

impl Default for GreeksPolicy {
    fn default() -> Self {
        GreeksPolicy {
            gamma_floor: 0.0,
            vega_floor: 0.0,
        }
    }
}

/// The tail-risk verdict for a position's net Greeks.
#[derive(Clone, Copy, Debug, Serialize, PartialEq)]
pub struct GreeksRisk {
    /// Net short gamma — losses accelerate as the underlying moves.
    pub naked_short_gamma: bool,
    /// Short gamma implies negative convexity → unbounded tail loss potential.
    pub unbounded_tail: bool,
    /// Net short vega — loses on a volatility spike.
    pub short_vega: bool,
    pub net_gamma: f64,
    pub net_vega: f64,
}

/// Classify a position's net Greeks for tail-selling exposure.
pub fn classify_greeks_risk(greeks: &Greeks, policy: &GreeksPolicy) -> GreeksRisk {
    let naked_short_gamma = greeks.gamma < policy.gamma_floor;
    GreeksRisk {
        naked_short_gamma,
        unbounded_tail: naked_short_gamma,
        short_vega: greeks.vega < policy.vega_floor,
        net_gamma: greeks.gamma,
        net_vega: greeks.vega,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const S: f64 = 100.0;
    const K: f64 = 100.0;
    const T: f64 = 1.0;
    const R: f64 = 0.05;
    const VOL: f64 = 0.2;

    #[test]
    fn atm_call_matches_textbook_value() {
        // S=K=100, T=1, r=5%, vol=20% → ≈ 10.4506.
        let c = bs_price(S, K, T, R, VOL, true);
        assert!((c - 10.4506).abs() < 1e-2, "call={c}");
    }

    #[test]
    fn put_call_parity_holds() {
        let c = bs_price(S, K, T, R, VOL, true);
        let p = bs_price(S, K, T, R, VOL, false);
        // C - P == S - K e^{-rT}
        let rhs = S - K * (-R * T).exp();
        assert!((c - p - rhs).abs() < 1e-6, "parity off: {}", c - p - rhs);
    }

    #[test]
    fn deep_itm_call_delta_approaches_one() {
        let g = bs_greeks(200.0, K, T, R, VOL, true);
        assert!(g.delta > 0.99, "delta={}", g.delta);
    }

    #[test]
    fn gamma_is_non_negative_and_vega_positive_for_a_long_option() {
        let g = bs_greeks(S, K, T, R, VOL, true);
        assert!(g.gamma >= 0.0);
        assert!(g.vega > 0.0);
        // A long call decays in time.
        assert!(g.theta < 0.0);
    }

    #[test]
    fn short_call_is_naked_short_gamma_and_unbounded_tail() {
        let legs = [Leg {
            strike: K,
            t_years: T,
            is_call: true,
            qty: -1.0,
        }];
        let g = portfolio_greeks(&legs, S, R, VOL);
        assert!(g.gamma < 0.0, "short call must be net-short gamma");
        let risk = classify_greeks_risk(&g, &GreeksPolicy::default());
        assert!(risk.naked_short_gamma);
        assert!(risk.unbounded_tail);
        assert!(risk.short_vega);
    }

    #[test]
    fn a_long_option_is_not_flagged() {
        let legs = [Leg {
            strike: K,
            t_years: T,
            is_call: true,
            qty: 1.0,
        }];
        let g = portfolio_greeks(&legs, S, R, VOL);
        let risk = classify_greeks_risk(&g, &GreeksPolicy::default());
        assert!(!risk.naked_short_gamma);
        assert!(!risk.unbounded_tail);
        assert!(!risk.short_vega);
    }

    #[test]
    fn long_call_payoff_and_breakeven() {
        let legs = [Leg {
            strike: 100.0,
            t_years: 0.0,
            is_call: true,
            qty: 1.0,
        }];
        // At expiry, spot 120 → intrinsic 20.
        assert!((portfolio_payoff_at_expiry(&legs, 120.0) - 20.0).abs() < 1e-12);
        // Paid 5 premium → break-even near 105.
        let grid: Vec<f64> = (90..=120).map(f64::from).collect();
        let bes = payoff_breakevens(&legs, 5.0, &grid);
        assert!(bes.iter().any(|b| (b - 105.0).abs() <= 1.0), "bes={bes:?}");
    }

    #[test]
    fn degenerate_inputs_collapse_to_intrinsic() {
        assert_eq!(bs_price(120.0, 100.0, 0.0, R, VOL, true), 20.0);
        assert_eq!(bs_price(80.0, 100.0, 0.0, R, VOL, true), 0.0);
        assert_eq!(bs_price(80.0, 100.0, 1.0, R, 0.0, false), 20.0);
    }
}
