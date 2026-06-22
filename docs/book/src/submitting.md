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
One `run` per seed × window — that is what makes pass^k and multi-window OOS
meaningful.

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
future bar — look-ahead is impossible by construction, not by convention.

## Teams

A multi-agent **team** competes as one submission while each member's contribution
is attributed. `sharpebench_harness::run_team` runs the members as a consensus `TeamAgent`
and also runs each member solo, feeding `sharpebench_core::roles::attribute_roles` to
estimate who carried the team.
