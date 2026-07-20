"""Tests for the pyo3 statistics binding.

These exercise the real native kernel, not a Python re-implementation, and assert
the properties that make the package worth shipping: deflation is monotone in the
trial count, a pure-noise track does not pass, and the bootstrap interval brackets
the point estimate it is an interval for.
"""

import math
import random

import pytest

sharpebench = pytest.importorskip("sharpebench")

from sharpebench import (  # noqa: E402
    METHODOLOGY_VERSION,
    benjamini_hochberg,
    bootstrap_dsr_ci,
    bootstrap_pvalue,
    deflated_sharpe_ratio,
    expected_max_sharpe,
    fdr_verdict,
    hlz_gate,
    is_my_sharpe_real,
    is_my_sharpe_real_full,
    min_track_record_length,
    moments,
    pass_k,
    probabilistic_sharpe_ratio,
    probability_of_backtest_overfitting,
    reality_check_pvalue,
    runs_for_power,
    selection_robustness,
    sharpe_ratio,
    spa_consistent_pvalue,
    spa_pvalue,
    step_down_significant,
)

N = 500


def edge_track(n: int = N, drift: float = 0.001) -> list:
    """A steady, low-vol positive-drift track: a strong (deterministic) edge."""
    return [drift + 0.0001 * ((i % 5) - 2) for i in range(n)]


def noise_track(seed: int, n: int = N, vol: float = 0.01) -> list:
    """Pure noise: zero true mean, so any Sharpe on it is luck."""
    rng = random.Random(seed)
    return [rng.gauss(0.0, vol) for _ in range(n)]


def noise_field(k: int, n: int = 240) -> list:
    return [noise_track(1000 + i, n) for i in range(k)]


# --------------------------------------------------------------------------- moments


def test_moments_and_sharpe():
    m = moments(edge_track())
    assert m["n_obs"] == N
    assert m["mean"] > 0.0
    assert m["std_dev"] > 0.0
    assert math.isfinite(m["skew"])
    assert math.isfinite(m["kurtosis"])
    assert sharpe_ratio(edge_track()) > 0.0
    assert sharpe_ratio([]) == 0.0


def test_moments_accepts_any_sequence():
    xs = edge_track(64)
    assert sharpe_ratio(tuple(xs)) == pytest.approx(sharpe_ratio(xs))


def test_moments_numpy_roundtrip():
    np = pytest.importorskip("numpy")
    xs = edge_track(120)
    assert sharpe_ratio(np.asarray(xs)) == pytest.approx(sharpe_ratio(xs))


def test_sortino_none_without_downside():
    """Sortino is undefined when nothing falls below target, so it is reported as None."""
    assert moments([0.01] * 32, 0.0)["sortino"] is None


# ------------------------------------------------------------------- deflated Sharpe


def test_psr_and_dsr_are_probabilities():
    xs = edge_track()
    psr = probabilistic_sharpe_ratio(xs)
    dsr = deflated_sharpe_ratio(xs, 200)
    assert 0.0 <= psr <= 1.0
    assert 0.0 <= dsr <= 1.0
    assert dsr <= psr  # deflating for the search never raises the probability


def test_deflation_is_monotone_in_trial_count():
    """The headline property: more trials searched, less believable the winner."""
    xs = edge_track(120, drift=0.0004)
    dsrs = [deflated_sharpe_ratio(xs, k) for k in (1, 10, 100, 1000, 10_000)]
    assert all(a >= b for a, b in zip(dsrs, dsrs[1:])), dsrs
    assert dsrs[0] > dsrs[-1]


def test_expected_max_sharpe_grows_with_trials():
    assert expected_max_sharpe(0.5, 1000) > expected_max_sharpe(0.5, 10)
    assert expected_max_sharpe(0.0, 1000) == 0.0


def test_min_track_record_length():
    assert min_track_record_length(edge_track()) > 0.0
    # A track that does not beat the benchmark can never clear it.
    assert math.isinf(min_track_record_length(noise_track(3), sr_benchmark=5.0))


# ------------------------------------------------------------------------- verdicts


