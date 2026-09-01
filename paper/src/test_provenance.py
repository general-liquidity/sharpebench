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
