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
sharpebench greeks <spot> <strike> <t> <r> <vol> <call|put>   Black-Scholes price + Greeks + tail-risk
sharpebench self-update                               update an update-enabled binary in place
```

Use `sharpebench --help` for the complete command and flag inventory. Commands
that render a human report accept the global `--json` flag for structured
output; file-producing commands already write their documented JSON artifact.

## `run`

Runs the reference agents (buy-and-hold, momentum) through the point-in-time
simulator over multiple windows × seeds with costs on, and prints the ranked
board. The teaching demo: watch deflation and pass^k in action.

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
sharpebench regime returns_a.csv returns_b.csv regimes.csv [--col NAME] [--json]
```

Compares two strategies' per-period returns *within* each market regime instead
of pooled. See [Regime-conditional comparison](methodology-regime.md). The three
CSVs are aligned by row; `regimes.csv` carries one label per period and is an
input, not something the CLI infers. Exit code is 0 whenever the report is
produced; read `pooled_hides_reversal` for the verdict.

## `audit-briefing` / `canary` / `score-allocation` / `greeks`

Standalone analysis surfaces over the kernel: lint a shared briefing for
input-side salience bias, derive a do-not-train contamination tripwire, score a
target-allocation weight-vector trajectory (validity + L1 turnover), and price an
option with its Greeks and short-gamma/vega tail-risk classification.

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
sharpebench disqualify <submissions.json> [--json]
```

Scores a JSON field of submissions (same format as `score`) and names every
disqualification/quality signal that fired for each agent, instead of the
single rank-eligible verdict. Five reasons mirror the scorer's hard eligibility
gates (`FailedPassK`, `DsrBelowBar`, `ProcessViolation`,
`BootstrapInsignificant`, `MandateBreached`); the advisory flags
(`HighSelectionGap`, `IsRediscovery`, `OosDecay`) are reported but never gate.
Pure legibility: nothing here changes eligibility semantics.

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
