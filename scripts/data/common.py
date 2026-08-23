"""Shared helpers for the frozen-dataset fetch scripts.

Mirrors `xtask/src/main.rs::write_dataset` byte-for-byte: rows sorted by date
then symbol (ASCII), fixed decimals, LF line endings, a `<hex>  <name>.csv`
SHA-256 sidecar. The loader (`crates/sharpebench-sim/src/data.rs`) aligns on the
intersection of every symbol's dates, so we intersect here too and never write a
row the benchmark would drop.
"""

from __future__ import annotations

import hashlib
import os
import sys
import time
from typing import Dict

import requests

# symbol -> {date -> close}
Series = Dict[str, Dict[str, float]]

DATA_DIR = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "..", "data")

_SESSION = requests.Session()
_SESSION.headers["User-Agent"] = "sharpebench-data-fetch/1.0 (+https://github.com/general-liquidity/sharpebench)"


def http_get(url: str, params: dict | None = None, retries: int = 5) -> requests.Response:
    """GET with simple exponential backoff; raises on a final non-2xx."""
    delay = 1.0
    for attempt in range(retries):
        r = _SESSION.get(url, params=params, timeout=60)
        if r.status_code == 200:
            return r
        if r.status_code in (418, 429, 500, 502, 503, 504) and attempt + 1 < retries:
            time.sleep(delay)
            delay *= 2
            continue
        r.raise_for_status()
    raise RuntimeError(f"GET {url} failed after {retries} attempts")


def align(series: Series) -> list[str]:
    """Dates common to every symbol, sorted (ISO strings sort chronologically)."""
    axis = None
    for m in series.values():
        s = set(m)
        axis = s if axis is None else axis & s
    dates = sorted(axis or ())
    if len(dates) < 2:
        raise SystemExit("fewer than 2 dates common to all symbols")
    return dates


def render(series: Series, decimals: int) -> str:
    dates = align(series)
    lines = ["date,symbol,close"]
    for d in dates:
        for sym in sorted(series):
            lines.append(f"{d},{sym},{series[sym][d]:.{decimals}f}")
    return "\n".join(lines) + "\n"


def write_dataset(name: str, series: Series, decimals: int, check: bool = False) -> str:
    """Write `data/<name>.csv` + `.sha256`. With `check=True`, only compare the
    rendered bytes against the existing file and report, without writing."""
    csv = render(series, decimals)
    digest = hashlib.sha256(csv.encode("ascii")).hexdigest()
    path = os.path.join(DATA_DIR, f"{name}.csv")
    n_rows = csv.count("\n") - 1
    dates = align(series)
    if check:
        if os.path.exists(path):
            with open(path, "rb") as f:
                existing = hashlib.sha256(f.read()).hexdigest()
            status = "MATCH" if existing == digest else "DIFFERS"
            print(f"{name}.csv: regenerated sha256={digest} existing={existing} -> {status}")
        else:
            print(f"{name}.csv: regenerated sha256={digest} (no existing file)")
        return digest
    with open(path, "w", newline="\n", encoding="ascii") as f:
        f.write(csv)
    with open(path + ".sha256", "w", newline="\n", encoding="ascii") as f:
        f.write(f"{digest}  {name}.csv\n")
    print(
        f"wrote {os.path.relpath(path)}  ({n_rows} rows, {len(dates)} bars x {len(series)} symbols, "
        f"{dates[0]} .. {dates[-1]})  sha256={digest}",
        file=sys.stderr,
    )
    return digest


def fred_series(series_id: str, start: str | None = None, end: str | None = None) -> Dict[str, float]:
    """One FRED series via the keyless `fredgraph.csv` endpoint. Rows marked `.`
    (holidays / missing) are skipped, never interpolated."""
    r = http_get("https://fred.stlouisfed.org/graph/fredgraph.csv", params={"id": series_id})
    out: Dict[str, float] = {}
    for line in r.text.splitlines()[1:]:
        parts = line.split(",")
        if len(parts) < 2:
            continue
        date, val = parts[0].strip(), parts[1].strip()
        if not date or val in ("", "."):
            continue
        if start and date < start:
            continue
        if end and date > end:
            continue
        out[date] = float(val)
    if not out:
        raise SystemExit(f"FRED {series_id}: no observations returned")
    return out
