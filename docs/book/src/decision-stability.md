# Decision stability

pass^k across execution seeds tells you whether an outcome repeats. It does not
tell you whether the agent's choices repeat. An agent that samples a different
order each time it is asked can pass pass^k, and on the board it looks the same
as one whose decisions are a fixed function of what it saw. Production records
of LLM trading agents show this axis separating otherwise comparable models: in
one replay league three frontier models were indistinguishable on decision
quality, while one changed its (symbol, side) choice in about 35% of repeated
cells and the others in about 90 to 95%. An advisor audit that reports set and
sizing stability across repeated runs as an axis apart from validity makes the
same point.

`sharpebench decision-stability` measures it from captured trajectories:

```bash
sharpebench capture out-1.json --cmd "python my_agent.py"
sharpebench capture out-2.json --cmd "python my_agent.py"
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
the running binary; the command discards the score those checks compute. The
cost model is the default one unless `--short-borrow-bps` names the borrow rate
the trajectories were captured under, as it must for `verify-trajectory`.

A copy of a capture agrees with the original by construction and would pull the
rate toward zero. The command refuses a file named twice. It also compares the
JSON bytes of every run: when two runs of one window are byte-identical, it
reports them as `identical_replicate_runs` and refuses the field unless you pass
`--declare-identical-replicates`. Pass it only when the runs are separate
executions. Two captures of a deterministic agent over the same seeds are
byte-identical. Two captures of a sampling agent differ as soon as one decision
or its audit text does. The report cannot tell a re-capture from a copy: equal
bytes look the same either way, so a zero over declared identical replicates is
only as good as the declaration. The report lists every input by digest so a
reader can see which inputs are equal.

## Which steps are compared

A trajectory records each decision with the date of its observation, not the
observation. The command regenerates the observation by replaying the recorded
decisions through the engine. The observation at a step depends only on the
frozen dataset, the run's seed, the cost model and the decisions recorded
before that step, so the replay presents exactly what the agent was shown.
`capture` never arms a fault plan (fault injection applies only to `run`
sweeps, which write no trajectory), so no faulted presentation needs
reconstructing. The command identifies each observation by SHA-256 over its
`sharpebench/canonical-json/v1` framed pre-image, the canonical form the
forecast contract digests use. Under that form `0` and `-0` are one number. An
observation containing a non-finite number has no canonical form, and the
command refuses the artifact.

The replicates of a window start in one group. At every step the command splits
each group by the step's observation digest, compares the decisions inside each
part, and then splits each part by the decision its members made. A group at
step `t` therefore holds the replicates that were shown equal observations at
`t` and at every earlier step, **and** made the same decisions at every earlier
step. The
[determinism contract](submitting.md#decisions-must-be-deterministic-under-re-execution)
lets a decision depend on the run's earlier observations and on the agent's own
earlier decisions, so replicates whose histories differ in either may decide
differently without any non-determinism. With this rule an agent that honours
the contract reports exactly zero.

The decision split also stops one difference from being counted many times. A
difference the engine does not act on, such as a confidence, an action label or
a restated target, leaves the observations equal. Without the split, an agent
whose later choices follow from that earlier choice would register the same
difference at every later step.

Replicates leave the comparison in two ways, and the report counts each:

- a replicate whose observation no other member of its group shares leaves at
  that step, and its remaining steps count in
  `steps_excluded_diverged_observation`;
- a replicate whose decision no other member shared is compared at that step
  and leaves afterwards, and its remaining steps count in
  `steps_excluded_diverged_decision`.

Observations diverge as soon as fills do. Under the default cost model slippage
depends on the seed, so the seeds of one capture usually share only the steps
before the agent's first fill: a `capture buy-and-hold` file compares one step
per window and excludes the other 79 steps of each of its 8 seeds. A second
capture of the same seeds matches every step of a deterministic agent, which is
why repeated captures are the replicates that compare a whole window.

## When two decisions differ

Two decisions differ when their `orders` arrays have different lengths, or when
the orders at any one position differ in `symbol`, `action`, `target_weight` or
`confidence`. A stated confidence differs from an unstated one. These are the
score-bearing order fields of the determinism contract, and the command compares
them exactly:

- it applies no tolerance to `target_weight`. A tolerance would be a free
  parameter the contract does not define, and the engine executes the exact
  weight it is given;
- the position of each order counts, because the engine fills orders in their
  listed order;
- it ignores `reasoning` and each order's `rationale`, which are audit text the
  scorer never reads;
- it ignores `cost`, which reports spend rather than a choice. Token counts can
  vary between samples that choose the same orders.

## The rate

The headline rate is pairwise disagreement. A compared group of `n` replicates
holds `n(n-1)/2` replicate pairs, and a pair differs when its two decisions
differ. The rate is the number of differing pairs over the number of pairs,
summed over every compared group.

Suppose replicates choose independently, picking option `j` with probability
`p_j`. A pair then differs with probability `1 - sum p_j^2`, whatever the size
of its group, so the rate does not move with how many captures you supply or
how long histories stay shared. A share of differing groups would move: for a
binary choice with flip probability `q`, a group of `n` differs with probability
`1 - (1 - q)^n - q^n`, which at `q = 0.25` climbs from 0.375 at `n = 2` to 0.822
at `n = 6`. The report gives the group counts and the distribution of group
sizes as context and does not divide them.

The test suite pins both facts on exact outcome fields: at `q = 1/4` the
pairwise rate is exactly `3/8` for every group size from 2 to 6 while the group
share takes the values above, and for three options with probabilities
`1/2, 1/4, 1/4` the rate is exactly `5/8` for group sizes 2 to 5.

## The report

| Field | Meaning |
|---|---|
| `pairwise_disagreement` | `{"status":"available","value":...}` (`differing_pairs / pairs_compared`), or `{"status":"unavailable","reason":...}` |
| `pairs_compared`, `differing_pairs` | replicate pairs in compared groups, and those whose decisions differ |
| `groups_compared`, `groups_with_differing_decisions`, `group_sizes` | context: compared groups, those holding more than one decision, and compared groups by size |
| `steps_total` | every recorded step of every replicate |
| `steps_compared` | steps inside a compared group |
| `steps_excluded_diverged_observation` | steps after a replicate's observation stopped matching any other member's |
| `steps_excluded_diverged_decision` | steps after a replicate made a decision no other member made |
| `steps_unreplicated` | steps of a window that has a single replicate |
| `identical_replicate_runs` | runs byte-identical to an earlier run of the same window |
| `identical_replicates_declared` | whether `--declare-identical-replicates` was passed |
| `inputs[]`, `identical_inputs` | each input trajectory's SHA-256 over its compact JSON and its run count, and the inputs whose digest repeats an earlier one |
| `dataset_sha256`, `cost_model_sha256`, `engine_version`, `runner_artifact_sha256` | what every input was verified against |
| `windows[]` | the same counts per window, with `replicates` and every differing group (`step`, `observation_sha256`, `replicates`, `distinct_decisions`, `differing_pairs`) |

The top-level counts are sums over the windows. For a window with two or more
replicates, `steps_compared` plus the two excluded counts equals `steps_total`;
for a window with one, `steps_unreplicated` does.

The rate is typed unavailable rather than reported as zero when there is
nothing to compare: `single_replicate` when no window has two replicates, and
`no_matched_observation` when replicates exist but no step's history was shared
by two of them. The report also states the rules it applied, under
`observation_digest`, `grouping`, `decision_difference`, `rate` and
`sampling_unit`.

## Sampling unit and uncertainty

The independent unit is the replicate run; `windows[].replicates` counts them.
Groups at a later step are subsets of earlier groups and reuse the same runs,
so neither `groups_compared` nor `pairs_compared` is a sample size.

The rate carries no interval. A bootstrap over replicate runs would draw the
same run more than once, and a run always agrees with its own copy, which is
the bias the identical-replicate check exists to refuse. With the few
replicates per window a field usually provides (the 8 execution seeds of one
`capture`, or a few repeated captures), a percentile interval would also be
coarse. Read the rate
beside `windows[].replicates` and the excluded counts.

## Guarantee boundary

A zero rate says that no compared pair disagreed. It says nothing about the
steps that were excluded. The rate measures consistency, not quality: a stable
agent can be wrong every time.

The command reads SharpeBench trajectory files. SharpeArena's local model field
records its own per-step observation hashes and a repetition axis in its
journals, but those journals are not an input to this command.
