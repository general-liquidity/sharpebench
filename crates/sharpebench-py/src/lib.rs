//! pyo3 bindings for SharpeBench's honest-backtest statistics.
//!
//! This binding answers one question for a pandas/numpy user: **"here is my return
//! series, is the Sharpe real or luck?"** It takes plain numeric sequences (any
//! Python sequence, including 1-D / 2-D numpy arrays) and returns plain floats,
//! lists and dicts, so nothing about the Rust kernel leaks into the caller.
//!
//! It exposes the same deterministic math as the CLI and the WASM kernel:
//! `sharpebench-stats` (deflated / probabilistic Sharpe, the data-snooping
//! bootstrap family, BH-FDR, selection robustness), `sharpebench-edge` (the
//! two-tier honesty verdict, MinTRL, CSCV PBO, the Harvey-Liu-Zhu gate) and
//! `sharpebench-core` (pass^k).
//!
//! `#![forbid(unsafe_code)]` is deliberately absent: pyo3's `#[pymodule]` /
//! `#[pyfunction]` macros expand to `unsafe` FFI glue. Every crate this binding
//! calls into does forbid it, so all of the statistics are unsafe-free.

use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;
use pyo3::types::PyDict;
use pyo3::wrap_pyfunction;

use sharpebench_core::budget_curve::{budget_curve as core_budget_curve, BudgetCurveOpts};
use sharpebench_core::pass_k::{pass_k as core_pass_k, PassMode};
use sharpebench_edge::{
    is_my_sharpe_real as core_lite, is_my_sharpe_real_full as core_full,
    min_track_record_length as core_mintrl, probability_of_backtest_overfitting as core_pbo,
    HarveyLiuZhu, HlzGate, HonestyConfig, HonestyVerdict, Verdict, METHODOLOGY_VERSION,
};
use sharpebench_stats::significance::{
    bootstrap_dsr_ci as core_dsr_ci, bootstrap_pvalue as core_bootstrap_pvalue,
    reality_check_pvalue as core_reality_check, runs_for_power as core_runs_for_power,
    spa_consistent_pvalue as core_spa_consistent, spa_pvalue as core_spa,
    step_down_significant as core_step_down,
};
use sharpebench_stats::stats::{
    downside_deviation as core_downside_deviation, kurtosis as core_kurtosis, mean as core_mean,
    skewness as core_skewness, sortino_ratio as core_sortino, std_dev as core_std_dev,
};
use sharpebench_stats::{
    benjamini_hochberg as core_bh, deflated_sharpe_ratio as core_dsr,
    expected_max_sharpe as core_expected_max, fdr_verdict as core_fdr_verdict,
    probabilistic_sharpe_ratio as core_psr, selection_robustness as core_selection,
    sharpe_ratio as core_sharpe,
};

/// The fixed bootstrap seed used when a caller does not pick one, so a result is
/// reproducible by default rather than silently run-dependent.
const DEFAULT_SEED: u64 = 0x5BA7_ED60_2026_0008;
/// The Lopez de Prado working assumption for cross-trial Sharpe dispersion.
const DEFAULT_TRIALS_SR_STD: f64 = 0.5;

fn require_non_empty(field: &[Vec<f64>], what: &str) -> PyResult<()> {
    if field.is_empty() {
        return Err(PyValueError::new_err(format!("{what} is empty")));
    }
    Ok(())
}

fn verdict_label(v: Verdict) -> &'static str {
    match v {
        Verdict::Pass => "pass",
        Verdict::Borderline => "borderline",
        Verdict::Fail => "fail",
    }
}

fn honesty_config(
    n_trials: u32,
    trials_sr_std: Option<f64>,
    confidence: f64,
    borderline: f64,
    sr_benchmark: f64,
) -> HonestyConfig {
    HonestyConfig {
        n_trials,
        trials_sr_std,
        confidence,
        borderline,
        sr_benchmark,
    }
}

