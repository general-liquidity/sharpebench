# SharpeBench

**SharpeBench is a luck-robust, deflation-surviving benchmark for AI trading agents.**

Most trading benchmarks rank agents on raw or lightly risk-adjusted return over a
short window and a handful of runs. With Sharpe-ratio confidence intervals
routinely wider than the gaps between contestants, those rankings are
statistically indistinguishable from a luck contest. SharpeBench exists to answer
a sharper question:

> Is this agent's edge **real skill**, or the luckiest draw out of many tries?

It answers it without a judge. The scoring kernel is a pure, deterministic Rust
library (`sharpebench-core`): no I/O, no system clock, no ambient randomness, no `unsafe`.
The same trajectories always produce byte-identical scores on any platform, so a
result is reproducible forever — and a benchmark whose verdicts are assertions
rather than opinions cannot be gamed by learning a judge's biases.

## The one-line thesis

An agent does **not** rank on raw return. It ranks only if its edge survives:

1. **Deflation** for the number of agents tested (Deflated Sharpe / PSR),
2. **Reliability** across *every* seed × window (pass^k, mode "all"),
3. **Significance** that beats data-snooping (stationary bootstrap + White's
   Reality Check + Romano–Wolf step-down),
4. **Process discipline** over the decision trace (risk-gate-before-order,
   drawdown halts, no manipulative orders).

Raw mean return is recorded and displayed, but it is **never** the rank key. Run
the reference agents (`sharpebench run`) to watch a lucky agent with the higher
raw return get demoted below a steadily-skilled one.

## Layout

| Crate | Responsibility |
|---|---|
| `sharpebench-core` | The pure scoring kernel (DSR/PSR/pass^k/significance/process/composite). |
| `sharpebench-sim` | Point-in-time simulator (look-ahead is structurally impossible) + reference agents. |
| `sharpebench-harness` | Drives agents across windows × seeds into submissions; team harness. |
| `sharpebench-attest` | Forward-attestation commitments + tamper-evident signed result chains. |
| `sharpebench-leaderboard` | Render + sign + persist a published board. |
| `sharpebench-wasm` | The identical kernel compiled to WASM, embeddable in any host. |
| `sharpebench-cli` | `sharpebench` — run / score / stress / audit / commit / sign / verify. |
