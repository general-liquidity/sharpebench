#!/usr/bin/env python3
"""Figures computed from the committed evidence records.

Reads paper/evidence/final/*.jsonl and writes vector PDFs into paper/figures/.
No number is typed in: every bar and point is a reduction over the records the
sweep and the risk-managed evaluation wrote. Run from the paper/ directory:

    python src/make-evidence-figures.py
"""
import json
import os

import matplotlib

matplotlib.use("pdf")
matplotlib.rcParams["pdf.fonttype"] = 42
import matplotlib.pyplot as plt

HERE = os.path.dirname(os.path.abspath(__file__))
EV = os.path.join(HERE, "..", "evidence", "final")
OUT = os.path.join(HERE, "..", "figures")

INK = "#0b1220"
GREEN = "#098551"
RED = "#dc2626"
BLUE = "#1d4ed8"
GRAY = "#6b7280"

DATASETS = [
    ("us-indices-1w", "US eq 1w"),
    ("us-indices-1d", "US eq 1d"),
    ("crypto-majors-1w", "crypto 1w"),
    ("crypto-majors-1d", "crypto 1d"),
    ("crypto-majors-4h", "crypto 4h"),
    ("crypto-majors-1h", "crypto 1h"),
    ("fx-majors-1d", "FX 1d"),
    ("commodities-1d", "cmdty 1d"),
    ("rates-1d", "rates 1d"),
]


def load(name):
    path = os.path.join(EV, f"{name}.jsonl")
    out = []
    with open(path, encoding="utf-8") as h:
        for line in h:
            line = line.strip()
            if line.endswith("}"):
                out.append(json.loads(line))
    return out


def default_cell(recs, dataset=None):
    """The sweep's default configuration. The risk-managed evaluation runs only
    that configuration and omits the grid fields, so records without them are
    already the default cell."""
    return [
        r for r in recs
        if r.get("dsr_bar", 0.95) == 0.95 and r.get("n_trials", 50) == 50
        and r.get("sr_std_pinned") is None
        and (dataset is None or r["dataset"] == dataset)
    ]


def style(ax):
    for sp in ("top", "right"):
        ax.spines[sp].set_visible(False)
    ax.spines["left"].set_color("#cbd5e1")
    ax.spines["bottom"].set_color("#cbd5e1")
    ax.set_axisbelow(True)
    ax.grid(axis="y", color="#e8edf3", linewidth=0.9)
    ax.tick_params(colors=INK, length=0)


# ---- Figure A: worst-run drawdown per dataset, three agents --------------------
rm = load("risk-managed")
fig, ax = plt.subplots(figsize=(7.8, 3.9))
labels, bh, mo, rmv = [], [], [], []
for ds, short in DATASETS:
    sweep = default_cell(load(ds))
    cell_rm = [r for r in default_cell(rm, ds) if r["agent_id"] == "risk-managed"]
    labels.append(short)
    bh.append(next(r["worst_run_drawdown"] for r in sweep if r["agent_id"] == "buy-and-hold"))
    mo.append(next(r["worst_run_drawdown"] for r in sweep if r["agent_id"] == "momentum"))
    rmv.append(cell_rm[0]["worst_run_drawdown"] if cell_rm else float("nan"))
x = range(len(labels))
w = 0.27
ax.bar([i - w for i in x], bh, w, color=GRAY, label="buy-and-hold", zorder=3)
ax.bar(list(x), mo, w, color=RED, label="momentum", zorder=3)
ax.bar([i + w for i in x], rmv, w, color=GREEN, label="risk-managed", zorder=3)
ax.axhline(0.20, color=INK, linewidth=1.1, linestyle=(0, (5, 4)))
ax.text(len(labels) - 0.5, 0.215, "never-catastrophic bound (0.20)", ha="right",
        va="bottom", fontsize=9.5, color=INK)
ax.set_xticks(list(x))
ax.set_xticklabels(labels, fontsize=9.5, color=INK)
ax.set_ylabel("worst single-window drawdown", fontsize=11, color=INK)
ax.set_ylim(0, 1.05)
ax.legend(frameon=False, fontsize=9.5, ncol=3, loc="upper left")
style(ax)
fig.tight_layout()
fig.savefig(os.path.join(OUT, "evidence-drawdowns.pdf"), bbox_inches="tight")
plt.close(fig)

# ---- Figure B: best luck-floor DSR vs N on real data ---------------------------
fig, ax = plt.subplots(figsize=(7.8, 3.9))
for ds, short, color in [
    ("crypto-majors-1w", "crypto 1w", RED),
    ("rates-1d", "rates 1d", BLUE),
    ("us-indices-1w", "US eq 1w", GRAY),
]:
    recs = [r for r in load(ds) if r["sr_std_pinned"] is None and r["dsr_bar"] == 0.95]
    ns = sorted({r["n_trials"] for r in recs})
    ys = [max(r["deflated_sharpe"] for r in recs
              if r["n_trials"] == n and r["agent_id"].startswith("luck")) for n in ns]
    ax.plot(ns, ys, color=color, linewidth=2.2, marker="o", markersize=4.5,
            label=f"best random agent, {short}", zorder=3)
ax.axhline(0.95, color=INK, linewidth=1.1, linestyle=(0, (5, 4)))
ax.text(200, 0.905, "eligibility bar", ha="right", va="top", fontsize=9.5, color=INK)
ax.set_xscale("log")
ax.set_xticks([1, 10, 50, 200])
ax.set_xticklabels(["1", "10", "50", "200"], fontsize=10)
ax.set_xlabel("trials deflated for (N)", fontsize=11, color=INK)
ax.set_ylabel("deflated Sharpe of the best zero-skill agent", fontsize=10.5, color=INK)
ax.set_ylim(-0.03, 1.05)
ax.legend(frameon=False, fontsize=9.5, loc="upper right")
style(ax)
fig.tight_layout()
fig.savefig(os.path.join(OUT, "evidence-luck-deflation.pdf"), bbox_inches="tight")
plt.close(fig)

print("wrote evidence-drawdowns.pdf and evidence-luck-deflation.pdf")
