# The simulator

`sharpebench-sim` is the point-in-time engine every score runs through. It is
deterministic (no clock, no ambient RNG, fixed float reduction order), so a backtest
reproduces byte-for-byte on any machine. This page documents the parts of the engine
that shape a score: the cost model, the synthetic data generators, and the state
snapshots.

## Execution costs

A frictionless fill flatters every strategy, so the simulator charges a realistic cost
on every fill. An edge has to survive the friction it would actually meet:

- **Fees**: a proportional per-trade fee.
- **Slippage**: seeded, so the same run always pays the same slippage.
- **Market impact**: a square-root function of order size against available liquidity.
- **Financing**: a carry cost on held inventory.
- **Liquidity caps**: an order larger than the available depth is only partially filled.
- **Turnover cost (TRF)**: a cost proportional to how much the portfolio weights move
  from one step to the next. `trf_factor(weights_prev, weights_new, c)` returns a
  multiplicative factor in `(0, 1]` that discounts return by the turnover incurred;
  `c = 0` returns exactly `1.0` (no turnover charge). The iteration is deterministic
  (only mul/add/div/max, capped at a pinned number of sweeps), so the cost reproduces on
  any platform. Turnover cost is what separates an edge that survives rebalancing from
  one that only looks good before trading frictions.

The cost profile is selectable, so an edge can be required to clear the bar even under
adverse execution. Four ship, each named here as it is named in evidence records and in
the paper; the Rust identifier differs and is given beside it:

| Reported name | `CostProfile` variant | Fee / slippage / impact / financing (bps) |
|---|---|---|
| `frictionless` | `CostProfile::None` | 0 / 0 / 0 / 0, no fill delay |
| `typical` | `CostProfile::Typical` | 2 / 3 / 50 / 5 |
| `stressed` | `CostProfile::WorstCase` | 10 / 15 / 150 / 20, 10% participation cap, two-bar delay |
| `realistic` | `CostProfile::Realistic` | typical, plus seeded fill delay, partial fills and queue-position slippage |

`CostProfile::name()` is the one place that maps a variant to its reported name.

## Synthetic data

For tests, calibration, and the luck floor, the engine produces deterministic synthetic
price paths from a seed. There are two generators:

- `synthetic(...)`: the baseline geometric random walk, one RNG draw per bar.
- `synthetic_parameterized(...)`: the same walk with two extra knobs, a volatility
  multiplier and a jump probability, for generating higher-volatility or jump-prone
  regimes. It consumes the RNG identically to `synthetic` when `vol_mult = 1.0` and
  `jump_prob = 0.0`, so the baseline is reproduced exactly, and prices are kept strictly
  positive by flooring the per-bar growth factor.

Because both are seeded and use the fixed-reduction-order arithmetic, a synthetic
scenario is a stable fixture: the same seed yields the same bars forever, which is what
lets the luck floor and the regression tests assert byte-identical results.

## State snapshots

`clone_state` and `restore_state` capture and restore the mutable cursor of a running
environment in `O(1)`:

- `clone_state() -> EnvState` snapshots where the run currently is, including the seeded
  execution-noise RNG cursor. The immutable config (data, window, costs, seed) is not
  copied, since it never changes, so a snapshot is cheap.
- `restore_state(state)` rewinds the environment to a snapshot, after which it produces
  the exact byte-identical trajectory it would have produced from that point.

This is what makes what-if rollouts and tree search over the same frozen data cheap:
branch from a snapshot, explore, restore, branch again, without re-running the backtest
from the start. Because the RNG cursor is part of the snapshot, every branch stays
deterministic.
