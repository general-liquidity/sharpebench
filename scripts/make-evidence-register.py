#!/usr/bin/env python3
"""Write docs/evidence-register.md from docs/evidence-register.jsonl.

The JSONL file is the authority. The Markdown is a readable projection of it, so
a reader and a checker never consult two different registers. Regenerate after
editing a row:

    python scripts/make-evidence-register.py

``scripts/check-evidence-register.py`` fails when the committed Markdown is not
what this script writes from the committed rows.
"""

from __future__ import annotations

import json
import pathlib
import sys

ROOT = pathlib.Path(__file__).resolve().parents[1]
REGISTER_JSONL = ROOT / "docs" / "evidence-register.jsonl"
REGISTER_MD = ROOT / "docs" / "evidence-register.md"

# The queue is a working list, not a second register. The checker enforces the bound.
MAX_QUEUE = 12

DISPOSITION_MEANING = [
    (
        "historical-only",
        "Validly records an older experiment. The claim is retained and bound to that "
        "version. It does not validate changed current behavior.",
    ),
    (
        "still-applicable",
        "A documented comparison establishes that the computation, the inputs and the "
        "claim remain applicable. The check is recorded, not inferred.",
    ),
    (
        "needs-rescore",
        "Recorded inputs may suffice to compute a current-method result. Sufficiency is "
        "verified first; any output is separately versioned and never overwrites the old "
        "one.",
    ),
    (
        "needs-new-experiment",
        "Changed generation, policy, execution or measurement, or missing inputs, prevent "
        "a defensible rescore. A new protocol and budget request is required.",
    ),
    (
        "unresolved",
        "Evidence is insufficient to choose a disposition. What is missing is recorded and "
        "no current validation is implied.",
    ),
]

PREAMBLE = """# Evidence-freshness and applicability register

Ticket P00-E. This register maps every table, every figure and every empirical
claim in the inventoried manuscript scope of the two products to the artifact
that produced it, the command and version behind that artifact, the changes that
landed afterwards, and what the claim supports today.

The register maps existing evidence. It does not rerun, regenerate or authorize
any experiment, and no frozen evidence file, golden or manuscript number was
changed to produce it. Where an artifact could not be located the row records
`unresolved` rather than a guess, and unknown historical versions stay unknown.

`docs/evidence-register.jsonl` is the machine-readable authority; this file is
generated from it by `scripts/make-evidence-register.py`.
`scripts/check-evidence-register.py` fails on a missing artifact, a claim with no
disposition, a manuscript table or figure with no row, and a status change with
no recorded rationale.

Rows marked with an external repository cover the companion product, whose tree
is not part of this repository. Their artifact paths are recorded relative to
that repository's root and are not opened by the checker, which reports coverage
only for what it can read.

## Changing a row

Edit the JSONL row, then run `python scripts/make-evidence-register.py` to
rewrite this file and `python scripts/check-evidence-register.py` to validate
both. A disposition that changes gets a new `status_history` entry with its own
rationale; repeating the previous entry's text fails the check. An artifact's
`sha256` is over the file's bytes, and the checker prints the digest it computed
when a recorded one does not match.

## Dispositions

| Disposition | Meaning and action |
|---|---|
"""


def escape(text: str) -> str:
    return str(text).replace("|", "\\|").replace("\n", " ")


def bullets(values: list[str], empty: str) -> str:
    if not values:
        return f"- {empty}\n"
    return "".join(f"- {value}\n" for value in values)


def artifact_line(artifact: dict) -> str:
    path = artifact.get("path", "")
    digest = artifact.get("sha256")
    source = artifact.get("digest_source")
    note = artifact.get("note")
    parts = [f"`{path}`"]
    if digest:
        parts.append(f"sha256 `{digest}`")
    else:
        parts.append("no digest recorded")
    if source:
        parts.append(f"digest source: {source}")
    if artifact.get("in_repo") is False:
        parts.append("not in this repository")
    if note:
        parts.append(note)
    return ", ".join(parts)


