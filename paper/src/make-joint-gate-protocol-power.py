#!/usr/bin/env python3
"""Sample-size and feasibility calculations behind the joint-gate protocol.

Every count in `docs/studies/joint-gate/PROTOCOL.md` comes from this file. Four
calculations, each a design input and none of them a validation result:

1. The Neyman-Pearson bound: under serially independent normal returns no rule
   of any geometry beats the most powerful test of the mean on the same history.
   This is what makes a "no feasible geometry" outcome a statement about the
   data and not about the candidates tried.
2. A candidate-rule screen. A Monte Carlo over window Sharpes with the normal
   sampling approximation `joint_gate_power.sharpe_pass_probability` uses,
   including the field effect the fixed-bar calculation leaves out: the peers'
   Sharpes are drawn too, so measured dispersion can raise the bar, and a skilled
   focal entrant raises its own.
3. Replicate counts from the Wilson interval the study reports under, by a
   search that, like `sharpebench_study::required_simulation_runs`, does not
   assume the width is monotone in the count.
4. Operating characteristics of the decision rule at those counts, by exact
   binomial enumeration.

    python paper/src/make-joint-gate-protocol-power.py <out.json>

Deterministic: the seed is fixed and no clock is read. The screen's draws are
not study draws: the study's generators and seeds are unrelated to these.
"""

from __future__ import annotations

import json
import math
import sys

import numpy as np
from scipy import stats

PERIODS_PER_YEAR = 252
FIELD_SIZE = 12
SCREEN_REPLICATIONS = 400_000
SEED = 20261031
LEVEL_Z = 2.5758293035489004  # ConfidenceLevel::NinetyNine
EULER = 0.5772156649015329
HISTORY_YEARS = (10, 20)
EFFECTS = (0.75, 1.0, 1.25, 1.5, 2.0, 3.0)
HALF_WIDTH = 0.02
WHOLE_FIELD_LIMIT = 0.05
POWER_BOUND = 0.80
DETECTION_BOUND = 0.99


def z(p):
    return float(stats.norm.ppf(p))


def expected_max_multiplier(n):
    """The multiplier `expected_max_sharpe` puts on the dispersion."""
    if n <= 1:
        return 0.0
    return (1.0 - EULER) * z(1.0 - 1.0 / n) + EULER * z(1.0 - 1.0 / (n * math.e))


def wilson(events, trials, zeta=LEVEL_Z):
    x, n, z2 = float(events), float(trials), zeta * zeta
    denominator = n + z2
    center = (x + z2 / 2.0) / denominator
    spread = zeta / denominator * math.sqrt(x * (n - x) / n + z2 / 4.0)
    return max(0.0, center - spread), min(1.0, center + spread)


def wilson_half_width(rate, n, zeta=LEVEL_Z):
    lower, upper = wilson(min(round(rate * n), n), n, zeta)
    return (upper - lower) / 2.0


def required_runs(rate, half_width, zeta=LEVEL_Z):
    """Smallest n with Wilson half width at most `half_width` at `rate`."""
    n = max(1, int(zeta * zeta * max(rate * (1 - rate), 1e-9) / half_width**2))
    while wilson_half_width(rate, n, zeta) > half_width:
        n = int(n * 1.02) + 1
    floor = max(1, int(n * 0.9))
    for candidate in range(floor, n + 1):
        if wilson_half_width(rate, candidate, zeta) <= half_width:
            return candidate
    return n


def zero_event_runs(limit, zeta=LEVEL_Z):
    """Smallest n whose Wilson upper bound at zero events is at most `limit`."""
    return math.ceil(zeta * zeta * (1.0 - limit) / limit)


def all_detected_runs(bound, zeta=LEVEL_Z):
    """Smallest n whose Wilson lower bound with every replicate detected reaches
    `bound`."""
    return math.ceil(zeta * zeta * bound / (1.0 - bound))


def neyman_pearson_rows():
    """The largest power any level-alpha rule has at (s, years): the one-sided
    test of the mean, Phi(s sqrt(years) - z_{1-alpha})."""
    rows = []
    for field_limit in (WHOLE_FIELD_LIMIT, 0.01):
        alpha = 1.0 - (1.0 - field_limit) ** (1.0 / FIELD_SIZE)
        for years in HISTORY_YEARS:
            for s in EFFECTS:
                rows.append(
                    {
                        "whole_field_limit": field_limit,
                        "per_entry_alpha": alpha,
                        "years": years,
                        "effect_annual": s,
                        "power_ceiling": float(
                            stats.norm.cdf(s * math.sqrt(years) - z(1.0 - alpha))
                        ),
                    }
                )
            rows.append(
                {
                    "whole_field_limit": field_limit,
                    "per_entry_alpha": alpha,
                    "years": years,
                    "smallest_effect_at_80_percent": (z(1.0 - alpha) + z(0.8))
                    / math.sqrt(years),
                }
            )
    return rows


