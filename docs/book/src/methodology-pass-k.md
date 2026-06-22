# pass^k reliability

A single backtest is a coin flip you can re-toss until it lands well. SharpeBench
runs every agent across many seeds and many windows, and asks whether it clears
the per-run bar on **all** of them.

`pass^k` (after Sierra's τ²-bench reliability metric) aggregates `k` runs:

- **mode `All`** — the agent must pass on *every* run. This is the mode used for
  the eligibility gate, because a money agent that is safe-on-average is not safe.
- **mode `Any`** / **`AtLeast(m)`** — available for reporting and for non-safety
  axes.

Each run passes if its individual Probabilistic Sharpe clears `per_run_psr_bar`
(default `0.90`). Because the simulator applies **seeded** execution noise, the
same strategy produces slightly different returns under different execution seeds
— so a one-seed fluke cannot top the board, and a genuine edge shows up
everywhere.

The headline consequence: an agent that earns a spectacular return on one run and
noise on the rest **fails pass^k** and is ineligible, no matter how high its
pooled raw return. See the `lucky_high_return_fails_pass_k` test in
`sharpebench-core/src/composite.rs`.