fn honesty_dict<'py>(py: Python<'py>, v: &HonestyVerdict) -> PyResult<Bound<'py, PyDict>> {
    let d = PyDict::new(py);
    d.set_item("sharpe", v.sharpe)?;
    d.set_item("n_obs", v.n_obs)?;
    d.set_item("skew", v.skew)?;
    d.set_item("kurtosis", v.kurtosis)?;
    d.set_item("n_trials", v.n_trials)?;
    d.set_item("expected_max_sharpe", v.expected_max_sharpe)?;
    d.set_item("deflated_sharpe", v.deflated_sharpe)?;
    d.set_item("probabilistic_sharpe", v.probabilistic_sharpe)?;
    d.set_item("haircut", v.haircut)?;
    d.set_item("haircut_sharpe", v.haircut_sharpe)?;
    d.set_item("min_track_record_len", v.min_track_record_len)?;
    d.set_item("verdict", verdict_label(v.verdict))?;
    d.set_item("explanation", v.explanation.as_str())?;
    d.set_item("methodology_version", v.methodology_version.as_str())?;
    Ok(d)
}

fn hlz_dict<'py>(py: Python<'py>, g: &HlzGate) -> PyResult<Bound<'py, PyDict>> {
    let d = PyDict::new(py);
    d.set_item("t_stat", g.t_stat)?;
    d.set_item("t_threshold", g.t_threshold)?;
    d.set_item("passed", g.passed)?;
    d.set_item("explanation", g.explanation.as_str())?;
    Ok(d)
}

/// Observed (per-period, not annualized) Sharpe ratio of a return series.
#[pyfunction]
fn sharpe_ratio(returns: Vec<f64>) -> f64 {
    core_sharpe(&returns)
}

/// Probabilistic Sharpe Ratio: `P(true Sharpe > sr_benchmark)` given the observed
/// track's length, skew and kurtosis.
#[pyfunction]
#[pyo3(signature = (returns, sr_benchmark = 0.0))]
fn probabilistic_sharpe_ratio(returns: Vec<f64>, sr_benchmark: f64) -> f64 {
    core_psr(&returns, sr_benchmark)
}

/// The Sharpe you should expect the *best* of `n_trials` independent trials to
/// show under the null of zero true skill.
#[pyfunction]
#[pyo3(signature = (trials_sr_std, n_trials))]
fn expected_max_sharpe(trials_sr_std: f64, n_trials: u32) -> f64 {
    core_expected_max(trials_sr_std, n_trials)
}

/// Deflated Sharpe Ratio: the probability the edge survives the search that found
/// it. Rises with track length, falls as `n_trials` (the multiple-testing
/// footprint) grows. `n_trials = 1` is almost always a lie.
#[pyfunction]
#[pyo3(signature = (returns, n_trials, trials_sr_std = DEFAULT_TRIALS_SR_STD))]
fn deflated_sharpe_ratio(returns: Vec<f64>, n_trials: u32, trials_sr_std: f64) -> f64 {
    core_dsr(&returns, n_trials, trials_sr_std)
}

/// Minimum track record length (in periods) needed for the observed Sharpe to be
/// statistically above `sr_benchmark` at `confidence`. `inf` when the observed
/// Sharpe is already at or below the benchmark.
#[pyfunction]
#[pyo3(signature = (returns, sr_benchmark = 0.0, confidence = 0.95))]
fn min_track_record_length(returns: Vec<f64>, sr_benchmark: f64, confidence: f64) -> f64 {
    core_mintrl(&returns, sr_benchmark, confidence)
}

/// LITE honesty verdict from one return series. Returns a dict with the Sharpe,
/// PSR, expected-max-Sharpe, deflated Sharpe, haircut, MinTRL, a
/// `pass|borderline|fail` verdict and a plain-English explanation.
#[pyfunction]
#[pyo3(signature = (
    returns,
    n_trials = 1,
    trials_sr_std = None,
    confidence = 0.95,
    borderline = 0.90,
    sr_benchmark = 0.0,
))]
fn is_my_sharpe_real<'py>(
    py: Python<'py>,
    returns: Vec<f64>,
    n_trials: u32,
    trials_sr_std: Option<f64>,
    confidence: f64,
    borderline: f64,
    sr_benchmark: f64,
) -> PyResult<Bound<'py, PyDict>> {
    let cfg = honesty_config(
        n_trials,
        trials_sr_std,
        confidence,
        borderline,
        sr_benchmark,
    );
    honesty_dict(py, &core_lite(&returns, &cfg))
}

