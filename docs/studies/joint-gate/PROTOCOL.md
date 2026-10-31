# Joint-gate validation protocol

Protocol `joint-gate-statistical-legs` and `joint-gate-integrity-legs`, version
1.0.0. Sealed 2026-10-31, not approved.

This document states what the study measures before any of it is measured. The
machine-readable forms are [`campaign.json`](campaign.json),
[`statistical.protocol.json`](statistical.protocol.json) and
[`integrity.protocol.json`](integrity.protocol.json), bound together by
[`seal.json`](seal.json). Every count here comes from
[`protocol-power.json`](protocol-power.json), written by
`paper/src/make-joint-gate-protocol-power.py`, or from the measured smoke run in
section 11. If this text and the sealed files disagree, the sealed files govern.

## 1. Status and authority

The protocol is frozen but not executable. Both documents carry
`budget.approved: false`, so `validate` refuses them and the producer refuses a
frozen run. That is deliberate. Running the campaign, and adopting the targets in
section 6, are owner decisions (D3). The targets are a proposal. Nothing here
changes a shipped default or threshold, and a rule the study finds feasible is
evidence for a versioned change, not that change.

Two amendments are expected before the run, each at a raised version as
`check_amendment` requires, and neither after any frozen draw has been read:

1. **Engine rebind.** The final run is on the merged engine. `scoring_spec_hash`
   holds a fingerprint of the linked scorer (`joint-gate-study fingerprint`): a
   board of a fixed field and the default configuration. If merging moves it, the
   producer refuses with `engine moved` and names both values.
2. **Approval.** The owner sets `approved: true` and names the approver.

Any other change after approval is a deviation, recorded in the report and in a
raised version, and a change made after reading frozen results makes the study
exploratory.

## 2. Claims

The study asks one question about one object: under the shipped scorer, or a
named variant of it, how often is a zero-skill entrant declared eligible, how
often is a skilled one, and does any candidate rule reach a stated power at a
stated false-eligibility rate on an affordable history.

It does not ask whether any agent has skill, whether the simulator is realistic,
or whether an eligible system is profitable to deploy. Paths in the statistical
legs are generated directly and are not market data.

The two legs are reported separately so that a power shortfall cannot read as an
integrity failure, or the reverse.

| Leg | Question | Measure | Protocol |
|---|---|---|---|
| Statistical | Deflation, reliability, bootstrap | False-eligibility rates and power curves | `joint-gate-statistical-legs` |
| Integrity | Fabrication, mandate, process, negative controls | Detection rates | `joint-gate-integrity-legs` |

## 3. Frozen estimand

These are the definitions in the shipped code at the sealed fingerprint. A
variant from the literature is a different statistic, and citing one for the
other is an error.

**Sharpe.** Per period, `SR = mean / sd`, with the `n - 1` standard deviation.
No annualization inside a statistic.

**PSR.** `PSR(SR*) = Phi(z)`, where

    z = (SR - SR*) sqrt(n - 1) / sqrt(1 - g3 SR + ((g4 - 1) / 4) SR^2)

`g3` and `g4` are population-normalized skewness and non-excess kurtosis. The
standard error is the plug-in at the **observed** Sharpe (Bailey and Lopez de
Prado 2012, 2014; Lopez de Prado, Lipton and Zoonekynd 2026, eq. 3), not the one
evaluated under the null (their eqs. 4 and 5). The radicand is floored at
`1e-12`. PSR is one minus a one-sided p-value under serial independence. It is
not the probability that the true Sharpe exceeds `SR*`.

**DSR.** `DSR = PSR(SR0)` with

    SR0 = null_mean + sigma ((1 - gamma) z(1 - 1/N) + gamma z(1 - 1/(N e)))

This is a shift to the expected maximum alone. There is no rescaling by the
standard deviation of the maximum, unlike the variant in the 2026 reference, and
no autocorrelation term. `null_mean` is 0. `N` is `max(host_n, field size)` plus
the entrant's declared trials, which are 0 here. `sigma` is per period:
`prior / sqrt(periods_per_year)` for a field under the measurement floor,
otherwise the shipped measured dispersion. The measured value is the sample
standard deviation of pooled per-period Sharpes after excluding tracks with no
Sharpe ratio, collapsing near-clones at cosine 0.995 and fencing votes at 10.75
robust scales, floored at the rule's prior. A fenced entrant keeps the dispersion
measured with its own vote. DSR is not a posterior.

**Pooled track.** Aligned seeds are averaged per bar within a window, and windows
are concatenated. Eight executions of one window are one market observation per
bar.

