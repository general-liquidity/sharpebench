#!/usr/bin/env python3
"""Import one closed SharpeArena prospective field as committed paper evidence."""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import re
import shutil
import subprocess
import tempfile
from pathlib import Path
from typing import Any

PLAN_SCHEMA = "sharpearena.prospective-forecast-field-plan.v1"
COMMIT_SCHEMA = "sharpearena.prospective-forecast-commit.v1"
RESOLUTION_SCHEMA = "sharpearena.prospective-forecast-resolution.v1"
RESOLUTION_MANIFEST_SCHEMA = "sharpearena.prospective-forecast-resolution-manifest.v1"
IMPORT_SCHEMA = "sharpebench.prospective-forecast-field-import.v1"
AGENT_ID = re.compile(r"^[a-z0-9][a-z0-9._-]*$")
SHA256 = re.compile(r"^[0-9a-f]{64}$")


class ProspectiveImportError(RuntimeError):
    """The source is not a closed, committed prospective field."""


def _sha256(payload: bytes) -> str:
    return hashlib.sha256(payload).hexdigest()


def _canonical_sha256(value: object) -> str:
    payload = json.dumps(
        value, sort_keys=True, separators=(",", ":"), ensure_ascii=False
    ).encode("utf-8")
    return _sha256(payload)


def _read_json(path: Path) -> Any:
    try:
        return json.loads(
            path.read_text(encoding="utf-8"),
            parse_constant=lambda value: (_ for _ in ()).throw(ValueError(value)),
        )
    except (OSError, UnicodeError, json.JSONDecodeError, ValueError) as error:
        raise ProspectiveImportError(
            f"cannot read strict JSON from {path}: {error}"
        ) from error


def _mapping(value: object, label: str) -> dict[str, Any]:
    if not isinstance(value, dict):
        raise ProspectiveImportError(f"{label} must be a JSON object")
    return value


def _git(repo: Path, *arguments: str, binary: bool = False) -> str | bytes:
    process = subprocess.run(
        ["git", "-C", str(repo), *arguments],
        check=False,
        capture_output=True,
        text=not binary,
    )
    if process.returncode != 0:
        stderr = (
            process.stderr.decode("utf-8", errors="replace")
            if binary
            else process.stderr
        )
        raise ProspectiveImportError(
            f"git {' '.join(arguments)} failed: {stderr.strip()}"
        )
    return process.stdout


def _expected_paths(agent_ids: list[str]) -> tuple[set[str], set[str], set[str]]:
    pending = {f"pending/{agent_id}.json" for agent_id in agent_ids}
    inference = {f"inference/{agent_id}.json" for agent_id in agent_ids}
    resolved = {f"resolved/{agent_id}.json" for agent_id in agent_ids}
    sealed = {
        "field-plan.json",
        "field-plan.sha256",
        "observation.json",
        *pending,
        *inference,
    }
    resolution = {"resolution.json", *resolved}
    complete = {
        *sealed,
        "forecast-commit.json",
        *resolution,
        "resolution-manifest.json",
    }
    return sealed, resolution, complete


def _verify_digest_map(
    source: Path, value: object, expected_paths: set[str], label: str
) -> None:
    files = _mapping(value, f"{label}.files")
    if set(files) != expected_paths:
        raise ProspectiveImportError(f"{label} names the wrong closed file set")
    for relative, expected in files.items():
        if not isinstance(expected, str) or SHA256.fullmatch(expected) is None:
            raise ProspectiveImportError(
                f"{label} has an invalid digest for {relative}"
            )
        path = source / relative
        if not path.is_file() or _sha256(path.read_bytes()) != expected:
            raise ProspectiveImportError(f"{label} digest mismatch: {relative}")


def _verify_resolved_ledgers(
    source: Path, agent_ids: list[str], contract_ids: list[str]
) -> None:
    reference_resolutions: list[dict[str, Any]] | None = None
    for agent_id in agent_ids:
        document = _mapping(
            _read_json(source / "resolved" / f"{agent_id}.json"),
            f"resolved ledger {agent_id}",
        )
        identity = _mapping(document.get("identity"), f"{agent_id}.identity")
        resolutions = document.get("resolutions")
        revisions = document.get("revisions")
        if (
            document.get("schema_version") != "sharpe.forecast-evidence.v1"
            or identity.get("agent_id") != agent_id
            or not isinstance(resolutions, list)
            or not isinstance(revisions, list)
        ):
            raise ProspectiveImportError(
                f"resolved ledger has the wrong envelope: {agent_id}"
            )
        if [record.get("claim_id") for record in resolutions] != contract_ids or any(
            record.get("status") != "resolved" for record in resolutions
        ):
            raise ProspectiveImportError(
                f"resolved support is incomplete or reordered: {agent_id}"
            )
        eligible = [
            revision.get("claim_id")
            for revision in revisions
            if revision.get("status") == "eligible"
        ]
        if eligible != contract_ids:
            raise ProspectiveImportError(
                f"forecast support differs from the plan: {agent_id}"
            )
        if reference_resolutions is None:
            reference_resolutions = resolutions
        elif resolutions != reference_resolutions:
            raise ProspectiveImportError(
                "agents do not share byte-equivalent resolution records"
            )


