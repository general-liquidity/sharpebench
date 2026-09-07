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
//! Local gamma/vega are sensitivities, not proofs of unbounded loss. Payoff-tail
//! classification separately requires positions and hedges. The pricer assumes
//! European exercise, a non-dividend-paying positive underlying and constant
//! rate/volatility. Pure f64 with an approximate normal CDF; no cross-platform
//! bit-identity or exact-arithmetic guarantee follows from that choice.

use serde::{Deserialize, Serialize};

/// Invalid options inputs or an unavailable numerical result, never a risk pass.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OptionsError {
    InvalidParameter(&'static str),
    NumericalRange,
    UndefinedGreeks,
    MismatchedExpiries,
}

impl std::fmt::Display for OptionsError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidParameter(name) => write!(f, "invalid options parameter: {name}"),
            Self::NumericalRange => {
                f.write_str("options calculation exceeds finite numerical range")
            }
            Self::UndefinedGreeks => {
                f.write_str("Greeks are undefined at the deterministic payoff kink")
            }
            Self::MismatchedExpiries => {
                f.write_str("payoff-tail classification requires a common expiry")
            }
        }
    }
}

impl std::error::Error for OptionsError {}

fn validate_inputs(spot: f64, strike: f64, t: f64, r: f64, vol: f64) -> Result<(), OptionsError> {
    for (name, valid) in [
        ("spot", spot.is_finite() && spot > 0.0),
        ("strike", strike.is_finite() && strike > 0.0),
        ("t_years", t.is_finite() && t >= 0.0),
        ("rate", r.is_finite()),
        ("vol", vol.is_finite() && vol >= 0.0),
    ] {
        if !valid {
            return Err(OptionsError::InvalidParameter(name));
        }
    }
    Ok(())
}

fn finite(value: f64) -> Result<f64, OptionsError> {
    if value.is_finite() {
        Ok(value)
    } else {
        Err(OptionsError::NumericalRange)
    }
}

fn discounted_strike(strike: f64, t: f64, r: f64) -> Result<f64, OptionsError> {
    let discount = finite((-finite(r * t)?).exp())?;
    let discounted = finite(strike * discount)?;
    if discount == 0.0 || discounted == 0.0 {
        return Err(OptionsError::NumericalRange);
    }
    Ok(discounted)
}

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

impl Greeks {
    fn checked(self) -> Result<Self, OptionsError> {
        for value in [self.delta, self.gamma, self.theta, self.vega, self.rho] {
            finite(value)?;
        }
        Ok(self)
    }
}

fn d1_d2(spot: f64, strike: f64, t: f64, r: f64, vol: f64) -> Result<(f64, f64), OptionsError> {
    let std_dev = finite(vol * t.sqrt())?;
    if std_dev == 0.0 {
        return Err(OptionsError::NumericalRange);
    }
    let d1 = finite((spot.ln() - strike.ln() + finite(r * t)?) / std_dev + 0.5 * std_dev)?;
    Ok((d1, finite(d1 - std_dev)?))
}

/// European Black-Scholes price under the module's model assumptions.
/// At expiry use spot intrinsic. With positive time and zero volatility, use
/// `max(S - K exp(-r t), 0)` for a call and the reverse difference for a put.
/// Inputs must be finite, spot/strike positive and time/volatility nonnegative.
pub fn bs_price(
    spot: f64,
    strike: f64,
    t: f64,
    r: f64,
    vol: f64,
    is_call: bool,
) -> Result<f64, OptionsError> {
    validate_inputs(spot, strike, t, r, vol)?;
    let kd = if t == 0.0 {
        strike
    } else {
        discounted_strike(strike, t, r)?
    };
    if t == 0.0 || vol == 0.0 {
        return Ok(if is_call { spot - kd } else { kd - spot }.max(0.0));
    }
    let (d1, d2) = d1_d2(spot, strike, t, r, vol)?;
    let price = if is_call {
        spot * norm_cdf(d1) - kd * norm_cdf(d2)
    } else {
        kd * norm_cdf(-d2) - spot * norm_cdf(-d1)
    };
    // The CDF approximation can round a far-OTM value slightly below zero.
    Ok(finite(price)?.max(0.0))
}