**Eligibility.** `rank_eligible` off the board of `rank`, unchanged:
`DSR >= dsr_bar`, pass^k over the runs against zero at `per_run_psr_bar`, process
clean, mandate respected, bootstrap p below 0.05 with no refusal of the deflation
or the bootstrap. The bootstrap is the shipped fixed-seed stationary bootstrap on
raw pooled returns, 2000 resamples, restart probability 0.1. The producer calls
the kernel and never recombines the legs.

**Rates.** Replication is by independent simulated field. Entrants, windows and
seeds nested in a field are not independent observations.

- `whole_field_false_eligibility`: P(any zero-skill entrant in a field is
  eligible). The primary error estimand.
- `focal_false_positive`: P(the fixed focal entrant is eligible) with zero skill.
  One entrant per field, so it is a binomial over fields and needs no cluster
  correction. A pooled per-entrant rate would need one when entrants share a path.
- `power_at_effect`: P(the focal entrant is eligible) with a planted annualized
  Sharpe `s`, the constant `s / sqrt(252)` added to a zero-mean unit-variance path
  before seed noise.

## 4. The rule is the study variable

Every rule below is a `ScoreConfig` with the named fields changed and all others
at the shipped default. The dispersion prior sets both `trials_sr_std` and
`min_measured_trials_sr_std`, so measurement can tighten a bar and cannot relax
it, the relation the defaults have. The list was fixed from the closed-form and
screen results in section 6, before any full-scorer draw on reserved seeds.

| Rule | Host N | Prior | Windows | Per-run bar | Passes | DSR bar |
|---|---:|---:|---:|---:|---|---:|
| R0-shipped | 50 | 0.50 | 6 | 0.90 | all | 0.95 |
| R1-three-windows | 50 | 0.50 | 3 | 0.75 | all | 0.95 |
| R2-pooled | 50 | 0.50 | 1 | 0.50 | all | 0.95 |
| R3-majority | 50 | 0.50 | 6 | 0.90 | two thirds of runs | 0.95 |
| R4-field-n | 12 | 0.50 | 6 | 0.75 | all | 0.95 |
| R5-low-prior | 50 | 0.25 | 6 | 0.75 | all | 0.95 |
| R6-field-n-low-prior-majority | 12 | 0.25 | 6 | 0.75 | two thirds of runs | 0.95 |
| R7-field-n-low-prior-three | 12 | 0.25 | 3 | 0.60 | all | 0.95 |
| R8-field-n-low-prior-pooled | 12 | 0.25 | 1 | 0.50 | all | 0.95 |
| R9-frontier | 12 | 0.25 | 1 | 0.50 | all | 0.80 |

Host N, prior, window count and per-run bar each vary. R9 also moves the DSR bar,
because the screen shows the 0.95 rules leave most of the whole-field limit
unspent, and a rule near the bound in section 6 has to spend it. The two-thirds
rule counts runs, because `PassMode::AtLeast` does. On one window, a per-run bar of
0.50 is close to no reliability leg, which is what the pooled rules mean to test.

Fields hold 12 entrants, so a host N of 12 is the field floor. A field of 50 is a
sensitivity (section 8).

## 5. Null grid

Twelve zero-skill generators. Each path has mean zero and unconditional variance
one, so a planted Sharpe means the same thing in every cell. Windows are
contiguous slices of one path, so dependence crosses window boundaries.

| Generator | rho | Tails | Volatility | Regime | Shared variance |
|---|---:|---|---|---|---:|
| iid-normal | 0 | normal | constant | none | 0 |
| ar-minus-0.2 | -0.2 | normal | constant | none | 0 |
| ar-0.2 | 0.2 | normal | constant | none | 0 |
| ar-0.4 | 0.4 | normal | constant | none | 0 |
| ar-0.6 | 0.6 | normal | constant | none | 0 |
| student-t5 | 0 | t, 5 df | constant | none | 0 |
| student-t3 | 0 | t, 3 df | constant | none | 0 |
| garch-t5 | 0 | t, 5 df | GARCH 0.08, 0.90 | none | 0 |
| regime-break | 0 | normal | constant | two-state | 0 |
| composite | 0.2 | t, 5 df | GARCH 0.08, 0.90 | two-state | 0 |
| shared-iid | 0 | normal | constant | none | 0.5 |
| shared-composite | 0.2 | t, 5 df | GARCH 0.08, 0.90 | two-state | 0.5 |

The regime is a hidden Markov chain with stay probabilities 0.995 and 0.98, calm
mean 0.08 calm-volatility units, and stress volatility twice calm, scaled to a
zero long-run mean. These are conventional daily-equity orders of magnitude,
chosen as stress settings. They are not estimates from any panel, and the study
says nothing about which generator resembles a market.

