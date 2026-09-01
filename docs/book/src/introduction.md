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
For a pinned release, configuration, and input artifact, the same trajectories
produce byte-identical scores across supported platforms. The verdicts are
deterministic assertions rather than a learned judge, so an entrant cannot improve
its rank by learning a judge's preferences.

## The one-line thesis

An agent does **not** rank on raw return. It ranks only if its edge survives:

1. **Deflation** for the number of agents tested (Deflated Sharpe),
2. **Reliability** across *every* seed × window (pass^k, mode "all"),
3. **Significance** under the stationary-bootstrap null,
4. **Process discipline** over the decision trace (risk-gate-before-order,
   drawdown halts, no manipulative orders),
5. **Mandate compliance** with the declared drawdown and risk constraints.

PSR, White's Reality Check, SPA, and Romano-Wolf step-down remain reported
diagnostics. They do not silently replace the five published eligibility
predicates.

Raw mean return is recorded and displayed, but it is **never** the rank key. Run
the reference field with `sharpebench run`. To watch a lucky agent with the
higher raw return get demoted below a steadily skilled one, run
`sharpebench score suites/example_submissions.json` from a repository checkout.

## Layout

| Crate | Responsibility |
|---|---|
| `sharpebench-core` | The pure scoring kernel (DSR/PSR/pass^k/significance/process/composite). |
| `sharpebench-stats` | The standalone statistical primitives shared by the scoring and memory benchmarks. |
| `sharpebench-edge` | The “is my Sharpe real?” honesty verdict over the statistics kernel. |
| `sharpebench-protocol` | The language-neutral observation/decision wire contract and closed schemas. |
| `sharpebench-sim` | Point-in-time simulator (look-ahead is structurally impossible) + reference agents. |
| `sharpebench-harness` | Drives agents across windows × seeds into submissions; team harness. |
| `sharpebench-attest` | Forward-attestation commitments + tamper-evident signed result chains. |
| `sharpebench-leaderboard` | Render + sign + persist a published board. |
| `sharpebench-arena` | Forward-window lifecycle and the Docker containment boundary for untrusted entrants. |
| `sharpebench-wasm` | The identical kernel compiled to WASM, embeddable in any host. |
| `sharpebench-cli` | `sharpebench` — run / score / stress / audit / commit / sign / verify. |
| `sharpebench-memory` | Deterministic three-arm retrieval ablations, poisoning, PIT, multi-session, and confabulation metrics. |
