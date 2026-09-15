"""The kernel's Sharpe statistics, transcribed for the paper's figure producers.

Each function mirrors one function of `crates/sharpebench-stats` with the same
operation order, so a figure drawn here shows the kernel's formula and not an
independent approximation of it:

- `erf`, `norm_cdf`, `norm_ppf`: `stats.rs` (Abramowitz and Stegun 7.1.26;
  Acklam's rational inverse). The kernel does not use an exact error function,
  and neither does this module.
- `mean`, `std_dev`, `skewness`, `kurtosis`: `stats.rs`. The Sharpe ratio uses
  the n - 1 sample standard deviation; the standardized third and fourth moments
  use the population normalization m2 = sum((x - mean)^2) / n, and kurtosis is
  non-excess (normal = 3).
- `sharpe_ratio`, `probabilistic_sharpe_ratio`, `expected_max_sharpe`:
  `deflated_sharpe.rs`. The PSR standard error is the plug-in one at the
  observed Sharpe with sqrt(n - 1), as in the paper's eq:psr.

`psr_from_moments` is eq:psr written over (Sharpe, skewness, kurtosis, n)
rather than over a return series, for figures that state their moments instead
of drawing a series. `test_kernel_stats.py` pins it to the Bailey and Lopez de
Prado (2014) worked example and to the committed `tab:units` bars.
"""

from __future__ import annotations

import math

EULER_GAMMA = 0.577_215_664_901_532_9
PSR_RADICAND_FLOOR = 1e-12

_A = (
    -3.969683028665376e01,
    2.209460984245205e02,
    -2.759285104469687e02,
    1.38357751867269e02,
    -3.066479806614716e01,
    2.506628277459239e00,
)
_B = (
    -5.447609879822406e01,
    1.615858368580409e02,
    -1.556989798598866e02,
    6.680131188771972e01,
    -1.328068155288572e01,
)
_C = (
    -7.784894002430293e-03,
    -3.223964580411365e-01,
    -2.400758277161838e00,
    -2.549732539343734e00,
    4.374664141464968e00,
    2.938163982698783e00,
)
_D = (
    7.784695709041462e-03,
    3.224671290700398e-01,
    2.445134137142996e00,
    3.754408661907416e00,
)
_P_LOW = 0.02425


def erf(x: float) -> float:
    sign = -1.0 if x < 0.0 else 1.0
    x = abs(x)
    t = 1.0 / (1.0 + 0.3275911 * x)
    y = 1.0 - (
        ((((1.061405429 * t - 1.453152027) * t) + 1.421413741) * t - 0.284496736) * t
        + 0.254829592
    ) * t * math.exp(-x * x)
    return sign * y


def norm_cdf(x: float) -> float:
    return 0.5 * (1.0 + erf(x / math.sqrt(2.0)))


def norm_ppf(p: float) -> float:
    if p <= 0.0:
        return -math.inf
    if p >= 1.0:
        return math.inf
    a, b, c, d = _A, _B, _C, _D
    if p < _P_LOW:
        q = math.sqrt(-2.0 * math.log(p))
        return (((((c[0] * q + c[1]) * q + c[2]) * q + c[3]) * q + c[4]) * q + c[5]) / (
            (((d[0] * q + d[1]) * q + d[2]) * q + d[3]) * q + 1.0
        )
    if p <= 1.0 - _P_LOW:
        q = p - 0.5
        r = q * q
        return (
            (((((a[0] * r + a[1]) * r + a[2]) * r + a[3]) * r + a[4]) * r + a[5])
            * q
            / (((((b[0] * r + b[1]) * r + b[2]) * r + b[3]) * r + b[4]) * r + 1.0)
        )
    q = math.sqrt(-2.0 * math.log(1.0 - p))
    return -(((((c[0] * q + c[1]) * q + c[2]) * q + c[3]) * q + c[4]) * q + c[5]) / (
        (((d[0] * q + d[1]) * q + d[2]) * q + d[3]) * q + 1.0
    )


