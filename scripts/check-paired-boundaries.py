#!/usr/bin/env python3
"""Paired-boundary gate: a documented numeric domain has a boundary test.

The 2026-09-07 audit deferred "full mutation and paired-boundary gates as a
standing CI leg". The appendix defines the term no further than that line, so
this is the smallest reading that runs in CI: a public kernel function whose
doc comment states a domain for a numeric input (finite, non-negative, in
(0, 1), alpha, confidence, ...) is paired with a boundary test that names it,
so the edge of the domain is exercised and not only the interior.

Convention. A boundary test is a `#[test]` whose name contains `boundary` or
`boundaries`, or any `#[test]` in a file whose name does, such as
`tests/greeks_boundaries.rs` and `tests/r02_statistical_boundaries.rs`. A
function is covered when its name appears in the body of a boundary test, or
anywhere in a boundary file, in either scanned crate.

Candidates. `pub fn` items outside `#[cfg(test)]` modules under
`crates/sharpebench-core/src` and `crates/sharpebench-stats/src` whose
parameter list contains a numeric type and whose doc comment matches one of
the domain patterns below.

The allowlist holds the functions that were uncovered when the gate was
introduced, so the leg is green on main. It is a ratchet: a row whose function
gains a boundary test, or no longer exists, fails the gate until the row is
removed, and a newly uncovered function fails the gate until it gets a test.
The printed uncovered count is what is left to shrink.
"""

from __future__ import annotations

import argparse
from dataclasses import dataclass, field
from pathlib import Path
import re
import sys


ROOT = Path(__file__).resolve().parent.parent
CRATES = ("crates/sharpebench-core", "crates/sharpebench-stats")

DOMAIN_PATTERNS = (
    ("finite", re.compile(r"\b(?:non-?)?finite\b", re.IGNORECASE)),
    ("non-negative", re.compile(r"\bnon-?negative\b", re.IGNORECASE)),
    ("positive", re.compile(r"\bpositive\b", re.IGNORECASE)),
    ("in (0, 1)", re.compile(r"\(0,\s*1\)")),
    ("in [0, 1]", re.compile(r"\[0,\s*1\]")),
    ("0..=1", re.compile(r"0\.\.=1")),
    ("alpha", re.compile(r"\balpha\b", re.IGNORECASE)),
    ("confidence", re.compile(r"\bconfidence\b", re.IGNORECASE)),
    ("probability", re.compile(r"\bprobabilit(?:y|ies)\b", re.IGNORECASE)),
)
NUMERIC_TYPE = re.compile(
    r"\b(?:f32|f64|u8|u16|u32|u64|u128|usize|i8|i16|i32|i64|i128|isize)\b"
)
PUB_FN = re.compile(r"^[ \t]*pub fn (\w+)", re.MULTILINE)
TEST_FN = re.compile(r"#\[test\]\s*(?:#\[[^\]]*\]\s*)*fn\s+(\w+)\s*\(")
TEST_MOD = re.compile(
    r"#\[cfg\(test\)\]\s*(?:#\[[^\]]*\]\s*)*(?:pub(?:\([^)]*\))?\s+)?mod\s+\w+\s*\{"
)
IDENT = re.compile(r"\b[A-Za-z_]\w*\b")

# Functions with a documented numeric domain and no boundary test on the day
# the gate landed. Remove a row when its function gets a boundary test; the
# gate fails on a stale row so the list can only shrink.
ALLOWLIST: frozenset[str] = frozenset(
    {
        "core::attribution::alpha_beta",
        "core::calibration::brier_score",
        "core::calibration::epistemic_uncertainty",
        "core::composite::reliability_never_catastrophic",
        "core::decay::crowding_half_life",
        "core::econrationality::elicit_revealed_selection",
        "core::percentile::percentile_of",
        "core::percentile::reference_dsr_population",
        "core::rolling::rolling_sharpe",
        "stats::deflated_sharpe::probabilistic_sharpe_ratio",
        "stats::deflated_sharpe::expected_max_sharpe",
        "stats::fdr::benjamini_hochberg",
        "stats::paired_randomization::paired_swap_test",
        "stats::selection::selection_robustness",
        "stats::selection::percentile_selection",
        "stats::significance::bootstrap_pvalue",
        "stats::significance::bootstrap_dsr_ci",
        "stats::significance::bootstrap_dsr_ci_against_null",
        "stats::significance::runs_for_power",
        "stats::significance::reality_check_pvalue",
        "stats::significance::step_down_significant",
        "stats::stats::kurtosis",
        "stats::validation::dispersion",
        "stats::validation::block_probability",
    }
)


