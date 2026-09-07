"""Synthetic shard mutations; never run the sweep or rewrite frozen evidence."""

from __future__ import annotations

import copy
import itertools
import json
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
EVIDENCE = ROOT / "paper/evidence"
BARS = (0.8, 0.9, 0.95, 0.99)
AGENTS = ("buy-and-hold", "momentum", "hold") + tuple(
    f"luck-floor-{i:02}" for i in range(5)
)


def synthetic_grid():
    # Use the historical record's output shape, not its outcomes as new evidence.
    first = (EVIDENCE / "final/us-indices-1d.jsonl").read_text().splitlines()[0]
    template = json.loads(first)
    records = []
    for bar, trials, pinned, agent in itertools.product(
        BARS, (1, 10, 50, 200), (None, 0.2, 0.35, 0.5), AGENTS
    ):
        row = copy.deepcopy(template)
        row.update(dsr_bar=bar, n_trials=trials, sr_std_pinned=pinned, agent_id=agent)
        records.append(row)
    return records


class SweepGridTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.root = Path(self.tmp.name)
        self.rows = synthetic_grid()

    def assemble(self, rows, *, extra_line=None):
        parts = []
        for i, bar in enumerate(BARS):
            path = self.root / f"bar-{i}.jsonl"
            lines = [json.dumps(r) for r in rows if r["dsr_bar"] == bar]
            if extra_line is not None and i == 0:
                lines[0] = extra_line
            path.write_text("\n".join(lines) + "\n", encoding="utf-8")
            parts.append(str(path))
        target = self.root / "assembled.jsonl"
        target.write_bytes(b"preserve existing output\n")
        proc = subprocess.run(
            [sys.executable, str(EVIDENCE / "assemble_sweep.py"), str(target), *parts],
            capture_output=True,
            text=True,
            timeout=10,
        )
        return proc, target

    def assert_refused(self, rows, reason, **kwargs):
        proc, target = self.assemble(rows, **kwargs)
        self.assertNotEqual(proc.returncode, 0, proc.stdout)
        self.assertIn(reason, proc.stderr)
        self.assertEqual(target.read_bytes(), b"preserve existing output\n")

    def test_valid_grid_accepts_and_keeps_lf(self):
        proc, target = self.assemble(self.rows)
        self.assertEqual(proc.returncode, 0, proc.stderr)
        self.assertEqual(len(target.read_bytes().splitlines()), 512)
        self.assertNotIn(b"\r", target.read_bytes())

    def test_128_copies_are_not_a_complete_shard(self):
        rows = [copy.deepcopy(self.rows[i * 128]) for i in range(4) for _ in range(128)]
        self.assert_refused(rows, "duplicate cell")

    def test_duplicate_replacing_one_cell_is_refused(self):
        self.rows[1] = copy.deepcopy(self.rows[0])
        self.assert_refused(self.rows, "duplicate cell")

    def test_missing_cell_is_refused(self):
        self.assert_refused(self.rows[1:], "missing")

    def test_unknown_and_coerced_keys_are_refused(self):
        for field, value in (
            ("agent_id", "replacement-agent"),
            ("n_trials", 2),
            ("n_trials", True),
            ("n_trials", 1.0),
            ("sr_std_pinned", False),
            ("sr_std_pinned", 0.25),
        ):
            with self.subTest(field=field, value=value):
                rows = copy.deepcopy(self.rows)
                rows[0][field] = value
                self.assert_refused(rows, "invalid grid key")

    def test_dataset_and_geometry_must_agree(self):
        for field, value in (
            ("dataset", "rates-1d"),
            ("periods_per_year", 365),
            ("asset_class", "crypto"),
            ("n_seeds", 7),
            ("n_bars", 5000),
            ("n_windows", 7),
            ("regimes", ["Bull"] * 6),
        ):
            with self.subTest(field=field):
                rows = copy.deepcopy(self.rows)
                rows[1][field] = value
                self.assert_refused(rows, "metadata")

    def test_geometry_cannot_change_between_valid_shards(self):
        for row in self.rows[128:256]:
            row["n_bars"] += 10
        self.assert_refused(self.rows, "metadata")

    def test_effective_configuration_cannot_change_between_entrants(self):
        for field, value in (
            ("effective_n_trials", 99),
            ("min_field_for_measured_sr_std", 99),
            ("dedup_clones_for_measured_sr_std", False),
            ("min_measured_trials_sr_std_annualized", 0.4),
            ("deflation_null_mean_per_period", 0.5),
        ):
            with self.subTest(field=field):
                rows = copy.deepcopy(self.rows)
                rows[1][field] = value
                self.assert_refused(rows, "metadata")

    def test_required_renderer_fields_are_checked_before_output(self):
        for field, value in (
            ("deflated_sharpe", None),
            ("passed_k", "false"),
            ("rank_eligible", 1),
            ("pooled_observations", True),
            ("trials_sr_std_source", "invented"),
            ("raw_mean_return", 10**400),
        ):
            with self.subTest(field=field):
                rows = copy.deepcopy(self.rows)
                rows[0][field] = value
                self.assert_refused(rows, "invalid renderer field")
        del self.rows[0]["deflated_sharpe"]
        self.assert_refused(self.rows, "invalid renderer field")

    def test_missing_null_prior_field_is_not_the_null_prior(self):
        del self.rows[0]["sr_std_pinned"]
        self.assert_refused(self.rows, "invalid grid key")

    def test_nonfinite_output_and_duplicate_json_keys_are_refused(self):
        line = json.dumps(self.rows[0])
        for replacement in (
            line[:-1] + ', "extra": NaN}',
            line[:-1] + ', "extra": 1e999}',
            line[:-1] + ', "n_trials": 1}',
        ):
            with self.subTest(line=replacement[-35:]):
                self.assert_refused(self.rows, "invalid JSON", extra_line=replacement)

    def test_shuffling_input_has_canonical_output(self):
        normal, target = self.assemble(self.rows)
        self.assertEqual(normal.returncode, 0, normal.stderr)
        expected = target.read_bytes()
        shuffled, target = self.assemble(list(reversed(self.rows)))
        self.assertEqual(shuffled.returncode, 0, shuffled.stderr)
        self.assertEqual(target.read_bytes(), expected)

    def test_unicode_separator_inside_json_string_is_not_a_record_boundary(self):
        self.rows[0]["note"] = "a\u2028b"
        line = json.dumps(self.rows[0], ensure_ascii=False)
        proc, target = self.assemble(self.rows, extra_line=line)
        self.assertEqual(proc.returncode, 0, proc.stderr)
        self.assertIn("a\u2028b".encode(), target.read_bytes())
        self.assertEqual(len(target.read_bytes().split(b"\n")), 513)

    def test_reducer_refuses_duplicate_grid_before_any_table_output(self):
        rows = copy.deepcopy(self.rows)
        rows[1] = copy.deepcopy(rows[0])
        (self.root / "us-indices-1d.jsonl").write_text(
            "\n".join(json.dumps(r) for r in rows) + "\n", encoding="utf-8"
        )
        proc = subprocess.run(
            [sys.executable, str(EVIDENCE / "analyze.py"), str(self.root)],
            capture_output=True,
            text=True,
            timeout=10,
        )
        self.assertNotEqual(proc.returncode, 0)
        self.assertIn("duplicate cell", proc.stderr)
        self.assertEqual(proc.stdout, "")

    def test_reducer_refuses_empty_directory(self):
        proc = subprocess.run(
            [sys.executable, str(EVIDENCE / "analyze.py"), str(self.root)],
            capture_output=True,
            text=True,
            timeout=10,
        )
        self.assertNotEqual(proc.returncode, 0)
        self.assertIn("missing dataset", proc.stderr)
        self.assertEqual(proc.stdout, "")

    def test_committed_principal_sweeps_have_the_declared_support(self):
        # This checks archived structure only, not fresh-engine numerical parity.
        sys.path.insert(0, str(EVIDENCE))
        self.addCleanup(sys.path.remove, str(EVIDENCE))
        from sweep_grid import DATASETS, read_records, validate_grid

        self.assertEqual(len(DATASETS), 9)
        for dataset in DATASETS:
            with self.subTest(dataset=dataset):
                rows = read_records(EVIDENCE / "final" / f"{dataset}.jsonl")
                self.assertEqual(len(validate_grid(rows, dataset)), 512)


if __name__ == "__main__":
    unittest.main()
