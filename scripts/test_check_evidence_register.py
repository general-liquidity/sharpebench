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

# What the migration wrote on every entry whose history did not say who recorded it.
UNKNOWN_RECORDER = {"recorded_by": {"kind": "unknown"}, "independent": False}


LINK = {
    "repository": "sharpearena",
    "path": "docs/evidence-register.jsonl",
    "claim_id": "SA-tab-f1",
}


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
                **UNKNOWN_RECORDER,
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

    def empty_artifact_row(self, root, **artifact):
        (root / "paper" / "empty.jsonl").write_bytes(b"")
        row = good_row(artifacts=[{"path": "paper/empty.jsonl", "in_repo": True, **artifact}])
        del row["no_measurement"]
        return row

    def test_an_empty_artifact_with_the_empty_digest_fails(self):
        # Regression: the empty-input digest matched the empty file, so a row
        # pointing at nothing read as a verified artifact.
        with TemporaryRoot() as root:
            row = self.empty_artifact_row(root, sha256=CHECK.EMPTY_SHA256)
            problems = CHECK.validate([row], root)
            self.assertTrue(any("is empty" in p for p in problems), problems)

    def test_an_empty_artifact_with_no_digest_fails(self):
        with TemporaryRoot() as root:
            problems = CHECK.validate([self.empty_artifact_row(root)], root)
            self.assertTrue(any("is empty" in p for p in problems), problems)

    def test_the_empty_digest_on_a_nonempty_file_fails(self):
        with TemporaryRoot() as root:
            (root / "paper" / "full.jsonl").write_bytes(b"present\n")
            row = good_row(
                artifacts=[
                    {"path": "paper/full.jsonl", "sha256": CHECK.EMPTY_SHA256, "in_repo": True}
                ]
            )
            del row["no_measurement"]
            problems = CHECK.validate([row], root)
            self.assertTrue(any("digest of empty input" in p for p in problems), problems)

    def test_a_declared_empty_artifact_passes(self):
        with TemporaryRoot() as root:
            row = self.empty_artifact_row(root, sha256=CHECK.EMPTY_SHA256, empty=True)
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
                        **UNKNOWN_RECORDER,
                    }
                ],
            )
            del row["no_measurement"]
            self.assertEqual(CHECK.validate([row], root), [])

    def test_an_external_row_records_its_artifact_without_opening_it(self):
        with TemporaryRoot() as root:
            row = good_row(
                external_repo="sharpearena",
                companion_register=LINK,
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
                companion_register=LINK,
                artifacts=[{"path": "paper/evidence/f1-baselines.json", "in_repo": True}],
            )
            del row["no_measurement"]
            problems = CHECK.validate([row], root)
            self.assertTrue(any("cannot be marked in_repo" in p for p in problems), problems)


