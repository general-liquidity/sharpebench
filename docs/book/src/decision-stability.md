# Decision stability

pass^k across execution seeds tells you whether an outcome repeats. It does not
tell you whether the agent's choices repeat. An agent that samples a different
order each time it is asked can pass pass^k, and it looks the same on the board
as one whose decisions are a fixed function of what it saw. Production records
of LLM trading agents find that this is the axis on which otherwise comparable
models separate: in one replay league three frontier models were
indistinguishable on decision quality while one changed its (symbol, side)
choice in about 35% of repeated cells and the others in about 90 to 95%. An
advisor audit that reports set and sizing stability across repeated runs as a
separate axis from validity makes the same point.

`sharpebench decision-stability` measures it from captured trajectories:

```bash
sharpebench capture <out-1.json> --cmd "python my_agent.py"
sharpebench capture <out-2.json> --cmd "python my_agent.py"
sharpebench decision-stability out-1.json out-2.json [--data <csv>] [--json]
```

The report is rank-neutral. It is a separate document carrying
`rank_input: false`, and no gate, score or rank reads it. `run` and `score`
output is unchanged.

## Replicates

Every captured run is a replicate of its window, identified by
`(window_start, window_end)`. The execution seeds of one capture are replicates
of each other, and so are the same seeds in a second capture of the same agent.
All trajectories must name the same agent, and each one must pass the strict
checks of `verify-trajectory` against the resolved dataset, the cost model and
the running binary; the score those checks compute is discarded. The cost model
is the default one unless `--short-borrow-bps` names the borrow rate the
trajectories were captured under, as it must for `verify-trajectory`. A
file named twice is refused, because it would add a replicate that agrees with
itself by construction.

## Which steps are compared

A trajectory records each decision with the date of its observation, not the
observation. The command regenerates the observation by replaying the recorded
decisions through the engine: the observation at a step depends only on the
frozen dataset, the run's seed, the cost model and the decisions recorded
before that step, so the replay presents exactly what the agent was shown.
`capture` never arms a fault plan (fault injection applies only to `run`
sweeps, which write no trajectory), so there is no faulted presentation to
reconstruct. Each observation is identified by SHA-256 over its
`sharpebench/canonical-json/v1` framed pre-image, the same canonical form the
forecast contract digests use. Under that form `0` and `-0` are one number. An
observation containing a non-finite number has no canonical form and the
command refuses the artifact.

The replicates of a window start in one group. At every step each group is
split by the step's observation digest, so a group at step `t` holds the
replicates whose observations were identical at `t` **and at every earlier
step**. The whole history is required because the
[determinism contract](submitting.md#decisions-must-be-deterministic-under-re-execution)
lets a decision depend on the run's earlier observations and on the agent's own
earlier decisions. Two replicates that meet on one observation after their
histories differed may decide differently without any non-determinism, so they
are not compared. With this rule an agent that honours the contract reports
exactly zero.

A replicate whose history no other replicate shares leaves the comparison for
the rest of the window, and each of its remaining steps is counted in
`steps_excluded_diverged_observation`. Observations diverge as soon as fills do.
Under the default cost model slippage depends on the seed, so the seeds of one
capture usually share only the steps before the agent's first fill: a
`capture buy-and-hold` file compares one step per window and excludes the other
79 steps of each of its 8 seeds. A second capture of the same seeds matches
every step of a deterministic agent, which is why repeated captures are the
replicates that compare a whole window.

## When two decisions differ

Two decisions differ when their `orders` arrays have different lengths, or when
the orders at any one position differ in `symbol`, `action`, `target_weight` or
`confidence`. These are the score-bearing order fields of the determinism
contract, and they are compared exactly:

- no tolerance is applied to `target_weight`. A tolerance would be a free
  parameter the contract does not define, and the engine executes the exact
  weight it is given;
- the order of the orders counts, because the engine fills them in that order;
- `reasoning` and each order's `rationale` are not compared: they are audit text
  the scorer never reads;
- `cost` is not compared: it reports spend, not a choice, and token counts can
  vary between samples that choose the same orders.

A group differs when its members hold more than one distinct decision. The
rate is the number of differing groups over the number of groups compared.
Every group counts once, whatever its size.

## The report

| Field | Meaning |
|---|---|
| `groups_compared` | groups of two or more replicates with a shared observation history |
| `groups_with_differing_decisions` | compared groups holding more than one distinct decision |
| `differing_fraction` | `{"status":"available","value":...}`, or `{"status":"unavailable","reason":...}` |
| `steps_total` | every recorded step of every replicate |
| `steps_compared` | steps inside a compared group |
| `steps_excluded_diverged_observation` | steps of a replicate whose history no other replicate shared |
| `steps_unreplicated` | steps of a window that has a single replicate |
| `windows[]` | the same counts per window, with `replicates` and every differing group (`step`, `observation_sha256`, `replicates`, `distinct_decisions`) |

The top-level counts are sums over the windows. For a window with two or more
replicates, `steps_compared + steps_excluded_diverged_observation` equals
`steps_total`; for a window with one, `steps_unreplicated` does.

The rate is typed unavailable rather than reported as zero when there is
nothing to compare: `single_replicate` when no window has two replicates, and
`no_matched_observation` when replicates exist but no step's history was shared
by two of them. The report also states the digest, grouping and difference
rules it applied, under `observation_digest`, `grouping` and
`decision_difference`.

## Guarantee boundary

A zero rate says that no compared group disagreed. It says nothing about the
steps that were excluded, and a rate over a handful of groups is a weak
statement: read it beside `groups_compared` and the excluded count. The rate
measures consistency, not quality; a stable agent can be consistently wrong.

The command reads SharpeBench trajectory files. SharpeArena's local model field
records its own per-step observation hashes and a repetition axis in its
journals, but those journals are not an input to this command.
