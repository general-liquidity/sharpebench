#!/usr/bin/env python3
"""Tests for scripts/check-evidence-register.py.

Each of the three failure modes the ticket names has its own test, and a fourth
group covers the legs that keep the register's own references honest. The last
test runs the checker over the committed register, so a real edit that breaks a
rule fails here as well as in CI.
"""

from __future__ import annotations

import hashlib
import importlib.util
import json
import pathlib
import tempfile
import unittest

ROOT = pathlib.Path(__file__).resolve().parents[1]


def load_checker():
    spec = importlib.util.spec_from_file_location(
        "check_evidence_register", ROOT / "scripts" / "check-evidence-register.py"
    )
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


CHECK = load_checker()


def good_row(**overrides) -> dict:
    row = {
        "claim_id": "T-1",
        "product": "SharpeBench",
        "kind": "table",
        "labels": ["tab:example"],
        "source_location": ["paper/sections/05-experiments.tex:1"],
        "claim": "An example claim.",
        "artifacts": [],
        "no_measurement": "The caption states that no row reports a measurement.",
        "producer_command": "none",
        "producing_commit": "unknown",
        "effective_configuration": "not applicable",
        "missing_provenance": [],
        "later_changes": [],
        "present_applicability": "Applicable as a description.",
        "disposition": "still-applicable",
        "disposition_rationale": "No measurement can go stale.",
        "owner": "evidence owner",
        "follow_up": None,
        "status_history": [
            {
                "date": "2026-09-23",
                "disposition": "still-applicable",
                "rationale": "Initial entry.",
            }
        ],
    }
    row.update(overrides)
    return row


class TemporaryRoot:
    """A throwaway tree with the one source file the rows point at."""

    def __enter__(self):
        self._dir = tempfile.TemporaryDirectory()
        root = pathlib.Path(self._dir.name)
        (root / "paper" / "sections").mkdir(parents=True)
        (root / "paper" / "sections" / "05-experiments.tex").write_text(
            "line one\n", encoding="utf-8"
        )
        self.root = root
        return root

    def __exit__(self, *exc):
        self._dir.cleanup()
        return False


class MissingArtifact(unittest.TestCase):
    def test_an_artifact_that_is_not_on_disk_fails(self):
        with TemporaryRoot() as root:
            row = good_row(
                artifacts=[{"path": "paper/evidence/final/gone.jsonl", "in_repo": True}],
            )
            del row["no_measurement"]
            problems = CHECK.validate([row], root)
            self.assertTrue(any("is not on disk" in problem for problem in problems), problems)

    def test_an_artifact_whose_digest_moved_fails(self):
        with TemporaryRoot() as root:
            target = root / "paper" / "evidence.jsonl"
            target.write_text("present\n", encoding="utf-8")
            row = good_row(
                artifacts=[
                    {"path": "paper/evidence.jsonl", "sha256": "0" * 64, "in_repo": True}
                ],
            )
            del row["no_measurement"]
            problems = CHECK.validate([row], root)
            self.assertTrue(any("hashes to" in problem for problem in problems), problems)

    def test_a_matching_digest_passes(self):
        with TemporaryRoot() as root:
            target = root / "paper" / "evidence.jsonl"
            target.write_bytes(b"present\n")
            digest = hashlib.sha256(b"present\n").hexdigest()
            row = good_row(
                artifacts=[
                    {"path": "paper/evidence.jsonl", "sha256": digest, "in_repo": True}
                ],
            )
            del row["no_measurement"]
            self.assertEqual(CHECK.validate([row], root), [])

    def test_no_artifact_and_no_stated_reason_fails(self):
        with TemporaryRoot() as root:
            row = good_row()
            del row["no_measurement"]
            problems = CHECK.validate([row], root)
            self.assertTrue(any("no artifact is recorded" in p for p in problems), problems)

    def test_no_artifact_is_allowed_when_the_row_is_unresolved(self):
        with TemporaryRoot() as root:
            row = good_row(
                disposition="unresolved",
                disposition_rationale="The artifact could not be located.",
                status_history=[
                    {
                        "date": "2026-09-23",
                        "disposition": "unresolved",
                        "rationale": "Initial entry.",
                    }
                ],
            )
            del row["no_measurement"]
            self.assertEqual(CHECK.validate([row], root), [])

    def test_an_external_row_records_its_artifact_without_opening_it(self):
        with TemporaryRoot() as root:
            row = good_row(
                external_repo="sharpearena",
                artifacts=[
                    {
                        "path": "paper/evidence/f1-baselines.json",
                        "sha256": "a" * 64,
                        "in_repo": False,
                    }
                ],
            )
            del row["no_measurement"]
            self.assertEqual(CHECK.validate([row], root), [])

    def test_an_external_row_cannot_claim_an_in_repo_artifact(self):
        with TemporaryRoot() as root:
            row = good_row(
                external_repo="sharpearena",
                artifacts=[{"path": "paper/evidence/f1-baselines.json", "in_repo": True}],
            )
            del row["no_measurement"]
            problems = CHECK.validate([row], root)
            self.assertTrue(any("cannot be marked in_repo" in p for p in problems), problems)