The committed development evidence stops at rho 0.2, and an earlier audit saw the
conjunction admit more at 0.4 to 0.6, which is why the grid goes there. The PSR
variance assumes serial independence, so a positive rho makes the gate too
favorable by construction, and this grid measures by how much on the whole rule.

Histories are 2520 and 5040 bars, ten and twenty years at 252 periods. Every
window count divides both. Each entrant has 8 execution seeds, with independent
noise of 0.1 path volatilities per seed. That level is an assumption, not a
measurement, and section 8 varies it. Costs are not modelled on these paths: the
frictionless case is the conservative one, because a cost holds a null mean below
zero. The engine controls in section 7 carry costs.

Draws for one (block, generator, history, replicate) are shared across rules and
effects, so comparisons between rules are paired within a replicate.

## 6. Planted skill, targets, and the bound

A rule cannot beat the most powerful test of the mean on the same returns. Under
iid normal returns, at per-entry level alpha, no rule of any geometry exceeds
power `Phi(s sqrt(years) - z(1 - alpha))`. At a whole-field limit of 0.05 over 12
independent entrants, alpha is 0.00427:

| Years | Smallest annual Sharpe with power 0.80 at any rule | Ceiling at s = 1 | Ceiling at s = 1.5 |
|---:|---:|---:|---:|
| 10 | 1.10 | 0.70 | 0.98 |
| 20 | 0.78 | 0.97 | 1.00 |

At a limit of 0.01 the first column is 1.26 and 0.89. At a per-entry level of
1e-6, the shipped pass^k leg's nominal rate, a Sharpe of 1 needs 31 years at 80
percent, and the shipped six-window rule needs more, because it spends its
history less efficiently. So a Sharpe of 1 at 80 percent on ten years is not
available to any rule at these limits, under the most favorable generator. The
study does not need to run to establish that, and does not test it.

The screen (candidate-rule table in `protocol-power.json`, 400,000 draws per cell,
iid normal, sampling-approximate, reserved seeds untouched) adds one thing a
fixed-bar calculation misses. Measured dispersion includes the skilled entrant.
In a field of 12, one entrant at s = 1.5 over ten years measures a dispersion near
0.53, so a prior of 0.25 yields a mean bar of 0.88, not the 0.42 the prior alone
implies. Lowering the prior below what the entrant's own skill measures buys
little. At 10 years and s = 1.5:

| Rule | Whole-field null | Power s = 1.5, 10y | Power s = 2, 10y | Power s = 1, 20y |
|---|---:|---:|---:|---:|
| R0-shipped | 0 of 400,000 | 0.05 | 0.36 | 0.01 |
| R2-pooled | 0 of 400,000 | 0.09 | 0.50 | 0.01 |
| R4-field-n | 0.00003 | 0.43 | 0.83 | 0.15 |
| R8-field-n-low-prior-pooled | 0.0005 | 0.69 | 0.98 | 0.60 |
| R9-frontier | 0.035 | 0.95 | 1.00 | 0.92 |

These are design inputs under the assumptions that favor the rules. They are not
results, and no replacement rule is recommended by them.

Proposed targets, any one of which makes a rule feasible at that target:

| Effect (annual Sharpe) | History | Why |
|---:|---|---|
| 2.0 | 10 years | Reachable by a mild change to the rule |
| 1.5 | 10 years | Inside the bound, needs the frontier |
| 1.25 | 20 years | Inside the bound on a long history |
| 1.0 | 20 years | The effect the published shipped-rule analysis is stated at |

Primary cells for power are `iid-normal`, `ar-0.2` and `composite`. The power
bound is 0.80 and the whole-field limit is 0.05. A smallest effect of interest is
an owner choice: these are the proposal, and the sensitivity to them is the power
curve at 0.5, 0.75, 1, 1.25, 1.5, 2, 3 and 6, the last a positive control that
must pass everywhere. The curves cover effects below and above each bar on
`iid-normal` and `composite`.

## 7. Integrity legs and negative controls

Each is scored under R0, at 2520 bars, 6 windows, 2 seeds, 657 replicates. `twin`
is the same track without the violation, admitted in the same replicate, so a
refusal is the violation's doing.

