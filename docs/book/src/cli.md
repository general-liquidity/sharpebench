# CLI reference

The `sharpebench` binary (crate `sharpebench-cli`) is the command-line entry point.

```text
sharpebench run                       run reference agents through the sim and rank them
sharpebench score <submissions.json>  rank a JSON field of pre-computed submissions
sharpebench check <returns.csv> --trials N             test one return series for backtest honesty
sharpebench realism [--data <csv>]    run the stylized-facts dataset gate
sharpebench commit <agent> <window> <digest> <salt>   forward-attestation pre-registration
sharpebench stress                    run the adversarial stress suite (contamination-masked)
sharpebench audit                     self-audit: prove the scorer resists gaming
sharpebench sign <subs.json> <key> <out.json>         score + sign a board to a file
sharpebench verify <board.json> <key> verify a signed board's chain
sharpebench capture <agent> <out.json>                capture an agent's raw-decision trajectory
sharpebench verify-trajectory <traj.json>             replay a trajectory → recompute its score
sharpebench audit-briefing <briefing.json>            audit a shared briefing for salience bias
sharpebench canary <seed>                             derive a do-not-train contamination tripwire
sharpebench sandbox-check <image@sha256:digest>       run the live Docker-boundary acceptance checks
sharpebench score-allocation <alloc.json>             score a weight-vector trajectory (turnover)
sharpebench greeks <spot> <strike> <t> <r> <vol> <call|put>   Black-Scholes price + Greeks + local exposure
sharpebench self-update                               update an update-enabled binary in place
```

Use `sharpebench --help` for the complete command and flag inventory. Commands
that render a human report accept the global `--json` flag for structured
output; file-producing commands already write their documented JSON artifact.

## `run`

Runs the reference agents (buy-and-hold, momentum) through the point-in-time
simulator over multiple windows × seeds with costs on, and prints the ranked
board. The teaching demo: watch deflation and pass^k in action.

The built-in momentum agent uses a 10-return-interval lookback, requiring 11
observed closes per symbol. It equal-weights symbols with a positive return over
that exact trailing window. Shorter histories, nonpositive or nonfinite trailing
prices, or nonfinite returns receive explicit zero targets, not a shorter-window signal.
Rust callers can set `Momentum { lookback: L }`; zero or overflowing lookbacks
also leave the signal unavailable. The observation's history budget is separate:
requesting a lookback beyond it does not expose additional bars. Older versions
ignored this setting and used all supplied history, so their reference-agent
results must not be presented as measurements of the repaired strategy.

Three external-agent transports are explicit rather than interchangeable:

- `--image <repository@sha256:...>` launches an already-present, digest-pinned
  image through the fail-closed Docker boundary. No daemon, mutable reference,
  absent image, failed readiness check, indeterminate OOM verdict, or failed
  cleanup becomes host execution.
- `--cmd "<program>"` executes a trusted program on the host and prints an
  unsandboxed warning on every run. Its environment is cleared to a small
  platform allowlist; opt named variables in with
  `SHARPEBENCH_AGENT_ENV=NAME1,NAME2`.
- `--http <addr>` posts to an endpoint whose isolation the operator owns.

Add `--checkpoint <path>` to resume an external sweep. The checkpoint contract
binds the dataset, costs, score configuration, running CLI binary, entrant,
ordered windows, ordered seeds, and retry policy. A checkpointed `--cmd` or
`--http` run also requires `--entrant-sha256 <digest>` because a command line or
endpoint address does not identify the artifact that served it. A mismatched or
legacy checkpoint is refused rather than overwritten.

Exhausted runtime failures make the external sweep noncertifying: the CLI emits
expected, completed, runtime-failed, and agent-failed cell counts, then exits
without a board. Agent-caused protocol faults remain in the pass^k denominator
as failing sentinels.

