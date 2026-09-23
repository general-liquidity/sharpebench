# Development-tier evidence

Exploratory calibration runs. Everything in this directory is **development
tier**: it is not frozen validation, no number here may be reported as a
validated claim, and nothing here was tuned against a held-out set. Frozen
evidence stays in `paper/evidence/final/` and is not touched by anything here.
These files are outside the provenance manifest's artifact scope for the same
reason: they are not paper artifacts.

## joint-gate-power.jsonl

Ticket P12-A, section 10.A of the remaining-product-work plan: calibration of
the full joint eligibility rule rather than one leg of it.

Producer: `paper/src/make-joint-gate-power.py`, with the importable parts in
`paper/src/joint_gate_power.py` and regressions in
`paper/src/test_joint_gate_power.py`.

```
python paper/src/make-joint-gate-power.py --jobs 12
```

No agent, simulator, market data, model or provider is involved. The producer
reads the committed default-cell deflation bars and simulates returns.

### Legs modelled

As `composite.rs` builds `rank_eligible`:

- deflation: pooled DSR at least 0.95 against the panel's committed per-period
  bar in the default cell, held at its committed value;
- reliability: pass^k in all-runs mode, six windows each reaching a per-run PSR
  of at least 0.90 against zero;
- bootstrap: stationary-bootstrap p below 0.05 on the pooled track, with the
  shipped 2000 resamples and restart probability 0.1.

Each leg is a threshold on the true Sharpe, so one set of draws gives the whole
curve. Adding a constant to a return series leaves its dispersion and shape
alone, and the bootstrap resamples the centered track, whose null distribution
of resampled means does not move when the constant is added. The joint rule
passes at true Sharpe s exactly when s reaches the largest of the three
thresholds, and its false-positive rate is that curve at zero.

### Legs not modelled

The process gate, the mandate gate, the risk gates, the influential-vote and
dispersion disclosure, costs and execution-seed variation, missingness and
refusal accounting, the measured bars' dependence on the field that produced
them, cross-agent dependence within a field, and the kernel's single fixed
bootstrap seed (the leg here is averaged over resampling noise instead).

Every unmodelled leg is a further conjunct, so it can only refuse more. The
pass probabilities below are upper bounds on the shipped predicate's, and the
false-positive rates are upper bounds on its rate.

The replay diagnostics in `crates/sharpebench-sim/src/replay_nulls.rs` are not
legs of this rule. `rank_eligible` never reads them; they are reported beside a
verified trajectory and the CLI is their only consumer.

The bootstrap leg is not modelled on `crypto-majors-1h`, whose pooled track of
23940 returns costs more per replication than this tier affords at 2000
resamples. That panel's joint rows are absent and carry a `leg_not_modelled`
record instead; its two-leg rows still bound the shipped predicate.

### Run identity

Seed 20260923, 64000 replications for the two-leg conjunction and 1600 for the
three-leg one, split into 64 fixed seeded chunks per geometry. The output bytes
do not depend on the worker count. Wall clock on 12 workers: 76.9 seconds for
the two-leg phase, 206.2 for the three-leg phase, 4 minutes 43 seconds end to
end. The runtime is printed and written by `--runtime-out`, never into the
evidence file, so the file stays byte-reproducible.

The two phases draw separately, so a rule measured in one is not comparable
with a rule measured in the other. Every record therefore names its `draws`,
`two_leg_run` or `joint_run`, and the three-leg run reports all six rules from
its own draws so the bootstrap leg's increment can be read within one run.

False-positive rates carry exact Clopper-Pearson bounds at one-sided 0.05 on
each side. Power points carry the Wilson score interval at the same level and
the binomial standard error.

### False-positive rate under the zero-skill null

Per entry, on `us-indices-1d` with serially independent returns. The field
column is the probability that at least one of the panel's 8 default-cell
entries is admitted, treating entries as independent; entries in a real field
share one market history, so this is not what such a field would show.

