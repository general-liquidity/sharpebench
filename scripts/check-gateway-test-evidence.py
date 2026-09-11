#!/usr/bin/env python3
"""Gate: the gateway chapter's test-evidence table matches the tree.

`docs/book/src/model-gateway.md` publishes a table of how many tests cover the
gateway. It is the book's evidence that the money accounting is tested, and a
reader weighing whether to trust it has no way to check a number that nothing
recounts. This recounts it.

Two measures, because one does not fit both cases.

**Counted files.** A file that exists only for the gateway: every test in it is
gateway evidence, so the count is the number of test functions in the file. The
counted rows are the table whose header is `| Where | Tests | Covers |`. A row's
count must equal the tree's, and no test in a counted file may be `#[ignore]`,
which is the chapter's other claim about them.

**Named tests.** A file with gateway tests among others: `sandbox.rs` holds two
gateway tests and dozens of the sandbox's own, so counting the file would put a
different measure in the same column. Those rows name the test instead, in the
table whose header is `| Test | Where | Runs | Covers |`. The named function
must exist in the named file, and its `Runs` cell must match whether it carries
`#[ignore]`.

Counting rule: a test function is a `fn` whose attribute run contains `#[test]`
or `#[<path>::test]`, the attribute alone on its line, so a `#[test]` inside a
doc comment is not counted. On the current tree this agrees exactly with
`cargo nextest list` plus the ignored tests it omits.

What this gate does not claim: that a counted file's tests are good, or that a
test in one file cannot exercise code in another. `tests/journal_ownership_review.rs`
is a counted row precisely because it covers `gateway_journal.rs` from outside,
and a number here is a count of tests, never a coverage percentage.

Isolating the cause. A bare "the table and the tree disagree" is an outcome two
independent causes produce, and the audit rule this repository recorded is that
such an assertion says nothing until the cause is isolated. So every failure is
classified against a baseline revision (`--baseline`, `HEAD` by default): the
report names whether the table moved, the tree moved, both moved, or the
disagreement is older than the baseline.
"""

from __future__ import annotations

import argparse
from dataclasses import dataclass
from pathlib import Path
import re
import subprocess
import sys


ROOT = Path(__file__).resolve().parent.parent
CHAPTER = "docs/book/src/model-gateway.md"

COUNTED_HEADER = "| Where | Tests | Covers |"
NAMED_HEADER = "| Test | Where | Runs | Covers |"

TEST_ATTR = re.compile(r"^#\[(?:[A-Za-z_][A-Za-z0-9_]*::)*test\]$")
IGNORE_ATTR = re.compile(r"^#\[ignore\b")
FN_DECL = re.compile(r"^(?:pub(?:\([^)]*\))?\s+)?(?:async\s+)?(?:unsafe\s+)?fn\s+([A-Za-z_][A-Za-z0-9_]*)")
BACKTICKED = re.compile(r"^`([^`]+)`$")

RUNS_ALWAYS = "always"
RUNS_IGNORED = "`#[ignore]`"


@dataclass(frozen=True)
class CountedRow:
    line: int
    path: str
    claimed: int


@dataclass(frozen=True)
class NamedRow:
    line: int
    test: str
    path: str
    ignored: bool


@dataclass(frozen=True)
class TestFn:
    name: str
    ignored: bool


def read_at(revision: str | None, path: str) -> str | None:
    """The file's text at `revision`, or from the working tree when None."""
    if revision is None:
        candidate = ROOT / path
        return candidate.read_text(encoding="utf-8") if candidate.is_file() else None
    result = subprocess.run(
        ["git", "show", f"{revision}:{path}"],
        cwd=ROOT,
        capture_output=True,
    )
    if result.returncode != 0:
        return None
    return result.stdout.decode("utf-8")


def tests_in(text: str) -> list[TestFn]:
    found: list[TestFn] = []
    attrs: list[str] = []
    for raw in text.splitlines():
        line = raw.strip()
        if line.startswith("#["):
            attrs.append(line)
            continue
        declared = FN_DECL.match(line)
        if declared:
            if any(TEST_ATTR.match(attr) for attr in attrs):
                found.append(
                    TestFn(
                        name=declared.group(1),
                        ignored=any(IGNORE_ATTR.match(attr) for attr in attrs),
                    )
                )
            attrs = []
            continue
        # Doc comments and blank lines sit between an item's attributes; any
        # other line ends the attribute run.
        if line and not line.startswith("//"):
            attrs = []
    return found


def split_row(line: str) -> list[str]:
    return [cell.strip() for cell in line.strip().strip("|").split("|")]


def parse_chapter(text: str) -> tuple[list[CountedRow], list[NamedRow], list[str]]:
    counted: list[CountedRow] = []
    named: list[NamedRow] = []
    problems: list[str] = []
    mode: str | None = None
    for number, raw in enumerate(text.splitlines(), start=1):
        line = raw.rstrip()
        if line.strip() == COUNTED_HEADER:
            mode = "counted"
            continue
        if line.strip() == NAMED_HEADER:
            mode = "named"
            continue
        if mode is None:
            continue
        if not line.startswith("|"):
            mode = None
            continue
        cells = split_row(line)
        if all(set(cell) <= {"-", ":"} for cell in cells):
            continue
        if mode == "counted":
            if len(cells) != 3:
                problems.append(f"{CHAPTER}:{number}: a counted row needs three cells")
                continue
            path = BACKTICKED.match(cells[0])
            if not path:
                problems.append(f"{CHAPTER}:{number}: the Where cell is not a backticked path")
                continue
            if not cells[1].isdigit():
                problems.append(
                    f"{CHAPTER}:{number}: the Tests cell is {cells[1]!r}, not a count"
                )
                continue
            counted.append(CountedRow(number, path.group(1), int(cells[1])))
        else:
            if len(cells) != 4:
                problems.append(f"{CHAPTER}:{number}: a named row needs four cells")
                continue
            test = BACKTICKED.match(cells[0])
            path = BACKTICKED.match(cells[1])
            if not test or not path:
                problems.append(
                    f"{CHAPTER}:{number}: the Test and Where cells must be backticked"
                )
                continue
            if cells[2] not in (RUNS_ALWAYS, RUNS_IGNORED):
                problems.append(
                    f"{CHAPTER}:{number}: the Runs cell is {cells[2]!r}, "
                    f"not {RUNS_ALWAYS!r} or {RUNS_IGNORED!r}"
                )
                continue
            named.append(NamedRow(number, test.group(1), path.group(1), cells[2] == RUNS_IGNORED))
    return counted, named, problems


