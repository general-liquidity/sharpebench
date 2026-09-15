"""Regressions for make-power-curve.py, the power of the default gates.

The producer's shortcut is that a gate passes at true Sharpe s exactly when s
reaches a threshold solved in closed form. Every case that checks the shortcut
compares it with the kernel's PSR evaluated on the actual shifted series through
kernel_stats, so a wrong root, a wrong moment normalization or a wrong degrees-
of-freedom factor shows up as a disagreement. Subprocess cases run on staged
copies in a temporary directory and never write a committed file.
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
PRODUCER = HERE / "make-power-curve.py"
EVIDENCE = ROOT / "paper/evidence/final"
FRAGMENT = ROOT / "paper/sections/power-fragment.tex"


def load(name, path):
    spec = importlib.util.spec_from_file_location(name, path)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


pc = load("make_power_curve", PRODUCER)
k = pc.kernel_stats


def kernel_passes(series, benchmark, bar):
    return k.probabilistic_sharpe_ratio([float(x) for x in series], benchmark) >= bar


class ClosedFormThreshold(unittest.TestCase):
    def test_threshold_is_the_kernel_psr_boundary_over_moments(self):
        rng = np.random.default_rng(7)
        for _ in range(300):
            n = int(rng.integers(20, 3000))
            skew = float(rng.uniform(-2.0, 2.0))
            kurt = float(1.0 + skew * skew + rng.uniform(0.0, 8.0))
            benchmark = float(rng.uniform(0.0, 0.5))
            for bar in (0.90, 0.95):
                z = k.norm_cdf_inverse(bar)
                u = float(pc.min_passing_sharpe(n, skew, kurt, z, benchmark))
                with self.subTest(n=n, skew=skew, kurt=kurt, benchmark=benchmark, bar=bar):
                    self.assertGreater(u, benchmark)
                    self.assertGreaterEqual(
                        k.psr_from_moments(u * (1 + 1e-9) + 1e-12, n, benchmark, skew, kurt), bar
                    )
                    self.assertLess(
                        k.psr_from_moments(u * (1 - 1e-7), n, benchmark, skew, kurt), bar
                    )

    def test_zero_benchmark_normal_moments_reduce_to_the_textbook_bound(self):
        z = k.norm_cdf_inverse(0.90)
        for n in (46, 77, 408, 3990):
            u = float(pc.min_passing_sharpe(n, 0.0, 3.0, z, 0.0))
            self.assertAlmostEqual(u, z / math.sqrt(n - 1 - z * z / 2), places=12)

    def test_threshold_true_sharpe_matches_the_kernel_on_real_series(self):
        rng = np.random.default_rng(11)
        z_bar = k.norm_cdf_inverse(0.90)
        noise = rng.standard_normal((40, 77))
        # A heavy-tailed, skewed draw exercises the moment terms harder.
        noise[20:] = rng.standard_exponential((20, 77)) ** 1.5
        thresholds = pc.threshold_true_sharpe(noise, z_bar, 0.0)
        for series, s in zip(noise, thresholds):
            scale = abs(s) + 1.0
            self.assertTrue(kernel_passes(s + 1e-9 * scale + series, 0.0, 0.90))
            self.assertFalse(kernel_passes(s - 1e-7 * scale + series, 0.0, 0.90))

    def test_the_passk_leg_is_the_maximum_window_threshold(self):
        """All six windows pass at s exactly when s reaches the largest of the
        six thresholds, checked on every replication at two Sharpe levels."""
        rng = np.random.default_rng(3)
        z_bar = k.norm_cdf_inverse(0.90)
        noise = rng.standard_normal((150, pc.N_WINDOWS, 77))
        leg = pc.threshold_true_sharpe(noise, z_bar, 0.0).max(axis=1)
        for annualized in (1.5, 2.5):
            s = annualized / math.sqrt(52.0)
            for rep in range(noise.shape[0]):
                if abs(s - leg[rep]) < 1e-9:
                    continue
                every = all(kernel_passes(s + w, 0.0, 0.90) for w in noise[rep])
                self.assertEqual(every, s >= leg[rep], (annualized, rep))

    def test_the_dsr_leg_matches_the_kernel_on_the_pooled_track(self):
        rng = np.random.default_rng(5)
        z_bar = k.norm_cdf_inverse(0.95)
        bar = 0.15783322160605548
        pooled = rng.standard_normal((60, pc.N_WINDOWS * 78))
        thresholds = pc.threshold_true_sharpe(pooled, z_bar, bar)
        for series, s in zip(pooled, thresholds):
            self.assertTrue(kernel_passes(s + 1e-9 + series, bar, 0.95))
            self.assertFalse(kernel_passes(s - 1e-7 + series, bar, 0.95))

    def test_a_simulated_chunk_agrees_with_the_kernel_on_its_own_draws(self):
        """End to end: regenerate a chunk's noise from its documented seed and
        check both legs' thresholds against the kernel on those very series."""
        window_len, runs, bar, reps = 30, 1, 0.1, 6
        passk, dsr = pc.simulate_chunk((window_len, runs, [bar], 0, reps))
        seq = np.random.SeedSequence(
            entropy=pc.SEED, spawn_key=(pc.N_WINDOWS, window_len, runs, 0)
        )
        rng = np.random.Generator(np.random.PCG64(seq))
        noise = rng.standard_normal((reps, pc.N_WINDOWS * runs, window_len))
        for rep in range(reps):
            for s, expect in ((passk[rep] + 1e-9, True), (passk[rep] - 1e-7, False)):
                every = all(kernel_passes(s + w, 0.0, 0.90) for w in noise[rep])
                self.assertEqual(every, expect, rep)
            pooled = noise[rep].reshape(-1)
            self.assertTrue(kernel_passes(dsr[0][rep] + 1e-9 + pooled, bar, 0.95))
            self.assertFalse(kernel_passes(dsr[0][rep] - 1e-7 + pooled, bar, 0.95))

    def test_an_infeasible_kurtosis_is_refused_rather_than_solved(self):
        with self.assertRaises(pc.PowerSupportError):
            pc.min_passing_sharpe(20, 0.0, 400.0, k.norm_cdf_inverse(0.9), 0.0)


