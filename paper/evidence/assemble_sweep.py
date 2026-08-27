"""Assemble DSR-bar shards from ``evidence_sweep`` in canonical grid order."""

from __future__ import annotations

import json
import sys
from pathlib import Path


if len(sys.argv) != 6:
    raise SystemExit("usage: assemble_sweep.py OUT BAR-0.80 BAR-0.90 BAR-0.95 BAR-0.99")

out = Path(sys.argv[1])
parts = [Path(path) for path in sys.argv[2:]]
expected_bars = [0.80, 0.90, 0.95, 0.99]
lines: list[str] = []
dataset: str | None = None
for path, expected_bar in zip(parts, expected_bars, strict=True):
    part_lines = path.read_text(encoding="utf-8").splitlines()
    if len(part_lines) != 128:
        raise SystemExit(f"{path}: expected 128 records, found {len(part_lines)}")
    records = [json.loads(line) for line in part_lines]
    if any(record["dsr_bar"] != expected_bar for record in records):
        raise SystemExit(f"{path}: contains a record outside DSR bar {expected_bar}")
    part_datasets = {record["dataset"] for record in records}
    if len(part_datasets) != 1:
        raise SystemExit(f"{path}: expected one dataset, found {sorted(part_datasets)}")
    part_dataset = part_datasets.pop()
    if dataset is None:
        dataset = part_dataset
    elif dataset != part_dataset:
        raise SystemExit(f"dataset mismatch: {dataset} versus {part_dataset}")
    lines.extend(part_lines)

# Result artifacts are hashed byte-exact, so the writer must not translate newlines.
with out.open("w", encoding="utf-8", newline="") as handle:
    handle.write("\n".join(lines) + "\n")
print(f"wrote {out}: {len(lines)} records for {dataset}")
