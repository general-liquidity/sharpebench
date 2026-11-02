#!/usr/bin/env python3
"""Compare the current engine with the frozen evidence on cheap shards, field by field.

The frozen tables in `paper/evidence/final/` are bound to the engine that wrote
them, and nothing re-executes their producers, so a change to what the current
engine computes used to surface only when someone reran a producer by hand. The
2026-11-02 drift audit found one such change that nobody had noticed: the
commodities buy-and-hold track and all five luck floors had a non-finite mean
return since 4378ad4 met the WTI settlement d882803 readmitted.

This runs `evidence_sweep` and `external_rules_eval` on three one-dataset shards
(us-indices-1w, commodities-1d and fx-majors-1d; the sweep at one dsr_bar),
joins every output row to its frozen row by key and summarizes each field that
moved: how many rows, the largest absolute change, and how many values became
or stopped being null. Boolean verdicts are listed row by row. The summary must
equal the reviewed one committed in `docs/evidence-drift.json`, where every
entry names the change points that explain it:

    python paper/src/check-current-engine-shadow.py            # check (CI)
    python paper/src/check-current-engine-shadow.py --write    # refresh the manifest

A field change or verdict flip the manifest does not list fails the check, and
so does a listed change that no longer occurs or whose magnitude moved, so a
pull request that changes what the engine computes on these shards has to
change the manifest in the same diff. `--write` keeps every attribution it can
match and leaves new entries unattributed, which the check refuses until a
reviewer names their change point.

Outputs go to a scratch directory outside frozen evidence (refused otherwise).
Nothing here writes a frozen artifact or the provenance manifest.
"""

from __future__ import annotations

import argparse
import concurrent.futures
import importlib.util
import json
import math
import os
import subprocess
import sys
import tempfile
import time
from pathlib import Path

REPO = Path(__file__).resolve().parents[2]
MANIFEST = REPO / "docs" / "evidence-drift.json"
FROZEN = REPO / "paper" / "evidence" / "final"
SCHEMA = "sharpebench.evidence-drift.v1"

DATASETS = ("us-indices-1w", "commodities-1d", "fx-majors-1d")
SWEEP_DSR_BAR = 0.95
PRODUCERS = ("evidence_sweep", "external_rules_eval")
KEYS = {
    "evidence_sweep": ("dataset", "dsr_bar", "n_trials", "sr_std_pinned", "agent_id"),
    "external_rules_eval": ("dataset", "cost_profile", "agent_id"),
}
VERDICT_FIELDS = frozenset(
    {
        "passed_k",
        "rank_eligible",
        "eligible_never_catastrophic",
        "process_ok",
        "step_down_significant",
    }
)
# A numeric field moved when it differs by more than this, relative to the
# larger of 1 and the frozen magnitude. It sits above the last-bit differences
# that separate platforms and below every change the audit attributed.
MOVED_RELATIVE = 1e-12
# Two runs agree on a field's largest change when they match to this relative
# precision, which leaves room for a platform's last bits.
MAX_ABS_RELATIVE = 1e-6

_rerun_spec = importlib.util.spec_from_file_location(
    "current_engine_rerun", Path(__file__).with_name("make-current-engine-rerun.py")
)
rerun = importlib.util.module_from_spec(_rerun_spec)
_rerun_spec.loader.exec_module(rerun)


class ShadowRefusal(ValueError):
    """A shard cannot be compared: its output is missing, empty or misaligned."""


def shards():
    """Every (producer, dataset, extra arguments) the shadow job runs."""
    return [
        (producer, dataset, [f"{SWEEP_DSR_BAR}"] if producer == "evidence_sweep" else [])
        for producer in PRODUCERS
        for dataset in DATASETS
    ]


def read_jsonl(path: Path):
    with path.open(encoding="utf-8") as handle:
        return [json.loads(line) for line in handle if line.strip()]


def frozen_rows(producer: str, dataset: str):
    if producer == "evidence_sweep":
        rows = read_jsonl(FROZEN / f"{dataset}.jsonl")
        return [r for r in rows if r.get("dsr_bar") == SWEEP_DSR_BAR]
    rows = read_jsonl(FROZEN / "external-rules.jsonl")
    return [r for r in rows if r.get("dataset") == dataset]


def key_of(producer: str, row: dict) -> str:
    return "|".join(json.dumps(row.get(field)) for field in KEYS[producer])


def index_rows(producer: str, dataset: str, side: str, rows):
    """Key the rows, refusing an empty shard and a repeated key."""
    if not rows:
        raise ShadowRefusal(f"{producer} {dataset}: the {side} shard has zero records")
    indexed = {}
    for row in rows:
        key = key_of(producer, row)
        if key in indexed:
            raise ShadowRefusal(f"{producer} {dataset}: {side} repeats key {key}")
        indexed[key] = row
    return indexed


def is_number(value) -> bool:
    return isinstance(value, (int, float)) and not isinstance(value, bool)


