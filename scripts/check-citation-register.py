#!/usr/bin/env python3
"""Validate the citation register against the bibliography, the paper and the code.

The evidence register (``check-evidence-register.py``) binds the paper's own
claims to the artifacts that produced them. Nothing bound the paper's external
citations, or the credit lines in the source code that name the work a
mechanism was taken from, and the one recorded miscredit in this repository was
in source code, not in the paper. ``docs/citation-register.jsonl`` records both,
one JSON object per line, and this script fails when the register and the tree
disagree.

Bibliography entries (``"kind": "bib"``):

* every ``paper/refs.bib`` key has exactly one register entry, and every
  register entry names a key that exists;
* ``identifier`` is derived mechanically from the bibliography entry, in this
  order: ``doi:<doi>``, ``arXiv:<eprint>``, ``url:<url>``, else ``none``. The
  register cannot claim an identifier the bibliography does not carry;
* ``mechanism`` says what the paper credits the work for, and is not empty;
* ``locators`` lists every paper source file that cites the key, and only
  those, so a citation added to or removed from a file fails until the register
  says so. ``pages`` lists every page locator the paper gives the key in a
  citation's optional argument (``\\citep[p.~4]{key}``), and only those.

Code credit lines (``"kind": "credit"``):

* every comment line in the scanned source that credits a work (``After X``,
  ``Port of``, ``(after X``) is matched by an entry whose ``file`` is that
  file and whose ``text`` occurs on that line;
* every credit entry points at a file that exists and a credit line that still
  carries its ``text``;
* ``identifier`` is ``bib:<key>`` for a key in ``paper/refs.bib``, or
  ``unresolved`` with a ``note`` saying why. Unresolved credits do not fail the
  check, but they are counted and printed.

Nothing is fetched. Existence is checked against this repository only; whether
a source says what it is credited for is the citation-audit procedure's job,
once per submission.

Exit 0 when the register is consistent, 1 otherwise.
"""

from __future__ import annotations

import json
import pathlib
import re
import sys

ROOT = pathlib.Path(__file__).resolve().parents[1]
REGISTER = ROOT / "docs" / "citation-register.jsonl"
BIB = ROOT / "paper" / "refs.bib"

ENTRY = re.compile(r"@(\w+)\s*\{\s*([^,\s]+)\s*,(.*?)\n\}", re.S)
FIELD = re.compile(r"(\w+)\s*=\s*\{((?:[^{}]|\{[^{}]*\})*)\}", re.S)
CITE = re.compile(
    r"\\(?:cite|citet|citep|citealp|citealt|citeauthor|citeyear|nocite)\*?"
    r"((?:\[[^\]]*\])*)\{([^}]*)\}"
)
OPTIONAL = re.compile(r"\[([^\]]*)\]")
# Written with \s so that this line is not itself a credit line.
CREDIT = re.compile(r"(?://|#).*?(\bAfter\s[A-Z]|\bPort\sof\b|\(after\s[A-Z])")

# Where credit lines are looked for, relative to the root. Built bundles and
# vendored dependencies are not this repository's prose.
CREDIT_GLOBS = (
    "crates/**/*.rs",
    "xtask/**/*.rs",
    "scripts/**/*.py",
    "paper/src/**/*.py",
    "npm/**/*.js",
    "npm/**/*.mjs",
    "npm/**/*.ts",
)
CREDIT_EXCLUDED_PARTS = ("node_modules", "pkg", "target")


def load_register(path: pathlib.Path) -> list[dict]:
    rows = []
    for number, line in enumerate(path.read_text(encoding="utf-8").splitlines(), 1):
        if not line.strip():
            continue
        try:
            rows.append(json.loads(line))
        except json.JSONDecodeError as error:
            raise SystemExit(f"{path}:{number}: not JSON: {error}")
    return rows


def bib_entries(root: pathlib.Path) -> dict[str, dict[str, str]]:
    """Key to lower-cased field map for every entry in refs.bib."""
    text = (root / "paper" / "refs.bib").read_text(encoding="utf-8")
    entries: dict[str, dict[str, str]] = {}
    for _, key, body in ENTRY.findall(text):
        entries[key] = {
            name.lower(): re.sub(r"\s+", " ", value).strip() for name, value in FIELD.findall(body)
        }
    return entries


def identifier_of(fields: dict[str, str]) -> str:
    """The identifier the register must record for one bibliography entry."""
    if fields.get("doi"):
        return f"doi:{fields['doi']}"
    if fields.get("eprint"):
        return f"arXiv:{fields['eprint']}"
    if fields.get("url"):
        return f"url:{fields['url']}"
    return "none"


def paper_sources(root: pathlib.Path) -> list[pathlib.Path]:
    return [root / "paper" / "main.tex"] + sorted((root / "paper" / "sections").glob("*.tex"))


def citation_sites(root: pathlib.Path) -> tuple[dict[str, set[str]], dict[str, set[str]]]:
    """For every cited key: the paper files that cite it, and the page
    locators its citations give in an optional argument."""
    files: dict[str, set[str]] = {}
    pages: dict[str, set[str]] = {}
    for source in paper_sources(root):
        if not source.is_file():
            continue
        relative = source.relative_to(root).as_posix()
        for match in CITE.finditer(source.read_text(encoding="utf-8")):
            options = [o.strip() for o in OPTIONAL.findall(match.group(1)) if o.strip()]
            for key in (k.strip() for k in match.group(2).split(",")):
                if not key:
                    continue
                files.setdefault(key, set()).add(relative)
                pages.setdefault(key, set()).update(options)
    return files, pages


