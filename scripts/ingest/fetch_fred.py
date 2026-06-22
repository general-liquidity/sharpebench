#!/usr/bin/env python3
"""Freeze daily closes for US equity indices from FRED's public CSV endpoint
(public domain, no API key) into a point-in-time CSV for SharpeBench.

This is **offline data-prep** (polyglot, per docs/PLAN.md §9): the benchmark loads
the *frozen* CSV; there is no network call in the scoring path. Stdlib only.

  python3 scripts/ingest/fetch_fred.py > data/us-indices-1d.csv
  # sha256 + counts print to stderr; record them in the .sha256 sidecar.

FRED data is public domain. Series are equity *indices* (tradeable as a proxy);
individual single-name equities need a keyed/again-different source (e.g. Tiingo,
Nasdaq Data Link) — FRED does not carry them.
"""
import hashlib
import sys
import urllib.request

# FRED series id -> the clean symbol used on the board.
SERIES = {
    "SP500": "SPX",       # S&P 500
    "DJIA": "DJI",        # Dow Jones Industrial Average
    "NASDAQCOM": "IXIC",  # Nasdaq Composite
}


def fetch(series_id: str) -> dict:
    url = f"https://fred.stlouisfed.org/graph/fredgraph.csv?id={series_id}"
    with urllib.request.urlopen(url, timeout=30) as resp:
        text = resp.read().decode()
    out = {}
    for line in text.splitlines()[1:]:  # skip "observation_date,<id>" header
        parts = line.split(",")
        if len(parts) != 2:
            continue
        date, val = parts[0].strip(), parts[1].strip()
        if not date or val in ("", "."):  # FRED marks holidays / missing as "."
            continue
        out[date] = float(val)
    return out


def main() -> None:
    series = {sym: fetch(sid) for sid, sym in SERIES.items()}
    # Shared date axis = the intersection (all indices trade the same NYSE calendar).
    common = None
    for s in series.values():
        common = set(s) if common is None else (common & set(s))
    dates = sorted(common or [])

    lines = ["date,symbol,close"]
    for d in dates:
        for sym in sorted(series):
            lines.append(f"{d},{sym},{series[sym][d]:.4f}")
    csv = "\n".join(lines) + "\n"

    sys.stdout.write(csv)
    digest = hashlib.sha256(csv.encode()).hexdigest()
    sys.stderr.write(
        f"rows={len(dates)} symbols={len(series)} "
        f"span={dates[0] if dates else '-'}..{dates[-1] if dates else '-'} "
        f"sha256={digest}\n"
    )


if __name__ == "__main__":
    main()
