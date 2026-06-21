# Benchmark integrity

The research on agent-eval integrity (BenchJack; Berkeley RDI's survey finding
eight major benchmarks gameable) shows that benchmarks with an LLM judge, or with
a single tunable target, get gamed. SharpeBench is **judge-free and
deterministic**, so its resistance to gaming is something you can *run*, not just
claim.

## The self-audit

`sharpebench audit` fires a battery of known attacks at the live scorer and checks
each is demoted:

```text
[DEFENDED] luck-not-skill             win on one lucky seed with the highest raw return
[DEFENDED] risk-gate-bypass           place an order that skipped the risk gate
[DEFENDED] sim-exploitation           submit a manipulative / absurd-size order
[DEFENDED] mandate-breach             exceed the drawdown mandate to chase return
[DEFENDED] raw-return-cannot-buy-rank post the biggest raw return on only some runs
```

The command exits non-zero if **any** attack is not demoted. That makes the audit
a regression gate: a future change that silently weakens a gate (say, relaxing the
process check, or making raw return leak into the rank key) fails the audit in CI
instead of shipping unnoticed. The same battery runs as a unit test
(`benchmark_resists_every_known_attack`).

## Why determinism matters here

Because `sb-core` is pure — no clock, no ambient RNG, fixed float reduction order —
the audit's verdict is a property of the code, not of the machine or the run.
Anyone can reproduce it byte-for-byte. A benchmark whose integrity proof is
reproducible cannot be quietly degraded, and a leaderboard whose scorer is open and
deterministic cannot favour the host. That is the whole design: **verify, don't
trust.**
