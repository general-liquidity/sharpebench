# G10: the statrs special-function question, measured and disposed

Date: 2026-09-10. Scope: `crates/sharpebench-stats/src/stats.rs` `erf`,
`norm_cdf` and `norm_ppf`, their pins, and every artifact whose printed bytes
depend on them.

This file is the evidence for G10. It replaces claim with measurement. The
earlier session recorded differences between the hand-rolled bodies and
`statrs`, and PR #39 pinned the hand-rolled bits; neither established which
implementation is closer to the true function. A difference is not an error, an
identical bit is not a correct formula, and a compatibility pin is not a
migration. Everything below was re-measured from scratch.

## 1. The reference and its precision

`mpmath` 1.3.0 arbitrary-precision arithmetic at `mp.dps = 60`, about 200 bits
of mantissa, roughly 10^44 times finer than the f64 unit roundoff. The `rug`
and MPFR route was not available on this host (no `rug` module), so the
60-digit `mpmath` evaluation is the reference of record.

Every grid argument is converted from its exact f64 bit pattern to an `mpf`, so
the reference is evaluated at the same real number the Rust code receives. The
error metrics are computed in 60-digit arithmetic against the exact reference,
not against a reference first rounded to f64.

| Function | Reference expression |
|---|---|
| `erf(x)` | `mpmath.erf(x)` |
| `norm_cdf(x)` | `mpmath.erfc(-x / sqrt 2) / 2`, the complementary form, which avoids the cancellation `1 + erf(x)` suffers in the left tail |
| `norm_ppf(p)` | `sign * sqrt(2) * y` where `erfc(y) = 2 * min(p, 1-p)`, solved by Newton iteration in 60-digit arithmetic from the f64 candidate. `mpmath` 1.3.0 has no `erfcinv`, and `erfinv(2p - 1)` loses every digit in the deep tails, so the complementary equation is solved directly |

Reference sanity, against published constants rather than against either
implementation under test:

```
erf(1)    0.8427007929497148693412206   (published 0.84270079294971486934122)
erf(0.5)  0.5204998778130465376827467   (published 0.52049987781304653768274)
Phi(1.96) 0.9750021048517795658634157   (published 0.975002104851780)
Phi(-3)   0.001349898031630094526651815 (published 0.00134989803163009452665)
```

Two reference limitations are stated rather than hidden. Beyond `|x| = 1000`
the `norm_cdf` reference is taken as the saturated 0 or 1, because `mpmath`'s
`erfc` series selection overflows there and the true value is 1 or 0 to within
10^-217000, far below anything an f64 represents. And in the far left tail the
true `norm_cdf` underflows below the smallest subnormal, so both
implementations return exactly 0 and the *relative* error there is 1 by
construction; that column is a property of f64, not of either implementation.

## 2. Accuracy, both implementations, whole-domain grids

857,623 points for `erf`, 857,609 for `norm_cdf`, 203,747 for `norm_ppf`,
1,918,979 in total: a dense linear sweep, a log sweep from 10^-320 to 10^2 on
both signs, the branch boundaries of the two hand-rolled approximations and
their f64 neighbours, signed zeros, the smallest subnormal, the smallest
normal, the largest finite, both infinities, NaN, and for `norm_ppf` a
`nextafter` walk of the 200 f64 values nearest 0 and nearest 1 plus
out-of-range and boundary arguments.

`hand` is Abramowitz and Stegun 7.1.26 with `norm_cdf = (1 + erf(x/sqrt2))/2`
and Acklam's rational approximation, as shipped through v0.19.0. `statrs` is
`statrs` 0.19.1 `function::erf::{erf, erfc, erfc_inv}` in the wrapper forms
adopted below.

### erf

