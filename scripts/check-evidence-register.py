#!/usr/bin/env python3
"""Validate the evidence-freshness and applicability register (ticket P00-E).

The register maps every table, figure and empirical claim in the inventoried
manuscript scope to the artifact that produced it, the command and version that
produced that artifact, the changes that landed afterwards, and a disposition
saying what the claim still supports today. A claim whose artifact has gone
missing, or that carries no disposition, or whose disposition moved without a
reason written down, is exactly the state the register exists to prevent, so
each of those fails this check.

The three failure modes the ticket names:

* a missing artifact: a row names an in-repository artifact path that is not on
  disk, or that no longer hashes to the digest the row recorded, or a row names
  no artifact at all while neither taking the disposition ``unresolved`` nor
  stating in ``no_measurement`` why the claim reports no measurement;
* a claim with no disposition: a row omits the field, leaves it empty, or uses a
  value outside the five the ticket defines, or a table or figure in the
  manuscript has no row at all;
* a status change with no recorded rationale: a row's ``status_history`` changes
  disposition between two entries without a rationale of its own, or ends on a
  disposition other than the row's current one.

Three further legs keep the register honest about itself: every source location
must name a file that exists and a line inside it; the priority queue of headline
claims stays under the generator's bound, so it remains a working list rather
than a second copy of the register; and the Markdown register must be what
``make-evidence-register.py`` writes from the JSONL, so the readable copy cannot
drift away from the machine-readable one.

Rows for the companion product are marked ``external_repo`` and their artifacts
are recorded but not opened: this repository does not contain that tree, and a
check that silently passed on an unreadable path would report coverage it never
verified.

Exit 0 when the register is consistent, 1 otherwise.
"""

from __future__ import annotations

import hashlib
import json
import pathlib
import re
import sys

ROOT = pathlib.Path(__file__).resolve().parents[1]
REGISTER_JSONL = ROOT / "docs" / "evidence-register.jsonl"
REGISTER_MD = ROOT / "docs" / "evidence-register.md"

DISPOSITIONS = (
    "historical-only",
    "still-applicable",
    "needs-rescore",
    "needs-new-experiment",
    "unresolved",
)

# Labels of this repository's manuscript that every row set must cover. Anything
# the paper labels as a table or a figure is in the inventoried scope.
LABEL_SOURCES = [ROOT / "paper" / "main.tex"] + sorted(
    (ROOT / "paper" / "sections").glob("*.tex")
)
LABEL_PATTERN = re.compile(r"\\label\{((?:tab|fig):[^}]+)\}")

REQUIRED_TEXT_FIELDS = (
    "claim_id",
    "product",
    "kind",
    "claim",
    "producer_command",
    "producing_commit",
    "effective_configuration",
    "present_applicability",
    "disposition",
    "disposition_rationale",
    "owner",
)
REQUIRED_LIST_FIELDS = (
    "source_location",
    "labels",
    "artifacts",
    "missing_provenance",
    "later_changes",
    "status_history",
)

DATE = re.compile(r"^\d{4}-\d{2}-\d{2}$")
LOCATION = re.compile(r"^(?P<path>[^:]+):(?P<line>\d+)$")


def load_rows(path: pathlib.Path) -> list[dict]:
    rows = []
    for number, line in enumerate(path.read_text(encoding="utf-8").splitlines(), 1):
        if not line.strip():
            continue
        try:
            rows.append(json.loads(line))
        except json.JSONDecodeError as error:
            raise SystemExit(f"{path}:{number}: not JSON: {error}")
    return rows


def sha256_of(path: pathlib.Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1 << 20), b""):
            digest.update(chunk)
    return digest.hexdigest()


def manuscript_labels() -> set[str]:
    found: set[str] = set()
    for source in LABEL_SOURCES:
        if not source.exists():
            continue
        found.update(LABEL_PATTERN.findall(source.read_text(encoding="utf-8")))
    return found


def check_fields(row: dict, where: str, problems: list[str]) -> None:
    for field in REQUIRED_TEXT_FIELDS:
        value = row.get(field)
        if not isinstance(value, str) or not value.strip():
            problems.append(f"{where}: field {field!r} is missing or empty")
    for field in REQUIRED_LIST_FIELDS:
        if not isinstance(row.get(field), list):
            problems.append(f"{where}: field {field!r} is missing or not a list")


