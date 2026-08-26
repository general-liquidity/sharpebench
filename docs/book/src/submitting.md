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

## Building on the reference entrants

`sharpebench_core::entrants` publishes rules from the literature as specified,
deterministic, hidden-state-free transforms with caller-supplied thresholds:
Donchian channel breakout, the Brock-Lakonishok-LeBaron variable moving
average, Faber's ten-month filter and Wilder's RSI, plus a regime-conditioned
RSI, a bounce counter, a signal gate, a max-exposure timeout, an ATR
breakout, a distribution-day count and a follow-through day. Each names its
source and its parameters in its docstring.

They are **entrants to be scored, not infrastructure to score with**. They are
unit-tested; no field evaluation has been run on any of them, and no result
for any of them is claimed anywhere in this repository. Treat them as a
starting point for your own submission, not as a published baseline.

## Teams

A multi-agent **team** competes as one submission while each member's contribution
is attributed. `sharpebench_harness::run_team` runs the members as a consensus `TeamAgent`
and also runs each member solo, feeding `sharpebench_core::roles::attribute_roles` to
estimate who carried the team.
