"""Hermetic tests for the prospective-field import boundary."""

from __future__ import annotations

import importlib.util
import json
import os
import subprocess
import unittest
from pathlib import Path
from tempfile import TemporaryDirectory

HERE = Path(__file__).resolve().parent


def _load_importer():
    spec = importlib.util.spec_from_file_location(
        "prospective_field_importer", HERE / "import-prospective-field.py"
    )
    assert spec is not None and spec.loader is not None
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


importer = _load_importer()


def _write_json(path: Path, value: object) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(
        json.dumps(value, indent=2, sort_keys=True) + "\n",
        encoding="utf-8",
        newline="\n",
    )


def _git(root: Path, *arguments: str) -> str:
    process = subprocess.run(
        [
            "git",
            "-C",
            str(root),
            "-c",
            "user.name=Prospective fixture",
            "-c",
            "user.email=fixture@example.invalid",
            *arguments,
        ],
        check=True,
        capture_output=True,
        text=True,
        env={**os.environ, "GIT_CONFIG_NOSYSTEM": "1"},
    )
    return process.stdout.strip()


def _closed_field(root: Path, schema="sharpe.forecast-evidence.v1", encoding=None) -> Path:
    source = root / "paper/evidence/prospective-forecast-field"
    plan = {
        "schema_version": importer.PLAN_SCHEMA,
        "models": [{"agent_id": "fixture-agent"}],
        "contracts": [{"contract_id": "fixture-contract"}],
    }
    _write_json(source / "field-plan.json", plan)
    (source / "field-plan.sha256").write_text(
        importer._canonical_sha256(plan) + "\n", encoding="utf-8", newline="\n"
    )
    _write_json(source / "observation.json", {"frozen": True})
    revision = {"claim_id": "fixture-contract", "status": "eligible", "prediction": [0.5]}
    if encoding is not None:
        revision["contract_digest_encoding"] = encoding
    ledger = {
        "schema_version": schema,
        "identity": {"agent_id": "fixture-agent", "model_id": "fixture-model"},
        "contracts": plan["contracts"],
        "revisions": [revision],
    }
    _write_json(source / "pending/fixture-agent.json", ledger)
    _write_json(source / "inference/fixture-agent.json", {"inference": True})
    sealed, resolution_files, _ = importer._expected_paths(["fixture-agent"])
    forecast_commit = {
        "schema_version": importer.COMMIT_SCHEMA,
        "field_plan_sha256": importer._canonical_sha256(plan),
        "files": {
            relative: importer._sha256((source / relative).read_bytes())
            for relative in sorted(sealed)
        },
    }
    _write_json(source / "forecast-commit.json", forecast_commit)
    _write_json(
        source / "resolved/fixture-agent.json",
        {
            **ledger,
            "resolutions": [
                {
                    "claim_id": "fixture-contract",
                    "status": "resolved",
                    "outcome": 1.0,
                }
            ],
        },
    )
    _write_json(
        source / "resolution.json",
        {
            "schema_version": importer.RESOLUTION_SCHEMA,
            "field_plan_sha256": importer._canonical_sha256(plan),
            "forecast_commit_sha256": importer._sha256(
                (source / "forecast-commit.json").read_bytes()
            ),
        },
    )
    resolution_manifest = {
        "schema_version": importer.RESOLUTION_MANIFEST_SCHEMA,
        "files": {
            relative: importer._sha256((source / relative).read_bytes())
            for relative in sorted(resolution_files)
        },
    }
    _write_json(source / "resolution-manifest.json", resolution_manifest)
    _git(root, "init", "--quiet", "--initial-branch=main")
    _git(root, "remote", "add", "origin", "https://example.invalid/arena.git")
    _git(root, "add", "-A")
    _git(root, "commit", "--quiet", "-m", "closed field")
    return source