def years_for_rows():
    rows = []
    for alpha in (1e-6, 2e-4, WHOLE_FIELD_LIMIT / FIELD_SIZE, 0.01 / FIELD_SIZE):
        for s in (1.0, 1.5, 2.0):
            rows.append(
                {
                    "per_entry_alpha": alpha,
                    "effect_annual": s,
                    "years_at_80_percent": ((z(1.0 - alpha) + z(0.8)) / s) ** 2,
                }
            )
    return rows


def psr_threshold(n, z_bar, benchmark):
    """Smallest passing per-period Sharpe at normal moments: the root of the
    squared gate on the side of the benchmark that `z_bar` selects, as
    `joint_gate_power.min_passing_sharpe` with skew 0 and kurtosis 3."""
    big_a = (n - 1.0) - z_bar * z_bar * 0.5
    b = np.asarray(benchmark, dtype=float)
    if big_a <= 0.0:
        return np.full_like(b, np.inf)
    big_b = -2.0 * b * (n - 1.0)
    big_c = b * b * (n - 1.0) - z_bar * z_bar
    root = np.sqrt(big_b * big_b - 4.0 * big_a * big_c)
    sign = 1.0 if z_bar >= 0.0 else -1.0
    return (-big_b + sign * root) / (2.0 * big_a)


def screen_rule(rng, rule, years, effect):
    """Pass rate of the focal entrant and the any-false rate, on one set of
    draws. Sharpes are per period, sampled normal with the Lo (2002) variance."""
    bars = int(years * PERIODS_PER_YEAR)
    windows = rule["windows"]
    w_bars = bars // windows
    s = effect / math.sqrt(PERIODS_PER_YEAR)
    reps = SCREEN_REPLICATIONS
    null_se = math.sqrt(1.0 / (w_bars - 1.0))
    window_sharpes = null_se * rng.standard_normal((reps, FIELD_SIZE, windows))
    if effect != 0.0:
        focal_se = math.sqrt((1.0 + 0.5 * s * s) / (w_bars - 1.0))
        window_sharpes[:, 0, :] = s + focal_se * rng.standard_normal((reps, windows))
    pooled = window_sharpes.mean(axis=2)
    prior = rule["prior"] / math.sqrt(PERIODS_PER_YEAR)
    dispersion = np.maximum(pooled.std(axis=1, ddof=1), prior)
    bar = dispersion * expected_max_multiplier(max(rule["host_n"], FIELD_SIZE))
    dsr = pooled >= psr_threshold(bars, z(rule["dsr_bar"]), bar)[:, None]
    run_threshold = float(psr_threshold(w_bars, z(rule["per_run_bar"]), 0.0))
    needed = math.ceil(rule.get("share", 1.0) * windows)
    reliable = (window_sharpes >= run_threshold).sum(axis=2) >= needed
    boot = pooled >= z(0.95) / math.sqrt(bars)
    eligible = dsr & reliable & boot
    if effect == 0.0:
        return {
            "per_entry": float(eligible.mean()),
            "whole_field": float(eligible.any(axis=1).mean()),
        }
    return {
        "power": float(eligible[:, 0].mean()),
        "dsr_leg": float(dsr[:, 0].mean()),
        "reliability_leg": float(reliable[:, 0].mean()),
        "mean_bar_annual": float(bar.mean() * math.sqrt(PERIODS_PER_YEAR)),
    }


