# Benchmark integrity

The research on agent-eval integrity (BenchJack; Berkeley RDI's survey finding
eight major benchmarks gameable) shows that benchmarks with an LLM judge, or with
a single tunable target, get gamed. SharpeBench is **judge-free and
deterministic**, so its resistance to gaming is something you can *run*, not just
claim.

## The self-audit

The live battery has nine cases, and all nine are claimed defenses that must be
demoted on every commit. The ninth, the sock-puppet Sybil field, was for a time
marked `expected_vulnerable`: on the measured-deflation path `rank` estimates
`trials_sr_std` from the field, and 200 near-duplicate low-dispersion puppets
shrank that estimate enough to lower the deflation bar and admit a borderline
agent. `rank` now collapses near-clone streams before it measures: pooled streams
whose `|cosine|` reaches `CLONE_COLLAPSE_COSINE` (0.995) are joined into
clusters, and each cluster votes once with its median Sharpe, both in the
dispersion estimate and in the field count the measurement floor is checked
against. Clones are still scored and still appear on the board; they just do not
vote on the bar. The constant is deliberately not the rediscovery screen's 0.97:
rediscovery flags a *similar strategy* for review, the collapse removes
*duplicate votes* and must never silence an honest agent that is merely
collinear with another. Honest agents are collinear: on the benchmark's own
evidence fields a long-only luck-floor agent sits at cosine 0.971 to 0.990
against buy-and-hold (largest honest pair 0.990 on weekly US indices), which
0.97 would have merged. The audit's 200 puppets sit at 0.99999 or above against
each other, the borderline agent at 0.934 against them. A harness test rebuilds
every committed evidence field and asserts zero merges at 0.995, so the
collapse is the identity on the shipped evidence. The case proves both halves:
with `dedup_clones_for_measured_sr_std: false` the exposure reproduces (the 200
puppets shrink the measured dispersion from 0.326 to 0.056 and lift the agent's
DSR from 0.000 to 0.952, admitting it), and with the default on the same field
measures 0.302, exactly what a field with one puppet measures, and the agent
stays refused. The cluster casts one dispersion vote, but all 207 visible
submissions still count toward the observable trial floor and remain on the
board.

`sharpebench audit` fires a battery of known attacks at the live scorer and checks
each is demoted:

```text
[DEFENDED] luck-not-skill             win on one lucky seed with the highest raw return
[DEFENDED] risk-gate-bypass           place an order that skipped the risk gate
[DEFENDED] sim-exploitation           submit a manipulative / absurd-size order
[DEFENDED] mandate-breach             exceed the drawdown mandate to chase return
[DEFENDED] raw-return-cannot-buy-rank post the biggest raw return on only some runs
[DEFENDED] cheat-reward-hacker        top raw return by bypassing the gate + padding confidence
[DEFENDED] tail-seller                smooth linear returns earned by selling tail risk (naked short gamma)
[DEFENDED] adversarial-input          look excellent in-sample with an accurate forecast head, then collapse under a small in-range input perturbation
[DEFENDED] sybil-sock-puppets         flood the field with near-duplicate agents to shrink measured trials_sr_std and lower the bar
```

The command exits non-zero if **any** attack is not demoted. That makes the audit
a regression gate: a future change that silently weakens a gate (say, relaxing the
process check, or making raw return leak into the rank key) fails the audit in CI
instead of shipping unnoticed. The same battery runs as a unit test
(`benchmark_resists_every_known_attack`).

## Declaring what the evidence digest covers

A signed record that excludes a field group without saying what it covers is
worse than an unsigned one, because a consumer cannot tell signed fields from
unsigned ones by inspection and will assume the signature covers everything.

`evidence_coverage` is the answer: a machine-readable inventory declaring, per
digest, which fields it binds and which are excluded **with a stated reason**.
A test fails when a new field is neither covered nor excluded, so the
inventory cannot silently fall behind the struct it describes. Secrets are
redacted before hashing, so verification never depends on holding secret
material.

## Agreement and dissent, without a judge

The audit above proves the scorer demotes known attacks. A different question
it could not previously ask is whether the automated gate agrees with the
**human** who triaged the gold set in the first place. Two deterministic
modules in `sharpebench-stats` answer it, with no model in either, which is
why they belong in a judge-free kernel:

- `agreement`: Cohen's kappa with the standard chance correction, and
  Spearman rho with tie handling. `gate_vs_human` rolls the pair up and
  `reproduces_triage(min_kappa)` turns it into a pass or fail.
- `dissent`: Kendall tau-b, tie-corrected in the denominator, splitting a
  disagreement into whether the **ranking** differs or the **levels** differ.
  A single agreement number collapses those two, which are different defects
  with different fixes.

Neither is judge-specific. The same split applies to two seeds, two windows,
or two scorer configurations, which is where it is most useful here.

## Why determinism matters here

Because `sharpebench-core` is pure (no clock, no ambient RNG, fixed float
reduction order), the audit's verdict is a property of the code, not of the machine or the run.
Anyone can reproduce it byte-for-byte. A benchmark whose integrity proof is
reproducible cannot be quietly degraded, and a leaderboard whose scorer is open and
deterministic cannot favour the host. That is the whole design: **verify, don't
trust.**
