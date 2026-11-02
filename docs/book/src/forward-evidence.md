# Forward evidence ledger

Each forward window is ranked on its own data. A window lasts weeks, so a
verdict taken one window at a time has almost no power, and until this ledger
nothing carried from one window to the next. The forward evidence ledger lets
forward evidence accumulate across windows without fixing a horizon in advance,
and stays valid when someone reads it after every window and stops when the
number looks good.

It is rank-neutral. No score, row, gate or ordering reads it. pass^k, the
deflated Sharpe and every other column of the board are computed exactly as
before and reported beside it. The two answer different questions: pass^k asks
whether every run clears a bar, the deflated Sharpe asks whether the pooled
Sharpe clears a selection-adjusted bar at a fixed horizon, and the ledger asks
whether an artifact's forward returns have a positive conditional mean at all,
at every bar at once.

The math is in `sharpebench_stats::forward_eprocess`; the ledger is in
`sharpebench_arena::forward_evidence`. The module docs of the first state the
rule and prove it valid; this page summarizes both.

## The null

For one committed artifact, the observations are its arena-derived returns,
one per bar, in publish order and then bar order. The null is

```text
H0:  E[x_t | F_{t-1}] <= 0  and  x_t >= -2 B_t  for every bar t,
```

where `F_{t-1}` is everything observed before bar `t` and `B_t` is the per-bar
loss bound in force, fixed before the bar. This is a conditional-mean null. It
allows heteroskedasticity, regime switches and serial dependence in the
variance, and asks only that no bar is predictably profitable. It is the null
"Sharpe at most zero". The deflated Sharpe's null, a Sharpe at most a positive
bar, is not a mean null and is not tested here.

## The rule

The bettor keeps the bar count `n`, the sum `S` and the sum of squares `Q` of
the artifact's observations, and its wealth `K`, which starts at 1. Before each
bar it stakes

```text
lambda = 0                          during the first 20 bars, when S <= 0,
                                    or once the process has stopped;
lambda = min(S / Q, 1 / (2 B))      otherwise.
```

`S / Q` is the sample mean over the sample second moment, the approximate
growth-rate-adaptive (aGRAPA) bet of Waudby-Smith and Ramdas (2024, appendix
B.3) for a mean tested at zero. Bets are never negative, because the null is
one-sided. After the bar:

- if `x < -B` (a breach of the bound), `K` becomes 0 and the process stops;
- otherwise `K` becomes `K (1 + lambda x)`, clamped at `1e300`, where the
  process also stops.

Every step is an IEEE-754 add, multiply, divide or comparison in a fixed order,
so the wealth is the same bits on every platform and anyone holding the signed
observations can recompute it.

## Why it is valid

Under `H0` the wealth is a nonnegative supermartingale that starts at 1:

1. The stake is fixed before the bar and lies in `[0, 1/(2B)]`.
2. Off a breach, `x >= -B`, so the factor `1 + lambda x` is at least 1/2.
3. On a breach the factor is 0, which is at most `1 + lambda x` because
   `x >= -2B` under the null. So the factor never exceeds `1 + lambda x`.
4. Taking the conditional expectation, the factor's mean is at most
   `1 + lambda E[x | F] <= 1`. The clamp at the ceiling only lowers the wealth.

Ville's inequality then gives `P(sup_t K_t >= 1/alpha) <= alpha` (Ramdas,
Grünwald, Vovk and Shafer, 2023). Three things follow. The anytime p-value
`1 / max K` is valid at every bar at once. The current wealth read at a time
chosen from the data so far is an e-value (the running maximum is not).
And because the index counts observations rather than days, a gap between
windows or a decision to continue after looking changes nothing (Grünwald, de
Heide and Koolen, 2024, Proposition 2 and Corollary 1).

The support condition is an assumption the arithmetic cannot enforce. The
guarantee needs bars no lower than `-2B`, and any bar below `-B` zeroes the
process, so a strategy that breaks its declared bound can never be called
skilled by it. A bar below `-2B` is outside the guarantee.

## The loss bound

The default bound is 1, a bar that loses the whole account, which caps the
stake at 1/2. A capture that declares the `drawdown_capped` mandate with bound
`D` in `(0, 1]` gives `B = D`: the kernel measures per-run drawdown on the
compounded equity curve, and a drawdown of at most `D` means no bar loses more
than `D`.