@dataclass(frozen=True)
class Candidate:
    key: str
    name: str
    path: str
    line: int
    domains: tuple[str, ...]


@dataclass
class Report:
    candidates: list[Candidate] = field(default_factory=list)
    references: set[str] = field(default_factory=set)

    @property
    def covered(self) -> list[Candidate]:
        return [c for c in self.candidates if c.name in self.references]

    @property
    def uncovered(self) -> list[Candidate]:
        return [c for c in self.candidates if c.name not in self.references]


def matching_brace(text: str, open_index: int) -> int:
    """Index of the `}` closing the `{` at ``open_index``.

    Skips string, raw-string and char literals and both comment forms, so a
    brace inside a format string or a comment does not unbalance the count.
    Returns ``len(text)`` when the text ends first.
    """
    depth = 0
    i = open_index
    n = len(text)
    while i < n:
        ch = text[i]
        if text.startswith("//", i):
            end = text.find("\n", i)
            i = n if end < 0 else end + 1
            continue
        if text.startswith("/*", i):
            end = text.find("*/", i + 2)
            i = n if end < 0 else end + 2
            continue
        if ch == "r" and i + 1 < n and text[i + 1] in "#\"":
            j = i + 1
            hashes = 0
            while j < n and text[j] == "#":
                hashes += 1
                j += 1
            if j < n and text[j] == '"':
                close = '"' + "#" * hashes
                end = text.find(close, j + 1)
                i = n if end < 0 else end + len(close)
                continue
        if ch == '"':
            i += 1
            while i < n and text[i] != '"':
                i += 2 if text[i] == "\\" else 1
            i += 1
            continue
        if ch == "'":
            # A char literal is 'x' or '\..'; anything else is a lifetime.
            if i + 2 < n and text[i + 1] == "\\":
                end = text.find("'", i + 2)
                i = n if end < 0 else end + 1
                continue
            if i + 2 < n and text[i + 2] == "'":
                i += 3
                continue
            i += 1
            continue
        if ch == "{":
            depth += 1
        elif ch == "}":
            depth -= 1
            if depth == 0:
                return i
        i += 1
    return n


def strip_test_modules(text: str) -> str:
    """Blank every `#[cfg(test)] mod ... { ... }` body, keeping line numbers."""
    out = text
    for match in reversed(list(TEST_MOD.finditer(text))):
        open_index = match.end() - 1
        close_index = matching_brace(text, open_index)
        span = text[match.start() : close_index + 1]
        out = out[: match.start()] + "\n" * span.count("\n") + out[close_index + 1 :]
    return out


def doc_comment_above(lines: list[str], fn_line: int) -> str:
    """The contiguous `///` block (through attributes) above line ``fn_line``."""
    doc: list[str] = []
    i = fn_line - 1
    while i >= 0:
        stripped = lines[i].strip()
        if stripped.startswith("///"):
            doc.append(stripped[3:])
        elif stripped.startswith("#[") or stripped == "":
            if stripped == "" and doc:
                break
        else:
            break
        i -= 1
    return "\n".join(reversed(doc))


def parameter_list(text: str, fn_start: int) -> str:
    open_paren = text.find("(", fn_start)
    if open_paren < 0:
        return ""
    depth = 0
    for i in range(open_paren, len(text)):
        if text[i] == "(":
            depth += 1
        elif text[i] == ")":
            depth -= 1
            if depth == 0:
                return text[open_paren : i + 1]
    return text[open_paren:]


