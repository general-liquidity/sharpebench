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


def _closed_field(root: Path) -> Path:
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
    _write_json(source / "pending/fixture-agent.json", {"pending": True})
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
            "schema_version": "sharpe.forecast-evidence.v1",
            "identity": {"agent_id": "fixture-agent"},
            "revisions": [{"claim_id": "fixture-contract", "status": "eligible"}],
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
