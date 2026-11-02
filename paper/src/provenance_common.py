"""Canonical trust policy shared by the provenance writer and validator."""

from __future__ import annotations

import hashlib
import os
import re
import subprocess
from functools import lru_cache
from pathlib import Path
from pathlib import PurePosixPath


SOURCE_SCOPE = (
    "Cargo.toml",
    "Cargo.lock",
    "crates/**/*.toml",
    "crates/**/*.rs",
    "crates/**/schema/*.json",
    "paper/src/*.py",
    "paper/evidence/*.py",
    "paper/main.tex",
    "paper/sections/*.tex",
    "paper/refs.bib",
    "data/*.csv",
    # arena/state.json is excluded: the daily clock rewrites it, so binding it
    # would invalidate the manifest between any merge and any tag.
    "arena/windows/**/*.json",
)
ARTIFACT_SCOPE = (
    "paper/evidence/final/*.jsonl",
    "paper/evidence/prospective-forecast-field/**/*.json",
    "paper/evidence/prospective-forecast-field/**/*.sha256",
    "paper/figures/*.pdf",
    # The forecast-quality tutorial's two committed reports, which the paper's
    # forecast-quality section reads and the README describes.
    "examples/forecast-quality/fixtures/**/report.json",
)
EXCLUDED_DIR_NAMES = ("target", ".venv", "__pycache__", "node_modules", ".git")
EXCLUDED_ARTIFACT_PREFIXES = ("llm-cache-", "llm-field-")
SCHEMA_VERSION = 4
REPRODUCTION_ENTRYPOINT = "commands in paper/sections/A-commands.tex"
NOTE = "Incomplete paid LLM runs are excluded from result provenance."
MANIFEST_FIELDS = frozenset(
    {
        "schema_version",
        "generated_at_head",
        "generated_at_head_dirty",
        "source_snapshot_sha256",
        "source_snapshot_scope",
        "source_snapshot_excludes",
        "artifact_scope",
        "artifact_excluded_prefixes",
        "reproduction_entrypoint",
        "note",
        "source_files",
        "artifacts",
    }
)
_HEX_DIGEST = re.compile(r"^[0-9a-f]{64}$")
_GIT_COMMIT = re.compile(r"^[0-9a-f]{40}(?:[0-9a-f]{24})?$")


def canonical_bytes(data: bytes) -> bytes:
    return data if b"\x00" in data else data.replace(b"\r\n", b"\n")


def digest_bytes(data: bytes, *, canonical_text: bool = False) -> str:
    if canonical_text:
        data = canonical_bytes(data)
    return hashlib.sha256(data).hexdigest()


def snapshot_digest(records: list[dict[str, str]]) -> str:
    payload = "".join(
        f"{item['sha256']}  {item['path']}\n" for item in records
    ).encode()
    return hashlib.sha256(payload).hexdigest()


def _matches(path: str, pattern: str) -> bool:
    """Match a repository-root glob while keeping ``*`` inside one component."""
    translated: list[str] = ["^"]
    index = 0
    while index < len(pattern):
        char = pattern[index]
        if char == "*" and index + 1 < len(pattern) and pattern[index + 1] == "*":
            index += 2
            if index < len(pattern) and pattern[index] == "/":
                translated.append("(?:.*/)?")
                index += 1
            else:
                translated.append(".*")
            continue
        if char == "*":
            translated.append("[^/]*")
        elif char == "?":
            translated.append("[^/]")
        else:
            translated.append(re.escape(char))
        index += 1
    translated.append("$")
    return re.fullmatch("".join(translated), path) is not None


@lru_cache(maxsize=None)
def _candidate_files(root: Path, excludes: frozenset[str]) -> tuple[Path, ...]:
    candidates: list[Path] = []
    for directory, names, files in os.walk(root):
        names[:] = [name for name in names if name not in excludes]
        base = Path(directory)
        for name in files:
            candidates.append(base / name)
    return tuple(candidates)


def matching_files(root: Path, pattern: str, excludes: frozenset[str]) -> list[Path]:
    """Expand ``pattern`` without descending into excluded build directories."""
    return [
        path
        for path in _candidate_files(root, excludes)
        if _matches(path.relative_to(root).as_posix(), pattern)
    ]


SOURCE_RECORD_FIELDS = frozenset({"path", "sha256"})
# An artifact also records the commit that produced its bytes. Unlike
# `generated_at_head`, which moves whenever the manifest is regenerated, it is
# carried forward while the bytes are unchanged, so it keeps naming the source
# state the artifact was produced from.
ARTIFACT_RECORD_FIELDS = frozenset({"path", "sha256", "produced_at"})


