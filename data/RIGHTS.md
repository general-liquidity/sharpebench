# Dataset rights audit

Reviewed 2026-09-01 against the upstream publishers' current notices. This is
an evidence record, not legal advice. It records what the cited sources say and
whether this repository contains a separate permission grant.

## Release status

The code is licensed `MIT OR Apache-2.0`. Those licenses do not apply to the
third-party observations in `data/*.csv`.

Do not describe the nine CSV files as collectively public domain,
redistributable, or covered by the repository license. No written permission is
recorded for the three index series or the Binance observations. The current
FRED terms also prohibit using FRED content to develop a software system and,
without prior written consent, prohibit caching, archiving, or providing that
content to third parties. The present data distribution therefore remains a
release blocker until the maintainer obtains the needed permissions or migrates
to sources whose terms permit the intended use.

Removing or replacing the affected files is not a documentation-only change.
The v0.9.0 paper evidence was computed from their exact bytes. A direct-source
migration must create new dataset identities and regenerate the empirical
field; it must not relabel the historical results as if they used the new data.

## Series-by-series record

| Repository series | Upstream series | Publisher's current notice | Permission recorded here |
|:--|:--|:--|:--|
| `us-indices-1d`, `us-indices-1w` | [SP500](https://fred.stlouisfed.org/series/SP500) | S&P Dow Jones Indices copyright. FRED marks it pre-approval required and the series note prohibits reproduction without prior written permission. | No |
| `us-indices-1d`, `us-indices-1w` | [DJIA](https://fred.stlouisfed.org/series/DJIA) | S&P Dow Jones Indices copyright. FRED marks it pre-approval required and the series note prohibits reproduction without prior written permission. | No |
| `us-indices-1d`, `us-indices-1w` | [NASDAQCOM](https://fred.stlouisfed.org/series/NASDAQCOM) | Nasdaq copyright; FRED marks the series pre-approval required. | No |
| `fx-majors-1d` | [DEXUSEU](https://fred.stlouisfed.org/series/DEXUSEU), [DEXUSUK](https://fred.stlouisfed.org/series/DEXUSUK), [DEXUSAL](https://fred.stlouisfed.org/series/DEXUSAL), [DEXJPUS](https://fred.stlouisfed.org/series/DEXJPUS), [DEXSZUS](https://fred.stlouisfed.org/series/DEXSZUS) | FRED labels each `Public Domain: Citation Requested`. FRED defines that label as a use category, not a representation that every series is public domain, and subjects it to the service-wide prohibited uses. | No separate FRED consent recorded |
| `commodities-1d` | [DCOILWTICO](https://fred.stlouisfed.org/series/DCOILWTICO), [DCOILBRENTEU](https://fred.stlouisfed.org/series/DCOILBRENTEU) | U.S. Energy Information Administration source; FRED labels both `Public Domain: Citation Requested`, subject to the same service-wide terms. | No separate FRED consent recorded |
| `rates-1d` | [DGS10](https://fred.stlouisfed.org/series/DGS10) | Federal Reserve Board source; FRED labels it `Public Domain: Citation Requested`, subject to the same service-wide terms. | No separate FRED consent recorded |
| `crypto-majors-1h`, `crypto-majors-4h`, `crypto-majors-1d`, `crypto-majors-1w` | [Binance Spot klines](https://developers.binance.com/en/docs/products/spot/rest-api) | Binance documents keyless access to public market data and free historical access. The reviewed official pages do not grant this repository a license to redistribute copied or derived kline observations. | No |

The controlling FRED source for the service-wide conditions is its
[Legal Notices, Information and Disclaimers](https://fred.stlouisfed.org/legal/).
That page also says that a `Public Domain: Citation Requested` label may cover
copyrighted or public-domain material and that third-party permissions remain
the user's responsibility.

## Remediation choices

One of these paths must precede another public data release:

1. obtain and archive written grants covering repository and package
   redistribution plus the benchmark's software use;
2. replace the restricted series with directly sourced, permission-compatible
   data, assign new dataset identifiers, rerun the empirical field, and report
   the resulting benchmark version as a new evidence stratum; or
3. stop distributing the affected CSVs and narrow reproducibility to hashes and
   derived evidence that can lawfully remain public.

The audit found no basis for silently selecting one of these paths. The current
historical evidence stays identified as historical while the rights decision is
open.
