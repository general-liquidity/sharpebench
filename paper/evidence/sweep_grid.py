"""Declared principal-sweep geometry, independent of whichever cells arrive.

This is the paper's four-bar, four-trial, four-prior, eight-entrant grid, not a
general submission schema. An intentional grid change must update this contract.
Validation establishes structural coverage, not correctness of scored outcomes.
"""

from __future__ import annotations

import itertools
import json
import math
from pathlib import Path

DATASETS = {
    "us-indices-1d": ("equity-index", "1d", 252),
    "us-indices-1w": ("equity-index", "1w", 52),
    "crypto-majors-1h": ("crypto", "1h", 8760),
    "crypto-majors-4h": ("crypto", "4h", 2190),
    "crypto-majors-1d": ("crypto", "1d", 365),
    "crypto-majors-1w": ("crypto", "1w", 52),
    "fx-majors-1d": ("fx", "1d", 252),
    "commodities-1d": ("commodities", "1d", 252),
    "rates-1d": ("rates", "1d", 252),
}
DSR_BARS = (0.80, 0.90, 0.95, 0.99)
N_TRIALS = (1, 10, 50, 200)
SR_STD = (None, 0.20, 0.35, 0.50)
AGENTS = ("buy-and-hold", "momentum", "hold") + tuple(
    f"luck-floor-{i:02}" for i in range(5)
)
GEOMETRY = ("n_bars", "n_symbols", "n_windows", "window_len", "n_seeds", "regimes")
CONFIGURATION = (
    "effective_n_trials",
    "min_field_for_measured_sr_std",
    "dedup_clones_for_measured_sr_std",
    "min_measured_trials_sr_std_annualized",
    "deflation_null_mean_per_period",
)


class GridError(ValueError):
    """The records do not cover the declared principal sweep."""


def _object(pairs):
    result = {}
    for key, value in pairs:
        if key in result:
            raise GridError(f"duplicate JSON key {key!r}")
        result[key] = value
    return result


def _finite(value):
    if isinstance(value, float) and not math.isfinite(value):
        raise GridError("nonfinite JSON number")
    if isinstance(value, dict):
        for item in value.values():
            _finite(item)
    elif isinstance(value, list):
        for item in value:
            _finite(item)


def read_records(path: Path):
    """Return (decoded record, original line) pairs; never skip malformed lines."""
    result = []
    for number, line in enumerate(path.read_text(encoding="utf-8").splitlines(), 1):
        try:
            row = json.loads(line, object_pairs_hook=_object)
            _finite(row)
            if not isinstance(row, dict):
                raise GridError("record must be an object")
        except (ValueError, RecursionError) as exc:
            raise GridError(f"{path}:{number}: invalid JSON: {exc}") from exc
        result.append((row, line))
    return result


def expected_keys(bars=DSR_BARS):
    return tuple(itertools.product(bars, N_TRIALS, SR_STD, AGENTS))


def _key(row):
    bar, trials, prior, agent = (
        row.get(k) for k in ("dsr_bar", "n_trials", "sr_std_pinned", "agent_id")
    )
    if (
        type(bar) not in (int, float)
        or bar not in DSR_BARS
        or type(trials) is not int
        or trials not in N_TRIALS
        or "sr_std_pinned" not in row
        or (
            prior is not None
            and (type(prior) not in (int, float) or prior not in SR_STD)
        )
        or not isinstance(agent, str)
        or agent not in AGENTS
    ):
        raise GridError("invalid grid key (dsr_bar, n_trials, sr_std_pinned, agent_id)")
    return bar, trials, prior, agent


def _metadata(row, dataset):
    if (
        not isinstance(dataset, str)
        or dataset not in DATASETS
        or row.get("dataset") != dataset
    ):
        raise GridError("metadata: dataset does not match the declared input")
    class_, timeframe, ppy = DATASETS[dataset]
    if (row.get("asset_class"), row.get("timeframe"), row.get("periods_per_year")) != (
        class_,
        timeframe,
        ppy,
    ):
        raise GridError("metadata: asset class, timeframe or periods_per_year differs")
    for name in GEOMETRY[:-1]:
        if type(row.get(name)) is not int or row[name] <= 0:
            raise GridError(f"metadata: {name} must be a positive integer")
    regimes = row.get("regimes")
    if (
        row["n_seeds"] != 8
        or not isinstance(regimes, list)
        or len(regimes) != row["n_windows"]
        or any(not isinstance(r, str) or not r for r in regimes)
    ):
        raise GridError("metadata: invalid seed count or window regime labels")
    return tuple(row[name] for name in GEOMETRY)


def _renderer_fields(row):
    """Require renderer inputs, not merely enough fields to identify a cell."""
    for name in ("passed_k", "rank_eligible", "eligible_never_catastrophic"):
        if type(row.get(name)) is not bool:
            raise GridError(f"invalid renderer field: {name} must be boolean")
    for name in (
        "deflated_sharpe",
        "worst_run_drawdown",
        "bootstrap_p",
        "raw_mean_return",
        "trials_sr_std_annualized_equivalent",
        "deflation_bar_annualized_equivalent",
        "min_measured_trials_sr_std_annualized",
        "deflation_null_mean_per_period",
    ):
        value = row.get(name)
        try:
            finite = type(value) in (int, float) and math.isfinite(value)
        except OverflowError:
            finite = False
        if not finite:
            raise GridError(
                f"invalid renderer field: {name} must be finite numeric data"
            )
    for name in (
        "pooled_observations",
        "effective_n_trials",
        "min_field_for_measured_sr_std",
    ):
        if type(row.get(name)) is not int or row[name] <= 0:
            raise GridError(
                f"invalid renderer field: {name} must be a positive integer"
            )
    if row.get("trials_sr_std_source") not in (
        "configured",
        "measured",
        "measured_floored",
    ):
        raise GridError("invalid renderer field: unknown dispersion source")
    if type(row.get("dedup_clones_for_measured_sr_std")) is not bool:
        raise GridError("metadata: clone deduplication control must be boolean")


def validate_grid(records, dataset, *, bars=DSR_BARS):
    """Require each expected cell exactly once, then return canonical key order.

    Counts, regimes and dataset descriptors must agree across the entire grid.
    No expected axis is inferred from the records themselves. Output numbers are
    required to be finite by read_records, not rescored or statistically revalidated.
    """
    keys = expected_keys(bars)
    expected = set(keys)
    found = {}
    geometry = None
    configurations = {}
    for row, line in records:
        key = _key(row)
        if key not in expected:
            raise GridError(f"invalid grid key outside requested shard: {key!r}")
        if key in found:
            raise GridError(f"duplicate cell: {key!r}")
        current = _metadata(row, dataset)
        _renderer_fields(row)
        if geometry is None:
            geometry = current
        elif geometry != current:
            raise GridError("metadata: dataset geometry differs between cells")
        configuration = tuple(row[name] for name in CONFIGURATION)
        if key[:3] in configurations and configurations[key[:3]] != configuration:
            raise GridError(
                "metadata: effective configuration differs between entrant rows"
            )
        configurations[key[:3]] = configuration
        found[key] = (row, line)
    missing = expected - found.keys()
    if missing:
        example = next(key for key in keys if key in missing)
        raise GridError(f"missing {len(missing)} expected cells; first: {example!r}")
    return [found[key] for key in keys]
