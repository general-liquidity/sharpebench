"""Regressions for the development-tier joint-gate calibration.

Three kinds of case, as ticket P12-A requires. Known-answer cases check a
quantity against a value derived outside the producer: the textbook PSR bound,
the textbook Clopper-Pearson interval, the iid-bootstrap normal limit and the
AR(1) autocorrelation. Control cases run a deliberately wrong formula and
require it to disagree, so a case that would pass either way is not counted as
evidence. The matched-false-positive regression pins the correction to the
superseded fourteenfold history comparison.

Subprocess cases run on staged copies in a temporary directory and never write
a committed file.
"""

from __future__ import annotations

import importlib.util
import json
import math
import re
import shutil
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

import numpy as np

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[1]
PRODUCER = HERE / "make-joint-gate-power.py"
FROZEN_EVIDENCE = ROOT / "paper/evidence/final"
DEVELOPMENT_EVIDENCE = ROOT / "paper/evidence/development/joint-gate-power.jsonl"
REPORT = ROOT / "paper/evidence/development/README.md"

sys.path.insert(0, str(HERE))
import joint_gate_power as jg  # noqa: E402
import kernel_stats as k  # noqa: E402


def load(name, path):
    spec = importlib.util.spec_from_file_location(name, path)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


producer = load("make_joint_gate_power", PRODUCER)
power_curve = load("make_power_curve", HERE / "make-power-curve.py")


def kernel_passes(series, benchmark, bar):
    return k.probabilistic_sharpe_ratio([float(x) for x in series], benchmark) >= bar