/// FULL honesty verdict over a whole field of candidates: the LITE verdict on the
/// winner plus White's Reality Check, Hansen's SPA (both variants), Romano-Wolf
/// step-down, the CSCV Probability of Backtest Overfitting and the Harvey-Liu-Zhu
/// factor gate.
///
/// `field` is **N rows (strategies) x T cols (time)**. `winner_idx` defaults to
/// the highest-Sharpe row, which is the candidate a search would actually keep.
#[pyfunction]
#[pyo3(signature = (
    field,
    winner_idx = None,
    n_trials = 1,
    trials_sr_std = None,
    confidence = 0.95,
    borderline = 0.90,
    sr_benchmark = 0.0,
))]
#[allow(clippy::too_many_arguments)]
fn is_my_sharpe_real_full<'py>(
    py: Python<'py>,
    field: Vec<Vec<f64>>,
    winner_idx: Option<usize>,
    n_trials: u32,
    trials_sr_std: Option<f64>,
    confidence: f64,
    borderline: f64,
    sr_benchmark: f64,
) -> PyResult<Bound<'py, PyDict>> {
    require_non_empty(&field, "field")?;
    let winner = match winner_idx {
        Some(i) if i >= field.len() => {
            return Err(PyValueError::new_err(format!(
                "winner_idx {i} out of range for a field of {} candidates",
                field.len()
            )))
        }
        Some(i) => i,
        None => field
            .iter()
            .enumerate()
            .max_by(|(_, a), (_, b)| {
                core_sharpe(a)
                    .partial_cmp(&core_sharpe(b))
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
            .map(|(i, _)| i)
            .unwrap_or(0),
    };
    let cfg = honesty_config(
        n_trials,
        trials_sr_std,
        confidence,
        borderline,
        sr_benchmark,
    );
    let full = core_full(&field, winner, &cfg);

    let d = PyDict::new(py);
    d.set_item("winner_idx", winner)?;
    d.set_item("honesty", honesty_dict(py, &full.honesty)?)?;
    d.set_item("reality_check_p", full.reality_check_p)?;
    d.set_item("spa_p", full.spa_p)?;
    d.set_item("spa_consistent_p", full.spa_consistent_p)?;
    d.set_item("step_down", full.step_down)?;
    d.set_item("pbo", full.pbo)?;
    d.set_item("hlz", hlz_dict(py, &full.hlz)?)?;
    Ok(d)
}

/// Percentile confidence interval and standard error for the Deflated Sharpe
/// Ratio via the stationary bootstrap. Returns
/// `{"point", "se", "lower", "upper"}`; deterministic given `seed`.
#[pyfunction]
#[pyo3(signature = (
    returns,
    n_trials,
    trials_sr_std = DEFAULT_TRIALS_SR_STD,
    seed = DEFAULT_SEED,
    n_boot = 1000,
    block_prob = 0.1,
    ci = 0.90,
))]
#[allow(clippy::too_many_arguments)]
fn bootstrap_dsr_ci<'py>(
    py: Python<'py>,
    returns: Vec<f64>,
    n_trials: u32,
    trials_sr_std: f64,
    seed: u64,
    n_boot: usize,
    block_prob: f64,
    ci: f64,
) -> PyResult<Bound<'py, PyDict>> {
    let c = core_dsr_ci(
        &returns,
        n_trials,
        trials_sr_std,
        seed,
        n_boot,
        block_prob,
        ci,
    );
    let d = PyDict::new(py);
    d.set_item("point", c.point)?;
    d.set_item("se", c.se)?;
    d.set_item("lower", c.lower)?;
    d.set_item("upper", c.upper)?;
    Ok(d)
}

/// Stationary-bootstrap p-value for a single excess-return series against the
/// null of zero mean excess return.
#[pyfunction]
#[pyo3(signature = (excess, seed = DEFAULT_SEED, n_boot = 2000, block_prob = 0.1))]
fn bootstrap_pvalue(excess: Vec<f64>, seed: u64, n_boot: usize, block_prob: f64) -> f64 {
    core_bootstrap_pvalue(&excess, seed, n_boot, block_prob)
}

/// White's Reality Check p-value over a field of candidates (**N rows x T cols**
/// of excess returns): the probability the best of them beats the benchmark by
/// luck alone.
#[pyfunction]
#[pyo3(signature = (field, seed = DEFAULT_SEED, n_boot = 2000, block_prob = 0.1))]
fn reality_check_pvalue(
    field: Vec<Vec<f64>>,
    seed: u64,
    n_boot: usize,
    block_prob: f64,
) -> PyResult<f64> {
    require_non_empty(&field, "field")?;
    Ok(core_reality_check(&field, seed, n_boot, block_prob))
}

