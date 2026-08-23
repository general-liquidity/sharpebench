#!/usr/bin/env python3
"""Crypto majors (BTC/ETH/SOL/BNB/XRP vs USDT) from Binance public klines, no key.

    python scripts/data/fetch_crypto.py --interval 1h
    python scripts/data/fetch_crypto.py --interval 1d --check   # compare against the frozen file

Paginates 1000 klines per request via `startTime`, keeps the kline close and the
kline open time (UTC). Daily/weekly bars are stamped `YYYY-MM-DD`; intraday bars
`YYYY-MM-DDTHH:MM` (UTC), which still sorts chronologically for the loader.
Deterministic: Binance historical klines are immutable, so the same window
reproduces the same bytes.
"""

from __future__ import annotations

import argparse
import datetime as dt

from common import Series, http_get, write_dataset

TICKERS = [("BTCUSDT", "BTC"), ("ETHUSDT", "ETH"), ("SOLUSDT", "SOL"), ("BNBUSDT", "BNB"), ("XRPUSDT", "XRP")]
DEFAULT_START = "2023-09-27"  # first bar of the frozen crypto-majors-1d.csv
DEFAULT_END = "2026-06-22"    # last bar of the frozen crypto-majors-1d.csv
EARLIEST = "2017-01-01"       # before any of the five pairs listed; intersection trims it


def to_ms(date: str) -> int:
    return int(dt.datetime.fromisoformat(date).replace(tzinfo=dt.timezone.utc).timestamp() * 1000)


def stamp(open_ms: int, interval: str) -> str:
    t = dt.datetime.fromtimestamp(open_ms / 1000, tz=dt.timezone.utc)
    return t.strftime("%Y-%m-%d") if interval in ("1d", "1w") else t.strftime("%Y-%m-%dT%H:%M")


def fetch(ticker: str, interval: str, start_ms: int, end_ms: int) -> dict[str, float]:
    out: dict[str, float] = {}
    cursor = start_ms
    while cursor <= end_ms:
        r = http_get(
            "https://api.binance.com/api/v3/klines",
            params={"symbol": ticker, "interval": interval, "startTime": cursor, "endTime": end_ms, "limit": 1000},
        )
        rows = r.json()
        if not rows:
            break
        for k in rows:
            out[stamp(int(k[0]), interval)] = float(k[4])
        cursor = int(rows[-1][0]) + 1
        if len(rows) < 1000:
            break
    return out


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("--interval", default="1d", choices=["1h", "4h", "1d", "1w"])
    ap.add_argument("--start", default=None, help="first kline open date (UTC), inclusive")
    ap.add_argument("--end", default=DEFAULT_END, help="last kline open date (UTC), inclusive")
    ap.add_argument("--check", action="store_true", help="compare against the frozen file instead of writing")
    a = ap.parse_args()
    start = a.start or (EARLIEST if a.interval == "1w" else DEFAULT_START)
    start_ms, end_ms = to_ms(start), to_ms(a.end) + 86_400_000 - 1
    series: Series = {}
    for ticker, sym in TICKERS:
        series[sym] = fetch(ticker, a.interval, start_ms, end_ms)
        print(f"{ticker} {a.interval}: {len(series[sym])} bars", flush=True)
    write_dataset(f"crypto-majors-{a.interval}", series, 8, check=a.check)


if __name__ == "__main__":
    main()
