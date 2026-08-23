# Frozen datasets

SharpeBench runs on **frozen, point-in-time, checksummed** datasets — never a live
API in the scoring path, so a score reproduces byte-for-byte forever. Fetchers are
offline (the `xtask` crate and `scripts/data/`); the benchmark only ever loads the
frozen artifact.

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

## `crypto-majors-1h.csv`, `crypto-majors-4h.csv`, `crypto-majors-1w.csv`

The same five majors at other bar sizes, so the deflated-Sharpe gate can be
studied as a function of track length `n` and of the return distribution's skew
and kurtosis, which change with bar size.

- **Symbols:** BTC, ETH, SOL, BNB, XRP (vs USDT, treated as USD).
- **Bars:** kline **close** stamped with the kline **open time** (UTC). Intraday
  bars use `YYYY-MM-DDTHH:MM`, which still sorts chronologically for the loader.
  - `1h`: 24 000 bars / symbol, `2023-09-27T00:00` .. `2026-06-22T23:00` — the
    same 1000-day window as the daily file, no gaps (120 000 rows).
  - `4h`: 6 000 bars / symbol, `2023-09-27T00:00` .. `2026-06-22T20:00` (30 000 rows).
  - `1w`: 307 bars / symbol, `2020-08-10` .. `2026-06-22` (1 535 rows). Weekly bars
    open Monday 00:00 UTC; the window starts at the earliest week common to all
    five pairs (SOL listed on Binance 2020-08-11) — the intersection trims the
    older BTC/ETH/BNB/XRP history. The first SOL bar is a partial listing week.
- **Source:** Binance public REST API (`/api/v3/klines`, no key, paginated 1000
  klines per request) — public market data. Fetched 2026-08-23.
- **Raw / derived:** raw.
- **Integrity:** `<file>.sha256`. Regenerate and compare:

```bash
python scripts/data/fetch_crypto.py --interval 1h            # also 4h, 1w
python scripts/data/fetch_crypto.py --interval 1h --check    # compare bytes against the frozen file
```

Note on the frozen `crypto-majors-1d.csv`: re-fetching its window with the script
reproduces 4 995 of its 5 000 rows byte-for-byte. The five `2026-06-22` rows
differ because the frozen file was captured intraday on that date (a partial
last bar, e.g. BTC `65049.42` frozen vs `64020.01` final). The frozen file is
left untouched; the 1h/4h/1w files contain only completed bars.

## `us-indices-1w.csv` (derived)

- **Symbols:** SPX, DJI, IXIC.
- **Bars:** weekly — the **last close of each ISO week** (Mon..Sun) taken from
  `us-indices-1d.csv`, stamped with the actual date of that last trading day.
  522 bars / symbol, `2016-06-24` .. `2026-06-17` (1 566 rows). The first and last
  weeks are partial.
- **Source:** derived offline from the frozen `us-indices-1d.csv` (FRED, public
  domain) — no network. Derived 2026-08-23.
- **Raw / derived:** **derived**; a pure function of the daily file's bytes.
- **Integrity:** `us-indices-1w.csv.sha256`.

```bash
python scripts/data/derive_weekly.py          # us-indices-1d.csv -> us-indices-1w.csv
```

## `fx-majors-1d.csv`

- **Symbols:** EURUSD, GBPUSD, AUDUSD, JPYUSD, CHFUSD.
- **Convention:** every close is the **USD price of 1 unit of the base currency**,
  so "long" means long the base vs USD. FRED quotes EUR/GBP/AUD that way
  (`DEXUSEU`, `DEXUSUK`, `DEXUSAL`); JPY and CHF are quoted per USD (`DEXJPUS`,
  `DEXSZUS`) and are **inverted** here (`JPYUSD = 1 / DEXJPUS`,
  `CHFUSD = 1 / DEXSZUS`), hence the symbol names rather than USDJPY / USDCHF.
  8 decimals so the inverted series keep their precision.
- **Bars:** daily noon buying rates (New York), 4 157 bars / symbol,
  `2010-01-04` .. `2026-08-14` (20 785 rows). FRED carries these back to 1971
  (EUR to 1999); the window is **capped at 2010-01-01** for comparability across
  the FRED files, and at 2026-08-14, the last date published for all five at
  freeze time.
- **Source:** FRED `fredgraph.csv` (no key), Federal Reserve Board H.10 release —
  **public domain**. Fetched 2026-08-23.
- **Raw / derived:** raw (values inverted for JPY/CHF, nothing else).
- **Integrity:** `fx-majors-1d.csv.sha256`.

```bash
python scripts/data/fetch_fred.py fx
```

## `commodities-1d.csv`

- **Symbols:** WTI (`DCOILWTICO`, Cushing OK spot), BRENT (`DCOILBRENTEU`, Europe spot).
- **Bars:** daily spot closes in USD/bbl, 4 126 bars common to both,
  `2010-01-04` .. `2026-08-14` (8 252 rows). WTI has 4 168 and Brent 4 205
  observations in the window; the loader's date intersection drops the rest.
- **Gold is not included:** FRED's daily LBMA gold series (`GOLDAMGBD228NLBM`,
  `GOLDPMGBD228NLBM`) now return 404 — they were withdrawn from FRED. No keyless,
  redistributable daily gold series was found, so gold is skipped rather than
  sourced from a licensed feed.