class ClosedFormLegs(unittest.TestCase):
    def test_the_shared_threshold_is_the_one_the_power_curve_producer_uses(self):
        """The two producers carry the same closed form. A transcription that
        drifted would put the development tier on a different gate."""
        rng = np.random.default_rng(17)
        for _ in range(200):
            n = int(rng.integers(20, 5000))
            skew = float(rng.uniform(-2.0, 2.0))
            kurt = float(1.0 + skew * skew + rng.uniform(0.0, 6.0))
            benchmark = float(rng.uniform(0.0, 0.4))
            z = k.norm_cdf_inverse(float(rng.choice([0.90, 0.95])))
            self.assertEqual(
                float(jg.min_passing_sharpe(n, skew, kurt, z, benchmark)),
                float(power_curve.min_passing_sharpe(n, skew, kurt, z, benchmark)),
            )

    def test_normal_moments_against_zero_reduce_to_the_textbook_bound(self):
        """Known answer: with skewness 0 and kurtosis 3 the PSR condition is
        u sqrt(n - 1) >= z sqrt(1 + u^2 / 2), whose root is z / sqrt(n - 1 -
        z^2 / 2). Derived by hand, not from the producer."""
        z = k.norm_cdf_inverse(0.90)
        for n in (46, 78, 408, 3990):
            self.assertAlmostEqual(
                float(jg.min_passing_sharpe(n, 0.0, 3.0, z, 0.0)),
                z / math.sqrt(n - 1 - z * z / 2),
                places=12,
            )

    def test_a_wrong_degrees_of_freedom_fails_the_kernel_boundary(self):
        """Control: the same closed form with n in place of n - 1 puts the
        threshold on the wrong side of the kernel's own PSR."""
        n, z = 408, k.norm_cdf_inverse(0.90)
        right = z / math.sqrt(n - 1 - z * z / 2)
        wrong = z / math.sqrt(n - z * z / 2)
        self.assertGreaterEqual(k.psr_from_moments(right * (1 + 1e-9), n, 0.0), 0.90)
        self.assertLess(k.psr_from_moments(wrong, n, 0.0), 0.90)

    def test_the_window_leg_is_the_kernel_gate_on_every_window(self):
        rng = np.random.default_rng(23)
        z_bar = k.norm_cdf_inverse(jg.PER_RUN_PSR_BAR)
        track = jg.draw_track(rng, 40, jg.N_WINDOWS * 46, 0.0)
        thresholds = jg.window_thresholds(track, jg.N_WINDOWS, z_bar)
        windows = track.reshape(40, jg.N_WINDOWS, 46)
        for replication in range(40):
            leg = float(thresholds[replication].max())
            scale = abs(leg) + 1.0
            for shift, expected in ((leg + 1e-9 * scale, True), (leg - 1e-7 * scale, False)):
                every = all(
                    kernel_passes(shift + window, 0.0, jg.PER_RUN_PSR_BAR)
                    for window in windows[replication]
                )
                self.assertEqual(every, expected, replication)

    def test_the_deflation_leg_is_the_kernel_gate_on_the_pooled_track(self):
        rng = np.random.default_rng(29)
        panel = producer.panel_bar(str(FROZEN_EVIDENCE), "us-indices-1d")
        bar = panel["deflation_bar_per_period"]
        z_bar = k.norm_cdf_inverse(jg.DSR_BAR)
        track = jg.draw_track(rng, 30, jg.N_WINDOWS * 78, 0.0)
        thresholds = jg.threshold_true_sharpe(track, z_bar, bar)
        for series, threshold in zip(track, thresholds):
            self.assertTrue(kernel_passes(threshold + 1e-9 + series, bar, jg.DSR_BAR))
            self.assertFalse(kernel_passes(threshold - 1e-7 + series, bar, jg.DSR_BAR))

    def test_a_psr_bar_below_one_half_passes_at_a_negative_sharpe(self):
        """Known answer: the textbook bound z / sqrt(n - 1 - z^2 / 2) is signed,
        and a bar below PSR 0.5 has z < 0, so the smallest passing Sharpe is
        below zero. Squaring the gate loses that branch and returns the
        reflection, which is a strictly stricter rule.
        """
        bar = 1.0 - 0.05 ** (1.0 / jg.N_WINDOWS)
        z = k.norm_cdf_inverse(bar)
        self.assertLess(z, 0.0)
        for n in (234, 571, 2386):
            threshold = float(jg.min_passing_sharpe(n, 0.0, 3.0, z, 0.0))
            self.assertAlmostEqual(threshold, z / math.sqrt(n - 1 - z * z / 2), places=12)
            self.assertLess(threshold, 0.0)
            self.assertAlmostEqual(k.psr_from_moments(threshold, n, 0.0), bar, places=12)
            self.assertLess(k.psr_from_moments(threshold * (1.0 + 1e-9), n, 0.0), bar)
            self.assertGreaterEqual(k.psr_from_moments(threshold * (1.0 - 1e-9), n, 0.0), bar)
            # Control: the reflected root the squared gate hands back is deep
            # inside the passing set, so using it refuses skill the rule admits.
            self.assertGreater(k.psr_from_moments(-threshold, n, 0.0), bar)

    def test_the_signed_threshold_is_the_kernel_gate_on_a_drawn_series(self):
        """The same branch against the kernel's own PSR rather than against the
        closed form, on series whose sample moments are not the normal ones."""
        bar = 1.0 - 0.05 ** (1.0 / jg.N_WINDOWS)
        z = k.norm_cdf_inverse(bar)
        track = jg.draw_track(np.random.default_rng(41), 30, 234, 0.0)
        thresholds = jg.threshold_true_sharpe(track, z, 0.0)
        for series, threshold in zip(track, thresholds):
            self.assertTrue(kernel_passes(threshold + 1e-9 + series, 0.0, bar))
            self.assertFalse(kernel_passes(threshold - 1e-7 + series, 0.0, bar))

    def test_an_infeasible_kurtosis_is_refused_rather_than_solved(self):
        with self.assertRaises(jg.PowerSupportError):
            jg.min_passing_sharpe(20, 0.0, 400.0, k.norm_cdf_inverse(0.9), 0.0)


def replay_bootstrap_pvalue(length, n_boot, block_prob, seed, centered, observed):
    """The kernel's p-value, counted directly, over the same resample paths
    `bootstrap_threshold` draws from a generator seeded the same way.

    Deliberately written as the definition rather than as the producer's
    order-statistic shortcut: a wrong `keep`, a wrong smoothing term or a wrong
    inequality direction shows up as a disagreement.
    """
    rng = np.random.Generator(np.random.PCG64(seed))
    index = rng.integers(0, length, (1, n_boot))
    total = np.zeros((1, n_boot))
    for _ in range(length):
        total += np.take_along_axis(centered[None, :], index, axis=1)
        restart = rng.random((1, n_boot)) < block_prob
        index = np.where(restart, rng.integers(0, length, (1, n_boot)), index + 1)
        np.mod(index, length, out=index)
    means = total[0] / length
    at_least = int(np.sum(means >= observed))
    return (at_least + 1.0) / (n_boot + 1.0)


