"""Canonical trust policy shared by the provenance writer and validator."""

from __future__ import annotations

import hashlib
import os
import re
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
    "arena/**/*.json",
)
ARTIFACT_SCOPE = ("paper/evidence/final/*.jsonl", "paper/figures/*.pdf")
EXCLUDED_DIR_NAMES = ("target", ".venv", "__pycache__", "node_modules", ".git")
EXCLUDED_ARTIFACT_PREFIXES = ("llm-cache-", "llm-field-")
SCHEMA_VERSION = 3
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


def _record_problems(name: str, records: object) -> list[str]:
    if not isinstance(records, list):
        return [f"manifest rule: {name} must be a list"]
    problems: list[str] = []
    seen: set[str] = set()
    for index, record in enumerate(records):
        if not isinstance(record, dict) or set(record) != {"path", "sha256"}:
            problems.append(
                f"manifest rule: {name}[{index}] must have exactly path and sha256"
            )
            continue
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
    problems.extend(_record_problems("artifacts", manifest.get("artifacts")))
    for field in ("source_files", "artifacts"):
        entries = manifest.get(field)
        if isinstance(entries, list) and not entries:
            problems.append(f"manifest rule: {field} is empty; the scope bound nothing")
    return problems
