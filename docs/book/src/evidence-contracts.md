# Evidence contracts

Deterministic scoring is meaningful only when the evidence proves which
experiment ran. SharpeBench binds resumable sweeps and captured trajectories at
the point where missing or mismatched work could otherwise become a shorter,
easier field.

## Resumable external sweeps

`sharpebench run --checkpoint <path>` stores a `SweepContract` beside the task
matrix. The contract binds:

- dataset SHA-256;
- cost-model digest;
- score-configuration digest;
- running CLI artifact SHA-256;
- entrant artifact SHA-256;
- invocation SHA-256, derived from the transport and exact endpoint, image, or
  command plus the names of explicitly passed environment variables;
- ordered evaluation windows;
- ordered execution seeds; and
- retry policy.

The digest of an image reference is available from its immutable
`repository@sha256:...` identity. A command line or HTTP address is not an
artifact identity, so checkpointed `--cmd` and `--http` runs require
`--entrant-sha256 <digest>`. Supplying that artifact digest does not weaken the
invocation binding: changing the address, command arguments, or
`SHARPEBENCH_AGENT_ENV` names still requires a new checkpoint.

Resume is exact. A missing, malformed, legacy, or different contract is an
error; SharpeBench does not silently replace it or mix tasks from two
experiments. Before assembly, every declared task must be terminal and
structurally consistent. A completed or agent-failed task carries exactly one
full-length run. A runtime-failed task carries no run and records at least one
attempt.

## Complete denominators

Entrant faults and infrastructure faults have different consequences:

- An entrant protocol or resource-limit fault produces a failing sentinel with
  the correct window length. It remains in the pass^k denominator.
- A runtime or transport failure is retried. If retries are exhausted, the
  sweep is incomplete and no certifying board is emitted.

The CLI reports expected, completed, runtime-failed, and agent-failed cell
counts in both text and JSON modes. Infrastructure failure cannot improve an
entrant by deleting a difficult cell.

## Principal paper sweeps

[`sweep_grid.py`](../../../paper/evidence/sweep_grid.py#L1) declares the principal
paper sweep independently of the records received. Each dataset must contain
every combination of these axes exactly once:

| Key field | Declared values, in assembly order |
|---|---|
| `dsr_bar` | `0.80`, `0.90`, `0.95`, `0.99` |
| `n_trials` | `1`, `10`, `50`, `200` |
| `sr_std_pinned` | `null` (automatic prior), `0.20`, `0.35`, `0.50` |
| `agent_id` | `buy-and-hold`, `momentum`, `hold`, `luck-floor-00` through `luck-floor-04` |

This is 4 × 4 × 4 × 8 = 512 cells per dataset, or 128 per DSR-bar shard.
Counts alone do not establish coverage. Missing or duplicate cells, undeclared
axis values and wrongly typed keys are refused; for example, `n_trials: 1.0`
is not the integer `1`, and an absent `sr_std_pinned` is not explicit `null`.

Input is strict JSONL: blank or malformed lines, non-object records, duplicate
JSON keys and nonfinite numbers anywhere in a record are errors. Dataset,
asset class, timeframe and periods per year must match the declared dataset.
`n_bars`, `n_symbols`, `n_windows`, `window_len`, `n_seeds` and ordered `regimes`
must agree across all cells and shards. Counts must be positive integers, the
seed count must be eight, and each window needs one nonempty regime label.

Within each configuration, entrant rows must also agree on effective trial
count, measurement controls, clone deduplication and deflation null mean. The
fields consumed by the table reducer must have the required boolean, finite
numeric, dispersion-label or positive-integer types. These checks validate
recorded inputs to the table, without recomputing their statistical meaning.

For four existing shards of one dataset, supply them in DSR-bar order:

```text
python paper/evidence/assemble_sweep.py OUT BAR-0.80 BAR-0.90 BAR-0.95 BAR-0.99
```

[`assemble_sweep.py`](../../../paper/evidence/assemble_sweep.py#L1) validates each
shard and the combined grid before opening `OUT`; invalid input leaves an
existing output untouched. It orders records by the axes above, with agent ID
varying fastest. This differs from the producer's incoming score-rank order.
Original JSON line contents, including numeric spelling and field order, are
preserved; only record order and LF line endings are standardized. This is not
canonical JSON reserialization or an atomic-write guarantee after validation.

The table reducer requires all nine dataset files: `us-indices-1d`,
`us-indices-1w`, `crypto-majors-1h`, `crypto-majors-4h`, `crypto-majors-1d`,
`crypto-majors-1w`, `fx-majors-1d`, `commodities-1d` and `rates-1d`, each with a
`.jsonl` suffix. It validates all nine grids before printing a table header:

```sh
python paper/evidence/analyze.py paper/evidence/final
```

Other files in that directory do not substitute for a required dataset. The
committed principal sweeps pass this structural check: 512 cells each, 4,608
total. Validation does not rerun the producer, recompute scores, establish
current-engine numerical parity, or validate the complete scored-record schema.
Frozen artifacts remain historical evidence. An intentional change to the
principal grid requires updating its declared contract, not deriving new axes
from whatever records arrive.

## Captured trajectories

Trajectory contract schema 2 binds dataset, costs, engine version, ordered
windows, ordered seeds, and, for CLI captures, the runner artifact. Strict
verification requires exactly one run for every window-by-seed cell in the
declared order.

For every run, the verifier checks:

- `0 <= start < end <= dataset length`;
- the exact declared window and seed;
- `steps.len() == end - start`;
- sequential step indices; and
- observation identity equal to the frozen dataset date for that step.

The replay score derives `execution_seeds_per_window` from the contract, so
seed replicates remain replicates instead of becoming extra market-time
observations. The intact capture and direct score are byte-identical under the
same configuration.

## Legacy evidence

Older trajectories still deserialize, but strict verification refuses an
absent or unsupported contract. The CLI option
`--allow-unbound-trajectory` permits an explicit diagnostic regrade. Such a
regrade recomputes returns from the recorded decisions, but it does not prove
the original data, runner, matrix, or replicate semantics.

## What the contracts do not prove

The contracts identify declared artifacts and execution geometry. They do not
prove who controlled an HTTP endpoint, whether a declared digest was measured
inside a remote service, or whether an operator published every attempted
experiment. Public claims still need pre-registration, artifact distribution,
and an independently published verifying key.
