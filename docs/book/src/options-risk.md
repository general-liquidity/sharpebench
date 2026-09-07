# Options pricing and payoff risk

These are standalone diagnostics, not estimates of an agent's trading edge.
The `greeks` CLI and npm/MCP tool price **one long European option**, not a book
of short options. Their local gamma/vega flags do not classify its payoff tails.

## Pricing boundaries

The Black-Scholes model here assumes a positive non-dividend-paying underlying,
European exercise, and constant continuously compounded rate and volatility.
All inputs must be finite, spot and strike strictly positive, and time remaining
and volatility nonnegative. A finite negative rate is supported. Negative
volatility and negative time are errors, not expiration shortcuts.

At expiration the price is spot intrinsic. At positive time with zero volatility,
the underlying is deterministic under the model and the strike remains discounted:

```text
call = max(S - K exp(-r T), 0)
put  = max(K exp(-r T) - S, 0)
```

This is the zero-standard-deviation boundary in the
[QuantLib Black formula](https://github.com/lballabio/QuantLib/blob/master/ql/pricingengines/blackformula.cpp),
expressed in spot rather than forward units. For example, S=K=100, T=1, r=0.05,
vol=0 gives a call price of 4.8770575499, not zero.

Away from the deterministic payoff kink, gamma and vega are zero; delta is the
in-the-money step evaluated against the **discounted** strike. Theta is the
negative derivative with respect to time remaining, and rho is the derivative
with respect to the rate. For an in-the-money zero-volatility call these are
`-r K exp(-r T)` and `T K exp(-r T)`; the in-the-money put has opposite signs.
Vega and rho are per unit change, not per percentage point; theta is per year.

At the kink the full Greek vector is undefined. `bs_price` can still return a
price, but `bs_greeks` returns `OptionsError::UndefinedGreeks`. The combined
CLI/npm/MCP quote refuses rather than returning zero sensitivities. At expiry
away from the kink, Greeks use a documented post-expiry convention: payoff delta
and zero other sensitivities, not a claim about the time-to-expiry limit.

The normal CDF uses an approximation. Finite f64 arithmetic and transcendental
functions do not promise exact prices or cross-platform byte equality. Numeric
overflow, discount underflow, and nonrepresentable results are explicit errors.

## Local convexity is not loss boundedness

`classify_greeks_risk` reports `net_short_gamma`, `short_vega`, `net_gamma` and
`net_vega`. Flags compare strictly below finite nonpositive policy floors, which
default to zero. Invalid Greeks or policy values cannot produce a successful
safe-looking verdict.

Negative gamma alone cannot establish a naked position or unbounded terminal
loss. A short put, bounded credit spread and covered call are counterexamples.
Conversely, a portfolio may have positive local gamma and still lose without
bound at sufficiently high terminal prices. The
[OCC/OIC risk overview](https://www.optionseducation.org/optionsoverview/leverage-risk)
distinguishes the short-call and short-put loss tails, and its
[covered-call description](https://www.optionseducation.org/strategies/all-strategies/covered-call-buy-write)
explains the role of the underlying hedge.

The separate Rust function `classify_payoff_tail(legs, underlying_qty)` assumes:

- Every leg is a European vanilla option on the same underlying and expiry.
- Terminal spot lies in `[0, infinity)`.
- Quantities are signed payoff units, with contract multipliers already applied.
- The supplied book includes all hedges; finite premiums and cash only shift it.

Above all strikes, payoff slope is `underlying_qty + sum(call quantities)`.
A negative slope means unbounded loss. Otherwise, continuity on the finite
interval from zero through the largest strike plus a nonnegative upper-tail slope
gives a finite lower bound. Mixed-expiry books are refused because their hedges
need not remain in place. The classifier does not infer omitted positions or
bound interim margin, American early assignment, slippage or dynamic trading.
It reports boundedness, not the magnitude of the worst loss.

The slope sum retains floating-point rounding residuals so large cancelling
hedges cannot erase a small uncovered short. This is a calculation over the
represented f64 quantities, not exact recovery of decimal inputs. Nonfinite
inputs and intermediate overflow are refused.

## Migration

Rust `bs_price`, `bs_greeks`, `portfolio_greeks` and `classify_greeks_risk` now
return `Result<_, OptionsError>`. Propagate errors; do not substitute zero risk.
Replace `GreeksRisk.naked_short_gamma` with `net_short_gamma` for the local
exposure flag. `GreeksRisk.unbounded_tail` is removed rather than mapped to false.
Call `classify_payoff_tail` with the complete supported portfolio when terminal
boundedness is needed. Neither classification is an attestation of the book.

Successful CLI/npm/MCP quotes still have `{price, greeks, risk}` with the corrected
risk fields. CLI refusals exit 2 with no result JSON. npm throws; the raw wasm
export returns `{error}` JSON. There is no dedicated Python options binding.
These changes do not regenerate historical benchmark evidence.