| Band | n | hand max abs | hand max rel | statrs max abs | statrs max rel |
|---|---|---|---|---|---|
| signed zero and subnormal | 9,896 | 1.000e-09 | infinite | 4.941e-324 | 1.138e-01 |
| 2.3e-308 <= abs(x) <= 1 | 346,116 | 1.394e-07 | 3.85e+298 | 4.939e-11 | 9.489e-11 |
| 1 < abs(x) <= 3 | 200,380 | 1.386e-07 | 1.455e-07 | 7.000e-12 | 8.306e-12 |
| 3 < abs(x) < inf | 301,228 | 1.465e-08 | 1.465e-08 | 1.814e-15 | 1.814e-15 |
| infinities | 2 | 0 | 0 | 0 | 0 |
| NaN | 1 | NaN in, NaN out | | NaN in, NaN out | |
| all finite arguments | 857,622 | **1.394e-07** | infinite | **4.939e-11** | 1.138e-01 |

`statrs` is strictly closer to the reference at 811,411 points, `hand` at 8,
and they agree at 46,203. The two infinite / astronomically large relative
errors in the `hand` column are the same defect seen twice: `erf(0)` returns
`1e-9` instead of `0`, and every argument below about 10^-9 returns that same
constant, so the relative error diverges as the argument shrinks. The 1.138e-01
in the `statrs` column is subnormal quantization, not error: `erf(5e-324)` is
5.64e-324 and the nearest representable f64 is 4.94e-324, one ULP away.

Worst case for each, side by side:

```
hand   worst at x = 0.04513999999999996: hand 0.05090060070660829  ref-err 1.3938e-07
statrs worst at x = -0.5:                statrs -0.5204998777636538 ref-err 4.9393e-11
```

### norm_cdf

| Band | n | hand max abs | hand max rel | statrs max abs | statrs max rel |
|---|---|---|---|---|---|
| signed zero and subnormal | 9,896 | 5.000e-10 | 1.000e-09 | 0 | 0 |
| 2.3e-308 <= abs(x) <= 2 | 276,350 | 6.969e-08 | 3.036e-06 | 2.469e-11 | 1.030e-10 |
| 2 < abs(x) <= 6 | 60,382 | 6.928e-08 | 3.582e-03 | 1.016e-12 | 8.250e-11 |
| 6 < abs(x) < inf | 510,978 | 3.532e-12 | see note | 5.551e-17 | see note |
| infinities | 2 | 0 | 0 | 0 | 0 |
| NaN | 1 | NaN in, NaN out | | NaN in, NaN out | |
| all finite arguments | 857,608 | **6.969e-08** | 3.036e-06 | **2.469e-11** | 1.030e-10 |

The relative column in the far tail is the f64 underflow artifact described in
section 1 and carries no information about either implementation.

`statrs` is strictly closer at 604,528 points, `hand` at 0, and they agree at
253,080. There is no argument in the whole grid where the hand-rolled
`norm_cdf` is closer to the truth.

A qualitative change worth naming: the hand-rolled `norm_cdf` saturates to
exactly `0` from about `x = -8.3` downward, because A&S 7.1.26 saturates
`erf` there. The migrated function does not: `norm_cdf(-8.5)` is now
9.4795e-18 and `norm_cdf(-11.44)` is 1.3030e-30. Deep-tail PSR values that used
to floor at exactly zero are now small positive numbers. This is a correctness
improvement and it is the source of the single ordering change in section 4.

### norm_ppf

| Band | n | hand max abs | hand max rel | statrs max abs | statrs max rel |
|---|---|---|---|---|---|
| central, 0.02425 <= p <= 0.97575 | 190,333 | 2.227e-09 | 1.129e-09 | 7.192e-16 | 4.721e-16 |
| wings | 10,296 | 1.278e-08 | 1.125e-09 | 3.867e-15 | 4.086e-16 |
| deep tail, p < 1e-30 or 1-p < 1e-16 | 3,110 | 6.784e-08 | 1.763e-09 | 1.198e-14 | 3.925e-16 |
| boundaries p <= 0 or p >= 1 | 7 | 0 | 0 | 0 | 0 |
| NaN | 1 | NaN in, NaN out | | NaN in, NaN out (after the guard) | |
| all finite arguments | 203,746 | **6.784e-08** | **1.763e-09** | **1.198e-14** | **4.721e-16** |

`statrs` is strictly closer at 203,738 points, `hand` at 0, and they are equal
at 8. Acklam's documented 1.15e-9 relative accuracy is reproduced almost
exactly (measured 1.76e-9 worst case, no refinement step in the shipped body);
`statrs` is at 4.7e-16, roughly one ULP.

