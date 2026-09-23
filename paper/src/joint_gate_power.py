"""Shared pieces of the development-tier joint-gate calibration.

`make-joint-gate-power.py` is a hyphenated script, so the parts another module
or a test wants to import directly live here instead: the return generator, the
three legs' per-replication thresholds, the interval methods and the closed-form
gate-design comparison.

Tier: development calibration (section 10.A of the remaining-product-work plan).
Nothing here is frozen validation and nothing here may be reported as a
validated claim. No threshold here is tuned against a held-out set.
"""

from __future__ import annotations

import math
import os
import sys

import numpy as np

HERE = os.path.dirname(os.path.abspath(__file__))
if HERE not in sys.path:
    sys.path.insert(0, HERE)

import kernel_stats  # noqa: E402

# The shipped joint predicate's statistical legs, as `composite.rs` builds
# `rank_eligible`: dsr >= dsr_bar, pass^k over the runs, and bootstrap_p < alpha.
DSR_BAR = 0.95
PER_RUN_PSR_BAR = 0.90
N_WINDOWS = 6
BOOTSTRAP_ALPHA = 0.05
BOOTSTRAP_N_BOOT = 2000
BOOTSTRAP_BLOCK_PROB = 0.1


class PowerSupportError(ValueError):
    """The request is not supported by the committed records or by the method."""


# ---- return generation ----------------------------------------------------------
def draw_track(rng, size, length, rho):
    """`size` independent tracks of `length` zero-mean unit-variance returns.

    `rho` is the first-order autocorrelation of a stationary Gaussian AR(1),
    normalized so the marginal standard deviation is 1 whatever `rho` is. A true
    per-period Sharpe of `s` is then the constant `s` added to the track, the
    same effect definition as the serially independent case at `rho = 0`.
    """
    if not -0.99 <= rho <= 0.99:
        raise PowerSupportError(f"rho {rho} is outside the modelled range")
    track = rng.standard_normal((size, length))
    if rho == 0.0:
        return track
    scale = math.sqrt(1.0 - rho * rho)
    track[:, 0] /= scale
    for t in range(1, length):
        track[:, t] += rho * track[:, t - 1]
    track *= scale
    return track


# ---- the three legs' thresholds -------------------------------------------------
def min_passing_sharpe(n, skew, kurt, z_bar, benchmark):
    """Smallest observed per-period Sharpe passing eq:psr at `benchmark`.

    With a = (kurt - 1) / 4, the gate (u - b) sqrt(n - 1) >= z sqrt(1 - skew u +
    a u^2) with u > b squares to A u^2 + B u + C >= 0, where A = n - 1 - z^2 a,
    B = z^2 skew - 2 b (n - 1) and C = b^2 (n - 1) - z^2. The quadratic is
    -z^2 (1 - skew b + a b^2) < 0 at u = b, so b lies strictly between its roots
    and the passing set is [larger root, infinity) whenever A > 0. Elementwise.
    Same formula as `make-power-curve.py`; `test_joint_gate_power.py` pins the
    two against each other and both against the kernel.
    """
    n = float(n)
    a = (np.asarray(kurt, dtype=float) - 1.0) / 4.0
    z2 = z_bar * z_bar
    big_a = (n - 1.0) - z2 * a
    if np.any(big_a <= 0.0):
        raise PowerSupportError(
            f"kurtosis too large for a closed-form threshold over {int(n)} returns"
        )
    big_b = z2 * np.asarray(skew, dtype=float) - 2.0 * benchmark * (n - 1.0)
    big_c = benchmark * benchmark * (n - 1.0) - z2
    root = np.sqrt(big_b * big_b - 4.0 * big_a * big_c)
    q = -0.5 * (big_b + np.where(big_b >= 0.0, root, -root))
    return np.maximum(q / big_a, big_c / q)


def threshold_true_sharpe(z, z_bar, benchmark):
    """Per series (last axis), the true per-period Sharpe at and above which the
    shifted series passes PSR >= the bar whose z threshold is `z_bar`.

    The kernel's Sharpe is the mean over the n - 1 standard deviation, and its
    skewness and kurtosis are population-normalized; all three come from the
    unshifted series because adding a constant leaves dispersion and shape alone.
    """
    n = z.shape[-1]
    center = z.mean(axis=-1)
    dev = z - center[..., None]
    dev2 = dev * dev
    m2 = dev2.mean(axis=-1)
    skew = (dev2 * dev).mean(axis=-1) / m2**1.5
    kurt = (dev2 * dev2).mean(axis=-1) / (m2 * m2)
    sd = np.sqrt(m2 * n / (n - 1.0))
    u = min_passing_sharpe(n, skew, kurt, z_bar, benchmark)
    return u * sd - center