def check_disposition(row: dict, where: str, problems: list[str]) -> None:
    disposition = row.get("disposition")
    if disposition not in DISPOSITIONS:
        problems.append(
            f"{where}: disposition {disposition!r} is not one of {', '.join(DISPOSITIONS)}"
        )
    rationale = row.get("disposition_rationale")
    if isinstance(rationale, str) and not rationale.strip():
        problems.append(f"{where}: disposition carries no rationale")


def check_artifacts(row: dict, where: str, root: pathlib.Path, problems: list[str]) -> None:
    artifacts = row.get("artifacts")
    if not isinstance(artifacts, list):
        return
    external = row.get("external_repo")
    no_measurement = row.get("no_measurement")
    declared_no_measurement = isinstance(no_measurement, str) and no_measurement.strip()
    if not artifacts and row.get("disposition") != "unresolved" and not declared_no_measurement:
        problems.append(
            f"{where}: no artifact is recorded, so the row must either take the disposition "
            f"'unresolved' or state in 'no_measurement' why it reports no measurement, "
            f"and it does neither (disposition {row.get('disposition')!r})"
        )
    for index, artifact in enumerate(artifacts):
        label = f"{where}: artifact[{index}]"
        if not isinstance(artifact, dict):
            problems.append(f"{label}: not an object")
            continue
        path = artifact.get("path")
        if not isinstance(path, str) or not path.strip():
            problems.append(f"{label}: no path recorded")
            continue
        if artifact.get("in_repo") is False:
            if not external:
                problems.append(
                    f"{label}: marked out of repository but the row names no external_repo"
                )
            continue
        if external:
            problems.append(
                f"{label}: row covers {external}, so its artifacts cannot be marked in_repo"
            )
            continue
        target = root / path
        if not target.exists():
            problems.append(f"{label}: {path} is not on disk")
            continue
        recorded = artifact.get("sha256")
        if isinstance(recorded, str) and recorded:
            if target.is_dir():
                problems.append(f"{label}: {path} is a directory and cannot carry a digest")
                continue
            actual = sha256_of(target)
            if actual != recorded:
                problems.append(
                    f"{label}: {path} hashes to {actual}, the row records {recorded}"
                )


def check_locations(row: dict, where: str, root: pathlib.Path, problems: list[str]) -> None:
    locations = row.get("source_location")
    if not isinstance(locations, list):
        return
    if not locations:
        problems.append(f"{where}: no source location recorded")
    external = bool(row.get("external_repo"))
    for location in locations:
        match = LOCATION.match(location) if isinstance(location, str) else None
        if match is None:
            problems.append(f"{where}: source location {location!r} is not 'path:line'")
            continue
        if external:
            # The other product's tree is not here, so the location is recorded and
            # left unopened rather than checked against a file this repository lacks.
            continue
        target = root / match.group("path")
        if not target.exists():
            problems.append(f"{where}: source location {location} names no file")
            continue
        lines = target.read_text(encoding="utf-8", errors="replace").splitlines()
        if not 1 <= int(match.group("line")) <= len(lines):
            problems.append(
                f"{where}: source location {location} is past the end of the file "
                f"({len(lines)} lines)"
            )


def check_status_history(row: dict, where: str, problems: list[str]) -> None:
    history = row.get("status_history")
    if not isinstance(history, list):
        return
    if not history:
        problems.append(f"{where}: status_history is empty")
        return
    previous = None
    last_date = ""
    for index, entry in enumerate(history):
        label = f"{where}: status_history[{index}]"
        if not isinstance(entry, dict):
            problems.append(f"{label}: not an object")
            return
        date = entry.get("date")
        disposition = entry.get("disposition")
        rationale = entry.get("rationale")
        if not isinstance(date, str) or not DATE.match(date):
            problems.append(f"{label}: date {date!r} is not YYYY-MM-DD")
        elif date < last_date:
            problems.append(f"{label}: date {date} is before the previous entry {last_date}")
        else:
            last_date = date
        if disposition not in DISPOSITIONS:
            problems.append(f"{label}: disposition {disposition!r} is not a defined value")
        if not isinstance(rationale, str) or not rationale.strip():
            problems.append(f"{label}: no rationale recorded")
            rationale = None
        if previous is not None and disposition != previous["disposition"]:
            if rationale is None or rationale.strip() == previous["rationale"].strip():
                problems.append(
                    f"{label}: disposition changed from {previous['disposition']!r} to "
                    f"{disposition!r} with no rationale of its own"
                )
        previous = {
            "disposition": disposition,
            "rationale": rationale if isinstance(rationale, str) else "",
        }
    final = history[-1]
    if isinstance(final, dict) and final.get("disposition") != row.get("disposition"):
        problems.append(
            f"{where}: status_history ends on {final.get('disposition')!r} but the row "
            f"records {row.get('disposition')!r}"
        )