class Reductions(unittest.TestCase):
    def test_crossing_is_the_smallest_sharpe_reaching_the_probability(self):
        values = np.arange(1, 101, dtype=float)
        at, lo, hi = pc.crossing(values, 5)
        self.assertEqual(at, 5.0)
        self.assertLessEqual(lo, at)
        self.assertGreaterEqual(hi, at)
        self.assertEqual(pc.crossing(values, 95)[0], 95.0)
        self.assertEqual(pc.crossing(values, 50)[0], 50.0)

    def test_pass_probability_counts_thresholds_at_or_below(self):
        values = np.array([1.0, 2.0, 2.0, 3.0])
        self.assertEqual(pc.pass_probability(values, 2.0)[0], 0.75)
        self.assertEqual(pc.pass_probability(values, 0.5)[0], 0.0)
        self.assertEqual(pc.pass_probability(values, 3.0)[0], 1.0)

    def test_dsr_minimum_is_where_the_normal_moment_dsr_reaches_the_bar(self):
        panel = pc.panel_bar(str(EVIDENCE), "us-indices-1d")
        u, u_ann = pc.dsr_min_admissible(panel)
        n, b = panel["pooled_observations"], panel["deflation_bar_per_period"]
        self.assertGreaterEqual(k.psr_from_moments(u * (1 + 1e-9), n, b), 0.95)
        self.assertLess(k.psr_from_moments(u * (1 - 1e-7), n, b), 0.95)
        self.assertAlmostEqual(u_ann, u * math.sqrt(252.0), places=12)
        self.assertGreater(u_ann, panel["deflation_bar_annualized_equivalent"])