| Control | Construction | Detected when | What the row also records |
|---|---|---|---|
| look_ahead_oracle | Agent reads the next bar from the tape. Capture claims the committed image | Re-execution of the committed image diverges | Whether the scorer alone admits it, the expected case |
| fabrication | One recorded decision of an honest capture overwritten | Re-execution diverges | Scorer verdict |
| process_violation | Sharpe 8 track with a `DenylistBypass` on one random run | Ineligible with `process_ok` false | Twin admitted |
| mandate_breach | Sharpe 8 track with a 75 percent single-bar loss under a 50 percent per-run cap | Ineligible with `mandate_ok` false | Twin admitted |
| constant_window_griefer | Each window constant at its own level, among peers | Refused, and the focal entrant's dispersion and DSR unchanged to the bit | |
| dynamic_leverage | SR004: leverage in [0.5, 1.5] set from trailing volatility on a GARCH market path at annual Sharpe 0.87 | Ineligible | Sharpe lift over the market |
| cost_losing_random | Luck-floor allocators through `run_backtest`, Typical costs, zero-premium tape | No allocator eligible | Whether any is eligible at zero cost |
| passive_beta | Buy-and-hold through `run_backtest`, Typical costs, market Sharpe 0.87 tape | Ineligible | |

Re-execution is the defence for the first two, not the statistics. A control that
the scorer would admit is reported that way, because that is the finding: the
statistics do not see look-ahead, and the certifying path has to.

The 0.87 is the top of the realized range of the frozen US index panels, used as
a population value. Passive beta admitted on a tape at this Sharpe would be a
statistical failure of a no-skill entrant. Reported, not excused.

Replicates: 657 all detected give a Wilson lower bound of 0.99 at the two-sided 99
percent level. One miss in 657 fails the 0.99 bound, and the protocol reads that
as the finding it is.

## 8. Sensitivities

Reported beside the decision, never part of it, at 805 fields per cell (half
width 0.02 at a rate of 0.05, 0.045 at 0.5).

- **Block length.** Bootstrap restart probability 0.025, 0.05, 0.1, 0.2, so mean
  blocks of 40, 20, 10 and 5, on `iid-normal`, `ar-0.2`, `ar-0.6` and `garch-t5`,
  under R0 and R8. The kernel's own default is 0.1.
- **Seed average against per-seed path.** Noise of 0.5 path volatilities. The
  alternative scores each execution seed as its own path and admits an entrant
  only on every one. Averaging lowers track volatility, so this measures how much
  of a pass the average buys.
- **Field size.** 50 entrants under R0 and R8.

## 9. Multiplicity, precision, stopping, missingness

**Multiplicity.** Within a rule the claim is an intersection: every null cell and
every target cell must pass, so each is read at its own level and the
intersection-union test needs no adjustment. Across rules the question is whether
any is feasible, so the family is the ten rules with Bonferroni: each one-sided
bound at 0.005, the tails of a two-sided 99 percent Wilson interval, gives a
family level of 0.05. Sensitivities and curves are descriptive and outside the
family.

**Precision.** 4141 fields per primary cell, the smallest count whose Wilson half
width at 99 percent is at most 0.02 at a rate of 0.5, which covers every primary
rate. It is checked by `validate` against `required_simulation_runs`.

Operating characteristics of the decision at 4141 fields, exact binomial:

| Truth | P(claim supported) |
|---|---:|
| Whole-field rate 0.0125 | 1.00 |
| Whole-field rate 0.025 | 1.00 |
| Whole-field rate 0.0375 | 0.89 |
| Whole-field rate 0.05 (at the limit) | 0.004 |
| Power 0.80 (at the bound) | 0.004 |
| Power 0.82 | 0.74 |
| Power 0.83 | 0.99 |
| Power 0.85 | 1.00 |

So a rule needs true power near 0.83 and a true whole-field rate near 0.037 to
pass with high probability. The margin is the cost of the family level.

**FPR resolution limit.** With zero events in N independent fields, the 95 percent
upper bound is near 3/N. At 4141 fields that is 7.2e-4, and 1.3e-3 exact at the
one-sided 0.005 level this study reads. The 0 of 64,000 in the development
evidence bounds a per-entry rate near 4.7e-5, and bounding at 1e-5 takes about
300,000 independent fields. **The nominal 1e-6 of the shipped pass^k leg is not
resolvable by this study or any affordable one, and no claim here is made at that
scale.** The claim is the whole-field limit of 0.05 over the stated generators,
which is resolvable. A count of zero is reported as that bound and never as a zero
rate.

**Stopping.** Fixed. Every planned shard runs and there is no interim reading of
frozen draws. A frozen run cannot reduce its replicates, and a report refuses a
missing or short shard.

