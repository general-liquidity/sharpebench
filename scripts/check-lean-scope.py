#!/usr/bin/env python3
"""Require every Lean module in the SharpeBenchFormal library to declare its scope.

A module's doc comment must carry a ``## Scope`` block with a ``Covers:`` line naming the
production rules it models and an ``Assumes:`` line naming the assumptions the model rests
on, and every backticked repository path in the block must exist. A proof whose scope is not
written down reads as broader than it is; this check keeps every module's coverage statement
next to the theorems and fails CI when it is missing or names a path that no longer exists.

The library is the root module ``formal/SharpeBenchFormal.lean`` together with every ``.lean``
file under ``formal/SharpeBenchFormal/``, at any depth, so a new module cannot escape the
requirement by sitting in a subdirectory or by being the root.

A repository path is written relative to the repository root, so it contains a directory
separator. A backticked token without one is prose (a Lean identifier, a Rust constant, a
version string) and is not checked.

Passing proves that the block is present and that every path it names exists. It does not
prove that the model still corresponds to the code at those paths.

Exit 0 when every module declares scope, 1 otherwise.
"""

from __future__ import annotations

import pathlib
import re
import sys

ROOT = pathlib.Path(__file__).resolve().parents[1]
FORMAL = ROOT / "formal"
LIBRARY = "SharpeBenchFormal"
SCOPE_BLOCK = re.compile(r"^## Scope[ \t]*$(.*?)(?=^## |^-/|\Z)", re.M | re.S)
REPO_PATH = re.compile(r"`([A-Za-z0-9_][A-Za-z0-9_./-]*\.[A-Za-z0-9]+)`")


def named_paths(body: str) -> list[str]:
    found = dict.fromkeys(REPO_PATH.findall(body))
    return [path for path in found if "/" in path]


def check(path: pathlib.Path) -> list[str]:
    rel = path.relative_to(ROOT).as_posix()
    text = path.read_text(encoding="utf-8")
    block = SCOPE_BLOCK.search(text)
    if block is None:
        return [f"{rel}: missing a '## Scope' block in the module doc comment"]
    body = block.group(1)
    errors = []
    for key in ("Covers:", "Assumes:"):
        if re.search(rf"^\s*(?:- )?{re.escape(key)}", body, re.M) is None:
            errors.append(f"{rel}: '## Scope' block has no '{key}' line")
    paths = named_paths(body)
    if not paths:
        errors.append(f"{rel}: '## Scope' block names no repository path")
    for named in paths:
        if not (ROOT / named).exists():
            errors.append(f"{rel}: '## Scope' block names a path that does not exist: {named}")
    return errors


def modules() -> list[pathlib.Path]:
    root_module = FORMAL / f"{LIBRARY}.lean"
    found = sorted((FORMAL / LIBRARY).rglob("*.lean"))
    return ([root_module] if root_module.exists() else []) + found


def main() -> int:
    found = modules()
    if not found:
        print(f"no Lean modules found for {LIBRARY} under {FORMAL}", file=sys.stderr)
        return 1
    errors = [error for module in found for error in check(module)]
    for error in errors:
        print(error, file=sys.stderr)
    if errors:
        return 1
    print(f"OK: {len(found)} Lean module(s) declare their scope")
    return 0


if __name__ == "__main__":
    sys.exit(main())