class MissingDisposition(unittest.TestCase):
    def test_an_absent_disposition_fails(self):
        with TemporaryRoot() as root:
            row = good_row()
            row["disposition"] = ""
            problems = CHECK.validate([row], root)
            self.assertTrue(any("is not one of" in p for p in problems), problems)
            self.assertTrue(any("'disposition' is missing or empty" in p for p in problems))

    def test_a_disposition_outside_the_defined_set_fails(self):
        with TemporaryRoot() as root:
            row = good_row(disposition="probably-fine")
            problems = CHECK.validate([row], root)
            self.assertTrue(any("is not one of" in p for p in problems), problems)

    def test_an_empty_rationale_fails(self):
        with TemporaryRoot() as root:
            row = good_row(disposition_rationale="   ")
            problems = CHECK.validate([row], root)
            self.assertTrue(any("carries no rationale" in p for p in problems), problems)

    def test_a_manuscript_label_with_no_row_fails(self):
        with tempfile.TemporaryDirectory() as directory:
            source = pathlib.Path(directory) / "one.tex"
            source.write_text(
                "\\label{tab:covered}\n\\label{fig:orphan}\n", encoding="utf-8"
            )
            original = CHECK.LABEL_SOURCES
            CHECK.LABEL_SOURCES = [source]
            try:
                problems: list[str] = []
                CHECK.check_coverage([good_row(labels=["tab:covered"])], problems)
            finally:
                CHECK.LABEL_SOURCES = original
            self.assertEqual(len(problems), 1, problems)
            self.assertIn("fig:orphan", problems[0])

    def test_an_external_rows_labels_do_not_count_as_coverage(self):
        with tempfile.TemporaryDirectory() as directory:
            source = pathlib.Path(directory) / "one.tex"
            source.write_text("\\label{tab:covered}\n", encoding="utf-8")
            original = CHECK.LABEL_SOURCES
            CHECK.LABEL_SOURCES = [source]
            try:
                problems: list[str] = []
                CHECK.check_coverage(
                    [good_row(labels=["tab:covered"], external_repo="sharpearena")], problems
                )
            finally:
                CHECK.LABEL_SOURCES = original
            self.assertEqual(len(problems), 1, problems)
            self.assertIn("tab:covered", problems[0])


class StatusChangeWithoutRationale(unittest.TestCase):
    def history(self, second_rationale):
        return [
            {
                "date": "2026-09-23",
                "disposition": "historical-only",
                "rationale": "Initial entry: bound to the frozen snapshot.",
            },
            {
                "date": "2026-10-01",
                "disposition": "needs-rescore",
                "rationale": second_rationale,
            },
        ]

    def test_a_changed_disposition_repeating_the_previous_rationale_fails(self):
        with TemporaryRoot() as root:
            row = good_row(
                disposition="needs-rescore",
                status_history=self.history("Initial entry: bound to the frozen snapshot."),
            )
            problems = CHECK.validate([row], root)
            self.assertTrue(any("no rationale of its own" in p for p in problems), problems)

    def test_a_changed_disposition_with_an_empty_rationale_fails(self):
        with TemporaryRoot() as root:
            row = good_row(disposition="needs-rescore", status_history=self.history("  "))
            problems = CHECK.validate([row], root)
            self.assertTrue(any("no rationale recorded" in p for p in problems), problems)
            self.assertTrue(any("no rationale of its own" in p for p in problems), problems)

    def test_a_changed_disposition_with_its_own_rationale_passes(self):
        with TemporaryRoot() as root:
            row = good_row(
                disposition="needs-rescore",
                status_history=self.history(
                    "The committed per-seed returns were verified sufficient to recompute."
                ),
            )
            self.assertEqual(CHECK.validate([row], root), [])

    def test_a_history_that_ends_elsewhere_than_the_current_disposition_fails(self):
        with TemporaryRoot() as root:
            row = good_row(
                disposition="needs-rescore",
                status_history=[
                    {
                        "date": "2026-09-23",
                        "disposition": "historical-only",
                        "rationale": "Initial entry.",
                    }
                ],
            )
            problems = CHECK.validate([row], root)
            self.assertTrue(any("status_history ends on" in p for p in problems), problems)

    def test_an_empty_history_fails(self):
        with TemporaryRoot() as root:
            problems = CHECK.validate([good_row(status_history=[])], root)
            self.assertTrue(any("status_history is empty" in p for p in problems), problems)

    def test_history_dates_must_not_go_backwards(self):
        with TemporaryRoot() as root:
            row = good_row(
                status_history=[
                    {
                        "date": "2026-10-01",
                        "disposition": "still-applicable",
                        "rationale": "Initial entry.",
                    },
                    {
                        "date": "2026-09-23",
                        "disposition": "still-applicable",
                        "rationale": "A later look.",
                    },
                ]
            )
            problems = CHECK.validate([row], root)
            self.assertTrue(any("is before the previous entry" in p for p in problems), problems)