/// Hansen's Superior Predictive Ability p-value (liberal / lower studentized
/// variant) over the same **N x T** field.
#[pyfunction]
#[pyo3(signature = (field, seed = DEFAULT_SEED, n_boot = 2000, block_prob = 0.1))]
fn spa_pvalue(field: Vec<Vec<f64>>, seed: u64, n_boot: usize, block_prob: f64) -> PyResult<f64> {
    require_non_empty(&field, "field")?;
    Ok(core_spa(&field, seed, n_boot, block_prob))
}

/// Hansen's consistent SPA p-value over the same **N x T** field.
#[pyfunction]
#[pyo3(signature = (field, seed = DEFAULT_SEED, n_boot = 2000, block_prob = 0.1))]
fn spa_consistent_pvalue(
    field: Vec<Vec<f64>>,
    seed: u64,
    n_boot: usize,
    block_prob: f64,
) -> PyResult<f64> {
    require_non_empty(&field, "field")?;
    Ok(core_spa_consistent(&field, seed, n_boot, block_prob))
}

/// Romano-Wolf step-down: per-candidate significance at `alpha` controlling the
/// family-wise error rate across the whole **N x T** field.
#[pyfunction]
#[pyo3(signature = (field, seed = DEFAULT_SEED, n_boot = 2000, block_prob = 0.1, alpha = 0.05))]
fn step_down_significant(
    field: Vec<Vec<f64>>,
    seed: u64,
    n_boot: usize,
    block_prob: f64,
    alpha: f64,
) -> PyResult<Vec<bool>> {
    require_non_empty(&field, "field")?;
    Ok(core_step_down(&field, seed, n_boot, block_prob, alpha))
}

/// CSCV Probability of Backtest Overfitting.
///
/// `perf_matrix` is **T rows (time) x N cols (strategies)**, the transpose of the
/// data-snooping `field` orientation. `s` is the (even) number of contiguous time
/// blocks. Near 0.5 means the in-sample winner is no better than chance
/// out-of-sample.
#[pyfunction]
#[pyo3(signature = (perf_matrix, s = 16))]
fn probability_of_backtest_overfitting(perf_matrix: Vec<Vec<f64>>, s: usize) -> f64 {
    core_pbo(&perf_matrix, s)
}

/// Benjamini-Hochberg step-up: which p-values are rejected at false-discovery
/// rate `q`. Returns one bool per input p-value, in input order.
#[pyfunction]
#[pyo3(signature = (p_values, q = 0.05))]
fn benjamini_hochberg(p_values: Vec<f64>, q: f64) -> Vec<bool> {
    core_bh(&p_values, q)
}

/// Benjamini-Hochberg with the summary an operator wants: `{"q", "n_tested",
/// "n_discoveries", "rejected", "threshold"}` (`threshold` is `None` when nothing
/// is rejected).
#[pyfunction]
#[pyo3(signature = (p_values, q = 0.05))]
fn fdr_verdict<'py>(py: Python<'py>, p_values: Vec<f64>, q: f64) -> PyResult<Bound<'py, PyDict>> {
    let v = core_fdr_verdict(&p_values, q);
    let d = PyDict::new(py);
    d.set_item("q", v.q)?;
    d.set_item("n_tested", v.n_tested)?;
    d.set_item("n_discoveries", v.n_discoveries)?;
    d.set_item("rejected", v.rejected)?;
    d.set_item("threshold", v.threshold)?;
    Ok(d)
}

/// The Harvey-Liu-Zhu (2016) factor gate: a claimed factor needs `|t| >= 3.0`,
/// not the conventional 2.0, once the multiple-testing history of factor research
/// is priced in.
#[pyfunction]
#[pyo3(signature = (t_stat, t_threshold = None))]
fn hlz_gate<'py>(
    py: Python<'py>,
    t_stat: f64,
    t_threshold: Option<f64>,
) -> PyResult<Bound<'py, PyDict>> {
    let gate = match t_threshold {
        Some(t) => HarveyLiuZhu::new(t),
        None => HarveyLiuZhu::default(),
    };
    hlz_dict(py, &gate.evaluate(t_stat))
}

