# Superseded historical note: pass^k in All-mode is a test of every-regime profitability, not of edge

> **Status, 2026-08-25:** This note predates seed-block pooling, the fixed
> deflation null, clone collapse and the final regenerated evidence. It remains
> a record of the design interpretation, not a source for current DSR,
> bootstrap, eligibility or deflation values. Use the nine final sweep files
> (512 records each) and `paper/review/audit-math-2026-08-25.md` for the
> current record.
>
> Two statements below are now false and are retained only as history. The
> claim that "every real field has eight agents, so `rank` takes the measured
> path" no longer holds: ranking clusters the seed-averaged pooled streams
> before measuring, and on five of the nine panels fewer than five votes
> survive, so those cells fall back to the configured prior. The DSR values
> quoted here (us-indices-1w 0.984, crypto-majors-1w 0.745/0.978,
> crypto-majors-1d 0.144) are pre-floor, pre-pooling values; the current ones
> are 0.178, 0.241 and 0.251.

Date: 2026-08-23. Source: `paper/evidence/after-v0.3.0/`, seven complete
datasets, 4,072 records, kernel at v0.3.0 (units fix applied).

## Observation

After the units fix, the deflated Sharpe column on real datasets is unchanged
from v0.2.1, and zero agents are eligible at any cell of the grid. Both
facts have exact causes and neither is a defect in the fix.

The DSR is unchanged because every real field has eight agents, so `rank`
takes the measured path: the dispersion of per-period Sharpes across the
field is already per-period and is correctly not converted. The units fix
changed only the configured prior, which real fields do not use. The values
reported (us-indices-1w 0.984, crypto-majors-1w 0.745, us-indices-1d 0.000)
are the true verdict of the measured statistic. On weekly data the index
clears the 0.95 bar. On daily data it does not, because a 50-trial benchmark
against a measured per-period dispersion of 0.070 sits at 0.159 per period
and the daily index is at about 0.036.

The remaining blocker on every dataset is pass^k.

## Why pass^k fails everywhere, and why that is correct

Every dataset's walk-forward windows include at least one Bear regime:

| dataset | windows | bars each | regimes |
|---|---|---|---|
| us-indices-1d | 6 | 408 | Bull Bull Bull Bear Bull Bull |
| us-indices-1w | 6 | 78 | Bull Bull Bull Bear Bull Bull |
| crypto-majors-1w | 6 | 46 | Bull Bear Bull Bull Bull Bear |

pass^k in All-mode requires every run (every window times every seed) to
clear per-run PSR 0.90 against zero. A long-only agent in a bear window has
a negative realised Sharpe on that window and cannot clear any positive bar.
Therefore no long-only reference agent can pass pass^k on any dataset that
contains a downturn. Buy-and-hold, the baseline every agent must beat, is
structurally ineligible on every real dataset shipped.

The bar is also steep on short windows because PSR scales with sqrt(n - 1).
At 78 bars, PSR 0.90 needs a per-period Sharpe of 0.146, which on weekly
data is an annualized Sharpe of 1.05 in every window including the bear
one. At 46 bars it is 1.31.

## What this means

pass^k in All-mode does not ask "does this agent have an edge." It asks
"is this agent profitable with 90% confidence in every regime and every
execution seed." That is a much stronger claim, and it is the right gate
for an agent about to be handed capital: the methodology page already says
"a money agent that is safe-on-average is not safe." A regime-dependent edge
such as equity beta is correctly ineligible under it. The benchmark is not
failing to rank buy-and-hold; it is declining to certify that owning the
index is safe in a bear market, which it is not.

Two consequences for the paper:

1. The headline empirical claim must be stated exactly: on nine frozen
   datasets spanning four asset classes and four bar sizes, no reference
   agent is rank-eligible, because every dataset contains a downturn and
   the reliability gate requires profitability in every window. This is
   evidence that the gate is strict, not that it is broken. The luck floor
   never beats a reference agent on raw return (0 of 9), so the floor is
   behaving.

2. There is an honest design question the paper should pose rather than
   resolve: whether the reliability gate should certify "profitable in every
   regime" (today) or "never catastrophic in any regime" (a drawdown or
   tail bound per run, with the edge tested on the pooled track). The
   second admits a regime-dependent edge while still refusing an agent that
   blows up. The kernel already has `PassMode::AtLeast(m)` and a mandate
   with a drawdown cap, so both designs are expressible today. This is an
   ablation for the paper (experiment E3), not a change to make before it.

## The ablation, run (v0.3.0 kernel, both verdicts on identical trajectories)

The never-catastrophic preset (pass_mode Any, 20 percent per-run drawdown
bound) was scored beside the default on the same cells. It admits nobody
either, and the worst-run drawdown column explains it:

| dataset | agent | DSR | worst run drawdown | every-regime | never-catastrophic |
|---|---|---|---|---|---|
| us-indices-1w | buy-and-hold | 0.984 | 0.320 | no | no |
| crypto-majors-1w | momentum | 0.978 | 0.563 | no | no |
| crypto-majors-1w | buy-and-hold | 0.745 | 0.671 | no | no |
| crypto-majors-1d | buy-and-hold | 0.144 | 0.476 | no | no |
| crypto-majors-1d | luck floor (best) | 0.000 | 0.603 | no | no |

A 20 percent per-regime drawdown bound is not weaker than every-regime
profitability on these datasets. It is stricter in practice, because every
reference agent is unhedged long-only exposure through the 2020 and 2022
drawdowns. The weekly crypto momentum agent clears the deflation bar at
0.978 and is refused on a 56 percent drawdown in one window. A raw-return
leaderboard ranks that agent first.

The honest summary: no unhedged long-only strategy is safe to hand capital
under either definition of reliability, and the benchmark says so with the
drawdown that proves it. The ablation did not find a gate that admits the
reference agents. It found that the reference agents should not be admitted.

## What changes in the evidence plan

The sweep should add a second pass mode so the paper can show both verdicts
side by side: All-mode (today's gate) and a per-run drawdown mandate with
the edge tested on the pooled track. Which agents clear which gate on which
asset class and timeframe is the table the paper was always meant to have.
