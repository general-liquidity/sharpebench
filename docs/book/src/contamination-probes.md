# Contamination probes

A pretrained model may have read the very price history it is scored on. The
paper's protocol rule answers that for certification: a backtest verdict for a
pretrained model is advisory, and certification needs a forward window. The
probes on this page do not change that rule. They measure, for one entrant,
how much of its behaviour depends on handles that identify the history, and
they are validated offline with planted agents. None of them calls a model.

Every probe here is rank-neutral. Its document carries `rank_input: false` and
`rank_neutral: true`; nothing reaches a score, a rank, the gate, eligibility or
the pass^k pool, and `run` output is the same whether or not a probe has run.

## What the mask hides and what it does not

`Dataset::masked_view` (the `--masked` flag) renames symbols to `ASSET_nnn`,
replaces dates with `t0, t1, ...`, and rebases each series by a power of two so
every simple return, and so every backtest, is bit-identical. It hides names,
dates and price level. It does not hide the path: the returns are the source's
returns exactly. Lopez-Lira, Tang and Zhu find that masking fails for language
models because they reconstruct entities and dates from minimal context
(arXiv:2504.14765), and Entity Neutering judges a mask by whether an independent
model identifies the subject at the rate of random chance (Engelberg, Manela,
Mullins and Vulicevic, SSRN 5182756). The probes below test the mask that way,
and report where it fails.

## Partial blinding levels

The full mask hides three handles at once, so a gap against it says that an
agent reads one of them, not which. `Dataset::blinded_view` hides one at a
time, after the four-level protocol of KTD-Fin (arXiv:2605.28359, section 3.3):

| Level | Symbols | Dates | Price level | KTD-Fin level |
|---|---|---|---|---|
| `tickers_only` | `ASSET_nnn` | real | real | stock-blind |
| `dates_only` | real | `t{i}` | real | date-blind |
| `levels_only` | real | real | rebased | none |
| `full` | `ASSET_nnn` | `t{i}` | rebased | blinded (plus level) |

`full` is `masked_view` byte for byte, so the `--masked` digest and every
existing golden are unchanged. Every level keeps returns exact.

## Date-shift presentation

A date mask shows an agent no calendar at all, which an honest agent might also
react to. `Dataset::date_shifted` shows the same bars under real-looking dates
moved by a declared number of days, the counterfactual-input idea of Profit
Mirage (Li, Zeng, Xing, Xu and Xu, arXiv:2510.07920) applied to the calendar.
Symbols, prices, dividends and marks are untouched. The shift relabels the
calendar rather than resampling one, so a shifted trading day can fall on a
weekend; an offset that is a multiple of 7 keeps every weekday (364 days is 52
weeks). A dataset whose dates are not `YYYY-MM-DD` calendar days is refused
rather than guessed, and so is a zero offset.

## `presentation-stability`

```bash
sharpebench presentation-stability <entrant> [--data <csv>] [--date-shift-days N] [--short-borrow-bps <bps>] [--declared-training-cutoff YYYY-MM-DD] [--json]
```

The entrant is one of `--agent buy-and-hold|momentum`, `--cmd "<prog>"`,
`--http <addr>` or `--image <repository@sha256:...>`; a gateway-attached
entrant runs through the same harness call
(`sharpebench_sim::presentation_suite` with
`sharpebench_harness::contamination_probe::record_external_run`). On each of
`run`'s windows a fresh agent runs once per presentation, every decision is
mapped back to the source symbols, and these pairs are compared exactly:

1. unmasked against each of `tickers_only`, `dates_only`, `levels_only` and
   `full`;
2. `full` against `full` with the symbols listed in reverse (position bias);
3. unmasked against the date shift, when `--date-shift-days` is given.

Each pair reports the steps compared, the disagreeing steps, the first
disagreement, the mean largest weight gap and both arms' per-bar Sharpe. The
document is `sharpebench.presentation-stability.v1`.

What the planted agents show (`crates/sharpebench-sim/tests/presentation_stability.rs`
and `crates/sharpebench-cli/tests/contamination_probe_cli.rs`):

