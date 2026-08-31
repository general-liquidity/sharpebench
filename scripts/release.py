#!/usr/bin/env python3
"""Cut a SharpeBench release from an isolated, freshly fetched Git worktree.

The operator's checkout is not the release tree.  ``rehearse`` runs the exact
changelog -> cargo-release -> clean provenance-rebind sequence on a temporary
branch and deletes it; ``execute`` additionally creates the annotated tag and
atomically pushes the two release commits plus the tag to ``origin/main``.
"""

from __future__ import annotations

import argparse
import os
import re
import shutil
import subprocess
import sys
import tempfile
from datetime import UTC, datetime
from pathlib import Path
from typing import Callable


ROOT = Path(__file__).resolve().parents[1]
BASE_REF = "refs/remotes/origin/main"
WORKTREE_PREFIX = "sharpebench-release-"
_IDENTITY = {
    "GIT_AUTHOR_NAME": "SharpeBench Release",
    "GIT_AUTHOR_EMAIL": "release@general-liquidity.invalid",
    "GIT_COMMITTER_NAME": "SharpeBench Release",
    "GIT_COMMITTER_EMAIL": "release@general-liquidity.invalid",
}


class ReleaseError(RuntimeError):
    """A release precondition or step failed without publishing anything."""


def run(
    root: Path, *args: str, check: bool = True
) -> subprocess.CompletedProcess[str]:
    completed = subprocess.run(
        list(args),
        cwd=root,
        capture_output=True,
        text=True,
        check=False,
        env={**os.environ, **_IDENTITY},
    )
    if check and completed.returncode != 0:
        detail = (completed.stderr or completed.stdout).strip()
        raise ReleaseError(f"{' '.join(args)} failed ({completed.returncode}): {detail}")
    return completed


def git(root: Path, *args: str) -> str:
    return run(root, "git", *args).stdout.strip()


def git_bytes(root: Path, object_name: str) -> bytes:
    completed = subprocess.run(
        ["git", "show", object_name],
        cwd=root,
        capture_output=True,
        check=False,
        env={**os.environ, **_IDENTITY},
    )
    if completed.returncode != 0:
        raise ReleaseError(
            f"git show {object_name} failed: {completed.stderr.decode(errors='replace').strip()}"
        )
    return completed.stdout


def workspace_version(data: bytes) -> str:
    match = re.search(rb'(?m)^version\s*=\s*"([0-9]+\.[0-9]+\.[0-9]+)"', data)
    if match is None:
        raise ReleaseError("Cargo.toml has no workspace package version")
    return match.group(1).decode()


def next_version(current: str, bump: str) -> str:
    if re.fullmatch(r"[0-9]+\.[0-9]+\.[0-9]+", bump):
        return bump
    major, minor, patch = (int(part) for part in current.split("."))
    if bump == "major":
        return f"{major + 1}.0.0"
    if bump == "minor":
        return f"{major}.{minor + 1}.0"
    if bump == "patch":
        return f"{major}.{minor}.{patch + 1}"
    raise ReleaseError("bump must be patch, minor, major, or MAJOR.MINOR.PATCH")


def unreleased_section(text: str) -> str:
    heading = "## [Unreleased]\n"
    parts = text.split(heading, 1)
    if len(parts) != 2:
        return ""
    return parts[1].split("\n## [", 1)[0].strip()


def unreleased_entries(text: str) -> list[str]:
    section = unreleased_section(text)
    return [
        line
        for line in section.splitlines()
        if line.strip()
        and not line.lstrip().startswith("#")
        and re.match(r"^\[[^\]]+\]: \S", line) is None
    ]


def require_pushed_changelog(root: Path, base: str) -> None:
    local = (root / "CHANGELOG.md").read_text(encoding="utf-8")
    based = git_bytes(root, f"{base}:CHANGELOG.md").decode("utf-8")
    if unreleased_section(local) != unreleased_section(based):
        raise ReleaseError(
            "the working checkout's CHANGELOG.md [Unreleased] entries differ from "
            f"the release base {base}; commit and push the notes before cutting"
        )
    if not unreleased_entries(based):
        raise ReleaseError(
            "CHANGELOG.md [Unreleased] is empty at the release base; record at least "
            "one real entry before cutting"
        )


def prepare_changelog(root: Path, current: str, target: str) -> None:
    path = root / "CHANGELOG.md"
    text = path.read_text(encoding="utf-8")
    heading = "## [Unreleased]\n"
    if text.count(heading) != 1:
        raise ReleaseError("CHANGELOG.md must contain exactly one Unreleased heading")
    if not unreleased_entries(text):
        raise ReleaseError("CHANGELOG.md [Unreleased] contains no release entries")
    compare = (
        "[Unreleased]: https://github.com/general-liquidity/sharpebench/compare/"
        f"v{target}...HEAD"
    )
    text, replaced = re.subn(r"(?m)^\[Unreleased\]: .+$", compare, text, count=1)
    if replaced != 1:
        raise ReleaseError("CHANGELOG.md has no Unreleased comparison link")
    text = text.replace(
        heading,
        f"{heading}\n## [{target}] - {datetime.now(UTC).date().isoformat()}\n",
        1,
    )
    release_link = (
        f"[{target}]: https://github.com/general-liquidity/sharpebench/compare/"
        f"v{current}...v{target}\n"
    )
    marker = f"[{current}]:"
    index = text.find(marker)
    if index < 0:
        raise ReleaseError(f"CHANGELOG.md has no link definition for {current}")
    text = text[:index] + release_link + text[index:]
    path.write_text(text, encoding="utf-8", newline="\n")
    git(root, "add", "--", "CHANGELOG.md")
    git(root, "commit", "-m", f"docs(changelog): prepare v{target}", "--", "CHANGELOG.md")


