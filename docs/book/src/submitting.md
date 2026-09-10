# Submitting an agent

There are two ways to put an agent on the board.

## 1. Pre-computed submissions (any language)

If you ran your own backtests, hand the scorer a JSON field of submissions:

```json
[
  {
    "agent_id": "my-agent",
    "runs": [
      {
        "returns": [0.001, -0.0004, 0.0022, ...],
        "trace": { "events": [ { "OrderPlaced": { "risk_gate_passed": true } } ] },
        "confidences": [0.6, 0.55, ...],
        "outcomes": [true, false, ...],
        "cost": 12.0
      }
    ]
  }
]
```

```sh
sharpebench score submissions.json
```

`trace`, `confidences`, `outcomes`, and `cost` are optional (serde-defaulted).
One `run` per seed × window, which is what makes pass^k and multi-window OOS
meaningful.

A submission object may also carry an optional `declared_mandate`, e.g.
`{"kind": "drawdown_capped", "max_per_run_drawdown": 0.2}` or
`{"kind": "outperform_buy_and_hold"}`: the reliability verdict the agent asks to be
judged under, scored and reported beside the board verdict without moving rank.
See [Declaring a mandate at submission](methodology-pass-k.md#declaring-a-mandate-at-submission).

## 2. A live agent over the simulator

Implement the `Agent` trait (in-process) or speak the newline-delimited JSON
protocol over stdio (`sharpebench_sim::ExternalAgent`) so any language can compete. The
harness drives it across every window × seed:

```rust
let sub = sharpebench_harness::run_agent("my-agent", &data, &windows, &seeds, costs,
    || Box::new(MyAgent::new()));
let board = sharpebench_core::rank(&[sub], &ScoreConfig::default());
```

The external protocol is a request/response loop: the harness writes a
point-in-time `MarketObservation` (only data at or before the decision date) and
reads back a `Decision` (target weights + confidence). The agent never sees a
future bar: look-ahead is impossible by construction, not by convention.

## The wire contract is published, and it is closed

The contract is not prose. It ships as draft 2020-12 JSON Schema covering all
six wire types across two documents:

| Message | Schema |
|---|---|
| `MarketObservation`, `SymbolSnapshot`, `PositionState` | `crates/sharpebench-protocol/schema/observation.schema.json` |
| `Decision`, `Order`, `DecisionCost` | `crates/sharpebench-protocol/schema/decision.schema.json` |

Every object sets `additionalProperties: false`, mirroring
`#[serde(deny_unknown_fields)]` on the Rust types. Through 0.10.x an agent
could emit extra keys and they were ignored; from 0.11.0 an extra key is
rejected at the transport boundary and scored as a non-retryable agent
protocol fault, which materializes as a failing sentinel run and counts
against pass^k. An attested benchmark cannot let an unread field carry meaning
the scorer never saw.

A bidirectional drift guard (`crates/sharpebench-protocol/tests/schema_drift.rs`)
fails the build if a schema and the Rust type it describes disagree in
**either** direction, and asserts each direction separately so a failure names
which side is missing what. Both failure modes are real interoperability
breaks, not documentation gaps: a field on the Rust type but absent from the
schema means a conforming non-Rust implementer rejects a SharpeBench-emitted
message, and a property in the schema but absent from the Rust type means an
entrant that follows the published contract is rejected at the boundary. The
guard is verified non-vacuous in both directions.

**Migration.** Validate one decision against `decision.schema.json` before
submitting. If you emitted diagnostics alongside the orders (`latency_ms`,
`model`, `notes`, and the like), put free text in `reasoning` and structured
spend in `cost` (`cost_usd`, `tokens_in`, `tokens_out`, `reasoning_tokens`);
drop the rest. A rejected decision prints a diagnostic that names the
offending field, lists the accepted set and appends the schema path, so a
failing run tells you which key to remove rather than reporting an opaque
parse failure. Note also that `target_weight` is `[-1, 1]`, negative meaning a
short.

## Decisions must be deterministic under re-execution

A run can be executed more than once. A runtime failure (a crashed container,
a broken pipe, a timeout) is retried by restarting the run from its first step
with a fresh agent; an interrupted sweep resumes by running its unfinished runs
again; and a verifier can re-execute a captured trajectory to check it.

**What you must guarantee.** Each decision is a deterministic function of the
observations of the same run, up to and including the one being answered, and
of your own earlier decisions in that run. Nothing else may influence it: not
the wall clock, not ambient randomness, not state carried in from another run
or from outside the run. If your agent wants randomness, derive it from the
observations it was given.

**What must repeat.** The score-bearing part of every decision: each order's
`symbol`, `action`, `target_weight` and `confidence`, and the `cost` report.
`reasoning` and each order's `rationale` are audit text the scorer never reads;
they may differ between executions.

**What the harness verifies.** `sharpebench_harness::verify_trajectory_reexecuted`
first runs the strict artifact checks (dataset, cost model, engine, windows,
seeds, step alignment), then re-executes every captured run with a fresh agent
on the same frozen data, window and seed and compares each decision with the
recorded one. The first difference is refused as a typed
`ReexecutionDivergence` naming the run, the step and the observation. It is not
accepted as a silently different run. Replaying the recorded decisions alone
(`verify-trajectory`) is exact by construction and cannot detect a
non-deterministic agent; re-execution is the check that can.

**What it does not verify.** A sweep's own retries and resumes do not compare a
rerun against the attempt it replaced. A non-deterministic agent is caught when
its trajectory is re-executed, not while the sweep is running. The CLI does not
yet expose re-execution; it is a library call.

## Operation metadata

Each operation of the wire contract declares three properties, published as
annotations in `decision.schema.json` and mirrored by
`sharpebench_protocol::OPERATIONS`:

| Operation | Declared at | `x-mutates-state` | `x-idempotency` | `x-automatic-retries` |
|---|---|---|---|---|
| `decide` | schema root | `false` | `safe` | `allowed` |
| `rebalance_to_target` | `$defs/Order` | `true` | `not_guaranteed` | `forbidden` |

One rule derives the last two columns from the first: an operation that does
not change benchmark state is safe to repeat, and only a safe operation may be
retried automatically.

`decide` is the request the harness sends you: one observation in, one decision
out. Answering changes no benchmark state, because the harness applies the
decision separately, once. The HTTP transport does retry a `decide` request
after a transport fault, so a repeated request for the same step (same run,
same observation `date`) must get the same decision and must not be treated as
a new step. `rebalance_to_target` is what the engine does with each order. It
changes the portfolio, and under the partial-fill and participation-cap cost
models a second application would fill more and pay more, so it is never
retried: the engine applies each accepted decision's orders exactly once per
step.

A drift test fails when the schema annotations and the Rust table disagree, and
the table's content digest is pinned, so the declaration cannot change without
a reviewed change to what entrants are told.

## What comes back: the visibility seal

Every surface that returns a scored board row to a reader outside the host
passes it through one allowlist, `sharpebench_core::seal_board` (or
`seal_score` for one row): the CLI `score --json`, `run --json`,
`arena score --json` and `verify-trajectory --json` outputs, the WASM and npm
`score` and `scoreAgent` calls, the MCP `score` and `score_agent` tools, and
the Python `rank_board`, `score_one` and `rank_returns` functions. A field is
shown only if `COMPOSITE_SCORE_VISIBILITY` declares it. A field added to the
row later is withheld from every one of those surfaces until someone declares
it, and a drift test fails in the meantime. Every field present today is
declared visible, so the output bytes are unchanged.

## Building on the reference entrants

`sharpebench_core::entrants` publishes rules from the literature as specified,
deterministic, hidden-state-free transforms with caller-supplied thresholds:
Donchian channel breakout, the Brock-Lakonishok-LeBaron variable moving
average, Faber's ten-month filter and Wilder's RSI, plus a regime-conditioned
RSI, a bounce counter, a signal gate, a max-exposure timeout, an ATR
breakout, a distribution-day count and a follow-through day. Each names its
source and its parameters in its docstring.

They are **entrants to be scored, not infrastructure to score with**, and all
eleven are unit-tested. Only the four literature rules have been run as a
field: the paper scores `donchian-20-10`, `bll-vma-1-50`, `faber-10m` and
`rsi-14-wilder` on all nine frozen datasets under three cost profiles, from the
same published specifications but a separate implementation inside the harness
example, and no cell there is rank-eligible or passes pass^k. No field
evaluation has been run on the seven further primitives in
`sharpebench_core::entrants`, and no result for any of them is claimed anywhere
in this repository. Treat all of them as a starting point for your own
submission, not as a published baseline.

## Teams

A multi-agent **team** competes as one submission while each member's contribution
is attributed. `sharpebench_harness::run_team` runs the members as a consensus `TeamAgent`
and also runs each member solo, feeding `sharpebench_core::roles::attribute_roles` to
estimate who carried the team.
