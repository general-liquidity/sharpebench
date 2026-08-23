#!/usr/bin/env python3
"""Derive a weekly dataset from a frozen daily one: the last close of each ISO
week (Mon..Sun), stamped with the actual date of that last bar, so every date in
the output is a real trading day present in the daily source.

    python scripts/data/derive_weekly.py            # data/us-indices-1d.csv -> data/us-indices-1w.csv

No network: the output is a pure function of the daily file's bytes, so it is
reproducible as long as the daily file's .sha256 still verifies. A partial first
or last week is kept (its last available close), same as an exchange weekly bar.
"""

from __future__ import annotations

import argparse
import datetime as dt
import os

from common import DATA_DIR, Series, write_dataset


def read_long_csv(path: str) -> tuple[Series, int]:
    series: Series = {}
    decimals = 0
    with open(path, encoding="ascii") as f:
        header = f.readline().strip().split(",")
        di, si, ci = header.index("date"), header.index("symbol"), header.index("close")
        for line in f:
            if not line.strip():
                continue
            p = [x.strip() for x in line.split(",")]
            series.setdefault(p[si], {})[p[di]] = float(p[ci])
            frac = p[ci].split(".")[1] if "." in p[ci] else ""
            decimals = max(decimals, len(frac))
    return series, decimals


def weekly(series: Series) -> Series:
    out: Series = {}
    for sym, m in series.items():
        last_in_week: dict[tuple[int, int], str] = {}
        for d in sorted(m):
            y, w, _ = dt.date.fromisoformat(d).isocalendar()
            last_in_week[(y, w)] = d  # sorted ascending, so the last write wins
        out[sym] = {d: m[d] for d in last_in_week.values()}
    return out


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("--source", default="us-indices-1d")
    ap.add_argument("--target", default="us-indices-1w")
    ap.add_argument("--check", action="store_true")
    a = ap.parse_args()
    series, decimals = read_long_csv(os.path.join(DATA_DIR, f"{a.source}.csv"))
    write_dataset(a.target, weekly(series), decimals, check=a.check)


if __name__ == "__main__":
    main()