def compare_shard(producer: str, dataset: str, frozen, current):
    """Field summaries, verdict flips and fields the current engine added."""
    old = index_rows(producer, dataset, "frozen", frozen)
    new = index_rows(producer, dataset, "current", current)
    if set(old) != set(new):
        missing = sorted(set(old) - set(new))
        extra = sorted(set(new) - set(old))
        raise ShadowRefusal(
            f"{producer} {dataset}: rows do not align with the frozen shard; "
            f"missing {missing[:3]} ({len(missing)}), extra {extra[:3]} ({len(extra)})"
        )
    fields: dict[str, dict] = {}
    flips = []
    added: set[str] = set()
    for key in sorted(old):
        before, after = old[key], new[key]
        added.update(set(after) - set(before))
        for field in sorted(before):
            a = before[field]
            b = after.get(field, "<absent>")
            entry = fields.setdefault(
                field,
                {"rows_moved": 0, "max_abs": None, "became_null": 0, "became_number": 0},
            )
            if field in VERDICT_FIELDS and isinstance(a, bool):
                if a != b:
                    entry["rows_moved"] += 1
                    flips.append({"key": key, "field": field, "frozen": a, "current": b})
                continue
            if is_number(a) and is_number(b):
                delta = abs(float(b) - float(a))
                if math.isfinite(delta) and delta > MOVED_RELATIVE * max(1.0, abs(float(a))):
                    entry["rows_moved"] += 1
                    entry["max_abs"] = max(entry["max_abs"] or 0.0, delta)
                elif not math.isfinite(delta):
                    entry["rows_moved"] += 1
            elif is_number(a) and b is None:
                entry["rows_moved"] += 1
                entry["became_null"] += 1
            elif a is None and is_number(b):
                entry["rows_moved"] += 1
                entry["became_number"] += 1
            elif a != b:
                entry["rows_moved"] += 1
    moved = {
        field: summary for field, summary in fields.items() if summary["rows_moved"] > 0
    }
    return moved, flips, sorted(added)


def summarize(outputs: dict):
    """The manifest's measured sections from {(producer, dataset): rows}."""
    observed, flips, added = [], [], {}
    for producer, dataset, _ in shards():
        moved, shard_flips, shard_added = compare_shard(
            producer, dataset, frozen_rows(producer, dataset), outputs[(producer, dataset)]
        )
        for field, summary in sorted(moved.items()):
            observed.append({"producer": producer, "dataset": dataset, "field": field, **summary})
        for flip in shard_flips:
            flips.append({"producer": producer, "dataset": dataset, **flip})
        added.setdefault(producer, set()).update(shard_added)
    return {
        "observed": observed,
        "verdict_flips": flips,
        "added_fields": {producer: sorted(fields) for producer, fields in sorted(added.items())},
    }


def observed_key(entry: dict):
    return (entry["producer"], entry["dataset"], entry["field"])


def flip_key(entry: dict):
    return (entry["producer"], entry["dataset"], entry["key"], entry["field"])


def same_magnitude(listed, measured) -> bool:
    if listed is None or measured is None:
        return listed is measured
    return abs(listed - measured) <= MAX_ABS_RELATIVE * max(abs(listed), abs(measured)) + 1e-15


def check_manifest(manifest: dict, measured: dict):
    """Every difference between the reviewed manifest and this measurement."""
    problems = []
    if manifest.get("schema") != SCHEMA:
        problems.append(f"manifest schema is {manifest.get('schema')!r}, expected {SCHEMA}")
    points = {point.get("id") for point in manifest.get("change_points", [])}
    listed = {observed_key(e): e for e in manifest.get("observed", [])}
    actual = {observed_key(e): e for e in measured["observed"]}
    for key in sorted(set(actual) - set(listed)):
        problems.append(f"unlisted field change {key}: {actual[key]}")
    for key in sorted(set(listed) - set(actual)):
        problems.append(f"listed field change no longer observed {key}")
    for key in sorted(set(listed) & set(actual)):
        want, got = listed[key], actual[key]
        for count in ("rows_moved", "became_null", "became_number"):
            if want.get(count) != got[count]:
                problems.append(f"{key}: {count} listed {want.get(count)}, measured {got[count]}")
        if not same_magnitude(want.get("max_abs"), got["max_abs"]):
            problems.append(f"{key}: max_abs listed {want.get('max_abs')}, measured {got['max_abs']}")
    listed_flips = {flip_key(e): e for e in manifest.get("verdict_flips", [])}
    actual_flips = {flip_key(e): e for e in measured["verdict_flips"]}
    for key in sorted(set(actual_flips) - set(listed_flips)):
        problems.append(f"unlisted verdict flip {key}: {actual_flips[key]}")
    for key in sorted(set(listed_flips) - set(actual_flips)):
        problems.append(f"listed verdict flip no longer observed {key}")
    for key in sorted(set(listed_flips) & set(actual_flips)):
        want, got = listed_flips[key], actual_flips[key]
        if (want.get("frozen"), want.get("current")) != (got["frozen"], got["current"]):
            problems.append(f"verdict flip {key}: listed {want}, measured {got}")
    for entry in [*manifest.get("observed", []), *manifest.get("verdict_flips", [])]:
        named = entry.get("attributed_to") or []
        if not named:
            problems.append(f"no change point attributed for {entry}")
        for point in named:
            if point not in points:
                problems.append(f"unknown change point {point!r} for {entry}")
    if manifest.get("added_fields") != measured["added_fields"]:
        problems.append(
            f"added fields listed {manifest.get('added_fields')}, measured {measured['added_fields']}"
        )
    return problems