### Accuracy on the arguments the kernel actually evaluates

The kernel was instrumented (a temporary logging shim in `stats.rs`, reverted
before any commit) and run over the two committed golden fields and a full
`evidence_sweep` on `us-indices-1w`. That yields 178,472 distinct `erf`
arguments, 178,472 distinct `norm_cdf` arguments and 8 distinct `norm_ppf`
arguments. The golden fields alone account for 10,033 distinct arguments per
function, matching the earlier session's count.

| Function | distinct kernel arguments | argument range | hand max abs | hand max rel | statrs max abs | statrs max rel |
|---|---|---|---|---|---|---|
| `erf` | 178,472 | [-10.64, 32.18] | 1.394e-07 | 1.837e-04 | 4.939e-11 | 9.489e-11 |
| `norm_cdf` | 178,472 | [-15.05, 45.51] | 6.969e-08 | 1.0 (underflow) | 2.470e-11 | 1.030e-10 |
| `norm_ppf` | 8 | [0.875, 0.998161] | 2.895e-09 | 1.124e-09 | 5.593e-16 | 1.926e-16 |

`norm_ppf` is evaluated at only eight distinct quantiles across a whole sweep,
because `expected_max_sharpe` requests `1 - 1/N` and `1 - 1/(Ne)` for the four
host trial counts on the grid. All eight, with both implementations:

```
p                   hand                 hand err    statrs               statrs err
0.875               1.1503493805157545   1.397e-10   1.150349380376008    8.783e-17
0.9                 1.2815515641401563   1.404e-09   1.2815515655446006   3.003e-18
0.9540150698535697  1.6850969890758702   9.626e-10   1.6850969900384327   1.057e-16
0.98                2.053748909003034    1.629e-09   2.0537489106318225   1.642e-16
0.9926424111765711  2.4393139558632      2.005e-09   2.4393139538578947   6.307e-17
0.995               2.5758293064439264   2.895e-09   2.575829303548901    3.744e-16
0.9981606027941428  2.904466610244198    7.119e-10   2.9044666095322835   5.593e-16
0.9632120558828557  1.789241766492572    1.911e-09   1.789241764581628    1.086e-16
```

### Which is more accurate, where

Plainly: `statrs` is more accurate everywhere that matters, and the
hand-rolled bodies are not closer anywhere except at 8 of 857,622 `erf`
arguments, where the A&S polynomial happens to land nearer by coincidence.
`erf` improves by about 3.5 orders of magnitude, `norm_cdf` by about 3.5, and
`norm_ppf` by about 7. The hand-rolled `erf(0) = 1e-9` is a defect of the
formula, not a rounding difference.

The result that does **not** favour `statrs` unreservedly: `statrs`'s `erf` is
not correctly rounded either. It carries about 4.9e-11 of absolute error near
`x = 0.5`, roughly 10^5 ULP. Its `erfc` path (used by `norm_cdf`) is a little
better at 2.5e-11, and its `erfc_inv` path (used by `norm_ppf`) is near machine
precision. So this migration buys three to seven orders of magnitude, not
correctness to the last bit, and the regenerated pins are change detectors
rather than a correctness proof. That distinction is written into the pin
file's own header.

## 3. Dependency, feature, target and licence review

`sharpebench-stats` had no dependencies at all before this change. It now has
exactly one direct dependency:

```toml
statrs = { version = "0.19.1", default-features = false, features = ["std"] }
```

The feature selection is load bearing. `statrs` 0.19.1 declares
`default = ["std", "nalgebra", "rand"]`. Taking the defaults would pull
`nalgebra` and with it `simba`, `matrixmultiply`, `safe_arch`, `wide`,
`bytemuck`, `rawpointer`, `glam`, `num-complex`, `num-rational` and
`num-bigint`, several of which are unsafe-heavy SIMD crates, plus `rand` and
`rand_core`, for a linear-algebra and sampling surface the three wrappers never
touch. With `default-features = false, features = ["std"]` none of them is
compiled, confirmed by `cargo tree -p sharpebench-stats -e normal` both with
and without `--all-features`.