class MissingLabelSource(unittest.TestCase):
    """A label source that disappears fails coverage instead of leaving it with
    nothing to check. The cases delete one file from a copy of this product's
    paper, or from a small tree whose section inputs a nested fragment, so the
    same test runs unchanged in both products."""

    def coverage_without(self, relative: str | None) -> list[str]:
        import shutil

        with tempfile.TemporaryDirectory() as directory:
            paper = pathlib.Path(directory) / "paper"
            shutil.copytree(ROOT / "paper" / "sections", paper / "sections")
            shutil.copy(ROOT / "paper" / "main.tex", paper / "main.tex")
            if relative is not None:
                (paper / relative).unlink()
            rows = [
                json.loads(line)
                for line in CHECK.REGISTER_JSONL.read_text(encoding="utf-8").splitlines()
                if line.strip()
            ]
            return self.coverage(paper, rows)

    def coverage(self, paper: pathlib.Path, rows: list[dict]) -> list[str]:
        original = CHECK.LABEL_SOURCES
        CHECK.LABEL_SOURCES = [paper / "main.tex"] + sorted((paper / "sections").glob("*.tex"))
        try:
            problems: list[str] = []
            CHECK.check_coverage(rows, problems)
        finally:
            CHECK.LABEL_SOURCES = original
        return problems

    def test_the_full_copy_has_no_coverage_problem(self):
        self.assertEqual(self.coverage_without(None), [])

    def test_a_deleted_main_file_fails(self):
        problems = self.coverage_without("main.tex")
        self.assertTrue(any("main.tex is not on disk" in p for p in problems), problems)

    def test_a_deleted_input_section_fails(self):
        # The glob no longer lists the deleted file, so only the \input in
        # main.tex can say that it is missing.
        main = (ROOT / "paper" / "main.tex").read_text(encoding="utf-8")
        name = CHECK.INPUT_PATTERN.findall(main)[0]
        relative = name if name.endswith(".tex") else name + ".tex"
        problems = self.coverage_without(relative)
        missing = pathlib.PurePosixPath(relative).name
        self.assertTrue(any(f"{missing} is not on disk" in p for p in problems), problems)

    def nested_tree(self, directory: str, with_fragment: bool) -> pathlib.Path:
        """main.tex inputs one section, and that section inputs a fragment that
        sits outside the sections glob, so only the nested input names it."""
        paper = pathlib.Path(directory) / "paper"
        (paper / "sections").mkdir(parents=True)
        (paper / "main.tex").write_text(r"\input{sections/01-body}" "\n", encoding="utf-8")
        # TeX resolves the nested path from main.tex's directory, not the section's.
        # The commented-out \input names a file that never exists and is ignored.
        (paper / "sections" / "01-body.tex").write_text(
            r"\label{tab:body}" "\n" r"% \input{fragments/commented-out}" "\n"
            r"\input{fragments/nested}" "\n",
            encoding="utf-8",
        )
        if with_fragment:
            (paper / "fragments").mkdir()
            (paper / "fragments" / "nested.tex").write_text(
                r"\label{fig:nested}" "\n", encoding="utf-8"
            )
        return paper

    def test_a_nested_fragment_label_needs_a_row(self):
        with tempfile.TemporaryDirectory() as directory:
            paper = self.nested_tree(directory, with_fragment=True)
            problems = self.coverage(paper, [good_row(labels=["tab:body"])])
        self.assertEqual(len(problems), 1, problems)
        self.assertIn("fig:nested has no register row", problems[0])

    def test_a_deleted_nested_fragment_fails(self):
        with tempfile.TemporaryDirectory() as directory:
            paper = self.nested_tree(directory, with_fragment=False)
            problems = self.coverage(paper, [good_row(labels=["tab:body"])])
        self.assertEqual(len(problems), 1, problems)
        self.assertIn("nested.tex is not on disk", problems[0])

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
                **UNKNOWN_RECORDER,
            },
            {
                "date": "2026-10-01",
                "disposition": "needs-rescore",
                "rationale": second_rationale,
                **UNKNOWN_RECORDER,
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
                        **UNKNOWN_RECORDER,
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
                        **UNKNOWN_RECORDER,
                    },
                    {
                        "date": "2026-09-23",
                        "disposition": "still-applicable",
                        "rationale": "A later look.",
                        **UNKNOWN_RECORDER,
                    },
                ]
            )
            problems = CHECK.validate([row], root)
            self.assertTrue(any("is before the previous entry" in p for p in problems), problems)


class Recorder(unittest.TestCase):
    def entry(self, **fields):
        base = {"date": "2026-09-23", "disposition": "still-applicable", "rationale": "Initial."}
        base.update(fields)
        return base

    def problems(self, **fields):
        with TemporaryRoot() as root:
            return CHECK.validate([good_row(status_history=[self.entry(**fields)])], root)

    def test_an_entry_with_no_recorder_fails(self):
        problems = self.problems()
        self.assertTrue(any("'recorded_by' is missing" in p for p in problems), problems)
        self.assertTrue(any("'independent' is missing" in p for p in problems), problems)

    def test_a_named_recorder_without_an_id_fails(self):
        for kind in ("human", "model"):
            problems = self.problems(recorded_by={"kind": kind}, independent=False)
            self.assertTrue(any(f"a {kind} recorder needs an id" in p for p in problems), kind)

    def test_an_unknown_recorder_cannot_be_independent(self):
        problems = self.problems(recorded_by={"kind": "unknown"}, independent=True)
        self.assertTrue(any("cannot be independent" in p for p in problems), problems)

    def test_an_undefined_recorder_kind_fails(self):
        problems = self.problems(recorded_by={"kind": "reviewer", "id": "x"}, independent=True)
        self.assertTrue(any("is not one of" in p for p in problems), problems)

    def test_named_and_unknown_recorders_pass_and_are_counted_apart(self):
        rows = [
            good_row(
                claim_id="R-1",
                status_history=[
                    self.entry(recorded_by={"kind": "human", "id": "reviewer-a"}, independent=True)
                ],
            ),
            good_row(
                claim_id="R-2",
                status_history=[
                    self.entry(recorded_by={"kind": "model", "id": "author-agent"}, independent=False)
                ],
            ),
            good_row(claim_id="R-3", status_history=[self.entry(**UNKNOWN_RECORDER)]),
            good_row(claim_id="R-4", status_history=[self.entry()]),
        ]
        with TemporaryRoot() as root:
            problems = CHECK.validate(rows[:3], root)
        self.assertEqual(problems, [])
        # The entry with no recorder at all counts as unknown, never as human.
        self.assertEqual(CHECK.recorder_counts(rows), {"human": 1, "model": 1, "unknown": 2})


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
                        **UNKNOWN_RECORDER,
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