def merged_manifest(manifest: dict, measured: dict) -> dict:
    """`manifest` with its measured sections replaced, keeping attributions."""
    listed = {observed_key(e): e for e in manifest.get("observed", [])}
    listed_flips = {flip_key(e): e for e in manifest.get("verdict_flips", [])}
    out = dict(manifest)
    out["observed"] = [
        {**e, "attributed_to": listed.get(observed_key(e), {}).get("attributed_to", [])}
        for e in measured["observed"]
    ]
    out["verdict_flips"] = [
        {**e, "attributed_to": listed_flips.get(flip_key(e), {}).get("attributed_to", [])}
        for e in measured["verdict_flips"]
    ]
    out["added_fields"] = measured["added_fields"]
    return out


def build(target_dir: Path) -> Path:
    command = ["cargo", "build", "--release", "--locked", "-p", "sharpebench-harness"]
    for producer in PRODUCERS:
        command += ["--example", producer]
    subprocess.run(command, cwd=REPO, check=True)
    return target_dir / "release" / "examples"


def run_shard(binary_dir: Path, work: Path, producer: str, dataset: str, extra):
    """Run one shard; a producer that exits nonzero or publishes nothing refuses."""
    suffix = ".exe" if os.name == "nt" else ""
    out = work / producer / f"{dataset}.jsonl"
    out.parent.mkdir(parents=True, exist_ok=True)
    completed = subprocess.run(
        [str(binary_dir / f"{producer}{suffix}"), str(out), dataset, *extra],
        cwd=REPO,
        capture_output=True,
        text=True,
    )
    if completed.returncode != 0 or not out.is_file():
        raise ShadowRefusal(
            f"{producer} {dataset} exited {completed.returncode} without publishing: "
            f"{completed.stderr.strip()[-500:]}"
        )
    return read_jsonl(out)


def run_shards(binary_dir: Path, work: Path, jobs: int) -> dict:
    """Run every shard, `jobs` at a time. Each is deterministic on its own."""
    with concurrent.futures.ThreadPoolExecutor(max_workers=jobs) as pool:
        futures = {
            (producer, dataset): pool.submit(run_shard, binary_dir, work, producer, dataset, extra)
            for producer, dataset, extra in shards()
        }
        return {key: future.result() for key, future in futures.items()}


def write_manifest(document: dict) -> None:
    payload = json.dumps(document, indent=2, sort_keys=True) + "\n"
    MANIFEST.write_bytes(payload.encode("utf-8"))


def main(argv=None) -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--write", action="store_true", help="refresh the manifest's measurement")
    parser.add_argument("--bin-dir", type=Path, help="prebuilt producer binaries")
    parser.add_argument("--work", type=Path, help="scratch directory for shard outputs")
    parser.add_argument("--jobs", type=int, default=min(len(shards()), os.cpu_count() or 1))
    args = parser.parse_args(argv)
    started = time.monotonic()
    try:
        with tempfile.TemporaryDirectory(prefix="sb-shadow-") as scratch:
            work = rerun.check_output(args.work or Path(scratch))
            target = Path(os.environ.get("CARGO_TARGET_DIR", REPO / "target"))
            binary_dir = args.bin_dir or build(target)
            built = time.monotonic()
            measured = summarize(run_shards(binary_dir, work, max(1, args.jobs)))
    except (ShadowRefusal, rerun.RerunRefusal) as refusal:
        print(f"refused: {refusal}", file=sys.stderr)
        return 1
    ran = time.monotonic()
    manifest = json.loads(MANIFEST.read_text(encoding="utf-8"))
    if args.write:
        write_manifest(merged_manifest(manifest, measured))
        manifest = json.loads(MANIFEST.read_text(encoding="utf-8"))
    problems = check_manifest(manifest, measured)
    print(
        f"shadow: {len(measured['observed'])} field changes, "
        f"{len(measured['verdict_flips'])} verdict flips over {len(shards())} shards; "
        f"build {built - started:.0f}s, producers {ran - built:.0f}s"
    )
    if problems:
        print("\n".join(problems))
        print(f"\nFAIL: {len(problems)} drift problem(s); see docs/evidence-drift.json")
        return 1
    print("OK: every change against the frozen shards is listed and attributed")
    return 0


if __name__ == "__main__":
    sys.exit(main())