def credit_lines(root: pathlib.Path) -> list[tuple[str, str]]:
    """Every (file, line) in the scanned source whose comment credits a work."""
    found = []
    seen: set[pathlib.Path] = set()
    for pattern in CREDIT_GLOBS:
        for path in sorted(root.glob(pattern)):
            if path in seen or not path.is_file():
                continue
            seen.add(path)
            if any(part in CREDIT_EXCLUDED_PARTS for part in path.relative_to(root).parts):
                continue
            relative = path.relative_to(root).as_posix()
            for line in path.read_text(encoding="utf-8", errors="replace").splitlines():
                if CREDIT.search(line):
                    found.append((relative, line))
    return found


def check_bib(rows: list[dict], root: pathlib.Path, problems: list[str]) -> None:
    entries = bib_entries(root)
    files, pages = citation_sites(root)
    seen: dict[str, int] = {}
    for index, row in enumerate(rows):
        if row.get("kind") != "bib":
            continue
        key = row.get("key")
        where = f"bib entry {index + 1} ({key!r})"
        if not isinstance(key, str) or key not in entries:
            problems.append(f"{where}: names no key in paper/refs.bib")
            continue
        if key in seen:
            problems.append(f"{where}: repeats the entry on line {seen[key] + 1}")
            continue
        seen[key] = index
        expected = identifier_of(entries[key])
        if row.get("identifier") != expected:
            problems.append(
                f"{where}: identifier {row.get('identifier')!r} is not the one refs.bib "
                f"carries, {expected!r}"
            )
        mechanism = row.get("mechanism")
        if not isinstance(mechanism, str) or not mechanism.strip():
            problems.append(f"{where}: no credited mechanism recorded")
        locators = row.get("locators")
        if not isinstance(locators, list) or not locators:
            problems.append(f"{where}: no locator recorded")
            locators = []
        cited_in = files.get(key, set())
        for locator in locators:
            if not isinstance(locator, str) or not (root / locator).is_file():
                problems.append(f"{where}: locator {locator!r} names no file")
            elif locator not in cited_in:
                problems.append(f"{where}: {locator} does not cite {key}")
        for missing in sorted(cited_in - {l for l in locators if isinstance(l, str)}):
            problems.append(f"{where}: {missing} cites {key} but is not among its locators")
        recorded_pages = row.get("pages", [])
        if not isinstance(recorded_pages, list):
            problems.append(f"{where}: pages is not a list")
            recorded_pages = []
        given = pages.get(key, set())
        for page in recorded_pages:
            if page not in given:
                problems.append(f"{where}: no citation of {key} gives the page locator {page!r}")
        for page in sorted(given - set(recorded_pages)):
            problems.append(f"{where}: a citation of {key} gives {page!r}, which is not recorded")
    for key in sorted(set(entries) - set(seen)):
        problems.append(f"docs/citation-register.jsonl: refs.bib key {key} has no register entry")


def check_credits(rows: list[dict], root: pathlib.Path, problems: list[str]) -> None:
    entries = bib_entries(root)
    credits = [(index, row) for index, row in enumerate(rows) if row.get("kind") == "credit"]
    for index, row in credits:
        where = f"credit entry {index + 1} ({row.get('file')!r})"
        path, text = row.get("file"), row.get("text")
        if not isinstance(path, str) or not isinstance(text, str) or not text.strip():
            problems.append(f"{where}: needs a file and a text")
            continue
        target = root / path
        if not target.is_file():
            problems.append(f"{where}: {path} is not on disk")
            continue
        lines = target.read_text(encoding="utf-8", errors="replace").splitlines()
        if not any(text in line and CREDIT.search(line) for line in lines):
            problems.append(f"{where}: no credit line in {path} carries {text!r}")
        mechanism = row.get("mechanism")
        if not isinstance(mechanism, str) or not mechanism.strip():
            problems.append(f"{where}: no credited mechanism recorded")
        identifier = row.get("identifier")
        if isinstance(identifier, str) and identifier.startswith("bib:"):
            if identifier[4:] not in entries:
                problems.append(f"{where}: {identifier} names no key in paper/refs.bib")
        elif identifier == "unresolved":
            note = row.get("note")
            if not isinstance(note, str) or not note.strip():
                problems.append(f"{where}: an unresolved credit needs a note saying why")
        else:
            problems.append(f"{where}: identifier {identifier!r} is neither bib:<key> nor unresolved")
    for path, line in credit_lines(root):
        if not any(
            row.get("file") == path and isinstance(row.get("text"), str) and row["text"] in line
            for _, row in credits
        ):
            problems.append(f"{path}: credit line has no register entry: {line.strip()}")


def check_kinds(rows: list[dict], problems: list[str]) -> None:
    for index, row in enumerate(rows):
        if row.get("kind") not in ("bib", "credit"):
            problems.append(f"register line {index + 1}: kind {row.get('kind')!r} is not bib or credit")


def validate(rows: list[dict], root: pathlib.Path) -> list[str]:
    problems: list[str] = []
    check_kinds(rows, problems)
    check_bib(rows, root, problems)
    check_credits(rows, root, problems)
    return problems


def main() -> int:
    if not REGISTER.exists():
        print(f"FAIL: {REGISTER} is missing", file=sys.stderr)
        return 1
    rows = load_register(REGISTER)
    problems = validate(rows, ROOT)
    if problems:
        for problem in problems:
            print(problem, file=sys.stderr)
        print(f"\nFAIL: {len(problems)} citation register problem(s)", file=sys.stderr)
        return 1
    bib = sum(1 for row in rows if row.get("kind") == "bib")
    credits = [row for row in rows if row.get("kind") == "credit"]
    unresolved = sum(1 for row in credits if row.get("identifier") == "unresolved")
    print(
        f"OK: {bib} bibliography entries and {len(credits)} code credits registered; "
        f"{unresolved} credit(s) unresolved"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