| Rule | Admitted | Rate | 95 percent upper | Field upper |
|---|---|---|---|---|
| one window at PSR 0.90 | 6434 of 64000 | 0.1005 | 0.103 | 0.579 |
| pass^k, six windows | 0 of 64000 | 0.0000 | 4.68e-05 | 0.000374 |
| deflation | 0 of 64000 | 0.0000 | 4.68e-05 | 0.000374 |
| pass^k and deflation | 0 of 64000 | 0.0000 | 4.68e-05 | 0.000374 |
| bootstrap | 82 of 1600 | 0.0512 | 0.0613 | 0.397 |
| all three legs | 0 of 1600 | 0.0000 | 0.00187 | 0.0149 |

The two calibrated legs land where their nominal levels put them: a single
window at PSR 0.90 admits 0.1005 of zero-skill entries and the bootstrap leg at
alpha 0.05 admits 0.0512. No replication passed any conjunction on any panel.
That is a bound, not a zero rate: with 64000 replications the two-leg rule's
per-entry rate is at most 4.68e-05 and the three-leg rule's, at 1600
replications, is at most 0.00187.

The three-leg bound is looser than the two-leg one only because the bootstrap
leg is expensive, not because the extra leg admits more. The joint rule is a
subset of the two-leg rule, so 4.68e-05 bounds it too.

### Detection power

True annualized Sharpe at which each rule admits with probability 5, 50 and 95
percent, at rho 0. `Bar` is the panel's committed annualized deflation
benchmark, so the columns cover the region below and above each bar.

| Panel | Bars | Bar | 2 legs 5% | 2 legs 50% | 2 legs 95% | Joint 5% | Joint 50% | Joint 95% |
|---|---|---|---|---|---|---|---|---|
| us-indices-1w | 78 | 1.1382 | 1.28 | 2.07 | 3.02 | 1.29 | 2.05 | 2.98 |
| us-indices-1d | 408 | 1.1382 | 1.22 | 1.97 | 2.88 | 1.24 | 1.98 | 2.88 |
| crypto-majors-1w | 46 | 1.1382 | 1.67 | 2.71 | 3.95 | 1.71 | 2.71 | 3.91 |
| crypto-majors-1d | 156 | 1.1382 | 2.39 | 3.85 | 5.65 | 2.41 | 3.85 | 5.61 |
| crypto-majors-4h | 990 | 1.1382 | 2.32 | 3.74 | 5.46 | 2.29 | 3.74 | 5.48 |
| crypto-majors-1h | 3990 | 27.3309 | 27.34 | 28.35 | 29.36 | not modelled | not modelled | not modelled |
| fx-majors-1d | 682 | 6.8254 | 6.82 | 7.26 | 7.69 | 6.81 | 7.25 | 7.67 |
| commodities-1d | 677 | 1.1382 | 1.14 | 1.55 | 2.24 | 1.16 | 1.54 | 2.24 |
| rates-1d | 682 | 3.4362 | 3.43 | 3.85 | 4.27 | 3.43 | 3.84 | 4.25 |

The two-leg columns come from the 64000-replication run and the joint columns
from the 1600-replication one, so the small differences between them are
sampling noise between two runs, not the bootstrap leg's effect. On the
three-leg run's own draws the bootstrap leg changes nothing: on all eight
panels where it is modelled, the two-leg and three-leg crossings agree to two
decimals at 5, 50 and 95 percent. A leg that admits one zero-skill entry in
twenty is far weaker than a conjunction of six per-window tests at 0.10 and a
deflation bar above an annualized Sharpe of 1, so it almost never binds. Those
same-draws rows are the `draws: joint_run` records.

At each panel's own bar the joint rule admits rarely: on `us-indices-1d` an
agent whose true annualized Sharpe equals the 1.1382 bar is admitted with
probability 0.032, at half the bar with probability 0.000, and at twice the bar
with probability 0.719. On `commodities-1d` the same three points are 0.042,
0.000 and 0.956. On `crypto-majors-1d`, whose windows span under half a year,
they are 0.000, 0.000 and 0.033. The full grid, at multiples 0.25 through 3 of
each bar plus four absolute anchors, is in the `power_point` records.