class ReferenceIntegrity(unittest.TestCase):
    def test_a_repeated_claim_id_fails(self):
        with TemporaryRoot() as root:
            problems = CHECK.validate([good_row(), good_row()], root)
            self.assertTrue(any("claim_id repeats" in p for p in problems), problems)

    def test_a_source_location_past_the_end_of_the_file_fails(self):
        with TemporaryRoot() as root:
            row = good_row(source_location=["paper/sections/05-experiments.tex:99"])
            problems = CHECK.validate([row], root)
            self.assertTrue(any("past the end of the file" in p for p in problems), problems)

    def test_a_source_location_without_a_line_fails(self):
        with TemporaryRoot() as root:
            row = good_row(source_location=["paper/sections/05-experiments.tex"])
            problems = CHECK.validate([row], root)
            self.assertTrue(any("is not 'path:line'" in p for p in problems), problems)

    def test_a_markdown_copy_that_drifts_from_the_rows_fails(self):
        rows = [
            json.loads(line)
            for line in CHECK.REGISTER_JSONL.read_text(encoding="utf-8").splitlines()
            if line.strip()
        ]
        with tempfile.TemporaryDirectory() as directory:
            stale = pathlib.Path(directory) / "evidence-register.md"
            stale.write_text("# not the generated register\n", encoding="utf-8")
            original = CHECK.REGISTER_MD
            CHECK.REGISTER_MD = stale
            try:
                problems: list[str] = []
                CHECK.check_markdown(rows, problems)
            finally:
                CHECK.REGISTER_MD = original
            self.assertEqual(len(problems), 1, problems)
            self.assertIn("regenerate it", problems[0])


class PriorityQueue(unittest.TestCase):
    def queued(self, count):
        return [
            good_row(
                claim_id=f"Q-{index}",
                disposition="needs-new-experiment",
                disposition_rationale="Generation changed.",
                headline=True,
                status_history=[
                    {
                        "date": "2026-09-23",
                        "disposition": "needs-new-experiment",
                        "rationale": "Initial entry.",
                    }
                ],
            )
            for index in range(count)
        ]

    def test_a_queue_at_the_bound_passes(self):
        problems: list[str] = []
        CHECK.check_queue_bound(self.queued(3), problems, 3)
        self.assertEqual(problems, [])

    def test_a_queue_over_the_bound_fails(self):
        problems: list[str] = []
        CHECK.check_queue_bound(self.queued(4), problems, 3)
        self.assertEqual(len(problems), 1, problems)
        self.assertIn("above the bound", problems[0])

    def test_a_headline_row_that_needs_nothing_fails(self):
        problems: list[str] = []
        CHECK.check_queue_bound([good_row(headline=True)], problems, 3)
        self.assertEqual(len(problems), 1, problems)
        self.assertIn("belongs in no queue", problems[0])

    def test_the_bound_is_read_from_the_generator(self):
        self.assertIsInstance(CHECK.queue_bound(), int)


class CommittedRegister(unittest.TestCase):
    def test_the_committed_register_passes(self):
        self.assertEqual(CHECK.main(), 0)


if __name__ == "__main__":
    unittest.main()