| Planted agent | tickers-only | dates-only | levels-only | full | reordered | date shift |
|---|---|---|---|---|---|---|
| momentum, equal-weight hold | 0 | 0 | 0 | 0 | 0 | 0 |
| ticker recall (buys SPX by name) | every step | 0 | 0 | every step | 0 | 0 |
| date recall (keyed on calendar dates) | 0 | every step | 0 | every step | 0 | most steps |
| level reader | 0 | 0 | every step | every step | 0 | 0 |

Both arms of the reordered pair are fully masked, so neither shows the
date-recall agent a date. A 52-week shift lands most days on another remembered day, so it recalls the
wrong bar rather than nothing, and its Sharpe falls under the shifted calendar.

## `contamination-probe`

```bash
sharpebench contamination-probe <entrant> [--data <csv>] [--window-bars N] [--windows N] [--twin-seed N] [--with-costs [--short-borrow-bps <bps>] [--fill-delay-bars N]] [--declared-training-cutoff YYYY-MM-DD] [--json]
```

The shape-level probe for the part the mask cannot hide. Each of up to
`--windows` (default 12) consecutive windows of `--window-bars` (default 40)
bars after a 20-bar warmup gets a synthetic twin: a stationary-bootstrap
resampling of its own bar relatives that matches its total return, volatility
and regime within declared tolerances. Both are masked. The entrant runs once on
each, and the edge is real Sharpe minus twin Sharpe; a whole-window sign-flip
test with a minimum-edge floor decides `memorization`. Every twin is built before
any arm runs. The document is `sharpebench.contamination-probe.v1`.

Both arms run frictionless by default. The probe asks whether an agent recalls
the path, not whether recall pays after costs, and a recalling agent that
switches its book every bar pays away under the default impact charge what it
remembers: the planted `--cmd` memorizer is convicted frictionless and was not
convicted under `run`'s costs when the fixture was built. `--with-costs` opts
both arms into `run`'s cost model, and only with it do `--short-borrow-bps` and
`--fill-delay-bars` apply; given without it, either is a usage error rather
than a flag silently ignored. `--frictionless` is still accepted and changes
nothing, and combined with `--with-costs` it is a usage error. The document
records the mode as `context.frictionless`.

Its stated limit stands: block resampling copies stretches of the real path into
the twin, which biases the edge toward zero, never toward a false conviction.

## `identifiability-probe`

```bash
sharpebench identifiability-probe (--cmd "<prog>" | --http <addr> | --image <repository@sha256:...>) [--data <csv>] [--window-bars N] [--windows N] [--twin-seed N] [--top-k N] [--year-tolerance N] [--declared-training-cutoff YYYY-MM-DD] [--json]
```

The probe asks a closed question per window and symbol, after KTD-Fin's
de-anonymization probe (section 4.3): here is one masked close series, here are
the candidate tickers and years, name your top-k tickers and one year. It asks
the same question about the window's twin, whose answer is taken to be the real
window's, and never says which arm a question comes from; the two are asked in
a seeded order. It reports top-1, top-k, year and joint (top-1 ticker and year)
hit rates for each arm, the rates of a uniform guesser, and a sign-flip test on
the per-window real-minus-twin joint rate (`path_identified`). Windows are those
of `contamination-probe` (`--window-bars`, default 40, after a 20-bar warmup, up
to `--windows`, default 12); `--top-k` defaults to 2 and `--year-tolerance` to
0. The source must carry `YYYY-MM-DD` dates, because the year of a window's
first scored bar is the truth, so the synthetic default dataset is refused as
`undated_window`. The document is `sharpebench.identifiability-probe.v1`.

The question travels the entrant's own transport, so the probe reaches every
external entrant the other probes reach. A `--cmd` or `--image` entrant reads a
question as one stdin line in place of an observation, an `--http` entrant
receives it as `POST /identify`, and a gateway-attached entrant gets it on the
decision pipe, where any model call it makes to answer is brokered, charged and
journaled like a decision's
(`sharpebench_harness::contamination_probe::ask_external` over a
`GatewayEntrant`). The question is `sharpebench.identity-question.v1`:

```json
{"schema_version":"sharpebench.identity-question.v1","question_id":0,"symbol":"ASSET_000",
 "dates":["t0","t1","..."],"closes":[64.0,64.3,"..."],"scored_from":20,
 "candidate_tickers":["NDX","RUT","SPX"],"candidate_years":[2016,2017],"top_k":2}
```

and the answer is one line of `sharpebench.identity-answer.v1` that echoes the
`question_id`:

