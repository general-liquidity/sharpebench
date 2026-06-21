# Significance & multiple testing

Three complementary tests guard against an edge that is really noise — and against
the leader being merely the luckiest of a large field. All are deterministic given
a seed (a seeded SplitMix64 drives a stationary bootstrap), so the p-values are
reproducible.

## 1. Per-agent stationary-bootstrap p-value

`bootstrap_pvalue` asks: under the null "true mean ≤ 0", how often does a
block-resampled version of the agent's excess returns produce an average as large
as the one observed? Block resampling (expected block length `1/block_prob`)
preserves serial correlation, so autocorrelated returns don't fool the test. An
agent must beat `alpha` (default `0.05`) to be eligible.

## 2. White's Reality Check (field-wide)

`reality_check_pvalue` is a data-snooping test: the probability that the **best**
agent's outperformance over the field benchmark arose by chance, *given how many
agents were tried*. A shared bootstrap index path across all agents preserves
cross-agent correlation. Low p ⇒ the field leader's edge is real, not the luckiest
draw. This value is reported on every row of the board.

## 3. Romano–Wolf step-down (per-agent, family-wise)

`step_down_significant` returns a per-agent verdict that controls the family-wise
error rate across the whole field, but is more powerful than the single-step
Reality Check: after confirming the strongest winners, it re-tests the survivors
against the maximum statistic over the *remaining* agents only. The result is a
boolean per agent — "this agent's outperformance survives correction for every
agent tested" — reported as `step_down_significant`.

Together: the bootstrap gate stops a single noisy agent ranking; the Reality Check
stops the field leader being luck; step-down hands every contestant an honest,
multiplicity-corrected significance verdict.
