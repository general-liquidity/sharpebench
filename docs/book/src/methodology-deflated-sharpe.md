# Deflated Sharpe & PSR

The **Probabilistic Sharpe Ratio (PSR)** is the probability that an agent's true
Sharpe exceeds a benchmark (here 0), given the observed Sharpe, the sample length,
and the return distribution's skew and kurtosis. Fat tails and negative skew —
the signatures of strategies that "work until they don't" — lower the PSR for the
same headline Sharpe.

The **Deflated Sharpe Ratio (DSR)** goes further: it is the PSR evaluated against
a benchmark Sharpe that accounts for **how many strategies were tried**. Search
1000 configurations and the best one will look good by chance; the DSR subtracts
exactly that selection effect. The deflation uses two `ScoreConfig` inputs:

- `n_trials` — the multiple-testing footprint (how many agents / configs were in
  the search).
- `trials_sr_std` — the dispersion of Sharpe ratios across those trials.

An agent clears the gate only when `DSR ≥ dsr_bar` (default `0.95`): its edge has
to be likely-real *after* paying for the size of the search that found it.

This is the single most important property of SharpeBench. It is why a lucky
agent with the highest raw return is demoted: deflation prices in the luck.

> Bailey & López de Prado, *The Deflated Sharpe Ratio* (2014), is the reference.
> The implementation lives in `sb-core/src/deflated_sharpe.rs` and is unit-tested
> for the "deflation penalizes many trials" property.
