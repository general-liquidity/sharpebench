#!/usr/bin/env python3
"""Require every Lean module under formal/SharpeBenchFormal/ to declare its scope.

A module's doc comment must carry a ``## Scope`` block with a ``Covers:`` line naming the
production rules it models and an ``Assumes:`` line naming the assumptions the model rests
on, and at least one backticked repository path in the block must exist. A proof whose
scope is not written down reads as broader than it is; this check keeps every module's
coverage statement next to the theorems and fails CI when it is missing or points at no
path that still exists.

Passing proves that the block is present and that a named path exists. It does not prove
that the model still corresponds to the code at that path.

Exit 0 when every module declares scope, 1 otherwise.
"""

from __future__ import annotations

import pathlib
import re
import sys

ROOT = pathlib.Path(__file__).resolve().parents[1]
FORMAL = ROOT / "formal" / "SharpeBenchFormal"
SCOPE_BLOCK = re.compile(r"^## Scope[ \t]*$(.*?)(?=^## |^-/|\Z)", re.M | re.S)
REPO_PATH = re.compile(r"`([A-Za-z0-9_][A-Za-z0-9_./-]*\.[A-Za-z0-9]+)`")


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
    paths = REPO_PATH.findall(body)
    if not any((ROOT / p).exists() for p in paths):
        errors.append(
            f"{rel}: '## Scope' block references no existing repository path "
            f"(found {paths or 'none'})"
        )
    return errors


def main() -> int:
    modules = sorted(FORMAL.glob("*.lean"))
    if not modules:
        print(f"no Lean modules found under {FORMAL}", file=sys.stderr)
        return 1
    errors = [error for module in modules for error in check(module)]
    for error in errors:
        print(error, file=sys.stderr)
    if errors:
        return 1
    print(f"OK: {len(modules)} Lean module(s) declare their scope")
    return 0


if __name__ == "__main__":
    sys.exit(main())
