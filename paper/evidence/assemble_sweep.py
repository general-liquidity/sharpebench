"""Assemble four fully validated principal-sweep shards in declared key order."""

from __future__ import annotations

import sys
from pathlib import Path

from sweep_grid import DSR_BARS, GridError, read_records, validate_grid


def assemble(out: Path, parts: list[Path]):
    if len(parts) != len(DSR_BARS):
        raise GridError("expected four DSR-bar shards")
    records = []
    dataset = None
    for path, bar in zip(parts, DSR_BARS, strict=True):
        rows = read_records(path)
        if dataset is None and rows:
            dataset = rows[0][0].get("dataset")
        try:
            records.extend(validate_grid(rows, dataset, bars=(bar,)))
        except GridError as exc:
            raise GridError(f"{path}: {exc}") from exc
    # Compare geometry BETWEEN valid shards too, before opening the output.
    ordered = validate_grid(records, dataset)
    with out.open("w", encoding="utf-8", newline="") as handle:
        handle.write("\n".join(line for _, line in ordered) + "\n")
    print(f"wrote {out}: {len(ordered)} records for {dataset}")


def main():
    if len(sys.argv) != 6:
        raise SystemExit(
            "usage: assemble_sweep.py OUT BAR-0.80 BAR-0.90 BAR-0.95 BAR-0.99"
        )
    try:
        assemble(Path(sys.argv[1]), [Path(p) for p in sys.argv[2:]])
    except (GridError, OSError) as exc:
        raise SystemExit(str(exc)) from exc


if __name__ == "__main__":
    main()
