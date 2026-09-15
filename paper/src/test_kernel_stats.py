"""kernel_stats is the kernel's formula, not a look-alike.

Three independent anchors: the kernel's own golden PSRs (bit-identical), the
Bailey and Lopez de Prado (2014) worked example the kernel's regression test
also reproduces, and the committed tab:units bars. A figure that imports this
module therefore draws what the kernel computes.
"""

from __future__ import annotations

import importlib.util
import json
import math
import unittest
from pathlib import Path

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[1]
GOLDEN = ROOT / "crates/sharpebench-core/golden"


def load(name):
    spec = importlib.util.spec_from_file_location(name.replace("-", "_"), HERE / f"{name}.py")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


k = load("kernel_stats")


def pooled_track(submission):
    """The kernel's pooled track for a golden field of one seed per window:
    the runs concatenated in submission order."""
    return [x for run in submission["runs"] for x in run["returns"]]


class GoldenKernelAgreement(unittest.TestCase):
    def test_psr_reproduces_the_golden_scores_bit_for_bit(self):
        inputs = json.loads((GOLDEN / "synthetic_field.input.json").read_text(encoding="utf-8"))
        scores = json.loads((GOLDEN / "synthetic_field.scores.json").read_text(encoding="utf-8"))
        self.assertEqual([s["agent_id"] for s in inputs], [s["agent_id"] for s in scores])
        interior = 0
        for submission, score in zip(inputs, scores):
            track = pooled_track(submission)
            self.assertEqual(len(track), score["pooled_observations"])
            with self.subTest(agent=score["agent_id"]):
                self.assertEqual(
                    k.probabilistic_sharpe_ratio(track, 0.0).hex(), score["psr"].hex()
                )
            interior += 0.0 < score["psr"] < 1.0
        # Saturated PSRs of 0 or 1 would agree with almost any formula.
        self.assertGreaterEqual(interior, 2)

    def test_the_normal_cdf_is_the_kernels_approximation_not_an_exact_one(self):
        # erf(0) in Abramowitz and Stegun 7.1.26 is 1e-9, not 0: the committed
        # evidence prints PSR 0.5000000005 for an all-zero track.
        self.assertEqual(k.psr_from_moments(0.0, 2448, 0.0), 0.5000000005)
        self.assertNotEqual(k.norm_cdf(0.0), 0.5 * (1.0 + math.erf(0.0)))

    def test_cdf_inverse_is_the_threshold_of_the_kernels_cdf(self):
        for p in (0.90, 0.95):
            z = k.norm_cdf_inverse(p)
            self.assertGreaterEqual(k.norm_cdf(z), p)
            self.assertLess(k.norm_cdf(z - 1e-9), p)
            self.assertAlmostEqual(z, k.norm_ppf(p), places=5)


class BaileyWorkedExample(unittest.TestCase):
    """Bailey and Lopez de Prado (2014), pp. 9-10 of the working paper: N = 100,
    V[{SR_n}] = 1/2 annualized, T = 1250 daily returns at 250 a year, skewness
    -3, kurtosis 10, annualized Sharpe 2.5."""

    sigma = math.sqrt(0.5) / math.sqrt(250.0)
    sr = 2.5 / math.sqrt(250.0)

    def test_threshold(self):
        self.assertAlmostEqual(k.expected_max_sharpe(self.sigma, 100), 0.1132, delta=5e-5)

    def test_deflated_sharpe_at_one_hundred_trials(self):
        dsr = k.deflated_sharpe_from_moments(self.sr, 1250, 100, self.sigma, -3.0, 10.0)
        self.assertAlmostEqual(dsr, 0.9004, delta=5e-5)

    def test_deflated_sharpe_at_forty_six_trials(self):
        dsr = k.deflated_sharpe_from_moments(self.sr, 1250, 46, self.sigma, -3.0, 10.0)
        self.assertAlmostEqual(dsr, 0.9505, delta=5e-5)

    def test_normal_moments_at_eighty_eight_trials(self):
        dsr = k.deflated_sharpe_from_moments(self.sr, 1250, 88, self.sigma)
        self.assertAlmostEqual(dsr, 0.9505, delta=5e-5)

    def test_the_skew_term_enters_multiplied_by_the_observed_sharpe(self):
        normal = k.deflated_sharpe_from_moments(self.sr, 1250, 100, self.sigma, 0.0, 10.0)
        skewed = k.deflated_sharpe_from_moments(self.sr, 1250, 100, self.sigma, -3.0, 10.0)
        self.assertGreater(normal, skewed)


class UnitsTableBars(unittest.TestCase):
    """tab:units: SR*_0 at N = 50 per period, and the annualized Sharpe that
    reaches it at 1h, 4h, 1d and 1w, under the calendars that table used."""

    ROWS = (
        (0.070, 0.159, (14.9, 7.5, 2.5, 1.1)),
        (0.200, 0.455, (42.6, 21.3, 7.2, 3.3)),
        (0.350, 0.797, (74.6, 37.3, 12.6, 5.7)),
        (0.500, 1.138, (106.5, 53.3, 18.1, 8.2)),
    )
    PERIODS = (8760.0, 2190.0, 252.0, 52.0)

    def test_every_printed_cell(self):
        for sigma, bar, annualized in self.ROWS:
            computed = k.expected_max_sharpe(sigma, 50)
            with self.subTest(sigma=sigma):
                self.assertEqual(f"{computed:.3f}", f"{bar:.3f}")
                for periods, printed in zip(self.PERIODS, annualized):
                    self.assertEqual(
                        f"{computed * math.sqrt(periods):.1f}", f"{printed:.1f}"
                    )


class DeflationCurveFigure(unittest.TestCase):
    def test_the_track_crosses_the_bar_between_32_and_64_trials(self):
        figures = load("make-figures")
        curve = dict(zip(figures.TRIALS, figures.deflation_curve()))
        self.assertGreaterEqual(curve[32], figures.DSR_BAR)
        self.assertLess(curve[64], figures.DSR_BAR)
        values = [curve[n] for n in figures.TRIALS]
        self.assertEqual(values, sorted(values, reverse=True))


if __name__ == "__main__":
    unittest.main()