class ProducerRuns(unittest.TestCase):
    REPS = 256

    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.dir = Path(self.tmp.name)
        self.sweep = self.dir / "sweep"
        self.sweep.mkdir()
        for dataset in pc.PANELS:
            shutil.copy(EVIDENCE / f"{dataset}.jsonl", self.sweep / f"{dataset}.jsonl")

    def run_producer(self, *args):
        return subprocess.run(
            [sys.executable, str(PRODUCER), *args],
            capture_output=True, text=True, timeout=600,
        )

    def compute(self, name, *extra):
        out = self.dir / name
        proc = self.run_producer(
            "compute", "--replications", str(self.REPS), "--sweep-dir", str(self.sweep),
            "--evidence", str(out), *extra,
        )
        self.assertEqual(proc.returncode, 0, proc.stderr)
        return out

    def test_output_does_not_depend_on_worker_count_or_run(self):
        one = self.compute("one.jsonl", "--jobs", "1").read_bytes()
        again = self.compute("again.jsonl", "--jobs", "1").read_bytes()
        two = self.compute("two.jsonl", "--jobs", "2").read_bytes()
        self.assertEqual(one, again)
        self.assertEqual(one, two)

    def test_a_shared_geometry_is_drawn_once(self):
        """us-indices-1d is the daily curve's geometry, and fx-majors-1d and
        rates-1d share one geometry: their pass^k legs must be identical."""
        rows = [json.loads(line) for line in self.compute("shared.jsonl").read_text().splitlines()]
        curve = next(r for r in rows if r["record"] == "curve_summary" and r["geometry"] == "daily")
        panels = {r["dataset"]: r for r in rows if r["record"] == "panel"}
        for key in ("sharpe_at_5pct", "sharpe_at_95pct", "pass_probability_at_2p0"):
            self.assertEqual(panels["us-indices-1d"]["legs"]["pass_k"][key], curve[key])
        self.assertEqual(panels["fx-majors-1d"]["legs"]["pass_k"], panels["rates-1d"]["legs"]["pass_k"])
        self.assertNotEqual(panels["fx-majors-1d"]["legs"]["dsr"], panels["rates-1d"]["legs"]["dsr"])

    def test_both_legs_are_at_least_as_demanding_as_either(self):
        rows = [json.loads(line) for line in self.compute("both.jsonl").read_text().splitlines()]
        for panel in (r for r in rows if r["record"] == "panel"):
            legs = panel["legs"]
            for percent in pc.CROSSINGS:
                key = f"sharpe_at_{percent}pct"
                self.assertGreaterEqual(legs["both"][key], legs["pass_k"][key])
                self.assertGreaterEqual(legs["both"][key], legs["dsr"][key])

    def rewrite(self, dataset, mutate):
        path = self.sweep / f"{dataset}.jsonl"
        rows = [json.loads(line) for line in path.read_text().splitlines() if line.strip()]
        rows = mutate(rows)
        path.write_text("".join(json.dumps(r) + "\n" for r in rows))

    def assert_refused(self, reason):
        out = self.dir / "refused.jsonl"
        proc = self.run_producer(
            "compute", "--replications", str(self.REPS), "--sweep-dir", str(self.sweep),
            "--evidence", str(out),
        )
        self.assertNotEqual(proc.returncode, 0, proc.stdout)
        self.assertIn(reason, proc.stderr)
        self.assertNotIn("Traceback", proc.stderr)
        self.assertFalse(out.exists())

    def default_cell(self, row):
        return all(row.get(key) == value for key, value in pc.DEFAULT_CELL.items())

    def test_two_bars_in_one_default_cell_are_refused(self):
        def mutate(rows):
            first = next(r for r in rows if self.default_cell(r))
            first["deflation_bar_per_period"] *= 1.5
            return rows
        self.rewrite("rates-1d", mutate)
        self.assert_refused("rates-1d: the default cell carries 2 distinct bars")

    def test_a_panel_without_a_default_cell_is_refused(self):
        self.rewrite("fx-majors-1d", lambda rows: [r for r in rows if not self.default_cell(r)])
        self.assert_refused("no default-cell records for fx-majors-1d")

    def test_a_missing_panel_is_refused(self):
        (self.sweep / "crypto-majors-1h.jsonl").unlink()
        self.assert_refused("missing evidence file")

    def test_a_pooled_length_that_is_not_the_window_geometry_is_refused(self):
        def mutate(rows):
            for r in rows:
                r["pooled_observations"] += 1
            return rows
        self.rewrite("us-indices-1w", mutate)
        self.assert_refused("us-indices-1w: pooled length is not windows times window length")

    def test_the_figure_refuses_evidence_without_both_curves(self):
        out = self.compute("fig.jsonl")
        rows = [json.loads(line) for line in out.read_text().splitlines()]
        kept = [r for r in rows if r.get("geometry") != "weekly"]
        out.write_text("".join(json.dumps(r) + "\n" for r in kept))
        pdf = self.dir / "power.pdf"
        proc = self.run_producer("figure", "--evidence", str(out), "--pdf", str(pdf))
        self.assertNotEqual(proc.returncode, 0)
        self.assertIn("no complete weekly curve", proc.stderr)
        self.assertFalse(pdf.exists())

    def test_the_figure_is_reproducible_and_embeds_no_type3_font(self):
        out = self.compute("fig2.jsonl")
        first, second = self.dir / "a.pdf", self.dir / "b.pdf"
        for pdf in (first, second):
            proc = self.run_producer("figure", "--evidence", str(out), "--pdf", str(pdf))
            self.assertEqual(proc.returncode, 0, proc.stderr)
        data = first.read_bytes()
        self.assertEqual(data, second.read_bytes())
        self.assertNotIn(b"/Type3", data)
        self.assertNotIn(b"/CreationDate", data)