class BootstrapLeg(unittest.TestCase):
    LENGTH = 300
    N_BOOT = 400
    BLOCK = 0.1
    SEED = 424242

    def test_the_threshold_is_where_the_counted_p_value_crosses_alpha(self):
        base = np.random.default_rng(5).standard_normal((1, self.LENGTH))
        threshold = float(
            jg.bootstrap_threshold(
                base.copy(),
                np.random.Generator(np.random.PCG64(self.SEED)),
                self.N_BOOT,
                self.BLOCK,
                jg.BOOTSTRAP_ALPHA,
            )[0]
        )
        series = base[0]
        for shift, should_pass in ((threshold + 1e-12, True), (threshold - 1e-9, False)):
            shifted = series + shift
            centered = shifted - shifted.mean()
            p = replay_bootstrap_pvalue(
                self.LENGTH, self.N_BOOT, self.BLOCK, self.SEED, centered, shifted.mean()
            )
            self.assertEqual(p < jg.BOOTSTRAP_ALPHA, should_pass, (shift, p))

    def test_the_iid_bootstrap_threshold_approaches_the_normal_quantile(self):
        """Known answer: at restart probability 1 the stationary bootstrap is
        the iid bootstrap, whose resampled mean has standard deviation
        sd / sqrt(n), so the 95th percentile of the null distribution is about
        1.645 sd / sqrt(n) above zero. Checked against that limit, not against
        the producer."""
        length, n_boot = 4000, 20_000
        series = np.random.default_rng(9).standard_normal((1, length))
        threshold = float(
            jg.bootstrap_threshold(
                series.copy(),
                np.random.Generator(np.random.PCG64(77)),
                n_boot,
                1.0,
                jg.BOOTSTRAP_ALPHA,
            )[0]
        )
        centered = series[0] - series[0].mean()
        expected = 1.6448536269514722 * centered.std() / math.sqrt(length) - series[0].mean()
        self.assertAlmostEqual(threshold, expected, delta=0.04 * abs(expected - 0.0) + 0.002)

    def test_an_alpha_with_no_attainable_rejection_region_is_refused(self):
        with self.assertRaises(jg.PowerSupportError):
            jg.bootstrap_keep(1e-6, 2000)

    def test_the_keep_index_matches_the_smoothed_p_value_definition(self):
        """Control on the algebra: `keep` must be the largest count of resampled
        means that still leaves (count + 1) / (n_boot + 1) below alpha."""
        for alpha, n_boot in ((0.05, 2000), (0.05, 399), (0.10, 1000), (0.01, 999)):
            keep = jg.bootstrap_keep(alpha, n_boot)
            self.assertLess((keep - 1 + 1.0) / (n_boot + 1.0), alpha)
            self.assertGreaterEqual((keep + 1.0) / (n_boot + 1.0), alpha)


class SerialDependence(unittest.TestCase):
    def test_rho_zero_is_the_plain_normal_draw(self):
        expected = np.random.Generator(np.random.PCG64(101)).standard_normal((3, 50))
        got = jg.draw_track(np.random.Generator(np.random.PCG64(101)), 3, 50, 0.0)
        np.testing.assert_array_equal(got, expected)

    def test_the_ar1_track_has_unit_variance_and_the_requested_autocorrelation(self):
        """Known answer: a stationary AR(1) normalized this way has marginal
        variance 1 and lag-1 autocorrelation rho, whatever rho is."""
        rng = np.random.default_rng(31)
        for rho in (-0.2, -0.1, 0.1, 0.2, 0.5):
            track = jg.draw_track(rng, 4000, 500, rho)
            self.assertAlmostEqual(float(track.var()), 1.0, delta=0.02)
            lagged = float((track[:, :-1] * track[:, 1:]).mean())
            self.assertAlmostEqual(lagged, rho, delta=0.02)

    def test_dropping_the_stationary_start_leaves_a_variance_deficit(self):
        """Control: `draw_track` rescales the first innovation before running
        the recursion. Without that one line the track starts under-dispersed
        and its variance over a short window falls below 1, which the
        known-answer case above would catch."""
        rho, size, length = 0.8, 20_000, 20
        scale = math.sqrt(1.0 - rho * rho)
        wrong = np.random.default_rng(37).standard_normal((size, length))
        for t in range(1, length):
            wrong[:, t] += rho * wrong[:, t - 1]
        wrong *= scale
        self.assertLess(float(wrong.var()), 0.95)
        right = jg.draw_track(np.random.default_rng(37), size, length, rho)
        self.assertAlmostEqual(float(right.var()), 1.0, delta=0.03)

    def test_an_unrepresentable_rho_is_refused(self):
        with self.assertRaises(jg.PowerSupportError):
            producer.rho_key(0.0001)
        with self.assertRaises(jg.PowerSupportError):
            jg.draw_track(np.random.default_rng(1), 2, 10, 1.5)