def candidates_in(path: Path, crate: str, root: Path) -> list[Candidate]:
    text = strip_test_modules(path.read_text(encoding="utf-8"))
    lines = text.split("\n")
    module = path.stem
    found: list[Candidate] = []
    for match in PUB_FN.finditer(text):
        name = match.group(1)
        line_index = text.count("\n", 0, match.start())
        params = parameter_list(text, match.start())
        if not NUMERIC_TYPE.search(params):
            continue
        doc = doc_comment_above(lines, line_index)
        domains = tuple(label for label, pattern in DOMAIN_PATTERNS if pattern.search(doc))
        if not domains:
            continue
        key = f"{crate}::{module}::{name}"
        found.append(
            Candidate(
                key=key,
                name=name,
                path=path.relative_to(root).as_posix(),
                line=line_index + 1,
                domains=domains,
            )
        )
    return found


def boundary_references_in(path: Path) -> set[str]:
    text = path.read_text(encoding="utf-8")
    if "boundar" in path.stem:
        return set(IDENT.findall(text))
    refs: set[str] = set()
    for match in TEST_FN.finditer(text):
        if "boundar" not in match.group(1):
            continue
        open_index = text.find("{", match.end())
        if open_index < 0:
            continue
        close_index = matching_brace(text, open_index)
        refs.update(IDENT.findall(text[open_index : close_index + 1]))
    return refs


def scan(root: Path, crates: tuple[str, ...] = CRATES) -> Report:
    report = Report()
    for crate_dir in crates:
        crate = crate_dir.rsplit("-", 1)[-1]
        src = root / crate_dir / "src"
        for path in sorted(src.rglob("*.rs")):
            report.candidates.extend(candidates_in(path, crate, root))
        for sub in ("src", "tests"):
            for path in sorted((root / crate_dir / sub).rglob("*.rs")):
                report.references |= boundary_references_in(path)
    seen: set[str] = set()
    unique: list[Candidate] = []
    for candidate in report.candidates:
        if candidate.key not in seen:
            seen.add(candidate.key)
            unique.append(candidate)
    report.candidates = unique
    return report


def evaluate(report: Report, allowlist: frozenset[str]) -> tuple[list[str], list[str]]:
    """(failures, notes). Empty failures means the gate passes."""
    failures: list[str] = []
    notes: list[str] = []
    uncovered_keys = {c.key for c in report.uncovered}
    for candidate in report.uncovered:
        if candidate.key not in allowlist:
            failures.append(
                f"{candidate.path}:{candidate.line}: `{candidate.name}` documents "
                f"{', '.join(candidate.domains)} and no boundary test names it; "
                "add a `*_boundary_*` test or a row to ALLOWLIST"
            )
    covered_keys = {c.key for c in report.covered}
    for key in sorted(allowlist):
        if key in covered_keys:
            failures.append(f"ALLOWLIST row `{key}` is now covered; remove the row")
        elif key not in uncovered_keys:
            failures.append(f"ALLOWLIST row `{key}` names no current candidate; remove the row")
    notes.append(
        f"paired-boundary gate: {len(report.candidates)} candidates, "
        f"{len(report.covered)} covered, {len(report.uncovered)} uncovered "
        f"({len(allowlist)} allowlisted)"
    )
    return failures, notes


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    parser.add_argument("--root", type=Path, default=ROOT)
    parser.add_argument(
        "--verbose", action="store_true", help="print every candidate with its status"
    )
    parser.add_argument(
        "--print-allowlist",
        action="store_true",
        help="print the currently uncovered keys as Python set rows and exit 0",
    )
    args = parser.parse_args(argv)
    report = scan(args.root)
    if args.print_allowlist:
        for candidate in report.uncovered:
            print(f'        "{candidate.key}",')
        return 0
    if args.verbose:
        for candidate in report.candidates:
            status = "covered" if candidate.name in report.references else "UNCOVERED"
            print(f"{status:9} {candidate.key}  ({', '.join(candidate.domains)})")
    failures, notes = evaluate(report, ALLOWLIST)
    for note in notes:
        print(note)
    for candidate in report.uncovered:
        print(f"  uncovered: {candidate.key}  ({', '.join(candidate.domains)})")
    sys.stdout.flush()
    for failure in failures:
        print(f"FAIL: {failure}", file=sys.stderr)
    return 1 if failures else 0


if __name__ == "__main__":
    sys.exit(main())