def render(rows: list[dict]) -> str:
    out = [PREAMBLE]
    for name, meaning in DISPOSITION_MEANING:
        out.append(f"| {name} | {meaning} |\n")

    counts: dict[str, int] = {}
    per_product: dict[str, list[dict]] = {}
    for row in rows:
        counts[row.get("disposition", "")] = counts.get(row.get("disposition", ""), 0) + 1
        per_product.setdefault(row.get("product", ""), []).append(row)

    out.append("\n## Counts\n\n| Disposition | Rows |\n|---|---|\n")
    for name, _ in DISPOSITION_MEANING:
        out.append(f"| {name} | {counts.get(name, 0)} |\n")
    out.append(f"| **total** | **{len(rows)}** |\n")

    queue = [
        row
        for row in rows
        if row.get("headline")
        and row.get("disposition") in ("needs-new-experiment", "needs-rescore")
    ]
    queue.sort(key=lambda row: row.get("disposition") != "needs-new-experiment")
    out.append(
        "\n## Priority queue\n\nThe headline claims whose present applicability is not "
        "established, worst first. The queue is bounded at "
        f"{MAX_QUEUE} rows so that it stays a working list rather than a second copy of the "
        "register; every other claim is tracked by its row alone, and no historical audit is "
        "reopened for it.\n\n"
    )
    out.append("| Claim | Product | Disposition | What it would take |\n|---|---|---|---|\n")
    for row in queue:
        out.append(
            "| `{id}` | {product} | {disposition} | {follow} |\n".format(
                id=escape(row.get("claim_id", "")),
                product=escape(row.get("product", "")),
                disposition=escape(row.get("disposition", "")),
                follow=escape(row.get("follow_up") or "not yet assigned"),
            )
        )

    out.append("\n## Summary\n\n")
    out.append("| Claim | Product | Kind | Source | Disposition | Owner | Follow-up |\n")
    out.append("|---|---|---|---|---|---|---|\n")
    for row in rows:
        follow_up = row.get("follow_up") or "none"
        out.append(
            "| `{id}` | {product} | {kind} | {source} | {disposition} | {owner} | {follow} |\n".format(
                id=escape(row.get("claim_id", "")),
                product=escape(row.get("product", "")),
                kind=escape(row.get("kind", "")),
                source=escape(", ".join(row.get("source_location", []))),
                disposition=escape(row.get("disposition", "")),
                owner=escape(row.get("owner", "")),
                follow=escape(follow_up),
            )
        )

    for product in sorted(per_product):
        out.append(f"\n## {product}\n")
        for row in per_product[product]:
            out.append(f"\n### `{row.get('claim_id', '')}`\n\n")
            if row.get("external_repo"):
                out.append(
                    f"Covers the **{row['external_repo']}** repository, which is not part of "
                    "this tree. Paths below are relative to that repository's root.\n\n"
                )
            out.append(f"**Claim.** {row.get('claim', '')}\n\n")
            out.append(
                "**Source.** "
                + ", ".join(f"`{location}`" for location in row.get("source_location", []))
                + "\n\n"
            )
            labels = row.get("labels", [])
            if labels:
                out.append(
                    "**Labels.** " + ", ".join(f"`{label}`" for label in labels) + "\n\n"
                )
            out.append("**Artifacts.**\n")
            out.append(
                bullets(
                    [artifact_line(artifact) for artifact in row.get("artifacts", [])],
                    row.get("no_measurement") or "none located",
                )
            )
            out.append(f"\n**Producer command.** {row.get('producer_command', '')}\n\n")
            out.append(f"**Producing commit.** {row.get('producing_commit', '')}\n\n")
            out.append(
                f"**Effective configuration.** {row.get('effective_configuration', '')}\n\n"
            )
            out.append("**Missing provenance.**\n")
            out.append(bullets(row.get("missing_provenance", []), "none identified"))
            out.append("\n**Relevant later changes.**\n")
            out.append(bullets(row.get("later_changes", []), "none identified"))
            out.append(f"\n**Present applicability.** {row.get('present_applicability', '')}\n\n")
            out.append(
                f"**Disposition.** `{row.get('disposition', '')}`. "
                f"{row.get('disposition_rationale', '')}\n\n"
            )
            out.append(f"**Owner.** {row.get('owner', '')}\n\n")
            out.append(f"**Follow-up.** {row.get('follow_up') or 'none'}\n\n")
            out.append("**Status history.**\n\n| Date | Disposition | Rationale |\n|---|---|---|\n")
            for entry in row.get("status_history", []):
                out.append(
                    "| {date} | {disposition} | {rationale} |\n".format(
                        date=escape(entry.get("date", "")),
                        disposition=escape(entry.get("disposition", "")),
                        rationale=escape(entry.get("rationale", "")),
                    )
                )
    return "".join(out)


def main() -> int:
    rows = [
        json.loads(line)
        for line in REGISTER_JSONL.read_text(encoding="utf-8").splitlines()
        if line.strip()
    ]
    REGISTER_MD.write_text(render(rows), encoding="utf-8", newline="\n")
    print(f"wrote {REGISTER_MD.relative_to(ROOT)} from {len(rows)} rows")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
