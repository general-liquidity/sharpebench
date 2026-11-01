# How to read a row

A scored row has no single field that says what it shows. The rank-eligible
flag is a conjunction of nine conditions, two of the bars it compares against
are configuration values that are not on the row, and the facts that qualify
it, such as the evidence mode or a stress replay, are attached after the board
is sealed. This page walks through the row in the order a reader should take
it: the headline verdict, the conditions behind it, the refusals, and the
qualifiers. A worked example closes the page.

The headline verdict is available as `sharpebench headline`,
`sharpebench score --headline` (see the [CLI reference](cli.md#headline)), the
wasm `headline` export, the npm `headline()` function and the MCP `headline`
tool. It is a separate record (schema `sharpebench.headline.v1`) and never a
field of the score, so it changes no score bytes, no rank, no ordinal and no
hash of the signed chain.

## The headline verdict

The verdict is one word. The first rule that matches decides it:

| Order | Verdict | When it applies |
|---|---|---|
| 1 | `refused` | No scored row exists, for example an arena intake refusal or an input refusal. Only the code that assembles a board can produce it. |
| 2 | `unavailable` | A statistic the gates read could not be computed: `bootstrap_error` is set, `deflation_error` is set for a numerical or configuration reason, or `exposure_path_error` is set for a reason other than the exposure-path refusal. |
| 3 | `degenerate_track` | The deflation family refused the track's shape, so the track has no Sharpe ratio (reason `DegenerateTrack`). |
| 4 | `disqualified` | A block-severity process violation (`process_ok` false) or a breached drawdown mandate (`mandate_ok` false), whatever the statistics say. |
| 5 | `skill_supported` | The row is rank-eligible. |
| 6 | `cannot_tell` | `FailedPassK` is the only hard reason, and `pass_k_floors_plausible` is false: pass^k needs a window whose per-run floor is out of practical reach, so a NO is expected of almost any track. |
| 7 | `no_evidence_of_skill` | Any other ineligible row: the deflated Sharpe is below its bar, the bootstrap is not significant, pass^k failed on reachable floors, the standard error was not certified, or the exposure path is unproven. |

For every score the scorer produces, `skill_supported` holds exactly when
`rank_eligible` is true, and every other verdict except `refused` lists at least
one hard reason. A row whose `pass_k_floors_plausible` is absent (a score
archived before the field existed) is never read as out of reach, so a failed
pass^k there reads `no_evidence_of_skill`.

There is no "luck" verdict. Failing to reject the hypothesis that a track has
no skill does not show that the track was lucky, so the last rule says only
that the evidence for skill is absent.

## The nine conditions and the reasons they produce

A row is rank-eligible only if all nine conditions hold. Each failed condition
produces a hard reason in the `disqualify` taxonomy, and the headline lists the
hard reasons of its row in `hard_reasons`.

| Condition | Row field | Hard reason |
|---|---|---|
| The deflated Sharpe clears `dsr_bar` | `deflated_sharpe` | `DsrBelowBar` |
| pass^k clears the per-run bar on every required run | `passed_k` | `FailedPassK` |
| No block-severity process event | `process_ok` | `ProcessViolation` |
| The bootstrap p-value is below `alpha` | `bootstrap_p` | `BootstrapInsignificant` |
| Every run respects the drawdown mandate | `mandate_ok` | `MandateBreached` |
| The deflation family was computed | `deflation_error` | `DeflationUnavailable` or `DegenerateTrack` |
| The bootstrap was computed | `bootstrap_error` | `BootstrapUnavailable` |
| The admitted deflated Sharpe's standard error is certified | `standard_error_refusal` | `StandardErrorUncertified` |
| The profit clears the risk the exposure history implies | `exposure_path_error` | `ExposurePathUnproven` |

`dsr_bar` and `alpha` are configuration values, not row fields. The
classifier and the headline read them from the configuration the board was
scored with.

When the deflation family or the bootstrap cannot be computed, the scorer puts
the deflated Sharpe at the 0.0 floor and the bootstrap p-value at the 1.0
sentinel, so `DsrBelowBar` or `BootstrapInsignificant` can be listed beside the
reason that names the cause. The verdict reads the cause.

Four more reasons are advisory and never remove a row from the rank:
`SelectionUnavailable`, `HighSelectionGap`, `IsRediscovery` and `OosDecay`. The
headline lists them under `qualifiers.advisory`. The last two need evidence a
submissions file does not carry, and only `sharpebench disqualify --known` and
`--oos-windows` supply it (see [`disqualify`](cli.md#disqualify)).

## Refusals

A refusal is a deliberate decision not to measure, and it is different from a
computation that failed. The verdict keeps the two apart.

The deflation family refuses a track's shape with one of four requirements,
listed in `sharpebench_core::DEGENERATE_TRACK_REQUIREMENTS`. The last three are
exported by `sharpebench-stats`, and the first is restated in core as
`sharpebench_core::CONSTANT_SERIES`. The row's `deflation_error` is
then `returns ` followed by the requirement, and the reason is
`DegenerateTrack`:

- `CONSTANT_SERIES`: every observation of the pooled track is the same value.
- `NO_WINDOW_VARIES`: every window is constant within itself, so the pooled
  dispersion is only the level difference between windows.
- `NO_RESOLVED_MOTION`: in every window the moments prove that no bar is a
  loss, or would be once its most distant bar is set aside.
- `NO_OBSERVED_LOSS`: no bar of any window is below zero.

The derivation of each is in [Deflated Sharpe & PSR](methodology-deflated-sharpe.md).
Any other `deflation_error`, such as an invalid `periods_per_year`, too few
observations or an interval level outside (0, 1), is `DeflationUnavailable`
and reads `unavailable`.

The exposure-path gate refuses with `EXPOSURE_PATH_UNPROVEN`. That refusal
reads `no_evidence_of_skill`; an `exposure_path_error` with any other text,
such as a statistic that is not finite, reads `unavailable`.

Some refusals happen before a row exists. The arena's intake refuses an entrant
and lists it under `refusals`, an external sweep with an exhausted runtime cell
emits no board, and malformed or misaligned input (duplicate agent IDs, an
unkeyed field under `--require-run-keys`, a duplicate period) is refused before
scoring. Those have no score to read; a board assembler that reports them as
rows uses the `refused` verdict with the refusal text as its `cause`.

## Qualifiers

Qualifiers are listed beside the verdict and never change it.

| Qualifier | Source | Values |
|---|---|---|
| `evidence` | the harness evidence mode, supplied by the caller | `unknown`, `certifying` with its mode, or `noncertifying` with its mode and reasons |
| `tied`, `tie_group` | `dsr_tied`, `tie_group` | whether the row shares its deflated-Sharpe tie band, and the band |
| `stress` | the stress replay record, supplied by the caller | `not_run`, `clean`, `marked`, `unavailable` |
| `advisory` | the classifier | the advisory reasons |
| `declared_mandate` | `mandate_verdict_label()` | the declared-mandate verdict, when one was declared |
| `rank_mode` | `Certification::describe()` | the opt-in rank-mode verdict, when a rank mode was applied |

A field read from a submissions file carries no evidence mode and no stress
replay, so `score --headline`, `headline`, wasm, npm and MCP report `unknown`
and `not_run`. A library caller that holds those records passes them in
`HeadlineContext`.

### Three meanings of "certifying"

The codebase uses the word in three unrelated senses, and none of them joins
the others:

1. The row's `evidence_mode.certifying` (harness `evidence_mode::disclosure`):
   true for a forward window, and for a historical window only when the
   entrant's cell could not reach the network.
2. The arena board's `certifying` flag (`board_certifies`): false when the
   board accepted supplied returns, ranked a row that was replayed rather than
   re-executed, or ranked no row.
3. The rank-mode `certification` (`--rank-mode lifecycle-certified/v1`): whether
   a row's lifecycle evidence meets that mode.

In the headline, "certifying" has the first sense only, and it appears only in
`qualifiers.evidence`. The arena's flag is reported there as a noncertifying
reason by a caller that has it, and the rank-mode verdict is reported as
`rank_mode`.

## A worked example

The field has three agents: `skilled`, a sine-wave track with a positive
drift; `gains-only`, a positive drift that never loses a bar; and `noise`, the
same wave with no drift.

```text
$ sharpebench score field.json --headline
#    agent                    DSR      PSR      pass^k   proc    boot_p    raw_ret headline
------------------------------------------------------------------------------------
1    skilled               1.0000   1.0000     yes 1/1    yes    0.0005    0.00216 skill_supported
—    gains-only            0.0000   1.0000     yes 1/1    yes    0.0005    0.00264 degenerate_track
—    noise                 0.4528   0.6667      NO 0/1    yes    0.2919    0.00016 no_evidence_of_skill
```

`skilled` is rank-eligible, so it reads `skill_supported`, and its empty
`hard_reasons` says that every condition held.

`gains-only` has the highest raw return and a PSR of 1.0, and both are
misleading. Its `deflation_error` is the `NO_OBSERVED_LOSS` refusal, so the
deflated Sharpe of 0.0 is the floor and not an estimate:

```text
$ sharpebench headline field.json
gains-only: degenerate_track [DsrBelowBar, DegenerateTrack]
```

The verdict is `degenerate_track` and not `no_evidence_of_skill`, because the
track was never measured. `DsrBelowBar` is listed because the floor is below
the bar, and the `cause` field of the JSON record carries the refusal text.

`noise` failed pass^k, and the board's footer says that window 0 needs an
annualized Sharpe of 2.65 per run, above the plausibility bound of 2.00. A row
whose only hard reason was that failure would read `cannot_tell`. This row also
has a deflated Sharpe below its bar and an insignificant bootstrap, so the
evidence against it does not depend on pass^k, and it reads
`no_evidence_of_skill`:

```text
noise: no_evidence_of_skill [FailedPassK, DsrBelowBar, BootstrapInsignificant]
```

Under `--json`, `score --headline` prints `{"board": ..., "headlines": [...]}`,
and the `board` member is byte-identical to the output of `score --json`.
