"""Deterministic domain and degenerate-root checks for the PSR threshold solver."""

import importlib.util
import math
import sys
import unittest
import warnings
from pathlib import Path

import numpy as np

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
import joint_gate_power as joint  # noqa: E402
import kernel_stats as kernel  # noqa: E402

spec = importlib.util.spec_from_file_location("solver_power_curve", HERE / "make-power-curve.py")
curve = importlib.util.module_from_spec(spec)
spec.loader.exec_module(curve)


class SolverDomain(unittest.TestCase):
    def test_half_probability_is_the_benchmark_including_zero(self):
        for module in (joint, curve):
            for benchmark in (-0.3, 0.0, 0.4):
                with warnings.catch_warnings():
                    warnings.simplefilter("error")
                    got = module.min_passing_sharpe(20, [0.0, 0.5], [3.0, 3.0], 0.0, benchmark)
                np.testing.assert_array_equal(got, [benchmark, benchmark])

    def test_nonfinite_inputs_are_explicitly_unsupported(self):
        for module in (joint, curve):
            for slot in range(5):
                for bad in (math.nan, math.inf, -math.inf):
                    args = [20, 0.0, 3.0, 1.0, 0.1]
                    args[slot] = bad
                    with self.subTest(module=module.__name__, slot=slot, bad=bad):
                        with self.assertRaises(module.PowerSupportError):
                            module.min_passing_sharpe(*args)

    def test_count_and_moment_domain_are_explicit(self):
        for module in (joint, curve):
            for n in (-1, 0, 1, 2.5):
                with self.assertRaises(module.PowerSupportError):
                    module.min_passing_sharpe(n, 0.0, 3.0, 1.0, 0.0)
            for skew, kurt in ((0.0, 0.0), (2.0, 3.0)):
                with self.assertRaises(module.PowerSupportError):
                    module.min_passing_sharpe(20, skew, kurt, 1.0, 0.0)
            with self.assertRaises(module.PowerSupportError):
                module.min_passing_sharpe(20, [0.0, 0.0], [3.0, 3.0, 3.0], 1.0, 0.0)

    def test_float_overflow_and_underflow_do_not_return_nan_thresholds(self):
        for module in (joint, curve):
            for z, benchmark in ((1e200, 0.0), (1e-200, 0.0), (1.0, 1e200), (1e-10, 1e10)):
                with self.assertRaises(module.PowerSupportError):
                    module.min_passing_sharpe(20, 0.0, 3.0, z, benchmark)
            with self.assertRaises(module.PowerSupportError):
                module.min_passing_sharpe(20, 2.0, 5.0, 1e-10, 1.0)

    def test_two_point_moments_use_the_kernel_floor_at_the_double_root(self):
        # skew=2, kurt=5 are valid two-point moments: V(u)=(u-1)^2.
        # With benchmark 1 the squared equation has a double root there,
        # but the kernel's positive floor makes the actual threshold displaced.
        for module in (joint, curve):
            for z in (-1.0, 1.0):
                got = float(module.min_passing_sharpe(20, 2.0, 5.0, z, 1.0))
                expected = 1.0 + z * math.sqrt(kernel.PSR_RADICAND_FLOOR / 19.0)
                self.assertEqual(got, expected)
                self.assertLess(kernel.psr_from_moments(got - 1e-9, 20, 1.0, 2.0, 5.0), kernel.norm_cdf(z))
                self.assertGreater(kernel.psr_from_moments(got + 1e-9, 20, 1.0, 2.0, 5.0), kernel.norm_cdf(z))

    def test_positive_z_ordinary_roots_are_bit_identical_to_legacy_formula(self):
        # No numerical evidence is regenerated: these are fixed synthetic inputs.
        for module in (joint, curve):
            for n, skew, kurt, z, benchmark in ((408, 0.0, 3.0, 1.28155, 0.0), (77, -0.7, 4.0, 1.64485, 0.2)):
                a = (kurt - 1.0) / 4.0
                aa = n - 1.0 - z * z * a
                bb = z * z * skew - 2.0 * benchmark * (n - 1.0)
                cc = benchmark * benchmark * (n - 1.0) - z * z
                root = np.sqrt(bb * bb - 4.0 * aa * cc)
                q = -0.5 * (bb + (root if bb >= 0.0 else -root))
                self.assertEqual(float(module.min_passing_sharpe(n, skew, kurt, z, benchmark)), max(q / aa, cc / q))

    def test_signed_normal_boundary_and_broadcasting_match_the_kernel(self):
        for module in (joint, curve):
            for z in (-1.0, 1.0):
                got = module.min_passing_sharpe(20, np.zeros((2, 1)), np.full((1, 3), 3.0), z, 0.0)
                self.assertEqual(got.shape, (2, 3))
                expected = z / math.sqrt(19.0 - z * z / 2.0)
                np.testing.assert_allclose(got, expected, rtol=1e-15)
                self.assertAlmostEqual(kernel.psr_from_moments(float(got[0, 0]), 20, 0.0), kernel.norm_cdf(z), places=13)


if __name__ == "__main__":
    unittest.main()