A bound must be fixed before the bars it caps, or it could be chosen from
them. So an artifact's bound is fixed by the first window in which it declares
one, takes effect from its next window, and never changes afterwards. Until
then the default applies. The commitment does not bind the declared mandate,
which travels in the capture; a bound tighter than the artifact's real losses
can only zero that artifact's own wealth, so it cannot manufacture evidence.

## What is observed

- **Only re-executed rows.** An observation is the pooled, seed-averaged
  return per bar of a row the arena re-derived by running the committed
  artifact again (`re-executed` provenance). Supplied and replayed rows are
  never observed, because nothing shows their returns came from the committed
  artifact without hindsight.
- **Per artifact digest.** The process belongs to the digest. An artifact
  entered in many windows is one process. Two agents revealing one digest in
  one window are one observation, the first by agent id.
- **Disjoint windows.** A window whose first scored bar is not dated after the
  ledger's last observed bar is not applied, and the header says why in
  `not_applied`. Overlapping data would show the bettor bars it has already
  seen. Dates compare as strings, which orders ISO 8601 dates.

## The family and e-BH

The family is every artifact committed to an evidence-bearing window: each
observed digest, plus each commitment that produced no observation (refused,
unrevealed or not re-executed), counted by its commitment hash at e-value 1.
Nothing leaves it. An operator who abandons an artifact and commits a fresh one
leaves the old one in the family at its last wealth, so restarting until
something gets lucky raises the bar.

Each ledger publishes the base e-BH decisions at the declared level 0.05: sort
the family's e-values from largest to smallest, find the largest `k` with
`e_[k] >= K / (0.05 k)`, and reject the `k` largest. e-BH controls the false
discovery rate under arbitrary dependence between the e-values (Wang and
Ramdas, 2022, Theorem 2), which matters because entrants trade one market path.
The guarantee is for the decisions of one board. Collecting every artifact any
board ever rejected is not covered.

## Where it is signed

Every board carries a `forward_evidence` object in its signed header:

| Field | Meaning |
|---|---|
| `schema` | `sharpebench-arena-forward-evidence/v1` |
| `prior` | The ledger before this window: the previous board's `posterior`, or the empty genesis ledger |
| `commitments` | The window's commitments, every one a family member |
| `record` | The window's observations, as `window.json` records them; omitted when no row was re-executed |
| `not_applied` | Why `record` was not folded in; omitted when it was |
| `posterior` | The ledger after this window |

A ledger lists each artifact's `bars`, `sum`, `sum_sq`, `e_value`,
`max_e_value`, `anytime_p_value`, `breached`, `windows` and the loss bound in
force, the unresolved commitment hashes, `family_size` and `discoveries`.
`board.md` prints the processes the window moved and the e-BH decisions.

A scored window that re-executed a row records its contribution in
`window.json` as `forward` and is written under schema 9 (a faulted window
never re-executes, so it never records one). No binary that predates the field loads
that schema, so a rollback cannot publish such a window without its ledger. A
window with no re-executed row keeps the schema and the bytes it always had.
Schema 9 implies support for schema 8's live capture roots, so a window with
both a live-captured row and a re-executed row is written under schema 9 and
keeps both records.

`arena verify` checks, for every board that carries the ledger, that `prior`
is the previous board's `posterior` (or genesis when no earlier board carries
one), that every observed agent has a re-executed row on the board, and that
`posterior` recomputes bit for bit from `prior`, the commitments and the
observations. The header's `record` and `commitments` must equal the window
file's (`forward_record` and `forward_commitments` in `identity_mismatches`).
Once a board carries the ledger, every later board must; a later board without
it fails with `forward_evidence_error`. Boards signed before the ledger existed
carry none and verify as they always did, and the first board after them starts
from the genesis ledger. The `--json` report adds `forward_ledger`, the newest
checked posterior, when there is one.

## Power

Under a Brownian approximation with unit annual volatility, annual Sharpe `s`
and horizon `T` years, the fixed-horizon z test is the ceiling no anytime-valid
test beats at its own horizon, and the likelihood-ratio e-process that knows
`s` is the best an e-process can do. Both are closed forms
(`fixed_horizon_power`, `oracle_likelihood_ratio_power`); the test suite pins
them to the values below. The normal-mixture column is the audit note's
4,000-path Monte Carlo; an independent 20,000-path rerun gives 0.432 and 0.766
for its first and last rows, within the note's error.