/// Deflated-Sharpe spread across candidate return streams:
/// `{"n_candidates", "best_dsr", "median_dsr", "selection_gap"}`. A large
/// `selection_gap` means the headline is a lucky pick, not a family of edges.
#[pyfunction]
#[pyo3(signature = (candidates, n_trials, trials_sr_std = DEFAULT_TRIALS_SR_STD))]
fn selection_robustness<'py>(
    py: Python<'py>,
    candidates: Vec<Vec<f64>>,
    n_trials: u32,
    trials_sr_std: f64,
) -> PyResult<Bound<'py, PyDict>> {
    let r = core_selection(&candidates, n_trials, trials_sr_std);
    let d = PyDict::new(py);
    d.set_item("n_candidates", r.n_candidates)?;
    d.set_item("best_dsr", r.best_dsr)?;
    d.set_item("median_dsr", r.median_dsr)?;
    d.set_item("selection_gap", r.selection_gap)?;
    Ok(d)
}

/// Number of independent runs needed to detect `effect` at significance `alpha`
/// with power `power`.
#[pyfunction]
#[pyo3(signature = (effect, alpha = 0.05, power = 0.80))]
fn runs_for_power(effect: f64, alpha: f64, power: f64) -> usize {
    core_runs_for_power(effect, alpha, power)
}

/// pass^k reliability over a per-run pass/fail vector. `mode` is `"all"` (the
/// safety-grade default: every run must pass), `"any"`, or `"at_least"` with `n`.
#[pyfunction]
#[pyo3(signature = (passed_per_run, mode = "all", n = None))]
fn pass_k(passed_per_run: Vec<bool>, mode: &str, n: Option<usize>) -> PyResult<bool> {
    let m = match mode {
        "all" => PassMode::All,
        "any" => PassMode::Any,
        "at_least" => PassMode::AtLeast(n.ok_or_else(|| {
            PyValueError::new_err("mode='at_least' requires n=<number of runs that must pass>")
        })?),
        other => {
            return Err(PyValueError::new_err(format!(
                "unknown mode {other:?} (expected all | any | at_least)"
            )))
        }
    };
    Ok(core_pass_k(&passed_per_run, m))
}

/// The distribution moments the Sharpe family is built on:
/// `{"n_obs", "mean", "std_dev", "skew", "kurtosis", "downside_deviation",
/// "sortino"}` (`sortino` is `None` when downside deviation is zero). `kurtosis`
/// is non-excess (normal = 3), matching the deflated-Sharpe convention.
#[pyfunction]
#[pyo3(signature = (returns, target = 0.0))]
fn moments<'py>(py: Python<'py>, returns: Vec<f64>, target: f64) -> PyResult<Bound<'py, PyDict>> {
    let d = PyDict::new(py);
    d.set_item("n_obs", returns.len())?;
    d.set_item("mean", core_mean(&returns))?;
    d.set_item("std_dev", core_std_dev(&returns))?;
    d.set_item("skew", core_skewness(&returns))?;
    d.set_item("kurtosis", core_kurtosis(&returns))?;
    d.set_item(
        "downside_deviation",
        core_downside_deviation(&returns, target),
    )?;
    d.set_item("sortino", core_sortino(&returns, target))?;
    Ok(d)
}

