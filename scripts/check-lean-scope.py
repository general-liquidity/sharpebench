#!/usr/bin/env python3
"""Require every Lean module in the SharpeBenchFormal library to declare its scope, and keep the
names its documentation uses true.

A module's doc comment must carry a ``## Scope`` block with a ``Covers:`` line naming the
production rules it models and an ``Assumes:`` line naming the assumptions the model rests on. A
proof whose scope is not written down reads as broader than it is. The check fails CI when:

- a module has no block, or its block lacks ``Covers:`` or ``Assumes:``, or names no path;
- the block names a repository path (a backticked token containing ``/``) that does not exist;
- the block names a production symbol (a backticked identifier that is not a Lean name the
  library's own code uses) that occurs, as a whole word, in none of the non-Lean files the block
  names, so a renamed or deleted Rust or Python function cannot leave the model claiming to cover
  it;
- a theorem listed under the module's ``## Main results`` is not declared as a theorem in it;
- a theorem of the library is not named in the non-vacuity module, which gives each theorem a
  satisfiable instance of its hypotheses and a wrong definition its property refutes.

The library is the root module ``formal/SharpeBenchFormal.lean`` together with every ``.lean``
file under ``formal/SharpeBenchFormal/``, at any depth, so a new module cannot escape the
requirement by sitting in a subdirectory or by being the root.

Passing proves that the names exist where the documentation says they are. It does not prove that
the model still corresponds to the code at those names: the shared conformance cases and the
statement manifest are the checks on that.

Exit 0 when every check passes, 1 otherwise.
"""

from __future__ import annotations

import pathlib
import re
import sys

ROOT = pathlib.Path(__file__).resolve().parents[1]
LIBRARY = "SharpeBenchFormal"
NON_VACUITY = "NonVacuity.lean"
# Modules that state no theorem of their own: the case table checker and the non-vacuity module.
NO_THEOREMS = {NON_VACUITY, "Cases.lean"}
SCOPE_BLOCK = re.compile(r"^## Scope[ \t]*$(.*?)(?=^## |^-/|\Z)", re.M | re.S)
MAIN_RESULTS = re.compile(r"^## Main results[ \t]*$(.*?)(?=^## |^-/|\Z)", re.M | re.S)
MAIN_RESULT = re.compile(r"^- `([A-Za-z_][A-Za-z0-9_'.]*)`", re.M)
REPO_PATH = re.compile(r"`([A-Za-z0-9_][A-Za-z0-9_./-]*\.[A-Za-z0-9]+)`")
IDENTIFIER = re.compile(r"`([A-Za-z_][A-Za-z0-9_]*)`")
LEAN_WORD = re.compile(r"[A-Za-z_][A-Za-z0-9_']*")
THEOREM = re.compile(r"^\s*(?:@\[[^\]]*\]\s*)?(?:private\s+|protected\s+)?theorem\s+([^\s(:{\[]+)", re.M)


def named_paths(body: str) -> list[str]:
    found = dict.fromkeys(REPO_PATH.findall(body))
    return [path for path in found if "/" in path]


def strip_comments(text: str) -> str:
    """Lean source without block comments (nested) or line comments."""
    out, depth, i = [], 0, 0
    while i < len(text):
        pair = text[i : i + 2]
        if pair == "/-":
            depth, i = depth + 1, i + 2
        elif pair == "-/" and depth:
            depth, i = depth - 1, i + 2
        elif depth:
            i += 1
        elif pair == "--":
            end = text.find("\n", i)
            i = len(text) if end < 0 else end
        else:
            out.append(text[i])
            i += 1
    return "".join(out)


def lean_names(sources: list[str]) -> set[str]:
    names: set[str] = set()
    for text in sources:
        for word in LEAN_WORD.findall(strip_comments(text)):
            names.update(part for part in word.split(".") if part)
    return names


def declared_theorems(text: str) -> list[str]:
    return [name.rsplit(".", 1)[-1] for name in THEOREM.findall(strip_comments(text))]


def check(root: pathlib.Path, path: pathlib.Path, lean: set[str]) -> list[str]:
    rel = path.relative_to(root).as_posix()
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
        if not (root / named).exists():
            errors.append(f"{rel}: '## Scope' block names a path that does not exist: {named}")
    production = [
        root / named
        for named in paths
        if (root / named).is_file() and not named.endswith(".lean")
    ]
    corpus = "\n".join(file.read_text(encoding="utf-8", errors="replace") for file in production)
    for symbol in dict.fromkeys(IDENTIFIER.findall(body)):
        if symbol in lean:
            continue
        if re.search(rf"(?<![A-Za-z0-9_]){re.escape(symbol)}(?![A-Za-z0-9_])", corpus) is None:
            errors.append(
                f"{rel}: '## Scope' block names production symbol '{symbol}', which occurs in no "
                "non-Lean file the block names"
            )
    results = MAIN_RESULTS.search(text)
    if results is not None:
        declared = set(declared_theorems(text))
        for name in MAIN_RESULT.findall(results.group(1)):
            if name not in declared:
                errors.append(f"{rel}: '## Main results' lists '{name}', which it does not declare")
    return errors


def modules(root: pathlib.Path) -> list[pathlib.Path]:
    formal = root / "formal"
    root_module = formal / f"{LIBRARY}.lean"
    found = sorted((formal / LIBRARY).rglob("*.lean"))
    return ([root_module] if root_module.exists() else []) + found


def non_vacuity_errors(root: pathlib.Path, found: list[pathlib.Path]) -> list[str]:
    witness = root / "formal" / LIBRARY / NON_VACUITY
    if not witness.exists():
        return [f"{witness.relative_to(root).as_posix()}: missing; every theorem needs a witness"]
    covered = set(re.findall(r"`([A-Za-z_][A-Za-z0-9_']*)`", witness.read_text(encoding="utf-8")))
    errors = []
    for module in found:
        if module.name in NO_THEOREMS:
            continue
        for name in declared_theorems(module.read_text(encoding="utf-8")):
            if name not in covered:
                errors.append(
                    f"{module.relative_to(root).as_posix()}: theorem '{name}' has no witness or "
                    f"mutant in {NON_VACUITY}"
                )
    return errors


def run(root: pathlib.Path) -> list[str]:
    found = modules(root)
    if not found:
        return [f"no Lean modules found for {LIBRARY} under {root / 'formal'}"]
    lean = lean_names([module.read_text(encoding="utf-8") for module in found])
    errors = [error for module in found for error in check(root, module, lean)]
    return errors + non_vacuity_errors(root, found)


def main() -> int:
    errors = run(ROOT)
    for error in errors:
        print(error, file=sys.stderr)
    if errors:
        return 1
    print(f"OK: {len(modules(ROOT))} Lean module(s) declare their scope and keep their names true")
    return 0


if __name__ == "__main__":
    sys.exit(main())