**Missingness.** A typed refusal stays typed. An entrant the kernel refuses is
ineligible, the field stays in the denominator, and `unavailable_entrants` says
how often. A null field where a refusal removes an entrant is therefore counted as
not false-eligible for that entrant, and the count is reported, so a reader can
see if refusals are carrying a rate. Infrastructure failure is a rerun of a
deterministic shard, up to three attempts. A shard that still cannot complete
leaves the run incomplete, and an incomplete run has no report.

## 10. Stated outcome

Exactly one of these, decided by `report.rs` from the realized counts. Each is a
result.

1. **Feasible rule.** At least one rule keeps every primary null cell's
   whole-field upper bound at or below 0.05, and reaches a target with every
   primary cell's power lower bound at or above 0.80. The report names the rules
   and which targets each reached.
2. **No feasible geometry.** At least one rule keeps size, and none reaches any
   target. Read with section 6: when the target is inside the bound, this says the
   candidates fall short of what the data allow. When it is outside, it is the
   bound.
3. **Size not controlled.** No rule keeps size across the null grid. The report
   names each rule's worst generator.

Separately, `integrity_passed` says whether every control's detection lower bound
reached 0.99. Neither statement implies the other.

Selection among feasible rules is not automatic. R0 is preferred when feasible,
then the listed order, because the list runs from least to most change. A preference
is not a decision to adopt: adoption is an owner call and a methodology version.

A tie between rules means nothing is claimed between them. Comparing rules is
descriptive, paired by replicate.

## 11. Compute

Expected cost is about **1,180 core-hours for the statistical protocol and 2.5 for
the integrity protocol**, about 19 hours of wall clock on 64 cores. The estimate
is a measurement, not a model: a two-replicate smoke run through every cell of the
sealed grid, scaled by each block's planned replicates. It ran on 24 workers on a
host shared with other build jobs, so it is an estimate on the high side, from one
sample. Each `rank` call took about 2 core-seconds on a field of 12.

| Block | Replicates | Rank calls per replicate | Core-seconds per replicate | Core-hours |
|---|---:|---:|---:|---:|
| null-grid | 4141 | 240 | 534 | 614 |
| power-10y | 4141 | 60 | 96 | 111 |
| power-20y | 4141 | 60 | 203 | 233 |
| power-curves | 805 | 320 | 687 | 154 |
| block-length | 805 | 64 | 105 | 24 |
| seed-path | 805 | 72 | 122 | 27 |
| field-50 | 805 | 12 | 75 | 17 |

The approved cap in each protocol is 1.5 times its estimate. There is no spend:
no model, provider or market data is involved. Shards are 5 replicates, about 45
core-minutes on the null grid, which bounds what an interruption can lose.

## 12. What this cannot establish

Direct paths have no market microstructure, no execution model, and no agent. The
statistical legs measure the rule on generators, not on markets, and the grid
bounds nothing outside itself. A rate on these generators is not an unconditional
bound on the rule. The bootstrap uses the kernel's fixed seed, so a rate here is
conditional on that seed's draws across paths, which is the shipped behaviour.
Regime edges, where skill exists in some windows only, are not planted. A zero-skill
null here has no cost, so it is harder on the gate than a cost-bearing field, and
the cost-bearing control runs separately. Entrants are not selected, so the
deflation count is not exercised against a real search. Forward windows and
real-history replicates are out of scope.

The current-engine rerun of the frozen panels is a separate producer
(`paper/src/make-current-engine-rerun.py`) and contributes nothing to these
claims. It writes beside the frozen evidence, never over it.

## 13. Reproduction

```
cargo build --release -p sharpebench-joint-gate
BIN=$CARGO_TARGET_DIR/release/joint-gate-study
$BIN fingerprint
$BIN plan --campaign docs/studies/joint-gate/campaign.json
$BIN smoke --campaign docs/studies/joint-gate/campaign.json --out DIR --replicates 2
$BIN frozen --campaign docs/studies/joint-gate/campaign.json --out DIR \
  --protocol docs/studies/joint-gate/statistical.protocol.json \
  --protocol docs/studies/joint-gate/integrity.protocol.json
$BIN report --campaign docs/studies/joint-gate/campaign.json --out DIR
```

Rerunning the same command after an interruption resumes: complete shards are
kept, a stray temporary is ignored, and a shard from another identity is a refusal.
Output directories inside frozen evidence are refused. A smoke run salts the seed,
so it never reads a frozen draw. Shard bytes do not depend on the worker count.
`DIR` holds `run-identity.json`, `effective-config.json` (the exact `ScoreConfig`
and digest of every rule and block), `cells.json`, `shards/`, `runtime.json` and
`report.json`. `runtime.json` is the only file with a clock, and no shard reads it.
