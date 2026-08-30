"""Validate the paper evidence manifest against the complete recorded scope."""

from __future__ import annotations

import hashlib
import json
import re
import subprocess
import sys
from pathlib import Path


ROOT = Path(__file__).resolve().parents[2]
MANIFEST = ROOT / "paper" / "evidence" / "provenance.json"


def sha256(path: Path, *, canonical_text: bool = False) -> str:
    data = path.read_bytes()
    if canonical_text:
        data = data.replace(b"\r\n", b"\n")
    return hashlib.sha256(data).hexdigest()


def match(pattern: str, excludes: frozenset[str]) -> list[Path]:
    return [
        path
        for path in ROOT.glob(pattern)
        if path.is_file()
        and not any(part in excludes for part in path.relative_to(ROOT).parts)
    ]


def expand(patterns: list[str], excludes: frozenset[str]) -> list[str]:
    found: set[Path] = set()
    for pattern in patterns:
        found.update(match(pattern, excludes))
    return sorted(path.relative_to(ROOT).as_posix() for path in found)


def check_scope_is_live(name: str, patterns: list[str], excludes: frozenset[str]) -> list[str]:
    """A recorded pattern that now matches nothing has stopped guarding anything.

    The digest and unrecorded-file legs below are both driven by these
    expansions, so a pattern that goes empty removes files from the checked set
    without removing anything from the manifest, and the run still reports OK.
    A whole scope could be emptied by one renamed directory and this validator
    would pass on a tree it had checked nothing in.
    """
    return [
        f"{name}: DEAD PATTERN {pattern} matches no file, so it guards nothing. "
        "Either the path moved (fix it) or the pattern is dead (drop it and "
        "regenerate with paper/src/make-provenance.py)."
        for pattern in patterns
        if not match(pattern, excludes)
    ]


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


def git(*args: str) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        ["git", *args], cwd=ROOT, capture_output=True, text=True, check=False
    )


def check_generation(manifest: dict) -> list[str]:
    """Validate the two fields that bind the manifest to a commit.

    `generated_at_head_dirty` is a property of the generation, not of the tree
    the validator is run against, so it is enforced unconditionally: a manifest
    generated from a dirty worktree describes source bytes that are in no
    commit, and accepting it would let uncommitted work pass as recorded.

    `generated_at_head` cannot equal the commit that contains the manifest -
    that would be self-referential - so the strongest available check is
    ancestry: the recorded commit must be reachable from HEAD. Enforcing that
    needs the recorded object, which a shallow clone does not have, so the
    paper-provenance job checks out full history. Outside a Git worktree the
    ancestry leg is reported as unenforced rather than silently passed.
    """
    problems: list[str] = []

    if manifest.get("generated_at_head_dirty") is not False:
        problems.append(
            "generation: DIRTY generated_at_head_dirty is "
            f"{manifest.get('generated_at_head_dirty')!r}, expected false. The "
            "manifest was generated from a worktree with uncommitted changes, so "
            "its source digests correspond to no commit. Commit the sources, then "
            "regenerate with paper/src/make-provenance.py."
        )

    recorded = manifest.get("generated_at_head")
    if not isinstance(recorded, str) or not re.fullmatch(r"[0-9a-f]{40}", recorded):
        problems.append(f"generation: HEAD generated_at_head is not a commit id: {recorded!r}")
        return problems

    if git("rev-parse", "--git-dir").returncode != 0:
        print(
            "generation: not a Git worktree, so generated_at_head ancestry is NOT "
            f"enforced here (recorded {recorded}); the dirty flag is enforced."
        )
        return problems

    if git("cat-file", "-e", f"{recorded}^{{commit}}").returncode != 0:
        shallow = git("rev-parse", "--is-shallow-repository").stdout.strip() == "true"
        problems.append(
            f"generation: HEAD generated_at_head {recorded} is not a commit in this "
            + (
                "repository, and the clone is shallow. Fetch full history "
                "(fetch-depth: 0) before validating."
                if shallow
                else "repository."
            )
        )
        return problems

    if git("merge-base", "--is-ancestor", recorded, "HEAD").returncode != 0:
        head = git("rev-parse", "HEAD").stdout.strip()
        problems.append(
            f"generation: HEAD generated_at_head {recorded} is not an ancestor of "
            f"HEAD {head}. The manifest was generated on a commit this one does not "
            "build on, so it does not describe this history."
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

    problems = check_generation(manifest)
    problems += check_group(
        "source", manifest["source_files"], canonical_text=True
    )
    problems += check_group("artifact", manifest["artifacts"])
    excludes = frozenset(manifest["source_snapshot_excludes"])
    problems += check_scope_is_live(
        "source", manifest["source_snapshot_scope"], excludes
    )
    problems += check_scope_is_live("artifact", manifest["artifact_scope"], excludes)

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