def verify_source(source: Path) -> tuple[Path, str, str, list[str]]:
    source = source.resolve()
    plan = _mapping(_read_json(source / "field-plan.json"), "field plan")
    if plan.get("schema_version") != PLAN_SCHEMA:
        raise ProspectiveImportError("field plan has an unsupported schema")
    models = plan.get("models")
    contracts = plan.get("contracts")
    if not isinstance(models, list) or not models or not isinstance(contracts, list):
        raise ProspectiveImportError("field plan has no model or contract inventory")
    if any(not isinstance(record, dict) for record in models):
        raise ProspectiveImportError("field plan model inventory must contain objects")
    raw_agent_ids = [record.get("agent_id") for record in models]
    if any(
        not isinstance(value, str) or AGENT_ID.fullmatch(value) is None
        for value in raw_agent_ids
    ):
        raise ProspectiveImportError("field plan has an invalid agent ID")
    agent_ids = sorted(raw_agent_ids)
    if len(set(agent_ids)) != len(agent_ids):
        raise ProspectiveImportError("field plan repeats an agent ID")
    if any(not isinstance(record, dict) for record in contracts):
        raise ProspectiveImportError(
            "field plan contract inventory must contain objects"
        )
    contract_ids = [record.get("contract_id") for record in contracts]
    if any(not isinstance(value, str) or not value for value in contract_ids):
        raise ProspectiveImportError("field plan has an invalid contract ID")
    if len(set(contract_ids)) != len(contract_ids):
        raise ProspectiveImportError("field plan repeats a contract ID")

    plan_sha256 = _canonical_sha256(plan)
    try:
        recorded_plan_sha256 = (
            (source / "field-plan.sha256").read_text(encoding="utf-8").strip()
        )
    except (OSError, UnicodeError) as error:
        raise ProspectiveImportError(
            f"cannot read field-plan.sha256: {error}"
        ) from error
    if recorded_plan_sha256 != plan_sha256:
        raise ProspectiveImportError(
            "field-plan.sha256 does not match canonical plan bytes"
        )

    sealed_paths, resolution_paths, complete_paths = _expected_paths(agent_ids)
    actual_paths = {
        path.relative_to(source).as_posix()
        for path in source.rglob("*")
        if path.is_file()
    }
    if actual_paths != complete_paths:
        raise ProspectiveImportError(
            "source directory differs from the closed field file set"
        )

    forecast_commit = _mapping(
        _read_json(source / "forecast-commit.json"), "forecast commit"
    )
    if (
        forecast_commit.get("schema_version") != COMMIT_SCHEMA
        or forecast_commit.get("field_plan_sha256") != plan_sha256
    ):
        raise ProspectiveImportError("forecast commit differs from the field plan")
    _verify_digest_map(
        source, forecast_commit.get("files"), sealed_paths, "forecast commit"
    )

    forecast_commit_sha256 = _sha256((source / "forecast-commit.json").read_bytes())
    resolution = _mapping(_read_json(source / "resolution.json"), "resolution")
    if (
        resolution.get("schema_version") != RESOLUTION_SCHEMA
        or resolution.get("field_plan_sha256") != plan_sha256
        or resolution.get("forecast_commit_sha256") != forecast_commit_sha256
    ):
        raise ProspectiveImportError(
            "resolution does not bind the frozen forecast commit"
        )
    resolution_manifest = _mapping(
        _read_json(source / "resolution-manifest.json"), "resolution manifest"
    )
    if resolution_manifest.get("schema_version") != RESOLUTION_MANIFEST_SCHEMA:
        raise ProspectiveImportError("resolution manifest has an unsupported schema")
    _verify_digest_map(
        source,
        resolution_manifest.get("files"),
        resolution_paths,
        "resolution manifest",
    )
    _verify_resolved_ledgers(source, agent_ids, contract_ids)

    repository = Path(str(_git(source, "rev-parse", "--show-toplevel")).strip())
    source_relative = source.relative_to(repository).as_posix()
    commit = str(_git(repository, "rev-parse", "HEAD")).strip()
    for relative in sorted(complete_paths):
        committed = _git(
            repository,
            "show",
            f"{commit}:{source_relative}/{relative}",
            binary=True,
        )
        if committed != (source / relative).read_bytes():
            raise ProspectiveImportError(
                f"source file differs from commit {commit}: {relative}"
            )
    return repository, commit, plan_sha256, sorted(complete_paths)


def import_field(source: Path, target: Path) -> dict[str, Any]:
    repository, commit, plan_sha256, paths = verify_source(source)
    source = source.resolve()
    target = target.resolve()
    if target.exists():
        raise ProspectiveImportError(f"output already exists: {target}")
    target.parent.mkdir(parents=True, exist_ok=True)
    staging = Path(tempfile.mkdtemp(prefix=f".{target.name}.", dir=target.parent))
    try:
        for relative in paths:
            destination = staging / relative
            destination.parent.mkdir(parents=True, exist_ok=True)
            shutil.copyfile(source / relative, destination)
        remote = str(_git(repository, "remote", "get-url", "origin")).strip()
        manifest = {
            "schema_version": IMPORT_SCHEMA,
            "source_repository": remote,
            "source_commit": commit,
            "source_path": source.relative_to(repository).as_posix(),
            "field_plan_sha256": plan_sha256,
            "files": {
                relative: _sha256((staging / relative).read_bytes())
                for relative in paths
            },
        }
        (staging / "source-manifest.json").write_text(
            json.dumps(manifest, indent=2, sort_keys=True) + "\n",
            encoding="utf-8",
            newline="\n",
        )
        os.replace(staging, target)
    except BaseException:
        shutil.rmtree(staging, ignore_errors=True)
        raise
    return manifest


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--source", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    arguments = parser.parse_args()
    try:
        manifest = import_field(arguments.source, arguments.output)
    except ProspectiveImportError as error:
        parser.error(str(error))
    print(
        f"imported {len(manifest['files'])} committed field files from "
        f"{manifest['source_commit']}"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