def test_lite_verdict_on_a_real_edge():
    v = is_my_sharpe_real(edge_track(), n_trials=20)
    assert v["verdict"] == "pass"
    assert v["sharpe"] > 0.0
    assert v["haircut"] == pytest.approx(1.0 - v["deflated_sharpe"])
    assert v["haircut_sharpe"] == pytest.approx(v["sharpe"] * v["deflated_sharpe"])
    assert v["methodology_version"] == METHODOLOGY_VERSION
    assert v["explanation"]


def test_pure_noise_does_not_pass():
    """The property the package exists for: luck must not be certified as skill."""
    for seed in range(6):
        v = is_my_sharpe_real(noise_track(seed), n_trials=500)
        assert v["verdict"] != "pass", (seed, v["deflated_sharpe"])


def test_lite_verdict_trial_count_can_flip_a_pass_to_a_fail():
    xs = edge_track(120, drift=0.00035)
    assert is_my_sharpe_real(xs, n_trials=1)["verdict"] != "fail"
    assert is_my_sharpe_real(xs, n_trials=1_000_000)["verdict"] == "fail"


def test_full_verdict_over_a_noise_field():
    field = noise_field(8)
    full = is_my_sharpe_real_full(field, n_trials=8)
    assert 0 <= full["winner_idx"] < len(field)
    # winner_idx defaults to the highest-Sharpe row.
    assert full["winner_idx"] == max(range(len(field)), key=lambda i: sharpe_ratio(field[i]))
    assert full["honesty"]["verdict"] != "pass"
    for key in ("reality_check_p", "spa_p", "spa_consistent_p", "pbo"):
        assert 0.0 <= full[key] <= 1.0
    assert len(full["step_down"]) == len(field)
    assert set(full["hlz"]) == {"t_stat", "t_threshold", "passed", "explanation"}


def test_full_verdict_rejects_a_bad_winner_index():
    with pytest.raises(ValueError):
        is_my_sharpe_real_full(noise_field(3), winner_idx=99)
    with pytest.raises(ValueError):
        is_my_sharpe_real_full([])


# ------------------------------------------------------------------- bootstrap CIs


def test_dsr_ci_brackets_the_point_estimate():
    xs = edge_track(200, drift=0.0005)
    ci = bootstrap_dsr_ci(xs, n_trials=100, n_boot=400)
    assert ci["point"] == pytest.approx(deflated_sharpe_ratio(xs, 100))
    assert ci["lower"] <= ci["point"] <= ci["upper"]
    assert ci["se"] >= 0.0


def test_dsr_ci_is_deterministic_given_the_seed():
    xs = noise_track(11, 200)
    a = bootstrap_dsr_ci(xs, n_trials=10, n_boot=200, seed=7)
    b = bootstrap_dsr_ci(xs, n_trials=10, n_boot=200, seed=7)
    assert a == b
    assert bootstrap_dsr_ci(xs, n_trials=10, n_boot=200, seed=8) != a


def test_dsr_ci_degenerate_track_is_zero_width():
    ci = bootstrap_dsr_ci([0.01], n_trials=1)
    assert ci["lower"] == ci["point"] == ci["upper"]
    assert ci["se"] == 0.0


def test_bootstrap_pvalue_separates_edge_from_noise():
    assert bootstrap_pvalue(edge_track(240), n_boot=400) < 0.05
    assert bootstrap_pvalue(noise_track(5, 240), n_boot=400) > 0.05


# --------------------------------------------------------------- data-snooping family


@pytest.mark.parametrize("fn", [reality_check_pvalue, spa_pvalue, spa_consistent_pvalue])
def test_snooping_pvalues_do_not_reject_a_noise_field(fn):
    p = fn(noise_field(10), n_boot=400)
    assert 0.0 <= p <= 1.0
    assert p > 0.05


def test_snooping_pvalues_reject_a_field_containing_a_real_edge():
    field = noise_field(6, 240) + [edge_track(240, drift=0.004)]
    assert reality_check_pvalue(field, n_boot=400) < 0.05
    assert spa_pvalue(field, n_boot=400) < 0.05