def norm_cdf_inverse(p: float) -> float:
    """The z at which the kernel's own `norm_cdf` reaches `p`.

    A gate `norm_cdf(z) >= p` is the threshold `z >= norm_cdf_inverse(p)`. The
    kernel's CDF is an approximation, so this is found by bisection on that CDF
    rather than taken from an exact normal quantile; the two differ by under
    1e-6 near the bars used here.
    """
    if not 0.0 < p < 1.0:
        raise ValueError(f"probability {p} is not in (0, 1)")
    lo, hi = -40.0, 40.0
    for _ in range(200):
        mid = 0.5 * (lo + hi)
        if norm_cdf(mid) >= p:
            hi = mid
        else:
            lo = mid
        if hi - lo <= 4.0 * math.ulp(hi):
            break
    return hi


def mean(xs) -> float:
    if not xs:
        return 0.0
    return sum(xs) / len(xs)


def std_dev(xs) -> float:
    n = len(xs)
    if n < 2:
        return 0.0
    m = mean(xs)
    return math.sqrt(sum((x - m) * (x - m) for x in xs) / (n - 1.0))


def _population_std_dev(xs, center: float) -> float:
    return math.sqrt(sum((x - center) ** 2 for x in xs) / len(xs))


def skewness(xs) -> float:
    n = len(xs)
    if n < 2:
        return 0.0
    m = mean(xs)
    s = _population_std_dev(xs, m)
    if s == 0.0:
        return 0.0
    return sum(((x - m) / s) ** 3 for x in xs) / n


def kurtosis(xs) -> float:
    n = len(xs)
    if n < 2:
        return 3.0
    m = mean(xs)
    s = _population_std_dev(xs, m)
    if s == 0.0:
        return 3.0
    return sum(((x - m) / s) ** 4 for x in xs) / n


def sharpe_ratio(xs) -> float:
    s = std_dev(xs)
    if s == 0.0:
        return 0.0
    return mean(xs) / s


def psr_from_moments(
    sr: float, n: int, sr_benchmark: float, skew: float = 0.0, kurt: float = 3.0
) -> float:
    """eq:psr at an observed per-period Sharpe `sr` over `n` returns."""
    if n < 2:
        return 0.0
    radicand = 1.0 - skew * sr + ((kurt - 1.0) / 4.0) * sr * sr
    denom = math.sqrt(max(radicand, PSR_RADICAND_FLOOR))
    z = (sr - sr_benchmark) * math.sqrt(n - 1.0) / denom
    return norm_cdf(z)


def probabilistic_sharpe_ratio(xs, sr_benchmark: float) -> float:
    """The kernel's PSR of a return series against a per-period benchmark."""
    if len(xs) < 2:
        return 0.0
    return psr_from_moments(
        sharpe_ratio(xs), len(xs), sr_benchmark, skewness(xs), kurtosis(xs)
    )


def expected_max_sharpe(trials_sr_std: float, n_trials: int) -> float:
    """Expected maximum Sharpe of `n_trials` zero-skill trials (eq:deflation, mu0 = 0)."""
    if not math.isfinite(trials_sr_std) or trials_sr_std < 0.0:
        raise ValueError(f"trials_sr_std {trials_sr_std} is not a finite dispersion")
    n = float(max(n_trials, 1))
    if n <= 1.0 or trials_sr_std == 0.0:
        return 0.0
    z1 = norm_ppf(1.0 - 1.0 / n)
    z2 = norm_ppf(1.0 - 1.0 / (n * math.e))
    return trials_sr_std * ((1.0 - EULER_GAMMA) * z1 + EULER_GAMMA * z2)


def deflated_sharpe_from_moments(
    sr: float,
    n: int,
    n_trials: int,
    trials_sr_std: float,
    skew: float = 0.0,
    kurt: float = 3.0,
) -> float:
    """The DSR: eq:psr evaluated against the eq:deflation benchmark."""
    return psr_from_moments(
        sr, n, expected_max_sharpe(trials_sr_std, n_trials), skew, kurt
    )
