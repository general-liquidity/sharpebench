#!/usr/bin/env python3
"""FX majors, commodities and rates from FRED's keyless `fredgraph.csv` endpoint.

    python scripts/data/fetch_fred.py fx            # -> data/fx-majors-1d.csv
    python scripts/data/fetch_fred.py commodities   # -> data/commodities-1d.csv
    python scripts/data/fetch_fred.py rates         # -> data/rates-1d.csv

All series are US-government work (Federal Reserve H.10 / H.15, EIA) — public
domain. Holidays / missing days (FRED `.`) are dropped, never interpolated. The
window is capped (`--start` / `--end`) so a re-run reproduces the frozen bytes even
though FRED keeps appending new observations.

FX convention: every symbol is the USD price of 1 unit of the base currency, so a
long position is long the base vs USD. DEXUSEU/DEXUSUK/DEXUSAL are already quoted
that way; DEXJPUS (JPY per USD) and DEXSZUS (CHF per USD) are inverted, hence the
symbols JPYUSD and CHFUSD rather than USDJPY / USDCHF.

Rates: DGS10 is a *yield in percent*, not a price (see data/README.md).
"""

from __future__ import annotations

import argparse

from common import Series, fred_series, write_dataset

DEFAULT_START = "2010-01-01"
DEFAULT_END = "2026-08-14"  # last date every FRED series here had published at freeze time

TASKS = {
    # name: (decimals, [(fred_id, symbol, invert)])
    "fx": ("fx-majors-1d", 8, [
        ("DEXUSEU", "EURUSD", False),
        ("DEXUSUK", "GBPUSD", False),
        ("DEXUSAL", "AUDUSD", False),
        ("DEXJPUS", "JPYUSD", True),
        ("DEXSZUS", "CHFUSD", True),
    ]),
    "commodities": ("commodities-1d", 4, [
        ("DCOILWTICO", "WTI", False),
        ("DCOILBRENTEU", "BRENT", False),
    ]),
    "rates": ("rates-1d", 4, [
        ("DGS10", "UST10Y", False),
    ]),
}


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("task", choices=sorted(TASKS))
    ap.add_argument("--start", default=DEFAULT_START)
    ap.add_argument("--end", default=DEFAULT_END)
    ap.add_argument("--check", action="store_true", help="compare against the frozen file instead of writing")
    a = ap.parse_args()
    name, decimals, specs = TASKS[a.task]
    series: Series = {}
    for fred_id, sym, invert in specs:
        raw = fred_series(fred_id, a.start, a.end)
        series[sym] = {d: (1.0 / v if invert else v) for d, v in raw.items()}
        print(f"{fred_id} -> {sym}: {len(raw)} obs, {min(raw)} .. {max(raw)}", flush=True)
    write_dataset(name, series, decimals, check=a.check)


if __name__ == "__main__":
    main()