def test_step_down_flags_only_the_real_edge():
    field = noise_field(6, 240) + [edge_track(240, drift=0.004)]
    flags = step_down_significant(field, n_boot=400)
    assert len(flags) == len(field)
    assert flags[-1] is True
    assert not any(flags[:-1])


@pytest.mark.parametrize("fn", [reality_check_pvalue, spa_pvalue, spa_consistent_pvalue, step_down_significant])
def test_snooping_rejects_an_empty_field(fn):
    with pytest.raises(ValueError):
        fn([])


def test_pbo_on_pure_noise_is_not_confident():
    """T x N orientation (the transpose of the snooping field)."""
    field = noise_field(8, 240)
    perf = [[field[n][t] for n in range(len(field))] for t in range(240)]
    pbo = probability_of_backtest_overfitting(perf, 8)
    assert 0.0 <= pbo <= 1.0
    assert pbo > 0.2  # an in-sample winner picked out of noise does not generalize


def test_pbo_degenerate_inputs():
    assert probability_of_backtest_overfitting([], 8) == 0.0
    assert probability_of_backtest_overfitting([[0.1, 0.2]], 8) == 0.0


# -------------------------------------------------------------------------- BH-FDR


def test_benjamini_hochberg_and_verdict():
    p_values = [0.001, 0.008, 0.02, 0.4, 0.7, 0.9]
    rejected = benjamini_hochberg(p_values, 0.05)
    assert len(rejected) == len(p_values)
    assert rejected[0] is True
    assert rejected[-1] is False

    v = fdr_verdict(p_values, 0.05)
    assert v["q"] == 0.05
    assert v["n_tested"] == len(p_values)
    assert v["n_discoveries"] == sum(rejected)
    assert v["rejected"] == rejected
    assert v["threshold"] is not None


def test_fdr_verdict_with_no_discoveries():
    v = fdr_verdict([0.6, 0.7, 0.8], 0.05)
    assert v["n_discoveries"] == 0
    assert v["threshold"] is None


# ---------------------------------------------------------------------------- HLZ


def test_hlz_gate_bar_is_three_not_two():
    assert hlz_gate(2.5)["passed"] is False
    assert hlz_gate(3.4)["passed"] is True
    assert hlz_gate(-3.4)["passed"] is True  # two-sided
    assert hlz_gate(2.5, t_threshold=2.0)["passed"] is True
    assert hlz_gate(3.0)["t_threshold"] == 3.0
    assert "Harvey-Liu-Zhu" in hlz_gate(1.0)["explanation"]


# ------------------------------------------------------------- selection robustness


def test_selection_robustness_gap_is_large_when_the_winner_is_a_fluke():
    candidates = noise_field(9, 240) + [edge_track(240, drift=0.004)]
    r = selection_robustness(candidates, n_trials=10)
    assert r["n_candidates"] == len(candidates)
    assert r["best_dsr"] >= r["median_dsr"]
    assert r["selection_gap"] == pytest.approx(r["best_dsr"] - r["median_dsr"])
    assert r["selection_gap"] > 0.1


def test_selection_robustness_empty():
    r = selection_robustness([], n_trials=1)
    assert r["n_candidates"] == 0


# ------------------------------------------------------------------ power / pass^k


def test_runs_for_power_falls_as_the_effect_grows():
    assert runs_for_power(0.1) > runs_for_power(1.0)
    assert runs_for_power(0.0) > 0


def test_pass_k_modes():
    runs = [True, True, False, True]
    assert pass_k(runs) is False  # default mode="all" is the safety-grade bar
    assert pass_k(runs, "any") is True
    assert pass_k(runs, "at_least", 3) is True
    assert pass_k(runs, "at_least", 4) is False
    assert pass_k([True, True]) is True
    assert pass_k([]) is False


def test_pass_k_rejects_bad_modes():
    with pytest.raises(ValueError):
        pass_k([True], "most")
    with pytest.raises(ValueError):
        pass_k([True], "at_least")
