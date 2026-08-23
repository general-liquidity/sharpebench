#!/usr/bin/env python3
"""Turn the evidence sweep records into the tables in the paper.

Reads every JSONL file under paper/evidence/final/ and prints, for each
dataset, the default-configuration row of the eligibility table plus the
grid-wide eligibility and luck-floor checks. Every number in Section 5 of
the paper comes from this script and nowhere else. No plotting, no model,
no randomness: it is a reduction over committed records.

Usage:  python paper/evidence/analyze.py [paper/evidence/final]
"""
import glob
import json
import os
import sys

ROOT = sys.argv[1] if len(sys.argv) > 1 else os.path.join(os.path.dirname(__file__), "final")

ORDER = [
    "us-indices-1d", "us-indices-1w",
    "crypto-majors-1h", "crypto-majors-4h", "crypto-majors-1d", "crypto-majors-1w",
    "fx-majors-1d", "commodities-1d", "rates-1d",
]
DEFAULT = dict(dsr_bar=0.95, n_trials=50)


def load(dataset):
    path = os.path.join(ROOT, f"{dataset}.jsonl")
    if not os.path.exists(path):
        return []
    out = []
    with open(path, encoding="utf-8") as h:
        for line in h:
            line = line.strip()
            if line.endswith("}"):
                out.append(json.loads(line))
    return out


def is_default(r):
    return (r["dsr_bar"] == DEFAULT["dsr_bar"] and r["n_trials"] == DEFAULT["n_trials"]
            and r["sr_std_pinned"] is None)


def main():
    total = 0
    print("dataset            agent           ppy    DSR    pass^k  worstDD  every  never   boot_p   sr_std(src)")
    luck_violations = 0
    for d in ORDER:
        recs = load(d)
        if not recs:
            print(f"{d:18s} (no records)")
            continue
        total += len(recs)
        complete = "complete" if len(recs) == 512 else f"PARTIAL {len(recs)}/512"
        cell = [r for r in recs if is_default(r)]
        refs = [r for r in cell if not r["agent_id"].startswith("luck")]
        for r in sorted(refs, key=lambda x: -x["deflated_sharpe"]):
            print(f"{d:18s} {r['agent_id']:14s} {r['periods_per_year']:5.0f}  {r['deflated_sharpe']:.3f}  "
                  f"{str(r['passed_k']):5s}   {r['worst_run_drawdown']:.3f}   {str(r['rank_eligible']):5s}  "
                  f"{str(r['eligible_never_catastrophic']):5s}  {r['bootstrap_p']:.4f}   "
                  f"{r['trials_sr_std_used']:.3f}({r['trials_sr_std_source'][:4]})")
        best_ref = max(r["raw_mean_return"] for r in cell if r["agent_id"] in ("buy-and-hold", "momentum"))
        best_luck = max(r["raw_mean_return"] for r in cell if r["agent_id"].startswith("luck"))
        best_luck_dsr = max(r["deflated_sharpe"] for r in recs if r["agent_id"].startswith("luck"))
        flag = ""
        if best_luck > best_ref:
            luck_violations += 1
            flag = "  LUCK BEATS REFERENCE"
        grid_every = sorted({r["agent_id"] for r in recs if r["rank_eligible"]})
        grid_never = sorted({r["agent_id"] for r in recs if r["eligible_never_catastrophic"]})
        print(f"{'':18s} [{complete}] grid eligible: every={grid_every or 'none'} never={grid_never or 'none'}; "
              f"best luck raw={best_luck:+.5f} vs ref={best_ref:+.5f}{flag}; max luck DSR anywhere={best_luck_dsr:.3f}")
    print(f"\nrecords: {total}   luck-floor violations: {luck_violations}")


if __name__ == "__main__":
    main()