### Candidate gate designs at matched false-positive rates

Closed form under the same effect definition: serially independent normal daily
returns at 252 periods a year, true annualized Sharpe 1, history measured as
the total length each design needs to reach the stated power. The designs are a
pooled single test, a majority rule requiring four of six windows, and the
shipped every-window rule, each solved to the same nominal per-entry rate.

| Matched rate | Design | 50 percent power | 80 percent power |
|---|---|---|---|
| 1e-06 | every window, six of six | 38.0 years | 56.8 years |
| 1e-06 | four of six windows | 32.9 years | 45.7 years |
| 1e-06 | pooled single test | 22.6 years | 31.4 years |
| 0.05 | every window, six of six | 5.6 years | 14.0 years |
| 0.05 | four of six windows | 4.0 years | 9.1 years |
| 0.05 | pooled single test | 2.7 years | 6.2 years |

At the shipped rule's own nominal rate of 1e-06 the window structure costs a
factor of 1.68 in history at 50 percent power and 1.81 at 80 percent, against a
pooled test. Requiring four windows instead of six costs 1.15 and 1.24. The
factor is not a property of the window structure alone: matched at 0.05 instead,
the every-window rule costs 2.05 and 2.25 against a pooled test and 1.41 and
1.54 against four of six. Six conjunctive windows only reach a whole-rule rate
of 0.05 at a per-window bar of PSR 0.393, below one half, where a window passes
at a negative observed Sharpe and the squared PSR gate has to be solved on the
signed branch.

A superseded comparison is kept in the evidence under
`comparison: superseded_unmatched_false_positive_rate`, labelled and never used
as a factor. It reproduces the earlier draft's fourteenfold result, 38.0 years
against 2.7, by comparing the every-window rule at a nominal rate of 1e-06 with
a pooled test at 0.05. Those are different rules at different rates, so the
ratio is not a comparison of gate designs. `require_matched` in
`joint_gate_power.py` refuses to hand back an unmatched comparison, and
`test_joint_gate_power.py` pins both the superseded factor and the corrected
one so the error cannot come back silently.

### Dependence sensitivity

The same calculation with stationary Gaussian AR(1) returns, normalized to unit
marginal standard deviation so a true annualized Sharpe means the same thing at
every rho. Windows are contiguous slices of one track, so the dependence
carries across window boundaries. On `us-indices-1d`:

| rho | One window, false-positive rate | 2 legs 5% | 2 legs 50% | 2 legs 95% |
|---|---|---|---|---|
| -0.2 | 0.0597 | 1.23 | 1.80 | 2.55 |
| -0.1 | 0.0804 | 1.20 | 1.89 | 2.71 |
| 0.0 | 0.1005 | 1.22 | 1.97 | 2.88 |
| 0.1 | 0.1237 | 1.25 | 2.08 | 3.08 |
| 0.2 | 0.1479 | 1.27 | 2.19 | 3.30 |

The direction is the one eq:psr predicts: it assumes serially independent
returns, so it is too favorable under positive autocorrelation and too strict
under negative. A single window's false-positive rate runs from 0.0597 at rho
-0.2 to 0.1479 at rho 0.2 against a nominal 0.10, and the true Sharpe the
two-leg conjunction needs for 95 percent power moves from 2.55 to 3.30. Both
error rate and power move together, so a reader cannot read one without the
other. First-order autocorrelation is not measured on any panel here, so this
says how the verdicts would move under a stated dependence, not how they move
on the real panels.

### What this does not establish

Real returns are not AR(1) Gaussian draws either. The bars are held at their
committed values, although a different field could move a measured bar or switch
a configured panel to a measured one. A regime-dependent edge is not one true
Sharpe in every window and is not covered. Nothing here measures any agent's
true Sharpe, so it does not say which, if any, refused agents had skill. The
replication counts here were chosen to fit a development-tier budget: they
bound rare-event rates only to the precision stated above, and a frozen
validation would need its own sample-size rationale.