def safe_to_delete(path: Path, parent: Path) -> bool:
    return bool(str(path)) and path != parent and parent in path.parents and not path.is_symlink()


def cleanup_worktree(
    *,
    is_present: Callable[[], bool],
    remove: Callable[[], None],
    deregister: Callable[[], None],
) -> tuple[str, ...]:
    if not is_present():
        deregister()
        return ("deregister",)
    try:
        remove()
    except OSError:
        return ("remove-failed",)
    if is_present():
        return ("remove-failed",)
    deregister()
    return ("remove", "deregister")


def remove_worktree(root: Path, parent: Path, tree: Path) -> None:
    if not safe_to_delete(tree, parent):
        raise ReleaseError(f"refusing to remove unsafe worktree path {tree}")
    steps = cleanup_worktree(
        is_present=tree.exists,
        remove=lambda: shutil.rmtree(tree),
        deregister=lambda: run(root, "git", "worktree", "prune", check=False),
    )
    if steps == ("remove-failed",):
        raise ReleaseError(f"release worktree remains at {tree}")
    if parent.exists() and not any(parent.iterdir()):
        parent.rmdir()


def resolve_base(root: Path, base_ref: str) -> str:
    completed = run(
        root, "git", "rev-parse", "--verify", f"{base_ref}^{{commit}}", check=False
    )
    if completed.returncode != 0:
        raise ReleaseError(f"release base {base_ref} does not resolve to a commit")
    return completed.stdout.strip()


def cut_release(root: Path, bump: str, current: str, target: str, branch: str) -> str:
    prepare_changelog(root, current, target)
    run(
        root,
        "cargo",
        "release",
        bump,
        "--execute",
        "--no-confirm",
        "--no-push",
        "--allow-branch",
        branch,
    )
    actual = workspace_version((root / "Cargo.toml").read_bytes())
    if actual != target:
        raise ReleaseError(f"cargo-release produced {actual}, expected {target}")

    run(root, sys.executable, "paper/src/make-provenance.py")
    git(root, "add", "--", "paper/evidence/provenance.json")
    git(
        root,
        "commit",
        "-m",
        f"chore(provenance): rebind after the {target} version bump",
        "--",
        "paper/evidence/provenance.json",
    )
    run(root, sys.executable, "paper/src/check-provenance.py")
    if git(root, "status", "--porcelain"):
        raise ReleaseError("release worktree is dirty after the provenance rebind")
    return git(root, "rev-parse", "HEAD")


def release(
    root: Path, bump: str, *, push: bool, base_ref: str, fetch: bool
) -> tuple[str, str]:
    if fetch:
        run(root, "git", "fetch", "--quiet", "origin", "main", "--tags")
    base = resolve_base(root, base_ref)
    require_pushed_changelog(root, base)
    current = workspace_version(git_bytes(root, f"{base}:Cargo.toml"))
    target = next_version(current, bump)
    tag = f"v{target}"
    if run(root, "git", "rev-parse", "--verify", tag, check=False).returncode == 0:
        raise ReleaseError(f"tag {tag} already exists")
    if push and run(root, "git", "ls-remote", "--exit-code", "--tags", "origin", tag, check=False).returncode == 0:
        raise ReleaseError(f"tag {tag} already exists on origin")

    branch = f"release-{tag}-{os.getpid()}"
    parent = Path(tempfile.mkdtemp(prefix=WORKTREE_PREFIX))
    tree = parent / tag
    try:
        # Include creation in the cleanup boundary. `git worktree add` can leave a
        # directory, registration, or branch behind when checkout fails midway.
        run(root, "git", "worktree", "add", "--quiet", "-b", branch, str(tree), base)
        head = cut_release(tree, bump, current, target, branch)
        if push:
            git(tree, "tag", "-a", tag, "-m", f"SharpeBench {tag}")
            run(
                tree,
                "git",
                "push",
                "--atomic",
                "origin",
                "HEAD:refs/heads/main",
                f"refs/tags/{tag}",
            )
        return head, target
    finally:
        remove_worktree(root, parent, tree)
        run(root, "git", "branch", "-D", branch, check=False)


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    subparsers = parser.add_subparsers(dest="command", required=True)
    for command in ("rehearse", "execute"):
        child = subparsers.add_parser(command)
        child.add_argument("bump")
        child.add_argument("--base-ref", default=BASE_REF)
        child.add_argument("--no-fetch", action="store_true")
    args = parser.parse_args()
    try:
        head, target = release(
            ROOT,
            args.bump,
            push=args.command == "execute",
            base_ref=args.base_ref,
            fetch=not args.no_fetch,
        )
    except ReleaseError as error:
        print(f"release refused: {error}", file=sys.stderr)
        return 2
    verb = "created and pushed" if args.command == "execute" else "rehearsed"
    print(f"OK: {verb} v{target} from isolated tree {head[:12]}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