Compiled graph actually added:

| Crate | Version | Licence | `unsafe` in `src/` |
|---|---|---|---|
| `statrs` | 0.19.1 | MIT | none; the crate is `#![forbid(unsafe_code)]` |
| `approx` | 0.5.1 | Apache-2.0 | none |
| `num-traits` | 0.2.19 | MIT OR Apache-2.0 | 1 file |
| `libm` | 0.2.16 | MIT OR Apache-2.0 | 9 files |
| `thiserror` + `thiserror-impl` | 2.0.20 | MIT OR Apache-2.0 | none |
| `syn`, `quote`, `proc-macro2`, `unicode-ident` | current | MIT OR Apache-2.0 | build-time only |

`sharpebench-stats` keeps `#![forbid(unsafe_code)]` at its own root. The
workspace invariant is about workspace package roots and is intact, but the
compiled graph does now contain `libm` and `num-traits`, which use `unsafe`
internally. That is a real, if small, widening of the trusted surface and is
recorded here rather than glossed.

`Cargo.lock` gains 24 entries, because a lockfile records a package's optional
dependencies whether or not they are selected. Fifteen of those (`nalgebra`,
`simba`, `glam` at three versions, `safe_arch`, `wide`, `bytemuck`,
`matrixmultiply`, `rawpointer`, `num-complex`, `num-rational`, `num-bigint`,
`num-integer`, `rand`, `rand_core`, `autocfg`) are never compiled. They do
enter `cargo deny`'s graph, because `deny.toml` sets `[graph] all-features =
true`; `cargo deny check` passes on all four legs with them present, so the
advisory and licence surface is clean today, but the audit surface is wider
than the build surface and a future advisory against an uncompiled crate would
still fail CI.

Targets and toolchain: `statrs` 0.19.1 is edition 2024 with `rust-version =
1.89.0`, under the workspace's pinned 1.96.0 toolchain. The
`wasm32-unknown-unknown` build was exercised end to end (section 5); the
repository declares no MSRV of its own beyond the pinned toolchain, so
`statrs`'s 1.89.0 floor introduces no new constraint. Licences are MIT and
Apache-2.0, both already in `deny.toml`'s allow list; no new SPDX expression
was needed.

## 4. Impact ledger

### 4.1 Function outputs and methodology identity

| | Before (through v0.19.0) | After |
|---|---|---|
| `erf` | Abramowitz and Stegun 7.1.26, single rational-times-exponential form | `statrs::function::erf::erf` |
| `norm_cdf` | `0.5 * (1 + erf(x / sqrt 2))` | `0.5 * erfc(-x / sqrt 2)` |
| `norm_ppf` | Acklam rational approximation, three branches at `p = 0.02425` and `p = 0.97575` | `0.0 - sqrt(2) * erfc_inv(2p)`, guarded |
| `mean`, `variance`, `std_dev`, `skewness`, `kurtosis` | hand-rolled | unchanged, hand-rolled |

The moment estimators were deliberately excluded. The standardized moments use
the population normalisation `m2 = sum((x - mean)^2) / n` that the 2026-09-07
audit (R03) fixed, and `variance` is Bessel-corrected while `skewness` and
`kurtosis` are not; a general-purpose crate carries its own bias-adjustment
convention for exactly these quantities, and `kurtosis` here is non-excess with
a 3.0 fallback. Substituting a crate's version would have silently changed the
convention. The reason is recorded in the module doc as well as here.

`norm_cdf` and `norm_ppf` are reached through `erfc` and `erfc_inv` rather than
through a `Normal` distribution object. The free-function forms were verified
bit for bit against `Normal::cdf` and `Normal::inverse_cdf` over the whole
measurement grid: 0 mismatches in 857,608 `cdf` arguments and 0 in 203,739
in-range `ppf` arguments. The one form that did differ, a plain leading minus
in `norm_ppf`, returned `-0.0` at `p = 0.5` where the pre-migration body
returned `+0.0`; writing `0.0 -` reproduces the positive zero and is the reason
the code is written that way.

Total contract, preserved exactly. `statrs`'s `Normal::inverse_cdf` panics on
NaN and on any argument outside `[0, 1]`, where `norm_ppf` has always returned
NaN and the signed infinities. The wrapper guards all three cases before
calling in. `statrs`'s `erf` and `erfc` are already total (NaN to NaN,
infinities to the saturated values), so `erf` needs no guard;
`norm_cdf` keeps an explicit NaN guard so the contract is visible at the call
site rather than inherited.

### 4.2 Code goldens

Regenerated with `SHARPEBENCH_UPDATE_GOLDEN=1`.

| Fixture | leaf values | changed | fields that moved | max abs delta |
|---|---|---|---|---|
| `example_submissions.scores.json` | 186 | 8 | `dsr_se`, `dsr_ci_low`, `deflation_bar_per_period`, `deflation_bar_annualized_equivalent` | 2.168e-09 (`dsr_se`) |
| `synthetic_field.scores.json` | 248 | 28 | `deflated_sharpe`, `psr`, `dsr_ci_low`, `dsr_ci_high`, `dsr_se`, `deflation_bar_per_period`, `deflation_bar_annualized_equivalent` | 6.909e-08 (`deflated_sharpe`) |

No boolean, string or ordering value changed in either fixture. Agent order and
every verdict field are identical.

### 4.3 Bit pins

`crates/sharpebench-stats/tests/special_function_bits.rs`: 70 of 93 pins moved.
The unchanged 23 are the saturated tails, the infinities and the boundary
returns. The regenerated table belongs to the release that carries this
migration; the values v0.19.0 shipped are the pre-migration ones and the file
header now says so.

### 4.4 Reproducible producers, statrs effect isolated

The only sound way to measure this migration's effect on a producer is A/B on
the same tree. A copy of the working tree with only the three migrated files
reverted to `HEAD` was built separately, and every reproducible producer was
run in both trees from the same committed data with the same seeds. The
difference between the two output sets is the migration and nothing else.

| Producer | records | records changed | max abs delta `deflated_sharpe` | max abs delta `psr` | max abs delta deflation bar | verdict changes |
|---|---|---|---|---|---|---|
| `evidence_sweep` us-indices-1d | 512 | 512 | 6.936e-08 | 6.647e-08 | 5.149e-11 | 0 |
| `evidence_sweep` us-indices-1w | 512 | 512 | 6.941e-08 | 6.823e-08 | 1.134e-10 | 0 |
| `evidence_sweep` crypto-majors-1d | 512 | 512 | 6.932e-08 | 6.934e-08 | 4.279e-11 | 0 |
| `evidence_sweep` crypto-majors-1w | 512 | 512 | 6.957e-08 | 6.169e-08 | 1.134e-10 | 0 |
| `evidence_sweep` crypto-majors-4h | 512 | 512 | 6.800e-08 | 5.318e-08 | 1.747e-11 | 0 |
| `evidence_sweep` crypto-majors-1h | 512 | 512 | 6.772e-08 | 5.151e-08 | 2.095e-10 | 0 |
| `evidence_sweep` fx-majors-1d | 512 | 512 | 6.870e-08 | 6.537e-08 | 3.079e-10 | 0 |
| `evidence_sweep` commodities-1d | 512 | 128 | 6.870e-08 | 6.356e-08 | 5.149e-11 | 0 |
| `evidence_sweep` rates-1d | 512 | 512 | 6.909e-08 | 3.994e-08 | 1.490e-10 | 0 |
| `risk_managed_eval` | 100 | 89 | 6.926e-08 | 6.934e-08 | 0 | 0 |
| `external_rules_eval` | 351 | 344 | 6.958e-08 | 6.966e-08 | 1.324e-10 | 0 |
| `pass_witness` | 156 | 156 | 6.927e-08 | 6.873e-08 | 3.251e-11 | 0 |
| `luck_floor_1000` | 2,002 | 2,002 | 0 | 6.945e-08 | 0 | 0 |
| `mandate_eval` | 81 | 70 | 6.827e-08 | 6.934e-08 | 0 | 0 |
| `relative_mandate_eval` | 81 | 70 | 6.827e-08 | 6.934e-08 | 0 | 0 |
| `seed_leg_eval` | 2,646 | 2,646 | 6.648e-08 | 6.969e-08 | 0 | 0 |

Totals across all 16 producers and 10,025 records: `psr` moved on 9,210 values
with a maximum absolute delta of 6.969e-08, `deflated_sharpe` on 4,453 with a
maximum of 6.958e-08, `deflation_bar_per_period` on 4,724 with a maximum of
3.079e-10, `deflation_bar_annualized_equivalent` on 4,568 with a maximum of
1.961e-08. The largest single delta anywhere is 1.210e-07, on the perturbation
`spread` field of `risk_managed_eval`, which is a difference of two PSR values
and therefore carries both.

**Verdict fields: zero changes.** 7,313 `passed_k` values, 5,348
`rank_eligible`, 4,706 `eligible_never_catastrophic`, 5,238 `process_ok` and
4,608 `step_down_significant` were compared record by record on an exact grid
key. Not one moved. No eligibility verdict and no pass^k outcome changes.

**One ordering change, and it needs stating precisely.** In the two
`luck_floor_1000` summary records, `argmax_agent_configured` on `us-indices-1d`
moves from `luck-floor-999` to `luck-floor-695`. The cause is the deep-tail
saturation described in section 2: before the migration, all 1,000 luck-floor
agents scored `deflated_sharpe` exactly `0.0` on the `configured` and
`shipped_floor` paths, because `norm_cdf` floored, so the argmax over 1,000
identical zeros was an arbitrary tie-break. After the migration those scores
are distinct values between 1.3e-28 and 1.1e-26, and the argmax is the agent
that genuinely attains the maximum, which is also the agent the
`field_measured` path already selected in both trees. So the change removes a
degenerate tie rather than inverting a ranking; the reported maximum moves from
`0.0` to 1.1126e-26, still twenty-five orders of magnitude below the 0.95 DSR
bar, and `n_rank_eligible` remains 0 on every path in both trees. No published
prose cites `argmax_agent_configured` or either agent label. It is reported
here as an ordering change regardless, because the acceptance condition for
this row asks for one.

### 4.5 Artifacts that stay frozen

`paper/evidence/final/*.jsonl`, `paper/evidence/after-v0.3.0/*.jsonl`,
`paper/evidence/baseline-v0.2.1/*.jsonl`, `paper/evidence/prospective-forecast-field/`
and `paper/figures/*.pdf` are **not** regenerated.

The reason is measured, not procedural. `paper/evidence/final/` is the frozen
v0.9.0 snapshot, as `paper/sections/A-commands.tex` already states. Running the
current engineering tree's `evidence_sweep` on `us-indices-1w` and comparing to
the committed file, on an exact 512-key grid with matching key sets, gives:

```
deflated_sharpe    448 of 512 records differ, max delta 2.222e-01
psr                448 of 512 records differ, max delta 1.240e-01
raw_mean_return    448 of 512 records differ, max delta 5.424e-04
worst_run_drawdown 448 of 512 records differ, max delta 1.785e-03
bootstrap_p        256 of 512 records differ, max delta 9.745e-02
```

That is six orders of magnitude larger than anything this migration does, and
`raw_mean_return` moving proves the difference is in the simulation and agent
path, not in the statistics. No producer in this tree can reproduce those
records. Regenerating them under the statrs change would silently fold every
kernel repair since v0.9.0 into an artifact labelled as a numerics migration,
which is precisely the substitution `AGENTS.md` rule 3 and the goal's execution
policy forbid. They stay frozen and are labelled pre-migration in the
methodology chapter.

### 4.6 Wrappers and packaged surfaces

`npm/pkg/sharpebench_bg.wasm` was rebuilt with the pinned release recipe. Its
SHA-256 moves from
`bf26d0ef3054ef7a2a2eb3d80333c5993d1d2ed958f39383c231c4f5b2c32d4a` to
`40069fc989fb735aa6f03fa714adeb0dffc7491360d7bcd910c7d99dcdc99c08`. The
generated `.js` and `.d.ts` files are unchanged. All 20 npm tests pass against
the rebuilt bundle, and the three `sharpebench-wasm` native-parity tests pass
against the regenerated goldens.

The tutorial reports under `examples/forecast-quality/`, the prospective
forecast report, the text board and the `arena` records are unchanged, as the
earlier session predicted: nothing they print at their printed precision passes
through these three functions. The full 989-test `nextest` run over the
workspace confirms it, including the harness clone-merge tests that reconstruct
evidence fields.

## 5. Disposition

**Reject the migration.** The accuracy measurement in section 2 stands and is
not in dispute: statrs is closer to a 60-digit reference on 811,411 of 857,622
erf arguments, 604,528 of 857,608 normal-CDF arguments and 203,738 of 203,746
inverse arguments, and it is never worse on the latter two. It is rejected on a
property the accuracy work did not measure: reproducibility across the three
supported targets.

The migration was implemented and pushed as pull request #49 so the question
could be answered by evidence rather than argument, and continuous integration
answered it. The two code goldens, regenerated on Windows, reproduce on Windows
and fail on both Linux and macOS in run 34469688817. The control is
unambiguous: the same three jobs on main, carrying the hand-rolled bodies, pass
on every target.

Neither implementation uses a fused multiply-add, so this is not the usual
contraction difference. Both call the platform exponential and logarithm. What
differs is the arguments they pass. The Abramowitz and Stegun form evaluates a
single exponential of negative x squared, and the three platform libraries agree
on that at every argument this kernel evaluates; the statrs rational path does
not. That agreement is an empirical property of three vendors' math libraries
rather than a guarantee, which is worth stating plainly: the hand-rolled body's
reproducibility is a measured fact, not a design feature.

The deciding argument is what the product promises. A luck-robust benchmark
whose scores are cited in a paper and pinned by provenance digests promises
that a committed field rescored anywhere reproduces byte for byte. An error of
1.4e-07 that is disclosed, bounded and identical on every supported target is
compatible with that promise. An error of 4.9e-11 that varies by target is not,
because it makes two honest operators disagree about the same submission. The
hand-rolled error is orders of magnitude below every bar the kernel tests
against, so nothing in the published results turns on it.

What stays in place: the hand-rolled bodies, unchanged; the 93 exact bit pins
and the NaN contract that already protect them; and this measurement, which
turns "the existing pins suffice" from an assumption into a documented
comparison against a stated reference with a stated error metric.

What would reopen it: an implementation that is both closer to the reference
and bit-identical across the three targets. A vendored correctly-rounded
routine restricted to operations IEEE-754 defines exactly would qualify, as
would a build that forces one deterministic math library on every target and
demonstrates it in the three-platform job. Neither is available today. If the
paper's numerical evidence is ever regenerated wholesale, the question should
be reopened at that point, because the reproducibility baseline would be
re-established from scratch.

## 6. Verification

Every command was run from the worktree root with `CARGO_TARGET_DIR` redirected
to a second volume, because the system disk was at zero bytes free.

| Command | Exit |
|---|---|
| `cargo deny check` | 0, advisories ok, bans ok, licenses ok, sources ok |
| `cargo fmt --all --check` | 0 |
| `cargo clippy --all-targets --all-features -- -D warnings` | 0 |
| `RUSTDOCFLAGS=-Dwarnings cargo doc --workspace --exclude xtask --no-deps` | 0 |
| `cargo nextest run --workspace --exclude xtask` | 0, 989 passed, 14 skipped |
| `cargo test --workspace --exclude xtask --doc` | 0 |
| `python -m unittest paper/src/test_provenance.py paper/src/test_sweep_grid.py` | 0, 23 tests |
| `wasm-pack build crates/sharpebench-wasm --target nodejs --out-dir ../../npm/pkg --out-name sharpebench` | 0, wasm-pack 0.15.0 |
| `npm install && npm run build && npm test` | 0, 20 passed, node 24.18.0 |
| `cargo test --release -p sharpebench-harness --test evidence_fields_no_clone_merges -- --ignored` | 0, 1 passed (`mandate_field_changes_dispersion_source_on_three_panels`) |
| `cargo run --release -p sharpebench-harness --example <each of the 16 producers>` in both trees | 0 for all 32 runs |
