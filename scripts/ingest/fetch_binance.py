#!/usr/bin/env python3
"""Freeze daily close bars for a set of crypto majors from Binance's public API
into a point-in-time CSV for SharpeBench.

This is **offline data-prep** (polyglot, per docs/PLAN.md §9): the benchmark loads
the *frozen* CSV; there is no network call in the scoring path, so scores stay
reproducible forever. Stdlib only (urllib) — no pip dependencies, no API key.

  python3 scripts/ingest/fetch_binance.py > data/crypto-majors-1d.csv
  # the sha256 + row/symbol counts are printed to stderr; record them in the manifest.

Output is long-format, point-in-time, and date-aligned across symbols:
  date,symbol,close
  2024-01-01,BTC,42283.58000000
  ...
"""
import hashlib
import json
import sys
import urllib.request
from datetime import datetime, timezone

# Binance ticker -> the clean symbol used on the board.
SYMBOLS = {
    "BTCUSDT": "BTC",
    "ETHUSDT": "ETH",
    "SOLUSDT": "SOL",
    "BNBUSDT": "BNB",
    "XRPUSDT": "XRP",
}
INTERVAL = "1d"
LIMIT = 1000  # Binance max per request (~2.7y of daily bars)


def fetch(ticker: str) -> dict:
    url = (
        f"https://api.binance.com/api/v3/klines"
        f"?symbol={ticker}&interval={INTERVAL}&limit={LIMIT}"
    )
    with urllib.request.urlopen(url, timeout=30) as resp:
        klines = json.load(resp)
    # kline = [open_time_ms, open, high, low, close, volume, ...]
    return {
        datetime.fromtimestamp(k[0] / 1000, tz=timezone.utc).strftime("%Y-%m-%d"): float(k[4])
        for k in klines
    }


def main() -> None:
    series = {clean: fetch(ticker) for ticker, clean in SYMBOLS.items()}
    # Shared date axis = the intersection, so every symbol is aligned step-for-step.
    common = None
    for s in series.values():
        common = set(s) if common is None else (common & set(s))
    dates = sorted(common or [])

    lines = ["date,symbol,close"]
    for d in dates:
        for sym in sorted(series):
            lines.append(f"{d},{sym},{series[sym][d]:.8f}")
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