| alpha | s | T | Fixed horizon | Oracle LR | Normal mixture |
|---|---:|---:|---:|---:|---:|
| 0.00427 | 1.0 | 10 | 0.703 | 0.553 | 0.421 |
| 0.00427 | 1.0 | 20 | 0.967 | 0.909 | 0.868 |
| 0.00427 | 1.5 | 10 | 0.983 | 0.939 | 0.910 |
| 0.05 | 1.0 | 10 | 0.935 | 0.851 | 0.769 |

Years to 80 percent power at `s = 1`: 12.25 fixed against 15.25 for the oracle
e-process at alpha 0.00427, and 6.25 against 8.75 at alpha 0.05.

The shipped bettor estimates the alternative and is capped by the bound, so it
sits below the oracle. The rates below come from an independent numpy
implementation of the same rule on the same daily grid, 4,000 paths each
(standard error at most 0.008). The test suite reproduces the three marked
cells with the Rust bettor within Monte Carlo error.

| Annual vol | Bound B | alpha 0.00427, s 1, T 10 | alpha 0.00427, s 1, T 20 | alpha 0.00427, s 1.5, T 10 | alpha 0.05, s 1, T 10 |
|---:|---:|---:|---:|---:|---:|
| 1.0 | 1.0 | 0.146 | 0.816 | 0.694 | 0.687 (checked) |
| 1.0 | 0.5 | 0.441 | 0.851 | 0.922 (checked) | 0.742 (checked) |
| 1.0 | 0.2 | 0.365 | 0.802 | 0.882 | 0.668 |
| 0.15 | 1.0 | 0.000 | 0.000 | 0.000 | 0.000 |
| 0.15 | 0.5 | 0.000 | 0.000 | 0.000 | 0.000 |
| 0.15 | 0.2 | 0.020 | 0.648 | 0.325 | 0.519 |

The last three rows are the finding that matters for real entrants. The growth
optimal stake is `s / sigma` in annual units, about 6.7 for a Sharpe of 1 at 15
percent volatility, and the cap `1 / (2B)` is 0.5 under the default bound. A
capped bettor grows its wealth far more slowly than the optimum, so without a
tight declared loss bound a distribution-free forward test has essentially no
power at realistic volatility. A `drawdown_capped` declaration is what makes
the ledger informative.

None of this changes the low-power finding of the joint-gate study: no rule
reaches 80 percent power at `s = 1` on 10 years. What the ledger changes is
that forward evidence now accumulates instead of being discarded window by
window. Calendar time remains the constraint.

## Size

The test suite simulates null tracks offline with fixed seeds (3,000 tracks of
600 bars, bound 0.2) and checks that `P(sup K >= 1/alpha)` stays within
Monte Carlo error of `alpha`. Observed rates at alpha 0.1 and 0.05:

| Null | alpha 0.1 | alpha 0.05 |
|---|---:|---:|
| Mean zero, many small gains and a rare loss at the bound | 0.055 | 0.027 |
| Mean zero, volatility switching between two persistent regimes | 0.028 | 0.014 |
| Mean zero, chosen adversarially from the bettor's current stake | 0.040 | 0.016 |
| Negative drift | 0.001 | 0.000 |

Whether the cap holds its size under the GARCH, regime and cross-sectional
dependence of the joint-gate study's section 5 is an experiment for that study,
not something this page claims.

## References

- Grünwald, P., de Heide, R. and Koolen, W. M. (2024). Safe testing. *Journal
  of the Royal Statistical Society Series B* 86(5):1091-1128.
  doi:10.1093/jrsssb/qkae011.
- Ramdas, A., Grünwald, P., Vovk, V. and Shafer, G. (2023). Game-theoretic
  statistics and safe anytime-valid inference. *Statistical Science*
  38(4):576-601. doi:10.1214/23-STS894.
- Wang, R. and Ramdas, A. (2022). False discovery rate control with e-values.
  *Journal of the Royal Statistical Society Series B* 84(3):822-852.
  doi:10.1111/rssb.12489.
- Waudby-Smith, I. and Ramdas, A. (2024). Estimating means of bounded random
  variables by betting. *Journal of the Royal Statistical Society Series B*
  86(1):1-27. doi:10.1093/jrsssb/qkad009.