- **WARNING — negative price:** on `2020-04-20` WTI closed at **-36.98**. The row is
  kept as published (no editing of upstream data), but simple returns across it
  are meaningless (`-302%` into the bar, `-124%` out of it) and they dominate the
  pooled moments: the realism battery reports excess kurtosis **+2438** and skew
  **-37.7** on this file, versus **+6.3** excess kurtosis when the window starts
  2020-05-01. Run `fetch_fred.py commodities --start 2020-05-01` (or any other
  cap) if a study needs a strictly positive price path, and say so.
- **Source:** FRED `fredgraph.csv` (no key), data from the U.S. Energy Information
  Administration — **public domain**. Fetched 2026-08-23.
- **Raw / derived:** raw.
- **Integrity:** `commodities-1d.csv.sha256`.

```bash
python scripts/data/fetch_fred.py commodities
```

## `rates-1d.csv`

- **Symbol:** UST10Y (`DGS10`, 10-year Treasury constant-maturity yield).
- **WARNING — the `close` column is a YIELD IN PERCENT, not a price.** A value of
  `4.2500` means 4.25%. A strategy run on this file trades the **yield series
  directly** (a "long" profits when yields rise), and a "return" of
  `close[t] / close[t-1] - 1` is a relative change in yield, not a bond return.
  The file loads and scores like any other dataset, so it is useful for studying
  the deflated-Sharpe gate on a series with a very different moment structure,
  but it must not be read as a Treasury price or total-return index. Yields near
  zero (DGS10 hit 0.52 in 2020) make relative changes large.
- **Bars:** daily, 4 157 bars, `2010-01-04` .. `2026-08-14` (4 157 rows). Single
  symbol — `DGS2` / `DGS30` are a one-line addition in the script if a
  cross-section is wanted.
- **Source:** FRED `fredgraph.csv` (no key), Federal Reserve Board H.15 release —
  **public domain**. Fetched 2026-08-23.
- **Raw / derived:** raw.
- **Integrity:** `rates-1d.csv.sha256`.

```bash
python scripts/data/fetch_fred.py rates
```

## Summary

Rows exclude the header. Fetch date for every file added 2026-08-23 is stated in
its section above; the two original files were frozen 2026-06-22.

| file | symbols | rows | bars | date range | source | license | raw / derived |
|---|---|---|---|---|---|---|---|
| `crypto-majors-1d.csv` | BTC ETH SOL BNB XRP | 5 000 | 1 000 | 2023-09-27 .. 2026-06-22 | Binance `/api/v3/klines` | public market data | raw |
| `crypto-majors-1h.csv` | BTC ETH SOL BNB XRP | 120 000 | 24 000 | 2023-09-27T00:00 .. 2026-06-22T23:00 | Binance `/api/v3/klines` | public market data | raw |
| `crypto-majors-4h.csv` | BTC ETH SOL BNB XRP | 30 000 | 6 000 | 2023-09-27T00:00 .. 2026-06-22T20:00 | Binance `/api/v3/klines` | public market data | raw |
| `crypto-majors-1w.csv` | BTC ETH SOL BNB XRP | 1 535 | 307 | 2020-08-10 .. 2026-06-22 | Binance `/api/v3/klines` | public market data | raw |
| `us-indices-1d.csv` | SPX DJI IXIC | 7 539 | 2 513 | 2016-06-20 .. 2026-06-17 | FRED `SP500` `DJIA` `NASDAQCOM` | public domain | raw |
| `us-indices-1w.csv` | SPX DJI IXIC | 1 566 | 522 | 2016-06-24 .. 2026-06-17 | derived from `us-indices-1d.csv` | public domain | **derived** |
| `fx-majors-1d.csv` | EURUSD GBPUSD AUDUSD JPYUSD CHFUSD | 20 785 | 4 157 | 2010-01-04 .. 2026-08-14 | FRED `DEXUSEU` `DEXUSUK` `DEXUSAL` `DEXJPUS` `DEXSZUS` | public domain | raw (JPY/CHF inverted) |
| `commodities-1d.csv` | WTI BRENT | 8 252 | 4 126 | 2010-01-04 .. 2026-08-14 | FRED `DCOILWTICO` `DCOILBRENTEU` | public domain | raw |
| `rates-1d.csv` | UST10Y (yield, %) | 4 157 | 4 157 | 2010-01-04 .. 2026-08-14 | FRED `DGS10` | public domain | raw |

Every file has a `<file>.sha256` sidecar (`sha256sum -c <file>.sha256` inside
`data/`, or re-run the script with `--check`). The Python scripts under
`scripts/data/` need only `requests`; they sort rows by date then symbol, use
fixed decimals, and write no timestamps, so a re-run over the same upstream data
reproduces the bytes. FRED and Binance windows are pinned with `--start`/`--end`
defaults because both sources keep appending.

## Adding sources

Any source that produces aligned `date,symbol,close[,dividend]` rows works. Live now:
crypto (Binance, 1h/4h/1d/1w), US equity indices (FRED, 1d + derived 1w), FX majors,
oil and the 10-year yield (FRED). Known gaps:

- **Single-name equities** — DJIA / S&P constituents need a keyed source (Tiingo, Nasdaq Data Link) or a JS-capable Stooq fetch; FRED carries indices, not single names. Not attempted.
- **Gold** — FRED withdrew the daily LBMA series; no keyless, redistributable daily source found.
- **Fundamentals** — SEC EDGAR financial-statement datasets (public domain) → the `fundamentals` channel.

Add new fetchers to `scripts/data/` (Python) or the `xtask` crate (offline, `publish = false`) and keep the scoring path pure.
