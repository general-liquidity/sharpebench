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