def _record_problems(
    name: str, records: object, fields: frozenset[str] = SOURCE_RECORD_FIELDS
) -> list[str]:
    if not isinstance(records, list):
        return [f"manifest rule: {name} must be a list"]
    problems: list[str] = []
    seen: set[str] = set()
    for index, record in enumerate(records):
        if not isinstance(record, dict) or set(record) != fields:
            problems.append(
                f"manifest rule: {name}[{index}] must have exactly "
                + ", ".join(sorted(fields))
            )
            continue
        if "produced_at" in fields and (
            not isinstance(record["produced_at"], str)
            or _GIT_COMMIT.fullmatch(record["produced_at"]) is None
        ):
            problems.append(
                f"manifest rule: {name}[{index}] produced_at is not a full commit id"
            )
        path = record["path"]
        digest = record["sha256"]
        posix = PurePosixPath(path) if isinstance(path, str) else None
        if (
            not isinstance(path, str)
            or not path
            or posix is None
            or posix.is_absolute()
            or posix.as_posix() != path
            or "\\" in path
            or any(char in path for char in "\x00\r\n")
            or ".." in posix.parts
        ):
            problems.append(f"manifest rule: {name}[{index}] has an invalid path")
        elif path in seen:
            problems.append(f"manifest rule: {name} repeats path {path}")
        else:
            seen.add(path)
        if not isinstance(digest, str) or _HEX_DIGEST.fullmatch(digest) is None:
            problems.append(f"manifest rule: {name}[{index}] has an invalid sha256")
    return problems


def manifest_rule_problems(manifest: object) -> list[str]:
    """Reject a manifest that attempts to redefine validator policy."""
    if not isinstance(manifest, dict):
        return ["manifest rule: top level must be an object"]
    canonical = {
        "schema_version": SCHEMA_VERSION,
        "source_snapshot_scope": list(SOURCE_SCOPE),
        "source_snapshot_excludes": list(EXCLUDED_DIR_NAMES),
        "artifact_scope": list(ARTIFACT_SCOPE),
        "artifact_excluded_prefixes": list(EXCLUDED_ARTIFACT_PREFIXES),
        "reproduction_entrypoint": REPRODUCTION_ENTRYPOINT,
        "note": NOTE,
    }
    problems = [
        f"manifest rule: {field} does not equal the canonical validator rule"
        for field, expected in canonical.items()
        if manifest.get(field) != expected
    ]
    if set(manifest) != MANIFEST_FIELDS:
        missing = sorted(MANIFEST_FIELDS - set(manifest))
        extra = sorted(set(manifest) - MANIFEST_FIELDS)
        problems.append(
            f"manifest rule: top-level fields differ; missing={missing}, extra={extra}"
        )
    head = manifest.get("generated_at_head")
    if not isinstance(head, str) or _GIT_COMMIT.fullmatch(head) is None:
        problems.append("manifest rule: generated_at_head is not a full commit id")
    if not isinstance(manifest.get("generated_at_head_dirty"), bool):
        problems.append("manifest rule: generated_at_head_dirty must be boolean")
    snapshot = manifest.get("source_snapshot_sha256")
    if not isinstance(snapshot, str) or _HEX_DIGEST.fullmatch(snapshot) is None:
        problems.append("manifest rule: source_snapshot_sha256 is not a sha256")
    problems.extend(_record_problems("source_files", manifest.get("source_files")))
    problems.extend(
        _record_problems("artifacts", manifest.get("artifacts"), ARTIFACT_RECORD_FIELDS)
    )
    for field in ("source_files", "artifacts"):
        entries = manifest.get(field)
        if isinstance(entries, list) and not entries:
            problems.append(f"manifest rule: {field} is empty; the scope bound nothing")
    return problems


class ProducedAtUnknown(ValueError):
    """No commit in this history holds an artifact's current bytes."""


def committed_blob(root: Path, commit: str, path: str) -> bytes | None:
    completed = subprocess.run(
        ["git", "cat-file", "blob", f"{commit}:{path}"],
        cwd=root,
        capture_output=True,
        check=False,
    )
    return completed.stdout if completed.returncode == 0 else None


def produced_at(
    root: Path, path: str, digest: str, previous: dict[str, dict] | None = None
) -> str:
    """The commit that produced the artifact at `path` with bytes `digest`.

    A previous manifest's record is carried forward while the bytes are
    unchanged, so regenerating the manifest never rebinds it. Otherwise it is
    the last commit that wrote `path`, provided that commit holds exactly these
    bytes; the manifest is generated from a clean tree, so new bytes are
    committed before it is. Anything else is refused rather than guessed.
    """
    record = (previous or {}).get(path)
    if record and record.get("sha256") == digest and record.get("produced_at"):
        return record["produced_at"]
    commit = subprocess.run(
        ["git", "log", "-1", "--format=%H", "--", path],
        cwd=root,
        capture_output=True,
        text=True,
        check=False,
    ).stdout.strip()
    blob = committed_blob(root, commit, path) if commit else None
    if blob is None or digest_bytes(blob) != digest:
        raise ProducedAtUnknown(
            f"{path}: no commit holds these bytes (last commit writing it: "
            f"{commit or 'none'}); commit the artifact before regenerating the manifest"
        )
    return commit