class CompanionRegister(unittest.TestCase):
    """The mirrored rows open against the register of record."""

    def setUp(self):
        self._dir = tempfile.TemporaryDirectory()
        base = pathlib.Path(self._dir.name)
        self.ours, self.theirs = base / "ours", base / "theirs"
        for root in (self.ours, self.theirs):
            (root / "scripts").mkdir(parents=True)
            (root / "docs").mkdir()
            for name in CHECK.SHARED_SCRIPTS:
                (root / name).write_bytes(b"shared\n")
        artifact = self.theirs / "paper" / "evidence.json"
        artifact.parent.mkdir()
        artifact.write_bytes(b"frozen\n")
        self.digest = hashlib.sha256(b"frozen\n").hexdigest()
        self.write_theirs(disposition="still-applicable")
        (self.ours / "docs" / "evidence-register.jsonl").write_text("", encoding="utf-8")

    def tearDown(self):
        self._dir.cleanup()

    def write_theirs(self, disposition):
        row = good_row(claim_id="SA-tab-f1", disposition=disposition)
        (self.theirs / "docs" / "evidence-register.jsonl").write_text(
            json.dumps(row) + "\n", encoding="utf-8"
        )

    def mirrored(self, **overrides):
        values = dict(
            claim_id="SA-tab-f1",
            external_repo="sharpearena",
            companion_register=LINK,
            artifacts=[{"path": "paper/evidence.json", "sha256": self.digest, "in_repo": False}],
        )
        values.update(overrides)
        return good_row(**values)

    def problems(self, row):
        found: list[str] = []
        CHECK.check_companion([row], self.ours, self.theirs, found)
        return found

    def test_a_matching_companion_passes(self):
        self.assertEqual(self.problems(self.mirrored()), [])

    def test_a_moved_digest_fails(self):
        (self.theirs / "paper" / "evidence.json").write_bytes(b"moved\n")
        problems = self.problems(self.mirrored())
        self.assertTrue(any("hashes to" in problem for problem in problems), problems)

    def test_an_artifact_gone_from_the_companion_fails(self):
        (self.theirs / "paper" / "evidence.json").unlink()
        problems = self.problems(self.mirrored())
        self.assertTrue(any("is not on disk" in problem for problem in problems), problems)

    def test_a_disposition_the_two_registers_disagree_on_fails(self):
        self.write_theirs(disposition="historical-only")
        problems = self.problems(self.mirrored())
        self.assertTrue(any("register of record" in problem for problem in problems), problems)

    def test_a_mirrored_row_with_no_row_of_record_fails(self):
        link = dict(LINK, claim_id="SA-gone")
        problems = self.problems(self.mirrored(companion_register=link))
        self.assertTrue(any("of record" in problem for problem in problems), problems)

    def test_a_generator_that_drifted_between_the_products_fails(self):
        (self.theirs / "scripts" / "make-evidence-register.py").write_bytes(b"forked\n")
        problems = self.problems(self.mirrored())
        self.assertTrue(any("differs between" in problem for problem in problems), problems)

    def test_line_endings_alone_are_not_drift(self):
        (self.theirs / "scripts" / "make-evidence-register.py").write_bytes(b"shared\r\n")
        self.assertEqual(self.problems(self.mirrored()), [])

    def test_the_other_products_mirrored_rows_are_opened_too(self):
        row = self.mirrored(artifacts=[{"path": "gone.json", "in_repo": False}])
        row["companion_register"] = dict(LINK, claim_id="SB-1")
        (self.theirs / "docs" / "evidence-register.jsonl").write_text(
            json.dumps(row) + "\n", encoding="utf-8"
        )
        (self.ours / "docs" / "evidence-register.jsonl").write_text(
            json.dumps(good_row(claim_id="SB-1")) + "\n", encoding="utf-8"
        )
        found: list[str] = []
        CHECK.check_companion([], self.ours, self.theirs, found)
        self.assertTrue(any("gone.json" in problem for problem in found), found)

    def test_a_mirrored_row_with_no_link_fails(self):
        with TemporaryRoot() as root:
            row = self.mirrored(source_location=["paper/sections/05-experiments.tex:1"])
            del row["companion_register"]
            del row["no_measurement"]
            problems = CHECK.validate([row], root)
            self.assertTrue(any("names no companion_register" in p for p in problems), problems)

    def test_a_link_to_another_repository_fails(self):
        with TemporaryRoot() as root:
            row = self.mirrored(companion_register=dict(LINK, repository="elsewhere"))
            del row["no_measurement"]
            problems = CHECK.validate([row], root)
            self.assertTrue(any("companion_register names" in p for p in problems), problems)


class CommittedRegister(unittest.TestCase):
    def test_the_committed_register_passes(self):
        self.assertEqual(CHECK.main([]), 0)


if __name__ == "__main__":
    unittest.main()