/// Luck-robust performance-vs-budget curve: the honest, out-of-sample inversion of
/// an in-distribution scaling law. `points` is an ordered list of
/// `(budget, held_out_returns)` for the **same** agent at increasing training/compute
/// budget, each point's returns drawn from a window disjoint from that point's
/// training set (you enforce the split).
///
/// Returns a dict `{"n_budget_points", "peak_budget", "peak_dsr",
/// "peak_dsr_deflated_for_selection", "overfit_onset", "is_monotone_improving",
/// "points"}` where each point is `{"budget", "n_returns", "oos_dsr", "oos_sharpe",
/// "oos_sharpe_annualized", "oos_p_value", "marginal_dsr_per_budget"}`.
/// `peak_dsr_deflated_for_selection` pays for the search over budgets and is `<=`
/// the naive `peak_dsr`; `overfit_onset` / `marginal_dsr_per_budget` are `None` where
/// they do not apply. No monotone law is fitted: the curve is reported, not gated.
#[pyfunction]
#[pyo3(signature = (
    points,
    periods_per_year = 252.0,
    base_n_trials = 1,
    trials_sr_std = DEFAULT_TRIALS_SR_STD,
    seed = DEFAULT_SEED,
    n_boot = 2000,
    block_prob = 0.1,
))]
#[allow(clippy::too_many_arguments)]
fn budget_curve<'py>(
    py: Python<'py>,
    points: Vec<(f64, Vec<f64>)>,
    periods_per_year: f64,
    base_n_trials: u32,
    trials_sr_std: f64,
    seed: u64,
    n_boot: usize,
    block_prob: f64,
) -> PyResult<Bound<'py, PyDict>> {
    let borrowed: Vec<(f64, &[f64])> = points.iter().map(|(b, r)| (*b, r.as_slice())).collect();
    let opts = BudgetCurveOpts {
        periods_per_year,
        base_n_trials,
        trials_sr_std,
        bootstrap_seed: seed,
        n_boot,
        block_prob,
    };
    let report = core_budget_curve(&borrowed, &opts).map_err(PyValueError::new_err)?;

    let pts = pyo3::types::PyList::empty(py);
    for p in &report.points {
        let pd = PyDict::new(py);
        pd.set_item("budget", p.budget)?;
        pd.set_item("n_returns", p.n_returns)?;
        pd.set_item("oos_dsr", p.oos_dsr)?;
        pd.set_item("oos_sharpe", p.oos_sharpe)?;
        pd.set_item("oos_sharpe_annualized", p.oos_sharpe_annualized)?;
        pd.set_item("oos_p_value", p.oos_p_value)?;
        pd.set_item("marginal_dsr_per_budget", p.marginal_dsr_per_budget)?;
        pts.append(pd)?;
    }

    let d = PyDict::new(py);
    d.set_item("n_budget_points", report.n_budget_points)?;
    d.set_item("peak_budget", report.peak_budget)?;
    d.set_item("peak_dsr", report.peak_dsr)?;
    d.set_item(
        "peak_dsr_deflated_for_selection",
        report.peak_dsr_deflated_for_selection,
    )?;
    d.set_item("overfit_onset", report.overfit_onset)?;
    d.set_item("is_monotone_improving", report.is_monotone_improving)?;
    d.set_item("points", pts)?;
    Ok(d)
}

/// The `sharpebench_py` native module (imported as `sharpebench.sharpebench_py`).
#[pymodule]
fn sharpebench_py(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(sharpe_ratio, m)?)?;
    m.add_function(wrap_pyfunction!(probabilistic_sharpe_ratio, m)?)?;
    m.add_function(wrap_pyfunction!(expected_max_sharpe, m)?)?;
    m.add_function(wrap_pyfunction!(deflated_sharpe_ratio, m)?)?;
    m.add_function(wrap_pyfunction!(min_track_record_length, m)?)?;
    m.add_function(wrap_pyfunction!(is_my_sharpe_real, m)?)?;
    m.add_function(wrap_pyfunction!(is_my_sharpe_real_full, m)?)?;
    m.add_function(wrap_pyfunction!(bootstrap_dsr_ci, m)?)?;
    m.add_function(wrap_pyfunction!(bootstrap_pvalue, m)?)?;
    m.add_function(wrap_pyfunction!(reality_check_pvalue, m)?)?;
    m.add_function(wrap_pyfunction!(spa_pvalue, m)?)?;
    m.add_function(wrap_pyfunction!(spa_consistent_pvalue, m)?)?;
    m.add_function(wrap_pyfunction!(step_down_significant, m)?)?;
    m.add_function(wrap_pyfunction!(probability_of_backtest_overfitting, m)?)?;
    m.add_function(wrap_pyfunction!(benjamini_hochberg, m)?)?;
    m.add_function(wrap_pyfunction!(fdr_verdict, m)?)?;
    m.add_function(wrap_pyfunction!(hlz_gate, m)?)?;
    m.add_function(wrap_pyfunction!(selection_robustness, m)?)?;
    m.add_function(wrap_pyfunction!(runs_for_power, m)?)?;
    m.add_function(wrap_pyfunction!(pass_k, m)?)?;
    m.add_function(wrap_pyfunction!(moments, m)?)?;
    m.add_function(wrap_pyfunction!(budget_curve, m)?)?;
    m.add("METHODOLOGY_VERSION", METHODOLOGY_VERSION)?;
    m.add(
        "__doc__",
        "Native pyo3 bindings for SharpeBench's honest-backtest statistics.",
    )?;
    Ok(())
}
