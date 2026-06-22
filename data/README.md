# Frozen datasets

SharpeBench runs on **frozen, point-in-time, checksummed** datasets — never a live
API in the scoring path, so a score reproduces byte-for-byte forever. Fetchers are
offline (`scripts/ingest/`); the benchmark only ever loads the frozen artifact.

## `crypto-majors-1d.csv`

- **Symbols:** BTC, ETH, SOL, BNB, XRP (quoted vs USDT, treated as USD).
- **Bars:** daily closes, ~1000 days.
- **Source:** Binance public REST API (`/api/v3/klines`, no key) — public market data.
- **Format:** long — `date,symbol,close` (ISO `YYYY-MM-DD`; series aligned on the
  date axis common to all symbols).
- **Integrity:** `crypto-majors-1d.csv.sha256`.

Run on it, or regenerate it:

```bash
cargo run -p sb-cli -- run --data data/crypto-majors-1d.csv
python3 scripts/ingest/fetch_binance.py > data/crypto-majors-1d.csv   # then update the .sha256 sidecar
```

## Adding sources

Any source that produces aligned `date,symbol,close[,dividend]` rows works. Next up:

- **Equities** — Stooq EOD (daily) for DJIA / S&P parity *(needs a JS-capable fetch — its endpoint is behind a browser challenge)*.
- **Fundamentals** — SEC EDGAR financial-statement datasets (public domain) → the `fundamentals` channel.
- **Macro** — FRED (public domain).

Keep new fetchers in `scripts/ingest/` (offline, polyglot) and the scoring path pure.