```json
{"schema_version":"sharpebench.identity-answer.v1","question_id":0,"tickers":["SPX","NDX"],"year":2016}
{"schema_version":"sharpebench.identity-answer.v1","question_id":0,"declined":true}
```

Tickers are ranked best first and only the first `top_k` count; a name that is
not a candidate is a miss. Naming nothing (no tickers, no year) is an answer
and scores no hit. The answer is closed: an unknown member, another
`question_id`, a declined answer that also names something, or a line that is
not an answer at all is the entrant's protocol fault. Every question runs on a
fresh entrant (a fresh process, connection or container), so no answer can
lean on an earlier question and a real window never meets its twin in one
process. The reference agents have no channel to answer on, so `--agent` is a
usage error here, and so are the cost flags, because the probe runs no
backtest.

A declined question, or a spawn, transport, protocol, budget or resource
failure on any question, refuses the whole document with
`unavailable.reason: "answer"`, the `question_id`, and an `error` of
`{"kind":"declined"}` or `{"kind":"failed","failure":"<failure kind>"}`. No
failed question is scored as a miss: an entrant that refuses or crashes would
otherwise read as one that cannot identify the series.

The negative control is the expected result, and it is reported as such. A
planted path-matching attacker that holds the public frozen dataset
re-identifies every masked window, ticker and year, and none of the twins,
because masked returns are bit-identical to the source's. The mask defeats
name, date and level lookups, not path lookups. A planted texture reader that
ranks tickers by volatility identifies the twins as often as the real windows,
and is not called a path identification. What the planted entrants show over
`--cmd` on the first 300 dates of the daily index file (seven windows, three
symbols, 42 questions):

| Planted entrant | real joint | twin joint | real top-1 | twin top-1 | `path_identified` | status |
|---|---|---|---|---|---|---|
| path recall (looks the path up) | 1.0 | 0.0 | 1.0 | 0.0 | true | measured |
| texture only (volatility rank, no year) | 0.0 | 0.0 | 0.48 | 0.48 | false | measured |
| declines every question | | | | | | unavailable, `declined` |
| decision-only (exits when asked) | | | | | | unavailable, `transport_error` |
| uniform guesser (chance) | 0.17 | | 0.33 | | | |

See `crates/sharpebench-sim/tests/identifiability.rs`,
`crates/sharpebench-cli/tests/contamination_probe_cli.rs` and
`the_identifiability_probe_runs_against_a_gateway_entrant` in
`crates/sharpebench-harness/src/gateway_serve_tests.rs`, where a fake provider
that recalls paths is caught through the gateway on the real windows and not on
their twins. Pointing the probe at a model field needs model calls and is a
separate study; nothing here calls one.

## Declared training cutoff

```bash
sharpebench run --cmd "<prog>" --declared-training-cutoff YYYY-MM-DD [--json]
```

An operator may declare when an entrant's model stopped seeing data. On `run`
(with an external entrant only) the declaration is published on the entrant's
row as `declared_training_cutoff` (`sharpebench.declared-training-cutoff.v1`),
beside `evidence_mode`, and both probe commands put it in their context. It
classes each scored window as ending before the declared day, starting on or
after it, straddling it, or unclassifiable, reading the cutoff as an
information boundary the way FORESIGHT-9 evaluates agents only after one
(arXiv:2608.29372). It is `trusted: false` and `verified: false`, and
`evidence_mode` keeps ignoring it: the row is byte-identical to one without the
flag apart from this field.

Verifying a cutoff is future work and needs model calls: Gao, Jiang and Yan
check one with a date-only recall query whose Lookahead Propensity collapses
right after the true cutoff (arXiv:2512.23847), and a chronologically
consistent model such as ChronoGPT holds its cutoff by construction rather
than by declaration (He, Lv, Manela and Wu, arXiv:2502.21206).

## Failures and refusals

A spawn, transport, protocol, resource or finalization failure on any arm
refuses the whole document with a typed `unavailable` object that names the
window, the arm or presentation and the failure kind. An arm is never dropped:
an external agent that degrades to a hold would otherwise put the harness's
holds into the comparison as the entrant's own decisions. A window that cannot
be twinned, an undefined Sharpe ratio, or a calendar that cannot be shifted is
refused the same way. Exit codes are `0` measured, `1` unavailable or
unreadable, `2` usage.
