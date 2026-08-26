"""Validate the paper evidence manifest against the complete recorded scope."""

from __future__ import annotations

import hashlib
import json
import sys
from pathlib import Path


ROOT = Path(__file__).resolve().parents[2]
MANIFEST = ROOT / "paper" / "evidence" / "provenance.json"


def sha256(path: Path, *, canonical_text: bool = False) -> str:
    data = path.read_bytes()
    if canonical_text:
        data = data.replace(b"\r\n", b"\n")
    return hashlib.sha256(data).hexdigest()


def expand(patterns: list[str], excludes: frozenset[str]) -> list[str]:
    found: set[Path] = set()
    for pattern in patterns:
        found.update(
            path
            for path in ROOT.glob(pattern)
            if path.is_file()
            and not any(part in excludes for part in path.relative_to(ROOT).parts)
        )
    return sorted(path.relative_to(ROOT).as_posix() for path in found)


def check_group(
    name: str, records: list[dict[str, str]], *, canonical_text: bool = False
) -> list[str]:
    problems: list[str] = []
    for record in records:
        path = ROOT / record["path"]
        if not path.is_file():
            problems.append(f"{name}: MISSING {record['path']}")
            continue
        actual = sha256(path, canonical_text=canonical_text)
        if actual != record["sha256"]:
            problems.append(
                f"{name}: DIGEST {record['path']}\n"
                f"    recorded {record['sha256']}\n"
                f"    actual   {actual}"
            )
    return problems


def main() -> int:
    if not MANIFEST.is_file():
        print(f"missing manifest: {MANIFEST}", file=sys.stderr)
        return 2
    manifest = json.loads(MANIFEST.read_text(encoding="utf-8"))
    if manifest.get("schema_version") != 3:
        print("unsupported provenance schema_version", file=sys.stderr)
        return 2

    problems = check_group(
        "source", manifest["source_files"], canonical_text=True
    )
    problems += check_group("artifact", manifest["artifacts"])
    excludes = frozenset(manifest["source_snapshot_excludes"])

    recorded_sources = {item["path"] for item in manifest["source_files"]}
    for path in expand(manifest["source_snapshot_scope"], excludes):
        if path not in recorded_sources:
            problems.append(f"source: UNRECORDED {path}")

    prefixes = tuple(manifest["artifact_excluded_prefixes"])
    recorded_artifacts = {item["path"] for item in manifest["artifacts"]}
    for path in expand(manifest["artifact_scope"], excludes):
        if Path(path).name.startswith(prefixes):
            continue
        if path not in recorded_artifacts:
            problems.append(f"artifact: UNRECORDED {path}")

    snapshot = hashlib.sha256(
        "".join(
            f"{item['sha256']}  {item['path']}\n" for item in manifest["source_files"]
        ).encode()
    ).hexdigest()
    if snapshot != manifest["source_snapshot_sha256"]:
        problems.append(
            "snapshot: DIGEST source_snapshot_sha256\n"
            f"    recorded {manifest['source_snapshot_sha256']}\n"
            f"    actual   {snapshot}"
        )

    if problems:
        print("\n".join(problems))
        print(f"\nFAIL: {len(problems)} provenance problem(s)")
        return 1
    print(
        f"OK: {len(recorded_sources)} sources and "
        f"{len(recorded_artifacts)} artifacts match the tree"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