def claimed_count_at(revision: str, path: str) -> int | None:
    text = read_at(revision, CHAPTER)
    if text is None:
        return None
    counted, _, _ = parse_chapter(text)
    for row in counted:
        if row.path == path:
            return row.claimed
    return None


def tree_count_at(revision: str, path: str) -> int | None:
    text = read_at(revision, path)
    if text is None:
        return None
    return len(tests_in(text))


def which_side_moved(baseline: str, path: str, claimed: int, actual: int) -> str:
    was_claimed = claimed_count_at(baseline, path)
    was_actual = tree_count_at(baseline, path)
    if was_claimed is None or was_actual is None:
        return f"    (no comparable row at {baseline}, so neither side can be isolated)"
    table_moved = was_claimed != claimed
    tree_moved = was_actual != actual
    if table_moved and not tree_moved:
        return (
            f"    the table moved: it claimed {was_claimed} at {baseline} and claims "
            f"{claimed} now, while the tree has held {actual}"
        )
    if tree_moved and not table_moved:
        return (
            f"    the tree moved: it had {was_actual} tests at {baseline} and has "
            f"{actual} now, while the table has held {claimed}"
        )
    if tree_moved and table_moved:
        return (
            f"    both moved: the table {was_claimed} to {claimed}, the tree "
            f"{was_actual} to {actual}"
        )
    return (
        f"    neither moved since {baseline}, where the table already claimed "
        f"{was_claimed} against {was_actual} tests: the disagreement is older"
    )


def check(baseline: str) -> list[str]:
    chapter = read_at(None, CHAPTER)
    if chapter is None:
        return [f"{CHAPTER} is missing"]
    counted, named, failures = parse_chapter(chapter)
    if not counted:
        failures.append(f"{CHAPTER}: no counted rows found under {COUNTED_HEADER!r}")
    if not named:
        failures.append(f"{CHAPTER}: no named rows found under {NAMED_HEADER!r}")

    for row in counted:
        text = read_at(None, row.path)
        if text is None:
            failures.append(f"{CHAPTER}:{row.line}: {row.path} does not exist")
            continue
        tests = tests_in(text)
        if len(tests) != row.claimed:
            failures.append(
                f"{CHAPTER}:{row.line}: the table claims {row.claimed} tests in "
                f"{row.path}, the tree has {len(tests)}"
            )
            failures.append(which_side_moved(baseline, row.path, row.claimed, len(tests)))
        ignored = [test.name for test in tests if test.ignored]
        if ignored:
            failures.append(
                f"{CHAPTER}:{row.line}: {row.path} is a counted file, so the chapter "
                f"says none of its tests is #[ignore]; these are: {', '.join(sorted(ignored))}"
            )

    for row in named:
        text = read_at(None, row.path)
        if text is None:
            failures.append(f"{CHAPTER}:{row.line}: {row.path} does not exist")
            continue
        match = [test for test in tests_in(text) if test.name == row.test]
        if not match:
            failures.append(
                f"{CHAPTER}:{row.line}: {row.path} has no test named {row.test}"
            )
            continue
        if match[0].ignored != row.ignored:
            wanted = RUNS_IGNORED if match[0].ignored else RUNS_ALWAYS
            failures.append(
                f"{CHAPTER}:{row.line}: the table runs {row.test} as "
                f"{RUNS_IGNORED if row.ignored else RUNS_ALWAYS}, the tree says {wanted}"
            )
    return failures


def report(baseline: str) -> None:
    chapter = read_at(None, CHAPTER)
    counted, named, _ = parse_chapter(chapter or "")
    for row in counted:
        text = read_at(None, row.path)
        actual = len(tests_in(text)) if text is not None else "missing"
        print(f"{actual:>6}  {row.path}  (table: {row.claimed})")
    for row in named:
        text = read_at(None, row.path)
        tests = tests_in(text) if text is not None else []
        match = [test for test in tests if test.name == row.test]
        state = RUNS_IGNORED if match and match[0].ignored else RUNS_ALWAYS
        print(f"{'named':>6}  {row.path}::{row.test}  ({state}, of {len(tests)} in the file)")


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--baseline",
        default="HEAD",
        help="revision the failure report isolates a drift against (default: HEAD)",
    )
    parser.add_argument(
        "--print",
        dest="only_print",
        action="store_true",
        help="print the counts the tree actually has and exit 0",
    )
    args = parser.parse_args()

    if args.only_print:
        report(args.baseline)
        return 0

    failures = check(args.baseline)
    if failures:
        print("The gateway chapter's test evidence disagrees with the tree:")
        for failure in failures:
            print(f"  {failure}" if not failure.startswith("    ") else failure)
        return 1
    print(f"OK: {CHAPTER} test evidence matches the tree")
    return 0


if __name__ == "__main__":
    sys.exit(main())
