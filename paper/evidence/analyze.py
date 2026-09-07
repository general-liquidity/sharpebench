#!/usr/bin/env python3
"""Turn the evidence sweep records into the tables in the paper.

Reads every JSONL file under paper/evidence/final/ and prints, for each
dataset, default-configuration reference rows plus grid-wide eligibility and
luck-floor checks. It displays both the dispersion source and the annualized
dispersion/deflation bar serialized by the scorer, so dimensional provenance
is visible rather than inferred from a configuration. No plotting, no model,
no randomness: it is a reduction over committed records.

Usage:  python paper/evidence/analyze.py [paper/evidence/final]
"""

import os
import sys
from pathlib import Path

from sweep_grid import DATASETS, GridError, read_records, validate_grid

ROOT = (
    sys.argv[1]
    if len(sys.argv) > 1
    else os.path.join(os.path.dirname(__file__), "final")
)

ORDER = list(DATASETS)
DEFAULT = dict(dsr_bar=0.95, n_trials=50)


def load(dataset):
    path = os.path.join(ROOT, f"{dataset}.jsonl")
    if not os.path.exists(path):
        raise GridError(f"missing dataset: {path}")
    return [row for row, _ in validate_grid(read_records(Path(path)), dataset)]


def is_default(r):
    return (
        r["dsr_bar"] == DEFAULT["dsr_bar"]
        and r["n_trials"] == DEFAULT["n_trials"]
        and r["sr_std_pinned"] is None
    )


def main():
    # Check every required dataset before printing anything that looks complete.
    datasets = {d: load(d) for d in ORDER}
    total = 0
    print(
        "dataset            agent           ppy    DSR    pass^k  worstDD  every  never   boot_p   sigma_ann(src)  bar_ann  pooled_n"
    )
    luck_violations = 0
    for d in ORDER:
        recs = datasets[d]
        total += len(recs)
        complete = "complete declared grid"
        cell = [r for r in recs if is_default(r)]
        refs = [r for r in cell if not r["agent_id"].startswith("luck")]
        for r in sorted(refs, key=lambda x: -x["deflated_sharpe"]):
            print(
                f"{d:18s} {r['agent_id']:14s} {r['periods_per_year']:5.0f}  {r['deflated_sharpe']:.3f}  "
                f"{str(r['passed_k']):5s}   {r['worst_run_drawdown']:.3f}   {str(r['rank_eligible']):5s}  "
                f"{str(r['eligible_never_catastrophic']):5s}  {r['bootstrap_p']:.4f}   "
                f"{r['trials_sr_std_annualized_equivalent']:.3f}({r['trials_sr_std_source']})  "
                f"{r['deflation_bar_annualized_equivalent']:.3f}  {r['pooled_observations']:8d}"
            )
        best_ref = max(
            r["raw_mean_return"]
            for r in cell
            if r["agent_id"] in ("buy-and-hold", "momentum")
        )
        best_luck = max(
            r["raw_mean_return"] for r in cell if r["agent_id"].startswith("luck")
        )
        best_luck_dsr = max(
            r["deflated_sharpe"] for r in recs if r["agent_id"].startswith("luck")
        )
        flag = ""
        if best_luck > best_ref:
            luck_violations += 1
            flag = "  LUCK BEATS REFERENCE"
        grid_every = sorted({r["agent_id"] for r in recs if r["rank_eligible"]})
        grid_never = sorted(
            {r["agent_id"] for r in recs if r["eligible_never_catastrophic"]}
        )
        print(
            f"{'':18s} [{complete}] grid eligible: every={grid_every or 'none'} never={grid_never or 'none'}; "
            f"best luck raw={best_luck:+.5f} vs ref={best_ref:+.5f}{flag}; max luck DSR anywhere={best_luck_dsr:.3f}"
        )
    print(f"\nrecords: {total}   luck-floor violations: {luck_violations}")


if __name__ == "__main__":
    try:
        main()
    except (GridError, OSError) as exc:
        raise SystemExit(str(exc)) from exc