def queue_bound() -> int:
    """Read the bound from the generator, so the two never disagree."""
    import importlib.util

    spec = importlib.util.spec_from_file_location(
        "make_evidence_register", ROOT / "scripts" / "make-evidence-register.py"
    )
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module.MAX_QUEUE


def check_queue_bound(rows: list[dict], problems: list[str], bound: int) -> None:
    """The priority queue is a working list, so its size is capped rather than
    allowed to grow into a second copy of the register."""
    queued = [
        row
        for row in rows
        if row.get("headline")
        and row.get("disposition") in ("needs-new-experiment", "needs-rescore")
    ]
    if len(queued) > bound:
        names = ", ".join(str(row.get("claim_id")) for row in queued)
        problems.append(
            f"docs/evidence-register.jsonl: {len(queued)} headline claims are queued, above "
            f"the bound of {bound}; close or de-flag one before adding another ({names})"
        )
    for row in rows:
        if row.get("headline") and row.get("disposition") not in (
            "needs-new-experiment",
            "needs-rescore",
        ):
            problems.append(
                f"row {row.get('claim_id')!r}: marked headline but its disposition is "
                f"{row.get('disposition')!r}, so it belongs in no queue"
            )


def check_coverage(rows: list[dict], problems: list[str]) -> None:
    covered: set[str] = set()
    for row in rows:
        if row.get("external_repo"):
            continue
        labels = row.get("labels")
        if isinstance(labels, list):
            covered.update(label for label in labels if isinstance(label, str))
    for label in sorted(manuscript_labels() - covered):
        problems.append(
            f"docs/evidence-register.jsonl: manuscript label {label} has no register row"
        )


def check_markdown(rows: list[dict], problems: list[str]) -> None:
    try:
        sys.path.insert(0, str(ROOT / "scripts"))
        import importlib.util

        spec = importlib.util.spec_from_file_location(
            "make_evidence_register", ROOT / "scripts" / "make-evidence-register.py"
        )
        module = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(module)
    except Exception as error:  # the generator is the authority; say so plainly
        problems.append(f"scripts/make-evidence-register.py could not be loaded: {error}")
        return
    if not REGISTER_MD.exists():
        problems.append("docs/evidence-register.md is missing")
        return
    expected = module.render(rows)
    actual = REGISTER_MD.read_text(encoding="utf-8")
    if expected != actual:
        problems.append(
            "docs/evidence-register.md is not what scripts/make-evidence-register.py "
            "writes from docs/evidence-register.jsonl; regenerate it"
        )


def validate(rows: list[dict], root: pathlib.Path) -> list[str]:
    problems: list[str] = []
    seen: dict[str, int] = {}
    for index, row in enumerate(rows):
        claim_id = row.get("claim_id")
        where = f"row {index + 1} ({claim_id!r})"
        if isinstance(claim_id, str) and claim_id in seen:
            problems.append(f"{where}: claim_id repeats row {seen[claim_id] + 1}")
        elif isinstance(claim_id, str):
            seen[claim_id] = index
        check_fields(row, where, problems)
        check_disposition(row, where, problems)
        check_artifacts(row, where, root, problems)
        check_locations(row, where, root, problems)
        check_status_history(row, where, problems)
    return problems


def main() -> int:
    if not REGISTER_JSONL.exists():
        print(f"FAIL: {REGISTER_JSONL} is missing", file=sys.stderr)
        return 1
    rows = load_rows(REGISTER_JSONL)
    problems = validate(rows, ROOT)
    check_coverage(rows, problems)
    check_queue_bound(rows, problems, queue_bound())
    check_markdown(rows, problems)
    if problems:
        for problem in problems:
            print(problem, file=sys.stderr)
        print(f"\nFAIL: {len(problems)} register problem(s)", file=sys.stderr)
        return 1
    print(f"OK: {len(rows)} register rows, every claim dispositioned")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
