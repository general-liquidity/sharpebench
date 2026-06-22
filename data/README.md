# Frozen datasets

SharpeBench runs on **frozen, point-in-time, checksummed** datasets — never a live
API in the scoring path, so a score reproduces byte-for-byte forever. Fetchers are
offline (the `xtask` crate); the benchmark only ever loads the frozen artifact.

## `crypto-majors-1d.csv`

- **Symbols:** BTC, ETH, SOL, BNB, XRP (quoted vs USDT, treated as USD).
- **Bars:** daily closes, ~1000 days.
- **Source:** Binance public REST API (`/api/v3/klines`, no key) — public market data.
- **Format:** long — `date,symbol,close` (ISO `YYYY-MM-DD`; series aligned on the
  date axis common to all symbols).
- **Integrity:** `crypto-majors-1d.csv.sha256`.

Run on it, or regenerate it:

```bash
cargo run -p sharpebench -- run --data data/crypto-majors-1d.csv
cargo run -p xtask -- crypto                                   # re-fetch + write the .sha256 sidecar
```

## `us-indices-1d.csv`

- **Symbols:** SPX (S&P 500), DJI (Dow Jones Industrial Average), IXIC (Nasdaq Composite).
- **Bars:** daily closes, ~2500 days (10 years).
- **Source:** FRED public CSV endpoint (`fredgraph.csv`, no key) — **public domain**.
- **Format:** long — `date,symbol,close` (aligned on the shared NYSE-calendar axis).
- **Integrity:** `us-indices-1d.csv.sha256`.

```bash
cargo run -p sharpebench -- run --data data/us-indices-1d.csv
cargo run -p xtask -- indices                                  # re-fetch + write the .sha256 sidecar
```

## Adding sources

Any source that produces aligned `date,symbol,close[,dividend]` rows works. Live now:
crypto (Binance) and US equity indices (FRED). Next up:

- **Single-name equities** — DJIA / S&P constituents need a keyed source (Tiingo, Nasdaq Data Link) or a JS-capable Stooq fetch; FRED carries indices, not single names.
- **Fundamentals** — SEC EDGAR financial-statement datasets (public domain) → the `fundamentals` channel.
- **Macro / commodities** — more FRED series (rates, gold, oil).

Add new fetchers to the `xtask` crate (offline, `publish = false`) and keep the scoring path pure.
