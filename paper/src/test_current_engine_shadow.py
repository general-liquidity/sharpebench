"""Regressions for the current-engine shadow check.

No producer runs here: the comparison and the manifest rules are exercised on
planted rows, and the one end-to-end case is a refusal that happens before any
producer would start.
"""

from __future__ import annotations

import copy
import importlib.util
import json
import unittest
from pathlib import Path

SOURCE = Path(__file__).with_name("check-current-engine-shadow.py")
SPEC = importlib.util.spec_from_file_location("current_engine_shadow", SOURCE)
shadow = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(shadow)

SWEEP = "evidence_sweep"


def row(agent: str, **fields) -> dict:
    base = {
        "dataset": "commodities-1d",
        "dsr_bar": 0.95,
        "n_trials": 50,
        "sr_std_pinned": None,
        "agent_id": agent,
        "psr": 0.5,
        "raw_mean_return": 0.001,
        "passed_k": False,
    }
    base.update(fields)
    return base


class ShardComparisonTests(unittest.TestCase):
    def test_a_shard_with_zero_records_is_refused(self) -> None:
        # The sweep of 1bd06e2 published zero commodities records with exit 0.
        with self.assertRaisesRegex(shadow.ShadowRefusal, "zero records"):
            shadow.compare_shard(SWEEP, "commodities-1d", [row("a")], [])

    def test_rows_that_do_not_align_with_the_frozen_shard_are_refused(self) -> None:
        with self.assertRaisesRegex(shadow.ShadowRefusal, "do not align"):
            shadow.compare_shard(SWEEP, "commodities-1d", [row("a")], [row("b")])
        with self.assertRaisesRegex(shadow.ShadowRefusal, "repeats key"):
            shadow.compare_shard(SWEEP, "commodities-1d", [row("a")], [row("a"), row("a")])

    def test_moves_nulls_and_verdicts_are_counted_and_last_bits_are_not(self) -> None:
        frozen = [row("a"), row("b"), row("c")]
        current = [
            row("a", psr=0.5 + 1e-16),
            row("b", psr=0.75, raw_mean_return=None),
            row("c", passed_k=True, pooled_sharpe_per_period=0.1),
        ]
        moved, flips, added = shadow.compare_shard(SWEEP, "commodities-1d", frozen, current)
        self.assertEqual(
            moved["psr"], {"rows_moved": 1, "max_abs": 0.25, "became_null": 0, "became_number": 0}
        )
        self.assertEqual(moved["raw_mean_return"]["became_null"], 1)
        self.assertEqual(moved["passed_k"]["rows_moved"], 1)
        self.assertEqual([(f["field"], f["frozen"], f["current"]) for f in flips], [("passed_k", False, True)])
        self.assertEqual(added, ["pooled_sharpe_per_period"])


class ManifestTests(unittest.TestCase):
    def setUp(self) -> None:
        self.measured = {
            "observed": [
                {
                    "producer": SWEEP,
                    "dataset": "commodities-1d",
                    "field": "psr",
                    "rows_moved": 3,
                    "max_abs": 0.25,
                    "became_null": 0,
                    "became_number": 0,
                }
            ],
            "verdict_flips": [
                {
                    "producer": SWEEP,
                    "dataset": "commodities-1d",
                    "key": "k",
                    "field": "passed_k",
                    "frozen": False,
                    "current": True,
                }
            ],
            "added_fields": {SWEEP: ["pooled_sharpe_per_period"]},
        }
        self.manifest = shadow.merged_manifest(
            {"schema": shadow.SCHEMA, "change_points": [{"id": "CP1"}]}, self.measured
        )
        for entry in [*self.manifest["observed"], *self.manifest["verdict_flips"]]:
            entry["attributed_to"] = ["CP1"]

    def problems(self, manifest) -> list[str]:
        return shadow.check_manifest(manifest, self.measured)

    def test_an_attributed_manifest_that_matches_passes(self) -> None:
        self.assertEqual(self.problems(self.manifest), [])
        # A platform's last bits do not count as a magnitude change.
        tolerant = copy.deepcopy(self.manifest)
        tolerant["observed"][0]["max_abs"] = 0.25 * (1 + 1e-9)
        self.assertEqual(self.problems(tolerant), [])

    def test_every_departure_from_the_measurement_fails(self) -> None:
        def planted(change):
            manifest = copy.deepcopy(self.manifest)
            change(manifest)
            return manifest

        cases = (
            (lambda m: m["observed"].clear(), "unlisted field change"),
            (lambda m: m["verdict_flips"].clear(), "unlisted verdict flip"),
            (lambda m: m["observed"].append({**m["observed"][0], "field": "dsr"}), "no longer observed"),
            (lambda m: m["observed"][0].update(rows_moved=2), "rows_moved listed 2"),
            (lambda m: m["observed"][0].update(max_abs=0.2), "max_abs listed 0.2"),
            (lambda m: m["verdict_flips"][0].update(current=False), "verdict flip"),
            (lambda m: m["observed"][0].update(attributed_to=[]), "no change point attributed"),
            (lambda m: m["observed"][0].update(attributed_to=["CP9"]), "unknown change point"),
            (lambda m: m.update(added_fields={}), "added fields"),
            (lambda m: m.update(schema="v0"), "manifest schema"),
        )
        for change, diagnostic in cases:
            with self.subTest(diagnostic=diagnostic):
                problems = self.problems(planted(change))
                self.assertTrue(any(diagnostic in p for p in problems), problems)

    def test_refreshing_keeps_attributions_and_leaves_new_entries_unattributed(self) -> None:
        measured = copy.deepcopy(self.measured)
        measured["observed"].append({**measured["observed"][0], "field": "dsr"})
        merged = shadow.merged_manifest(self.manifest, measured)
        self.assertEqual([e["attributed_to"] for e in merged["observed"]], [["CP1"], []])
        self.assertEqual(merged["change_points"], self.manifest["change_points"])


class CommittedManifestTests(unittest.TestCase):
    def test_the_committed_manifest_attributes_every_entry(self) -> None:
        manifest = json.loads(shadow.MANIFEST.read_text(encoding="utf-8"))
        self.assertEqual(manifest["schema"], shadow.SCHEMA)
        points = {p["id"] for p in manifest["change_points"]}
        self.assertGreaterEqual(len(points), 11, "the audit's eleven change points")
        for entry in [*manifest["observed"], *manifest["verdict_flips"]]:
            self.assertTrue(entry["attributed_to"], entry)
            self.assertLessEqual(set(entry["attributed_to"]), points, entry)

    def test_output_inside_frozen_evidence_is_refused_before_any_producer_runs(self) -> None:
        code = shadow.main(
            ["--work", str(shadow.REPO / "paper/evidence/final"), "--bin-dir", "unused"]
        )
        self.assertEqual(code, 1)


if __name__ == "__main__":
    unittest.main()