class Intervals(unittest.TestCase):
    def test_the_textbook_clopper_pearson_interval(self):
        """Known answer: the exact two-sided 95 percent interval for 3 of 10 is
        (0.0667, 0.6525) in every statistics text."""
        lower, upper = jg.clopper_pearson(3, 10, 0.025)
        self.assertAlmostEqual(lower, 0.0667, delta=1e-4)
        self.assertAlmostEqual(upper, 0.6525, delta=1e-4)
        self.assertAlmostEqual(jg.binomial_cdf(2, 10, lower), 0.975, delta=1e-5)
        self.assertAlmostEqual(jg.binomial_cdf(3, 10, upper), 0.025, delta=1e-5)

    def test_no_observed_failure_gives_the_exact_rule_of_three_bound(self):
        for n in (100, 6_400, 200_000):
            _, upper = jg.clopper_pearson(0, n, 0.05)
            self.assertAlmostEqual(upper, 1.0 - 0.05 ** (1.0 / n), places=12)

    def test_a_wald_interval_would_certify_a_zero_rate_and_is_not_used(self):
        """Control: the normal-approximation interval at zero successes is the
        single point zero, which would read as proof that the rule never admits
        a zero-skill entry. The exact bound must be strictly above it."""
        passes, replications = 0, 200_000
        rate, se, _, upper = jg.rate_with_interval(passes, replications)
        self.assertEqual((rate, se), (0.0, 0.0))
        self.assertGreater(upper, 0.0)
        self.assertLess(upper, 2e-5)

    def test_the_field_rate_is_the_independent_entry_union(self):
        self.assertAlmostEqual(jg.field_probability(0.01, 8), 1.0 - 0.99**8, places=15)
        self.assertEqual(jg.field_probability(0.0, 8), 0.0)


