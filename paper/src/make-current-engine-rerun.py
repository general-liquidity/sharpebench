#!/usr/bin/env python3
"""Rerun the frozen evidence producers on the current engine, beside the frozen files.

The frozen tables in `paper/evidence/final/` are bound to the engine that wrote
them. This produces the current engine's numbers in a directory of their own,
for comparison, and cannot overwrite the originals: an output inside frozen
evidence, or a directory that already holds a run, is refused before anything
runs. Nothing here regenerates a published number.

    python paper/src/make-current-engine-rerun.py --out DIR [--jobs N] [--dataset NAME ...]

`evidence_sweep` and `external_rules_eval` run one process per dataset. Each
stages through `<out>.partial` and refuses a short grid, so a file present in
the output is whole, and a rerun of the same command resumes by skipping files
already present. The records now carry each entrant's pooled Sharpe and clone
cluster, so a later dispersion estimator can be evaluated without re-simulating
the field. `manifest.json` binds the commit, the exact commands and a digest of
every output; it holds no clock, so two runs of one commit compare byte for
byte.
"""

from __future__ import annotations

import argparse
import concurrent.futures
import hashlib
import json
import os
import subprocess
import sys
from pathlib import Path

REPO = Path(__file__).resolve().parents[2]
DATASETS = (
    "us-indices-1d",
    "us-indices-1w",
    "crypto-majors-1h",
    "crypto-majors-4h",
    "crypto-majors-1d",
    "crypto-majors-1w",
    "fx-majors-1d",
    "commodities-1d",
    "rates-1d",
)
PRODUCERS = ("evidence_sweep", "external_rules_eval")
FROZEN_ROOTS = (
    "paper/evidence/final",
    "paper/evidence/after-v0.3.0",
    "paper/evidence/baseline-v0.2.1",
    "paper/evidence/development",
    "paper/evidence/prospective-forecast-field",
)


class RerunRefusal(ValueError):
    """The requested rerun would not be a clean sibling of the frozen evidence."""


def check_output(out: Path, repo: Path = REPO) -> Path:
    """Resolve `out`, or refuse it. Symlinks are resolved, so a link into frozen
    evidence is the frozen directory."""
    resolved = out.resolve()
    for root in FROZEN_ROOTS:
        frozen = (repo / root).resolve()
        if resolved == frozen or frozen in resolved.parents:
            raise RerunRefusal(f"{out} is inside frozen evidence {root}")
    return resolved


def select_datasets(requested):
    unknown = sorted(set(requested) - set(DATASETS))
    if unknown:
        raise RerunRefusal(f"unknown datasets {unknown}; declared {list(DATASETS)}")
    return [d for d in DATASETS if not requested or d in requested]


def plan(out: Path, datasets):
    return [
        (producer, dataset, out / producer / f"{dataset}.jsonl")
        for producer in PRODUCERS
        for dataset in datasets
    ]


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for block in iter(lambda: handle.read(1 << 20), b""):
            digest.update(block)
    return digest.hexdigest()


def command_for(producer: str, dataset: str, output: Path, binary_dir: Path):
    return [str(binary_dir / producer), str(output), dataset]


def manifest(commit: str, dirty: bool, jobs, repo: Path = REPO):
    return {
        "tier": "current-engine rerun, not frozen evidence",
        "commit": commit,
        "dirty": dirty,
        "producers": list(PRODUCERS),
        "outputs": {
            str(path.relative_to(jobs[0][2].parents[1])).replace(os.sep, "/"): sha256(path)
            for _, _, path in jobs
        },
    }


def git(*args):
    return subprocess.run(
        ["git", "-C", str(REPO), *args], check=True, capture_output=True, text=True
    ).stdout.strip()


def build(target_dir: Path) -> Path:
    for producer in PRODUCERS:
        subprocess.run(
            ["cargo", "build", "--release", "-p", "sharpebench-harness", "--example", producer],
            cwd=REPO,
            check=True,
        )
    return target_dir / "release" / "examples"


def run_one(job, binary_dir: Path):
    producer, dataset, output = job
    if output.exists():
        return
    output.parent.mkdir(parents=True, exist_ok=True)
    subprocess.run(command_for(producer, dataset, output, binary_dir), cwd=REPO, check=True)


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--out", required=True, type=Path)
    parser.add_argument("--jobs", type=int, default=os.cpu_count() or 1)
    parser.add_argument("--dataset", action="append", default=[])
    args = parser.parse_args(argv)
    try:
        out = check_output(args.out)
        datasets = select_datasets(args.dataset)
    except RerunRefusal as refusal:
        print(f"refused: {refusal}", file=sys.stderr)
        return 1
    target = Path(os.environ.get("CARGO_TARGET_DIR", REPO / "target"))
    binary_dir = build(target)
    jobs = plan(out, datasets)
    with concurrent.futures.ThreadPoolExecutor(max_workers=args.jobs) as pool:
        for future in [pool.submit(run_one, job, binary_dir) for job in jobs]:
            future.result()
    document = manifest(git("rev-parse", "HEAD"), bool(git("status", "--porcelain")), jobs)
    with (out / "manifest.json").open("w", encoding="utf-8", newline="\n") as handle:
        json.dump(document, handle, indent=2, sort_keys=True)
        handle.write("\n")
    return 0


if __name__ == "__main__":
    sys.exit(main())