def window_thresholds(track, n_windows, z_bar):
    """Per track, each window's threshold true Sharpe for the per-run PSR bar
    against zero. Windows are contiguous slices of the pooled track, so serial
    dependence carries across the window boundaries."""
    size, length = track.shape
    if length % n_windows:
        raise PowerSupportError(f"track length {length} is not {n_windows} equal windows")
    windows = track.reshape(size, n_windows, length // n_windows)
    return threshold_true_sharpe(windows, z_bar, 0.0)


def bootstrap_keep(alpha, n_boot):
    """How many resampled means may reach the observed one and still reject.

    The kernel's p-value is (count + 1) / (n_boot + 1) with count the number of
    resampled means at or above the observed mean, and the gate is p < alpha.
    The rejection region is therefore count <= keep - 1, which is the same as
    the observed mean strictly exceeding the `keep`-th largest resampled mean.
    """
    keep = math.ceil(alpha * (n_boot + 1)) - 1
    if not 1 <= keep <= n_boot:
        raise PowerSupportError(
            f"alpha {alpha} with {n_boot} resamples leaves no attainable rejection region"
        )
    return keep


def bootstrap_threshold(track, rng, n_boot, block_prob, alpha):
    """Per track, the true Sharpe at and above which the shipped bootstrap leg
    passes, that is `bootstrap_pvalue(pooled) < alpha`.

    The kernel resamples blocks of the *centered* track, so its null
    distribution of resampled means does not move when a constant is added to
    the track, while the observed mean does. The leg is therefore a threshold on
    the observed mean and one set of resamples per replication gives the whole
    curve, exactly as the closed-form threshold does for the other two legs.

    The resampler is NumPy's, not the kernel's fixed-seed SplitMix64 draw: the
    leg is modelled as a statistic averaged over its own resampling noise, while
    the shipped gate fixes one seed for every agent. This does not reproduce
    that seed's particular draw.
    """
    keep = bootstrap_keep(alpha, n_boot)
    size, length = track.shape
    observed = track.mean(axis=1)
    centered = track - observed[:, None]
    index = rng.integers(0, length, (size, n_boot))
    total = np.zeros((size, n_boot))
    for _ in range(length):
        total += np.take_along_axis(centered, index, axis=1)
        restart = rng.random((size, n_boot)) < block_prob
        index = np.where(restart, rng.integers(0, length, (size, n_boot)), index + 1)
        np.mod(index, length, out=index)
    total /= length
    kth = np.partition(total, n_boot - keep, axis=1)[:, n_boot - keep]
    return kth - observed


# ---- intervals ------------------------------------------------------------------
def _log_binomial_coefficients(k, n):
    """log C(n, i) for i = 0..k, from the ratio recursion in log space."""
    if k == 0:
        return np.zeros(1)
    i = np.arange(1, k + 1, dtype=float)
    return np.concatenate(([0.0], np.cumsum(np.log(n - i + 1.0) - np.log(i))))


def binomial_cdf(k, n, p, log_coefficients=None):
    """P(X <= k) for X ~ Binomial(n, p), summed in log space.

    The binomial coefficients do not depend on p, so a caller that evaluates the
    same (k, n) at many p values passes them in once. Without that the bisection
    in `clopper_pearson` recomputes k terms per step, which is the whole cost of
    an exact interval at a count in the thousands.
    """
    if p <= 0.0:
        return 1.0
    if p >= 1.0:
        return 1.0 if k >= n else 0.0
    if log_coefficients is None:
        log_coefficients = _log_binomial_coefficients(k, n)
    i = np.arange(k + 1, dtype=float)
    terms = log_coefficients + i * math.log(p) + (n - i) * math.log1p(-p)
    return min(float(np.exp(terms).sum()), 1.0)


def wilson_interval(k, n, alpha):
    """Wilson score interval at two-sided level 1 - 2 * alpha.

    Used where the count is a substantial fraction of the replications, which is
    every point of a power curve away from its tails. It costs one closed form
    rather than a bisection over an exact tail of k terms, and at those counts
    the normal approximation it rests on is the one a power curve reports anyway.
    Rare-event bounds, where that approximation fails, use `clopper_pearson`.
    """
    if n <= 0 or not 0 <= k <= n:
        raise PowerSupportError(f"{k} successes out of {n} is not a binomial outcome")
    z = -kernel_stats.norm_ppf(alpha)
    phat = k / n
    denominator = 1.0 + z * z / n
    center = (phat + z * z / (2.0 * n)) / denominator
    half = (z / denominator) * math.sqrt(phat * (1.0 - phat) / n + z * z / (4.0 * n * n))
    return max(0.0, center - half), min(1.0, center + half)


def clopper_pearson(k, n, alpha):
    """Exact binomial bounds at one-sided level `alpha`, as (lower, upper).

    The upper bound solves P(X <= k) = alpha and the lower P(X >= k) = alpha, by
    bisection on the exact tail. Passing alpha/2 gives the usual two-sided
    interval. At k = 0 the upper bound is 1 - alpha**(1/n): a run with no
    observed false positive can claim that bound and not a zero rate.
    """
    if n <= 0 or not 0 <= k <= n:
        raise PowerSupportError(f"{k} successes out of {n} is not a binomial outcome")
    if not 0.0 < alpha < 1.0:
        raise PowerSupportError(f"alpha {alpha} is not in (0, 1)")
    if k == n:
        upper = 1.0
    else:
        coefficients = _log_binomial_coefficients(k, n)
        lo, hi = 0.0, 1.0
        for _ in range(200):
            mid = 0.5 * (lo + hi)
            if binomial_cdf(k, n, mid, coefficients) > alpha:
                lo = mid
            else:
                hi = mid
        upper = 0.5 * (lo + hi)
    if k == 0:
        lower = 0.0
    else:
        coefficients = _log_binomial_coefficients(k - 1, n)
        lo, hi = 0.0, 1.0
        for _ in range(200):
            mid = 0.5 * (lo + hi)
            if 1.0 - binomial_cdf(k - 1, n, mid, coefficients) < alpha:
                lo = mid
            else:
                hi = mid
        lower = 0.5 * (lo + hi)
    return lower, upper


def rate_with_interval(passes, replications, alpha=0.05, exact=True):
    """Observed rate, its Monte Carlo standard error and an interval.

    `exact` selects Clopper-Pearson, which is what a rare-event bound needs and
    what a run with no observed failure must report. Power points pass
    `exact=False` for the Wilson interval; see `wilson_interval`.
    """
    p = passes / replications
    se = math.sqrt(p * (1.0 - p) / replications)
    interval = clopper_pearson if exact else wilson_interval
    lower, upper = interval(passes, replications, alpha)
    return p, se, lower, upper


def field_probability(per_entry, field_size):
    """P(at least one of `field_size` independent zero-skill entries passes).

    Entries in a real field share one market history, so their verdicts are not
    independent and this is not the field rate such a field would show. It is
    the independent-entry value and is reported under that name.
    """
    return 1.0 - (1.0 - per_entry) ** field_size


# ---- closed-form gate-design comparison ------------------------------------------
def sharpe_pass_probability(n, s, benchmark, z_bar):
    """P(one PSR test over `n` returns passes) at true per-period Sharpe `s`.

    The observed Sharpe passes when it reaches the kernel's own threshold at
    normal moments, and under serially independent normal returns its sampling
    distribution is approximately normal with mean `s` and variance
    (1 + s^2 / 2) / (n - 1) (Lo 2002). The Monte Carlo legs above do not use
    this approximation. It is here so a *design* comparison over history lengths
    can be solved instead of simulated at every candidate length, and
    `test_joint_gate_power.py` checks it against the Monte Carlo at the lengths
    the comparison reports.
    """
    if n - 1.0 <= 0.5 * z_bar * z_bar:
        # No observed Sharpe clears the bar over so few returns: the plug-in
        # standard error at the observed Sharpe grows faster than the estimate.
        return 0.0
    threshold = float(min_passing_sharpe(n, 0.0, 3.0, z_bar, benchmark))
    sd = math.sqrt((1.0 + 0.5 * s * s) / (n - 1.0))
    return kernel_stats.norm_cdf((s - threshold) / sd)


def binomial_at_least(m, k, q):
    return sum(math.comb(k, j) * q**j * (1.0 - q) ** (k - j) for j in range(m, k + 1))


class GateDesign:
    """A candidate eligibility rule over a total history of `bars` returns.

    The history splits into `windows` equal windows and `required` of them must
    pass a one-sided PSR test at level `alpha_per_test` against zero. One window
    requiring one pass is a pooled rule; six requiring six is the shipped pass^k
    leg; six requiring four is a majority variant.
    """

    def __init__(self, name, windows, required, alpha_per_test):
        if not 1 <= required <= windows:
            raise PowerSupportError(f"{required} of {windows} is not a rule")
        if not 0.0 < alpha_per_test < 1.0:
            raise PowerSupportError(f"per-test level {alpha_per_test} is not in (0, 1)")
        self.name = name
        self.windows = windows
        self.required = required
        self.alpha_per_test = alpha_per_test
        self.psr_bar = 1.0 - alpha_per_test
        self.z_bar = kernel_stats.norm_cdf_inverse(self.psr_bar)

    @property
    def nominal_false_positive_rate(self):
        """Per-entry rate at a true Sharpe of zero with each test at its nominal
        level. The Monte Carlo legs measure the realized rate, which differs
        because the shipped PSR uses sample moments."""
        return binomial_at_least(self.required, self.windows, self.alpha_per_test)

    def pass_probability(self, bars, s):
        if bars % self.windows:
            raise PowerSupportError(f"{bars} bars is not {self.windows} equal windows")
        per_window = sharpe_pass_probability(bars // self.windows, s, 0.0, self.z_bar)
        return binomial_at_least(self.required, self.windows, per_window)

    def required_bars(self, s, power, max_bars=40_000_000):
        """Smallest total history, a whole number of windows, whose pass
        probability at true per-period Sharpe `s` reaches `power`."""
        if not 0.0 < power < 1.0:
            raise PowerSupportError(f"target power {power} is not in (0, 1)")
        step = self.windows
        lo = hi = step * (int(self.z_bar * self.z_bar) + 4)
        while self.pass_probability(hi, s) < power:
            lo, hi = hi, hi * 2
            if hi > max_bars:
                raise PowerSupportError(
                    f"{self.name} does not reach power {power} at per-period Sharpe {s} "
                    f"within {max_bars} bars"
                )
        while hi - lo > step:
            mid = ((lo + hi) // (2 * step)) * step
            if mid <= lo:
                break
            if self.pass_probability(mid, s) < power:
                lo = mid
            else:
                hi = mid
        return hi


def design_at_false_positive_rate(name, windows, required, target_rate):
    """The design of that shape whose nominal per-entry rate is `target_rate`."""
    if not 0.0 < target_rate < 1.0:
        raise PowerSupportError(f"target rate {target_rate} is not in (0, 1)")
    lo, hi = 1e-15, 1.0 - 1e-15
    for _ in range(400):
        mid = math.sqrt(lo * hi)
        if binomial_at_least(required, windows, mid) < target_rate:
            lo = mid
        else:
            hi = mid
    design = GateDesign(name, windows, required, 0.5 * (lo + hi))
    achieved = design.nominal_false_positive_rate
    if abs(achieved - target_rate) > 1e-8 * target_rate:
        raise PowerSupportError(
            f"{name}: solved rate {achieved} does not reach the requested {target_rate}"
        )
    return design


def compare_designs(design_a, design_b, s_annualized, periods_per_year, power):
    """History each design needs for `power` at the same effect, and the ratio."""
    s = s_annualized / math.sqrt(periods_per_year)
    rate_a = design_a.nominal_false_positive_rate
    rate_b = design_b.nominal_false_positive_rate
    bars_a = design_a.required_bars(s, power)
    bars_b = design_b.required_bars(s, power)
    return {
        "design_a": design_a.name,
        "design_b": design_b.name,
        "false_positive_rate_matched": abs(math.log(rate_a) - math.log(rate_b)) < 1e-9,
        "nominal_false_positive_rate_a": rate_a,
        "nominal_false_positive_rate_b": rate_b,
        "per_test_psr_bar_a": design_a.psr_bar,
        "per_test_psr_bar_b": design_b.psr_bar,
        "effect_sharpe_annualized": s_annualized,
        "periods_per_year": periods_per_year,
        "target_power": power,
        "bars_a": bars_a,
        "bars_b": bars_b,
        "years_a": bars_a / periods_per_year,
        "years_b": bars_b / periods_per_year,
        "history_factor_a_over_b": bars_a / bars_b,
    }


def require_matched(comparison):
    """Refuse to hand back an unmatched comparison as a headline factor.

    The error this guards is on the record. An earlier draft reported that the
    every-window rule costs fourteen times the history of a pooled test, which
    compared a rule at a nominal rate of 1e-6 with one at 0.05. At a matched
    rate the factor is under two.
    """
    if not comparison["false_positive_rate_matched"]:
        raise PowerSupportError(
            f"{comparison['design_a']} at nominal rate "
            f"{comparison['nominal_false_positive_rate_a']:.3g} and "
            f"{comparison['design_b']} at {comparison['nominal_false_positive_rate_b']:.3g} "
            "do not share a false-positive rate, so their history ratio is not a "
            "comparison of gate designs"
        )
    return comparison