CANDIDATES = [
    {"id": "R0-shipped", "host_n": 50, "prior": 0.5, "windows": 6, "per_run_bar": 0.90, "dsr_bar": 0.95},
    {"id": "R1-three-windows", "host_n": 50, "prior": 0.5, "windows": 3, "per_run_bar": 0.75, "dsr_bar": 0.95},
    {"id": "R2-pooled", "host_n": 50, "prior": 0.5, "windows": 1, "per_run_bar": 0.50, "dsr_bar": 0.95},
    {"id": "R3-majority", "host_n": 50, "prior": 0.5, "windows": 6, "per_run_bar": 0.90, "dsr_bar": 0.95, "share": 2.0 / 3.0},
    {"id": "R4-field-n", "host_n": 12, "prior": 0.5, "windows": 6, "per_run_bar": 0.75, "dsr_bar": 0.95},
    {"id": "R5-low-prior", "host_n": 50, "prior": 0.25, "windows": 6, "per_run_bar": 0.75, "dsr_bar": 0.95},
    {"id": "R6-field-n-low-prior-majority", "host_n": 12, "prior": 0.25, "windows": 6, "per_run_bar": 0.75, "dsr_bar": 0.95, "share": 2.0 / 3.0},
    {"id": "R7-field-n-low-prior-three", "host_n": 12, "prior": 0.25, "windows": 3, "per_run_bar": 0.60, "dsr_bar": 0.95},
    {"id": "R8-field-n-low-prior-pooled", "host_n": 12, "prior": 0.25, "windows": 1, "per_run_bar": 0.50, "dsr_bar": 0.95},
    {"id": "R9-frontier", "host_n": 12, "prior": 0.25, "windows": 1, "per_run_bar": 0.50, "dsr_bar": 0.80},
]


def screen():
    rng = np.random.default_rng(SEED)
    rows = []
    for rule in CANDIDATES:
        for years in HISTORY_YEARS:
            rows.append(
                {
                    "rule": rule["id"],
                    "years": years,
                    "null": screen_rule(rng, rule, years, 0.0),
                    "power": {str(s): screen_rule(rng, rule, years, s) for s in EFFECTS},
                }
            )
    return rows


def decision_operating_characteristics(fields, limit, bound):
    """P(the decision rule supports the claim) at true rates, exactly."""
    upper_k = 0
    while upper_k + 1 <= fields and wilson(upper_k + 1, fields)[1] <= limit:
        upper_k += 1
    lower_k = 0
    while lower_k <= fields and wilson(lower_k, fields)[0] < bound:
        lower_k += 1
    return {
        "fields": fields,
        "largest_passing_false_count": upper_k,
        "smallest_passing_power_count": lower_k,
        "size": [
            {"true_rate": r, "probability_supported": float(stats.binom.cdf(upper_k, fields, r))}
            for r in (limit / 4, limit / 2, 0.75 * limit, limit)
        ],
        "power": [
            {"true_power": p, "probability_supported": float(stats.binom.sf(lower_k - 1, fields, p))}
            for p in (bound, bound + 0.02, bound + 0.03, bound + 0.05, bound + 0.1)
        ],
    }


def main():
    if len(sys.argv) != 2:
        print(__doc__, file=sys.stderr)
        return 2
    primary = required_runs(0.5, HALF_WIDTH)
    detection = all_detected_runs(DETECTION_BOUND)
    document = {
        "tier": "design calculation, not validation",
        "field_size": FIELD_SIZE,
        "level_z": LEVEL_Z,
        "expected_max_multiplier": {str(n): expected_max_multiplier(n) for n in (12, 50, 200)},
        "neyman_pearson": neyman_pearson_rows(),
        "years_at_80_percent": years_for_rows(),
        "screen": screen(),
        "replicates": {
            "half_width": HALF_WIDTH,
            "at_rate_0.5": primary,
            "at_rate_0.8": required_runs(POWER_BOUND, HALF_WIDTH),
            "at_rate_0.05": required_runs(WHOLE_FIELD_LIMIT, HALF_WIDTH),
            "zero_event_upper_at_limit": zero_event_runs(WHOLE_FIELD_LIMIT),
            "all_detected_lower_at_0.99": detection,
        },
        "operating_characteristics": decision_operating_characteristics(
            primary, WHOLE_FIELD_LIMIT, POWER_BOUND
        ),
        "resolution_limit": [
            {
                "fields": n,
                "rule_of_three": 3.0 / n,
                "wilson_upper_at_zero": wilson(0, n)[1],
                "exact_one_sided_0.005": 1.0 - 0.005 ** (1.0 / n),
            }
            for n in (1_000, primary, 64_000, 300_000)
        ],
    }
    with open(sys.argv[1], "w", encoding="utf-8", newline="\n") as handle:
        json.dump(document, handle, indent=1, sort_keys=True)
        handle.write("\n")
    return 0


if __name__ == "__main__":
    sys.exit(main())