class CommittedEvidence(unittest.TestCase):
    """The committed JSONL and the fragment's printed numbers agree."""

    @classmethod
    def setUpClass(cls):
        path = EVIDENCE / "power-curve.jsonl"
        cls.rows = [json.loads(line) for line in path.read_text(encoding="utf-8").splitlines()]
        cls.fragment = FRAGMENT.read_text(encoding="utf-8")

    def test_meta_records_the_run(self):
        meta = [r for r in self.rows if r["record"] == "meta"]
        self.assertEqual(len(meta), 1)
        self.assertEqual(meta[0]["seed"], pc.SEED)
        self.assertEqual(meta[0]["replications"], pc.REPLICATIONS)
        self.assertEqual(meta[0]["kind"], "protocol_property")

    def test_every_table_row_matches_the_panel_record(self):
        panels = {r["dataset"]: r for r in self.rows if r["record"] == "panel"}
        body = self.fragment.split("\\label{tab:dsr-mde}")[1].split("\\bottomrule")[0]
        body = body.split("\\midrule")[1]
        printed = [line for line in body.splitlines() if line.strip().endswith("\\\\")]
        self.assertEqual(len(printed), len(pc.PANELS))
        for dataset, line in zip(pc.PANELS, printed):
            p = panels[dataset]
            legs = p["legs"]
            expected = [
                str(p["window_len"]),
                f"{p['deflation_bar_annualized_equivalent']:.4f}",
                f"{p['dsr_min_admissible_sharpe_annualized']:.2f}",
                f"{legs['dsr']['sharpe_at_95pct']:.2f}",
                f"{legs['pass_k']['sharpe_at_95pct']:.2f}",
                f"{legs['both']['sharpe_at_5pct']:.2f}",
                f"{legs['both']['sharpe_at_95pct']:.2f}",
            ]
            cells = [c.strip() for c in line.rstrip("\\ ").split("&")][1:]
            with self.subTest(dataset=dataset):
                self.assertEqual(cells, expected)

    # Decimals the prose may state that are not read from the evidence: the
    # gate bars, the shipped prior, the MC error bound and closeness margins the
    # text quotes as bounds, and the two measured wall times of the committed run.
    DECLARED = {"0.90", "0.95", "0.5", "0.01", "0.02", "0.0012", "313.2", "53.6"}

    def evidence_numbers(self):
        """Every decimal the prose is allowed to print, as it would print it."""
        curves = {r["geometry"]: r for r in self.rows if r["record"] == "curve_summary"}
        witness = {
            r["geometry"]: r for r in self.rows if r["record"] == "witness_construction_summary"
        }
        meta = next(r for r in self.rows if r["record"] == "meta")
        allowed = set(self.DECLARED) | {f"{meta['dsr_z_threshold']:.4f}"}
        for c in curves.values():
            allowed |= {
                f"{c['sharpe_at_5pct']:.2f}",
                f"{c['sharpe_at_95pct']:.2f}",
                f"{c['sharpe_at_95pct']:.1f}",
                f"{100 * c['pass_probability_at_1p0']:.1f}",
                f"{100 * c['pass_probability_at_2p0']:.1f}",
                f"{c['sharpe_at_95pct'] - c['sharpe_at_5pct']:.2f}",
            }
        for w in witness.values():
            allowed |= {f"{w['sharpe_at_5pct']:.2f}", f"{w['sharpe_at_95pct']:.2f}"}
        for p in (r for r in self.rows if r["record"] == "panel"):
            legs = p["legs"]
            allowed |= {
                f"{p['deflation_bar_annualized_equivalent']:.4f}",
                f"{p['dsr_min_admissible_sharpe_annualized']:.2f}",
                f"{p['dsr_min_admissible_sharpe_annualized'] - p['deflation_bar_annualized_equivalent']:.2f}",
                f"{legs['dsr']['sharpe_at_95pct']:.2f}",
                f"{legs['pass_k']['sharpe_at_95pct']:.2f}",
                f"{legs['both']['sharpe_at_5pct']:.2f}",
                f"{legs['both']['sharpe_at_95pct']:.2f}",
            }
        return allowed, curves, witness

    def prose(self):
        text = self.fragment.split("\\begin{figure}")[0]
        return "\n".join(line for line in text.splitlines() if not line.startswith("%"))

    def test_every_prose_decimal_is_read_from_the_evidence(self):
        """Both directions: a stated number must come from the evidence, so a
        drifted value is caught even when the correct one appears elsewhere."""
        allowed, _, _ = self.evidence_numbers()
        stray = sorted(set(re.findall(r"\d+\.\d+", self.prose())) - allowed)
        self.assertEqual(stray, [])

    def test_the_headline_crossings_are_stated_where_they_are_claimed(self):
        _, curves, witness = self.evidence_numbers()
        text = self.prose()
        d, w = curves["daily"], curves["weekly"]
        self.assertIn(
            f"It reaches 5 percent at ${d['sharpe_at_5pct']:.2f}$ and 95 percent at "
            f"${d['sharpe_at_95pct']:.2f}$.", text
        )
        self.assertIn(
            f"the same four values are ${100 * w['pass_probability_at_1p0']:.1f}$ percent, "
            f"${100 * w['pass_probability_at_2p0']:.1f}$ percent, ${w['sharpe_at_5pct']:.2f}$ "
            f"and ${w['sharpe_at_95pct']:.2f}$.", text
        )
        wd, ww = witness["witness-daily"], witness["witness-weekly"]
        self.assertIn(
            f"at ${wd['sharpe_at_5pct']:.2f}$ and ${wd['sharpe_at_95pct']:.2f}$ on the "
            "witness's daily geometry", text
        )
        self.assertIn(
            f"at ${ww['sharpe_at_5pct']:.2f}$ and ${ww['sharpe_at_95pct']:.2f}$ on its "
            "weekly geometry", text
        )


if __name__ == "__main__":
    unittest.main()