/// Black-Scholes Greeks, with theta = negative derivative with respect to time
/// remaining. At zero volatility away from the discounted-strike kink, delta is
/// a step, gamma/vega zero, and theta/rho retain discounting. Exactly at the kink
/// the full vector is unavailable, not a fabricated set of zero sensitivities.
/// At expiry away from the spot-strike kink, only payoff delta is reported;
/// the other fields use the explicit post-expiry zero convention.
pub fn bs_greeks(
    spot: f64,
    strike: f64,
    t: f64,
    r: f64,
    vol: f64,
    is_call: bool,
) -> Result<Greeks, OptionsError> {
    validate_inputs(spot, strike, t, r, vol)?;
    let kd = if t == 0.0 {
        strike
    } else {
        discounted_strike(strike, t, r)?
    };
    if t == 0.0 || vol == 0.0 {
        if spot == kd {
            return Err(OptionsError::UndefinedGreeks);
        }
        let delta = if is_call {
            f64::from(spot > kd)
        } else {
            -f64::from(spot < kd)
        };
        return (Greeks {
            delta,
            theta: if t == 0.0 || delta == 0.0 {
                0.0
            } else {
                -delta * r * kd
            },
            rho: if t == 0.0 || delta == 0.0 {
                0.0
            } else {
                delta * t * kd
            },
            ..Greeks::default()
        })
        .checked();
    }
    let (d1, d2) = d1_d2(spot, strike, t, r, vol)?;
    let sqrt_t = t.sqrt();
    let pdf_d1 = norm_pdf(d1);

    let delta = if is_call {
        norm_cdf(d1)
    } else {
        norm_cdf(d1) - 1.0
    };
    let gamma = (pdf_d1 / (vol * sqrt_t)) / spot;
    let vega = spot * pdf_d1 * sqrt_t;
    let theta = if is_call {
        -(spot * pdf_d1 * vol) / (2.0 * sqrt_t) - r * kd * norm_cdf(d2)
    } else {
        -(spot * pdf_d1 * vol) / (2.0 * sqrt_t) + r * kd * norm_cdf(-d2)
    };
    let rho = if is_call {
        kd * t * norm_cdf(d2)
    } else {
        -kd * t * norm_cdf(-d2)
    };
    (Greeks {
        delta,
        gamma,
        theta,
        vega,
        rho,
    })
    .checked()
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
pub fn portfolio_greeks(legs: &[Leg], spot: f64, r: f64, vol: f64) -> Result<Greeks, OptionsError> {
    validate_inputs(spot, 1.0, 0.0, r, vol)?;
    let mut g = Greeks::default();
    for leg in legs {
        if !leg.qty.is_finite() {
            return Err(OptionsError::InvalidParameter("qty"));
        }
        validate_inputs(spot, leg.strike, leg.t_years, r, vol)?;
        if leg.qty == 0.0 {
            continue;
        }
        let lg = bs_greeks(spot, leg.strike, leg.t_years, r, vol, leg.is_call)?;
        g.delta += leg.qty * lg.delta;
        g.gamma += leg.qty * lg.gamma;
        g.theta += leg.qty * lg.theta;
        g.vega += leg.qty * lg.vega;
        g.rho += leg.qty * lg.rho;
        g.checked()?;
    }
    Ok(g)
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
    /// Net gamma strictly below this is flagged (finite, nonpositive; default 0).
    pub gamma_floor: f64,
    /// Net vega strictly below this is flagged (finite, nonpositive; default 0).
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

/// Local exposure flags at the supplied Greeks and policy thresholds.
#[derive(Clone, Copy, Debug, Serialize, PartialEq)]
pub struct GreeksRisk {
    /// Negative local convexity below the policy floor. Does not imply nakedness
    /// or an unbounded loss; those cannot be recovered from net Greeks alone.
    pub net_short_gamma: bool,
    /// Net short vega — loses on a volatility spike.
    pub short_vega: bool,
    pub net_gamma: f64,
    pub net_vega: f64,
}

/// Classify a position's net Greeks for tail-selling exposure.
pub fn classify_greeks_risk(
    greeks: &Greeks,
    policy: &GreeksPolicy,
) -> Result<GreeksRisk, OptionsError> {
    greeks.checked()?;
    for (name, floor) in [
        ("gamma_floor", policy.gamma_floor),
        ("vega_floor", policy.vega_floor),
    ] {
        if !floor.is_finite() || floor > 0.0 {
            return Err(OptionsError::InvalidParameter(name));
        }
    }
    Ok(GreeksRisk {
        net_short_gamma: greeks.gamma < policy.gamma_floor,
        short_vega: greeks.vega < policy.vega_floor,
        net_gamma: greeks.gamma,
        net_vega: greeks.vega,
    })
}

/// Terminal loss classification, conditional on a complete same-underlying,
/// same-expiry European vanilla portfolio and nonnegative terminal spot.
/// Finite premiums/cash shift the payoff but do not change its boundedness.
#[derive(Clone, Copy, Debug, Serialize, PartialEq)]
pub struct PayoffTailRisk {
    pub unbounded_loss: bool,
    /// Above every strike, payoff slope is underlying units plus signed calls.
    pub high_spot_slope: f64,
}

/// Unlike local gamma, a negative terminal upper-tail slope establishes unbounded
/// loss in this model. Puts have bounded loss on S >= 0. Include the underlying
/// hedge in payoff units (after applying each contract multiplier to leg qty).
/// Mixed-expiry books are refused: a calendar hedge need not survive each expiry.
/// This is not a bound on interim margin calls, early assignment or trading costs.
pub fn classify_payoff_tail(
    legs: &[Leg],
    underlying_qty: f64,
) -> Result<PayoffTailRisk, OptionsError> {
    if !underlying_qty.is_finite() {
        return Err(OptionsError::InvalidParameter("underlying_qty"));
    }
    let mut expiry = None;
    // Retain each addition's rounding residual. A naive running sum can label
    // 1e16 underlying units plus calls [-1, -1e16] as a zero-slope hedge even
    // though its terminal slope is -1. Nonoverlapping partials preserve the sign
    // of the sum of the represented f64 quantities; overflow is still refused.
    let mut partials = vec![underlying_qty];
    for leg in legs {
        validate_inputs(1.0, leg.strike, leg.t_years, 0.0, 0.0)?;
        if !leg.qty.is_finite() {
            return Err(OptionsError::InvalidParameter("qty"));
        }
        if leg.qty == 0.0 {
            continue;
        }
        if expiry.is_some_and(|t| t != leg.t_years) {
            return Err(OptionsError::MismatchedExpiries);
        }
        expiry = Some(leg.t_years);
        if leg.is_call {
            let mut x = leg.qty;
            let mut retained = 0;
            for i in 0..partials.len() {
                let mut y = partials[i];
                if x.abs() < y.abs() {
                    std::mem::swap(&mut x, &mut y);
                }
                let hi = finite(x + y)?;
                let lo = y - (hi - x);
                if lo != 0.0 {
                    partials[retained] = lo;
                    retained += 1;
                }
                x = hi;
            }
            partials.truncate(retained);
            if x != 0.0 {
                partials.push(x);
            }
        }
    }
    let slope = finite(partials.iter().sum())?;
    Ok(PayoffTailRisk {
        unbounded_loss: slope < 0.0,
        high_spot_slope: slope,
    })
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
        let c = bs_price(S, K, T, R, VOL, true).unwrap();
        assert!((c - 10.4506).abs() < 1e-2, "call={c}");
    }

    #[test]
    fn put_call_parity_holds() {
        let c = bs_price(S, K, T, R, VOL, true).unwrap();
        let p = bs_price(S, K, T, R, VOL, false).unwrap();
        // C - P == S - K e^{-rT}
        let rhs = S - K * (-R * T).exp();
        assert!((c - p - rhs).abs() < 1e-6, "parity off: {}", c - p - rhs);
    }

    #[test]
    fn deep_itm_call_delta_approaches_one() {
        let g = bs_greeks(200.0, K, T, R, VOL, true).unwrap();
        assert!(g.delta > 0.99, "delta={}", g.delta);
    }

    #[test]
    fn gamma_is_non_negative_and_vega_positive_for_a_long_option() {
        let g = bs_greeks(S, K, T, R, VOL, true).unwrap();
        assert!(g.gamma >= 0.0);
        assert!(g.vega > 0.0);
        // A long call decays in time.
        assert!(g.theta < 0.0);
    }

    #[test]
    fn short_call_has_short_gamma_and_a_negative_terminal_tail_slope() {
        let legs = [Leg {
            strike: K,
            t_years: T,
            is_call: true,
            qty: -1.0,
        }];
        let g = portfolio_greeks(&legs, S, R, VOL).unwrap();
        assert!(g.gamma < 0.0, "short call must be net-short gamma");
        let risk = classify_greeks_risk(&g, &GreeksPolicy::default()).unwrap();
        assert!(risk.net_short_gamma);
        assert!(classify_payoff_tail(&legs, 0.0).unwrap().unbounded_loss);
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
        let g = portfolio_greeks(&legs, S, R, VOL).unwrap();
        let risk = classify_greeks_risk(&g, &GreeksPolicy::default()).unwrap();
        assert!(!risk.net_short_gamma);
        assert!(!classify_payoff_tail(&legs, 0.0).unwrap().unbounded_loss);
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
    fn expiration_uses_spot_intrinsic() {
        assert_eq!(bs_price(120.0, 100.0, 0.0, R, VOL, true).unwrap(), 20.0);
        assert_eq!(bs_price(80.0, 100.0, 0.0, R, VOL, true).unwrap(), 0.0);
        assert_eq!(bs_price(80.0, 100.0, 0.0, R, 0.0, false).unwrap(), 20.0);
    }
}
