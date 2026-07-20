"""SharpeBench: honest-backtest statistics for your own return series.

One question, answered deterministically: *is this Sharpe real, or an artifact of
luck and multiple testing?* Everything here takes plain numeric sequences (lists,
tuples, 1-D/2-D numpy arrays, ``df["ret"].to_numpy()``) and returns plain floats,
lists and dicts.

    >>> from sharpebench import is_my_sharpe_real
    >>> returns = [0.001 + 0.0001 * ((i % 5) - 2) for i in range(500)]
    >>> v = is_my_sharpe_real(returns, n_trials=200)
    >>> v["verdict"] in {"pass", "borderline", "fail"}
    True

The whole surface is a pyo3 binding over the same Rust kernel the SharpeBench CLI
and the ``@general-liquidity/sharpebench`` npm package use, so a number computed
here is byte-identical to the one the benchmark reports.

Scope: this package scores *arbitrary* return series. It does not run agents. The
sibling ``sharpearena`` package hosts the leak-free RL environment and scores runs
of it via ``score_run``; the two are complementary, since arena produces the track,
sharpebench judges whether the track means anything.
"""

from .sharpebench_py import (
    METHODOLOGY_VERSION,
    benjamini_hochberg,
    bootstrap_dsr_ci,
    bootstrap_pvalue,
    budget_curve,
    deflated_sharpe_ratio,
    expected_max_sharpe,
    fdr_verdict,
    hlz_gate,
    is_my_sharpe_real,
    is_my_sharpe_real_full,
    min_track_record_length,
    moments,
    pass_k,
    probability_of_backtest_overfitting,
    probabilistic_sharpe_ratio,
    reality_check_pvalue,
    runs_for_power,
    selection_robustness,
    sharpe_ratio,
    spa_consistent_pvalue,
    spa_pvalue,
    step_down_significant,
)

__all__ = [
    "METHODOLOGY_VERSION",
    "benjamini_hochberg",
    "bootstrap_dsr_ci",
    "bootstrap_pvalue",
    "budget_curve",
    "deflated_sharpe_ratio",
    "expected_max_sharpe",
    "fdr_verdict",
    "hlz_gate",
    "is_my_sharpe_real",
    "is_my_sharpe_real_full",
    "min_track_record_length",
    "moments",
    "pass_k",
    "probabilistic_sharpe_ratio",
    "probability_of_backtest_overfitting",
    "reality_check_pvalue",
    "runs_for_power",
    "selection_robustness",
    "sharpe_ratio",
    "spa_consistent_pvalue",
    "spa_pvalue",
    "step_down_significant",
]