See [The arena](arena.md#sandboxed-entrants) for the boundary and acceptance
evidence.

## `score`

Ranks a JSON field of pre-computed submissions (see
[Submitting an agent](submitting.md)). The board shows DSR, PSR, pass^k, process,
bootstrap p, and raw return, with a footer naming how many of the submitted agents
are eligible.

`score` and [`disqualify`](#disqualify) share these host scoring controls:

| Flag | Meaning |
|---|---|
| `--periods-per-year N` | Positive finite annualization frequency |
| `--execution-seeds-per-window N` | Positive integer number of adjacent execution replicates per window |
| `--pass-mode MODE` | `all`, `any`, `at-least:N`, or `relative-to-benchmark` |
| `--benchmark-agent ID` | Field member used for the relative verdict; defaults to `buy-and-hold` |

Each supplied flag requires a value; an omitted value is a usage error in both
commands.

Each submission may carry `declared_mandate`, as described under
[declaring a mandate](methodology-pass-k.md#declaring-a-mandate-at-submission).
The declaration adds `declared_passed_k` and `declared_mandate_eligible` beside
the host verdict; it does not change host eligibility, rank order or ordinal.
Unknown declaration kinds, duplicate agent IDs and whitespace-only IDs are
refused. IDs are compared exactly, without trimming or case normalization.

These identity checks do not verify temporal alignment of legacy JSON `runs`.
The caller still supplies consistent window, seed and period order across the
field; use [captured trajectory contracts](evidence-contracts.md#captured-trajectories)
when the task requires their stronger identity checks.

## Analysis CSV input

`check`, `regime`, `select`, `rediscover`, `uncertainty` and `decay-prior` share
readers for unquoted comma-separated analysis tables. They refuse quoted fields,
blank observations, ragged rows, missing selected cells and nonfinite or invalid
selected numbers. Headers must contain distinct, nonempty names. A multi-column
`select` file requires every candidate column to contain complete finite data.
Missing cells are never dropped independently to make shorter vectors.

Header detection is heuristic when no column is named: numeric series inspect
the first cell, while multi-column `select` inspects the whole first row. Prefer
explicit names where `--col` is offered. Other columns may hold text when only
one numeric column is selected. These checks apply to the numerical analysis
readers; the separate `import` command is unchanged.

## `stress`

Runs the adversarial stress suite (flash-crash, whipsaw, …) with
contamination-masking so an agent can't fingerprint the scenario.

## `audit`

Runs the [benchmark self-audit](integrity.md). Exits non-zero if any claimed defense
is not demoted.

## `commit` / `sign` / `verify`

The [forward-attestation](attestation.md) surface: pre-register a strategy digest,
sign a published board, and verify a board's chain. HMAC verification requires a
shared secret whose holders can also forge. Public verification uses the
Ed25519 chain and a verifying key obtained through an independent channel.

## `capture` / `verify-trajectory`

Capture an agent's raw per-seed×window decision trajectory to JSON, then have a
separate verifier replay it through the simulator and recompute the score from
the raw decisions. New captures bind the data, costs, engine, runner, exact
ordered windows, and exact ordered seeds. Strict verification requires every
declared cell and every decision step, validates step and observation identity,
and derives replicate grouping from the contract. Missing, duplicated,
reordered, shortened, or cross-environment evidence is refused.

`--allow-unbound-trajectory` is an explicit legacy or cross-version regrade. It
does not claim that the artifact reproduces its original execution conditions.
See [Evidence contracts](evidence-contracts.md).

## `regime`

```bash
sharpebench regime returns_a.csv returns_b.csv regimes.csv [--col NAME] [--regime-col NAME] [--period-col NAME] [--json]
```

Compares two strategies' per-period returns *within* each market regime instead
of pooled. See [Regime-conditional comparison](methodology-regime.md).
`--col` selects the return column in both strategy files; `--regime-col` selects
the label column independently. Without those flags, each reader uses its first
column. Regime labels are supplied by the caller.

All three files must have the same number of complete observations; no series
is truncated. `--period-col NAME` additionally requires a nonempty, unique period
ID on every row and the identical ordered ID sequence in all three files. The
period column must differ from the selected value column. It compares trimmed
ID strings, without parsing dates, sorting rows or joining an intersection:

```sh
sharpebench regime a.csv b.csv labels.csv \
  --col return --regime-col state --period-col period --json
```

Here the strategy files have `period,return` headers and the label file has
`period,state`. Without `--period-col`, row alignment is the caller's assertion;
equal lengths do not establish common dates or temporal support. Invalid input
produces no report and a nonzero exit. A produced report exits 0; read
`pooled_hides_reversal` for its verdict.

## `lineage`

```bash
sharpebench lineage strategy-evidence.json [--json]
```

Verifies one SharpeArena generated-strategy ledger and reports its observed
trial count, candidate ancestry, cited idea sources, and best-versus-median
robustness within each host-derived strategy family. It recomputes the ledger
and family bindings and requires validation scores for every selectable
candidate. The report is diagnostic only and cannot alter eligibility, rank, or
the trial denominator. See [Candidate lineage diagnostics](candidate-lineage.md).

## `audit-briefing` / `canary` / `score-allocation` / `greeks`

Standalone analysis surfaces over the kernel: lint a shared briefing for
input-side salience bias, derive a do-not-train contamination tripwire, score a
target-allocation weight-vector trajectory (validity + L1 turnover), and price an
one long European option with its Greeks and local gamma/vega exposure flags.
Invalid inputs and undefined Greek vectors are refused. Local Greeks do not
establish payoff boundedness; see [Options pricing and payoff risk](options-risk.md).

## `select`

```bash
sharpebench select <candidates.csv...> [--alpha A] [--utility mean_return|sharpe] [--seed N] [--boot N] [--block-prob P] [--json]
```

Ranks candidate strategies on a percentile of their bootstrapped utility instead
of the point-estimate argmax, so the winner has to be good on most resampled
histories rather than on the one that happened to be observed. Pass one CSV per
candidate (first column read), or a single CSV whose columns are the candidates.

The output names both the point winner and the percentile winner, whether they
agree (disagreement is the whole reason to run this), and each candidate's
optimism gap: how much of its headline utility fails to survive resampling. The
point winner's gap is the number to report next to any headline result.

`--alpha` defaults to 0.5, the middle of the band. An alpha below 0.3 still
computes but prints a warning: the extreme lower tail of a bootstrap
distribution is decided by a handful of unlucky resamples nobody has real data
for. The warning flags a choice; it does not veto one. Deterministic given
(data, `--seed`).

## `disqualify`

```bash
sharpebench disqualify <submissions.json> [--periods-per-year N] [--execution-seeds-per-window N] [--pass-mode MODE] [--benchmark-agent ID] [--json]
```

Scores a JSON field of submissions (same format as `score`) and names every
disqualification/quality signal that fired for each agent, instead of the
single rank-eligible verdict. Pass the same field and [host controls](#score) as
`score`: explanations come from the ranked field, including its benchmark and
field-dependent deflation, rather than separately scoring each submission.
Five reasons mirror the scorer's hard eligibility
gates (`FailedPassK`, `DsrBelowBar`, `ProcessViolation`,
`BootstrapInsignificant`, `MandateBreached`); the advisory flags
(`HighSelectionGap`, `IsRediscovery`, `OosDecay`) are reported but never gate.
JSON rows contain `agent_id`, host `rank_eligible` and `reasons`. These reasons
explain the host score only, including its drawdown mandate; they do not explain
the separate declared-mandate verdict. To inspect that verdict, use the
`declared_*` fields returned by `score`. Explanation generation changes neither
eligibility nor rank.

## `rediscover`

```bash
sharpebench rediscover <submitted.csv> <known.csv...> [--threshold T] [--center] [--json]
```

Screens a submitted pooled return stream against a library of known prior
strategy streams and flags near-duplicates on `|cosine|` similarity. A stream
must be all but collinear with a known one to flag (default threshold 0.97);
leveraged and inverted variants of a known stream flag too, while
correlated-but-distinct strategies do not. `--center` de-means first (Pearson);
the default compares raw direction, because for return streams the direction is
the strategy. Novelty screening only: it says nothing about skill.

## `uncertainty`

```bash
sharpebench uncertainty <returns.csv> [--reference <csv>] [--outcomes <csv>] [--confidences <csv>]... [--json]
```

Decomposes the uncertainty behind one scored case into three legs, printed side
by side and never summed:

- **aleatoric** (from `--outcomes`, 0/1 per decision): irreducible outcome
  noise; more evidence will not reduce it. High reading: stop looking.
- **epistemic** (from repeatable `--confidences` streams): reducible ignorance,
  read off disagreement between independent confidence streams plus how thin
  the evidence behind them is. High reading: keep looking.
- **distributional** (case returns vs `--reference`): unlikeness to the
  reference series, as a location or dispersion shift. High reading: the
  reference cannot vouch for this case.

The epistemic leg is a lower bound, never an upper one: unanimous or correlated
signals understate it, so a low reading is weak evidence of knowledge and only
high readings are informative. The command prints this caveat with every
result. Inputs you omit are reported as not measured, not as zero risk.

## `decay-prior`

```bash
sharpebench decay-prior --measured-ic <ic.csv> --adoption X --theta Y --delta-max Z [--curvature C] [--anomaly-ratio R] [--json]
```

Measures the edge's half-life from its IC series (regressing `ln|IC|` on time)
and sets it against the expected half-life from a crowding model,
`ln2 / (theta + delta_max * adoption^curvature)`. The expected half-life is a
model prior, reported never gating: it comes out of a crowding model, not out
of a dataset, and nothing ranks on it. All rates are per period of the supplied
IC series, and there is deliberately no default calibration; the caller owns
every rate.

A measured/expected ratio below `--anomaly-ratio` (default 0.5) flags the decay
as too fast for crowding to be the whole story, which usually points at
overfitting, a broken data pipeline, or a regime the strategy was never fit
for. The flag is a diagnostic, not a verdict.
