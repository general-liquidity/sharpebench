# Superseded historical note: the deflation thresholds are in annualized units, the statistic is per-period

> **Status, 2026-08-25:** This note documents the pre-repair experiment only.
> It is not a source for current manuscript numbers. The current, pooled-seed
> evidence is the nine 512-record files in `paper/evidence/final/`; each record
> serializes `trials_sr_std_source`, per-period and annualized dispersion, the
> fixed zero-Sharpe null, and per-period and annualized deflation bars. Current
> analysis must use those artifacts and `paper/review/audit-math-2026-08-25.md`.
> In particular, the weekly-versus-daily 0.984/0.000 contrast below came from
> the small field-measured path, which the configured-prior unit fix did not
> change. It diagnosed instability in the unfloored field estimate and must not
> be attributed to the configured-prior unit bug.

Date: 2026-08-23. Source: `paper/evidence/sweep.jsonl`, datasets `us-indices-1d`
and `us-indices-1w`, 1,024 records, kernel at commit d0ae4f2 (v0.2.1).

## Observation

Across the entire grid (dsr_bar in {0.80, 0.90, 0.95, 0.99}, n_trials in
{1, 10, 50, 200}, trials_sr_std measured or pinned to {0.20, 0.35, 0.50}),
zero agents are rank-eligible on either dataset. That includes buy-and-hold
on the US indices, which posts PSR = 1.0000 and bootstrap_p = 0.0005 and still
scores DSR = 0.0000 and fails pass^k.

PSR = 1.0 and DSR = 0.0 are mathematically compatible because PSR compares the
series with zero while DSR compares it with a search-adjusted threshold. Their
coexistence localized the refusal to that threshold rather than the return
estimate and motivated the unit audit below.

## Cause

The kernel computes every Sharpe per period and says so (deflated_sharpe.rs:4,
"do not pre-annualize"). The arithmetic is correct. Two thresholds were
calibrated in annualized units and applied at the period frequency:

1. `trials_sr_std`. The shipped default 0.5 is Lopez de Prado's worked example,
   which is an annualized dispersion of Sharpe ratios across trials. Applied
   per period it sets the deflation benchmark `sr_star` to 1.14 per period at
   n_trials = 50. Even the value measured from the field (0.070) gives 0.159
   per period, which is four times the per-period Sharpe of the entire US
   stock market (about 0.036 per day).

2. `per_run_psr_bar = 0.90`. A per-run PSR of 0.90 against zero needs
   SR x sqrt(n - 1) of about 1.28. On a 1300-bar window the market scrapes it
   (0.9027); on a 250-bar window it reaches 0.71. pass^k in All-mode therefore
   fails whenever any out-of-sample window is shorter than roughly 1300 bars.

## Implied annualized threshold an agent had to beat, n_trials = 50

| trials_sr_std | per-period sr_star | 1h (8760/yr) | 4h (2190/yr) | 1d (252/yr) | 1w (52/yr) |
|---|---|---|---|---|---|
| 0.070 (measured) | 0.159 | 14.9 | 7.5 | 2.5 | 1.1 |
| 0.200 | 0.455 | 42.6 | 21.3 | 7.2 | 3.3 |
| 0.350 | 0.797 | 74.6 | 37.3 | 12.6 | 5.7 |
| 0.500 (old default) | 1.138 | 106.5 | 53.3 | 18.1 | 8.2 |

SPX buy-and-hold is roughly 0.5 to 0.9 annualized. Renaissance Medallion is
cited near 2 to 3. Under the shipped default a daily strategy needed an
annualized Sharpe of 18 and an hourly one needed 106. The bar was not high.
It was unreachable, and it got more unreachable as the square root of the
number of periods per year.

## Confirmation across eight datasets (4,189 records)

Zero agents eligible on all eight complete datasets. But the DSR of
buy-and-hold at the default config is not uniformly zero, and its pattern is
the prediction:

| dataset | periods/yr | buy-and-hold DSR | PSR | fails on |
|---|---|---|---|---|
| us-indices-1w | 52 | 0.984 | 1.000 | pass^k only |
| crypto-majors-1w | 52 | 0.745 | 1.000 | DSR and pass^k |
| crypto-majors-1d | 365 | 0.144 | 0.999 | DSR and pass^k |
| us-indices-1d | 252 | 0.000 | 1.000 | DSR and pass^k |
| crypto-majors-4h | 2190 | 0.000 | 1.000 | DSR and pass^k |

The same asset scores 0.984 weekly and 0.000 daily. Nothing about the market
changed; the annualized prior spreads over 52 periods at weekly and 252 at
daily, so the per-period bar is lowest where there are fewest periods.
Weekly US indices clears the DSR bar (0.984 > 0.95) and fails only pass^k,
which isolates the second threshold from the first in the data itself.

The luck floor never beats a reference agent on raw return on any dataset
(0 violations of 8). The falsification leg holds on real data.

## Why this is a result and not only a bug

It is a reproducible demonstration that a widely cited deflation threshold,
taken from the literature and applied at the frequency the statistic is
actually computed at, is unsatisfiable by the market itself. Every one of the
rival benchmarks criticised in the essay reports annualized Sharpe without
deflation; this benchmark deflated without annualizing. Both are unit errors,
in opposite directions. The paper should present this as the first empirical
finding, with the table above, before presenting the corrected protocol.

It also explains, completely, why the luck-demotion demonstration only ever
worked on synthetic and hand-built inputs: those had per-period returns large
enough to clear an annualized bar.

## Fix (design decision, new minor version)

`ScoreConfig` gains `periods_per_year`. `trials_sr_std` and the per-run PSR
bar are then stated in annualized terms and converted to per-period inside
the kernel: a per-period dispersion is the annualized one divided by
sqrt(periods_per_year), and the per-run bar becomes a bar on the annualized
Sharpe the window implies. The per-period kernel stays exactly as it is; only
the thresholds learn what a period is. This changes who ranks on every real
dataset, so it is 0.3.0, not a patch, and the golden fixtures regenerate.

After the fix, rerun the sweep. The interesting question becomes which agents
clear the corrected bar on which asset classes and timeframes, which is the
experiment the paper was always supposed to run.