class ProspectiveFieldImportTests(unittest.TestCase):
    def test_v2_import_accepts_both_supported_digest_labels(self):
        for label in ("sharpebench/canonical-json/v1", "legacy"):
            with self.subTest(label=label), TemporaryDirectory() as directory:
                root = Path(directory) / "arena"
                root.mkdir()
                source = _closed_field(root, "sharpe.forecast-evidence.v2", label)
                importer.import_field(source, Path(directory) / "imported")

    def test_wrong_envelope_digest_labels_are_refused(self):
        for schema, label in [("sharpe.forecast-evidence.v2", None),
                              ("sharpe.forecast-evidence.v2", "invented"),
                              ("sharpe.forecast-evidence.v1", "legacy")]:
            with self.subTest(schema=schema, label=label), TemporaryDirectory() as directory:
                root = Path(directory) / "arena"
                root.mkdir()
                source = _closed_field(root, schema, label)
                with self.assertRaisesRegex(importer.ProspectiveImportError, "digest encoding"):
                    importer.import_field(source, Path(directory) / "imported")

    def test_rehashing_resolved_rewrites_cannot_replace_sealed_fields(self):
        for field in ("identity", "contracts", "revisions"):
            with self.subTest(field=field), TemporaryDirectory() as directory:
                root = Path(directory) / "arena"
                root.mkdir()
                source = _closed_field(root)
                path = source / "resolved/fixture-agent.json"
                document = json.loads(path.read_text())
                if field == "identity":
                    document[field]["model_id"] = "different-model"
                elif field == "contracts":
                    document[field][0]["question"] = "different-question"
                else:
                    document[field][0]["prediction"] = [1.0]
                _write_json(path, document)
                manifest_path = source / "resolution-manifest.json"
                manifest = json.loads(manifest_path.read_text())
                manifest["files"]["resolved/fixture-agent.json"] = importer._sha256(path.read_bytes())
                _write_json(manifest_path, manifest)
                _git(root, "add", "paper/evidence/prospective-forecast-field")
                _git(root, "commit", "--quiet", "-m", "rewritten resolved ledger")
                with self.assertRaisesRegex(importer.ProspectiveImportError, "sealed forecast"):
                    importer.import_field(source, Path(directory) / "imported")

    def test_closed_committed_field_imports_with_a_source_receipt(self) -> None:
        with TemporaryDirectory() as directory:
            root = Path(directory) / "arena"
            root.mkdir()
            source = _closed_field(root)
            output = Path(directory) / "bench-field"

            manifest = importer.import_field(source, output)

            self.assertEqual(manifest["schema_version"], importer.IMPORT_SCHEMA)
            self.assertEqual(manifest["source_commit"], _git(root, "rev-parse", "HEAD"))
            self.assertEqual(
                manifest["source_repository"], "https://example.invalid/arena.git"
            )
            self.assertEqual(
                set(manifest["files"]),
                importer._expected_paths(["fixture-agent"])[2],
            )
            self.assertTrue((output / "source-manifest.json").is_file())

    def test_a_plan_declaring_no_contracts_is_refused(self) -> None:
        # Aimed at the inventory check specifically. A whole-import fixture
        # never reaches it: the closed-file-set comparison refuses first, so a
        # test driving import_field would pass with this check deleted and
        # prove nothing about it.
        #
        # The check matters because every downstream verification compares an
        # agent's claim list against the contract set derived from the plan.
        # With no contracts that comparison is trivially satisfied, so a plan
        # declaring zero forecasts would verify as a closed field having
        # checked nothing. The message always said both inventories were
        # required; only the model list was checked.
        with TemporaryDirectory() as directory:
            source = Path(directory) / "prospective-forecast-field"
            source.mkdir()
            plan = {
                "schema_version": importer.PLAN_SCHEMA,
                "models": [{"agent_id": "fixture-agent"}],
                "contracts": [],
            }
            _write_json(source / "field-plan.json", plan)

            with self.assertRaises(importer.ProspectiveImportError) as caught:
                importer.verify_source(source)
            self.assertIn("inventory", str(caught.exception))

    def test_changed_or_incomplete_source_is_refused_without_output(self) -> None:
        with TemporaryDirectory() as directory:
            root = Path(directory) / "arena"
            root.mkdir()
            source = _closed_field(root)
            output = Path(directory) / "bench-field"
            resolved = source / "resolved/fixture-agent.json"
            document = json.loads(resolved.read_text(encoding="utf-8"))
            document["resolutions"][0]["outcome"] = 0.0
            _write_json(resolved, document)

            with self.assertRaisesRegex(
                importer.ProspectiveImportError, "resolution manifest digest mismatch"
            ):
                importer.import_field(source, output)

            self.assertFalse(output.exists())


if __name__ == "__main__":
    unittest.main()