def wrong_every_window_bars(design, s, power):
    """A deliberately wrong history rule: it asks each window to pass with the
    whole rule's target probability instead of its k-th root, so it under-counts
    the history an every-window rule needs."""
    step = design.windows
    lo = hi = step * (int(design.z_bar * design.z_bar) + 4)
    while jg.sharpe_pass_probability(hi // step, s, 0.0, design.z_bar) < power:
        lo, hi = hi, hi * 2
    while hi - lo > step:
        mid = ((lo + hi) // (2 * step)) * step
        if mid <= lo:
            break
        if jg.sharpe_pass_probability(mid // step, s, 0.0, design.z_bar) < power:
            lo = mid
        else:
            hi = mid
    return hi


class MatchedFalsePositiveRate(unittest.TestCase):
    """The P12-A correction: the superseded fourteenfold history comparison put
    a rule at a nominal rate of 1e-6 against one at 0.05. At a matched rate the
    factor is under two. These cases exist so that error cannot come back."""

    PERIODS = 252.0
    EFFECT = 1.0

    def setUp(self):
        self.every = jg.GateDesign("every_window_6of6_psr_0.90", 6, 6, 1.0 - jg.PER_RUN_PSR_BAR)
        self.pooled_005 = jg.GateDesign("pooled_psr_0.95", 1, 1, 0.05)
        self.pooled_matched = jg.design_at_false_positive_rate(
            "pooled_matched", 1, 1, self.every.nominal_false_positive_rate
        )

    def compare(self, other, power):
        return jg.compare_designs(self.every, other, self.EFFECT, self.PERIODS, power)

    def test_the_two_rules_do_not_share_a_nominal_false_positive_rate(self):
        self.assertAlmostEqual(self.every.nominal_false_positive_rate, 1e-6, places=12)
        self.assertAlmostEqual(self.pooled_005.nominal_false_positive_rate, 0.05, places=12)
        self.assertAlmostEqual(self.pooled_matched.nominal_false_positive_rate, 1e-6, places=12)

    def test_the_superseded_comparison_reproduces_the_fourteenfold_factor(self):
        """Reproduced so the record shows exactly which comparison was wrong,
        never as a factor between gate designs."""
        self.assertAlmostEqual(self.compare(self.pooled_005, 0.5)["history_factor_a_over_b"],
                               14.0, delta=0.1)
        self.assertAlmostEqual(self.compare(self.pooled_005, 0.8)["history_factor_a_over_b"],
                               9.2, delta=0.1)

    def test_at_a_matched_rate_the_factor_is_about_one_point_seven(self):
        at_half = self.compare(self.pooled_matched, 0.5)
        self.assertTrue(at_half["false_positive_rate_matched"])
        self.assertAlmostEqual(at_half["history_factor_a_over_b"], 1.68, delta=0.03)
        self.assertAlmostEqual(at_half["years_a"], 37.98, delta=0.1)
        self.assertAlmostEqual(at_half["years_b"], 22.65, delta=0.1)
        self.assertAlmostEqual(
            self.compare(self.pooled_matched, 0.8)["history_factor_a_over_b"], 1.81, delta=0.03
        )

    def test_an_unmatched_comparison_is_refused_as_a_design_comparison(self):
        with self.assertRaises(jg.PowerSupportError) as caught:
            jg.require_matched(self.compare(self.pooled_005, 0.5))
        self.assertIn("do not share a false-positive rate", str(caught.exception))
        jg.require_matched(self.compare(self.pooled_matched, 0.5))

    def test_the_matched_factor_depends_on_which_rate_the_designs_are_matched_at(self):
        """Matching at 0.05 instead of 1e-6 gives a different, larger factor, so
        the matched rate has to be stated with the number. Six of six matched at
        0.05 needs a per-window bar of PSR 0.393, below one half, which is the
        branch `min_passing_sharpe` has to take signed."""
        every_at_005 = jg.design_at_false_positive_rate("every_at_005", 6, 6, 0.05)
        pooled_at_005 = jg.design_at_false_positive_rate("pooled_at_005", 1, 1, 0.05)
        self.assertLess(every_at_005.psr_bar, 0.5)
        matched = jg.require_matched(
            jg.compare_designs(every_at_005, pooled_at_005, self.EFFECT, self.PERIODS, 0.5)
        )
        self.assertAlmostEqual(matched["history_factor_a_over_b"], 2.05, delta=0.03)
        self.assertGreater(matched["history_factor_a_over_b"], 1.68)

    def test_the_matched_005_history_agrees_with_the_monte_carlo_at_that_length(self):
        """The design whose per-window bar is below PSR 0.5 checked the same way
        as the pooled one: at the solved length the simulated every-window pass
        probability must be the 50 percent the closed form asked for. Under the
        unsigned threshold this length was 3426 bars, where the rule really
        passes about four times in five."""
        design = jg.design_at_false_positive_rate("every_at_005", 6, 6, 0.05)
        s = self.EFFECT / math.sqrt(self.PERIODS)
        bars = design.required_bars(s, 0.5)
        rng = np.random.Generator(np.random.PCG64(20260923))
        track = jg.draw_track(rng, 20_000, bars, 0.0)
        thresholds = jg.window_thresholds(track, design.windows, design.z_bar)
        self.assertAlmostEqual(float((thresholds.max(axis=1) <= s).mean()), 0.5, delta=0.02)
        self.assertLess(bars, 3426)

    def test_a_design_compared_with_itself_costs_the_same_history(self):
        self.assertEqual(self.compare(self.every, 0.5)["history_factor_a_over_b"], 1.0)

    def test_the_wrong_per_window_power_target_changes_the_matched_factor(self):
        """Control: an every-window rule whose per-window target is the whole
        rule's power rather than its sixth root needs much less history, and a
        matched factor computed that way would land outside the band asserted
        above. The producer's rule and the wrong one must disagree."""
        s = self.EFFECT / math.sqrt(self.PERIODS)
        right = self.every.required_bars(s, 0.5)
        wrong = wrong_every_window_bars(self.every, s, 0.5)
        self.assertLess(wrong, 0.75 * right)
        wrong_factor = wrong / self.pooled_matched.required_bars(s, 0.5)
        self.assertLess(wrong_factor, 1.5)

    def test_the_closed_form_history_agrees_with_the_monte_carlo_at_that_length(self):
        """The design comparison is closed form; the Monte Carlo legs are not.
        At the reported matched-rate pooled length the simulated pass
        probability must be the 50 percent the closed form solved for."""
        s = self.EFFECT / math.sqrt(self.PERIODS)
        bars = self.pooled_matched.required_bars(s, 0.5)
        rng = np.random.Generator(np.random.PCG64(20260923))
        track = jg.draw_track(rng, 20_000, bars, 0.0)
        thresholds = jg.threshold_true_sharpe(track, self.pooled_matched.z_bar, 0.0)
        simulated = float((thresholds <= s).mean())
        self.assertAlmostEqual(simulated, 0.5, delta=0.02)


class ProducerRuns(unittest.TestCase):
    REPLICATIONS = 256
    JOINT_REPLICATIONS = 64

    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.dir = Path(self.tmp.name)
        self.sweep = self.dir / "sweep"
        self.sweep.mkdir()
        for dataset in producer.PANELS:
            shutil.copy(FROZEN_EVIDENCE / f"{dataset}.jsonl", self.sweep / f"{dataset}.jsonl")

    def run_producer(self, *args):
        return subprocess.run(
            [sys.executable, str(PRODUCER), *args], capture_output=True, text=True, timeout=1800
        )

    def compute(self, name, *extra):
        out = self.dir / name
        proc = self.run_producer(
            "--replications", str(self.REPLICATIONS),
            "--joint-replications", str(self.JOINT_REPLICATIONS),
            "--sweep-dir", str(self.sweep),
            "--evidence", str(out),
            "--rhos-two-leg", "0.0,0.1",
            "--rhos-joint", "0.0",
            *extra,
        )
        self.assertEqual(proc.returncode, 0, proc.stderr)
        return out

    def rows(self, path):
        return [json.loads(line) for line in path.read_text(encoding="utf-8").splitlines()]

    def test_output_does_not_depend_on_worker_count_or_run(self):
        one = self.compute("one.jsonl", "--jobs", "1").read_bytes()
        again = self.compute("again.jsonl", "--jobs", "1").read_bytes()
        two = self.compute("two.jsonl", "--jobs", "3").read_bytes()
        self.assertEqual(one, again)
        self.assertEqual(one, two)

    def test_the_runtime_estimate_is_reported_outside_the_evidence_bytes(self):
        out = self.dir / "timed.jsonl"
        runtime = self.dir / "runtime.json"
        proc = self.run_producer(
            "--replications", str(self.REPLICATIONS),
            "--joint-replications", str(self.JOINT_REPLICATIONS),
            "--sweep-dir", str(self.sweep), "--evidence", str(out),
            "--rhos-two-leg", "0.0", "--rhos-joint", "0.0",
            "--runtime-out", str(runtime),
        )
        self.assertEqual(proc.returncode, 0, proc.stderr)
        timing = json.loads(runtime.read_text())
        self.assertGreater(timing["two_leg_seconds"] + timing["joint_seconds"], 0.0)
        self.assertNotIn("seconds", out.read_text(encoding="utf-8"))

    def test_the_joint_rule_never_admits_more_than_any_leg_it_contains(self):
        """Within one run's draws the conjunction is the pointwise maximum of
        its legs' thresholds, so it admits no more noise and needs at least as
        much skill. Rules from the two runs are drawn separately and are not
        comparable, which is why every record names its draws."""
        rows = self.rows(self.compute("legs.jsonl"))
        admitted = {
            (r["dataset"], r["rule"]): r["admitted"]
            for r in rows
            if r["record"] == "false_positive_rate"
            and r["rho"] == 0.0
            and r["draws"] == producer.JOINT_DRAWS
        }
        summary = {
            (r["dataset"], r["rule"]): r
            for r in rows
            if r["record"] == "power_summary"
            and r["rho"] == 0.0
            and r["draws"] == producer.JOINT_DRAWS
        }
        modelled = {dataset for dataset, rule in admitted if rule == "joint"}
        self.assertEqual(len(modelled), len(producer.PANELS) - 1)
        for dataset in modelled:
            for leg in ("pass_k", "dsr", "bootstrap"):
                self.assertLessEqual(
                    admitted[(dataset, "joint")], admitted[(dataset, leg)], (dataset, leg)
                )
                for percent in producer.CROSSINGS:
                    key = f"sharpe_at_{percent}pct"
                    self.assertGreaterEqual(
                        summary[(dataset, "joint")][key],
                        summary[(dataset, leg)][key],
                        (dataset, leg, percent),
                    )

    def test_every_matched_comparison_record_really_matches(self):
        rows = self.rows(self.compute("designs.jsonl"))
        matched = [
            r
            for r in rows
            if r["record"] == "gate_comparison"
            and r["comparison"] == "matched_false_positive_rate"
        ]
        superseded = [
            r
            for r in rows
            if r["record"] == "gate_comparison"
            and r["comparison"] == "superseded_unmatched_false_positive_rate"
        ]
        self.assertTrue(matched and superseded)
        for record in matched:
            self.assertTrue(record["false_positive_rate_matched"])
            self.assertAlmostEqual(
                record["nominal_false_positive_rate_a"],
                record["nominal_false_positive_rate_b"],
                delta=1e-12 * record["nominal_false_positive_rate_a"],
            )
        for record in superseded:
            self.assertFalse(record["false_positive_rate_matched"])
            self.assertIn("not a comparison of gate designs", record["superseded"])

    def test_the_run_writes_only_the_development_namespace(self):
        before = {
            path: path.read_bytes() for path in sorted(FROZEN_EVIDENCE.glob("*.jsonl"))
        }
        self.compute("namespace.jsonl")
        self.assertEqual(
            {path: path.read_bytes() for path in sorted(FROZEN_EVIDENCE.glob("*.jsonl"))}, before
        )
        self.assertEqual(Path(producer.EVIDENCE).parent.name, "development")
        self.assertNotEqual(Path(producer.EVIDENCE).parent, FROZEN_EVIDENCE)

    def assert_refused(self, reason, *extra):
        out = self.dir / "refused.jsonl"
        proc = self.run_producer(
            "--replications", str(self.REPLICATIONS),
            "--joint-replications", str(self.JOINT_REPLICATIONS),
            "--sweep-dir", str(self.sweep), "--evidence", str(out),
            "--rhos-two-leg", "0.0", "--rhos-joint", "0.0", *extra,
        )
        self.assertNotEqual(proc.returncode, 0, proc.stdout)
        self.assertIn(reason, proc.stderr)
        self.assertNotIn("Traceback", proc.stderr)
        self.assertFalse(out.exists())

    def test_a_missing_panel_is_refused(self):
        (self.sweep / "crypto-majors-1h.jsonl").unlink()
        self.assert_refused("missing evidence file")

    def test_two_bars_in_one_default_cell_are_refused(self):
        path = self.sweep / "rates-1d.jsonl"
        rows = [json.loads(line) for line in path.read_text().splitlines() if line.strip()]
        first = next(
            r
            for r in rows
            if all(r.get(key) == value for key, value in producer.DEFAULT_CELL.items())
        )
        first["deflation_bar_per_period"] *= 1.5
        path.write_text("".join(json.dumps(r) + "\n" for r in rows))
        self.assert_refused("rates-1d: the default cell carries 2 distinct bars")

    def test_a_joint_rho_outside_the_two_leg_set_is_refused(self):
        self.assert_refused(
            "is simulated jointly but not for two legs", "--rhos-joint", "0.0,0.1"
        )


class CommittedRun(unittest.TestCase):
    """The committed development-tier run and the report beside it agree."""

    @classmethod
    def setUpClass(cls):
        cls.rows = [
            json.loads(line)
            for line in DEVELOPMENT_EVIDENCE.read_text(encoding="utf-8").splitlines()
        ]
        cls.meta = next(r for r in cls.rows if r["record"] == "meta")
        cls.report = REPORT.read_text(encoding="utf-8")

    def rate(self, dataset, rule, rho=0.0, draws=None):
        draws = draws or producer.TWO_LEG_DRAWS
        return next(
            r
            for r in self.rows
            if r["record"] == "false_positive_rate"
            and r["dataset"] == dataset
            and r["rule"] == rule
            and r["rho"] == rho
            and r["draws"] == draws
        )

    def test_the_run_is_labelled_development_tier_throughout(self):
        self.assertEqual(self.meta["tier"], producer.TIER)
        self.assertIn("not a validated claim", self.meta["claim_status"])
        self.assertTrue(all(row.get("tier") == producer.TIER for row in self.rows))
        self.assertTrue(self.meta["legs_not_modelled"])

    def test_a_zero_count_is_never_reported_as_a_zero_rate(self):
        """Observing no false positive is not proof of a zero rate, so every
        record with no admission still has to carry a positive upper bound, per
        entry and for the field."""
        zero_counts = 0
        for record in self.rows:
            if record["record"] != "false_positive_rate":
                continue
            if record["admitted"] == 0:
                zero_counts += 1
                self.assertEqual(record["per_entry_rate"], 0.0)
                self.assertGreater(record["per_entry_interval95"][1], 0.0)
                self.assertGreater(record["field_rate_upper95_independent_entries"], 0.0)
        self.assertGreater(zero_counts, 0)

    def test_the_joint_rule_admits_far_less_noise_than_its_loosest_leg(self):
        """The bootstrap leg alone rejects at 0.05, so on its own it admits one
        zero-skill entry in twenty. The conjunction has to be orders of
        magnitude tighter on every panel."""
        modelled = {
            r["dataset"]
            for r in self.rows
            if r["record"] == "false_positive_rate" and r["rule"] == "joint"
        }
        self.assertEqual(len(modelled), len(producer.PANELS) - 1)
        for dataset in producer.PANELS:
            self.assertLess(
                self.rate(dataset, "two_leg")["per_entry_interval95"][1], 1e-4, dataset
            )
            if dataset not in modelled:
                continue
            joint = self.rate(dataset, "joint", draws=producer.JOINT_DRAWS)
            bootstrap = self.rate(dataset, "bootstrap", draws=producer.JOINT_DRAWS)
            self.assertGreater(bootstrap["per_entry_rate"], 0.03, dataset)
            self.assertLess(joint["per_entry_interval95"][1], 5e-3, dataset)

    def test_the_two_calibrated_legs_land_near_their_nominal_levels(self):
        """A single window's PSR bar is 0.90 and the bootstrap leg's alpha is
        0.05, so their realized rates under the zero-skill null should be close
        to 0.10 and 0.05. A leg far from its nominal level would mean the
        modelled gate is not the shipped one."""
        window = self.rate("us-indices-1d", "per_window")
        self.assertAlmostEqual(window["per_entry_rate"], 0.10, delta=0.01)
        bootstrap = self.rate("us-indices-1d", "bootstrap", draws=producer.JOINT_DRAWS)
        self.assertAlmostEqual(bootstrap["per_entry_rate"], 0.05, delta=0.02)

    def test_positive_autocorrelation_raises_the_per_window_false_positive_rate(self):
        """eq:psr assumes serially independent returns, so it is too favorable
        under positive autocorrelation and too strict under negative. The
        dependence sensitivity has to show that ordering."""
        rates = [self.rate("us-indices-1d", "per_window", rho)["per_entry_rate"]
                 for rho in (-0.2, -0.1, 0.0, 0.1, 0.2)]
        self.assertEqual(rates, sorted(rates))
        self.assertGreater(rates[-1] - rates[0], 0.02)

    def test_every_number_in_the_report_is_read_from_the_evidence(self):
        # Decimals the report states that no record carries: the gate bars, the
        # rho values and bar multiples the run was configured with, and the two
        # measured wall times, which are deliberately kept out of the evidence
        # bytes so the file stays reproducible.
        allowed = {
            "0.90", "0.95", "0.05", "0.10", "0.25", "0.0", "0.1", "0.2", "0.5",
            "1.0", "2.0", "3.0", "76.9", "206.2",
            str(producer.SEED), str(producer.REPLICATIONS), str(producer.JOINT_REPLICATIONS),
        }
        for row in self.rows:
            if row["record"] == "false_positive_rate":
                allowed |= {
                    f"{row['per_entry_rate']:.4f}",
                    f"{row['per_entry_interval95'][1]:.3g}",
                    f"{row['field_rate_upper95_independent_entries']:.3g}",
                }
            elif row["record"] == "power_point":
                allowed |= {f"{row['pass_probability']:.3f}", f"{row['sharpe_annualized']:.4f}"}
            elif row["record"] == "power_summary":
                allowed |= {f"{row[f'sharpe_at_{p}pct']:.2f}" for p in producer.CROSSINGS}
            elif row["record"] == "gate_comparison":
                allowed |= {
                    f"{row['history_factor_a_over_b']:.2f}",
                    f"{row['years_a']:.1f}",
                    f"{row['years_b']:.1f}",
                    f"{row['per_test_psr_bar_a']:.3f}",
                    f"{row['per_test_psr_bar_b']:.3f}",
                }
            elif row["record"] == "panel":
                allowed |= {f"{row['deflation_bar_annualized_equivalent']:.4f}"}
        body = "\n".join(
            line for line in self.report.splitlines() if not line.startswith("<!--")
        )
        stray = sorted(set(re.findall(r"\d+\.\d+(?:e-?\d+)?", body)) - allowed)
        self.assertEqual(stray, [])


if __name__ == "__main__":
    unittest.main()
