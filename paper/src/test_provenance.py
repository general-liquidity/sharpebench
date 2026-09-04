"""Tests for the closed provenance policy and committed-byte validation."""

from __future__ import annotations

import copy
import importlib.util
import json
import sys
import unittest
from pathlib import Path


HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[1]
sys.path.insert(0, str(HERE))

import provenance_common as common


def load_checker():
    spec = importlib.util.spec_from_file_location(
        "sharpebench_check_provenance", HERE / "check-provenance.py"
    )
    assert spec is not None and spec.loader is not None
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


checker = load_checker()


class ProvenancePolicyTests(unittest.TestCase):
    def setUp(self) -> None:
        self.manifest = json.loads(
            (ROOT / "paper/evidence/provenance.json").read_text(encoding="utf-8")
        )

    def test_current_manifest_uses_the_closed_policy(self) -> None:
        self.assertEqual(common.manifest_rule_problems(self.manifest), [])

    def test_every_manifest_rule_rejects_its_planted_counterexample(self) -> None:
        """One registry covers every rule family with a named negative control.

        The real manifest above is the valid control. Keeping the mutations in
        one table means a new validator rule has one obvious place to acquire a
        non-vacuous fixture, and CI exercises the whole registry whenever this
        checker changes.
        """

        def mutate(field, value):
            def apply(manifest):
                manifest[field] = value

            return apply

        def mutate_source_record(change):
            def apply(manifest):
                change(manifest["source_files"][0])

            return apply

        def duplicate_source(manifest):
            manifest["source_files"].append(copy.deepcopy(manifest["source_files"][0]))

        cases = (
            ("canonical policy", mutate("schema_version", -1), "canonical validator rule"),
            ("closed top level", lambda m: m.update({"unreviewed": True}), "top-level fields differ"),
            ("commit identity", mutate("generated_at_head", "not-a-commit"), "not a full commit id"),
            ("dirty flag type", mutate("generated_at_head_dirty", "false"), "must be boolean"),
            ("snapshot digest", mutate("source_snapshot_sha256", "xyz"), "not a sha256"),
            ("record schema", mutate_source_record(lambda r: r.update({"size": 1})), "exactly path and sha256"),
            ("rooted path", mutate_source_record(lambda r: r.update(path="../escape")), "invalid path"),
            ("record digest", mutate_source_record(lambda r: r.update(sha256="0")), "invalid sha256"),
            ("unique path", duplicate_source, "repeats path"),
            ("nonempty sources", mutate("source_files", []), "source_files is empty"),
            ("nonempty artifacts", mutate("artifacts", []), "artifacts is empty"),
        )
        self.assertGreaterEqual(len(cases), 10, "a truncated registry proves too little")
        for name, apply, diagnostic in cases:
            with self.subTest(rule=name):
                planted = copy.deepcopy(self.manifest)
                apply(planted)
                problems = common.manifest_rule_problems(planted)
                self.assertTrue(
                    any(diagnostic in problem for problem in problems),
                    f"planted counterexample for {name!r} escaped: {problems}",
                )

    def test_manifest_cannot_remove_scope_or_add_an_exclusion(self) -> None:
        narrowed = copy.deepcopy(self.manifest)
        narrowed["source_snapshot_scope"].pop()
        problems = common.manifest_rule_problems(narrowed)
        self.assertTrue(any("source_snapshot_scope" in item for item in problems))

        excluded = copy.deepcopy(self.manifest)
        excluded["source_snapshot_excludes"].append("crates")
        problems = common.manifest_rule_problems(excluded)
        self.assertTrue(any("source_snapshot_excludes" in item for item in problems))

    def test_manifest_rejects_duplicate_and_traversing_records(self) -> None:
        duplicate = copy.deepcopy(self.manifest)
        duplicate["source_files"].append(copy.deepcopy(duplicate["source_files"][0]))
        self.assertTrue(
            any("repeats path" in item for item in common.manifest_rule_problems(duplicate))
        )

        traversing = copy.deepcopy(self.manifest)
        traversing["source_files"][0]["path"] = "../outside"
        self.assertTrue(
            any("invalid path" in item for item in common.manifest_rule_problems(traversing))
        )

    def test_text_digest_is_crlf_invariant_but_binary_is_exact(self) -> None:
        self.assertEqual(
            common.digest_bytes(b"a\r\nb\r\n", canonical_text=True),
            common.digest_bytes(b"a\nb\n", canonical_text=True),
        )
        self.assertNotEqual(
            common.digest_bytes(b"a\x00\r\n", canonical_text=True),
            common.digest_bytes(b"a\x00\n", canonical_text=True),
        )

    def test_scope_globs_are_rooted_and_double_star_crosses_directories(self) -> None:
        self.assertTrue(common._matches("Cargo.toml", "Cargo.toml"))
        self.assertFalse(common._matches("xtask/Cargo.toml", "Cargo.toml"))
        self.assertTrue(
            common._matches("crates/demo/src/nested/lib.rs", "crates/**/*.rs")
        )
        self.assertTrue(common._matches("arena/state.json", "arena/**/*.json"))
        self.assertTrue(
            common._matches(
                "paper/evidence/prospective-forecast-field/field-plan.json",
                "paper/evidence/prospective-forecast-field/**/*.json",
            )
        )
        self.assertTrue(
            common._matches(
                "paper/evidence/prospective-forecast-field/resolved/agent.json",
                "paper/evidence/prospective-forecast-field/**/*.json",
            )
        )
        self.assertFalse(
            common._matches(
                "paper/evidence/prospective-forecast-field/private/model.bin",
                "paper/evidence/prospective-forecast-field/**/*.json",
            )
        )

    def test_clean_generation_records_match_the_committed_bytes(self) -> None:
        commit = self.manifest["generated_at_head"]
        self.assertEqual(
            checker.check_committed_group(
                "source", self.manifest["source_files"], commit, canonical_text=True
            ),
            [],
        )
        self.assertEqual(
            checker.check_committed_group(
                "artifact", self.manifest["artifacts"], commit
            ),
            [],
        )


if __name__ == "__main__":
    unittest.main()
