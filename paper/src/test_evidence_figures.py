"""Figure-support regressions: staged copies only, never the committed figures.

Every subprocess case runs a copy of the producer inside a temporary paper tree
whose evidence is a mutated copy, so no committed record or PDF is read into a
new artifact or overwritten.
"""

from __future__ import annotations

import importlib.util
import json
import shutil
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
# Records are separated by a bare newline, never by the platform separator: the
# cases below stage byte-level damage and must not depend on the host.
NL = chr(10)
EVIDENCE = ROOT / "paper/evidence/final"
PRODUCER = ROOT / "paper/src/make-evidence-figures.py"

SWEEPS = [
    "us-indices-1w",
    "us-indices-1d",
    "crypto-majors-1w",
    "crypto-majors-1d",
    "crypto-majors-4h",
    "crypto-majors-1h",
    "fx-majors-1d",
    "commodities-1d",
    "rates-1d",
]
INPUTS = {
    "drawdowns": SWEEPS + ["risk-managed"],
    "luck-deflation": ["crypto-majors-1w", "rates-1d", "us-indices-1w"],
    "pass-witness": ["pass-witness"],
    "luck-floor-1000": ["luck-floor-1000"],
}
OUTPUTS = {
    "drawdowns": "evidence-drawdowns.pdf",
    "luck-deflation": "evidence-luck-deflation.pdf",
    "pass-witness": "evidence-pass-witness.pdf",
    "luck-floor-1000": "evidence-luck-floor-1000.pdf",
}


def module():
    spec = importlib.util.spec_from_file_location("make_evidence_figures", PRODUCER)
    loaded = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(loaded)
    return loaded


class FigureSupportTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.paper = Path(self.tmp.name) / "paper"
        (self.paper / "src").mkdir(parents=True)
        (self.paper / "evidence/final").mkdir(parents=True)
        (self.paper / "figures").mkdir()
        shutil.copy(PRODUCER, self.paper / "src" / PRODUCER.name)

    def stage(self, figure):
        for name in INPUTS[figure]:
            shutil.copy(
                EVIDENCE / f"{name}.jsonl", self.paper / "evidence/final" / f"{name}.jsonl"
            )

    def path(self, name):
        return self.paper / "evidence/final" / f"{name}.jsonl"

    def records(self, name):
        text = self.path(name).read_text(encoding="utf-8")
        return [json.loads(line) for line in text.splitlines() if line.strip()]

    def rewrite(self, name, rows):
        self.path(name).write_text(
            "".join(json.dumps(r) + "\n" for r in rows), encoding="utf-8"
        )

    def run_figure(self, figure):
        return subprocess.run(
            [sys.executable, str(self.paper / "src" / PRODUCER.name), figure],
            capture_output=True,
            text=True,
            timeout=300,
        )

    def assert_refused(self, figure, reason):
        proc = self.run_figure(figure)
        self.assertNotEqual(proc.returncode, 0, proc.stdout)
        self.assertIn(reason, proc.stderr)
        self.assertNotIn("Traceback", proc.stderr)
        self.assertFalse((self.paper / "figures" / OUTPUTS[figure]).exists())

    def test_every_figure_builds_from_the_committed_records(self):
        for figure in INPUTS:
            with self.subTest(figure=figure):
                self.stage(figure)
                proc = self.run_figure(figure)
                self.assertEqual(proc.returncode, 0, proc.stderr)
                self.assertTrue((self.paper / "figures" / OUTPUTS[figure]).exists())

    def test_missing_dataset_file_is_refused(self):
        self.stage("drawdowns")
        self.path("rates-1d").unlink()
        self.assert_refused("drawdowns", "missing evidence file")

    def test_empty_dataset_file_is_refused(self):
        self.stage("drawdowns")
        self.path("rates-1d").write_text("", encoding="utf-8")
        self.assert_refused("drawdowns", "no records in")

    def test_absent_risk_managed_cell_is_not_a_blank_bar(self):
        self.stage("drawdowns")
        rows = [
            r
            for r in self.records("risk-managed")
            if not (r["dataset"] == "rates-1d" and r["agent_id"] == "risk-managed")
        ]
        self.rewrite("risk-managed", rows)
        self.assert_refused("drawdowns", "the rates-1d risk-managed default cell")

    def test_duplicated_default_cell_is_refused(self):
        self.stage("drawdowns")
        rows = self.records("risk-managed")
        rows += [
            r
            for r in rows
            if r["dataset"] == "rates-1d" and r["agent_id"] == "risk-managed"
        ]
        self.rewrite("risk-managed", rows)
        self.assert_refused("drawdowns", "expected one")

    def test_selection_matching_no_records_is_refused(self):
        self.stage("luck-deflation")
        rows = self.records("rates-1d")
        for row in rows:
            row["dsr_bar"] = 0.99
        self.rewrite("rates-1d", rows)
        self.assert_refused("luck-deflation", "rates-1d at the 0.95 bar")

    def test_missing_witness_shape_is_refused(self):
        self.stage("pass-witness")
        rows = [r for r in self.records("pass-witness") if r["shape"] != "daily-shaped"]
        self.rewrite("pass-witness", rows)
        self.assert_refused("pass-witness", "witness records of shape daily-shaped")

    def test_luck_floor_dataset_without_agent_records_is_refused(self):
        self.stage("luck-floor-1000")
        rows = [
            r
            for r in self.records("luck-floor-1000")
            if not (r["record"] == "agent" and r["dataset"] == "crypto-majors-1d")
        ]
        self.rewrite("luck-floor-1000", rows)
        self.assert_refused(
            "luck-floor-1000", "luck-floor-1000 agent records for crypto-majors-1d"
        )

    def test_luck_floor_dataset_without_a_summary_is_refused(self):
        self.stage("luck-floor-1000")
        rows = [
            r
            for r in self.records("luck-floor-1000")
            if not (r["record"] == "summary" and r["dataset"] == "crypto-majors-1d")
        ]
        self.rewrite("luck-floor-1000", rows)
        self.assert_refused("luck-floor-1000", "no summary record for crypto-majors-1d")

    def test_truncated_record_is_refused_not_dropped(self):
        """A record cut off mid-write used to be skipped by the `endswith("}")`
        line filter, so a damaged file rendered as a smaller normal-looking
        figure."""
        self.stage("luck-floor-1000")
        text = self.path("luck-floor-1000").read_text(encoding="utf-8")
        lines = text.splitlines()
        lines[3] = lines[3][: len(lines[3]) // 2]
        self.path("luck-floor-1000").write_text(
            NL.join(lines) + NL, encoding="utf-8"
        )
        self.assert_refused("luck-floor-1000", "not a JSON record")

    def test_trailing_garbage_after_a_record_is_refused(self):
        """The old filter accepted any line ending in "}", so a concatenated or
        doubled record passed the check and then failed inside json.loads, or
        worse, parsed as something else."""
        self.stage("drawdowns")
        text = self.path("rates-1d").read_text(encoding="utf-8")
        lines = text.splitlines()
        lines[0] = lines[0] + " {}"
        self.path("rates-1d").write_text(NL.join(lines) + NL, encoding="utf-8")
        self.assert_refused("drawdowns", "not a JSON record")

    def test_a_bare_json_value_is_not_accepted_as_a_record(self):
        """A line that parses but is not an object is not a record. It would
        otherwise reach the plotting code and fail on a key lookup, or silently
        count toward a support total."""
        self.stage("drawdowns")
        text = self.path("rates-1d").read_text(encoding="utf-8")
        self.path("rates-1d").write_text("[1, 2]" + NL + text, encoding="utf-8")
        self.assert_refused("drawdowns", "not an object")

    def test_blank_lines_remain_acceptable(self):
        """Strictness is about records, not whitespace: a trailing or interior
        blank line is not a damaged record and must not refuse a good file."""
        self.stage("luck-floor-1000")
        text = self.path("luck-floor-1000").read_text(encoding="utf-8")
        self.path("luck-floor-1000").write_text(
            NL + text + NL + NL, encoding="utf-8"
        )
        proc = self.run_figure("luck-floor-1000")
        self.assertEqual(proc.returncode, 0, proc.stderr)

    def test_dropped_agent_rows_do_not_renormalize_onto_the_stated_field_size(self):
        """The ECDF divided by however many rows survived while the axis kept
        asserting 1,000 agents, so a file missing rows plotted as a complete
        field."""
        self.stage("luck-floor-1000")
        rows = self.records("luck-floor-1000")
        dropped = 0
        kept = []
        for row in rows:
            if (
                row["record"] == "agent"
                and row["dataset"] == "us-indices-1d"
                and dropped < 5
            ):
                dropped += 1
                continue
            kept.append(row)
        self.assertEqual(dropped, 5)
        self.rewrite("luck-floor-1000", kept)
        self.assert_refused("luck-floor-1000", "agent records and its summary declares")

    def test_summary_field_size_disagreeing_across_datasets_is_refused(self):
        """One axis label describes both curves, so the two datasets must agree
        on the field size before either is drawn."""
        self.stage("luck-floor-1000")
        rows = self.records("luck-floor-1000")
        kept = []
        removed = 0
        for row in rows:
            if row["record"] == "summary" and row["dataset"] == "crypto-majors-1d":
                row["n_agents"] = row["n_agents"] - 1
            if (
                row["record"] == "agent"
                and row["dataset"] == "crypto-majors-1d"
                and removed < 1
            ):
                removed += 1
                continue
            kept.append(row)
        self.assertEqual(removed, 1)
        self.rewrite("luck-floor-1000", kept)
        self.assert_refused("luck-floor-1000", "datasets disagree on field size")


def agent(dataset, index, shipped, field):
    return {
        "record": "agent",
        "dataset": dataset,
        "agent_id": f"luck-floor-{index:02}",
        "rank_eligible_shipped_floor": shipped,
        "rank_eligible_field": field,
    }


def summary(dataset, shipped, field):
    return {
        "record": "summary",
        "dataset": dataset,
        "shipped_floor": {"n_rank_eligible": shipped},
        "field_measured": {"n_rank_eligible": field},
    }


class EligibilityUnionTests(unittest.TestCase):
    def setUp(self):
        self.figures = module()

    def test_overlapping_paths_are_unioned_not_added(self):
        agents = [
            agent("us-indices-1d", 0, True, True),
            agent("us-indices-1d", 1, True, True),
            agent("us-indices-1d", 2, False, True),
            agent("us-indices-1d", 3, False, False),
        ]
        summaries = {"us-indices-1d": summary("us-indices-1d", 2, 3)}
        self.assertEqual(self.figures.eligible_union(agents, summaries), 3)

    def test_union_spans_datasets_without_merging_identities(self):
        agents = [
            agent("us-indices-1d", 0, True, False),
            agent("crypto-majors-1d", 0, True, False),
        ]
        summaries = {
            "us-indices-1d": summary("us-indices-1d", 1, 0),
            "crypto-majors-1d": summary("crypto-majors-1d", 1, 0),
        }
        self.assertEqual(self.figures.eligible_union(agents, summaries), 2)

    def test_marginal_disagreeing_with_the_summary_is_refused(self):
        agents = [agent("us-indices-1d", 0, True, False)]
        summaries = {"us-indices-1d": summary("us-indices-1d", 4, 0)}
        with self.assertRaises(self.figures.EvidenceSupportError):
            self.figures.eligible_union(agents, summaries)

    def test_repeated_agent_identity_is_refused(self):
        agents = [agent("us-indices-1d", 0, True, False)] * 2
        summaries = {"us-indices-1d": summary("us-indices-1d", 1, 0)}
        with self.assertRaises(self.figures.EvidenceSupportError):
            self.figures.eligible_union(agents, summaries)

    def test_committed_records_have_no_eligible_cell_on_either_path(self):
        # The published annotation is unchanged by the union: both are zero.
        records = [
            json.loads(line)
            for line in (EVIDENCE / "luck-floor-1000.jsonl")
            .read_text(encoding="utf-8")
            .splitlines()
            if line.strip()
        ]
        agents = [r for r in records if r["record"] == "agent"]
        summaries = {r["dataset"]: r for r in records if r["record"] == "summary"}
        added = sum(
            s["shipped_floor"]["n_rank_eligible"] + s["field_measured"]["n_rank_eligible"]
            for s in summaries.values()
        )
        self.assertEqual(self.figures.eligible_union(agents, summaries), 0)
        self.assertEqual(added, 0)


if __name__ == "__main__":
    unittest.main()
